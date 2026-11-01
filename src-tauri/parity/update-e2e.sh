#!/usr/bin/env bash
# Local end-to-end check of the packaged auto-update, with nothing published: builds a
# signed 2.0.0 and 2.0.1 AppImage, serves 2.0.1's latest.json from 127.0.0.1, and leaves
# you to run 2.0.0 and let it update itself (download, signature check, install, relaunch).
#
#   export TAURI_SIGNING_PRIVATE_KEY=~/.tauri/snowify.key   # path or contents
#   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=...           # if the key has one
#   src-tauri/parity/update-e2e.sh            # build both, then serve
#   src-tauri/parity/update-e2e.sh --serve    # serve again without rebuilding
set -euo pipefail
: "${TAURI_SIGNING_PRIVATE_KEY:?export TAURI_SIGNING_PRIVATE_KEY (key path or contents) first}"
# Accept a key path, including an unexpanded leading ~ (e.g. from a quoted export): tauri
# only reads it as a file when the path exists, otherwise it decodes the text as the key.
key=${TAURI_SIGNING_PRIVATE_KEY/#\~/$HOME}
if [[ -f $key ]]; then
  TAURI_SIGNING_PRIVATE_KEY=$(<"$key")
  export TAURI_SIGNING_PRIVATE_KEY
fi
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
out=$root/target/update-e2e
port=${PORT:-8765}
base=http://127.0.0.1:$port

build() {
  local version=$1
  # Local-only overrides: version, a plain-HTTP localhost endpoint (the updater refuses
  # http unless told otherwise), signed updater artifacts, AppImage only.
  cat > "$out/overlay-$version.json" <<EOF
{
  "version": "$version",
  "bundle": { "targets": ["appimage"], "createUpdaterArtifacts": true },
  "plugins": { "updater": {
    "endpoints": ["$base/latest.json"],
    "dangerousInsecureTransportProtocol": true
  } }
}
EOF
  (cd "$root" && npx --yes @tauri-apps/cli@2 build --config "$out/overlay-$version.json")
  mkdir -p "$out/$version"
  cp "$root"/target/release/bundle/appimage/Snowify_"$version"_*.AppImage{,.sig} "$out/$version/"
}

mkdir -p "$out"
if [[ ${1:-} != --serve ]]; then
  build 2.0.0
  build 2.0.1
fi

serve=$out/serve
rm -rf "$serve" && mkdir -p "$serve"
new=$(ls "$out"/2.0.1/*.AppImage)
cp "$new" "$serve/"
node -e '
  const fs = require("fs");
  const [file, sig, url, out] = process.argv.slice(1);
  fs.writeFileSync(out, JSON.stringify({
    version: "2.0.1", notes: "Local update test", pub_date: new Date().toISOString(),
    platforms: { "linux-x86_64": { signature: fs.readFileSync(sig, "utf8"), url } },
  }, null, 2));
' "$new" "$new.sig" "$base/$(basename "$new")" "$serve/latest.json"

old=$(ls "$out"/2.0.0/*.AppImage)
# The update replaces the AppImage in place, so run a copy and keep the original 2.0.0.
cp "$old" "$out/Snowify-under-test.AppImage"
cat <<EOF

Serving $serve on $base (Ctrl+C to stop).
In another terminal run:
  $out/Snowify-under-test.AppImage
Expect, within ~5 s: the update banner for 2.0.1. Install it; the app should restart as
2.0.1 (Settings shows the version). Re-run with --serve to test again from a fresh copy.
Signature rejection: edit $serve/latest.json's signature, restart the 2.0.0 copy, and expect
the update to fail with an error status instead of installing.
EOF
exec python3 -m http.server "$port" --bind 127.0.0.1 --directory "$serve"
