//! Differential test: `src/main/audio-cache.js` vs `snowify::audio_cache`, on real directory
//! trees: creation, cleanup, and the error/warning paths.

use serde_json::{Value, json};
use snowify::{audio_cache, logger, node};
use std::collections::HashMap;

mod common;
#[path = "common/tree.rs"]
mod tree;
use common::Diff;
use tree::{Entry, dir, file, link};

const C: &str = "snowify-audio-cache";

struct Case {
    env: Vec<(&'static str, String)>,
    tree: Vec<Entry>,
    ops: &'static [&'static str],
}

#[test]
fn audio_cache_matches_source() {
    let base = std::env::temp_dir().join(format!("snowify-parity-audio-{}", std::process::id()));
    let r = |i: usize| base.join(format!("case{i}")).display().to_string();
    let ab = |i: usize, rel: &str| format!("{}/{rel}", r(i));
    let cases: Vec<Case> = vec![
        Case {
            env: vec![("TMPDIR", ab(0, "t"))],
            tree: vec![dir("t")],
            ops: &["ensure", "ensure", "cleanup", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(1, "t"))],
            tree: vec![
                dir("t/snowify-audio-cache/a/b"),
                file("t/snowify-audio-cache/a/b/f.webm"),
                file("t/snowify-audio-cache/x.m4a"),
            ],
            ops: &["cleanup", "ensure"],
        },
        Case {
            env: vec![("TMPDIR", ab(2, "t"))],
            tree: vec![dir("t"), file("t/snowify-audio-cache")],
            ops: &["ensure", "cleanup", "ensure"],
        },
        Case {
            env: vec![("TMPDIR", ab(3, "t"))],
            tree: vec![dir("t"), link("t/snowify-audio-cache", "{root}/t/nowhere")],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(4, "a/b/c"))],
            tree: vec![],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(5, "f/x"))],
            tree: vec![file("f")],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(6, "ro"))],
            tree: vec![dir("ro").mode(0o555)],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(7, "t"))],
            tree: vec![
                dir("t/snowify-audio-cache/ro"),
                file("t/snowify-audio-cache/ro/f"),
                file("t/snowify-audio-cache/z"),
                dir("t/snowify-audio-cache/ro").mode(0o555),
            ],
            ops: &["cleanup", "ensure"],
        },
        Case {
            env: vec![("TMPDIR", ab(8, "t/"))],
            tree: vec![dir("t")],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", String::new()), ("TMP", ab(9, "t2"))],
            tree: vec![dir("t2")],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", "rel".into())],
            tree: vec![dir("rel")],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(11, "日本"))],
            tree: vec![dir("日本")],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(12, "t"))],
            tree: vec![
                dir("t"),
                dir("elsewhere"),
                file("elsewhere/keep"),
                link("t/snowify-audio-cache", "{root}/elsewhere"),
            ],
            ops: &["ensure", "cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(13, "t"))],
            tree: vec![
                dir("t/snowify-audio-cache/locked"),
                file("t/snowify-audio-cache/locked/f"),
                dir("t/snowify-audio-cache/locked").mode(0o000),
            ],
            ops: &["cleanup"],
        },
        Case {
            env: vec![("TMPDIR", ab(14, "t/../t2/."))],
            tree: vec![dir("t"), dir("t2")],
            ops: &["ensure", "cleanup"],
        },
    ];

    let mut d = Diff::default();
    let mut errors = 0;
    for (i, case) in cases.iter().enumerate() {
        let root = base.join(format!("case{i}"));
        let ops = json!(case.ops);

        tree::build(&root, &case.tree);
        let src = common::run_source_with("audio_cache.js", &ops, |cmd| {
            cmd.current_dir(&root)
                .env_remove("TMPDIR")
                .env_remove("TMP")
                .env_remove("TEMP");
            for (k, v) in &case.env {
                cmd.env(k, v);
            }
        });
        let src_tree = tree::snapshot(&root);

        tree::build(&root, &case.tree);
        std::env::set_current_dir(&root).unwrap();
        let env: HashMap<&str, &str> = case.env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let dir = node::path::join(&[
            &node::tmpdir_from(false, |k| env.get(k).map(|v| v.to_string())),
            C,
        ]);
        let logs_before = logger::LOGS.lock().unwrap().entries().len();
        let results: Vec<Value> = case
            .ops
            .iter()
            .map(|op| match *op {
                "cleanup" => {
                    audio_cache::cleanup_cache_dir(&dir);
                    Value::Null
                }
                _ => match audio_cache::ensure_cache_dir(&dir) {
                    Ok(p) => json!({ "ok": p }),
                    Err(e) => json!({ "err": [e.code, e.message] }),
                },
            })
            .collect();
        let logs: Vec<Value> = logger::LOGS.lock().unwrap().entries()[logs_before..]
            .iter()
            .map(|e| json!([e.level.encode(), e.msg]))
            .collect();
        std::env::set_current_dir(&base).unwrap();
        let rust_tree = tree::snapshot(&root);

        errors += src["results"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r.get("err").is_some())
            .count()
            + src["logs"].as_array().unwrap().len();
        let what = format!("case{i} {:?}", case.env);
        d.check(format_args!("{what} dir"), &src["dir"], &json!(dir));
        d.check(
            format_args!("{what} results"),
            &src["results"],
            &json!(results),
        );
        d.check(format_args!("{what} logs"), &src["logs"], &json!(logs));
        d.check(
            format_args!("{what} tree"),
            &json!(src_tree),
            &json!(rust_tree),
        );
    }
    tree::remove(&base);
    assert!(
        errors >= 4,
        "only {errors} error/warning outcomes in the source; fixtures miss the error paths"
    );
    d.finish("audio_cache");
}
