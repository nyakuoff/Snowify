//! Port of `src/main/ytmusic.js`: YouTube Music browsing over the ytmusic-api port
//! (`crate::ytm`), with the handlers' own raw-response walks. Every handler catches and
//! logs (`console.error(label, err)`) as the source does.

use crate::{
    host::Host,
    ipc::{self, HandlerResult, JsString, JsValue},
    js,
    jsv::{self, J, chain, get, iter, opt, or},
    logger::{self, LogArg},
    ytm::{Response, Ytm, YtmError, parsers},
};
use regex::Regex;
use std::{collections::HashMap, sync::LazyLock};

// ─── Helpers (exported by the source too) ───

/// `formatDuration(seconds)`.
pub fn format_duration(seconds: &JsValue) -> JsValue {
    let n = match seconds {
        JsValue::Undefined | JsValue::Null => return "".into(),
        JsValue::Object(_) | JsValue::Map(_) | JsValue::Set(_) => f64::NAN,
        v => ipc::to_number(v).unwrap_or(f64::NAN),
    };
    if n.is_nan() || n <= 0.0 {
        return "".into();
    }
    let m = js::math_floor(n / 60.0);
    let s = js::number_to_string(js::math_floor(n % 60.0));
    let pad = if s.encode_utf16().count() < 2 { "0".repeat(2 - s.encode_utf16().count()) } else { String::new() };
    format!("{}:{pad}{s}", js::number_to_string(m)).into()
}

/// `getBestThumbnail(thumbnails)`.
pub fn best_thumbnail(thumbnails: &JsValue) -> J<JsValue> {
    if !opt(thumbnails, "length").truthy() {
        return Ok("".into());
    }
    // `[...thumbnails]`: arrays and strings spread; anything else isn't iterable.
    let items = iter(thumbnails)?;
    let width = |v: &JsValue| -> J<JsValue> { Ok(or(get(v, "width")?, JsValue::Number(0.0))) };
    let sorted = jsv::v8_sort(items, &|a, b| Ok(jsv::minus(&width(b)?, &width(a)?)))?;
    let first = sorted.into_iter().next().unwrap_or_default();
    Ok(or(opt(&first, "url"), "".into()))
}

/// `/=(?:w\d+-h\d+|s\d+|p-w\d+).*$/` (first match; `.` doesn't cross line ends).
fn size_suffix_at(url: &str) -> Option<usize> {
    let term = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    url.match_indices('=').map(|(i, _)| i).find(|&i| {
        let rest = &url[i + 1..];
        let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
        let alt = if let Some(r) = rest.strip_prefix('w') {
            let d = digits(r);
            d > 0 && r[d..].strip_prefix("-h").is_some_and(|h| digits(h) > 0)
        } else if let Some(r) = rest.strip_prefix("p-w") {
            digits(r) > 0
        } else if let Some(r) = rest.strip_prefix('s') {
            digits(r) > 0
        } else {
            false
        };
        alt && !url[i..].contains(term)
    })
}

/// `getSquareThumbnail(thumbnails, size)`.
pub fn square_thumbnail(thumbnails: &JsValue, size: u32) -> J<JsValue> {
    let url = best_thumbnail(thumbnails)?;
    if !url.truthy() {
        return Ok("".into());
    }
    let s = jsv::string(&url)?;
    if s.contains("lh3.googleusercontent.com")
        && let Some(i) = size_suffix_at(&s)
    {
        return Ok(format!("{}=w{size}-h{size}-l90-rj", &s[..i]).into());
    }
    Ok(url)
}

static ARTIST_SPLIT: LazyLock<Regex> = LazyLock::new(|| {
    let ws = r"[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";
    Regex::new(&format!(",{ws}*|{ws}*&{ws}*")).expect("regex")
});

/// `parseArtistsFromRuns(runs)`.
pub fn artists_from_runs(runs: &JsValue) -> J<Vec<JsValue>> {
    if !opt(runs, "length").truthy() {
        return Ok(Vec::new());
    }
    let list = jsv::arr(runs)?;
    let mut artist_runs = Vec::new();
    for r in &list {
        let nav = get(r, "navigationEndpoint")?;
        let page = chain(&nav, &["browseEndpoint", "browseEndpointContextSupportedConfigs", "browseEndpointContextMusicConfig", "pageType"]);
        if jsv::strict_eq(&page, "MUSIC_PAGE_TYPE_ARTIST") {
            artist_runs.push(r.clone());
        }
    }
    if !artist_runs.is_empty() {
        return artist_runs
            .iter()
            .map(|r| {
                let id = get(&get(&get(r, "navigationEndpoint")?, "browseEndpoint")?, "browseId")?;
                Ok(JsValue::object([("name", get(r, "text")?), ("id", id)]))
            })
            .collect();
    }
    if !list.is_empty() && !get(&list[0], "navigationEndpoint")?.truthy() {
        let text = jsv::string(&jsv::join_texts(&list, "")?)?;
        let artist_text = match text.find(" \u{2022} ") {
            Some(i) => &text[..i],
            None => &text[..],
        };
        return Ok(ARTIST_SPLIT
            .split(artist_text)
            .filter(|n| !n.is_empty())
            .map(|n| JsValue::object([("name", JsValue::from(js::trim(n))), ("id", JsValue::Null)]))
            .collect());
    }
    Ok(Vec::new())
}

/// `buildArtistFields(artists)`: `{ artist, artistId, artists }`. `artists` is an array value,
/// possibly marked shared: the field is that same array.
pub fn artist_fields(artists: &JsValue) -> J<Vec<(&'static str, JsValue)>> {
    let list = jsv::arr(artists)?;
    if list.is_empty() {
        return Ok(vec![("artist", "Unknown Artist".into()), ("artistId", JsValue::Null), ("artists", jsv::empty_array())]);
    }
    let names: Vec<JsValue> = list.iter().map(|a| get(a, "name")).collect::<J<_>>()?;
    Ok(vec![("artist", jsv::join(&names, ", ")), ("artistId", or(get(&list[0], "id")?, JsValue::Null)), ("artists", artists.clone())])
}

/// A fresh array (referenced once).
fn fresh(v: Vec<JsValue>) -> JsValue {
    JsValue::Array(v)
}

/// The `{ artist, artistId, artists }` fields from a library result's own `artist`.
fn own_artist_fields(item: &JsValue) -> J<Vec<(&'static str, JsValue)>> {
    let a = get(item, "artist")?;
    let artists = if a.truthy() {
        JsValue::Array(vec![JsValue::object([("name", get(&a, "name")?), ("id", or(get(&a, "artistId")?, JsValue::Null))])])
    } else {
        jsv::empty_array()
    };
    Ok(vec![("artist", or(opt(&a, "name"), "Unknown Artist".into())), ("artistId", or(opt(&a, "artistId"), JsValue::Null)), ("artists", artists)])
}

fn url_for(id: &JsValue) -> JsValue {
    format!("https://music.youtube.com/watch?v={}", ipc::to_js_string(id)).into()
}

fn duration_ms(d: &JsValue) -> JsValue {
    JsValue::Number(if d.truthy() { js::math_round(ipc::to_number(d).unwrap_or(f64::NAN) * 1000.0) } else { 0.0 })
}

fn object(props: Vec<(&str, JsValue)>) -> JsValue {
    let mut o = JsValue::Object(Vec::new());
    for (k, v) in props {
        ipc::define(&mut o, k.into(), v);
    }
    o
}

/// `mapSongToTrack(song, artists)`.
pub fn map_song_to_track(song: &JsValue, artists: Option<&JsValue>) -> J<JsValue> {
    let fields = match artists {
        Some(a) => artist_fields(a)?,
        None => own_artist_fields(song)?,
    };
    let id = get(song, "videoId")?;
    let duration = get(song, "duration")?;
    let mut props = vec![("id", id.clone()), ("title", or(get(song, "name")?, "Unknown".into()))];
    props.extend(fields);
    props.extend([
        ("album", or(chain(song, &["album", "name"]), JsValue::Null)),
        ("albumId", or(chain(song, &["album", "albumId"]), JsValue::Null)),
        ("thumbnail", square_thumbnail(&get(song, "thumbnails")?, 226)?),
        ("duration", format_duration(&duration)),
        ("durationMs", duration_ms(&duration)),
        ("url", url_for(&id)),
    ]);
    Ok(object(props))
}

/// `rawArtistsMap`: a plain object keyed by `String(videoId)`, each value one array object.
type ArtistMap = HashMap<String, JsValue>;

/// The shelf walk `yt:search` does on its raw response.
fn raw_artists(raw: &Option<JsValue>) -> J<ArtistMap> {
    let mut map = ArtistMap::new();
    let Some(raw) = raw else { return Ok(map) };
    let shelves = or(
        chain(raw, &["contents", "tabbedSearchResultsRenderer", "tabs", "0", "tabRenderer", "content", "sectionListRenderer", "contents"]),
        jsv::empty_array(),
    );
    for s in iter(&shelves)? {
        for entry in iter(&or(chain(&s, &["musicShelfRenderer", "contents"]), jsv::empty_array()))? {
            let r = opt(&entry, "musicResponsiveListItemRenderer");
            if !r.truthy() {
                continue;
            }
            let cols = or(get(&r, "flexColumns")?, jsv::empty_array());
            let video_id = chain(&opt(&cols, "0"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs", "0", "navigationEndpoint", "watchEndpoint", "videoId"]);
            if !video_id.truthy() {
                continue;
            }
            let all_runs = or(chain(&opt(&cols, "1"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs"]), jsv::empty_array());
            let runs = jsv::arr(&all_runs)?;
            let mut dot = None;
            for (i, run) in runs.iter().enumerate() {
                if jsv::strict_eq(&get(run, "text")?, " \u{2022} ") {
                    dot = Some(i);
                    break;
                }
            }
            let artists = artists_from_runs(&match dot {
                Some(i) => JsValue::Array(runs[..i].to_vec()),
                None => all_runs.clone(),
            })?;
            if !artists.is_empty() {
                map.insert(ipc::to_js_string(&video_id), ipc::shared(fresh(artists)));
            }
        }
    }
    Ok(map)
}

// ─── Init ───

/// `initYTMusic(ctx)`: the instance, the system proxy, `initialize()`.
pub async fn init<H: Host>(ytm: &Ytm, host: &H) -> Result<(), YtmError> {
    match host.resolve_proxy("https://music.youtube.com/").await {
        Ok(s) if !s.is_empty() && s != "DIRECT" => {
            let first = js::trim(s.split(';').next().unwrap_or_default()).to_owned();
            let mut parts = first.split(|c: char| js::trim(&c.to_string()).is_empty()).filter(|p| !p.is_empty());
            // `firstProxy.split(/\s+/, 2)`: a leading separator makes the first part "".
            let leading_ws = first.chars().next().is_some_and(|c| js::trim(&c.to_string()).is_empty());
            let (ty, host_port) = if leading_ws { (String::new(), parts.next().map(str::to_owned)) } else { (parts.next().unwrap_or_default().to_owned(), parts.next().map(str::to_owned)) };
            if let Some(hp) = host_port {
                let url = if ty == "SOCKS5" || ty == "SOCKS4" { format!("{}://{hp}", ty.to_lowercase()) } else { format!("http://{hp}") };
                if url::Url::parse(&url).is_ok() {
                    *ytm.proxy.lock().unwrap_or_else(|e| e.into_inner()) = Some(url);
                    logger::log(&[format!("[YTMusic] Using system proxy: {first}").as_str().into()]);
                } else {
                    logger::warn(&["[YTMusic] Could not resolve system proxy:".into(), "Invalid URL".into()]);
                }
            }
        }
        Ok(_) => {}
        Err(m) => logger::warn(&["[YTMusic] Could not resolve system proxy:".into(), m.as_str().into()]),
    }
    ytm.initialize(host).await?;
    *ytm.ready.lock().unwrap_or_else(|e| e.into_inner()) = true;
    Ok(())
}

/// `ensureYTMusic(ctx)`.
pub async fn ensure<H: Host>(ytm: &Ytm, host: &H) -> Result<(), YtmError> {
    let ok = *ytm.ready.lock().unwrap_or_else(|e| e.into_inner())
        && ytm.state.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| ipc::get_prop(&s.config, "INNERTUBE_API_KEY").unwrap_or_default().truthy());
    if ok {
        return Ok(());
    }
    *ytm.ready.lock().unwrap_or_else(|e| e.into_inner()) = false;
    init(ytm, host).await
}

fn fail<T>(label: &str, e: &YtmError, fallback: T) -> T {
    logger::error(&[label.into(), LogArg::from_js(&e.log_value())]);
    fallback
}

fn plain<T>(r: J<T>) -> Result<T, YtmError> {
    r.map_err(|()| YtmError::Plain(String::new()))
}

// ─── Library calls ───

const SONGS: &str = "Eg-KAQwIARAAGAAgACgAMABqChAEEAMQCRAFEAo%3D";

/// `searchSongs(query)`.
pub async fn search_songs<H: Host>(ytm: &Ytm, host: &H, query: &JsValue) -> Result<Vec<JsValue>, YtmError> {
    let data = ytm.construct(host, "search", vec![("query", query.clone()), ("params", SONGS.into())], vec![]).await?.value();
    let items = crate::ytm::traverse::traverse_list(&data, &["musicResponsiveListItemRenderer"])?;
    Ok(items.iter().map(parsers::song_search_result).collect::<Result<_, _>>()?)
}

/// `.catch(() => null)` on a raw request.
async fn raw_or_null<H: Host>(ytm: &Ytm, host: &H, body: Vec<(&str, JsValue)>) -> Option<JsValue> {
    ytm.construct(host, "search", body, vec![]).await.ok().map(Response::value)
}

// ─── Handlers ───

/// `yt:search(query, musicOnly)`.
pub async fn search<H: Host>(ytm: &Ytm, host: &H, query: &JsValue, music_only: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        if music_only.truthy() {
            let raw_body = vec![("query", query.clone()), ("params", JsValue::from("EgWKAQIIAWoOEAMQBBAJEAoQBRAREBU%3D"))];
            let (songs, raw) = futures_util::join!(search_songs(ytm, host, query), raw_or_null(ytm, host, raw_body));
            let songs = songs?;
            let map = plain(raw_artists(&raw))?;
            let mut out = Vec::new();
            for s in songs.iter() {
                if !plain(get(s, "videoId"))?.truthy() {
                    continue;
                }
                let artists = map.get(&ipc::to_js_string(&plain(get(s, "videoId"))?));
                out.push(plain(map_song_to_track(s, artists))?);
            }
            return Ok(JsValue::Array(out));
        }
        let general = async {
            let data = ytm.construct(host, "search", vec![("query", query.clone()), ("params", JsValue::Null)], vec![]).await?.value();
            let items = crate::ytm::traverse::traverse_list(&data, &["musicResponsiveListItemRenderer"])?;
            let parsed = items.iter().map(parsers::search_result).collect::<Result<Vec<_>, _>>()?;
            Ok::<_, YtmError>(parsed.into_iter().filter(JsValue::truthy).collect::<Vec<_>>())
        };
        let (results, raw) = futures_util::join!(general, raw_or_null(ytm, host, vec![("query", query.clone())]));
        let results = results?;
        let map = plain(raw_artists(&raw))?;
        let mut out = Vec::new();
        for r in &results {
            let ty = opt(r, "type");
            if !(jsv::strict_eq(&ty, "SONG") || jsv::strict_eq(&ty, "VIDEO")) || !opt(r, "videoId").truthy() {
                continue;
            }
            let id = plain(get(r, "videoId"))?;
            let fields = match map.get(&ipc::to_js_string(&id)) {
                Some(a) => plain(artist_fields(a))?,
                None => plain(own_artist_fields(r))?,
            };
            let duration = plain(get(r, "duration"))?;
            let mut props = vec![("id", id.clone()), ("title", or(plain(get(r, "name"))?, "Unknown".into()))];
            props.extend(fields);
            props.extend([
                ("thumbnail", plain(square_thumbnail(&plain(get(r, "thumbnails"))?, 226))?),
                ("duration", format_duration(&duration)),
                ("durationMs", duration_ms(&duration)),
                ("url", url_for(&id)),
            ]);
            out.push(object(props));
        }
        Ok(JsValue::Array(out))
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Search error:", &e, jsv::empty_array())))
}

/// `yt:searchSuggestions(query)`.
pub async fn search_suggestions<H: Host>(ytm: &Ytm, host: &H, query: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let raw = ytm.construct(host, "music/get_search_suggestions", vec![("input", query.clone())], vec![]).await?.value();
        plain((|| {
            let sections = jsv::nullish_or(opt(&raw, "contents"), jsv::empty_array());
            let (mut text_suggestions, mut direct) = (Vec::new(), Vec::new());
            for section in iter(&sections)? {
                let items = jsv::nullish_or(chain(&section, &["searchSuggestionsSectionRenderer", "contents"]), jsv::empty_array());
                for item in iter(&items)? {
                    let ssr = get(&item, "searchSuggestionRenderer")?;
                    if ssr.truthy() {
                        let runs = jsv::nullish_or(chain(&ssr, &["suggestion", "runs"]), jsv::empty_array());
                        let text = jsv::join_texts(&jsv::arr(&runs)?, "")?;
                        if text.truthy() {
                            text_suggestions.push(text);
                        }
                        continue;
                    }
                    let renderer = get(&item, "musicResponsiveListItemRenderer")?;
                    if !renderer.truthy() {
                        continue;
                    }
                    let nav = get(&renderer, "navigationEndpoint")?;
                    let thumbs = chain(&renderer, &["thumbnail", "musicThumbnailRenderer", "thumbnail", "thumbnails"]);
                    let thumbnail = if opt(&thumbs, "length").truthy() {
                        let len = ipc::to_number(&opt(&thumbs, "length")).unwrap_or(0.0);
                        get(&get(&thumbs, &js::number_to_string(len - 1.0))?, "url")?
                    } else {
                        "".into()
                    };
                    let cols = jsv::nullish_or(get(&renderer, "flexColumns")?, jsv::empty_array());
                    let title_runs = jsv::nullish_or(chain(&opt(&cols, "0"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs"]), jsv::empty_array());
                    let subtitle_runs = jsv::nullish_or(chain(&opt(&cols, "1"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs"]), jsv::empty_array());
                    let title = jsv::join_texts(&jsv::arr(&title_runs)?, "")?;
                    let subtitle = jsv::join_texts(&jsv::arr(&subtitle_runs)?, "")?;
                    if opt(&nav, "browseEndpoint").truthy() {
                        let be = get(&nav, "browseEndpoint")?;
                        let page = chain(&be, &["browseEndpointContextSupportedConfigs", "browseEndpointContextMusicConfig", "pageType"]);
                        if jsv::strict_eq(&page, "MUSIC_PAGE_TYPE_ARTIST") {
                            direct.push(JsValue::object([("type", JsValue::from("artist")), ("name", title), ("artistId", get(&be, "browseId")?), ("thumbnail", thumbnail), ("subtitle", subtitle)]));
                            continue;
                        }
                        if jsv::strict_eq(&page, "MUSIC_PAGE_TYPE_ALBUM") {
                            direct.push(JsValue::object([("type", JsValue::from("album")), ("name", title), ("albumId", get(&be, "browseId")?), ("thumbnail", thumbnail), ("subtitle", subtitle)]));
                            continue;
                        }
                    }
                    let vid = chain(&nav, &["watchEndpoint", "videoId"]);
                    if vid.truthy() {
                        let artists = artists_from_runs(&subtitle_runs)?;
                        let mut props = vec![("type", JsValue::from("song")), ("id", vid.clone()), ("title", title)];
                        props.extend(artist_fields(&fresh(artists))?);
                        props.extend([("thumbnail", thumbnail), ("url", url_for(&vid))]);
                        direct.push(object(props));
                    }
                }
            }
            Ok(JsValue::object([("textSuggestions", JsValue::Array(text_suggestions)), ("directResults", JsValue::Array(direct))]))
        })())
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Search suggestions error:", &e, JsValue::object([("textSuggestions", jsv::empty_array()), ("directResults", jsv::empty_array())]))))
}

/// `title.toLowerCase()` of a carousel header.
fn carousel_title(carousel: &JsValue) -> J<String> {
    let t = or(chain(carousel, &["header", "musicCarouselShelfBasicHeaderRenderer", "title", "runs", "0", "text"]), "".into());
    Ok(jsv::string(&t)?.to_lowercase())
}

fn two_row_thumbs(r: &JsValue) -> JsValue {
    or(chain(r, &["thumbnailRenderer", "musicThumbnailRenderer", "thumbnail", "thumbnails"]), jsv::empty_array())
}

/// `.map(item => ... || null).filter(Boolean)` over `carousel.contents || []`.
fn carousel_items(carousel: &JsValue, f: &dyn Fn(&JsValue) -> J<Option<JsValue>>) -> J<JsValue> {
    let contents = or(get(carousel, "contents")?, jsv::empty_array());
    let mut out = Vec::new();
    for item in jsv::arr(&contents)? {
        if let Some(v) = f(&item)?
            && v.truthy()
        {
            out.push(v);
        }
    }
    Ok(JsValue::Array(out))
}

/// `yt:artistInfo(artistId)`.
pub async fn artist_info<H: Host>(ytm: &Ytm, host: &H, artist_id: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let data = ytm.construct(host, "browse", vec![("browseId", artist_id.clone())], vec![]).await?.value();
        let artist = parsers::artist(&data, artist_id)?;
        let (mut monthly, mut banner) = (JsValue::from(""), JsValue::from(""));
        let (mut fans, mut live, mut featured) = (jsv::empty_array(), jsv::empty_array(), jsv::empty_array());
        let mut top_artists = ArtistMap::new();
        let mut top_plays: HashMap<String, JsValue> = HashMap::new();
        // The inner `try { ... } catch (_) {}`: whatever was assigned before a throw stays.
        let _ = async {
            let raw = ytm.construct(host, "browse", vec![("browseId", artist_id.clone())], vec![]).await.map_err(|_| ())?.value();
            let header = or(chain(&raw, &["header", "musicImmersiveHeaderRenderer"]), chain(&raw, &["header", "musicVisualHeaderRenderer"]));
            monthly = or(chain(&header, &["monthlyListenerCount", "runs", "0", "text"]), "".into());
            let banner_thumbs = or(chain(&header, &["thumbnail", "musicThumbnailRenderer", "thumbnail", "thumbnails"]), jsv::empty_array());
            let banner_url = best_thumbnail(&banner_thumbs)?;
            // `bannerUrl?.includes(...)`: on a non-string throws.
            banner = if matches!(banner_url, JsValue::Undefined | JsValue::Null) {
                banner_url
            } else {
                let s = jsv::string(&banner_url)?;
                if s.contains("lh3.googleusercontent.com") {
                    match size_suffix_at(&s) {
                        Some(i) => format!("{}=w1440-h600-p-l90-rj", &s[..i]).into(),
                        None => banner_url,
                    }
                } else {
                    banner_url
                }
            };
            let sections = or(
                chain(&raw, &["contents", "singleColumnBrowseResultsRenderer", "tabs", "0", "tabRenderer", "content", "sectionListRenderer", "contents"]),
                jsv::empty_array(),
            );
            let artist_name = get(&artist, "name")?;
            for section in iter(&sections)? {
                let carousel = opt(&section, "musicCarouselShelfRenderer");
                if !carousel.truthy() {
                    continue;
                }
                let title = carousel_title(&carousel)?;
                if title.contains("fans might also like") {
                    fans = carousel_items(&carousel, &|item| {
                        let r = opt(item, "musicTwoRowItemRenderer");
                        let browse = or(chain(&r, &["navigationEndpoint", "browseEndpoint", "browseId"]), "".into());
                        if !r.truthy() || !jsv::string(&browse)?.starts_with("UC") {
                            return Ok(None);
                        }
                        let thumb = square_thumbnail(&two_row_thumbs(&r), 226)?;
                        Ok(Some(JsValue::object([("artistId", browse), ("name", or(chain(&r, &["title", "runs", "0", "text"]), "Unknown".into())), ("thumbnail", thumb)])))
                    })?;
                } else if title.contains("live performance") {
                    live = carousel_items(&carousel, &|item| {
                        let r = opt(item, "musicTwoRowItemRenderer");
                        let vid = or(chain(&r, &["navigationEndpoint", "watchEndpoint", "videoId"]), "".into());
                        if !r.truthy() || !vid.truthy() {
                            return Ok(None);
                        }
                        Ok(Some(JsValue::object([
                            ("videoId", vid),
                            ("name", or(chain(&r, &["title", "runs", "0", "text"]), "Untitled".into())),
                            ("artist", or(artist_name.clone(), "Unknown Artist".into())),
                            ("artistId", artist_id.clone()),
                            ("thumbnail", best_thumbnail(&two_row_thumbs(&r))?),
                            ("duration", "".into()),
                        ])))
                    })?;
                } else if title.contains("featured on") {
                    featured = carousel_items(&carousel, &|item| {
                        let r = opt(item, "musicTwoRowItemRenderer");
                        let pid = or(chain(&r, &["navigationEndpoint", "browseEndpoint", "browseId"]), "".into());
                        if !r.truthy() || !pid.truthy() {
                            return Ok(None);
                        }
                        Ok(Some(JsValue::object([("playlistId", pid), ("name", or(chain(&r, &["title", "runs", "0", "text"]), "Unknown Playlist".into())), ("thumbnail", square_thumbnail(&two_row_thumbs(&r), 300)?)])))
                    })?;
                }
            }
            for section in iter(&sections)? {
                let shelf = opt(&section, "musicShelfRenderer");
                if !shelf.truthy() {
                    continue;
                }
                for item in iter(&or(get(&shelf, "contents")?, jsv::empty_array()))? {
                    let r = opt(&item, "musicResponsiveListItemRenderer");
                    if !r.truthy() {
                        continue;
                    }
                    let cols = or(get(&r, "flexColumns")?, jsv::empty_array());
                    let vid = chain(&opt(&cols, "0"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs", "0", "navigationEndpoint", "watchEndpoint", "videoId"]);
                    if !vid.truthy() {
                        continue;
                    }
                    let artists = artists_from_runs(&or(chain(&opt(&cols, "1"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs"]), jsv::empty_array()))?;
                    if !artists.is_empty() {
                        top_artists.insert(ipc::to_js_string(&vid), ipc::shared(fresh(artists)));
                    }
                    let plays = or(chain(&opt(&cols, "2"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs", "0", "text"]), "".into());
                    if plays.truthy() {
                        top_plays.insert(ipc::to_js_string(&vid), plays);
                    }
                }
                break;
            }
            Ok::<(), ()>(())
        }
        .await;
        plain((|| {
            let mut top_songs = Vec::new();
            for song in jsv::arr(&or(get(&artist, "topSongs")?, jsv::empty_array()))? {
                let vid = get(&song, "videoId")?;
                if !vid.truthy() {
                    continue;
                }
                let key = ipc::to_js_string(&vid);
                let mut track = map_song_to_track(&song, top_artists.get(&key))?;
                if let Some(p) = top_plays.get(&key) {
                    ipc::define(&mut track, "plays".into(), p.clone());
                }
                top_songs.push(track);
            }
            let album_list = |key: &str, ty: &str| -> J<JsValue> {
                Ok(JsValue::Array(
                    jsv::arr(&or(get(&artist, key)?, jsv::empty_array()))?
                        .iter()
                        .map(|a| {
                            Ok(JsValue::object([
                                ("albumId", get(a, "albumId")?),
                                ("playlistId", get(a, "playlistId")?),
                                ("name", get(a, "name")?),
                                ("year", get(a, "year")?),
                                ("type", JsValue::from(ty)),
                                ("thumbnail", square_thumbnail(&get(a, "thumbnails")?, 300)?),
                            ]))
                        })
                        .collect::<J<Vec<_>>>()?,
                ))
            };
            let top_albums = album_list("topAlbums", "Album")?;
            let top_singles = album_list("topSingles", "Single")?;
            let top_videos = JsValue::Array(
                jsv::arr(&or(get(&artist, "topVideos")?, jsv::empty_array()))?
                    .iter()
                    .map(|v| {
                        let a = get(v, "artist")?;
                        Ok(JsValue::object([
                            ("videoId", get(v, "videoId")?),
                            ("name", or(get(v, "name")?, "Untitled Video".into())),
                            ("artist", or(opt(&a, "name"), "Unknown Artist".into())),
                            ("artistId", or(opt(&a, "artistId"), JsValue::Null)),
                            ("thumbnail", best_thumbnail(&get(v, "thumbnails")?)?),
                            ("duration", format_duration(&get(v, "duration")?)),
                        ]))
                    })
                    .collect::<J<Vec<_>>>()?,
            );
            Ok(JsValue::object([
                ("name", or(get(&artist, "name")?, "Unknown".into())),
                ("artistId", or(get(&artist, "artistId")?, "".into())),
                ("description", "".into()),
                ("followers", JsValue::Number(0.0)),
                ("monthlyListeners", monthly.clone()),
                ("banner", banner.clone()),
                ("tags", jsv::empty_array()),
                ("avatar", square_thumbnail(&get(&artist, "thumbnails")?, 512)?),
                ("topSongs", JsValue::Array(top_songs)),
                ("topAlbums", top_albums),
                ("topSingles", top_singles),
                ("topVideos", top_videos),
                ("fansAlsoLike", fans.clone()),
                ("livePerformances", live.clone()),
                ("featuredOn", featured.clone()),
            ]))
        })())
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Artist info error:", &e, JsValue::Null)))
}

/// `yt:searchPlaylists(query)`.
pub async fn search_playlists<H: Host>(ytm: &Ytm, host: &H, query: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let data = ytm.construct(host, "search", vec![("query", query.clone()), ("params", "Eg-KAQwIABAAGAAgACgBMABqChAEEAMQCRAFEAo%3D".into())], vec![]).await?.value();
        let items = crate::ytm::traverse::traverse_list(&data, &["musicResponsiveListItemRenderer"])?;
        let list = items.iter().map(parsers::playlist_search_result).collect::<Result<Vec<_>, _>>()?;
        plain(
            list.iter()
                .map(|p| {
                    Ok(JsValue::object([
                        ("playlistId", get(p, "playlistId")?),
                        ("name", get(p, "name")?),
                        ("artist", or(chain(p, &["artist", "name"]), "".into())),
                        ("thumbnail", square_thumbnail(&get(p, "thumbnails")?, 300)?),
                    ]))
                })
                .collect::<J<Vec<_>>>(),
        )
        .map(JsValue::Array)
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Search playlists error:", &e, jsv::empty_array())))
}

/// `getPlaylistVideos(playlistId)`, continuation loop included.
async fn playlist_videos<H: Host>(ytm: &Ytm, host: &H, playlist_id: &JsValue) -> Result<Vec<JsValue>, YtmError> {
    let id = jsv::string(playlist_id).map_err(|()| YtmError::Plain("playlistId.startsWith is not a function".into()))?;
    let id = if id.starts_with("PL") { JsValue::from(format!("VL{id}")) } else { playlist_id.clone() };
    let data = ytm.construct(host, "browse", vec![("browseId", id)], vec![]).await?.value();
    use crate::ytm::traverse::{traverse, traverse_list};
    let mut songs = traverse_list(&data, &["musicPlaylistShelfRenderer", "musicResponsiveListItemRenderer"])?;
    let mut continuation = traverse(&data, &["continuation"])?;
    if let JsValue::Array(items) = &continuation {
        continuation = items.first().cloned().unwrap_or_default();
    }
    while !matches!(continuation, JsValue::Array(_)) {
        let more = ytm.construct(host, "browse", vec![], vec![("continuation", continuation.clone())]).await?.value();
        songs.extend(traverse_list(&more, &["musicResponsiveListItemRenderer"])?);
        continuation = traverse(&more, &["continuation"])?;
    }
    let mut out = Vec::new();
    for s in &songs {
        if let Some(v) = parsers::video_playlist(s)? {
            out.push(v);
        }
    }
    Ok(out)
}

/// `yt:getPlaylistVideos(playlistId)`.
pub async fn get_playlist_videos<H: Host>(ytm: &Ytm, host: &H, playlist_id: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let list = playlist_videos(ytm, host, playlist_id).await?;
        let mut out = Vec::new();
        for v in &list {
            if !plain(get(v, "videoId"))?.truthy() {
                continue;
            }
            let id = plain(get(v, "videoId"))?;
            let duration = plain(get(v, "duration"))?;
            let mut props = vec![("id", id.clone()), ("title", or(plain(get(v, "name"))?, "Unknown".into()))];
            props.extend(plain(own_artist_fields(v))?);
            props.extend([
                ("album", or(chain(v, &["album", "name"]), JsValue::Null)),
                ("albumId", or(chain(v, &["album", "albumId"]), JsValue::Null)),
                ("thumbnail", plain(square_thumbnail(&plain(get(v, "thumbnails"))?, 226))?),
                ("duration", format_duration(&duration)),
                ("durationMs", duration_ms(&duration)),
                ("url", url_for(&id)),
            ]);
            out.push(object(props));
        }
        Ok(JsValue::Array(out))
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Get playlist videos error:", &e, jsv::empty_array())))
}

/// `yt:albumTracks(albumId)`.
pub async fn album_tracks<H: Host>(ytm: &Ytm, host: &H, album_id: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let lib = async {
            let data = ytm.construct(host, "browse", vec![("browseId", album_id.clone())], vec![]).await?.value();
            Ok::<_, YtmError>(parsers::album(&data, album_id)?)
        };
        let raw = async { ytm.construct(host, "browse", vec![("browseId", album_id.clone())], vec![]).await.ok().map(Response::value) };
        let (album, raw) = futures_util::join!(lib, raw);
        let album = album?;
        plain((|| {
            let mut map = ArtistMap::new();
            let mut album_artists = fresh(Vec::new());
            if let Some(raw) = &raw {
                let header_runs = or(
                    chain(raw, &["contents", "twoColumnBrowseResultsRenderer", "tabs", "0", "tabRenderer", "content", "sectionListRenderer", "contents", "0", "musicResponsiveHeaderRenderer", "straplineTextOne", "runs"]),
                    jsv::empty_array(),
                );
                album_artists = ipc::shared(fresh(artists_from_runs(&header_runs)?));
                let items = or(
                    chain(raw, &["contents", "twoColumnBrowseResultsRenderer", "secondaryContents", "sectionListRenderer", "contents", "0", "musicShelfRenderer", "contents"]),
                    jsv::empty_array(),
                );
                for item in iter(&items)? {
                    let r = opt(&item, "musicResponsiveListItemRenderer");
                    if !r.truthy() {
                        continue;
                    }
                    let cols = or(get(&r, "flexColumns")?, jsv::empty_array());
                    let vid = chain(&opt(&cols, "0"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs", "0", "navigationEndpoint", "watchEndpoint", "videoId"]);
                    if !vid.truthy() {
                        continue;
                    }
                    let artists = artists_from_runs(&or(chain(&opt(&cols, "1"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs"]), jsv::empty_array()))?;
                    if !artists.is_empty() {
                        map.insert(ipc::to_js_string(&vid), ipc::shared(fresh(artists)));
                    }
                }
            }
            let has_album_artists = !jsv::arr(&album_artists)?.is_empty();
            let mut tracks = Vec::new();
            for song in jsv::arr(&or(get(&album, "songs")?, jsv::empty_array()))? {
                let vid = get(&song, "videoId")?;
                if !vid.truthy() {
                    continue;
                }
                let artists = map.get(&ipc::to_js_string(&vid)).or(if has_album_artists { Some(&album_artists) } else { None });
                tracks.push(map_song_to_track(&song, artists)?);
            }
            let fields = if has_album_artists {
                artist_fields(&album_artists)?
            } else {
                let a = get(&album, "artist")?;
                if chain(&a, &["id"]).truthy() {
                    artist_fields(&fresh(vec![JsValue::object([("name", get(&a, "name")?), ("id", get(&a, "id")?)])]))?
                } else {
                    artist_fields(&fresh(Vec::new()))?
                }
            };
            let mut props = vec![("name", or(get(&album, "name")?, "Unknown Album".into()))];
            props.extend(fields);
            props.extend([("year", or(get(&album, "year")?, JsValue::Null)), ("thumbnail", square_thumbnail(&get(&album, "thumbnails")?, 300)?), ("tracks", JsValue::Array(tracks))]);
            Ok(object(props))
        })())
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Album tracks error:", &e, JsValue::Null)))
}

/// `yt:searchArtists(query)`.
pub async fn search_artists<H: Host>(ytm: &Ytm, host: &H, query: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let raw = ytm.construct(host, "search", vec![("query", query.clone()), ("params", "Eg-KAQwIABAAGAAgASgAMABqChAEEAMQCRAFEAo%3D".into())], vec![]).await?.value();
        plain((|| {
            let mut items = Vec::new();
            let shelves = or(chain(&raw, &["contents", "tabbedSearchResultsRenderer", "tabs", "0", "tabRenderer", "content", "sectionListRenderer", "contents"]), jsv::empty_array());
            for s in iter(&shelves)? {
                for entry in iter(&or(chain(&s, &["musicShelfRenderer", "contents"]), jsv::empty_array()))? {
                    let r = opt(&entry, "musicResponsiveListItemRenderer");
                    if !r.truthy() {
                        continue;
                    }
                    let cols = or(get(&r, "flexColumns")?, jsv::empty_array());
                    let mut runs = Vec::new();
                    for c in jsv::arr(&cols)? {
                        // `flatMap`: an array result spreads, anything else is one element.
                        match or(chain(&c, &["musicResponsiveListItemFlexColumnRenderer", "text", "runs"]), jsv::empty_array()) {
                            JsValue::Array(rs) => runs.extend(rs),
                            v => runs.push(v),
                        }
                    }
                    let browse = or(chain(&r, &["navigationEndpoint", "browseEndpoint", "browseId"]), "".into());
                    let name = or(chain(&JsValue::Array(runs.clone()), &["0", "text"]), "".into());
                    let rest = if runs.len() > 1 { &runs[1..] } else { &[][..] };
                    let sub = jsv::string(&jsv::join_texts(rest, "")?)?;
                    let sub = strip_leading_bullet(&sub);
                    let thumbs = or(chain(&r, &["thumbnail", "musicThumbnailRenderer", "thumbnail", "thumbnails"]), jsv::empty_array());
                    if browse.truthy() && name.truthy() {
                        items.push(JsValue::object([("artistId", browse), ("name", name), ("thumbnail", best_thumbnail(&thumbs)?), ("subtitle", strip_artist_prefix(&sub).into())]));
                    }
                }
            }
            Ok(JsValue::Array(items))
        })())
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Search artists error:", &e, jsv::empty_array())))
}

fn is_ws(c: char) -> bool {
    js::trim(&c.to_string()).is_empty()
}

/// `.replace(/^\s*•\s*/, '').trim()`.
fn strip_leading_bullet(s: &str) -> String {
    let t = s.trim_start_matches(is_ws);
    let out = match t.strip_prefix('\u{2022}') {
        Some(r) => r.trim_start_matches(is_ws),
        None => s,
    };
    js::trim(out).to_owned()
}

/// `.replace(/^Artist\s*•?\s*/i, '').trim()` (ASCII-only `/i`).
fn strip_artist_prefix(s: &str) -> String {
    let out = if s.len() >= 6 && s.is_char_boundary(6) && s[..6].eq_ignore_ascii_case("artist") {
        let r = s[6..].trim_start_matches(is_ws);
        let r = r.strip_prefix('\u{2022}').unwrap_or(r);
        r.trim_start_matches(is_ws)
    } else {
        s
    };
    js::trim(out).to_owned()
}

/// `yt:searchAlbums(query)`.
pub async fn search_albums<H: Host>(ytm: &Ytm, host: &H, query: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let data = ytm.construct(host, "search", vec![("query", query.clone()), ("params", "Eg-KAQwIABAAGAEgACgAMABqChAEEAMQCRAFEAo%3D".into())], vec![]).await?.value();
        let items = crate::ytm::traverse::traverse_list(&data, &["musicResponsiveListItemRenderer"])?;
        let list = items.iter().map(parsers::album_search_result).collect::<Result<Vec<_>, _>>()?;
        plain(
            list.iter()
                .map(|a| {
                    let artist = get(a, "artist")?;
                    Ok(JsValue::object([
                        ("albumId", get(a, "albumId")?),
                        ("name", get(a, "name")?),
                        ("artist", or(opt(&artist, "name"), "Unknown Artist".into())),
                        ("artistId", or(opt(&artist, "artistId"), JsValue::Null)),
                        ("year", get(a, "year")?),
                        ("thumbnail", square_thumbnail(&get(a, "thumbnails")?, 226)?),
                    ]))
                })
                .collect::<J<Vec<_>>>(),
        )
        .map(JsValue::Array)
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Search albums error:", &e, jsv::empty_array())))
}

/// `yt:searchVideos(query)`.
pub async fn search_videos<H: Host>(ytm: &Ytm, host: &H, query: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let data = ytm.construct(host, "search", vec![("query", query.clone()), ("params", "Eg-KAQwIABABGAAgACgAMABqChAEEAMQCRAFEAo%3D".into())], vec![]).await?.value();
        let items = crate::ytm::traverse::traverse_list(&data, &["musicResponsiveListItemRenderer"])?;
        let list = items.iter().map(parsers::video_search_result).collect::<Result<Vec<_>, _>>()?;
        plain(
            list.iter()
                .map(|v| {
                    let id = get(v, "videoId")?;
                    let duration = get(v, "duration")?;
                    let mut props = vec![("id", id.clone()), ("title", or(get(v, "name")?, "Unknown".into()))];
                    props.extend(own_artist_fields(v)?);
                    props.extend([("thumbnail", best_thumbnail(&get(v, "thumbnails")?)?), ("duration", format_duration(&duration)), ("durationMs", duration_ms(&duration)), ("url", url_for(&id))]);
                    Ok(object(props))
                })
                .collect::<J<Vec<_>>>(),
        )
        .map(JsValue::Array)
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Search videos error:", &e, jsv::empty_array())))
}

/// `yt:setCountry(countryCode)`.
pub async fn set_country<H: Host>(ytm: &Ytm, host: &H, code: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let mut st = ytm.state.lock().unwrap_or_else(|e| e.into_inner());
        let st = st.as_mut().ok_or(YtmError::Plain(String::new()))?;
        let code = or(code.clone(), "".into());
        let has_client = |c: &JsValue| chain(c, &["INNERTUBE_CONTEXT", "client"]).truthy();
        let set_client = |c: &mut JsValue, k: &str, v: JsValue| {
            if let JsValue::Object(props) = c
                && let Some((_, ctx)) = props.iter_mut().find(|(n, _)| *n == JsString::from("INNERTUBE_CONTEXT"))
                && let JsValue::Object(cp) = ctx
                && let Some((_, client)) = cp.iter_mut().find(|(n, _)| *n == JsString::from("client"))
            {
                let _ = ipc::set(client, k, v);
            }
        };
        if code.truthy() {
            let _ = ipc::set(&mut st.config, "GL", code.clone());
            let _ = ipc::set(&mut st.config, "INNERTUBE_CONTEXT_GL", code.clone());
            if has_client(&st.config) {
                set_client(&mut st.config, "gl", code.clone());
            }
        }
        let _ = ipc::set(&mut st.config, "HL", "en".into());
        let _ = ipc::set(&mut st.config, "INNERTUBE_CONTEXT_HL", "en".into());
        if has_client(&st.config) {
            set_client(&mut st.config, "hl", "en".into());
        }
        Ok::<_, YtmError>(JsValue::Bool(true))
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Set country error:", &e, JsValue::Bool(false))))
}

fn two_row_artist_fields(r: &JsValue) -> J<Vec<(&'static str, JsValue)>> {
    let sub_runs = or(chain(r, &["subtitle", "runs"]), jsv::empty_array());
    let artists = artists_from_runs(&sub_runs)?;
    if !artists.is_empty() {
        return artist_fields(&fresh(artists));
    }
    Ok(vec![("artist", jsv::join_texts(&jsv::arr(&sub_runs)?, "")?), ("artistId", JsValue::Null)])
}

/// `#${(color >>> 0).toString(16).padStart(8, '0').slice(0, 6)}`.
fn color_hex(color: &JsValue) -> JsValue {
    let n = ipc::to_number(color).unwrap_or(f64::NAN);
    let u = if n.is_finite() { (n.trunc().rem_euclid(4_294_967_296.0)) as u32 } else { 0 };
    format!("#{}", &format!("{u:08x}")[..6]).into()
}

/// `yt:explore()`.
pub async fn explore<H: Host>(ytm: &Ytm, host: &H) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let raw = ytm.construct(host, "browse", vec![("browseId", "FEmusic_explore".into())], vec![]).await?.value();
        plain((|| {
            let sections = or(chain(&raw, &["contents", "singleColumnBrowseResultsRenderer", "tabs", "0", "tabRenderer", "content", "sectionListRenderer", "contents"]), jsv::empty_array());
            let (mut albums, mut moods, mut videos) = (jsv::empty_array(), jsv::empty_array(), jsv::empty_array());
            for section in iter(&sections)? {
                let carousel = opt(&section, "musicCarouselShelfRenderer");
                if !carousel.truthy() {
                    continue;
                }
                let title = carousel_title(&carousel)?;
                if title.contains("new albums") || title.contains("new release") {
                    albums = carousel_items(&carousel, &|item| {
                        let r = opt(item, "musicTwoRowItemRenderer");
                        if !r.truthy() {
                            return Ok(None);
                        }
                        let id = or(chain(&r, &["navigationEndpoint", "browseEndpoint", "browseId"]), "".into());
                        if !id.truthy() {
                            return Ok(None);
                        }
                        let mut props = vec![("albumId", id), ("name", or(chain(&r, &["title", "runs", "0", "text"]), "Unknown".into()))];
                        props.extend(two_row_artist_fields(&r)?);
                        props.extend([("thumbnail", square_thumbnail(&two_row_thumbs(&r), 300)?), ("year", JsValue::Null), ("type", "Album".into())]);
                        Ok(Some(object(props)))
                    })?;
                } else if title.contains("music video") {
                    videos = carousel_items(&carousel, &|item| {
                        let r = opt(item, "musicTwoRowItemRenderer");
                        if !r.truthy() {
                            return Ok(None);
                        }
                        let vid = or(chain(&r, &["navigationEndpoint", "watchEndpoint", "videoId"]), "".into());
                        if !vid.truthy() {
                            return Ok(None);
                        }
                        let mut props = vec![("id", vid.clone()), ("title", or(chain(&r, &["title", "runs", "0", "text"]), "Unknown".into()))];
                        props.extend(two_row_artist_fields(&r)?);
                        props.extend([("thumbnail", best_thumbnail(&two_row_thumbs(&r))?), ("duration", "".into()), ("durationMs", JsValue::Number(0.0)), ("url", url_for(&vid))]);
                        Ok(Some(object(props)))
                    })?;
                } else if title.contains("mood") || title.contains("genre") {
                    moods = carousel_items(&carousel, &|item| {
                        let r = or(opt(item, "musicNavigationButtonRenderer"), opt(item, "musicTwoRowItemRenderer"));
                        if !r.truthy() {
                            return Ok(None);
                        }
                        let browse = or(or(chain(&r, &["clickCommand", "browseEndpoint", "browseId"]), chain(&r, &["navigationEndpoint", "browseEndpoint", "browseId"])), "".into());
                        let params = or(or(chain(&r, &["clickCommand", "browseEndpoint", "params"]), chain(&r, &["navigationEndpoint", "browseEndpoint", "params"])), "".into());
                        let label = or(or(chain(&r, &["buttonText", "runs", "0", "text"]), chain(&r, &["title", "runs", "0", "text"])), "".into());
                        let color = chain(&r, &["solid", "leftStripeColor"]);
                        if !browse.truthy() || !label.truthy() {
                            return Ok(None);
                        }
                        Ok(Some(JsValue::object([("browseId", browse), ("params", params), ("label", label), ("color", if color.truthy() { color_hex(&color) } else { JsValue::Null })])))
                    })?;
                }
            }
            Ok(JsValue::object([("newAlbums", albums), ("moods", moods), ("newMusicVideos", videos)]))
        })())
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Explore error:", &e, JsValue::Null)))
}

/// `yt:charts()`.
pub async fn charts<H: Host>(ytm: &Ytm, host: &H) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let raw = ytm.construct(host, "browse", vec![("browseId", "FEmusic_charts".into())], vec![]).await?.value();
        let (mut top_artists, mut trending) = (jsv::empty_array(), None::<JsValue>);
        plain((|| {
            let mut sections = or(chain(&raw, &["contents", "singleColumnBrowseResultsRenderer", "tabs", "0", "tabRenderer", "content", "sectionListRenderer", "contents"]), jsv::empty_array());
            if !opt(&sections, "length").truthy() {
                sections = or(chain(&raw, &["contents", "sectionListRenderer", "contents"]), jsv::empty_array());
            }
            for section in iter(&sections)? {
                let carousel = opt(&section, "musicCarouselShelfRenderer");
                if !carousel.truthy() {
                    continue;
                }
                let title = carousel_title(&carousel)?;
                if title.contains("video chart") || title.contains("trending") {
                    for item in iter(&or(get(&carousel, "contents")?, jsv::empty_array()))? {
                        let r = opt(&item, "musicTwoRowItemRenderer");
                        if !r.truthy() {
                            continue;
                        }
                        let runs = or(chain(&r, &["title", "runs"]), jsv::empty_array());
                        let item_title = jsv::string(&jsv::join_texts(&jsv::arr(&runs)?, "")?)?.to_lowercase();
                        let browse = or(chain(&r, &["navigationEndpoint", "browseEndpoint", "browseId"]), "".into());
                        if item_title.contains("trending") && browse.truthy() {
                            trending = Some(browse);
                            break;
                        }
                    }
                } else if title.contains("top artist") || title.contains("trending artist") {
                    let contents = jsv::arr(&or(get(&carousel, "contents")?, jsv::empty_array()))?;
                    let mut out = Vec::new();
                    for item in contents.iter().take(20) {
                        let r = or(opt(item, "musicResponsiveListItemRenderer"), opt(item, "musicTwoRowItemRenderer"));
                        if !r.truthy() {
                            continue;
                        }
                        let id = or(chain(&r, &["navigationEndpoint", "browseEndpoint", "browseId"]), "".into());
                        if !id.truthy() || !jsv::string(&id)?.starts_with("UC") {
                            continue;
                        }
                        let name = or(or(chain(&r, &["title", "runs", "0", "text"]), chain(&r, &["flexColumns", "0", "musicResponsiveListItemFlexColumnRenderer", "text", "runs", "0", "text"])), "Unknown".into());
                        let thumbs = or(or(chain(&r, &["thumbnailRenderer", "musicThumbnailRenderer", "thumbnail", "thumbnails"]), chain(&r, &["thumbnail", "musicThumbnailRenderer", "thumbnail", "thumbnails"])), jsv::empty_array());
                        out.push(JsValue::object([("artistId", id), ("name", name), ("thumbnail", square_thumbnail(&thumbs, 226)?)]));
                    }
                    top_artists = JsValue::Array(out);
                }
            }
            Ok(())
        })())?;
        let mut top_songs = jsv::empty_array();
        if let Some(pl) = trending {
            let inner = async {
                let raw = ytm.construct(host, "browse", vec![("browseId", pl)], vec![]).await?.value();
                plain((|| {
                    let shelf = chain(&raw, &["contents", "twoColumnBrowseResultsRenderer", "secondaryContents", "sectionListRenderer", "contents", "0", "musicPlaylistShelfRenderer"]);
                    let mut out = Vec::new();
                    for item in jsv::arr(&or(opt(&shelf, "contents"), jsv::empty_array()))? {
                        let r = opt(&item, "musicResponsiveListItemRenderer");
                        if !r.truthy() {
                            continue;
                        }
                        let cols = or(get(&r, "flexColumns")?, jsv::empty_array());
                        let vid = or(
                            chain(&r, &["overlay", "musicItemThumbnailOverlayRenderer", "content", "musicPlayButtonRenderer", "playNavigationEndpoint", "watchEndpoint", "videoId"]),
                            chain(&opt(&cols, "0"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs", "0", "navigationEndpoint", "watchEndpoint", "videoId"]),
                        );
                        if !vid.truthy() {
                            continue;
                        }
                        let name = or(chain(&opt(&cols, "0"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs", "0", "text"]), "Unknown".into());
                        let artist_runs = or(chain(&opt(&cols, "1"), &["musicResponsiveListItemFlexColumnRenderer", "text", "runs"]), jsv::empty_array());
                        let artists = artists_from_runs(&artist_runs)?;
                        let fields = if artists.is_empty() { vec![("artist", jsv::join_texts(&jsv::arr(&artist_runs)?, "")?), ("artistId", JsValue::Null)] } else { artist_fields(&fresh(artists))? };
                        let rank = or(chain(&r, &["customIndexColumn", "musicCustomIndexColumnRenderer", "text", "runs", "0", "text"]), "".into());
                        let rank_n = js::parse_int(&ipc::to_js_string(&rank));
                        let mut props = vec![("id", vid.clone()), ("title", name)];
                        props.extend(fields);
                        props.extend([
                            ("thumbnail", square_thumbnail(&or(chain(&get(&r, "thumbnail")?, &["musicThumbnailRenderer", "thumbnail", "thumbnails"]), jsv::empty_array()), 226)?),
                            ("rank", JsValue::Number(if rank_n.is_nan() || rank_n == 0.0 { 0.0 } else { rank_n })),
                            ("duration", or(chain(&r, &["fixedColumns", "0", "musicResponsiveListItemFixedColumnRenderer", "text", "runs", "0", "text"]), "".into())),
                            ("url", url_for(&vid)),
                        ]);
                        out.push(object(props));
                    }
                    Ok(JsValue::Array(out))
                })())
            }
            .await;
            match inner {
                Ok(v) => top_songs = v,
                Err(e) => logger::error(&["Chart playlist fetch error:".into(), LogArg::from_js(&e.log_value())]),
            }
        }
        Ok::<_, YtmError>(JsValue::object([("topSongs", top_songs), ("topVideos", jsv::empty_array()), ("topArtists", top_artists)]))
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Charts error:", &e, JsValue::Null)))
}

/// `yt:browseMood(browseId, params)`.
pub async fn browse_mood<H: Host>(ytm: &Ytm, host: &H, browse_id: &JsValue, params: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let raw = ytm.construct(host, "browse", vec![("browseId", browse_id.clone()), ("params", params.clone())], vec![]).await?.value();
        plain((|| {
            let grid = or(chain(&raw, &["contents", "singleColumnBrowseResultsRenderer", "tabs", "0", "tabRenderer", "content", "sectionListRenderer", "contents"]), jsv::empty_array());
            let mut out = Vec::new();
            for section in iter(&grid)? {
                let items = or(or(chain(&section, &["gridRenderer", "items"]), chain(&section, &["musicCarouselShelfRenderer", "contents"])), jsv::empty_array());
                for item in iter(&items)? {
                    let r = opt(&item, "musicTwoRowItemRenderer");
                    let pid = or(chain(&r, &["navigationEndpoint", "browseEndpoint", "browseId"]), "".into());
                    if !r.truthy() || !pid.truthy() {
                        continue;
                    }
                    let sub = jsv::join_texts(&jsv::arr(&or(chain(&r, &["subtitle", "runs"]), jsv::empty_array()))?, "")?;
                    out.push(JsValue::object([("playlistId", pid), ("name", or(chain(&r, &["title", "runs", "0", "text"]), "Unknown".into())), ("subtitle", sub), ("thumbnail", square_thumbnail(&two_row_thumbs(&r), 300)?)]));
                }
            }
            Ok(JsValue::Array(out))
        })())
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("Browse mood error:", &e, jsv::empty_array())))
}

/// `/^[a-zA-Z0-9-_]{11}$/` (the `-` after the `0-9` range is a literal).
fn valid_video_id(s: &str) -> bool {
    s.len() == 11 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `yt:getTrackInfo(videoId)`.
pub async fn get_track_info<H: Host>(ytm: &Ytm, host: &H, video_id: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let id = jsv::string(video_id).map_err(|()| YtmError::Plain("videoId.match is not a function".into()))?;
        if !valid_video_id(&id) {
            return Err(YtmError::Plain("Invalid videoId".into()));
        }
        let data = ytm.construct(host, "player", vec![("videoId", video_id.clone())], vec![]).await?.value();
        let song = parsers::song(&data)?;
        if plain(get(&song, "videoId"))? != *video_id {
            return Err(YtmError::Plain("Invalid videoId".into()));
        }
        plain(map_song_to_track(&song, None))
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("getTrackInfo error:", &e, JsValue::Null)))
}

/// `yt:getUpNexts(videoId)`.
pub async fn get_up_nexts<H: Host>(ytm: &Ytm, host: &H, video_id: &JsValue) -> HandlerResult {
    let r = async {
        ensure(ytm, host).await?;
        let playlist = format!("RDAMVM{}", ipc::to_js_string(video_id));
        let raw = ytm
            .construct(host, "next", vec![("videoId", video_id.clone()), ("playlistId", playlist.into()), ("isAudioOnly", JsValue::Bool(true))], vec![])
            .await?
            .value();
        plain((|| {
            let contents = or(
                chain(&raw, &["contents", "singleColumnMusicWatchNextResultsRenderer", "tabbedRenderer", "watchNextTabbedResultsRenderer", "tabs", "0", "tabRenderer", "content", "musicQueueRenderer", "content", "playlistPanelRenderer", "contents"]),
                jsv::empty_array(),
            );
            let list = jsv::arr(&contents)?;
            let mut out = Vec::new();
            for item in list.iter().skip(1) {
                let r = opt(item, "playlistPanelVideoRenderer");
                let vid = chain(&r, &["navigationEndpoint", "watchEndpoint", "videoId"]);
                if !r.truthy() || !vid.truthy() {
                    continue;
                }
                let all_runs = or(chain(&r, &["longBylineText", "runs"]), jsv::empty_array());
                let runs = jsv::arr(&all_runs)?;
                let mut dot = None;
                for (i, run) in runs.iter().enumerate() {
                    if jsv::strict_eq(&get(run, "text")?, " \u{2022} ") {
                        dot = Some(i);
                        break;
                    }
                }
                let artists = artists_from_runs(&match dot {
                    Some(i) => JsValue::Array(runs[..i].to_vec()),
                    None => all_runs.clone(),
                })?;
                let text = or(chain(&r, &["lengthText", "runs", "0", "text"]), "".into());
                let parts: Vec<f64> = jsv::string(&text)?.split(':').map(js::string_to_number).collect();
                let ms = match parts.len() {
                    2 => (parts[0] * 60.0 + parts[1]) * 1000.0,
                    3 => (parts[0] * 3600.0 + parts[1] * 60.0 + parts[2]) * 1000.0,
                    _ => 0.0,
                };
                let mut props = vec![("id", vid.clone()), ("title", or(chain(&r, &["title", "runs", "0", "text"]), "Unknown".into()))];
                props.extend(artist_fields(&fresh(artists))?);
                props.extend([("thumbnail", square_thumbnail(&or(chain(&r, &["thumbnail", "thumbnails"]), jsv::empty_array()), 226)?), ("duration", text), ("durationMs", JsValue::Number(ms)), ("url", url_for(&vid))]);
                out.push(object(props));
            }
            Ok(JsValue::Array(out))
        })())
    }
    .await;
    Ok(r.unwrap_or_else(|e| fail("getUpNexts error:", &e, jsv::empty_array())))
}

// ─── Streams, downloads and the cache (yt-dlp) ───

use crate::{
    host::SaveDialog,
    ipc::Thrown,
    net::JsErr,
    node::{self, child_process},
    stream_cache::StreamCache,
};

/// `stream-cache.js`'s map and `audio-cache.js`'s `_activeDownloadProc`.
#[derive(Default)]
pub struct Streams {
    pub cache: std::sync::Mutex<StreamCache<JsValue>>,
    /// The active download: a token to compare (`getActiveDownloadProc() === proc`) and its killer.
    active: std::sync::Mutex<Option<(u64, child_process::Killer)>>,
    next: std::sync::atomic::AtomicU64,
}

fn thrown(e: JsErr) -> Thrown {
    Thrown::Error { name: e.name, message: e.message }
}

/// `stderr?.trim() || err.message`.
fn exec_failure(o: &child_process::Output) -> JsValue {
    let t = js::trim(&o.stderr);
    if t.is_empty() { o.err.as_ref().map(|e| JsValue::String(e.message.clone())).unwrap_or_default() } else { t.into() }
}

/// `runYtDlp(args, timeout)` inside `yt:getStreamUrl`: the first output line, or a rejection
/// (a string, or the Error `execFile` threw).
async fn run_yt_dlp(ytdlp: &str, args: &[JsValue], timeout: u64) -> Result<JsValue, Thrown> {
    let (_, fut) = child_process::exec_file(ytdlp, args, timeout).map_err(thrown)?;
    let o = fut.await;
    if o.err.is_some() {
        return Err(Thrown::Value(exec_failure(&o)));
    }
    let url = js::trim(&o.stdout).split('\n').next().unwrap_or_default().to_owned();
    if url.is_empty() {
        return Err(Thrown::Value("yt-dlp returned no URL".into()));
    }
    Ok(url.into())
}

const YT_FLAGS: [&str; 3] = ["--no-warnings", "--no-playlist", "--no-check-certificates"];

fn args(list: &[&str]) -> Vec<JsValue> {
    list.iter().map(|a| JsValue::from(*a)).collect()
}

/// `yt:getStreamUrl(videoUrl, quality, trackMeta = {}, sources = ['youtube'])`.
pub async fn get_stream_url(s: &Streams, ytdlp: &str, now: i64, a: [JsValue; 4]) -> HandlerResult {
    let [video_url, quality, track_meta, sources] = a;
    let track_meta = if track_meta == JsValue::Undefined { JsValue::Object(Vec::new()) } else { track_meta };
    let sources = if sources == JsValue::Undefined { JsValue::Array(vec!["youtube".into()]) } else { sources };
    let fmt = if jsv::strict_eq(&quality, "worstaudio") { "worstaudio/worstaudio*/worst" } else { "bestaudio/bestaudio*/best" };
    let list = match &sources {
        JsValue::Array(items) if !items.is_empty() => items.clone(),
        _ => vec!["youtube".into()],
    };
    // `[...new Set(list)]` (SameValueZero: NaN equals NaN).
    let mut active: Vec<JsValue> = Vec::new();
    for v in list {
        let same = |x: &JsValue| *x == v || matches!((x, &v), (JsValue::Number(a), JsValue::Number(b)) if a.is_nan() && b.is_nan());
        if !active.iter().any(same) {
            active.push(v);
        }
    }
    let mut last = Thrown::error("No enabled sources provided a stream URL");
    for source in &active {
        if jsv::strict_eq(source, "youtube") {
            let key = format!("audio:{}:{fmt}", ipc::to_js_string(&video_url));
            if let Some(v) = s.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key, now) {
                return Ok(v);
            }
            let mut argv = args(&["-f", fmt, "--get-url"]);
            argv.extend(args(&YT_FLAGS));
            argv.push(video_url.clone());
            match run_yt_dlp(ytdlp, &argv, 15_000).await {
                Ok(url) => {
                    s.cache.lock().unwrap_or_else(|e| e.into_inner()).set(key, url.clone(), now);
                    return Ok(url);
                }
                Err(e) => last = e,
            }
        } else if jsv::strict_eq(source, "soundcloud") {
            let title = opt(&track_meta, "title");
            if !title.truthy() {
                continue;
            }
            let artist = ipc::get_prop(&track_meta, "artist").unwrap_or_default();
            let q = format!("scsearch1:{}{}", ipc::to_js_string(&title), if artist.truthy() { format!(" {}", ipc::to_js_string(&artist)) } else { String::new() });
            let key = format!("sc:{q}");
            if let Some(v) = s.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key, now) {
                return Ok(v);
            }
            let mut argv = args(&["-f", "bestaudio/best", "--get-url"]);
            argv.extend(args(&YT_FLAGS));
            argv.push(q.as_str().into());
            match run_yt_dlp(ytdlp, &argv, 20_000).await {
                Ok(url) => {
                    s.cache.lock().unwrap_or_else(|e| e.into_inner()).set(key, url.clone(), now);
                    return Ok(url);
                }
                Err(e) => last = e,
            }
        }
    }
    Err(last)
}

/// `yt:getVideoStreamUrl(videoId, quality, premuxed)`.
pub async fn get_video_stream_url(s: &Streams, ytdlp: &str, now: i64, video_id: &JsValue, quality: &JsValue, premuxed: &JsValue) -> HandlerResult {
    let h = js::parse_int(&ipc::to_js_string(quality));
    let height = js::number_to_string(if h.is_nan() || h == 0.0 { 720.0 } else { h });
    let fmt = if premuxed.truthy() {
        format!("best[height<={height}][protocol!=m3u8_native][protocol!=m3u8]/best[protocol!=m3u8_native][protocol!=m3u8]/best")
    } else {
        format!("bestvideo[height<={height}][ext=mp4]+bestaudio[ext=m4a]/best[height<={height}]/best")
    };
    let key = format!("video:{}:{fmt}", ipc::to_js_string(video_id));
    if let Some(v) = s.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key, now) {
        return Ok(v);
    }
    let mut argv = args(&["-f", &fmt, "--get-url", "--no-warnings", "--no-playlist", "--no-check-certificates"]);
    argv.push(format!("https://music.youtube.com/watch?v={}", ipc::to_js_string(video_id)).into());
    let (_, fut) = child_process::exec_file(ytdlp, &argv, 20_000).map_err(thrown)?;
    let o = fut.await;
    if o.err.is_some() {
        return Err(Thrown::Value(exec_failure(&o)));
    }
    let urls: Vec<&str> = js::trim(&o.stdout).split('\n').filter(|u| !u.is_empty()).collect();
    if urls.is_empty() {
        return Err(Thrown::Value("yt-dlp returned no video URL".into()));
    }
    let result = JsValue::object([("videoUrl", JsValue::from(urls[0])), ("audioUrl", urls.get(1).map_or(JsValue::Null, |u| JsValue::from(*u)))]);
    s.cache.lock().unwrap_or_else(|e| e.into_inner()).set(key, result.clone(), now);
    Ok(result)
}

/// `readdirSync(dir).find(f => f.startsWith(videoId + '.'))`.
fn cached_file(dir: &str, video_id: &JsValue) -> Result<Option<String>, node::FsError> {
    let prefix = format!("{}.", ipc::to_js_string(video_id));
    Ok(node::readdir_sync(dir)?.into_iter().find(|f| f.starts_with(&prefix)))
}

fn fs_thrown(e: node::FsError) -> Thrown {
    Thrown::Error { name: "Error".into(), message: e.message.into() }
}

/// `yt:downloadAudio(videoUrl, quality, videoId)`.
pub async fn download_audio(s: &Streams, ytdlp: &str, cache_dir: &str, video_url: &JsValue, quality: &JsValue, video_id: &JsValue) -> HandlerResult {
    let dir = crate::audio_cache::ensure_cache_dir(cache_dir).map_err(fs_thrown)?.to_owned();
    if let Some(f) = cached_file(&dir, video_id).map_err(fs_thrown)? {
        return Ok(JsValue::object([("path", JsValue::from(node::path::join(&[&dir, &f])))]));
    }
    let fmt = if jsv::strict_eq(quality, "worstaudio") { "worstaudio/worstaudio*/worst" } else { "bestaudio/bestaudio*/best" };
    let out = node::path::join(&[&dir, &format!("{}.%(ext)s", ipc::to_js_string(video_id))]);
    let mut argv = args(&["-f", fmt, "-o", &out, "--no-part", "--no-warnings", "--no-playlist", "--no-check-certificates"]);
    argv.push(video_url.clone());
    let (killer, fut) = child_process::exec_file(ytdlp, &argv, 120_000).map_err(thrown)?;
    let token = s.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *s.active.lock().unwrap_or_else(|e| e.into_inner()) = Some((token, killer));
    let o = fut.await;
    {
        let mut a = s.active.lock().unwrap_or_else(|e| e.into_inner());
        if a.as_ref().is_some_and(|(t, _)| *t == token) {
            *a = None;
        }
    }
    let find = || -> Option<String> { if node::exists_sync(&dir) { cached_file(&dir, video_id).ok().flatten() } else { None } };
    if o.err.is_some() {
        if o.killed() || o.signal().as_deref() == Some("SIGTERM") {
            if let Some(p) = find() {
                let _ = node::unlink_sync(&node::path::join(&[&dir, &p]));
            }
            return Err(Thrown::Value("cancelled".into()));
        }
        return Err(Thrown::Value(exec_failure(&o)));
    }
    match find() {
        Some(f) => Ok(JsValue::object([("path", JsValue::from(node::path::join(&[&dir, &f])))])),
        None => Err(Thrown::Value("Download completed but file not found".into())),
    }
}

/// `cache:cancelDownload`.
pub fn cancel_download(s: &Streams) -> HandlerResult {
    if let Some((_, k)) = s.active.lock().unwrap_or_else(|e| e.into_inner()).take() {
        k.kill();
    }
    Ok(JsValue::object([("ok", JsValue::Bool(true))]))
}

/// `cache:deleteFile(filePath)`.
pub fn delete_file(cache_dir: &str, file: &JsValue) -> HandlerResult {
    // JS calls path.resolve(filePath), whose validator names the first argument paths[0].
    let p = crate::themes::path_arg(file).map_err(|_| thrown(crate::themes::invalid_path_arg("paths[0]", file)))?;
    let cwd = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let resolved = node::path::posix_more::resolve(&[&p], &cwd);
    if !resolved.starts_with(&format!("{cache_dir}/")) {
        return Ok(JsValue::object([("error", JsValue::from("Invalid path"))]));
    }
    let _ = node::unlink_sync(&resolved);
    Ok(JsValue::object([("ok", JsValue::Bool(true))]))
}

/// `cache:clear`.
pub fn clear_cache(cache_dir: &str) -> HandlerResult {
    crate::audio_cache::cleanup_cache_dir(cache_dir);
    Ok(JsValue::object([("ok", JsValue::Bool(true))]))
}

/// `song:saveTo(videoUrl, title, artist)`.
pub async fn save_to<H: Host>(host: &H, ytdlp: &str, dialog_title: &str, video_url: &JsValue, title: &JsValue, artist: &JsValue) -> HandlerResult {
    let base = format!("{}{}", if title.truthy() { ipc::to_js_string(title) } else { "track".into() }, if artist.truthy() { format!(" - {}", ipc::to_js_string(artist)) } else { String::new() });
    let safe: String = base.chars().map(|c| if "/\\?%*:|\"<>".contains(c) { '_' } else { c }).collect();
    let picked = host.show_save_dialog(SaveDialog { title: dialog_title.into(), default_path: format!("{safe}.mp3"), filters: vec![("MP3 Audio".into(), vec!["mp3".into()])] }).await;
    let Some(file) = picked.filter(|f| !f.is_empty()) else {
        return Ok(JsValue::object([("canceled", JsValue::Bool(true))]));
    };
    let mut argv = args(&["-x", "--audio-format", "mp3", "--audio-quality", "0", "--no-part", "--no-warnings", "--no-playlist", "--no-check-certificates", "-o", &file]);
    argv.push(video_url.clone());
    let (_, fut) = child_process::exec_file(ytdlp, &argv, 300_000).map_err(thrown)?;
    let o = fut.await;
    Ok(if o.err.is_some() {
        JsValue::object([("error", exec_failure(&o))])
    } else {
        JsValue::object([("success", JsValue::Bool(true)), ("filePath", JsValue::from(file.as_str()))])
    })
}

/// `net:httpGet(rawUrl, reqHeaders = {})`: JSON from an allow-listed HTTPS host, or `null`.
pub async fn http_get<H: Host>(host: &H, raw_url: &JsValue, req_headers: &JsValue) -> HandlerResult {
    let r = async {
        let url = url::Url::parse(&ipc::to_js_string(raw_url)).map_err(|_| "Invalid URL".to_owned())?;
        if url.scheme() != "https" {
            return Err("Only HTTPS URLs are supported".to_owned());
        }
        let hostname = url.host_str().unwrap_or_default().to_owned();
        if hostname != "api.github.com" && hostname != "raw.githubusercontent.com" {
            return Err("Host is not allowed".to_owned());
        }
        let mut headers: Vec<(String, String)> = vec![
            ("User-Agent".into(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36".into()),
            ("Accept".into(), "application/json".into()),
            ("Accept-Language".into(), "en".into()),
        ];
        if let JsValue::Object(props) = req_headers.deref_shared() {
            for (k, v) in props {
                let name = js::trim(&k.to_string_lossy()).to_owned();
                const BLOCKED: [&str; 6] = ["authorization", "cookie", "proxy-authorization", "host", "connection", "content-length"];
                if name.is_empty() || BLOCKED.contains(&name.to_lowercase().as_str()) || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    continue;
                }
                // JS passes `{ ...defaults, ...safeHeaders }` to https.request; object keys are case-sensitive.
                headers.retain(|(h, _)| h != &name);
                headers.push((name, ipc::to_js_string(&v)));
            }
        }
        // `hostname` + `pathname + search`: any port and fragment are dropped.
        let target = format!("https://{hostname}{}{}", url.path(), url.query().map(|q| format!("?{q}")).unwrap_or_default());
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let res = host.https_get(&target, &refs, 10_000).await.map_err(|e| match e {
            crate::host::NetError::Timeout => "Request timeout".to_owned(),
            crate::host::NetError::Failed { message, .. } => message,
        })?;
        let text: Vec<u16> = String::from_utf8_lossy(&res.body).encode_utf16().collect();
        let body = js::json_parse(&text).unwrap_or(JsValue::Null);
        Ok(JsValue::object([("status", JsValue::Number(f64::from(res.status))), ("body", body)]))
    }
    .await;
    Ok(r.unwrap_or_else(|m| {
        logger::error(&["[net:httpGet] failed:".into(), m.as_str().into()]);
        JsValue::Null
    }))
}
