//! `MpegParser` (MP3/MP2 and ADTS AAC) from `music-metadata` 7.14, with `{ duration: true }`.
//! A state-for-state port: the sync scan over a reused 1024-byte buffer, frame counting that
//! resets after an unparseable resync, Xing/Info/LAME headers, and the CBR estimate from the
//! file size in `finalize`.

use super::{
    Collector, MmErr, Opts, R, Tok,
    id3::{ascii, bits, get_bit, id3v1_parse, try_read_id3v2, u32be},
};
use crate::js::{math_round, parse_int};

const MAX_PEEK: usize = 1024;

#[derive(Clone)]
struct Header {
    version: Option<f64>,
    layer: u8,
    bitrate: f64,
    sampling_rate: Option<f64>,
    padding: bool,
    channel_mode_index: i32,
    protected_by_crc: bool,
    frame_length: i32,
}

const VERSION_ID: [Option<f64>; 4] = [Some(2.5), None, Some(2.0), Some(1.0)];
const LAYER: [u8; 4] = [0, 3, 2, 1];
const ADTS_RATES: [Option<f64>; 16] = [
    Some(96000.0), Some(88200.0), Some(64000.0), Some(48000.0), Some(44100.0), Some(32000.0), Some(24000.0),
    Some(22050.0), Some(16000.0), Some(12000.0), Some(11025.0), Some(8000.0), Some(7350.0), None, None, Some(-1.0),
];

fn bitrate_kbps(index: i32, codec: &str) -> Option<f64> {
    const T: [[f64; 6]; 14] = [
        [32., 32., 32., 32., 8., 8.],
        [64., 48., 40., 48., 16., 16.],
        [96., 56., 48., 56., 24., 24.],
        [128., 64., 56., 64., 32., 32.],
        [160., 80., 64., 80., 40., 40.],
        [192., 96., 80., 96., 48., 48.],
        [224., 112., 96., 112., 56., 56.],
        [256., 128., 112., 128., 64., 64.],
        [288., 160., 128., 144., 80., 80.],
        [320., 192., 160., 160., 96., 96.],
        [352., 224., 192., 176., 112., 112.],
        [384., 256., 224., 192., 128., 128.],
        [416., 320., 256., 224., 144., 144.],
        [448., 384., 320., 256., 160., 160.],
    ];
    let col = ["11", "12", "13", "21", "22", "23"].iter().position(|c| *c == codec)?;
    (1..=14).contains(&index).then(|| T[index as usize - 1][col])
}

impl Header {
    fn parse(b: &[u8; 4]) -> R<Self> {
        let version_index = bits(b, 1, 3, 2);
        let layer = LAYER[bits(b, 1, 5, 2) as usize];
        let protected_by_crc = !get_bit(b, 1, 0);
        if version_index > 1 && layer == 0 {
            return Ok(Self {
                version: Some(if version_index == 2 { 4.0 } else { 2.0 }),
                layer,
                bitrate: 0.0,
                sampling_rate: ADTS_RATES[bits(b, 2, 2, 4) as usize],
                padding: false,
                channel_mode_index: 0,
                protected_by_crc,
                frame_length: bits(b, 3, 6, 2).wrapping_shl(11),
            });
        }
        let bitrate_index = bits(b, 2, 0, 4);
        let rate_index = bits(b, 2, 4, 2);
        let version = VERSION_ID[version_index as usize];
        let codec = format!("{}{layer}", version.map_or(0.0, f64::floor) as i64);
        let kbps = if bitrate_index == 0 || bitrate_index == 0x0F { None } else { bitrate_kbps(bitrate_index, &codec) };
        let Some(kbps) = kbps.filter(|k| *k != 0.0) else {
            return Err(MmErr::Other("Cannot determine bit-rate".into()));
        };
        let sampling_rate = match (rate_index, version) {
            (3, _) => None,
            (i, Some(v)) => {
                let t: [f64; 3] = if v == 1.0 { [44100., 48000., 32000.] } else if v == 2.0 { [22050., 24000., 16000.] } else { [11025., 12000., 8000.] };
                Some(t[i as usize])
            }
            _ => None,
        };
        if sampling_rate.is_none() {
            return Err(MmErr::Other("Cannot determine sampling-rate".into()));
        }
        Ok(Self {
            version,
            layer,
            bitrate: kbps * 1000.0,
            sampling_rate,
            padding: bits(b, 2, 6, 1) == 1,
            channel_mode_index: bits(b, 3, 0, 2),
            protected_by_crc,
            frame_length: 0,
        })
    }

    fn samples_per_frame(&self) -> f64 {
        let row = if self.version == Some(1.0) { [0., 384., 1152., 1152.] } else { [0., 384., 1152., 576.] };
        row[self.layer as usize]
    }

    fn side_info_length(&self) -> Option<usize> {
        if self.layer != 3 {
            return Some(2);
        }
        let v = self.version?;
        Some(match (self.channel_mode_index == 3, v == 1.0) {
            (true, true) => 17,
            (true, false) => 9,
            (false, true) => 32,
            (false, false) => 17,
        })
    }
}

struct State {
    frame_count: f64,
    sync_frame_count: f64,
    total_data_length: f64,
    bitrates: Vec<f64>,
    calculate_eof_duration: bool,
    header_buf: [u8; 4],
    peek_buf: Vec<u8>,
    mpeg_offset: f64,
    frame_size: f64,
    audio_header: Option<Header>,
    samples_per_frame: Option<f64>,
    offset: f64,
}

/// The `parse()` of `AbstractID3Parser` for MPEG: an EndOfStreamError anywhere ends it quietly.
pub fn parse(tok: &mut Tok, c: &mut Collector, opts: &Opts) -> R<()> {
    let mut st = State {
        frame_count: 0.0,
        sync_frame_count: -1.0,
        total_data_length: 0.0,
        bitrates: Vec::new(),
        calculate_eof_duration: false,
        header_buf: [0; 4],
        peek_buf: vec![0; MAX_PEEK],
        mpeg_offset: 0.0,
        frame_size: 0.0,
        audio_header: None,
        samples_per_frame: None,
        offset: 0.0,
    };
    let r = (|| {
        try_read_id3v2(tok, c)?;
        post_id3v2(tok, c, &mut st)?;
        id3v1_parse(tok, c, opts)?;
        finalize(tok, c, &st);
        Ok(())
    })();
    match r {
        Err(MmErr::Eos) => Ok(()),
        r => r,
    }
}

fn post_id3v2(tok: &mut Tok, c: &mut Collector, st: &mut State) -> R<()> {
    let r = (|| -> R<()> {
        loop {
            sync(tok, st)?;
            if common_header(tok, c, st)? {
                return Ok(());
            }
        }
    })();
    match r {
        Err(MmErr::Eos) => {
            if st.calculate_eof_duration {
                let samples = st.frame_count * st.samples_per_frame.unwrap_or(f64::NAN);
                c.format.number_of_samples = Some(samples);
                c.format.duration = Some(samples / c.format.sample_rate.unwrap_or(f64::NAN));
            }
            Ok(())
        }
        r => r,
    }
}

fn sync(tok: &mut Tok, st: &mut State) -> R<()> {
    let mut got_first_sync = false;
    loop {
        let mut bo: usize = 0;
        let peeked = tok.peek(MAX_PEEK, None, true)?;
        let len = peeked.len();
        st.peek_buf[..len].copy_from_slice(&peeked);
        if len <= 163 {
            return Err(MmErr::Eos);
        }
        loop {
            let at = st.peek_buf.get(bo).copied();
            if got_first_sync && at.is_some_and(|b| b & 0xE0 == 0xE0) {
                st.header_buf[0] = 0xFF;
                st.header_buf[1] = at.unwrap_or(0);
                tok.ignore(bo as f64);
                if st.sync_frame_count == st.frame_count {
                    st.frame_count = 0.0;
                    st.frame_size = 0.0;
                }
                st.sync_frame_count = st.frame_count;
                return Ok(());
            }
            got_first_sync = false;
            // indexOf over the whole (reused) buffer, stale bytes included.
            match st.peek_buf.get(bo..).and_then(|s| s.iter().position(|b| *b == 0xFF)) {
                None => {
                    if len < MAX_PEEK {
                        return Err(MmErr::Eos);
                    }
                    tok.ignore(len as f64);
                    break;
                }
                Some(i) => {
                    bo += i + 1;
                    got_first_sync = true;
                }
            }
        }
    }
}

fn common_header(tok: &mut Tok, c: &mut Collector, st: &mut State) -> R<bool> {
    if st.frame_count == 0.0 {
        st.mpeg_offset = tok.position - 1.0;
    }
    let three = tok.peek(3, None, false)?;
    st.header_buf[1..4].copy_from_slice(&three);
    let header = match Header::parse(&st.header_buf) {
        Ok(h) => h,
        Err(_) => {
            tok.ignore(1.0);
            return Ok(false);
        }
    };
    tok.ignore(3.0);
    c.format.sample_rate = header.sampling_rate;
    st.frame_count += 1.0;
    if header.version.is_some_and(|v| v >= 2.0) && header.layer == 0 {
        adts(tok, c, st, header)
    } else {
        audio_frame(tok, c, st, header)
    }
}

fn audio_frame(tok: &mut Tok, c: &mut Collector, st: &mut State, header: Header) -> R<bool> {
    let slot = [None, Some(4.0), Some(1.0), Some(1.0)][header.layer as usize];
    let Some(slot) = slot else {
        return Err(MmErr::Other("invalid slot_size".into()));
    };
    let spf = header.samples_per_frame();
    let fsize = (spf / 8.0) * header.bitrate / header.sampling_rate.unwrap_or(f64::NAN) + if header.padding { slot } else { 0.0 };
    st.frame_size = fsize.floor();
    st.bitrates.push(header.bitrate);
    st.audio_header = Some(header.clone());
    if st.frame_count == 1.0 {
        st.offset = 4.0;
        skip_side_information(tok, c, st)?;
        return Ok(false);
    }
    if st.frame_count == 3.0 {
        if st.bitrates.iter().all(|b| *b == st.bitrates[0]) {
            st.samples_per_frame = Some(spf);
            c.format.codec_profile = Some("CBR".into());
            if tok.size != 0.0 {
                return Ok(true);
            }
        } else if c.format.duration.is_some_and(|d| d != 0.0 && !d.is_nan()) {
            return Ok(true);
        }
    }
    if st.frame_count == 4.0 {
        st.samples_per_frame = Some(spf);
        st.calculate_eof_duration = true;
    }
    st.offset = 4.0;
    if header.protected_by_crc {
        tok.token(2)?;
        st.offset += 2.0;
    }
    skip_side_information(tok, c, st)?;
    Ok(false)
}

fn adts(tok: &mut Tok, c: &mut Collector, st: &mut State, mut header: Header) -> R<bool> {
    let b = tok.token(3)?;
    header.frame_length = header.frame_length.wrapping_add(bits(&b, 0, 0, 11));
    st.total_data_length += f64::from(header.frame_length);
    st.samples_per_frame = Some(1024.0);
    let fl = f64::from(header.frame_length);
    tok.ignore(if fl > 7.0 { fl - 7.0 } else { 1.0 });
    if st.frame_count == 3.0 {
        c.format.codec_profile = Some("AAC".into());
        st.calculate_eof_duration = true;
    }
    let _ = c;
    Ok(false)
}

fn skip_frame_data(tok: &mut Tok, left: f64) -> R<()> {
    if left < 0.0 {
        return Err(MmErr::Other("frame-data-left cannot be negative".into()));
    }
    tok.ignore(left);
    Ok(())
}

fn skip_side_information(tok: &mut Tok, c: &mut Collector, st: &mut State) -> R<()> {
    let side = st.audio_header.as_ref().and_then(Header::side_info_length);
    match side {
        Some(n) => {
            tok.token(n)?;
            st.offset += n as f64;
        }
        // Uint8ArrayType(undefined): an empty read.
        None => st.offset = f64::NAN,
    }
    read_xtra_info_header(tok, c, st)
}

fn read_xtra_info_header(tok: &mut Tok, c: &mut Collector, st: &mut State) -> R<()> {
    let tag = ascii(&tok.token(4)?);
    st.offset += 4.0;
    match tag.as_str() {
        "Info" => {
            c.format.codec_profile = Some("CBR".into());
            return read_xing_info_header(tok, c, st).map(|_| ());
        }
        "Xing" => {
            let vbr_scale = read_xing_info_header(tok, c, st)?;
            c.format.codec_profile = Some(format!("V{}", crate::js::number_to_string(((100.0 - vbr_scale) / 10.0).floor())));
            return Ok(());
        }
        "Xtra" => {}
        "LAME" => {
            tok.token(6)?;
            if st.frame_size >= st.offset + 6.0 {
                st.offset += 6.0;
                return skip_frame_data(tok, st.frame_size - st.offset);
            }
        }
        _ => {}
    }
    let left = st.frame_size - st.offset;
    if left >= 0.0 {
        skip_frame_data(tok, left)?;
    }
    Ok(())
}

/// `readXingInfoHeader`; returns the VBR scale (NaN when absent).
fn read_xing_info_header(tok: &mut Tok, c: &mut Collector, st: &mut State) -> R<f64> {
    let start = tok.position;
    let flags = tok.token(4)?;
    let mut num_frames = f64::NAN;
    let mut stream_size = None;
    let mut vbr_scale = f64::NAN;
    if get_bit(&flags, 3, 0) {
        num_frames = u32be(&tok.token(4)?, 0)?;
    }
    if get_bit(&flags, 3, 1) {
        stream_size = Some(u32be(&tok.token(4)?, 0)?);
    }
    if get_bit(&flags, 3, 2) {
        tok.token(100)?;
    }
    if get_bit(&flags, 3, 3) {
        vbr_scale = u32be(&tok.token(4)?, 0)?;
    }
    let lame = ascii(&tok.peek_token(4)?);
    if lame == "LAME" {
        tok.ignore(4.0);
        let version = ascii(&tok.token(5)?);
        // /\d+.\d+/: leftmost match, greedy digits with backtracking; '.' is any character
        // but a line terminator.
        let b = version.as_bytes();
        let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
        let mut found = None;
        'outer: for start in 0..b.len() {
            let mut max = 0;
            while digit(start + max) {
                max += 1;
            }
            for k in (1..=max).rev() {
                let dot = start + k;
                if dot >= b.len() || matches!(b[dot], b'\n' | b'\r') {
                    continue;
                }
                let mut end = dot + 1;
                while digit(end) {
                    end += 1;
                }
                if end > dot + 1 {
                    found = Some(version[start..end].to_owned());
                    break 'outer;
                }
            }
        }
        if let Some(m) = found {
            let parts: Vec<f64> = m.split('.').map(parse_int).collect();
            if parts.first().is_some_and(|v| *v >= 3.0) && parts.get(1).is_some_and(|v| *v >= 90.0) {
                let ext = tok.token(27)?;
                c.format.duration = Some(u32be(&ext, 20)? / 1000.0);
            }
        }
    }
    st.offset += tok.position - start;
    if stream_size.is_some_and(|s| s != 0.0) {
        let h = st.audio_header.as_ref().expect("set before side information");
        c.format.duration = Some(num_frames * h.samples_per_frame() / h.sampling_rate.unwrap_or(f64::NAN));
        return Ok(vbr_scale);
    }
    skip_frame_data(tok, st.frame_size - st.offset)?;
    Ok(vbr_scale)
}

fn finalize(tok: &Tok, c: &mut Collector, st: &State) {
    let has_id3v1 = c.native.iter().any(|t| t == "ID3v1");
    let truthy = |d: Option<f64>| d.is_some_and(|d| d != 0.0 && !d.is_nan());
    if truthy(c.format.duration) && tok.size != 0.0 {
        return;
    }
    if tok.size != 0.0 && c.format.codec_profile.as_deref() == Some("CBR") {
        let mpeg_size = tok.size - st.mpeg_offset - if has_id3v1 { 128.0 } else { 0.0 };
        let samples = math_round(mpeg_size / st.frame_size) * st.samples_per_frame.unwrap_or(f64::NAN);
        c.format.number_of_samples = Some(samples);
        c.format.duration = Some(samples / c.format.sample_rate.unwrap_or(f64::NAN));
    }
}
