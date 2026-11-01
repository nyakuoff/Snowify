//! Rust backend for Snowify. Each module ports one `src/main/*.js` file; see `PORT.md`.
//! `js` and `node` reproduce the language and runtime semantics those files rely on.

pub mod audio_cache;
pub mod cloud_auth;
pub mod cloud_backend;
pub mod deep_link;
pub mod discord;
pub mod electron_locale;
pub mod host;
pub mod firebase;
pub mod i18n;
pub mod ipc;
pub mod js;
pub mod jsv;
pub mod logger;
pub mod local_storage;
pub mod lyrics;
pub mod media;
pub mod media_proxy;
pub mod mm;
pub mod net;
pub mod node;
pub mod plugins;
pub mod real_host;
pub mod shell;
pub mod stream_cache;
pub mod themes;
pub mod thumbbar;
#[cfg(windows)]
pub mod login_items;
pub mod ytm;
pub mod updater;
pub mod ytmusic;
pub mod ytdlp;

// Node/libuv errno values and native error translation are platform-specific.
#[cfg(target_os = "linux")]
pub mod uv_errors;
#[cfg(windows)]
#[path = "uv_errors_windows.rs"]
pub mod uv_errors;
#[cfg(windows)]
mod windows_time;
