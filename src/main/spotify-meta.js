// ─── Spotify anonymous metadata enrichment ───
// Uses Spotify's public web-player token endpoint (no user account needed).
// Token is ~1 hour lifetime; cached in-process. On any failure, returns null.
'use strict';

const https = require('https');

const _tokenCache = { token: null, expiresAt: 0 };

function httpsGet(url, headers) {
  return new Promise((resolve, reject) => {
    const opts = new URL(url);
    const req = https.request({
      hostname: opts.hostname,
      path: opts.pathname + opts.search,
      method: 'GET',
      headers: {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
        Accept: 'application/json',
        ...headers,
      },
    }, (res) => {
      let data = '';
      res.on('data', chunk => { data += chunk; });
      res.on('end', () => {
        try { resolve({ status: res.statusCode, body: JSON.parse(data) }); }
        catch (_) { reject(new Error('Invalid JSON from ' + url)); }
      });
    });
    req.on('error', reject);
    req.setTimeout(8000, () => { req.destroy(new Error('Request timeout')); });
    req.end();
  });
}

async function fetchToken() {
  if (_tokenCache.token && Date.now() < _tokenCache.expiresAt) return _tokenCache.token;
  const res = await httpsGet(
    'https://open.spotify.com/get_access_token?reason=transport&productType=web_player',
    { 'Accept-Language': 'en' }
  );
  if (res.status !== 200 || !res.body?.accessToken) {
    throw new Error(`Spotify token fetch failed: HTTP ${res.status}`);
  }
  _tokenCache.token = res.body.accessToken;
  // Use the server's expiry, minus a 5-minute safety buffer
  const serverExpiry = res.body.accessTokenExpirationTimestampMs;
  _tokenCache.expiresAt = serverExpiry ? serverExpiry - 300_000 : Date.now() + 50 * 60 * 1000;
  return _tokenCache.token;
}

/**
 * Enrich a track with Spotify metadata (genres, popularity, Spotify ID).
 * Returns null on any error so callers can safely ignore failures.
 *
 * @param {string} title
 * @param {string} artist
 * @returns {Promise<{spotifyId:string, popularity:number, genres:string[]}|null>}
 */
async function enrichTrack(title, artist) {
  const token = await fetchToken();

  // Search for the track
  const q = encodeURIComponent(`track:${title}${artist ? ' artist:' + artist : ''}`);
  const searchRes = await httpsGet(
    `https://api.spotify.com/v1/search?q=${q}&type=track&limit=1`,
    { Authorization: `Bearer ${token}` }
  );
  if (searchRes.status !== 200) throw new Error(`Spotify search HTTP ${searchRes.status}`);

  const track = searchRes.body?.tracks?.items?.[0];
  if (!track) return null;

  const result = { spotifyId: track.id, popularity: track.popularity ?? 0, genres: [] };

  // Fetch genres from the primary artist
  const primaryArtistId = track.artists?.[0]?.id;
  if (primaryArtistId) {
    try {
      const artistRes = await httpsGet(
        `https://api.spotify.com/v1/artists/${primaryArtistId}`,
        { Authorization: `Bearer ${token}` }
      );
      if (artistRes.status === 200 && Array.isArray(artistRes.body?.genres)) {
        result.genres = artistRes.body.genres.slice(0, 5); // cap to 5
      }
    } catch (_) { /* genres are optional — ignore errors */ }
  }

  return result;
}

module.exports = { enrichTrack };
