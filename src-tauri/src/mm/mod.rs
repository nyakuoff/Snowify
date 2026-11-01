//! Port of the parts of `music-metadata` 7.14 that `src/main/media.js` (`parseLocalFile`) uses:
//! `parseFile(path, { duration: true })` → `common.title/artist/album/picture[0]` and
//! `format.duration`, including when it throws (the caller then returns `null`).
//!
//! Structure follows the source: a strtok3-style tokenizer ([`Tok`]), the
//! `MetadataCollector` ([`Collector`]), and one module per container parser.

pub mod asf;
pub mod id3;
pub mod mp4;
pub mod mpeg;
pub mod riff;
pub mod tables;
pub mod xiph;

use crate::ipc::JsString;
use std::{fs::File, io::Read, os::unix::fs::FileExt};

/// How a parse fails. `Eos` is strtok3's `EndOfStreamError`, which several parsers catch.
#[derive(Debug, Clone, PartialEq)]
pub enum MmErr {
    Eos,
    Other(String),
}

pub type R<T> = Result<T, MmErr>;

pub fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(MmErr::Other(msg.into()))
}

enum Src {
    File(File),
    Buf(Vec<u8>),
}

/// strtok3 6.3 `FileTokenizer` (or `BufferTokenizer`, for ID3 chunks inside WAV/AIFF).
pub struct Tok {
    src: Src,
    pub position: f64,
    pub size: f64,
}

impl Tok {
    pub fn from_buffer(b: Vec<u8>) -> Self {
        let size = b.len() as f64;
        Self { src: Src::Buf(b), position: 0.0, size }
    }

    fn pread(&self, len: usize, at: f64) -> R<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let mut got = 0;
        if at >= 0.0 {
            match &self.src {
                Src::Buf(b) => {
                    let start = (at as usize).min(b.len());
                    let end = (start + len).min(b.len());
                    buf[..end - start].copy_from_slice(&b[start..end]);
                    got = end - start;
                }
                Src::File(f) => {
                    while got < len {
                        match f.read_at(&mut buf[got..], at as u64 + got as u64) {
                            Ok(0) => break,
                            Ok(n) => got += n,
                            Err(e) => return err(crate::node::FsError::uv(&e, "read", std::path::Path::new("")).message),
                        }
                    }
                }
            }
        }
        buf.truncate(got);
        Ok(buf)
    }

    /// `normalizeOptions`: a `position` of 0 (or none) means the current position.
    fn norm(&self, position: Option<f64>) -> R<f64> {
        match position {
            Some(p) if p < self.position => err("`options.position` must be equal or greater than `tokenizer.position`"),
            Some(p) if p != 0.0 => Ok(p),
            _ => Ok(self.position),
        }
    }

    pub fn read(&mut self, len: usize, position: Option<f64>, may_be_less: bool) -> R<Vec<u8>> {
        let at = self.norm(position)?;
        self.position = at;
        let buf = self.pread(len, at)?;
        self.position += buf.len() as f64;
        if buf.len() < len && !may_be_less {
            return Err(MmErr::Eos);
        }
        Ok(buf)
    }

    pub fn peek(&mut self, len: usize, position: Option<f64>, may_be_less: bool) -> R<Vec<u8>> {
        let at = self.norm(position)?;
        let buf = self.pread(len, at)?;
        if buf.len() < len && !may_be_less {
            return Err(MmErr::Eos);
        }
        Ok(buf)
    }

    /// `readToken`/`peekToken`: exactly `len` bytes or EOS.
    pub fn token(&mut self, len: usize) -> R<Vec<u8>> {
        self.read(len, None, false)
    }

    pub fn token_at(&mut self, len: usize, position: f64) -> R<Vec<u8>> {
        self.read(len, Some(position), false)
    }

    pub fn peek_token(&mut self, len: usize) -> R<Vec<u8>> {
        self.peek(len, None, false)
    }

    /// `ignore(n)`: clamps at the end of the file; a negative `n` moves back.
    pub fn ignore(&mut self, n: f64) -> f64 {
        let left = self.size - self.position;
        if n > left {
            self.position += left;
            return left;
        }
        self.position += n;
        n
    }
}

/// `RandomFileReader.randomRead`: a negative position reads at the handle's own sequential
/// offset (Node passes it through as "current position").
pub(crate) struct RandomReader {
    file: File,
    size: f64,
}

impl RandomReader {
    /// Returns the whole zero-filled buffer, as `Buffer.alloc(len)` + `fileHandle.read`.
    fn read(&mut self, len: usize, position: f64) -> R<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let mut n = 0;
        while n < len {
            let r = if position < 0.0 {
                self.file.read(&mut buf[n..])
            } else {
                self.file.read_at(&mut buf[n..], position as u64 + n as u64)
            };
            match r {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(buf)
    }
}

/// A picture as tag parsers produce it, before `postFixPicture`.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub format: Option<String>,
    pub data: Vec<u8>,
}

/// The JS values tag parsers hand to `addTag` (only as detailed as the collector needs).
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Undef,
    Null,
    Str(JsString),
    Num(f64),
    Pic(Picture),
    /// Any object value (UFID, POPM, COMM …): truthy, not a string.
    Obj,
}

impl Val {
    pub fn str(s: &str) -> Self {
        Self::Str(s.into())
    }
}

/// `format` fields the parsers read back.
#[derive(Debug, Default)]
pub struct Format {
    pub duration: Option<f64>,
    pub sample_rate: Option<f64>,
    pub codec_profile: Option<String>,
    pub number_of_samples: Option<f64>,
}

/// `MetadataCollector` for the common tags this app reads, with the side effects (and
/// throws) of `postMap` for every common tag.
#[derive(Debug, Default)]
pub struct Collector {
    pub format: Format,
    /// Tag types seen, in order (`native` keys).
    pub native: Vec<String>,
    title: Option<Val>,
    artist: Option<Val>,
    album: Option<Val>,
    artists: Option<Vec<Val>>,
    pictures: Option<Vec<Picture>>,
    origin: std::collections::HashMap<String, u32>,
}

const TAG_PRIORITY: [&str; 11] =
    ["matroska", "APEv2", "vorbis", "ID3v2.4", "ID3v2.3", "ID3v2.2", "exif", "asf", "iTunes", "AIFF", "ID3v1"];
const ARTIFICIAL: u32 = 500;

fn priority(tag_type: &str) -> u32 {
    match tag_type {
        "artificial" => ARTIFICIAL,
        t => TAG_PRIORITY.iter().position(|p| *p == t).map_or(0, |i| i as u32 + 1),
    }
}

/// Generic (common) id for a native tag, via the generated mapper tables.
fn common_name(tag_type: &str, id: &str) -> Option<&'static str> {
    let (_, ci, map) = tables::TAG_MAPS.iter().find(|(t, _, _)| *t == tag_type)?;
    let key = if *ci { crate::mm::upper(id) } else { id.to_owned() };
    map.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn singleton(id: &str) -> bool {
    tables::COMMON_TAGS.iter().find(|(k, _, _)| *k == id).is_some_and(|(_, multiple, _)| !multiple)
}

/// JS `String.prototype.toUpperCase` (via a lossless round trip where possible).
pub fn upper(s: &str) -> String {
    s.to_uppercase()
}

fn js_str(v: &Val) -> Option<String> {
    match v {
        Val::Str(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

impl Collector {
    pub fn add_tag(&mut self, tag_type: &str, id: &str, value: Val) -> R<()> {
        if !self.native.iter().any(|t| t == tag_type) {
            self.native.push(tag_type.to_owned());
        }
        let Some(common) = common_name(tag_type, id) else { return Ok(()) };
        self.post_map(tag_type, common, value)
    }

    fn post_map(&mut self, tag_type: &str, id: &str, value: Val) -> R<()> {
        let type_error = |m: &str| MmErr::Other(format!("TypeError: {m}"));
        match id {
            "artist" => {
                if self.origin.get("artist") == Some(&priority(tag_type)) {
                    return self.post_map("artificial", "artists", value);
                }
                if self.artists.is_none() {
                    self.set_generic("artificial", "artists", value.clone());
                }
            }
            "artists" => {
                if self.artist.is_none() || self.origin.get("artist") == Some(&ARTIFICIAL) {
                    let present = self.artists.as_ref().is_some_and(|a| a.contains(&value));
                    if !present {
                        // Array#join renders undefined as ''.
                        let mut all: Vec<String> = self.artists.iter().flatten().map(|v| js_str(v).unwrap_or_default()).collect();
                        all.push(js_str(&value).unwrap_or_default());
                        self.set_generic("artificial", "artist", Val::str(&join_artists(&all)));
                    }
                }
            }
            "picture" => {
                if let Val::Pic(p) = value {
                    if let Some(p) = post_fix_picture(p) {
                        self.set_generic(tag_type, "picture", Val::Pic(p));
                    }
                }
                return Ok(());
            }
            "totaltracks" | "totaldiscs" | "movementTotal" => return Ok(()),
            // normalizeTrack(value): `origVal.toString()`.
            "track" | "disk" | "movementIndex" => {
                return match value {
                    Val::Undef | Val::Null => Err(type_error("Cannot read properties of undefined (reading 'toString')")),
                    _ => Ok(()),
                };
            }
            "date" => {
                if !matches!(value, Val::Str(_)) {
                    return Err(type_error("tag.value.substr is not a function"));
                }
            }
            "replaygain_track_gain" | "replaygain_track_peak" | "replaygain_album_gain" | "replaygain_album_peak"
            | "replaygain_track_minmax" | "replaygain_undo" => {
                if !matches!(value, Val::Str(_)) {
                    return Err(type_error("value.split is not a function"));
                }
            }
            _ => {}
        }
        if value != Val::Null {
            self.set_generic(tag_type, id, value);
        }
        Ok(())
    }

    fn set_generic(&mut self, tag_type: &str, id: &str, value: Val) {
        let prio0 = self.origin.get(id).copied().unwrap_or(1000);
        let prio1 = priority(tag_type);
        if singleton(id) {
            if prio1 <= prio0 {
                match id {
                    "title" => self.title = Some(value),
                    "artist" => self.artist = Some(value),
                    "album" => self.album = Some(value),
                    _ => {}
                }
                self.origin.insert(id.to_owned(), prio1);
            }
            return;
        }
        let list_push = |list: &mut Option<Vec<Val>>, v: Val, replace: bool| {
            if replace {
                *list = Some(vec![v]);
            } else if let Some(l) = list {
                l.push(v);
            }
        };
        match id {
            "artists" => {
                if prio1 == prio0 {
                    let unique = tables::COMMON_TAGS.iter().any(|(k, _, u)| *k == "artists" && *u);
                    if !unique || !self.artists.as_ref().is_some_and(|a| a.contains(&value)) {
                        list_push(&mut self.artists, value, false);
                    }
                } else if prio1 < prio0 {
                    list_push(&mut self.artists, value, true);
                    self.origin.insert(id.to_owned(), prio1);
                }
            }
            "picture" => {
                let Val::Pic(p) = value else { return };
                if prio1 == prio0 {
                    self.pictures.get_or_insert_with(Vec::new).push(p);
                } else if prio1 < prio0 {
                    self.pictures = Some(vec![p]);
                    self.origin.insert(id.to_owned(), prio1);
                }
            }
            _ => {
                if prio1 < prio0 {
                    self.origin.insert(id.to_owned(), prio1);
                }
            }
        }
    }
}

/// `joinArtists`: "a, b & c".
pub fn join_artists(a: &[String]) -> String {
    if a.len() > 2 {
        format!("{} & {}", a[..a.len() - 1].join(", "), a[a.len() - 1])
    } else {
        a.join(" & ")
    }
}

/// `postFixPicture`: empty data is dropped; a missing format is sniffed (file-type) or the
/// picture dropped; the format is lowercased and `image/jpg` becomes `image/jpeg`.
fn post_fix_picture(mut p: Picture) -> Option<Picture> {
    if p.data.is_empty() {
        return None;
    }
    let format = match p.format.take().filter(|f| !f.is_empty()) {
        Some(f) => f,
        None => sniff_mime(&p.data)?.to_owned(),
    };
    let format = format.to_lowercase();
    p.format = Some(if format == "image/jpg" { "image/jpeg".into() } else { format });
    Some(p)
}

/// The `file-type` signatures that can apply to embedded pictures and to audio files whose
/// extension is unknown.
pub fn sniff_mime(b: &[u8]) -> Option<&'static str> {
    let at = |o: usize, sig: &[u8]| b.len() >= o + sig.len() && &b[o..o + sig.len()] == sig;
    Some(if at(0, &[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if at(0, &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        "image/png"
    } else if at(0, b"GIF") {
        "image/gif"
    } else if at(0, b"RIFF") && at(8, b"WEBP") {
        "image/webp"
    } else if at(0, b"BM") {
        "image/bmp"
    } else if at(0, &[0x49, 0x49, 0x2A, 0x00]) || at(0, &[0x4D, 0x4D, 0x00, 0x2A]) {
        "image/tiff"
    } else if at(0, b"8BPS") {
        "image/vnd.adobe.photoshop"
    } else if at(0, &[0x00, 0x00, 0x01, 0x00]) {
        "image/x-icon"
    } else if at(0, b"ID3") {
        "audio/mpeg"
    } else if at(0, b"fLaC") {
        "audio/x-flac"
    } else if at(0, b"OggS") {
        "audio/ogg"
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        "audio/vnd.wave"
    } else if at(0, b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC")) {
        "audio/aiff"
    } else if at(4, b"ftyp") {
        "audio/mp4"
    } else if at(0, &[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        "audio/x-ms-asf"
    } else if b.len() >= 2 && b[0] == 0xFF && b[1] & 0xE0 == 0xE0 {
        "audio/mpeg"
    } else {
        return None;
    })
}

/// What `parseLocalFile` reads from the result.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub title: Option<Val>,
    pub artist: Option<Val>,
    pub album: Option<Val>,
    pub picture: Option<Picture>,
    pub duration: Option<f64>,
}

/// `ParserFactory.getParserIdForExtension`.
fn parser_for(path: &str) -> Option<&'static str> {
    let ext = path.rfind('.').map_or(String::new(), |i| path[i..].to_lowercase());
    let ext = if ext.is_empty() { path.to_lowercase() } else { ext };
    Some(match ext.as_str() {
        ".mp2" | ".mp3" | ".m2a" | ".aac" => "mpeg",
        ".ape" => "apev2",
        ".mp4" | ".m4a" | ".m4b" | ".m4pa" | ".m4v" | ".m4r" | ".3gp" => "mp4",
        ".wma" | ".wmv" | ".asf" => "asf",
        ".flac" => "flac",
        ".ogg" | ".ogv" | ".oga" | ".ogm" | ".ogx" | ".opus" | ".spx" => "ogg",
        ".aif" | ".aiff" | ".aifc" => "aiff",
        ".wav" | ".bwf" => "riff",
        ".wv" | ".wvp" => "wavpack",
        ".mpc" => "musepack",
        ".dsf" => "dsf",
        ".dff" => "dsdiff",
        ".mka" | ".mkv" | ".mk3d" | ".mks" | ".webm" => "matroska",
        _ => return None,
    })
}

fn parser_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "audio/mpeg" => "mpeg",
        "audio/x-flac" | "audio/flac" => "flac",
        "audio/ogg" => "ogg",
        "audio/vnd.wave" | "audio/wav" => "riff",
        "audio/aiff" => "aiff",
        "audio/mp4" => "mp4",
        "audio/x-ms-asf" => "asf",
        _ => return None,
    })
}

/// Options passed down to parsers (`{ duration: true }` plus what `scanAppendingHeaders` adds).
#[derive(Debug, Clone, Default)]
pub struct Opts {
    /// `options.apeHeader`: the APEv2 footer and the tag's start offset.
    pub ape_header: Option<(id3::ApeFooter, f64)>,
}

/// `mm.parseFile(filePath, { duration: true })`.
pub fn parse_file(path: &str) -> R<Parsed> {
    // strtok3.fromFile: stat, then open (isFile is never called, so a directory gets through).
    let meta = std::fs::metadata(path).map_err(|e| MmErr::Other(crate::node::FsError::uv(&e, "stat", std::path::Path::new(path)).message))?;
    let size = meta.len() as f64;
    let open = || File::open(path).map_err(|e| MmErr::Other(crate::node::FsError::uv(&e, "open", std::path::Path::new(path)).message));
    let mut opts = Opts::default();
    {
        let mut reader = RandomReader { file: open()?, size };
        opts.ape_header = id3::scan_appending_headers(&mut reader)?;
    }
    let mut tok = Tok { src: Src::File(open()?), position: 0.0, size };
    let mut c = Collector::default();
    let parser = match parser_for(path) {
        Some(p) => p,
        None => {
            let head = tok.peek(4100, None, true)?;
            parser_for_mime(sniff_mime(&head).ok_or_else(|| MmErr::Other("Failed to determine audio format".into()))?)
                .ok_or_else(|| MmErr::Other("Guessed MIME-type not supported".into()))?
        }
    };
    match parser {
        "mpeg" => mpeg::parse(&mut tok, &mut c, &opts)?,
        "flac" => xiph::parse_flac(&mut tok, &mut c, &opts)?,
        "ogg" => xiph::parse_ogg(&mut tok, &mut c)?,
        "mp4" => mp4::parse(&mut tok, &mut c)?,
        "riff" => riff::parse_wav(&mut tok, &mut c)?,
        "aiff" => riff::parse_aiff(&mut tok, &mut c)?,
        "asf" => asf::parse(&mut tok, &mut c)?,
        other => return err(format!("parser not ported: {other}")),
    }
    let first_pic = c.pictures.as_ref().and_then(|p| p.first().cloned());
    Ok(Parsed { title: c.title, artist: c.artist, album: c.album, picture: first_pic, duration: c.format.duration })
}

impl RandomReader {
    pub fn file_size(&self) -> f64 {
        self.size
    }
}
