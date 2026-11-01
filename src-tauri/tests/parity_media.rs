//! Differential test of media origin behaviour: `parity/media-probe.js` in real Electron 33
//! (a file:// page with the app's googlevideo CORS hook) vs the Tauri shim + loopback proxy.
//! Covers a file:// cover inserted via innerHTML, a missing file, file:// audio through Web
//! Audio, and what src/currentSrc/getAttribute report. A live googlevideo stream is included
//! when `yt-dlp` can resolve one (the count printed says whether it was).
//!
//! Needs a display (both run real windows; CI uses xvfb-run).

use serde_json::{Value, json};
use std::process::Command;

mod common;
use common::{Diff, root};

fn result_of(out: &[u8]) -> Value {
    let text = String::from_utf8_lossy(out);
    let line = text
        .lines()
        .rev()
        .find(|l| l.contains("\"results\""))
        .unwrap_or_else(|| panic!("no result:\n{text}"));
    let v: Value = serde_json::from_str(line).unwrap();
    serde_json::from_str(v["results"].as_str().unwrap()).unwrap()
}

fn write_wav(path: &std::path::Path) {
    let (rate, secs) = (44_100u32, 8u32);
    let n = rate * secs;
    let mut b = Vec::with_capacity(44 + n as usize * 2);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + n * 2).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let s = ((i as f64) * 2.0 * std::f64::consts::PI * 440.0 / f64::from(rate)).sin() * 12000.0;
        b.extend_from_slice(&(s as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

#[test]
fn media_origin_matches_electron() {
    let dir = std::env::temp_dir().join(format!("snowify-parity-media-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("cover with space ü.png");
    std::fs::copy(root().join("../assets/logo.png"), &png).unwrap();
    let wav = dir.join("tone 1.wav");
    write_wav(&wav);
    let wav_hash = dir.join("tone #1.wav");
    write_wav(&wav_hash);
    let gv = Command::new("yt-dlp")
        .args([
            "-f",
            "251",
            "-g",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| s.contains(".googlevideo.com/"));
    let fixtures = json!({ "png": png, "wav": wav, "wavHash": wav_hash, "gv": gv }).to_string();

    let electron = Command::new(root().join("../node_modules/.bin/electron"))
        .arg(root().join("parity/electron/media/main.js"))
        .env("SNOWIFY_MEDIA_FIXTURES", &fixtures)
        .output()
        .expect("run electron");
    let tauri = Command::new(env!("CARGO_BIN_EXE_snowify"))
        .env("SNOWIFY_MEDIA_TEST", &fixtures)
        .output()
        .expect("run snowify");
    let _ = std::fs::remove_dir_all(&dir);

    let (e, t) = (result_of(&electron.stdout), result_of(&tauri.stdout));
    let mut d = Diff::default();
    for key in ["error", "img", "missing", "wav", "wavHash", "gv"] {
        d.check(key, &e[key], &t[key]);
    }
    // The Electron side must actually exercise the paths, or matching proves nothing.
    assert_eq!(
        e["img"]["width"], 512,
        "electron did not load the file:// cover: {e}"
    );
    assert_eq!(e["wav"]["audible"], true, "electron produced no audio: {e}");
    println!(
        "media: googlevideo stream {}",
        if gv.is_some() {
            "included"
        } else {
            "unavailable (yt-dlp), skipped"
        }
    );
    d.finish("media");
}
