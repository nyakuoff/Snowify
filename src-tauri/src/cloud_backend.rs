//! The production [`Backend`] and [`Keyring`] for `cloud_auth`: Firebase through
//! `crate::firebase` (Auth over Identity Toolkit, Firestore over REST) and the password in
//! the OS keyring (Secret Service on Linux, Credential Manager on Windows).

use crate::{
    cloud_auth::{Backend, BackendError, Keyring, User},
    firebase::{self, Auth, firestore},
    host::Host,
    ipc::JsValue,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// `firebase.auth` + `firebase.db` for one app instance.
pub struct FirebaseBackend<H: Host> {
    host: Arc<H>,
    auth: Mutex<Auth>,
    /// `auth.currentUser`, readable without awaiting the auth lock.
    current: std::sync::Mutex<Option<User>>,
}

fn js(v: &Value) -> JsValue {
    JsValue::from(v)
}

fn user(u: &firebase::User) -> User {
    User { uid: JsValue::from(u.uid.as_str()), email: js(&u.email), display_name: js(&u.display_name), photo_url: js(&u.photo_url) }
}

fn fail(e: firebase::Error) -> BackendError {
    BackendError(e.message.as_str().into(), e.code)
}

impl<H: Host> FirebaseBackend<H> {
    /// A backend whose auth-state listener is registered (`onAuthStateChanged` at startup).
    pub fn new(host: Arc<H>) -> Self {
        let mut auth = Auth::default();
        auth.on_auth_state_changed();
        Self { host, auth: Mutex::new(auth), current: std::sync::Mutex::new(None) }
    }

    /// Queued auth-state callbacks since the last call: the current user's fields, or null.
    pub async fn take_events(&self) -> Vec<Option<User>> {
        let events = self.auth.lock().await.take_events();
        events
            .iter()
            .map(|v| {
                if v.is_null() {
                    None
                } else {
                    Some(User { uid: js(&v["uid"]), email: js(&v["email"]), display_name: js(&v["displayName"]), photo_url: js(&v["photoURL"]) })
                }
            })
            .collect()
    }

    fn sync_current(&self, auth: &Auth) {
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = auth.current_user.as_ref().map(user);
    }

    fn doc(collection: &str, id: &JsValue) -> Result<firestore::Document, BackendError> {
        match id {
            JsValue::String(s) => firestore::doc(collection, &s.to_string_lossy()).map_err(fail),
            // `doc(db, collection, id)` with a non-string segment throws before any request.
            _ => Err(BackendError("Invalid document reference".into(), "invalid-argument".into())),
        }
    }
}

impl<H: Host + 'static> Backend for FirebaseBackend<H> {
    fn current_user(&self) -> Option<User> {
        self.current.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    async fn sign_in(&self, email: &JsValue, password: &JsValue) -> Result<User, BackendError> {
        let mut a = self.auth.lock().await;
        let r = a.sign_in_with_email_and_password(&*self.host, email, password).await;
        self.sync_current(&a);
        r.map(|u| user(&u)).map_err(fail)
    }

    async fn sign_up(&self, email: &JsValue, password: &JsValue) -> Result<User, BackendError> {
        let mut a = self.auth.lock().await;
        let r = a.create_user_with_email_and_password(&*self.host, email, password).await;
        self.sync_current(&a);
        r.map(|u| user(&u)).map_err(fail)
    }

    async fn send_password_reset(&self, email: &JsValue) -> Result<(), BackendError> {
        self.auth.lock().await.send_password_reset_email(&*self.host, email).await.map_err(fail)
    }

    async fn sign_out(&self) -> Result<(), BackendError> {
        let mut a = self.auth.lock().await;
        a.sign_out();
        self.sync_current(&a);
        Ok(())
    }

    async fn update_profile(&self, profile: &JsValue) -> Result<(), BackendError> {
        let mut a = self.auth.lock().await;
        let r = a.update_profile(&*self.host, profile).await;
        self.sync_current(&a);
        r.map_err(fail)
    }

    async fn get_doc(&self, collection: &str, id: &JsValue) -> Result<Option<JsValue>, BackendError> {
        let d = Self::doc(collection, id)?;
        let mut a = self.auth.lock().await;
        let r = firestore::get_doc(&*self.host, &mut a, &d).await;
        self.sync_current(&a);
        Ok(r.map_err(fail)?.data)
    }

    async fn set_doc_merge(&self, collection: &str, id: &JsValue, data: &JsValue) -> Result<(), BackendError> {
        let d = Self::doc(collection, id)?;
        let mut a = self.auth.lock().await;
        let r = firestore::set_doc(&*self.host, &mut a, &d, data).await;
        self.sync_current(&a);
        r.map_err(fail)
    }

    async fn update_doc(&self, collection: &str, id: &JsValue, data: &JsValue) -> Result<(), BackendError> {
        let d = Self::doc(collection, id)?;
        let mut a = self.auth.lock().await;
        let r = firestore::update_doc(&*self.host, &mut a, &d, data).await;
        self.sync_current(&a);
        r.map_err(fail)
    }

    fn now_ms(&self) -> f64 {
        self.host.now_ms() as f64
    }
}

/// The saved password, under the port's own keyring entry.
pub struct OsKeyring;

const SERVICE: &str = "Snowify";
const ACCOUNT: &str = "auth-password";

/// Runs a keyring call off the async runtime (the Secret Service client blocks internally).
fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::spawn(f).join().ok()
}

impl Keyring for OsKeyring {
    fn available(&self) -> bool {
        off_runtime(|| {
            let e = keyring::Entry::new(SERVICE, ACCOUNT).ok()?;
            match e.get_password() {
                Ok(_) | Err(keyring::Error::NoEntry) => Some(()),
                Err(_) => None,
            }
        })
        .flatten()
        .is_some()
    }

    fn store(&self, password: &str) -> Result<(), String> {
        let pw = password.to_owned();
        off_runtime(move || keyring::Entry::new(SERVICE, ACCOUNT).and_then(|e| e.set_password(&pw)).map_err(|e| e.to_string())).unwrap_or_else(|| Err("keyring thread panicked".into()))
    }

    fn load(&self) -> Option<String> {
        off_runtime(|| keyring::Entry::new(SERVICE, ACCOUNT).ok()?.get_password().ok()).flatten()
    }

    fn clear(&self) {
        off_runtime(|| {
            if let Ok(e) = keyring::Entry::new(SERVICE, ACCOUNT) {
                let _ = e.delete_credential();
            }
        });
    }
}

/// `base::GetHomeDir()`: `$HOME` when set and non-empty.
fn home() -> String {
    std::env::var("HOME").ok().filter(|h| !h.is_empty()).unwrap_or_else(|| "/tmp".into())
}

/// `xdg_user_dir_lookup(type)` (xdg-user-dirs, as Chromium bundles it): the last
/// `XDG_<TYPE>_DIR="…"` line of `user-dirs.dirs`, `$HOME`-relative or absolute; otherwise
/// `$HOME/Desktop` for DESKTOP and `$HOME` for everything else.
fn xdg_user_dir(ty: &str) -> String {
    let home = home();
    let config = match std::env::var("XDG_CONFIG_HOME") {
        Ok(c) if !c.is_empty() => format!("{c}/user-dirs.dirs"),
        _ => format!("{home}/.config/user-dirs.dirs"),
    };
    let mut found = None;
    for line in std::fs::read_to_string(config).unwrap_or_default().lines() {
        let p = line.trim_start_matches([' ', '\t']);
        let Some(p) = p.strip_prefix("XDG_").and_then(|p| p.strip_prefix(ty)).and_then(|p| p.strip_prefix("_DIR")) else { continue };
        let Some(p) = p.trim_start_matches([' ', '\t']).strip_prefix('=') else { continue };
        let Some(p) = p.trim_start_matches([' ', '\t']).strip_prefix('"') else { continue };
        let (relative, p) = match p.strip_prefix("$HOME") {
            Some(rest) => (true, rest),
            None if p.starts_with('/') => (false, p),
            None => continue,
        };
        let mut value = String::new();
        let mut chars = p.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '"' {
                break;
            }
            if c == '\\' && chars.peek().is_some() {
                value.push(chars.next().unwrap());
            } else {
                value.push(c);
            }
        }
        found = Some(if relative { format!("{home}{value}") } else { value });
    }
    let dir = found.unwrap_or_else(|| if ty == "DESKTOP" { format!("{home}/Desktop") } else { home.clone() });
    let trimmed = dir.trim_end_matches('/');
    if trimmed.is_empty() { "/".into() } else { trimmed.into() }
}

/// Electron's `app.getPath('pictures' | 'downloads' | 'desktop')` on Linux: Chromium's
/// `GetUserMediaDirectory` falls back to `~/Pictures` when the XDG dir is home or the desktop.
pub fn image_roots() -> [String; 3] {
    let home = home();
    let desktop = xdg_user_dir("DESKTOP");
    let pictures = xdg_user_dir("PICTURES");
    let pictures = if pictures != home && pictures != desktop { pictures } else { format!("{home}/Pictures") };
    [pictures, xdg_user_dir("DOWNLOAD"), desktop]
}
