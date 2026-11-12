//! The Tauri shell: Electron's `src/main.js` lifecycle and `src/main/window.js`, plus the IPC
//! entry points the `window.snowify` shim calls (`ipc_invoke`, `ipc_send`).
//!
//! Handlers live with their modules; `invoke`/`send` below route channels to them. A channel
//! with no handler rejects exactly like Electron's `ipcRenderer.invoke` does.

use crate::{
    audio_cache,
    deep_link::{self, DeepLink, MainWindow, WindowDestroyed},
    electron_locale,
    i18n::{self, Translations},
    ipc::{self, HandlerResult, JsValue, Thrown},
    logger::{self, LogArg},
    media_proxy::{self, MediaProxy},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder, TrayIconEvent},
};

/// The shared main-process state (`src/main/context.js` plus module-level variables).
pub struct Shell {
    pub pending_deep_link: Mutex<Option<DeepLink>>,
    pub translations: Mutex<Translations>,
    pub proxy: MediaProxy,
    updater: std::sync::OnceLock<
        tokio::sync::Mutex<crate::updater::AutoUpdater<crate::updater::TauriBackend>>,
    >,
    updater_exit_ready: AtomicBool,
    updater_downloaded: AtomicBool,
    updater_install_pending: AtomicBool,
    minimize_to_tray: AtomicBool,
    close_ready: AtomicBool,
    force_quit: AtomicBool,
    tray: Mutex<Option<TrayIcon>>,
    /// `lyrics.js` module state (in-flight lookups).
    lyrics: std::sync::Arc<crate::lyrics::Lyrics>,
    /// `discord.js` module state.
    discord: crate::discord::Discord,
    /// `app.relaunch()` was called: start a new instance when this one exits.
    relaunch: AtomicBool,
    /// `ctx.ytmusic` / `ctx.ytmusicReady`.
    pub ytm: std::sync::Arc<crate::ytm::Ytm>,
    /// The stream-URL cache and the active download.
    streams: crate::ytmusic::Streams,
    /// `cloud-auth.js` module state and its Firebase instance.
    cloud: std::sync::Arc<crate::cloud_auth::CloudAuth>,
    firebase: std::sync::Arc<crate::cloud_backend::FirebaseBackend<crate::real_host::RealHost>>,
}

/// `process.platform`.
pub fn platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        os => os,
    }
}

fn shell(app: &AppHandle) -> tauri::State<'_, Shell> {
    app.state::<Shell>()
}

fn send_updater_status(app: &AppHandle, value: JsValue) {
    if value.get("status") == &JsValue::from("downloaded") {
        shell(app).updater_downloaded.store(true, Ordering::SeqCst);
    }
    send_to_renderer(app, "updater:status", vec![value]);
}

/// An explicit updater install has already applied the package: do not run the
/// install-on-quit hook again while Tauri's restart is waiting for the event loop.
pub(crate) fn prepare_update_restart(app: &AppHandle) {
    let s = shell(app);
    s.updater_exit_ready.store(true, Ordering::SeqCst);
    s.relaunch.store(false, Ordering::SeqCst);
}

/// `webContents.send(channel, ...args)`.
pub fn send_to_renderer(app: &AppHandle, channel: &str, args: Vec<JsValue>) {
    let args: Vec<Value> = args.iter().map(JsValue::encode).collect();
    let _ = app.emit_to(
        "main",
        "snowify://ipc",
        json!({ "channel": channel, "args": args }),
    );
}

/// `loadMainTranslations(override)` against the bundled `locales/*.json`.
fn load_main_translations(app: &AppHandle, override_locale: &JsValue) -> Result<(), Thrown> {
    let locale = i18n::resolve_locale(override_locale, &electron_locale::current())?;
    let assets = app.asset_resolver();
    let read = |name: &str| {
        assets
            .get(format!("locales/{name}"))
            .map(|a| String::from_utf8_lossy(&a.bytes).into_owned())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    };
    let t = Translations::load_with(read, locale).map_err(|e| Thrown::error(e.to_string()))?;
    *shell(app)
        .translations
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = t;
    Ok(())
}

// ─── The main window, as `handleDeepLink` and `second-instance` see it ───

struct Window(WebviewWindow);

impl MainWindow for Window {
    fn has_web_contents(&self) -> Result<bool, WindowDestroyed> {
        Ok(true)
    }
    fn send_deep_link(&self, link: &DeepLink) -> Result<(), WindowDestroyed> {
        let payload = JsValue::object([
            ("type", JsValue::from(link.kind.as_str())),
            ("id", JsValue::from(link.id.as_str())),
        ]);
        send_to_renderer(self.0.app_handle(), "app:deepLink", vec![payload]);
        Ok(())
    }
    fn is_minimized(&self) -> Result<bool, WindowDestroyed> {
        self.0.is_minimized().map_err(|_| WindowDestroyed)
    }
    fn restore(&self) -> Result<(), WindowDestroyed> {
        self.0.unminimize().map_err(|_| WindowDestroyed)
    }
    fn focus(&self) -> Result<(), WindowDestroyed> {
        self.0.set_focus().map_err(|_| WindowDestroyed)
    }
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

fn handle_deep_link(app: &AppHandle, url: &str) {
    let window = main_window(app).map(Window);
    let s = shell(app);
    let mut pending = s
        .pending_deep_link
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    deep_link::handle(
        url,
        window.as_ref().map(|w| w as &dyn MainWindow),
        &mut pending,
    );
}

pub fn on_second_instance(app: &AppHandle, argv: Vec<String>) {
    let window = main_window(app).map(Window);
    let s = shell(app);
    let mut pending = s
        .pending_deep_link
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // A failing window call throws out of the Electron event handler.
    if deep_link::on_second_instance(
        &argv,
        window.as_ref().map(|w| w as &dyn MainWindow),
        &mut pending,
    )
    .is_err()
    {
        uncaught_exception("TypeError: Object has been destroyed");
    }
}

/// Electron's default `uncaughtException` handler (the source registers none):
/// `dialog.showErrorBox(title, 'Uncaught Exception:\n' + error.stack)`, modal, then the app
/// carries on. Rust has no JS frames, so the stack is its first line only.
fn uncaught_exception(stack: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("A JavaScript error occurred in the main process")
        .set_description(format!("Uncaught Exception:\n{stack}"))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

// ─── Window (src/main/window.js) ───

fn create_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let s = shell(app);
    let boot = json!({
        "platform": platform(),
        "proxy": { "port": s.proxy.port, "token": s.proxy.token },
        "transportProbe": transport_test::enabled(),
    });
    let migration = {
        use crate::host::Host;
        let user_data = crate::real_host::RealHost { window: None }.user_data();
        match crate::local_storage::read_leveldb(&std::path::Path::new(&user_data).join("Local Storage/leveldb")) {
            Ok(map) if map.is_empty() => String::new(),
            Ok(map) => {
                logger::log(&[format!("[localStorage migration] prepared {} keys", map.len()).as_str().into()]);
                crate::local_storage::injection_script(&map)
            }
            Err(error) => {
                logger::error(&[format!("[localStorage migration] failed: {error}").as_str().into()]);
                String::new()
            }
        }
    };
    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Snowify")
        .inner_size(1500.0, 860.0)
        .min_inner_size(900.0, 600.0)
        .background_color(tauri::window::Color(0x12, 0x12, 0x12, 0xff))
        .initialization_script(format!(
            "{migration}\nwindow.__SNOWIFY_BOOT__ = {boot};\n(() => {{\n{}\n{}\n}})();",
            include_str!("../codec.js"),
            include_str!("../shim.js")
        ));
    builder = builder.decorations(false);
    if transport_test::enabled() {
        builder = builder.initialization_script(transport_test::page_script());
    }
    let window = builder.build()?;
    #[cfg(windows)]
    crate::thumbbar::install(
        &window,
        crate::thumbbar::buttons(&s.translations.lock().unwrap_or_else(|e| e.into_inner()), false),
    );

    #[cfg(target_os = "linux")]
    window.with_webview(|wv| {
        use webkit2gtk::WebViewExt;
        // Debug aid: mirror the renderer console to stdout (SNOWIFY_CONSOLE=1).
        if cfg!(debug_assertions) && std::env::var_os("SNOWIFY_CONSOLE").is_some() {
            use webkit2gtk::SettingsExt;
            if let Some(settings) = wv.inner().settings() {
                settings.set_enable_write_console_messages_to_stdout(true);
            }
        }
    })?;

    let app2 = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            let s = shell(&app2);
            if s.close_ready.load(Ordering::SeqCst) {
                return;
            }
            api.prevent_close();
            let Some(win) = main_window(&app2) else {
                return;
            };
            if s.minimize_to_tray.load(Ordering::SeqCst) && !s.force_quit.load(Ordering::SeqCst) {
                let _ = win.hide();
                return;
            }
            send_to_renderer(&app2, "app:before-close", vec![]);
            let app3 = app2.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                shell(&app3).close_ready.store(true, Ordering::SeqCst);
                if let Some(w) = main_window(&app3) {
                    let _ = w.close();
                }
            });
        }
    });
    Ok(window)
}

fn show_and_focus(app: &AppHandle) {
    if let Some(w) = main_window(app) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// A 16×16 tray icon from `assets/logo.png` (Electron: `nativeImage.resize({ width: 16, height: 16 })`).
fn tray_icon() -> tauri::image::Image<'static> {
    let logo = tauri::image::Image::from_bytes(include_bytes!("../../assets/logo.png"))
        .expect("bundled logo.png");
    let (w, h) = (logo.width() as usize, logo.height() as usize);
    let src = logo.rgba();
    let mut out = vec![0u8; 16 * 16 * 4];
    // Area average: each output pixel covers a (w/16)×(h/16) block.
    for oy in 0..16 {
        for ox in 0..16 {
            let (x0, x1, y0, y1) = (
                ox * w / 16,
                (ox + 1) * w / 16,
                oy * h / 16,
                (oy + 1) * h / 16,
            );
            let mut acc = [0u64; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += u64::from(src[(y * w + x) * 4 + c]);
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)).max(1) as u64;
            for (c, a) in acc.iter().enumerate() {
                out[(oy * 16 + ox) * 4 + c] = (a / n) as u8;
            }
        }
    }
    tauri::image::Image::new_owned(out, 16, 16)
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let s = shell(app);
    let mut tray = s.tray.lock().unwrap_or_else(|e| e.into_inner());
    if tray.is_some() {
        return Ok(());
    }
    let open = MenuItemBuilder::with_id("open", "Open Snowify").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Snowify").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&open)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&quit)
        .build()?;
    *tray = Some(
        TrayIconBuilder::new()
            .icon(tray_icon())
            .tooltip("Snowify")
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, e| match e.id().as_ref() {
                "open" => show_and_focus(app),
                "quit" => {
                    shell(app).force_quit.store(true, Ordering::SeqCst);
                    if let Some(w) = main_window(app) {
                        let _ = w.close();
                    }
                }
                _ => {}
            })
            .on_tray_icon_event(|tray, e| {
                if let TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    ..
                } = e
                {
                    show_and_focus(tray.app_handle());
                }
            })
            .build(app)?,
    );
    Ok(())
}

fn destroy_tray(app: &AppHandle) {
    shell(app)
        .tray
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
}

// ─── IPC entry points ───

#[tauri::command]
async fn ipc_invoke(app: AppHandle, channel: String, args: Vec<Value>) -> Result<Value, Value> {
    let args = args
        .iter()
        .map(JsValue::decode)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Value::from)?;
    // The rejection message travels wire-encoded: it can hold a lone surrogate.
    match invoke(&app, &channel, args).await {
        Some(Ok(v)) => crate::ipc::clone_reply(&v)
            .map_err(|e| JsValue::String(e.rejection_message(&channel)).encode()),
        Some(Err(t)) => Err(JsValue::String(t.rejection_message(&channel)).encode()),
        None => Err(Value::from(format!(
            "Error invoking remote method '{channel}': Error: No handler registered for '{channel}'"
        ))),
    }
}

#[tauri::command]
fn ipc_send(app: AppHandle, channel: String, args: Vec<Value>) -> Result<(), String> {
    let args = args
        .iter()
        .map(JsValue::decode)
        .collect::<Result<Vec<_>, _>>()?;
    send(&app, &channel, args);
    Ok(())
}

/// Argument `i`, `undefined` when the renderer passed fewer.
fn arg(args: &[JsValue], i: usize) -> &JsValue {
    args.get(i).unwrap_or(&JsValue::Undefined)
}

/// The `app.getPath` values cloud-auth reads.
fn cloud_paths() -> crate::cloud_auth::Paths {
    use crate::host::Host as _;
    crate::cloud_auth::Paths {
        user_data: crate::real_host::RealHost { window: None }.user_data(),
        image_roots: crate::cloud_backend::image_roots(),
        cwd: std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

/// The `onAuthStateChanged` listener cloud-auth registers: each queued callback sends
/// `auth:stateChanged` with the user's info, or does nothing while there is no window.
async fn deliver_auth_events(app: &AppHandle) {
    let s = shell(app);
    for user in s.firebase.take_events().await {
        if main_window(app).is_none() {
            continue;
        }
        let payload = crate::cloud_auth::state_payload(&s.cloud, &*s.firebase, user).await;
        send_to_renderer(app, "auth:stateChanged", vec![payload]);
    }
}

/// `getYtDlpPath()` for this app.
fn ytdlp_path(app: &AppHandle, host: &crate::real_host::RealHost) -> String {
    use crate::host::Host as _;
    use tauri::Manager;
    let resources = app.path().resource_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    crate::ytdlp::yt_dlp_path(&crate::ytdlp::Env::current(resources, &host.repo_root(), host.user_data()))
}

/// `ipcMain.handle` channels. `None`: no handler registered.
async fn invoke(app: &AppHandle, channel: &str, args: Vec<JsValue>) -> Option<HandlerResult> {
    if transport_test::enabled()
        && let Some(r) = transport_test::invoke(app, channel, &args)
    {
        return Some(r);
    }
    let host = crate::real_host::RealHost {
        window: main_window(app),
    };
    let a0 = arg(&args, 0).clone();
    Some(match channel {
        "discord:connect" => shell(app).discord.handle_connect().await,
        "discord:disconnect" => shell(app).discord.handle_disconnect().await,
        "discord:updatePresence" => shell(app).discord.handle_update(&a0).await,
        "discord:clearPresence" => shell(app).discord.handle_clear().await,
        "app:getVersion" => Ok(crate::updater::version().into()),
        "app:getLocale" => Ok(electron_locale::current().into()),
        "app:getChangelog" => crate::updater::changelog(&host, &a0).await,
        "app:getRecentReleases" => crate::updater::recent_releases(&host).await,
        "updater:check" => {
            let s = shell(app);
            let mut updater = s.updater.get().expect("updater initialized").lock().await;
            Ok(updater.check(true, &|v| send_updater_status(app, v)).await)
        }
        // `app.relaunch(); app.quit()`: `before-quit` cleanup now, then the normal window
        // close (which may hide to the tray and leave the relaunch pending).
        "app:restart" => {
            let s = shell(app);
            s.relaunch.store(true, Ordering::SeqCst);
            audio_cache::cleanup_cache_dir(&audio_cache::AUDIO_CACHE_DIR);
            if let Some(w) = main_window(app) {
                let _ = w.close();
            }
            Ok(JsValue::Undefined)
        }
        "yt:search" => crate::ytmusic::search(&shell(app).ytm, &host, &a0, arg(&args, 1)).await,
        "yt:searchSuggestions" => crate::ytmusic::search_suggestions(&shell(app).ytm, &host, &a0).await,
        "yt:artistInfo" => crate::ytmusic::artist_info(&shell(app).ytm, &host, &a0).await,
        "yt:searchPlaylists" => crate::ytmusic::search_playlists(&shell(app).ytm, &host, &a0).await,
        "yt:getPlaylistVideos" => crate::ytmusic::get_playlist_videos(&shell(app).ytm, &host, &a0).await,
        "yt:albumTracks" => crate::ytmusic::album_tracks(&shell(app).ytm, &host, &a0).await,
        "yt:searchArtists" => crate::ytmusic::search_artists(&shell(app).ytm, &host, &a0).await,
        "yt:searchAlbums" => crate::ytmusic::search_albums(&shell(app).ytm, &host, &a0).await,
        "yt:searchVideos" => crate::ytmusic::search_videos(&shell(app).ytm, &host, &a0).await,
        "yt:setCountry" => crate::ytmusic::set_country(&shell(app).ytm, &host, &a0).await,
        "yt:explore" => crate::ytmusic::explore(&shell(app).ytm, &host).await,
        "yt:charts" => crate::ytmusic::charts(&shell(app).ytm, &host).await,
        "yt:browseMood" => crate::ytmusic::browse_mood(&shell(app).ytm, &host, &a0, arg(&args, 1)).await,
        "yt:getTrackInfo" => crate::ytmusic::get_track_info(&shell(app).ytm, &host, &a0).await,
        "yt:getUpNexts" => crate::ytmusic::get_up_nexts(&shell(app).ytm, &host, &a0).await,
        "spotify:matchTrack" => crate::media::spotify_match_track(&shell(app).ytm, &host, &a0, arg(&args, 1)).await,
        "yt:getStreamUrl" => {
            let a = |i: usize| arg(&args, i).clone();
            crate::ytmusic::get_stream_url(&shell(app).streams, &ytdlp_path(app, &host), crate::host::Host::now_ms(&host), [a(0), a(1), a(2), a(3)]).await
        }
        "yt:getVideoStreamUrl" => crate::ytmusic::get_video_stream_url(&shell(app).streams, &ytdlp_path(app, &host), crate::host::Host::now_ms(&host), &a0, arg(&args, 1), arg(&args, 2)).await,
        "yt:downloadAudio" => crate::ytmusic::download_audio(&shell(app).streams, &ytdlp_path(app, &host), &audio_cache::AUDIO_CACHE_DIR, &a0, arg(&args, 1), arg(&args, 2)).await,
        "cache:deleteFile" => crate::ytmusic::delete_file(&audio_cache::AUDIO_CACHE_DIR, &a0),
        "cache:clear" => crate::ytmusic::clear_cache(&audio_cache::AUDIO_CACHE_DIR),
        "cache:cancelDownload" => crate::ytmusic::cancel_download(&shell(app).streams),
        "song:saveTo" => {
            let title = shell(app).translations.lock().unwrap_or_else(|e| e.into_inner()).mt("dialog.saveSong", &[]).map(|t| t.to_string()).unwrap_or_default();
            crate::ytmusic::save_to(&host, &ytdlp_path(app, &host), &title, &a0, arg(&args, 1), arg(&args, 2)).await
        }
        "net:httpGet" => {
            let headers = if *arg(&args, 1) == JsValue::Undefined { JsValue::Object(Vec::new()) } else { arg(&args, 1).clone() };
            crate::ytmusic::http_get(&host, &a0, &headers).await
        }
        "auth:signInWithEmail" | "auth:signUpWithEmail" | "auth:sendPasswordReset" | "auth:signOut" | "auth:getUser" | "profile:update"
        | "profile:readImage" | "profile:updateExtras" | "profile:get" | "cloud:save" | "cloud:load" => {
            let s = shell(app);
            let (c, fb, k, p, a1) = (&s.cloud, &*s.firebase, &crate::cloud_backend::OsKeyring, cloud_paths(), arg(&args, 1));
            let t = s.translations.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let r = match channel {
                "auth:signInWithEmail" => c.sign_in_with(fb, k, &p, &a0, a1, false).await,
                "auth:signUpWithEmail" => c.sign_in_with(fb, k, &p, &a0, a1, true).await,
                "auth:sendPasswordReset" => c.send_password_reset(fb, &a0).await,
                "auth:signOut" => c.sign_out(fb, k, &p).await,
                "auth:getUser" => c.get_user(fb).await,
                "profile:update" => c.update_profile(fb, &t, &a0).await,
                "profile:readImage" => c.read_image(&p, &a0),
                "profile:updateExtras" => c.update_extras(fb, &t, &a0).await,
                "profile:get" => c.get_profile(fb, &a0).await,
                "cloud:save" => c.save(fb, &t, &a0).await,
                _ => c.load(fb).await,
            };
            // The listener is async in JS: its sends don't hold up the handler's reply.
            let h = app.clone();
            tauri::async_runtime::spawn(async move { deliver_auth_events(&h).await });
            r
        }
        "lyrics:get" => {
            let a = |i: usize| arg(&args, i).clone();
            crate::lyrics::handle(shell(app).lyrics.clone(), std::sync::Arc::new(host), [a(0), a(1), a(2), a(3)]).await
        }
        "theme:scan" => crate::themes::scan(&host).await,
        "theme:load" => crate::themes::load(&host, &a0).await,
        "theme:add" => crate::themes::add(&host).await,
        "theme:reload" => crate::themes::reload(&host, &a0).await,
        "theme:remove" => crate::themes::remove(&host, &a0).await,
        "theme:openFolder" => crate::themes::open_folder(&host).await,
        "themes:getInstalled" => crate::themes::get_installed(&host).await,
        "themes:install" => crate::themes::install(&host, &a0).await,
        "themes:uninstallMarketplace" => crate::themes::uninstall_marketplace(&host, &a0).await,
        "spotify:pickCsv" => crate::media::spotify_pick_csv(&host).await,
        "playlist:exportCsv" => crate::media::export_playlist_csv(&host, &a0, arg(&args, 1)).await,
        "library:export" => crate::media::export_library(&host, &a0).await,
        "library:import" => crate::media::import_library(&host).await,
        "playlist:pickImage" => crate::media::pick_image(&host).await,
        "playlist:saveImage" => crate::media::save_image(&host, &a0, arg(&args, 1)).await,
        "playlist:deleteImage" => crate::media::delete_image(&a0).await,
        "local:pickAudioFiles" => crate::media::pick_audio_files(&host).await,
        "local:pickFolder" => crate::media::pick_folder(&host).await,
        "local:scanFolder" => crate::media::scan_folder(&a0).await,
        "local:copyToPlaylistFolder" => crate::media::copy_to_playlist_folder(&host, &a0, arg(&args, 1)).await,
        "plugins:getRegistry" => crate::plugins::get_registry(&host).await,
        "plugins:getInstalled" => crate::plugins::get_installed(&host).await,
        "plugins:install" => crate::plugins::install(&host, &a0).await,
        "plugins:uninstall" => crate::plugins::uninstall(&host, &a0).await,
        "plugins:getFiles" => crate::plugins::get_files(&host, &a0).await,
        "app:getPendingDeepLink" => {
            let link = deep_link::take_pending(
                &mut shell(app)
                    .pending_deep_link
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()),
            );
            Ok(link.map_or(JsValue::Null, |l| {
                JsValue::object([
                    ("type", JsValue::from(l.kind.as_str())),
                    ("id", JsValue::from(l.id)),
                ])
            }))
        }
        "shell:openExternal" => {
            if let Some(url) = arg(&args, 0).as_str()
                && (url.starts_with("https://") || url.starts_with("http://"))
            {
                open_external(&url);
            }
            Ok(JsValue::Undefined)
        }
        "app:getLogs" => {
            let logs = logger::LOGS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entries();
            Ok(JsValue::Array(
                logs.iter().map(logger::LogEntry::to_js).collect(),
            ))
        }
        "app:appendLog" => (|| {
            let entry = arg(&args, 0);
            let level = ipc::get_prop(entry, "level")?;
            let level = if level.truthy() {
                level
            } else {
                JsValue::from("log")
            };
            let msg = ipc::get_prop(entry, "msg")?;
            let msg = if msg.truthy() { msg } else { JsValue::from("") };
            logger::LOGS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .capture(level, &[LogArg::from_js(&msg)], logger::now_ms());
            Ok(JsValue::Undefined)
        })(),
        "app:setLocale" => load_main_translations(app, arg(&args, 0)).map(|()| JsValue::Undefined),
        _ => return None,
    })
}

/// `ipcMain.on` channels; unknown channels are ignored, as in Electron.
fn send(app: &AppHandle, channel: &str, args: Vec<JsValue>) {
    if transport_test::enabled() {
        transport_test::send(channel, &args);
    }
    let win = main_window(app);
    match channel {
        "window:startDragging" => {
            if let Some(w) = win {
                let _ = w.start_dragging();
            }
        }
        "window:minimize" => {
            if let Some(w) = win {
                let _ = w.minimize();
            }
        }
        "window:maximize" => {
            if let Some(w) = win {
                if w.is_maximized().unwrap_or(false) {
                    let _ = w.unmaximize();
                } else {
                    let _ = w.maximize();
                }
            }
        }
        "window:close" => {
            if let Some(w) = win {
                let _ = w.close();
            }
        }
        "window:setMinimizeToTray" => {
            let enabled = arg(&args, 0).truthy();
            shell(app).minimize_to_tray.store(enabled, Ordering::SeqCst);
            if enabled {
                let _ = setup_tray(app);
            } else {
                destroy_tray(app);
            }
        }
        // Electron's setLoginItemSettings is a no-op on Linux (Windows only here).
        "window:setOpenAtLogin" => {
            #[cfg(windows)]
            {
                // Electron's fallback AUMID uses the executable's ProductName: the
                // stock Electron executable in dev, Snowify in the packaged NSIS app.
                let product = if cfg!(debug_assertions) { "Electron" } else {
                    app.config().product_name.as_deref().unwrap_or("Snowify")
                };
                if let Ok(items) = crate::login_items::LoginItems::current(product) {
                    items.set(&crate::login_items::Settings {
                        open_at_login: arg(&args, 0).truthy(), ..Default::default()
                    });
                }
            }
        }
        // Thumbnail toolbar buttons exist only on Windows.
        "thumbar:updateState" => {
            #[cfg(windows)]
            if let Some(w) = win {
                let buttons = crate::thumbbar::buttons(&shell(app).translations.lock().unwrap_or_else(|e| e.into_inner()), arg(&args, 0).truthy());
                crate::thumbbar::update(&w, buttons);
            }
        }
        "updater:install" => {
            // electron-updater coalesces downloadUpdate calls while a download is in flight.
            // Do not queue a second send that would turn into an install after it finishes.
            if shell(app).updater_install_pending.swap(true, Ordering::SeqCst) {
                return;
            }
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let s = shell(&app);
                let mut updater = s.updater.get().expect("updater initialized").lock().await;
                updater.install(&|v| send_updater_status(&app, v)).await;
                s.updater_install_pending.store(false, Ordering::SeqCst);
            });
        }
        "app:close-ready" => {
            shell(app).close_ready.store(true, Ordering::SeqCst);
            if let Some(w) = win {
                let _ = w.close();
            }
        }
        _ => {}
    }
}

/// `shell.openExternal(url)`: hands the URL to the desktop's handler.
fn open_external(url: &str) {
    #[cfg(target_os = "linux")]
    let cmd = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(windows)]
    let cmd = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    let _ = cmd;
}

// ─── Lifecycle (src/main.js) ───

pub fn run() {
    let proxy = media_proxy::start().expect("start the media proxy");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            on_second_instance(app, argv)
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Shell {
            pending_deep_link: Mutex::new(None),
            translations: Mutex::new(Translations::default()),
            proxy,
            updater: std::sync::OnceLock::new(),
            updater_exit_ready: AtomicBool::new(false),
            updater_downloaded: AtomicBool::new(false),
            updater_install_pending: AtomicBool::new(false),
            minimize_to_tray: AtomicBool::new(false),
            close_ready: AtomicBool::new(false),
            force_quit: AtomicBool::new(false),
            tray: Mutex::new(None),
            lyrics: Default::default(),
            discord: Default::default(),
            relaunch: AtomicBool::new(false),
            ytm: Default::default(),
            streams: crate::ytmusic::Streams::desktop(),
            cloud: Default::default(),
            firebase: std::sync::Arc::new(crate::cloud_backend::FirebaseBackend::new(std::sync::Arc::new(crate::real_host::RealHost { window: None }))),
        })
        .invoke_handler(tauri::generate_handler![ipc_invoke, ipc_send])
        .setup(|app| {
            let handle = app.handle().clone();
            let _ = shell(&handle).updater.set(tokio::sync::Mutex::new(
                crate::updater::AutoUpdater::new(
                    crate::updater::TauriBackend::new(handle.clone()), cfg!(debug_assertions),
                ),
            ));
            #[cfg(any(target_os = "linux", windows))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let _ = app.deep_link().register("snowify");
            }
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let h = handle.clone();
                app.deep_link().on_open_url(move |e| {
                    for url in e.urls() {
                        handle_deep_link(&h, url.as_str());
                    }
                });
            }
            if let Err(e) = load_main_translations(&handle, &JsValue::Undefined) {
                logger::error(&[
                    "[i18n]".into(),
                    e.rejection_message("startup").to_string_lossy().into(),
                ]);
            }
            // `registerCloudAuth(ipcMain, ctx)`: the listener's initial callback (no window yet,
            // so it is dropped), then `_autoSignIn()` running alongside the rest of startup.
            {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    deliver_auth_events(&h).await;
                    let s = shell(&h);
                    crate::cloud_auth::auto_sign_in(&*s.firebase, &crate::cloud_backend::OsKeyring, &cloud_paths()).await;
                    deliver_auth_events(&h).await;
                });
            }
            // The rest of `app.whenReady()`: `await initYTMusic(ctx)` comes before the window.
            use crate::host::Host as _;
            use tauri::Manager;
            let resources = app.path().resource_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            tauri::async_runtime::spawn(async move {
                let host = crate::real_host::RealHost { window: None };
                if let Err(e) = crate::ytmusic::init(&shell(&handle).ytm, &host).await {
                    logger::error(&["[YTMusic] Initialization failed (will retry on first use):".into(), crate::logger::LogArg::from_js(&e.message())]);
                }
                if let Err(e) = create_window(&handle) {
                    logger::error(&["[window]".into(), e.to_string().as_str().into()]);
                    return;
                }
                // `updateYtDlp()`: fire-and-forget.
                let env = crate::ytdlp::Env::current(resources, &host.repo_root(), host.user_data());
                tauri::async_runtime::spawn(async move {
                    if let Some(update) = crate::ytdlp::update_yt_dlp(&env) {
                        update.await;
                    }
                });
                // `initAutoUpdater(ctx)`.
                let delay = shell(&handle).updater.get().unwrap().lock().await.init();
                if let Some(delay) = delay {
                    let h = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(delay).await;
                        let s = shell(&h);
                        s.updater.get().unwrap().lock().await
                            .check(false, &|v| send_updater_status(&h, v)).await;
                    });
                }
                // Cold-start deep link (Linux/Windows pass it as an argument).
                let argv: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
                if let Some(url) = deep_link::find_arg(&argv) {
                    handle_deep_link(&handle, url);
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Snowify")
        .run(|app, event| {
            // Actual application quit, after the renderer close handshake. Hiding to the
            // tray never reaches this hook. Wait for an in-flight updater operation before
            // installing, then allow the second exit request through.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                if !cfg!(debug_assertions) && code.unwrap_or(0) == 0
                    && shell(app).updater_downloaded.load(Ordering::SeqCst)
                    && !shell(app).updater_exit_ready.swap(true, Ordering::SeqCst)
                {
                    api.prevent_exit();
                    let h = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let s = shell(&h);
                        s.updater.get().unwrap().lock().await
                            .quit(&|v| send_updater_status(&h, v));
                        h.exit(0);
                    });
                }
            }
            if let tauri::RunEvent::Exit = event {
                audio_cache::cleanup_cache_dir(&audio_cache::AUDIO_CACHE_DIR);
                // `app.relaunch()`: process.execPath with process.argv.slice(1).
                if shell(app).relaunch.load(Ordering::SeqCst)
                    && let Ok(exe) = std::env::current_exe()
                {
                    let _ = std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn();
                }
            }
        });
}

/// The transport harness's main-process half (debug builds, `SNOWIFY_TRANSPORT_TEST=1`):
/// the same handlers as `parity/electron/transport/main.js`.
mod transport_test {
    use super::*;

    const CORPUS: &str = include_str!("../parity/corpus/transport.json");

    /// The probe page: corpus expressions compiled in as closures (the page CSP forbids eval).
    pub fn page_script() -> String {
        if let Ok(fixtures) = std::env::var("SNOWIFY_MEDIA_TEST") {
            return format!(
                "window.stop();\nif (!document.body) document.documentElement.appendChild(document.createElement('body'));\nwindow.__MEDIA_FIXTURES__ = {fixtures};\nwindow.addEventListener('DOMContentLoaded', () => {{}});\n{}",
                include_str!("../parity/media-probe.js"),
            );
        }
        let corpus: Value = serde_json::from_str(CORPUS).unwrap_or_default();
        let values: Vec<String> = corpus["values"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|e| format!("() => ({e})"))
            .collect();
        format!(
            "window.stop();\nif (!document.body) document.documentElement.appendChild(document.createElement('body'));\nwindow.__PROBE_CORPUS_JSON__ = {CORPUS};\nwindow.__PROBE_VALUES__ = [{}];\n{}\n{}",
            values.join(",\n"),
            include_str!("../parity/describe.js"),
            include_str!("../parity/transport-probe.js"),
        )
    }

    static SEEN: Mutex<Vec<Value>> = Mutex::new(Vec::new());
    static ECHOED: Mutex<Vec<Option<JsValue>>> = Mutex::new(Vec::new());

    /// Debug builds only: `SNOWIFY_TRANSPORT_TEST=1` (IPC seam) or `SNOWIFY_MEDIA_TEST=<fixtures
    /// JSON>` (media origin) replace the app page with a probe page.
    pub fn enabled() -> bool {
        cfg!(debug_assertions)
            && (std::env::var_os("SNOWIFY_TRANSPORT_TEST").is_some()
                || std::env::var_os("SNOWIFY_MEDIA_TEST").is_some())
    }

    /// The values `transport.json`'s `throws` evaluate to, in order.
    fn thrown(i: usize) -> Thrown {
        let err = |name: &str, message: &str| Thrown::Error {
            name: name.into(),
            message: message.into(),
        };
        match i {
            0 => err("Error", "main failed"),
            1 => err("TypeError", "main type"),
            2 => err("Error", "coded"),
            3 => Thrown::Value("a string".into()),
            4 => Thrown::Value(JsValue::Number(42.0)),
            5 => Thrown::Value(JsValue::Undefined),
            6 => Thrown::Value(JsValue::Null),
            7 => Thrown::Value(JsValue::object([(
                "message",
                JsValue::from("plain object"),
            )])),
            8 => err("Error", "custom"),
            9 => err("Error", ""),
            _ => err("Error", "multi\nline"),
        }
    }

    pub fn invoke(app: &AppHandle, channel: &str, args: &[JsValue]) -> Option<HandlerResult> {
        Some(match channel {
            "echo" => {
                SEEN.lock().unwrap().push(json!({ "argc": args.len(), "args": args.iter().map(JsValue::encode).collect::<Vec<_>>() }));
                let i = arg(args, 1).as_number().unwrap_or(0.0) as usize;
                let mut echoed = ECHOED.lock().unwrap();
                let len = echoed.len().max(i + 1);
                echoed.resize(len, None);
                echoed[i] = Some(arg(args, 0).clone());
                Ok(arg(args, 0).clone())
            }
            "echoArity" => Ok(JsValue::Number(args.len() as f64)),
            "throw" => Err(thrown(arg(args, 0).as_number().unwrap_or(0.0) as usize)),
            "emitAll" => {
                let echoed = ECHOED.lock().unwrap().clone();
                for (i, v) in echoed.into_iter().enumerate() {
                    if let Some(v) = v {
                        send_to_renderer(app, "ev", vec![JsValue::Number(i as f64), v]);
                    }
                }
                send_to_renderer(app, "evDone", vec![]);
                Ok(JsValue::Undefined)
            }
            "done" => {
                let results = arg(args, 0).as_str().unwrap_or_default();
                let seen = SEEN.lock().unwrap().clone();
                println!("{}", json!({ "seen": seen, "results": results }));
                app.exit(0);
                Ok(JsValue::Undefined)
            }
            _ => return None,
        })
    }

    pub fn send(channel: &str, args: &[JsValue]) {
        if channel == "send" {
            SEEN.lock().unwrap().push(json!({ "send": true, "args": args.iter().map(JsValue::encode).collect::<Vec<_>>() }));
        }
    }
}
