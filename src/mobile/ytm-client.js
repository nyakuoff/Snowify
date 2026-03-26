/**
 * src/mobile/ytm-client.js
 *
 * YouTube Music client for mobile (Capacitor).
 * - Uses raw fetch() calls to the InnerTube API for all data queries.
 *   @capacitor/http intercepts all fetch() calls at the native layer,
 *   so CORS is bypassed transparently for music.youtube.com endpoints.
 * - Stream URLs are fetched via the InnerTube /player endpoint using the
 *   ANDROID_MUSIC client which returns pre-signed URLs (no cipher needed).
 */

// ─── InnerTube session state ───────────────────────────────────────────────

let _apiKey   = null;
let _context  = null;
let _initDone = false;
let _initP    = null;

// Android client context — returns pre-signed stream URLs without cipher.
// Client playback nonce — appended to stream URLs
function generateCpn() {
  const chars = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_';
  return Array.from({ length: 16 }, () => chars[Math.floor(Math.random() * 64)]).join('');
}

// ANDROID client v20 — confirmed returns plain `url` fields in adaptiveFormats.
// Tested against both youtube.com and music.youtube.com tracks (2026-03).
const ANDROID_CONTEXT = {
  client: {
    clientName: 'ANDROID',
    clientVersion: '20.02.7',
    androidSdkVersion: 34,
    hl: 'en',
    gl: 'US',
    userAgent: 'com.google.android.youtube/20.02.7 (Linux; U; Android 14) gzip',
  },
};

async function initSession() {
  if (_initDone) return;
  if (_initP) return _initP;

  _initP = (async () => {
    // 1. Fetch music.youtube.com to extract the InnerTube key + WEB_REMIX context
    try {
      const resp = await fetch('https://music.youtube.com/', { cache: 'no-store' });
      const html = await resp.text();
      const keyMatch  = html.match(/"INNERTUBE_API_KEY"\s*:\s*"([^"]+)"/);
      const ctxMatch  = html.match(/"INNERTUBE_CONTEXT"\s*:\s*(\{[\s\S]*?\})\s*,\s*"INNERTUBE_CONTEXT_CLIENT_NAME"/);
      if (keyMatch?.[1])  _apiKey  = keyMatch[1];
      if (ctxMatch?.[1]) {
        try { _context = JSON.parse(ctxMatch[1]); } catch (_) {}
      }
    } catch (_) {}

    // Fallback values tested as of 2025
    if (!_apiKey)  _apiKey  = 'AIzaSyC9XL3ZjWddXya6X74dJoCTL-KNEFJOM';
    if (!_context) _context = {
      client: {
        clientName: 'WEB_REMIX',
        clientVersion: '1.20241231.01.00',
        hl: 'en',
        gl: 'US',
        platform: 'DESKTOP',
        userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',
      },
    };

    _initDone = true;
    _initP    = null;
  })();
  return _initP;
}

async function musicRequest(endpoint, body) {
  await initSession();
  const resp = await fetch(
    `https://music.youtube.com/youtubei/v1/${endpoint}?key=${_apiKey}&prettyPrint=false`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ context: _context, ...body }),
    }
  );
  return resp.json();
}

// ─── Helpers (ported directly from src/main/ytmusic.js) ──────────────────

function formatDuration(seconds) {
  if (seconds == null || isNaN(seconds) || seconds <= 0) return '';
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, '0')}`;
}

function getBestThumbnail(thumbnails) {
  if (!thumbnails?.length) return '';
  return [...thumbnails].sort((a, b) => (b.width || 0) - (a.width || 0))[0]?.url || '';
}

function getSquareThumbnail(thumbnails, size = 226) {
  const url = getBestThumbnail(thumbnails);
  if (!url) return '';
  if (url.includes('lh3.googleusercontent.com')) {
    return url.replace(/=(?:w\d+-h\d+|s\d+|p-w\d+).*$/, `=w${size}-h${size}-l90-rj`);
  }
  return url;
}

function parseArtistsFromRuns(runs) {
  if (!runs?.length) return [];
  const artistRuns = runs.filter(r => {
    const pageType = r.navigationEndpoint?.browseEndpoint
      ?.browseEndpointContextSupportedConfigs
      ?.browseEndpointContextMusicConfig?.pageType;
    return pageType === 'MUSIC_PAGE_TYPE_ARTIST';
  });
  if (artistRuns.length > 0) {
    return artistRuns.map(r => ({
      name: r.text,
      id: r.navigationEndpoint.browseEndpoint.browseId,
    }));
  }
  if (runs.length >= 1 && !runs[0].navigationEndpoint) {
    const text = runs.map(r => r.text).join('');
    const dotIdx = text.indexOf(' \u2022 ');
    const artistText = dotIdx >= 0 ? text.slice(0, dotIdx) : text;
    return artistText.split(/,\s*|\s*&\s*/).filter(Boolean)
      .map(name => ({ name: name.trim(), id: null }));
  }
  return [];
}

function buildArtistFields(artists) {
  if (!artists?.length) return { artist: 'Unknown Artist', artistId: null, artists: [] };
  return {
    artist:   artists.map(a => a.name).join(', '),
    artistId: artists[0].id || null,
    artists,
  };
}

function mapSongFromShelf(r) {
  // r is a musicResponsiveListItemRenderer
  const cols = r.flexColumns || [];
  const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]
    ?.navigationEndpoint?.watchEndpoint?.videoId;
  if (!videoId) return null;
  const titleRuns = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
  const allRuns   = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
  const dotIdx    = allRuns.findIndex(run => run.text === ' \u2022 ');
  const artistRuns = dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns;
  const artists    = parseArtistsFromRuns(artistRuns);

  // Album from col[2] or col[3] browse endpoint
  let album = null, albumId = null;
  for (let i = 2; i < cols.length; i++) {
    const runs = cols[i]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
    const albumRun = runs.find(run =>
      run.navigationEndpoint?.browseEndpoint
        ?.browseEndpointContextSupportedConfigs
        ?.browseEndpointContextMusicConfig?.pageType === 'MUSIC_PAGE_TYPE_ALBUM'
    );
    if (albumRun) { album = albumRun.text; albumId = albumRun.navigationEndpoint.browseEndpoint.browseId; break; }
  }

  // Duration from fixed columns
  const durationText = r.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || '';

  const thumbs = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
  return {
    id: videoId,
    title: titleRuns.map(r => r.text).join('') || 'Unknown',
    ...buildArtistFields(artists),
    album,
    albumId,
    thumbnail: getSquareThumbnail(thumbs),
    duration:  durationText,
    durationMs: (() => {
      if (!durationText) return 0;
      const p = durationText.split(':').map(Number);
      if (p.length === 2) return (p[0] * 60 + p[1]) * 1000;
      if (p.length === 3) return (p[0] * 3600 + p[1] * 60 + p[2]) * 1000;
      return 0;
    })(),
    url: `https://music.youtube.com/watch?v=${videoId}`,
  };
}

// ─── Search ───────────────────────────────────────────────────────────────

export async function search(query, musicOnly = true) {
  try {
    if (musicOnly) {
      // songs-only params
      const rawData = await musicRequest('search', {
        query,
        params: 'EgWKAQIIAWoOEAMQBBAJEAoQBRAREBU%3D',
      });
      const shelves = rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]
        ?.tabRenderer?.content?.sectionListRenderer?.contents || [];
      const items = [];
      for (const s of shelves) {
        for (const entry of (s?.musicShelfRenderer?.contents || [])) {
          const r = entry?.musicResponsiveListItemRenderer;
          if (r) { const t = mapSongFromShelf(r); if (t) items.push(t); }
        }
      }
      return items;
    } else {
      const rawData = await musicRequest('search', { query });
      const shelves = rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]
        ?.tabRenderer?.content?.sectionListRenderer?.contents || [];
      const items = [];
      for (const s of shelves) {
        for (const entry of (s?.musicShelfRenderer?.contents || [])) {
          const r = entry?.musicResponsiveListItemRenderer;
          if (r) { const t = mapSongFromShelf(r); if (t) items.push(t); }
        }
      }
      return items;
    }
  } catch (err) {
    console.error('[YTM] search error', err);
    return [];
  }
}

export async function searchSuggestions(query) {
  try {
    const rawData = await musicRequest('music/get_search_suggestions', { input: query });
    const sections = rawData?.contents ?? [];
    const textSuggestions = [];
    const directResults = [];

    for (const section of sections) {
      const items = section?.searchSuggestionsSectionRenderer?.contents ?? [];
      for (const item of items) {
        if (item.searchSuggestionRenderer) {
          const text = (item.searchSuggestionRenderer.suggestion?.runs ?? []).map(r => r.text).join('');
          if (text) textSuggestions.push(text);
          continue;
        }
        const renderer = item.musicResponsiveListItemRenderer;
        if (!renderer) continue;
        const navEndpoint = renderer.navigationEndpoint;
        const thumbs = renderer.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails;
        const thumbnail = thumbs?.length ? thumbs[thumbs.length - 1].url : '';
        const cols = renderer.flexColumns ?? [];
        const titleRuns    = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs ?? [];
        const subtitleRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs ?? [];
        const title    = titleRuns.map(r => r.text).join('');
        const subtitle = subtitleRuns.map(r => r.text).join('');

        if (navEndpoint?.browseEndpoint) {
          const pageType = navEndpoint.browseEndpoint
            ?.browseEndpointContextSupportedConfigs
            ?.browseEndpointContextMusicConfig?.pageType;
          if (pageType === 'MUSIC_PAGE_TYPE_ARTIST') {
            directResults.push({ type: 'artist', name: title, artistId: navEndpoint.browseEndpoint.browseId, thumbnail, subtitle });
            continue;
          }
          if (pageType === 'MUSIC_PAGE_TYPE_ALBUM') {
            directResults.push({ type: 'album', name: title, albumId: navEndpoint.browseEndpoint.browseId, thumbnail, subtitle });
            continue;
          }
        }
        if (navEndpoint?.watchEndpoint?.videoId) {
          const videoId = navEndpoint.watchEndpoint.videoId;
          const artists = parseArtistsFromRuns(subtitleRuns);
          directResults.push({
            type: 'song', id: videoId, title,
            ...buildArtistFields(artists), thumbnail,
            url: `https://music.youtube.com/watch?v=${videoId}`,
          });
        }
      }
    }
    return { textSuggestions, directResults };
  } catch (err) {
    console.error('[YTM] searchSuggestions error', err);
    return { textSuggestions: [], directResults: [] };
  }
}

export async function searchArtists(query) {
  try {
    const rawData = await musicRequest('search', {
      query,
      params: 'Eg-KAQwIABAAGAAgASgAMABqChAEEAMQCRAFEAo%3D',
    });
    const items = [];
    for (const s of (rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]
      ?.tabRenderer?.content?.sectionListRenderer?.contents || [])) {
      for (const entry of (s?.musicShelfRenderer?.contents || [])) {
        const r = entry?.musicResponsiveListItemRenderer;
        if (!r) continue;
        const cols = r.flexColumns || [];
        const runs = cols.flatMap(c => c?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
        const browseId = r.navigationEndpoint?.browseEndpoint?.browseId || '';
        const name     = runs[0]?.text || '';
        const thumbnails = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
        if (browseId && name) items.push({
          artistId:  browseId,
          name,
          thumbnail: getBestThumbnail(thumbnails),
        });
      }
    }
    return items;
  } catch (err) {
    console.error('[YTM] searchArtists error', err);
    return [];
  }
}

export async function searchAlbums(query) {
  try {
    const rawData = await musicRequest('search', {
      query,
      params: 'EgWKAQIYAWoOEAMQBBAJEAoQBRAREBU%3D',
    });
    const items = [];
    for (const s of (rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]
      ?.tabRenderer?.content?.sectionListRenderer?.contents || [])) {
      for (const entry of (s?.musicShelfRenderer?.contents || [])) {
        const r = entry?.musicResponsiveListItemRenderer;
        if (!r) continue;
        const cols = r.flexColumns || [];
        const titleRuns = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
        const subtitleRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
        const browseId = titleRuns[0]?.navigationEndpoint?.browseEndpoint?.browseId || '';
        if (!browseId) continue;
        const artists    = parseArtistsFromRuns(subtitleRuns);
        const artistData = buildArtistFields(artists);
        const thumbs = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
        items.push({
          albumId:  browseId,
          name:     titleRuns.map(r => r.text).join('') || 'Unknown',
          artist:   artistData.artist,
          artistId: artistData.artistId,
          thumbnail: getSquareThumbnail(thumbs),
        });
      }
    }
    return items;
  } catch (err) {
    console.error('[YTM] searchAlbums error', err);
    return [];
  }
}

export async function searchVideos(query) {
  try {
    const rawData = await musicRequest('search', {
      query,
      params: 'EgWKAQIQAWoOEAMQBBAJEAoQBRAREBU%3D',
    });
    const items = [];
    for (const s of (rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]
      ?.tabRenderer?.content?.sectionListRenderer?.contents || [])) {
      for (const entry of (s?.musicShelfRenderer?.contents || [])) {
        const r = entry?.musicResponsiveListItemRenderer;
        if (!r) continue;
        const cols = r.flexColumns || [];
        const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]
          ?.navigationEndpoint?.watchEndpoint?.videoId;
        if (!videoId) continue;
        const title = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text || 'Unknown';
        const allRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
        const artists = parseArtistsFromRuns(allRuns);
        const durationText = r.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || '';
        const thumbs = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
        items.push({
          id: videoId,
          title,
          ...buildArtistFields(artists),
          thumbnail: getBestThumbnail(thumbs),
          duration:  durationText,
          durationMs: 0,
          url: `https://music.youtube.com/watch?v=${videoId}`,
        });
      }
    }
    return items;
  } catch (err) {
    console.error('[YTM] searchVideos error', err);
    return [];
  }
}

export async function searchPlaylists(query) {
  try {
    const rawData = await musicRequest('search', {
      query,
      params: 'EgWKAQIoAWoOEAMQBBAJEAoQBRAREBU%3D',
    });
    const items = [];
    for (const s of (rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]
      ?.tabRenderer?.content?.sectionListRenderer?.contents || [])) {
      for (const entry of (s?.musicShelfRenderer?.contents || [])) {
        const r = entry?.musicResponsiveListItemRenderer;
        if (!r) continue;
        const cols = r.flexColumns || [];
        const titleRuns = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
        const browseId  = titleRuns[0]?.navigationEndpoint?.browseEndpoint?.browseId || '';
        if (!browseId) continue;
        const subtitleRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
        const thumbs = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
        items.push({
          playlistId: browseId,
          name:       titleRuns.map(r => r.text).join('') || 'Unknown',
          author:     subtitleRuns.map(r => r.text).join('').replace(/^.*\u2022\s*/, '').trim(),
          thumbnail:  getSquareThumbnail(thumbs, 300),
        });
      }
    }
    return items;
  } catch (err) {
    console.error('[YTM] searchPlaylists error', err);
    return [];
  }
}

// ─── Playlist videos ──────────────────────────────────────────────────────

export async function getPlaylistVideos(playlistId) {
  try {
    const rawData = await musicRequest('browse', { browseId: playlistId });
    const shelf   = rawData?.contents?.twoColumnBrowseResultsRenderer
      ?.secondaryContents?.sectionListRenderer?.contents?.[0]?.musicPlaylistShelfRenderer
      || rawData?.header?.musicImmersiveHeaderRenderer
      || null;

    const shelfContents = rawData?.contents?.twoColumnBrowseResultsRenderer
      ?.secondaryContents?.sectionListRenderer?.contents?.[0]
      ?.musicPlaylistShelfRenderer?.contents
      || rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]
        ?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]
        ?.musicShelfRenderer?.contents
      || [];

    return shelfContents.map(item => {
      const r = item?.musicResponsiveListItemRenderer;
      if (!r) return null;
      return mapSongFromShelf(r);
    }).filter(Boolean);
  } catch (err) {
    console.error('[YTM] getPlaylistVideos error', err);
    return [];
  }
}

// ─── Artist info ──────────────────────────────────────────────────────────

export async function artistInfo(artistId) {
  try {
    const rawData = await musicRequest('browse', { browseId: artistId });

    const header = rawData?.header?.musicImmersiveHeaderRenderer
      || rawData?.header?.musicVisualHeaderRenderer;
    const name = header?.title?.runs?.[0]?.text || 'Unknown';
    const monthlyListeners = header?.monthlyListenerCount?.runs?.[0]?.text || '';

    const bannerThumbs = header?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
    const bannerUrl    = getBestThumbnail(bannerThumbs);
    const banner = bannerUrl?.includes('lh3.googleusercontent.com')
      ? bannerUrl.replace(/=(?:w\d+-h\d+|s\d+|p-w\d+).*$/, '=w1440-h600-p-l90-rj')
      : bannerUrl;

    const thumbnails = header?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails
      || header?.foregroundThumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails
      || [];
    const avatar = getSquareThumbnail(thumbnails, 512);

    const sections = rawData?.contents?.singleColumnBrowseResultsRenderer
      ?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];

    const topSongs    = [];
    const topAlbums   = [];
    const topSingles  = [];
    const topVideos   = [];
    const fansAlsoLike   = [];
    const livePerformances = [];
    const featuredOn  = [];
    const rawTopSongsArtists = {};

    for (const section of sections) {
      // Top songs shelf
      const shelf = section?.musicShelfRenderer;
      if (shelf) {
        for (const item of (shelf.contents || [])) {
          const r = item?.musicResponsiveListItemRenderer;
          if (!r) continue;
          const cols = r.flexColumns || [];
          const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]
            ?.navigationEndpoint?.watchEndpoint?.videoId;
          if (!videoId) continue;
          const artists = parseArtistsFromRuns(cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
          if (artists.length) rawTopSongsArtists[videoId] = artists;
          const track = mapSongFromShelf(r);
          if (track) topSongs.push(track);
        }
      }

      const carousel = section?.musicCarouselShelfRenderer;
      if (!carousel) continue;
      const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || '').toLowerCase();

      if (title.includes('fans') && title.includes('like')) {
        for (const item of (carousel.contents || [])) {
          const r = item?.musicTwoRowItemRenderer;
          const browseId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          if (!r || !browseId.startsWith('UC')) continue;
          fansAlsoLike.push({
            artistId: browseId,
            name: r?.title?.runs?.[0]?.text || 'Unknown',
            thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 226),
          });
        }
      } else if (title.includes('album')) {
        for (const item of (carousel.contents || [])) {
          const r = item?.musicTwoRowItemRenderer;
          const albumId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          if (!r || !albumId) continue;
          const thumbs = r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          topAlbums.push({
            albumId,
            name:      r?.title?.runs?.[0]?.text || 'Unknown',
            year:      parseInt(r?.subtitle?.runs?.find(s => /\d{4}/.test(s.text))?.text) || null,
            type:      'Album',
            thumbnail: getSquareThumbnail(thumbs, 300),
          });
        }
      } else if (title.includes('single') || title.includes('ep')) {
        for (const item of (carousel.contents || [])) {
          const r = item?.musicTwoRowItemRenderer;
          const albumId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          if (!r || !albumId) continue;
          const thumbs = r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          topSingles.push({
            albumId,
            name:      r?.title?.runs?.[0]?.text || 'Unknown',
            year:      parseInt(r?.subtitle?.runs?.find(s => /\d{4}/.test(s.text))?.text) || null,
            type:      'Single',
            thumbnail: getSquareThumbnail(thumbs, 300),
          });
        }
      } else if (title.includes('video') || title.includes('live')) {
        for (const item of (carousel.contents || [])) {
          const r = item?.musicTwoRowItemRenderer;
          const videoId = r?.navigationEndpoint?.watchEndpoint?.videoId;
          if (!r || !videoId) continue;
          const subtitleRuns = r?.subtitle?.runs || [];
          const artists = parseArtistsFromRuns(subtitleRuns);
          const thumbs = r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          topVideos.push({
            videoId,
            name:      r?.title?.runs?.[0]?.text || 'Untitled',
            ...buildArtistFields(artists),
            thumbnail: getBestThumbnail(thumbs),
            duration:  '',
          });
        }
      } else if (title.includes('featured')) {
        for (const item of (carousel.contents || [])) {
          const r = item?.musicTwoRowItemRenderer;
          const playlistId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          if (!r || !playlistId) continue;
          const thumbs = r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          featuredOn.push({
            playlistId,
            name: r?.title?.runs?.[0]?.text || 'Unknown Playlist',
            thumbnail: getSquareThumbnail(thumbs, 300),
          });
        }
      }
    }

    // Apply rawTopSongsArtists to topSongs where available
    for (const song of topSongs) {
      if (rawTopSongsArtists[song.id]) {
        const af = buildArtistFields(rawTopSongsArtists[song.id]);
        Object.assign(song, af);
      }
    }

    return {
      name, artistId, monthlyListeners, banner, avatar,
      description: '', followers: 0, tags: [],
      topSongs: topSongs.slice(0, 10),
      topAlbums, topSingles, topVideos,
      fansAlsoLike, livePerformances, featuredOn,
    };
  } catch (err) {
    console.error('[YTM] artistInfo error', err);
    return null;
  }
}

// ─── Album tracks ─────────────────────────────────────────────────────────

export async function albumTracks(albumId) {
  try {
    const rawData = await musicRequest('browse', { browseId: albumId });

    const headerRuns = rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]
      ?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]
      ?.musicResponsiveHeaderRenderer?.straplineTextOne?.runs || [];
    const albumArtists = parseArtistsFromRuns(headerRuns);

    const titleRuns = rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]
      ?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]
      ?.musicResponsiveHeaderRenderer?.title?.runs || [];
    const albumName = titleRuns.map(r => r.text).join('') || 'Unknown Album';

    const thumbs = rawData?.header?.musicImmersiveHeaderRenderer?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails
      || rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]
          ?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]
          ?.musicResponsiveHeaderRenderer?.thumbnail?.croppedSquareThumbnailRenderer?.thumbnail?.thumbnails
      || [];

    const year = (() => {
      const subtitleRuns = rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]
        ?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]
        ?.musicResponsiveHeaderRenderer?.subtitle?.runs || [];
      const yearText = subtitleRuns.find(r => /^\d{4}$/.test(r.text?.trim()));
      return yearText ? parseInt(yearText.text) : null;
    })();

    const shelfContents = rawData?.contents?.twoColumnBrowseResultsRenderer
      ?.secondaryContents?.sectionListRenderer?.contents?.[0]
      ?.musicShelfRenderer?.contents || [];

    const rawArtistsMap = {};
    for (const item of shelfContents) {
      const r = item?.musicResponsiveListItemRenderer;
      if (!r) continue;
      const cols = r.flexColumns || [];
      const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]
        ?.navigationEndpoint?.watchEndpoint?.videoId;
      if (!videoId) continue;
      const artists = parseArtistsFromRuns(cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
      if (artists.length) rawArtistsMap[videoId] = artists;
    }

    const tracks = shelfContents.map(item => {
      const r = item?.musicResponsiveListItemRenderer;
      if (!r) return null;
      const track = mapSongFromShelf(r);
      if (!track) return null;
      const artists = rawArtistsMap[track.id] || (albumArtists.length ? albumArtists : null);
      if (artists) Object.assign(track, buildArtistFields(artists));
      if (!track.album) { track.album = albumName; track.albumId = albumId; }
      return track;
    }).filter(Boolean);

    const albumArtistFields = albumArtists.length ? buildArtistFields(albumArtists) : { artist: 'Unknown Artist', artistId: null };
    return {
      name:      albumName,
      artist:    albumArtistFields.artist,
      artistId:  albumArtistFields.artistId,
      year,
      thumbnail: getSquareThumbnail(thumbs, 300),
      tracks,
    };
  } catch (err) {
    console.error('[YTM] albumTracks error', err);
    return null;
  }
}

// ─── Up Next / Radio ──────────────────────────────────────────────────────

export async function getUpNexts(videoId) {
  try {
    const rawData = await musicRequest('next', {
      videoId,
      playlistId: `RDAMVM${videoId}`,
      isAudioOnly: true,
    });
    const contents = rawData?.contents?.singleColumnMusicWatchNextResultsRenderer
      ?.tabbedRenderer?.watchNextTabbedResultsRenderer?.tabs?.[0]?.tabRenderer
      ?.content?.musicQueueRenderer?.content?.playlistPanelRenderer?.contents || [];

    return contents.slice(1).map(item => {
      const r = item?.playlistPanelVideoRenderer;
      const vid = r?.navigationEndpoint?.watchEndpoint?.videoId;
      if (!r || !vid) return null;
      const allRuns  = r.longBylineText?.runs || [];
      const dotIdx   = allRuns.findIndex(run => run.text === ' \u2022 ');
      const artists  = parseArtistsFromRuns(dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns);
      const durationText = r.lengthText?.runs?.[0]?.text || '';
      const parts = durationText.split(':').map(Number);
      let durationMs = 0;
      if (parts.length === 2) durationMs = (parts[0] * 60 + parts[1]) * 1000;
      else if (parts.length === 3) durationMs = (parts[0] * 3600 + parts[1] * 60 + parts[2]) * 1000;
      return {
        id: vid,
        title: r.title?.runs?.[0]?.text || 'Unknown',
        ...buildArtistFields(artists),
        thumbnail: getSquareThumbnail(r.thumbnail?.thumbnails || []),
        duration: durationText,
        durationMs,
        url: `https://music.youtube.com/watch?v=${vid}`,
      };
    }).filter(Boolean);
  } catch (err) {
    console.error('[YTM] getUpNexts error', err);
    return [];
  }
}

// ─── Explore ─────────────────────────────────────────────────────────────

export async function explore() {
  try {
    const rawData = await musicRequest('browse', { browseId: 'FEmusic_explore' });
    const sections = rawData?.contents?.singleColumnBrowseResultsRenderer
      ?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
    const result = { newAlbums: [], moods: [], newMusicVideos: [] };

    for (const section of sections) {
      const carousel = section?.musicCarouselShelfRenderer;
      if (!carousel) continue;
      const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || '').toLowerCase();

      if (title.includes('new album') || title.includes('new release')) {
        result.newAlbums = (carousel.contents || []).map(item => {
          const r = item?.musicTwoRowItemRenderer;
          if (!r) return null;
          const albumId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          if (!albumId) return null;
          const subtitleRuns = r?.subtitle?.runs || [];
          const artists = parseArtistsFromRuns(subtitleRuns);
          const artistFields = artists.length ? buildArtistFields(artists) : { artist: subtitleRuns.map(s => s.text).join(''), artistId: null };
          return {
            albumId,
            name: r?.title?.runs?.[0]?.text || 'Unknown',
            ...artistFields,
            thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 300),
            year: null, type: 'Album',
          };
        }).filter(Boolean);
      } else if (title.includes('music video')) {
        result.newMusicVideos = (carousel.contents || []).map(item => {
          const r = item?.musicTwoRowItemRenderer;
          const videoId = r?.navigationEndpoint?.watchEndpoint?.videoId || '';
          if (!r || !videoId) return null;
          const subtitleRuns = r?.subtitle?.runs || [];
          const artists = parseArtistsFromRuns(subtitleRuns);
          const artistFields = artists.length ? buildArtistFields(artists) : { artist: subtitleRuns.map(s => s.text).join(''), artistId: null };
          return {
            id: videoId,
            title: r?.title?.runs?.[0]?.text || 'Unknown',
            ...artistFields,
            thumbnail: getBestThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || []),
            duration: '', durationMs: 0,
            url: `https://music.youtube.com/watch?v=${videoId}`,
          };
        }).filter(Boolean);
      } else if (title.includes('mood') || title.includes('genre')) {
        result.moods = (carousel.contents || []).map(item => {
          const r = item?.musicNavigationButtonRenderer || item?.musicTwoRowItemRenderer;
          if (!r) return null;
          const browseId = r?.clickCommand?.browseEndpoint?.browseId || r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          const params   = r?.clickCommand?.browseEndpoint?.params   || r?.navigationEndpoint?.browseEndpoint?.params   || '';
          const label    = r?.buttonText?.runs?.[0]?.text || r?.title?.runs?.[0]?.text || '';
          const color    = r?.solid?.leftStripeColor;
          if (!browseId || !label) return null;
          return {
            browseId, params, label,
            color: color ? `#${(color >>> 0).toString(16).padStart(8, '0').slice(2)}` : null,
          };
        }).filter(Boolean);
      }
    }
    return result;
  } catch (err) {
    console.error('[YTM] explore error', err);
    return null;
  }
}

// ─── Charts ───────────────────────────────────────────────────────────────

export async function charts() {
  try {
    const rawData = await musicRequest('browse', { browseId: 'FEmusic_charts' });
    let sections = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]
      ?.tabRenderer?.content?.sectionListRenderer?.contents
      || rawData?.contents?.sectionListRenderer?.contents
      || [];

    const result = { topSongs: [], topVideos: [], topArtists: [] };
    let trendingPlaylistId = null;

    for (const section of sections) {
      const carousel = section?.musicCarouselShelfRenderer;
      if (!carousel) continue;
      const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || '').toLowerCase();

      if (title.includes('trending') || title.includes('video chart')) {
        for (const item of (carousel.contents || [])) {
          const r = item?.musicTwoRowItemRenderer;
          if (!r) continue;
          const itemTitle = (r?.title?.runs || []).map(run => run.text).join('').toLowerCase();
          const browseId  = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          if (itemTitle.includes('trending') && browseId) { trendingPlaylistId = browseId; break; }
        }
      } else if (title.includes('top artist') || title.includes('trending artist')) {
        result.topArtists = (carousel.contents || []).slice(0, 20).map(item => {
          const r = item?.musicResponsiveListItemRenderer || item?.musicTwoRowItemRenderer;
          if (!r) return null;
          const artistId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          if (!artistId || !artistId.startsWith('UC')) return null;
          const name = r?.title?.runs?.[0]?.text
            || r?.flexColumns?.[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text
            || 'Unknown';
          return {
            artistId, name,
            thumbnail: getSquareThumbnail(
              r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails
              || r?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 226),
          };
        }).filter(Boolean);
      }
    }

    if (trendingPlaylistId) {
      try {
        const plRaw = await musicRequest('browse', { browseId: trendingPlaylistId });
        const shelfContents = plRaw?.contents?.twoColumnBrowseResultsRenderer
          ?.secondaryContents?.sectionListRenderer?.contents?.[0]
          ?.musicPlaylistShelfRenderer?.contents || [];
        result.topSongs = shelfContents.map(item => {
          const r = item?.musicResponsiveListItemRenderer;
          if (!r) return null;
          const cols = r.flexColumns || [];
          const videoId = r?.overlay?.musicItemThumbnailOverlayRenderer
            ?.content?.musicPlayButtonRenderer?.playNavigationEndpoint?.watchEndpoint?.videoId
            || cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]
              ?.navigationEndpoint?.watchEndpoint?.videoId;
          if (!videoId) return null;
          const trackName  = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text || 'Unknown';
          const artistRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
          const artists    = parseArtistsFromRuns(artistRuns);
          const rank       = r?.customIndexColumn?.musicCustomIndexColumnRenderer?.text?.runs?.[0]?.text || '';
          const durText    = r?.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || '';
          return {
            id:    videoId,
            title: trackName,
            ...buildArtistFields(artists),
            thumbnail: getSquareThumbnail(r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || []),
            rank: parseInt(rank, 10) || 0,
            duration: durText,
            url: `https://music.youtube.com/watch?v=${videoId}`,
          };
        }).filter(Boolean);
      } catch (_) {}
    }
    return result;
  } catch (err) {
    console.error('[YTM] charts error', err);
    return null;
  }
}

// ─── Browse mood ──────────────────────────────────────────────────────────

export async function browseMood(browseId, params) {
  try {
    const rawData = await musicRequest('browse', { browseId, params });
    const grid = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]
      ?.tabRenderer?.content?.sectionListRenderer?.contents || [];
    const playlists = [];
    for (const section of grid) {
      for (const item of (section?.gridRenderer?.items || section?.musicCarouselShelfRenderer?.contents || [])) {
        const r = item?.musicTwoRowItemRenderer;
        const playlistId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
        if (!r || !playlistId) continue;
        playlists.push({
          playlistId,
          name:      r?.title?.runs?.[0]?.text || 'Unknown',
          subtitle:  (r?.subtitle?.runs || []).map(s => s.text).join(''),
          thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 300),
        });
      }
    }
    return playlists;
  } catch (err) {
    console.error('[YTM] browseMood error', err);
    return [];
  }
}

// ─── Track info (for deep links) ─────────────────────────────────────────

export async function getTrackInfo(videoId) {
  try {
    const rawData = await musicRequest('next', { videoId, isAudioOnly: true });
    const contents = rawData?.contents?.singleColumnMusicWatchNextResultsRenderer
      ?.tabbedRenderer?.watchNextTabbedResultsRenderer?.tabs?.[0]?.tabRenderer
      ?.content?.musicQueueRenderer?.content?.playlistPanelRenderer?.contents || [];
    const r = contents[0]?.playlistPanelVideoRenderer;
    if (!r) {
      // fallback: plain song lookup
      const songData = await musicRequest('next', { videoId });
      const r2 = songData?.contents?.singleColumnMusicWatchNextResultsRenderer
        ?.tabbedRenderer?.watchNextTabbedResultsRenderer?.tabs?.[0]?.tabRenderer
        ?.content?.musicQueueRenderer?.content?.playlistPanelRenderer?.contents?.[0]
        ?.playlistPanelVideoRenderer;
      if (!r2) return null;
      return parseWatchRenderer(r2, videoId);
    }
    return parseWatchRenderer(r, videoId);
  } catch (err) {
    console.error('[YTM] getTrackInfo error', err);
    return null;
  }
}

function parseWatchRenderer(r, videoId) {
  const allRuns = r.longBylineText?.runs || [];
  const dotIdx  = allRuns.findIndex(run => run.text === ' \u2022 ');
  const artists = parseArtistsFromRuns(dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns);
  const durationText = r.lengthText?.runs?.[0]?.text || '';
  return {
    id: videoId,
    title: r.title?.runs?.[0]?.text || 'Unknown',
    ...buildArtistFields(artists),
    thumbnail: getSquareThumbnail(r.thumbnail?.thumbnails || []),
    duration: durationText,
    url: `https://music.youtube.com/watch?v=${videoId}`,
  };
}

// ─── Stream URL extraction ────────────────────────────────────────────────
// ANDROID client (v20) returns plain url fields — no cipher, no po_token needed.
// Falls back to Piped API if the primary fails.

async function fetchPlayerData(videoId) {
  const resp = await fetch(
    'https://www.youtube.com/youtubei/v1/player?prettyPrint=false',
    {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-YouTube-Client-Name': '3',
        'X-YouTube-Client-Version': ANDROID_CONTEXT.client.clientVersion,
        'User-Agent': ANDROID_CONTEXT.client.userAgent,
      },
      body: JSON.stringify({
        context: ANDROID_CONTEXT,
        videoId,
        contentCheckOk: true,
        racyCheckOk: true,
      }),
    }
  );
  return resp.json();
}

export async function getStreamUrl(videoUrl, quality = 'bestaudio') {
  await initSession();

  const videoId = videoUrl?.includes('watch?v=')
    ? new URL(videoUrl).searchParams.get('v')
    : videoUrl;
  if (!videoId) throw new Error('Invalid video URL');

  const cpn = generateCpn();
  const data = await fetchPlayerData(videoId);
  const status = data?.playabilityStatus?.status;

  if (status === 'OK') {
    const af = data?.streamingData?.adaptiveFormats ?? [];
    let audioFormats = af.filter(f => f.mimeType?.startsWith('audio/') && f.url);

    // Piped fallback if ANDROID returned no direct audio URLs
    if (!audioFormats.length) {
      console.log('[YTM] No direct audio URLs, trying Piped API…');
      try {
        const piped = await fetch(`https://pipedapi.kavin.rocks/streams/${videoId}`).then(r => r.json());
        audioFormats = (piped?.audioStreams ?? [])
          .filter(s => s.url)
          .map(s => ({ mimeType: s.mimeType ?? 'audio/webm', bitrate: s.bitrate ?? 0, url: s.url }));
      } catch (e) {
        console.error('[YTM] Piped API failed:', e);
      }
    }

    if (audioFormats.length) {
      const sorted = quality === 'worstaudio'
        ? [...audioFormats].sort((a, b) => (a.bitrate || 0) - (b.bitrate || 0))
        : [...audioFormats].sort((a, b) => (b.bitrate || 0) - (a.bitrate || 0));
      return `${sorted[0].url}&cpn=${cpn}`;
    }

    // Muxed fallback
    const muxed = (data?.streamingData?.formats ?? []).filter(f => f.url);
    if (muxed.length) return `${muxed[0].url}&cpn=${cpn}`;
  }

  console.error('[YTM] Player response:', JSON.stringify(data?.playabilityStatus));
  throw new Error(`No stream URLs found (status: ${data?.playabilityStatus?.status})`);
}

// ─── Video stream URL ────────────────────────────────────────────────────

export async function getVideoStreamUrl(videoId, quality = '720', premuxed = false) {
  await initSession();

  let data = await fetchPlayerData(videoId);
  const height = parseInt(quality) || 720;

  const hasVideo = (data?.streamingData?.adaptiveFormats ?? []).some(f => f.url && f.mimeType?.includes('video/'));

  if (premuxed) {
    const muxed = (data?.streamingData?.formats || [])
      .filter(f => f.url && (f.height || 0) <= height)
      .sort((a, b) => (b.height || 0) - (a.height || 0));
    if (muxed.length) return { videoUrl: muxed[0].url, audioUrl: null };
  }

  const adaptive = data?.streamingData?.adaptiveFormats || [];
  const videoFmts = adaptive
    .filter(f => f.url && f.mimeType?.includes('video/mp4') && (f.height || 0) <= height)
    .sort((a, b) => (b.height || 0) - (a.height || 0));
  const audioFmts = adaptive
    .filter(f => f.url && f.mimeType?.startsWith('audio/'))
    .sort((a, b) => (b.bitrate || 0) - (a.bitrate || 0));

  if (!videoFmts.length || !audioFmts.length) {
    const muxed = (data?.streamingData?.formats || []).filter(f => f.url);
    if (muxed.length) return { videoUrl: muxed[0].url, audioUrl: null };
    throw new Error('No suitable video/audio formats');
  }

  return {
    videoUrl: videoFmts[0].url,
    audioUrl: audioFmts[0].url,
  };
}
