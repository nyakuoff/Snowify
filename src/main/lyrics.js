const { SyncLyrics } = require('@stef-0012/synclyrics');
const LRCLIB = 'https://lrclib.net/api';

let _mxmTokenData = null;
const _lyricsInflight = new Map();

// LRU cache (max 50 entries)
const _lyricsCache = new Map();
const lyricsCacheProxy = {
  get(key) { return _lyricsCache.get(key); },
  set(key, value) {
    if (_lyricsCache.size >= 50) {
      const oldest = _lyricsCache.keys().next().value;
      _lyricsCache.delete(oldest);
    }
    _lyricsCache.set(key, value);
  },
  has(key) { return _lyricsCache.has(key); },
  delete(key) { return _lyricsCache.delete(key); },
  clear() { _lyricsCache.clear(); },
  get size() { return _lyricsCache.size; }
};

const lyricsManager = new SyncLyrics({
  cache: lyricsCacheProxy,
  logLevel: 'none',
  sources: ['musixmatch', 'lrclib', 'netease'],
  saveMusixmatchToken: (tokenData) => { _mxmTokenData = tokenData; },
  getMusixmatchToken: () => _mxmTokenData,
});

function normalizeTitle(value) {
  return String(value || '')
    .replace(/\s*\((feat\.|ft\.|featuring)[^)]+\)/gi, '')
    .replace(/\s*\[(feat\.|ft\.|featuring)[^\]]+\]/gi, '')
    .replace(/\s*-\s*(remaster(ed)?|radio edit|live|mono|stereo|explicit)\b.*$/gi, '')
    .replace(/\s*\((remaster(ed)?|radio edit|live|mono|stereo|explicit)[^)]+\)/gi, '')
    .replace(/\s+/g, ' ')
    .trim();
}

function normalizeArtist(value) {
  const raw = String(value || '').trim();
  if (!raw) return '';
  return raw
    .split(/,|&| x | feat\.| ft\.| featuring /i)[0]
    .replace(/\s+/g, ' ')
    .trim();
}

function normalizeAlbum(value) {
  return String(value || '').replace(/\s+/g, ' ').trim();
}

function mapLyricsPayload(data) {
  if (!data) return null;
  const synced = data.lyrics?.lineSynced?.lyrics || data.syncedLyrics || null;
  const plain = data.lyrics?.plain?.lyrics || data.plainLyrics || null;
  const source = data.lyrics?.lineSynced?.source || data.lyrics?.plain?.source || data.source || 'Unknown';
  if (!synced && !plain) return null;
  return { synced, plain, source };
}

async function fetchViaSyncLyrics(query) {
  const data = await lyricsManager.getLyrics(query);
  return mapLyricsPayload(data);
}

async function fetchViaLrclib(query) {
  const params = new URLSearchParams({
    track_name: query.track || '',
    artist_name: query.artist || '',
    album_name: query.album || '',
  });
  if (query.length && query.length > 0) params.set('duration', Math.round(query.length / 1000).toString());

  const resp = await fetch(`${LRCLIB}/get?${params.toString()}`, {
    headers: { 'Lrclib-Client': 'Snowify Desktop' },
  });
  if (!resp.ok) return null;
  const data = await resp.json();
  return mapLyricsPayload(data ? { ...data, source: 'lrclib' } : null);
}

async function resolveLyrics(trackName, artistName, albumName, durationSec) {
  const normalizedTitle = normalizeTitle(trackName);
  const normalizedArtist = normalizeArtist(artistName);
  const normalizedAlbum = normalizeAlbum(albumName);
  const lengthMs = durationSec ? Math.round(durationSec * 1000) : undefined;

  const attempts = [
    { track: trackName || '', artist: artistName || '', album: albumName || '', length: lengthMs },
    { track: normalizedTitle, artist: artistName || '', album: albumName || '', length: lengthMs },
    { track: normalizedTitle, artist: normalizedArtist, album: normalizedAlbum, length: lengthMs },
    { track: normalizedTitle, artist: normalizedArtist, album: '', length: lengthMs },
    { track: normalizedTitle, artist: normalizedArtist, album: '', length: undefined },
  ].filter((attempt, index, arr) => {
    if (!attempt.track || !attempt.artist) return false;
    return arr.findIndex(a => JSON.stringify(a) === JSON.stringify(attempt)) === index;
  });

  for (const attempt of attempts) {
    const result = await fetchViaSyncLyrics(attempt).catch(() => null);
    if (result) return result;
  }

  for (const attempt of attempts) {
    const result = await fetchViaLrclib(attempt).catch(() => null);
    if (result) return result;
  }

  return null;
}

function register(ipcMain) {
  ipcMain.handle('lyrics:get', async (_event, trackName, artistName, albumName, durationSec) => {
    const cacheKey = `${normalizeTitle(trackName)}|${normalizeArtist(artistName)}|${Math.round(durationSec || 0)}`;
    if (_lyricsInflight.has(cacheKey)) return _lyricsInflight.get(cacheKey);

    const pending = (async () => {
    try {
      return await resolveLyrics(trackName, artistName, albumName, durationSec);
    } catch (err) {
      console.error('Lyrics fetch error:', err.message);
      return null;
    } finally {
      _lyricsInflight.delete(cacheKey);
    }
    })();

    _lyricsInflight.set(cacheKey, pending);
    return pending;
  });
}

module.exports = { register };
