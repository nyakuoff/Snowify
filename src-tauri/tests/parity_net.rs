//! Differential test of real network failures: Node's `https.get` errors (message and
//! `JSON.stringify(err)`) vs `RealHost::https_get`'s mapping of them.

use serde_json::{Value, json};
use snowify::{
    host::{Host, NetError},
    ipc::{JsValue, json_stringify},
    real_host::RealHost,
};

mod common;
use common::Diff;

#[tokio::test(flavor = "current_thread")]
async fn network_errors_match_node() {
    let urls = [
        "https://snowify-nonexistent-host.invalid/registry.json",
        "https://127.0.0.1:1/x",
        "not a url",
        "http://example.com/x",
    ];
    let src = common::run_source("net.js", &json!(urls));
    let host = RealHost { window: None };
    let mut d = Diff::default();
    for (u, s) in urls.iter().zip(src.as_array().unwrap()) {
        let r = match host
            .https_get(u, &[("User-Agent", "Snowify")], 15_000)
            .await
        {
            Ok(res) => json!({ "ok": res.status }),
            Err(NetError::Timeout) => json!({ "message": "Timeout" }),
            Err(NetError::Failed { message, props, .. }) if s.get("sync").is_some() => {
                let _ = props;
                json!({ "sync": message })
            }
            Err(NetError::Failed { message, props, .. }) => {
                let obj = JsValue::object(props.iter().map(|(k, v)| (k.as_str(), v.clone())));
                json!({ "message": message, "props": json_stringify(&obj).unwrap().unwrap() })
            }
        };
        d.check(u, s, &r);
    }
    let _: Option<Value> = None;
    d.finish("net");
}
