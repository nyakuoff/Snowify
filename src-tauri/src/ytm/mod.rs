//! A port of `ytmusic-api` 5.3.1 (the `YTMusic` class and its parsers) as Snowify's main
//! process drives it, over axios 1.13's behaviour: header merging, JSON transforms, status
//! validation, the cookie interceptors, and `AxiosError#toJSON` for the log panel.

pub mod cookies;
pub mod parsers;
pub mod proxy;
pub mod traverse;
pub mod zod;

use crate::{
    host::{Host, HttpRequest, NetError, RawResponse},
    ipc::{self, JsString, JsValue},
    js,
};
use std::sync::Mutex;
use traverse::AXIOS_RESPONSE;

const BASE: &str = "https://music.youtube.com/";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_4) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/81.0.4044.129 Safari/537.36";
const LANG: &str = "en-US,en;q=0.5";

/// What a failed call throws, as the handlers' `console.error(label, err)` logs it.
#[derive(Debug, Clone, PartialEq)]
pub enum YtmError {
    /// An `AxiosError`: its `toJSON()`.
    Axios(JsValue),
    /// Anything else (`TypeError`, `RangeError`, `new Error(...)`): logs as `{}`. The
    /// message is only read by the startup log line, and only where it is known.
    Plain(String),
}

impl YtmError {
    /// `JSON.stringify(err)` as the log panel shows it.
    pub fn log_value(&self) -> JsValue {
        match self {
            Self::Axios(j) => j.clone(),
            Self::Plain(_) => JsValue::Object(Vec::new()),
        }
    }

    /// `err.message`.
    pub fn message(&self) -> JsValue {
        match self {
            Self::Axios(j) => ipc::get_prop(j, "message").unwrap_or_default(),
            Self::Plain(m) => m.as_str().into(),
        }
    }
}

impl From<traverse::Overflow> for YtmError {
    fn from(_: traverse::Overflow) -> Self {
        Self::Plain("Maximum call stack size exceeded".into())
    }
}

impl From<traverse::ParseError> for YtmError {
    fn from(e: traverse::ParseError) -> Self {
        match e {
            traverse::ParseError::Overflow => traverse::Overflow.into(),
            traverse::ParseError::TypeError => Self::Plain(String::new()),
        }
    }
}

/// The library instance's state (`ctx.ytmusic`).
#[derive(Default)]
pub struct State {
    /// `this.config`.
    pub config: JsValue,
    jar: cookies::Jar,
}

/// `ctx.ytmusic` plus the flags `ytmusic.js` keeps on `ctx`.
#[derive(Default)]
pub struct Ytm {
    /// `None` until `initYTMusic` first creates the instance.
    pub state: Mutex<Option<State>>,
    /// `ctx.ytmusicReady`.
    pub ready: Mutex<bool>,
    /// The agent `initYTMusic` put on the client (stays until another init replaces it).
    pub proxy: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `-(new Date()).getTimezoneOffset()`.
#[cfg(unix)]
pub fn utc_offset_minutes() -> f64 {
    // SAFETY: localtime_r into a caller-owned struct.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0.0;
        }
        (tm.tm_gmtoff / 60) as f64
    }
}

/// `new Intl.DateTimeFormat().resolvedOptions().timeZone`: `$TZ` when it names a zone,
/// else the `/etc/localtime` link target, else `UTC`.
// ponytail: no ICU alias canonicalisation (e.g. "US/Pacific" stays as is); add a link table
// if a user's zone name ever differs from what V8 reports.
#[cfg(unix)]
pub fn time_zone() -> String {
    let from_path = |p: &str| p.split_once("zoneinfo/").map(|(_, z)| z.to_owned());
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':');
        if tz.starts_with('/') {
            if let Some(z) = from_path(tz) {
                return z;
            }
        } else if !tz.is_empty() && std::path::Path::new("/usr/share/zoneinfo").join(tz).is_file() {
            return tz.to_owned();
        }
    }
    std::fs::read_link("/etc/localtime").ok().and_then(|p| from_path(&p.to_string_lossy())).unwrap_or_else(|| "UTC".into())
}

/// `AxiosHeaders` normalisation: `undefined`, `null` and `false` are dropped from what is
/// sent and logged; arrays keep their (stringified) items; anything else becomes `String(v)`.
fn normalize_header(v: &JsValue) -> Option<JsValue> {
    match v {
        JsValue::Undefined | JsValue::Null | JsValue::Bool(false) => None,
        JsValue::Array(items) => Some(JsValue::Array(items.iter().map(|i| normalize_header(i).unwrap_or_else(|| i.clone())).collect())),
        v => Some(JsValue::from(ipc::to_js_string(v))),
    }
}

/// The wire values of a normalised header (an array goes out as repeated headers).
fn header_values(v: &JsValue) -> Vec<String> {
    match v {
        JsValue::Array(items) => items.iter().map(ipc::to_js_string).collect(),
        v => vec![ipc::to_js_string(v)],
    }
}

fn cfg(state: &State, key: &str) -> JsValue {
    ipc::get_prop(&state.config, key).unwrap_or_default()
}

/// The per-request config axios logs in `AxiosError#toJSON` (`config.headers` as merged,
/// before the adapter adds `Content-Length`).
fn error_config(headers: &[(String, JsValue)], method: &str, url: &str, data: Option<&str>) -> JsValue {
    let mut props: Vec<(&str, JsValue)> = vec![
        (
            "transitional",
            JsValue::object([
                ("silentJSONParsing", JsValue::Bool(true)),
                ("forcedJSONParsing", JsValue::Bool(true)),
                ("clarifyTimeoutError", JsValue::Bool(false)),
                ("legacyInterceptorReqResOrdering", JsValue::Bool(true)),
            ]),
        ),
        ("adapter", JsValue::Array(vec!["xhr".into(), "http".into(), "fetch".into()])),
        ("transformRequest", JsValue::Array(vec![JsValue::Null])),
        ("transformResponse", JsValue::Array(vec![JsValue::Null])),
        ("timeout", JsValue::Number(0.0)),
        ("xsrfCookieName", "XSRF-TOKEN".into()),
        ("xsrfHeaderName", "X-XSRF-TOKEN".into()),
        ("maxContentLength", JsValue::Number(-1.0)),
        ("maxBodyLength", JsValue::Number(-1.0)),
        ("env", JsValue::Object(Vec::new())),
        ("headers", JsValue::Object(headers.iter().map(|(k, v)| (JsString::from(k.as_str()), v.clone())).collect())),
        ("baseURL", BASE.into()),
        ("withCredentials", JsValue::Bool(true)),
    ];
    if data.is_some() {
        props.push(("responseType", "json".into()));
    }
    props.push(("method", method.into()));
    props.push(("url", url.into()));
    if let Some(d) = data {
        props.push(("data", d.into()));
    }
    props.push(("allowAbsoluteUrls", JsValue::Bool(true)));
    JsValue::object(props)
}

/// `AxiosError#toJSON()`. `stack` is only its first line (see PORT.md, owner sign-off).
fn axios_error(name: &str, message: &str, config: JsValue, code: &str, status: Option<u16>) -> YtmError {
    let mut props: Vec<(&str, JsValue)> = vec![
        ("message", message.into()),
        ("name", name.into()),
        ("stack", format!("{name}: {message}").into()),
        ("config", config),
        ("code", code.into()),
    ];
    if let Some(s) = status {
        props.push(("status", JsValue::Number(f64::from(s))));
    }
    YtmError::Axios(JsValue::object(props))
}

/// What `constructRequest`/`client.get` resolve with.
pub enum Response {
    /// `res.data` (it had `responseContext`), or the GET's transformed body.
    Data(JsValue),
    /// The axios response object itself: no `responseContext` in the body.
    Axios(JsValue),
}

impl Response {
    pub fn value(self) -> JsValue {
        match self {
            Self::Data(v) | Self::Axios(v) => v,
        }
    }
}

/// The axios response object as the parsers see it: `.contents` etc. are undefined, and
/// walking it overflows (its `request` is cyclic).
fn axios_response_object(status: u16, status_text: &str, data: JsValue) -> JsValue {
    JsValue::object([
        ("status", JsValue::Number(f64::from(status))),
        ("statusText", status_text.into()),
        ("headers", JsValue::Object(Vec::new())),
        ("config", JsValue::Object(Vec::new())),
        ("request", JsValue::object([(AXIOS_RESPONSE, JsValue::Bool(true))])),
        ("data", data),
    ])
}

/// `transformResponse`: the body as text, JSON-parsed when possible. `silentJSONParsing` is
/// on by default, so a parse failure just leaves the text.
fn transform(body: &[u8]) -> JsValue {
    let text = String::from_utf8_lossy(body);
    if text.is_empty() {
        return JsValue::from("");
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    js::json_parse(&units).unwrap_or(JsValue::String(JsString(units)))
}

impl Ytm {
    /// One request through the instance's axios client: headers merged with the cookie
    /// interceptor, then the response interceptor storing `set-cookie`.
    async fn request<H: Host>(&self, host: &H, method: &'static str, url: &str, mut headers: Vec<(String, JsValue)>, data: Option<String>) -> Result<(RawResponse, JsValue), YtmError> {
        let now = crate::logger::now_ms() as f64;
        {
            let mut st = lock(&self.state);
            let st = st.as_mut().expect("instance exists");
            let cookie = st.jar.header(now);
            if !cookie.is_empty() {
                headers.push(("cookie".into(), cookie.into()));
            }
        }
        let headers: Vec<(String, JsValue)> = headers.into_iter().filter_map(|(k, v)| normalize_header(&v).map(|v| (k, v))).collect();
        let full = format!("https://music.youtube.com/{}", url.trim_start_matches('/'));
        // The http adapter sets Content-Length on the same headers object the error logs.
        let mut logged = headers.clone();
        if let Some(d) = &data {
            logged.push(("Content-Length".into(), d.len().to_string().into()));
        }
        let config = error_config(&logged, method, url, data.as_deref());
        let mut wire: Vec<(String, String)> = Vec::new();
        for (k, v) in &headers {
            for s in header_values(v) {
                wire.push((k.clone(), s));
            }
        }
        let proxy = lock(&self.proxy).clone();
        let req = HttpRequest { method, url: full, headers: wire, body: data.as_ref().map(|d| d.clone().into_bytes()), proxy };
        let res = match host.http(req).await {
            Ok(r) => r,
            Err(NetError::Timeout) => return Err(axios_error("AxiosError", "timeout of 0ms exceeded", config, "ECONNABORTED", None)),
            Err(NetError::Failed { name, message, props }) => {
                let code = props.iter().find(|(k, _)| k == "code").map(|(_, v)| ipc::to_js_string(v)).unwrap_or_default();
                return Err(axios_error(&name, &message, config, &code, None));
            }
        };
        let parsed = transform(&res.body);
        if !(200..300).contains(&res.status) {
            let code = if res.status >= 500 { "ERR_BAD_RESPONSE" } else { "ERR_BAD_REQUEST" };
            return Err(axios_error("AxiosError", &format!("Request failed with status code {}", res.status), config, code, Some(res.status)));
        }
        // The response interceptor: every `set-cookie` into the jar (a throw rejects).
        {
            let mut st = lock(&self.state);
            let st = st.as_mut().expect("instance exists");
            for (k, v) in &res.headers {
                if k != "set-cookie" {
                    continue;
                }
                st.jar.created += 1;
                if let Some(c) = cookies::parse(v, st.jar.created) {
                    st.jar.set(c, now).map_err(|e| YtmError::Plain(e.0))?;
                }
            }
        }
        Ok((res, parsed))
    }

    fn base_headers(post: bool) -> Vec<(String, JsValue)> {
        let mut h = vec![("Accept".to_owned(), JsValue::from("application/json, text/plain, */*"))];
        if post {
            h.push(("Content-Type".into(), "application/json".into()));
        }
        h.push(("Accept-Encoding".into(), "gzip".into()));
        h.push(("User-Agent".into(), UA.into()));
        h.push(("Accept-Language".into(), LANG.into()));
        h
    }

    /// `initialize()`: GET `/`, then merge the first `ytcfg.set({...});` it finds.
    pub async fn initialize<H: Host>(&self, host: &H) -> Result<(), YtmError> {
        {
            let mut st = lock(&self.state);
            if st.is_none() {
                *st = Some(State { config: JsValue::Object(Vec::new()), jar: cookies::Jar::default() });
            }
        }
        let (_, html) = self.request(host, "get", "/", Self::base_headers(false), None).await?;
        // `html.match(...)` on a non-string throws.
        let JsValue::String(html) = html else { return Err(YtmError::Plain("html.match is not a function".into())) };
        let html = html.to_string_lossy();
        // `/ytcfg\.set\(.*?\);/` (no `g`): the first one only, `.` not crossing line ends.
        let found = html.match_indices("ytcfg.set(").find_map(|(i, _)| {
            let rest = &html[i + 10..];
            let line_end = rest.find(['\n', '\r', '\u{2028}', '\u{2029}']).unwrap_or(rest.len());
            rest[..line_end].find(");").map(|j| &rest[..j])
        });
        let mut st = lock(&self.state);
        let st = st.as_mut().expect("instance exists");
        if let Some(text) = found {
            let units: Vec<u16> = text.encode_utf16().collect();
            // `{ ...this.config, ...config }` for an object; a non-object spreads nothing
            // (a string spreads its characters as index keys).
            if let Ok(parsed) = js::json_parse(&units) {
                spread_into(&mut st.config, &parsed);
            }
        }
        Ok(())
    }

    /// `constructRequest(endpoint, body, query)`.
    pub async fn construct<H: Host>(&self, host: &H, endpoint: &str, body: Vec<(&str, JsValue)>, query: Vec<(&str, JsValue)>) -> Result<Response, YtmError> {
        let (headers, url, data) = {
            let st = lock(&self.state);
            let st = st.as_ref().expect("instance exists");
            let mut h = Self::base_headers(true);
            h.push(("x-origin".into(), BASE.into()));
            let visitor = cfg(st, "VISITOR_DATA");
            h.push(("X-Goog-Visitor-Id".into(), if visitor.truthy() { visitor } else { "".into() }));
            for (name, key) in [
                ("X-YouTube-Client-Name", "INNERTUBE_CONTEXT_CLIENT_NAME"),
                ("X-YouTube-Client-Version", "INNERTUBE_CLIENT_VERSION"),
                ("X-YouTube-Device", "DEVICE"),
                ("X-YouTube-Page-CL", "PAGE_CL"),
                ("X-YouTube-Page-Label", "PAGE_BUILD_LABEL"),
            ] {
                h.push((name.into(), cfg(st, key)));
            }
            let offset = utc_offset_minutes();
            h.push(("X-YouTube-Utc-Offset".into(), js::number_to_string(offset).into()));
            h.push(("X-YouTube-Time-Zone".into(), time_zone().into()));
            let mut params: Vec<(&str, String)> = query.iter().map(|(k, v)| (*k, ipc::to_js_string(v))).collect();
            params.retain(|(k, _)| *k != "alt" && *k != "key");
            params.push(("alt", "json".into()));
            params.push(("key", ipc::to_js_string(&cfg(st, "INNERTUBE_API_KEY"))));
            let url = format!("youtubei/{}/{endpoint}?{}", ipc::to_js_string(&cfg(st, "INNERTUBE_API_VERSION")), crate::lyrics::form(&params));
            let context = JsValue::object([
                ("capabilities", JsValue::Object(Vec::new())),
                (
                    "client",
                    JsValue::object([
                        ("clientName", cfg(st, "INNERTUBE_CLIENT_NAME")),
                        ("clientVersion", cfg(st, "INNERTUBE_CLIENT_VERSION")),
                        ("experimentIds", JsValue::Array(Vec::new())),
                        ("experimentsToken", "".into()),
                        ("gl", cfg(st, "GL")),
                        ("hl", cfg(st, "HL")),
                        ("locationInfo", JsValue::object([("locationPermissionAuthorizationStatus", JsValue::from("LOCATION_PERMISSION_AUTHORIZATION_STATUS_UNSUPPORTED"))])),
                        (
                            "musicAppInfo",
                            JsValue::object([
                                ("musicActivityMasterSwitch", JsValue::from("MUSIC_ACTIVITY_MASTER_SWITCH_INDETERMINATE")),
                                ("musicLocationMasterSwitch", "MUSIC_LOCATION_MASTER_SWITCH_INDETERMINATE".into()),
                                ("pwaInstallabilityStatus", "PWA_INSTALLABILITY_STATUS_UNKNOWN".into()),
                            ]),
                        ),
                        ("utcOffsetMinutes", JsValue::Number(offset)),
                    ]),
                ),
                (
                    "request",
                    JsValue::object([
                        (
                            "internalExperimentFlags",
                            JsValue::Array(
                                ["force_music_enable_outertube_tastebuilder_browse", "force_music_enable_outertube_playlist_detail_browse", "force_music_enable_outertube_search_suggestions"]
                                    .iter()
                                    .map(|k| JsValue::object([("key", JsValue::from(*k)), ("value", "true".into())]))
                                    .collect(),
                            ),
                        ),
                        ("sessionIndex", JsValue::Object(Vec::new())),
                    ]),
                ),
                ("user", JsValue::object([("enableSafetyMode", JsValue::Bool(false))])),
            ]);
            let mut payload = JsValue::object([("context", context)]);
            for (k, v) in body {
                ipc::define(&mut payload, k.into(), v);
            }
            // `JSON.stringify` of the payload (a BigInt in it throws before sending).
            let data = ipc::json_stringify(&payload).map_err(|_| YtmError::Plain("Do not know how to serialize a BigInt".into()))?.unwrap_or_default();
            (h, url, data)
        };
        let (res, parsed) = self.request(host, "post", &url, headers, Some(data)).await?;
        // `"responseContext" in res.data`: a primitive throws.
        match &parsed {
            JsValue::Object(props) if props.iter().any(|(k, _)| *k == JsString::from("responseContext")) => Ok(Response::Data(parsed)),
            JsValue::Object(_) | JsValue::Array(_) => Ok(Response::Axios(axios_response_object(res.status, &res.status_text, parsed))),
            v => Err(YtmError::Plain(format!("Cannot use 'in' operator to search for 'responseContext' in {}", ipc::to_js_string(v)))),
        }
    }
}

/// `target = { ...target, ...source }`.
fn spread_into(target: &mut JsValue, source: &JsValue) {
    match source {
        JsValue::Object(props) => {
            for (k, v) in props {
                ipc::define(target, k.clone(), v.clone());
            }
        }
        JsValue::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                ipc::define(target, JsString::from(i.to_string()), v.clone());
            }
        }
        JsValue::String(s) => {
            for (i, u) in s.0.iter().enumerate() {
                ipc::define(target, JsString::from(i.to_string()), JsValue::String(JsString(vec![*u])));
            }
        }
        _ => {}
    }
}

#[cfg(windows)]
pub fn utc_offset_minutes() -> f64 { crate::windows_time::utc_offset_ms(crate::logger::now_ms() as i64) as f64 / 60000.0 }
#[cfg(windows)]
pub fn time_zone() -> String { crate::windows_time::time_zone() }
