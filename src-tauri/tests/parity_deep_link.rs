//! Differential test: deep-link handling in `src/main.js` (loaded unmodified) vs
//! `snowify::deep_link`, including the window branch through a recording mock window.

use serde_json::{Value, json};
use snowify::{
    deep_link::{self, DeepLink, MainWindow, WindowDestroyed},
    logger,
};
use std::cell::RefCell;

mod common;
use common::Diff;

/// Mirrors the mock in `parity/node/deep_link.js`: records calls, fails at `fail_at`.
struct MockWindow {
    web_contents: bool,
    minimized: bool,
    fail_at: Option<String>,
    calls: RefCell<Vec<String>>,
}

impl MockWindow {
    fn call(&self, name: String) -> Result<(), WindowDestroyed> {
        if self.fail_at.as_deref() == name.split(' ').next() {
            self.calls.borrow_mut().push(format!("!{name}"));
            return Err(WindowDestroyed);
        }
        self.calls.borrow_mut().push(name);
        Ok(())
    }
}

impl MainWindow for MockWindow {
    fn has_web_contents(&self) -> Result<bool, WindowDestroyed> {
        self.call("webContents".into()).map(|()| self.web_contents)
    }
    fn send_deep_link(&self, link: &DeepLink) -> Result<(), WindowDestroyed> {
        self.call("webContents".into())?;
        self.call(format!(
            "send app:deepLink {}/{}",
            link.kind.as_str(),
            link.id
        ))
    }
    fn is_minimized(&self) -> Result<bool, WindowDestroyed> {
        self.call("isMinimized".into()).map(|()| self.minimized)
    }
    fn restore(&self) -> Result<(), WindowDestroyed> {
        self.call("restore".into())
    }
    fn focus(&self) -> Result<(), WindowDestroyed> {
        self.call("focus".into())
    }
}

fn corpus() -> Vec<Value> {
    let prefixes = [
        "snowify://",
        "snowify:/",
        "snowify:",
        "snowify:///",
        "Snowify://",
        "",
        "http://",
        " snowify://",
        "snowify:\\\\",
    ];
    let kinds = [
        "track",
        "album",
        "artist",
        "Track",
        "playlist",
        "",
        "track/",
        "constructor",
    ];
    let ids = [
        "abc", "", "a/b", "a//b/", "/", "日本", "x?y=1#z", "%20", "..", "a b", "🎵",
    ];
    let mut urls = vec![
        "".to_string(),
        "snowify://".into(),
        "snowify://track".into(),
        "snowify:track/x".into(),
    ];
    for p in prefixes {
        for k in kinds {
            for id in ids {
                urls.push(format!("{p}{k}/{id}"));
            }
        }
    }

    let mut ops = Vec::new();
    for u in &urls {
        ops.push(json!(["openUrl", u]));
        ops.push(json!(["getPending"]));
    }
    // Last valid link wins; invalid ones leave it alone; reading clears it.
    for u in [
        "snowify://track/1",
        "bad",
        "snowify://album/2",
        "snowify://nope/3",
    ] {
        ops.push(json!(["openUrl", u]));
    }
    ops.push(json!(["getPending"]));
    ops.push(json!(["getPending"]));

    let argvs = [
        json!(["/usr/bin/snowify"]),
        json!(["x", "snowify://track/1", "snowify://album/2"]),
        json!(["snowify:track/1"]),
        json!(["--flag", "snowify://bad"]),
        json!(["snowify://artist/日本/x"]),
    ];
    let mut windows = vec![Value::Null];
    for web_contents in [true, false] {
        for minimized in [true, false] {
            for fail_at in [
                None,
                Some("webContents"),
                Some("send"),
                Some("isMinimized"),
                Some("restore"),
                Some("focus"),
            ] {
                windows.push(json!({ "webContents": web_contents, "minimized": minimized, "failAt": fail_at }));
            }
        }
    }
    for w in &windows {
        ops.push(json!(["window", w]));
        for u in [
            "snowify://track/abc",
            "snowify://bad/x",
            "snowify:album//a/b",
        ] {
            ops.push(json!(["openUrl", u]));
            ops.push(json!(["getPending"]));
        }
        for a in &argvs {
            ops.push(json!(["secondInstance", a]));
            ops.push(json!(["getPending"]));
        }
    }
    ops
}

fn link_json(l: Option<DeepLink>) -> Value {
    l.map_or(
        Value::Null,
        |l| json!({ "type": l.kind.as_str(), "id": l.id }),
    )
}

#[test]
fn deep_links_match_source() {
    let ops = corpus();
    let out = common::run_source("deep_link.js", &Value::Array(ops.clone()));

    let mut window: Option<MockWindow> = None;
    let mut pending = None;
    let mut results = Vec::new();
    for op in &ops {
        let take_calls = |w: &Option<MockWindow>| -> Value {
            json!(
                w.as_ref()
                    .map(|w| std::mem::take(&mut *w.calls.borrow_mut()))
                    .unwrap_or_default()
            )
        };
        let r = match op[0].as_str().unwrap() {
            "window" => {
                window = op[1].as_object().map(|w| MockWindow {
                    web_contents: w["webContents"].as_bool().unwrap(),
                    minimized: w["minimized"].as_bool().unwrap(),
                    fail_at: w["failAt"].as_str().map(str::to_owned),
                    calls: RefCell::default(),
                });
                Value::Null
            }
            "openUrl" => {
                let w = window.as_ref().map(|w| w as &dyn MainWindow);
                deep_link::handle(op[1].as_str().unwrap(), w, &mut pending);
                take_calls(&window)
            }
            "secondInstance" => {
                let argv: Vec<&str> = op[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap())
                    .collect();
                let w = window.as_ref().map(|w| w as &dyn MainWindow);
                let threw = deep_link::on_second_instance(&argv, w, &mut pending).is_err();
                let mut calls = take_calls(&window);
                if threw {
                    calls.as_array_mut().unwrap().push("<threw>".into());
                }
                calls
            }
            _ => link_json(deep_link::take_pending(&mut pending)),
        };
        results.push(r);
    }
    let logs: Vec<String> = logger::LOGS
        .lock()
        .unwrap()
        .entries()
        .into_iter()
        .map(|e| e.msg)
        .collect();

    let mut d = Diff::default();
    for ((op, s), r) in ops
        .iter()
        .zip(out["results"].as_array().unwrap())
        .zip(&results)
    {
        d.check(op, s, r);
    }
    d.check("log messages", &out["logs"], &json!(logs));
    let links = results.iter().filter(|r| r.is_object()).count();
    assert!(
        links > 50,
        "only {links} links parsed; corpus is not exercising the valid path"
    );
    d.finish("deep_link");
}
