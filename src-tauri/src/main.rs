// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The AppImage runtime (linuxdeploy's AppRun) exports PYTHONHOME/PYTHONPATH pointing at a
    // Python it does not ship. The bundled yt-dlp is a Python zipapp and a system `yt-dlp` is
    // one too; either inherits them and dies with "Failed to import encodings". The app itself
    // bundles no Python, so drop the variables before any child is spawned.
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPDIR").is_some() {
        // SAFETY: single-threaded startup, before Tauri's runtime starts any thread.
        unsafe {
            std::env::remove_var("PYTHONHOME");
            std::env::remove_var("PYTHONPATH");
        }
    }
    snowify::shell::run();
}
