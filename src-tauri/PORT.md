# Snowify → Rust port

Branch `port/rust`. Started 2026-10-27.

## End state (decided 2026-10-27)

**A — replacement.** A Tauri 2 Rust binary replaces Electron, the Node main process
(`src/main.js`, `src/main/*`, `src/firebase.js`) and `src/preload.js`. The renderer
(`src/renderer/**`, HTML/CSS/ES modules) ships **unchanged**, so the UI, JS plugins and CSS
themes are untouched. No language bindings at any point.

Out of scope: the Android/Capacitor app (`src/mobile/*`, `android/`). It never used the
Electron main process and keeps working as-is.

**Platforms: Linux and Windows only. macOS is dropped entirely (owner decision 2026-10-30).**
The port carries no macOS code: no `mac-setup.js` (Homebrew yt-dlp install), no macOS
branches of `ytdlp.js` (Homebrew/Python lookup) or `window.js` (overlay title bar,
`platform-darwin`), no `activate`/`open-url` lifecycle, no macOS bundle in the release
workflow. The Electron app keeps its macOS build until the cut-over (step 12) deletes it.

## Seam

`window.snowify` (defined in `src/preload.js`). The renderer already reaches the backend only
through it:

- 83 `ipcMain` channels: 75 `invoke`/`handle` and 8 `send`/`on` (window controls, tray,
  thumbbar, install update, close-ready)
- 7 main→renderer events: `app:before-close`, `app:deepLink`, `auth:stateChanged`,
  `thumbar:{prev,playPause,next}`, `updater:status`
- 2 synchronous members: `platform`, `resolveImageUrl` (pure, URL de-proxy)

In Tauri the same object is defined by an init script on top of `invoke`/`listen`, so the
renderer code doesn't change. Each channel becomes one `#[tauri::command]`.

## Parity contract

- **Unit:** one `window.snowify` call: its arguments in, and out the resolved value or the
  rejection, plus any events emitted and files written. The startup sequence, deep-link
  handling and window/tray behaviour are separate units, checked manually against a
  checklist when the shell lands.
- **Acceptable differences (closed list):**
  - Log wording, and timing.
  - Absolute paths under tmp/userData. Normalized by the harness to `<tmp>` / `<userData>`;
    the layout *beneath* those roots is not normalized.
  - Error message wording. The rejection itself still has to happen.
  - JSON object key order, wherever the renderer only reads the result by key.
- **Unacceptable differences:**
  - Resolve vs reject, for any input.
  - The shape and values of a resolved result, compared as deep-equal JSON.
  - Channel and event names, and event payloads.
  - On-disk formats: userData layout, theme and plugin directories, exported library/CSV
    files.
  - The Firestore document shape and collection names, including the v2/legacy split.
  - Translated strings.
  - Narrowed input acceptance: an input the source handled has to stay handled.
- **Measurement:** each ported module has `tests/parity_<module>.rs`. It runs the **source
  JS, unmodified,** through `parity/node/<module>.js` and the Rust module over the same
  cases, and prints `compared N, differences D`. Electron APIs are mocked; sibling modules
  are stubbed only where the unit under test doesn't use them. Golden cases pin quirks of
  the source and are checked against the source first, so if the source changes underneath
  us the test says so. Corpora are fixed files or seeded generators (reproducible).
  Filesystem cases build the same tree twice at the same path and compare the results, the
  messages and the resulting trees. Network modules (ytmusic, lyrics, cloud) will replay
  recorded HTTP responses; how the source side consumes those gets decided when that phase
  starts.
- **Runtime:** the source runs under **Electron 33's own Node 20.18**
  (`ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron`), never the system Node. Node 26
  differs from 20.18 in `path.win32`, `os.tmpdir` and `fs.rmSync`.
- **Mutation checks:** after a module lands, its key branches are broken one at a time and
  each break has to make a harness fail. A survivor is either covered by a new case or
  shown to be an equivalent mutant (listed under Coverage gaps).

`cargo test` runs every parity test. It needs `npm install` at the repo root (for Electron).

### Parity status

| Module | Cases compared | Differences |
|---|---|---|
| `i18n.js` → `i18n.rs` | 18,894 (28 locale/override cases × all keys + 53 edge calls, 8 fixture locale files, 17 goldens) | 0 |
| JS semantics → `js.rs` (`Number#toString`, `String()`, `JSON.stringify`, `toISOString`) | 35,046 (26k doubles incl. raw bit patterns and 20k tie-prone decimals, 3k random JSON values, 3k dates across ±8.64e15) | 0 |
| Node 20 runtime → `node.rs` (`path` posix+win32, `os.tmpdir`, errno map, `existsSync`, `mkdirSync({recursive})`, `rmSync({recursive,force})`) | 16,368 (16k random paths, 301 env combos, errno table, 33 fs cases on real trees) | 0 |
| deep links (`main.js`) → `deep_link.rs` | 2,024 (796 URLs, 25 window states incl. every failing call, `second-instance` argv) | 0 |
| `stream-cache.js` → `stream_cache.rs` | 20,000 ops (TTL edges, clock going backwards, sweep above 200; internal size compared) | 0 |
| `logger.js` → `logger.rs` | 2,001 (1,500 entries, every newest entry checked, plus the final 500-cap buffer) | 0 |
| `audio-cache.js` → `audio_cache.rs` | 60 (15 env/tree cases: results, warnings, resulting trees) | 0 |

| IPC seam (`shim.js` + `codec.js` + `ipc.rs`) vs **real Electron 33** (contextBridge + ipcRenderer) | 314 (63 values both directions, handler-received values, 11 thrown kinds, `send`, events, API object shape, missing-handler rejection) | 0 |
| Media origin (`media_proxy.rs` + shim) vs real Electron 33 `file://` page | 6 deep cases (innerHTML file:// cover, missing file, file:// audio via Web Audio, `#`-in-name quirk, live googlevideo via Web Audio, src/currentSrc/getAttribute) | 0 |
| `app.getLocale()` → `electron_locale.rs` | 1,086 recorded Electron runs (every ISO region × en/es/pt/zh, LANGUAGE/LC_*/LANG precedence, aliases) | 0 |

| `themes.js` → `themes.rs` (unmodified source; Electron, dialogs, shell, https mocked identically) | 162 (12 scenarios: results, log lines, dialog/openPath/https calls, resulting trees with file contents) | 0 |
| `plugins.js` → `plugins.rs` | 103 (8 scenarios incl. dev-mode reads of the real `plugins/`, path traversal, odd manifests, binary logos) | 0 |
| V8 `JSON.parse` → `js::json_parse` (values and exact SyntaxError messages, lone surrogates) | 12,009 fuzzed documents | 0 |
| Real network failures → `real_host.rs` (Node `https.get` message + error properties) | 4 live cases (DNS, refused, invalid URL, wrong protocol) | 0 |
| `music-metadata` 7.14 → `src/mm/` (title/artist/album/picture/duration as `parseLocalFile` reads them, or the thrown message) | 1,091 files (ffmpeg-made MP3 CBR/VBR/Xing/VBRI, FLAC, Ogg Vorbis/Opus/Speex/Theora, MP4/M4A, WAV, AIFF, WMA, APE/Lyrics3/ID3v1/v2.2–2.4; plus truncations, byte flips and hand-built tags) | 0 |
| `child_process.execFile` → `node/child_process.rs` (exit codes, signals incl. real-time, timeout and external kills, maxBuffer, spawn errors thrown vs passed to the callback, glibc `execvp` PATH search and `/bin/sh` fallback, bad UTF-8, odd argument values) | 34 scripted cases | 0 |
| `util.inspect(string)` → `js::inspect_string` (quote choice, escapes, lone surrogates, newline split, 10k cap) | 4,003 fuzzed strings | 0 |
| `ytdlp.js` → `ytdlp.rs` (Linux/Windows lookup order, copy + chmod + `-U`, every setup failure) | 30 (10 scenarios: results, log lines, resulting trees) | 0 |
| `lyrics.js` + synclyrics → `lyrics.rs` (every request's URL/headers/redirect mode, results, log lines; token redirect + cookie jar, captcha retry, LRU eviction, concurrent lookups, malformed JSON at every step, odd renderer values) | 418 (9 scenarios) | 0 |
| `discord.js` + discord-rpc → `discord.rs` (results and every frame a scripted fake Discord receives: handshake, SET_ACTIVITY payloads incl. odd values, ERROR replies, PING/PONG, CLOSE opcode, hang-ups, server-sent event names, READY variants, coalesced frames, connect timeout, disconnect mid-connect) | 90 (10 scenarios) | 0 |
| `ytmusic.js` + ytmusic-api → `ytmusic.rs` + `ytm/` (every handler result, log line and wire request — method, URL, headers, body, agent proxy — for a recorded live session plus 13 edge scenarios: HTTP/network/parse errors, bodies without `responseContext`, zod mismatch dumps, `Object.prototype` search types, cookie rules, proxy strings, init failures, odd arguments) | 200 | 0 |
| `ytmusic.js` yt-dlp/download/cache/`net:httpGet` handlers → `ytmusic.rs` (results, logs, HTTP calls, save dialogs, and the resulting tree incl. the fake yt-dlp's argv log: cache hits, source dedupe, soundcloud, stderr vs message rejections, sync `execFile` throws, cancel/partial cleanup, header sanitising) | 55 | 0 |
| Firebase Auth SDK → `firebase/mod.rs` (every Identity Toolkit/securetoken request incl. SDK headers and heartbeat, error codes/messages, token refresh, auth-state events, odd credential values) | 93 | 0 |
| `firestore/lite` serializer → `firebase/firestore.rs` (setDoc merge masks, updateDoc paths, value encoding, validation errors, read decoding) | 527 | 0 |
| `cloud-auth.js` → `cloud_auth.rs` (13 scenarios: every handler result, log line, `auth:stateChanged` send, Firestore document state and saved credentials; legacy migration/backfill, auto sign-in, image reads, odd arguments) | 110 | 0 |
| `updater.js` → `updater.rs` (changelog and release list: every response shape, HTTP status, timeout, network error, odd version values; logs) | 51 | 0 |
| rest of `media.js` → `media.rs` (Spotify CSV, playlist CSV export, library export/import, cover images, local audio pick/scan/copy) | 81 (5 scenarios: results, log lines, dialog calls, resulting trees) | 0 |

Total: ~139,000 comparisons, 0 differences. Mutation checks: i18n 3/3 caught; step 2 9/12
caught; step 5 (`media.rs`) 9/11 caught. Every survivor is listed under Coverage gaps.

## Inventory and order (leaf-first)

| # | Source | LOC | Channels | Notes / Rust replacement |
|---|---|---|---|---|
| 1 | `i18n.js` | 25 | — | **done** |
| 2 | deep links (`main.js`), `stream-cache.js`, `logger.js`, `audio-cache.js` | ~120 | 3 | **done**, plus the shared `js.rs` / `node.rs` runtime layer. `main.js` is tested unmodified, so no extraction commit was needed. `_activeDownloadProc` moves with `ytmusic.js` (step 10) |
| 3 | **Tauri shell**: `window.js`, `thumbbar.js`, window/shell bits of `media.js`, preload shim | ~250 | ~12 | **done on Linux** (`shell.rs`). Engine spike passed: the renderer boots in WebKitGTK; YouTube opus and local audio play through Web Audio and the LUFS worklet (measured −15.9 LUFS on a real stream). Windows thumbbar buttons and Windows `setOpenAtLogin` still to do (Coverage gaps) |
| 4 | `themes.js`, `plugins.js` | 390 | 14 | **done**. Shared: `host.rs` (test seam for userData, dialogs, opener, one HTTPS request), `net.rs` (the source's redirect/timeout loop), Node `fs`/`path` and error objects in `node.rs` |
| 5 | rest of `media.js` | ~300 | 14 | **done** except `spotify:matchTrack`, which needs ytmusic's `mapSongToTrack` (moves to step 10). `music-metadata` is reimplemented in `src/mm/` (no crate reproduces its tag priorities, duration estimates or error messages) |
| 6 | `ytdlp.js` | ~80 | — | **done** (working-tree version, `ytdlp.rs`), on top of a Node-exact `execFile` (`node/child_process.rs`) that ytmusic's stream/download handlers reuse. `mac-setup.js` is not ported (macOS dropped) |
| 7 | `lyrics.js` | 136 | 1 | **done** (`lyrics.rs`): `@stef-0012/synclyrics` 2.5.10 reimplemented from its bundle (Musixmatch desktop API, LRCLIB search, Netease), with its instance-wide `_fetching`/`_trackId` quirks. Network goes through a `fetch` seam on `Host` (undici defaults in `RealHost`) |
| 8 | `discord.js` | 66 | 4 | **done** (`discord.rs`): `@xhayper/discord-rpc` 1.3.0's IPC transport reimplemented (no crate reproduces its socket discovery, one-frame-per-read parsing or event quirks). Windows named pipes come with the Windows runner |
| 9 | `updater.js` | 87 | 7 | **done except the packaged update flow** (`updater.rs`): version, locale, changelog, release list, dev-mode check/install, `app:restart` (relaunch + quit through the close interception). electron-updater → `tauri-plugin-updater` lands with step 12, because it needs the owner's signing key |
| 10 | `ytmusic.js` | 837 | 23 | **done** (`ytmusic.rs`, `ytm/`). `ytmusic-api` 5.3.1 is reimplemented in `src/ytm/` with the axios 1.13 behaviour it depends on (header merging, silent JSON parsing, status validation, `AxiosError#toJSON`), tough-cookie 4.1.4's jar, zod 3.25's `safeParse` + `zodToJsonSchema` log dumps (schemas generated by `parity/node/gen_ytm_schemas.js`), and V8's small-array sort. `youtubei.js` is only used by the mobile app, so it isn't ported |
| 11 | `cloud-auth.js` + `firebase.js` | ~420 | 11 | **done except the gaps in `FIREBASE-11A-REPORT.md`** (`cloud_auth.rs`, `cloud_backend.rs`, `firebase/`). Auth over Identity Toolkit REST, Firestore over REST (no Rust SDK). Password in the OS keyring (`keyring` crate, Secret Service / Credential Manager), cleared on sign-out. `app.getPath('pictures'/'downloads'/'desktop')` follows Chromium's xdg-user-dirs lookup |
| 12 | cut-over | — | — | delete `src/main*`, `preload.js`, `firebase.js`, electron/electron-builder deps; move packaging + release workflow to `tauri build`. Bundle config is in place: AppImage + NSIS, `snowify://` scheme, Windows `.ico`. electron-builder skipped a missing `bin/<os>/yt-dlp` but Tauri fails the build, so the yt-dlp resources live in `release/{linux,windows}.json`: run `node scripts/download-yt-dlp.js <os>`, then `tauri build --config release/<os>.json` |

The Node main process stays the shipping app until step 12. "Run both" happens in CI through
the parity tests, not as shadow traffic, because a Tauri process can't host the Node main
process.

## Risks — accepted by owner 2026-10-27

All three were accepted as-is. The engine spike at step 3 still runs, to find problems early.


1. **Webview engine.** Tauri uses WebKitGTK on Linux, WKWebView on macOS and WebView2 on
   Windows, not Electron's Chromium. The renderer's Web Audio graph (crossfade, the LUFS
   `AudioWorklet`), media codecs for YouTube streams (opus/webm on WebKitGTK needs
   GStreamer plugins) and CSS can all behave differently. That's outside the IPC contract
   but the user sees it. It gets a spike at step 3 (run the unmodified renderer, play a track)
   before more is invested.
2. **Auto-update.** `electron-updater` reads `latest*.yml` from GitHub releases;
   `tauri-plugin-updater` uses a different manifest and signing scheme. Existing Electron
   installs can't auto-update into the Tauri build, so a one-time manual migration release
   is needed. Accepted 2026-10-27.
3. **`app.commandLine.appendSwitch('disable-renderer-backgrounding')`** needs a webview
   equivalent (background throttling would stall playback).

## Not ported

- **`youtubei.js`**: only `src/mobile/` uses it; the desktop main process never loads it.
- **Electron `safeStorage` (Chromium OSCrypt) compatibility** for `auth_credentials.json`
  (owner decision 2026-11-01). The port keeps the password in the OS keyring under its own
  entry. Files written by the Electron build (`enc: "safeStorage-v1"`) can't be decrypted, so
  those users sign in once more after updating. The legacy plaintext-password migration
  path is still ported.

- **macOS**, all of it (owner decision 2026-10-30, see End state).
- **PAC scripts and proxy auto-detect (WPAD)** for YouTube Music traffic (owner decision
  2026-10-30). `session.resolveProxy` is replaced by Chromium's Linux rules for manual
  proxies (environment variables, GNOME/KDE manual settings; HTTP and SOCKS). A PAC or
  auto-detect configuration resolves to DIRECT.
Nothing yet. Owner decision 2026-10-27: port every behaviour, including unreachable edge cases,
even if that means remaking a module from scratch. `i18n` therefore models JS lookups that fall
through to `Object.prototype`, non-string locale values, `String(v)` coercion of params
(including JS number formatting and replacer functions), and the `TypeError` paths.
`resolve_locale` takes the raw IPC value, so the `app:setLocale` command must pass the argument
through as `serde_json::Value`.

One boundary is enforced at build time instead of at runtime. Shipped locale files must be JSON
objects of strings, and `tests/parity_i18n.rs` fails otherwise. JS would also accept an array, a
number or a string at the top level, or lone-surrogate escapes, and would then look keys up on
that value's prototype. Since the files ship with the app, that can only happen through a
malformed build.

## Coverage gaps (tracked; not exceptions)

- **Per-OS runtime behaviour, verified on Linux only.** `src/uv_errors.rs` is the Linux errno
  map, and the crate refuses to compile elsewhere until it's regenerated per OS. Windows
  `os.tmpdir` is ported but can't run differentially on Linux (Node fixes `isWindows` at
  load). Still to port with a Windows runner: Node's `fixWinEPERMSync` and
  `toNamespacedPath` (`\\?\` prefixes in error paths), and the Win32→libuv error
  translation. Plan: a CI runner on windows-latest running the same `cargo test`.
- **Platform shell pieces** (need a Windows runner): thumbbar buttons (`ITaskbarList3`),
  `setOpenAtLogin` (registry Run key), and the media proxy on WebView2.
- **Electron's uncaught-exception box** ("A JavaScript error occurred in the main process").
  Electron shows it when a main-process listener throws, e.g. `second-instance` on a
  destroyed window, which `shell.rs` currently drops silently. Scheduled with the cut-over.
  Rust has no JS stack, so the box will read `Name: message` without frames.
- **Equivalent / unreachable mutants** (step 2):
  - Deep link: stripping the second leading slash makes no difference, because empty
    segments are filtered anyway.
  - `mkdir`: "EEXIST on a non-dir parent → ENOTDIR" needs the tree to change mid-call
    (a race).
  - rimraf: "`EPERM` → retry as rmdir" can't happen on Linux without root (it was the
    macOS path; kept because it's Node's platform-independent code).
  - `discord.rs` (step 8): dropping the `exists_sync` filter on socket candidates is
    equivalent (a missing path fails to connect and is skipped). A server-sent
    `evt: "ready"` frame only changes what a *concurrent* `discord:connect` returns, so it is
    observable only through racy timing.
  - `media.rs` (step 5): `Number(null)` in the CSV duration is unreachable (a falsy
    `durationMs` skips it), and `deleteImage('')` fails `existsSync('')` either way.
- **`lyrics.rs` text handling:** titles go through the title/artist regexes as lossy UTF-8,
  so a lone surrogate in a track title becomes U+FFFD there (the source keeps it). The
  `trackId` path (which throws on one) is exact.
- **`discord.rs` timestamps:** `new Date(string)` is exact for the ECMAScript date-time
  format only; other strings need V8's legacy date parser and become NaN (`null` in the
  payload) here. The renderer sends numbers. Windows named-pipe transport: Windows runner.
- **ytmusic port, known approximations:**
  - Request bodies and `rawArtistsMap` keys use `String(value)` of renderer values; a video id
    naming an `Object.prototype` member (`"constructor"`, `"__proto__"`) would hit the
    prototype in the source.
  - `Intl…timeZone` comes from `$TZ` or the `/etc/localtime` link without ICU's alias
    canonicalisation.
  - With no agent proxy, axios would also honour `HTTP(S)_PROXY` from the environment (and
    send absolute-form requests through it); the port's client ignores the environment there.
  - A changing system proxy is picked up per `initYTMusic`, as in the source, but each
    distinct proxy keeps its own cached client.
  - V8's TimSort is ported exactly only below 64 elements (thumbnail lists are a handful).
  - The recorded session (`parity/corpus/ytmusic/`) is gitignored because it carries the
    recorder's region and time zone; `parity/node/ytm_record.js` makes a new one.
- **`media.js` value shapes not yet modelled:** a `Uint8Array` passed as a path argument
  (Node accepts it as a path), and a `Date` written into CSV cells (`String(date)` needs the
  local-time `Date#toString`).

## Step 3 design decisions

- **Media origin.** Electron's renderer was a `file://` page with a header hook adding
  `Access-Control-Allow-Origin: *` to googlevideo responses. The webview page is
  `tauri://localhost`, with no hook. Three options were tried:
  - `disable-web-security`: rejected, because it widens CORS for every origin.
  - Registering `tauri:` as a local scheme: rejected, because it breaks WebKit's own
    media-control icons (`blob:` URLs).
  - **Chosen:** the shim routes `file:` and googlevideo URLs on `img`/`audio`/`video`/`source`
    through a token-protected loopback server (`media_proxy.rs`). The shim's getters
    (`src`, `currentSrc`, `getAttribute`) report the original URLs, so the renderer sees
    Electron's values. This is verified differentially (`parity_media.rs`).
- **CSP.** Electron's CSP is used verbatim, plus `ipc: http://ipc.localhost` in
  `connect-src` for Tauri's IPC transport. The renderer never fetches those.
- **Exposure.** `window.snowify` is frozen on a non-writable, non-configurable, enumerable
  global, and its functions report `length` 0 and `name` `''`, as contextBridge does.
- **JavaScriptCore Errors** carry extra `line`/`column`/`sourceURL` properties. They are
  stripped from Errors decoded off IPC. Errors the renderer creates itself keep them; that's
  part of the accepted webview engine difference.

## Requirements carried into step 3 (Tauri shell) — all met

- ✅ **IPC transport must preserve structured-clone semantics.** Electron IPC carries
  `undefined`, `NaN`, `-0`, `Date`, `BigInt`, `Map` and typed arrays; Tauri `invoke` is JSON.
  Example: `appendLog({ msg: new Date() })` logs a *quoted* ISO string in the source, but
  would log it unquoted after a JSON round-trip. The preload shim has to tag non-JSON values,
  and the Rust side has to decode them (`logger::LogArg` already distinguishes them).
- ✅ `app:setLocale` takes its argument as a raw JS value (see `i18n::resolve_locale`).
- ✅ `second-instance`: a failing window call throws out of the event handler in the source
  (`deep_link::on_second_instance` returns `Err`). What Electron does with that uncaught
  main-process exception has to be reproduced at that point.
- ✅ argv/env can be non-UTF-8; the source sees them lossily decoded. Use `args_os` and lossy
  conversion, never `std::env::args` (it panics).

## Needs owner sign-off

- **JS stack traces inside log entries.** When a ytmusic request fails with an HTTP or
  network error, the log panel entry is `JSON.stringify(axiosError)`, and axios includes
  `stack`: Node's frames with file paths and line numbers of axios and ytmusic-api. The port
  writes the same entry with every other field identical, and `stack` is just the first
  line (`AxiosError: <message>`), because those JS frames don't exist any more. (Plain
  `Error`s log as `{}` in both, since `stack` isn't enumerable there.)

- **Terminal output of `console.*`.** The source prints the raw arguments through Node's
  `util.inspect` (colours, `[Object: null prototype]`, depth limits). The port prints the
  captured `msg` instead. The in-app log panel (`app:getLogs`) is identical; only the
  process's stdout/stderr differ. Porting `util.inspect` is a large job for output nobody
  sees in the packaged app. Say if you want it anyway.

## Follow-ups (improvements spotted; NOT done during the port)

- Non-English locales return the raw key for keys they're missing (de and others lack 7
  keys) instead of falling back to English.
- `stream-cache` only evicts expired entries once there are more than 200, so it can grow
  without bound.
