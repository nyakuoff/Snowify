const _streamCache = new Map();
const STREAM_CACHE_TTL = 4 * 60 * 60 * 1000; // 4 hours

function getCachedUrl(key) {
  const entry = _streamCache.get(key);
  if (!entry) return null;
  if (Date.now() - entry.ts > STREAM_CACHE_TTL) {
    _streamCache.delete(key);
    return null;
  }
  return entry.value;
}

function setCachedUrl(key, value) {
  _streamCache.set(key, { value, ts: Date.now() });
  if (_streamCache.size > 200) {
    const now = Date.now();
    for (const [k, v] of _streamCache) {
      if (now - v.ts > STREAM_CACHE_TTL) _streamCache.delete(k);
    }
  }
}

module.exports = { getCachedUrl, setCachedUrl };
