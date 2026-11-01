#!/usr/bin/env python3
"""Mutate copied Firebase sources only. Reuse compiled dependencies read-only.
Build the four Firebase parity targets first. No Cargo builds or source edits in
the original checkout. Baseline exports supply unrelated modules; only the three
target modules and their production Backend adapter are recompiled by rustc.
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
media = (ws/'src-tauri/src/media.rs').read_text()
media = media[media.index('pub(crate) fn base64url'):media.index('/// `parseLocalFile')]
wrapper = ws/'firebase_mut_lib.rs'
wrapper.write_text('extern crate baseline;\npub use baseline::*;\n'
    '#[path="src-tauri/src/firebase/mod.rs"] pub mod firebase;\n'
    '#[path="src-tauri/src/cloud_auth.rs"] pub mod cloud_auth;\n'
    '#[path="src-tauri/src/cloud_backend.rs"] pub mod cloud_backend;\n'
    'pub mod media {\n'+media+'}\n')
cache = ws/'firebase-deps'
cache.mkdir(exist_ok=True)
externs = ['-L', 'dependency='+str(cache), '-L', 'dependency='+str(deps)]
for name in ['snowify', 'serde_json', 'tokio', 'regex', 'url', 'getrandom', 'keyring',
             'flate2', 'futures_util', 'libc']:
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
'firebase/mod.rs': [
 ('heartbeat', 'if !self.heartbeat_sent {', 'if false {'),
 ('refresh-boundary', 'host.now_ms() <= u.expires - 30000', 'host.now_ms() < u.expires - 30000'),
 ('refresh-form', '.replace(\'+\', "%20")', '.replace(\'+\', "+")'),
 ('signout-event', 'self.current_user.take().is_some() && self.listening', 'self.current_user.take().is_some() && false'),
 ('near-expiry', 'seconds(&v["expiresIn"]) < 30', 'false'),
 ('recaptcha', 'for _ in 0..2', 'for _ in 0..1'),
 ('mfa', 'if v.get("mfaPendingCredential").is_some()', 'if false'),
 ('confirmation', 'if v.get("needConfirmation").is_some()', 'if false'),
 ('profile-token', 'if let Some(t) = v["idToken"].as_str()', 'if let Some(t) = None::<&str>'),
 ('invalidated', '["auth/user-disabled", "auth/user-token-expired"]', '["auth/never", "auth/never"]'),
],
'firebase/firestore.rs': [
 ('integer', 'n.abs() <= 9007199254740991.0', 'n.abs() <= 0.0'),
 ('nested-array', 'if array {', 'if false {'),
 ('empty-mask', 'if entries.is_empty() && !p.is_empty()', 'if false'),
 ('update-precondition', 'json!({"exists":true})', 'json!({"exists":false})'),
 ('offline', 'Failed to get document because the client is offline.', 'offline mutant'),
 ('transient', '                "unavailable"', '                "never"'),
 ('resource-backoff', 'delay = 60_000;', 'delay = 1;'),
 ('reference', 'JsValue::FirestoreReference(', 'JsValue::String('),
 ('status-message', 'format!("{number} {name}: {message}")', 'format!("{name}: {message}")'),
 ('auth-header', 'format!("Bearer {}",', 'format!("Wrong {}",'),
],
'cloud_auth.rs': [
 ('missing-backfill', 'if matches!(v2_value, JsValue::Undefined | JsValue::Null)', 'if false'),
 ('array-backfill', '&& v.is_empty()', '&& !v.is_empty()'),
 ('nested-backfill', 'if nested_changed {', 'if false {'),
 ('migration-time', 'JsValue::Number(now)', 'JsValue::Number(0.0)'),
 ('keyring', 'if k.available() {', 'if false {'),
 ('bio-limit', 'u.len().min(200)', 'u.len().min(2)'),
 ('photo', 'if photo_url != JsValue::Undefined {', 'if false {'),
 ('extras-fallback', 'if b.update_doc(V2, &user.uid, &obj(update)).await.is_err()', 'if false'),
 ('save-time', 'JsValue::Number(b.now_ms())', 'JsValue::Number(0.0)'),
 ('profile-name', 'if display_name != JsValue::Undefined {', 'if false {'),
]}
targets = {
 'firebase/mod.rs': ['parity_firebase_auth'],
 'firebase/firestore.rs': ['parity_firestore_full', 'parity_firestore'],
 'cloud_auth.rs': ['parity_cloud_auth'],
}
def test(target, label):
    binary = ws/('mut-'+target)
    r = subprocess.run(['rustc', '--edition=2024', '--test',
        str(ws/'src-tauri/tests'/ (target+'.rs')), *externs,
        '--extern', 'snowify='+str(ws/'libsnowify_mut.rlib'), '-o', str(binary)],
        env=env, capture_output=True, text=True)
    if r.returncode:
        return 'compile-error', r.stderr
    try:
        r = subprocess.run([str(binary), '--nocapture'], env=env,
                           capture_output=True, text=True, timeout=35)
        return ('killed' if r.returncode else 'passed'), r.stdout+r.stderr
    except subprocess.TimeoutExpired:
        return 'killed', 'test exceeded 35 seconds'
results = []
for file, cases in mutations.items():
    source = ws/'src-tauri/src'/file
    original = source.read_text()
    for target in targets[file]:
        status, log = test(target, 'baseline')
        assert status == 'passed', (target, log)
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
                for target in targets[file]:
                    status, log = test(target, name)
                    if status != 'passed':
                        break
            if status == 'passed':
                status = 'survived'
            (ws/('firebase-'+name+'.log')).write_text(log)
            results.append([file, name, status])
            (ws/'firebase-mutations.json').write_text(json.dumps(results, indent=2))
            print(file, name, status, flush=True)
    finally:
        source.write_text(original)
        os.utime(source, None)
        assert build().returncode == 0
(ws/'firebase-mutations.json').write_text(json.dumps(results, indent=2))
print(json.dumps(results), flush=True)
