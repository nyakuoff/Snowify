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
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<snowify::ipc::HandlerResult>> + 'a>>
{
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
    let repo_root = common::root()
        .join("..")
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
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
        rel(),
        rel(),
        rel(),
        rel(),
        rel(),
        rel(),
    ];
    let scenario = json!({ "module": "updater", "userData": "/mock/ud", "isPackaged": false, "dialogs": [], "http": http, "ops": ops });
    let src = common::run_source("main_module.js", &scenario);
    let host = MockHost::new(&scenario, &repo_root);
    let rust = host::run_ops(&host, &scenario, &dispatch).await;
    let mut d = Diff::default();
    for key in ["results", "logs", "httpCalls"] {
        let (a, b) = (&src[key], &rust[key]);
        for i in 0..a.as_array().unwrap().len().max(b.as_array().unwrap().len()) {
            d.check(
                format_args!("{key}[{i}]"),
                a.get(i).unwrap_or(&Value::Null),
                b.get(i).unwrap_or(&Value::Null),
            );
        }
    }
    packaged_parity(&mut d).await;
    d.finish("updater");
}

/// Scripted network/installer effects; the policy under test is the production policy.
struct FakeUpdater {
    checks: std::collections::VecDeque<Value>,
    downloads: std::collections::VecDeque<Value>,
    installs: std::collections::VecDeque<Value>,
    calls: Vec<Value>,
    checked: Option<String>,
}

fn update_error(value: &Value) -> updater::UpdateError {
    if value.is_null() {
        updater::UpdateError {
            message: JsValue::Undefined,
            logged: JsValue::Null,
        }
    } else {
        snowify::net::JsErr::error(value.as_str().unwrap_or_default()).into()
    }
}

impl updater::Backend for FakeUpdater {
    async fn check(&mut self) -> Result<Option<updater::Update>, updater::UpdateError> {
        self.calls.push(json!(["check"]));
        let r = self.checks.pop_front().expect("scripted check");
        if let Some(error) = r.get("error") {
            return Err(update_error(error));
        }
        self.checked = r["version"].as_str().map(str::to_owned);
        Ok(r["available"]
            .as_bool()
            .unwrap_or(false)
            .then(|| updater::Update {
                version: r["version"].as_str().unwrap().into(),
                body: r["notes"].as_str().map(str::to_owned),
            }))
    }

    fn checked_version(&self) -> Option<String> {
        self.checked.clone()
    }

    async fn download(
        &mut self,
        progress: &mut (dyn FnMut(f64) + Send),
    ) -> Result<String, updater::UpdateError> {
        self.calls.push(json!(["download"]));
        let r = self.downloads.pop_front().expect("scripted download");
        for p in r["progress"].as_array().into_iter().flatten() {
            progress(p.as_f64().unwrap());
        }
        if let Some(error) = r.get("error") {
            return Err(update_error(error));
        }
        Ok(r["version"].as_str().unwrap().into())
    }

    fn install(&mut self, silent: bool, relaunch: bool) -> Result<(), updater::UpdateError> {
        self.calls.push(json!(["install", silent, relaunch]));
        if let Some(r) = self.installs.pop_front() {
            if let Some(error) = r.get("error") {
                return Err(update_error(error));
            }
        }
        Ok(())
    }
}

async fn packaged_parity(d: &mut Diff) {
    // Separate module instances, as separate Electron processes would have. Startup rejection
    // never logs; handler rejection does. Downloads emit the library error and catch status.
    let scenarios = vec![
        json!({"packaged": false, "actions": ["timer", "check", "install", "quit"]}),
        json!({"packaged": true, "checks": [
            {"available": true, "version": "2.1.0", "notes": "## Notes\nü"},
            {"available": false, "version": "2.0.0"},
            {"available": false, "version": "1.0.0"},
            {"empty": true},
        ], "actions": ["timer", "check", "check", "check", "quit"]}),
        json!({"packaged": true, "checks": [
            {"error": "startup failed"}, {"error": "manual failed"}, {"error": ""}, {"error": null},
        ], "actions": ["timer", "check", "check", "check", "quit"]}),
        json!({"packaged": true, "checks": [{"available": true, "version": "2.1.0"}],
            "downloads": [{"version": "2.1.0", "progress": [0, 0.49, 0.49999999999999994, 0.5, 49.49, 49.5, 99.5, 100]}],
            "actions": ["check", "install", "install", "quit"]}),
        json!({"packaged": true, "checks": [{"available": true, "version": "2.1.0", "notes": ""}],
            "downloads": [{"version": "2.1.0", "progress": [10.25, 100]}],
            "actions": ["timer", "install", "quit", "quit"]}),
        json!({"packaged": true, "checks": [{"available": true, "version": "2.1.0"}],
            "downloads": [{"error": "network lost", "progress": [20.5]}, {"error": ""}, {"error": null},
                {"version": "2.1.0", "progress": [100]}],
            "actions": ["check", "install", "install", "install", "install", "quit"]}),
        json!({"packaged": true, "checks": [{"available": true, "version": "2.1.0"}, {"available": true, "version": "2.2.0"}],
            "downloads": [{"version": "2.1.0", "progress": [100]}],
            "actions": ["check", "install", "check", "install", "quit"]}),
        json!({"packaged": true, "destroyed": true, "checks": [{"error": "no window"}],
            "actions": ["check"]}),
        json!({"packaged": true, "eventError": "", "actions": ["error"]}),
        json!({"packaged": true, "eventError": null, "actions": ["error"]}),
        json!({"packaged": true, "checks": [{"available": true, "version": "2.1.0"}],
            "downloads": [{"version": "2.1.0", "progress": [100]}],
            "installs": [{"error": "installer failed"}, {"error": ""}],
            "actions": ["check", "install", "install", "install", "quit"]}),
    ];
    for (index, scenario) in scenarios.into_iter().enumerate() {
        *snowify::logger::LOGS.lock().unwrap() = snowify::logger::LogBuffer::new();
        let source = common::run_source("updater.js", &scenario);
        let backend = FakeUpdater {
            checks: scenario["checks"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into(),
            downloads: scenario["downloads"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into(),
            installs: scenario["installs"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into(),
            calls: Vec::new(),
            checked: None,
        };
        let packaged = scenario["packaged"].as_bool().unwrap();
        let mut flow = updater::AutoUpdater::new(backend, !packaged);
        let delay = flow.init();
        let sends = std::sync::Mutex::new(Vec::new());
        let send = |v: JsValue| {
            if scenario["destroyed"] != true {
                sends
                    .lock()
                    .unwrap()
                    .push(json!(["updater:status", v.encode()]));
            }
        };
        let mut results = Vec::new();
        for action in scenario["actions"].as_array().unwrap() {
            let result = match action.as_str().unwrap() {
                "timer" => {
                    if delay.is_some() {
                        flow.check(false, &send).await;
                    }
                    Value::Null
                }
                "quit" => {
                    flow.quit(&send);
                    Value::Null
                }
                "error" => {
                    flow.error(&update_error(&scenario["eventError"]), &send);
                    Value::Null
                }
                "check" => json!({"ok": flow.check(true, &send).await.encode()}),
                "install" => {
                    flow.install(&send).await;
                    json!({"ok": JsValue::Undefined.encode()})
                }
                _ => unreachable!(),
            };
            results.push(result);
        }
        let logs: Vec<Value> = snowify::logger::LOGS
            .lock()
            .unwrap()
            .entries()
            .iter()
            .map(|e| json!([e.level.encode(), e.msg]))
            .collect();
        let rust = json!({"results": results, "sends": sends.into_inner().unwrap(), "calls": flow.backend.calls,
            "logs": logs, "timers": delay.map(|d| vec![d.as_millis()]).unwrap_or_default(),
            "config": if packaged { json!([false, true]) } else { json!([null, null]) },
        });
        for key in ["results", "sends", "calls", "logs", "timers", "config"] {
            let a = source[key].as_array().unwrap();
            let b = rust[key].as_array().unwrap();
            d.check(
                format_args!("scenario {index} {key} length"),
                &json!(a.len()),
                &json!(b.len()),
            );
            for i in 0..a.len().max(b.len()) {
                d.check(
                    format_args!("scenario {index} {key}[{i}]"),
                    a.get(i).unwrap_or(&Value::Null),
                    b.get(i).unwrap_or(&Value::Null),
                );
            }
        }
    }
}
