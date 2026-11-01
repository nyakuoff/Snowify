//! Port of `src/main/updater.js`: version/locale, the GitHub changelog and release list,
//! and the packaged auto-update policy, backed by tauri-plugin-updater.

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
    status(
        "error",
        vec![("message", "Auto-update is not available in dev mode".into())],
    )
}

/// Metadata shared by the plugin adapter and the scripted parity backend.
#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    /// Tauri's `body` is the manifest's `notes`; absent notes stay JS undefined.
    pub body: Option<String>,
}

/// Keep the optional message separate from the logged rejection: JS can reject null,
/// and Error.message is non-enumerable (so an Error logs as `{}`).
#[derive(Debug)]
pub struct UpdateError {
    pub message: JsValue,
    pub logged: JsValue,
}

impl From<JsErr> for UpdateError {
    fn from(err: JsErr) -> Self {
        Self {
            message: JsValue::String(err.message.clone()),
            logged: err.log_json(),
        }
    }
}

impl UpdateError {
    fn log(&self, label: &str) {
        logger::error(&[label.into(), LogArg::from_js(&self.logged)]);
    }
}

/// All network/installer effects stay behind this Rust-only seam. A successful download
/// stages verified bytes in the backend; later checks must not replace that staged update.
pub trait Backend: Send {
    fn check(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<Update>, UpdateError>> + Send;
    /// Electron returns updateInfo.version even when the remote release is not newer.
    fn checked_version(&self) -> Option<String>;
    fn download(
        &mut self,
        progress: &mut (dyn FnMut(f64) + Send),
    ) -> impl std::future::Future<Output = Result<String, UpdateError>> + Send;
    fn install(&mut self, silent: bool, relaunch: bool) -> Result<(), UpdateError>;
}

fn round_percent(percent: f64) -> f64 {
    // Adding 0.5 first would mis-round the representable value just below 0.5.
    if (-0.5..0.0).contains(&percent) {
        return -0.0;
    }
    let floor = percent.floor();
    if percent - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

pub type StatusSink<'a> = &'a (dyn Fn(JsValue) + Send + Sync);

/// Renderer-facing policy, independent of Tauri and testable without network or installers.
pub struct AutoUpdater<B> {
    pub backend: B,
    dev: bool,
    downloaded: bool,
    install_triggered: bool,
}

impl<B: Backend> AutoUpdater<B> {
    pub fn new(backend: B, dev: bool) -> Self {
        Self {
            backend,
            dev,
            downloaded: false,
            install_triggered: false,
        }
    }

    /// No auto-download; the shell schedules exactly one check after this delay.
    pub fn init(&self) -> Option<std::time::Duration> {
        if self.dev {
            init_dev();
            None
        } else {
            Some(std::time::Duration::from_secs(5))
        }
    }

    pub fn error(&self, err: &UpdateError, send: StatusSink<'_>) {
        send(status(
            "error",
            vec![(
                "message",
                or(err.message.clone(), "Update check failed".into()),
            )],
        ));
    }

    /// `manual=false` is the startup check: swallow its rejection without a console log.
    pub async fn check(&mut self, manual: bool, send: StatusSink<'_>) -> JsValue {
        if self.dev {
            if manual {
                send(check_dev());
            }
            return JsValue::Null;
        }
        send(status("checking", vec![]));
        match self.backend.check().await {
            Ok(update) => {
                if let Some(update) = update {
                    send(status(
                        "available",
                        vec![
                            ("version", update.version.as_str().into()),
                            (
                                "releaseNotes",
                                update.body.map(JsValue::from).unwrap_or_default(),
                            ),
                        ],
                    ));
                    or(update.version.into(), JsValue::Null)
                } else {
                    send(status("up-to-date", vec![]));
                    self.backend
                        .checked_version()
                        .map(|v| or(v.into(), JsValue::Null))
                        .unwrap_or(JsValue::Null)
                }
            }
            Err(err) => {
                self.error(&err, send);
                if manual {
                    err.log("Update check error:");
                }
                JsValue::Null
            }
        }
    }

    /// The first send downloads only; a subsequent send installs with UI and forces relaunch.
    pub async fn install(&mut self, send: StatusSink<'_>) {
        if self.dev {
            return;
        }
        if self.downloaded {
            self.install_triggered = true;
            if let Err(err) = self.backend.install(false, true) {
                self.error(&err, send);
            }
            return;
        }
        let mut progress = |percent: f64| {
            // JS Math.round chooses +infinity at ties, unlike f64::round for negative ties.
            send(status(
                "downloading",
                vec![("percent", JsValue::Number(round_percent(percent)))],
            ));
        };
        match self.backend.download(&mut progress).await {
            Ok(version) => {
                self.downloaded = true;
                send(status("downloaded", vec![("version", version.into())]));
            }
            Err(err) => {
                // electron-updater emits error, then downloadUpdate's rejection reaches catch.
                self.error(&err, send);
                err.log("Update download error:");
                send(status(
                    "error",
                    vec![("message", or(err.message.clone(), "Download failed".into()))],
                ));
            }
        }
    }

    /// autoInstallOnAppQuit: install without reopening on an ordinary quit.
    /// Consume the flag first so a reentrant exit cannot install twice.
    pub fn quit(&mut self, send: StatusSink<'_>) {
        if !self.dev && !self.install_triggered && std::mem::take(&mut self.downloaded) {
            if let Err(err) = self.backend.install(true, false) {
                self.error(&err, send);
            }
        }
    }
}

/// Real Tauri adapter. Download and signature verification finish before `downloaded` is
/// emitted. The verified package lives until explicit install or actual app exit.
pub struct TauriBackend {
    app: tauri::AppHandle,
    candidate: Option<tauri_plugin_updater::Update>,
    staged: Option<(tauri_plugin_updater::Update, Vec<u8>)>,
    checked: Option<String>,
}

impl TauriBackend {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            app,
            candidate: None,
            staged: None,
            checked: None,
        }
    }
}

fn plugin_error(err: tauri_plugin_updater::Error) -> UpdateError {
    JsErr::error(err.to_string()).into()
}

impl Backend for TauriBackend {
    async fn check(&mut self) -> Result<Option<Update>, UpdateError> {
        use tauri_plugin_updater::UpdaterExt;
        // Tauri returns None for a non-newer version, but Electron returns that manifest's
        // version. Capture it without accepting a downgrade or doing a second request.
        let checked = std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture = checked.clone();
        let app = self.app.clone();
        let updater = self
            .app
            .updater_builder()
            .version_comparator(move |current, remote| {
                *capture.lock().unwrap() = Some(remote.version.to_string());
                remote.version > current
            })
            .on_before_exit(move || {
                crate::audio_cache::cleanup_cache_dir(&crate::audio_cache::AUDIO_CACHE_DIR);
                app.cleanup_before_exit();
            })
            .build()
            .map_err(plugin_error)?;
        self.candidate = updater.check().await.map_err(plugin_error)?;
        self.checked = checked.lock().unwrap().clone();
        Ok(self.candidate.as_ref().map(|u| Update {
            version: u.version.clone(),
            body: u.body.clone(),
        }))
    }

    fn checked_version(&self) -> Option<String> {
        self.checked.clone()
    }

    async fn download(
        &mut self,
        progress: &mut (dyn FnMut(f64) + Send),
    ) -> Result<String, UpdateError> {
        let update = self
            .candidate
            .as_ref()
            .ok_or_else(|| UpdateError::from(JsErr::error("Please check update first")))?;
        let mut received = 0u64;
        let mut last_percent = None;
        let bytes = update
            .download(
                |chunk, total| {
                    received += chunk as u64;
                    let percent = total.filter(|n| *n > 0)
                        .map(|n| received as f64 / n as f64 * 100.0).unwrap_or(0.0);
                    last_percent = Some(percent);
                    progress(percent);
                },
                || {},
            )
            .await
            .map_err(plugin_error)?;
        if last_percent != Some(100.0) { progress(100.0); }
        let version = update.version.clone();
        self.staged = Some((update.clone(), bytes));
        Ok(version)
    }

    fn install(&mut self, _silent: bool, relaunch: bool) -> Result<(), UpdateError> {
        if let Some((update, bytes)) = self.staged.as_ref() {
            // Windows is configured for basic UI, matching quitAndInstall(false, true).
            // The plugin launches the installer and exits on Windows; on Linux we restart.
            update
                .clone()
                .restart_after_install(relaunch)
                .install(bytes)
                .map_err(plugin_error)?;
            self.staged = None;
            if relaunch {
                crate::shell::prepare_update_restart(&self.app);
                self.app.restart();
            }
        }
        Ok(())
    }
}
