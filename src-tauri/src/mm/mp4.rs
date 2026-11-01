//! `MP4Parser` (m4a/mp4/…) from `music-metadata` 7.14: the atom walk (`mp4/Atom.js`), the atom
//! parsers that affect duration and tags, `ilst` metadata items, and the end-of-parse track
//! summary. Throws where the source throws (several atom tokens and table checks).

use super::{
    Collector, MmErr, Picture, R, Tok, Val, err,
    id3::{ascii, latin1, u24be, u32be, u8_at},
    xiph::fourcc,
};
use crate::ipc::JsString;

fn i16be(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 2).map(|s| f64::from(i16::from_be_bytes([s[0], s[1]]))).ok_or_else(range)
}

fn u16be(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 2).map(|s| f64::from(u16::from_be_bytes([s[0], s[1]]))).ok_or_else(range)
}

fn i32be(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 4).map(|s| f64::from(i32::from_be_bytes([s[0], s[1], s[2], s[3]]))).ok_or_else(range)
}

fn range() -> MmErr {
    MmErr::Other("RangeError: Offset is outside the bounds of the DataView".into())
}

/// `Buffer.alloc(len)` throws for a negative size.
fn alloc_len(len: f64) -> R<usize> {
    if len < 0.0 || len.is_nan() {
        return err("RangeError [ERR_INVALID_ARG_VALUE]: The argument 'size' is invalid");
    }
    Ok(len as usize)
}

struct AtomHeader {
    /// `BigInt` length; 0 means "to the end".
    length: f64,
    name: String,
    extended: bool,
}

impl AtomHeader {
    fn header_len(&self) -> f64 {
        if self.extended { 16.0 } else { 8.0 }
    }
    fn payload(&self, remaining: f64) -> f64 {
        (if self.length == 0.0 { remaining } else { self.length }) - self.header_len()
    }
}

#[derive(Default)]
struct Track {
    time_scale: Option<f64>,
    duration: Option<f64>,
    /// `None` until an `stsd` atom is seen (the summary then throws, as the source does).
    ssd: Option<Vec<SoundDesc>>,
    stts: Option<Vec<(f64, f64)>>,
}

#[derive(Clone)]
struct SoundDesc {
    /// `description`: (numAudioChannels, sampleRate); absent for other versions.
    description: Option<(f64, f64)>,
}

struct Mp4 {
    tracks: Vec<Track>,
}

pub fn parse(tok: &mut Tok, c: &mut Collector) -> R<()> {
    let mut p = Mp4 { tracks: Vec::new() };
    let mut remaining = tok.size;
    while tok.size == 0.0 || remaining > 0.0 {
        match tok.peek_token(8) {
            Ok(h) if latin1(&h[4..8]) == "\0\0\0\0" => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let atom = read_atom(tok, c, &mut p, None, remaining)?;
        remaining -= if atom.length == 0.0 { remaining } else { atom.length };
    }
    // Summary: every track must have a sample description list.
    for t in &p.tracks {
        if t.ssd.is_none() {
            return err("TypeError: Cannot read properties of undefined (reading 'forEach')");
        }
    }
    let audio = p.tracks.iter().find(|t| {
        t.ssd.as_ref().and_then(|s| s.first()).and_then(|d| d.description).is_some_and(|(ch, _)| ch > 0.0)
    });
    if let Some(t) = audio {
        if t.time_scale.is_some_and(|s| s > 0.0) {
            c.format.duration = Some(t.duration.unwrap_or(f64::NAN) / t.time_scale.unwrap_or(f64::NAN));
        }
        let (_, rate) = t.ssd.as_ref().and_then(|s| s.first()).and_then(|d| d.description).unwrap_or((0.0, f64::NAN));
        c.format.sample_rate = Some(rate);
        // `timeToSampleTable.length` is read unguarded when the time scale is 0.
        if t.time_scale == Some(0.0) {
            let Some(stts) = &t.stts else {
                return err("TypeError: Cannot read properties of undefined (reading 'length')");
            };
            if stts.is_empty() {
                return Ok(());
            }
            let total: f64 = stts.iter().map(|(count, dur)| count * dur).sum();
            c.format.duration = Some(total / rate);
        }
    }
    Ok(())
}

fn read_atom(tok: &mut Tok, c: &mut Collector, p: &mut Mp4, parent: Option<&str>, remaining: f64) -> R<AtomHeader> {
    let h = tok.token(8)?;
    let mut header = AtomHeader { length: u32be(&h, 0)?, name: latin1(&h[4..8]), extended: false };
    if header.length == 1.0 {
        let ext = tok.token(8)?;
        header.length = u64::from_be_bytes(ext[..8].try_into().unwrap_or([0; 8])) as f64;
        // `header.length === BigInt(1)` is re-checked after the update: only a 64-bit
        // length of exactly 1 counts as extended.
        header.extended = header.length == 1.0;
    }
    let payload = header.payload(remaining);
    read_data(tok, c, p, &header, parent, payload)?;
    Ok(header)
}

fn read_atoms(tok: &mut Tok, c: &mut Collector, p: &mut Mp4, parent: &str, mut size: f64) -> R<()> {
    while size > 0.0 {
        let atom = read_atom(tok, c, p, Some(parent), size)?;
        size -= if atom.length == 0.0 { size } else { atom.length };
    }
    Ok(())
}

fn read_data(tok: &mut Tok, c: &mut Collector, p: &mut Mp4, a: &AtomHeader, parent: Option<&str>, remaining: f64) -> R<()> {
    match a.name.as_str() {
        "moov" | "udta" | "trak" | "mdia" | "minf" | "stbl" | "<id>" | "ilst" | "tref" => {
            read_atoms(tok, c, p, &a.name.clone(), a.payload(remaining))
        }
        "meta" => {
            let peek = tok.peek_token(8)?;
            let padding = if latin1(&peek[4..8]) == "hdlr" { 0.0 } else { 4.0 };
            tok.ignore(padding);
            read_atoms(tok, c, p, "meta", a.payload(remaining) - padding)
        }
        _ => handle_atom(tok, c, p, a, parent, remaining),
    }
}

fn track<'a>(p: &'a mut Mp4, atom: &str) -> R<&'a mut Track> {
    p.tracks.last_mut().ok_or_else(|| MmErr::Other(format!("TypeError: Cannot set properties of undefined (in {atom})")))
}

/// A `SimpleTableAtom` / `StszAtom` entry list; the count must match the remaining length.
fn table(b: &[u8], off: usize, remaining: f64, n: f64, width: f64) -> R<usize> {
    if remaining == 0.0 {
        return Ok(0);
    }
    if remaining != n * width {
        return err("mismatch number-of-entries with remaining atom-length");
    }
    let _ = (b, off);
    Ok(n as usize)
}

fn handle_atom(tok: &mut Tok, c: &mut Collector, p: &mut Mp4, a: &AtomHeader, parent: Option<&str>, remaining: f64) -> R<()> {
    if matches!(parent, Some("ilst" | "<id>")) {
        return parse_item(tok, c, p, a);
    }
    let fixed = |len: f64, exp: f64, id: &str| -> R<()> {
        if len < exp { err(format!("Atom {id} expected to be {exp}, but specifies {len} bytes long.")) } else { Ok(()) }
    };
    match a.name.as_str() {
        "mvhd" => {
            fixed(remaining, 100.0, "mvhd")?;
            tok.token(alloc_len(remaining)?)?;
        }
        "mdhd" => {
            fixed(remaining, 24.0, "mdhd")?;
            let b = tok.token(alloc_len(remaining)?)?;
            let (scale, dur) = (u32be(&b, 12)?, u32be(&b, 16)?);
            let t = track(p, "mdhd")?;
            t.time_scale = Some(scale);
            t.duration = Some(dur);
        }
        "chap" => {
            let mut len = remaining;
            track(p, "chap")?;
            while len >= 4.0 {
                tok.token(4)?;
                len -= 4.0;
            }
        }
        "tkhd" => {
            let b = tok.token(alloc_len(remaining)?)?;
            u8_at(&b, 0)?;
            u24be(&b, 1)?;
            let d = u32be(&b, 20)?;
            // TrackHeaderAtom also reads layer/alternateGroup/volume, up to offset 30.
            u16be(&b, 28)?;
            p.tracks.push(Track { duration: Some(d), ..Track::default() });
        }
        "mdat" => {
            tok.ignore(remaining);
        }
        "ftyp" => {
            let mut len = remaining;
            while len > 0.0 {
                tok.token(4)?;
                len -= 4.0;
            }
        }
        "stsd" => {
            let b = tok.token(alloc_len(remaining)?)?;
            let n = u32be(&b, 4)?;
            let mut off = 8usize;
            let mut descs = Vec::new();
            let mut i = 0.0;
            while i < n {
                i += 1.0;
                let size = u32be(&b, off)?;
                off += 4;
                fourcc(b.get(off..off + 4).ok_or_else(range)?)?;
                u16be(&b, off + 10)?;
                let dlen = (size - 12.0).max(0.0) as usize;
                let start = (off + 12).min(b.len());
                let desc = &b[start..(start + dlen).min(b.len())];
                // parseSoundSampleDescription
                let version = i16be(desc, 0)?;
                i16be(desc, 2)?;
                i32be(desc, 4)?;
                let description = if version == 0.0 || version == 1.0 {
                    let ch = i16be(desc, 8)?;
                    i16be(desc, 10)?;
                    i16be(desc, 12)?;
                    i16be(desc, 14)?;
                    let rate = u16be(desc, 16)? + u16be(desc, 18)? / 10000.0;
                    Some((ch, rate))
                } else {
                    None
                };
                descs.push(SoundDesc { description });
                off += size as usize;
            }
            track(p, "stsd")?.ssd = Some(descs);
        }
        "stsc" | "stco" => {
            let b = tok.token(alloc_len(remaining)?)?;
            let n = i32be(&b, 4)?;
            let width = if a.name == "stsc" { 12.0 } else { 4.0 };
            table(&b, 8, remaining - 8.0, n, width)?;
            track(p, &a.name)?;
        }
        "stts" => {
            let b = tok.token(alloc_len(remaining)?)?;
            let n = i32be(&b, 4)?;
            let count = table(&b, 8, remaining - 8.0, n, 8.0)?;
            let entries = (0..count).map(|k| Ok((i32be(&b, 8 + k * 8)?, i32be(&b, 12 + k * 8)?))).collect::<R<Vec<_>>>()?;
            track(p, "stts")?.stts = Some(entries);
        }
        "stsz" => {
            let b = tok.token(alloc_len(remaining)?)?;
            let n = i32be(&b, 8)?;
            i32be(&b, 4)?;
            table(&b, 12, remaining - 12.0, n, 4.0)?;
            track(p, "stsz")?;
        }
        "date" => {
            let b = tok.token(alloc_len(remaining)?)?;
            c.add_tag("iTunes", "date", Val::Str(JsString::from(String::from_utf8_lossy(&b).into_owned())))?;
        }
        _ => {
            tok.ignore(remaining);
        }
    }
    Ok(())
}

/// `parseMetadataItemData`: the children (`data`, `name`, `mean`, `rate`) of one item.
fn parse_item(tok: &mut Tok, c: &mut Collector, _p: &mut Mp4, item: &AtomHeader) -> R<()> {
    let mut key = item.name.clone();
    let mut size = item.payload(0.0);
    while size > 0.0 {
        let h = tok.token(8)?;
        let mut child = AtomHeader { length: u32be(&h, 0)?, name: latin1(&h[4..8]), extended: false };
        if child.length == 1.0 {
            let ext = tok.token(8)?;
            child.length = u64::from_be_bytes(ext[..8].try_into().unwrap_or([0; 8])) as f64;
            child.extended = child.length == 1.0;
        }
        let payload = child.payload(size);
        match child.name.as_str() {
            "data" => value_atom(tok, c, &key, &child)?,
            "name" | "mean" | "rate" => {
                let b = tok.token(alloc_len(payload)?)?;
                u8_at(&b, 0)?;
                u24be(&b, 1)?;
                let name = String::from_utf8_lossy(b.get(4..).unwrap_or_default()).into_owned();
                key = format!("{key}:{name}");
            }
            _ => {
                tok.token(alloc_len(payload)?)?;
            }
        }
        size -= if child.length == 0.0 { size } else { child.length };
    }
    Ok(())
}

fn value_atom(tok: &mut Tok, c: &mut Collector, key: &str, a: &AtomHeader) -> R<()> {
    let b = tok.token(alloc_len(a.length - 8.0)?)?;
    let set = u8_at(&b, 0)?;
    let kind = u24be(&b, 1)?;
    u24be(&b, 4)?;
    let value = b.get(8..).unwrap_or_default();
    if set != 0.0 {
        return err(format!("Unsupported type-set != 0: {set}"));
    }
    match kind as u32 {
        0 => match key {
            "trkn" | "disk" => {
                let (n, of) = (u8_at(value, 3)?, u8_at(value, 5)?);
                c.add_tag("iTunes", key, Val::str(&format!("{n}/{of}")))?;
            }
            "gnre" => {
                let g = u8_at(value, 1)? as usize;
                let name = g.checked_sub(1).and_then(|i| super::tables::GENRES.get(i));
                c.add_tag("iTunes", key, name.map_or(Val::Undef, |g| Val::str(g)))?;
            }
            "rate" => c.add_tag("iTunes", key, Val::str(&ascii(value)))?,
            _ => {}
        },
        1 | 18 => c.add_tag("iTunes", key, Val::Str(JsString::from(String::from_utf8_lossy(value).into_owned())))?,
        13 => c.add_tag("iTunes", key, Val::Pic(Picture { format: Some("image/jpeg".into()), data: value.to_vec() }))?,
        14 => c.add_tag("iTunes", key, Val::Pic(Picture { format: Some("image/png".into()), data: value.to_vec() }))?,
        21 | 22 => {
            let n = match value.len() {
                1 => f64::from(if kind == 21.0 { value[0] as i8 as i32 } else { i32::from(value[0]) }),
                2 if kind == 21.0 => i16be(value, 0)?,
                2 => u16be(value, 0)?,
                3 => {
                    let u = u24be(value, 0)?;
                    if kind == 21.0 && u >= 8_388_608.0 { u - 16_777_216.0 } else { u }
                }
                4 if kind == 21.0 => i32be(value, 0)?,
                4 => u32be(value, 0)?,
                8 => {
                    let raw: [u8; 8] = value[..8].try_into().unwrap_or([0; 8]);
                    if kind == 21.0 { i64::from_be_bytes(raw) as f64 } else { u64::from_be_bytes(raw) as f64 }
                }
                n => {
                    let t = format!("{}{}{}", if kind == 21.0 { "INT" } else { "UINT" }, n * 8, if n > 1 { "_BE" } else { "" });
                    return err(format!("Token for integer type not found: \"{t}\""));
                }
            };
            c.add_tag("iTunes", key, Val::Num(n))?;
        }
        65 => {
            let v = *value.first().ok_or_else(range)? as i8;
            c.add_tag("iTunes", key, Val::Num(f64::from(v)))?;
        }
        66 => c.add_tag("iTunes", key, Val::Num(i16be(value, 0)?))?,
        67 => c.add_tag("iTunes", key, Val::Num(i32be(value, 0)?))?,
        _ => {}
    }
    Ok(())
}
