//! Differential test: `src/main/ytmusic.js` + `media.js`'s `spotify:matchTrack` over the real
//! ytmusic-api/axios/tough-cookie/zod (unmodified; https answered from a recorded YouTube
//! Music session plus overrides) vs `snowify::ytmusic` over `snowify::ytm`. Compared: every
//! handler result, every log line, and every request on the wire (method, URL, headers, body).
//!
//! The recording is local (it carries the recorder's region and time zone, so it's
//! gitignored); `parity/node/ytm_record.js` makes a new one.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, ytm::Ytm, ytmusic};
use std::sync::Arc;

mod common;
#[path = "common/host.rs"]
mod host;
use common::Diff;
use host::MockHost;

fn recording() -> String {
    common::root().join("parity/corpus/ytmusic/session1.json.gz").display().to_string()
}

async fn dispatch(ytm: &Ytm, h: &MockHost, ch: &str, a: &[JsValue]) -> Option<snowify::ipc::HandlerResult> {
    let g = |i: usize| a.get(i).cloned().unwrap_or_default();
    Some(match ch {
        "yt:search" => ytmusic::search(ytm, h, &g(0), &g(1)).await,
        "yt:searchSuggestions" => ytmusic::search_suggestions(ytm, h, &g(0)).await,
        "yt:artistInfo" => ytmusic::artist_info(ytm, h, &g(0)).await,
        "yt:searchPlaylists" => ytmusic::search_playlists(ytm, h, &g(0)).await,
        "yt:getPlaylistVideos" => ytmusic::get_playlist_videos(ytm, h, &g(0)).await,
        "yt:albumTracks" => ytmusic::album_tracks(ytm, h, &g(0)).await,
        "yt:searchArtists" => ytmusic::search_artists(ytm, h, &g(0)).await,
        "yt:searchAlbums" => ytmusic::search_albums(ytm, h, &g(0)).await,
        "yt:searchVideos" => ytmusic::search_videos(ytm, h, &g(0)).await,
        "yt:setCountry" => ytmusic::set_country(ytm, h, &g(0)).await,
        "yt:explore" => ytmusic::explore(ytm, h).await,
        "yt:charts" => ytmusic::charts(ytm, h).await,
        "yt:browseMood" => ytmusic::browse_mood(ytm, h, &g(0), &g(1)).await,
        "yt:getTrackInfo" => ytmusic::get_track_info(ytm, h, &g(0)).await,
        "yt:getUpNexts" => ytmusic::get_up_nexts(ytm, h, &g(0)).await,
        "spotify:matchTrack" => snowify::media::spotify_match_track(ytm, h, &g(0), &g(1)).await,
        _ => return None,
    })
}

/// Runs the scenario's ops on the Rust side, shaped like `ytm_replay.js`'s output.
async fn run_rust(scenario: &Value, repo_root: &str) -> Value {
    let host = Arc::new(MockHost::new(scenario, repo_root));
    let ytm = Ytm::default();
    let logs_before = snowify::logger::LOGS.lock().unwrap().entries().len();
    let mut results = Vec::new();
    for op in scenario["ops"].as_array().unwrap() {
        let ch = op[0].as_str().unwrap();
        if ch == "$init" {
            if let Err(e) = ytmusic::init(&ytm, &*host).await {
                snowify::logger::error(&["[YTMusic] Initialization failed (will retry on first use):".into(), snowify::logger::LogArg::from_js(&e.message())]);
            }
            results.push(Value::Null);
            continue;
        }
        let args: Vec<JsValue> = op[1].as_array().unwrap().iter().map(|a| JsValue::decode(a).unwrap()).collect();
        results.push(match dispatch(&ytm, &host, ch, &args).await.unwrap_or_else(|| panic!("no handler {ch}")) {
            Ok(v) => json!({ "ok": snowify::ipc::normalize_identity(&v).encode() }),
            Err(t) => json!({ "err": JsValue::String(t.rejection_message(ch)).encode() }),
        });
    }
    let logs: Vec<Value> = snowify::logger::LOGS.lock().unwrap().entries()[logs_before..].iter().map(|e| json!([e.level.encode(), e.msg])).collect();
    json!({ "results": results, "logs": logs, "requests": *host.requests.lock().unwrap() })
}

/// The JS stack text axios puts in a logged AxiosError can't exist in Rust (see PORT.md, owner
/// sign-off): compare everything up to the first frame.
fn strip_stacks(v: &Value) -> Value {
    match v {
        Value::Array(rows) => Value::Array(rows.iter().map(strip_stacks).collect()),
        Value::String(s) if s.contains("\"stack\":\"") => {
            let re = regex::Regex::new(r#""stack":"((?:[^"\\]|\\.)*?)(\\n    at (?:[^"\\]|\\.)*)?""#).unwrap();
            Value::String(re.replace_all(s, r#""stack":"$1""#).into_owned())
        }
        v => v.clone(),
    }
}

// ─── InnerTube fixture builders ───

const SEARCH: &str = "https://music.youtube.com/youtubei/v1/search";
const BROWSE: &str = "https://music.youtube.com/youtubei/v1/browse";
const NEXT: &str = "https://music.youtube.com/youtubei/v1/next";
const PLAYER: &str = "https://music.youtube.com/youtubei/v1/player";

fn ok(data: Value) -> Value {
    json!({ "status": 200, "statusText": "OK", "headers": { "content-type": "application/json; charset=UTF-8" }, "data": data.to_string() })
}
fn raw(status: u16, data: &str, headers: Value) -> Value {
    json!({ "status": status, "statusText": "", "headers": headers, "data": data })
}
fn over(method: &str, url: &str, body: Option<&str>, response: Value) -> Value {
    json!({ "method": method, "url": url, "bodyIncludes": body, "response": response })
}
fn run(text: &str) -> Value {
    json!({ "text": text })
}
fn nav_run(text: &str, page: &str, id: &str) -> Value {
    json!({ "text": text, "navigationEndpoint": { "browseEndpoint": { "browseId": id, "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": page } } } } })
}
fn thumbs(list: Value) -> Value {
    json!({ "musicThumbnailRenderer": { "thumbnail": { "thumbnails": list } } })
}
/// A search-shelf song row: `kind` is the first subtitle run ("Song", "Video", …).
fn row(video: &str, title: &str, kind: &str, rest: Vec<Value>, thumbnail: Value) -> Value {
    let mut sub = vec![run(kind), run(" \u{2022} ")];
    sub.extend(rest);
    json!({ "musicResponsiveListItemRenderer": {
        "thumbnail": thumbnail,
        "flexColumns": [
            { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": title, "navigationEndpoint": { "watchEndpoint": { "videoId": video, "watchEndpointMusicSupportedConfigs": { "watchEndpointMusicConfig": { "musicVideoType": "MUSIC_VIDEO_TYPE_ATV" } } } } }] } } },
            { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": sub } } },
        ],
        "playlistItemData": { "videoId": video },
        "overlay": { "musicItemThumbnailOverlayRenderer": { "content": { "musicPlayButtonRenderer": { "playNavigationEndpoint": { "watchEndpoint": { "videoId": video } } } } } },
    } })
}
fn shelf(rows: Vec<Value>) -> Value {
    json!({ "responseContext": {}, "contents": { "tabbedSearchResultsRenderer": { "tabs": [{ "tabRenderer": { "content": { "sectionListRenderer": { "contents": [{ "musicShelfRenderer": { "contents": rows } }] } } } }] } } })
}
fn qbody(q: &str) -> String {
    format!("\"query\":\"{q}\"")
}
fn op(ch: &str, args: Vec<Value>) -> Value {
    json!([ch, args.into_iter().map(|a| JsValue::from(&a).encode()).collect::<Vec<_>>()])
}

/// Scenarios on top of the recorded session (its GET / and config), each overriding a few
/// requests.
fn edge_scenarios(rec: &str) -> Vec<(&'static str, Value)> {
    let good_thumbs = thumbs(json!([{ "url": "https://lh3.googleusercontent.com/abc=w60-h60-l90-rj", "width": 60, "height": 60 }, { "url": "https://lh3.googleusercontent.com/abc=w120-h120-l90-rj", "width": 120, "height": 120 }]));
    let artist = nav_run("Daft Punk", "MUSIC_PAGE_TYPE_ARTIST", "UCRr1xG_2WIDs18a6cIiCxeA");
    let album = nav_run("Discovery", "MUSIC_PAGE_TYPE_ALBUM", "MPREb_x");
    let mut out = Vec::new();
    let sc = |overrides: Vec<Value>, ops: Vec<Value>, proxy: Value| {
        let mut all = vec![json!(["$init"])];
        all.extend(ops);
        json!({ "userData": "/mock/ud", "recording": rec, "overrides": overrides, "resolveProxy": proxy, "ops": all })
    };
    out.push(("http and parse errors", sc(
        vec![
            over("POST", SEARCH, Some(&qbody("e500")), raw(500, "{\"error\":{\"code\":500}}", json!({ "content-type": "application/json" }))),
            over("POST", SEARCH, Some(&qbody("e404html")), raw(404, "<html>gone</html>", json!({ "content-type": "text/html" }))),
            over("POST", SEARCH, Some(&qbody("badjson")), raw(200, "{not json", json!({ "content-type": "application/json" }))),
            over("POST", SEARCH, Some(&qbody("net")), json!({ "error": { "message": "read ECONNRESET", "code": "ECONNRESET" } })),
            over("POST", SEARCH, Some(&qbody("prim")), ok(json!(42))),
            over("POST", SEARCH, Some(&qbody("noctx")), ok(json!({ "contents": {} }))),
            over("POST", SEARCH, Some(&qbody("arr")), ok(json!([1, 2]))),
        ],
        vec![
            op("yt:search", vec![json!("e500"), json!(true)]),
            op("yt:search", vec![json!("e404html"), json!(false)]),
            op("yt:search", vec![json!("badjson"), json!(false)]),
            op("yt:searchVideos", vec![json!("net")]),
            op("yt:searchAlbums", vec![json!("prim")]),
            op("yt:search", vec![json!("noctx"), json!(false)]),
            op("yt:searchArtists", vec![json!("noctx")]),
            op("yt:searchSuggestions", vec![json!("noctx")]),
            op("yt:searchPlaylists", vec![json!("arr")]),
        ],
        json!("DIRECT"),
    )));
    out.push(("zod mismatches and odd rows", sc(
        vec![over("POST", SEARCH, Some(&qbody("zod")), ok(shelf(vec![
            row("vid00000001", "Plain Song", "Song", vec![artist.clone(), run(" \u{2022} "), album.clone(), run(" \u{2022} "), run("3:05")], good_thumbs.clone()),
            row("vid00000002", "No Width", "Song", vec![artist.clone(), run(" \u{2022} "), run("1:02:03")], thumbs(json!([{ "url": "u", "height": 1, "extra": true }]))),
            row("vid00000003", "Odd Duration", "Video", vec![run("Someone"), run(" \u{2022} "), run("12:3")], thumbs(json!("str"))),
            row("vid00000004", "Album Year", "Album", vec![artist.clone(), run(" \u{2022} "), run("19x9")], good_thumbs.clone()),
            row("vid00000005", "Proto", "constructor", vec![], good_thumbs.clone()),
            row("vid00000006", "Single", "Single", vec![artist.clone(), run(" \u{2022} "), run("2001")], good_thumbs.clone()),
            row("vid00000007", "Artist Row", "Artist", vec![run("1M subscribers")], good_thumbs.clone()),
            row("vid00000008", "Playlist Row", "Playlist", vec![run("YouTube Music")], good_thumbs.clone()),
            row("vid00000009", "Unknown", "Station", vec![], good_thumbs.clone()),
        ])))],
        vec![
            op("yt:search", vec![json!("zod"), json!(false)]),
            op("yt:search", vec![json!("zod"), json!(true)]),
            op("yt:searchAlbums", vec![json!("zod")]),
            op("yt:searchVideos", vec![json!("zod")]),
            op("yt:searchPlaylists", vec![json!("zod")]),
            op("spotify:matchTrack", vec![json!("Plain Song (Live)"), json!("Daft Punk")]),
        ],
        json!("DIRECT"),
    )));
    out.push(("throwing search type", sc(
        vec![over("POST", SEARCH, Some(&qbody("proto")), ok(shelf(vec![row("vid0000000a", "X", "__proto__", vec![], good_thumbs.clone())])))],
        vec![op("yt:search", vec![json!("proto"), json!(false)])],
        json!("DIRECT"),
    )));
    out.push(("odd arguments", sc(
        vec![],
        vec![
            op("yt:getTrackInfo", vec![json!(12345)]),
            op("yt:getTrackInfo", vec![json!("@@@@@@@@@@@")]),
            op("yt:getPlaylistVideos", vec![json!(7)]),
            op("yt:setCountry", vec![json!({ "a": 1 })]),
            json!(["yt:search", [{ "$": "b", "v": "5" }, false]]),
            op("yt:search", vec![json!({ "$": "b", "v": "5" }), json!(false)]),
            op("yt:setCountry", vec![json!("")]),
            op("spotify:matchTrack", vec![json!(5), json!("x")]),
        ],
        json!("DIRECT"),
    )));
    for (name, p) in [
        ("http proxy", json!("PROXY 127.0.0.1:8080; DIRECT")),
        ("socks5 proxy", json!("SOCKS5 10.0.0.1:1080")),
        ("socks v4 as http", json!("SOCKS 1.2.3.4:1080")),
        ("bad proxy host", json!("PROXY bad host:1")),
        ("proxy lookup fails", json!({ "error": "boom" })),
        ("proxy without port part", json!("PROXY")),
    ] {
        out.push((name, sc(vec![], vec![op("yt:search", vec![json!("daft punk"), json!(true)])], p)));
    }
    out.push(("no instance before init", json!({ "userData": "/mock/ud", "recording": rec, "overrides": [], "resolveProxy": "DIRECT", "ops": [op("spotify:matchTrack", vec![json!("a"), json!("b")])] })));
    out.push(("init failures", json!({ "userData": "/mock/ud", "recording": rec, "resolveProxy": "DIRECT",
        "overrides": [over("GET", "https://music.youtube.com/", None, raw(200, "<html>no config</html>", json!({ "set-cookie": ["a=1; Path=/", "b=2; Domain=youtube.com; Path=/deep; Max-Age=100", "c=3; Domain=.music.youtube.com", "__Secure-x=1", "d=4; Expires=Wed, 01 Jan 2020 00:00:00 GMT"] })))],
        "ops": [json!(["$init"]), op("yt:search", vec![json!("x"), json!(true)]), op("yt:explore", vec![])] })));
    out.push(("cookie in a foreign domain", json!({ "userData": "/mock/ud", "recording": rec, "resolveProxy": "DIRECT",
        "overrides": [over("GET", "https://music.youtube.com/", None, raw(200, "<html></html>", json!({ "set-cookie": ["z=1; Domain=example.org"] })))],
        "ops": [json!(["$init"]), op("yt:charts", vec![])] })));
    out.push(("html that parses as JSON", json!({ "userData": "/mock/ud", "recording": rec, "resolveProxy": "DIRECT",
        "overrides": [over("GET", "https://music.youtube.com/", None, raw(200, "123", json!({})))],
        "ops": [json!(["$init"]), op("yt:browseMood", vec![json!("x"), json!("y")])] })));
    // A "Single" search row reaches the album parser, whose year lookup throws on a number.
    let single_row = row("vid0000000s", "Single Thing", "Single", vec![artist.clone(), run(" \u{2022} "), json!({ "text": 2020 })], good_thumbs.clone());
    out.push(("single search row throws in the album parser", sc(
        vec![over("POST", SEARCH, Some(&qbody("single")), ok(shelf(vec![single_row])))],
        vec![op("yt:search", vec![json!("single"), json!(false)])],
        json!("DIRECT"),
    )));
    // A top-level renderer plus a nested one: for the last key the walk stops at the first
    // match, so only the top-level row is returned (not the one under `contents`).
    let direct_row = row("vid0000000d", "Direct Song", "Song", vec![artist.clone(), run(" \u{2022} "), run("2:45")], good_thumbs.clone());
    let nested_row = row("vid0000000n", "Nested Song", "Song", vec![artist.clone(), run(" \u{2022} "), run("3:05")], good_thumbs.clone());
    let mut direct_page = shelf(vec![nested_row]);
    direct_page["musicResponsiveListItemRenderer"] = direct_row["musicResponsiveListItemRenderer"].clone();
    out.push(("renderer at the root stops the walk", sc(
        vec![over("POST", SEARCH, Some(&qbody("direct")), ok(direct_page))],
        vec![op("yt:search", vec![json!("direct"), json!(false)])],
        json!("DIRECT"),
    )));
    // The same key nested inside the matched value: stopping at the root match must not also
    // collect the nested renderer (the walk would report two songs).
    let mut dead_end_row = row("vid0000000e", "Dead End Song", "Song", vec![artist.clone(), run(" \u{2022} "), run("2:30")], good_thumbs.clone());
    let hidden_row = row("vid0000000f", "Hidden Song", "Song", vec![artist.clone(), run(" \u{2022} "), run("4:00")], good_thumbs.clone());
    dead_end_row["musicResponsiveListItemRenderer"]["musicResponsiveListItemRenderer"] = hidden_row["musicResponsiveListItemRenderer"].clone();
    let dead_end_page = json!({
        "responseContext": {},
        "musicResponsiveListItemRenderer": dead_end_row["musicResponsiveListItemRenderer"].clone()
    });
    out.push(("a dead-end renderer does not yield its nested renderer", sc(
        vec![over("POST", SEARCH, Some(&qbody("deadend")), ok(dead_end_page))],
        vec![op("yt:search", vec![json!("deadend"), json!(false)])],
        json!("DIRECT"),
    )));
    // The artist page's top-video carousel is the third carousel.
    let artist_page = json!({
        "responseContext": {},
        "header": { "musicVisualHeaderRenderer": {
            "title": { "runs": [{ "text": "Daft Punk" }] },
            "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [
                { "url": "https://lh3.googleusercontent.com/avatar=w120-h120-l90-rj", "width": 120, "height": 120 }
            ] } } }
        } },
        "contents": { "singleColumnBrowseResultsRenderer": { "tabs": [{ "tabRenderer": { "content": { "sectionListRenderer": { "contents": [
            { "musicShelfRenderer": { "contents": [] } },
            { "musicCarouselShelfRenderer": { "contents": [] } },
            { "musicCarouselShelfRenderer": { "contents": [] } },
            { "musicCarouselShelfRenderer": { "contents": [{ "musicTwoRowItemRenderer": {
                "title": { "runs": [{ "text": "Live Video" }] },
                "navigationEndpoint": { "watchEndpoint": { "videoId": "vid0000000v" } },
                "thumbnailRenderer": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [
                    { "url": "https://i.ytimg.com/vi/vid0000000v/hq.jpg", "width": 120, "height": 120 }
                ] } } }
            } }] } },
            { "musicCarouselShelfRenderer": { "contents": [] } },
            { "musicCarouselShelfRenderer": { "contents": [] } }
        ] } } } }] } }
    });
    out.push(("artist top video", sc(
        vec![over("POST", BROWSE, Some("\"browseId\":\"UCartist\""), ok(artist_page))],
        vec![op("yt:artistInfo", vec![json!("UCartist")])],
        json!("DIRECT"),
    )));
    // An empty playNavigationEndpoint id: the video id comes from the thumbnail URL.
    let playlist_video = json!({ "musicResponsiveListItemRenderer": {
        "flexColumns": [
            { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{
                "text": "Thumb Id Video",
                "navigationEndpoint": { "watchEndpoint": { "videoId": "vid0000000t",
                    "watchEndpointMusicSupportedConfigs": { "watchEndpointMusicConfig": { "musicVideoType": "MUSIC_VIDEO_TYPE_ATV" } } } }
            }] } } },
            { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{
                "text": "Some Artist",
                "navigationEndpoint": { "browseEndpoint": { "browseId": "UCsome",
                    "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ARTIST" } } } }
            }] } } }
        ],
        "fixedColumns": [{ "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "3:05" }] } } }],
        "playNavigationEndpoint": { "watchEndpoint": { "videoId": "" } },
        "thumbnails": [{ "url": "https://i.ytimg.com/vi/abcdefghijk/hq.jpg", "width": 100, "height": 100 }]
    } });
    let playlist_page = json!({
        "responseContext": {},
        "continuation": [[]],
        "contents": { "singleColumnBrowseResultsRenderer": { "tabs": [{ "tabRenderer": { "content": { "sectionListRenderer": { "contents": [
            { "musicPlaylistShelfRenderer": { "contents": [playlist_video] } }
        ] } } } }] } }
    });
    out.push(("playlist video id from its thumbnail url", sc(
        vec![over("POST", BROWSE, Some("\"browseId\":\"pltest\""), ok(playlist_page))],
        vec![op("yt:getPlaylistVideos", vec![json!("pltest")])],
        json!("DIRECT"),
    )));
    // The overlay has no playlistId, so the thumbnail overlay's (non-string) id is used and
    // the AlbumDetailed check logs it (a blank fallback would pass the string check silently).
    let mut album_row = row("vid0000000a", "Overlay Album", "Album", vec![artist.clone(), run(" \u{2022} "), run("2001")], good_thumbs.clone());
    album_row["musicResponsiveListItemRenderer"]["thumbnailOverlay"] = json!({ "playlistId": 12345 });
    out.push(("album playlist id falls back to the thumbnail overlay", sc(
        vec![over("POST", SEARCH, Some(&qbody("overlay")), ok(shelf(vec![album_row])))],
        vec![op("yt:searchAlbums", vec![json!("overlay")])],
        json!("DIRECT"),
    )));
    let _ = (BROWSE, NEXT, PLAYER);
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ytmusic_matches_source() {
    let rec = recording();
    assert!(std::path::Path::new(&rec).exists(), "no recording at {rec}: run parity/node/ytm_record.js first");
    let repo_root = common::root().join("..").canonicalize().unwrap().display().to_string();
    let session: Value = {
        let gz = std::fs::read(&rec).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&gz[..]), &mut text).unwrap();
        serde_json::from_str(&text).unwrap()
    };
    let mut ops = vec![json!(["$init"])];
    ops.extend(session["ops"].as_array().unwrap().iter().cloned());

    let mut scenarios = vec![("recorded session", json!({ "userData": "/mock/ud", "recording": rec, "overrides": [], "resolveProxy": "DIRECT", "ops": ops }))];
    scenarios.extend(edge_scenarios(&rec));
    let mut d = Diff::default();
    for (name, scenario) in scenarios {
        let src = common::run_source_with("ytm_replay.js", &scenario, |c| {
            c.env("TZ", "America/New_York");
        });
        // SAFETY: set before the Rust side reads the clock's zone; nothing else runs then.
        unsafe { std::env::set_var("TZ", "America/New_York") };
        let rust = run_rust(&scenario, &repo_root).await;
        if std::env::var("YTM_DUMP").as_deref() == Ok(name) {
            for side in [&src, &rust] {
                eprintln!("DUMP ===");
                for r in side["results"].as_array().unwrap() {
                    eprintln!("DUMP res {}", &r.to_string()[..r.to_string().len().min(160)]);
                }
                for l in side["logs"].as_array().unwrap() {
                    eprintln!("DUMP log {}", &l.to_string()[..l.to_string().len().min(160)]);
                }
                for q in side["requests"].as_array().unwrap() {
                    let b = q[3].as_str().unwrap_or("");
                    let tail = &b[b.len().saturating_sub(70)..];
                    eprintln!("DUMP req {} {} ...{}", q[0], &q[1].as_str().unwrap()[..q[1].as_str().unwrap().len().min(60)], tail);
                }
            }
        }
        for key in ["results", "logs", "requests"] {
            let (a, b) = (strip_stacks(&src[key]), strip_stacks(&rust[key]));
            let n = a.as_array().unwrap().len().max(b.as_array().unwrap().len());
            for i in 0..n {
                d.check(format_args!("{name} {key}[{i}]"), a.get(i).unwrap_or(&Value::Null), b.get(i).unwrap_or(&Value::Null));
            }
        }
    }
    d.finish("ytmusic");
}
