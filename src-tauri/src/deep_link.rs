//! Port of the deep-link handling in `src/main.js`: `snowify://{track,album,artist}/<id>`.

use crate::logger;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Track,
    Album,
    Artist,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Artist => "artist",
        }
    }
}

/// The `{ type, id }` payload of `app:deepLink` / `app:getPendingDeepLink`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepLink {
    pub kind: Kind,
    pub id: String,
}

/// Strips `^snowify:/?/?`, splits on `/` dropping empty segments, and takes the first segment
/// as the type and the rest, re-joined with `/`, as the id.
pub fn parse(url: &str) -> Option<DeepLink> {
    let cleaned = url.strip_prefix("snowify:").map_or(url, |r| {
        let r = r.strip_prefix('/').unwrap_or(r);
        r.strip_prefix('/').unwrap_or(r)
    });
    let mut parts = cleaned.split('/').filter(|p| !p.is_empty());
    let kind = parts.next();
    let id = parts.collect::<Vec<_>>().join("/");
    if id.is_empty() {
        return None;
    }
    let kind = match kind? {
        "track" => Kind::Track,
        "album" => Kind::Album,
        "artist" => Kind::Artist,
        _ => return None,
    };
    Some(DeepLink { kind, id })
}

/// `argv.find(a => a.startsWith('snowify://'))`, for cold start and `second-instance`.
pub fn find_arg<S: AsRef<str>>(args: &[S]) -> Option<&str> {
    args.iter()
        .map(AsRef::as_ref)
        .find(|a| a.starts_with("snowify://"))
}

/// Electron's "Object has been destroyed": a window call made after the window closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowDestroyed;

/// The main window as `handleDeepLink` uses it. Every call may fail, as Electron throws on
/// a destroyed window; `handle` then stops, like the source's `try`.
pub trait MainWindow {
    /// `!!mainWindow.webContents`.
    fn has_web_contents(&self) -> Result<bool, WindowDestroyed>;
    fn send_deep_link(&self, link: &DeepLink) -> Result<(), WindowDestroyed>;
    fn is_minimized(&self) -> Result<bool, WindowDestroyed>;
    fn restore(&self) -> Result<(), WindowDestroyed>;
    fn focus(&self) -> Result<(), WindowDestroyed>;
}

/// `handleDeepLink(url)`: logs, then sends the link to the window if there is one,
/// otherwise stores it for `app:getPendingDeepLink` (replacing any earlier one).
pub fn handle(url: &str, window: Option<&dyn MainWindow>, pending: &mut Option<DeepLink>) {
    let _ = (|| -> Result<(), WindowDestroyed> {
        let Some(link) = parse(url) else {
            return Ok(());
        };
        logger::log(&[format!("Deep link: {}/{}", link.kind.as_str(), link.id).into()]);
        match window {
            Some(w) if w.has_web_contents()? => {
                w.send_deep_link(&link)?;
                if w.is_minimized()? {
                    w.restore()?;
                }
                w.focus()
            }
            _ => {
                *pending = Some(link);
                Ok(())
            }
        }
    })();
}

/// The `second-instance` handler: brings the window forward, then handles the first
/// `snowify://` argument. Unlike `handle`, a failing window call is not caught in the source
/// (it throws out of the event handler); that surfaces here as `Err`.
pub fn on_second_instance<S: AsRef<str>>(
    argv: &[S],
    window: Option<&dyn MainWindow>,
    pending: &mut Option<DeepLink>,
) -> Result<(), WindowDestroyed> {
    if let Some(w) = window {
        if w.is_minimized()? {
            w.restore()?;
        }
        w.focus()?;
    }
    if let Some(url) = find_arg(argv) {
        handle(url, window, pending);
    }
    Ok(())
}

/// `app:getPendingDeepLink`: returns the stored link (or `null`) and clears it.
pub fn take_pending(pending: &mut Option<DeepLink>) -> Option<DeepLink> {
    pending.take()
}
