//! Real Windows executables: libuv's search, raw quoting, exit DWORDs and termination.
#[path = "../common/mod.rs"]
mod common;
use common::Diff;
use serde_json::{Value, json};
use snowify::{ipc::JsValue, net::JsErr, node::child_process};
fn show(e: &JsErr) -> Value {
    json!({"string": format!("{}: {}", e.name, e.message.to_string_lossy()), "props": JsValue::Object(e.props.iter().map(|(k,v)| (k.as_str().into(),v.clone())).collect()).encode()})
}
#[tokio::test(flavor = "current_thread")]
async fn exec_file_matches_windows_electron() {
    let fixture = std::env::var_os("SNOWIFY_PARITY_PROCESS_FIXTURE")
        .expect("set SNOWIFY_PARITY_PROCESS_FIXTURE (see parity/windows/run.sh)");
    let base = std::env::temp_dir().join(format!("snowify-win-exec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    for dir in ["one", "two", "quoted;path", "cwd"] {
        std::fs::create_dir_all(base.join(dir)).unwrap();
    }
    for name in [
        "echo.exe",
        "one/tool.com",
        "one/tool.exe",
        "two/tool.exe",
        "quoted;path/quoted.exe",
        "cwd/local.exe",
        "onlyext",
        "dot..exe",
        "trailing.exe",
        "other.xyz",
        "two/bad.exe",
    ] {
        std::fs::copy(&fixture, base.join(name)).unwrap();
    }
    std::fs::write(base.join("one/bad.exe"), "invalid executable").unwrap();
    std::fs::write(base.join("batch.cmd"), "@echo unexpected").unwrap();
    std::fs::write(base.join("one/ps.ps1"), "unexpected").unwrap();
    std::fs::create_dir_all(base.join("directory.exe")).unwrap();
    let p = |s: &str| base.join(s).display().to_string();
    let s = |s: &str| JsValue::from(s).encode();
    let c = |file: String, args: Vec<Value>| json!({"file":file,"args":args,"timeout":0});
    let mut cases = vec![
        c(p("echo.exe"), vec![]),
        c(
            p("echo.exe"),
            [
                "", "plain", "a b", "a\tb", "a\"b", "a\\\"b", "a b\\", "\\", "\\\\", "a\nline",
                "é🎵", "it's q",
            ]
            .into_iter()
            .map(s)
            .collect(),
        ),
        c(
            p("echo.exe"),
            vec![
                JsValue::Number(5.0).encode(),
                JsValue::Null.encode(),
                JsValue::Undefined.encode(),
                JsValue::Array(vec!["x".into(), JsValue::Null]).encode(),
                JsValue::object([("x", 1.0.into())]).encode(),
            ],
        ),
        c(
            p("echo.exe"),
            vec![JsValue::String(snowify::ipc::JsString(vec![0xD800, 0x61])).encode()],
        ),
        c(p("echo.exe"), vec![s("stderr")]),
        c(p("echo.exe"), vec![s("binary")]),
        c(p("missing"), vec![s("arg")]),
        c(p("directory.exe"), vec![]),
        c("tool".into(), vec![s("search")]),
        c("tool.exe".into(), vec![]),
        c("quoted".into(), vec![]),
        c("local".into(), vec![]),
        c(p("onlyext"), vec![]),
        c(p("other.xyz"), vec![]),
        c(p("dot."), vec![]),
        c(p("trailing."), vec![]),
        c("bad".into(), vec![]),
        c(p("batch.cmd"), vec![]),
        c(p("missing.CMD . "), vec![]),
        c(p("missing.Bat"), vec![]),
        c("ps".into(), vec![]),
        c(".".into(), vec![]),
        c("".into(), vec![]),
        c(format!("{}\0x", p("echo.exe")), vec![]),
        c(p("echo.exe"), vec![s("a\0b")]),
    ];
    for code in [
        0u32, 1, 2, 255, 256, 65535, 2147483648, 3221225477, 4294967295,
    ] {
        cases.push(c(p("echo.exe"), vec![s("exit"), s(&code.to_string())]));
    }
    cases.push(json!({"file":p("echo.exe"),"args":[s("wait")],"timeout":300}));
    cases.push(json!({"file":p("echo.exe"),"args":[s("wait")],"timeout":0,"killAfter":200}));
    cases.push(c(p("echo.exe"), vec![s("big")]));
    cases.push(c(p("echo.exe"), vec![s("orphan")]));
    cases.push(json!({"file":p("echo.exe"),"args":[s("orphan")],"timeout":300}));
    let path = format!("{};\"{}\";{}", p("one"), p("quoted;path"), p("two"));
    let source = common::run_source_with("exec_file.js", &json!({"cases":cases}), |cmd| {
        cmd.env("PATH", &path)
            .env("PATHEXT", ".PS1;.CMD")
            .current_dir(base.join("cwd"));
    });
    let old_path = std::env::var_os("PATH");
    let old_cwd = std::env::current_dir().unwrap();
    unsafe {
        std::env::set_var("PATH", &path);
        std::env::set_var("PATHEXT", ".PS1;.CMD");
    }
    std::env::set_current_dir(base.join("cwd")).unwrap();
    let mut diff = Diff::default();
    for (i, c) in cases.iter().enumerate() {
        let args = c["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| JsValue::decode(a).unwrap())
            .collect::<Vec<_>>();
        let rust = match child_process::exec_file(
            c["file"].as_str().unwrap(),
            &args,
            c["timeout"].as_u64().unwrap(),
        ) {
            Err(e) => json!({"threw":show(&e)}),
            Ok((killer, fut)) => {
                if let Some(ms) = c["killAfter"].as_u64() {
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                        killer.kill();
                    });
                }
                let o = tokio::time::timeout(std::time::Duration::from_secs(20), fut)
                    .await
                    .expect("execFile callback timed out");
                json!({"err":o.err.as_ref().map(show),"stdout":o.stdout,"stderr":o.stderr})
            }
        };
        diff.check(
            format!("Windows execFile case {i}: {}", c["file"]),
            &source[i],
            &rust,
        );
    }
    std::env::set_current_dir(old_cwd).unwrap();
    unsafe {
        if let Some(path) = old_path {
            std::env::set_var("PATH", path);
        }
    }
    let _ = std::fs::remove_dir_all(base);
    diff.finish("exec_file Windows");
}
