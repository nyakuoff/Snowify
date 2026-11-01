//! FLAC and Ogg (Vorbis, Opus) from `music-metadata` 7.14: `flac/FlacParser`, `ogg/OggParser`,
//! `ogg/vorbis/*`, `ogg/opus/*`.

use super::{
    Collector, MmErr, Opts, Picture, R, Tok, Val, err,
    id3::{ascii, bits, get_bit, id3v1_parse, latin1, try_read_id3v2, u24be, u32be, u32le},
};
use crate::ipc::JsString;

/// `FourCcToken.get`: `/^[\x21-\x7e©][\x20-\x7e\x00()]{3}/` on the latin1 string.
pub fn fourcc(b: &[u8]) -> R<String> {
    let s = latin1(b);
    let c: Vec<char> = s.chars().collect();
    let first = c.first().is_some_and(|c| ('\x21'..='\x7e').contains(c) || *c == '©');
    let rest = c.len() >= 4 && c[1..4].iter().all(|c| ('\x20'..='\x7e').contains(c) || *c == '\0');
    if first && rest {
        Ok(s)
    } else {
        err(format!("FourCC contains invalid characters: \"{s}\""))
    }
}

/// `Buffer.from(str, 'base64')`: skips characters outside the (standard or URL-safe)
/// alphabet and stops at padding.
fn node_base64(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut n) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(v);
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
        }
    }
    out
}

/// `VorbisPictureToken.get`.
fn vorbis_picture(b: &[u8]) -> R<Picture> {
    let mut off = 4usize;
    let mime_len = u32be(b, off)? as usize;
    off += 4;
    let format = String::from_utf8_lossy(&b[off.min(b.len())..(off + mime_len).min(b.len())]).into_owned();
    off += mime_len;
    let desc_len = u32be(b, off)? as usize;
    off += 4 + desc_len;
    for _ in 0..4 {
        u32be(b, off)?;
        off += 4;
    }
    let data_len = u32be(b, off)? as usize;
    off += 4;
    let data = b[off.min(b.len())..(off + data_len).min(b.len())].to_vec();
    Ok(Picture { format: Some(format), data })
}

/// `VorbisParser.addTag` (base64 pictures decoded) → `metadata.addTag('vorbis', …)`.
fn vorbis_add_tag(c: &mut Collector, id: &str, value: Val) -> R<()> {
    let value = match (id, value) {
        ("METADATA_BLOCK_PICTURE", Val::Str(s)) => Val::Pic(vorbis_picture(&node_base64(&s.to_string_lossy()))?),
        (_, v) => v,
    };
    c.add_tag("vorbis", id, value)
}

/// `VorbisDecoder.readStringUtf8` at `*off`.
fn read_string_utf8(b: &[u8], off: &mut usize) -> R<String> {
    let len = u32le(b, *off)? as usize;
    *off += 4;
    let s = String::from_utf8_lossy(&b[(*off).min(b.len())..(*off + len).min(b.len())]).into_owned();
    *off += len;
    Ok(s)
}

/// `parseUserComment`: `KEY=value`; without '=', the key drops its last character.
fn user_comment(b: &[u8], off: &mut usize) -> R<(String, String)> {
    let v = read_string_utf8(b, off)?;
    let u: Vec<u16> = v.encode_utf16().collect();
    let (key, value) = match u.iter().position(|c| *c == u16::from(b'=')) {
        Some(i) => (&u[..i], &u[i + 1..]),
        None => (&u[..u.len().saturating_sub(1)], &u[..]),
    };
    Ok((String::from_utf16_lossy(key).to_uppercase(), String::from_utf16_lossy(value)))
}

fn user_comment_list(c: &mut Collector, b: &[u8], mut off: usize) -> R<()> {
    let vendor = u32le(b, off)? as usize;
    off += 4 + vendor;
    let mut count = u32le(b, off)?;
    off += 4;
    while count > 0.0 {
        count -= 1.0;
        let (k, v) = user_comment(b, &mut off)?;
        vorbis_add_tag(c, &k, Val::Str(JsString::from(v)))?;
    }
    Ok(())
}

// ─── FLAC ───

pub fn parse_flac(tok: &mut Tok, c: &mut Collector, opts: &Opts) -> R<()> {
    let r = (|| {
        try_read_id3v2(tok, c)?;
        flac_blocks(tok, c)?;
        id3v1_parse(tok, c, opts)?;
        Ok(())
    })();
    match r {
        Err(MmErr::Eos) => Ok(()),
        r => r,
    }
}

fn flac_blocks(tok: &mut Tok, c: &mut Collector) -> R<()> {
    if fourcc(&tok.token(4)?)? != "fLaC" {
        return err("Invalid FLAC preamble");
    }
    loop {
        let h = tok.token(4)?;
        let last = get_bit(&h, 0, 7);
        let kind = bits(&h, 0, 1, 7);
        let len = u24be(&h, 1)?;
        match kind {
            0 => {
                if len != 34.0 {
                    return err("Unexpected block-stream-info length");
                }
                let si = tok.token(34)?;
                let sample_rate = f64::from((u24be(&si, 10)? as i32) >> 4);
                c.format.sample_rate = Some(sample_rate);
                let total = f64::from(bits(&si, 13, 4, 36));
                if total > 0.0 {
                    c.format.duration = Some(total / sample_rate);
                }
            }
            4 => {
                let data = tok.token(len as usize)?;
                let mut off = 0;
                read_string_utf8(&data, &mut off)?;
                let count = u32le(&data, off)?;
                off += 4;
                let mut i = 0.0;
                while i < count {
                    i += 1.0;
                    let (k, v) = user_comment(&data, &mut off)?;
                    vorbis_add_tag(c, &k, Val::Str(JsString::from(v)))?;
                }
            }
            6 => {
                let data = tok.token(len as usize)?;
                let pic = vorbis_picture(&data)?;
                vorbis_add_tag(c, "METADATA_BLOCK_PICTURE", Val::Pic(pic))?;
            }
            _ => {
                tok.ignore(len);
            }
        }
        if last {
            return Ok(());
        }
    }
}

// ─── Ogg ───

#[derive(PartialEq, Clone, Copy)]
enum Codec {
    Vorbis,
    Opus,
    /// `SpeexParser`: Vorbis page handling with its own 80-byte header.
    Speex,
    /// `TheoraParser`: reads the first page only; flush and duration are no-ops.
    Theora,
}

struct Consumer {
    codec: Codec,
    segments: Vec<Vec<u8>>,
    pre_skip: f64,
}

struct Page {
    continued: bool,
    first: bool,
    last: bool,
    granule: f64,
    seq: f64,
}

impl Consumer {
    fn parse_first(&mut self, c: &mut Collector, data: &[u8]) -> R<()> {
        match self.codec {
            Codec::Vorbis => {
                let packet = *data.first().ok_or_else(|| MmErr::Other("RangeError [ERR_BUFFER_OUT_OF_BOUNDS]".into()))?;
                if ascii(&data[1.min(data.len())..7.min(data.len())]) != "vorbis" {
                    return err("Metadata does not look like Vorbis");
                }
                if packet != 1 {
                    return err("First Ogg page should be type 1: the identification header");
                }
                if data.len() < 7 + 21 {
                    return err("RangeError: Offset is outside the bounds of the DataView");
                }
                c.format.sample_rate = Some(u32le(data, 12)?);
            }
            Codec::Opus => {
                if data.len() < 19 {
                    return err("ID-header-page 0 should be at least 19 bytes long");
                }
                self.pre_skip = f64::from(i16::from_le_bytes([data[10], data[11]]));
                let rate = f64::from(i32::from_le_bytes([data[12], data[13], data[14], data[15]]));
                if ascii(&data[..8]) != "OpusHead" {
                    return err("Illegal ogg/Opus magic-signature");
                }
                c.format.sample_rate = Some(rate);
            }
            Codec::Speex => {
                if data.len() < 80 {
                    return err("RangeError [ERR_OUT_OF_RANGE]: The value of \"offset\" is out of range.");
                }
                c.format.sample_rate = Some(f64::from(i32::from_le_bytes([data[36], data[37], data[38], data[39]])));
            }
            Codec::Theora => {
                if data.len() < 41 {
                    return err("RangeError [ERR_OUT_OF_RANGE]: The value of \"offset\" is out of range.");
                }
            }
        }
        Ok(())
    }

    fn parse_full(&mut self, c: &mut Collector, data: &[u8]) -> R<()> {
        match self.codec {
            Codec::Theora => {}
            Codec::Vorbis | Codec::Speex => {
                let packet = *data.first().ok_or_else(|| MmErr::Other("RangeError [ERR_BUFFER_OUT_OF_BOUNDS]".into()))?;
                if packet == 3 {
                    user_comment_list(c, data, 7)?;
                }
            }
            Codec::Opus => {
                if ascii(&data[..8.min(data.len())]) == "OpusTags" {
                    user_comment_list(c, data, 8)?;
                }
            }
        }
        Ok(())
    }

    fn parse_page(&mut self, c: &mut Collector, h: &Page, data: Vec<u8>) -> R<()> {
        if self.codec == Codec::Theora {
            return if h.first { self.parse_first(c, &data) } else { Ok(()) };
        }
        if h.first {
            self.parse_first(c, &data)?;
        } else {
            if h.continued {
                if self.segments.is_empty() {
                    return err("Cannot continue on previous page");
                }
                self.segments.push(data.clone());
            }
            if h.last || !h.continued {
                if !self.segments.is_empty() {
                    let full = self.segments.concat();
                    self.parse_full(c, &full)?;
                }
                self.segments = if h.last { Vec::new() } else { vec![data] };
            }
        }
        if h.last {
            self.duration(c, h);
        }
        Ok(())
    }

    fn duration(&self, c: &mut Collector, h: &Page) {
        let rate = c.format.sample_rate.filter(|r| *r != 0.0 && !r.is_nan());
        if rate.is_some() && h.granule >= 0.0 {
            c.format.duration = Some(match self.codec {
                Codec::Vorbis | Codec::Speex => h.granule / rate.unwrap_or(f64::NAN),
                Codec::Opus => (h.granule - self.pre_skip) / 48000.0,
                Codec::Theora => return,
            });
        }
    }
}

pub fn parse_ogg(tok: &mut Tok, c: &mut Collector) -> R<()> {
    let mut consumer: Option<Consumer> = None;
    let mut last_header: Option<Page> = None;
    let r = (|| -> R<()> {
        loop {
            let raw = tok.token(27)?;
            if fourcc(&raw[..4])? != "OggS" {
                return err("Invalid Ogg capture pattern");
            }
            let flags = raw[5];
            let granule = u64::from_le_bytes(raw[6..14].try_into().unwrap_or([0; 8])) as f64;
            let header = Page {
                continued: flags & 1 != 0,
                first: flags & 2 != 0,
                last: flags & 4 != 0,
                granule,
                seq: u32le(&raw, 18)?,
            };
            // `this.header = header` happens before the segment table is read.
            last_header = Some(Page { continued: header.continued, first: header.first, last: header.last, granule: header.granule, seq: header.seq });
            let segs = tok.token(raw[26] as usize)?;
            let total: usize = segs.iter().map(|s| *s as usize).sum();
            let data = tok.token(total)?;
            if header.first {
                let id = ascii(&data[..7.min(data.len())]);
                let codec = match id.as_str() {
                    "\x01vorbis" => Codec::Vorbis,
                    "OpusHea" => Codec::Opus,
                    "Speex  " => Codec::Speex,
                    "fishead" | "\x00theora" => Codec::Theora,
                    _ => return err(format!("gg audio-codec not recognized (id={id})")),
                };
                consumer = Some(Consumer { codec, segments: Vec::new(), pre_skip: 0.0 });
            }
            let Some(cons) = consumer.as_mut() else {
                return err("TypeError: Cannot read properties of undefined (reading 'parsePage')");
            };
            let last = header.last;
            cons.parse_page(c, &header, data)?;
            if last {
                return Ok(());
            }
        }
    })();
    match r {
        Err(MmErr::Eos) => {
            if let Some(h) = &last_header {
                match &consumer {
                    Some(cons) => cons.duration(c, h),
                    None => return err("TypeError: Cannot read properties of undefined (reading 'calculateDuration')"),
                }
            }
            Ok(())
        }
        Err(MmErr::Other(m)) if m.starts_with("FourCC") => {
            if last_header.as_ref().is_some_and(|h| h.seq > 0.0)
                && let Some(cons) = consumer.as_mut()
            {
                if cons.codec != Codec::Theora {
                    let full = cons.segments.concat();
                    cons.parse_full(c, &full)?;
                }
            }
            Ok(())
        }
        r => r,
    }
}
