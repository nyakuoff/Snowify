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
import { initializeApp } from 'firebase/app';
import {
  getAuth,
  onAuthStateChanged as fbOnAuthStateChanged,
  signInWithEmailAndPassword,
  createUserWithEmailAndPassword,
  sendPasswordResetEmail,
  signOut as fbSignOut,
  updateProfile as fbUpdateProfile,
} from 'firebase/auth';
import {
  initializeFirestore,
  persistentLocalCache,
  doc,
  getDoc,
  setDoc,
  updateDoc,
} from 'firebase/firestore';

// Must match MainActivity.PROXY_PORT
const PROXY_PORT = 17890;
const proxyUrl = (url) => `http://127.0.0.1:${PROXY_PORT}/stream?url=${encodeURIComponent(url)}`;
const PROXY_PREFIX = `http://127.0.0.1:${PROXY_PORT}/stream?url=`;

function isHttpUrl(value) {
  return typeof value === 'string' && /^https?:\/\//i.test(value);
}

function isProxyAssetUrl(value) {
  return typeof value === 'string' && value.startsWith(PROXY_PREFIX);
}

function getRawUrlFromProxy(value) {
  if (!isProxyAssetUrl(value)) return null;
  try {
    const u = new URL(value);
    return u.searchParams.get('url');
  } catch {
    return null;
  }
}

function deproxyUrl(value) {
  const raw = getRawUrlFromProxy(value);
  return raw || value;
}

function normalizeForStorage(value, key = '') {
  if (Array.isArray(value)) {
    return value.map((entry) => normalizeForStorage(entry));
  }
  if (value && typeof value === 'object') {
    const out = {};
    for (const [entryKey, entryValue] of Object.entries(value)) {
      out[entryKey] = normalizeForStorage(entryValue, entryKey);
    }
    return out;
  }
  if (typeof value === 'string') {
    if (isProxyAssetUrl(value)) return deproxyUrl(value);
    if (/^(thumbnail|avatar|banner|photoURL|logoUrl|albumArt|artwork)$/i.test(key)) return value;
  }
  return value;
}

function resolveImageUrl(url) {
  if (!isHttpUrl(url) || isProxyAssetUrl(url)) return url;
  return proxyUrl(url);
}

function resolveStreamUrl(url, platform) {
  if (!isHttpUrl(url)) return url;
  // Android uses native ExoPlayer via MobilePlayerPlugin, so it can consume
  // remote stream URLs directly without WebView CORS constraints.
  if (platform === 'android') return url;
  return proxyUrl(url);
}

function proxifyArtworkUrls(value, key = '') {
  if (Array.isArray(value)) {
    return value.map((entry) => proxifyArtworkUrls(entry));
  }
  if (value && typeof value === 'object') {
    const out = {};
    for (const [entryKey, entryValue] of Object.entries(value)) {
      out[entryKey] = proxifyArtworkUrls(entryValue, entryKey);
    }
    return out;
  }
  if (typeof value === 'string' && /^(thumbnail|avatar|banner|photoURL|logoUrl|albumArt|artwork)$/i.test(key)) {
    return resolveImageUrl(value);
  }
  return value;
}

const firebaseConfig = {
  apiKey: 'AIzaSyCNuw8kqgbULTLjC890BzKWvnmdvFCX0og',
  authDomain: 'snowify-dcda0.firebaseapp.com',
  projectId: 'snowify-dcda0',
  storageBucket: 'snowify-dcda0.firebasestorage.app',
  messagingSenderId: '925903561467',
  appId: '1:925903561467:web:c4298e4f47ac80d1c9c65e'
};

const firebaseApp = initializeApp(firebaseConfig);
const firebaseAuth = getAuth(firebaseApp);
// Capacitor WebViews are more reliable with Firestore long-polling than the
// default streaming transport.  persistentLocalCache enables IndexedDB offline
// persistence so setDoc() commits to local cache immediately (fast, offline-safe)
// and syncs to the server in the background — surviving app suspend/kill.
const firebaseDb = initializeFirestore(firebaseApp, {
  experimentalForceLongPolling: true,
  useFetchStreams: false,
  localCache: persistentLocalCache(),
});

const SYNC_COLLECTION_V2 = 'users_v2';
const SYNC_COLLECTION_LEGACY = 'users';
const syncReadyUsers = new Set();

function isPlainObject(value) {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function mergeForBackfill(v2Data = {}, legacyData = {}) {
  const merged = { ...v2Data };
  let changed = false;

  for (const [key, legacyValue] of Object.entries(legacyData || {})) {
    const v2Value = merged[key];

    if (v2Value === undefined || v2Value === null) {
      merged[key] = legacyValue;
      changed = true;
      continue;
    }

    if (Array.isArray(legacyValue) && Array.isArray(v2Value) && v2Value.length === 0 && legacyValue.length > 0) {
      merged[key] = legacyValue;
      changed = true;
      continue;
    }

    if (isPlainObject(legacyValue) && isPlainObject(v2Value)) {
      const nested = mergeForBackfill(v2Value, legacyValue);
      if (nested.changed) {
        merged[key] = nested.merged;
        changed = true;
      }
    }
  }

  return { merged, changed };
}

function syncDocRef(uid, useLegacy = false) {
  return doc(firebaseDb, useLegacy ? SYNC_COLLECTION_LEGACY : SYNC_COLLECTION_V2, uid);
}

async function migrateLegacyDocIfNeeded(uid) {
  if (syncReadyUsers.has(uid)) return null;
  try {
    const v2Snap = await getDoc(syncDocRef(uid));
    const v2Data = v2Snap.exists() ? (v2Snap.data() || {}) : null;

    const legacySnap = await getDoc(syncDocRef(uid, true));
    if (!legacySnap.exists()) {
      if (v2Data) syncReadyUsers.add(uid);
      return v2Data;
    }

    const legacyData = legacySnap.data() || {};
    if (!v2Data) {
      await setDoc(syncDocRef(uid), {
        ...legacyData,
        migratedFrom: SYNC_COLLECTION_LEGACY,
        migratedAt: Date.now(),
      }, { merge: true });

      syncReadyUsers.add(uid);
      return legacyData;
    }

    const { merged, changed } = mergeForBackfill(v2Data, legacyData);
    if (changed) {
      await setDoc(syncDocRef(uid), {
        ...merged,
        migratedFrom: SYNC_COLLECTION_LEGACY,
        migratedAt: Date.now(),
      }, { merge: true });
    }

    syncReadyUsers.add(uid);
    return changed ? merged : v2Data;
  } catch (_) {
    return null;
  }
}

async function readUserData(uid) {
  try {
    const v2Snap = await getDoc(syncDocRef(uid));
    if (v2Snap.exists()) {
      syncReadyUsers.add(uid);
      return v2Snap.data();
    }
  } catch (_) {
    // Fall through to legacy migration path.
  }

  return migrateLegacyDocIfNeeded(uid);
}

async function getUserInfo(user) {
  const info = {
    uid: user.uid,
    email: user.email,
    displayName: user.displayName,
    photoURL: user.photoURL || null,
  };

  try {
    const data = await readUserData(user.uid);
    if (data) {
      const profile = data.profile;
      if (profile?.photoURL) info.photoURL = profile.photoURL;
      if (profile?.displayName && !info.displayName) info.displayName = profile.displayName;
    }
  } catch (_) {}

  return proxifyArtworkUrls(info);
}

let _authSub = null;
const _authListeners = new Set();

function ensureAuthSubscription() {
  if (_authSub) return;
  _authSub = fbOnAuthStateChanged(firebaseAuth, async (user) => {
    const payload = user ? await getUserInfo(user) : null;
    _authListeners.forEach((cb) => {
      try { cb(payload); } catch (_) {}
    });
  });
}
import { getLyrics }   from './lyrics-client.js';
import { Filesystem, Directory, Encoding } from '@capacitor/filesystem';
import { App as CapacitorApp } from '@capacitor/app';
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
  try { await Filesystem.mkdir({ path: THEMES_DIR, directory: DATA_DIR, recursive: true }); } catch (_) {}
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

// ─── Native audio shim (Android / ExoPlayer) ──────────────────────────────
//
// Implements the HTMLAudioElement surface used by DualAudioEngine, but
// delegates all playback to the MobilePlayer Capacitor plugin which runs
// ExoPlayer on the native Android side.  This bypasses WebView CORS
// restrictions that block YouTube CDN stream URLs in the <audio> element.
//
// Two instances are created (shimId "a" and "b") corresponding to audio-player
// and audio-player-b.  document.getElementById / document.querySelector are
// patched inside installMobileBridge() so app.js transparently receives shims.

class NativeAudioShim extends EventTarget {
  constructor(shimId) {
    super();
    this.shimId   = shimId;
    this.id       = shimId === 'a' ? 'audio-player' : 'audio-player-b';
    this._src     = '';
    this._paused  = true;
    this._currentTime = 0;
    this._duration    = 0;
    this._volume      = 1;
    this._readyState  = 0;
    this._error       = null;
    // Dummy properties expected by LoudnessNormalizer / CSS code
    this.style    = {};
    this.preload  = 'auto';
    this.className = '';

    const P = () => window.Capacitor?.Plugins?.MobilePlayer;

    const attach = () => {
      const plugin = P();
      if (!plugin) return;
      plugin.addListener('playerReady', d => {
        if (d.id !== this.shimId) return;
        this._duration   = d.durationMs > 0 ? d.durationMs / 1000 : 0;
        this._readyState = 4;
        this._fire('loadedmetadata');
        this._fire('canplay');
        this._fire('canplaythrough');
      });
      plugin.addListener('playerTimeUpdate', d => {
        if (d.id !== this.shimId) return;
        this._currentTime = d.positionMs / 1000;
        if (d.durationMs > 0) this._duration = d.durationMs / 1000;
        this._fire('timeupdate');
      });
      plugin.addListener('playerEnded', d => {
        if (d.id !== this.shimId) return;
        this._paused = true;
        this._fire('ended');
      });
      plugin.addListener('playerError', d => {
        if (d.id !== this.shimId) return;
        this._paused = true;
        this._error  = { code: 4, message: d.message };
        this._fire('error');
      });
    };

    // Capacitor may not be fully ready synchronously — attach as soon as possible
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', attach, { once: true });
    } else {
      attach();
    }
  }

  _fire(name) {
    this.dispatchEvent(new Event(name));
  }

  // ── src property ──────────────────────────────────────────────────────────
  get src() { return this._src; }
  set src(url) {
    this._src        = url ?? '';
    this._readyState = 0;
    this._duration   = 0;
    this._currentTime = 0;
    if (this._src) {
      window.Capacitor?.Plugins?.MobilePlayer?.load({ id: this.shimId, url: this._src });
    }
  }

  // ── playback control ──────────────────────────────────────────────────────
  load() {
    if (this._src) {
      window.Capacitor?.Plugins?.MobilePlayer?.load({ id: this.shimId, url: this._src });
    }
  }

  play() {
    const plugin = window.Capacitor?.Plugins?.MobilePlayer;
    if (!plugin) return Promise.resolve();
    this._paused = false;
    return plugin.play({ id: this.shimId }).catch(e => { this._paused = true; throw e; });
  }

  pause() {
    this._paused = true;
    window.Capacitor?.Plugins?.MobilePlayer?.pause({ id: this.shimId });
  }

  // ── attributes ────────────────────────────────────────────────────────────
  removeAttribute(attr) {
    if (attr === 'src') {
      this._src        = '';
      this._readyState = 0;
      this._paused     = true;
      window.Capacitor?.Plugins?.MobilePlayer?.stop({ id: this.shimId });
    }
  }
  setAttribute() {} // no-op

  // ── volume ────────────────────────────────────────────────────────────────
  get volume() { return this._volume; }
  set volume(v) {
    this._volume = Math.max(0, Math.min(1, v));
    window.Capacitor?.Plugins?.MobilePlayer?.setVolume({ id: this.shimId, volume: this._volume });
  }

  // ── position / duration ───────────────────────────────────────────────────
  get currentTime() { return this._currentTime; }
  set currentTime(t) {
    this._currentTime = t;
    const plugin = window.Capacitor?.Plugins?.MobilePlayer;
    if (plugin) {
      plugin.seekTo({ id: this.shimId, positionMs: Math.round(t * 1000) })
        .then(() => this._fire('seeked'))
        .catch(() => {});
    }
  }

  get duration()   { return this._duration || 0; }
  get paused()     { return this._paused; }
  get readyState() { return this._readyState; }
  get error()      { return this._error; }
}

// ─── Install the bridge ───────────────────────────────────────────────────

export function installMobileBridge() {
  // The <audio> elements in index.html have crossorigin="anonymous" for the
  // desktop loudness normalizer (Electron doesn't enforce CORS for file://).
  // On mobile, Capacitor serves from https://localhost so Chromium enforces
  // CORS strictly — YouTube CDN never returns ACAO headers, causing
  // NotSupportedError before any audio plays.  Strip the attribute now so
  // the browser makes plain (non-CORS) requests to googlevideo.com.
  // Loudness normalization defaults to OFF, so this has no practical effect.
  // The <script> tag is late in <body> so the audio elements already exist.
  ['audio-player', 'audio-player-b'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.removeAttribute('crossorigin');
  });

  // Detect Android vs iOS from user-agent
  const ua = navigator.userAgent || '';
  const platform = /android/i.test(ua) ? 'android' : /iphone|ipad|ipod/i.test(ua) ? 'darwin' : 'linux';

  // On Android, replace the real <audio> elements with ExoPlayer-backed shims
  // so DualAudioEngine drives native playback without WebView CORS restrictions.
  if (platform === 'android') {
    const shimA = new NativeAudioShim('a');
    const shimB = new NativeAudioShim('b');
    const _origGetById  = document.getElementById.bind(document);
    const _origQuerySel = document.querySelector.bind(document);
    document.getElementById = (id) => {
      if (id === 'audio-player')   return shimA;
      if (id === 'audio-player-b') return shimB;
      return _origGetById(id);
    };
    document.querySelector = (sel) => {
      if (sel === '#audio-player')   return shimA;
      if (sel === '#audio-player-b') return shimB;
      return _origQuerySel(sel);
    };
  }

  window.snowify = {
    // Platform
    platform,

    // Window controls (no titlebar on mobile)
    minimize: () => {},
    maximize: () => {},
    close:    () => {},
    setMinimizeToTray: () => {},
    setOpenAtLogin:    () => {},

    resolveImageUrl,

    // YouTube Music
    search:             async (q, musicOnly) => proxifyArtworkUrls(await ytm.search(q, musicOnly)),
    searchArtists:      async q            => proxifyArtworkUrls(await ytm.searchArtists(q)),
    searchAlbums:       async q            => proxifyArtworkUrls(await ytm.searchAlbums(q)),
    searchVideos:       async q            => proxifyArtworkUrls(await ytm.searchVideos(q)),
    searchPlaylists:    async q            => proxifyArtworkUrls(await ytm.searchPlaylists(q)),
    getPlaylistVideos:  async id           => proxifyArtworkUrls(await ytm.getPlaylistVideos(id)),
    searchSuggestions:  async q            => proxifyArtworkUrls(await ytm.searchSuggestions(q)),
    // iOS uses proxied stream URLs; Android uses direct URLs because playback
    // is handled by native ExoPlayer and does not need WebView proxying.
    getStreamUrl:       async (url, q)      => {
      const rawUrl = await ytm.getStreamUrl(url, q);
      return resolveStreamUrl(rawUrl, platform);
    },
    getVideoStreamUrl:  async (id, q, premuxed) => {
      const r = await ytm.getVideoStreamUrl(id, q, premuxed);
      return {
        videoUrl: r.videoUrl ? resolveStreamUrl(r.videoUrl, platform) : null,
        audioUrl: r.audioUrl ? resolveStreamUrl(r.audioUrl, platform) : null,
      };
    },
    getTrackInfo:       async id           => proxifyArtworkUrls(await ytm.getTrackInfo(id)),
    artistInfo:         async id           => proxifyArtworkUrls(await ytm.artistInfo(id)),
    albumTracks:        async id           => proxifyArtworkUrls(await ytm.albumTracks(id)),
    getUpNexts:         async id           => proxifyArtworkUrls(await ytm.getUpNexts(id)),
    explore:            async ()           => proxifyArtworkUrls(await ytm.explore()),
    charts:             async ()           => proxifyArtworkUrls(await ytm.charts()),
    browseMood:         async (bid, params) => proxifyArtworkUrls(await ytm.browseMood(bid, params)),
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

    // Account & cloud sync
    signInWithEmail: async (email, password) => {
      try {
        const r = await signInWithEmailAndPassword(firebaseAuth, email, password);
        return getUserInfo(r.user);
      } catch (err) {
        return { error: err.message };
      }
    },
    signUpWithEmail: async (email, password) => {
      try {
        const r = await createUserWithEmailAndPassword(firebaseAuth, email, password);
        return getUserInfo(r.user);
      } catch (err) {
        return { error: err.message };
      }
    },
    sendPasswordReset: async (email) => {
      try {
        await sendPasswordResetEmail(firebaseAuth, email);
        return { ok: true };
      } catch (err) {
        return { error: err.message };
      }
    },
    authSignOut: async () => {
      try {
        await fbSignOut(firebaseAuth);
        return true;
      } catch (err) {
        return { error: err.message };
      }
    },
    getUser: async () => {
      const user = firebaseAuth.currentUser;
      if (!user) return null;
      return getUserInfo(user);
    },
    updateProfile: async ({ displayName, photoURL }) => {
      const user = firebaseAuth.currentUser;
      if (!user) return { error: 'Not signed in' };
      try {
        if (displayName !== undefined) {
          await fbUpdateProfile(user, { displayName });
        }
        const profileData = { displayName: user.displayName || '' };
        if (photoURL !== undefined) profileData.photoURL = deproxyUrl(photoURL);
        await migrateLegacyDocIfNeeded(user.uid);
        await setDoc(syncDocRef(user.uid), { profile: profileData }, { merge: true });
        return proxifyArtworkUrls({
          uid: user.uid,
          email: user.email,
          displayName: user.displayName,
          photoURL: photoURL !== undefined ? photoURL : user.photoURL,
        });
      } catch (err) {
        return { error: err.message };
      }
    },
    updateProfileExtras: async ({ banner, bio }) => {
      const user = firebaseAuth.currentUser;
      if (!user) return { error: 'Not signed in' };
      try {
        await migrateLegacyDocIfNeeded(user.uid);
        const docRef = syncDocRef(user.uid);
        const updateData = {};
        if (banner !== undefined) updateData['profile.banner'] = deproxyUrl(banner);
        if (bio !== undefined) updateData['profile.bio'] = String(bio).slice(0, 200);
        try {
          await updateDoc(docRef, updateData);
        } catch (_) {
          const profile = {};
          if (banner !== undefined) profile.banner = deproxyUrl(banner);
          if (bio !== undefined) profile.bio = String(bio).slice(0, 200);
          await setDoc(docRef, { profile }, { merge: true });
        }
        return { success: true };
      } catch (err) {
        return { error: err.message };
      }
    },
    getProfile: async (uid) => {
      try {
        const data = await readUserData(uid);
        if (!data) return null;
        const profile = data.profile || {};
        if (data['profile.banner'] && !profile.banner) profile.banner = data['profile.banner'];
        if (data['profile.bio'] && !profile.bio) profile.bio = data['profile.bio'];
        return proxifyArtworkUrls(profile);
      } catch (_) {
        return null;
      }
    },
    readImage: async (filePath) => {
      if (typeof filePath === 'string' && filePath.startsWith('data:image/')) return filePath;
      return null;
    },
    cloudSave: async (data) => {
      const user = firebaseAuth.currentUser;
      if (!user) return { error: 'Not signed in' };
      try {
        await migrateLegacyDocIfNeeded(user.uid);
        const cleanData = normalizeForStorage(data);
        await setDoc(syncDocRef(user.uid), { ...cleanData, updatedAt: Date.now() }, { merge: true });
        return true;
      } catch (err) {
        return { error: err.message };
      }
    },
    cloudLoad: async () => {
      const user = firebaseAuth.currentUser;
      if (!user) return null;
      try {
        const data = await readUserData(user.uid);
        return data ? proxifyArtworkUrls(data) : null;
      } catch (_) {
        return null;
      }
    },
    onAuthStateChanged: (callback) => {
      ensureAuthSubscription();
      _authListeners.add(callback);
      return () => _authListeners.delete(callback);
    },

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

    // App lifecycle — fire registered callback when the app goes to background
    // so pending saves (localStorage + cloud) are flushed before the OS can kill the process.
    onBeforeClose: (callback) => {
      CapacitorApp.addListener('appStateChange', async ({ isActive }) => {
        if (!isActive) {
          try { await callback(); } catch (_) {}
        }
      });
    },
    closeReady: () => {},
    onThumbarNext:    () => {},

    // Discord (no-op)
    connectDiscord:    () => Promise.resolve(null),
    disconnectDiscord: () => Promise.resolve(null),
    updatePresence:    () => Promise.resolve(null),
    clearPresence:     () => Promise.resolve(null),

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
