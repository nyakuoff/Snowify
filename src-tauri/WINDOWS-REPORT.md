# Windows portability report — 2026-11-01

Changed runtime: `Cargo.{toml,lock}`, `src/{lib,node,uv_errors,discord,js,real_host}.rs`, `src/{mm,ytm}/mod.rs`.
Added runtime: `src/uv_errors_windows.rs`, `src/windows_time.rs`, `src/node/windows_fs.rs`, `src/node/child_process/windows.rs`; updated `src/node/child_process.rs`.
Changed harness: `tests/common/{mod,tree}.rs`, `tests/parity_js.rs`, `parity/node/{node,js}.js`; added `tests/{platform,windows}/*.rs`, `parity/windows/{run.sh,process-fixture.c}`.
`PORT.md` and concurrently assigned files were not edited; no commit made.
Sources are read-only references; Electron `DEPS@v33.4.11` pins Node `v20.18.3` (libuv 1.46.0) and Chromium `130.0.6723.191`.
UV constants/descriptions: `deps/uv/include/{uv/errno.h,uv.h}@nodejs/node/v20.18.3`; native Win32/WSA translation: `deps/uv/src/win/error.c` at that tag (including negative passthrough and UNKNOWN).
execFile: `deps/uv/src/win/{process,pipe}.c`, `src/{process_wrap.cc,util-inl.h}`, `lib/{child_process,internal/child_process}.js@nodejs/node/v20.18.3`: cwd/PATH search, COM/EXE only, ignore PATHEXT, raw quoting/argv[0], batch rejection, unsigned exit DWORDs, overlapped stdio, job lifetime, TerminateProcess/SIGTERM.
Filesystem: `deps/uv/src/win/fs.c`, `src/{node_file.cc,api/exceptions.cc}`, `lib/internal/fs/rimraf.js@nodejs/node/v20.18.3`: chmod only owner write bit controls readonly, read/scandir/mkdir exceptions, readonly unlink, fixWinEPERMSync, copy/self-copy, error path namespace removal, native directory order.
Media positioned reads preserve the sequential file cursor, as `uv_fs_read` does in the same `fs.c`; Unix read_at remains unchanged.
Discord: installed `node_modules/@xhayper/discord-rpc/dist/transport/IPC.js@1.3.0`: try `\\?\pipe\discord-ipc-{0..9}` without existsSync; preserve existing framing/handshake/events.
Dates: [V8 ICUTimezoneCache](https://raw.githubusercontent.com/v8/v8/3551594a5f6604c7e5070f408cc81d60d08ddbbf/src/objects/intl-objects.cc) (Chromium/Electron pinned revision): ICU offsets, FORMER for both gaps/overlaps; Windows uses dynamically loaded system ICU C APIs.
Approximation: system ICU tzdata/canonical aliases can differ from Electron's bundled ICU; ICU <69 uses offsets sampled ±48h for local-time resolution; unavailable ICU/error falls back to UTC. The Wine corpus intentionally exposes differences, including historical/future dates and eight zones.
Linux gates: `cargo build -q` PASS; requested six parity tests PASS: exec_file 35, node 36430, js 51058, discord 97, mm 1091, media_files 81 comparisons, all differences 0.
GNU gate: `CARGO_TARGET_DIR=/tmp/wintarget cargo check --target x86_64-pc-windows-gnu --lib --tests` PASS, 0 errors/warnings (run offline with cached dependencies).
MSVC gate: blocked before project compilation: offline Cargo lacks `vswhom`/`vswhom-sys`; normal Cargo cannot write the read-only global cache. No MSVC success claimed.
Windows runner build-only PASS: four GNU test EXEs linked, MinGW fixture compiled, cached Electron 33.4.11 unpacked; `WebView2Loader.dll` copied beside test EXEs. Bash/JS syntax and Linux all-tests compiler checks PASS.
Run from src-tauri: `parity/windows/run.sh` (or `--build-only`); requires Wine, winepath, MinGW, cached Cargo dependencies and Electron zip. Default Wine prefix/artifacts/logs are isolated under `/tmp/snowify-windows.*`; no Wine invoked here because wineserver sockets are unavailable.
Harness overrides: `SNOWIFY_PARITY_ELECTRON` (path), `SNOWIFY_PARITY_LAUNCHER` (JSON argv prefix), `SNOWIFY_PARITY_ROOT`; defaults unchanged. The Windows EXEs launch Windows Electron directly and execute the existing JS runners.
Still needs Wine: complete execFile results/errors/quoting/search/kills/buffer tests, full UV table/fs error objects/chmod, ICU dates/offsets and fake Discord named-pipe exchange. No Windows runtime parity claimed yet.
Existing Unix shell/socket fixtures for ytdlp/cloud_auth/ytstreams are cfg-gated by wrapper test targets; their concurrently edited files remain untouched and their Linux tests remain enabled.
Unported Windows shell items: ITaskbarList3 thumbbar buttons; setOpenAtLogin HKCU Run/StartupApproved registry behavior (`shell/browser/browser_win.cc@electron/electron/v33.4.11`). Both existing no-op handlers remain; they need native shell implementation/validation.
Please manually verify Windows boot, home/explore/search/library/settings navigation, playback/shortcuts, settings save, plugins, and the WebView2 media proxy after the Wine run.
