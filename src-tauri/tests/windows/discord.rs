//! The unmodified discord-rpc client and Rust connect to the same fake named pipe.
#[path = "../common/mod.rs"]
mod common;
use common::Diff;
use serde_json::{Value, json};
use snowify::{discord::Discord, ipc::JsValue};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::ServerOptions,
};
fn normalise(mut v: Value) -> Value {
    if let Some(n) = v.get_mut("nonce") {
        *n = json!("<nonce>");
    }
    if let Some(n) = v.pointer_mut("/args/pid") {
        *n = json!("<pid>");
    }
    if let Some(n) = v.pointer_mut("/args/activity/created_at") {
        *n = json!("<now>");
    }
    v
}
async fn write_frame(s: &mut tokio::net::windows::named_pipe::NamedPipeServer, op: u32, v: Value) {
    let b = v.to_string().into_bytes();
    let mut frame = op.to_le_bytes().to_vec();
    frame.extend((b.len() as u32).to_le_bytes());
    frame.extend(b);
    s.write_all(&frame).await.unwrap();
}
fn serve() -> (Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let mut s = ServerOptions::new()
        .first_pipe_instance(true)
        .create(r"\\?\pipe\discord-ipc-9")
        .unwrap();
    let log = Arc::new(Mutex::new(vec![]));
    let frames = log.clone();
    let task = tokio::spawn(async move {
        s.connect().await.unwrap();
        let mut ready = false;
        loop {
            let mut head = [0u8; 8];
            if s.read_exact(&mut head).await.is_err() {
                break;
            }
            let op = u32::from_le_bytes(head[..4].try_into().unwrap());
            let n = u32::from_le_bytes(head[4..].try_into().unwrap());
            let mut data = vec![0; n as usize];
            s.read_exact(&mut data).await.unwrap();
            let v: Value = serde_json::from_slice(&data).unwrap();
            frames
                .lock()
                .unwrap()
                .push(json!([op, normalise(v.clone())]));
            if !ready {
                ready = true;
                write_frame(
                    &mut s,
                    1,
                    json!({"cmd":"DISPATCH","evt":"READY","data":{"user":{"id":"test"}}}),
                )
                .await;
            } else {
                write_frame(
                    &mut s,
                    1,
                    json!({"cmd":v["cmd"],"nonce":v["nonce"],"data":{}}),
                )
                .await;
            }
        }
    });
    (log, task)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discord_matches_windows_named_pipe() {
    let data = JsValue::object([
        ("title", "Windows pipe".into()),
        ("artist", "Artist".into()),
        ("videoId", "id".into()),
    ]);
    let ops = json!([
        ["discord:connect", []],
        ["discord:updatePresence", [data.encode()]],
        ["discord:clearPresence", []],
        ["discord:disconnect", []]
    ]);
    let scenario = json!({"module":"discord","userData":"C:\\mock\\ud","isPackaged":true,"dialogs":[],"http":{},"ops":ops,"opTimeoutMs":2000});
    let (source_log, source_task) = serve();
    let source =
        tokio::task::spawn_blocking(move || common::run_source("main_module.js", &scenario))
            .await
            .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), source_task)
        .await
        .unwrap()
        .unwrap();
    let (rust_log, rust_task) = serve();
    let client = Discord::default();
    let mut results = vec![];
    for ch in ["connect", "update", "clear", "disconnect"] {
        let result = match ch {
            "connect" => client.handle_connect().await,
            "update" => client.handle_update(&data).await,
            "clear" => client.handle_clear().await,
            _ => client.handle_disconnect().await,
        }
        .unwrap();
        results.push(json!({"ok":result.encode()}));
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), rust_task)
        .await
        .unwrap()
        .unwrap();
    let mut d = Diff::default();
    d.check(
        "named pipe handler results",
        &source["results"],
        &json!(results),
    );
    d.check(
        "named pipe protocol frames",
        &json!(*source_log.lock().unwrap()),
        &json!(*rust_log.lock().unwrap()),
    );
    d.finish("discord Windows named pipe");
}
