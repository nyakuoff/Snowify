//! Port of `src/main/updater.js`: version/locale, the GitHub changelog and release list,
//! and the dev-mode updater paths. The packaged update flow (electron-updater) is replaced
//! by Tauri's updater at the cut-over (Risk 2), where its signing key is set up.

use crate::{
    host::{Host, NetError},
    ipc::{self, HandlerResult, JsString, JsValue, to_js_string},
    js,
    logger::{self, LogArg},
    net::JsErr,
};

const RELEASES: &str = "https://api.github.com/repos/nyakuoff/Snowify/releases";

/// `app.getVersion()`: the version in `package.json`.
pub fn version() -> String {
    let pkg: serde_json::Value = serde_json::from_str(include_str!("../../package.json")).expect("package.json");
    pkg["version"].as_str().unwrap_or_default().to_owned()
}

/// A thrown `TypeError` (nothing enumerable, so it logs as `{}`).
fn type_error() -> JsErr {
    JsErr { name: "TypeError".into(), message: JsString::default(), props: Vec::new() }
}

/// The source's inline `https.get` promise: body on 200, else `GitHub API <status>`;
/// `Timeout` after 10 s idle; Node's error on failure.
async fn github_get<H: Host>(host: &H, url: &str) -> Result<Vec<u16>, JsErr> {
    let headers = [("User-Agent", "Snowify"), ("Accept", "application/vnd.github+json")];
    match host.https_get(url, &headers, 10_000).await {
        Ok(res) if res.status == 200 => Ok(String::from_utf8_lossy(&res.body).encode_utf16().collect()),
        Ok(res) => Err(JsErr::error(format!("GitHub API {}", res.status))),
        Err(NetError::Timeout) => Err(JsErr::error("Timeout")),
        Err(NetError::Failed { name, message, props }) => Err(JsErr { name, message: message.into(), props }),
    }
}

fn get(v: &JsValue, k: &str) -> Result<JsValue, JsErr> {
    ipc::get_prop(v, k).map_err(|_| type_error())
}

/// `v?.replace(/^v/, '')`: undefined when nullish, a TypeError for a non-string.
fn strip_v(v: &JsValue) -> Result<JsValue, JsErr> {
    match v {
        JsValue::Undefined | JsValue::Null => Ok(JsValue::Undefined),
        JsValue::String(s) => Ok(JsValue::String(JsString(s.0.strip_prefix(&[b'v' as u16]).unwrap_or(&s.0).to_vec()))),
        _ => Err(type_error()),
    }
}

fn or(v: JsValue, d: JsValue) -> JsValue {
    if v.truthy() { v } else { d }
}

fn log_failure(label: &str, e: &JsErr) {
    logger::error(&[label.into(), LogArg::from_js(&e.log_json())]);
}

/// `app:getChangelog(version)`.
pub async fn changelog<H: Host>(host: &H, version: &JsValue) -> HandlerResult {
    let r = async {
        // `version.startsWith(...)`: strings and String objects have it.
        let text = match version {
            JsValue::String(s) => s.0.clone(),
            JsValue::Boxed(b) if matches!(**b, JsValue::String(_)) => to_js_string(b).encode_utf16().collect(),
            JsValue::Shared(_, inner) if matches!(**inner, JsValue::String(_)) => to_js_string(inner).encode_utf16().collect(),
            _ => return Err(type_error()),
        };
        let mut tag: Vec<u16> = Vec::new();
        if text.first() != Some(&(b'v' as u16)) {
            tag.push(b'v' as u16);
        }
        tag.extend(&text);
        let url = format!("{RELEASES}/tags/{}", String::from_utf16_lossy(&tag));
        let body = github_get(host, &url).await?;
        let release = js::json_parse(&body).map_err(|m| JsErr { name: "SyntaxError".into(), message: m, props: Vec::new() })?;
        let mut v_name: Vec<u16> = vec![b'v' as u16];
        v_name.extend(&text);
        Ok(JsValue::object([
            ("version", or(strip_v(&get(&release, "tag_name")?)?, version.clone())),
            ("name", or(get(&release, "name")?, JsValue::String(JsString(v_name)))),
            ("body", or(get(&release, "body")?, "".into())),
            ("date", or(get(&release, "published_at")?, "".into())),
            ("url", or(get(&release, "html_url")?, "".into())),
        ]))
    }
    .await;
    Ok(r.unwrap_or_else(|e| {
        log_failure("Failed to fetch changelog:", &e);
        JsValue::Null
    }))
}

/// `app:getRecentReleases`.
pub async fn recent_releases<H: Host>(host: &H) -> HandlerResult {
    let r = async {
        let body = github_get(host, &format!("{RELEASES}?per_page=20")).await?;
        let list = js::json_parse(&body).map_err(|m| JsErr { name: "SyntaxError".into(), message: m, props: Vec::new() })?;
        let JsValue::Array(items) = list else { return Err(type_error()) };
        let mut out = Vec::new();
        for r in &items {
            let tag = get(r, "tag_name")?;
            let version = or(strip_v(&tag)?, "".into());
            let name = or(get(r, "name")?, or(tag, "".into()));
            let body = or(get(r, "body")?, "".into());
            let date = or(get(r, "published_at")?, "".into());
            let url = or(get(r, "html_url")?, "".into());
            let assets = match or(get(r, "assets")?, JsValue::Array(Vec::new())) {
                JsValue::Array(list) => list
                    .iter()
                    .map(|a| {
                        Ok(JsValue::object([
                            ("name", get(a, "name")?),
                            ("size", get(a, "size")?),
                            ("url", get(a, "browser_download_url")?),
                        ]))
                    })
                    .collect::<Result<Vec<_>, JsErr>>()?,
                _ => return Err(type_error()),
            };
            out.push(JsValue::object([
                ("version", version),
                ("name", name),
                ("body", body),
                ("date", date),
                ("url", url),
                ("assets", JsValue::Array(assets)),
            ]));
        }
        Ok(JsValue::Array(out))
    }
    .await;
    Ok(r.unwrap_or_else(|e| {
        log_failure("Failed to fetch releases:", &e);
        JsValue::Array(Vec::new())
    }))
}

/// `sendUpdateStatus(ctx, status, info)`'s payload.
pub fn status(status: &str, info: Vec<(&str, JsValue)>) -> JsValue {
    let mut props = vec![("status", JsValue::from(status))];
    props.extend(info);
    JsValue::object(props)
}

/// `initAutoUpdater` in a dev build.
pub fn init_dev() {
    logger::log(&["Auto-updater disabled in dev mode".into()]);
}

/// `updater:check` in a dev build: the status to send, then `null`.
pub fn check_dev() -> JsValue {
    status("error", vec![("message", "Auto-update is not available in dev mode".into())])
}
