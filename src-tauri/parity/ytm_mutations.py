#!/usr/bin/env python3
"""Mutate copied ytm/ytmusic sources only. Reuse compiled dependencies read-only.
Baseline must pass before mutants are classified; compile errors are not kills.
Only /tmp/mutws is written; the checkout is rsynced in and never modified.
"""
import json
import os
from pathlib import Path
import subprocess
import shutil

repo = Path(__file__).resolve().parents[2]
ws = Path('/tmp/mutws')
ws.mkdir(exist_ok=True)
subprocess.run(['rsync', '-a', '--exclude=target', '--exclude=.git',
                str(repo/'src-tauri')+'/', str(ws/'src-tauri')+'/'], check=True)
for name in ['node_modules', 'src', 'plugins', 'assets', 'bin']:
    if not (ws/name).exists():
        (ws/name).symlink_to(repo/name, target_is_directory=True)
deps = repo/'src-tauri/target/debug/deps'
def newest(name, predicate=lambda p: True):
    return max((p for p in deps.glob('lib'+name+'-*.rlib') if predicate(p)),
               key=lambda p: p.stat().st_mtime)
lyrics = (ws/'src-tauri/src/lyrics.rs').read_text()
form = lyrics[lyrics.index('pub(crate) fn form'):]
form = form[:form.index('\n}\n')+3]
media = (ws/'src-tauri/src/media.rs').read_text()
match_track = media[media.index('// ─── spotify:matchTrack ───'):]
wrapper = ws/'ytm_mut_lib.rs'
wrapper.write_text('extern crate baseline;\npub use baseline::*;\n'
    '#[path="src-tauri/src/ytm/mod.rs"] pub mod ytm;\n'
    '#[path="src-tauri/src/ytmusic.rs"] pub mod ytmusic;\n'
    'pub mod lyrics {\n'+form+'}\n'
    'pub mod media {\n'
    'use crate::host::Host;\n'
    'use crate::ipc::{HandlerResult, JsValue, to_js_string};\n'
    'use crate::logger::{self, LogArg};\n'
    +match_track+'}\n')
cache = ws/'ytm-deps'
cache.mkdir(exist_ok=True)
externs = ['-L', 'dependency='+str(cache), '-L', 'dependency='+str(deps)]
for name in ['snowify', 'serde_json', 'tokio', 'regex', 'url', 'getrandom', 'keyring',
             'flate2', 'futures_util', 'libc', 'icu_normalizer']:
    pred = (lambda p: 'getrandom-0.3.' in (deps/(p.stem[3:]+'.d')).read_text()) if name == 'getrandom' else lambda p: True
    selected = newest(name, pred)
    cached = cache/selected.name
    shutil.copy2(selected, cached)
    externs += ['--extern', ('baseline' if name == 'snowify' else name)+'='+str(cached)]
env = {**os.environ, 'CARGO_MANIFEST_DIR': str(ws/'src-tauri')}
def build():
    return subprocess.run(['rustc', '--edition=2024', '--crate-name', 'snowify_mut',
        '--crate-type', 'rlib', str(wrapper), *externs, '-o', str(ws/'libsnowify_mut.rlib')],
        env=env, capture_output=True, text=True)
# Match baseline's serde_json identity; several feature combinations may be cached.
for lib in sorted(deps.glob('libserde_json-*.rlib'), key=lambda p: -p.stat().st_mtime):
    i = next(i for i, a in enumerate(externs) if a.startswith('serde_json='))
    externs[i] = 'serde_json='+str(lib)
    result = build()
    if result.returncode == 0:
        break
assert result.returncode == 0, result.stderr

mutations = {
'ytm/mod.rs': [
 ('utc-offset', 'tm.tm_gmtoff / 60', 'tm.tm_gmtoff / 3600'),
 ('status-500', 'if res.status >= 500', 'if res.status > 500'),
 ('json-content-type', 'h.push(("Content-Type".into(), "application/json".into()));', 'h.push(("Content-Type".into(), "text/plain".into()));'),
 ('context-gl', '("gl", cfg(st, "GL")),', '("gl", cfg(st, "HL")),'),
 ('query-filter', 'params.retain(|(k, _)| *k != "alt" && *k != "key");', 'params.retain(|(k, _)| *k == "alt" && *k == "key");'),
 ('client-version-header', '("X-YouTube-Client-Version", "INNERTUBE_CLIENT_VERSION"),', '("X-YouTube-Client-Version", "INNERTUBE_CLIENT_NAME"),'),
 ('response-context', '*k == JsString::from("responseContext")', '*k == JsString::from("responseContextX")'),
 ('config-merge', 'spread_into(&mut st.config, &parsed);', 'let _ = &parsed;'),
 ('api-key', 'params.push(("key", ipc::to_js_string(&cfg(st, "INNERTUBE_API_KEY"))));', 'params.push(("key", "wrong".into()));'),
 ('user-agent', 'h.push(("User-Agent".into(), UA.into()));', 'h.push(("User-Agent".into(), "UA".into()));'),
],
'ytm/parsers.rs': [
 ('duration-hour', 'or0(parts[2]) * 3600.0', 'or0(parts[2]) * 60.0'),
 ('year-length', 'y.len() == 4', 'y.len() == 3'),
 ('song-duration', '("duration", plus(&traverse_string(data, &["videoDetails", "lengthSeconds"])?)),', '("duration", JsValue::Number(0.0)),'),
 ('search-single', '"EP" | "Single" | "Album" => album_search_result(item),', '"EP" | "Album" => album_search_result(item),'),
 ('search-proto', '"__defineGetter__" | "__defineSetter__" | "__proto__" => Err(ParseError::TypeError),', '"__defineGetter__" | "__defineSetter__" => Err(ParseError::TypeError),'),
 ('artist-top-duration', '("artist", artist_basic.clone()),\n        ("duration", JsValue::Null),', '("artist", artist_basic.clone()),\n        ("duration", JsValue::Number(1.0)),'),
 ('carousel-index', 'carousel(data, 0, &|i| album_artist_top(i, &basic))?', 'carousel(data, 1, &|i| album_artist_top(i, &basic))?'),
 ('video-thumb-prefix', '"https://i.ytimg.com/vi/"', '"http://i.ytimg.com/vi/"'),
 ('album-overlay', 'let playlist_id = if overlay.truthy() { overlay } else { traverse_string(item, &["thumbnailOverlay", "playlistId"])? };', 'let playlist_id = overlay;'),
 ('artist-fallback', 'let title = at(&cols, 0);\n    let artist = find_or(&cols, is_artist, at(&cols, 3))?;', 'let title = at(&cols, 0);\n    let artist = find_or(&cols, is_artist, at(&cols, 2))?;'),
],
'ytm/cookies.rs': [
 ('public-suffix', "if !cd.contains('.') {", "if cd.contains('.') {"),
 ('domain-match', "host.as_bytes()[host.len() - dom.len() - 1] == b'.'", "host.as_bytes()[host.len() - dom.len() - 1] != b'.'"),
 ('secure-prefix', 'if c.key.starts_with("__Secure-") && !c.secure {', 'if false {'),
 ('host-prefix', 'if c.key.starts_with("__Host-") && !(c.secure && c.host_only && c.path.as_deref() == Some("/")) {', 'if false {'),
 ('sort-order', 'bl.cmp(&al).then(a.creation.total_cmp(&b.creation))', 'al.cmp(&bl).then(a.creation.total_cmp(&b.creation))'),
 ('expired-retain', 'self.cookies.retain(|c| c.expiry_time() > now);', 'self.cookies.retain(|_| true);'),
 ('path-match', "req == cookie || (req.starts_with(cookie) && (cookie.ends_with('/') || req.as_bytes().get(cookie.len()) == Some(&b'/')))", 'req == cookie || req.starts_with(cookie)'),
 ('max-age-zero', 'if age <= 0.0 { f64::NEG_INFINITY } else { age * 1000.0 }', 'if age < 0.0 { f64::NEG_INFINITY } else { age * 1000.0 }'),
 ('domain-strip', "let d = d.strip_prefix('.').unwrap_or(d);", 'let d = d;'),
 ('creation-sort', 'then(a.creation_index.cmp(&b.creation_index))', 'then(b.creation_index.cmp(&a.creation_index))'),
],
'ytm/traverse.rs': [
 ('flatten', 'JsValue::Array(items) => res.extend(items),', 'JsValue::Array(items) => res.push(JsValue::Array(items)),'),
 ('list-wrap', 'JsValue::Array(items) => items,', 'JsValue::Array(items) => vec![JsValue::Array(items)],'),
 ('string-truthy', 'Ok(if first.truthy() { first } else { JsValue::from("") })', 'Ok(first)'),
 ('title-prefix', '"MUSIC_VIDEO_TYPE_"', '"MUSIC_VIDEO_TYPEX"'),
 ('artist-user-channel', 't == JsValue::from("MUSIC_PAGE_TYPE_USER_CHANNEL") || ', ''),
 ('album-type', '== JsValue::from("MUSIC_PAGE_TYPE_ALBUM")', '== JsValue::from("MUSIC_PAGE_TYPE_ALBUMX")'),
 ('duration-digits', "d1 >= 1 && b.get(i + d1) == Some(&b':')", "d1 >= 2 && b.get(i + d1) == Some(&b':')"),
 ('find-last', 'for c in cols {\n        if pred(c)? {\n            return Ok(Some(c.clone()));\n        }\n    }\n    Ok(None)', 'for c in cols.iter().rev() {\n        if pred(c)? {\n            return Ok(Some(c.clone()));\n        }\n    }\n    Ok(None)'),
 ('dead-end', 'if dead_end {\n            return Ok(single_or_list(res));\n        }', 'if false {\n            return Ok(single_or_list(res));\n        }'),
 ('axios-overflow', 'if props.iter().any(|(k, _)| *k == JsString::from(AXIOS_RESPONSE)) {\n                return Err(Overflow);\n            }', 'if props.iter().any(|(k, _)| *k == JsString::from(AXIOS_RESPONSE)) {\n                return Ok(single_or_list(res));\n            }'),
],
'ytmusic.rs': [
 ('thumb-order', 'jsv::v8_sort(items, &|a, b| Ok(jsv::minus(&width(b)?, &width(a)?)))?', 'jsv::v8_sort(items, &|a, b| Ok(jsv::minus(&width(a)?, &width(b)?)))?'),
 ('search-filter', 'if !(jsv::strict_eq(&ty, "SONG") || jsv::strict_eq(&ty, "VIDEO")) || !opt(r, "videoId").truthy() {', 'if !jsv::strict_eq(&ty, "SONG") || !opt(r, "videoId").truthy() {'),
 ('search-songs-params', 'const SONGS: &str = "Eg-KAQwIARAAGAAgACgAMABqChAEEAMQCRAFEAo%3D";', 'const SONGS: &str = "Eg-KAQwIARAAGAAgACgAMABqChAEEAMQCRAFEAo%3Dx";'),
 ('suggestions-thumb', 'get(&get(&thumbs, &js::number_to_string(len - 1.0))?, "url")?', 'get(&get(&thumbs, &js::number_to_string(len - 2.0))?, "url")?'),
 ('banner-size', 'format!("{}=w1440-h600-p-l90-rj", &s[..i]).into()', 'format!("{}=w1440-h601-p-l90-rj", &s[..i]).into()'),
 ('explore-albums', 'if title.contains("new albums") || title.contains("new release") {', 'if title.contains("new albumsx") || title.contains("new release") {'),
 ('charts-take', 'for item in contents.iter().take(20) {', 'for item in contents.iter().take(19) {'),
 ('valid-video-id', 's.len() == 11 && s.bytes()', 's.len() == 10 && s.bytes()'),
 ('up-nexts-skip', 'for item in list.iter().skip(1) {', 'for item in list.iter().skip(0) {'),
 ('stream-worst', 'let fmt = if jsv::strict_eq(&quality, "worstaudio") { "worstaudio/worstaudio*/worst" } else { "bestaudio/bestaudio*/best" };', 'let fmt = if jsv::strict_eq(&quality, "worstaudio") { "bestaudio/bestaudio*/best" } else { "bestaudio/bestaudio*/best" };'),
 ('http-host', 'if hostname != "api.github.com" && hostname != "raw.githubusercontent.com" {', 'if hostname != "api.github.com" {'),
],
}
targets = ['parity_ytmusic', 'parity_ytstreams', 'parity_ytm_cookies']

def test(target):
    binary = ws/('mut-'+target)
    r = subprocess.run(['rustc', '--edition=2024', '--test',
        str(ws/'src-tauri/tests'/ (target+'.rs')), *externs,
        '--extern', 'snowify='+str(ws/'libsnowify_mut.rlib'), '-o', str(binary)],
        env=env, capture_output=True, text=True)
    if r.returncode:
        return 'compile-error', r.stderr
    try:
        r = subprocess.run([str(binary), '--nocapture'], env=env,
                           capture_output=True, text=True, timeout=180)
        return ('killed' if r.returncode else 'passed'), r.stdout+r.stderr
    except subprocess.TimeoutExpired:
        return 'killed', 'test exceeded 180 seconds'

for target in targets:
    status, log = test(target)
    assert status == 'passed', (target, log[-4000:])
    print('baseline', target, status, flush=True)

results = []
for file, cases in mutations.items():
    source = ws/'src-tauri/src'/file
    original = source.read_text()
    try:
        for name, old, new in cases:
            if old not in original:
                results.append([file, name, 'not-applied'])
                print(file, name, 'not-applied', flush=True)
                continue
            source.write_text(original.replace(old, new, 1))
            os.utime(source, None)
            r = build()
            status, log = ('compile-error', r.stderr) if r.returncode else ('passed', '')
            if not r.returncode:
                for target in targets:
                    status, log = test(target)
                    if status != 'passed':
                        break
            if status == 'passed':
                status = 'survived'
            (ws/('ytm-'+name+'.log')).write_text(log)
            results.append([file, name, status])
            (ws/'ytm-mutations.json').write_text(json.dumps(results, indent=2))
            print(file, name, status, flush=True)
    finally:
        source.write_text(original)
        os.utime(source, None)
        assert build().returncode == 0
(ws/'ytm-mutations.json').write_text(json.dumps(results, indent=2))
print(json.dumps(results), flush=True)
