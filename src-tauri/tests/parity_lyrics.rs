//! Differential test: `src/main/lyrics.js` (global
//! `fetch` replayed from the scenario) vs `snowify::lyrics`, comparing results, log lines and
//! every request made (URL, headers as sent, redirect mode), in order.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, lyrics};
use std::sync::Arc;

mod common;
#[path = "common/host.rs"]
mod host;
use common::Diff;
use host::MockHost;

const LRC_GET: &str = "https://lrclib.net/api/get";

fn ok(v: Value) -> Value {
    json!({ "status": 200, "statusText": "OK", "headers": [["content-type", "application/json"]], "body": v.to_string() })
}
fn raw(status: u16, body: &str) -> Value {
    json!({ "status": status, "body": body })
}
fn w(v: Value) -> Value {
    JsValue::from(&v).encode()
}
fn get(args: [Value; 4]) -> Value {
    json!(["lyrics:get", args.into_iter().map(w).collect::<Vec<_>>()])
}

struct Scenario {
    name: &'static str,
    fetch: Vec<(&'static str, Vec<Value>)>,
    ops: Vec<Value>,
}

fn scenarios() -> Vec<Scenario> {
    let song = |t: &str, a: &str, al: &str, d: Value| get([json!(t), json!(a), json!(al), d]);
    vec![
        Scenario {
            name: "get hit, then normalized retries",
            fetch: vec![(LRC_GET, vec![raw(404, "{}"), raw(404, "{}"), ok(json!({ "syncedLyrics": null, "plainLyrics": "got it", "source": "x" }))])],
            ops: vec![song("Track - Remastered 2011", "A & B", "  Big   Album ", json!("181.6"))],
        },
        Scenario {
            name: "concurrency",
            fetch: vec![(LRC_GET, vec![ok(json!({ "syncedLyrics": "[00:01.00] get" }))])],
            ops: vec![
                json!(["$parallel", [song("One", "A", "", json!(1)), song("Two", "B", "", json!(2)), song("One", "A", "", json!(1.2))]]),
                song("Two", "B", "", json!(2)),
            ],
        },
        Scenario {
            name: "odd inputs",
            fetch: vec![(LRC_GET, vec![raw(404, "")])],
            ops: vec![
                get([json!(5), json!(true), json!(null), json!(3)]),
                get([json!({ "$": "b", "v": "5" }), json!("A"), json!(""), json!(1)]),
                get([json!("T"), json!("A"), json!(""), json!({ "$": "b", "v": "7" })]),
                get([json!({ "$": "s", "v": [0xD800, 0x61] }), json!("A"), json!(""), json!(1)]),
                get([json!(""), json!(""), json!(""), json!(0)]),
                song("Live Song - Live at Wembley", "A x B", "", json!(1)),
                song("Song (Remastered 2009) [feat. Y]", "A feat. B", "", json!(1)),
                song("Remaſtered - remaſtered", "ſ, b", "", json!(1)),
                song("Tab\tand\u{A0}nbsp\u{2028}x", " featuring Z", "", json!(-0.4)),
                get([json!([1, 2]), json!({ "a": 1 }), json!([]), json!("1e3")]),
            ],
        },
        Scenario {
            name: "malformed responses",
            fetch: vec![(LRC_GET, vec![ok(json!([1])), ok(json!("str")), raw(200, ""), json!({ "error": true }), ok(json!({ "lyrics": { "plain": { "lyrics": "nested", "source": "deep" } } }))])],
            ops: vec![song("S", "A", "", json!(2)), song("S", "A", "B", json!(3))],
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lyrics_match_source() {
    let repo_root = common::root().join("..").canonicalize().unwrap().display().to_string();
    let mut d = Diff::default();
    let all = scenarios();
    for s in all {
        let fetch: serde_json::Map<String, Value> = s.fetch.iter().map(|(k, v)| ((*k).to_owned(), json!(v))).collect();
        let scenario = json!({
            "module": "lyrics", "userData": "/mock/ud", "isPackaged": true, "dialogs": [], "http": {},
            "fetch": fetch, "opTimeoutMs": 30000, "ops": s.ops,
        });
        let src = common::run_source("main_module.js", &scenario);

        let host = Arc::new(MockHost::new(&scenario, &repo_root));
        let state = Arc::new(lyrics::Lyrics::default());
        let (h2, st2) = (host.clone(), state.clone());
        let dispatch = move |_h: &MockHost, ch: String, args: Vec<JsValue>| {
            let (h, st) = (h2.clone(), st2.clone());
            Box::pin(async move {
                assert_eq!(ch, "lyrics:get");
                let a = |i: usize| args.get(i).cloned().unwrap_or_default();
                Some(lyrics::handle(st, h, [a(0), a(1), a(2), a(3)]).await)
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = Option<snowify::ipc::HandlerResult>>>>
        };
        let rust = host::run_ops(&host, &scenario, &dispatch).await;

        for key in ["results", "logs", "fetchCalls"] {
            let (a, b) = (&src[key], &rust[key]);
            let n = a.as_array().map_or(0, Vec::len).max(b.as_array().map_or(0, Vec::len));
            for i in 0..n {
                d.check(format_args!("{} {key}[{i}]", s.name), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
            }
        }
    }
    d.finish("lyrics");
}
