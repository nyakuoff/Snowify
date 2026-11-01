//! Differential test: the file-facing handlers of `src/main/media.js` (unmodified; Electron,
//! dialogs and the clock mocked) vs `snowify::media`, scenario by scenario on identical trees.
//! Local-audio scenarios use ffmpeg-made files, parsed by music-metadata on the source side.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, media};
use std::{path::Path, process::Command};

mod common;
#[path = "common/host.rs"]
mod host;
#[path = "common/tree.rs"]
mod tree;
use common::{Diff, root};
use host::MockHost;
use tree::{Entry, dir, link};

const NOW: i64 = 1_761_600_000_123;

fn w(v: Value) -> Value {
    JsValue::from(&v).encode()
}

fn op(ch: &str, args: Vec<Value>) -> Value {
    json!([ch, args])
}

struct Scenario {
    name: &'static str,
    tree: Vec<Entry>,
    contents: Vec<(&'static str, Vec<u8>)>,
    dialogs: Value,
    save_dialogs: Value,
    ops: Vec<Value>,
}

fn t(p: &'static str, s: &str) -> (&'static str, Vec<u8>) {
    (p, s.as_bytes().to_vec())
}

fn audio(name: &str, args: &[&str]) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("snowify-parity-media-files-fx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(name);
    let ok = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
        .args(args)
        .arg(&out)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "ffmpeg failed for {name}");
    std::fs::read(out).unwrap()
}

/// An m4a whose `©nam` value is retyped to a BE signed integer (data type 21), so
/// music-metadata reports a numeric title; `zero` also blanks the value to 0.
fn m4a_numeric_title(name: &str, zero: bool) -> Vec<u8> {
    let mut b = audio(name, &["-c:a", "aac", "-metadata", "title=abcd"]);
    let at = b.windows(4).position(|w| w == b"\xa9nam").expect("no title atom") + 4;
    assert_eq!(&b[at + 4..at + 8], b"data");
    b[at + 8..at + 12].copy_from_slice(&[0, 0, 0, 21]);
    if zero {
        b[at + 16..at + 20].fill(0);
    }
    b
}

fn scenarios(r: &dyn Fn(&str) -> String) -> Vec<Scenario> {
    let mp3 = audio("a.mp3", &["-c:a", "libmp3lame", "-metadata", "title=Song A", "-metadata", "artist=Art", "-metadata", "album=Alb"]);
    let flac = audio("b.flac", &["-c:a", "flac"]);
    let ogg = audio("c.ogg", &["-c:a", "libvorbis", "-metadata", "artist=Ogg Artist"]);
    vec![
        Scenario {
            name: "spotify csv",
            tree: vec![dir("csv/dir.csv")],
            contents: vec![
                t("csv/My_Playlist.csv", "Track Name,Artist Name(s),Album\r\n\"Hello, World\",\"A\\, B\",X\r\n,NoTitle,Y\r\n\"Quote \"\"q\"\"\",,Z\r\n   \r\nPlain,Solo"),
                t("csv/weird.csv", "SONG_NAME,ARTISTS(S)\nOne,Two\nThree"),
                t("csv/dash.csv", "track-name\nOnly Title"),
                t("csv/notitle.csv", "Foo,Bar\n1,2"),
                t("csv/single.csv", "Track Name"),
                t("csv/bom.csv", "\u{FEFF}Title,Artist\nBom,Row"),
                t("csv/sep.csv", "Track\u{2028}Name,artist\u{2028}name\nA,B"),
                t("csv/artists.csv", "name,artists\nN,M"),
                t("csv/twosep.csv", "track__name,artist\nA,B"),
            ],
            dialogs: json!([
                { "canceled": false, "filePaths": [r("csv/My_Playlist.csv"), r("csv/weird.csv"), r("csv/dash.csv"), r("csv/notitle.csv"), r("csv/single.csv"), r("csv/bom.csv"), r("csv/sep.csv"), r("csv/artists.csv"), r("csv/twosep.csv"), r("csv/missing.csv"), r("csv/dir.csv")] },
                { "canceled": true, "filePaths": [] },
                { "canceled": false, "filePaths": [r("csv/notitle.csv")] },
            ]),
            save_dialogs: json!([]),
            ops: vec![op("spotify:pickCsv", vec![]), op("spotify:pickCsv", vec![]), op("spotify:pickCsv", vec![])],
        },
        Scenario {
            name: "playlist csv export",
            tree: vec![dir("out"), dir("ro").mode(0o555)],
            contents: vec![],
            dialogs: json!([]),
            save_dialogs: json!([
                { "canceled": false, "filePath": r("out/one.csv") },
                { "canceled": true },
                { "canceled": false, "filePath": "" },
                { "canceled": false, "filePath": r("ro/x.csv") },
                { "canceled": false, "filePath": r("out/two.csv") },
                { "canceled": false, "filePath": r("out/three.csv") },
                { "canceled": false, "filePath": r("out/four.csv") },
            ]),
            ops: vec![
                op("playlist:exportCsv", vec![w(json!("My/Play:list*?\"<>|")), w(json!([
                    { "title": "A, comma", "artist": "Q \"quoted\"", "album": "Multi\nline", "durationMs": 185_000 },
                    { "title": "Str", "artist": "x", "durationMs": "61000" },
                    { "title": "Neg", "durationMs": -1500 },
                    { "title": "Bool", "durationMs": true },
                    { "title": "Arr", "durationMs": [5000] },
                    { "title": 5, "artist": null, "album": { "a": 1 }, "durationMs": 0 },
                    { "durationMs": 1e21 },
                ]))]),
                op("playlist:exportCsv", vec![w(json!("canceled")), w(json!([]))]),
                op("playlist:exportCsv", vec![w(json!("empty path")), w(json!([]))]),
                op("playlist:exportCsv", vec![w(json!("ro")), w(json!([]))]),
                op("playlist:exportCsv", vec![w(json!("nan")), w(json!([{ "durationMs": { "$": "n", "v": "NaN" } }])).clone()]),
                op("playlist:exportCsv", vec![w(json!("strtracks")), w(json!("abc"))]),
                op("playlist:exportCsv", vec![w(json!("nulltrack")), w(json!([null]))]),
                op("playlist:exportCsv", vec![w(json!(5)), w(json!([]))]),
                op("playlist:exportCsv", vec![w(json!(null)), w(json!([]))]),
                op("playlist:exportCsv", vec![json!({ "$": "u" }), w(json!([]))]),
            ],
        },
        Scenario {
            name: "library export and import",
            tree: vec![dir("lib")],
            contents: vec![t("lib/in.json", "{\"playlists\": []} ü")],
            dialogs: json!([
                { "canceled": false, "filePaths": [r("lib/in.json")] },
                { "canceled": true, "filePaths": [] },
                { "canceled": false, "filePaths": [r("lib/missing.json")] },
            ]),
            save_dialogs: json!([
                { "canceled": false, "filePath": r("lib/out.json") },
                { "canceled": false, "filePath": r("lib/bytes.json") },
                { "canceled": false, "filePath": r("lib/num.json") },
                { "canceled": false, "filePath": r("lib/obj.json") },
                { "canceled": true },
            ]),
            ops: vec![
                op("library:export", vec![w(json!("{\"a\":1}"))]),
                op("library:export", vec![json!({ "$": "view", "k": "Uint8Array", "b": [104, 105, 33], "o": 1, "l": 2 })]),
                op("library:export", vec![w(json!(5))]),
                op("library:export", vec![w(json!({ "x": 1 }))]),
                op("library:export", vec![w(json!("x"))]),
                op("library:import", vec![]),
                op("library:import", vec![]),
                op("library:import", vec![]),
            ],
        },
        Scenario {
            name: "playlist covers",
            tree: vec![dir("img")],
            contents: vec![t("img/c.png", "png"), t("img/noext", "raw"), t("ud/playlist-covers/old.jpg", "old")],
            dialogs: json!([
                { "canceled": false, "filePaths": [r("img/c.png"), r("img/noext")] },
                { "canceled": true, "filePaths": [] },
            ]),
            save_dialogs: json!([]),
            ops: vec![
                op("playlist:pickImage", vec![]),
                op("playlist:pickImage", vec![]),
                op("playlist:saveImage", vec![w(json!("pl1")), w(json!(r("img/c.png")))]),
                op("playlist:saveImage", vec![w(json!(7)), w(json!(r("img/noext")))]),
                op("playlist:saveImage", vec![w(json!("pl2")), w(json!(r("img/missing.png")))]),
                op("playlist:saveImage", vec![w(json!("pl3")), w(json!(5))]),
                op("playlist:saveImage", vec![w(json!("a/b")), w(json!(r("img/c.png")))]),
                op("playlist:deleteImage", vec![w(json!(r("ud/playlist-covers/old.jpg")))]),
                op("playlist:deleteImage", vec![w(json!(r("ud/playlist-covers/old.jpg")))]),
                op("playlist:deleteImage", vec![w(json!(r("img")))]),
                op("playlist:deleteImage", vec![w(json!(null))]),
                op("playlist:deleteImage", vec![w(json!(5))]),
            ],
        },
        Scenario {
            name: "local audio",
            tree: vec![dir("music/sub/deeper"), dir("music/empty"), dir("other"), link("music/linkdir", "{root}/other"), dir("dest")],
            contents: vec![
                ("music/a.mp3", mp3.clone()),
                ("music/sub/B.FLAC", flac.clone()),
                ("music/sub/deeper/c.ogg", ogg.clone()),
                t("music/notes.txt", "x"),
                ("music/real.txt", mp3.clone()),
                ("music/num.m4a", m4a_numeric_title("num.m4a", false)),
                ("music/zero.m4a", m4a_numeric_title("zero.m4a", true)),
                t("music/broken.mp3", "not audio really"),
                ("other/hidden.mp3", mp3.clone()),
                ("dest/a.mp3", mp3.clone()),
            ],
            dialogs: json!([
                { "canceled": false, "filePaths": [r("music/a.mp3"), r("music/sub/B.FLAC"), r("music/broken.mp3"), r("music/missing.mp3")] },
                { "canceled": true, "filePaths": [] },
                { "canceled": false, "filePaths": [r("music")] },
                { "canceled": true, "filePaths": [] },
            ]),
            save_dialogs: json!([]),
            ops: vec![
                op("local:pickAudioFiles", vec![]),
                op("local:pickAudioFiles", vec![]),
                op("local:pickFolder", vec![]),
                op("local:pickFolder", vec![]),
                op("local:scanFolder", vec![w(json!(r("music/sub")))]),
                op("local:scanFolder", vec![w(json!(r("nope")))]),
                op("local:scanFolder", vec![w(json!(null))]),
                op("local:scanFolder", vec![w(json!(5))]),
                op("local:copyToPlaylistFolder", vec![w(json!(r("music/a.mp3"))), w(json!(r("dest")))]),
                op("local:copyToPlaylistFolder", vec![w(json!(r("music/sub/B.FLAC"))), w(json!(r("dest")))]),
                op("local:copyToPlaylistFolder", vec![w(json!(r("music/a.mp3"))), w(json!(r("nodest")))]),
                op("local:copyToPlaylistFolder", vec![w(json!(r("music/missing.mp3"))), w(json!(r("dest")))]),
                op("local:copyToPlaylistFolder", vec![w(json!(5)), w(json!(r("dest")))]),
            ],
        },
    ]
}

fn build(root: &Path, s: &Scenario) {
    tree::build(root, &s.tree);
    for (p, body) in &s.contents {
        let full = root.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, body).unwrap();
    }
    tree::build_modes(root, &s.tree);
}

fn dispatch<'a>(
    h: &'a MockHost,
    ch: String,
    args: Vec<JsValue>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<snowify::ipc::HandlerResult>> + 'a>> {
    Box::pin(async move {
        let a = |i: usize| args.get(i).cloned().unwrap_or(JsValue::Undefined);
        Some(match ch.as_str() {
            "spotify:pickCsv" => media::spotify_pick_csv(h).await,
            "playlist:exportCsv" => media::export_playlist_csv(h, &a(0), &a(1)).await,
            "library:export" => media::export_library(h, &a(0)).await,
            "library:import" => media::import_library(h).await,
            "playlist:pickImage" => media::pick_image(h).await,
            "playlist:saveImage" => media::save_image(h, &a(0), &a(1)).await,
            "playlist:deleteImage" => media::delete_image(&a(0)).await,
            "local:pickAudioFiles" => media::pick_audio_files(h).await,
            "local:pickFolder" => media::pick_folder(h).await,
            "local:scanFolder" => media::scan_folder(&a(0)).await,
            "local:copyToPlaylistFolder" => media::copy_to_playlist_folder(h, &a(0), &a(1)).await,
            _ => return None,
        })
    })
}

#[tokio::test(flavor = "current_thread")]
async fn media_files_match_source() {
    let base = std::env::temp_dir().join(format!("snowify-parity-media-files-{}", std::process::id()));
    let repo_root = root().join("..").canonicalize().unwrap().display().to_string();
    let probe = base.join("case");
    let r = |rel: &str| format!("{}/{rel}", probe.display());
    let mut d = Diff::default();
    for s in scenarios(&r) {
        let scenario = json!({
            "module": "media", "userData": r("ud"), "isPackaged": true, "now": NOW,
            "dialogs": s.dialogs, "saveDialogs": s.save_dialogs, "http": {}, "ops": s.ops,
        });
        build(&probe, &s);
        let src = common::run_source("main_module.js", &scenario);
        let src_tree = tree::snapshot(&probe);

        build(&probe, &s);
        let host = MockHost::new(&scenario, &repo_root);
        let rust = host::run_ops(&host, &scenario, &dispatch).await;
        let rust_tree = tree::snapshot(&probe);

        for key in ["results", "logs", "dialogCalls"] {
            let (a, b) = (&src[key], &rust[key]);
            let n = a.as_array().map_or(0, Vec::len).max(b.as_array().map_or(0, Vec::len));
            for i in 0..n {
                d.check(format_args!("{} {key}[{i}]", s.name), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
            }
        }
        d.check(format_args!("{} tree", s.name), &json!(src_tree), &json!(rust_tree));
    }
    tree::remove(&base);
    d.finish("media_files");
}
