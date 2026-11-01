//! Port of `src/main/cloud-auth.js`: email/password auth, the profile and sync documents in
//! Firestore (`users_v2`, backfilled from the legacy `users` collection), and the saved
//! credentials used for automatic sign-in.
//!
//! Firebase sits behind [`Backend`] (the real one wraps `crate::firebase`; the parity test uses
//! a fake that behaves like its JS twin). Saved passwords live in the OS keyring behind
//! [`Keyring`]: Electron's `safeStorage` format is not read (owner decision, see PORT.md).

use crate::{
    i18n::Translations,
    ipc::{self, HandlerResult, JsString, JsValue, Thrown},
    logger,
    node::{self, path::posix as path},
};
use std::{collections::HashSet, future::Future, sync::Mutex};

const V2: &str = "users_v2";
const LEGACY: &str = "users";
/// `auth_credentials.json`'s `enc` for a password kept in the OS keyring.
pub const KEYRING_ENC: &str = "keyring-v1";

/// A Firebase failure: `err.message` is all cloud-auth reads.
#[derive(Debug, Clone)]
pub struct BackendError(pub JsValue);

/// The signed-in user as cloud-auth reads it (`user.uid`, `.email`, `.displayName`,
/// `.photoURL`).
#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub uid: JsValue,
    pub email: JsValue,
    pub display_name: JsValue,
    pub photo_url: JsValue,
}

/// What cloud-auth calls on `firebase/auth` and `firebase/firestore`.
pub trait Backend: Send + Sync {
    /// `auth.currentUser`.
    fn current_user(&self) -> Option<User>;
    fn sign_in(&self, email: &JsValue, password: &JsValue) -> impl Future<Output = Result<User, BackendError>> + Send;
    fn sign_up(&self, email: &JsValue, password: &JsValue) -> impl Future<Output = Result<User, BackendError>> + Send;
    fn send_password_reset(&self, email: &JsValue) -> impl Future<Output = Result<(), BackendError>> + Send;
    fn sign_out(&self) -> impl Future<Output = Result<(), BackendError>> + Send;
    /// `updateProfile(auth.currentUser, profile)`.
    fn update_profile(&self, profile: &JsValue) -> impl Future<Output = Result<(), BackendError>> + Send;
    /// `getDoc(doc(db, collection, id))`: `Ok(None)` when the document doesn't exist.
    fn get_doc(&self, collection: &str, id: &JsValue) -> impl Future<Output = Result<Option<JsValue>, BackendError>> + Send;
    /// `setDoc(doc(db, collection, id), data, { merge: true })`.
    fn set_doc_merge(&self, collection: &str, id: &JsValue, data: &JsValue) -> impl Future<Output = Result<(), BackendError>> + Send;
    /// `updateDoc(doc(db, collection, id), data)`.
    fn update_doc(&self, collection: &str, id: &JsValue, data: &JsValue) -> impl Future<Output = Result<(), BackendError>> + Send;
    /// `Date.now()`.
    fn now_ms(&self) -> f64;
}

/// Where the password is kept (`safeStorage` in the source).
pub trait Keyring: Send + Sync {
    /// `safeStorage.isEncryptionAvailable()`.
    fn available(&self) -> bool;
    fn store(&self, password: &str) -> Result<(), String>;
    fn load(&self) -> Option<String>;
    fn clear(&self);
}

/// Paths cloud-auth reads from Electron's `app.getPath`.
pub struct Paths {
    pub user_data: String,
    /// `pictures`, `downloads`, `desktop`.
    pub image_roots: [String; 3],
    /// `process.cwd()` for `path.resolve`.
    pub cwd: String,
}

/// Module state: `_wiredAuthState` lives in the shell; this is `_syncReadyUsers`.
#[derive(Default)]
pub struct CloudAuth {
    sync_ready: Mutex<HashSet<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn key(uid: &JsValue) -> String {
    ipc::to_js_string(uid)
}

fn obj(props: Vec<(&str, JsValue)>) -> JsValue {
    let mut o = JsValue::Object(Vec::new());
    for (k, v) in props {
        ipc::define(&mut o, k.into(), v);
    }
    o
}

/// `{ ...value }`: own enumerable properties (array indices, string characters).
fn spread(value: &JsValue) -> JsValue {
    let mut o = JsValue::Object(Vec::new());
    match value.deref_shared() {
        JsValue::Object(props) => {
            for (k, v) in props {
                ipc::define(&mut o, k.clone(), v.clone());
            }
        }
        JsValue::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                ipc::define(&mut o, JsString::from(i.to_string()), v.clone());
            }
        }
        JsValue::String(s) => {
            for (i, u) in s.0.iter().enumerate() {
                ipc::define(&mut o, JsString::from(i.to_string()), JsValue::String(JsString(vec![*u])));
            }
        }
        _ => {}
    }
    o
}

/// `merged[key]` on the spread object (own property, else `Object.prototype`).
fn lookup(o: &JsValue, k: &JsString) -> JsValue {
    match ipc::lookup(o, &k.to_string_lossy()) {
        Ok(ipc::Lookup::Own(v)) => v,
        // A prototype member is neither undefined nor null and isn't an array or plain object.
        Ok(ipc::Lookup::Proto) => JsValue::Bool(true),
        _ => JsValue::Undefined,
    }
}

/// `_isPlainObject(value)`: truthy, `typeof 'object'`, not an array.
fn is_plain_object(v: &JsValue) -> bool {
    matches!(
        v.deref_shared(),
        JsValue::Object(_) | JsValue::Date(_) | JsValue::RegExp { .. } | JsValue::Map(_) | JsValue::Set(_) | JsValue::Error { .. } | JsValue::Boxed(_) | JsValue::ArrayBuffer(_) | JsValue::View { .. }
    )
}

/// `Object.entries(value || {})`.
fn entries(v: &JsValue) -> Vec<(JsString, JsValue)> {
    if !v.truthy() {
        return Vec::new();
    }
    match spread(v) {
        JsValue::Object(p) => p,
        _ => Vec::new(),
    }
}

/// `_mergeForBackfill(v2Data, legacyData)`.
pub fn merge_for_backfill(v2: &JsValue, legacy: &JsValue) -> (JsValue, bool) {
    let v2 = if *v2 == JsValue::Undefined { JsValue::Object(Vec::new()) } else { v2.clone() };
    let legacy = if *legacy == JsValue::Undefined { JsValue::Object(Vec::new()) } else { legacy.clone() };
    let mut merged = spread(&v2);
    let mut changed = false;
    for (k, legacy_value) in entries(&legacy) {
        let v2_value = lookup(&merged, &k);
        if matches!(v2_value, JsValue::Undefined | JsValue::Null) {
            ipc::define(&mut merged, k, legacy_value);
            changed = true;
            continue;
        }
        if let (JsValue::Array(l), JsValue::Array(v)) = (legacy_value.deref_shared(), v2_value.deref_shared())
            && v.is_empty()
            && !l.is_empty()
        {
            ipc::define(&mut merged, k, legacy_value.clone());
            changed = true;
            continue;
        }
        if is_plain_object(&legacy_value) && is_plain_object(&v2_value) {
            let (nested, nested_changed) = merge_for_backfill(&v2_value, &legacy_value);
            if nested_changed {
                ipc::define(&mut merged, k, nested);
                changed = true;
            }
        }
    }
    (merged, changed)
}

/// `{ ...data, migratedFrom: 'users', migratedAt: Date.now() }`.
fn migrated(data: &JsValue, now: f64) -> JsValue {
    let mut o = spread(data);
    ipc::define(&mut o, "migratedFrom".into(), LEGACY.into());
    ipc::define(&mut o, "migratedAt".into(), JsValue::Number(now));
    o
}

/// `snap.data() || {}`.
fn or_empty(v: JsValue) -> JsValue {
    if v.truthy() { v } else { JsValue::Object(Vec::new()) }
}

impl CloudAuth {
    /// `_migrateLegacyDocIfNeeded(uid)`.
    pub async fn migrate_legacy<B: Backend>(&self, b: &B, uid: &JsValue) -> JsValue {
        if lock(&self.sync_ready).contains(&key(uid)) {
            return JsValue::Null;
        }
        let r: Result<JsValue, BackendError> = async {
            let v2 = b.get_doc(V2, uid).await?.map(or_empty);
            let Some(legacy) = b.get_doc(LEGACY, uid).await? else {
                if v2.is_some() {
                    lock(&self.sync_ready).insert(key(uid));
                }
                return Ok(v2.unwrap_or(JsValue::Null));
            };
            let legacy = or_empty(legacy);
            let Some(v2) = v2 else {
                b.set_doc_merge(V2, uid, &migrated(&legacy, b.now_ms())).await?;
                lock(&self.sync_ready).insert(key(uid));
                return Ok(legacy);
            };
            let (merged, changed) = merge_for_backfill(&v2, &legacy);
            if changed {
                b.set_doc_merge(V2, uid, &migrated(&merged, b.now_ms())).await?;
            }
            lock(&self.sync_ready).insert(key(uid));
            Ok(if changed { merged } else { v2 })
        }
        .await;
        r.unwrap_or(JsValue::Null)
    }

    /// `_readUserData(uid)`.
    pub async fn read_user_data<B: Backend>(&self, b: &B, uid: &JsValue) -> JsValue {
        if let Ok(Some(data)) = b.get_doc(V2, uid).await {
            lock(&self.sync_ready).insert(key(uid));
            return data;
        }
        self.migrate_legacy(b, uid).await
    }

    /// `_getUserInfo(user)`.
    pub async fn user_info<B: Backend>(&self, b: &B, user: &User) -> JsValue {
        let mut photo = if user.photo_url.truthy() { user.photo_url.clone() } else { JsValue::Null };
        let mut name = user.display_name.clone();
        let data = self.read_user_data(b, &user.uid).await;
        if data.truthy() {
            let profile = ipc::get_prop(&data, "profile").unwrap_or_default();
            let opt = |k: &str| if matches!(profile, JsValue::Undefined | JsValue::Null) { JsValue::Undefined } else { ipc::get_prop(&profile, k).unwrap_or_default() };
            if opt("photoURL").truthy() {
                photo = opt("photoURL");
            }
            if opt("displayName").truthy() && !name.truthy() {
                name = opt("displayName");
            }
        }
        obj(vec![("uid", user.uid.clone()), ("email", user.email.clone()), ("displayName", name), ("photoURL", photo)])
    }
}

// ─── Credentials ───

fn credentials_path(p: &Paths) -> String {
    path::join(&[&p.user_data, "auth_credentials.json"])
}

fn trimmed_string(v: &JsValue) -> String {
    let s = if v.truthy() { ipc::to_js_string(v) } else { String::new() };
    crate::js::trim(&s).to_owned()
}

/// `_saveCredentials(email, password)`: the email in the file, the password in the keyring.
pub fn save_credentials<K: Keyring>(k: &K, p: &Paths, email: &JsValue, password: &JsValue) {
    let r = (|| -> Result<(), String> {
        let mut payload = vec![("email", JsValue::from(trimmed_string(email)))];
        if k.available() {
            k.store(&if password.truthy() { ipc::to_js_string(password) } else { String::new() })?;
            payload.push(("enc", KEYRING_ENC.into()));
        }
        let text = ipc::json_stringify(&obj(payload)).ok().flatten().unwrap_or_default();
        node::write_file(&credentials_path(p), text.as_bytes()).map_err(|e| e.message)
    })();
    if let Err(m) = r {
        logger::warn(&["[Auth] Failed to save credentials:".into(), m.as_str().into()]);
    }
}

/// `_loadCredentials()`.
pub fn load_credentials<K: Keyring>(k: &K, p: &Paths) -> Option<(String, String)> {
    let file = credentials_path(p);
    if !node::exists_sync(&file) {
        return None;
    }
    let text: Vec<u16> = node::read_file_utf8(&file).ok()?.encode_utf16().collect();
    let parsed = crate::js::json_parse(&text).ok()?;
    let field = |n: &str| match &parsed {
        JsValue::Undefined | JsValue::Null => JsValue::Undefined,
        v => ipc::get_prop(v, n).unwrap_or_default(),
    };
    let email = trimmed_string(&field("email"));
    if email.is_empty() {
        return None;
    }
    // Legacy migration path: a plaintext password.
    if let JsValue::String(pw) = field("password")
        && !pw.0.is_empty()
    {
        let pw = pw.to_string_lossy();
        if k.available() {
            save_credentials(k, p, &email.as_str().into(), &pw.as_str().into());
        } else {
            clear_credentials(p);
        }
        return Some((email, pw));
    }
    // Only the port's own format decrypts; Electron's safeStorage-v1 files don't.
    if field("enc") != JsValue::from(KEYRING_ENC) || !k.available() {
        return None;
    }
    let pw = k.load().filter(|p| !p.is_empty())?;
    Some((email, pw))
}

/// `_clearCredentials()`.
pub fn clear_credentials(p: &Paths) {
    let file = credentials_path(p);
    if node::exists_sync(&file)
        && let Err(e) = node::unlink_sync(&file)
    {
        logger::warn(&["[Auth] Failed to clear credentials:".into(), e.message.as_str().into()]);
    }
}

/// `_autoSignIn()`.
pub async fn auto_sign_in<B: Backend, K: Keyring>(b: &B, k: &K, p: &Paths) {
    let Some((email, password)) = load_credentials(k, p) else { return };
    if b.sign_in(&email.as_str().into(), &password.as_str().into()).await.is_err() {
        clear_credentials(p);
    }
}

// ─── Handlers ───

fn error_of(e: BackendError) -> JsValue {
    obj(vec![("error", e.0)])
}

fn not_signed_in(t: &Translations) -> JsValue {
    let m = t.mt("auth.notSignedIn", &[]).map(|m| m.to_string()).unwrap_or_default();
    obj(vec![("error", m.into())])
}

/// `async (_event, { a, b }) => …`: destructuring undefined/null throws before the body.
fn destructure(arg: &JsValue, first: &str) -> Result<(), Thrown> {
    match arg {
        JsValue::Undefined | JsValue::Null => {
            // V8 names null as 'object null' here.
            let (shown, what) = if *arg == JsValue::Undefined { ("undefined", "undefined") } else { ("object null", "null") };
            Err(Thrown::type_error(format!("Cannot destructure property '{first}' of '{shown}' as it is {what}.")))
        }
        _ => Ok(()),
    }
}

impl CloudAuth {
    /// `auth:signInWithEmail` / `auth:signUpWithEmail`.
    pub async fn sign_in_with<B: Backend, K: Keyring>(&self, b: &B, k: &K, p: &Paths, email: &JsValue, password: &JsValue, sign_up: bool) -> HandlerResult {
        let r = if sign_up { b.sign_up(email, password).await } else { b.sign_in(email, password).await };
        Ok(match r {
            Ok(user) => {
                save_credentials(k, p, email, password);
                self.user_info(b, &user).await
            }
            Err(e) => error_of(e),
        })
    }

    /// `auth:sendPasswordReset`.
    pub async fn send_password_reset<B: Backend>(&self, b: &B, email: &JsValue) -> HandlerResult {
        Ok(match b.send_password_reset(email).await {
            Ok(()) => obj(vec![("ok", JsValue::Bool(true))]),
            Err(e) => error_of(e),
        })
    }

    /// `auth:signOut`.
    pub async fn sign_out<B: Backend>(&self, b: &B, p: &Paths) -> HandlerResult {
        Ok(match b.sign_out().await {
            Ok(()) => {
                clear_credentials(p);
                JsValue::Bool(true)
            }
            Err(e) => error_of(e),
        })
    }

    /// `auth:getUser`.
    pub async fn get_user<B: Backend>(&self, b: &B) -> HandlerResult {
        Ok(match b.current_user() {
            Some(u) => self.user_info(b, &u).await,
            None => JsValue::Null,
        })
    }

    /// `profile:update(_event, { displayName, photoURL })`.
    pub async fn update_profile<B: Backend>(&self, b: &B, t: &Translations, arg: &JsValue) -> HandlerResult {
        destructure(arg, "displayName")?;
        let display_name = ipc::get_prop(arg, "displayName").unwrap_or_default();
        let photo_url = ipc::get_prop(arg, "photoURL").unwrap_or_default();
        let Some(user) = b.current_user() else { return Ok(not_signed_in(t)) };
        let r: Result<JsValue, BackendError> = async {
            if display_name != JsValue::Undefined {
                b.update_profile(&obj(vec![("displayName", display_name.clone())])).await?;
            }
            // `user` is the live currentUser object: updateProfile changed it.
            let user = b.current_user().unwrap_or(user);
            let mut profile = vec![("displayName", if user.display_name.truthy() { user.display_name.clone() } else { "".into() })];
            if photo_url != JsValue::Undefined {
                profile.push(("photoURL", photo_url.clone()));
            }
            self.migrate_legacy(b, &user.uid).await;
            b.set_doc_merge(V2, &user.uid, &obj(vec![("profile", obj(profile))])).await?;
            Ok(obj(vec![
                ("uid", user.uid.clone()),
                ("email", user.email.clone()),
                ("displayName", user.display_name.clone()),
                ("photoURL", if photo_url != JsValue::Undefined { photo_url.clone() } else { user.photo_url.clone() }),
            ]))
        }
        .await;
        Ok(r.unwrap_or_else(error_of))
    }

    /// `profile:readImage(filePath)`: a data URL for an image under Pictures, Downloads or
    /// Desktop, at most 8 MiB; otherwise `null`.
    pub fn read_image(&self, p: &Paths, file: &JsValue) -> HandlerResult {
        let r = (|| -> Option<JsValue> {
            let s = ipc::to_js_string(file);
            let ext = node::path::posix_more::extname(&s).to_lowercase();
            if ![".png", ".jpg", ".jpeg", ".webp"].contains(&ext.as_str()) {
                return None;
            }
            let resolved = std::fs::canonicalize(&s).ok()?.to_string_lossy().into_owned();
            let ok = p
                .image_roots
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| format!("{}/", node::path::posix_more::resolve(&[r], &p.cwd)))
                .any(|root| resolved.starts_with(&root));
            if !ok {
                return None;
            }
            let meta = std::fs::metadata(&resolved).ok()?;
            if !meta.is_file() || meta.len() > 8 * 1024 * 1024 {
                return None;
            }
            let bytes = std::fs::read(&resolved).ok()?;
            let mime = match ext.as_str() {
                ".png" => "image/png",
                ".webp" => "image/webp",
                _ => "image/jpeg",
            };
            Some(format!("data:{mime};base64,{}", crate::media::base64(&bytes)).into())
        })();
        Ok(r.unwrap_or(JsValue::Null))
    }

    /// `profile:updateExtras(_event, { banner, bio })`.
    pub async fn update_extras<B: Backend>(&self, b: &B, t: &Translations, arg: &JsValue) -> HandlerResult {
        destructure(arg, "banner")?;
        let banner = ipc::get_prop(arg, "banner").unwrap_or_default();
        let bio = ipc::get_prop(arg, "bio").unwrap_or_default();
        let Some(user) = b.current_user() else { return Ok(not_signed_in(t)) };
        let banner_str = jsv_units(&banner);
        if banner != JsValue::Undefined && banner != JsValue::from("") && !banner_str.starts_with(&"data:image/".encode_utf16().collect::<Vec<_>>()) {
            return Ok(obj(vec![("error", "Invalid banner format".into())]));
        }
        if banner.truthy() && banner_str.len() > 800_000 {
            return Ok(obj(vec![("error", "Banner image is too large. Please use a smaller image.".into())]));
        }
        // `String(bio).slice(0, 200)` in UTF-16 units.
        let bio_cut = || {
            let u = jsv_units(&bio);
            JsValue::String(JsString(u[..u.len().min(200)].to_vec()))
        };
        self.migrate_legacy(b, &user.uid).await;
        let mut update = Vec::new();
        if banner != JsValue::Undefined {
            update.push(("profile.banner", banner.clone()));
        }
        if bio != JsValue::Undefined {
            update.push(("profile.bio", bio_cut()));
        }
        let r: Result<(), BackendError> = async {
            if b.update_doc(V2, &user.uid, &obj(update)).await.is_err() {
                let mut profile = Vec::new();
                if banner != JsValue::Undefined {
                    profile.push(("banner", banner.clone()));
                }
                if bio != JsValue::Undefined {
                    profile.push(("bio", bio_cut()));
                }
                b.set_doc_merge(V2, &user.uid, &obj(vec![("profile", obj(profile))])).await?;
            }
            Ok(())
        }
        .await;
        Ok(match r {
            Ok(()) => obj(vec![("success", JsValue::Bool(true))]),
            Err(e) => error_of(e),
        })
    }

    /// `profile:get(uid)`.
    pub async fn get_profile<B: Backend>(&self, b: &B, uid: &JsValue) -> HandlerResult {
        let data = self.read_user_data(b, uid).await;
        if !data.truthy() {
            return Ok(JsValue::Null);
        }
        let prop = |o: &JsValue, k: &str| ipc::get_prop(o, k).unwrap_or_default();
        let mut profile = prop(&data, "profile");
        if !profile.truthy() {
            profile = JsValue::Object(Vec::new());
        }
        // Assigning onto a non-object profile (a string, a number) is silently dropped.
        for (flat, field) in [("profile.banner", "banner"), ("profile.bio", "bio")] {
            if prop(&data, flat).truthy() && !prop(&profile, field).truthy() {
                let _ = ipc::set(&mut profile, field, prop(&data, flat));
            }
        }
        Ok(profile)
    }

    /// `cloud:save(data)`.
    pub async fn save<B: Backend>(&self, b: &B, t: &Translations, data: &JsValue) -> HandlerResult {
        let Some(user) = b.current_user() else { return Ok(not_signed_in(t)) };
        self.migrate_legacy(b, &user.uid).await;
        let mut doc = spread(data);
        ipc::define(&mut doc, "updatedAt".into(), JsValue::Number(b.now_ms()));
        Ok(match b.set_doc_merge(V2, &user.uid, &doc).await {
            Ok(()) => JsValue::Bool(true),
            Err(e) => error_of(e),
        })
    }

    /// `cloud:load()`.
    pub async fn load<B: Backend>(&self, b: &B) -> HandlerResult {
        let Some(user) = b.current_user() else { return Ok(JsValue::Null) };
        let data = self.read_user_data(b, &user.uid).await;
        Ok(if data.truthy() { data } else { JsValue::Null })
    }
}

fn jsv_units(v: &JsValue) -> Vec<u16> {
    crate::jsv::units(v)
}

/// The listener cloud-auth registers: what `auth:stateChanged` carries for this user.
pub async fn state_payload<B: Backend>(c: &CloudAuth, b: &B, user: Option<User>) -> JsValue {
    match user {
        Some(u) => c.user_info(b, &u).await,
        None => JsValue::Null,
    }
}

