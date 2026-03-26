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
// Artist banner/header images are fetched via Spotify's Partner API using the same
// web-player token. This is best-effort and falls back gracefully if unavailable.
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

  async function getArtistMeta(artistName) {
    const token = await fetchToken();
    const q = encodeURIComponent(artistName);
    const res = await window.snowify.httpGet(
      `https://api.spotify.com/v1/search?q=${q}&type=artist&limit=1`,
      { 'Authorization': `Bearer ${token}`, 'Referer': 'https://open.spotify.com/', 'Origin': 'https://open.spotify.com' }
    );
    if (!res || res.status !== 200) return null;
    const artist = res.body?.artists?.items?.[0];
    if (!artist) return null;

    const result = {
      avatar: artist.images?.[0]?.url ?? null,
      banner: null,
      bio: null,
      genres: artist.genres?.slice(0, 5) ?? [],
      followers: artist.followers?.total ?? null,
    };

    // Try Spotify's Partner API for the artist's header/panoramic banner + bio.
    // This uses the same web-player token and is fragile (hash may change),
    // but fails silently — the rest of the metadata is already populated.
    try {
      const vars = encodeURIComponent(JSON.stringify({
        uri: `spotify:artist:${artist.id}`,
        locale: '',
        includePrerelease: false,
      }));
      const exts = encodeURIComponent(JSON.stringify({
        persistedQuery: {
          version: 1,
          sha256Hash: '35648a112beb1794e39ab931365f6ae4a8d45e65396d641eeda94e4003d41497',
        },
      }));
      const partnerRes = await window.snowify.httpGet(
        `https://api-partner.spotify.com/pathfinder/v1/query?operationName=queryArtistOverview&variables=${vars}&extensions=${exts}`,
        {
          'Authorization': `Bearer ${token}`,
          'app-platform': 'WebPlayer',
          'Referer': 'https://open.spotify.com/',
          'Origin': 'https://open.spotify.com',
        }
      );
      if (partnerRes?.status === 200) {
        const visuals = partnerRes.body?.data?.artistUnion?.visuals;
        // Header image — wide panoramic banner (e.g. 2400×800)
        const headerSources = visuals?.headerImage?.sources;
        if (headerSources?.length) {
          const best = [...headerSources].sort((a, b) => (b.width || 0) - (a.width || 0))[0];
          if (best?.url) result.banner = best.url;
        }
        // Higher-res avatar from visuals if available
        const avatarSources = visuals?.avatarImage?.sources;
        if (avatarSources?.length) {
          const best = [...avatarSources].sort((a, b) => (b.width || 0) - (a.width || 0))[0];
          if (best?.url) result.avatar = best.url;
        }
        // Bio text
        const bio = partnerRes.body?.data?.artistUnion?.biography?.text;
        if (bio) result.bio = bio;
      }
    } catch (_) { /* partner API is best-effort — banner/bio stay null if unavailable */ }

    return result;
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
      getArtistMeta,
    });
  }

  register();
})();

