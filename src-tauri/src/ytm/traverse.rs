//! ytmusic-api's `traverse`, `traverseList`, `traverseString` and the `is*` column tests.
//!
//! `traverse` collects every value under `key` anywhere below the data (one level of
//! results flattened per step, a single result unwrapped, none as `[]`), stopping at an
//! object that holds the *last* key. The quirks of that encoding (a found array value is
//! indistinguishable from a list of results) are part of the behaviour and kept.

use crate::ipc::{JsString, JsValue};

/// A property marking the axios response object `constructRequest` returns when the body
/// has no `responseContext`: walking it reaches the cyclic `ClientRequest` and overflows
/// the stack.
pub const AXIOS_RESPONSE: &str = "\u{0}axios-response";

/// `RangeError: Maximum call stack size exceeded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overflow;

/// `data2 instanceof Object && key in data2` (`Object.prototype` keys never occur here).
fn own(v: &JsValue, key: &str) -> Option<JsValue> {
    match v {
        JsValue::Object(props) => props.iter().find(|(k, _)| *k == JsString::from(key)).map(|(_, v)| v.clone()),
        JsValue::Array(items) if key == "length" => Some(JsValue::Number(items.len() as f64)),
        _ => None,
    }
}

fn single_or_list(mut res: Vec<JsValue>) -> JsValue {
    if res.len() == 1 { res.pop().unwrap() } else { JsValue::Array(res) }
}

/// `.flat()` of one more result into `res`.
fn push_flat(res: &mut Vec<JsValue>, v: JsValue) {
    match v {
        JsValue::Array(items) => res.extend(items),
        v => res.push(v),
    }
}

fn again(data: &JsValue, key: &str, dead_end: bool) -> Result<JsValue, Overflow> {
    let mut res = Vec::new();
    if let Some(v) = own(data, key) {
        res.push(v);
        if dead_end {
            return Ok(single_or_list(res));
        }
    }
    match data {
        JsValue::Array(items) => {
            for v in items {
                push_flat(&mut res, again(v, key, false)?);
            }
        }
        JsValue::Object(props) => {
            if props.iter().any(|(k, _)| *k == JsString::from(AXIOS_RESPONSE)) {
                return Err(Overflow);
            }
            for (_, v) in props {
                push_flat(&mut res, again(v, key, false)?);
            }
        }
        _ => {}
    }
    Ok(single_or_list(res))
}

/// `traverse(data, ...keys)`.
pub fn traverse(data: &JsValue, keys: &[&str]) -> Result<JsValue, Overflow> {
    let last = keys.last().copied();
    let mut value = data.clone();
    for key in keys {
        value = again(&value, key, Some(*key) == last)?;
    }
    Ok(value)
}

/// `traverseList(data, ...keys)`: `[traverse(...)].flat()`.
pub fn traverse_list(data: &JsValue, keys: &[&str]) -> Result<Vec<JsValue>, Overflow> {
    Ok(match traverse(data, keys)? {
        JsValue::Array(items) => items,
        v => vec![v],
    })
}

/// `traverseString(data, ...keys)`: the first value if truthy, else `""` (not always a
/// string: whatever JSON value was found).
pub fn traverse_string(data: &JsValue, keys: &[&str]) -> Result<JsValue, Overflow> {
    let first = traverse_list(data, keys)?.into_iter().next().unwrap_or_default();
    Ok(if first.truthy() { first } else { JsValue::from("") })
}

/// A thrown error inside a parser: the stack overflow, or a TypeError from calling a
/// string method on a non-string.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Overflow,
    TypeError,
}

impl From<Overflow> for ParseError {
    fn from(_: Overflow) -> Self {
        Self::Overflow
    }
}

/// `.startsWith`/`.match` on what `traverseString` returned.
pub fn as_str(v: &JsValue) -> Result<String, ParseError> {
    match v {
        JsValue::String(s) => Ok(s.to_string_lossy()),
        _ => Err(ParseError::TypeError),
    }
}

/// `isTitle(data)`.
pub fn is_title(data: &JsValue) -> Result<bool, ParseError> {
    Ok(as_str(&traverse_string(data, &["musicVideoType"])?)?.starts_with("MUSIC_VIDEO_TYPE_"))
}

/// `isArtist(data)`: `[...].includes(x)` compares strictly, so a non-string is just false.
pub fn is_artist(data: &JsValue) -> Result<bool, ParseError> {
    let t = traverse_string(data, &["pageType"])?;
    Ok(t == JsValue::from("MUSIC_PAGE_TYPE_USER_CHANNEL") || t == JsValue::from("MUSIC_PAGE_TYPE_ARTIST"))
}

/// `isAlbum(data)`.
pub fn is_album(data: &JsValue) -> Result<bool, ParseError> {
    Ok(traverse_string(data, &["pageType"])? == JsValue::from("MUSIC_PAGE_TYPE_ALBUM"))
}

/// `isDuration(data)`: `/(\d{1,2}:)?\d{1,2}:\d{1,2}/` anywhere in the text.
pub fn is_duration(data: &JsValue) -> Result<bool, ParseError> {
    let s = as_str(&traverse_string(data, &["text"])?)?;
    let b = s.as_bytes();
    // Any "d{1,2}:d{1,2}" (the optional group can't change whether something matches).
    Ok((0..b.len()).any(|i| {
        let digits = |at: usize| b[at..].iter().take_while(|c| c.is_ascii_digit()).count();
        let d1 = digits(i);
        d1 >= 1 && b.get(i + d1) == Some(&b':') && digits(i + d1 + 1) >= 1
    }))
}

/// `columns.find(pred)`.
pub fn find(cols: &[JsValue], pred: fn(&JsValue) -> Result<bool, ParseError>) -> Result<Option<JsValue>, ParseError> {
    for c in cols {
        if pred(c)? {
            return Ok(Some(c.clone()));
        }
    }
    Ok(None)
}
