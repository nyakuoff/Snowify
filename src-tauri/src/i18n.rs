//! Port of `src/main/i18n.js`: main-process translations (dialogs, tray, errors).
//!
//! Full JS semantics of `data?.[key] ?? key` followed by `str.replaceAll('{{k}}', v)` are kept,
//! including lookups that fall through to `Object.prototype`, non-string locale values, and
//! `String(v)` coercion of params.

use crate::ipc::{JsValue, Thrown, to_js_string};
use crate::js;
use serde_json::{Map, Value};
use std::{borrow::Cow, fmt, fs, io, path::Path};

const SUPPORTED: [&str; 12] = [
    "en", "es", "pt", "fr", "de", "ja", "ko", "zh", "it", "tr", "ru", "hi",
];

/// Own properties of `Object.prototype` (V8), reachable through `data[key]` on the parsed
/// locale object when the file has no own key of that name.
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

/// `(override || app.getLocale()).split('-')[0].toLowerCase()`, or `"en"` if unsupported.
/// `override_locale` is the raw IPC argument: any falsy value falls through to `system`; a
/// string (or boxed `String`) is used; anything else throws the source's `TypeError`.
pub fn resolve_locale(override_locale: &JsValue, system: &str) -> Result<&'static str, Thrown> {
    let raw = match override_locale {
        v if !v.truthy() => system.to_owned(),
        JsValue::String(s) => s.to_string_lossy(),
        JsValue::Boxed(b) if matches!(**b, JsValue::String(_)) => to_js_string(b),
        _ => {
            return Err(Thrown::type_error(
                "(overrideLocale || app.getLocale(...)).split is not a function",
            ));
        }
    };
    let lang = raw.split('-').next().unwrap_or_default().to_lowercase();
    Ok(SUPPORTED.into_iter().find(|s| *s == lang).unwrap_or("en"))
}

/// The loaded locale. `None` means never loaded or the file was JSON `null`; both make
/// every lookup miss (`data?.[key]` is `undefined`).
#[derive(Debug, Default)]
pub struct Translations(Option<Map<String, Value>>);

impl Translations {
    /// Loads `<dir>/<locale>.json`, falling back to `en.json` if that is unreadable or invalid.
    /// Errors only if `en.json` fails too (the source throws, leaving the previous data).
    ///
    /// A top-level other than an object or `null` counts as invalid. The source would accept
    /// it; shipped locale files are objects, which `tests/parity_i18n.rs` enforces.
    pub fn load(dir: &Path, locale: &str) -> io::Result<Self> {
        Self::load_with(|name| fs::read_to_string(dir.join(name)), locale)
    }

    /// [`Translations::load`] reading `<locale>.json` through `read` (e.g. bundled assets).
    pub fn load_with(
        read_file: impl Fn(&str) -> io::Result<String>,
        locale: &str,
    ) -> io::Result<Self> {
        let read = |l: &str| -> io::Result<Self> {
            let text = read_file(&format!("{l}.json"))?;
            match serde_json::from_str(&text).map_err(io::Error::other)? {
                Value::Object(map) => Ok(Self(Some(map))),
                Value::Null => Ok(Self(None)),
                _ => Err(io::Error::other("locale file is not a JSON object")),
            }
        };
        read(locale).or_else(|_| read("en"))
    }

    /// `mt(key, params)`. Params are applied in order, so a value containing a later
    /// `{{param}}` is substituted again. Params on a non-string result throw, as
    /// `.replaceAll` does in the source; an empty `params` never throws.
    pub fn mt(&self, key: &str, params: &[(&str, Param)]) -> Result<Translated, Thrown> {
        let found = self.0.as_ref().and_then(|map| match map.get(key) {
            Some(Value::Null) => None,
            Some(Value::String(s)) => Some(Translated::Str(s.clone())),
            Some(v) => Some(Translated::Json(v.clone())),
            None => OBJECT_PROTO
                .into_iter()
                .find(|p| *p == key)
                .map(Translated::ObjectProto),
        });
        let mut s = match found {
            None => key.to_owned(),
            Some(Translated::Str(s)) => s,
            Some(other) if params.is_empty() => return Ok(other),
            Some(_) => return Err(Thrown::type_error("str.replaceAll is not a function")),
        };
        for (k, v) in params {
            s = js_replace_all(&s, &format!("{{{{{k}}}}}"), v);
        }
        Ok(Translated::Str(s))
    }
}

/// The JS value `mt` returns. `Display` is JS `String(value)`.
#[derive(Debug, Clone, PartialEq)]
pub enum Translated {
    Str(String),
    /// A non-string, non-null locale value: number, boolean, array or object.
    Json(Value),
    /// A member inherited from `Object.prototype`: a function, or the prototype itself
    /// for `__proto__`.
    ObjectProto(&'static str),
}

impl fmt::Display for Translated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(s) => f.write_str(s),
            Self::Json(v) => f.write_str(&js::to_string(v)),
            Self::ObjectProto("__proto__") => f.write_str("[object Object]"),
            Self::ObjectProto("constructor") => f.write_str("function Object() { [native code] }"),
            Self::ObjectProto(name) => write!(f, "function {name}() {{ [native code] }}"),
        }
    }
}

/// A `mt` param value. Coerced like the replacement argument of JS `replaceAll`.
#[derive(Clone, Copy)]
pub enum Param<'a> {
    Str(&'a str),
    Num(f64),
    Bool(bool),
    Null,
    Undefined,
    /// A JS replacer function: called per match with (matched, UTF-16 offset, whole
    /// string). Its result is used as-is, without `$` expansion.
    Fn(&'a dyn Fn(&str, usize, &str) -> String),
}

impl<'a> From<&'a str> for Param<'a> {
    fn from(s: &'a str) -> Self {
        Self::Str(s)
    }
}

impl From<f64> for Param<'_> {
    fn from(n: f64) -> Self {
        Self::Num(n)
    }
}

impl From<i32> for Param<'_> {
    fn from(n: i32) -> Self {
        Self::Num(n.into())
    }
}

impl From<bool> for Param<'_> {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

/// `String.prototype.replaceAll` with a non-empty string pattern. A non-function replacement
/// is coerced to a string and its `$` patterns expanded (`$$`, `$&`, `` $` ``, `$'`; anything
/// else stays literal, since there are no capture groups).
fn js_replace_all(s: &str, pat: &str, param: &Param) -> String {
    let rep: Cow<str> = match *param {
        Param::Str(r) => r.into(),
        Param::Num(n) => js::number_to_string(n).into(),
        Param::Bool(b) => b.to_string().into(),
        Param::Null => "null".into(),
        Param::Undefined => "undefined".into(),
        Param::Fn(_) => "".into(),
    };
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (pos, m) in s.match_indices(pat) {
        out.push_str(&s[last..pos]);
        last = pos + m.len();
        if let Param::Fn(f) = param {
            out.push_str(&f(m, s[..pos].encode_utf16().count(), s));
            continue;
        }
        let mut rest = &*rep;
        while let Some(i) = rest.find('$') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            let (sub, skip) = match after.as_bytes().first() {
                Some(b'$') => ("$", 1),
                Some(b'&') => (m, 1),
                Some(b'`') => (&s[..pos], 1),
                Some(b'\'') => (&s[last..], 1),
                _ => ("$", 0),
            };
            out.push_str(sub);
            rest = &after[skip..];
        }
        out.push_str(rest);
    }
    out.push_str(&s[last..]);
    out
}
