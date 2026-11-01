//! Firebase JS SDK 12.11.0 Node behaviour used by cloud-auth. Node getAuth uses
//! in-memory persistence: credentials are deliberately not written by this layer.
use crate::host::{FetchRequest, Host};
use serde_json::{Value, json};
/// Public Firebase project API key from src/firebase.js.
pub const API_KEY: &str = "AIzaSyCNuw8kqgbULTLjC890BzKWvnmdvFCX0og";
/// Firebase application identifier from src/firebase.js.
pub const APP_ID: &str = "1:925903561467:web:c4298e4f47ac80d1c9c65e";
#[derive(Debug, Clone)]
/// SDK error code and exact user-facing message.
pub struct Error {
    /// Firebase error code; empty for native JavaScript errors.
    pub code: String,
    /// Exact error.message returned by the SDK.
    pub message: String,
}
impl Error {
    fn auth(code: &str, detail: Option<&str>) -> Self {
        let code = format!("auth/{code}");
        Self {
            message: detail
                .filter(|s| !s.is_empty())
                .map(|s| format!("Firebase: {s} ({code})."))
                .unwrap_or_else(|| format!("Firebase: Error ({code}).")),
            code,
        }
    }
}
/// User fields exposed by cloud-auth; tokens stay private.
#[derive(Debug, Clone)]
pub struct User {
    /// Firebase user identifier.
    pub uid: String,
    /// Email address or null.
    pub email: Value,
    /// Display name or null.
    pub display_name: Value,
    /// Profile photo URL or null.
    pub photo_url: Value,
    token: String,
    refresh: String,
    expires: i64,
}
impl User {
    /// Snapshot of the user fields read by cloud-auth.
    pub fn fields(&self) -> Value {
        json!({"uid":self.uid,"email":self.email,"displayName":self.display_name,"photoURL":self.photo_url})
    }
}
/// State events are queued in SDK callback order; drain after each awaited operation.
/// Registering a listener queues the initial current user, including null.
#[derive(Default)]
pub struct Auth {
    /// Signed-in user, or None after sign-out.
    pub current_user: Option<User>,
    events: Vec<Value>,
    listening: bool,
    heartbeat_sent: bool,
}
impl Auth {
    /// Register the startup listener and queue its initial snapshot.
    pub fn on_auth_state_changed(&mut self) {
        self.listening = true;
        self.events.push(self.fields());
    }
    /// Drain listener snapshots in delivery order.
    pub fn take_events(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.events)
    }
    /// Snapshot of the user fields read by cloud-auth.
    pub fn fields(&self) -> Value {
        self.current_user
            .as_ref()
            .map(User::fields)
            .unwrap_or(Value::Null)
    }
    /// Clear the current user and notify the listener if the uid changed.
    pub fn sign_out(&mut self) {
        if self.current_user.take().is_some() && self.listening {
            self.events.push(Value::Null);
        }
    }
    async fn request<H: Host>(
        &mut self,
        host: &H,
        endpoint: &str,
        body: String,
        refresh: bool,
    ) -> Result<Value, Error> {
        let mut headers = vec![
            (
                "Content-Type".into(),
                if refresh {
                    "application/x-www-form-urlencoded"
                } else {
                    "application/json"
                }
                .into(),
            ),
            (
                "X-Client-Version".into(),
                "Node/JsCore/12.11.0/FirebaseCore-web".into(),
            ),
            ("X-Firebase-gmpid".into(), APP_ID.into()),
        ];
        if !self.heartbeat_sent {
            let date = crate::js::iso_string(host.now_ms())[..10].to_owned();
            let heartbeat = json!({"version":2,"heartbeats":[{"agent":"fire-core-node/0.14.10 fire-core-cjs2020/0.14.10 fire-js/ fire-js-all-app/12.11.0 fire-auth-node/1.12.2 fire-auth-cjs2020/1.12.2 fire-fst-node/4.13.0 fire-fst-cjs2020/4.13.0","dates":[date]}]});
            headers.push((
                "X-Firebase-Client".into(),
                crate::media::base64url(heartbeat.to_string().as_bytes()),
            ));
            self.heartbeat_sent = true;
        }
        let domain = if refresh {
            "securetoken.googleapis.com"
        } else {
            "identitytoolkit.googleapis.com"
        };
        let config = endpoint == "recaptchaConfig";
        let req = FetchRequest {
            method: if config { "GET" } else { "POST" },
            body: if config {
                None
            } else {
                Some(body.into_bytes())
            },
            url: if config {
                format!(
                    "https://{domain}/v2/recaptchaConfig?key={API_KEY}&clientType=CLIENT_TYPE_WEB&version=RECAPTCHA_ENTERPRISE"
                )
            } else {
                format!("https://{domain}/v1/{endpoint}?key={API_KEY}")
            },
            headers,
            manual_redirect: false,
        };
        let r = host
            .fetch(req)
            .await
            .map_err(|_| Error::auth("network-request-failed", None))?;
        let v: Value = serde_json::from_slice(&r.body)
            .map_err(|_| Error::auth("network-request-failed", None))?;
        if v.get("needConfirmation").is_some() {
            return Err(Error::auth(
                "account-exists-with-different-credential",
                None,
            ));
        }
        if r.ok() && v.get("errorMessage").is_none() {
            return Ok(v);
        }
        let e = if r.ok() {
            v["errorMessage"].as_str()
        } else {
            v["error"]["message"].as_str()
        }
        .ok_or_else(|| Error::auth("network-request-failed", None))?;
        let mut parts = e.split(" : ");
        let server = parts.next().unwrap();
        let detail = parts.next();
        let map: Value = serde_json::from_str(include_str!("auth_errors.json")).unwrap();
        let fallback = regex::Regex::new(r"[_\s]+")
            .unwrap()
            .replace_all(&server.to_lowercase(), "-")
            .into_owned();
        let code = map[server].as_str().unwrap_or(&fallback);
        Err(Error::auth(
            code,
            if [
                "EMAIL_EXISTS",
                "USER_DISABLED",
                "FEDERATED_USER_ID_ALREADY_LINKED",
            ]
            .contains(&server)
            {
                None
            } else {
                detail
            },
        ))
    }
    /// Sign in and reload account fields, following the Node SDK request sequence.
    pub async fn sign_in_with_email_and_password<H: Host>(
        &mut self,
        host: &H,
        email: &str,
        password: &str,
    ) -> Result<User, Error> {
        self.sign_in(host, email, password, false).await
    }
    /// Create a password account and make it the current user.
    pub async fn create_user_with_email_and_password<H: Host>(
        &mut self,
        host: &H,
        email: &str,
        password: &str,
    ) -> Result<User, Error> {
        self.sign_in(host, email, password, true).await
    }
    async fn sign_in<H: Host>(
        &mut self,
        host: &H,
        email: &str,
        password: &str,
        signup: bool,
    ) -> Result<User, Error> {
        if !signup && email.is_empty() {
            return Err(Error::auth("missing-email", None));
        }
        if !signup && password.is_empty() {
            return Err(Error::auth("missing-password", None));
        }
        let body = json!({"returnSecureToken":true,"email":email,"password":password,"clientType":"CLIENT_TYPE_WEB"});
        let result = self
            .request(
                host,
                if signup {
                    "accounts:signUp"
                } else {
                    "accounts:signInWithPassword"
                },
                body.to_string(),
                false,
            )
            .await;
        let v = match result {
            Err(e) if e.code == "auth/missing-recaptcha-token" => {
                // injectRecaptchaFields retries verify once with forceRefresh. Node cannot load
                // the browser verifier; network failures retain their Firebase error.
                let mut error = e;
                for _ in 0..2 {
                    error = match self
                        .request(host, "recaptchaConfig", String::new(), false)
                        .await
                    {
                        Err(e) => e,
                        Ok(v) => Error {
                            code: String::new(),
                            message: if v.get("recaptchaKey").is_none() {
                                "recaptcha Enterprise site key undefined"
                            } else {
                                "RecaptchaVerifier is only supported in browser"
                            }
                            .into(),
                        },
                    };
                }
                return Err(error);
            }
            other => other?,
        };
        if v.get("mfaPendingCredential").is_some() {
            return Err(Error::auth("multi-factor-auth-required", None));
        }
        let mut v = v;
        if seconds(&v["expiresIn"]) < 30 {
            let body = format!(
                "grant_type=refresh_token&refresh_token={}",
                url::form_urlencoded::byte_serialize(
                    v["refreshToken"].as_str().unwrap_or("").as_bytes()
                )
                .collect::<String>()
                .replace('+', "%20")
            );
            let refreshed = self.request(host, "token", body, true).await?;
            v["idToken"] = refreshed["access_token"].clone();
            v["refreshToken"] = refreshed["refresh_token"].clone();
            v["expiresIn"] = refreshed["expires_in"].clone();
        }
        let token = v["idToken"].as_str().unwrap_or("").to_owned();
        let lookup = self
            .request(
                host,
                "accounts:lookup",
                json!({"idToken":token}).to_string(),
                false,
            )
            .await?;
        let data = lookup["users"]
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| Error::auth("internal-error", None))?;
        let user = User {
            uid: data["localId"].as_str().unwrap_or("").into(),
            email: nullable(&data["email"]),
            display_name: nullable(&data["displayName"]),
            photo_url: nullable(&data["photoUrl"]),
            token,
            refresh: v["refreshToken"].as_str().unwrap_or("").into(),
            expires: host.now_ms() + seconds(&v["expiresIn"]) * 1000,
        };
        let changed = self.current_user.as_ref().is_none_or(|u| u.uid != user.uid);
        self.current_user = Some(user.clone());
        if changed && self.listening {
            self.events.push(user.fields());
        }
        Ok(user)
    }
    /// Send a PASSWORD_RESET action email.
    pub async fn send_password_reset_email<H: Host>(
        &mut self,
        host: &H,
        email: &str,
    ) -> Result<(), Error> {
        self.request(
            host,
            "accounts:sendOobCode",
            json!({"requestType":"PASSWORD_RESET","email":email,"clientType":"CLIENT_TYPE_WEB"})
                .to_string(),
            false,
        )
        .await
        .map(|_| ())
    }
    /// Return the id token, refreshing when forced or within the expiry buffer.
    pub async fn get_id_token<H: Host>(&mut self, host: &H, force: bool) -> Result<String, Error> {
        let u = self
            .current_user
            .as_ref()
            .ok_or_else(|| Error::auth("null-user", None))?;
        if !force && host.now_ms() <= u.expires - 30000 {
            return Ok(u.token.clone());
        }
        let body = format!(
            "grant_type=refresh_token&refresh_token={}",
            url::form_urlencoded::byte_serialize(u.refresh.as_bytes())
                .collect::<String>()
                .replace('+', "%20")
        );
        let result = self.request(host, "token", body, true).await;
        let v = self.logout_if_invalidated(result)?;
        let u = self.current_user.as_mut().unwrap();
        u.token = v["access_token"].as_str().unwrap_or("").into();
        u.refresh = v["refresh_token"].as_str().unwrap_or("").into();
        u.expires = host.now_ms() + seconds(&v["expires_in"]) * 1000;
        Ok(u.token.clone())
    }
    fn logout_if_invalidated(&mut self, result: Result<Value, Error>) -> Result<Value, Error> {
        if let Err(e) = &result {
            if ["auth/user-disabled", "auth/user-token-expired"].contains(&e.code.as_str()) {
                self.sign_out();
            }
        }
        result
    }
    /// Update displayName/photoURL and retain the SDK response fields.
    pub async fn update_profile<H: Host>(
        &mut self,
        host: &H,
        profile: &Value,
    ) -> Result<(), Error> {
        if profile.get("displayName").is_none() && profile.get("photoURL").is_none() {
            return Ok(());
        }
        let token = self.get_id_token(host, false).await?;
        let mut body = json!({"idToken":token});
        for (a, b) in [("displayName", "displayName"), ("photoURL", "photoUrl")] {
            if let Some(v) = profile.get(a) {
                body[b] = v.clone();
            }
        }
        body["returnSecureToken"] = json!(true);
        let result = self
            .request(host, "accounts:update", body.to_string(), false)
            .await;
        let v = self.logout_if_invalidated(result)?;
        let u = self.current_user.as_mut().unwrap();
        u.display_name = nullable(&v["displayName"]);
        u.photo_url = nullable(&v["photoUrl"]);
        if let Some(t) = v["idToken"].as_str() {
            u.token = t.into();
            u.refresh = v["refreshToken"].as_str().unwrap_or("").into();
            u.expires = host.now_ms() + seconds(&v["expiresIn"]) * 1000;
        }
        Ok(())
    }
}
fn nullable(v: &Value) -> Value {
    if v.is_null() || v == "" {
        Value::Null
    } else {
        v.clone()
    }
}
fn seconds(v: &Value) -> i64 {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| v.as_i64())
        .unwrap_or(0)
}

/// Firestore document operations and SDK value serialization.
pub mod firestore;
