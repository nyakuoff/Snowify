//! Port of the file-facing handlers in `src/main/media.js`: Spotify CSV import, playlist CSV
//! export, library export/import, playlist covers, and local audio (pick, scan, copy) with
//! `parseLocalFile` on top of the `music-metadata` port. `spotify:matchTrack` lives with the
//! ytmusic port; window controls, logs and locale are in `shell.rs`.

use crate::{
    host::{Host, OpenDialog, SaveDialog},
    ipc::{self, HandlerResult, JsString, JsValue, Thrown, to_js_string},
    js,
    logger::{self, LogArg},
    mm,
    net::JsErr,
    node::{self, path::posix as path, path::posix_more as path2},
    themes::{invalid_arg_type, path_arg},
};

fn log_error(label: &str, err: &JsErr) {
    logger::error(&[label.into(), LogArg::from_js(&err.log_json())]);
}

fn type_error(m: impl Into<JsString>) -> Thrown {
    Thrown::type_error(m)
}

/// `parseCsvLine`.
pub fn parse_csv_line(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let (mut fields, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if quoted {
            if ch == '"' {
                if chars.get(i + 1) == Some(&'"') {
                    cur.push('"');
                    i += 1;
                } else {
                    quoted = false;
                }
            } else {
                cur.push(ch);
            }
        } else if ch == '"' {
            quoted = true;
        } else if ch == ',' {
            fields.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
        i += 1;
    }
    fields.push(cur);
    fields
}

/// `prefix` + one optional non-line-terminator character + `suffix`, ASCII case-insensitively
/// (JS `/i` without `u` folds only ASCII here).
fn loose(h: &str, prefix: &str, suffix: &str) -> bool {
    let l: Vec<char> = h.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (p, s): (Vec<char>, Vec<char>) = (prefix.chars().collect(), suffix.chars().collect());
    if l.len() < p.len() + s.len() || l.len() > p.len() + s.len() + 1 || l[..p.len()] != p[..] || l[l.len() - s.len()..] != s[..] {
        return false;
    }
    l.len() == p.len() + s.len() || !matches!(l[p.len()], '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `/^(track.?name|title|song.?name|name)$/i`.
fn is_title_header(h: &str) -> bool {
    let l = h.to_ascii_lowercase();
    l == "title" || l == "name" || loose(h, "track", "name") || loose(h, "song", "name")
}

/// `/^(artist.?name|artists?\(?s?\)?|artist)$/i`.
fn is_artist_header(h: &str) -> bool {
    if loose(h, "artist", "name") {
        return true;
    }
    let l = h.to_ascii_lowercase();
    let Some(mut rest) = l.strip_prefix("artist") else { return false };
    for opt in ['s', '(', 's', ')'] {
        rest = rest.strip_prefix(opt).unwrap_or(rest);
    }
    rest.is_empty()
}

pub async fn spotify_pick_csv<H: Host>(host: &H) -> HandlerResult {
    let picked = host
        .show_open_dialog(OpenDialog {
            title: "Select Spotify CSV export files".into(),
            filters: vec![("CSV Files".into(), vec!["csv".into()])],
            multi: true,
            directory: false,
        })
        .await;
    let Some(paths) = picked.filter(|p| !p.is_empty()) else {
        return Ok(JsValue::Null);
    };
    let mut playlists = Vec::new();
    for file in &paths {
        let raw = match node::read_file_utf8(file) {
            Ok(r) => r,
            Err(e) => {
                logger::error(&[format!("Error parsing CSV {file}:").into(), e.message.into()]);
                continue;
            }
        };
        let lines: Vec<&str> = raw.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).filter(|l| !js::trim(l).is_empty()).collect();
        if lines.len() < 2 {
            continue;
        }
        let headers: Vec<String> = parse_csv_line(lines[0]).iter().map(|h| js::trim(h).to_owned()).collect();
        let Some(title_idx) = headers.iter().position(|h| is_title_header(h)) else { continue };
        let artist_idx = headers.iter().position(|h| is_artist_header(h));
        let mut tracks = Vec::new();
        for line in &lines[1..] {
            let fields = parse_csv_line(line);
            let title = fields.get(title_idx).map(|t| js::trim(t).to_owned()).unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let artist = artist_idx
                .and_then(|i| fields.get(i))
                .map(|a| js::trim(&a.replace("\\,", ", ")).to_owned())
                .filter(|a| !a.is_empty())
                .unwrap_or_else(|| "Unknown Artist".into());
            tracks.push(JsValue::object([("title", JsValue::from(title)), ("artist", JsValue::from(artist))]));
        }
        let name = path2::basename_ext(file, ".csv").replace('_', " ");
        playlists.push(JsValue::object([("name", JsValue::from(name)), ("tracks", JsValue::Array(tracks))]));
    }
    Ok(if playlists.is_empty() { JsValue::Null } else { JsValue::Array(playlists) })
}

/// `str.replace(...)` called on a non-string: V8's TypeError for this source line.
fn need_string(v: &JsValue, prop: &str) -> Result<String, Thrown> {
    match v {
        JsValue::String(s) => Ok(s.to_string_lossy()),
        JsValue::Boxed(b) if matches!(**b, JsValue::String(_)) => Ok(to_js_string(b)),
        JsValue::Shared(_, inner) => need_string(inner, prop),
        JsValue::Undefined | JsValue::Null => {
            ipc::get_prop(v, prop)?;
            unreachable!()
        }
        _ => Err(type_error(format!("name.{prop} is not a function"))),
    }
}

pub async fn export_playlist_csv<H: Host>(host: &H, name: &JsValue, tracks: &JsValue) -> HandlerResult {
    let name = need_string(name, "replace")?;
    let safe: String = name.chars().map(|c| if "/\\?%*:|\"<>".contains(c) { '_' } else { c }).collect();
    let save = host
        .show_save_dialog(SaveDialog {
            title: "Export playlist as CSV".into(),
            default_path: format!("{safe}.csv"),
            filters: vec![("CSV Files".into(), vec!["csv".into()])],
        })
        .await;
    let Some(file) = save.filter(|p| !p.is_empty()) else {
        return Ok(JsValue::Bool(false));
    };
    let items = match tracks {
        JsValue::Array(items) => items.clone(),
        JsValue::Shared(_, inner) if matches!(**inner, JsValue::Array(_)) => match &**inner {
            JsValue::Array(items) => items.clone(),
            _ => unreachable!(),
        },
        JsValue::Undefined | JsValue::Null => {
            ipc::get_prop(tracks, "map")?;
            unreachable!()
        }
        _ => return Err(type_error("tracks.map is not a function")),
    };
    let esc = |v: &JsValue| {
        let s = match v {
            JsValue::Undefined | JsValue::Null => String::new(),
            v => to_js_string(v),
        };
        if s.contains(',') || s.contains('"') || s.contains('\n') { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
    };
    let mut rows = vec!["Title,Artist,Album,Duration".to_owned()];
    for t in &items {
        let dur_v = ipc::get_prop(t, "durationMs")?;
        let dur = if dur_v.truthy() {
            let ms = ipc::to_number(&dur_v)?;
            let minutes = js::number_to_string(js::math_floor(ms / 60000.0));
            let secs = js::number_to_string(js::math_floor((ms % 60000.0) / 1000.0));
            format!("{minutes}:{}{secs}", if secs.chars().count() < 2 { "0" } else { "" })
        } else {
            String::new()
        };
        let row = [esc(&ipc::get_prop(t, "title")?), esc(&ipc::get_prop(t, "artist")?), esc(&ipc::get_prop(t, "album")?), dur];
        rows.push(row.join(","));
    }
    node::write_file(&file, rows.join("\n").as_bytes()).map_err(|e| Thrown::error(e.message))?;
    Ok(JsValue::Bool(true))
}

/// `fs.writeFileSync(path, data, 'utf-8')`'s `data` validation.
fn write_data(v: &JsValue) -> Result<Vec<u8>, Thrown> {
    match v {
        JsValue::String(s) => Ok(s.to_string_lossy().into_bytes()),
        JsValue::View { buffer, offset, length, .. } => Ok(buffer[*offset..offset + length].to_vec()),
        JsValue::Shared(_, inner) => write_data(inner),
        v => {
            let e = invalid_arg_type("data", "of type string or an instance of Buffer, TypedArray, or DataView", v);
            Err(Thrown::Error { name: e.name, message: e.message })
        }
    }
}

pub async fn export_library<H: Host>(host: &H, json: &JsValue) -> HandlerResult {
    let date: String = js::iso_string(host.now_ms()).chars().take(10).collect();
    let save = host
        .show_save_dialog(SaveDialog {
            title: "Export Snowify Library".into(),
            default_path: format!("snowify-library-{date}.json"),
            filters: vec![("JSON".into(), vec!["json".into()])],
        })
        .await;
    let Some(file) = save.filter(|p| !p.is_empty()) else {
        return Ok(JsValue::Bool(false));
    };
    let data = write_data(json)?;
    node::write_file(&file, &data).map_err(|e| Thrown::error(e.message))?;
    Ok(JsValue::Bool(true))
}

pub async fn import_library<H: Host>(host: &H) -> HandlerResult {
    let picked = host
        .show_open_dialog(OpenDialog {
            title: "Import Snowify Library".into(),
            filters: vec![("JSON".into(), vec!["json".into()])],
            multi: false,
            directory: false,
        })
        .await;
    let Some(paths) = picked.filter(|p| !p.is_empty()) else {
        return Ok(JsValue::Null);
    };
    node::read_file_utf8(&paths[0]).map(JsValue::from).map_err(|e| Thrown::error(e.message))
}

pub async fn pick_image<H: Host>(host: &H) -> HandlerResult {
    let picked = host
        .show_open_dialog(OpenDialog {
            title: "Choose playlist cover image".into(),
            filters: vec![("Images".into(), ["jpg", "jpeg", "png", "webp", "bmp"].map(String::from).to_vec())],
            multi: false,
            directory: false,
        })
        .await;
    Ok(picked.and_then(|p| p.into_iter().next()).map_or(JsValue::Null, JsValue::from))
}

/// `getCoversDir()`.
fn covers_dir<H: Host>(host: &H) -> Result<String, JsErr> {
    let dir = path::join(&[&host.user_data(), "playlist-covers"]);
    if !node::exists_sync(&dir) {
        node::mkdir_recursive_sync(&dir)?;
    }
    Ok(dir)
}

pub async fn save_image<H: Host>(host: &H, playlist_id: &JsValue, source: &JsValue) -> HandlerResult {
    let r = (|| -> Result<JsValue, JsErr> {
        let dir = covers_dir(host)?;
        let src = path_arg(source)?;
        let ext = path2::extname(&src);
        let ext = if ext.is_empty() { ".jpg".to_owned() } else { ext };
        let dest = path::join(&[&dir, &format!("{}_{}{ext}", to_js_string(playlist_id), host.now_ms())]);
        node::copy_file_sync(&src, &dest)?;
        Ok(dest.into())
    })();
    Ok(r.unwrap_or_else(|e| {
        log_error("Save cover image error:", &e);
        JsValue::Null
    }))
}

pub async fn delete_image(image: &JsValue) -> HandlerResult {
    let r = (|| -> Result<(), JsErr> {
        if let JsValue::String(s) = image {
            let p = s.to_string_lossy();
            if !p.is_empty() && node::exists_sync(&p) {
                node::unlink_sync(&p)?;
            }
        }
        Ok(())
    })();
    Ok(JsValue::Bool(match r {
        Ok(()) => true,
        Err(e) => {
            log_error("Delete cover image error:", &e);
            false
        }
    }))
}

/// `Buffer.from(path).toString('base64url')`.
pub(crate) fn base64url(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut o = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..=c.len() {
            o.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    o
}

pub(crate) fn base64(b: &[u8]) -> String {
    base64url(b).replace('-', "+").replace('_', "/") + ["", "==", "="][b.len() % 3]
}

/// `parseLocalFile(filePath)`: the track object, or `null` when parsing throws.
pub fn parse_local_file(file: &str) -> JsValue {
    let Ok(m) = mm::parse_file(file) else {
        return JsValue::Null;
    };
    let val = |v: &Option<mm::Val>| match v {
        Some(mm::Val::Str(s)) if !s.0.is_empty() => Some(JsValue::String(s.clone())),
        Some(mm::Val::Num(n)) if *n != 0.0 && !n.is_nan() => Some(JsValue::Number(*n)),
        Some(mm::Val::Obj | mm::Val::Pic(_)) => Some(JsValue::Object(Vec::new())),
        _ => None,
    };
    let title = val(&m.title).unwrap_or_else(|| path2::basename_ext(file, &path2::extname(file)).into());
    let artist = val(&m.artist).unwrap_or_else(|| "Unknown Artist".into());
    let album = val(&m.album).unwrap_or_else(|| "".into());
    let duration_ms = match m.duration {
        Some(d) if d != 0.0 && !d.is_nan() => js::math_round(d * 1000.0),
        _ => 0.0,
    };
    let thumbnail = m.picture.map_or(String::new(), |p| {
        format!("data:{};base64,{}", p.format.filter(|f| !f.is_empty()).unwrap_or_else(|| "image/jpeg".into()), base64(&p.data))
    });
    JsValue::object([
        ("id", JsValue::from(format!("local_{}", base64url(file.as_bytes())))),
        ("title", title),
        ("artist", artist),
        ("album", album),
        ("thumbnail", JsValue::from(thumbnail)),
        ("durationMs", JsValue::Number(duration_ms)),
        ("isLocal", JsValue::Bool(true)),
        ("localPath", JsValue::from(file)),
    ])
}

const AUDIO_EXTS: [&str; 9] = ["mp3", "flac", "ogg", "wav", "aac", "m4a", "opus", "wma", "aiff"];

/// `scanAudioFolder`: depth-first in libuv's (byte-sorted) order; Dirent types don't follow
/// symlinks; unreadable directories are skipped.
pub fn scan_audio_folder(folder: &str) -> JsValue {
    fn walk(dir: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<(std::ffi::OsString, bool)> = rd
            .flatten()
            .map(|e| (e.file_name(), e.file_type().map(|t| t.is_dir()).unwrap_or(false)))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, is_dir) in entries {
            let name = name.to_string_lossy().into_owned();
            let full = path::join(&[dir, &name]);
            if is_dir {
                walk(&full, out);
            } else {
                let ext = path2::extname(&name).to_lowercase();
                if AUDIO_EXTS.contains(&ext.get(1..).unwrap_or_default()) {
                    out.push(full);
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(folder, &mut files);
    JsValue::Array(files.iter().map(|f| parse_local_file(f)).filter(|t| *t != JsValue::Null).collect())
}

pub async fn pick_audio_files<H: Host>(host: &H) -> HandlerResult {
    let picked = host
        .show_open_dialog(OpenDialog {
            title: "Import local audio files".into(),
            filters: vec![("Audio".into(), AUDIO_EXTS.map(String::from).to_vec())],
            multi: true,
            directory: false,
        })
        .await;
    let paths = picked.unwrap_or_default();
    Ok(JsValue::Array(paths.iter().map(|p| parse_local_file(p)).filter(|t| *t != JsValue::Null).collect()))
}

pub async fn pick_folder<H: Host>(host: &H) -> HandlerResult {
    let picked = host
        .show_open_dialog(OpenDialog { title: "Select audio folder".into(), filters: vec![], multi: false, directory: true })
        .await;
    let Some(folder) = picked.and_then(|p| p.into_iter().next()) else {
        return Ok(JsValue::Null);
    };
    let tracks = scan_audio_folder(&folder);
    Ok(JsValue::object([
        ("folderPath", JsValue::from(folder.as_str())),
        ("name", JsValue::from(path2::basename(&folder))),
        ("tracks", tracks),
    ]))
}

pub async fn scan_folder(folder: &JsValue) -> HandlerResult {
    match folder {
        JsValue::String(s) if !s.0.is_empty() && node::exists_sync(&s.to_string_lossy()) => Ok(scan_audio_folder(&s.to_string_lossy())),
        _ => Ok(JsValue::Array(Vec::new())),
    }
}

pub async fn copy_to_playlist_folder<H: Host>(host: &H, file: &JsValue, folder: &JsValue) -> HandlerResult {
    let folder = match folder {
        JsValue::String(s) if node::exists_sync(&s.to_string_lossy()) => s.to_string_lossy(),
        _ => return Ok(JsValue::Null),
    };
    let thrown = |e: JsErr| Thrown::Error { name: e.name, message: e.message };
    let file = path_arg(file).map_err(thrown)?;
    let ext = path2::extname(&file);
    let mut dest = path::join(&[&folder, &path2::basename(&file)]);
    if node::exists_sync(&dest) {
        let name = path2::basename_ext(&file, &ext);
        dest = path::join(&[&folder, &format!("{name}_{}{ext}", host.now_ms())]);
    }
    node::copy_file_sync(&file, &dest).map_err(|e| Thrown::error(e.message))?;
    Ok(parse_local_file(&dest))
}

// ─── spotify:matchTrack ───

/// `normalizeStr(s)`: `(s || '')` must be a string (a TypeError otherwise).
fn normalize_str(s: &JsValue) -> Result<String, ()> {
    let s = if s.truthy() { crate::jsv::string(s)? } else { String::new() };
    let nfd = icu_normalizer::DecomposingNormalizerBorrowed::new_nfd().normalize(&s.to_lowercase()).into_owned();
    let mapped: String = nfd
        .chars()
        .filter(|c| !('\u{300}'..='\u{36F}').contains(c))
        .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == ' ' { c } else { ' ' })
        .collect();
    Ok(mapped.split(' ').filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" "))
}

/// `tokenSimilarity(a, b)`: Jaccard over the token sets.
fn token_similarity(a: &JsValue, b: &JsValue) -> Result<f64, ()> {
    let (na, nb) = (normalize_str(a)?, normalize_str(b)?);
    let set = |s: &str| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for t in s.split(' ').filter(|t| !t.is_empty()) {
            if !v.iter().any(|x| x == t) {
                v.push(t.to_owned());
            }
        }
        v
    };
    let (ta, tb) = (set(&na), set(&nb));
    if ta.is_empty() && tb.is_empty() {
        return Ok(1.0);
    }
    if ta.is_empty() || tb.is_empty() {
        return Ok(0.0);
    }
    let inter = ta.iter().filter(|t| tb.contains(t)).count() as f64;
    Ok(inter / (ta.len() as f64 + tb.len() as f64 - inter))
}

/// The best artist score: `s.artist?.name`, then any `s.artists[].name`.
fn artist_score(song: &JsValue, artist: &JsValue) -> Result<f64, ()> {
    use crate::jsv::{chain, get, or};
    let mut best = token_similarity(artist, &or(chain(song, &["artist", "name"]), "".into()))?;
    if let JsValue::Array(list) = get(song, "artists")? {
        for a in &list {
            let s = token_similarity(artist, &or(crate::jsv::opt(a, "name"), "".into()))?;
            if s > best {
                best = s;
            }
        }
    }
    Ok(best)
}

const UNWANTED_TAGS: [&str; 12] = ["instrumental", "karaoke", "cover", "8d", "8d audio", "remix", "slowed", "sped up", "reverb", "nightcore", "acoustic", "live"];

/// `scoreMatch(song, targetTitle, targetArtist)`.
fn score_match(song: &JsValue, title: &JsValue, artist: &JsValue) -> Result<f64, ()> {
    use crate::jsv::{chain, get, or};
    let s_title = or(get(song, "name")?, "".into());
    let title_score = token_similarity(title, &s_title)?;
    let artist_score = artist_score(song, artist)?;
    let norm_target = normalize_str(title)?;
    let norm_result = normalize_str(&s_title)?;
    let mut penalty = 0.0;
    for tag in UNWANTED_TAGS {
        if norm_result.contains(tag) && !norm_target.contains(tag) {
            penalty += 0.5;
        }
    }
    let norm_album = normalize_str(&or(chain(song, &["album", "name"]), "".into()))?;
    for tag in UNWANTED_TAGS {
        if norm_album.contains(tag) && !norm_target.contains(tag) {
            penalty += 0.2;
        }
    }
    let extra = norm_result.replacen(&norm_target, "", 1);
    let extra = extra.trim();
    if !extra.is_empty() && !norm_target.is_empty() {
        let extra_tokens = extra.split(' ').filter(|t| !t.is_empty()).count() as f64;
        let target_tokens = norm_target.split(' ').filter(|t| !t.is_empty()).count() as f64;
        penalty += 0.15 * (extra_tokens / target_tokens.max(1.0));
    }
    if norm_result == norm_target {
        penalty -= 0.2;
    }
    let artist_norm_len = normalize_str(artist)?.len();
    if artist_score == 0.0 && artist_norm_len > 0 {
        penalty += 0.8;
    } else if artist_score < 0.3 && artist_norm_len > 0 {
        penalty += 0.3;
    }
    if artist_score >= 0.8 {
        penalty -= 0.25;
    }
    Ok((title_score * 0.4) + (artist_score * 0.6) - penalty)
}

/// `spotify:matchTrack(title, artist)`.
pub async fn spotify_match_track<H: Host>(ytm: &crate::ytm::Ytm, host: &H, title: &JsValue, artist: &JsValue) -> HandlerResult {
    use crate::ytm::YtmError;
    let r = async {
        // `ctx.ytmusic.searchSongs`: no instance yet throws.
        if ytm.state.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
            return Err(YtmError::Plain(String::new()));
        }
        let plain = |r: Result<f64, ()>| r.map_err(|()| YtmError::Plain(String::new()));
        let query = format!("{} {}", to_js_string(title), to_js_string(artist));
        let songs = crate::ytmusic::search_songs(ytm, host, &JsValue::from(query)).await?;
        let has_id = |s: &JsValue| crate::jsv::get(s, "videoId").map(|v| v.truthy());
        let mut candidates = Vec::new();
        for s in songs {
            if has_id(&s).map_err(|()| YtmError::Plain(String::new()))? {
                candidates.push(s);
            }
        }
        if !candidates.is_empty() {
            let mut top = f64::NEG_INFINITY;
            for s in candidates.iter().take(5) {
                let sc = plain(artist_score(s, artist))?;
                // `Math.max`: any NaN makes the result NaN.
                top = if top.is_nan() || sc.is_nan() { f64::NAN } else { top.max(sc) };
            }
            if top < 0.5 {
                let fallback_q = format!("{} {}", to_js_string(artist), to_js_string(title));
                if let Ok(extra) = crate::ytmusic::search_songs(ytm, host, &JsValue::from(fallback_q)).await {
                    let mut seen: Vec<JsValue> = candidates.iter().map(|c| crate::jsv::get(c, "videoId").unwrap_or_default()).collect();
                    for s in extra {
                        if !has_id(&s).unwrap_or(false) {
                            continue;
                        }
                        let id = crate::jsv::get(&s, "videoId").unwrap_or_default();
                        if !seen.contains(&id) {
                            seen.push(id);
                            candidates.push(s);
                        }
                    }
                }
            }
        }
        if candidates.is_empty() {
            return Ok(JsValue::Null);
        }
        let (mut best, mut best_score) = (candidates[0].clone(), f64::NEG_INFINITY);
        for song in &candidates {
            let score = plain(score_match(song, title, artist))?;
            if score > best_score {
                best_score = score;
                best = song.clone();
            }
        }
        crate::ytmusic::map_song_to_track(&best, None).map_err(|()| YtmError::Plain(String::new()))
    }
    .await;
    Ok(r.unwrap_or_else(|e| {
        logger::error(&["Spotify match error:".into(), LogArg::from_js(&e.log_value())]);
        JsValue::Null
    }))
}
