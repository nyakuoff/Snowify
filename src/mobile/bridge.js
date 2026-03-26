/**
 * src/mobile/bridge.js
 *
 * Installs window.snowify for Capacitor (Android / iOS).
 * Mirrors the full API surface exposed by src/preload.js so that
 * src/renderer/app.js runs unchanged.
 *
 * Feature mapping:
 *  - YouTube Music data + stream URLs → ytm-client.js (InnerTube fetch + youtubei.js)
 *  - Lyrics                           → lyrics-client.js (lrclib.net)
 *  - File system (themes, plugins,    → @capacitor/filesystem
 *    library export/import, covers)
 *  - File picking                     → @capacitor/filesystem (read-only via <input>)
 *  - External links                   → window.open (Capacitor handles via browser)
 *  - Desktop-only (Discord, tray,
 *    window controls, auto-updater)   → graceful no-ops
 */

import * as ytm from './ytm-client.js';
import { getLyrics }   from './lyrics-client.js';
import { Filesystem, Directory, Encoding } from '@capacitor/filesystem';
import { StatusBar, Style } from '@capacitor/status-bar';

// ─── Filesystem helpers ───────────────────────────────────────────────────

const DATA_DIR = Directory.Data;  // app's Documents / internal storage

async function fsRead(path) {
  try {
    const { data } = await Filesystem.readFile({ path, directory: DATA_DIR, encoding: Encoding.UTF8 });
    return data;
  } catch (_) { return null; }
}

async function fsWrite(path, data) {
  // Ensure parent directories exist
  const parts = path.split('/');
  if (parts.length > 1) {
    try {
      await Filesystem.mkdir({ path: parts.slice(0, -1).join('/'), directory: DATA_DIR, recursive: true });
    } catch (_) {}
  }
  await Filesystem.writeFile({ path, directory: DATA_DIR, data, encoding: Encoding.UTF8 });
}

async function fsDelete(path) {
  try { await Filesystem.deleteFile({ path, directory: DATA_DIR }); } catch (_) {}
}

async function fsList(path) {
  try {
    const { files } = await Filesystem.readdir({ path, directory: DATA_DIR });
    return files || [];
  } catch (_) { return []; }
}

async function fsReadBinary(path) {
  try {
    const { data } = await Filesystem.readFile({ path, directory: DATA_DIR });
    return data; // base64 string
  } catch (_) { return null; }
}

async function fsWriteBinary(path, base64data) {
  const parts = path.split('/');
  if (parts.length > 1) {
    try {
      await Filesystem.mkdir({ path: parts.slice(0, -1).join('/'), directory: DATA_DIR, recursive: true });
    } catch (_) {}
  }
  await Filesystem.writeFile({ path, directory: DATA_DIR, data: base64data });
}

// ─── File picker helpers (HTML <input> fallback) ─────────────────────────

function pickFile(accept) {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = accept;
    input.addEventListener('change', async () => {
      const file = input.files?.[0];
      if (!file) return resolve(null);
      const reader = new FileReader();
      reader.onload = e => resolve({ name: file.name, data: e.target.result, size: file.size });
      reader.readAsDataURL(file);
    }, { once: true });
    input.click();
  });
}

function pickFiles(accept, multiple = false) {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = accept;
    input.multiple = multiple;
    input.addEventListener('change', async () => {
      const files = [...(input.files || [])];
      if (!files.length) return resolve([]);
      const results = await Promise.all(files.map(file => new Promise(res => {
        const reader = new FileReader();
        reader.onload = e => res({ name: file.name, data: e.target.result, path: file.name, size: file.size });
        reader.readAsDataURL(file);
      })));
      resolve(results);
    }, { once: true });
    input.click();
  });
}

// ─── Themes ───────────────────────────────────────────────────────────────

const THEMES_DIR    = 'snowify/themes';
const MKT_META_FILE = 'snowify/themes/marketplace.json';

async function scanThemes() {
  const files = await fsList(THEMES_DIR);
  return files
    .filter(f => (f.name || f).endsWith('.css'))
    .map(f => {
      const n = f.name || f;
      return { id: n.replace('.css', ''), name: n.replace('.css', ''), installed: true };
    });
}

async function loadTheme(id) {
  return fsRead(`${THEMES_DIR}/${id}.css`);
}

async function addTheme() {
  const file = await pickFile('.css');
  if (!file) return null;
  // file.data is a data URL: "data:text/css;base64,..."
  const b64 = file.data.split(',')[1];
  const cssText = atob(b64);
  const id = file.name.replace(/\.css$/i, '').replace(/[^a-zA-Z0-9_-]/g, '_');
  await fsWrite(`${THEMES_DIR}/${id}.css`, cssText);
  return { id, name: id };
}

async function removeTheme(id) {
  await fsDelete(`${THEMES_DIR}/${id}.css`);
}

async function getInstalledMarketplaceThemes() {
  const raw = await fsRead(MKT_META_FILE);
  return raw ? JSON.parse(raw) : [];
}

async function installMarketplaceTheme(entry) {
  const resp = await fetch(entry.css);
  const css  = await resp.text();
  await fsWrite(`${THEMES_DIR}/${entry.id}.css`, css);
  const installed = await getInstalledMarketplaceThemes();
  const updated = installed.filter(t => t.id !== entry.id);
  updated.push({ id: entry.id, name: entry.name, version: entry.version });
  await fsWrite(MKT_META_FILE, JSON.stringify(updated));
  return true;
}

async function uninstallMarketplaceTheme(id) {
  await fsDelete(`${THEMES_DIR}/${id}.css`);
  const installed = await getInstalledMarketplaceThemes();
  await fsWrite(MKT_META_FILE, JSON.stringify(installed.filter(t => t.id !== id)));
}

// ─── Plugins ──────────────────────────────────────────────────────────────

const PLUGINS_DIR       = 'snowify/plugins';
const PLUGIN_REGISTRY_URL = 'https://raw.githubusercontent.com/nyakuoff/Snowify/main/plugins/registry.json';

async function getPluginRegistry() {
  try {
    const resp = await fetch(PLUGIN_REGISTRY_URL);
    return resp.json();
  } catch (_) { return []; }
}

async function getInstalledPlugins() {
  const dirs = await fsList(PLUGINS_DIR);
  const plugins = [];
  for (const d of dirs) {
    const id = d.name || d;
    const raw = await fsRead(`${PLUGINS_DIR}/${id}/snowify-plugin.json`);
    if (!raw) continue;
    try {
      const manifest = JSON.parse(raw);
      // Resolve local logo to base64 if it's a relative name
      if (manifest.logoUrl && !manifest.logoUrl.startsWith('data:') && !manifest.logoUrl.startsWith('http')) {
        const b64 = await fsReadBinary(`${PLUGINS_DIR}/${id}/${manifest.logoUrl}`);
        if (b64) {
          const ext = manifest.logoUrl.split('.').pop().toLowerCase();
          const mime = ext === 'svg' ? 'image/svg+xml' : `image/${ext}`;
          manifest.logoUrl = `data:${mime};base64,${b64}`;
        }
      }
      plugins.push(manifest);
    } catch (_) {}
  }
  return plugins;
}

async function installPlugin(entry) {
  const base = `https://raw.githubusercontent.com/${entry.repo}/${entry.branch || 'main'}/${entry.path || entry.id}`;
  const manifestText = await (await fetch(`${base}/snowify-plugin.json`)).text();
  await fsWrite(`${PLUGINS_DIR}/${entry.id}/snowify-plugin.json`, manifestText);

  if (entry.renderer) {
    const jsText = await (await fetch(`${base}/${entry.renderer}`)).text();
    await fsWrite(`${PLUGINS_DIR}/${entry.id}/${entry.renderer}`, jsText);
  }
  if (entry.styles) {
    const cssText = await (await fetch(`${base}/${entry.styles}`)).text();
    await fsWrite(`${PLUGINS_DIR}/${entry.id}/${entry.styles}`, cssText);
  }
  if (entry.logoUrl && !entry.logoUrl.startsWith('http')) {
    try {
      const logoResp = await fetch(`${base}/${entry.logoUrl}`);
      const blob     = await logoResp.blob();
      const b64 = await new Promise(res => {
        const r = new FileReader();
        r.onload = e => res(e.target.result.split(',')[1]);
        r.readAsDataURL(blob);
      });
      await fsWriteBinary(`${PLUGINS_DIR}/${entry.id}/${entry.logoUrl}`, b64);
    } catch (_) {}
  }
  return true;
}

async function uninstallPlugin(id) {
  try { await Filesystem.rmdir({ path: `${PLUGINS_DIR}/${id}`, directory: DATA_DIR, recursive: true }); } catch (_) {}
}

async function getPluginFiles(id) {
  const manifestRaw = await fsRead(`${PLUGINS_DIR}/${id}/snowify-plugin.json`);
  if (!manifestRaw) return null;
  const manifest = JSON.parse(manifestRaw);
  const js  = manifest.renderer ? await fsRead(`${PLUGINS_DIR}/${id}/${manifest.renderer}`) : null;
  const css = manifest.styles   ? await fsRead(`${PLUGINS_DIR}/${id}/${manifest.styles}`)   : null;
  return { manifest, js, css };
}

// ─── Playlist covers ──────────────────────────────────────────────────────

const COVERS_DIR = 'snowify/covers';

async function pickImage() {
  const file = await pickFile('image/*');
  if (!file) return null;
  // Return a temp data-url path; the renderer will call saveImage next
  return file.data; // data URL
}

async function saveImage(playlistId, sourceDataUrl) {
  if (!sourceDataUrl) return null;
  const b64  = sourceDataUrl.split(',')[1];
  const ext  = sourceDataUrl.includes('image/png') ? 'png' : 'jpg';
  const path = `${COVERS_DIR}/${playlistId}.${ext}`;
  await fsWriteBinary(path, b64);
  // Return a "virtual" path the app stores in state; on next load we resolve it
  return `snowify-cover://${playlistId}.${ext}`;
}

async function deleteImage(imagePath) {
  if (!imagePath) return;
  const name = imagePath.replace('snowify-cover://', '');
  await fsDelete(`${COVERS_DIR}/${name}`);
}

// ─── Library export/import ────────────────────────────────────────────────

async function exportLibrary(jsonStr) {
  const blob = new Blob([jsonStr], { type: 'application/json' });
  const url  = URL.createObjectURL(blob);
  const a    = document.createElement('a');
  a.href     = url;
  a.download = `snowify-library-${new Date().toISOString().slice(0, 10)}.json`;
  document.body.appendChild(a);
  a.click();
  setTimeout(() => { URL.revokeObjectURL(url); a.remove(); }, 1000);
  return { ok: true };
}

async function importLibrary() {
  const file = await pickFile('.json');
  if (!file) return null;
  const b64  = file.data.split(',')[1];
  return atob(b64);
}

// ─── Spotify CSV import ───────────────────────────────────────────────────

async function spotifyPickCsv() {
  const file = await pickFile('.csv');
  if (!file) return null;
  const b64     = file.data.split(',')[1];
  const csvText = atob(b64);

  // Parse CSV → tracks array (same logic as desktop media.js)
  const lines = csvText.split(/\r?\n/).filter(Boolean);
  const headers = lines[0].split(',').map(h => h.replace(/^"|"$/g, '').trim());
  const titleIdx  = headers.findIndex(h => /title|track/i.test(h));
  const artistIdx = headers.findIndex(h => /artist/i.test(h));
  const albumIdx  = headers.findIndex(h => /album/i.test(h));

  const tracks = [];
  for (let i = 1; i < lines.length; i++) {
    const cols = parseCSVLine(lines[i]);
    const title  = cols[titleIdx]?.trim()  || '';
    const artist = cols[artistIdx]?.trim() || '';
    const album  = cols[albumIdx]?.trim()  || '';
    if (title) tracks.push({ title, artist, album });
  }
  return tracks;
}

function parseCSVLine(line) {
  const result = [];
  let cur = '', inQuote = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (c === '"') { inQuote = !inQuote; }
    else if (c === ',' && !inQuote) { result.push(cur); cur = ''; }
    else { cur += c; }
  }
  result.push(cur);
  return result;
}

async function spotifyMatchTrack(title, artist) {
  // Use the same YTMusic search as desktop
  const results = await ytm.search(`${title} ${artist}`, true);
  return results[0] || null;
}

// ─── Export playlist as CSV (download) ────────────────────────────────────

async function exportPlaylistCsv(name, tracks) {
  const header = 'Title,Artist,Album\n';
  const rows   = tracks.map(t =>
    [`"${(t.title  || '').replace(/"/g, '""')}"`,
     `"${(t.artist || '').replace(/"/g, '""')}"`,
     `"${(t.album  || '').replace(/"/g, '""')}"`].join(',')
  ).join('\n');
  const csv = header + rows;
  const blob = new Blob([csv], { type: 'text/csv' });
  const url  = URL.createObjectURL(blob);
  const a    = document.createElement('a');
  a.href     = url;
  a.download = `${name || 'playlist'}.csv`;
  document.body.appendChild(a);
  a.click();
  setTimeout(() => { URL.revokeObjectURL(url); a.remove(); }, 1000);
  return { ok: true };
}

// ─── Local audio files ────────────────────────────────────────────────────

async function pickAudioFiles() {
  const files = await pickFiles('audio/*,.mp3,.flac,.ogg,.wav,.aac,.m4a,.opus,.wma,.aiff', true);
  return files.map(f => ({
    path:   f.name,
    title:  f.name.replace(/\.[^.]+$/, ''),
    artist: 'Unknown',
    album:  'Local',
    duration: 0,
    isLocal: true,
    localPath: f.data, // data URL for playback
  }));
}

function pickAudioFolder() {
  // Android folder picking requires SAF which needs a dedicated Capacitor plugin;
  // returning null gracefully will show "no folder selected" in the UI.
  return Promise.resolve(null);
}

function scanAudioFolder() { return Promise.resolve([]); }
function copyToPlaylistFolder() { return Promise.resolve({ ok: true }); }

// ─── Changelog / version ─────────────────────────────────────────────────

const GITHUB_API = 'https://api.github.com/repos/nyakuoff/Snowify';

async function getChangelog(version) {
  try {
    const resp = await fetch(`${GITHUB_API}/releases/tags/${version}`);
    if (!resp.ok) return null;
    const data = await resp.json();
    return data.body || null;
  } catch (_) { return null; }
}

async function getRecentReleases() {
  try {
    const resp = await fetch(`${GITHUB_API}/releases?per_page=20`);
    return resp.ok ? resp.json() : [];
  } catch (_) { return []; }
}

// ─── Install the bridge ───────────────────────────────────────────────────

export function installMobileBridge() {
  // Detect Android vs iOS from user-agent
  const ua = navigator.userAgent || '';
  const platform = /android/i.test(ua) ? 'android' : /iphone|ipad|ipod/i.test(ua) ? 'darwin' : 'linux';

  window.snowify = {
    // Platform
    platform,

    // Window controls (no titlebar on mobile)
    minimize: () => {},
    maximize: () => {},
    close:    () => {},
    setMinimizeToTray: () => {},
    setOpenAtLogin:    () => {},

    // YouTube Music
    search:             (q, musicOnly)     => ytm.search(q, musicOnly),
    searchArtists:      q                  => ytm.searchArtists(q),
    searchAlbums:       q                  => ytm.searchAlbums(q),
    searchVideos:       q                  => ytm.searchVideos(q),
    searchPlaylists:    q                  => ytm.searchPlaylists(q),
    getPlaylistVideos:  id                 => ytm.getPlaylistVideos(id),
    searchSuggestions:  q                  => ytm.searchSuggestions(q),
    getStreamUrl:       (url, q)           => ytm.getStreamUrl(url, q),
    getVideoStreamUrl:  (id, q, premuxed)  => ytm.getVideoStreamUrl(id, q, premuxed),
    getTrackInfo:       id                 => ytm.getTrackInfo(id),
    artistInfo:         id                 => ytm.artistInfo(id),
    albumTracks:        id                 => ytm.albumTracks(id),
    getUpNexts:         id                 => ytm.getUpNexts(id),
    explore:            ()                 => ytm.explore(),
    charts:             ()                 => ytm.charts(),
    browseMood:         (bid, params)      => ytm.browseMood(bid, params),
    setCountry:         () => Promise.resolve(true),

    // Caching: no-ops (no yt-dlp cache on mobile)
    downloadAudio:    () => Promise.resolve(null),
    saveSong:         () => Promise.resolve({ canceled: true }),
    deleteCachedAudio: () => Promise.resolve({ ok: true }),
    clearAudioCache:   () => Promise.resolve({ ok: true }),
    cancelDownload:    () => Promise.resolve({ ok: true }),

    // Lyrics
    getLyrics: (t, a, al, d) => getLyrics(t, a, al, d),

    // External links
    openExternal: url => { window.open(url, '_blank'); return Promise.resolve(); },

    // Themes
    scanThemes:                   ()      => scanThemes(),
    loadTheme:                    id      => loadTheme(id),
    reloadTheme:                  id      => loadTheme(id),
    addTheme:                     ()      => addTheme(),
    removeTheme:                  id      => removeTheme(id),
    openThemesFolder:             ()      => Promise.resolve(),
    getInstalledMarketplaceThemes: ()     => getInstalledMarketplaceThemes(),
    installMarketplaceTheme:      entry   => installMarketplaceTheme(entry),
    uninstallMarketplaceTheme:    id      => uninstallMarketplaceTheme(id),

    // Plugins
    getPluginRegistry: ()        => getPluginRegistry(),
    getInstalledPlugins: ()      => getInstalledPlugins(),
    installPlugin:  entry        => installPlugin(entry),
    uninstallPlugin: id          => uninstallPlugin(id),
    getPluginFiles: id           => getPluginFiles(id),
    restartApp:     ()           => { location.reload(); return Promise.resolve(); },

    // Playlist covers
    pickImage:     ()            => pickImage(),
    saveImage:     (id, src)     => saveImage(id, src),
    deleteImage:   path          => deleteImage(path),

    // Export
    exportPlaylistCsv: (n, t)   => exportPlaylistCsv(n, t),
    exportLibrary:  json         => exportLibrary(json),
    importLibrary:  ()           => importLibrary(),

    // Spotify import
    spotifyPickCsv:    ()        => spotifyPickCsv(),
    spotifyMatchTrack: (t, a)    => spotifyMatchTrack(t, a),

    // Local audio
    pickAudioFiles:         ()             => pickAudioFiles(),
    pickAudioFolder:        ()             => pickAudioFolder(),
    scanAudioFolder:        path           => scanAudioFolder(path),
    copyToPlaylistFolder:   (fp, dir)      => copyToPlaylistFolder(fp, dir),

    // App meta
    getVersion: () => Promise.resolve('2.0.0'),
    getLocale:  () => Promise.resolve(navigator.language?.split('-')[0] || 'en'),
    setLocale:  () => Promise.resolve(true),

    // Changelog
    getChangelog:     v => getChangelog(v),
    getRecentReleases: () => getRecentReleases(),

    // Auto-updater (no-op on mobile — handled by app store / manual APK)
    checkForUpdates:  () => Promise.resolve(null),
    installUpdate:    () => {},
    onUpdateStatus:   () => {},

    // Thumbbar (no-op on mobile)
    updateThumbar:    () => {},
    onThumbarPrev:    () => {},
    onThumbarPlayPause: () => {},
    onThumbarNext:    () => {},

    // Discord (no-op)
    connectDiscord:    () => Promise.resolve(null),
    disconnectDiscord: () => Promise.resolve(null),
    updatePresence:    () => Promise.resolve(null),
    clearPresence:     () => Promise.resolve(null),

    // Graceful close (no-op — no beforeunload hook needed on mobile)
    onBeforeClose: () => {},
    closeReady:    () => {},

    // Debug logs
    getLogs:    () => Promise.resolve([]),
    appendLog:  entry => { console.log('[PL]', entry?.message || entry); return Promise.resolve(); },

    // Deep links (stub; real deep links handled by Capacitor App plugin separately)
    onDeepLink:          () => {},
    getPendingDeepLink:  () => Promise.resolve(null),

    // Generic HTTP GET for plugins (fetch goes through CapacitorHttp natively)
    httpGet: async (url, headers = {}) => {
      try {
        const resp = await fetch(url, { headers });
        const body = await resp.json().catch(() => null);
        return { status: resp.status, body };
      } catch (_) { return null; }
    },
  };

  // ── Mobile-specific post-install setup ──────────────────────────────────

  // Dark status bar to match app theme
  try {
    StatusBar.setStyle({ style: Style.Dark });
    StatusBar.setBackgroundColor({ color: '#0d0d0d' });
  } catch (_) {}

  // Mark body so CSS can hide the desktop titlebar and apply safe areas
  document.documentElement.classList.add('platform-mobile');
  document.documentElement.classList.add(`platform-${platform}`);
}

// Auto-invoke when this script loads, but only in Capacitor/mobile environments.
// On Electron desktop, window.snowify is already set by contextBridge (preload.js)
// before any renderer scripts run, so we skip to avoid stomping it.
if (!window.snowify) {
  installMobileBridge();
}
