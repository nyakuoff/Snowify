//! Port of `src/main/lyrics.js` and the parts of `@stef-0012/synclyrics` 2.5.10 it drives
//! (Musixmatch desktop API, LRCLIB search, Netease), with the library's shared state: one
//! `_fetching` flag (a lookup that starts while another runs gets an empty result, which is
//! cached), the 50-entry cache proxy, and a Musixmatch token that never expires.
//!
//! Values from the renderer and from API JSON stay `JsValue`s. Inside the library every
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
    time::Duration,
};

const LRCLIB: &str = "https://lrclib.net/api";
const MXM: &str = "https://apic-desktop.musixmatch.com/ws/1.1";
const NETEASE: &str = "https://music.xianqiao.wang/neteaseapiv2";
const NETEASE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:93.0) Gecko/20100101 Firefox/93.0";
const SYNCLYRICS_UA: &str = "SyncLyrics (https://github.com/Stef-00012/SyncLyrics)";
const CACHE_MAX: usize = 50;

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

fn nullish(v: &JsValue) -> bool {
    matches!(v, JsValue::Undefined | JsValue::Null)
}

/// UTF-16 of `String(v)` (template literal / `String()` conversion).
fn units(v: &JsValue) -> Vec<u16> {
    match v {
        JsValue::String(s) => s.0.clone(),
        JsValue::Shared(_, inner) => units(inner),
        v => to_js_string(v).encode_utf16().collect(),
    }
}

/// `String#toLowerCase` on UTF-16: valid stretches use full Unicode lowercasing (final sigma
/// included), lone surrogates stay.
fn lower(u: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(u.len());
    let mut seg = String::new();
    for c in char::decode_utf16(u.iter().copied()) {
        match c {
            Ok(c) => seg.push(c),
            Err(e) => {
                out.extend(seg.to_lowercase().encode_utf16());
                seg.clear();
                out.push(e.unpaired_surrogate());
            }
        }
    }
    out.extend(seg.to_lowercase().encode_utf16());
    out
}

/// `v?.toLowerCase()`: undefined when nullish, a TypeError for a non-string.
fn opt_lower(v: &JsValue) -> Caught<Option<Vec<u16>>> {
    match v {
        JsValue::Undefined | JsValue::Null => Ok(None),
        JsValue::String(s) => Ok(Some(lower(&s.0))),
        JsValue::Shared(_, inner) => opt_lower(inner),
        _ => Err(()),
    }
}

/// `hay.includes(needle)` with `needle` already `String()`-converted.
fn includes(hay: &[u16], needle: &[u16]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

/// The search string `includes` gets for `x?.toLowerCase()` (undefined → "undefined").
fn needle(v: Option<Vec<u16>>) -> Vec<u16> {
    v.unwrap_or_else(|| "undefined".encode_utf16().collect())
}

/// `0 >= v` (v = some `.length`).
fn zero_ge(v: &JsValue) -> bool {
    let n = match v {
        JsValue::Object(_) | JsValue::Map(_) | JsValue::Set(_) => f64::NAN,
        v => ipc::to_number(v).unwrap_or(f64::NAN),
    };
    0.0 >= n
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

/// `n?.message?.header?.status_code === 401 && …hint === "captcha"`.
fn captcha(n: &JsValue) -> bool {
    chain(n, &["message", "header", "status_code"]) == JsValue::Number(401.0)
        && chain(n, &["message", "header", "hint"]) == JsValue::from("captcha")
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
static NETEASE_CREDIT: LazyLock<Regex> = LazyLock::new(|| {
    js_re(concat!(
        r"^(\s?作?\s*词|\s?作?\s*曲|\s?编\s*曲?|\s?监\s*制?|.*编写|.*和音|.*和声|.*合声|.*提琴|.*录|.*工程|.*工作室|.*设计|.*剪辑|.*制作|.*发行|.*出品|.*后期|.*混音|.*缩混|",
        r"原唱|翻唱|题字|文案|海报|古筝|二胡|钢琴|吉他|贝斯|笛子|鼓|弦乐| 人声 |lrc|publish|vocal|guitar|program|produce|write|mix).*(:|：)"
    ))
});

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

// ─── synclyrics ───

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

/// One `{ source, lyrics }` slot of the result object.
#[derive(Clone, Debug, Default)]
struct Slot {
    source: JsValue,
    lyrics: JsValue,
}

#[derive(Clone, Debug, Default)]
struct Found {
    plain: Slot,
    line: Slot,
    word: Slot,
}

impl Found {
    fn empty() -> Self {
        let s = Slot { source: JsValue::Null, lyrics: JsValue::Null };
        Self { plain: s.clone(), line: s.clone(), word: s }
    }

    /// What `getLyrics` resolves with, as far as `mapLyricsPayload` reads it.
    fn to_js(&self) -> JsValue {
        let slot = |s: &Slot| JsValue::object([("lyrics", s.lyrics.clone()), ("source", s.source.clone())]);
        JsValue::object([(
            "lyrics",
            JsValue::object([("plain", slot(&self.plain)), ("lineSynced", slot(&self.line)), ("wordSynced", slot(&self.word))]),
        )])
    }
}

/// A source's `{ source, plain, lineSynced, wordSynced }` (missing fields are undefined).
#[derive(Default)]
struct Partial {
    source: JsValue,
    plain: JsValue,
    line: JsValue,
    word: JsValue,
}

#[derive(Clone)]
struct Token {
    cookies: JsValue,
    usertoken: JsValue,
}

#[derive(Default)]
struct State {
    /// `lyricsCacheProxy`: insertion-ordered, evicts the oldest at 50 even on an update.
    cache: Vec<(String, Found)>,
    fetching: bool,
    token: Option<Token>,
    /// `this._trackId`: one field on the shared instance, so a lookup that starts while
    /// another awaits redirects where the earlier one stores its result.
    track_id: String,
}

type Inflight = Shared<BoxFuture<'static, HandlerResult>>;

/// The module's state: the synclyrics instance and `_lyricsInflight`.
#[derive(Default)]
pub struct Lyrics {
    state: Mutex<State>,
    inflight: Mutex<HashMap<String, Inflight>>,
}

impl Lyrics {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cache_get(&self, key: &str) -> Option<Found> {
        self.state().cache.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn cache_set(&self, key: String, value: Found) {
        let mut s = self.state();
        if s.cache.len() >= CACHE_MAX {
            s.cache.remove(0);
        }
        match s.cache.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = value,
            None => s.cache.push((key, value)),
        }
    }
}

async fn fetch<H: Host>(host: &H, url: String, headers: &[(&str, String)], manual: bool) -> Caught<FetchResponse> {
    let headers = headers.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
    host.fetch(FetchRequest { method: "GET", body: None, url, headers, manual_redirect: manual }).await
}

/// `getMusixmatchUsertoken(cookies)`: `Ok(None)` is a null/undefined result.
async fn mxm_token<H: Host>(lyr: &Lyrics, host: &H, cookies: JsValue) -> Option<Token> {
    let mut cookies = cookies;
    loop {
        if let Some(t) = lyr.state().token.clone() {
            return Some(t);
        }
        let res = fetch(
            host,
            format!("{MXM}/token.get?user_language=en&app_id=web-desktop-app-v1.0"),
            &[("cookie", usv(&cookies))],
            true,
        )
        .await
        .ok()?;
        if res.status == 301 {
            let jar: Vec<String> = res
                .set_cookies()
                .iter()
                .map(|c| c.split(';').next().unwrap_or_default().to_owned())
                .filter(|c| c.rsplit('=').next() != Some("unknown"))
                .collect();
            cookies = JsValue::from(jar.join("; "));
            continue;
        }
        if !res.ok() {
            return None;
        }
        let n = json(&res).ok()?;
        if captcha(&n) {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        }
        let p = chain(&n, &["message", "body", "user_token"]);
        if !p.truthy() {
            return None;
        }
        let t = Token { cookies, usertoken: p };
        lyr.state().token = Some(t.clone());
        return Some(t);
    }
}

struct MxmTrack {
    common: JsValue,
    track: JsValue,
    has_lyrics: JsValue,
    has_line: JsValue,
    has_word: JsValue,
}

/// `(F.track.track_name?.toLowerCase() === a.track?.toLowerCase()) && F.track.artist_name
/// ?.toLowerCase().includes(a.artist?.toLowerCase())`, with the same key names per source.
fn same_track(name: &JsValue, artist: &JsValue, q: &Query) -> Caught<bool> {
    if opt_lower(name)? != opt_lower(&q.track)? {
        return Ok(false);
    }
    Ok(match opt_lower(artist)? {
        None => false,
        Some(a) => includes(&a, &needle(opt_lower(&q.artist)?)),
    })
}

/// `arr?.find(pred)` on a JSON value: a non-array (non-nullish) throws.
fn find(arr: &JsValue, mut pred: impl FnMut(&JsValue) -> Caught<bool>) -> Caught<JsValue> {
    match arr {
        JsValue::Undefined | JsValue::Null => Ok(JsValue::Undefined),
        JsValue::Array(items) => {
            for it in items {
                if pred(it)? {
                    return Ok(it.clone());
                }
            }
            Ok(JsValue::Undefined)
        }
        _ => Err(()),
    }
}

async fn mxm_search<H: Host>(host: &H, q: &Query, t: &Token) -> Caught<Option<MxmTrack>> {
    let secs = match &q.length {
        JsValue::Number(n) if *n == 0.0 => 0.0,
        JsValue::Number(n) => *n / 1000.0,
        _ => f64::NAN,
    };
    let dur = if secs.is_nan() || secs == 0.0 { String::new() } else { js::number_to_string(secs) };
    let url = format!(
        "{MXM}/track.search?{}",
        form(&[
            ("app_id", "web-desktop-app-v1.0".into()),
            ("usertoken", usv(&t.usertoken)),
            ("q_track", or_empty(&q.track)),
            ("q_artist", or_empty(&q.artist)),
            ("q_album", or_empty(&q.album)),
            ("page_size", "20".into()),
            ("page", "1".into()),
            ("q_duration", dur),
            ("s_track_rating", "asc".into()),
        ])
    );
    let res = fetch(host, url, &[("cookie", usv(&t.cookies))], false).await?;
    if !res.ok() {
        return Ok(None);
    }
    let u = json(&res)?;
    if captcha(&u) || zero_ge(&chain(&u, &["message", "body", "track_list", "length"])) {
        return Ok(None);
    }
    let found = find(&chain(&u, &["message", "body", "track_list"]), |f| {
        let tr = get(f, "track")?;
        same_track(&get(&tr, "track_name")?, &get(&tr, "artist_name")?, q)
    })?;
    if !found.truthy() {
        return Ok(None);
    }
    let tr = opt(&found, "track")?;
    let field = |k: &str| opt(&tr, k).unwrap_or_default();
    let m = MxmTrack {
        common: field("commontrack_id"),
        track: field("track_id"),
        has_lyrics: field("has_lyrics"),
        has_line: field("has_subtitles"),
        has_word: field("has_richsync"),
    };
    if !m.has_lyrics.truthy() && !m.has_line.truthy() && !m.has_word.truthy() {
        return Ok(None);
    }
    Ok(Some(m))
}

/// One of the three `track.*.get` calls: the body value at `path`, or null.
async fn mxm_get<H: Host>(host: &H, t: &Token, method: &str, id_key: &str, id: &JsValue, path: &[&str]) -> Caught<JsValue> {
    if !id.truthy() {
        return Ok(JsValue::Null);
    }
    let url = format!("{MXM}/{method}?{}", form(&[("app_id", "web-desktop-app-v1.0".into()), ("usertoken", usv(&t.usertoken)), (id_key, usv(id))]));
    let res = fetch(host, url, &[("cookie", usv(&t.cookies))], false).await?;
    if !res.ok() {
        return Ok(JsValue::Null);
    }
    let h = json(&res)?;
    if captcha(&h) {
        return Ok(JsValue::Null);
    }
    let q = chain(&h, path);
    Ok(if q.truthy() { q } else { JsValue::Null })
}

/// The richsync body is parsed and mapped; only whether that works is ever observable (the
/// word-synced lyrics never reach `lyrics.js`), so a stand-in truthy value is kept.
fn parse_richsync(q: &JsValue) -> Caught<JsValue> {
    if zero_ge(&opt(q, "length")?) {
        return Ok(JsValue::Null);
    }
    let parsed = js::json_parse(&units(q)).map_err(|_| ())?;
    let JsValue::Array(lines) = parsed else { return Err(()) };
    for r in &lines {
        for k in ["ts", "te", "x"] {
            get(r, k)?;
        }
        let JsValue::Array(chars) = get(r, "l")? else { return Err(()) };
        for x in &chars {
            get(x, "c")?;
            get(x, "o")?;
        }
    }
    Ok(JsValue::Bool(true))
}

/// `fetchLyricsMusixmatch(a, types)`.
async fn musixmatch<H: Host>(lyr: &Lyrics, host: &H, q: &Query) -> Option<Partial> {
    let t = mxm_token(lyr, host, JsValue::Undefined).await?;
    let c = mxm_search(host, q, &t).await.ok().flatten()?;
    if !c.common.truthy() && !c.track.truthy() {
        return None;
    }
    let mut p = Partial { source: "Musixmatch".into(), ..Default::default() };
    // `_fetchLyricsMusixmatch`'s own guard.
    if !c.common.truthy() && !c.track.truthy() {
        return Some(p);
    }
    let sub = |r: Caught<JsValue>| r.unwrap_or(JsValue::Null);
    let (mut plain, mut line, mut word) = (JsValue::Null, JsValue::Null, JsValue::Null);
    if c.has_lyrics.truthy() && c.common.truthy() {
        plain = sub(mxm_get(host, &t, "track.lyrics.get", "commontrack_id", &c.common, &["message", "body", "lyrics", "lyrics_body"]).await);
    }
    if c.has_line.truthy() && c.common.truthy() {
        line = sub(mxm_get(host, &t, "track.subtitle.get", "commontrack_id", &c.common, &["message", "body", "subtitle", "subtitle_body"]).await);
    }
    if c.has_word.truthy() && c.common.truthy() {
        let body = mxm_get(host, &t, "track.richsync.get", "track_id", &c.track, &["message", "body", "richsync", "richsync_body"]).await;
        word = sub(body.and_then(|b| if b.truthy() { parse_richsync(&b) } else { Ok(JsValue::Null) }));
    }
    p.plain = plain;
    p.line = line;
    p.word = word;
    Some(p)
}

/// `fetchLyricsLrclib(a, types)`.
async fn lrclib_search<H: Host>(host: &H, q: &Query) -> Caught<Option<Partial>> {
    let params = form(&[("track_name", usv(&q.track)), ("artist_name", usv(&q.artist)), ("album_name", usv(&q.album)), ("q", usv(&q.track))]);
    let res = fetch(host, format!("https://lrclib.net/api/search?{params}"), &[("Lrclib-Client", SYNCLYRICS_UA.into()), ("User-Agent", SYNCLYRICS_UA.into())], false).await?;
    if !res.ok() {
        return Ok(None);
    }
    let g = json(&res)?;
    let JsValue::Array(_) = &g else { return Err(()) };
    let k = find(&g, |l| {
        let artist = get(l, "artistName")?;
        let ok = match opt_lower(&artist)? {
            None => false,
            Some(a) => includes(&a, &needle(opt_lower(&q.artist)?)),
        };
        Ok(ok && opt_lower(&get(l, "trackName")?)? == opt_lower(&q.track)?)
    })?;
    let empty = |v: &JsValue| !v.truthy() || zero_ge(&opt(v, "length").unwrap_or_default());
    let (synced, plain) = (opt(&k, "syncedLyrics")?, opt(&k, "plainLyrics")?);
    if !k.truthy() || (empty(&synced) && empty(&plain)) {
        return Ok(None);
    }
    Ok(Some(Partial { source: "lrclib.net".into(), plain, line: synced, word: JsValue::Null }))
}

/// `normalize(text)` from synclyrics (full-width punctuation, quotes, whitespace).
fn netease_normalize(u: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(u.len());
    for &c in u {
        let rep: &str = match c {
            0xFF08 => "(",
            0xFF09 => ")",
            0x3010 => "[",
            0x3011 => "]",
            0x3002 => ". ",
            0xFF1B => "; ",
            0xFF1A => ": ",
            0xFF1F => "? ",
            0xFF01 => "! ",
            0x3001 | 0xFF0C => ", ",
            0x2018 | 0x2019 | 0x2032 | 0xFF07 => "'",
            0x201C | 0x201D => "\"",
            0x301C => "~",
            0x00B7 | 0x30FB => "\u{2022}",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.extend(rep.encode_utf16());
    }
    // `.replace(/\s+/g, " ").trim()`
    let mut collapsed: Vec<u16> = Vec::with_capacity(out.len());
    for c in out {
        if is_js_space(c) {
            if collapsed.last() != Some(&0x20) || collapsed.is_empty() {
                collapsed.push(0x20);
            }
        } else {
            collapsed.push(c);
        }
    }
    trim_units(&collapsed).to_vec()
}

fn is_js_space(c: u16) -> bool {
    matches!(c, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

fn trim_units(u: &[u16]) -> &[u16] {
    let start = u.iter().position(|c| !is_js_space(*c)).unwrap_or(u.len());
    let end = u.iter().rposition(|c| !is_js_space(*c)).map_or(start, |e| e + 1);
    &u[start..end]
}

/// `line.match(/(\[.*?\])|([^\[\]]+)/g)`.
fn netease_tokens(line: &[u16]) -> Vec<Vec<u16>> {
    let term = |c: u16| matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029);
    let (open, close) = (b'[' as u16, b']' as u16);
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if line[i] == open {
            let close_at = line[i + 1..].iter().take_while(|c| !term(**c)).position(|c| *c == close);
            if let Some(off) = close_at {
                out.push(line[i..i + off + 2].to_vec());
                i += off + 2;
            } else {
                i += 1;
            }
        } else if line[i] == close {
            i += 1;
        } else {
            let end = line[i..].iter().position(|c| *c == open || *c == close).map_or(line.len(), |p| i + p);
            out.push(line[i..end].to_vec());
            i = end;
        }
    }
    out
}

/// `_parseNeteaseLyrics(text)`.
fn netease_parse(text: &[u16]) -> Vec<u16> {
    let mut lines: Vec<&[u16]> = Vec::new();
    let mut start = 0;
    for (i, &c) in text.iter().enumerate() {
        if c == 0x0A {
            let end = if i > start && text[i - 1] == 0x0D { i - 1 } else { i };
            lines.push(&text[start..end]);
            start = i + 1;
        }
    }
    lines.push(&text[start..]);
    let mut out: Vec<Vec<u16>> = Vec::new();
    for line in lines {
        let mut c = netease_tokens(trim_units(line));
        if c.len() < 2 {
            continue;
        }
        let mut b: Vec<u16> = Vec::new();
        if let Some(g) = c.iter().position(|t| t.last() != Some(&(b']' as u16))) {
            let t = c.remove(g);
            // `charAt(0).toUpperCase()`: one UTF-16 unit; a surrogate half stays as it is.
            let first = t[0];
            match char::from_u32(u32::from(first)) {
                Some(ch) => b.extend(ch.to_uppercase().collect::<String>().encode_utf16()),
                None => b.push(first),
            }
            b.extend(netease_normalize(&t[1..]));
        }
        if NETEASE_CREDIT.is_match(&String::from_utf16_lossy(&b)) {
            continue;
        }
        let mut row = c[0].clone();
        row.push(0x20);
        row.extend(&b);
        out.push(row);
    }
    out.join(&[0x0A][..])
}

/// `fetchLyricsNetease(a, types)`.
async fn netease<H: Host>(host: &H, q: &Query) -> Option<Partial> {
    let ua = [("User-Agent", NETEASE_UA.to_owned())];
    let id = async {
        let kw = format!("{} {}", usv(&q.track), usv(&q.artist));
        let url = format!("{NETEASE}/search?{}", form(&[("limit", "10".into()), ("type", "1".into()), ("keywords", kw)]));
        let res = fetch(host, url, &ua, false).await?;
        if !res.ok() {
            return Ok(JsValue::Null);
        }
        let l = json(&res)?;
        let result = opt(&l, "result")?;
        if nullish(&result) || !get(&result, "songs")?.truthy() || zero_ge(&chain(&l, &["result", "songs", "length"])) {
            return Ok(JsValue::Null);
        }
        let artist = opt_lower(&q.artist)?;
        let artist_dashless = artist.as_ref().map(|a| a.iter().map(|&c| if c == b'-' as u16 { 0x20 } else { c }).collect::<Vec<u16>>());
        let n = find(&chain(&l, &["result", "songs"]), |m| {
            if opt_lower(&get(m, "name")?)? != opt_lower(&q.track)? {
                return Ok(false);
            }
            let artists = get(m, "artists")?;
            let JsValue::Array(list) = &artists else { return Err(()) };
            for y in list {
                if let Some(name) = opt_lower(&get(y, "name")?)?
                    && includes(&name, &needle(artist.clone()))
                {
                    return Ok(true);
                }
            }
            for y in list {
                if let Some(name) = opt_lower(&get(y, "name")?)? {
                    let name: Vec<u16> = name.iter().map(|&c| if c == b'-' as u16 { 0x20 } else { c }).collect();
                    if includes(&name, &needle(artist_dashless.clone())) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        })?;
        if !n.truthy() {
            return Ok(JsValue::Null);
        }
        get(&n, "id")
    };
    let id: Caught<JsValue> = id.await;
    let id = id.unwrap_or(JsValue::Null);
    if !id.truthy() {
        return None;
    }
    let lyrics: Caught<JsValue> = async {
        let res = fetch(host, format!("{NETEASE}/lyric?{}", form(&[("id", usv(&id))])), &ua, false).await?;
        if !res.ok() {
            return Ok(JsValue::Null);
        }
        let g = chain(&json(&res)?, &["lrc", "lyric"]);
        if !g.truthy() {
            return Ok(JsValue::Null);
        }
        let JsValue::String(s) = g else { return Err(()) };
        Ok(JsValue::String(JsString(netease_parse(&s.0))))
    }
    .await;
    Some(Partial { source: "Netease".into(), line: lyrics.unwrap_or(JsValue::Null), plain: JsValue::Null, word: JsValue::Null, ..Default::default() })
}

/// `_getLyrics(query, types)` with all three types requested.
async fn sync_get<H: Host>(lyr: &Lyrics, host: &H, q: &Query) -> Found {
    let mut e = Found::empty();
    {
        let mut s = lyr.state();
        if s.fetching {
            return e;
        }
        s.fetching = true;
    }
    for source in ["musixmatch", "lrclib", "netease"] {
        let wants: &[u8] = match source {
            "musixmatch" => b"plw",
            "lrclib" => b"pl",
            _ => b"l",
        };
        let done = |t: u8| match t {
            b'p' => e.plain.lyrics.truthy(),
            b'l' => e.line.lyrics.truthy(),
            _ => e.word.lyrics.truthy(),
        };
        if wants.iter().all(|t| done(*t)) {
            continue;
        }
        let got = match source {
            "musixmatch" => musixmatch(lyr, host, q).await,
            "lrclib" => lrclib_search(host, q).await.ok().flatten(),
            _ => netease(host, q).await,
        };
        if let Some(b) = got {
            for (val, slot) in [(&b.plain, &mut e.plain), (&b.line, &mut e.line), (&b.word, &mut e.word)] {
                if val.truthy() && !slot.lyrics.truthy() {
                    slot.lyrics = val.clone();
                    slot.source = b.source.clone();
                }
            }
        }
    }
    lyr.state().fetching = false;
    e
}

/// `btoa(unescape(encodeURIComponent(text)))`; a lone surrogate throws `URIError`.
fn track_id(text: &[u16]) -> Caught<String> {
    let s = String::from_utf16(text).map_err(|_| ())?;
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for ch in s.as_bytes().chunks(3) {
        let n = ch.iter().enumerate().fold(0u32, |acc, (i, b)| acc | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= ch.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    Ok(out)
}

/// `lyricsManager.getLyrics(query)` then `mapLyricsPayload`.
async fn via_synclyrics<H: Host>(lyr: &Lyrics, host: &H, q: &Query) -> Caught<JsValue> {
    if !(q.track.truthy() || q.artist.truthy() || q.album.truthy()) {
        return Err(());
    }
    let part = |v: &JsValue| if v.truthy() { units(v) } else { Vec::new() };
    let mut text = part(&q.track);
    text.extend("----".encode_utf16());
    text.extend(part(&q.artist));
    text.extend("----".encode_utf16());
    text.extend(part(&q.album));
    let id = track_id(&text)?;
    lyr.state().track_id = id.clone();
    let c = lyr.cache_get(&id);
    let d = match &c {
        Some(c) => c.clone(),
        None => sync_get(lyr, host, q).await,
    };
    // After the await, `this._trackId` may belong to a later lookup.
    let id = lyr.state().track_id.clone();
    let lacks = |c: &Option<Found>, f: fn(&Found) -> &JsValue| c.as_ref().is_none_or(|c| !f(c).truthy());
    let has = lyr.state().cache.iter().any(|(k, _)| *k == id);
    if !has
        || (d.plain.lyrics.truthy() && lacks(&c, |f| &f.plain.lyrics))
        || (d.line.lyrics.truthy() && lacks(&c, |f| &f.line.lyrics))
        || (d.word.lyrics.truthy() && lacks(&c, |f| &f.word.lyrics))
    {
        lyr.cache_set(id, d.clone());
    }
    Ok(map_payload(&d.to_js()))
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
async fn resolve<H: Host>(lyr: &Lyrics, host: &H, args: [JsValue; 4]) -> HandlerResult {
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
        if let Ok(r) = via_synclyrics(lyr, host, a).await
            && r.truthy()
        {
            return Ok(r);
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
                let r = match resolve(&lyr2, &*host, args).await {
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
