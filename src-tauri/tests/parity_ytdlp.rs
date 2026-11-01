//! Differential test: `src/main/ytdlp.js` (working-tree version, unmodified) vs
//! `snowify::ytdlp`, scenario by scenario on identical trees with fake yt-dlp scripts.

use serde_json::{Value, json};
use snowify::ytdlp::{self, Env};

mod common;
#[path = "common/tree.rs"]
mod tree;
use common::{Diff, root};
use tree::{Entry, dir, file};

struct Scenario {
    name: &'static str,
    platform: &'static str,
    tree: Vec<Entry>,
    /// (path, body, mode)
    files: Vec<(&'static str, &'static str, u32)>,
    ops: Vec<&'static str>,
}

const PRINTS: &str = "#!/bin/sh\necho 'Updating to stable'\nprintf 'args: %s\\n\\n \\r\\n' \"$*\"\n";
const FAILS: &str = "#!/bin/sh\necho partial\necho 'ERROR: unable to write' >&2\nexit 2\n";
const SILENT: &str = "#!/bin/sh\n";

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "linux bundled copy and update",
            platform: "linux",
            tree: vec![dir("ud")],
            files: vec![("res/bin/linux/yt-dlp", PRINTS, 0o644)],
            ops: vec!["path", "update", "path", "update"],
        },
        Scenario { name: "linux nothing bundled", platform: "linux", tree: vec![dir("ud")], files: vec![], ops: vec!["path", "update", "path"] },
        Scenario {
            name: "linux userData copy fails",
            platform: "linux",
            tree: vec![],
            files: vec![("ud/bin/yt-dlp", FAILS, 0o755), ("res/bin/linux/yt-dlp", PRINTS, 0o755)],
            ops: vec!["path", "update"],
        },
        Scenario {
            name: "linux silent update",
            platform: "linux",
            tree: vec![],
            files: vec![("ud/bin/yt-dlp", SILENT, 0o755)],
            ops: vec!["update"],
        },
        Scenario {
            name: "linux userData copy not executable",
            platform: "linux",
            tree: vec![],
            files: vec![("ud/bin/yt-dlp", PRINTS, 0o644)],
            ops: vec!["update", "path"],
        },
        Scenario {
            name: "linux bundled is a directory",
            platform: "linux",
            tree: vec![dir("res/bin/linux/yt-dlp"), dir("ud")],
            files: vec![],
            ops: vec!["path", "update", "path"],
        },
        Scenario {
            name: "linux userData is a file",
            platform: "linux",
            tree: vec![file("ud")],
            files: vec![("res/bin/linux/yt-dlp", PRINTS, 0o755)],
            ops: vec!["update", "path"],
        },
        Scenario {
            name: "linux read-only userData bin",
            platform: "linux",
            tree: vec![dir("ud/bin").mode(0o555)],
            files: vec![("res/bin/linux/yt-dlp", PRINTS, 0o755)],
            ops: vec!["update"],
        },
        Scenario {
            name: "win32 dev binary",
            platform: "win32",
            tree: vec![dir("ud")],
            files: vec![],
            ops: vec!["path"],
        },
        Scenario {
            name: "win32 bundled",
            platform: "win32",
            tree: vec![dir("ud")],
            files: vec![("res/bin/win/yt-dlp.exe", PRINTS, 0o755)],
            ops: vec!["path", "update", "path"],
        },
    ]
}

fn build(root: &std::path::Path, s: &Scenario) {
    use std::os::unix::fs::PermissionsExt;
    tree::build(root, &s.tree);
    for (p, body, mode) in &s.files {
        let full = root.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, body).unwrap();
        std::fs::set_permissions(&full, std::fs::Permissions::from_mode(*mode)).unwrap();
    }
    tree::build_modes(root, &s.tree);
}

#[tokio::test(flavor = "current_thread")]
async fn ytdlp_matches_source() {
    let base = std::env::temp_dir().join(format!("snowify-parity-ytdlp-{}", std::process::id()));
    let repo = root().join("..").canonicalize().unwrap().display().to_string();
    let probe = base.join("case");
    let r = |rel: &str| format!("{}/{rel}", probe.display());
    let mut d = Diff::default();
    for s in scenarios() {
        let input = json!({ "platform": s.platform, "resourcesPath": r("res"), "userData": r("ud"), "ops": s.ops });
        tree::remove(&probe);
        build(&probe, &s);
        let src = common::run_source("ytdlp.js", &input);
        let src_tree = tree::snapshot(&probe);

        tree::remove(&probe);
        build(&probe, &s);
        let env = Env { platform: s.platform.into(), resources_path: r("res"), dirname: format!("{repo}/src/main"), user_data: r("ud") };
        let logs_before = snowify::logger::LOGS.lock().unwrap().entries().len();
        let mut results = Vec::new();
        for op in &s.ops {
            if *op == "path" {
                results.push(json!(ytdlp::yt_dlp_path(&env)));
            } else {
                if let Some(fut) = ytdlp::update_yt_dlp(&env) {
                    fut.await;
                }
                results.push(Value::Null);
            }
        }
        let logs: Vec<Value> = snowify::logger::LOGS.lock().unwrap().entries()[logs_before..]
            .iter()
            .map(|e| json!([e.level.encode(), e.msg]))
            .collect();
        let rust_tree = tree::snapshot(&probe);

        d.check(format_args!("{} results", s.name), &src["results"], &json!(results));
        d.check(format_args!("{} logs", s.name), &src["logs"], &json!(logs));
        d.check(format_args!("{} tree", s.name), &json!(src_tree), &json!(rust_tree));
    }
    tree::remove(&base);
    d.finish("ytdlp");
}
