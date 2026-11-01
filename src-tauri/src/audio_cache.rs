//! Port of `src/main/audio-cache.js`: the temp dir downloaded audio is cached in.
//!
//! The source's `_activeDownloadProc` holder moves with its only user, `ytmusic.js`.

use crate::{logger, node};
use std::sync::LazyLock;

/// `path.join(os.tmpdir(), 'snowify-audio-cache')`, evaluated once like the module constant.
pub static AUDIO_CACHE_DIR: LazyLock<String> =
    LazyLock::new(|| node::path::join(&[&node::tmpdir(), "snowify-audio-cache"]));

/// `ensureCacheDir()`: creates `dir` (recursively) unless something already exists there,
/// even a file. Fails with Node's `mkdir` error.
pub fn ensure_cache_dir(dir: &str) -> Result<&str, node::FsError> {
    if !node::exists_sync(dir) {
        node::mkdir_recursive_sync(dir)?;
    }
    Ok(dir)
}

/// `cleanupCacheDir()`: removes `dir` if it exists; a failure is logged as a warning.
pub fn cleanup_cache_dir(dir: &str) {
    if node::exists_sync(dir)
        && let Err(e) = node::rm_recursive_force_sync(dir)
    {
        logger::warn(&["Cache cleanup:".into(), e.message.into()]);
    }
}
