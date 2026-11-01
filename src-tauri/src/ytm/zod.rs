//! ytmusic-api's `checkType(data, schema)`: zod 3.25's `safeParse` for the schema shapes the
//! library uses, and on failure the `console.error` it writes (the data, the schema as
//! `zodToJsonSchema` prints it, and the ZodError, `JSON.stringify(…, null, 2)`). The data is
//! returned either way.

use crate::{
    ipc::{self, JsString, JsValue},
    logger,
};
use std::sync::LazyLock;

/// A zod schema, as far as ytmusic-api's types go.
pub enum Z {
    Str,
    Num,
    Bool,
    Any,
    Lit(&'static str),
    Nullable(Box<Z>),
    Arr(Box<Z>),
    /// `.strict()` object.
    Obj(Vec<(&'static str, Z)>),
}

use Z::*;

fn obj(fields: Vec<(&'static str, Z)>) -> Z {
    Obj(fields)
}
fn null(z: Z) -> Z {
    Nullable(Box::new(z))
}
fn arr(z: Z) -> Z {
    Arr(Box::new(z))
}
fn thumb() -> Z {
    obj(vec![("url", Str), ("width", Num), ("height", Num)])
}
fn artist_basic() -> Z {
    obj(vec![("artistId", null(Str)), ("name", Str)])
}
fn album_basic() -> Z {
    obj(vec![("albumId", Str), ("name", Str)])
}

pub fn song_detailed() -> Z {
    obj(vec![
        ("type", Lit("SONG")),
        ("videoId", Str),
        ("name", Str),
        ("artist", artist_basic()),
        ("album", null(album_basic())),
        ("duration", null(Num)),
        ("thumbnails", arr(thumb())),
    ])
}
pub fn video_detailed() -> Z {
    obj(vec![("type", Lit("VIDEO")), ("videoId", Str), ("name", Str), ("artist", artist_basic()), ("duration", null(Num)), ("thumbnails", arr(thumb()))])
}
pub fn artist_detailed() -> Z {
    obj(vec![("artistId", Str), ("name", Str), ("type", Lit("ARTIST")), ("thumbnails", arr(thumb()))])
}
pub fn album_detailed() -> Z {
    obj(vec![
        ("type", Lit("ALBUM")),
        ("albumId", Str),
        ("playlistId", Str),
        ("name", Str),
        ("artist", artist_basic()),
        ("year", null(Num)),
        ("thumbnails", arr(thumb())),
    ])
}
pub fn playlist_detailed() -> Z {
    obj(vec![("type", Lit("PLAYLIST")), ("playlistId", Str), ("name", Str), ("artist", artist_basic()), ("thumbnails", arr(thumb()))])
}
pub fn song_full() -> Z {
    obj(vec![
        ("type", Lit("SONG")),
        ("videoId", Str),
        ("name", Str),
        ("artist", artist_basic()),
        ("duration", Num),
        ("thumbnails", arr(thumb())),
        ("formats", arr(Any)),
        ("adaptiveFormats", arr(Any)),
    ])
}
pub fn artist_full() -> Z {
    obj(vec![
        ("artistId", Str),
        ("name", Str),
        ("type", Lit("ARTIST")),
        ("thumbnails", arr(thumb())),
        ("topSongs", arr(song_detailed())),
        ("topAlbums", arr(album_detailed())),
        ("topSingles", arr(album_detailed())),
        ("topVideos", arr(video_detailed())),
        ("featuredOn", arr(playlist_detailed())),
        ("similarArtists", arr(artist_detailed())),
    ])
}
pub fn album_full() -> Z {
    obj(vec![
        ("type", Lit("ALBUM")),
        ("albumId", Str),
        ("playlistId", Str),
        ("name", Str),
        ("artist", artist_basic()),
        ("year", null(Num)),
        ("thumbnails", arr(thumb())),
        ("songs", arr(song_detailed())),
    ])
}
pub fn playlist_full() -> Z {
    obj(vec![("type", Lit("PLAYLIST")), ("playlistId", Str), ("name", Str), ("artist", artist_basic()), ("videoCount", Num), ("thumbnails", arr(thumb()))])
}

/// zod's `getParsedType`.
fn parsed_type(v: &JsValue) -> &'static str {
    match v {
        JsValue::Undefined => "undefined",
        JsValue::String(_) => "string",
        JsValue::Number(n) if n.is_nan() => "nan",
        JsValue::Number(_) => "number",
        JsValue::Bool(_) => "boolean",
        JsValue::BigInt(_) => "bigint",
        JsValue::Null => "null",
        JsValue::Array(_) => "array",
        JsValue::Date(_) => "date",
        JsValue::Map(_) => "map",
        JsValue::Set(_) => "set",
        JsValue::Shared(_, inner) => parsed_type(inner),
        _ => "object",
    }
}

fn issue(fields: Vec<(&str, JsValue)>, path: &[JsValue], message: String) -> JsValue {
    let mut f = fields;
    f.push(("path", JsValue::Array(path.to_vec())));
    f.push(("message", message.into()));
    JsValue::object(f)
}

fn invalid_type(expected: &str, v: &JsValue, path: &[JsValue], out: &mut Vec<JsValue>) {
    let received = parsed_type(v);
    let message = if received == "undefined" { "Required".to_owned() } else { format!("Expected {expected}, received {received}") };
    out.push(issue(vec![("code", "invalid_type".into()), ("expected", expected.into()), ("received", received.into())], path, message));
}

fn check(v: &JsValue, z: &Z, path: &mut Vec<JsValue>, out: &mut Vec<JsValue>) {
    match z {
        Any => {}
        Str => {
            if parsed_type(v) != "string" {
                invalid_type("string", v, path, out);
            }
        }
        Num => {
            if parsed_type(v) != "number" {
                invalid_type("number", v, path, out);
            }
        }
        Bool => {
            if parsed_type(v) != "boolean" {
                invalid_type("boolean", v, path, out);
            }
        }
        Lit(want) => {
            if *v != JsValue::from(*want) {
                out.push(issue(
                    vec![("received", v.clone()), ("code", "invalid_literal".into()), ("expected", (*want).into())],
                    path,
                    format!("Invalid literal value, expected \"{want}\""),
                ));
            }
        }
        Nullable(inner) => {
            if *v != JsValue::Null {
                check(v, inner, path, out);
            }
        }
        Arr(item) => match v {
            JsValue::Array(items) => {
                for (i, it) in items.iter().enumerate() {
                    path.push(JsValue::Number(i as f64));
                    check(it, item, path, out);
                    path.pop();
                }
            }
            v => invalid_type("array", v, path, out),
        },
        Obj(fields) => {
            let JsValue::Object(props) = v else {
                invalid_type("object", v, path, out);
                return;
            };
            for (k, fz) in fields {
                path.push(JsValue::from(*k));
                let val = props.iter().find(|(pk, _)| *pk == JsString::from(*k)).map(|(_, v)| v.clone()).unwrap_or_default();
                check(&val, fz, path, out);
                path.pop();
            }
            let extra: Vec<JsValue> =
                props.iter().filter(|(k, _)| !fields.iter().any(|(f, _)| JsString::from(*f) == *k)).map(|(k, _)| JsValue::String(k.clone())).collect();
            if !extra.is_empty() {
                let list: Vec<String> = extra.iter().map(|k| format!("'{}'", ipc::to_js_string(k))).collect();
                out.push(issue(vec![("code", "unrecognized_keys".into()), ("keys", JsValue::Array(extra))], path, format!("Unrecognized key(s) in object: {}", list.join(", "))));
            }
        }
    }
}

static SCHEMAS: LazyLock<serde_json::Value> = LazyLock::new(|| serde_json::from_str(include_str!("schemas.json")).expect("schemas.json"));

/// `checkType(data, schema)`: logs on a mismatch, returns the data.
pub fn check_type(data: JsValue, z: &Z, schema_name: &str) -> JsValue {
    let mut issues = Vec::new();
    check(&data, z, &mut Vec::new(), &mut issues);
    if !issues.is_empty() {
        let report = JsValue::object([
            ("data", data.clone()),
            ("schema", JsValue::from(&SCHEMAS[schema_name])),
            ("error", JsValue::object([("issues", JsValue::Array(issues)), ("name", "ZodError".into())])),
        ]);
        let text = ipc::json_stringify_indent(&report, "  ").ok().flatten().unwrap_or_default();
        logger::error(&["Invalid data type, please report to https://github.com/zS1L3NT/ts-npm-ytmusic-api/issues/new/choose".into(), text.as_str().into()]);
    }
    data
}
