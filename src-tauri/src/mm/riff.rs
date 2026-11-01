//! WAV (`wav/WaveParser`) and AIFF (`aiff/AiffParser`) from `music-metadata` 7.14.

use super::{
    Collector, MmErr, R, Tok, Val, err,
    id3::{ascii, id3v2_parse, latin1, u32be, u32le},
    xiph::fourcc,
};
use crate::{ipc::JsString, js::math_round};

fn u16le(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 2).map(|s| f64::from(u16::from_le_bytes([s[0], s[1]]))).ok_or_else(range)
}

fn u16be(b: &[u8], o: usize) -> R<f64> {
    b.get(o..o + 2).map(|s| f64::from(u16::from_be_bytes([s[0], s[1]]))).ok_or_else(range)
}

fn range() -> MmErr {
    MmErr::Other("RangeError [ERR_OUT_OF_RANGE]".into())
}

fn alloc(len: f64) -> R<usize> {
    if len < 0.0 { err("RangeError [ERR_INVALID_ARG_VALUE]: The argument 'size' is invalid") } else { Ok(len as usize) }
}

/// `util.stripNulls`: leading and trailing NULs.
fn strip_nulls(s: &str) -> &str {
    s.trim_matches('\0')
}

/// An ID3v2 tag stored in a chunk, parsed on its own buffer tokenizer.
fn id3_chunk(tok: &mut Tok, c: &mut Collector, size: f64) -> R<()> {
    let data = tok.token(alloc(size)?)?;
    id3v2_parse(&mut Tok::from_buffer(data), c)
}

// ─── WAV ───

pub fn parse_wav(tok: &mut Tok, c: &mut Collector) -> R<()> {
    let h = tok.token(8)?;
    if latin1(&h[..4]) != "RIFF" {
        return Ok(());
    }
    let size = u32le(&h, 4)?;
    match riff_chunk(tok, c, size) {
        Err(MmErr::Eos) => Ok(()),
        r => r,
    }
}

fn riff_chunk(tok: &mut Tok, c: &mut Collector, chunk_size: f64) -> R<()> {
    let kind = fourcc(&tok.token(4)?)?;
    if kind != "WAVE" {
        return err(format!("Unsupported RIFF format: RIFF/{kind}"));
    }
    let mut remaining = chunk_size - 4.0;
    let mut block_align: Option<f64> = None;
    let mut fact: Option<f64> = None;
    while remaining >= 8.0 {
        let h = tok.token(8)?;
        let id = latin1(&h[..4]);
        let size = u32le(&h, 4)?;
        remaining -= 8.0 + size;
        match id.as_str() {
            "LIST" => {
                let list_type = latin1(&tok.token(4)?);
                if list_type == "INFO" {
                    let mut left = size - 4.0;
                    while left >= 8.0 {
                        let th = tok.token(8)?;
                        let tag_id = latin1(&th[..4]);
                        let tag_size = u32le(&th, 4)?;
                        let len = tag_size + f64::from((tag_size as i64 & 1) as i32);
                        let value = tok.token(alloc(len)?)?;
                        let text = ascii(&value[..(tag_size as usize).min(value.len())]);
                        c.add_tag("exif", &tag_id, Val::Str(JsString::from(strip_nulls(&text))))?;
                        left -= 8.0 + len;
                    }
                    if left != 0.0 {
                        return err(format!("Illegal remaining size: {left}"));
                    }
                } else {
                    tok.ignore(size - 4.0);
                }
            }
            "fact" => {
                if size < 4.0 {
                    return err("Invalid fact chunk size.");
                }
                let b = tok.token(alloc(size)?)?;
                fact = Some(u32le(&b, 0)?);
            }
            "fmt " => {
                if size < 16.0 {
                    return err("Invalid chunk size");
                }
                let b = tok.token(alloc(size)?)?;
                c.format.sample_rate = Some(u32le(&b, 4)?);
                block_align = Some(u16le(&b, 12)?);
                u16le(&b, 14)?;
            }
            "id3 " | "ID3 " => id3_chunk(tok, c, size)?,
            "data" => {
                let mut chunk = size;
                if tok.size != 0.0 {
                    let left = tok.size - tok.position;
                    if left < chunk {
                        chunk = left;
                    }
                }
                let samples = match fact {
                    Some(f) => Some(f),
                    None if chunk == 4_294_967_295.0 => None,
                    None => Some(chunk / block_align.unwrap_or(f64::NAN)),
                };
                if let Some(n) = samples.filter(|n| *n != 0.0 && !n.is_nan()) {
                    c.format.number_of_samples = Some(n);
                    c.format.duration = Some(n / c.format.sample_rate.unwrap_or(f64::NAN));
                }
                tok.ignore(size);
            }
            "bext" => {
                tok.token(420)?;
                tok.ignore(size - 420.0);
            }
            _ => {
                tok.ignore(size);
            }
        }
        if size % 2.0 == 1.0 {
            tok.ignore(1.0);
        }
    }
    Ok(())
}

// ─── AIFF ───

pub fn parse_aiff(tok: &mut Tok, c: &mut Collector) -> R<()> {
    let h = tok.token(8)?;
    if fourcc(&h[..4])? != "FORM" {
        return err("Invalid Chunk-ID, expected 'FORM'");
    }
    let kind = fourcc(&tok.token(4)?)?;
    let compressed = match kind.as_str() {
        "AIFF" => false,
        "AIFC" => true,
        _ => return err(format!("Unsupported AIFF type: {kind}")),
    };
    let r = (|| -> R<()> {
        while tok.size == 0.0 || tok.size - tok.position >= 8.0 {
            let ch = tok.token(8)?;
            let id = fourcc(&ch[..4])?;
            let size = u32be(&ch, 4)?;
            let next = 2.0 * math_round(size / 2.0);
            let read = match id.as_str() {
                "COMM" => {
                    let min = if compressed { 22.0 } else { 18.0 };
                    if size < min {
                        return err(format!("COMMON CHUNK size should always be at least {min}"));
                    }
                    let b = tok.token(alloc(size)?)?;
                    let shift = u16be(&b, 8)? as i32 - 16398;
                    let base = u16be(&b, 10)? as i32;
                    let rate = f64::from(if shift < 0 { base >> (shift.unsigned_abs() & 31) } else { base.wrapping_shl(shift as u32 & 31) });
                    let frames = u32be(&b, 2)?;
                    u16be(&b, 0)?;
                    u16be(&b, 6)?;
                    if compressed {
                        fourcc(b.get(18..22).ok_or_else(range)?)?;
                        if size > 22.0 {
                            let n = i32::from(*b.get(22).ok_or_else(range)? as i8);
                            if n > 0 && f64::from(23 + n + (n + 1) % 2) != size {
                                return err("Illegal pstring length");
                            }
                        }
                    }
                    c.format.sample_rate = Some(rate);
                    c.format.number_of_samples = Some(frames);
                    c.format.duration = Some(frames / rate);
                    size
                }
                "ID3 " => {
                    id3_chunk(tok, c, size)?;
                    size
                }
                "SSND" => 0.0,
                "NAME" | "AUTH" | "(c) " | "ANNO" => {
                    let b = tok.token(alloc(size)?)?;
                    for v in ascii(&b).split('\0') {
                        let t = super::id3::js_trim(&v.encode_utf16().collect::<Vec<_>>());
                        if !t.is_empty() {
                            c.add_tag("AIFF", &id, Val::Str(JsString(t)))?;
                        }
                    }
                    size
                }
                _ => 0.0,
            };
            tok.ignore(next - read);
        }
        Ok(())
    })();
    match r {
        Err(MmErr::Eos) => Ok(()),
        r => r,
    }
}
