//! Differential test: `src/main/lyrics.js` + `@stef-0012/synclyrics` (unmodified; global
//! `fetch` replayed from the scenario) vs `snowify::lyrics`, comparing results, log lines and
//! every request made (URL, headers as sent, redirect mode), in order.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, lyrics};
use std::sync::Arc;

mod common;
#[path = "common/host.rs"]
mod host;
use common::Diff;
use host::MockHost;

const TOKEN: &str = "https://apic-desktop.musixmatch.com/ws/1.1/token.get";
const SEARCH: &str = "https://apic-desktop.musixmatch.com/ws/1.1/track.search";
const MXM_LYRICS: &str = "https://apic-desktop.musixmatch.com/ws/1.1/track.lyrics.get";
const MXM_SUB: &str = "https://apic-desktop.musixmatch.com/ws/1.1/track.subtitle.get";
const MXM_RICH: &str = "https://apic-desktop.musixmatch.com/ws/1.1/track.richsync.get";
const LRC_SEARCH: &str = "https://lrclib.net/api/search";
const LRC_GET: &str = "https://lrclib.net/api/get";
const NE_SEARCH: &str = "https://music.xianqiao.wang/neteaseapiv2/search";
const NE_LYRIC: &str = "https://music.xianqiao.wang/neteaseapiv2/lyric";

fn ok(v: Value) -> Value {
    json!({ "status": 200, "statusText": "OK", "headers": [["content-type", "application/json"]], "body": v.to_string() })
}
fn raw(status: u16, body: &str) -> Value {
    json!({ "status": status, "body": body })
}
fn mxm(body: Value) -> Value {
    ok(json!({ "message": { "header": { "status_code": 200 }, "body": body } }))
}
fn captcha() -> Value {
    ok(json!({ "message": { "header": { "status_code": 401, "hint": "captcha" }, "body": "" } }))
}
fn redirect() -> Value {
    json!({ "status": 301, "headers": [
        ["set-cookie", "AWSELB=abc; Path=/"],
        ["set-cookie", "x-mxm-token-guid=unknown; Path=/"],
        ["set-cookie", "mxm-encrypted-token=; Path=/"],
        ["set-cookie", "x-mxm-user-id=g2%3A1; Path=/; Secure"],
    ], "body": "" })
}
fn token() -> Value {
    mxm(json!({ "user_token": "tok en/+" }))
}
fn w(v: Value) -> Value {
    JsValue::from(&v).encode()
}
fn get(args: [Value; 4]) -> Value {
    json!(["lyrics:get", args.into_iter().map(w).collect::<Vec<_>>()])
}

const NETEASE_LRC: &str = "[by:someone]\r\n[00:00.00] 作词 : 某人\r\n[00:01.00] 作曲：某人\n[00:02.50]第一句（括号）！\n[00:03.00][00:04.00]repeated line\n[00:05.00]Mix by: nobody\n[00:06.00]  ‘quoted’ “double”  \n\n[00:07.00]ß straße\n[00:08.00]\u{2028}x\n[broken\n[00:09.00]vocal：A\n[00:10.00]   \n";

struct Scenario {
    name: &'static str,
    fetch: Vec<(&'static str, Vec<Value>)>,
    ops: Vec<Value>,
}

fn scenarios() -> Vec<Scenario> {
    let song = |t: &str, a: &str, al: &str, d: Value| get([json!(t), json!(a), json!(al), d]);
    let search_hit = mxm(json!({ "track_list": [
        { "track": { "track_name": "Other", "artist_name": "Artist" } },
        { "track": { "track_name": "song", "artist_name": "Nope" } },
        { "track": { "track_name": "SONG", "artist_name": "The ARTIST Band", "commontrack_id": 77, "track_id": 99, "has_lyrics": 1, "has_subtitles": 1, "has_richsync": 1 } },
    ] }));
    let rich = json!([{ "ts": 1.0, "te": 2.0, "x": "hi", "l": [{ "c": "h", "o": 0 }, { "c": "i", "o": 0.5 }] }]).to_string();
    vec![
        Scenario {
            name: "musixmatch full, then caches",
            fetch: vec![
                (TOKEN, vec![redirect(), token()]),
                (SEARCH, vec![search_hit.clone()]),
                (MXM_LYRICS, vec![mxm(json!({ "lyrics": { "lyrics_body": "plain body" } }))]),
                (MXM_SUB, vec![mxm(json!({ "subtitle": { "subtitle_body": "[00:01.00] synced" } }))]),
                (MXM_RICH, vec![mxm(json!({ "richsync": { "richsync_body": rich } }))]),
            ],
            ops: vec![
                song("Song", "Artist", "Album", json!(215.4)),
                song("Song", "Artist", "Album", json!(215.4)),
                song("Song", "Artist", "Album", json!(100)),
            ],
        },
        Scenario {
            name: "musixmatch plain + lrclib synced",
            fetch: vec![
                (TOKEN, vec![token()]),
                (SEARCH, vec![mxm(json!({ "track_list": [{ "track": { "track_name": "Song", "artist_name": "Artist", "commontrack_id": 5, "has_lyrics": true, "has_subtitles": 0 } }] }))]),
                (MXM_LYRICS, vec![mxm(json!({ "lyrics": { "lyrics_body": "plain only" } }))]),
                (LRC_SEARCH, vec![ok(json!([
                    { "artistName": null, "trackName": "Song" },
                    { "artistName": "artist", "trackName": "song", "syncedLyrics": "[00:02.00] from lrclib", "plainLyrics": "" },
                ]))]),
            ],
            ops: vec![song("Song (feat. Guest)", "Artist", "", json!(0))],
        },
        Scenario {
            name: "nothing, then lrclib get fallback",
            fetch: vec![
                (TOKEN, vec![raw(500, "oops")]),
                (LRC_SEARCH, vec![ok(json!([]))]),
                (NE_SEARCH, vec![ok(json!({ "result": { "songs": [] } }))]),
                (LRC_GET, vec![raw(404, "{}"), raw(404, "{}"), ok(json!({ "syncedLyrics": null, "plainLyrics": "got it", "source": "x" }))]),
            ],
            ops: vec![song("Track - Remastered 2011", "A & B", "  Big   Album ", json!("181.6"))],
        },
        Scenario {
            name: "netease",
            fetch: vec![
                (TOKEN, vec![mxm(json!({}))]),
                (LRC_SEARCH, vec![raw(500, "")]),
                (NE_SEARCH, vec![ok(json!({ "result": { "songs": [
                    { "name": "Song", "artists": [{ "name": null }, { "name": "x" }] },
                    { "name": "song", "artists": [{ "name": "Jay-Z Band" }], "id": 4242 },
                ] } }))]),
                (NE_LYRIC, vec![ok(json!({ "lrc": { "lyric": NETEASE_LRC } }))]),
            ],
            ops: vec![song("Song", "jay z", "", json!(null))],
        },
        Scenario {
            name: "captcha retries",
            fetch: vec![
                (TOKEN, vec![captcha(), token()]),
                (SEARCH, vec![captcha()]),
                (LRC_SEARCH, vec![ok(json!([{ "artistName": "Artist", "trackName": "Song", "syncedLyrics": [], "plainLyrics": "p" }]))]),
            ],
            ops: vec![song("Song", "Artist", "Al", json!(10))],
        },
        Scenario {
            name: "concurrency",
            fetch: vec![
                (TOKEN, vec![token()]),
                (SEARCH, vec![mxm(json!({ "track_list": [] }))]),
                (LRC_SEARCH, vec![ok(json!([]))]),
                (NE_SEARCH, vec![raw(404, "")]),
                (LRC_GET, vec![ok(json!({ "syncedLyrics": "[00:01.00] get" }))]),
            ],
            ops: vec![
                json!(["$parallel", [song("One", "A", "", json!(1)), song("Two", "B", "", json!(2)), song("One", "A", "", json!(1.2))]]),
                song("Two", "B", "", json!(2)),
            ],
        },
        Scenario {
            name: "odd inputs",
            fetch: vec![(LRC_GET, vec![raw(404, "")])],
            ops: vec![
                get([json!(5), json!(true), json!(null), json!(3)]),
                get([json!({ "$": "b", "v": "5" }), json!("A"), json!(""), json!(1)]),
                get([json!("T"), json!("A"), json!(""), json!({ "$": "b", "v": "7" })]),
                get([json!({ "$": "s", "v": [0xD800, 0x61] }), json!("A"), json!(""), json!(1)]),
                get([json!(""), json!(""), json!(""), json!(0)]),
                song("Live Song - Live at Wembley", "A x B", "", json!(1)),
                song("Song (Remastered 2009) [feat. Y]", "A feat. B", "", json!(1)),
                song("Remaſtered - remaſtered", "ſ, b", "", json!(1)),
                song("Tab\tand\u{A0}nbsp\u{2028}x", " featuring Z", "", json!(-0.4)),
                get([json!([1, 2]), json!({ "a": 1 }), json!([]), json!("1e3")]),
            ],
        },
        Scenario {
            name: "malformed responses",
            fetch: vec![
                (TOKEN, vec![redirect(), raw(200, "\u{FEFF}{\"message\":{\"body\":{\"user_token\":7}}}")]),
                (SEARCH, vec![
                    mxm(json!({ "track_list": "abc" })),
                    mxm(json!({ "track_list": [null] })),
                    mxm(json!({ "track_list": [{ "track": { "track_name": 5 } }] })),
                    mxm(json!({ "track_list": [{ "track": { "track_name": "S", "artist_name": "A", "track_id": 1, "has_richsync": 1 } }] })),
                    mxm(json!({ "track_list": { "length": 3 } })),
                ]),
                (MXM_RICH, vec![mxm(json!({ "richsync": { "richsync_body": "not json" } }))]),
                (LRC_SEARCH, vec![ok(json!({ "find": 1 })), ok(json!(null)), ok(json!([5])), raw(200, "not json")]),
                (NE_SEARCH, vec![
                    ok(json!({ "result": { "songs": "xyz" } })),
                    ok(json!({ "result": { "songs": [{ "name": "S", "artists": {} }] } })),
                    ok(json!({ "result": { "songs": [{ "name": "S", "artists": [{ "name": "A" }], "id": 0 }] } })),
                    ok(json!({ "result": { "songs": [{ "name": "S", "artists": [{ "name": "A" }], "id": { "a": 1 } }] } })),
                    json!({ "error": true }),
                ]),
                (NE_LYRIC, vec![ok(json!({ "lrc": { "lyric": 12 } }))]),
                (LRC_GET, vec![ok(json!([1])), ok(json!("str")), raw(200, ""), json!({ "error": true }), ok(json!({ "lyrics": { "plain": { "lyrics": "nested", "source": "deep" } } }))]),
            ],
            ops: vec![song("S", "A", "", json!(2)), song("S", "A", "B", json!(3))],
        },
    ]
}

/// Shapes that only some branches tell apart.
fn edges() -> Vec<Scenario> {
    let song = |t: &str, a: &str, d: Value| get([json!(t), json!(a), json!(""), d]);
    vec![
        Scenario {
            name: "captcha search still listing tracks",
            fetch: vec![
                (TOKEN, vec![token()]),
                (SEARCH, vec![ok(json!({ "message": { "header": { "status_code": 401, "hint": "captcha" }, "body": { "track_list": [
                    { "track": { "track_name": "S", "artist_name": "A", "commontrack_id": 1, "has_lyrics": 1 } },
                ] } } }))]),
                (MXM_LYRICS, vec![mxm(json!({ "lyrics": { "lyrics_body": "should not appear" } }))]),
            ],
            ops: vec![song("S", "A", json!(1))],
        },
        Scenario {
            name: "lrclib empty array lyrics, sub-millisecond duration",
            fetch: vec![
                (TOKEN, vec![raw(403, "")]),
                (LRC_SEARCH, vec![ok(json!([{ "artistName": "A", "trackName": "S", "syncedLyrics": [], "plainLyrics": "" }]))]),
            ],
            ops: vec![song("S", "A", json!(0.0004))],
        },
        Scenario {
            name: "concurrent lookups swap cache entries",
            fetch: vec![
                (TOKEN, vec![raw(403, "")]),
                (LRC_SEARCH, vec![ok(json!([{ "artistName": "A", "trackName": "One", "syncedLyrics": "[00:01.00] one" }]))]),
            ],
            ops: vec![json!(["$parallel", [song("One", "A", json!(1)), song("Two", "B", json!(2))]]), song("Two", "B", json!(2))],
        },
    ]
}

/// LRU: 51 distinct songs with every endpoint failing (each result is cached empty), then the
/// first song again: evicted, so it is fetched again.
fn eviction() -> Scenario {
    let mut ops: Vec<Value> = (0..51).map(|i| get([json!(format!("T{i}")), json!("A"), json!(""), json!(0)])).collect();
    ops.push(get([json!("T0"), json!("A"), json!(""), json!(0)]));
    ops.push(get([json!("T50"), json!("A"), json!(""), json!(0)]));
    Scenario { name: "cache eviction", fetch: vec![(TOKEN, vec![raw(403, "")])], ops }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lyrics_match_source() {
    let repo_root = common::root().join("..").canonicalize().unwrap().display().to_string();
    let mut d = Diff::default();
    let mut all = scenarios();
    all.push(eviction());
    all.extend(edges());
    for s in all {
        let fetch: serde_json::Map<String, Value> = s.fetch.iter().map(|(k, v)| ((*k).to_owned(), json!(v))).collect();
        let scenario = json!({
            "module": "lyrics", "userData": "/mock/ud", "isPackaged": true, "dialogs": [], "http": {},
            "fetch": fetch, "opTimeoutMs": 30000, "ops": s.ops,
        });
        let src = common::run_source("main_module.js", &scenario);

        let host = Arc::new(MockHost::new(&scenario, &repo_root));
        let state = Arc::new(lyrics::Lyrics::default());
        let (h2, st2) = (host.clone(), state.clone());
        let dispatch = move |_h: &MockHost, ch: String, args: Vec<JsValue>| {
            let (h, st) = (h2.clone(), st2.clone());
            Box::pin(async move {
                assert_eq!(ch, "lyrics:get");
                let a = |i: usize| args.get(i).cloned().unwrap_or_default();
                Some(lyrics::handle(st, h, [a(0), a(1), a(2), a(3)]).await)
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = Option<snowify::ipc::HandlerResult>>>>
        };
        let rust = host::run_ops(&host, &scenario, &dispatch).await;

        for key in ["results", "logs", "fetchCalls"] {
            let (a, b) = (&src[key], &rust[key]);
            let n = a.as_array().map_or(0, Vec::len).max(b.as_array().map_or(0, Vec::len));
            for i in 0..n {
                d.check(format_args!("{} {key}[{i}]", s.name), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
            }
        }
    }
    d.finish("lyrics");
}
