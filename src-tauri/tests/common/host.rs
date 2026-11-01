//! The Rust half of `parity/node/main_module.js`'s mocks: a `Host` driven by the same scenario
//! JSON, recording the same calls, plus a runner that executes the scenario's ops through a
//! module's handlers and produces output shaped like the JS runner's.
#![allow(dead_code)]

use serde_json::{Value, json};
use snowify::{
    host::{
        FetchRequest, FetchResponse, Host, HttpRequest, HttpResponse, NetError, OpenDialog,
        RawResponse, SaveDialog,
    },
    ipc::{HandlerResult, JsValue},
};
use std::{collections::HashMap, future::Future, pin::Pin, sync::Mutex, time::Duration};

pub struct MockHost {
    pub user_data: String,
    pub repo_root: String,
    pub packaged: bool,
    dialogs: Mutex<Vec<Value>>,
    save_dialogs: Mutex<Vec<Value>>,
    now: i64,
    http: Mutex<HashMap<String, Vec<Value>>>,
    fetch: Mutex<Vec<(String, Vec<Value>)>>,
    pub fetch_calls: Mutex<Vec<Value>>,
    pub fetch_requests: Mutex<Vec<Value>>,
    /// axios replay (ytm_replay.js): recorded exchanges by key, and overrides.
    replay: Mutex<HashMap<String, Vec<Value>>>,
    overrides: Vec<Value>,
    resolve_proxy: Value,
    pub requests: Mutex<Vec<Value>>,
    pub dialog_calls: Mutex<Vec<Value>>,
    pub open_path_calls: Mutex<Vec<Value>>,
    pub http_calls: Mutex<Vec<Value>>,
}

impl MockHost {
    pub fn new(scenario: &Value, repo_root: &str) -> Self {
        let http = scenario["http"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_array().cloned().unwrap_or_default()))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            user_data: scenario["userData"].as_str().unwrap().into(),
            repo_root: repo_root.into(),
            packaged: scenario["isPackaged"].as_bool().unwrap_or(false),
            dialogs: Mutex::new(scenario["dialogs"].as_array().cloned().unwrap_or_default()),
            save_dialogs: Mutex::new(
                scenario["saveDialogs"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            ),
            now: scenario["now"].as_i64().unwrap_or(0),
            http: Mutex::new(http),
            fetch: Mutex::new(
                scenario["fetch"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| (k.clone(), v.as_array().cloned().unwrap_or_default()))
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            fetch_calls: Mutex::default(),
            fetch_requests: Mutex::default(),
            replay: Mutex::new(load_recording(scenario["recording"].as_str())),
            overrides: scenario["overrides"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            resolve_proxy: scenario
                .get("resolveProxy")
                .cloned()
                .unwrap_or(json!("DIRECT")),
            requests: Mutex::default(),
            dialog_calls: Mutex::default(),
            open_path_calls: Mutex::default(),
            http_calls: Mutex::default(),
        }
    }
}

fn js(v: &Value) -> JsValue {
    JsValue::from(v)
}

impl Host for MockHost {
    fn user_data(&self) -> String {
        self.user_data.clone()
    }
    fn is_packaged(&self) -> bool {
        self.packaged
    }
    fn repo_root(&self) -> String {
        self.repo_root.clone()
    }
    fn show_open_dialog(
        &self,
        opts: OpenDialog,
    ) -> impl Future<Output = Option<Vec<String>>> + Send {
        let mut props = vec![if opts.directory {
            "openDirectory"
        } else {
            "openFile"
        }];
        if opts.multi {
            props.push("multiSelections");
        }
        let filters: Vec<Value> = opts
            .filters
            .iter()
            .map(|(n, e)| json!({ "name": n, "extensions": e }))
            .collect();
        let mut call = json!({ "title": opts.title });
        if !filters.is_empty() {
            call["filters"] = json!(filters);
        }
        call["properties"] = json!(props);
        self.dialog_calls.lock().unwrap().push(call);
        let mut q = self.dialogs.lock().unwrap();
        let r = if q.is_empty() {
            json!({ "canceled": true, "filePaths": [] })
        } else {
            q.remove(0)
        };
        let out = if r["canceled"].as_bool().unwrap_or(false) {
            None
        } else {
            Some(
                r["filePaths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str().map(str::to_owned))
                    .collect(),
            )
        };
        async move { out }
    }
    fn show_save_dialog(&self, opts: SaveDialog) -> impl Future<Output = Option<String>> + Send {
        let filters: Vec<Value> = opts
            .filters
            .iter()
            .map(|(n, e)| json!({ "name": n, "extensions": e }))
            .collect();
        self.dialog_calls.lock().unwrap().push(
            json!({ "title": opts.title, "defaultPath": opts.default_path, "filters": filters }),
        );
        let mut q = self.save_dialogs.lock().unwrap();
        let r = if q.is_empty() {
            json!({ "canceled": true })
        } else {
            q.remove(0)
        };
        let out = if r["canceled"].as_bool().unwrap_or(false) {
            None
        } else {
            r["filePath"].as_str().map(str::to_owned)
        };
        async move { out }
    }

    fn now_ms(&self) -> i64 {
        self.now
    }

    fn open_path(&self, path: &str) {
        self.open_path_calls.lock().unwrap().push(json!(path));
    }
    fn resolve_proxy(&self, _url: &str) -> impl Future<Output = Result<String, String>> + Send {
        let r = match &self.resolve_proxy {
            Value::Object(o) => Err(o["error"].as_str().unwrap_or("").to_owned()),
            v => Ok(v.as_str().unwrap_or("DIRECT").to_owned()),
        };
        async move { r }
    }

    fn http(&self, req: HttpRequest) -> impl Future<Output = Result<RawResponse, NetError>> + Send {
        let body = req
            .body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).into_owned());
        // What axios's http adapter hands to https.request: its headers plus Content-Length.
        let mut headers: Vec<(String, String)> = req.headers.clone();
        if let Some(b) = &req.body {
            headers.push(("Content-Length".into(), b.len().to_string()));
        }
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        // The agent's proxy as `agent.proxy` shows it (a parsed URL's href for HTTP).
        let proxy = req.proxy.as_ref().map(|p| {
            if p.starts_with("http") {
                url::Url::parse(p)
                    .map(|u| u.to_string())
                    .unwrap_or_else(|_| p.clone())
            } else {
                p.clone()
            }
        });
        self.requests.lock().unwrap().push(json!([
            req.method.to_uppercase(),
            req.url,
            headers,
            body,
            proxy
        ]));
        let found = self.overrides.iter().find(|o| {
            o["method"]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case(req.method)
                && req.url.starts_with(o["url"].as_str().unwrap_or(""))
                && o["bodyIncludes"]
                    .as_str()
                    .is_none_or(|inc| body.as_deref().unwrap_or("").contains(inc))
        });
        let r = match found {
            Some(o) => o["response"].clone(),
            None => {
                let mut pool = self.replay.lock().unwrap();
                match pool.get_mut(&replay_key(req.method, &req.url, body.as_deref())) {
                    Some(list) if list.len() > 1 => list.remove(0),
                    Some(list) => list[0].clone(),
                    None => {
                        json!({ "status": 404, "statusText": "Not Found", "headers": { "content-type": "text/plain" }, "data": "no recording" })
                    }
                }
            }
        };
        async move {
            tokio::task::yield_now().await;
            if let Some(e) = r.get("error") {
                return Err(NetError::Failed {
                    name: "Error".into(),
                    message: e["message"].as_str().unwrap_or("").into(),
                    props: vec![("code".into(), js(&e["code"]))],
                });
            }
            let mut headers = Vec::new();
            for (k, v) in r["headers"].as_object().cloned().unwrap_or_default() {
                let k = k.to_ascii_lowercase();
                if ["content-encoding", "content-length", "transfer-encoding"].contains(&k.as_str())
                {
                    continue;
                }
                match v {
                    Value::Array(items) => headers.extend(
                        items
                            .iter()
                            .map(|i| (k.clone(), i.as_str().unwrap_or("").to_owned())),
                    ),
                    v => headers.push((
                        k,
                        v.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| v.to_string()),
                    )),
                }
            }
            Ok(RawResponse {
                status: r["status"].as_u64().unwrap_or(200) as u16,
                status_text: r["statusText"].as_str().unwrap_or("").into(),
                headers,
                body: r["data"].as_str().unwrap_or("").as_bytes().to_vec(),
            })
        }
    }

    fn fetch(&self, req: FetchRequest) -> impl Future<Output = Result<FetchResponse, ()>> + Send {
        self.fetch_requests.lock().unwrap().push(json!([
            req.method,
            req.url,
            req.normalized_headers(),
            req.body
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).into_owned())
        ]));
        self.fetch_calls.lock().unwrap().push(json!([
            req.url,
            req.normalized_headers(),
            if req.manual_redirect {
                "manual"
            } else {
                "follow"
            }
        ]));
        let r = {
            let mut f = self.fetch.lock().unwrap();
            match f
                .iter_mut()
                .find(|(k, _)| req.url.starts_with(k.as_str()))
                .map(|(_, l)| l)
            {
                Some(list) if list.len() > 1 => list.remove(0),
                Some(list) => list[0].clone(),
                None => json!({ "status": 404, "body": "" }),
            }
        };
        async move {
            tokio::task::yield_now().await;
            if r["error"].as_bool().unwrap_or(false) {
                return Err(());
            }
            let body = match r.get("b64").and_then(Value::as_str) {
                Some(b) => base64_decode(b),
                None => r["body"].as_str().unwrap_or("").as_bytes().to_vec(),
            };
            Ok(FetchResponse {
                status: r["status"].as_u64().unwrap_or(200) as u16,
                status_text: r["statusText"].as_str().unwrap_or("").into(),
                headers: r["headers"]
                    .as_array()
                    .map(|h| {
                        h.iter()
                            .map(|p| {
                                (
                                    p[0].as_str().unwrap().to_ascii_lowercase(),
                                    p[1].as_str().unwrap().to_owned(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                body,
            })
        }
    }

    fn https_get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        _timeout_ms: u64,
    ) -> impl Future<Output = Result<HttpResponse, NetError>> + Send {
        let r = (|| {
            let parsed = url::Url::parse(url).map_err(|_| NetError::Failed {
                name: "TypeError".into(),
                message: "Invalid URL".into(),
                props: vec![
                    ("code".into(), "ERR_INVALID_URL".into()),
                    ("input".into(), url.into()),
                ],
            })?;
            if parsed.scheme() != "https" {
                return Err(NetError::Failed {
                    name: "TypeError".into(),
                    message: format!(
                        "Protocol \"{}:\" not supported. Expected \"https:\"",
                        parsed.scheme()
                    ),
                    props: vec![("code".into(), "ERR_INVALID_PROTOCOL".into())],
                });
            }
            let hdrs: serde_json::Map<String, Value> = headers
                .iter()
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect();
            self.http_calls.lock().unwrap().push(json!([url, hdrs]));
            let mut http = self.http.lock().unwrap();
            let r = match http.get_mut(url) {
                Some(list) if list.len() > 1 => list.remove(0),
                Some(list) => list[0].clone(),
                None => json!({ "status": 404, "body": "" }),
            };
            if r["timeout"].as_bool().unwrap_or(false) {
                return Err(NetError::Timeout);
            }
            if let Some(e) = r.get("error") {
                let props = e["props"]
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), js(v))).collect())
                    .unwrap_or_default();
                return Err(NetError::Failed {
                    name: e["name"].as_str().unwrap_or("Error").into(),
                    message: e["message"].as_str().unwrap_or("").into(),
                    props,
                });
            }
            let body = match r.get("b64").and_then(Value::as_str) {
                Some(b) => base64_decode(b),
                None => r["body"].as_str().unwrap_or("").as_bytes().to_vec(),
            };
            let headers = r["headers"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            Ok(HttpResponse {
                status: r["status"].as_u64().unwrap_or(200) as u16,
                headers,
                body,
            })
        })();
        async move { r }
    }
}

/// `ytm_replay.js`'s request key: method, URL, body without `utcOffsetMinutes`.
fn replay_key(method: &str, url: &str, body: Option<&str>) -> String {
    let body = body.unwrap_or("");
    let re = regex::Regex::new(r#""utcOffsetMinutes":-?[0-9.e+-]+"#).unwrap();
    format!(
        "{} {url} {}",
        method.to_uppercase(),
        re.replacen(body, 1, "")
    )
}

fn load_recording(path: Option<&str>) -> HashMap<String, Vec<Value>> {
    let mut pool: HashMap<String, Vec<Value>> = HashMap::new();
    let Some(path) = path else { return pool };
    let gz = std::fs::read(path).unwrap_or_else(|e| panic!("recording {path}: {e}"));
    let mut text = String::new();
    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&gz[..]), &mut text).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    for e in v["exchanges"].as_array().unwrap() {
        let k = replay_key(
            e["method"].as_str().unwrap(),
            e["url"].as_str().unwrap(),
            e["body"].as_str(),
        );
        pool.entry(k).or_default().push(json!({ "status": e["status"], "statusText": e["statusText"], "headers": e["resHeaders"], "data": e["data"] }));
    }
    pool
}

fn base64_decode(s: &str) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| *c != b'=') {
        let Some(v) = A.iter().position(|a| *a == c) else {
            continue;
        };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

pub type Dispatch<'a> = &'a (
        dyn Fn(
    &'a MockHost,
    String,
    Vec<JsValue>,
) -> Pin<Box<dyn Future<Output = Option<HandlerResult>> + 'a>>
            + 'a
    );

/// Runs `scenario.ops` through `dispatch`, shaped like `main_module.js`'s output.
pub async fn run_ops<'a>(host: &'a MockHost, scenario: &Value, dispatch: Dispatch<'a>) -> Value {
    let logs_before = snowify::logger::LOGS.lock().unwrap().entries().len();
    let mut results = Vec::new();
    let wait = Duration::from_millis(scenario["opTimeoutMs"].as_u64().unwrap_or(2000));
    let run = |op: &Value| {
        let ch = op[0].as_str().unwrap().to_owned();
        let args: Vec<JsValue> = op[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| JsValue::decode(a).unwrap())
            .collect();
        let fut = dispatch(host, ch.clone(), args);
        async move {
            match tokio::time::timeout(wait, fut).await {
                Err(_) => json!("pending"),
                Ok(None) => panic!("no handler for {ch}"),
                Ok(Some(Ok(v))) => json!({ "ok": snowify::ipc::normalize_identity(&v).encode() }),
                Ok(Some(Err(t))) => {
                    json!({ "err": JsValue::String(t.rejection_message(&ch)).encode() })
                }
            }
        }
    };
    for op in scenario["ops"].as_array().unwrap() {
        if op[0] == "$sleep" {
            tokio::time::sleep(Duration::from_millis(op[1].as_u64().unwrap())).await;
            results.push(Value::Null);
            continue;
        }
        results.push(if op[0] == "$parallel" {
            Value::Array(
                futures_util::future::join_all(op[1].as_array().unwrap().iter().map(run)).await,
            )
        } else {
            run(op).await
        });
    }
    let logs: Vec<Value> = snowify::logger::LOGS.lock().unwrap().entries()[logs_before..]
        .iter()
        .map(|e| json!([e.level.encode(), e.msg]))
        .collect();
    json!({
        "results": results,
        "logs": logs,
        "dialogCalls": *host.dialog_calls.lock().unwrap(),
        "openPathCalls": *host.open_path_calls.lock().unwrap(),
        "httpCalls": *host.http_calls.lock().unwrap(),
        "fetchCalls": *host.fetch_calls.lock().unwrap(),
    })
}
