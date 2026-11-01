//! Differential test: music-metadata 7.14 `parseFile(path, { duration: true })` vs
//! `snowify::mm::parse_file`, on ffmpeg-generated files, mutations of them (truncations and
//! byte flips), and hand-made edge files. Needs `ffmpeg` on PATH.

use serde_json::{Value, json};
use snowify::{ipc::JsValue, js, mm};
use std::{path::Path, process::Command};

mod common;
use common::{Diff, Rng};

fn ffmpeg(out: &Path, args: &[&str]) -> bool {
    Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .arg(out)
        .status()
        .is_ok_and(|s| s.success())
}

fn val(v: &Option<mm::Val>) -> Value {
    match v {
        None | Some(mm::Val::Undef) => JsValue::Undefined.encode(),
        Some(mm::Val::Null) => JsValue::Null.encode(),
        Some(mm::Val::Str(s)) => JsValue::String(s.clone()).encode(),
        Some(mm::Val::Num(n)) => JsValue::Number(*n).encode(),
        Some(_) => json!("<object>"),
    }
}

fn b64(d: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in d.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            o.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    o
}

fn corpus(dir: &Path) -> Vec<String> {
    let tone = |secs: &str| vec!["-f", "lavfi", "-i", Box::leak(format!("sine=frequency=440:duration={secs}").into_boxed_str())];
    let cover_jpg = dir.join("cover.jpg");
    let cover_png = dir.join("cover.png");
    ffmpeg(&cover_jpg, &["-f", "lavfi", "-i", "color=c=red:s=16x16", "-frames:v", "1"]);
    ffmpeg(&cover_png, &["-f", "lavfi", "-i", "color=c=blue:s=8x8", "-frames:v", "1"]);
    let tags = ["-metadata", "title=Tïtle 🎵", "-metadata", "artist=AC/DC", "-metadata", "album=Älbum"];
    let mut made = Vec::new();
    let mut make = |name: &str, mut args: Vec<&str>| {
        let p = dir.join(name);
        if ffmpeg(&p, &{
            let mut a = tone("3");
            a.append(&mut args);
            a
        }) {
            made.push(p.display().to_string());
        }
    };
    let jpg = cover_jpg.to_str().unwrap();
    let png = cover_png.to_str().unwrap();
    make("cbr.mp3", [&["-c:a", "libmp3lame", "-b:a", "128k"][..], &tags].concat());
    make("vbr.mp3", [&["-c:a", "libmp3lame", "-q:a", "2"][..], &tags].concat());
    make("noxing.mp3", vec!["-c:a", "libmp3lame", "-b:a", "96k", "-write_xing", "0"]);
    make("v23.mp3", [&["-c:a", "libmp3lame", "-id3v2_version", "3", "-write_id3v1", "1"][..], &tags].concat());
    make("v24.mp3", [&["-c:a", "libmp3lame", "-id3v2_version", "4"][..], &tags].concat());
    make("mono22k.mp3", vec!["-c:a", "libmp3lame", "-ac", "1", "-ar", "22050", "-b:a", "48k"]);
    make("mp2.mp3", vec!["-c:a", "mp2", "-b:a", "192k"]);
    make("cover.mp3", vec!["-i", jpg, "-map", "0:a", "-map", "1:v", "-c:a", "libmp3lame", "-c:v", "copy", "-id3v2_version", "3", "-metadata", "title=With cover"]);
    make("coverpng.mp3", vec!["-i", png, "-map", "0:a", "-map", "1:v", "-c:a", "libmp3lame", "-c:v", "copy"]);
    make("tagged.flac", [&["-c:a", "flac"][..], &tags].concat());
    make("cover.flac", vec!["-i", png, "-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "copy", "-disposition:v", "attached_pic"]);
    make("plain.flac", vec!["-c:a", "flac", "-map_metadata", "-1"]);
    make("tagged.ogg", [&["-c:a", "libvorbis"][..], &tags].concat());
    make("plain.ogg", vec!["-c:a", "libvorbis", "-map_metadata", "-1"]);
    make("tagged.opus", [&["-c:a", "libopus"][..], &tags].concat());
    make("mono.opus", vec!["-c:a", "libopus", "-ac", "1", "-metadata", "ARTISTS=One", "-metadata", "artist=Solo"]);
    make("adts.aac", vec!["-c:a", "aac", "-f", "adts"]);
    make("speex.ogg", vec!["-c:a", "libspeex", "-ar", "16000", "-metadata", "title=Speex"]);
    make("flac_in.ogg", vec!["-c:a", "flac"]);
    make("tagged.wav", [&["-c:a", "pcm_s16le"][..], &tags].concat());
    make("plain.wav", vec!["-c:a", "pcm_s16le", "-map_metadata", "-1"]);
    make("float.wav", vec!["-c:a", "pcm_f32le", "-ar", "48000", "-metadata", "title=Float"]);
    make("tagged.aiff", [&["-c:a", "pcm_s16be", "-write_id3v2", "1"][..], &tags].concat());
    make("plain.aiff", vec!["-c:a", "pcm_s16be"]);
    make("tagged.wma", [&["-c:a", "wmav2"][..], &tags].concat());
    make("cover.wma", vec!["-i", jpg, "-map", "0:a", "-map", "1:v", "-c:a", "wmav2", "-c:v", "copy", "-metadata", "artist=W"]);
    make("tagged.m4a", [&["-c:a", "aac"][..], &tags].concat());
    make("alac.m4a", vec!["-c:a", "alac", "-metadata", "title=Lossless"]);
    make("cover.m4a", vec!["-i", jpg, "-map", "0:a", "-map", "1:v", "-c:a", "aac", "-c:v", "copy", "-disposition:v", "attached_pic", "-metadata", "artist=Cover Artist"]);
    make("coverpng.m4a", vec!["-i", png, "-map", "0:a", "-map", "1:v", "-c:a", "aac", "-c:v", "copy", "-disposition:v", "attached_pic"]);
    make("faststart.m4a", vec!["-c:a", "aac", "-movflags", "+faststart", "-metadata", "album=Fast"]);
    make("freeform.m4a", vec!["-c:a", "aac", "-movflags", "+use_metadata_tags", "-metadata", "ARTISTS=Free Form", "-metadata", "title=FF"]);
    let mut out = made;
    // Noise makes LAME's first frames differ, so the source counts frames to the end of file.
    for (name, extra) in [("vbr_noxing.mp3", vec!["-q:a", "2"]), ("vbr_noxing32k.mp3", vec!["-q:a", "6", "-ar", "32000"]), ("vbr_noxing_tagged.mp3", vec!["-q:a", "4", "-id3v2_version", "3", "-write_id3v1", "1", "-metadata", "title=Noise"])] {
        let p = dir.join(name);
        let mut args = vec!["-f", "lavfi", "-i", "anoisesrc=d=4:a=0.8", "-c:a", "libmp3lame", "-write_xing", "0"];
        args.extend(extra);
        if ffmpeg(&p, &args) {
            out.push(p.display().to_string());
        }
    }

    // Hand-made edge files.
    let mut raw = |name: &str, bytes: Vec<u8>| {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        out.push(p.display().to_string());
    };
    raw("empty.mp3", vec![]);
    raw("one.mp3", vec![0xFF]);
    raw("text.mp3", b"this is not audio at all, just some text".repeat(20));
    raw("id3v5.mp3", [b"ID3\x05\x00\x00\x00\x00\x00\x10".to_vec(), vec![0; 300]].concat());
    raw("id3only.mp3", [b"ID3\x03\x00\x00\x00\x00\x00\x15TIT2\x00\x00\x00\x0b\x00\x00\x01\xff\xfeH\x00i\x00\x00\x00".to_vec()].concat());
    raw("unknown.xyz", b"OggS".to_vec());
    let mut tagged = std::fs::read(dir.join("cbr.mp3")).unwrap_or_default();
    // An APEv2 tag (Title, Artist) and an ID3v1 trailer.
    let item = |k: &str, v: &str| [&(v.len() as u32).to_le_bytes()[..], &0u32.to_le_bytes(), k.as_bytes(), &[0], v.as_bytes()].concat();
    let items = [item("Title", "Ape Title"), item("Artist", "Ape Artist")].concat();
    let footer = [b"APETAGEX".as_slice(), &2000u32.to_le_bytes(), &((items.len() + 32) as u32).to_le_bytes(), &2u32.to_le_bytes(), &0u32.to_le_bytes(), &[0u8; 8]].concat();
    let mut v1 = b"TAG".to_vec();
    for f in [&b"V1 Title"[..], b"V1 Artist", b"V1 Album"] {
        let mut s = f.to_vec();
        s.resize(30, 0);
        v1.extend(s);
    }
    v1.extend(b"2024");
    v1.resize(125, 0);
    v1.extend([0, 7, 17]);
    let mut ape = tagged.clone();
    ape.extend(&items);
    ape.extend(&footer);
    ape.extend(&v1);
    raw("ape.mp3", ape);
    let mut lyr = tagged.clone();
    lyr.extend(b"LYRICSBEGINhello");
    lyr.extend(b"000016LYRICS200");
    lyr.extend(&v1);
    raw("lyrics.mp3", lyr);
    // ID3v2 tags built by hand: repeated frames and a NUL-separated multi-value TPE1 (v2.4).
    let frame = |id: &str, body: &[u8], v4: bool| {
        let n = body.len() as u32;
        let size = if v4 { (n & 0x7f) | ((n >> 7 & 0x7f) << 8) | ((n >> 14 & 0x7f) << 16) | ((n >> 21 & 0x7f) << 24) } else { n };
        [id.as_bytes(), &size.to_be_bytes(), &[0, 0], body].concat()
    };
    let tag = |major: u8, frames: Vec<Vec<u8>>| {
        let body = frames.concat();
        let n = body.len() as u32;
        let ss = [(n >> 21 & 0x7f) as u8, (n >> 14 & 0x7f) as u8, (n >> 7 & 0x7f) as u8, (n & 0x7f) as u8];
        [b"ID3".as_slice(), &[major, 0, 0], &ss, &body].concat()
    };
    let audio = tagged.clone();
    raw("double_tit2.mp3", [tag(3, vec![frame("TIT2", b" First", false), frame("TIT2", b" Second", false), frame("TPE1", b" X/Y/Z", false)]), audio.clone()].concat());
    raw("multi_v24.mp3", [tag(4, vec![frame("TPE1", b"Alpha Beta Gamma", true), frame("TALB", b"A B", true), frame("TIT2", b"\xfe\xff H i", true)]), audio.clone()].concat());
    raw("bad_utf16_tpe1.mp3", [tag(3, vec![frame("TPE1", b"\xfe\xff H ", false)]), audio.clone()].concat());
    raw("v22.mp3", [tag(2, vec![[b"TT2".as_slice(), &[0, 0, 4], b" Two"].concat(), [b"TP1".as_slice(), &[0, 0, 4], b" Art"].concat()]), audio.clone()].concat());
    raw("two_tags.mp3", [tag(3, vec![frame("TIT2", b" From v2.3", false)]), tag(4, vec![frame("TIT2", b"From v2.4", true)]), audio.clone()].concat());
    // Targeted fixtures for size rounding and object-count edge cases.
    let find = |b: &[u8], pat: &[u8]| b.windows(pat.len()).position(|w| w == pat);
    if let Ok(wav) = std::fs::read(dir.join("plain.wav")) {
        if let Some(fmt) = find(&wav, b"fmt ") {
            let info = [b"LIST".as_slice(), &22u32.to_le_bytes(), b"INFO", b"INAM", &5u32.to_le_bytes(), b"Hello\0"].concat();
            let junk = [b"junk".as_slice(), &3u32.to_le_bytes(), b"abc\0"].concat();
            let mut w = wav[..fmt].to_vec();
            w.extend(&junk);
            w.extend(&info);
            w.extend(&wav[fmt..]);
            let riff = (w.len() - 8) as u32;
            w[4..8].copy_from_slice(&riff.to_le_bytes());
            raw("oddchunks.wav", w);
        }
    }
    {
        let frames: u32 = 22050;
        let comm = [&1u16.to_be_bytes()[..], &frames.to_be_bytes(), &16u16.to_be_bytes(), &[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]].concat();
        let ssnd_data = vec![0u8; frames as usize * 2];
        let body = [
            b"AIFF".as_slice(),
            b"COMM", &(comm.len() as u32).to_be_bytes(), &comm,
            b"NAME", &5u32.to_be_bytes(), b"Hello\0",
            b"AUTH", &3u32.to_be_bytes(), b"Bob\0",
            b"SSND", &((ssnd_data.len() + 8) as u32).to_be_bytes(), &[0u8; 8], &ssnd_data,
        ]
        .concat();
        raw("odd.aiff", [b"FORM".as_slice(), &(body.len() as u32).to_be_bytes(), &body].concat());
    }
    if let Ok(wma) = std::fs::read(dir.join("tagged.wma")) {
        let fp = [0xa1, 0xdc, 0xab, 0x8c, 0x47, 0xa9, 0xcf, 0x11, 0x8e, 0xe4, 0x00, 0xc0, 0x0c, 0x20, 0x53, 0x65];
        if let Some(at) = find(&wma, &fp) {
            let mut w = wma.clone();
            w[at + 24 + 40] = w[at + 24 + 40].wrapping_add(7);
            raw("oddduration.wma", w);
        }
        for n in [0u32, 1, 2] {
            let mut w = wma.clone();
            w[24..28].copy_from_slice(&n.to_le_bytes());
            raw(&format!("objects{n}.wma"), w);
        }
    }
    if let Ok(m4a) = std::fs::read(dir.join("cover.m4a")) {
        if let Some(at) = find(&m4a, b"tkhd") {
            let mut m = m4a.clone();
            m[at - 4..at].copy_from_slice(&34u32.to_be_bytes());
            raw("shorttkhd.m4a", m);
        }
    }
    tagged.truncate(0);
    let d = dir.join("dir.mp3");
    std::fs::create_dir_all(&d).unwrap();
    out.push(d.display().to_string());

    // Mutations: truncations and byte flips of every generated file.
    let mut rng = Rng(0xabad_1dea_0bad_c0de);
    let sources: Vec<String> = out.iter().filter(|p| Path::new(p).is_file()).cloned().collect();
    for src in &sources {
        let bytes = std::fs::read(src).unwrap();
        if bytes.len() < 64 {
            continue;
        }
        let ext = Path::new(src).extension().unwrap().to_str().unwrap().to_owned();
        let stem = Path::new(src).file_stem().unwrap().to_str().unwrap().to_owned();
        for (i, cut) in [10, 100, 300, 1000, 1500, 2000, 2500, 3200, 4000, bytes.len() / 2, bytes.len().saturating_sub(129), bytes.len() - 1].into_iter().enumerate() {
            let p = dir.join(format!("{stem}.cut{i}.{ext}"));
            std::fs::write(&p, &bytes[..cut.min(bytes.len())]).unwrap();
            out.push(p.display().to_string());
        }
        let flips = if stem.contains("noxing") { 40 } else { 6 };
        for i in 0..flips {
            let mut b = bytes.clone();
            for _ in 0..1 + rng.below(3) {
                let at = if rng.below(2) == 0 { rng.below(b.len().min(4096)) } else { rng.below(b.len()) };
                b[at] = (rng.next() & 0xFF) as u8;
            }
            let p = dir.join(format!("{stem}.flip{i}.{ext}"));
            std::fs::write(&p, &b).unwrap();
            out.push(p.display().to_string());
        }
    }
    out
}

#[test]
fn music_metadata_matches_source() {
    let dir = std::env::temp_dir().join(format!("snowify-parity-mm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = corpus(&dir);
    let src = common::run_source("mm.js", &json!(paths));
    let mut d = Diff::default();
    for (p, s) in paths.iter().zip(src.as_array().unwrap()) {
        let r = match mm::parse_file(p) {
            Err(_) => json!({ "err": true }),
            Ok(m) => json!({ "ok": {
                "title": val(&m.title), "artist": val(&m.artist), "album": val(&m.album),
                "duration": m.duration.map_or_else(|| "undefined".to_owned(), js::number_to_string),
                "picture": m.picture.map(|p| json!({ "format": p.format, "data": b64(&p.data) })),
            } }),
        };
        let name = Path::new(p).file_name().unwrap().to_string_lossy().into_owned();
        d.check(name, s, &r);
    }
    let ok = src.as_array().unwrap().iter().filter(|r| r.get("ok").is_some()).count();
    println!("mm: {} files, {ok} parsed by the source", paths.len());
    let _ = std::fs::remove_dir_all(&dir);
    d.finish("mm");
}
