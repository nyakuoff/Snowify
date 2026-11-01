//! Differential test: `src/main/cloud-auth.js` (unmodified; Firebase and electron faked in
//! `parity/node/cloud_auth.js`) vs `snowify::cloud_auth` over `FakeBackend`/`FakeKeyring`,
//! which behave like the JS fakes. Compared: handler results, log lines, `auth:stateChanged`
//! sends, the resulting document store, and the saved credentials as each side decodes them
//! (the formats differ by owner decision: Electron safeStorage vs the OS keyring).

use serde_json::{Value, json};
use snowify::{
    cloud_auth::{self, Backend, BackendError, CloudAuth, Keyring, Paths, User},
    i18n::Translations,
    ipc::{self, JsString, JsValue},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

mod common;
use common::{Diff, root};

// ─── The Rust twin of the JS fake ───

struct FakeBackend {
    users: Mutex<Vec<Value>>,
    current: Mutex<Option<User>>,
    docs: Mutex<BTreeMap<String, JsValue>>,
    failures: Value,
    auth_errors: Mutex<Value>,
    /// Auth-state callbacks waiting to be delivered (the JS listener runs them async).
    events: Mutex<Vec<Option<User>>>,
    now: f64,
}

fn as_user(u: &Value) -> User {
    let f = |k: &str| match &u[k] {
        Value::Null => JsValue::Null,
        v => JsValue::from(v),
    };
    User { uid: f("uid"), email: f("email"), display_name: f("displayName"), photo_url: f("photoURL") }
}

fn err(m: &str) -> BackendError {
    BackendError(JsValue::from(m))
}

impl FakeBackend {
    fn scripted(&self, op: &str) -> Result<(), BackendError> {
        let mut e = self.auth_errors.lock().unwrap();
        if let Some(q) = e.get_mut(op).and_then(Value::as_array_mut)
            && !q.is_empty()
        {
            return Err(err(q.remove(0).as_str().unwrap()));
        }
        Ok(())
    }
    fn fail(&self, op: &str, path: &str) -> Result<(), BackendError> {
        match self.failures[format!("{op}:{path}")].as_str() {
            Some(m) => Err(err(m)),
            None => Ok(()),
        }
    }
    fn path(coll: &str, id: &JsValue) -> Result<String, BackendError> {
        match id {
            JsValue::String(s) if !s.0.is_empty() => Ok(format!("{coll}/{}", s.to_string_lossy())),
            _ => Err(err("Invalid document reference")),
        }
    }
    fn set_user(&self, u: Option<User>) {
        *self.current.lock().unwrap() = u.clone();
        self.events.lock().unwrap().push(u);
    }
}

fn is_map(v: &JsValue) -> bool {
    matches!(v, JsValue::Object(_))
}

fn deep_merge(target: &mut JsValue, data: &JsValue) {
    let JsValue::Object(props) = data else { return };
    for (k, v) in props {
        let existing = match target {
            JsValue::Object(t) => t.iter_mut().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        };
        match existing {
            Some(e) if is_map(v) && is_map(e) => deep_merge(e, v),
            _ => ipc::define(target, k.clone(), v.clone()),
        }
    }
}

impl Backend for FakeBackend {
    fn current_user(&self) -> Option<User> {
        self.current.lock().unwrap().clone()
    }
    async fn sign_in(&self, email: &JsValue, password: &JsValue) -> Result<User, BackendError> {
        self.scripted("signIn")?;
        let found = self.users.lock().unwrap().iter().find(|u| JsValue::from(&u["email"]) == *email && JsValue::from(&u["password"]) == *password).cloned();
        let u = as_user(&found.ok_or_else(|| err("Firebase: Error (auth/invalid-credential)."))?);
        self.set_user(Some(u.clone()));
        Ok(u)
    }
    async fn sign_up(&self, email: &JsValue, password: &JsValue) -> Result<User, BackendError> {
        self.scripted("signUp")?;
        let mut users = self.users.lock().unwrap();
        if users.iter().any(|u| JsValue::from(&u["email"]) == *email) {
            return Err(err("Firebase: Error (auth/email-already-in-use)."));
        }
        let u = json!({ "uid": format!("new-{}", users.len()), "email": email.encode(), "password": password.encode(), "displayName": null, "photoURL": null });
        let user = User { uid: JsValue::from(&u["uid"]), email: email.clone(), display_name: JsValue::Null, photo_url: JsValue::Null };
        users.push(u);
        drop(users);
        self.set_user(Some(user.clone()));
        Ok(user)
    }
    async fn send_password_reset(&self, _email: &JsValue) -> Result<(), BackendError> {
        self.scripted("reset")
    }
    async fn sign_out(&self) -> Result<(), BackendError> {
        self.set_user(None);
        Ok(())
    }
    async fn update_profile(&self, profile: &JsValue) -> Result<(), BackendError> {
        self.scripted("profile")?;
        let name = ipc::get_prop(profile, "displayName").unwrap_or_default();
        if let Some(u) = self.current.lock().unwrap().as_mut() {
            u.display_name = if name.truthy() { name } else { JsValue::Null };
        }
        Ok(())
    }
    async fn get_doc(&self, coll: &str, id: &JsValue) -> Result<Option<JsValue>, BackendError> {
        let p = Self::path(coll, id)?;
        self.fail("get", &p)?;
        Ok(self.docs.lock().unwrap().get(&p).cloned())
    }
    async fn set_doc_merge(&self, coll: &str, id: &JsValue, data: &JsValue) -> Result<(), BackendError> {
        let p = Self::path(coll, id)?;
        self.fail("set", &p)?;
        let mut docs = self.docs.lock().unwrap();
        let d = docs.entry(p).or_insert(JsValue::Object(Vec::new()));
        deep_merge(d, data);
        Ok(())
    }
    async fn update_doc(&self, coll: &str, id: &JsValue, data: &JsValue) -> Result<(), BackendError> {
        let p = Self::path(coll, id)?;
        self.fail("update", &p)?;
        let mut docs = self.docs.lock().unwrap();
        let Some(d) = docs.get_mut(&p) else {
            return Err(err(&format!("No document to update: projects/snowify-dcda0/databases/(default)/documents/{p}")));
        };
        let JsValue::Object(props) = data else { return Ok(()) };
        for (k, v) in props {
            let parts: Vec<String> = k.to_string_lossy().split('.').map(str::to_owned).collect();
            let mut o: &mut JsValue = d;
            for part in &parts[..parts.len() - 1] {
                let key = JsString::from(part.as_str());
                let JsValue::Object(fields) = o else { unreachable!() };
                if !fields.iter().any(|(n, v)| *n == key && is_map(v)) {
                    ipc::define(o, key.clone(), JsValue::Object(Vec::new()));
                }
                let JsValue::Object(fields) = o else { unreachable!() };
                o = fields.iter_mut().find(|(n, _)| *n == key).map(|(_, v)| v).unwrap();
            }
            ipc::define(o, JsString::from(parts.last().unwrap().as_str()), v.clone());
        }
        Ok(())
    }
    fn now_ms(&self) -> f64 {
        self.now
    }
}

#[derive(Default)]
struct FakeKeyring {
    available: bool,
    secret: Mutex<Option<String>>,
}

impl Keyring for FakeKeyring {
    fn available(&self) -> bool {
        self.available
    }
    fn store(&self, password: &str) -> Result<(), String> {
        *self.secret.lock().unwrap() = Some(password.into());
        Ok(())
    }
    fn load(&self) -> Option<String> {
        self.secret.lock().unwrap().clone()
    }
    fn clear(&self) {
        *self.secret.lock().unwrap() = None;
    }
}

// ─── Scenarios ───

fn w(v: Value) -> Value {
    JsValue::from(&v).encode()
}
fn op(ch: &str, args: Vec<Value>) -> Value {
    json!([ch, args.into_iter().map(w).collect::<Vec<_>>()])
}

struct Scenario {
    name: &'static str,
    keyring: bool,
    /// The saved credentials before startup: (kind, email, password).
    credentials: Option<(&'static str, &'static str, &'static str)>,
    users: Value,
    docs: Value,
    failures: Value,
    auth_errors: Value,
    ops: Vec<Value>,
}

fn base() -> Scenario {
    Scenario {
        name: "",
        keyring: true,
        credentials: None,
        users: json!([{ "uid": "u1", "email": "a@x.com", "password": "pw1", "displayName": null, "photoURL": null }, { "uid": "u2", "email": "b@x.com", "password": "pw2", "displayName": "Bee", "photoURL": "https://p/b" }]),
        docs: json!({}),
        failures: json!({}),
        auth_errors: json!({}),
        ops: vec![],
    }
}

fn scenarios(img: &str) -> Vec<Scenario> {
    let sign_in = |e: &str, p: &str| op("auth:signInWithEmail", vec![json!(e), json!(p)]);
    vec![
        Scenario {
            name: "sign in, profile, sync",
            ops: vec![
                op("auth:getUser", vec![]),
                op("cloud:save", vec![json!({ "x": 1 })]),
                sign_in("a@x.com", "nope"),
                sign_in("a@x.com", "pw1"),
                op("auth:getUser", vec![]),
                op("cloud:save", vec![json!({ "playlists": [{ "id": "p" }], "nested": { "a": 1 } })]),
                op("cloud:save", vec![json!(["arr"])]),
                op("cloud:save", vec![json!(null)]),
                op("cloud:load", vec![]),
                op("profile:update", vec![json!({ "displayName": "Ann", "photoURL": "https://p/a" })]),
                op("profile:update", vec![json!({ "photoURL": "https://p/a2" })]),
                op("profile:update", vec![json!({ "displayName": "" })]),
                op("profile:updateExtras", vec![json!({ "banner": "data:image/png;base64,AAA", "bio": "x".repeat(250) })]),
                op("profile:updateExtras", vec![json!({ "banner": "http://nope" })]),
                op("profile:updateExtras", vec![json!({ "banner": format!("data:image/{}", "a".repeat(800_000)) })]),
                op("profile:updateExtras", vec![json!({ "bio": "😀".repeat(150) })]),
                op("profile:get", vec![json!("u1")]),
                op("profile:get", vec![json!("nobody")]),
                op("profile:get", vec![json!(5)]),
                op("auth:sendPasswordReset", vec![json!("a@x.com")]),
                op("auth:signOut", vec![]),
                op("auth:getUser", vec![]),
                op("cloud:load", vec![]),
                op("profile:update", vec![json!({ "displayName": "x" })]),
                op("profile:updateExtras", vec![json!({ "bio": "x" })]),
                json!(["profile:update", [{ "$": "u" }]]),
                op("profile:updateExtras", vec![json!(null)]),
            ],
            ..base()
        },
        Scenario {
            name: "legacy backfill",
            docs: json!({
                "users/u2": { "playlists": [{ "id": "old" }], "liked": [1, 2], "settings": { "theme": "dark", "volume": 3, "deep": { "x": 1 } }, "profile": { "bio": "legacy bio" }, "extra": null },
                "users_v2/u2": { "playlists": [], "liked": [], "settings": { "theme": "light", "deep": {} }, "extra": "keep", "profile": null },
            }),
            ops: vec![sign_in("b@x.com", "pw2"), op("cloud:load", vec![]), op("profile:get", vec![json!("u2")])],
            ..base()
        },
        Scenario {
            name: "legacy only, migration write fails",
            docs: json!({ "users/u2": { "playlists": [1], "profile.banner": "data:image/b", "profile.bio": "flat" } }),
            failures: json!({ "set:users_v2/u2": "PERMISSION_DENIED: no" }),
            ops: vec![sign_in("b@x.com", "pw2"), op("cloud:load", vec![]), op("profile:get", vec![json!("u2")]), op("cloud:save", vec![json!({ "a": 1 })])],
            ..base()
        },
        Scenario {
            name: "reads failing",
            docs: json!({ "users/u1": { "a": 1 }, "users_v2/u1": { "b": 2 } }),
            failures: json!({ "get:users_v2/u1": "unavailable", "update:users_v2/u1": "update failed" }),
            ops: vec![sign_in("a@x.com", "pw1"), op("cloud:load", vec![]), op("profile:updateExtras", vec![json!({ "bio": "b" })])],
            ..base()
        },
        Scenario {
            name: "sign up and auth errors",
            auth_errors: json!({ "signIn": ["Firebase: Error (auth/too-many-requests)."], "reset": ["Firebase: Error (auth/missing-email)."], "profile": ["Firebase: Error (auth/user-token-expired)."] }),
            ops: vec![
                sign_in("a@x.com", "pw1"),
                op("auth:signUpWithEmail", vec![json!("a@x.com"), json!("x")]),
                op("auth:signUpWithEmail", vec![json!("  new@x.com "), json!(12345)]),
                op("auth:sendPasswordReset", vec![json!("")]),
                op("profile:update", vec![json!({ "displayName": "N" })]),
                op("auth:getUser", vec![]),
            ],
            ..base()
        },
        Scenario {
            name: "read image",
            ops: vec![
                op("profile:readImage", vec![json!(format!("{img}/pics/a.png"))]),
                op("profile:readImage", vec![json!(format!("{img}/pics/b.JPG"))]),
                op("profile:readImage", vec![json!(format!("{img}/pics/c.gif"))]),
                op("profile:readImage", vec![json!(format!("{img}/other/d.png"))]),
                op("profile:readImage", vec![json!(format!("{img}/pics/link.png"))]),
                op("profile:readImage", vec![json!(format!("{img}/pics/dir.webp"))]),
                op("profile:readImage", vec![json!(format!("{img}/dl/big.jpeg"))]),
                op("profile:readImage", vec![json!(format!("{img}/desk/../pics/a.png"))]),
                op("profile:readImage", vec![json!(format!("{img}/pics/missing.png"))]),
                op("profile:readImage", vec![json!(5)]),
            ],
            ..base()
        },
        Scenario { name: "auto sign-in from saved credentials", credentials: Some(("saved", "b@x.com", "pw2")), ops: vec![op("auth:getUser", vec![])], ..base() },
        Scenario { name: "saved credentials rejected", credentials: Some(("saved", "b@x.com", "wrong")), ops: vec![op("auth:getUser", vec![])], ..base() },
        Scenario { name: "legacy plaintext password", credentials: Some(("plain", " a@x.com ", "pw1")), ops: vec![op("auth:getUser", vec![])], ..base() },
        Scenario { name: "legacy plaintext, no keyring", keyring: false, credentials: Some(("plain", "a@x.com", "pw1")), ops: vec![op("auth:getUser", vec![]), op("auth:signInWithEmail", vec![json!("a@x.com"), json!("pw1")])], ..base() },
        Scenario { name: "saved, no keyring", keyring: false, credentials: Some(("saved", "a@x.com", "pw1")), ops: vec![op("auth:getUser", vec![])], ..base() },
        Scenario { name: "empty email", credentials: Some(("plain", "  ", "pw1")), ops: vec![op("auth:getUser", vec![])], ..base() },
        Scenario { name: "corrupt file", credentials: Some(("corrupt", "", "")), ops: vec![op("auth:getUser", vec![])], ..base() },
    ]
}

/// Writes the pre-startup credentials file in each side's own format.
fn write_credentials(dir: &std::path::Path, s: &Scenario, rust: bool, keyring: &FakeKeyring) {
    let Some((kind, email, password)) = s.credentials else { return };
    let text = match kind {
        "corrupt" => "{not json".to_owned(),
        "plain" => json!({ "email": email, "password": password }).to_string(),
        _ if rust => {
            *keyring.secret.lock().unwrap() = Some(password.into());
            json!({ "email": email, "enc": cloud_auth::KEYRING_ENC }).to_string()
        }
        _ => {
            use std::io::Write;
            let mut b64 = Vec::new();
            let enc = format!("ENC:{password}");
            let mut e = base64_writer(&mut b64);
            e.write_all(enc.as_bytes()).unwrap();
            drop(e);
            json!({ "email": email, "passwordEnc": String::from_utf8(b64).unwrap(), "enc": "safeStorage-v1" }).to_string()
        }
    };
    std::fs::write(dir.join("auth_credentials.json"), text).unwrap();
}

/// Minimal standard base64 (the JS fake's safeStorage output is `Buffer#toString('base64')`).
fn base64_writer(out: &mut Vec<u8>) -> impl std::io::Write + '_ {
    struct B64<'a>(&'a mut Vec<u8>, Vec<u8>);
    impl std::io::Write for B64<'_> {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.1.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Drop for B64<'_> {
        fn drop(&mut self) {
            const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            for ch in self.1.chunks(3) {
                let n = ch.iter().enumerate().fold(0u32, |acc, (i, b)| acc | u32::from(*b) << (16 - 8 * i));
                for i in 0..4 {
                    self.0.push(if i <= ch.len() { A[(n >> (18 - 6 * i) & 63) as usize] } else { b'=' });
                }
            }
        }
    }
    B64(out, Vec::new())
}

/// The Rust side's credentials, decoded like the JS runner's `credentials()`.
fn rust_credentials(dir: &std::path::Path, k: &FakeKeyring) -> Value {
    let Ok(raw) = std::fs::read_to_string(dir.join("auth_credentials.json")) else { return Value::Null };
    match serde_json::from_str::<Value>(&raw) {
        Ok(j) => {
            let stored = if j["enc"] == cloud_auth::KEYRING_ENC { k.load().map_or(Value::Null, Value::String) } else { Value::Null };
            json!({ "email": j.get("email").cloned().unwrap_or(Value::Null), "stored": stored, "legacy": j.get("password").cloned().unwrap_or(Value::Null) })
        }
        Err(_) => json!({ "raw": raw }),
    }
}

/// Delivers queued auth-state callbacks: the listener's `auth:stateChanged` sends.
async fn flush(sends: &mut Vec<Value>, c: &CloudAuth, b: &FakeBackend) {
    let events: Vec<Option<User>> = std::mem::take(&mut *b.events.lock().unwrap());
    for e in events {
        sends.push(json!(["auth:stateChanged", ipc::normalize_identity(&cloud_auth::state_payload(c, b, e).await).encode()]));
    }
}

async fn run_rust(s: &Scenario, scenario: &Value, paths: &Paths, t: &Translations, dir: &std::path::Path) -> Value {
    let keyring = FakeKeyring { available: s.keyring, ..Default::default() };
    write_credentials(dir, s, true, &keyring);
    let b = FakeBackend {
        users: Mutex::new(s.users.as_array().unwrap().clone()),
        current: Mutex::new(None),
        docs: Mutex::new(s.docs.as_object().unwrap().iter().map(|(k, v)| (k.clone(), JsValue::from(v))).collect()),
        failures: s.failures.clone(),
        auth_errors: Mutex::new(s.auth_errors.clone()),
        events: Mutex::new(vec![None]),
        now: scenario["now"].as_f64().unwrap(),
    };
    let c = Arc::new(CloudAuth::default());
    let logs_before = snowify::logger::LOGS.lock().unwrap().entries().len();
    let mut sends = Vec::new();
    // register(): the listener's initial callback, then `_autoSignIn()`.
    cloud_auth::auto_sign_in(&b, &keyring, paths).await;
    flush(&mut sends, &c, &b).await;
    let mut results = Vec::new();
    for o in scenario["ops"].as_array().unwrap() {
        let ch = o[0].as_str().unwrap();
        let a: Vec<JsValue> = o[1].as_array().unwrap().iter().map(|x| JsValue::decode(x).unwrap()).collect();
        let g = |i: usize| a.get(i).cloned().unwrap_or_default();
        let r = match ch {
            "auth:signInWithEmail" => c.sign_in_with(&b, &keyring, paths, &g(0), &g(1), false).await,
            "auth:signUpWithEmail" => c.sign_in_with(&b, &keyring, paths, &g(0), &g(1), true).await,
            "auth:sendPasswordReset" => c.send_password_reset(&b, &g(0)).await,
            "auth:signOut" => c.sign_out(&b, paths).await,
            "auth:getUser" => c.get_user(&b).await,
            "profile:update" => c.update_profile(&b, t, &g(0)).await,
            "profile:readImage" => c.read_image(paths, &g(0)),
            "profile:updateExtras" => c.update_extras(&b, t, &g(0)).await,
            "profile:get" => c.get_profile(&b, &g(0)).await,
            "cloud:save" => c.save(&b, t, &g(0)).await,
            "cloud:load" => c.load(&b).await,
            _ => panic!("no handler {ch}"),
        };
        results.push(match r {
            Ok(v) => json!({ "ok": ipc::normalize_identity(&v).encode() }),
            Err(e) => json!({ "err": JsValue::String(e.rejection_message(ch)).encode() }),
        });
        flush(&mut sends, &c, &b).await;
    }
    let logs: Vec<Value> = snowify::logger::LOGS.lock().unwrap().entries()[logs_before..].iter().map(|e| json!([e.level.encode(), e.msg])).collect();
    let docs = JsValue::Object(b.docs.lock().unwrap().iter().map(|(k, v)| (JsString::from(k.as_str()), v.clone())).collect());
    json!({ "results": results, "logs": logs, "sends": sends, "docs": docs.encode(), "credentials": rust_credentials(dir, &keyring) })
}

#[tokio::test(flavor = "current_thread")]
async fn cloud_auth_matches_source() {
    let base_dir = std::env::temp_dir().join(format!("snowify-parity-cloud-auth-{}", std::process::id()));
    let img = base_dir.join("img");
    let ud = base_dir.join("ud");
    let t = Translations::load(&root().join("../src/renderer/locales"), "en").unwrap();
    let paths = Paths {
        user_data: ud.display().to_string(),
        image_roots: [img.join("pics").display().to_string(), img.join("dl").display().to_string(), img.join("desk").display().to_string()],
        cwd: std::env::current_dir().unwrap().display().to_string(),
    };
    let reset = || {
        let _ = std::fs::remove_dir_all(&base_dir);
        for d in ["pics", "dl", "desk", "other"] {
            std::fs::create_dir_all(img.join(d)).unwrap();
        }
        std::fs::create_dir_all(img.join("pics/dir.webp")).unwrap();
        std::fs::create_dir_all(&ud).unwrap();
        std::fs::write(img.join("pics/a.png"), b"\x89PNG..").unwrap();
        std::fs::write(img.join("pics/b.JPG"), b"jpg").unwrap();
        std::fs::write(img.join("pics/c.gif"), b"gif").unwrap();
        std::fs::write(img.join("other/d.png"), b"d").unwrap();
        std::os::unix::fs::symlink(img.join("other/d.png"), img.join("pics/link.png")).unwrap();
        std::fs::write(img.join("dl/big.jpeg"), vec![0u8; 8 * 1024 * 1024 + 1]).unwrap();
    };
    let mut d = Diff::default();
    for s in scenarios(&img.display().to_string()) {
        let scenario = json!({
            "paths": { "userData": paths.user_data, "pictures": paths.image_roots[0], "downloads": paths.image_roots[1], "desktop": paths.image_roots[2] },
            "keyring": { "available": s.keyring },
            "users": s.users, "docs": s.docs, "failures": s.failures, "authErrors": s.auth_errors,
            "now": 1_761_800_000_000_i64, "ops": s.ops,
        });
        reset();
        write_credentials(&ud, &s, false, &FakeKeyring::default());
        let src = common::run_source("cloud_auth.js", &scenario);
        reset();
        let rust = run_rust(&s, &scenario, &paths, &t, &ud).await;
        for key in ["results", "logs", "sends"] {
            let (a, b) = (&src[key], &rust[key]);
            for i in 0..a.as_array().unwrap().len().max(b.as_array().unwrap().len()) {
                d.check(format_args!("{} {key}[{i}]", s.name), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
            }
        }
        d.check(format_args!("{} docs", s.name), &src["docs"], &rust["docs"]);
        d.check(format_args!("{} credentials", s.name), &src["credentials"], &rust["credentials"]);
    }
    let _ = std::fs::remove_dir_all(&base_dir);
    d.finish("cloud_auth");
}

/// Owner decision (PORT.md "Not ported"): a file written by Electron's safeStorage is not
/// decrypted, so the user signs in once more; it is left in place until then.
#[test]
fn electron_safe_storage_file_is_not_read() {
    let dir = std::env::temp_dir().join(format!("snowify-cloud-auth-ss-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("auth_credentials.json"), r#"{"email":"a@x.com","passwordEnc":"djExMAAA","enc":"safeStorage-v1"}"#).unwrap();
    let k = FakeKeyring { available: true, secret: Mutex::new(Some("stale".into())) };
    let p = Paths { user_data: dir.display().to_string(), image_roots: Default::default(), cwd: "/".into() };
    assert_eq!(cloud_auth::load_credentials(&k, &p), None);
    assert!(dir.join("auth_credentials.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
