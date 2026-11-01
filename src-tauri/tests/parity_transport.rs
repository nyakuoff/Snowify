//! Differential test of the IPC seam: the same probe page and value corpus run against real
//! Electron 33 (contextBridge + ipcRenderer, `parity/electron/transport`) and against the Tauri
//! build (`shim.js` + `codec.js` + `src/ipc.rs`). Compares what handlers receive, what the
//! renderer gets back, rejections, `send`, events and the exposed API's shape.
//!
//! Needs a display (both run real windows; CI uses xvfb-run).

use serde_json::json;
use std::process::Command;

mod common;
use common::{Diff, root};

/// The probe's result line, kept as raw text: it may contain lone-surrogate escapes.
fn last_json_line(out: &[u8], marker: &str) -> String {
    let text = String::from_utf8_lossy(out);
    text.lines()
        .rev()
        .find(|l| l.contains(marker))
        .unwrap_or_else(|| panic!("no output containing {marker}:\n{text}"))
        .to_owned()
}

#[test]
fn transport_matches_electron() {
    let electron = Command::new(root().join("../node_modules/.bin/electron"))
        .arg(root().join("parity/electron/transport/main.js"))
        .output()
        .expect("run electron");
    let electron = last_json_line(&electron.stdout, "\"seen\"");

    let tauri = Command::new(env!("CARGO_BIN_EXE_snowify"))
        .env("SNOWIFY_TRANSPORT_TEST", "1")
        .output()
        .expect("run snowify");
    let tauri = last_json_line(&tauri.stdout, "\"seen\"");

    let pairs = common::run_source(
        "transport_compare.js",
        &json!({ "electron": electron, "tauri": tauri }),
    );
    let mut d = Diff::default();
    for p in pairs.as_array().unwrap() {
        d.check(p[0].as_str().unwrap(), &p[1], &p[2]);
    }
    assert!(d.compared > 250, "only {} comparisons", d.compared);
    d.finish("transport");
}
