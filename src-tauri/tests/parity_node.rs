//! Differential test: `snowify::node` vs Node 20.18 (Electron 33): `path`, `os.tmpdir`, the
//! errno map, and `existsSync` / `mkdirSync({recursive})` / `rmSync({recursive, force})`.

use serde_json::{Value, json};
use snowify::node;
use std::collections::HashMap;

mod common;
#[path = "common/tree.rs"]
mod tree;
use common::{Diff, Rng};
use tree::{Entry, dir, file, link, raw_file};

const SEGMENTS: [&str; 30] = [
    "",
    ".",
    "..",
    "...",
    "a",
    "b",
    "/",
    "\\",
    "//",
    "\\\\",
    "C:",
    "c:\\",
    "Z:/",
    "\\\\server\\share",
    "//server/share",
    "?",
    ":",
    "::",
    "a:",
    "日本",
    "🎵",
    "a/b",
    "a\\..\\b",
    "./",
    "../",
    "\\\\?\\C:",
    "\\\\.\\dev",
    "x/.",
    "a//b/",
    "/..",
];

fn random_path(rng: &mut Rng) -> String {
    (0..1 + rng.below(4))
        .map(|_| *rng.pick(&SEGMENTS))
        .collect()
}

struct FsCase {
    op: &'static str,
    path: &'static str,
    tree: Vec<Entry>,
    dest: Option<&'static str>,
}

fn fs_cases() -> Vec<FsCase> {
    let c = |op, path, tree| FsCase {
        op,
        path,
        tree,
        dest: None,
    };
    let cp = |path, dest, tree| FsCase {
        op: "copy",
        path,
        tree,
        dest: Some(dest),
    };
    vec![
        c("rm", "x", vec![]),
        c("rm", "x", vec![file("x")]),
        c("rm", "x", vec![dir("x")]),
        c(
            "rm",
            "x",
            vec![dir("x/a/b"), file("x/a/b/f"), file("x/a/g"), file("x/h")],
        ),
        c(
            "rm",
            "x",
            vec![
                dir("out"),
                file("out/keep"),
                dir("x"),
                link("x/l", "{root}/out"),
                link("x/lf", "{root}/out/keep"),
                link("x/broken", "{root}/nope"),
            ],
        ),
        c(
            "rm",
            "x",
            vec![dir("out"), file("out/keep"), link("x", "{root}/out")],
        ),
        c(
            "rm",
            "x",
            vec![
                dir("x/ro"),
                file("x/ro/f"),
                file("x/z"),
                dir("x/ro").mode(0o555),
            ],
        ),
        c(
            "rm",
            "x",
            vec![
                dir("x/locked"),
                file("x/locked/f"),
                dir("x/locked").mode(0o000),
            ],
        ),
        c(
            "rm",
            "p/x",
            vec![dir("p/x/a"), file("p/x/a/f"), dir("p").mode(0o555)],
        ),
        c(
            "rm",
            "x/",
            vec![dir("x/ro"), file("x/ro/f"), dir("x/ro").mode(0o555)],
        ),
        c(
            "rm",
            "x",
            vec![
                dir("x/ro"),
                raw_file(b"x/ro/\xff\xfe-bad"),
                dir("x/ro").mode(0o555),
            ],
        ),
        c(
            "rm",
            "x",
            vec![
                dir("x/b"),
                file("x/b/f"),
                dir("x/a"),
                file("x/a/f"),
                dir("x/_"),
                dir("x/B"),
                file("x/B/f"),
                dir("x/b").mode(0o555),
                dir("x/a").mode(0o555),
                dir("x/B").mode(0o555),
            ],
        ),
        c(
            "rm",
            "x",
            vec![
                dir("x/ä"),
                file("x/ä/f"),
                dir("x/z"),
                file("x/z/f"),
                dir("x/ä").mode(0o555),
                dir("x/z").mode(0o555),
            ],
        ),
        c("mkdir", "n/a/b", vec![]),
        c("mkdir", "d", vec![dir("d")]),
        c("mkdir", "f", vec![file("f")]),
        c("mkdir", "f/a", vec![file("f")]),
        c("mkdir", "f/a/b", vec![file("f")]),
        c("mkdir", "ro/a", vec![dir("ro").mode(0o555)]),
        c("mkdir", "ro/a/b", vec![dir("ro").mode(0o555)]),
        c("mkdir", "broken", vec![link("broken", "{root}/nowhere")]),
        c("mkdir", "ld", vec![dir("d"), link("ld", "{root}/d")]),
        c("mkdir", "lf", vec![file("f"), link("lf", "{root}/f")]),
        c("mkdir", "n/d/", vec![]),
        c("mkdir", "n//d", vec![]),
        c("mkdir", "locked/a", vec![dir("locked").mode(0o000)]),
        c("exists", "f", vec![file("f")]),
        c("exists", "d", vec![dir("d")]),
        c("exists", "missing", vec![]),
        c("exists", "broken", vec![link("broken", "{root}/nowhere")]),
        c("exists", "lf", vec![file("f"), link("lf", "{root}/f")]),
        c("exists", "f/x", vec![file("f")]),
        c(
            "exists",
            "locked/x",
            vec![dir("locked"), file("locked/x"), dir("locked").mode(0o000)],
        ),
        c("rmNoForce", "missing", vec![]),
        c("rmNoForce", "x", vec![dir("x/a"), file("x/a/f")]),
        c("read", "f", vec![file("f")]),
        c("read", "missing", vec![]),
        c("read", "d", vec![dir("d")]),
        c("read", "locked", vec![file("locked").mode(0o000)]),
        c("read", "bad", vec![raw_file(b"bad")]),
        c("write", "new", vec![]),
        c("write", "f", vec![file("f")]),
        c("write", "d", vec![dir("d")]),
        c("write", "nodir/f", vec![]),
        c("write", "ro/f", vec![dir("ro").mode(0o555)]),
        c("unlink", "f", vec![file("f")]),
        c("unlink", "missing", vec![]),
        c("unlink", "d", vec![dir("d")]),
        c(
            "unlink",
            "ro/f",
            vec![dir("ro"), file("ro/f"), dir("ro").mode(0o555)],
        ),
        c(
            "readdir",
            "d",
            vec![
                dir("d/sub"),
                file("d/b"),
                file("d/A"),
                file("d/ä"),
                file("d/_"),
                raw_file(b"d/\xff"),
            ],
        ),
        c("readdir", "missing", vec![]),
        c("readdir", "f", vec![file("f")]),
        c("isDir", "d", vec![dir("d")]),
        c("isDir", "f", vec![file("f")]),
        c("isDir", "missing", vec![]),
        c("isDir", "ld", vec![dir("d"), link("ld", "{root}/d")]),
        cp("f", "g", vec![file("f")]),
        cp("f", "f", vec![file("f")]),
        cp("missing", "g", vec![]),
        cp("d", "g", vec![dir("d")]),
        cp("f", "nodir/g", vec![file("f")]),
        cp("f", "d", vec![file("f"), dir("d")]),
        cp("exec", "g", vec![file("exec").mode(0o751)]),
        cp("f", "ro/g", vec![file("f"), dir("ro").mode(0o555)]),
    ]
}

#[test]
fn node_runtime_matches_electron() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let mut path_cases: Vec<(String, Vec<String>)> = Vec::new();
    for f in [
        "posix.join",
        "win32.join",
        "posix.normalize",
        "win32.normalize",
        "posix.basename",
        "posix.dirname",
        "posix.extname",
        "posix.resolve",
        "posix.basenameExt",
    ] {
        for _ in 0..4000 {
            let n = if f.ends_with("join") || f.ends_with("resolve") {
                rng.below(4)
            } else {
                1
            };
            let mut args: Vec<String> = (0..n).map(|_| random_path(&mut rng)).collect();
            if f == "posix.basenameExt" {
                args.push(rng.pick(&[".csv", ".mp3", "x", "a", ".", "..", "/", "ab", "b.css", ""]).to_string());
            }
            path_cases.push((f.into(), args));
        }
    }

    const VALUES: [&str; 10] = [
        "", "/", "/x", "/x/", "/x//", "rel", "rel/", "🎵/", "a", "//",
    ];
    let mut tmp_cases: Vec<HashMap<&str, &str>> = vec![HashMap::new()];
    for _ in 0..300 {
        let mut env = HashMap::new();
        for k in ["TMPDIR", "TMP", "TEMP"] {
            if rng.below(3) > 0 {
                env.insert(k, *rng.pick(&VALUES));
            }
        }
        tmp_cases.push(env);
    }

    let base = std::env::temp_dir().join(format!("snowify-parity-node-{}", std::process::id()));
    let cases = fs_cases();
    let roots: Vec<_> = (0..cases.len())
        .map(|i| base.join(format!("case{i}")))
        .collect();
    let fs_input: Vec<Value> = cases
        .iter()
        .zip(&roots)
        .map(|(c, r)| {
            json!([
                c.op,
                format!("{}/{}", r.display(), c.path),
                c.dest.map(|d| format!("{}/{d}", r.display()))
            ])
        })
        .collect();

    for (c, r) in cases.iter().zip(&roots) {
        tree::build(r, &c.tree);
    }
    let cwd = std::path::Path::new("/tmp/cwd x");
    std::fs::create_dir_all(cwd).unwrap();
    let out = common::run_source_with(
        "node.js",
        &json!({
            "path": path_cases.iter().map(|(f, a)| json!([f, a])).collect::<Vec<_>>(),
            "tmpdir": tmp_cases,
            "fs": fs_input,
        }),
        |cmd| {
            cmd.current_dir(cwd);
        },
    );
    let source_trees: Vec<_> = roots.iter().map(|r| tree::snapshot(r)).collect();

    for (c, r) in cases.iter().zip(&roots) {
        tree::build(r, &c.tree);
    }
    let rust_fs: Vec<Value> = cases
        .iter()
        .zip(&roots)
        .map(|(c, r)| {
            let p = format!("{}/{}", r.display(), c.path);
            let dest = c
                .dest
                .map(|d| format!("{}/{d}", r.display()))
                .unwrap_or_default();
            let res: Result<Value, node::FsError> = match c.op {
                "exists" => Ok(json!(node::exists_sync(&p))),
                "mkdir" => node::mkdir_recursive_sync(&p).map(|()| json!(true)),
                "rm" => node::rm_recursive_force_sync(&p).map(|()| json!(true)),
                "rmNoForce" => node::rm_recursive_sync(&p).map(|()| json!(true)),
                "read" => node::read_file_utf8(&p).map(|s| json!(s)),
                "write" => node::write_file(&p, "written ✓".as_bytes()).map(|()| json!(true)),
                "copy" => node::copy_file_sync(&p, &dest).map(|()| json!(true)),
                "unlink" => node::unlink_sync(&p).map(|()| json!(true)),
                "readdir" => node::readdir_sync(&p).map(|n| json!(n)),
                "isDir" => node::stat_is_dir(&p).map(|d| json!(d)),
                _ => unreachable!(),
            };
            match res {
                Ok(v) => json!({ "ok": v }),
                Err(e) => json!({ "err": [e.code, e.message, e.to_json().to_string()] }),
            }
        })
        .collect();
    let rust_trees: Vec<_> = roots.iter().map(|r| tree::snapshot(r)).collect();
    tree::remove(&base);

    let mut d = Diff::default();
    for ((f, args), s) in path_cases.iter().zip(out["path"].as_array().unwrap()) {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let r = match f.as_str() {
            "posix.join" => node::path::posix::join(&a),
            "win32.join" => node::path::win32::join(&a),
            "posix.normalize" => node::path::posix::normalize(a[0]),
            "posix.basename" => node::path::posix_more::basename(a[0]),
            "posix.basenameExt" => node::path::posix_more::basename_ext(a[0], a[1]),
            "posix.dirname" => node::path::posix_more::dirname(a[0]),
            "posix.extname" => node::path::posix_more::extname(a[0]),
            "posix.resolve" => node::path::posix_more::resolve(&a, "/tmp/cwd x"),
            _ => node::path::win32::normalize(a[0]),
        };
        d.check(format_args!("{f}{args:?}"), s, &Value::from(r));
    }
    for (env, s) in tmp_cases.iter().zip(out["tmpdir"].as_array().unwrap()) {
        let r = node::tmpdir_from(false, |k| env.get(k).map(|v| v.to_string()));
        d.check(format_args!("tmpdir {env:?}"), s, &Value::from(r));
    }
    let table: Vec<Value> = snowify::uv_errors::UV_ERRORS
        .iter()
        .map(|(n, c, m)| json!([n, c, m]))
        .collect();
    d.check("errno table", &out["errors"], &Value::from(table));
    for (i, c) in cases.iter().enumerate() {
        let what = format_args!("{} {}", c.op, c.path).to_string();
        d.check(&what, &out["fs"][i], &rust_fs[i]);
        d.check(
            format_args!("{what} tree"),
            &json!(source_trees[i]),
            &json!(rust_trees[i]),
        );
    }
    // Pin that the fs cases really reach the error paths (not just "ok" everywhere).
    let errors = out["fs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.get("err").is_some())
        .count();
    assert!(
        errors >= 10,
        "only {errors} fs cases errored in the source; fixtures are not exercising error paths"
    );
    d.finish("node");
}
