const { SyncLyrics } = require('@stef-0012/synclyrics');

let _mxmTokenData = null;

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

function register(ipcMain) {
  ipcMain.handle('lyrics:get', async (_event, trackName, artistName, albumName, durationSec) => {
    try {
      const data = await lyricsManager.getLyrics({
        track: trackName || '',
        artist: artistName || '',
        album: albumName || '',
        length: durationSec ? Math.round(durationSec * 1000) : undefined,
      });
      if (!data) return null;
      const synced = data.lyrics?.lineSynced?.lyrics || null;
      const plain = data.lyrics?.plain?.lyrics || null;
      const source = data.lyrics?.lineSynced?.source || data.lyrics?.plain?.source || 'Unknown';
      if (!synced && !plain) return null;
      return { synced, plain, source };
    } catch (err) {
      console.error('Lyrics fetch error:', err.message);
      return null;
    }
  });
}

module.exports = { register };
