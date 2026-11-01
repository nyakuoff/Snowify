//! What main-process handlers need from the outside world, behind one trait so the parity
//! harness can substitute the same mocks it gives the JS source: `app.getPath('userData')`,
//! `app.isPackaged`, the repo checkout (`path.join(__dirname, '..', '..')` in dev),
//! `dialog.showOpenDialog`, `shell.openPath`, and a single `https.get` request.

use crate::ipc::JsValue;
use std::future::Future;

/// `dialog.showOpenDialog` options the source uses.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenDialog {
    pub title: String,
    pub filters: Vec<(String, Vec<String>)>,
    pub multi: bool,
    pub directory: bool,
}

/// `dialog.showSaveDialog` options the source uses.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveDialog {
    pub title: String,
    pub default_path: String,
    pub filters: Vec<(String, Vec<String>)>,
}

/// One HTTP response, before the source's own redirect/status handling.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    /// Lowercased names, as Node's `res.headers`.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A global `fetch(url, { headers, redirect })` request. Header values are already strings
/// (`new Headers()` stringifies them, so `cookie: undefined` is sent as `"undefined"`).
#[derive(Debug, Clone, PartialEq)]
pub struct FetchRequest {
    pub method: &'static str,
    pub body: Option<Vec<u8>>,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// `redirect: 'manual'`: a 3xx comes back as the response instead of being followed.
    pub manual_redirect: bool,
}

impl FetchRequest {
    /// The headers as `[...new Headers(init)]` lists them: lowercased names, sorted,
    /// repeated names joined with `", "`.
    pub fn normalized_headers(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for (k, v) in &self.headers {
            let k = k.to_ascii_lowercase();
            match out.iter_mut().find(|(n, _)| *n == k) {
                Some((_, old)) => *old = format!("{old}, {v}"),
                None => out.push((k, v.clone())),
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

/// A `fetch` response: status, status text, headers (lowercased names, in order, repeats
/// kept for `getSetCookie()`), body.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl FetchResponse {
    /// `res.ok`.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// `res.headers.getSetCookie()`.
    pub fn set_cookies(&self) -> Vec<String> {
        self.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.clone()).collect()
    }
}

/// One request through axios's Node http adapter (follow-redirects, up to 21 hops;
/// gzip/deflate/br decoded and `content-encoding` dropped, as axios does).
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    /// In the order axios sends them.
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// The agent's proxy URL (`http://host:port`, `socks5://host:port`), if one was set.
    pub proxy: Option<String>,
}

/// The raw response axios sees before `transformResponse`.
#[derive(Debug, Clone, PartialEq)]
pub struct RawResponse {
    pub status: u16,
    pub status_text: String,
    /// Lowercased names in arrival order; `set-cookie` may repeat.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// How a Node request fails.
#[derive(Debug, Clone, PartialEq)]
pub enum NetError {
    /// `req.setTimeout` fired.
    Timeout,
    /// An `'error'` event (or a synchronous throw for the first request): Node's message,
    /// error name, and the own enumerable properties (`errno`, `code`, `syscall`, `hostname`…).
    Failed {
        name: String,
        message: String,
        props: Vec<(String, JsValue)>,
    },
}

pub trait Host: Send + Sync {
    fn user_data(&self) -> String;
    fn is_packaged(&self) -> bool;
    /// The repository root the dev build reads local plugins/themes from.
    fn repo_root(&self) -> String;
    /// `None` when canceled or nothing was picked.
    fn show_open_dialog(
        &self,
        opts: OpenDialog,
    ) -> impl Future<Output = Option<Vec<String>>> + Send;
    /// `dialog.showSaveDialog`: the chosen path, `None` when canceled.
    fn show_save_dialog(&self, opts: SaveDialog) -> impl Future<Output = Option<String>> + Send;
    fn open_path(&self, path: &str);
    /// `Date.now()`.
    fn now_ms(&self) -> i64;
    /// One `https.get(url, { headers })` round trip, idle timeout `timeout_ms`.
    fn https_get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        timeout_ms: u64,
    ) -> impl Future<Output = Result<HttpResponse, NetError>> + Send;
    /// `session.defaultSession.resolveProxy(url)`: a PAC-style result (`DIRECT`,
    /// `PROXY host:port`, `SOCKS5 host:port`, `;`-separated). `Err` is a rejection message.
    fn resolve_proxy(&self, url: &str) -> impl Future<Output = Result<String, String>> + Send;
    /// One axios request (YouTube Music). `Err` is the Node error axios wraps.
    fn http(&self, req: HttpRequest) -> impl Future<Output = Result<RawResponse, NetError>> + Send;
    /// Global `fetch` (undici). `Err` is its `TypeError: fetch failed`.
    fn fetch(&self, req: FetchRequest) -> impl Future<Output = Result<FetchResponse, ()>> + Send;
}
