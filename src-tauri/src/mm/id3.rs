//! ID3v2 (2.2–2.4), ID3v1, APEv2 tags and the end-of-file header scan, as `music-metadata`
//! 7.14 reads them (`id3v2/`, `id3v1/`, `apev2/`, `lyrics3/`, `core.scanAppendingHeaders`).

use super::{Collector, MmErr, Opts, Picture, R, RandomReader, Tok, Val, err};
use crate::ipc::JsString;

// ─── Byte helpers with token-types' bounds behaviour (DataView: RangeError) ───

fn range_err<T>() -> R<T> {
    err("RangeError: Offset is outside the bounds of the DataView")
}

pub fn u8_at(b: &[u8], o: usize) -> R<f64> {
    b.get(o).map(|v| f64::from(*v)).map_or_else(range_err, Ok)
}

pub fn u32be(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 4).map(|s| f64::from(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))).map_or_else(range_err, Ok)
}

pub fn u32le(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 4).map(|s| f64::from(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))).map_or_else(range_err, Ok)
}

pub fn u24be(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 3).map(|s| f64::from(u32::from_be_bytes([0, s[0], s[1], s[2]]))).map_or_else(range_err, Ok)
}

/// `Buffer#toString('ascii')`: the high bit of each byte is dropped.
pub fn ascii(b: &[u8]) -> String {
    b.iter().map(|c| char::from(c & 0x7F)).collect()
}

/// `Buffer#toString('latin1' | 'binary')`.
pub fn latin1(b: &[u8]) -> String {
    b.iter().map(|c| char::from(*c)).collect()
}

/// `util.getBit`.
pub fn get_bit(b: &[u8], off: usize, bit: u32) -> bool {
    b.get(off).is_some_and(|v| v & (1 << bit) != 0)
}

/// `util.getBitAllignedNumber`, with JS's int32 bitwise semantics (`<<` wraps its count).
pub fn bits(b: &[u8], byte_offset: usize, bit_offset: u32, len: u32) -> i32 {
    let byte_off = byte_offset + (bit_offset / 8) as usize;
    let bit_off = bit_offset % 8;
    let mut value = i32::from(*b.get(byte_off).unwrap_or(&0));
    value &= 0xff >> bit_off;
    let bits_read = 8 - bit_off as i32;
    let bits_left = len as i32 - bits_read;
    if bits_left < 0 {
        value >>= (8 - bit_off as i32 - len as i32) & 31;
    } else if bits_left > 0 {
        value = value.wrapping_shl(bits_left as u32 & 31);
        value |= bits(b, byte_offset, bit_offset + bits_read as u32, bits_left as u32);
    }
    value
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Enc {
    Latin1,
    Utf16le,
    Utf8,
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `util.decodeString`: a leading FF FE is dropped (any encoding); for UTF-16, a leading FE FF
/// means big-endian and an odd length throws.
pub fn decode_string(b: &[u8], enc: Enc) -> R<JsString> {
    if b.len() >= 2 && b[0] == 0xFF && b[1] == 0xFE {
        return decode_string(&b[2..], enc);
    }
    if enc == Enc::Utf16le && b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        if b.len() % 2 != 0 {
            return err("Expected even number of octets for 16-bit unicode string");
        }
        let swapped: Vec<u8> = b.chunks(2).flat_map(|c| [c[1], c[0]]).collect();
        return decode_string(&swapped, enc);
    }
    Ok(JsString(match enc {
        Enc::Latin1 => b.iter().map(|c| u16::from(*c)).collect(),
        Enc::Utf8 => units(&String::from_utf8_lossy(b)),
        // Node drops a trailing odd byte.
        Enc::Utf16le => b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
    }))
}

/// `util.findZero`.
pub fn find_zero(b: &[u8], start: usize, end: usize, enc: Enc) -> usize {
    let mut i = start;
    let at = |i: usize| b.get(i).copied();
    if enc == Enc::Utf16le {
        while at(i) != Some(0) || at(i + 1) != Some(0) {
            if i >= end {
                return end;
            }
            i += 2;
        }
        i
    } else {
        while at(i) != Some(0) {
            if i >= end {
                return end;
            }
            i += 1;
        }
        i
    }
}

/// `.replace(/\x00+$/, '')`.
fn trim_trailing_nulls(mut s: Vec<u16>) -> Vec<u16> {
    while s.last() == Some(&0) {
        s.pop();
    }
    s
}

/// JS `String#trim` on UTF-16 units.
pub fn js_trim(s: &[u16]) -> Vec<u16> {
    let ws = |u: &u16| matches!(*u, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF);
    let start = s.iter().position(|u| !ws(u)).unwrap_or(s.len());
    let end = s.iter().rposition(|u| !ws(u)).map_or(start, |e| e + 1);
    s[start..end.max(start)].to_vec()
}

fn split(s: &[u16], sep: u16) -> Vec<Vec<u16>> {
    s.split(|u| *u == sep).map(<[u16]>::to_vec).collect()
}

// ─── ID3v2 ───

/// `FrameParser.splitValue`.
fn split_value(major: u8, text: Option<&[u16]>) -> R<Vec<JsString>> {
    let Some(text) = text else {
        return err("TypeError: Cannot read properties of undefined (reading 'split')");
    };
    let mut values = split(text, 0);
    if major < 4 && values.len() <= 1 {
        values = split(text, b'/' as u16);
    }
    Ok(values.into_iter().map(|v| JsString(js_trim(&trim_trailing_nulls(v)))).collect())
}

fn fix_picture_mime(f: &str) -> String {
    let f = f.to_lowercase();
    match f.as_str() {
        "jpg" => "image/jpeg".into(),
        "png" => "image/png".into(),
        _ => f,
    }
}

/// A parsed frame value, shaped like the source's output for `ID3v2Parser.parseId3Data`.
enum FrameVal {
    One(Val),
    Many(Vec<Val>),
    /// TXXX: description and texts.
    Txxx(JsString, Vec<JsString>),
    /// COMM/COM: (description, text) entries.
    Comm(Vec<(JsString, JsString)>),
}

/// `FrameParser.readData`; `None` when the source's output is falsy.
fn read_frame(major: u8, b: &[u8], id: &str) -> R<Option<FrameVal>> {
    if b.is_empty() {
        return Ok(None);
    }
    let enc = match b[0] {
        0x00 => Enc::Latin1,
        0x01 | 0x02 => Enc::Utf16le,
        _ => Enc::Utf8,
    };
    let nul = if enc == Enc::Utf16le { 2 } else { 1 };
    let len = b.len();
    let kind = if id != "TXXX" && id.starts_with('T') { "T*" } else { id };
    let sl = |a: usize, z: usize| &b[a.min(len)..z.min(len).max(a.min(len))];
    Ok(match kind {
        "T*" | "IPLS" | "MVIN" | "MVNM" | "PCS" | "PCST" => {
            let text = decode_string(&b[1..], enc).ok().map(|s| trim_trailing_nulls(s.0));
            let strs = |v: Vec<JsString>| FrameVal::Many(v.into_iter().map(Val::Str).collect());
            match id {
                "TMCL" | "TIPL" | "IPLS" => {
                    split_value(major, text.as_deref())?;
                    Some(FrameVal::One(Val::Obj))
                }
                "TRK" | "TRCK" | "TPOS" => text.filter(|t| !t.is_empty()).map(|t| FrameVal::One(Val::Str(JsString(t)))),
                "TCOM" | "TEXT" | "TOLY" | "TOPE" | "TPE1" | "TSRC" | "TCO" | "TCON" => {
                    let v = split_value(major, text.as_deref())?;
                    if matches!(id, "TCO" | "TCON") { Some(FrameVal::One(Val::Obj)) } else { Some(strs(v)) }
                }
                "PCS" | "PCST" => {
                    let first_empty = if major >= 4 {
                        split_value(major, text.as_deref())?.first().is_some_and(|f| f.0.is_empty())
                    } else {
                        text.as_ref().is_some_and(Vec::is_empty)
                    };
                    first_empty.then(|| FrameVal::One(Val::Num(1.0)))
                }
                _ if major >= 4 => Some(strs(split_value(major, text.as_deref())?)),
                _ => Some(FrameVal::Many(vec![text.map_or(Val::Undef, |t| Val::Str(JsString(t)))])),
            }
        }
        "TXXX" => {
            let fzero = find_zero(b, 1, len, enc);
            let desc = decode_string(sl(1, fzero), enc)?;
            let data = sl(fzero + nul, len);
            let text = trim_trailing_nulls(decode_string(data, enc)?.0);
            Some(FrameVal::Txxx(desc, split_value(major, Some(&text))?))
        }
        "PIC" | "APIC" => {
            let mut off = 1;
            let format = if major == 2 {
                let f = latin1(sl(off, off + 3));
                off += 3;
                f
            } else {
                let fz = find_zero(b, off, len, Enc::Latin1);
                let f = latin1(sl(off, fz));
                off = fz + 1;
                f
            };
            let format = fix_picture_mime(&format);
            off += 1; // picture type
            let fz = find_zero(b, off, len, enc);
            decode_string(sl(off, fz), enc)?;
            off = fz + nul;
            Some(FrameVal::One(Val::Pic(Picture { format: Some(format), data: sl(off, len).to_vec() })))
        }
        "CNT" | "PCNT" => Some(FrameVal::One(Val::Num(u32be(b, 0)?))),
        "SYLT" => {
            let mut off = 7;
            while off < len {
                let z = find_zero(b, off, len, enc);
                decode_string(sl(off, z), enc)?;
                off = z + 5;
            }
            Some(FrameVal::One(Val::Obj))
        }
        "ULT" | "USLT" | "COM" | "COMM" => {
            let mut off = 4;
            let fz = find_zero(b, off, len, enc);
            let desc = decode_string(sl(off, fz), enc)?;
            off = fz + nul;
            let text = JsString(trim_trailing_nulls(decode_string(sl(off, len), enc)?.0));
            Some(FrameVal::Comm(vec![(desc, text)]))
        }
        "UFID" | "PRIV" | "GEOB" | "MCDI" => Some(FrameVal::One(Val::Obj)),
        "POPM" => {
            let fz = find_zero(b, 0, len, Enc::Latin1);
            u8_at(b, fz + 1)?;
            Some(FrameVal::One(Val::Obj))
        }
        "WCOM" | "WCOP" | "WOAF" | "WOAR" | "WOAS" | "WORS" | "WPAY" | "WPUB" => {
            Some(FrameVal::One(Val::Str(decode_string(b, Enc::Latin1)?))).filter(|v| !matches!(v, FrameVal::One(Val::Str(s)) if s.0.is_empty()))
        }
        "WXXX" => {
            let fz = find_zero(b, 1, len, enc);
            decode_string(sl(1, fz), enc)?;
            Some(FrameVal::One(Val::Obj))
        }
        "WFD" | "WFED" => {
            let s = decode_string(sl(1, find_zero(b, 1, len, enc)), enc)?;
            (!s.0.is_empty()).then(|| FrameVal::One(Val::Str(s)))
        }
        _ => None,
    })
}

fn remove_unsync(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i + 1 < b.len() {
        out.push(b[i]);
        i += if b[i] == 0xFF && b[i + 1] == 0 { 2 } else { 1 };
    }
    if i < b.len() {
        out.push(b[i]);
    }
    out
}

fn syncsafe(b: &[u8], o: usize) -> f64 {
    let g = |i: usize| u32::from(*b.get(o + i).unwrap_or(&0));
    f64::from(g(3) & 0x7f | (g(2) << 7) | (g(1) << 14) | (g(0) << 21))
}

/// `ID3v2Parser.parse` for one tag at the tokenizer position.
pub fn id3v2_parse(tok: &mut Tok, c: &mut Collector) -> R<()> {
    let h = tok.token(10)?;
    if ascii(&h[..3]) != "ID3" {
        return err("expected ID3-header file-identifier 'ID3' was not found");
    }
    let major = h[3];
    let header_type = format!("ID3v2.{}", h[3] as i8);
    let size = syncsafe(&h, 6);
    let data_len = if get_bit(&h, 5, 6) {
        let ext = tok.token(10)?;
        let ext_size = u32be(&ext, 0)?;
        let remaining = ext_size - 10.0;
        if remaining > 0.0 {
            tok.ignore(remaining);
        }
        size - ext_size
    } else {
        size
    };
    if data_len < 0.0 {
        return err("RangeError: The argument 'size' is invalid");
    }
    let data = tok.token(data_len as usize)?;
    // parseMetadata
    let mut tags: Vec<(String, FrameVal)> = Vec::new();
    let mut off = 0usize;
    loop {
        if off == data.len() {
            break;
        }
        let hl = match major {
            2 => 6,
            3 | 4 => 10,
            _ => return err("header versionIndex is incorrect"),
        };
        if off + hl > data.len() {
            break;
        }
        let fh = &data[off..off + hl];
        off += hl;
        let (id, flen, unsync, dli) = if major == 2 {
            (ascii(&fh[..3]), u24be(fh, 3)? as usize, false, false)
        } else {
            let flen = if major == 4 { syncsafe(fh, 4) } else { u32be(fh, 4)? } as usize;
            (ascii(&fh[..4]), flen, get_bit(fh, 9, 1), get_bit(fh, 9, 0))
        };
        let start = off.min(data.len());
        off = off.saturating_add(flen);
        let mut fd = data[start..off.min(data.len())].to_vec();
        if major != 2 {
            if unsync {
                fd = remove_unsync(&fd);
            }
            if dli {
                fd = fd.get(4..).map(<[u8]>::to_vec).unwrap_or_default();
            }
        }
        if let Some(v) = read_frame(major, &fd, &id)? {
            tags.push((id, v));
        }
    }
    for (id, v) in tags {
        match v {
            FrameVal::Txxx(desc, texts) => {
                let name = if desc.0.is_empty() { id.clone() } else { format!("{id}:{}", desc.to_string_lossy()) };
                for t in texts {
                    c.add_tag(&header_type, &name, Val::Str(t))?;
                }
            }
            FrameVal::Comm(entries) => {
                for (desc, text) in entries {
                    let name = if desc.0.is_empty() { id.clone() } else { format!("{id}:{}", desc.to_string_lossy()) };
                    // COM adds the text; COMM adds the whole object.
                    c.add_tag(&header_type, &name, if id == "COM" { Val::Str(text) } else { Val::Obj })?;
                }
            }
            FrameVal::Many(vals) => {
                for val in vals {
                    c.add_tag(&header_type, &id, val)?;
                }
            }
            FrameVal::One(val) => c.add_tag(&header_type, &id, val)?,
        }
    }
    Ok(())
}

/// `AbstractID3Parser.tryReadId3v2Headers`.
pub fn try_read_id3v2(tok: &mut Tok, c: &mut Collector) -> R<()> {
    loop {
        let h = tok.peek_token(10)?;
        if ascii(&h[..3]) != "ID3" {
            return Ok(());
        }
        id3v2_parse(tok, c)?;
    }
}

// ─── APEv2 ───

#[derive(Debug, Clone, PartialEq)]
pub struct ApeFooter {
    pub id: String,
    pub size: f64,
    pub fields: f64,
}

fn ape_footer(b: &[u8]) -> ApeFooter {
    ApeFooter {
        id: ascii(&b[..8.min(b.len())]),
        size: u32le(b, 12).unwrap_or(0.0),
        fields: u32le(b, 16).unwrap_or(0.0),
    }
}

/// `APEv2Parser.parseTags`.
pub fn ape_parse_tags(tok: &mut Tok, c: &mut Collector, footer: &ApeFooter) -> R<()> {
    let mut key_buf = vec![0u8; 256];
    let mut remaining = footer.size - 32.0;
    let mut i = 0.0;
    while i < footer.fields {
        i += 1.0;
        if remaining < 8.0 {
            break;
        }
        let ih = tok.token(8)?;
        let size = u32le(&ih, 0)?;
        let flags = u32le(&ih, 4)? as u32;
        remaining -= 8.0 + size;
        // `length: Math.min(256, remaining)`: 0 means the whole buffer.
        let want = remaining.min(256.0);
        let want = if want == 0.0 { 256.0 } else { want };
        if want < 0.0 {
            return err("RangeError [ERR_OUT_OF_RANGE]: The value of \"length\" is out of range.");
        }
        let peeked = tok.peek(want as usize, None, false)?;
        key_buf[..peeked.len()].copy_from_slice(&peeked);
        let zero = find_zero(&key_buf, 0, key_buf.len(), Enc::Latin1);
        let key = ascii(&tok.token(zero)?);
        tok.ignore(1.0);
        remaining -= key.chars().count() as f64 + 1.0;
        match (flags & 6) >> 1 {
            0 => {
                let value = tok.token(size as usize)?;
                let value = String::from_utf8_lossy(&value).into_owned();
                for v in value.split('\0') {
                    c.add_tag("APEv2", &key, Val::str(v))?;
                }
            }
            1 => {
                let pic = tok.token(size as usize)?;
                let zero = find_zero(&pic, 0, pic.len(), Enc::Latin1);
                let data = pic.get(zero + 1..).map(<[u8]>::to_vec).unwrap_or_default();
                c.add_tag("APEv2", &key, Val::Pic(Picture { format: None, data }))?;
            }
            _ => {
                tok.ignore(size);
            }
        }
    }
    Ok(())
}

/// `core.scanAppendingHeaders`: finds an APEv2 footer before any ID3v1/Lyrics3 trailer.
pub(crate) fn scan_appending_headers(r: &mut RandomReader) -> R<Option<(ApeFooter, f64)>> {
    let size = r.file_size();
    let mut ape_offset = size;
    if size >= 128.0 && latin1(&r.read(3, size - 128.0)?) == "TAG" {
        ape_offset -= 128.0;
        let mut lyrics = 0.0;
        if size >= 143.0 {
            let txt = latin1(&r.read(15, size - 143.0)?);
            if txt.get(6..) == Some("LYRICS200") {
                lyrics = crate::js::parse_int(&txt[..6]) + 15.0;
            }
        }
        ape_offset -= lyrics;
    }
    let buf = r.read(32, ape_offset - 32.0)?;
    let footer = ape_footer(&buf);
    Ok((footer.id == "APETAGEX").then(|| {
        let start = ape_offset - footer.size;
        (footer, start)
    }))
}

// ─── ID3v1 ───

fn id3v1_string(b: &[u8]) -> Option<JsString> {
    let s = latin1(b);
    let s = s.split('\0').next().unwrap_or_default();
    let t = js_trim(&units(s));
    (!t.is_empty()).then_some(JsString(t))
}

/// `ID3v1Parser.parse` (which first parses an APEv2 tag found by the header scan).
pub fn id3v1_parse(tok: &mut Tok, c: &mut Collector, opts: &Opts) -> R<()> {
    if tok.size == 0.0 {
        return Ok(());
    }
    if let Some((footer, start)) = &opts.ape_header {
        tok.ignore(*start - tok.position);
        ape_parse_tags(tok, c, footer)?;
    }
    let offset = tok.size - 128.0;
    if tok.position > offset {
        return Ok(());
    }
    let b = tok.token_at(128, offset)?;
    if latin1(&b[..3]) != "TAG" {
        return Ok(());
    }
    for (id, range) in [("title", 3..33), ("artist", 33..63), ("album", 63..93), ("comment", 97..125)] {
        if let Some(v) = id3v1_string(&b[range]) {
            c.add_tag("ID3v1", id, Val::Str(v))?;
        }
    }
    if b[126] != 0 {
        c.add_tag("ID3v1", "track", Val::Num(f64::from(b[126])))?;
    }
    if let Some(v) = id3v1_string(&b[93..97]) {
        c.add_tag("ID3v1", "year", Val::Str(v))?;
    }
    if let Some(g) = super::tables::GENRES.get(b[127] as usize) {
        c.add_tag("ID3v1", "genre", Val::str(g))?;
    }
    Ok(())
}

impl From<std::io::Error> for MmErr {
    fn from(e: std::io::Error) -> Self {
        MmErr::Other(crate::node::FsError::uv_nopath(&e, "read").message)
    }
}
