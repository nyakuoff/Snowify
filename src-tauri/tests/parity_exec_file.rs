//! Differential test: `snowify::node::child_process::exec_file` vs Node 20's `execFile`, on
//! the same scripts: exit codes, signals, timeouts, external kills, maxBuffer, spawn errors
//! (thrown vs passed to the callback), PATH search, bad UTF-8 and odd argument values.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, net::JsErr, node::child_process};
use std::os::unix::fs::PermissionsExt;

mod common;
use common::Diff;

fn script(dir: &std::path::Path, name: &str, body: &str, mode: u32) -> String {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    p.display().to_string()
}

fn show(e: &JsErr) -> Value {
    json!({
        "string": format!("{}: {}", e.name, e.message.to_string_lossy()),
        "props": JsValue::Object(e.props.iter().map(|(k, v)| (k.as_str().into(), v.clone())).collect()).encode(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn exec_file_matches_node() {
    let dir = std::env::temp_dir().join(format!("snowify-parity-exec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("p1")).unwrap();
    std::fs::create_dir_all(dir.join("p2")).unwrap();
    std::fs::create_dir_all(dir.join("adir")).unwrap();
    let sh = |name: &str, body: &str| script(&dir, name, &format!("#!/bin/sh\n{body}\n"), 0o755);
    let echo = sh("echo", "printf '%s\\n' \"$@\"; echo err >&2");
    let fail = sh("fail", "echo out; printf 'bad thing\\nline2' >&2; exit 3");
    let warn = sh("warn", "echo ok; echo warning >&2");
    let term = sh("term", "echo before; kill -TERM $$; sleep 1");
    let kill9 = sh("kill9", "kill -9 $$; sleep 1");
    let usr1 = sh("usr1", "kill -USR1 $$; sleep 1");
    let rt = sh("rt", "kill -35 $$; sleep 1");
    let slow = sh("slow", "echo started; sleep 5");
    let trap = sh("trap", "trap 'echo trapped; exit 7' TERM; sleep 5 & wait");
    let orphan = sh("orphan", "(sleep 1; echo late) & echo early");
    let big = sh("big", "head -c 1100000 /dev/zero | tr '\\0' a");
    let big_err = sh("bigerr", "head -c 1100000 /dev/zero | tr '\\0' b >&2");
    let bytes = sh("bytes", "printf 'ok \\377\\376 \\303( \\342\\202 \\360\\237\\216 \\355\\240\\200 end'");
    let stdin = sh("stdin", "cat; echo done");
    let code = sh("code", "exit 300");
    let nosheb = script(&dir, "nosheb", "echo noshebang", 0o755);
    let noexec = script(&dir, "noexec", "#!/bin/sh\necho x", 0o644);
    let badelf = script(&dir, "badelf", "\x7fELFgarbage", 0o755);
    let badinterp = script(&dir, "badinterp", "#!/nonexistent/interp\n", 0o755);
    script(&dir, "p1/tool", "#!/bin/sh\necho p1", 0o644);
    script(&dir, "p2/tool", "#!/bin/sh\necho p2 \"$@\"", 0o755);
    let d = |p: &str| dir.join(p).display().to_string();
    let s = |v: &str| json!(v);
    let w = |v: Value| JsValue::from(&v).encode();
    let long_nul = format!("{}\0{}", "x".repeat(100), "y".repeat(100));
    // Inspected to ~150 characters: only the 128 cap truncates it.
    let mid_nul = format!("{}\0{}", "x".repeat(70), "y".repeat(70));

    let cases: Vec<Value> = vec![
        json!({ "file": echo, "args": [s("a b"), s("c'd\"e")], "timeout": 0 }),
        json!({ "file": echo, "args": [], "timeout": 0 }),
        json!({ "file": fail, "args": [s("x")], "timeout": 0 }),
        json!({ "file": warn, "args": [], "timeout": 0 }),
        json!({ "file": term, "args": [], "timeout": 0 }),
        json!({ "file": kill9, "args": [], "timeout": 0 }),
        json!({ "file": usr1, "args": [], "timeout": 0 }),
        json!({ "file": rt, "args": [], "timeout": 0 }),
        json!({ "file": slow, "args": [s("-U")], "timeout": 300 }),
        json!({ "file": trap, "args": [], "timeout": 300 }),
        json!({ "file": orphan, "args": [], "timeout": 0 }),
        json!({ "file": orphan, "args": [], "timeout": 400 }),
        json!({ "file": big, "args": [], "timeout": 0 }),
        json!({ "file": big_err, "args": [], "timeout": 0 }),
        json!({ "file": bytes, "args": [], "timeout": 0 }),
        json!({ "file": stdin, "args": [], "timeout": 300 }),
        json!({ "file": slow, "args": [], "timeout": 0, "killAfter": 200 }),
        json!({ "file": code, "args": [], "timeout": 0 }),
        json!({ "file": nosheb, "args": [], "timeout": 0 }),
        json!({ "file": noexec, "args": [s("a")], "timeout": 0 }),
        json!({ "file": d("adir"), "args": [], "timeout": 0 }),
        json!({ "file": d("missing"), "args": [w(json!(5)), w(json!(null))], "timeout": 0 }),
        json!({ "file": "tool", "args": [s("q")], "timeout": 0 }),
        json!({ "file": "nosuchtool", "args": [], "timeout": 0 }),
        json!({ "file": badelf, "args": [], "timeout": 0 }),
        json!({ "file": badinterp, "args": [], "timeout": 0 }),
        json!({ "file": format!("{noexec}/x"), "args": [], "timeout": 0 }),
        json!({ "file": "", "args": [], "timeout": 0 }),
        json!({ "file": format!("{echo}\0x"), "args": [], "timeout": 0 }),
        json!({ "file": echo, "args": [s("ok"), s("a\0b")], "timeout": 0 }),
        json!({ "file": echo, "args": [s(&long_nul)], "timeout": 0 }),
        json!({ "file": echo, "args": [s(&mid_nul)], "timeout": 0 }),
        json!({ "file": echo, "args": [s("it's \"q\"\0")], "timeout": 0 }),
        json!({ "file": echo, "args": [w(json!(5)), w(json!(null)), json!({ "$": "u" }), w(json!({ "a": 1 })), w(json!([1, [2, null]])), w(json!(true)), w(json!(["a\u{0}b"])), w(json!(-0.0)), w(json!(1.5e300))], "timeout": 0 }),
        json!({ "file": echo, "args": [{ "$": "s", "v": [0xD800, 0x61] }], "timeout": 0 }),
    ];
    let path_env = format!("{}:{}:/usr/bin:/bin", d("p1"), d("p2"));
    let src = common::run_source_with("exec_file.js", &json!({ "cases": cases }), |c| {
        c.env("PATH", &path_env);
    });
    // SAFETY: single-threaded test runtime; nothing else reads the environment concurrently.
    unsafe { std::env::set_var("PATH", &path_env) };

    let mut diff = Diff::default();
    for (i, c) in cases.iter().enumerate() {
        let args: Vec<JsValue> = c["args"].as_array().unwrap().iter().map(|a| JsValue::decode(a).unwrap()).collect();
        let rust = match child_process::exec_file(c["file"].as_str().unwrap(), &args, c["timeout"].as_u64().unwrap()) {
            Err(e) => json!({ "threw": show(&e) }),
            Ok((killer, fut)) => {
                if let Some(ms) = c["killAfter"].as_u64() {
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                        killer.kill();
                    });
                }
                let o = tokio::time::timeout(std::time::Duration::from_secs(20), fut)
                    .await
                    .unwrap_or_else(|_| panic!("case {i} {} never finished", c["file"]));
                json!({ "err": o.err.as_ref().map(show), "stdout": o.stdout, "stderr": o.stderr })
            }
        };
        diff.check(format_args!("case {i} {}", c["file"]), &src[i], &rust);
    }
    let _ = std::fs::remove_dir_all(&dir);
    diff.finish("exec_file");
}
