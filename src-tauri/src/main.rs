// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The AppImage runtime (linuxdeploy's AppRun) exports environment for things the AppImage
    // does not ship; normalize it before Tauri starts any thread or child process.
    #[cfg(target_os = "linux")]
    if let Some(appdir) = std::env::var_os("APPDIR") {
        // SAFETY: single-threaded startup, before Tauri's runtime starts any thread.
        unsafe {
            // PYTHONHOME/PYTHONPATH point at a Python that isn't bundled. The bundled yt-dlp is
            // a Python zipapp and a system `yt-dlp` is one too; either inherits them and dies
            // with "Failed to import encodings".
            std::env::remove_var("PYTHONHOME");
            std::env::remove_var("PYTHONPATH");
            // The GStreamer hook points GST_PLUGIN_SCANNER_1_0 at an empty directory on Arch;
            // the helper that ships with the bundled plugins lives beside them. Without this,
            // WebKitGTK cannot load plugins and its media backend dies with NULL-instance GLib
            // criticals.
            let gst = std::path::Path::new(&appdir).join("usr/lib/gstreamer-1.0");
            let scanner = gst.join("gst-plugin-scanner");
            if scanner.is_file() {
                std::env::set_var("GST_PLUGIN_SCANNER_1_0", &scanner);
                std::env::set_var("GST_PLUGIN_SCANNER", &scanner);
            }
        }
    }
    snowify::shell::run();
}
