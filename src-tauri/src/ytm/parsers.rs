//! ytmusic-api's parsers (the ones Snowify reaches), value for value: what they build, the
//! `checkType` each runs, and where they throw (a string method on a non-string, `.map` on
//! a non-array, walking an axios response).

use super::{
    traverse::{ParseError, as_str, find, is_album, is_artist, is_duration, is_title, traverse, traverse_list, traverse_string},
    zod::{self, check_type},
};
use crate::ipc::{self, JsValue, to_js_string};

type R = Result<JsValue, ParseError>;

fn s(v: &str) -> JsValue {
    JsValue::from(v)
}

fn or_null(v: JsValue) -> JsValue {
    if v.truthy() { v } else { JsValue::Null }
}

/// `columns[i]` (undefined past the end).
fn at(cols: &[JsValue], i: usize) -> JsValue {
    cols.get(i).cloned().unwrap_or_default()
}

/// `cols.find(pred) || fallback`.
fn find_or(cols: &[JsValue], pred: fn(&JsValue) -> Result<bool, ParseError>, fallback: JsValue) -> R {
    Ok(match find(cols, pred)? {
        Some(v) if v.truthy() => v,
        _ => fallback,
    })
}

/// `[].flat()` on what `traverseList` returned (one more level).
fn flat(list: Vec<JsValue>) -> Vec<JsValue> {
    let mut out = Vec::new();
    for v in list {
        match v {
            JsValue::Array(items) => out.extend(items),
            v => out.push(v),
        }
    }
    out
}

/// `{ name: traverseString(x, "text"), artistId: traverseString(x, "browseId") || null }`.
fn artist_of(x: &JsValue) -> R {
    Ok(JsValue::object([("name", traverse_string(x, &["text"])?), ("artistId", or_null(traverse_string(x, &["browseId"])?))]))
}

/// `obj?.text` on a found column.
fn text_of(v: &Option<JsValue>) -> JsValue {
    match v {
        Some(v) => ipc::get_prop(v, "text").unwrap_or_default(),
        None => JsValue::Undefined,
    }
}

/// `Parser.parseDuration(time)`.
pub fn parse_duration(time: &JsValue) -> R {
    if !time.truthy() {
        return Ok(JsValue::Null);
    }
    let t = as_str(time)?;
    let mut parts: Vec<f64> = t.split(':').rev().map(crate::js::string_to_number).collect();
    parts.resize(3, f64::NAN);
    let or0 = |n: f64| if n.is_nan() || n == 0.0 { 0.0 } else { n };
    Ok(JsValue::Number(or0(parts[0]) + or0(parts[1]) * 60.0 + or0(parts[2]) * 3600.0))
}

/// `AlbumParser.processYear(year)`.
fn process_year(year: &JsValue) -> R {
    if !year.truthy() {
        return Ok(JsValue::Null);
    }
    let y = as_str(year)?;
    Ok(if y.len() == 4 && y.bytes().all(|b| b.is_ascii_digit()) { JsValue::Number(crate::js::string_to_number(&y)) } else { JsValue::Null })
}

/// `+value`.
fn plus(v: &JsValue) -> JsValue {
    JsValue::Number(ipc::to_number(v).unwrap_or(f64::NAN))
}

// ─── Songs ───

/// `SongParser.parseSearchResult(item)` (also `parseHomeSection`).
pub fn song_search_result(item: &JsValue) -> R {
    let cols = traverse_list(item, &["flexColumns", "runs"])?;
    let title = at(&cols, 0);
    let artist = find_or(&cols, is_artist, at(&cols, 3))?;
    let album = find(&cols, is_album)?;
    let duration = find(&cols, is_duration)?;
    let album = match album {
        Some(a) if a.truthy() => JsValue::object([("name", traverse_string(&a, &["text"])?), ("albumId", traverse_string(&a, &["browseId"])?)]),
        _ => JsValue::Null,
    };
    let data = JsValue::object([
        ("type", s("SONG")),
        ("videoId", traverse_string(item, &["playlistItemData", "videoId"])?),
        ("name", traverse_string(&title, &["text"])?),
        ("artist", artist_of(&artist)?),
        ("album", album),
        ("duration", parse_duration(&text_of(&duration))?),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(check_type(data, &zod::song_detailed(), "SongDetailed"))
}

/// `SongParser.parseArtistTopSong(item, artistBasic)`.
fn song_artist_top(item: &JsValue, artist_basic: &JsValue) -> R {
    let cols = flat(traverse_list(item, &["flexColumns", "runs"])?);
    let title = find(&cols, is_title)?.unwrap_or_default();
    let album = find(&cols, is_album)?.unwrap_or_default();
    let data = JsValue::object([
        ("type", s("SONG")),
        ("videoId", traverse_string(item, &["playlistItemData", "videoId"])?),
        ("name", traverse_string(&title, &["text"])?),
        ("artist", artist_basic.clone()),
        ("album", JsValue::object([("name", traverse_string(&album, &["text"])?), ("albumId", traverse_string(&album, &["browseId"])?)])),
        ("duration", JsValue::Null),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(check_type(data, &zod::song_detailed(), "SongDetailed"))
}

/// `SongParser.parseAlbumSong(item, artistBasic, albumBasic, thumbnails)`.
fn song_album(item: &JsValue, artist_basic: &JsValue, album_basic: &JsValue, thumbnails: &JsValue) -> R {
    let title = find(&traverse_list(item, &["flexColumns", "runs"])?, is_title)?.unwrap_or_default();
    let duration = find(&traverse_list(item, &["fixedColumns", "runs"])?, is_duration)?;
    let data = JsValue::object([
        ("type", s("SONG")),
        ("videoId", traverse_string(item, &["playlistItemData", "videoId"])?),
        ("name", traverse_string(&title, &["text"])?),
        ("artist", artist_basic.clone()),
        ("album", album_basic.clone()),
        ("duration", parse_duration(&text_of(&duration))?),
        ("thumbnails", thumbnails.clone()),
    ]);
    Ok(check_type(data, &zod::song_detailed(), "SongDetailed"))
}

/// `SongParser.parse(data)` (`getSong`).
pub fn song(data: &JsValue) -> R {
    let d = JsValue::object([
        ("type", s("SONG")),
        ("videoId", traverse_string(data, &["videoDetails", "videoId"])?),
        ("name", traverse_string(data, &["videoDetails", "title"])?),
        ("artist", JsValue::object([("name", traverse_string(data, &["author"])?), ("artistId", traverse_string(data, &["videoDetails", "channelId"])?)])),
        ("duration", plus(&traverse_string(data, &["videoDetails", "lengthSeconds"])?)),
        ("thumbnails", JsValue::Array(traverse_list(data, &["videoDetails", "thumbnails"])?)),
        ("formats", JsValue::Array(traverse_list(data, &["streamingData", "formats"])?)),
        ("adaptiveFormats", JsValue::Array(traverse_list(data, &["streamingData", "adaptiveFormats"])?)),
    ]);
    Ok(check_type(d, &zod::song_full(), "SongFull"))
}

// ─── Videos ───

/// `VideoParser.parseSearchResult(item)` (no type check).
pub fn video_search_result(item: &JsValue) -> R {
    let cols = flat(traverse_list(item, &["flexColumns", "runs"])?);
    let title = find(&cols, is_title)?.unwrap_or_default();
    let artist = find_or(&cols, is_artist, at(&cols, 1))?;
    let duration = find(&cols, is_duration)?;
    Ok(JsValue::object([
        ("type", s("VIDEO")),
        ("videoId", traverse_string(item, &["playNavigationEndpoint", "videoId"])?),
        ("name", traverse_string(&title, &["text"])?),
        ("artist", JsValue::object([("artistId", or_null(traverse_string(&artist, &["browseId"])?)), ("name", traverse_string(&artist, &["text"])?)])),
        ("duration", parse_duration(&text_of(&duration))?),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]))
}

fn video_artist_top(item: &JsValue, artist_basic: &JsValue) -> R {
    Ok(JsValue::object([
        ("type", s("VIDEO")),
        ("videoId", traverse_string(item, &["videoId"])?),
        ("name", traverse_string(item, &["runs", "text"])?),
        ("artist", artist_basic.clone()),
        ("duration", JsValue::Null),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]))
}

/// `VideoParser.parsePlaylistVideo(item)`; `None` is its `return;`.
pub fn video_playlist(item: &JsValue) -> Result<Option<JsValue>, ParseError> {
    let flex = flat(traverse_list(item, &["flexColumns", "runs"])?);
    let fixed = flat(traverse_list(item, &["fixedColumns", "runs"])?);
    let title = find_or(&flex, is_title, at(&flex, 0))?;
    let artist = find_or(&flex, is_artist, at(&flex, 1))?;
    let duration = find(&fixed, is_duration)?;
    let id1 = traverse_string(item, &["playNavigationEndpoint", "videoId"])?;
    // `traverseList(item, "thumbnails")[0].url.match(...)`: throws without a string url.
    let first = traverse_list(item, &["thumbnails"])?.into_iter().next().unwrap_or_default();
    let url = as_str(&ipc::get_prop(&first, "url").map_err(|_| ParseError::TypeError)?)?;
    // `/https:\/\/i\.ytimg\.com\/vi\/(.+)\//`: the first occurrence, greedy to the last `/`.
    let id2 = url.match_indices("https://i.ytimg.com/vi/").find_map(|(i, _)| {
        let rest = &url[i + 23..];
        let line = &rest[..rest.find(['\n', '\r', '\u{2028}', '\u{2029}']).unwrap_or(rest.len())];
        line.rfind('/').filter(|&j| j > 0).map(|j| line[..j].to_owned())
    });
    // `videoId1 == ""`: loose equality (an array that stringifies to "" counts).
    let id1_empty = matches!(&id1, JsValue::String(s) if s.0.is_empty()) || (matches!(id1, JsValue::Array(_)) && to_js_string(&id1).is_empty());
    if id1_empty && id2.is_none() {
        return Ok(None);
    }
    let video_id = if id1.truthy() { id1 } else { JsValue::from(id2.unwrap_or_default()) };
    let data = JsValue::object([
        ("type", s("VIDEO")),
        ("videoId", video_id),
        ("name", traverse_string(&title, &["text"])?),
        ("artist", artist_of(&artist)?),
        ("duration", parse_duration(&text_of(&duration))?),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(Some(check_type(data, &zod::video_detailed(), "VideoDetailed")))
}

// ─── Albums ───

/// `AlbumParser.parseSearchResult(item)`.
pub fn album_search_result(item: &JsValue) -> R {
    let cols = flat(traverse_list(item, &["flexColumns", "runs"])?);
    let title = at(&cols, 0);
    let artist = find_or(&cols, is_artist, at(&cols, 3))?;
    let overlay = traverse_string(item, &["overlay", "playlistId"])?;
    let playlist_id = if overlay.truthy() { overlay } else { traverse_string(item, &["thumbnailOverlay", "playlistId"])? };
    // `columns.at(-1)?.text`.
    let last_text = match cols.last() {
        Some(JsValue::Undefined | JsValue::Null) | None => JsValue::Undefined,
        Some(c) => ipc::get_prop(c, "text").map_err(|_| ParseError::TypeError)?,
    };
    let data = JsValue::object([
        ("type", s("ALBUM")),
        ("albumId", traverse_list(item, &["browseId"])?.pop().unwrap_or_default()),
        ("playlistId", playlist_id),
        ("artist", artist_of(&artist)?),
        ("year", process_year(&last_text)?),
        ("name", traverse_string(&title, &["text"])?),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(check_type(data, &zod::album_detailed(), "AlbumDetailed"))
}

fn album_artist_top(item: &JsValue, artist_basic: &JsValue) -> R {
    let data = JsValue::object([
        ("type", s("ALBUM")),
        ("albumId", traverse_list(item, &["browseId"])?.pop().unwrap_or_default()),
        ("playlistId", traverse_string(item, &["musicPlayButtonRenderer", "playlistId"])?),
        ("name", traverse_string(item, &["title", "text"])?),
        ("artist", artist_basic.clone()),
        ("year", process_year(&traverse_list(item, &["subtitle", "text"])?.pop().unwrap_or_default())?),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(check_type(data, &zod::album_detailed(), "AlbumDetailed"))
}

/// `AlbumParser.parse(data, albumId)`.
pub fn album(data: &JsValue, album_id: &JsValue) -> R {
    let album_basic = JsValue::object([("albumId", album_id.clone()), ("name", traverse_string(data, &["tabs", "title", "text"])?)]);
    let artist_data = traverse(data, &["tabs", "straplineTextOne", "runs"])?;
    let artist_basic = JsValue::object([("artistId", or_null(traverse_string(&artist_data, &["browseId"])?)), ("name", traverse_string(&artist_data, &["text"])?)]);
    let thumbnails = JsValue::Array(traverse_list(data, &["background", "thumbnails"])?);
    let playlist_id = traverse_string(data, &["musicPlayButtonRenderer", "playlistId"])?;
    let year = process_year(&traverse_list(data, &["tabs", "subtitle", "text"])?.pop().unwrap_or_default())?;
    let songs = traverse_list(data, &["musicResponsiveListItemRenderer"])?
        .iter()
        .map(|item| song_album(item, &artist_basic, &album_basic, &thumbnails))
        .collect::<Result<Vec<_>, _>>()?;
    let album_name = ipc::get_prop(&album_basic, "name").unwrap_or_default();
    let d = JsValue::object([
        ("type", s("ALBUM")),
        ("albumId", album_id.clone()),
        ("name", album_name),
        ("playlistId", playlist_id),
        ("artist", artist_basic),
        ("year", year),
        ("thumbnails", thumbnails),
        ("songs", JsValue::Array(songs)),
    ]);
    Ok(check_type(d, &zod::album_full(), "AlbumFull"))
}

// ─── Artists & playlists ───

/// `ArtistParser.parseSearchResult(item)`.
pub fn artist_search_result(item: &JsValue) -> R {
    let cols = flat(traverse_list(item, &["flexColumns", "runs"])?);
    let title = at(&cols, 0);
    let data = JsValue::object([
        ("type", s("ARTIST")),
        ("artistId", traverse_string(item, &["browseId"])?),
        ("name", traverse_string(&title, &["text"])?),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(check_type(data, &zod::artist_detailed(), "ArtistDetailed"))
}

fn artist_similar(item: &JsValue) -> R {
    let data = JsValue::object([
        ("type", s("ARTIST")),
        ("artistId", traverse_string(item, &["browseId"])?),
        ("name", traverse_string(item, &["runs", "text"])?),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(check_type(data, &zod::artist_detailed(), "ArtistDetailed"))
}

/// `PlaylistParser.parseSearchResult(item)`.
pub fn playlist_search_result(item: &JsValue) -> R {
    let cols = flat(traverse_list(item, &["flexColumns", "runs"])?);
    let title = at(&cols, 0);
    let artist = find_or(&cols, is_artist, at(&cols, 3))?;
    let data = JsValue::object([
        ("type", s("PLAYLIST")),
        ("playlistId", traverse_string(item, &["overlay", "playlistId"])?),
        ("name", traverse_string(&title, &["text"])?),
        ("artist", artist_of(&artist)?),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(check_type(data, &zod::playlist_detailed(), "PlaylistDetailed"))
}

fn playlist_featured_on(item: &JsValue, artist_basic: &JsValue) -> R {
    let data = JsValue::object([
        ("type", s("PLAYLIST")),
        ("playlistId", traverse_string(item, &["navigationEndpoint", "browseId"])?),
        ("name", traverse_string(item, &["runs", "text"])?),
        ("artist", artist_basic.clone()),
        ("thumbnails", JsValue::Array(traverse_list(item, &["thumbnails"])?)),
    ]);
    Ok(check_type(data, &zod::playlist_detailed(), "PlaylistDetailed"))
}

/// `traverseList(data, "musicCarouselShelfRenderer")?.at(i)?.contents.map(f) ?? []`.
fn carousel(data: &JsValue, i: usize, f: &dyn Fn(&JsValue) -> R) -> R {
    let shelf = traverse_list(data, &["musicCarouselShelfRenderer"])?.get(i).cloned().unwrap_or_default();
    if matches!(shelf, JsValue::Undefined | JsValue::Null) {
        return Ok(JsValue::Array(Vec::new()));
    }
    match ipc::get_prop(&shelf, "contents").map_err(|_| ParseError::TypeError)? {
        JsValue::Array(items) => Ok(JsValue::Array(items.iter().map(f).collect::<Result<Vec<_>, _>>()?)),
        _ => Err(ParseError::TypeError),
    }
}

/// `ArtistParser.parse(data, artistId)`.
pub fn artist(data: &JsValue, artist_id: &JsValue) -> R {
    let name = traverse_string(data, &["header", "title", "text"])?;
    let basic = JsValue::object([("artistId", artist_id.clone()), ("name", name.clone())]);
    let thumbnails = JsValue::Array(traverse_list(data, &["header", "thumbnails"])?);
    let top_songs = traverse_list(data, &["musicShelfRenderer", "contents"])?.iter().map(|i| song_artist_top(i, &basic)).collect::<Result<Vec<_>, _>>()?;
    let top_albums = carousel(data, 0, &|i| album_artist_top(i, &basic))?;
    let top_singles = carousel(data, 1, &|i| album_artist_top(i, &basic))?;
    let top_videos = carousel(data, 2, &|i| video_artist_top(i, &basic))?;
    let featured = carousel(data, 3, &|i| playlist_featured_on(i, &basic))?;
    let similar = carousel(data, 4, &artist_similar)?;
    let d = JsValue::object([
        ("type", s("ARTIST")),
        ("artistId", artist_id.clone()),
        ("name", name),
        ("thumbnails", thumbnails),
        ("topSongs", JsValue::Array(top_songs)),
        ("topAlbums", top_albums),
        ("topSingles", top_singles),
        ("topVideos", top_videos),
        ("featuredOn", featured),
        ("similarArtists", similar),
    ]);
    Ok(check_type(d, &zod::artist_full(), "ArtistFull"))
}

/// `SearchParser.parse(item)`. `parsers[type]` is a plain object lookup, so a type naming
/// an `Object.prototype` member calls that member with `this = parsers`.
pub fn search_result(item: &JsValue) -> R {
    let cols = traverse_list(item, &["flexColumns"])?;
    let ty = traverse_list(&at(&cols, 1), &["runs", "text"])?.into_iter().next().unwrap_or_default();
    match to_js_string(&ty).as_str() {
        "Song" => song_search_result(item),
        "Video" => video_search_result(item),
        "Artist" => artist_search_result(item),
        "EP" | "Single" | "Album" => album_search_result(item),
        "Playlist" => playlist_search_result(item),
        // `Object(item)`: the item itself, or an empty object for null/undefined.
        "constructor" => Ok(match item {
            JsValue::Undefined | JsValue::Null => JsValue::Object(Vec::new()),
            v => v.clone(),
        }),
        // Called on `parsers` with a non-function argument, or not callable at all.
        "__defineGetter__" | "__defineSetter__" | "__proto__" => Err(ParseError::TypeError),
        // `parsers` itself: no `type`.
        "valueOf" => Ok(JsValue::object([("Song", JsValue::Object(Vec::new()))])),
        "toString" | "toLocaleString" => Ok("[object Object]".into()),
        // Booleans and `undefined` from the other members: the handler filters them out.
        "hasOwnProperty" | "isPrototypeOf" | "propertyIsEnumerable" => Ok(JsValue::Bool(false)),
        _ => Ok(JsValue::Null),
    }
}
