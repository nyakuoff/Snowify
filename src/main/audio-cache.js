const path = require('path');
const fs = require('fs');
const os = require('os');

const AUDIO_CACHE_DIR = path.join(os.tmpdir(), 'snowify-audio-cache');

// Shared ref so download handlers can cancel the active proc
let _activeDownloadProc = null;

function ensureCacheDir() {
  if (!fs.existsSync(AUDIO_CACHE_DIR)) fs.mkdirSync(AUDIO_CACHE_DIR, { recursive: true });
  return AUDIO_CACHE_DIR;
}

function cleanupCacheDir() {
  try {
    if (fs.existsSync(AUDIO_CACHE_DIR)) fs.rmSync(AUDIO_CACHE_DIR, { recursive: true, force: true });
  } catch (e) {
    console.warn('Cache cleanup:', e.message);
  }
}

function getActiveDownloadProc() { return _activeDownloadProc; }
function setActiveDownloadProc(proc) { _activeDownloadProc = proc; }

module.exports = { AUDIO_CACHE_DIR, ensureCacheDir, cleanupCacheDir, getActiveDownloadProc, setActiveDownloadProc };
