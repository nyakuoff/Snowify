//! Read-only Chromium localStorage migration. No database open, locks, repair or writes.
//! LevelDB's manifest chooses live SSTs and WALs; sequence numbers resolve overwrites.
//! Strings remain UTF-16 code units (including unpaired surrogates) end to end.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
pub type Storage = BTreeMap<Vec<u16>, Vec<u16>>;
pub const MARKER: &str = "__snowify_tauri_local_storage_migrated_v1";
type Result<T> = std::result::Result<T, String>;
fn bad() -> String {
    "invalid Chromium LevelDB".into()
}
fn var(data: &mut &[u8]) -> Result<u64> {
    let mut n = 0u64;
    for shift in (0..70).step_by(7) {
        let b = *data.first().ok_or_else(bad)?;
        *data = &data[1..];
        if shift == 63 && b > 1 {
            return Err(bad());
        }
        n |= ((b & 127) as u64) << shift;
        if b < 128 {
            return Ok(n);
        }
    }
    Err(bad())
}
fn take<'a>(data: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    let out = data.get(..n).ok_or_else(bad)?;
    *data = &data[n..];
    Ok(out)
}
fn slice<'a>(data: &mut &'a [u8]) -> Result<&'a [u8]> {
    let n = var(data)? as usize;
    take(data, n)
}
fn crc(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in data {
        c ^= *b as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ (0x82f63b78u32.wrapping_mul(c & 1));
        }
    }
    (!c).rotate_right(15).wrapping_add(0xa282ead8)
}
// WAL and MANIFEST share the 32KiB physical-record format.
fn records(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    let mut pending = Vec::new();
    let mut assembling = false;
    for block in bytes.chunks(32768) {
        let mut p = 0;
        while p + 7 <= block.len() {
            let h = &block[p..p + 7];
            let n = u16::from_le_bytes([h[4], h[5]]) as usize;
            if h.iter().all(|b| *b == 0) {
                break;
            }
            // A torn final WAL record is ignored, just as LevelDB recovery does.
            if p + 7 + n > block.len() {
                break;
            }
            let data = &block[p + 7..p + 7 + n];
            let mut check = Vec::with_capacity(n + 1);
            check.push(h[6]);
            check.extend(data);
            if crc(&check) != u32::from_le_bytes(h[..4].try_into().unwrap()) {
                return Err(bad());
            }
            match h[6] {
                1 => {
                    out.push(data.to_vec());
                    assembling = false;
                }
                2 => {
                    pending = data.to_vec();
                    assembling = true;
                }
                3 if assembling => pending.extend(data),
                4 if assembling => {
                    pending.extend(data);
                    out.push(std::mem::take(&mut pending));
                    assembling = false;
                }
                _ => return Err(bad()),
            }
            p += 7 + n;
        }
    }
    Ok(out)
}
fn snappy(mut input: &[u8]) -> Result<Vec<u8>> {
    let size = var(&mut input)? as usize;
    if size > 512 * 1024 * 1024 {
        return Err(bad());
    }
    let mut out = Vec::with_capacity(size);
    while !input.is_empty() {
        let tag = take(&mut input, 1)?[0];
        let (len, offset) = match tag & 3 {
            0 => {
                let mut n = (tag >> 2) as usize;
                if n >= 60 {
                    let b = take(&mut input, n - 59)?;
                    n = 0;
                    for (i, v) in b.iter().enumerate() {
                        n |= (*v as usize) << (8 * i);
                    }
                }
                let n = n.checked_add(1).ok_or_else(bad)?;
                if out.len() + n > size {
                    return Err(bad());
                }
                out.extend(take(&mut input, n)?);
                continue;
            }
            1 => (
                4 + ((tag >> 2) & 7) as usize,
                (((tag as usize) & 224) << 3) | take(&mut input, 1)?[0] as usize,
            ),
            2 => (
                1 + (tag >> 2) as usize,
                u16::from_le_bytes(take(&mut input, 2)?.try_into().unwrap()) as usize,
            ),
            _ => (
                1 + (tag >> 2) as usize,
                u32::from_le_bytes(take(&mut input, 4)?.try_into().unwrap()) as usize,
            ),
        };
        if offset == 0 || offset > out.len() || out.len() + len > size {
            return Err(bad());
        }
        for _ in 0..len {
            out.push(out[out.len() - offset]);
        }
    }
    if out.len() != size {
        return Err(bad());
    }
    Ok(out)
}
fn block(file: &[u8], mut handle: &[u8]) -> Result<Vec<u8>> {
    let offset = var(&mut handle)? as usize;
    let size = var(&mut handle)? as usize;
    let end = offset.checked_add(size).ok_or_else(bad)?;
    let data = file.get(offset..end).ok_or_else(bad)?;
    let trailer = file
        .get(end..end.checked_add(5).ok_or_else(bad)?)
        .ok_or_else(bad)?;
    let mut check = data.to_vec();
    check.push(trailer[0]);
    if crc(&check) != u32::from_le_bytes(trailer[1..].try_into().unwrap()) {
        return Err(bad());
    }
    match trailer[0] {
        0 => Ok(data.to_vec()),
        1 => snappy(data),
        _ => Err(bad()),
    }
}
fn entries(data: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    if data.len() < 4 {
        return Err(bad());
    }
    let count = u32::from_le_bytes(data[data.len() - 4..].try_into().unwrap()) as usize;
    let end = data
        .len()
        .checked_sub(4 + count.checked_mul(4).ok_or_else(bad)?)
        .ok_or_else(bad)?;
    let mut input = &data[..end];
    let mut previous = Vec::new();
    let mut out = Vec::new();
    while !input.is_empty() {
        let shared = var(&mut input)? as usize;
        let unshared = var(&mut input)? as usize;
        let n = var(&mut input)? as usize;
        if shared > previous.len() {
            return Err(bad());
        }
        previous.truncate(shared);
        previous.extend(take(&mut input, unshared)?);
        out.push((previous.clone(), take(&mut input, n)?.to_vec()));
    }
    Ok(out)
}
type Versions = BTreeMap<Vec<u8>, (u64, Option<Vec<u8>>)>;
fn put(map: &mut Versions, key: &[u8], seq: u64, value: Option<Vec<u8>>) {
    if map.get(key).is_none_or(|(old, _)| seq >= *old) {
        map.insert(key.to_vec(), (seq, value));
    }
}
fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| e.to_string())
}
fn string(bytes: &[u8]) -> Result<Vec<u16>> {
    match bytes.split_first() {
        Some((1, rest)) => Ok(rest.iter().map(|b| *b as u16).collect()),
        Some((0, rest)) if rest.len() % 2 == 0 => Ok(rest
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect()),
        _ => Err(bad()),
    }
}
/// `db` is <RealHost::user_data()>/Local Storage/leveldb. Missing is a fresh install.
/// A concurrent Electron writer may invalidate this read: fail and retry next startup.
pub fn read_leveldb(db: &Path) -> Result<Storage> {
    if !db.exists() {
        return Ok(Storage::new());
    }
    let current = read(&db.join("CURRENT"))?;
    let manifest = std::str::from_utf8(&current).map_err(|_| bad())?.trim();
    if !manifest.starts_with("MANIFEST-") || manifest.contains(['/', '\\']) {
        return Err(bad());
    }
    let mut tables = BTreeSet::new();
    let mut log = 0;
    let mut prev_log = 0;
    for record in records(&read(&db.join(manifest))?)? {
        let mut input = record.as_slice();
        while !input.is_empty() {
            match var(&mut input)? {
                1 => {
                    if slice(&mut input)? != b"leveldb.BytewiseComparator" {
                        return Err(bad());
                    }
                }
                2 => log = var(&mut input)?,
                3 | 4 => {
                    var(&mut input)?;
                }
                5 => {
                    var(&mut input)?;
                    slice(&mut input)?;
                }
                6 => {
                    var(&mut input)?;
                    tables.remove(&var(&mut input)?);
                }
                7 => {
                    var(&mut input)?;
                    tables.insert(var(&mut input)?);
                    var(&mut input)?;
                    slice(&mut input)?;
                    slice(&mut input)?;
                }
                9 => prev_log = var(&mut input)?,
                _ => return Err(bad()),
            }
        }
    }
    let mut map = Versions::new();
    for number in tables {
        let ldb = db.join(format!("{number:06}.ldb"));
        let file = read(&if ldb.exists() {
            ldb
        } else {
            db.join(format!("{number:06}.sst"))
        })?;
        if file.len() < 48
            || file[file.len() - 8..] != [0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb]
        {
            return Err(bad());
        }
        let mut footer = &file[file.len() - 48..file.len() - 8];
        var(&mut footer)?;
        var(&mut footer)?; // metaindex handle
        for (_, handle) in entries(&block(&file, footer)?)? {
            for (key, value) in entries(&block(&file, &handle)?)? {
                if key.len() < 8 {
                    return Err(bad());
                }
                let end = key.len() - 8;
                let tag = u64::from_le_bytes(key[end..].try_into().unwrap());
                let value = match tag & 255 {
                    0 => None,
                    1 => Some(value),
                    _ => return Err(bad()),
                };
                put(&mut map, &key[..end], tag >> 8, value);
            }
        }
    }
    for entry in fs::read_dir(db).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_none_or(|e| e != "log") {
            continue;
        }
        let number = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(bad)?;
        if number < log && number != prev_log {
            continue;
        }
        for record in records(&read(&path)?)? {
            if record.len() < 12 {
                return Err(bad());
            }
            let seq = u64::from_le_bytes(record[..8].try_into().unwrap());
            let count = u32::from_le_bytes(record[8..12].try_into().unwrap());
            let mut input = &record[12..];
            for i in 0..count {
                let kind = take(&mut input, 1)?[0];
                let key = slice(&mut input)?;
                let value = match kind {
                    0 => None,
                    1 => Some(slice(&mut input)?.to_vec()),
                    _ => return Err(bad()),
                };
                put(&mut map, key, seq + i as u64, value);
            }
            if !input.is_empty() {
                return Err(bad());
            }
        }
    }
    let version = map
        .get(b"VERSION".as_slice())
        .and_then(|(_, v)| v.as_deref());
    if !matches!(version, None | Some(b"1")) {
        return Err("unsupported Chromium localStorage VERSION".into());
    }
    let mut out = Storage::new();
    for (key, (_, value)) in map {
        // VERSION and META:<origin> are metadata, never renderer entries.
        if let (Some(key), Some(value)) = (key.strip_prefix(b"_file://\0"), value) {
            out.insert(string(key)?, string(&value)?);
        }
    }
    Ok(out)
}
fn literal(units: &[u16]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(2 + units.len() * 6);
    out.push('"');
    for unit in units {
        write!(out, "\\u{unit:04x}").unwrap();
    }
    out.push('"');
    out
}
/// One document-start script: guard first, marker last; failures remain retryable.
pub fn injection_script(map: &Storage) -> String {
    let mut script = format!(
        "(() => {{ if (window.top !== window || !['tauri://localhost', 'http://tauri.localhost'].includes(location.origin)) return; try {{ const s=window.localStorage; if (s.getItem('snowify_state')!==null || s.getItem('snowfy_state')!==null || s.getItem('{MARKER}')!==null) return; const undo=[]; try {{\n"
    );
    for (key, value) in map {
        script.push_str(&format!(
            "undo.push([{},s.getItem({})]); s.setItem({},{});\n",
            literal(key),
            literal(key),
            literal(key),
            literal(value)
        ));
    }
    script.push_str(&format!("s.setItem('{MARKER}','1'); }} catch (e) {{ for (const [k,v] of undo.reverse()) {{ if(v===null) s.removeItem(k); else s.setItem(k,v); }} throw e; }} }} catch (e) {{ console.error('[localStorage migration]',e); }} }})();"));
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(mut n: u64) -> Vec<u8> {
        let mut o = vec![];
        while n >= 128 {
            o.push(n as u8 | 128);
            n >>= 7;
        }
        o.push(n as u8);
        o
    }
    fn s(b: &[u8]) -> Vec<u8> {
        let mut o = v(b.len() as u64);
        o.extend(b);
        o
    }
    fn physical(data: &[u8]) -> Vec<u8> {
        let mut o = vec![];
        let chunks = data.chunks(32761);
        let count = chunks.len();
        for (i, b) in chunks.enumerate() {
            let kind = if count == 1 {
                1
            } else if i == 0 {
                2
            } else if i == count - 1 {
                4
            } else {
                3
            };
            let mut check = vec![kind];
            check.extend(b);
            o.extend(crc(&check).to_le_bytes());
            o.extend((b.len() as u16).to_le_bytes());
            o.push(kind);
            o.extend(b);
        }
        o
    }
    fn block_entries(items: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
        let mut o = vec![];
        for (k, b) in items {
            o.push(0);
            o.extend(v(k.len() as u64));
            o.extend(v(b.len() as u64));
            o.extend(k);
            o.extend(b);
        }
        o.extend(0u32.to_le_bytes());
        o.extend(1u32.to_le_bytes());
        o
    }
    fn append_block(file: &mut Vec<u8>, data: &[u8], compressed: bool) -> Vec<u8> {
        let mut encoded = data.to_vec();
        if compressed {
            encoded = v(data.len() as u64);
            let n = data.len() - 1;
            encoded.push(0xf0);
            encoded.push(n as u8);
            encoded.extend(data);
        }
        let mut handle = v(file.len() as u64);
        handle.extend(v(encoded.len() as u64));
        let mut check = encoded.clone();
        check.push(u8::from(compressed));
        file.extend(&check);
        file.extend(crc(&check).to_le_bytes());
        handle
    }
    fn internal(key: &[u8], seq: u64, kind: u64) -> Vec<u8> {
        let mut k = key.to_vec();
        k.extend(((seq << 8) | kind).to_le_bytes());
        k
    }
    #[test]
    fn synthetic_manifest_sst_wal() {
        let path = std::env::temp_dir().join(format!("snowify-reader-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
        // Only table 2 is live. Table 3 is explicitly deleted by a later edit.
        let mut manifest = vec![1];
        manifest.extend(s(b"leveldb.BytewiseComparator"));
        manifest.extend([2, 4, 7, 0, 2, 0, 0, 0, 7, 0, 3, 0, 0, 0]);
        let mut edits = physical(&manifest);
        edits.extend(physical(&[6, 0, 3]));
        fs::write(path.join("MANIFEST-000001"), edits).unwrap();
        let items = vec![
            (internal(b"VERSION", 1, 1), b"1".to_vec()),
            (internal(b"META:file://", 2, 1), vec![42]),
            (internal(b"_file://\0\x01latin", 3, 1), vec![1, 0xe9, 0xff]),
            (
                internal(b"_file://\0\x00\x00\xd8", 4, 1),
                vec![0, 0xff, 0xdf],
            ),
            (internal(b"_file://\0\x01over", 5, 1), vec![1, b'a']),
            (internal(b"_file://\0\x01gone", 6, 1), vec![1, b'b']),
            (internal(b"_https://other\0\x01secret", 7, 1), vec![1, b'c']),
        ];
        let mut file = vec![];
        let handle = append_block(&mut file, &block_entries(&items), true);
        let index = block_entries(&[(b"z".to_vec(), handle)]);
        let ih = append_block(&mut file, &index, false);
        let mut footer = vec![0, 0];
        footer.extend(ih);
        footer.resize(40, 0);
        footer.extend([0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb]);
        file.extend(footer);
        fs::write(path.join("000002.ldb"), file).unwrap();
        fs::write(path.join("000003.ldb"), b"obsolete: must not be scanned").unwrap();
        let mut batch = 10u64.to_le_bytes().to_vec();
        batch.extend(3u32.to_le_bytes());
        batch.push(1);
        batch.extend(s(b"_file://\0\x01over"));
        batch.extend(s(&[1, b'z']));
        batch.push(0);
        batch.extend(s(b"_file://\0\x01gone"));
        batch.push(1);
        batch.extend(s(b"_file://\0\x01large"));
        let mut big = vec![1];
        big.extend(vec![120; 100_000]);
        batch.extend(s(&big));
        fs::write(path.join("000004.log"), physical(&batch)).unwrap();
        // Stale log should never resurrect a key, even with a larger sequence.
        let mut stale = 999u64.to_le_bytes().to_vec();
        stale.extend(1u32.to_le_bytes());
        stale.push(1);
        stale.extend(s(b"_file://\0\x01gone"));
        stale.extend(s(&[1, 65]));
        fs::write(path.join("000001.log"), physical(&stale)).unwrap();
        let got = read_leveldb(&path).unwrap();
        assert_eq!(got.len(), 4);
        assert_eq!(
            got[&"latin".encode_utf16().collect::<Vec<_>>()],
            vec![0xe9, 0xff]
        );
        assert_eq!(got[&vec![0xd800]], vec![0xdfff]);
        assert_eq!(got[&"over".encode_utf16().collect::<Vec<_>>()], vec![122]);
        assert_eq!(
            got[&"large".encode_utf16().collect::<Vec<_>>()],
            vec![120; 100_000]
        );
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn snappy_copies_and_bad_data() {
        // literal 'a', short copy (offset 1, length 4), two-byte copy length 3.
        assert_eq!(snappy(&[8, 0, b'a', 1, 1, 10, 1, 0]).unwrap(), b"aaaaaaaa");
        assert!(snappy(&[1, 1, 0]).is_err());
        assert!(string(&[0, 1]).is_err());
        assert!(string(&[2]).is_err());
        let mut r = physical(b"test");
        r[8] ^= 1;
        assert!(records(&r).is_err());
        assert_eq!(string(&[1, 0x80, 0xff]).unwrap(), vec![128, 255]);
    }
}
