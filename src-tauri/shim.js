// window.snowify for the Tauri build: the same members as src/preload.js, carried over Tauri IPC
// with Electron's value semantics (contextBridge + structured clone), and Electron's
// googlevideo CORS rule applied to media elements. Injected before any page script.
// Rust wraps codec.js and this file in one closure (`encode`/`decode` come from codec.js) and
// fills in __SNOWIFY_BOOT__ = { platform, proxy } (see src/shell.rs).
{
  const T = window.__TAURI_INTERNALS__;
  const BOOT = window.__SNOWIFY_BOOT__;
  delete window.__SNOWIFY_BOOT__;

  // ─── ipcRenderer equivalents ───
  function invoke(channel, ...args) {
    let wire;
    try { wire = args.map(encode); } catch (e) { return Promise.reject(e); }
    return T.invoke('ipc_invoke', { channel, args: wire }).then(decode, (message) => {
      throw new Error(decode(message));
    });
  }
  function send(channel, ...args) {
    const wire = args.map(encode);
    T.invoke('ipc_send', { channel, args: wire }).catch(() => {});
  }
  // WebKit/WebView2 do not implement Electron's -webkit-app-region: drag.
  // Keep this native-only, and never turn the window controls into drag targets.
  document.addEventListener('mousedown', (event) => {
    if (event.button !== 0 || !(event.target instanceof Element) || !event.target.closest('.titlebar-drag')) return;
    event.preventDefault();
    send(event.detail === 2 ? 'window:maximize' : 'window:startDragging');
  });
  const listeners = new Map();
  function on(channel, fn) {
    if (!listeners.has(channel)) listeners.set(channel, []);
    listeners.get(channel).push(fn);
  }
  T.invoke('plugin:event|listen', {
    event: 'snowify://ipc',
    target: { kind: 'Any' },
    handler: T.transformCallback(({ payload }) => {
      for (const fn of listeners.get(payload.channel) || []) {
        fn({ sender: {}, ports: [] }, ...payload.args.map(decode));
      }
    }),
  });

  // ─── Electron's media origin: file:// page + googlevideo CORS header (src/media_proxy.rs) ───
  // Element URLs that need it are routed through the loopback proxy; the proxy URL carries the
  // original attribute, so the getters below report exactly what Electron's would.
  const PROXY = `http://127.0.0.1:${BOOT.proxy.port}/${BOOT.proxy.token}/m?a=`;
  const absolute = (u) => { try { return new URL(u, document.baseURI).href; } catch { return null; } };
  const needsProxy = (attr) => {
    const href = absolute(attr);
    return !!href && (href.startsWith('file:') || href.includes('.googlevideo.com/'));
  };
  const toProxy = (attr) => PROXY + encodeURIComponent(attr);
  const fromProxy = (v) => (typeof v === 'string' && v.startsWith(PROXY) ? decodeURIComponent(v.slice(PROXY.length)) : null);
  const setAttr = Element.prototype.setAttribute;
  const getAttr = Element.prototype.getAttribute;
  const ELEMENTS = [HTMLMediaElement, HTMLImageElement, HTMLSourceElement];
  const routed = (el) => ELEMENTS.some((C) => el instanceof C);
  function assignSrc(el, value) {
    const attr = String(value);
    setAttr.call(el, 'src', needsProxy(attr) ? toProxy(attr) : attr);
  }
  for (const C of ELEMENTS) {
    const proto = C.prototype;
    const srcDesc = Object.getOwnPropertyDescriptor(proto, 'src');
    Object.defineProperty(proto, 'src', {
      configurable: true,
      enumerable: srcDesc.enumerable,
      get() { const orig = fromProxy(getAttr.call(this, 'src')); return orig === null ? srcDesc.get.call(this) : absolute(orig); },
      set(v) { assignSrc(this, v); },
    });
    const cur = Object.getOwnPropertyDescriptor(proto, 'currentSrc');
    if (cur) {
      Object.defineProperty(proto, 'currentSrc', {
        configurable: true,
        enumerable: cur.enumerable,
        get() { const c = cur.get.call(this); const orig = fromProxy(c); return orig === null ? c : absolute(orig); },
      });
    }
  }
  Element.prototype.setAttribute = function (name, value) {
    if (routed(this) && String(name).toLowerCase() === 'src') return assignSrc(this, value);
    return setAttr.call(this, name, value);
  };
  Element.prototype.getAttribute = function (name) {
    const v = getAttr.call(this, name);
    if (routed(this) && String(name).toLowerCase() === 'src') {
      const orig = fromProxy(v);
      if (orig !== null) return orig;
    }
    return v;
  };
  // Elements created by the parser or innerHTML set their src attribute directly.
  const rewriteTree = (node) => {
    if (node.nodeType !== 1) return;
    const els = routed(node) ? [node] : [];
    if (node.querySelectorAll) els.push(...node.querySelectorAll('img[src],audio[src],video[src],source[src]'));
    for (const el of els) {
      const v = getAttr.call(el, 'src');
      if (v !== null && fromProxy(v) === null && needsProxy(v)) setAttr.call(el, 'src', toProxy(v));
    }
  };
  new MutationObserver((records) => {
    for (const r of records) {
      if (r.type === 'childList') r.addedNodes.forEach(rewriteTree);
      else rewriteTree(r.target);
    }
  }).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ['src'] });
  const NativeAudio = window.Audio;
  window.Audio = function Audio(src) {
    const a = new NativeAudio();
    if (arguments.length > 0 && src !== undefined) a.src = src;
    return a;
  };
  window.Audio.prototype = NativeAudio.prototype;

  // ─── window.snowify (src/preload.js) ───
  const MOBILE_PROXY_PREFIX = 'http://127.0.0.1:17890/stream?url=';
  function deproxyUrl(url) {
    if (typeof url !== 'string' || !url.startsWith(MOBILE_PROXY_PREFIX)) return url;
    try {
      const parsed = new URL(url);
      return parsed.searchParams.get('url') || url;
    } catch {
      return url;
    }
  }

  // contextBridge.exposeInMainWorld: a frozen copy on a non-writable, non-configurable,
  // enumerable global; exposed functions report length 0 and name ''.
  function expose(name, api) {
    for (const v of Object.values(api)) {
      if (typeof v === 'function') Object.defineProperties(v, { length: { value: 0 }, name: { value: '' } });
    }
    Object.defineProperty(window, name, { value: Object.freeze(api), writable: false, configurable: false, enumerable: true });
  }

  if (BOOT.transportProbe) {
    expose('probe', {
      echo: (...args) => invoke('echo', ...args),
      echoArity: (...args) => invoke('echoArity', ...args),
      throwAt: (i) => invoke('throw', i),
      send: (...args) => send('send', ...args),
      emitAll: () => invoke('emitAll'),
      onEv: (cb) => { on('ev', (_e, i, v) => cb(i, v)); },
      onRaw: (cb) => on('evDone', cb),
      onEvFail: (cb) => { on('evFail', (_e, i, m) => cb(i, m)); },
      missing: () => invoke('no:such:channel', 1),
      done: (results) => invoke('done', results),
    });
    return;
  }

  expose('snowify', {
    platform: BOOT.platform,
    resolveImageUrl: (url) => deproxyUrl(url),

    minimize: () => send('window:minimize'),
    maximize: () => send('window:maximize'),
    close: () => send('window:close'),
    setMinimizeToTray: (enabled) => send('window:setMinimizeToTray', enabled),
    setOpenAtLogin: (enabled) => send('window:setOpenAtLogin', enabled),

    search: (query, musicOnly) => invoke('yt:search', query, musicOnly),
    searchArtists: (query) => invoke('yt:searchArtists', query),
    searchAlbums: (query) => invoke('yt:searchAlbums', query),
    searchVideos: (query) => invoke('yt:searchVideos', query),
    searchPlaylists: (query) => invoke('yt:searchPlaylists', query),
    getPlaylistVideos: (playlistId) => invoke('yt:getPlaylistVideos', playlistId),
    searchSuggestions: (query) => invoke('yt:searchSuggestions', query),
    getStreamUrl: (videoUrl, quality, trackMeta, sources) => invoke('yt:getStreamUrl', videoUrl, quality, trackMeta, sources),
    downloadAudio: (videoUrl, quality, videoId) => invoke('yt:downloadAudio', videoUrl, quality, videoId),
    saveSong: (videoUrl, title, artist) => invoke('song:saveTo', videoUrl, title, artist),
    deleteCachedAudio: (filePath) => invoke('cache:deleteFile', filePath),
    clearAudioCache: () => invoke('cache:clear'),
    cancelDownload: () => invoke('cache:cancelDownload'),
    artistInfo: (artistId) => invoke('yt:artistInfo', artistId),
    albumTracks: (albumId) => invoke('yt:albumTracks', albumId),
    getUpNexts: (videoId) => invoke('yt:getUpNexts', videoId),
    getVideoStreamUrl: (videoId, quality, premuxed) => invoke('yt:getVideoStreamUrl', videoId, quality, premuxed),
    explore: () => invoke('yt:explore'),
    charts: () => invoke('yt:charts'),
    browseMood: (browseId, params) => invoke('yt:browseMood', browseId, params),
    setCountry: (code) => invoke('yt:setCountry', code),
    getLyrics: (trackName, artistName, albumName, duration) => invoke('lyrics:get', trackName, artistName, albumName, duration),
    openExternal: (url) => invoke('shell:openExternal', url),

    scanThemes: () => invoke('theme:scan'),
    loadTheme: (id) => invoke('theme:load', id),
    reloadTheme: (id) => invoke('theme:reload', id),
    addTheme: () => invoke('theme:add'),
    removeTheme: (id) => invoke('theme:remove', id),
    openThemesFolder: () => invoke('theme:openFolder'),

    pickImage: () => invoke('playlist:pickImage'),
    saveImage: (playlistId, sourcePath) => invoke('playlist:saveImage', playlistId, sourcePath),
    deleteImage: (imagePath) => invoke('playlist:deleteImage', imagePath),

    exportPlaylistCsv: (name, tracks) => invoke('playlist:exportCsv', name, tracks),

    signInWithEmail: (email, password) => invoke('auth:signInWithEmail', email, password),
    signUpWithEmail: (email, password) => invoke('auth:signUpWithEmail', email, password),
    sendPasswordReset: (email) => invoke('auth:sendPasswordReset', email),
    authSignOut: () => invoke('auth:signOut'),
    getUser: () => invoke('auth:getUser'),
    updateProfile: (data) => invoke('profile:update', data),
    updateProfileExtras: (data) => invoke('profile:updateExtras', data),
    getProfile: (uid) => invoke('profile:get', uid),
    readImage: (filePath) => invoke('profile:readImage', filePath),
    cloudSave: (data) => invoke('cloud:save', data),
    cloudLoad: () => invoke('cloud:load'),
    onAuthStateChanged: (callback) => {
      on('auth:stateChanged', (_event, user) => callback(user));
    },

    exportLibrary: (jsonStr) => invoke('library:export', jsonStr),
    importLibrary: () => invoke('library:import'),

    spotifyPickCsv: () => invoke('spotify:pickCsv'),
    spotifyMatchTrack: (title, artist) => invoke('spotify:matchTrack', title, artist),

    updateThumbar: (isPlaying) => send('thumbar:updateState', isPlaying),
    onThumbarPrev: (cb) => on('thumbar:prev', cb),
    onThumbarPlayPause: (cb) => on('thumbar:playPause', cb),
    onThumbarNext: (cb) => on('thumbar:next', cb),

    connectDiscord: () => invoke('discord:connect'),
    disconnectDiscord: () => invoke('discord:disconnect'),
    updatePresence: (data) => invoke('discord:updatePresence', data),
    clearPresence: () => invoke('discord:clearPresence'),

    onBeforeClose: (callback) => on('app:before-close', callback),
    closeReady: () => send('app:close-ready'),

    getLocale: () => invoke('app:getLocale'),
    setLocale: (locale) => invoke('app:setLocale', locale),

    getVersion: () => invoke('app:getVersion'),
    getChangelog: (version) => invoke('app:getChangelog', version),
    getRecentReleases: () => invoke('app:getRecentReleases'),
    checkForUpdates: () => invoke('updater:check'),
    installUpdate: () => send('updater:install'),
    onUpdateStatus: (callback) => {
      on('updater:status', (_event, data) => callback(data));
    },

    getLogs: () => invoke('app:getLogs'),
    appendLog: (entry) => invoke('app:appendLog', entry),

    getPluginRegistry: () => invoke('plugins:getRegistry'),
    getInstalledPlugins: () => invoke('plugins:getInstalled'),
    installPlugin: (entry) => invoke('plugins:install', entry),
    uninstallPlugin: (id) => invoke('plugins:uninstall', id),
    getPluginFiles: (id) => invoke('plugins:getFiles', id),
    restartApp: () => invoke('app:restart'),

    onDeepLink: (cb) => on('app:deepLink', (_e, data) => cb(data)),
    getPendingDeepLink: () => invoke('app:getPendingDeepLink'),

    pickAudioFiles: () => invoke('local:pickAudioFiles'),
    pickAudioFolder: () => invoke('local:pickFolder'),
    scanAudioFolder: (folderPath) => invoke('local:scanFolder', folderPath),
    copyToPlaylistFolder: (filePath, folderPath) => invoke('local:copyToPlaylistFolder', filePath, folderPath),

    getTrackInfo: (videoId) => invoke('yt:getTrackInfo', videoId),

    getInstalledMarketplaceThemes: () => invoke('themes:getInstalled'),
    installMarketplaceTheme: (entry) => invoke('themes:install', entry),
    uninstallMarketplaceTheme: (id) => invoke('themes:uninstallMarketplace', id),

    httpGet: (url, headers) => invoke('net:httpGet', url, headers),
  });
}
