#!/bin/bash
# Re-records parity/corpus/electron-locale.tsv from real Electron: one run per env case.
# Usage: record.sh < cases.txt > ../../corpus/electron-locale.tsv   (one "VAR=value ..." per line)
here=$(cd "$(dirname "$0")" && pwd)
echo "# Electron 33 app.getLocale() ground truth (Linux). Columns: env assignments (space-separated), locale."
while IFS= read -r line; do
  read -ra kv <<< "$line"
  out=$(env -i HOME="$HOME" DISPLAY="$DISPLAY" XAUTHORITY="$XAUTHORITY" XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" "${kv[@]}" \
    timeout 30 "$here/../../../../node_modules/.bin/electron" "$here/main.js" 2>/dev/null | head -1)
  [ -n "$out" ] && printf '%s\t%s\n' "$line" "$(node -e 'console.log(JSON.parse(process.argv[1]).locale)' "$out")"
done
