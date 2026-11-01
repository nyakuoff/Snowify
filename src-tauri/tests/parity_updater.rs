//! Differential test: `src/main/updater.js` (unmodified, dev mode; https mocked) vs
//! `snowify::updater` for the changelog and release-list handlers.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, updater};

mod common;
#[path = "common/host.rs"]
mod host;
use common::Diff;
use host::MockHost;

const REL: &str = "https://api.github.com/repos/nyakuoff/Snowify/releases";

fn ok(v: Value) -> Value {
    json!({ "status": 200, "body": v.to_string() })
}

fn w(v: Value) -> Value {
    JsValue::from(&v).encode()
}

fn dispatch<'a>(
    h: &'a MockHost,
    ch: String,
    args: Vec<JsValue>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<snowify::ipc::HandlerResult>> + 'a>> {
    Box::pin(async move {
        let a0 = args.first().cloned().unwrap_or_default();
        Some(match ch.as_str() {
            "app:getChangelog" => updater::changelog(h, &a0).await,
            "app:getRecentReleases" => updater::recent_releases(h).await,
            _ => return None,
        })
    })
}

#[tokio::test(flavor = "current_thread")]
async fn updater_matches_source() {
    let repo_root = common::root().join("..").canonicalize().unwrap().display().to_string();
    let release = json!({ "tag_name": "v2.1.0", "name": "Snowify 2.1", "body": "## Notes\n- ü", "published_at": "2026-10-01T00:00:00Z", "html_url": "https://github.com/x" });
    let releases = json!([
        release,
        { "tag_name": "2.0.0", "name": null, "assets": [{ "name": "a.AppImage", "size": 12, "browser_download_url": "https://dl/a" }, { "name": "b" }, 5] },
        { "tag_name": "", "body": 0 },
        { "tag_name": null, "name": "", "assets": [] },
    ]);
    let http = json!({
        format!("{REL}/tags/v2.1.0"): [ok(release.clone())],
        format!("{REL}/tags/v1.9"): [ok(json!({ "tag_name": "", "name": "", "body": null }))],
        format!("{REL}/tags/v3"): [ok(json!({ "tag_name": 3 }))],
        format!("{REL}/tags/v4"): [ok(json!(null))],
        format!("{REL}/tags/v5"): [{ "status": 200, "body": "<html>nope" }],
        format!("{REL}/tags/v6"): [{ "status": 404, "body": "{}" }],
        format!("{REL}/tags/v7"): [{ "timeout": true }],
        format!("{REL}/tags/v8"): [{ "error": { "name": "Error", "message": "getaddrinfo ENOTFOUND api.github.com", "props": { "errno": -3008, "code": "ENOTFOUND", "syscall": "getaddrinfo", "hostname": "api.github.com" } } }],
        format!("{REL}/tags/v 9 #x"): [ok(json!({ "tag_name": "vv9" }))],
        format!("{REL}?per_page=20"): [
            ok(releases),
            ok(json!([{ "tag_name": "x", "assets": "abc" }])),
            ok(json!({ "length": 1 })),
            ok(json!([null])),
            { "status": 500, "body": "" },
            ok(json!([{ "tag_name": { "a": 1 } }])),
        ],
    });
    let ch = |v: Value| json!(["app:getChangelog", [w(v)]]);
    let rel = || json!(["app:getRecentReleases", []]);
    let ops = vec![
        ch(json!("2.1.0")),
        ch(json!("v2.1.0")),
        ch(json!("1.9")),
        ch(json!("3")),
        ch(json!("v4")),
        ch(json!("5")),
        ch(json!("6")),
        ch(json!("7")),
        ch(json!("8")),
        ch(json!(" 9 #x")),
        ch(json!(5)),
        ch(json!(null)),
        json!(["app:getChangelog", [{ "$": "box", "v": "2.1.0" }]]),
        json!(["app:getChangelog", []]),
        rel(), rel(), rel(), rel(), rel(), rel(),
    ];
    let scenario = json!({ "module": "updater", "userData": "/mock/ud", "isPackaged": false, "dialogs": [], "http": http, "ops": ops });
    let src = common::run_source("main_module.js", &scenario);
    let host = MockHost::new(&scenario, &repo_root);
    let rust = host::run_ops(&host, &scenario, &dispatch).await;
    let mut d = Diff::default();
    for key in ["results", "logs", "httpCalls"] {
        let (a, b) = (&src[key], &rust[key]);
        for i in 0..a.as_array().unwrap().len().max(b.as_array().unwrap().len()) {
            d.check(format_args!("{key}[{i}]"), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
        }
    }
    d.finish("updater");
}
