//! Differential test: the yt-dlp, download, cache and `net:httpGet` handlers of
//! `src/main/ytmusic.js` (unmodified) vs `snowify::ytmusic`, with a fake yt-dlp that logs
//! its argv and acts on its last argument. Compared: results, logs, HTTP calls, and the
//! resulting tree (cache dir, saved files, the argv log).

use serde_json::{Value, json};
use snowify::{ipc::JsValue, ytmusic};
use std::os::unix::fs::PermissionsExt;

mod common;
#[path = "common/host.rs"]
mod host;
#[path = "common/tree.rs"]
mod tree;
use common::Diff;
use host::MockHost;

const FAKE: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$(dirname "$0")/../../argv.log"
for a; do last="$a"; done
out=""; prev=""
for a; do if [ "$prev" = "-o" ]; then out="$a"; fi; prev="$a"; done
case "$last" in
  *fail*) echo "  ERROR: [youtube] unavailable  " >&2; exit 1;;
  *quiet*) exit 2;;
  *empty*) exit 0;;
  *two*) echo "https://v.example/video"; echo; echo "https://a.example/audio"; exit 0;;
  *nofile*) exit 0;;
  *slow*) target=$(printf '%s' "$out" | sed 's/%(ext)s/part/'); echo partial > "$target"; sleep 5; exit 0;;
esac
if [ -n "$out" ]; then
  target=$(printf '%s' "$out" | sed 's/%(ext)s/webm/'); echo audio > "$target"
fi
echo "https://stream.example/$(printf '%s' "$last" | tr -c 'a-zA-Z0-9\n' '_')"
"#;

fn w(v: Value) -> Value {
    JsValue::from(&v).encode()
}
fn op(ch: &str, args: Vec<Value>) -> Value {
    json!([ch, args.into_iter().map(w).collect::<Vec<_>>()])
}

async fn dispatch_streams(s: &ytmusic::Streams, h: &MockHost, ytdlp: &str, cache: &str, ch: &str, a: &[JsValue]) -> Option<snowify::ipc::HandlerResult> {
    use snowify::host::Host as _;
    let g = |i: usize| a.get(i).cloned().unwrap_or_default();
    Some(match ch {
        "yt:getStreamUrl" => ytmusic::get_stream_url(s, ytdlp, h.now_ms(), [g(0), g(1), g(2), g(3)]).await,
        "yt:getVideoStreamUrl" => ytmusic::get_video_stream_url(s, ytdlp, h.now_ms(), &g(0), &g(1), &g(2)).await,
        "yt:downloadAudio" => ytmusic::download_audio(s, ytdlp, cache, &g(0), &g(1), &g(2)).await,
        "cache:deleteFile" => ytmusic::delete_file(cache, &g(0)),
        "cache:clear" => ytmusic::clear_cache(cache),
        "cache:cancelDownload" => ytmusic::cancel_download(s),
        "song:saveTo" => ytmusic::save_to(h, ytdlp, "dialog.saveSong", &g(0), &g(1), &g(2)).await,
        "net:httpGet" => ytmusic::http_get(h, &g(0), &if g(1) == JsValue::Undefined { JsValue::Object(Vec::new()) } else { g(1) }).await,
        _ => return None,
    })
}

static STREAMS: std::sync::OnceLock<ytmusic::Streams> = std::sync::OnceLock::new();
static PATHS: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();

// This is an intentional native runtime policy, not a change to the Electron parity model.
// Exercise real subprocess argv, cache reuse, and the non-YouTube source boundary.
#[tokio::test]
async fn desktop_streams_use_immediately_playable_youtube_client() {
    let dir = std::env::temp_dir().join(format!("snowify-desktop-streams-{}", std::process::id()));
    let bin = dir.join("ud/bin/yt-dlp");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, FAKE).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let streams = ytmusic::Streams::desktop();
    let bin = bin.to_str().unwrap();
    let audio = ["https://yt/watch?v=desktop".into(), "bestaudio".into(), JsValue::Undefined, JsValue::Undefined];
    ytmusic::get_stream_url(&streams, bin, 0, audio.clone()).await.unwrap();
    ytmusic::get_stream_url(&streams, bin, 1, audio).await.unwrap();
    ytmusic::get_video_stream_url(&streams, bin, 0, &"two".into(), &"480".into(), &true.into()).await.unwrap();
    ytmusic::download_audio(&streams, bin, dir.join("cache").to_str().unwrap(), &"https://yt/watch?v=prefetch".into(), &"worstaudio".into(), &"prefetch".into()).await.unwrap();
    ytmusic::get_stream_url(&streams, bin, 0, ["u".into(), "bestaudio".into(), JsValue::object([("title", "Song".into())]), JsValue::Array(vec!["soundcloud".into()])]).await.unwrap();
    let argv = std::fs::read_to_string(dir.join("argv.log")).unwrap();
    let calls: Vec<_> = argv.lines().collect();
    assert_eq!(calls.len(), 4, "the repeated audio request must use its cached URL");
    for call in &calls[..3] {
        assert!(call.contains("--extractor-args youtube:player_client=visionos"), "{call}");
    }
    assert!(calls[0].contains("-f bestaudio/bestaudio*/best"));
    assert!(calls[2].contains("-f worstaudio/worstaudio*/worst"));
    assert!(!calls[3].contains("--extractor-args"), "SoundCloud must be unchanged");
    std::fs::remove_dir_all(dir).unwrap();
}

fn dispatch<'a>(
    h: &'a MockHost, ch: String, args: Vec<JsValue>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<snowify::ipc::HandlerResult>> + 'a>> {
    Box::pin(async move {
        let (ytdlp, cache) = PATHS.get().unwrap();
        dispatch_streams(STREAMS.get().unwrap(), h, ytdlp, cache, &ch, &args).await
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ytstreams_match_source() {
    let base = std::env::temp_dir().join(format!("snowify-parity-ytstreams-{}", std::process::id()));
    let probe = base.join("case");
    let r = |rel: &str| format!("{}/{rel}", probe.display());
    let repo_root = common::root().join("..").canonicalize().unwrap().display().to_string();
    let cache = r("tmp/snowify-audio-cache");
    let api = "https://api.github.com/repos/nyakuoff/Snowify/releases";
    let ops = vec![
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=ok1"), json!("bestaudio")]),
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=ok1"), json!("bestaudio")]),
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=ok1"), json!("worstaudio")]),
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=fail"), json!("x")]),
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=quiet"), json!("x")]),
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=empty"), json!("x")]),
        op("yt:getStreamUrl", vec![json!("u"), json!("x"), json!({ "title": "T", "artist": "A" }), json!(["soundcloud"])]),
        op("yt:getStreamUrl", vec![json!("u"), json!("x"), json!({}), json!(["soundcloud"])]),
        op("yt:getStreamUrl", vec![json!("u"), json!("x"), json!(null), json!(["soundcloud", "other"])]),
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=fail"), json!("x"), json!({ "title": "fail" }), json!(["youtube", "youtube", "soundcloud"])]),
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=ok2"), json!("x"), json!({}), json!([])]),
        op("yt:getStreamUrl", vec![json!("https://yt/watch?v=ok3"), json!("x"), json!({}), json!("abc")]),
        op("yt:getStreamUrl", vec![json!("bad\u{0}url"), json!("x")]),
        op("yt:getVideoStreamUrl", vec![json!("two"), json!("480p"), json!(false)]),
        op("yt:getVideoStreamUrl", vec![json!("two"), json!("480p"), json!(false)]),
        op("yt:getVideoStreamUrl", vec![json!("x1"), json!("abc"), json!(true)]),
        op("yt:getVideoStreamUrl", vec![json!("empty"), json!(0), json!(1)]),
        op("yt:getVideoStreamUrl", vec![json!("fail"), json!("1080"), json!(false)]),
        op("yt:downloadAudio", vec![json!("https://yt/watch?v=ok"), json!("bestaudio"), json!("vidA")]),
        op("yt:downloadAudio", vec![json!("https://yt/watch?v=other"), json!("worstaudio"), json!("vidA")]),
        op("yt:downloadAudio", vec![json!("https://yt/watch?v=fail"), json!("x"), json!("vidB")]),
        op("yt:downloadAudio", vec![json!("https://yt/watch?v=nofile"), json!("x"), json!("vidC")]),
        json!(["$parallel", [op("yt:downloadAudio", vec![json!("https://yt/watch?v=slow"), json!("x"), json!("vidD")]), op("cache:cancelDownload", vec![])]]),
        op("cache:cancelDownload", vec![]),
        op("cache:deleteFile", vec![json!(format!("{cache}/vidA.webm"))]),
        op("cache:deleteFile", vec![json!(format!("{cache}/../escape"))]),
        op("cache:deleteFile", vec![json!(5)]),
        op("song:saveTo", vec![json!("https://yt/watch?v=ok"), json!("My/Song: \"x\""), json!("Art")]),
        op("song:saveTo", vec![json!("https://yt/watch?v=ok"), json!(null), json!("")]),
        op("song:saveTo", vec![json!("https://yt/watch?v=fail"), json!("T"), json!(null)]),
        op("song:saveTo", vec![json!("https://yt/watch?v=ok"), json!("T"), json!("A")]),
        op("cache:clear", vec![]),
        op("net:httpGet", vec![json!(format!("{api}?per_page=1#frag")), json!({ "Cookie": "x", "X-Ok": 5, "bad header": "y", "accept": "text/plain", " ": "z", "authorization": "t" })]),
        op("net:httpGet", vec![json!("https://api.github.com:8443/x")]),
        op("net:httpGet", vec![json!("https://raw.githubusercontent.com/a/b"), json!([1])]),
        op("net:httpGet", vec![json!("https://evil.example/x")]),
        op("net:httpGet", vec![json!("http://api.github.com/x")]),
        op("net:httpGet", vec![json!("not a url")]),
        op("net:httpGet", vec![json!("https://api.github.com/slow")]),
        op("net:httpGet", vec![json!("https://api.github.com/down")]),
    ];
    let http = json!({
        format!("{api}?per_page=1"): [{ "status": 200, "body": "[{\"tag_name\":\"v2\"}]" }],
        "https://api.github.com/x": [{ "status": 404, "body": "Not Found" }],
        "https://raw.githubusercontent.com/a/b": [{ "status": 200, "body": "\u{FEFF}{\"a\":1}" }],
        "https://api.github.com/slow": [{ "timeout": true }],
        "https://api.github.com/down": [{ "error": { "message": "getaddrinfo ENOTFOUND api.github.com", "props": { "code": "ENOTFOUND" } } }],
    });
    let scenario = json!({
        "module": "ytmusic", "userData": r("ud"), "isPackaged": false, "now": 1_761_800_000_000_i64, "opTimeoutMs": 15000,
        "dialogs": [], "saveDialogs": [{ "canceled": false, "filePath": r("out/a.mp3") }, { "canceled": true }, { "canceled": false, "filePath": r("out/b.mp3") }, { "canceled": false, "filePath": "" }],
        "http": http, "ops": ops,
    });
    let build = || {
        tree::remove(&probe);
        std::fs::create_dir_all(probe.join("ud/bin")).unwrap();
        std::fs::create_dir_all(probe.join("tmp")).unwrap();
        std::fs::create_dir_all(probe.join("out")).unwrap();
        let f = probe.join("ud/bin/yt-dlp");
        std::fs::write(&f, FAKE).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    build();
    let src = common::run_source_with("main_module.js", &scenario, |c| {
        c.env("TMPDIR", r("tmp"));
    });
    let src_tree = tree::snapshot(&probe);

    build();
    let host = MockHost::new(&scenario, &repo_root);
    STREAMS.get_or_init(ytmusic::Streams::default);
    PATHS.set((r("ud/bin/yt-dlp"), cache.clone())).unwrap();
    let rust = host::run_ops(&host, &scenario, &dispatch).await;
    let rust_tree = tree::snapshot(&probe);

    let mut d = Diff::default();
    for key in ["results", "logs", "httpCalls", "dialogCalls"] {
        let (a, b) = (&src[key], &rust[key]);
        for i in 0..a.as_array().unwrap().len().max(b.as_array().unwrap().len()) {
            d.check(format_args!("{key}[{i}]"), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
        }
    }
    d.check("tree", &json!(src_tree), &json!(rust_tree));
    tree::remove(&base);
    d.finish("ytstreams");
}
