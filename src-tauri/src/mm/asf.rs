//! ASF / WMA (`asf/AsfParser`, `asf/AsfObject`) from `music-metadata` 7.14. Errors inside the
//! header-object loop are swallowed by the source: parsing just stops there.

use super::{
    Collector, Picture, R, Tok, Val, err,
    id3::{Enc, decode_string},
};
use crate::ipc::JsString;

fn guid(hex: &str) -> [u8; 16] {
    let mut g = [0u8; 16];
    for (i, b) in g.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    g
}

const HEADER: &str = "3026b2758e66cf11a6d900aa0062ce6c";
const FILE_PROPERTIES: &str = "a1dcab8c47a9cf118ee400c00c205365";
const HEADER_EXTENSION: &str = "b503bf5f2ea9cf118ee300c00c205365";
const CONTENT_DESCRIPTION: &str = "3326b2758e66cf11a6d900aa0062ce6c";
const EXT_CONTENT_DESCRIPTION: &str = "40a4d0d207e3d21197f000a0c95ea850";
const CODEC_LIST: &str = "4052d1861d31d011a3a400a0c90348f6";
const STREAM_BITRATE: &str = "ce75f87b8d46d1118d82006097c9a2b2";
const PADDING: &str = "74d40618dfca0945a4ba9aabcb96aae8";
const STREAM_PROPERTIES: &str = "9107dcb7b7a9cf118ee600c00c205365";
const EXT_STREAM_PROPERTIES: &str = "cba5e61472c632438399a96952065b5a";
const METADATA: &str = "eacbf8c5af5b77488467aa8c44fa4cca";
const METADATA_LIBRARY: &str = "941c23449894d149a1411d134e457054";
const COMPATIBILITY: &str = "5d8bf1268445ec479f5f0e651f0452c9";
const INDEX_PLACEHOLDER: &str = "20deaad9177c9c4fbc288555dd98e2a2";

fn is(g: &[u8], hex: &str) -> bool {
    g == guid(hex)
}

fn u16le(b: &[u8], o: usize) -> R<usize> {
    b.get(o..o + 2).map(|s| usize::from(u16::from_le_bytes([s[0], s[1]]))).map_or_else(|| err("RangeError"), Ok)
}

fn u32le(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 4).map(|s| f64::from(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))).map_or_else(|| err("RangeError"), Ok)
}

fn u64le(b: &[u8], o: usize) -> R<u64> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap_or([0; 8]))).map_or_else(|| err("RangeError"), Ok)
}

fn object_len(size: f64) -> R<usize> {
    let len = size - 24.0;
    if len < 0.0 { err("RangeError: invalid size") } else { Ok(len as usize) }
}

/// `AsfUtil.parseUnicodeAttr`.
fn unicode(b: &[u8]) -> R<JsString> {
    let s = decode_string(b, Enc::Utf16le)?;
    let mut u = s.0;
    while u.first() == Some(&0) {
        u.remove(0);
    }
    while u.last() == Some(&0) {
        u.pop();
    }
    Ok(JsString(u))
}

/// `WmPictureToken.get`, including its quirk: data starts 4 bytes past the end of the
/// format string (the description loop never advances).
fn wm_picture(b: &[u8]) -> R<Picture> {
    b.first().ok_or(super::MmErr::Other("RangeError".into()))?;
    b.get(1..5).ok_or(super::MmErr::Other("RangeError".into()))?;
    let mut index = 5;
    loop {
        let w = b.get(index..index + 2).ok_or(super::MmErr::Other("RangeError".into()))?;
        if w == [0, 0] {
            break;
        }
        index += 2;
    }
    let fmt: Vec<u16> = b[5..index].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Ok(Picture { format: Some(String::from_utf16_lossy(&fmt)), data: b.get(index + 4..).map(<[u8]>::to_vec).unwrap_or_default() })
}

/// `State.postProcessTag`.
fn post_process(c: &mut Collector, name: &JsString, kind: usize, data: &[u8]) -> R<()> {
    let name_s = name.to_string_lossy();
    let value = if name_s == "WM/Picture" {
        Val::Pic(wm_picture(data)?)
    } else {
        match kind {
            0 => Val::Str(unicode(data)?),
            2 | 5 => {
                u16le(data, 0)?;
                Val::Obj
            }
            3 => Val::Num(u32le(data, 0)?),
            4 => Val::Num(u64le(data, 0)? as f64),
            1 | 6 => Val::Obj,
            _ => return err(format!("unexpected value headerType: {kind}")),
        }
    };
    c.add_tag("asf", &name_s, value)
}

pub fn parse(tok: &mut Tok, c: &mut Collector) -> R<()> {
    let top = tok.token(30)?;
    if !is(&top[..16], HEADER) {
        return err("expected asf header; but was not found");
    }
    let mut n = u32le(&top, 24)? as i64;
    // `do { … } while (--n)`: errors stop the loop silently.
    let _ = (|| -> R<()> {
        loop {
            object(tok, c)?;
            n -= 1;
            if n == 0 {
                return Ok(());
            }
        }
    })();
    Ok(())
}

fn object(tok: &mut Tok, c: &mut Collector) -> R<()> {
    let h = tok.token(24)?;
    let id = h[..16].to_vec();
    let size = u64le(&h, 16)? as f64;
    if is(&id, FILE_PROPERTIES) {
        let b = tok.token(object_len(size)?)?;
        let play = u64le(&b, 40)?;
        let preroll = u64le(&b, 56)?;
        u32le(&b, 76)?;
        c.format.duration = Some((play / 1000) as f64 / 10000.0 - preroll as f64 / 1000.0);
    } else if is(&id, STREAM_PROPERTIES) {
        tok.token(object_len(size)?)?;
    } else if is(&id, HEADER_EXTENSION) {
        let b = tok.token(22)?;
        u16le(&b, 16)?;
        let mut ext = u32le(&b, 18)?;
        loop {
            let eh = tok.token(24)?;
            let eid = eh[..16].to_vec();
            let esize = u64le(&eh, 16)? as f64;
            let remaining = esize - 24.0;
            if is(&eid, METADATA) || is(&eid, METADATA_LIBRARY) {
                let b = tok.token(object_len(esize)?)?;
                let count = u16le(&b, 0)?;
                let mut pos = 2;
                for _ in 0..count {
                    pos += 4;
                    let name_len = u16le(&b, pos)?;
                    pos += 2;
                    let kind = u16le(&b, pos)?;
                    pos += 2;
                    let data_len = u32le(&b, pos)? as usize;
                    pos += 4;
                    let name = unicode(&b[pos.min(b.len())..(pos + name_len).min(b.len())])?;
                    pos += name_len;
                    let data = b[pos.min(b.len())..(pos + data_len).min(b.len())].to_vec();
                    pos += data_len;
                    post_process(c, &name, kind, &data)?;
                }
            } else if is(&eid, PADDING) || is(&eid, COMPATIBILITY) || is(&eid, INDEX_PLACEHOLDER) {
                tok.ignore(remaining);
            } else if is(&eid, EXT_STREAM_PROPERTIES) {
                let b = tok.token(object_len(esize)?)?;
                b.get(56..60).ok_or(super::MmErr::Other("RangeError".into()))?;
            } else {
                tok.token(object_len(esize)?)?;
            }
            ext -= esize;
            if ext <= 0.0 {
                break;
            }
        }
    } else if is(&id, CONTENT_DESCRIPTION) {
        let b = tok.token(object_len(size)?)?;
        let mut pos = 10;
        for (i, name) in ["Title", "Author", "Copyright", "Description", "Rating"].iter().enumerate() {
            let len = u16le(&b, i * 2)?;
            if len > 0 {
                let end = pos + len;
                let v = unicode(&b[pos.min(b.len())..end.min(b.len())])?;
                c.add_tag("asf", name, Val::Str(v))?;
                pos = end;
            }
        }
    } else if is(&id, EXT_CONTENT_DESCRIPTION) {
        let b = tok.token(object_len(size)?)?;
        let count = u16le(&b, 0)?;
        let mut pos = 2;
        for _ in 0..count {
            let name_len = u16le(&b, pos)?;
            pos += 2;
            let name = unicode(&b[pos.min(b.len())..(pos + name_len).min(b.len())])?;
            pos += name_len;
            let kind = u16le(&b, pos)?;
            pos += 2;
            let value_len = u16le(&b, pos)?;
            pos += 2;
            let value = b[pos.min(b.len())..(pos + value_len).min(b.len())].to_vec();
            pos += value_len;
            post_process(c, &name, kind, &value)?;
        }
    } else if is(&id, CODEC_LIST) {
        let h = tok.token(20)?;
        let count = u16le(&h, 16)?;
        for _ in 0..count {
            tok.token(2)?;
            for _ in 0..2 {
                let n = u16le(&tok.token(2)?, 0)?;
                tok.token(n * 2)?;
            }
            let n = u16le(&tok.token(2)?, 0)?;
            tok.read(n, None, false)?;
        }
    } else if is(&id, STREAM_BITRATE) || is(&id, PADDING) {
        tok.ignore(size - 24.0);
    } else {
        tok.token(object_len(size)?)?;
    }
    Ok(())
}
