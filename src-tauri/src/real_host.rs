//! The production [`Host`]: Electron's `userData`, dev vs packaged, GTK/native file dialogs,
//! the desktop's opener, and one HTTPS request with Node's error shapes.

use crate::{
    host::{FetchRequest, FetchResponse, Host, HttpRequest, HttpResponse, NetError, OpenDialog, RawResponse, SaveDialog},
    ipc::JsValue,
    node,
};
use std::{future::Future, sync::OnceLock, time::Duration};

pub struct RealHost {
    pub window: Option<tauri::WebviewWindow>,
}

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// undici's request defaults (Node 20): what servers see from the source's `fetch`.
fn fetch_client(manual: bool) -> &'static reqwest::Client {
    static FOLLOW: OnceLock<reqwest::Client> = OnceLock::new();
    static MANUAL: OnceLock<reqwest::Client> = OnceLock::new();
    (if manual { &MANUAL } else { &FOLLOW }).get_or_init(|| {
        let mut h = reqwest::header::HeaderMap::new();
        for (k, v) in [("accept", "*/*"), ("accept-language", "*"), ("sec-fetch-mode", "cors"), ("user-agent", "node")] {
            h.insert(k, reqwest::header::HeaderValue::from_static(v));
        }
        reqwest::Client::builder()
            .default_headers(h)
            .redirect(if manual { reqwest::redirect::Policy::none() } else { reqwest::redirect::Policy::limited(20) })
            .build()
            .expect("http client")
    })
}

/// axios + follow-redirects: up to 21 redirects, bodies decompressed; one client per agent
/// proxy (`initYTMusic` sets `httpsAgent`).
fn axios_client(proxy: Option<&str>) -> reqwest::Client {
    static CLIENTS: std::sync::Mutex<Vec<(Option<String>, reqwest::Client)>> = std::sync::Mutex::new(Vec::new());
    let mut cs = CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, c)) = cs.iter().find(|(p, _)| p.as_deref() == proxy) {
        return c.clone();
    }
    let mut b = reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(21)).no_proxy();
    if let Some(p) = proxy.and_then(|p| reqwest::Proxy::all(p).ok()) {
        b = b.proxy(p);
    }
    let c = b.build().expect("http client");
    cs.push((proxy.map(str::to_owned), c.clone()));
    c
}

fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // The ported httpsGet loop follows redirects itself.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("http client")
    })
}

/// Maps a request failure onto the message and properties Node's `https` reports.
fn node_error(err: &reqwest::Error, url: &url::Url) -> NetError {
    let host = url.host_str().unwrap_or_default().to_owned();
    let mut chain = String::new();
    let mut src: Option<&dyn std::error::Error> = Some(err);
    while let Some(e) = src {
        chain.push_str(&e.to_string());
        chain.push('\n');
        src = e.source();
    }
    let io = {
        let mut s: Option<&dyn std::error::Error> = Some(err);
        let mut found = None;
        while let Some(e) = s {
            if let Some(io) = e.downcast_ref::<std::io::Error>() {
                found = Some(io.raw_os_error());
            }
            s = e.source();
        }
        found.flatten()
    };
    let io_code = io.and_then(|raw| {
        let errno = crate::uv_errors::errno(raw);
        crate::uv_errors::UV_ERRORS.iter().find(|row| row.0 == errno).map(|row| row.1)
    });
    let fail = |message: String, props: Vec<(&str, JsValue)>| NetError::Failed {
        name: "Error".into(),
        message,
        props: props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
    };
    let sys = |errno: i32, code: &str, syscall: &str| -> Vec<(&'static str, JsValue)> {
        #[cfg(windows)]
        let errno = crate::uv_errors::UV_ERRORS.iter().find(|row| row.1 == code).map_or(errno, |row| -row.0);
        vec![
            ("errno", JsValue::Number(f64::from(errno))),
            ("code", code.into()),
            ("syscall", syscall.into()),
        ]
    };
    if chain.contains("failed to lookup address") || chain.contains("dns error") {
        let (errno, code) = if chain.contains("Temporary failure") {
            (-3001, "EAI_AGAIN")
        } else {
            (-3008, "ENOTFOUND")
        };
        let mut props = sys(errno, code, "getaddrinfo");
        props.push(("hostname", host.as_str().into()));
        return fail(format!("getaddrinfo {code} {host}"), props);
    }
    let port = url.port_or_known_default().unwrap_or(443);
    match io_code {
        Some("ECONNREFUSED") => {
            let addr = chain
                .split("tcp connect error")
                .next()
                .map(|_| host.clone())
                .unwrap_or_default();
            let mut props = sys(-111, "ECONNREFUSED", "connect");
            props.push(("address", addr.as_str().into()));
            props.push(("port", JsValue::Number(f64::from(port))));
            fail(format!("connect ECONNREFUSED {addr}:{port}"), props)
        }
        Some("ECONNRESET") => fail("read ECONNRESET".into(), sys(-104, "ECONNRESET", "read")),
        Some("ENETUNREACH") => fail(
            format!("connect ENETUNREACH {host}:{port}"),
            sys(-101, "ENETUNREACH", "connect"),
        ),
        _ if chain.contains("expired") => fail(
            "certificate has expired".into(),
            vec![("code", "CERT_HAS_EXPIRED".into())],
        ),
        _ if chain.contains("UnknownIssuer") => fail(
            "unable to get local issuer certificate".into(),
            vec![("code", "UNABLE_TO_GET_ISSUER_CERT_LOCALLY".into())],
        ),
        _ if chain.contains("NotValidForName") => fail(
            format!(
                "Hostname/IP does not match certificate's altnames: Host: {host}. is not in the cert's altnames"
            ),
            vec![("code", "ERR_TLS_CERT_ALTNAME_INVALID".into())],
        ),
        _ => fail("socket hang up".into(), vec![("code", "ECONNRESET".into())]),
    }
}

impl Host for RealHost {
    fn user_data(&self) -> String {
        node::user_data_dir()
    }

    /// `app.isPackaged`: false for a debug build, as for `electron .`.
    fn is_packaged(&self) -> bool {
        !cfg!(debug_assertions)
    }

    fn repo_root(&self) -> String {
        node::path::join(&[env!("CARGO_MANIFEST_DIR"), ".."])
    }

    fn show_open_dialog(
        &self,
        opts: OpenDialog,
    ) -> impl Future<Output = Option<Vec<String>>> + Send {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(&opts.title);
        for (name, exts) in &opts.filters {
            dialog = dialog.add_filter(name, exts);
        }
        if let Some(w) = &self.window {
            dialog = dialog.set_parent(w);
        }
        let (multi, directory) = (opts.multi, opts.directory);
        async move {
            let paths: Vec<String> = match (directory, multi) {
                (true, true) => dialog.pick_folders().await.unwrap_or_default(),
                (true, false) => dialog.pick_folder().await.into_iter().collect(),
                (false, true) => dialog.pick_files().await.unwrap_or_default(),
                (false, false) => dialog.pick_file().await.into_iter().collect(),
            }
            .into_iter()
            .map(|h| h.path().to_string_lossy().into_owned())
            .collect();
            (!paths.is_empty()).then_some(paths)
        }
    }

    fn show_save_dialog(&self, opts: SaveDialog) -> impl Future<Output = Option<String>> + Send {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(&opts.title).set_file_name(&opts.default_path);
        for (name, exts) in &opts.filters {
            dialog = dialog.add_filter(name, exts);
        }
        if let Some(w) = &self.window {
            dialog = dialog.set_parent(w);
        }
        async move { dialog.save_file().await.map(|h| h.path().to_string_lossy().into_owned()) }
    }

    fn now_ms(&self) -> i64 {
        crate::logger::now_ms()
    }

    fn open_path(&self, path: &str) {
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
        #[cfg(windows)]
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }

    fn resolve_proxy(&self, url: &str) -> impl Future<Output = Result<String, String>> + Send {
        let r = crate::ytm::proxy::resolve(url);
        async move { Ok(r) }
    }

    fn http(&self, req: HttpRequest) -> impl Future<Output = Result<RawResponse, NetError>> + Send {
        async move {
            let url = url::Url::parse(&req.url).map_err(|_| NetError::Failed { name: "TypeError".into(), message: "Invalid URL".into(), props: vec![("code".into(), "ERR_INVALID_URL".into())] })?;
            let method = reqwest::Method::from_bytes(req.method.to_uppercase().as_bytes()).unwrap_or(reqwest::Method::GET);
            let mut r = axios_client(req.proxy.as_deref()).request(method, url.clone());
            for (k, v) in &req.headers {
                r = r.header(k.as_str(), v.as_str());
            }
            if let Some(b) = req.body {
                r = r.body(b);
            }
            let res = r.send().await.map_err(|e| node_error(&e, &url))?;
            let status = res.status();
            let headers = res
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_owned(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
                .collect();
            let body = res.bytes().await.map_err(|e| node_error(&e, &url))?.to_vec();
            Ok(RawResponse { status: status.as_u16(), status_text: status.canonical_reason().unwrap_or_default().to_owned(), headers, body })
        }
    }

    fn fetch(&self, req: FetchRequest) -> impl Future<Output = Result<FetchResponse, ()>> + Send {
        async move {
            let mut r = fetch_client(req.manual_redirect).request(reqwest::Method::from_bytes(req.method.as_bytes()).map_err(|_| ())?, &req.url);
            for (k, v) in &req.headers {
                r = r.header(k.as_str(), v.as_str());
            }
            if let Some(body) = req.body { r = r.body(body); }
            let res = r.send().await.map_err(|_| ())?;
            let status = res.status();
            let headers = res
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_owned(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
                .collect();
            let body = res.bytes().await.map_err(|_| ())?.to_vec();
            Ok(FetchResponse {
                status: status.as_u16(),
                status_text: status.canonical_reason().unwrap_or_default().to_owned(),
                headers,
                body,
            })
        }
    }

    fn https_get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        timeout_ms: u64,
    ) -> impl Future<Output = Result<HttpResponse, NetError>> + Send {
        let parsed = url::Url::parse(url);
        let headers: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        async move {
            // https.get's own synchronous checks.
            let url = parsed.map_err(|_| NetError::Failed {
                name: "TypeError".into(),
                message: "Invalid URL".into(),
                props: vec![("code".into(), "ERR_INVALID_URL".into())],
            })?;
            if url.scheme() != "https" {
                return Err(NetError::Failed {
                    name: "TypeError".into(),
                    message: format!(
                        "Protocol \"{}:\" not supported. Expected \"https:\"",
                        url.scheme()
                    ),
                    props: vec![("code".into(), "ERR_INVALID_PROTOCOL".into())],
                });
            }
            let idle = Duration::from_millis(timeout_ms);
            let mut req = client().get(url.clone());
            // Node https.request applies setHeader in object order: the last spelling
            // wins case-insensitively. reqwest's header() appends instead.
            for (i, (k, v)) in headers.iter().enumerate() {
                if !headers[i + 1..].iter().any(|(next, _)| next.eq_ignore_ascii_case(k)) {
                    req = req.header(k, v);
                }
            }
            let send = tokio::time::timeout(idle, req.send())
                .await
                .map_err(|_| NetError::Timeout)?;
            let res = send.map_err(|e| node_error(&e, &url))?;
            let status = res.status().as_u16();
            let resp_headers = res
                .headers()
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_ascii_lowercase(),
                        String::from_utf8_lossy(v.as_bytes()).into_owned(),
                    )
                })
                .collect();
            // Idle timeout between chunks, like req.setTimeout on the socket.
            let mut body = Vec::new();
            let mut stream = res.bytes_stream();
            use futures_util::StreamExt;
            loop {
                match tokio::time::timeout(idle, stream.next()).await {
                    Err(_) => return Err(NetError::Timeout),
                    Ok(None) => break,
                    Ok(Some(Ok(chunk))) => body.extend_from_slice(&chunk),
                    Ok(Some(Err(e))) => return Err(node_error(&e, &url)),
                }
            }
            Ok(HttpResponse {
                status,
                headers: resp_headers,
                body,
            })
        }
    }
}
