//! Windows filesystem errors are compared as complete Node error objects, plus UV's table.
#[path = "../common/mod.rs"]
mod common;
use common::Diff;
use serde_json::{Value, json};
use snowify::node;
use std::path::Path;
fn build(root: &Path) {
    let _ = node::rm_recursive_force_sync(&root.to_string_lossy());
    std::fs::create_dir_all(root.join("dir/sub")).unwrap();
    std::fs::create_dir_all(root.join("names")).unwrap();
    for name in ["z", "B", "a", "_", "ä", "🎵", "日本"] {
        std::fs::write(root.join("names").join(name), "x").unwrap();
    }
    std::fs::write(root.join("file"), "data").unwrap();
    std::fs::write(root.join("ro"), "read only").unwrap();
    node::chmod_sync(&root.join("ro").to_string_lossy(), 0o444).unwrap();
    std::fs::write(root.join("dir/sub/ro"), "nested").unwrap();
    node::chmod_sync(&root.join("dir/sub/ro").to_string_lossy(), 0o444).unwrap();
}
fn error_or(v: Result<Value, node::FsError>) -> Value {
    match v {
        Ok(v) => json!({"ok":v}),
        Err(e) => json!({"err":[e.code,e.message,e.to_json().to_string()]}),
    }
}
#[test]
fn node_matches_windows_electron() {
    let root = std::env::temp_dir().join(format!("snowify-win-fs-{}", std::process::id()));
    let p = |s: &str| root.join(s).display().to_string();
    let mut fs = vec![
        json!(["read", p("missing")]),
        json!(["read", p("dir")]),
        json!(["write", p("dir")]),
        json!(["write", p("ro")]),
        json!(["read", p("file/child")]),
        json!(["read", p("file")]),
        json!(["mkdir", p("file")]),
        json!(["mkdir", p("file/child")]),
        json!(["mkdir", p("new/a/b")]),
        json!(["mkdir", p("dir")]),
        json!(["mkdir", p("bad<name")]),
        json!(["readdir", p("file")]),
        json!(["readdir", p("missing")]),
        json!(["readdir", p("names")]),
        json!(["readdir", p("file/child")]),
        json!(["isDir", p("file")]),
        json!(["isDir", p("dir")]),
        json!(["isDir", p("missing")]),
        json!(["unlink", p("missing")]),
        json!(["unlink", p("dir")]),
        json!(["copy", p("missing"), p("out")]),
        json!(["copy", p("dir"), p("out")]),
        json!(["copy", p("file"), p("dir")]),
        json!(["copy", p("file"), p("file")]),
        json!(["copy", p("file"), p("out")]),
        json!(["rmNoForce", p("missing")]),
        json!(["exists", p("missing")]),
        json!(["exists", p("file")]),
        json!(["chmod", p("missing"), 0o755]),
    ];
    for mode in [
        0u32, 0o111, 0o222, 0o444, 0o644, 0o755, 0o777, 0o002, 0o020, 0o200, 0o4000,
    ] {
        fs.push(json!(["chmod", p("file"), mode]));
        fs.push(json!(["chmod", p("dir"), mode]));
    }
    fs.extend([
        json!(["unlink", p("ro")]),
        json!(["rm", p("dir")]),
        json!(["rm", p("missing")]),
    ]);
    build(&root);
    let out = common::run_source("node.js", &json!({"path":[],"tmpdir":[],"fs":fs}));
    build(&root);
    let mut d = Diff::default();
    let table = snowify::uv_errors::UV_ERRORS
        .iter()
        .map(|(n, c, m)| json!([n, c, m]))
        .collect::<Vec<_>>();
    d.check(
        "Windows UV_ERRORS (including EAI, EOF, UNKNOWN)",
        &out["errors"],
        &json!(table),
    );
    for (i, c) in fs.iter().enumerate() {
        let p = c[1].as_str().unwrap();
        let res = match c[0].as_str().unwrap() {
            "read" => node::read_file_utf8(p).map(Value::from),
            "write" => node::write_file(p, "written ✓".as_bytes()).map(|()| json!(true)),
            "mkdir" => node::mkdir_recursive_sync(p).map(|()| json!(true)),
            "unlink" => node::unlink_sync(p).map(|()| json!(true)),
            "copy" => node::copy_file_sync(p, c[2].as_str().unwrap()).map(|()| json!(true)),
            "isDir" => node::stat_is_dir(p).map(Value::from),
            "readdir" => node::readdir_sync(p).map(|v| json!(v)),
            "rm" => node::rm_recursive_force_sync(p).map(|()| json!(true)),
            "rmNoForce" => node::rm_recursive_sync(p).map(|()| json!(true)),
            "exists" => Ok(json!(node::exists_sync(p))),
            "chmod" => node::chmod_sync(p, c[2].as_u64().unwrap() as u32).map(|()| {
                let m = std::fs::metadata(p).unwrap();
                let mode = if m.permissions().readonly() {
                    0o444
                } else {
                    0o666
                };
                // libuv's Windows stat never sets execute bits, directories included.
                json!(mode)
            }),
            _ => unreachable!(),
        };
        let mut expected = out["fs"][i].clone();
        // Object key ordering is outside the parity contract. JSON.stringify(err) is
        // otherwise compared as an object so errno/code/syscall/path/dest all stay pinned.
        let mut actual = error_or(res);
        for value in [&mut expected, &mut actual] {
            if let Some(e) = value.get_mut("err").and_then(Value::as_array_mut) {
                e[2] = serde_json::from_str(e[2].as_str().unwrap()).unwrap();
            }
        }
        d.check(format!("Windows fs {c}"), &expected, &actual);
    }
    let _ = node::rm_recursive_force_sync(&root.to_string_lossy());
    d.finish("node Windows (uv errors + chmod + fs)");
}

#[test]
fn win32_error_translation_matches_pinned_libuv() {
    // Native errors exercise mapping separately from already-translated UV errors.
    for (raw, code, errno) in [
        (2, "ENOENT", -4058),
        (3, "ENOENT", -4058),
        (5, "EPERM", -4048),
        (32, "EBUSY", -4082),
        (80, "EEXIST", -4075),
        (87, "EINVAL", -4071),
        (145, "ENOTEMPTY", -4051),
        (206, "ENAMETOOLONG", -4064),
        (10013, "EACCES", -4092),
        (10061, "ECONNREFUSED", -4078),
        (109, "EOF", -4095),
        (123456, "UNKNOWN", -4094),
    ] {
        let err = node::FsError::uv_nopath(&std::io::Error::from_raw_os_error(raw), "test");
        assert_eq!((err.code, err.errno), (code, errno));
        assert_eq!(snowify::uv_errors::translate_sys_error(errno), errno);
    }
}
