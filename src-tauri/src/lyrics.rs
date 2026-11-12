//! Port of `src/main/lyrics.js`: LRCLIB `/get` lookups with in-flight de-duplication (`_lyricsInflight`).
//!
//! Values from the renderer and from API JSON stay `JsValue`s. Every
//! failure is caught and becomes `null`, so those paths return `Err(())` without a message.

use crate::{
    host::{FetchRequest, FetchResponse, Host},
    ipc::{self, HandlerResult, JsString, JsValue, Thrown, to_js_string},
    js,
    logger::{self, LogArg},
};
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use regex::Regex;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    };

const LRCLIB: &str = "https://lrclib.net/api";

/// A throw inside the library's `try` (always swallowed).
type Caught<T> = Result<T, ()>;

// ─── JS value helpers ───

/// `v.key` (throws on null/undefined).
fn get(v: &JsValue, key: &str) -> Caught<JsValue> {
    ipc::get_prop(v, key).map_err(|_| ())
}

/// `v?.key`.
fn opt(v: &JsValue, key: &str) -> Caught<JsValue> {
    match v {
        JsValue::Undefined | JsValue::Null => Ok(JsValue::Undefined),
        v => get(v, key),
    }
}

/// `v?.a?.b?.c…`.
fn chain(v: &JsValue, keys: &[&str]) -> JsValue {
    keys.iter().fold(v.clone(), |acc, k| opt(&acc, k).unwrap_or(JsValue::Undefined))
}

/// UTF-16 of `String(v)` (template literal / `String()` conversion).
fn units(v: &JsValue) -> Vec<u16> {
    match v {
        JsValue::String(s) => s.0.clone(),
        JsValue::Shared(_, inner) => units(inner),
        v => to_js_string(v).encode_utf16().collect(),
    }
}

/// `application/x-www-form-urlencoded` serialisation (`URLSearchParams#toString`).
pub(crate) fn form(pairs: &[(&str, String)]) -> String {
    fn enc(s: &str, out: &mut String) {
        for b in s.bytes() {
            match b {
                b' ' => out.push('+'),
                b'*' | b'-' | b'.' | b'_' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => out.push(b as char),
                b => out.push_str(&format!("%{b:02X}")),
            }
        }
    }
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        enc(k, &mut out);
        out.push('=');
        enc(v, &mut out);
    }
    out
}

/// `String(v)` as a USVString (what URLSearchParams and fetch headers take).
fn usv(v: &JsValue) -> String {
    String::from_utf16_lossy(&units(v))
}

/// `v || ""` then `String()`.
fn or_empty(v: &JsValue) -> String {
    if v.truthy() { usv(v) } else { String::new() }
}

/// `res.json()`: BOM-stripped, lossily decoded UTF-8 through `JSON.parse`.
fn json(res: &FetchResponse) -> Caught<JsValue> {
    let body = res.body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&res.body);
    let text: Vec<u16> = String::from_utf8_lossy(body).encode_utf16().collect();
    js::json_parse(&text).map_err(|_| ())
}

// ─── JS regexes (`\s`, `.` and ASCII-only `/i` as JS defines them) ───

const S: &str = r"[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";
const DOT: &str = r"[^\n\r\u{2028}\u{2029}]";

/// A pattern with `\s` and `.` swapped for JS's classes and ASCII letters made
/// case-insensitive the way non-Unicode `/i` folds them.
fn js_re(p: &str) -> Regex {
    let mut out = String::new();
    let mut chars = p.chars().peekable();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let n = chars.next().unwrap();
                match n {
                    's' if !in_class => out.push_str(S),
                    's' => out.push_str(&S[1..S.len() - 1]),
                    'b' => out.push_str(r"(?-u:\b)"),
                    n => {
                        out.push('\\');
                        out.push(n);
                    }
                }
            }
            '[' if !in_class => {
                in_class = true;
                out.push(c);
            }
            ']' if in_class => {
                in_class = false;
                out.push(c);
            }
            '.' if !in_class => out.push_str(DOT),
            c if c.is_ascii_alphabetic() && !in_class => {
                out.push_str(&format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase()));
            }
            c if c.is_ascii_alphabetic() => {
                out.push(c.to_ascii_lowercase());
                out.push(c.to_ascii_uppercase());
            }
            c => out.push(c),
        }
    }
    Regex::new(&out).expect("valid translated regex")
}

static FEAT_PAREN: LazyLock<Regex> = LazyLock::new(|| js_re(r"\s*\((feat\.|ft\.|featuring)[^)]+\)"));
static FEAT_BRACKET: LazyLock<Regex> = LazyLock::new(|| js_re(r"\s*\[(feat\.|ft\.|featuring)[^\]]+\]"));
static DASH_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| js_re(r"\s*-\s*(remaster(ed)?|radio edit|live|mono|stereo|explicit)\b.*$"));
static PAREN_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| js_re(r"\s*\((remaster(ed)?|radio edit|live|mono|stereo|explicit)[^)]+\)"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| js_re(r"\s+"));
static ARTIST_SPLIT: LazyLock<Regex> = LazyLock::new(|| js_re(r",|&| x | feat\.| ft\.| featuring "));

/// `String(value || '')`.
fn string_or_empty(v: &JsValue) -> String {
    if v.truthy() { usv(v) } else { String::new() }
}

/// `normalizeTitle(value)`.
pub fn normalize_title(v: &JsValue) -> String {
    let s = string_or_empty(v);
    let s = FEAT_PAREN.replace_all(&s, "");
    let s = FEAT_BRACKET.replace_all(&s, "");
    let s = DASH_SUFFIX.replace_all(&s, "");
    let s = PAREN_SUFFIX.replace_all(&s, "");
    js::trim(&SPACES.replace_all(&s, " ")).to_owned()
}

/// `normalizeArtist(value)`.
pub fn normalize_artist(v: &JsValue) -> String {
    let raw = string_or_empty(v);
    let raw = js::trim(&raw);
    if raw.is_empty() {
        return String::new();
    }
    let first = ARTIST_SPLIT.split(raw).next().unwrap_or_default();
    js::trim(&SPACES.replace_all(first, " ")).to_owned()
}

/// `normalizeAlbum(value)`.
pub fn normalize_album(v: &JsValue) -> String {
    js::trim(&SPACES.replace_all(&string_or_empty(v), " ")).to_owned()
}

/// `mapLyricsPayload(data)`.
fn map_payload(data: &JsValue) -> JsValue {
    if !data.truthy() {
        return JsValue::Null;
    }
    let or = |a: JsValue, b: JsValue| if a.truthy() { a } else { b };
    let synced = or(chain(data, &["lyrics", "lineSynced", "lyrics"]), or(opt(data, "syncedLyrics").unwrap_or_default(), JsValue::Null));
    let plain = or(chain(data, &["lyrics", "plain", "lyrics"]), or(opt(data, "plainLyrics").unwrap_or_default(), JsValue::Null));
    let source = or(
        chain(data, &["lyrics", "lineSynced", "source"]),
        or(chain(data, &["lyrics", "plain", "source"]), or(opt(data, "source").unwrap_or_default(), JsValue::from("Unknown"))),
    );
    if !synced.truthy() && !plain.truthy() {
        return JsValue::Null;
    }
    JsValue::object([("synced", synced), ("plain", plain), ("source", source)])
}

// ─── lyrics.js ───

/// One `{ track, artist, album, length }` attempt.
#[derive(Clone, Debug)]
struct Query {
    track: JsValue,
    artist: JsValue,
    album: JsValue,
    length: JsValue,
}

impl Query {
    fn to_js(&self) -> JsValue {
        JsValue::object([("track", self.track.clone()), ("artist", self.artist.clone()), ("album", self.album.clone()), ("length", self.length.clone())])
    }
}

type Inflight = Shared<BoxFuture<'static, HandlerResult>>;

/// The module's state: `_lyricsInflight`.
#[derive(Default)]
pub struct Lyrics {
    inflight: Mutex<HashMap<String, Inflight>>,
}

async fn fetch<H: Host>(host: &H, url: String, headers: &[(&str, String)], manual: bool) -> Caught<FetchResponse> {
    let headers = headers.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
    host.fetch(FetchRequest { method: "GET", body: None, url, headers, manual_redirect: manual }).await
}

/// lyrics.js's own `fetchViaLrclib(query)` against `/get`.
async fn via_lrclib<H: Host>(host: &H, q: &Query) -> Caught<JsValue> {
    let mut params = vec![("track_name", or_empty(&q.track)), ("artist_name", or_empty(&q.artist)), ("album_name", or_empty(&q.album))];
    if let JsValue::Number(n) = q.length
        && n > 0.0
    {
        params.push(("duration", js::number_to_string(js::math_round(n / 1000.0))));
    }
    let res = fetch(host, format!("{LRCLIB}/get?{}", form(&params)), &[("Lrclib-Client", "Snowify Desktop".into())], false).await?;
    if !res.ok() {
        return Ok(JsValue::Null);
    }
    let data = json(&res)?;
    if !data.truthy() {
        return Ok(JsValue::Null);
    }
    // `{ ...data, source: 'lrclib' }`: only an object contributes the named keys read later.
    let mut props = match &data {
        JsValue::Object(p) => p.clone(),
        _ => Vec::new(),
    };
    props.retain(|(k, _)| *k != JsString::from("source"));
    props.push(("source".into(), "lrclib".into()));
    Ok(map_payload(&JsValue::Object(props)))
}

/// `resolveLyrics(trackName, artistName, albumName, durationSec)`.
async fn resolve<H: Host>(host: &H, args: [JsValue; 4]) -> HandlerResult {
    let [track, artist, album, dur] = args;
    let (nt, na, nal) = (normalize_title(&track), normalize_artist(&artist), normalize_album(&album));
    let length = if dur.truthy() { JsValue::Number(js::math_round(ipc::to_number(&dur)? * 1000.0)) } else { JsValue::Undefined };
    let or = |v: &JsValue| if v.truthy() { v.clone() } else { JsValue::from("") };
    let q = |t: JsValue, a: JsValue, al: JsValue, len: JsValue| Query { track: t, artist: a, album: al, length: len };
    let all = [
        q(or(&track), or(&artist), or(&album), length.clone()),
        q(nt.as_str().into(), or(&artist), or(&album), length.clone()),
        q(nt.as_str().into(), na.as_str().into(), nal.as_str().into(), length.clone()),
        q(nt.as_str().into(), na.as_str().into(), "".into(), length),
        q(nt.as_str().into(), na.as_str().into(), "".into(), JsValue::Undefined),
    ];
    // `.filter(...)`: the dedupe compares JSON.stringify of every earlier entry (which can
    // throw on a BigInt), filtered or not.
    let mut attempts = Vec::new();
    for (i, a) in all.iter().enumerate() {
        if !a.track.truthy() || !a.artist.truthy() {
            continue;
        }
        let me = ipc::json_stringify(&a.to_js()).map_err(|_| Thrown::type_error("Do not know how to serialize a BigInt"))?;
        let mut first = i;
        for (j, b) in all.iter().enumerate().take(i + 1) {
            let other = ipc::json_stringify(&b.to_js()).map_err(|_| Thrown::type_error("Do not know how to serialize a BigInt"))?;
            if other == me {
                first = j;
                break;
            }
        }
        if first == i {
            attempts.push(a.clone());
        }
    }
    for a in &attempts {
        if let Ok(r) = via_lrclib(host, a).await
            && r.truthy()
        {
            return Ok(r);
        }
    }
    Ok(JsValue::Null)
}

/// `Math.round(x)` on an arbitrary value: a BigInt can't become a number.
fn round_value(v: &JsValue) -> Result<f64, Thrown> {
    let big = match v {
        JsValue::BigInt(_) => true,
        JsValue::Boxed(b) | JsValue::Shared(_, b) => matches!(**b, JsValue::BigInt(_)),
        _ => false,
    };
    if big {
        return Err(Thrown::type_error("Cannot convert a BigInt value to a number"));
    }
    Ok(js::math_round(ipc::to_number(v)?))
}

/// `ipcMain.handle('lyrics:get', (_e, trackName, artistName, albumName, durationSec) => …)`.
pub async fn handle<H: Host + 'static>(lyr: Arc<Lyrics>, host: Arc<H>, args: [JsValue; 4]) -> HandlerResult {
    let dur = &args[3];
    let n = round_value(&if dur.truthy() { dur.clone() } else { JsValue::Number(0.0) })?;
    let key = format!("{}|{}|{}", normalize_title(&args[0]), normalize_artist(&args[1]), js::number_to_string(n));
    let fut = {
        let mut inflight = lyr.inflight.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = inflight.get(&key) {
            f.clone()
        } else {
            let (lyr2, key2) = (lyr.clone(), key.clone());
            let f: BoxFuture<'static, HandlerResult> = async move {
                let r = match resolve(&*host, args).await {
                    Ok(v) => v,
                    Err(e) => {
                        let msg = match &e {
                            Thrown::Error { message, .. } => JsValue::String(message.clone()),
                            Thrown::Value(v) => ipc::get_prop(v, "message").unwrap_or_default(),
                        };
                        logger::error(&["Lyrics fetch error:".into(), LogArg::from_js(&msg)]);
                        JsValue::Null
                    }
                };
                lyr2.inflight.lock().unwrap_or_else(|e| e.into_inner()).remove(&key2);
                Ok(r)
            }
            .boxed();
            let f = f.shared();
            inflight.insert(key, f.clone());
            f
        }
    };
    fut.await
}
