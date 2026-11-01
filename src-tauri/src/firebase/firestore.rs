//! Firestore UserDataReader: merge masks, update paths and JSON-proto scalar types.
use super::{Auth, Error};
use crate::{
    host::{FetchRequest, Host},
    ipc::JsValue,
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
fn offline() -> Error {
    Error {
        code: "unavailable".into(),
        message: "Failed to get document because the client is offline.".into(),
    }
}
fn status_error(status: u16, v: &Value, stream: bool) -> Error {
    let e = if v.is_array() {
        &v[0]["error"]
    } else {
        &v["error"]
    };
    // A canonical REST google.rpc status preserves INVALID_ARGUMENT vs
    // FAILED_PRECONDITION, which both use HTTP 400.
    let name = e["status"].as_str().unwrap_or(match status {
        400 => "FAILED_PRECONDITION",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        409 => "ABORTED",
        429 => "RESOURCE_EXHAUSTED",
        500 => "INTERNAL",
        503 => "UNAVAILABLE",
        504 => "DEADLINE_EXCEEDED",
        _ => "UNKNOWN",
    });
    let number = match name {
        "CANCELLED" => 1,
        "UNKNOWN" => 2,
        "INVALID_ARGUMENT" => 3,
        "DEADLINE_EXCEEDED" => 4,
        "NOT_FOUND" => 5,
        "ALREADY_EXISTS" => 6,
        "PERMISSION_DENIED" => 7,
        "RESOURCE_EXHAUSTED" => 8,
        "FAILED_PRECONDITION" => 9,
        "ABORTED" => 10,
        "OUT_OF_RANGE" => 11,
        "UNIMPLEMENTED" => 12,
        "INTERNAL" => 13,
        "UNAVAILABLE" => 14,
        "UNAUTHENTICATED" => 16,
        _ => 2,
    };
    let message = e["message"].as_str().unwrap_or("");
    Error {
        code: name.to_ascii_lowercase().replace('_', "-"),
        message: if stream {
            format!("{number} {name}: {message}")
        } else {
            message.into()
        },
    }
}
async fn request<H: Host>(
    host: &H,
    auth: &mut Auth,
    method: &'static str,
    suffix: &str,
    body: Option<Value>,
) -> Result<Value, Error> {
    let write = suffix == ":commit";
    let mut delay = 0u64;
    loop {
        let database = ROOT.trim_end_matches("/documents");
        let mut headers = vec![
            ("content-type".into(), "application/json".into()),
            ("google-cloud-resource-prefix".into(), database.into()),
            ("x-goog-request-params".into(), database.into()),
            (
                "x-goog-api-client".into(),
                "gl-node/20.18.3 fire/12.11.0 grpc/1.9.15".into(),
            ),
            ("x-firebase-gmpid".into(), super::APP_ID.into()),
        ];
        if auth.current_user.is_some() {
            headers.push((
                "authorization".into(),
                format!("Bearer {}", {
                    // A changed token during credentials acquisition makes the full
                    // SDK restart acquisition, including its forced-refresh request.
                    let expired = auth
                        .current_user
                        .as_ref()
                        .is_some_and(|u| host.now_ms() > u.expires - 30000);
                    let token = auth.get_id_token(host, false).await?;
                    if expired {
                        auth.get_id_token(host, true).await?
                    } else {
                        token
                    }
                }),
            ));
        }
        let response = host
            .fetch(FetchRequest {
                method,
                url: format!("https://firestore.googleapis.com/v1/{ROOT}{suffix}"),
                headers,
                body: body.as_ref().map(|v| v.to_string().into_bytes()),
                manual_redirect: false,
            })
            .await;
        let error = match response {
            Err(_) => {
                if !write {
                    return Err(offline());
                }
                Error {
                    code: "unavailable".into(),
                    message: String::new(),
                }
            }
            Ok(r) => {
                let v: Value = serde_json::from_slice(&r.body).map_err(|_| Error {
                    code: "unknown".into(),
                    message: "Invalid JSON response".into(),
                })?;
                // batchGet is a streaming REST RPC. An error in a successful
                // stream is an application/target error, unlike failed transport.
                let embedded_error = v.get("error").is_some()
                    || v.as_array()
                        .is_some_and(|a| a.first().is_some_and(|r| r.get("error").is_some()));
                if r.ok() && !embedded_error {
                    return Ok(v);
                }
                if !write && !r.ok() {
                    return Err(offline());
                }
                status_error(r.status, &v, write)
            }
        };
        if !write
            || !matches!(
                error.code.as_str(),
                "unavailable"
                    | "unauthenticated"
                    | "internal"
                    | "deadline-exceeded"
                    | "resource-exhausted"
                    | "aborted"
                    | "unknown"
            )
        {
            return Err(error);
        }
        // Full SDK backoff: first retry immediately, then exponential delay.
        // Resource exhaustion starts at the maximum delay (60 seconds).
        if error.code == "resource-exhausted" {
            delay = 60_000;
        }
        let mut random = [0u8; 8];
        let jitter = if getrandom::fill(&mut random).is_ok() {
            0.5 + (u64::from_le_bytes(random) >> 11) as f64 / (1u64 << 53) as f64
        } else {
            1.0
        };
        tokio::time::sleep(std::time::Duration::from_millis(
            (delay as f64 * jitter).floor() as u64,
        ))
        .await;
        delay = if delay == 0 {
            1000
        } else {
            (delay * 3 / 2).min(60_000)
        };
    }
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
/// Read a document over REST batchGet using full-SDK offline delivery semantics.
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
    let rows = reply.as_array().ok_or_else(|| Error {
        code: "unknown".into(),
        message: "Invalid Firestore batchGet response".into(),
    })?;
    let row = rows
        .first()
        .filter(|r| r.get("found").is_some() || r.get("missing").is_some())
        .ok_or_else(|| Error {
            code: "unknown".into(),
            message: "Invalid Firestore batchGet result".into(),
        })?;
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
    if let Some(reference) = v["referenceValue"].as_str() {
        return Ok(JsValue::FirestoreReference(
            reference.split("/documents/").nth(1).unwrap_or("").into(),
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
