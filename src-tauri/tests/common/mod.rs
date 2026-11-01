//! Shared harness plumbing: runs a `parity/node/*.js` runner against the source under
//! Electron 33's own Node (`ELECTRON_RUN_AS_NODE=1`), never the system Node, so the source
//! behaves exactly as it does in the shipped app.
#![allow(dead_code)]

use serde_json::Value;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn root() -> PathBuf {
    std::env::var_os("SNOWIFY_PARITY_ROOT").map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf())
}

/// Feeds `input` as JSON on stdin and parses the runner's stdout as JSON.
pub fn run_source(script: &str, input: &Value) -> Value {
    run_source_with(script, input, |_| {})
}

/// Like [`run_source`], letting the caller set env/cwd on the command.
pub fn run_source_with(script: &str, input: &Value, configure: impl FnOnce(&mut Command)) -> Value {
    let electron = std::env::var_os("SNOWIFY_PARITY_ELECTRON").map(PathBuf::from)
        .unwrap_or_else(|| root().join("../node_modules/.bin/electron"));
    assert!(
        electron.exists(),
        "run `npm install` at the repo root: parity tests need Electron 33"
    );
    // JSON argv avoids shell interpolation and preserves paths containing spaces.
    // On native Windows (including the test .exe under Wine), pass electron.exe directly.
    let launcher: Vec<String> = std::env::var("SNOWIFY_PARITY_LAUNCHER")
        .map(|s| serde_json::from_str(&s).expect("SNOWIFY_PARITY_LAUNCHER must be a JSON argv array"))
        .unwrap_or_default();
    let mut cmd = if let Some(program) = launcher.first() {
        let mut cmd = Command::new(program);
        cmd.args(&launcher[1..]).arg(&electron);
        cmd
    } else { Command::new(electron) };
    cmd.arg(root().join("parity/node").join(script))
        .env("ELECTRON_RUN_AS_NODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    configure(&mut cmd);
    let mut child = cmd.spawn().expect("spawn electron");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "runner {script} failed");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "runner {script} printed invalid JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// Collects differences and prints the `compared N, differences D` line every harness reports.
#[derive(Default)]
pub struct Diff {
    pub compared: usize,
    pub diffs: Vec<String>,
}

impl Diff {
    pub fn check(&mut self, what: impl std::fmt::Display, source: &Value, rust: &Value) {
        self.compared += 1;
        if source != rust {
            self.diffs
                .push(format!("{what}: source={source} rust={rust}"));
        }
    }

    pub fn finish(self, name: &str) {
        println!(
            "{name} parity: compared {}, differences {}",
            self.compared,
            self.diffs.len()
        );
        assert!(
            self.diffs.is_empty(),
            "{}",
            self.diffs[..self.diffs.len().min(20)].join("\n")
        );
    }
}

/// Deterministic xorshift generator, so corpora are reproducible without a `rand` dependency.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}
