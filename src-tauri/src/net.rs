//! The source's two hand-rolled `httpsGet` helpers (`themes.js`, `plugins.js`): follow up to 5
//! redirects via `location`, reject `HTTP <status>` unless 200, reject `Timeout` after 15 s of
//! inactivity, reject `Too many redirects` past 5.

use crate::{
    host::{Host, NetError},
    ipc::{JsString, JsValue},
};

/// A rejection from `httpsGet`: the `Error` it rejects with.
#[derive(Debug, Clone, PartialEq)]
pub struct JsErr {
    pub name: String,
    pub message: JsString,
    /// Own enumerable properties (what `JSON.stringify(err)` shows in the log).
    pub props: Vec<(String, JsValue)>,
}

impl JsErr {
    pub fn error(message: impl Into<JsString>) -> Self {
        Self {
            name: "Error".into(),
            message: message.into(),
            props: Vec::new(),
        }
    }

    /// `JSON.stringify(err)` as `console.error` logs it.
    pub fn log_json(&self) -> JsValue {
        JsValue::object(self.props.iter().map(|(k, v)| (k.as_str(), v.clone())))
    }
}

impl From<crate::node::FsError> for JsErr {
    fn from(e: crate::node::FsError) -> Self {
        let props = (&e.to_json()).into();
        let JsValue::Object(props) = props else {
            unreachable!("to_json is an object")
        };
        Self {
            name: "Error".into(),
            props: props
                .into_iter()
                .map(|(k, v)| (k.to_string_lossy(), v))
                .collect(),
            message: e.message.into(),
        }
    }
}

const TIMEOUT_MS: u64 = 15_000;

/// The two variants differ only in headers and in how the body becomes a value.
#[derive(Clone, Copy)]
pub enum Flavor {
    /// `themes.js`: `User-Agent: Snowify`, body as text.
    Themes,
    /// `plugins.js`: also `Accept: application/vnd.github+json`.
    Plugins,
}

/// `httpsGet(url)`: the response body bytes (callers decode as UTF-8 or keep binary).
pub async fn https_get<H: Host>(host: &H, url: &str, flavor: Flavor) -> Result<Vec<u8>, JsErr> {
    let headers: &[(&str, &str)] = match flavor {
        Flavor::Themes => &[("User-Agent", "Snowify")],
        Flavor::Plugins => &[
            ("User-Agent", "Snowify"),
            ("Accept", "application/vnd.github+json"),
        ],
    };
    let mut url = url.to_owned();
    for redirects in 0.. {
        if redirects > 5 {
            return Err(JsErr::error("Too many redirects"));
        }
        let res = match host.https_get(&url, headers, TIMEOUT_MS).await {
            Ok(r) => r,
            Err(NetError::Timeout) => return Err(JsErr::error("Timeout")),
            Err(NetError::Failed {
                name,
                message,
                props,
            }) if redirects == 0 => {
                return Err(JsErr {
                    name,
                    message: message.into(),
                    props,
                });
            }
            // A redirect target that makes https.get throw does so inside the response
            // callback: an uncaught main-process exception, and the promise never settles.
            Err(NetError::Failed { .. }) => return std::future::pending().await,
        };
        if (300..400).contains(&res.status)
            && let Some(loc) = res.header("location").filter(|l| !l.is_empty())
        {
            url = loc.to_owned();
            continue;
        }
        if res.status != 200 {
            return Err(JsErr::error(format!("HTTP {}", res.status)));
        }
        return Ok(res.body);
    }
    unreachable!()
}
