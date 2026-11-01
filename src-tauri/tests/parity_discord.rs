//! Differential test: `src/main/discord.js` + `@xhayper/discord-rpc` (unmodified) vs
//! `snowify::discord`, against the same scripted fake Discord IPC server. Compared: handler
//! results and every frame the server receives (nonces, pid and `created_at` normalised).

use serde_json::{Value, json};
use snowify::{discord::Discord, ipc::JsValue};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

mod common;
#[path = "common/host.rs"]
mod host;
use common::Diff;
use host::MockHost;

/// How the fake Discord behaves on each connection.
#[derive(Clone)]
struct Script {
    /// "user", "nouser", "nulldata", "coalesced" (READY + PING in one write), "none".
    ready: &'static str,
    /// Per request, in order (the last repeats): "ok", "error", "error-null", "none",
    /// "close", "close-op", "ping-ok", "evt-disconnected", "evt-ready-ok".
    replies: Vec<&'static str>,
    /// The first connection only: its READY mode and when (ms after accept) it hangs up.
    first: Option<(&'static str, u64)>,
}

fn frame(op: u32, v: &Value) -> Vec<u8> {
    let body = v.to_string().into_bytes();
    let mut out = op.to_le_bytes().to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

fn normalise(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        if o.contains_key("nonce") {
            o.insert("nonce".into(), json!("<nonce>"));
        }
        if let Some(args) = o.get_mut("args").and_then(Value::as_object_mut) {
            if args.contains_key("pid") {
                args.insert("pid".into(), json!("<pid>"));
            }
            if let Some(a) = args.get_mut("activity").and_then(Value::as_object_mut)
                && a.get("created_at").is_some_and(Value::is_number)
            {
                a.insert("created_at".into(), json!("<now>"));
            }
        }
    }
    v
}

async fn serve_conn(mut s: UnixStream, mut script: Script, log: Arc<Mutex<Vec<Value>>>, conn: usize) {
    let mut hang_up = None;
    if conn == 0
        && let Some((ready, ms)) = script.first
    {
        script.ready = ready;
        hang_up = Some(Box::pin(tokio::time::sleep(Duration::from_millis(ms))));
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 65536];
    let mut replies = script.replies.iter().copied();
    let mut last = "ok";
    let mut sent_ready = false;
    loop {
        // Pull whole frames out of the buffer.
        while buf.len() >= 8 {
            let op = u32::from_le_bytes(buf[0..4].try_into().unwrap());
            let len = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
            if buf.len() < 8 + len {
                break;
            }
            let body: Value = serde_json::from_slice(&buf[8..8 + len]).unwrap_or(Value::Null);
            buf.drain(..8 + len);
            log.lock().unwrap().push(json!([conn, op, normalise(body.clone())]));
            if op == 0 && !sent_ready {
                sent_ready = true;
                let ready = |user: Value| json!({ "cmd": "DISPATCH", "evt": "READY", "data": { "v": 1, "user": user, "config": { "cdn_host": "cdn.discordapp.com" } } });
                let out = match script.ready {
                    "user" => frame(1, &ready(json!({ "id": "1", "username": "u" }))),
                    "nouser" => frame(1, &ready(Value::Null)),
                    "nulldata" => frame(1, &json!({ "cmd": "DISPATCH", "evt": "READY", "data": null })),
                    "coalesced" => [frame(1, &ready(json!({ "id": "1" }))), frame(3, &json!({ "p": 1 }))].concat(),
                    _ => Vec::new(),
                };
                let _ = s.write_all(&out).await;
                continue;
            }
            if op != 1 {
                continue;
            }
            let nonce = body["nonce"].clone();
            let cmd = body["cmd"].clone();
            let action = replies.next().unwrap_or(last);
            last = action;
            let ok = frame(1, &json!({ "cmd": cmd, "data": {}, "evt": null, "nonce": nonce }));
            let out = match action {
                "ok" => ok,
                "error" => frame(1, &json!({ "cmd": cmd, "evt": "ERROR", "data": { "code": 4000, "message": "bad" }, "nonce": nonce })),
                "error-null" => frame(1, &json!({ "cmd": cmd, "evt": "ERROR", "data": null, "nonce": nonce })),
                "close" => return,
                "close-op" => frame(2, &json!({ "code": 1000, "message": "bye" })),
                "ping-ok" => {
                    let _ = s.write_all(&frame(3, &json!({ "ping": 7 }))).await;
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    ok
                }
                "evt-disconnected" => frame(1, &json!({ "cmd": "X", "evt": "disconnected", "data": {}, "nonce": nonce })),
                "evt-ready-ok" => {
                    let _ = s.write_all(&frame(1, &json!({ "cmd": "X", "evt": "ready", "data": {} }))).await;
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    ok
                }
                _ => Vec::new(),
            };
            let _ = s.write_all(&out).await;
        }
        let read = s.read(&mut chunk);
        let n = match &mut hang_up {
            Some(t) => tokio::select! {
                n = read => n,
                () = t => return,
            },
            None => read.await,
        };
        match n {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

/// Listens at `path` until dropped; returns the shared frame log.
fn serve(path: &Path, script: Script) -> (Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let _ = std::fs::remove_file(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(path).unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let l2 = log.clone();
    let task = tokio::spawn(async move {
        let mut n = 0;
        while let Ok((s, _)) = listener.accept().await {
            tokio::spawn(serve_conn(s, script.clone(), l2.clone(), n));
            n += 1;
        }
    });
    (log, task)
}

struct Scenario {
    name: &'static str,
    /// None: no Discord running.
    script: Option<Script>,
    ops: Vec<Value>,
    op_timeout_ms: u64,
}

fn op(ch: &str, args: Vec<Value>) -> Value {
    json!([ch, args.into_iter().map(|a| JsValue::from(&a).encode()).collect::<Vec<_>>()])
}

fn scenarios() -> Vec<Scenario> {
    let connect = || op("discord:connect", vec![]);
    let disconnect = || op("discord:disconnect", vec![]);
    let clear = || op("discord:clearPresence", vec![]);
    let update = |d: Value| op("discord:updatePresence", vec![d]);
    let full = json!({ "title": "Song", "artist": "Artist", "thumbnail": "https://i/x.jpg", "startTimestamp": 1_761_800_000_000_i64, "endTimestamp": 1_761_800_215_000.7, "videoId": "abc123" });
    let s = |ready, replies: Vec<&'static str>| Some(Script { ready, replies, first: None });
    vec![
        Scenario {
            name: "normal session",
            script: s("user", vec!["ok"]),
            ops: vec![
                update(full.clone()),
                connect(),
                update(full.clone()),
                update(json!({})),
                update(json!({ "title": 5, "artist": "", "thumbnail": { "a": [1] }, "startTimestamp": "2026-10-30T12:00:00.5Z", "endTimestamp": true, "videoId": 0 })),
                update(json!({ "title": "t", "startTimestamp": 9e15, "endTimestamp": "2026-10-30" })),
                update(json!({ "startTimestamp": { "$": "d", "v": 1_700_000_000_000_i64 }, "endTimestamp": "not a date" })),
                update(json!(null)),
                update(json!("str")),
                update(json!({ "thumbnail": { "$": "b", "v": "1" } })),
                clear(),
                connect(),
                disconnect(),
                // Renderer calls are separate macrotasks: the old socket's close lands first.
                json!(["$sleep", 200]),
                update(full.clone()),
                clear(),
                connect(),
                update(full.clone()),
            ],
            op_timeout_ms: 3000,
        },
        Scenario { name: "no discord", script: None, ops: vec![connect(), update(full.clone()), disconnect()], op_timeout_ms: 3000 },
        Scenario {
            name: "error replies",
            script: s("user", vec!["error", "error-null", "ok"]),
            ops: vec![connect(), update(full.clone()), update(full.clone()), clear()],
            op_timeout_ms: 1500,
        },
        Scenario {
            name: "ping, close opcode, reconnect",
            script: s("user", vec!["ping-ok", "close-op", "ok"]),
            ops: vec![connect(), update(full.clone()), update(full.clone()), update(full.clone()), connect(), clear()],
            op_timeout_ms: 3000,
        },
        Scenario {
            name: "server hangs up",
            script: s("user", vec!["close", "ok"]),
            ops: vec![connect(), update(full.clone()), update(full.clone()), connect(), update(full.clone())],
            op_timeout_ms: 3000,
        },
        Scenario {
            name: "event names from the server",
            script: s("user", vec!["evt-disconnected", "evt-ready-ok"]),
            ops: vec![connect(), update(full.clone()), update(full.clone()), connect(), update(full.clone()), update(full.clone())],
            op_timeout_ms: 3000,
        },
        Scenario { name: "ready without user", script: s("nouser", vec!["ok"]), ops: vec![connect(), update(full.clone()), clear()], op_timeout_ms: 3000 },
        Scenario { name: "ready with null data", script: s("nulldata", vec!["ok"]), ops: vec![connect(), update(full.clone())], op_timeout_ms: 12_000 },
        Scenario { name: "coalesced ready", script: s("coalesced", vec!["ok"]), ops: vec![connect(), connect()], op_timeout_ms: 12_000 },
        Scenario {
            name: "orphaned connection closes after a reconnect",
            script: Some(Script { ready: "user", replies: vec!["ok"], first: Some(("none", 11_000)) }),
            ops: vec![connect(), connect(), json!(["$sleep", 2000]), update(full.clone())],
            op_timeout_ms: 12_000,
        },
        Scenario {
            name: "disconnect while connecting",
            script: s("user", vec!["ok"]),
            ops: vec![json!(["$parallel", [connect(), disconnect()]]), update(full.clone()), connect(), update(full.clone())],
            op_timeout_ms: 3000,
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discord_matches_source() {
    let base: PathBuf = std::env::temp_dir().join(format!("snowify-parity-discord-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    // An earlier candidate that exists but isn't a socket: both sides must skip it.
    std::fs::write(base.join("discord-ipc-0"), "not a socket").unwrap();
    let sock = base.join("app/com.discordapp.Discord/discord-ipc-1");
    // SAFETY: set before any task reads the environment.
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", &base) };
    let repo_root = common::root().join("..").canonicalize().unwrap().display().to_string();
    let mut d = Diff::default();
    for s in scenarios() {
        let scenario = json!({ "module": "discord", "userData": "/mock/ud", "isPackaged": true, "dialogs": [], "http": {}, "opTimeoutMs": s.op_timeout_ms, "ops": s.ops });

        let server = s.script.clone().map(|sc| serve(&sock, sc));
        let src = tokio::task::spawn_blocking({
            let (scenario, base) = (scenario.clone(), base.clone());
            move || common::run_source_with("main_module.js", &scenario, |c| {
                c.env("XDG_RUNTIME_DIR", &base);
            })
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let src_frames = server.map(|(log, task)| {
            task.abort();
            let v = log.lock().unwrap().clone();
            v
        });
        let _ = std::fs::remove_file(&sock);

        let server = s.script.clone().map(|sc| serve(&sock, sc));
        let host = MockHost::new(&scenario, &repo_root);
        let discord = Arc::new(Discord::default());
        let dc = discord.clone();
        let dispatch = move |_h: &MockHost, ch: String, args: Vec<JsValue>| {
            let dc = dc.clone();
            Box::pin(async move {
                let a0 = args.first().cloned().unwrap_or_default();
                Some(match ch.as_str() {
                    "discord:connect" => dc.handle_connect().await,
                    "discord:disconnect" => dc.handle_disconnect().await,
                    "discord:updatePresence" => dc.handle_update(&a0).await,
                    "discord:clearPresence" => dc.handle_clear().await,
                    _ => return None,
                })
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = Option<snowify::ipc::HandlerResult>>>>
        };
        let rust = host::run_ops(&host, &scenario, &dispatch).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let rust_frames = server.map(|(log, task)| {
            task.abort();
            let v = log.lock().unwrap().clone();
            v
        });
        let _ = std::fs::remove_file(&sock);

        let (a, b) = (&src["results"], &rust["results"]);
        for i in 0..a.as_array().unwrap().len().max(b.as_array().unwrap().len()) {
            d.check(format_args!("{} results[{i}]", s.name), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
        }
        let (a, b) = (src_frames.unwrap_or_default(), rust_frames.unwrap_or_default());
        for i in 0..a.len().max(b.len()) {
            d.check(format_args!("{} frames[{i}]", s.name), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    d.finish("discord");
}
