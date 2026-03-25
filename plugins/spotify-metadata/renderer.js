// ─── Spotify Metadata Plugin ─────────────────────────────────────────────────
// Registers Spotify as a metadata source in Snowify.
// When enabled, Snowify enriches each played track in the background with:
//   • Genre tags (up to 5)
//   • Popularity score (0–100)
//   • Artist images (thumbnail, standard, large)
//   • Album art URL
//
// All Spotify API calls are made here in the renderer via window.snowify.httpGet,
// which routes through the main process to bypass CORS. No built-in Snowify code
// handles Spotify — this plugin is the complete integration.
// ─────────────────────────────────────────────────────────────────────────────
(function () {
  'use strict';

  // sp_t is a random anonymous tracking ID that Spotify's web player sends.
  const _spT = ([1e7] + -1e3 + -4e3 + -8e3 + -1e11).replace(/[018]/g, c =>
    (c ^ (crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (c / 4)))).toString(16));

  const _tokenCache = { token: null, expiresAt: 0 };

  async function fetchToken() {
    if (_tokenCache.token && Date.now() < _tokenCache.expiresAt) return _tokenCache.token;
    const res = await window.snowify.httpGet(
      'https://open.spotify.com/get_access_token?reason=transport&productType=web_player',
      { 'Cookie': `sp_t=${_spT}; sp_landing=/`, 'Referer': 'https://open.spotify.com/', 'Origin': 'https://open.spotify.com' }
    );
    if (!res || res.status !== 200 || !res.body?.accessToken) {
      throw new Error(`Spotify token fetch failed: HTTP ${res?.status}`);
    }
    _tokenCache.token = res.body.accessToken;
    const serverExpiry = res.body.accessTokenExpirationTimestampMs;
    _tokenCache.expiresAt = serverExpiry ? serverExpiry - 300_000 : Date.now() + 50 * 60 * 1000;
    return _tokenCache.token;
  }

  async function enrich(title, artist) {
    const token = await fetchToken();
    const q = encodeURIComponent(`track:${title}${artist ? ' artist:' + artist : ''}`);
    const searchRes = await window.snowify.httpGet(
      `https://api.spotify.com/v1/search?q=${q}&type=track&limit=1`,
      { 'Authorization': `Bearer ${token}`, 'Referer': 'https://open.spotify.com/', 'Origin': 'https://open.spotify.com' }
    );
    if (!searchRes || searchRes.status !== 200) throw new Error(`Spotify search HTTP ${searchRes?.status}`);

    const track = searchRes.body?.tracks?.items?.[0];
    if (!track) return null;

    const result = {
      spotifyId: track.id,
      popularity: track.popularity ?? 0,
      genres: [],
      artistImages: [],
      albumArt: track.album?.images?.[0]?.url ?? null,
    };

    const primaryArtistId = track.artists?.[0]?.id;
    if (primaryArtistId) {
      try {
        const artistRes = await window.snowify.httpGet(
          `https://api.spotify.com/v1/artists/${primaryArtistId}`,
          { 'Authorization': `Bearer ${token}`, 'Referer': 'https://open.spotify.com/', 'Origin': 'https://open.spotify.com' }
        );
        if (artistRes?.status === 200) {
          const artistData = artistRes.body;
          if (Array.isArray(artistData?.genres)) result.genres = artistData.genres.slice(0, 5);
          if (Array.isArray(artistData?.images)) {
            result.artistImages = artistData.images.map(img => ({ url: img.url, width: img.width, height: img.height }));
          }
        }
      } catch (_) { /* artist enrichment is optional */ }
    }

    return result;
  }

  function register() {
    if (!window.SnowifySources) {
      setTimeout(register, 100);
      return;
    }

    window.SnowifySources.registerMetaSource({
      id: 'spotify',
      label: 'Spotify',
      desc: 'Enrich tracks with genres, popularity, and artist images via Spotify.',
      enrich,
    });
  }

  register();
})();

