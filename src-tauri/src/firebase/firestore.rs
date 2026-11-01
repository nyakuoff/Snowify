//! Firestore UserDataReader: merge masks, update paths and JSON-proto scalar types.
use super::{Auth, Error};
use crate::{
    host::{FetchRequest, Host},
    ipc::{self, JsValue},
};
use serde_json::{Value, json};
const ROOT: &str = "projects/snowify-dcda0/databases/(default)/documents";
#[derive(Clone, Debug)]
/// Document path relative to the configured database.
pub struct Document {
    /// Slash-separated collection/document path.
    pub path: String,
}
/// Construct and validate a document reference.
pub fn doc(collection: &str, id: &str) -> Result<Document, Error> {
    let path = format!("{collection}/{id}");
    if path.contains("//") {
        return Err(Error {
            code: "invalid-argument".into(),
            message: format!("Invalid segment ({path}). Paths must not contain // in them."),
        });
    }
    let path = path.trim_matches('/').to_owned();
    let n = path.split('/').count();
    if n % 2 != 0 {
        return Err(Error {
            code: "invalid-argument".into(),
            message: format!(
                "Invalid document reference. Document references must have an even number of segments, but {path} has {n}."
            ),
        });
    }
    Ok(Document { path })
}
fn canonical(path: &[String]) -> String {
    path.iter()
        .map(|s| {
            let escaped = s.replace('\\', "\\\\").replace('`', "\\`");
            if regex::Regex::new(r"^[_a-zA-Z][_a-zA-Z0-9]*$")
                .unwrap()
                .is_match(s)
            {
                escaped
            } else {
                format!("`{escaped}`")
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}
fn error(method: &str, reason: &str, path: &[String], doc: &Document) -> Error {
    Error {
        code: "invalid-argument".into(),
        message: format!(
            "Function {method}() called with invalid data. {reason} (found{} in document {})",
            if path.is_empty() {
                String::new()
            } else {
                format!(" in field {}", canonical(path))
            },
            doc.path
        ),
    }
}
fn parse(
    v: &JsValue,
    path: Option<Vec<String>>,
    array: bool,
    method: &str,
    doc: &Document,
    masks: &mut Vec<Vec<String>>,
) -> Result<Value, Error> {
    let p = path.clone().unwrap_or_default();
    let err = |reason: &str| error(method, reason, &p, doc);
    match v {
        JsValue::Object(entries) => {
            let mut fields = serde_json::Map::new();
            if entries.is_empty() && !p.is_empty() {
                masks.push(p.clone());
            }
            for (k, v) in entries {
                let k = k.to_string_lossy();
                let mut child = p.clone();
                child.push(k.clone());
                let child = path.as_ref().map(|_| child);
                if k.is_empty() || k.starts_with("__") && k.ends_with("__") {
                    return Err(error(
                        method,
                        if k.is_empty() {
                            "Document fields must not be empty"
                        } else {
                            "Document fields cannot begin and end with \"__\""
                        },
                        child.as_deref().unwrap_or(&[]),
                        doc,
                    ));
                }
                fields.insert(k, parse(v, child, false, method, doc, masks)?);
            }
            Ok(json!({"mapValue":{"fields":fields}}))
        }
        JsValue::Array(a) => {
            if array {
                return Err(err("Nested arrays are not supported"));
            }
            if !p.is_empty() {
                masks.push(p);
            }
            let values = a
                .iter()
                .map(|v| parse(v, None, true, method, doc, masks))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({"arrayValue":{"values":values}}))
        }
        _ => {
            if !p.is_empty() {
                masks.push(p.clone());
            }
            Ok(match v {
                JsValue::Null => json!({"nullValue":"NULL_VALUE"}),
                JsValue::Bool(b) => json!({"booleanValue":b}),
                JsValue::String(s) => json!({"stringValue":s.to_string_lossy()}),
                JsValue::Number(n) => {
                    if n.is_finite()
                        && n.fract() == 0.0
                        && n.abs() <= 9007199254740991.0
                        && !(*n == 0.0 && n.is_sign_negative())
                    {
                        json!({"integerValue":crate::js::number_to_string(*n)})
                    } else {
                        json!({"doubleValue":if n.is_nan(){json!("NaN")}else if *n==f64::INFINITY{json!("Infinity")}else if *n==f64::NEG_INFINITY{json!("-Infinity")}else if *n==0.0{json!("-0")}else{serde_json::from_str::<Value>(&crate::js::number_to_string(*n)).unwrap()}})
                    }
                }
                JsValue::Date(ms) => {
                    let iso = crate::js::iso_string(*ms as i64);
                    json!({"timestampValue":iso.replace('Z',"000000Z")})
                }
                JsValue::Undefined => return Err(err("Unsupported field value: undefined")),
                JsValue::View { kind, .. } => {
                    return Err(err(&format!(
                        "Unsupported field value: a custom {kind} object"
                    )));
                }
                _ => return Err(err("Unsupported field value: a custom Object object")),
            })
        }
    }
}
fn description(v: &JsValue) -> String {
    match v {
        JsValue::Undefined => "undefined".into(),
        JsValue::Null => "null".into(),
        JsValue::Bool(b) => b.to_string(),
        JsValue::Number(n) => crate::js::number_to_string(*n),
        JsValue::String(s) => format!("\"{}\"", s.to_string_lossy()),
        JsValue::Array(_) => "an array".into(),
        JsValue::Date(_) => "a custom Date object".into(),
        JsValue::View { kind, .. } => format!("a custom {kind} object"),
        _ => "a custom Object object".into(),
    }
}
/// REST commit body shared with Firestore Lite; setDoc dots are literal field names.
pub fn write_body(doc: &Document, data: &JsValue, update: bool) -> Result<Value, Error> {
    if update {
        if let JsValue::String(k) = data {
            return write_body(
                doc,
                &JsValue::Object(vec![(k.clone(), JsValue::Undefined)]),
                true,
            );
        }
    }
    let method = if update { "updateDoc" } else { "setDoc" };
    if !matches!(data, JsValue::Object(_)) {
        return Err(error(
            method,
            &format!("Data must be an object, but it was: {}", description(data)),
            &[],
            doc,
        ));
    }
    let mut masks = Vec::new();
    let mut fields = serde_json::Map::new();
    if update {
        let JsValue::Object(entries) = data else {
            return Err(error(
                method,
                "Data must be an object, but it was: null",
                &[],
                doc,
            ));
        };
        for (k, v) in entries {
            let k = k.to_string_lossy();
            let p: Vec<String> = k.split('.').map(str::to_owned).collect();
            if k.chars().any(|c| "~*/[]".contains(c)) {
                return Err(error(
                    method,
                    &format!(
                        "Invalid field path ({k}). Paths must not contain '~', '*', '/', '[', or ']'"
                    ),
                    &[],
                    doc,
                ));
            }
            if p.iter().any(String::is_empty) {
                return Err(error(
                    method,
                    &format!(
                        "Invalid field path ({k}). Paths must not be empty, begin with '.', end with '.', or contain '..'"
                    ),
                    &[],
                    doc,
                ));
            }
            for s in &p {
                if s.starts_with("__") && s.ends_with("__") {
                    return Err(error(
                        method,
                        "Document fields cannot begin and end with \"__\"",
                        &p,
                        doc,
                    ));
                }
            }
            let value = parse(v, Some(p.clone()), false, method, doc, &mut Vec::new())?;
            masks.push(p.clone());
            insert(&mut fields, &p, value);
        }
    } else {
        let parsed = parse(data, Some(Vec::new()), false, method, doc, &mut masks)?;
        fields = parsed["mapValue"]["fields"]
            .as_object()
            .cloned()
            .ok_or_else(|| error(method, "Data must be an object, but it was: null", &[], doc))?;
    }
    masks.sort_by(|a, b| {
        a.iter()
            .map(|s| s.encode_utf16().collect::<Vec<_>>())
            .cmp(b.iter().map(|s| s.encode_utf16().collect::<Vec<_>>()))
    });
    masks.dedup();
    let mut write = json!({"update":{"name":format!("{ROOT}/{}",doc.path),"fields":fields},"updateMask":{"fieldPaths":masks.iter().map(|p|canonical(p)).collect::<Vec<_>>()}});
    if update
        && write["update"]["fields"]
            .as_object()
            .is_some_and(|m| m.is_empty())
    {
        write["update"].as_object_mut().unwrap().remove("fields");
    }
    if update {
        write["currentDocument"] = json!({"exists":true});
    }
    Ok(json!({"writes":[write]}))
}
fn insert(fields: &mut serde_json::Map<String, Value>, path: &[String], v: Value) {
    if path.len() == 1 {
        fields.insert(path[0].clone(), v);
    } else {
        let entry = fields
            .entry(path[0].clone())
            .or_insert(json!({"mapValue":{"fields":{}}}));
        if entry["mapValue"]["fields"].as_object().is_none() {
            *entry = json!({"mapValue":{"fields":{}}});
        }
        insert(
            entry["mapValue"]["fields"].as_object_mut().unwrap(),
            &path[1..],
            v,
        );
    }
}
/// Document read result, distinguishing missing from an empty document.
pub struct Snapshot {
    /// Decoded fields, or None for a missing document.
    pub data: Option<JsValue>,
}
impl Snapshot {
    /// Whether this read returned a document.
    pub fn exists(&self) -> bool {
        self.data.is_some()
    }
    /// Decoded data, or undefined when the document does not exist.
    pub fn data(&self) -> JsValue {
        self.data.clone().unwrap_or(JsValue::Undefined)
    }
}
async fn request<H: Host>(
    host: &H,
    auth: &mut Auth,
    method: &'static str,
    suffix: &str,
    body: Option<Value>,
) -> Result<Value, Error> {
    let mut headers = vec![("Content-Type".into(), "application/json".into())];
    if auth.current_user.is_some() {
        headers.push((
            "Authorization".into(),
            format!("Bearer {}", auth.get_id_token(host, false).await?),
        ));
    }
    let r = host
        .fetch(FetchRequest {
            method,
            url: format!("https://firestore.googleapis.com/v1/{ROOT}{suffix}"),
            headers,
            body: body.map(|v| v.to_string().into_bytes()),
            manual_redirect: false,
        })
        .await
        .map_err(|_| Error {
            code: "unknown".into(),
            message: "Request failed with error: undefined".into(),
        })?;
    // Response.json() uses V8 JSON.parse messages even for non-2xx bodies.
    let text = String::from_utf8_lossy(&r.body);
    let text = text.trim_start_matches('\u{feff}');
    ipc::json_parse(text).map_err(|e| Error {
        code: "unknown".into(),
        message: format!("SyntaxError: {}", e.to_string_lossy()),
    })?;
    let v: Value = serde_json::from_str(text).map_err(|_| Error {
        code: "unknown".into(),
        message: "Invalid JSON response".into(),
    })?;
    if !r.ok() {
        // Lite maps HTTP status, not error.status (which can disagree).
        let code = match r.status {
            400 => "failed-precondition",
            401 => "unauthenticated",
            403 => "permission-denied",
            404 => "not-found",
            409 => "aborted",
            416 => "out-of-range",
            429 => "resource-exhausted",
            499 => "cancelled",
            500 => "unknown",
            501 => "unimplemented",
            503 => "unavailable",
            504 => "deadline-exceeded",
            400..=499 => "failed-precondition",
            500..=599 => "internal",
            _ => "unknown",
        };
        let e = if v.is_array() { &v[0] } else { &v };
        return Err(Error {
            code: code.into(),
            message: format!(
                "Request failed with error: {}",
                if e["error"]["message"].is_null() {
                    r.status_text.clone()
                } else {
                    ipc::to_js_string(&JsValue::from(&e["error"]["message"]))
                }
            ),
        });
    }
    Ok(v)
}
/// Merge fields into a document using a REST commit.
pub async fn set_doc<H: Host>(
    host: &H,
    auth: &mut Auth,
    doc: &Document,
    data: &JsValue,
) -> Result<(), Error> {
    request(
        host,
        auth,
        "POST",
        ":commit",
        Some(write_body(doc, data, false)?),
    )
    .await
    .map(|_| ())
}
/// Update field paths with an exists=true precondition.
pub async fn update_doc<H: Host>(
    host: &H,
    auth: &mut Auth,
    doc: &Document,
    data: &JsValue,
) -> Result<(), Error> {
    request(
        host,
        auth,
        "POST",
        ":commit",
        Some(write_body(doc, data, true)?),
    )
    .await
    .map(|_| ())
}
/// Read a document using the Lite SDK REST batchGet and the current auth token.
pub async fn get_doc<H: Host>(
    host: &H,
    auth: &mut Auth,
    doc: &Document,
) -> Result<Snapshot, Error> {
    let reply = request(
        host,
        auth,
        "POST",
        ":batchGet",
        Some(json!({"documents":[format!("{ROOT}/{}", doc.path)]})),
    )
    .await?;
    let native = |message: String| Error {
        code: String::new(),
        message,
    };
    let rows = reply.as_array().ok_or_else(|| {
        native(if reply.is_null() {
            "Cannot read properties of null (reading 'forEach')".into()
        } else {
            "response.forEach is not a function".into()
        })
    })?;
    let row = rows.first().ok_or_else(|| native(format!(
        "FIRESTORE (12.11.0_lite) INTERNAL ASSERTION FAILED: Unexpected state (ID: d7c2) CONTEXT: {}",
        json!({"key":{"path":{"segments":doc.path.split('/').collect::<Vec<_>>(),"offset":0,"len":doc.path.split('/').count()}}})
    )))?;
    if row.is_null() {
        return Err(native(
            "Cannot use 'in' operator to search for 'found' in null".into(),
        ));
    }
    if row.get("found").is_none() && row.get("missing").is_none() {
        return Err(native(format!(
            "FIRESTORE (12.11.0_lite) INTERNAL ASSERTION FAILED: Unexpected state (ID: 1c42) CONTEXT: {}",
            json!({"result":row})
        )));
    }
    let v = &row["found"];
    Ok(Snapshot {
        data: if v.is_null() {
            None
        } else {
            Some(decode_fields(&v["fields"])?)
        },
    })
}
/// Decode Firestore field values to Electron IPC values.
pub fn decode_fields(fields: &Value) -> Result<JsValue, Error> {
    Ok(JsValue::Object(
        fields
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| Ok((k.clone().into(), decode_value(v)?)))
            .collect::<Result<_, Error>>()?,
    ))
}
/// Decode a Firestore Value, preserving the SDK structured-clone shape.
pub fn decode_value(v: &Value) -> Result<JsValue, Error> {
    if v.get("nullValue").is_some() {
        return Ok(JsValue::Null);
    }
    if let Some(b) = v["booleanValue"].as_bool() {
        return Ok(JsValue::Bool(b));
    }
    for key in ["integerValue", "doubleValue"] {
        if let Some(n) = v.get(key) {
            return Ok(JsValue::Number(n.as_f64().unwrap_or_else(
                || match n.as_str().unwrap_or("") {
                    "NaN" => f64::NAN,
                    "Infinity" => f64::INFINITY,
                    "-Infinity" => f64::NEG_INFINITY,
                    s => s.parse().unwrap_or(0.0),
                },
            )));
        }
    }
    if let Some(s) = v["stringValue"].as_str() {
        return Ok(s.into());
    }
    if let Some(a) = v.get("arrayValue") {
        return Ok(JsValue::Array(
            a["values"]
                .as_array()
                .into_iter()
                .flatten()
                .map(decode_value)
                .collect::<Result<_, _>>()?,
        ));
    }
    if let Some(m) = v.get("mapValue") {
        if m["fields"]["__type__"]["stringValue"] == "__vector__" {
            return Ok(JsValue::Object(vec![(
                "_values".into(),
                decode_value(&m["fields"]["value"])?,
            )]));
        }
        return decode_fields(&m["fields"]);
    }
    if let Some(t) = v.get("timestampValue") {
        let (seconds, nanos) = if let Some(s) = t.as_str() {
            let ms = crate::js::date_parse_iso(s).unwrap_or(0.0) as i64;
            let fraction = s.split('.').nth(1).unwrap_or("").trim_end_matches('Z');
            let nano = format!("{fraction:0<9}")
                .chars()
                .take(9)
                .collect::<String>()
                .parse::<u32>()
                .unwrap_or(0);
            (ms.div_euclid(1000), nano)
        } else {
            (
                t["seconds"]
                    .as_i64()
                    .or_else(|| t["seconds"].as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(0),
                t["nanos"].as_u64().unwrap_or(0) as u32,
            )
        };
        return Ok(JsValue::from(
            &json!({"seconds":seconds,"nanoseconds":nanos}),
        ));
    }
    if let Some(s) = v["bytesValue"].as_str() {
        let mut bytes = Vec::new();
        let mut bits = 0u32;
        let mut count = 0;
        for c in s.bytes() {
            if let Some(n) = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
                .iter()
                .position(|x| *x == c)
            {
                bits = (bits << 6) | n as u32;
                count += 6;
                if count >= 8 {
                    count -= 8;
                    bytes.push((bits >> count) as u8);
                }
            }
        }
        let binary: String = bytes.iter().map(|b| char::from(*b)).collect();
        return Ok(JsValue::from(
            &json!({"_byteString":{"binaryString":binary}}),
        ));
    }
    if let Some(g) = v.get("geoPointValue") {
        return Ok(JsValue::from(
            &json!({"_lat":g["latitude"],"_long":g["longitude"]}),
        ));
    }
    Err(Error {
        code: "unimplemented".into(),
        message: "Firestore value decoding not implemented for this kind".into(),
    })
}
