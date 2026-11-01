// Records a live YouTube Music session through the unmodified ytmusic.js/media.js handlers
// (ytmusic-api's axios adapter wrapped), for the ytmusic parity test to replay to both sides.
// Usage: ELECTRON_RUN_AS_NODE=1 electron ytm_record.js <out.json.gz>
// Output: { ops: [[channel, [wire args]]], exchanges: [{ method, url, headers, body, status,
//           statusText, resHeaders, data }] }
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const Module = require('module');

const MAIN = path.resolve(__dirname, '../../../src/main');
const codecSrc = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { encode } = new Function('window', `${codecSrc}\nreturn { encode };`)(globalThis);
const handlers = {};
const electron = {
  app: { getPath: () => '/tmp/snowify-ytm-record', isPackaged: false, getLocale: () => 'en-US' },
  session: { defaultSession: { resolveProxy: async () => 'DIRECT' } },
  ipcMain: { handle: (ch, fn) => { handlers[ch] = fn; }, on: () => {} },
  dialog: {}, shell: {}, BrowserWindow: { getAllWindows: () => [] },
};
const load = Module._load;
Module._load = function (req, ...rest) { return req === 'electron' ? electron : load.call(this, req, ...rest); };

(async () => {
  const exchanges = [];
  const axios = (await import('axios')).default;
  const http = axios.getAdapter('http');
  axios.defaults.adapter = async (config) => {
    const res = await http(config);
    exchanges.push({
      method: config.method,
      url: axios.getUri(config),
      headers: config.headers.toJSON(),
      body: typeof config.data === 'string' ? config.data : null,
      status: res.status,
      statusText: res.statusText,
      resHeaders: res.headers.toJSON(),
      data: typeof res.data === 'string' ? res.data : JSON.stringify(res.data),
    });
    return res;
  };
  const ctx = {};
  require(path.join(MAIN, 'ytmusic')).register(electron.ipcMain, ctx);
  require(path.join(MAIN, 'media')).register(electron.ipcMain, ctx);
  const ops = [];
  const call = async (ch, ...args) => {
    ops.push([ch, args.map(encode)]);
    const r = await handlers[ch]({}, ...args);
    console.error(ch, JSON.stringify(args), '->', JSON.stringify(r)?.slice(0, 120));
    return r;
  };

  await call('yt:search', 'daft punk', true);
  const songs = await call('yt:search', 'daft punk', false);
  await call('yt:search', 'queen bohemian rhapsody', false);
  await call('yt:searchSuggestions', 'daft p');
  const artists = await call('yt:searchArtists', 'daft punk');
  if (artists[0]) await call('yt:artistInfo', artists[0].artistId);
  const albums = await call('yt:searchAlbums', 'discovery daft punk');
  if (albums[0]) await call('yt:albumTracks', albums[0].albumId);
  const playlists = await call('yt:searchPlaylists', 'daft punk essentials');
  if (playlists[0]) await call('yt:getPlaylistVideos', playlists[0].playlistId);
  await call('yt:searchVideos', 'daft punk');
  const explore = await call('yt:explore');
  await call('yt:charts');
  const mood = explore?.moods?.[0];
  if (mood) await call('yt:browseMood', mood.browseId, mood.params);
  const vid = songs.find((s) => s.id)?.id;
  if (vid) {
    await call('yt:getTrackInfo', vid);
    await call('yt:getUpNexts', vid);
  }
  await call('yt:getTrackInfo', 'not-an-id');
  await call('yt:artistInfo', 'UCxxxxxxxxxxxxxxxxxxxxxx');
  await call('spotify:matchTrack', 'One More Time', 'Daft Punk');
  await call('spotify:matchTrack', 'Zzyzx Unfindable Tune', 'Nobody Atall');
  await call('yt:setCountry', 'DE');
  await call('yt:search', 'schlager', false);
  await call('yt:setCountry', '');

  const out = zlib.gzipSync(JSON.stringify({ ops, exchanges }));
  fs.writeFileSync(process.argv[2], out);
  console.error(`recorded ${ops.length} ops, ${exchanges.length} exchanges, ${out.length} bytes gzipped`);
  process.exit(0);
})().catch((e) => { console.error('FAILED', e); process.exit(1); });
