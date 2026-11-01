//! The IPC seam: what an Electron `ipcMain` handler receives and returns, after the renderer's
//! value has gone through contextBridge and structured clone (see `parity/electron/transport`
//! for the ground truth this is tested against).
//!
//! Wire format (JSON, produced/consumed by `shim.js`): `null`, booleans, finite non-negative-
//! zero numbers and well-formed strings are plain JSON; everything else is tagged:
//! `{"$":"u"}` undefined, `{"$":"n","v":"NaN"|"Infinity"|"-Infinity"|"-0"}`,
//! `{"$":"b","v":"123"}` BigInt, `{"$":"s","v":[utf16 units]}` string with lone surrogates,
//! `[..]` array, `{"$":"o","v":[[key, value]..]}` object, `{"$":"d","v":ms|"NaN"}` Date,
//! `{"$":"re","s":source,"f":flags}`, `{"$":"m","v":[[k,v]..]}` Map, `{"$":"set","v":[..]}`,
//! `{"$":"ab","v":[bytes]}` ArrayBuffer, `{"$":"view","k":ctor,"b":[buffer bytes],"o":offset,"l":byteLength}`,
//! `{"$":"err","m":message,"st":stack|null}`, `{"$":"box","v":primitive}` boxed primitive,
//! `{"$":"id","i":n,"v":value}` first occurrence of a shared object, `{"$":"r","i":n}` reference.

use crate::js;
use serde_json::{Value, json};

/// A JS string: UTF-16 code units, so lone surrogates survive the round trip.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct JsString(pub Vec<u16>);

impl JsString {
    /// Lossy, as Node does when a string reaches the OS (lone surrogates become U+FFFD).
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }
}

impl From<&str> for JsString {
    fn from(s: &str) -> Self {
        Self(s.encode_utf16().collect())
    }
}

impl From<String> for JsString {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum JsValue {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    BigInt(String),
    String(JsString),
    /// Holes arrive as `Undefined` (contextBridge fills them).
    Array(Vec<JsValue>),
    /// Own enumerable string-keyed properties, in JS property order.
    Object(Vec<(JsString, JsValue)>),
    Date(f64),
    RegExp {
        source: JsString,
        flags: String,
    },
    Map(Vec<(JsValue, JsValue)>),
    Set(Vec<JsValue>),
    ArrayBuffer(Vec<u8>),
    /// A typed array or DataView: constructor name, its whole underlying buffer, and the
    /// viewed byte range.
    View {
        kind: String,
        buffer: Vec<u8>,
        offset: usize,
        length: usize,
    },
    /// Structured clone keeps only the message (and stack); the name becomes "Error".
    Error {
        message: JsString,
        stack: Option<JsString>,
    },
    /// `new Number(5)`, `new String('x')`, `new Boolean(false)`.
    Boxed(Box<JsValue>),
    /// First occurrence of an object referenced again later (shared or cyclic).
    Shared(u32, Box<JsValue>),
    Ref(u32),
}

impl From<&str> for JsValue {
    fn from(s: &str) -> Self {
        Self::String(s.into())
    }
}

impl From<String> for JsValue {
    fn from(s: String) -> Self {
        Self::String(s.into())
    }
}

impl From<bool> for JsValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<f64> for JsValue {
    fn from(n: f64) -> Self {
        Self::Number(n)
    }
}

impl<T: Into<JsValue>> From<Option<T>> for JsValue {
    fn from(v: Option<T>) -> Self {
        v.map_or(Self::Null, Into::into)
    }
}

/// Plain JSON as the JS value `JSON.parse` would produce.
impl From<&Value> for JsValue {
    fn from(v: &Value) -> Self {
        match v {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => Self::Number(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => Self::String(s.as_str().into()),
            Value::Array(items) => Self::Array(items.iter().map(Self::from).collect()),
            Value::Object(o) => Self::Object(
                o.iter()
                    .map(|(k, v)| (k.as_str().into(), Self::from(v)))
                    .collect(),
            ),
        }
    }
}

impl JsValue {
    /// `obj[key]` for a plain object (`Undefined` if absent or not an object).
    pub fn get(&self, key: &str) -> &JsValue {
        let key = JsString::from(key);
        match self.deref_shared() {
            Self::Object(props) => props
                .iter()
                .rev()
                .find(|(k, _)| *k == key)
                .map_or(&Self::Undefined, |(_, v)| v),
            _ => &Self::Undefined,
        }
    }

    pub fn deref_shared(&self) -> &JsValue {
        match self {
            Self::Shared(_, v) => v.deref_shared(),
            v => v,
        }
    }

    /// The string, if this is a string primitive (lossy for lone surrogates).
    pub fn as_str(&self) -> Option<String> {
        match self.deref_shared() {
            Self::String(s) => Some(s.to_string_lossy()),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self.deref_shared() {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// JS truthiness.
    pub fn truthy(&self) -> bool {
        match self.deref_shared() {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::BigInt(b) => b.trim_start_matches('-') != "0",
            Self::String(s) => !s.0.is_empty(),
            _ => true,
        }
    }

    /// Builds a plain object from string keys.
    pub fn object<K: Into<JsString>>(props: impl IntoIterator<Item = (K, JsValue)>) -> Self {
        Self::Object(props.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// Decodes the wire format. Malformed input is a bug in the shim, reported as `Err`.
    pub fn decode(v: &Value) -> Result<Self, String> {
        let bad = || format!("bad IPC value: {v}");
        Ok(match v {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => Self::Number(n.as_f64().ok_or_else(bad)?),
            Value::String(s) => Self::String(s.as_str().into()),
            Value::Array(items) => {
                Self::Array(items.iter().map(Self::decode).collect::<Result<_, _>>()?)
            }
            Value::Object(o) => {
                let s = |k: &str| o.get(k).and_then(Value::as_str).ok_or_else(bad);
                let list = |k: &str| o.get(k).and_then(Value::as_array).ok_or_else(bad);
                let num = |k: &str| o.get(k).and_then(Value::as_u64).ok_or_else(bad);
                let bytes = |k: &str| -> Result<Vec<u8>, String> {
                    list(k)?
                        .iter()
                        .map(|b| b.as_u64().map(|b| b as u8).ok_or_else(bad))
                        .collect()
                };
                let pairs = |k: &str| -> Result<Vec<(JsValue, JsValue)>, String> {
                    list(k)?
                        .iter()
                        .map(|p| Ok((Self::decode(&p[0])?, Self::decode(&p[1])?)))
                        .collect()
                };
                let units = |v: &Value| -> Result<JsString, String> {
                    match v {
                        Value::String(s) => Ok(s.as_str().into()),
                        Value::Object(_) => match Self::decode(v)? {
                            Self::String(s) => Ok(s),
                            _ => Err(bad()),
                        },
                        _ => Err(bad()),
                    }
                };
                match s("$")? {
                    "u" => Self::Undefined,
                    "n" => Self::Number(match s("v")? {
                        "NaN" => f64::NAN,
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        _ => -0.0,
                    }),
                    "b" => Self::BigInt(s("v")?.to_owned()),
                    "s" => Self::String(JsString(
                        list("v")?
                            .iter()
                            .map(|u| u.as_u64().map(|u| u as u16).ok_or_else(bad))
                            .collect::<Result<_, _>>()?,
                    )),
                    "o" => Self::Object(
                        list("v")?
                            .iter()
                            .map(|p| Ok((units(&p[0])?, Self::decode(&p[1])?)))
                            .collect::<Result<_, String>>()?,
                    ),
                    "d" => Self::Date(o.get("v").and_then(Value::as_f64).unwrap_or(f64::NAN)),
                    "re" => Self::RegExp {
                        source: units(&o["s"])?,
                        flags: s("f")?.to_owned(),
                    },
                    "m" => Self::Map(pairs("v")?),
                    "set" => Self::Set(
                        list("v")?
                            .iter()
                            .map(Self::decode)
                            .collect::<Result<_, _>>()?,
                    ),
                    "ab" => Self::ArrayBuffer(bytes("v")?),
                    "view" => Self::View {
                        kind: s("k")?.to_owned(),
                        buffer: bytes("b")?,
                        offset: num("o")? as usize,
                        length: num("l")? as usize,
                    },
                    "err" => Self::Error {
                        message: units(&o["m"])?,
                        stack: match o.get("st") {
                            None | Some(Value::Null) => None,
                            Some(st) => Some(units(st)?),
                        },
                    },
                    "box" => Self::Boxed(Box::new(Self::decode(&o["v"])?)),
                    "id" => Self::Shared(num("i")? as u32, Box::new(Self::decode(&o["v"])?)),
                    "r" => Self::Ref(num("i")? as u32),
                    _ => return Err(bad()),
                }
            }
        })
    }

    pub fn encode(&self) -> Value {
        let str_units = |s: &JsString| match String::from_utf16(&s.0) {
            Ok(s) => Value::String(s),
            Err(_) => json!({ "$": "s", "v": s.0 }),
        };
        match self {
            Self::Undefined => json!({ "$": "u" }),
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(*b),
            Self::Number(n) if n.is_nan() => json!({ "$": "n", "v": "NaN" }),
            Self::Number(n) if n.is_infinite() => {
                json!({ "$": "n", "v": if *n > 0.0 { "Infinity" } else { "-Infinity" } })
            }
            Self::Number(n) if *n == 0.0 && n.is_sign_negative() => json!({ "$": "n", "v": "-0" }),
            // Integral values as JSON integers, as JS writes them.
            Self::Number(n) if n.fract() == 0.0 && n.abs() < 1e21 => {
                serde_json::from_str(&js::number_to_string(*n)).unwrap_or_else(|_| json!(n))
            }
            Self::Number(n) => json!(n),
            Self::BigInt(b) => json!({ "$": "b", "v": b }),
            Self::String(s) => str_units(s),
            Self::Array(items) => Value::Array(items.iter().map(Self::encode).collect()),
            Self::Object(props) => {
                json!({ "$": "o", "v": props.iter().map(|(k, v)| json!([str_units(k), v.encode()])).collect::<Vec<_>>() })
            }
            Self::Date(t) => {
                json!({ "$": "d", "v": if t.is_nan() { json!("NaN") } else { json!(t) } })
            }
            Self::RegExp { source, flags } => {
                json!({ "$": "re", "s": str_units(source), "f": flags })
            }
            Self::Map(pairs) => {
                json!({ "$": "m", "v": pairs.iter().map(|(k, v)| json!([k.encode(), v.encode()])).collect::<Vec<_>>() })
            }
            Self::Set(items) => {
                json!({ "$": "set", "v": items.iter().map(Self::encode).collect::<Vec<_>>() })
            }
            Self::ArrayBuffer(b) => json!({ "$": "ab", "v": b }),
            Self::View {
                kind,
                buffer,
                offset,
                length,
            } => json!({ "$": "view", "k": kind, "b": buffer, "o": offset, "l": length }),
            Self::Error { message, stack } => {
                json!({ "$": "err", "m": str_units(message), "st": stack.as_ref().map(str_units) })
            }
            Self::Boxed(v) => json!({ "$": "box", "v": v.encode() }),
            Self::Shared(i, v) => json!({ "$": "id", "i": i, "v": v.encode() }),
            Self::Ref(i) => json!({ "$": "r", "i": i }),
        }
    }
}

/// What a handler throws. Electron reports it to the renderer as
/// `Error invoking remote method '<channel>': <String(thrown)>`.
#[derive(Debug, Clone, PartialEq)]
pub enum Thrown {
    /// An `Error` (or subclass) instance: `String(e)` is `name: message`, or `name` alone.
    Error { name: String, message: JsString },
    /// Any other thrown value.
    Value(JsValue),
}

impl Thrown {
    pub fn error(message: impl Into<JsString>) -> Self {
        Self::Error {
            name: "Error".into(),
            message: message.into(),
        }
    }

    pub fn type_error(message: impl Into<JsString>) -> Self {
        Self::Error {
            name: "TypeError".into(),
            message: message.into(),
        }
    }

    /// The message of the `Error` the renderer's promise rejects with (UTF-16: it can carry a
    /// lone surrogate from a V8 error message).
    pub fn rejection_message(&self, channel: &str) -> JsString {
        let mut out = JsString::from(format!("Error invoking remote method '{channel}': "));
        match self {
            Self::Error { name, message } if message.0.is_empty() => {
                out.0.extend(name.encode_utf16())
            }
            Self::Error { name, message } => {
                out.0.extend(format!("{name}: ").encode_utf16());
                out.0.extend_from_slice(&message.0);
            }
            // Electron never replies when the handler throws undefined/null.
            Self::Value(JsValue::Undefined | JsValue::Null) => {
                out.0.extend("reply was never sent".encode_utf16())
            }
            Self::Value(v) => out.0.extend(to_js_string(v).encode_utf16()),
        }
        out
    }
}

/// JS `String(v)` for a received value.
pub fn to_js_string(v: &JsValue) -> String {
    match v {
        JsValue::Undefined => "undefined".into(),
        JsValue::Null => "null".into(),
        JsValue::Bool(b) => b.to_string(),
        JsValue::Number(n) => js::number_to_string(*n),
        JsValue::BigInt(b) => b.clone(),
        JsValue::String(s) => s.to_string_lossy(),
        JsValue::Array(items) => items
            .iter()
            .map(|i| match i {
                JsValue::Undefined | JsValue::Null => String::new(),
                i => to_js_string(i),
            })
            .collect::<Vec<_>>()
            .join(","),
        JsValue::Error { message, .. } if message.0.is_empty() => "Error".into(),
        JsValue::Error { message, .. } => format!("Error: {}", message.to_string_lossy()),
        JsValue::Boxed(v) | JsValue::Shared(_, v) => to_js_string(v),
        JsValue::RegExp { source, flags } => format!("/{}/{flags}", source.to_string_lossy()),
        _ => "[object Object]".into(),
    }
}

pub type HandlerResult = Result<JsValue, Thrown>;

/// `value[key]` as handler code reads it: own properties of plain objects (and array
/// `length`/indices); reading off `undefined`/`null` throws V8's `TypeError`.
/// JS `ToNumber` as an arithmetic operator applies it (a BigInt operand throws).
pub fn to_number(v: &JsValue) -> Result<f64, Thrown> {
    Ok(match v {
        JsValue::Undefined => f64::NAN,
        JsValue::Null => 0.0,
        JsValue::Bool(b) => f64::from(u8::from(*b)),
        JsValue::Number(n) => *n,
        JsValue::String(s) => js::string_to_number(&s.to_string_lossy()),
        JsValue::BigInt(_) => return Err(Thrown::type_error("Cannot mix BigInt and other types, use explicit conversions")),
        JsValue::Date(t) => *t,
        JsValue::Boxed(b) | JsValue::Shared(_, b) => to_number(b)?,
        JsValue::Array(_) => js::string_to_number(&to_js_string(v)),
        _ => f64::NAN,
    })
}

pub fn get_prop(value: &JsValue, key: &str) -> Result<JsValue, Thrown> {
    Ok(match value {
        JsValue::Undefined | JsValue::Null => {
            let what = if *value == JsValue::Undefined {
                "undefined"
            } else {
                "null"
            };
            return Err(Thrown::type_error(format!(
                "Cannot read properties of {what} (reading '{key}')"
            )));
        }
        JsValue::Shared(_, v) => return get_prop(v, key),
        JsValue::Object(_) => value.get(key).clone(),
        JsValue::Array(items) if key == "length" => JsValue::Number(items.len() as f64),
        JsValue::Array(items) => key
            .parse::<usize>()
            .ok()
            .and_then(|i| items.get(i))
            .cloned()
            .unwrap_or(JsValue::Undefined),
        JsValue::String(s) if key == "length" => JsValue::Number(s.0.len() as f64),
        _ => JsValue::Undefined,
    })
}

/// `JSON.stringify` throwing (a BigInt, or a cycle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringifyThrows;

/// `JSON.stringify(value)`: `Ok(None)` where it returns `undefined`, `Err` where it throws.
pub fn json_stringify(value: &JsValue) -> Result<Option<String>, StringifyThrows> {
    json_stringify_indent(value, "")
}

/// `JSON.stringify(value, null, gap)` (gap `"  "` for an indent of 2).
pub fn json_stringify_indent(
    value: &JsValue,
    gap: &str,
) -> Result<Option<String>, StringifyThrows> {
    fn index<'a>(v: &'a JsValue, ids: &mut std::collections::HashMap<u32, &'a JsValue>) {
        match v {
            JsValue::Shared(i, inner) => {
                ids.insert(*i, inner);
                index(inner, ids);
            }
            JsValue::Array(items) | JsValue::Set(items) => items.iter().for_each(|i| index(i, ids)),
            JsValue::Object(props) => props.iter().for_each(|(_, v)| index(v, ids)),
            JsValue::Map(pairs) => pairs.iter().for_each(|(k, v)| {
                index(k, ids);
                index(v, ids);
            }),
            JsValue::Boxed(b) => index(b, ids),
            _ => {}
        }
    }
    fn quote(s: &JsString) -> String {
        let mut out = String::from("\"");
        let units = &s.0;
        let mut i = 0;
        while i < units.len() {
            let u = units[i];
            let pair = (0xD800..0xDC00).contains(&u)
                && units
                    .get(i + 1)
                    .is_some_and(|n| (0xDC00..0xE000).contains(n));
            if pair {
                out.push_str(&String::from_utf16_lossy(&units[i..i + 2]));
                i += 2;
                continue;
            }
            match u {
                0x22 => out.push_str("\\\""),
                0x5C => out.push_str("\\\\"),
                0x08 => out.push_str("\\b"),
                0x0C => out.push_str("\\f"),
                0x0A => out.push_str("\\n"),
                0x0D => out.push_str("\\r"),
                0x09 => out.push_str("\\t"),
                u if u < 0x20 || (0xD800..0xE000).contains(&u) => {
                    out.push_str(&format!("\\u{u:04x}"))
                }
                u => out.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}')),
            }
            i += 1;
        }
        out.push('"');
        out
    }
    fn view_elements(kind: &str, bytes: &[u8]) -> Result<Vec<String>, StringifyThrows> {
        let n = |b: &[u8], w: usize| -> Vec<[u8; 8]> {
            b.chunks_exact(w)
                .map(|c| {
                    let mut a = [0u8; 8];
                    a[..w].copy_from_slice(c);
                    a
                })
                .collect()
        };
        let f = |x: f64| {
            if x.is_finite() {
                js::number_to_string(x)
            } else {
                "null".into()
            }
        };
        Ok(match kind {
            "Int8Array" => bytes.iter().map(|b| (*b as i8).to_string()).collect(),
            "Uint8Array" | "Uint8ClampedArray" => bytes.iter().map(u8::to_string).collect(),
            "Int16Array" => n(bytes, 2)
                .iter()
                .map(|a| i16::from_le_bytes([a[0], a[1]]).to_string())
                .collect(),
            "Uint16Array" => n(bytes, 2)
                .iter()
                .map(|a| u16::from_le_bytes([a[0], a[1]]).to_string())
                .collect(),
            "Int32Array" => n(bytes, 4)
                .iter()
                .map(|a| i32::from_le_bytes([a[0], a[1], a[2], a[3]]).to_string())
                .collect(),
            "Uint32Array" => n(bytes, 4)
                .iter()
                .map(|a| u32::from_le_bytes([a[0], a[1], a[2], a[3]]).to_string())
                .collect(),
            "Float32Array" => n(bytes, 4)
                .iter()
                .map(|a| f(f64::from(f32::from_le_bytes([a[0], a[1], a[2], a[3]]))))
                .collect(),
            "Float64Array" => n(bytes, 8)
                .iter()
                .map(|a| f(f64::from_le_bytes(*a)))
                .collect(),
            "BigInt64Array" | "BigUint64Array" if !bytes.is_empty() => return Err(StringifyThrows),
            _ => Vec::new(),
        })
    }
    fn wrap(open: char, close: char, parts: Vec<String>, gap: &str, indent: &str) -> String {
        if parts.is_empty() {
            return format!("{open}{close}");
        }
        if gap.is_empty() {
            return format!("{open}{}{close}", parts.join(","));
        }
        let inner = format!("{indent}{gap}");
        format!(
            "{open}\n{inner}{}\n{indent}{close}",
            parts.join(&format!(",\n{inner}"))
        )
    }
    fn go<'a>(
        v: &'a JsValue,
        ids: &std::collections::HashMap<u32, &'a JsValue>,
        stack: &mut Vec<*const JsValue>,
        gap: &str,
        indent: &str,
    ) -> Result<Option<String>, StringifyThrows> {
        let v = match v {
            JsValue::Ref(i) => *ids.get(i).ok_or(StringifyThrows)?,
            JsValue::Shared(_, inner) => &**inner,
            v => v,
        };
        let container = matches!(
            v,
            JsValue::Array(_) | JsValue::Object(_) | JsValue::View { .. }
        );
        if container {
            if stack.contains(&(v as *const _)) {
                return Err(StringifyThrows); // "Converting circular structure to JSON"
            }
            stack.push(v as *const _);
        }
        let out = match v {
            JsValue::Undefined => None,
            JsValue::Null => Some("null".into()),
            JsValue::Bool(b) => Some(b.to_string()),
            JsValue::Number(n) if n.is_finite() => Some(js::number_to_string(*n)),
            JsValue::Number(_) => Some("null".into()),
            JsValue::BigInt(_) => return Err(StringifyThrows),
            JsValue::String(s) => Some(quote(s)),
            JsValue::Date(t) if t.is_nan() => Some("null".into()),
            JsValue::Date(t) => Some(format!("\"{}\"", js::iso_string(*t as i64))),
            JsValue::Boxed(b) => match &**b {
                JsValue::BigInt(_) => return Err(StringifyThrows),
                inner => go(inner, ids, stack, gap, indent)?,
            },
            JsValue::Array(items) => {
                let inner = format!("{indent}{gap}");
                let mut parts = Vec::new();
                for i in items {
                    parts.push(go(i, ids, stack, gap, &inner)?.unwrap_or_else(|| "null".into()));
                }
                Some(wrap('[', ']', parts, gap, indent))
            }
            JsValue::Object(props) => {
                let inner = format!("{indent}{gap}");
                let sep = if gap.is_empty() { ":" } else { ": " };
                let mut parts = Vec::new();
                for (k, val) in props {
                    if let Some(s) = go(val, ids, stack, gap, &inner)? {
                        parts.push(format!("{}{sep}{s}", quote(k)));
                    }
                }
                Some(wrap('{', '}', parts, gap, indent))
            }
            JsValue::View {
                kind,
                buffer,
                offset,
                length,
            } if kind != "DataView" => {
                let els = view_elements(kind, &buffer[*offset..offset + length])?;
                let sep = if gap.is_empty() { ":" } else { ": " };
                let parts: Vec<String> = els
                    .iter()
                    .enumerate()
                    .map(|(i, e)| format!("\"{i}\"{sep}{e}"))
                    .collect();
                Some(wrap('{', '}', parts, gap, indent))
            }
            // Map, Set, RegExp, Error, ArrayBuffer, DataView: no own enumerable properties.
            _ => Some("{}".into()),
        };
        if container {
            stack.pop();
        }
        Ok(out)
    }
    let mut ids = std::collections::HashMap::new();
    index(value, &mut ids);
    go(value, &ids, &mut Vec::new(), gap, "")
}

/// `JSON.parse(text)`, as V8 does it (see [`js::json_parse`]); `Err` is the SyntaxError message.
pub fn json_parse(text: &str) -> Result<JsValue, JsString> {
    js::json_parse(&text.encode_utf16().collect::<Vec<_>>())
}

/// `CreateDataProperty(obj, key, value)` on a plain object: replaces an existing key in place,
/// otherwise inserts in JS property order (array-index keys ascending before the rest).
pub fn define(obj: &mut JsValue, key: JsString, value: JsValue) {
    let JsValue::Object(props) = obj else { return };
    if let Some(slot) = props.iter_mut().find(|(k, _)| *k == key) {
        slot.1 = value;
        return;
    }
    match array_index(&key.to_string_lossy()).filter(|_| String::from_utf16(&key.0).is_ok()) {
        Some(i) => {
            let at = props
                .iter()
                .position(|(k, _)| array_index(&k.to_string_lossy()).is_none_or(|j| j > i))
                .unwrap_or(props.len());
            props.insert(at, (key, value));
        }
        None => props.push((key, value)),
    }
}

/// What `obj[key]` finds.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    Own(JsValue),
    /// Inherited from `Object.prototype` (a function, or the prototype for `__proto__`):
    /// truthy, with no properties the source reads.
    Proto,
    Missing,
}

impl Lookup {
    pub fn truthy(&self) -> bool {
        match self {
            Self::Own(v) => v.truthy(),
            Self::Proto => true,
            Self::Missing => false,
        }
    }

    pub fn value(&self) -> JsValue {
        match self {
            Self::Own(v) => v.clone(),
            _ => JsValue::Undefined,
        }
    }
}

const OBJECT_PROTO: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

fn array_index(key: &str) -> Option<usize> {
    let ok = key == "0"
        || (!key.is_empty() && !key.starts_with('0') && key.bytes().all(|b| b.is_ascii_digit()));
    key.parse::<u32>()
        .ok()
        .filter(|i| ok && *i != u32::MAX)
        .map(|i| i as usize)
}

/// `obj[key]` with prototype fallthrough; reading off `null`/`undefined` throws.
pub fn lookup(obj: &JsValue, key: &str) -> Result<Lookup, Thrown> {
    let inherited = |k: &str| {
        if OBJECT_PROTO.contains(&k) {
            Lookup::Proto
        } else {
            Lookup::Missing
        }
    };
    Ok(match obj {
        JsValue::Undefined | JsValue::Null => return Err(get_prop(obj, key).unwrap_err()),
        JsValue::Shared(_, v) => return lookup(v, key),
        JsValue::Object(props) => {
            let k = JsString::from(key);
            props
                .iter()
                .rev()
                .find(|(pk, _)| *pk == k)
                .map_or_else(|| inherited(key), |(_, v)| Lookup::Own(v.clone()))
        }
        JsValue::Array(items) => match array_index(key) {
            Some(i) => items
                .get(i)
                .map_or(Lookup::Missing, |v| Lookup::Own(v.clone())),
            None if key == "length" => Lookup::Own(JsValue::Number(items.len() as f64)),
            None => inherited(key),
        },
        _ => inherited(key),
    })
}

/// `obj[key] = value` in sloppy mode: ignored on primitives, throws on `null`/`undefined`.
pub fn set(obj: &mut JsValue, key: &str, value: JsValue) -> Result<(), Thrown> {
    match obj {
        JsValue::Undefined | JsValue::Null => {
            let what = if *obj == JsValue::Undefined {
                "undefined"
            } else {
                "null"
            };
            Err(Thrown::type_error(format!(
                "Cannot set properties of {what} (setting '{key}')"
            )))
        }
        JsValue::Shared(_, v) => set(v, key, value),
        JsValue::Object(_) => {
            define(obj, JsString::from(key), value);
            Ok(())
        }
        JsValue::Array(items) => {
            if let Some(i) = array_index(key) {
                if i >= items.len() {
                    items.resize(i + 1, JsValue::Undefined);
                }
                items[i] = value;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `delete obj[key]` (an array element becomes a hole, serialized as `null`).
pub fn delete(obj: &mut JsValue, key: &str) -> Result<(), Thrown> {
    match obj {
        JsValue::Undefined | JsValue::Null => Err(Thrown::type_error(
            "Cannot convert undefined or null to object",
        )),
        JsValue::Shared(_, v) => delete(v, key),
        JsValue::Object(props) => {
            let k = JsString::from(key);
            props.retain(|(pk, _)| *pk != k);
            Ok(())
        }
        JsValue::Array(items) => {
            if let Some(slot) = array_index(key).and_then(|i| items.get_mut(i)) {
                *slot = JsValue::Undefined;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Identities handlers attach to a value they place in several spots of one reply (the same
/// JS array referenced twice). Starts high so it never meets ids decoded from the renderer.
static NEXT_IDENTITY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1 << 30);

/// `value`, marked as one object wherever the marker is cloned to.
pub fn shared(value: JsValue) -> JsValue {
    JsValue::Shared(NEXT_IDENTITY.fetch_add(1, std::sync::atomic::Ordering::Relaxed), Box::new(value))
}

/// Numbers object identities the way `codec.js` (structured clone) does for a reply: values
/// reachable more than once get `$id` at their first pre-order occurrence and `$r` after;
/// values reachable once are emitted plain.
pub fn normalize_identity(v: &JsValue) -> JsValue {
    use std::collections::HashMap;
    fn count(v: &JsValue, refs: &mut HashMap<u32, u32>) {
        match v {
            JsValue::Shared(i, inner) => {
                let n = refs.entry(*i).or_insert(0);
                *n += 1;
                if *n == 1 {
                    count(inner, refs);
                }
            }
            JsValue::Ref(i) => *refs.entry(*i).or_insert(0) += 1,
            JsValue::Array(items) | JsValue::Set(items) => items.iter().for_each(|x| count(x, refs)),
            JsValue::Object(props) => props.iter().for_each(|(_, x)| count(x, refs)),
            JsValue::Map(pairs) => pairs.iter().for_each(|(k, x)| {
                count(k, refs);
                count(x, refs);
            }),
            JsValue::Boxed(b) => count(b, refs),
            _ => {}
        }
    }
    fn emit(v: &JsValue, refs: &HashMap<u32, u32>, ids: &mut HashMap<u32, u32>, next: &mut u32, defs: &HashMap<u32, JsValue>) -> JsValue {
        let rec = |x: &JsValue, ids: &mut HashMap<u32, u32>, next: &mut u32| emit(x, refs, ids, next, defs);
        match v {
            JsValue::Shared(i, _) | JsValue::Ref(i) => {
                if let Some(id) = ids.get(i) {
                    return JsValue::Ref(*id);
                }
                let inner = match v {
                    JsValue::Shared(_, inner) => (**inner).clone(),
                    _ => defs.get(i).cloned().unwrap_or_default(),
                };
                if refs.get(i).copied().unwrap_or(0) > 1 {
                    let id = *next;
                    *next += 1;
                    ids.insert(*i, id);
                    JsValue::Shared(id, Box::new(rec(&inner, ids, next)))
                } else {
                    rec(&inner, ids, next)
                }
            }
            JsValue::Array(items) => JsValue::Array(items.iter().map(|x| rec(x, ids, next)).collect()),
            JsValue::Set(items) => JsValue::Set(items.iter().map(|x| rec(x, ids, next)).collect()),
            JsValue::Object(props) => JsValue::Object(props.iter().map(|(k, x)| (k.clone(), rec(x, ids, next))).collect()),
            JsValue::Map(pairs) => JsValue::Map(pairs.iter().map(|(k, x)| (rec(k, ids, next), rec(x, ids, next))).collect()),
            JsValue::Boxed(b) => JsValue::Boxed(Box::new(rec(b, ids, next))),
            v => v.clone(),
        }
    }
    fn defs(v: &JsValue, out: &mut HashMap<u32, JsValue>) {
        match v {
            JsValue::Shared(i, inner) => {
                if !out.contains_key(i) {
                    out.insert(*i, (**inner).clone());
                    defs(inner, out);
                }
            }
            JsValue::Array(items) | JsValue::Set(items) => items.iter().for_each(|x| defs(x, out)),
            JsValue::Object(props) => props.iter().for_each(|(_, x)| defs(x, out)),
            JsValue::Map(pairs) => pairs.iter().for_each(|(k, x)| {
                defs(k, out);
                defs(x, out);
            }),
            JsValue::Boxed(b) => defs(b, out),
            _ => {}
        }
    }
    let mut refs = HashMap::new();
    count(v, &mut refs);
    let mut d = HashMap::new();
    defs(v, &mut d);
    emit(v, &refs, &mut HashMap::new(), &mut 0, &d)
}
