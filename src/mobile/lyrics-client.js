/**
 * src/mobile/lyrics-client.js
 *
 * Browser-native lyrics fetching for mobile.
 * lrclib.net has open CORS headers — works directly from WebView.
 * Musixmatch requires a token flow that's complex; we skip it and fall back
 * to lrclib which covers the vast majority of tracks.
 */

const LRCLIB = 'https://lrclib.net/api';

// Simple LRU cache (50 entries)
const _cache = new Map();
function cacheGet(k) { return _cache.get(k); }
function cacheSet(k, v) {
  if (_cache.size >= 50) _cache.delete(_cache.keys().next().value);
  _cache.set(k, v);
}

export async function getLyrics(trackName, artistName, albumName, durationSec) {
  const key = `${trackName}|${artistName}|${durationSec}`;
  const cached = cacheGet(key);
  if (cached !== undefined) return cached;

  try {
    const params = new URLSearchParams({
      track_name:  trackName  || '',
      artist_name: artistName || '',
      album_name:  albumName  || '',
    });
    if (durationSec && durationSec > 0) {
      params.set('duration', Math.round(durationSec).toString());
    }

    const resp = await fetch(`${LRCLIB}/get?${params.toString()}`, {
      headers: { 'Lrclib-Client': 'Snowify Mobile' },
    });

    if (!resp.ok) {
      cacheSet(key, null);
      return null;
    }

    const data = await resp.json();

    let synced = null;
    if (data.syncedLyrics) {
      // Parse "[mm:ss.xx] lyric line" into the format the app expects:
      // [{ time: number (ms), text: string }]
      const lines = [];
      for (const raw of data.syncedLyrics.split('\n')) {
        const m = raw.match(/^\[(\d{1,2}):(\d{2})\.(\d{1,3})\]\s*(.*)/);
        if (!m) continue;
        const timeMs = (parseInt(m[1]) * 60 + parseFloat(`${m[2]}.${m[3]}`)) * 1000;
        lines.push({ time: timeMs, text: m[4] });
      }
      if (lines.length) synced = lines;
    }

    const plain = data.plainLyrics || null;
    if (!synced && !plain) {
      cacheSet(key, null);
      return null;
    }

    const result = { synced, plain, source: 'lrclib' };
    cacheSet(key, result);
    return result;
  } catch (err) {
    console.warn('[Lyrics] lrclib fetch failed:', err.message);
    cacheSet(key, null);
    return null;
  }
}
