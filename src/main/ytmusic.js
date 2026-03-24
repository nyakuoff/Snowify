const { session } = require('electron');
const { getYtDlpPath } = require('./ytdlp');
const { getCachedUrl, setCachedUrl } = require('./stream-cache');
const { AUDIO_CACHE_DIR, ensureCacheDir, getActiveDownloadProc, setActiveDownloadProc } = require('./audio-cache');
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');

// ─── YTMusic helpers ───

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
      id: r.navigationEndpoint.browseEndpoint.browseId
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
    artist: artists.map(a => a.name).join(', '),
    artistId: artists[0].id || null,
    artists
  };
}

function mapSongToTrack(song, artists) {
  const artistFields = artists
    ? buildArtistFields(artists)
    : {
        artist: song.artist?.name || 'Unknown Artist',
        artistId: song.artist?.artistId || null,
        artists: song.artist ? [{ name: song.artist.name, id: song.artist.artistId || null }] : []
      };
  return {
    id: song.videoId,
    title: song.name || 'Unknown',
    ...artistFields,
    album: song.album?.name || null,
    albumId: song.album?.albumId || null,
    thumbnail: getSquareThumbnail(song.thumbnails),
    duration: formatDuration(song.duration),
    durationMs: song.duration ? Math.round(song.duration * 1000) : 0,
    url: `https://music.youtube.com/watch?v=${song.videoId}`
  };
}

// ─── YTMusic init ───

async function initYTMusic(ctx) {
  const YTMusic = (await import('ytmusic-api')).default;
  if (!ctx.ytmusic) ctx.ytmusic = new YTMusic();

  try {
    const proxyStr = await session.defaultSession.resolveProxy('https://music.youtube.com/');
    if (proxyStr && proxyStr !== 'DIRECT') {
      const firstProxy = proxyStr.split(';')[0].trim();
      const [type, hostPort] = firstProxy.split(/\s+/, 2);
      if (hostPort) {
        let agent;
        if (type === 'SOCKS5' || type === 'SOCKS4') {
          const { SocksProxyAgent } = require('socks-proxy-agent');
          agent = new SocksProxyAgent(`${type.toLowerCase()}://${hostPort}`);
        } else {
          const { HttpsProxyAgent } = require('https-proxy-agent');
          agent = new HttpsProxyAgent(`http://${hostPort}`);
        }
        ctx.ytmusic.client.defaults.httpsAgent = agent;
        ctx.ytmusic.client.defaults.httpAgent = agent;
        console.log(`[YTMusic] Using system proxy: ${firstProxy}`);
      }
    }
  } catch (proxyErr) {
    console.warn('[YTMusic] Could not resolve system proxy:', proxyErr.message);
  }

  await ctx.ytmusic.initialize();
  ctx.ytmusicReady = true;
}

async function ensureYTMusic(ctx) {
  if (ctx.ytmusicReady && ctx.ytmusic?.config?.INNERTUBE_API_KEY) return;
  ctx.ytmusicReady = false;
  await initYTMusic(ctx);
}

// ─── IPC handlers ───

function register(ipcMain, ctx) {
  ipcMain.handle('yt:search', async (_event, query, musicOnly) => {
    try {
      await ensureYTMusic(ctx);
      if (musicOnly) {
        const rawParams = 'EgWKAQIIAWoOEAMQBBAJEAoQBRAREBU%3D';
        const [songs, rawData] = await Promise.all([
          ctx.ytmusic.searchSongs(query),
          ctx.ytmusic.constructRequest('search', { query, params: rawParams }).catch(() => null)
        ]);

        const rawArtistsMap = {};
        if (rawData) {
          const shelves = rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]
            ?.tabRenderer?.content?.sectionListRenderer?.contents || [];
          for (const s of shelves) {
            for (const entry of (s?.musicShelfRenderer?.contents || [])) {
              const r = entry?.musicResponsiveListItemRenderer;
              if (!r) continue;
              const cols = r.flexColumns || [];
              const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]
                ?.navigationEndpoint?.watchEndpoint?.videoId;
              if (!videoId) continue;
              const allRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
              const dotIdx = allRuns.findIndex(run => run.text === ' \u2022 ');
              const artists = parseArtistsFromRuns(dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns);
              if (artists.length) rawArtistsMap[videoId] = artists;
            }
          }
        }
        return songs.filter(s => s.videoId).map(song => mapSongToTrack(song, rawArtistsMap[song.videoId] || null));
      } else {
        const [results, rawData] = await Promise.all([
          ctx.ytmusic.search(query),
          ctx.ytmusic.constructRequest('search', { query }).catch(() => null)
        ]);

        const rawArtistsMap = {};
        if (rawData) {
          const shelves = rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]
            ?.tabRenderer?.content?.sectionListRenderer?.contents || [];
          for (const s of shelves) {
            for (const entry of (s?.musicShelfRenderer?.contents || [])) {
              const r = entry?.musicResponsiveListItemRenderer;
              if (!r) continue;
              const cols = r.flexColumns || [];
              const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]
                ?.navigationEndpoint?.watchEndpoint?.videoId;
              if (!videoId) continue;
              const allRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
              const dotIdx = allRuns.findIndex(run => run.text === ' \u2022 ');
              const artists = parseArtistsFromRuns(dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns);
              if (artists.length) rawArtistsMap[videoId] = artists;
            }
          }
        }

        return results
          .filter(r => (r.type === 'SONG' || r.type === 'VIDEO') && r.videoId)
          .map(r => {
            const artists = rawArtistsMap[r.videoId] || null;
            const artistFields = artists
              ? buildArtistFields(artists)
              : {
                  artist: r.artist?.name || 'Unknown Artist',
                  artistId: r.artist?.artistId || null,
                  artists: r.artist ? [{ name: r.artist.name, id: r.artist.artistId || null }] : []
                };
            return {
              id: r.videoId,
              title: r.name || 'Unknown',
              ...artistFields,
              thumbnail: getSquareThumbnail(r.thumbnails),
              duration: formatDuration(r.duration),
              durationMs: r.duration ? Math.round(r.duration * 1000) : 0,
              url: `https://music.youtube.com/watch?v=${r.videoId}`
            };
          });
      }
    } catch (err) {
      console.error('Search error:', err);
      return [];
    }
  });

  ipcMain.handle('yt:searchSuggestions', async (_event, query) => {
    try {
      await ensureYTMusic(ctx);
      const rawData = await ctx.ytmusic.constructRequest('music/get_search_suggestions', { input: query });
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
          const titleRuns = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs ?? [];
          const subtitleRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs ?? [];
          const title = titleRuns.map(r => r.text).join('');
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
              url: `https://music.youtube.com/watch?v=${videoId}`
            });
          }
        }
      }
      return { textSuggestions, directResults };
    } catch (err) {
      console.error('Search suggestions error:', err);
      return { textSuggestions: [], directResults: [] };
    }
  });

  ipcMain.handle('yt:artistInfo', async (_event, artistId) => {
    try {
      await ensureYTMusic(ctx);
      const artist = await ctx.ytmusic.getArtist(artistId);
      let monthlyListeners = '', banner = '', fansAlsoLike = [], livePerformances = [], featuredOn = [];
      let rawTopSongsArtists = {}, rawTopSongsPlays = {};

      try {
        const rawData = await ctx.ytmusic.constructRequest('browse', { browseId: artistId });
        const header = rawData?.header?.musicImmersiveHeaderRenderer || rawData?.header?.musicVisualHeaderRenderer;
        monthlyListeners = header?.monthlyListenerCount?.runs?.[0]?.text || '';
        const bannerThumbs = header?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
        const bannerUrl = getBestThumbnail(bannerThumbs);
        banner = bannerUrl?.includes('lh3.googleusercontent.com')
          ? bannerUrl.replace(/=(?:w\d+-h\d+|s\d+|p-w\d+).*$/, '=w1440-h600-p-l90-rj')
          : bannerUrl;

        const sections = rawData?.contents?.singleColumnBrowseResultsRenderer
          ?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];

        for (const section of sections) {
          const carousel = section?.musicCarouselShelfRenderer;
          if (!carousel) continue;
          const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || '').toLowerCase();

          if (title.includes('fans might also like')) {
            fansAlsoLike = (carousel.contents || []).map(item => {
              const r = item?.musicTwoRowItemRenderer;
              const browseId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
              if (!r || !browseId.startsWith('UC')) return null;
              return { artistId: browseId, name: r?.title?.runs?.[0]?.text || 'Unknown', thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 226) };
            }).filter(Boolean);
          } else if (title.includes('live performance')) {
            livePerformances = (carousel.contents || []).map(item => {
              const r = item?.musicTwoRowItemRenderer;
              const videoId = r?.navigationEndpoint?.watchEndpoint?.videoId || '';
              if (!r || !videoId) return null;
              return { videoId, name: r?.title?.runs?.[0]?.text || 'Untitled', artist: artist.name || 'Unknown Artist', artistId, thumbnail: getBestThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || []), duration: '' };
            }).filter(Boolean);
          } else if (title.includes('featured on')) {
            featuredOn = (carousel.contents || []).map(item => {
              const r = item?.musicTwoRowItemRenderer;
              const playlistId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
              if (!r || !playlistId) return null;
              return { playlistId, name: r?.title?.runs?.[0]?.text || 'Unknown Playlist', thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 300) };
            }).filter(Boolean);
          }
        }

        for (const section of sections) {
          const shelf = section?.musicShelfRenderer;
          if (!shelf) continue;
          for (const item of (shelf.contents || [])) {
            const r = item?.musicResponsiveListItemRenderer;
            if (!r) continue;
            const cols = r.flexColumns || [];
            const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.navigationEndpoint?.watchEndpoint?.videoId;
            if (!videoId) continue;
            const artists = parseArtistsFromRuns(cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
            if (artists.length) rawTopSongsArtists[videoId] = artists;
            const playsText = cols[2]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text || '';
            if (playsText) rawTopSongsPlays[videoId] = playsText;
          }
          break;
        }
      } catch (_) {}

      return {
        name: artist.name || 'Unknown',
        artistId: artist.artistId || '',
        description: '', followers: 0, monthlyListeners, banner, tags: [],
        avatar: getSquareThumbnail(artist.thumbnails, 512),
        topSongs: (artist.topSongs || []).filter(s => s.videoId).map(song => {
          const track = mapSongToTrack(song, rawTopSongsArtists[song.videoId] || null);
          if (rawTopSongsPlays[song.videoId]) track.plays = rawTopSongsPlays[song.videoId];
          return track;
        }),
        topAlbums: (artist.topAlbums || []).map(a => ({ albumId: a.albumId, playlistId: a.playlistId, name: a.name, year: a.year, type: 'Album', thumbnail: getSquareThumbnail(a.thumbnails, 300) })),
        topSingles: (artist.topSingles || []).map(a => ({ albumId: a.albumId, playlistId: a.playlistId, name: a.name, year: a.year, type: 'Single', thumbnail: getSquareThumbnail(a.thumbnails, 300) })),
        topVideos: (artist.topVideos || []).map(v => ({ videoId: v.videoId, name: v.name || 'Untitled Video', artist: v.artist?.name || 'Unknown Artist', artistId: v.artist?.artistId || null, thumbnail: getBestThumbnail(v.thumbnails), duration: formatDuration(v.duration) })),
        fansAlsoLike, livePerformances, featuredOn
      };
    } catch (err) {
      console.error('Artist info error:', err);
      return null;
    }
  });

  ipcMain.handle('yt:searchPlaylists', async (_event, query) => {
    try {
      await ensureYTMusic(ctx);
      return (await ctx.ytmusic.searchPlaylists(query) || []).map(p => ({
        playlistId: p.playlistId, name: p.name, artist: p.artist?.name || '',
        thumbnail: getSquareThumbnail(p.thumbnails, 300)
      }));
    } catch (err) { console.error('Search playlists error:', err); return []; }
  });

  ipcMain.handle('yt:getPlaylistVideos', async (_event, playlistId) => {
    try {
      await ensureYTMusic(ctx);
      return (await ctx.ytmusic.getPlaylistVideos(playlistId) || []).filter(v => v.videoId).map(v => ({
        id: v.videoId, title: v.name || 'Unknown',
        artist: v.artist?.name || 'Unknown Artist', artistId: v.artist?.artistId || null,
        artists: v.artist ? [{ name: v.artist.name, id: v.artist.artistId || null }] : [],
        album: v.album?.name || null, albumId: v.album?.albumId || null,
        thumbnail: getSquareThumbnail(v.thumbnails), duration: formatDuration(v.duration),
        durationMs: v.duration ? Math.round(v.duration * 1000) : 0,
        url: `https://music.youtube.com/watch?v=${v.videoId}`
      }));
    } catch (err) { console.error('Get playlist videos error:', err); return []; }
  });

  ipcMain.handle('yt:albumTracks', async (_event, albumId) => {
    try {
      await ensureYTMusic(ctx);
      const [album, rawData] = await Promise.all([
        ctx.ytmusic.getAlbum(albumId),
        ctx.ytmusic.constructRequest('browse', { browseId: albumId }).catch(() => null)
      ]);
      const rawArtistsMap = {};
      let albumArtists = [];
      if (rawData) {
        const headerRuns = rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]?.musicResponsiveHeaderRenderer?.straplineTextOne?.runs || [];
        albumArtists = parseArtistsFromRuns(headerRuns);
        for (const item of (rawData?.contents?.twoColumnBrowseResultsRenderer?.secondaryContents?.sectionListRenderer?.contents?.[0]?.musicShelfRenderer?.contents || [])) {
          const r = item?.musicResponsiveListItemRenderer;
          if (!r) continue;
          const cols = r.flexColumns || [];
          const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.navigationEndpoint?.watchEndpoint?.videoId;
          if (!videoId) continue;
          const artists = parseArtistsFromRuns(cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
          if (artists.length) rawArtistsMap[videoId] = artists;
        }
      }
      const tracks = (album.songs || []).filter(s => s.videoId).map(song =>
        mapSongToTrack(song, rawArtistsMap[song.videoId] || (albumArtists.length ? albumArtists : null))
      );
      const albumArtistFields = albumArtists.length
        ? buildArtistFields(albumArtists)
        : buildArtistFields(album.artist?.id ? [{ name: album.artist.name, id: album.artist.id }] : []);
      return { name: album.name || 'Unknown Album', ...albumArtistFields, year: album.year || null, thumbnail: getSquareThumbnail(album.thumbnails, 300), tracks };
    } catch (err) { console.error('Album tracks error:', err); return null; }
  });

  ipcMain.handle('yt:searchArtists', async (_event, query) => {
    try {
      await ensureYTMusic(ctx);
      const rawData = await ctx.ytmusic.constructRequest('search', { query, params: 'Eg-KAQwIABAAGAAgASgAMABqChAEEAMQCRAFEAo%3D' });
      const items = [];
      for (const s of (rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [])) {
        for (const entry of (s?.musicShelfRenderer?.contents || [])) {
          const r = entry?.musicResponsiveListItemRenderer;
          if (!r) continue;
          const cols = r.flexColumns || [];
          const runs = cols.flatMap(c => c?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
          const browseId = r.navigationEndpoint?.browseEndpoint?.browseId || '';
          const name = runs[0]?.text || '';
          const subtitle = runs.slice(1).map(r => r.text).join('').replace(/^\s*•\s*/, '').trim();
          const thumbnails = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          if (browseId && name) items.push({ artistId: browseId, name, thumbnail: getBestThumbnail(thumbnails), subtitle: subtitle.replace(/^Artist\s*•?\s*/i, '').trim() });
        }
      }
      return items;
    } catch (err) { console.error('Search artists error:', err); return []; }
  });

  ipcMain.handle('yt:searchAlbums', async (_event, query) => {
    try {
      await ensureYTMusic(ctx);
      return (await ctx.ytmusic.searchAlbums(query)).map(a => ({
        albumId: a.albumId, name: a.name, artist: a.artist?.name || 'Unknown Artist',
        artistId: a.artist?.artistId || null, year: a.year, thumbnail: getSquareThumbnail(a.thumbnails)
      }));
    } catch (err) { console.error('Search albums error:', err); return []; }
  });

  ipcMain.handle('yt:searchVideos', async (_event, query) => {
    try {
      await ensureYTMusic(ctx);
      return (await ctx.ytmusic.searchVideos(query)).map(v => ({
        id: v.videoId, title: v.name || 'Unknown',
        artist: v.artist?.name || 'Unknown Artist', artistId: v.artist?.artistId || null,
        artists: v.artist ? [{ name: v.artist.name, id: v.artist.artistId || null }] : [],
        thumbnail: getBestThumbnail(v.thumbnails), duration: formatDuration(v.duration),
        durationMs: v.duration ? Math.round(v.duration * 1000) : 0,
        url: `https://music.youtube.com/watch?v=${v.videoId}`
      }));
    } catch (err) { console.error('Search videos error:', err); return []; }
  });

  ipcMain.handle('yt:setCountry', async (_event, countryCode) => {
    try {
      await ensureYTMusic(ctx);
      if (!ctx.ytmusic?.config) return false;
      const code = countryCode || '';
      if (code) {
        ctx.ytmusic.config.GL = code;
        ctx.ytmusic.config.INNERTUBE_CONTEXT_GL = code;
        if (ctx.ytmusic.config.INNERTUBE_CONTEXT?.client) ctx.ytmusic.config.INNERTUBE_CONTEXT.client.gl = code;
      }
      ctx.ytmusic.config.HL = 'en';
      ctx.ytmusic.config.INNERTUBE_CONTEXT_HL = 'en';
      if (ctx.ytmusic.config.INNERTUBE_CONTEXT?.client) ctx.ytmusic.config.INNERTUBE_CONTEXT.client.hl = 'en';
      ctx.currentCountry = code;
      return true;
    } catch (err) { console.error('Set country error:', err); return false; }
  });

  ipcMain.handle('yt:explore', async () => {
    try {
      await ensureYTMusic(ctx);
      const rawData = await ctx.ytmusic.constructRequest('browse', { browseId: 'FEmusic_explore' });
      const sections = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
      const result = { newAlbums: [], moods: [], newMusicVideos: [] };

      for (const section of sections) {
        const carousel = section?.musicCarouselShelfRenderer;
        if (!carousel) continue;
        const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || '').toLowerCase();

        if (title.includes('new albums') || title.includes('new release')) {
          result.newAlbums = (carousel.contents || []).map(item => {
            const r = item?.musicTwoRowItemRenderer;
            if (!r) return null;
            const albumId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
            if (!albumId) return null;
            const subtitleRuns = r?.subtitle?.runs || [];
            const artists = parseArtistsFromRuns(subtitleRuns);
            const artistFields = artists.length ? buildArtistFields(artists) : { artist: subtitleRuns.map(s => s.text).join(''), artistId: null };
            return { albumId, name: r?.title?.runs?.[0]?.text || 'Unknown', ...artistFields, thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 300), year: null, type: 'Album' };
          }).filter(Boolean);
        } else if (title.includes('music video')) {
          result.newMusicVideos = (carousel.contents || []).map(item => {
            const r = item?.musicTwoRowItemRenderer;
            if (!r) return null;
            const videoId = r?.navigationEndpoint?.watchEndpoint?.videoId || '';
            if (!videoId) return null;
            const subtitleRuns = r?.subtitle?.runs || [];
            const artists = parseArtistsFromRuns(subtitleRuns);
            const artistFields = artists.length ? buildArtistFields(artists) : { artist: subtitleRuns.map(s => s.text).join(''), artistId: null };
            return { id: videoId, title: r?.title?.runs?.[0]?.text || 'Unknown', ...artistFields, thumbnail: getBestThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || []), duration: '', durationMs: 0, url: `https://music.youtube.com/watch?v=${videoId}` };
          }).filter(Boolean);
        } else if (title.includes('mood') || title.includes('genre')) {
          result.moods = (carousel.contents || []).map(item => {
            const r = item?.musicNavigationButtonRenderer || item?.musicTwoRowItemRenderer;
            if (!r) return null;
            const browseId = r?.clickCommand?.browseEndpoint?.browseId || r?.navigationEndpoint?.browseEndpoint?.browseId || '';
            const params = r?.clickCommand?.browseEndpoint?.params || r?.navigationEndpoint?.browseEndpoint?.params || '';
            const label = r?.buttonText?.runs?.[0]?.text || r?.title?.runs?.[0]?.text || '';
            const color = r?.solid?.leftStripeColor;
            if (!browseId || !label) return null;
            return { browseId, params, label, color: color ? `#${(color >>> 0).toString(16).padStart(8, '0').slice(0, 6)}` : null };
          }).filter(Boolean);
        }
      }
      return result;
    } catch (err) { console.error('Explore error:', err); return null; }
  });

  ipcMain.handle('yt:charts', async () => {
    try {
      await ensureYTMusic(ctx);
      const rawData = await ctx.ytmusic.constructRequest('browse', { browseId: 'FEmusic_charts' });
      let sections = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
      if (!sections.length) sections = rawData?.contents?.sectionListRenderer?.contents || [];

      const result = { topSongs: [], topVideos: [], topArtists: [] };
      let trendingPlaylistId = null;

      for (const section of sections) {
        const carousel = section?.musicCarouselShelfRenderer;
        if (!carousel) continue;
        const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || '').toLowerCase();

        if (title.includes('video chart') || title.includes('trending')) {
          for (const item of (carousel.contents || [])) {
            const r = item?.musicTwoRowItemRenderer;
            if (!r) continue;
            const itemTitle = (r?.title?.runs || []).map(run => run.text).join('').toLowerCase();
            const browseId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
            if (itemTitle.includes('trending') && browseId) { trendingPlaylistId = browseId; break; }
          }
        } else if (title.includes('top artist') || title.includes('trending artist')) {
          result.topArtists = (carousel.contents || []).slice(0, 20).map(item => {
            const r = item?.musicResponsiveListItemRenderer || item?.musicTwoRowItemRenderer;
            if (!r) return null;
            const artistId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
            if (!artistId || !artistId.startsWith('UC')) return null;
            const name = r?.title?.runs?.[0]?.text || r?.flexColumns?.[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text || 'Unknown';
            return { artistId, name, thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || r?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 226) };
          }).filter(Boolean);
        }
      }

      if (trendingPlaylistId) {
        try {
          const plRaw = await ctx.ytmusic.constructRequest('browse', { browseId: trendingPlaylistId });
          const plShelf = plRaw?.contents?.twoColumnBrowseResultsRenderer?.secondaryContents?.sectionListRenderer?.contents?.[0]?.musicPlaylistShelfRenderer;
          result.topSongs = (plShelf?.contents || []).map(item => {
            const r = item?.musicResponsiveListItemRenderer;
            if (!r) return null;
            const cols = r.flexColumns || [];
            const videoId = r?.overlay?.musicItemThumbnailOverlayRenderer?.content?.musicPlayButtonRenderer?.playNavigationEndpoint?.watchEndpoint?.videoId
              || cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.navigationEndpoint?.watchEndpoint?.videoId;
            if (!videoId) return null;
            const trackName = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text || 'Unknown';
            const artistRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
            const artists = parseArtistsFromRuns(artistRuns);
            const artistFields = artists.length ? buildArtistFields(artists) : { artist: artistRuns.map(s => s.text).join(''), artistId: null };
            const rank = r?.customIndexColumn?.musicCustomIndexColumnRenderer?.text?.runs?.[0]?.text || '';
            return { id: videoId, title: trackName, ...artistFields, thumbnail: getSquareThumbnail(r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || []), rank: parseInt(rank, 10) || 0, duration: r?.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || '', url: `https://music.youtube.com/watch?v=${videoId}` };
          }).filter(Boolean);
        } catch (plErr) { console.error('Chart playlist fetch error:', plErr); }
      }
      return result;
    } catch (err) { console.error('Charts error:', err); return null; }
  });

  ipcMain.handle('yt:browseMood', async (_event, browseId, params) => {
    try {
      await ensureYTMusic(ctx);
      const rawData = await ctx.ytmusic.constructRequest('browse', { browseId, params });
      const grid = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
      const playlists = [];
      for (const section of grid) {
        for (const item of (section?.gridRenderer?.items || section?.musicCarouselShelfRenderer?.contents || [])) {
          const r = item?.musicTwoRowItemRenderer;
          const playlistId = r?.navigationEndpoint?.browseEndpoint?.browseId || '';
          if (!r || !playlistId) continue;
          playlists.push({ playlistId, name: r?.title?.runs?.[0]?.text || 'Unknown', subtitle: (r?.subtitle?.runs || []).map(s => s.text).join(''), thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 300) });
        }
      }
      return playlists;
    } catch (err) { console.error('Browse mood error:', err); return []; }
  });

  ipcMain.handle('yt:getStreamUrl', async (_event, videoUrl, quality) => {
    const fmt = quality === 'worstaudio' ? 'worstaudio/worstaudio*/worst' : 'bestaudio/bestaudio*/best';
    const cacheKey = `audio:${videoUrl}:${fmt}`;
    const cached = getCachedUrl(cacheKey);
    if (cached) return cached;

    return new Promise((resolve, reject) => {
      execFile(getYtDlpPath(), ['-f', fmt, '--get-url', '--no-warnings', '--no-playlist', '--no-check-certificates', videoUrl], { timeout: 15000 }, (err, stdout, stderr) => {
        if (err) return reject(stderr?.trim() || err.message);
        const url = stdout.trim().split('\n')[0];
        if (!url) return reject('yt-dlp returned no URL');
        setCachedUrl(cacheKey, url);
        resolve(url);
      });
    });
  });

  ipcMain.handle('yt:downloadAudio', async (_event, videoUrl, quality, videoId) => {
    const cacheDir = ensureCacheDir();
    const existing = fs.readdirSync(cacheDir).find(f => f.startsWith(videoId + '.'));
    if (existing) return { path: path.join(cacheDir, existing) };

    const fmt = quality === 'worstaudio' ? 'worstaudio/worstaudio*/worst' : 'bestaudio/bestaudio*/best';
    return new Promise((resolve, reject) => {
      const proc = execFile(getYtDlpPath(), ['-f', fmt, '-o', path.join(cacheDir, videoId + '.%(ext)s'), '--no-part', '--no-warnings', '--no-playlist', '--no-check-certificates', videoUrl], { timeout: 120000 }, (err, _stdout, stderr) => {
        if (getActiveDownloadProc() === proc) setActiveDownloadProc(null);
        if (err) {
          if (err.killed || err.signal === 'SIGTERM') {
            const partial = fs.existsSync(cacheDir) && fs.readdirSync(cacheDir).find(f => f.startsWith(videoId + '.'));
            if (partial) try { fs.unlinkSync(path.join(cacheDir, partial)); } catch (_) {}
            return reject('cancelled');
          }
          return reject(stderr?.trim() || err.message);
        }
        const downloaded = fs.existsSync(cacheDir) && fs.readdirSync(cacheDir).find(f => f.startsWith(videoId + '.'));
        if (!downloaded) return reject('Download completed but file not found');
        resolve({ path: path.join(cacheDir, downloaded) });
      });
      setActiveDownloadProc(proc);
    });
  });

  ipcMain.handle('cache:deleteFile', async (_event, filePath) => {
    const resolved = path.resolve(filePath);
    if (!resolved.startsWith(AUDIO_CACHE_DIR + path.sep)) return { error: 'Invalid path' };
    try { fs.unlinkSync(resolved); } catch (_) {}
    return { ok: true };
  });

  ipcMain.handle('cache:clear', async () => {
    const { cleanupCacheDir } = require('./audio-cache');
    cleanupCacheDir();
    return { ok: true };
  });

  ipcMain.handle('cache:cancelDownload', async () => {
    const proc = getActiveDownloadProc();
    if (proc) { proc.kill('SIGTERM'); setActiveDownloadProc(null); }
    return { ok: true };
  });

  ipcMain.handle('song:saveTo', async (_event, videoUrl, title, artist) => {
    const { dialog } = require('electron');
    const { mt } = require('./i18n');
    const safeName = `${title || 'track'}${artist ? ' - ' + artist : ''}`.replace(/[/\\?%*:|"<>]/g, '_');
    const result = await dialog.showSaveDialog(ctx.mainWindow, {
      title: mt ? mt('dialog.saveSong') : 'Save song',
      defaultPath: safeName + '.mp3',
      filters: [{ name: 'MP3 Audio', extensions: ['mp3'] }],
    });
    if (result.canceled || !result.filePath) return { canceled: true };
    return new Promise((resolve) => {
      const proc = execFile(getYtDlpPath(), ['-x', '--audio-format', 'mp3', '--audio-quality', '0', '--no-part', '--no-warnings', '--no-playlist', '--no-check-certificates', '-o', result.filePath, videoUrl], { timeout: 300000 }, (err, _stdout, stderr) => {
        if (err) return resolve({ error: stderr?.trim() || err.message });
        resolve({ success: true, filePath: result.filePath });
      });
      void proc;
    });
  });

  ipcMain.handle('yt:getVideoStreamUrl', async (_event, videoId, quality, premuxed) => {
    const height = parseInt(quality) || 720;
    const fmt = premuxed
      ? `best[height<=${height}][protocol!=m3u8_native][protocol!=m3u8]/best[protocol!=m3u8_native][protocol!=m3u8]/best`
      : `bestvideo[height<=${height}][ext=mp4]+bestaudio[ext=m4a]/best[height<=${height}]/best`;
    const cacheKey = `video:${videoId}:${fmt}`;
    const cached = getCachedUrl(cacheKey);
    if (cached) return cached;

    return new Promise((resolve, reject) => {
      execFile(getYtDlpPath(), ['-f', fmt, '--get-url', '--no-warnings', '--no-playlist', '--no-check-certificates', `https://music.youtube.com/watch?v=${videoId}`], { timeout: 20000 }, (err, stdout, stderr) => {
        if (err) return reject(stderr?.trim() || err.message);
        const urls = stdout.trim().split('\n').filter(Boolean);
        if (!urls.length) return reject('yt-dlp returned no video URL');
        const result = { videoUrl: urls[0], audioUrl: urls[1] || null };
        setCachedUrl(cacheKey, result);
        resolve(result);
      });
    });
  });

  ipcMain.handle('yt:getTrackInfo', async (_event, videoId) => {
    try {
      await ensureYTMusic(ctx);
      return mapSongToTrack(await ctx.ytmusic.getSong(videoId));
    } catch (err) { console.error('getTrackInfo error:', err); return null; }
  });

  ipcMain.handle('yt:getUpNexts', async (_event, videoId) => {
    try {
      await ensureYTMusic(ctx);
      const rawData = await ctx.ytmusic.constructRequest('next', { videoId, playlistId: `RDAMVM${videoId}`, isAudioOnly: true });
      const contents = rawData?.contents?.singleColumnMusicWatchNextResultsRenderer?.tabbedRenderer?.watchNextTabbedResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.musicQueueRenderer?.content?.playlistPanelRenderer?.contents || [];
      return contents.slice(1).map(item => {
        const r = item?.playlistPanelVideoRenderer;
        const vid = r?.navigationEndpoint?.watchEndpoint?.videoId;
        if (!r || !vid) return null;
        const allRuns = r.longBylineText?.runs || [];
        const dotIdx = allRuns.findIndex(run => run.text === ' \u2022 ');
        const artists = parseArtistsFromRuns(dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns);
        const durationText = r.lengthText?.runs?.[0]?.text || '';
        const parts = durationText.split(':').map(Number);
        let durationMs = 0;
        if (parts.length === 2) durationMs = (parts[0] * 60 + parts[1]) * 1000;
        else if (parts.length === 3) durationMs = (parts[0] * 3600 + parts[1] * 60 + parts[2]) * 1000;
        return { id: vid, title: r.title?.runs?.[0]?.text || 'Unknown', ...buildArtistFields(artists), thumbnail: getSquareThumbnail(r.thumbnail?.thumbnails || []), duration: durationText, durationMs, url: `https://music.youtube.com/watch?v=${vid}` };
      }).filter(Boolean);
    } catch (err) { console.error('getUpNexts error:', err); return []; }
  });
}

module.exports = { initYTMusic, ensureYTMusic, formatDuration, getBestThumbnail, getSquareThumbnail, parseArtistsFromRuns, buildArtistFields, mapSongToTrack, register };
