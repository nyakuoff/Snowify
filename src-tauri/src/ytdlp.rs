//! Port of `src/main/ytdlp.js` (working-tree version): where the yt-dlp binary is, and the
//! fire-and-forget self-update that copies the bundled binary into userData and runs `-U`.
//! macOS is out of scope (owner decision 2026-10-30), so its Homebrew lookup isn't ported.

use crate::{
    logger::{self, LogArg},
    node::{self, child_process},
};
use std::future::Future;

/// What the source reads from Electron and Node globals.
#[derive(Clone, Debug)]
pub struct Env {
    /// `process.platform`.
    pub platform: String,
    /// `process.resourcesPath`.
    pub resources_path: String,
    /// `__dirname` of `ytdlp.js` (`<repo>/src/main` in development).
    pub dirname: String,
    /// `app.getPath('userData')`.
    pub user_data: String,
}

impl Env {
    /// The running app's values.
    pub fn current(resources_path: String, repo_root: &str, user_data: String) -> Self {
        let platform = match std::env::consts::OS {
            "windows" => "win32",
            os => os,
        };
        Self { platform: platform.into(), resources_path, dirname: join(&[repo_root, "src", "main"]), user_data }
    }
}

/// Node's `path` is the host OS's flavour, whatever `process.platform` claims.
fn join(parts: &[&str]) -> String {
    if cfg!(windows) { node::path::win32::join(parts) } else { node::path::posix::join(parts) }
}

fn dirname(p: &str) -> String {
    node::path::posix_more::dirname(p)
}

fn bin_name(env: &Env) -> &'static str {
    if env.platform == "win32" { "yt-dlp.exe" } else { "yt-dlp" }
}

/// `getBundledPath(binName, subDir)`.
fn bundled_path(env: &Env, bin: &str, sub: &str) -> Option<String> {
    let bundled = join(&[&env.resources_path, "bin", sub, bin]);
    if node::exists_sync(&bundled) {
        return Some(bundled);
    }
    let dev = join(&[&env.dirname, "..", "..", "bin", sub, bin]);
    node::exists_sync(&dev).then_some(dev)
}

/// `getYtDlpPath()`.
pub fn yt_dlp_path(env: &Env) -> String {
    let bin = bin_name(env);
    let sub = if env.platform == "win32" { "win" } else { "linux" };
    let user_bin = join(&[&env.user_data, "bin", bin]);
    if node::exists_sync(&user_bin) {
        return user_bin;
    }
    bundled_path(env, bin, sub).unwrap_or_else(|| bin.into())
}

/// `updateYtDlp()`. The returned future is the `execFile` callback; `None` when nothing was
/// started.
pub fn update_yt_dlp(env: &Env) -> Option<impl Future<Output = ()> + Send + use<>> {
    let is_win = env.platform == "win32";
    let bin = bin_name(env);
    let user_bin = join(&[&env.user_data, "bin", bin]);
    let setup = || -> Result<Option<String>, crate::net::JsErr> {
        if !node::exists_sync(&user_bin) {
            let Some(src) = bundled_path(env, bin, if is_win { "win" } else { "linux" }) else {
                return Ok(None);
            };
            node::mkdir_recursive_sync(&dirname(&user_bin))?;
            node::copy_file_sync(&src, &user_bin)?;
            if !is_win {
                node::chmod_sync(&user_bin, 0o755)?;
            }
        }
        Ok(Some(user_bin.clone()))
    };
    let started = setup().and_then(|bin| match bin {
        Some(bin) => child_process::exec_file(&bin, &["-U".into()], 30_000).map(|(_, fut)| Some(fut)),
        None => Ok(None),
    });
    match started {
        Ok(fut) => fut.map(|fut| async move {
            let o = fut.await;
            match &o.err {
                Some(e) => logger::warn(&["[yt-dlp] self-update failed:".into(), LogArg::Str(e.message.to_string_lossy())]),
                None => {
                    let last = crate::js::trim(&o.stdout).rsplit('\n').next().unwrap_or_default().to_owned();
                    logger::log(&["[yt-dlp]".into(), LogArg::Str(last)]);
                }
            }
        }),
        Err(e) => {
            logger::warn(&["[yt-dlp] self-update setup failed:".into(), LogArg::Str(e.message.to_string_lossy())]);
            None
        }
    }
}
