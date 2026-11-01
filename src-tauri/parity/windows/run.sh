#!/usr/bin/env bash
# Build on Linux, then run the Windows Rust tests AND Electron's Node oracle in Wine.
# No Wine is invoked for --build-only. All source modules stay unmodified.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
work=$(mktemp -d /tmp/snowify-windows.XXXXXX)
trap 'rm -rf "$work"' EXIT
# On disk, not /tmp: the Windows build is ~8 GB and /tmp is often a RAM-backed tmpfs.
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$root/target/windows}
export WINEPREFIX=${WINEPREFIX:-$work/prefix}
export WINEDEBUG=${WINEDEBUG:--all}
export WINEDLLOVERRIDES=${WINEDLLOVERRIDES:-mscoree,mshtml=}
wine_bin=${WINE:-wine}
cc_bin=${CC_x86_64_pc_windows_gnu:-x86_64-w64-mingw32-gcc}
zip=${SNOWIFY_WINDOWS_ELECTRON_ZIP:-}
if [[ -z "$zip" ]]; then
  zip=$(rg --files --hidden "$HOME/.cache/electron" | rg '/electron-v33\.4\.11-win32-x64\.zip$' | head -n 1)
fi
[[ -f "$zip" ]] || { echo 'Set SNOWIFY_WINDOWS_ELECTRON_ZIP to the cached electron-v33.4.11-win32-x64.zip.' >&2; exit 1; }
python3 - "$zip" "$work/electron" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as archive:
    archive.extractall(sys.argv[2])
PY
"$cc_bin" -municode -O2 -Wall -Wextra -static "$root/parity/windows/process-fixture.c" -o "$work/process-fixture.exe"
cd "$root"
cargo test --offline --target x86_64-pc-windows-gnu --no-run \
  --test parity_exec_file --test parity_node --test parity_js --test parity_discord --test parity_windows_shell \
  --message-format=json > "$work/build.jsonl"
python3 - "$work/build.jsonl" > "$work/tests.txt" <<'PY'
import json, sys
wanted = {'parity_exec_file', 'parity_node', 'parity_js', 'parity_discord', 'parity_windows_shell'}
found = {}
for line in open(sys.argv[1]):
    m = json.loads(line)
    if m.get('reason') == 'compiler-artifact' and m.get('profile', {}).get('test') and m['target']['name'] in wanted and m.get('executable'):
        found[m['target']['name']] = m['executable']
if found.keys() != wanted:
    raise SystemExit('Missing test binaries: ' + repr(wanted - found.keys()))
# GNU Tauri imports the WebView2 loader even though these tests never open a WebView.
# Put the runtime DLL next to the test executables, just as the Tauri packager does.
import pathlib, shutil, os
registry = pathlib.Path(os.environ.get('CARGO_HOME', str(pathlib.Path.home() / '.cargo'))) / 'registry' / 'src'
loaders = list(registry.glob('*/webview2-com-sys-*/x64/WebView2Loader.dll'))
if not loaders:
    raise SystemExit('WebView2Loader.dll missing from the cached webview2-com-sys crate')
for name in sorted(found):
    shutil.copy2(loaders[-1], pathlib.Path(found[name]).parent / 'WebView2Loader.dll')
    print(found[name])
PY
printf 'Windows artifacts and logs: %s\n' "$work"
if [[ ${1:-} == --build-only ]]; then exit 0; fi
# These are Windows paths because the test .exe starts electron.exe directly. The
# launcher override is for a native Linux harness; it must be absent inside Wine.
unset SNOWIFY_PARITY_LAUNCHER
export SNOWIFY_PARITY_ROOT
SNOWIFY_PARITY_ROOT=$(winepath -w "$root")
export SNOWIFY_PARITY_ELECTRON
SNOWIFY_PARITY_ELECTRON=$(winepath -w "$work/electron/electron.exe")
export SNOWIFY_PARITY_PROCESS_FIXTURE
SNOWIFY_PARITY_PROCESS_FIXTURE=$(winepath -w "$work/process-fixture.exe")
export ELECTRON_RUN_AS_NODE=1
# Never pipe a Wine process's stdout into a Linux pipe (`| tee`): Node's process.stdout
# fails on that handle with `open EBADF`. Files, and pipes made inside Wine, work.
"$wine_bin" "$work/electron/electron.exe" -p 'JSON.stringify(process.versions)' > "$work/versions.json"
cat "$work/versions.json"
failed=0
while IFS= read -r binary; do
  name=$(basename -- "$binary" .exe)
  status=0
  "$wine_bin" "$binary" --nocapture --test-threads=1 < /dev/null > "$work/$name.log" 2>&1 || status=$?
  cat "$work/$name.log"
  [[ $status == 0 ]] || failed=1
done < "$work/tests.txt"
exit $failed
