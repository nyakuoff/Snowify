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
pub mod ytm;
pub mod updater;
pub mod ytmusic;
pub mod ytdlp;

// The errno numbers are per-OS; this table was generated on Linux.
#[cfg(target_os = "linux")]
pub mod uv_errors;
#[cfg(not(target_os = "linux"))]
compile_error!("src/uv_errors.rs is the Linux errno map; regenerate it on this OS (see PORT.md)");
