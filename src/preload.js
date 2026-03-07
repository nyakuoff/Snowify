const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('snowify', {
  // Platform
  platform: process.platform,

  // Window controls
  minimize: () => ipcRenderer.send('window:minimize'),
  maximize: () => ipcRenderer.send('window:maximize'),
  close: () => ipcRenderer.send('window:close'),

  // YouTube Music
  search: (query, musicOnly) => ipcRenderer.invoke('yt:search', query, musicOnly),
  searchArtists: (query) => ipcRenderer.invoke('yt:searchArtists', query),
  searchAlbums: (query) => ipcRenderer.invoke('yt:searchAlbums', query),
  searchVideos: (query) => ipcRenderer.invoke('yt:searchVideos', query),
  searchPlaylists: (query) => ipcRenderer.invoke('yt:searchPlaylists', query),
  getPlaylistVideos: (playlistId) => ipcRenderer.invoke('yt:getPlaylistVideos', playlistId),
  searchSuggestions: (query) => ipcRenderer.invoke('yt:searchSuggestions', query),
  getStreamUrl: (videoUrl, quality) => ipcRenderer.invoke('yt:getStreamUrl', videoUrl, quality),
  downloadAudio: (videoUrl, quality, videoId) => ipcRenderer.invoke('yt:downloadAudio', videoUrl, quality, videoId),
  deleteCachedAudio: (filePath) => ipcRenderer.invoke('cache:deleteFile', filePath),
  clearAudioCache: () => ipcRenderer.invoke('cache:clear'),
  cancelDownload: () => ipcRenderer.invoke('cache:cancelDownload'),
  artistInfo: (artistId) => ipcRenderer.invoke('yt:artistInfo', artistId),
  albumTracks: (albumId) => ipcRenderer.invoke('yt:albumTracks', albumId),
  getUpNexts: (videoId) => ipcRenderer.invoke('yt:getUpNexts', videoId),
  getVideoStreamUrl: (videoId, quality, premuxed) => ipcRenderer.invoke('yt:getVideoStreamUrl', videoId, quality, premuxed),
  explore: () => ipcRenderer.invoke('yt:explore'),
  charts: () => ipcRenderer.invoke('yt:charts'),
  browseMood: (browseId, params) => ipcRenderer.invoke('yt:browseMood', browseId, params),
  setCountry: (code) => ipcRenderer.invoke('yt:setCountry', code),
  getLyrics: (trackName, artistName, albumName, duration) => ipcRenderer.invoke('lyrics:get', trackName, artistName, albumName, duration),
  openExternal: (url) => ipcRenderer.invoke('shell:openExternal', url),

  // Custom themes
  scanThemes: () => ipcRenderer.invoke('theme:scan'),
  loadTheme: (id) => ipcRenderer.invoke('theme:load', id),
  reloadTheme: (id) => ipcRenderer.invoke('theme:reload', id),
  addTheme: () => ipcRenderer.invoke('theme:add'),
  removeTheme: (id) => ipcRenderer.invoke('theme:remove', id),
  openThemesFolder: () => ipcRenderer.invoke('theme:openFolder'),

  // Playlist covers
  pickImage: () => ipcRenderer.invoke('playlist:pickImage'),
  saveImage: (playlistId, sourcePath) => ipcRenderer.invoke('playlist:saveImage', playlistId, sourcePath),
  deleteImage: (imagePath) => ipcRenderer.invoke('playlist:deleteImage', imagePath),

  // Playlist export
  exportPlaylistCsv: (name, tracks) => ipcRenderer.invoke('playlist:exportCsv', name, tracks),

  // Account & Cloud Sync
  signInWithEmail: (email, password) => ipcRenderer.invoke('auth:signInWithEmail', email, password),
  signUpWithEmail: (email, password) => ipcRenderer.invoke('auth:signUpWithEmail', email, password),
  authSignOut: () => ipcRenderer.invoke('auth:signOut'),
  getUser: () => ipcRenderer.invoke('auth:getUser'),
  updateProfile: (data) => ipcRenderer.invoke('profile:update', data),
  updateProfileExtras: (data) => ipcRenderer.invoke('profile:updateExtras', data),
  getProfile: (uid) => ipcRenderer.invoke('profile:get', uid),
  readImage: (filePath) => ipcRenderer.invoke('profile:readImage', filePath),
  cloudSave: (data) => ipcRenderer.invoke('cloud:save', data),
  cloudLoad: () => ipcRenderer.invoke('cloud:load'),
  onAuthStateChanged: (callback) => {
    ipcRenderer.on('auth:stateChanged', (_event, user) => callback(user));
  },

  // Spotify import (CSV)
  spotifyPickCsv: () => ipcRenderer.invoke('spotify:pickCsv'),
  spotifyMatchTrack: (title, artist) => ipcRenderer.invoke('spotify:matchTrack', title, artist),

  // Windows thumbbar
  updateThumbar: (isPlaying) => ipcRenderer.send('thumbar:updateState', isPlaying),
  onThumbarPrev: (cb) => ipcRenderer.on('thumbar:prev', cb),
  onThumbarPlayPause: (cb) => ipcRenderer.on('thumbar:playPause', cb),
  onThumbarNext: (cb) => ipcRenderer.on('thumbar:next', cb),

  // Discord RPC
  connectDiscord: () => ipcRenderer.invoke('discord:connect'),
  disconnectDiscord: () => ipcRenderer.invoke('discord:disconnect'),
  updatePresence: (data) => ipcRenderer.invoke('discord:updatePresence', data),
  clearPresence: () => ipcRenderer.invoke('discord:clearPresence'),

  // Social / Friends
  getFriendCode: () => ipcRenderer.invoke('social:getFriendCode'),
  addFriend: (code) => ipcRenderer.invoke('social:addFriend', code),
  removeFriend: (uid) => ipcRenderer.invoke('social:removeFriend', uid),
  getFriends: () => ipcRenderer.invoke('social:getFriends'),
  updateSocialPresence: (data) => ipcRenderer.invoke('social:updatePresence', data),
  clearSocialPresence: () => ipcRenderer.invoke('social:clearPresence'),
  getPresence: (uid) => ipcRenderer.invoke('social:getPresence', uid),
  getFriendsPresence: (uids) => ipcRenderer.invoke('social:getFriendsPresence', uids),
  startSocialListening: () => ipcRenderer.invoke('social:startListening'),
  stopSocialListening: () => ipcRenderer.invoke('social:stopListening'),
  onFriendsUpdated: (cb) => ipcRenderer.on('social:friendsUpdated', (_e, friends) => cb(friends)),
  onPresenceUpdated: (cb) => ipcRenderer.on('social:presenceUpdated', (_e, data) => cb(data)),
  savePublicPlaylists: (playlists) => ipcRenderer.invoke('social:savePublicPlaylists', playlists),
  getPublicPlaylists: (uid) => ipcRenderer.invoke('social:getPublicPlaylists', uid),

  // Listen Along
  requestListenAlong: (targetUid) => ipcRenderer.invoke('social:requestListenAlong', targetUid),
  respondListenAlong: (fromUid, accepted) => ipcRenderer.invoke('social:respondListenAlong', fromUid, accepted),
  endListenAlong: () => ipcRenderer.invoke('social:endListenAlong'),
  onListenAlongRequest: (cb) => ipcRenderer.on('social:listenAlongRequest', (_e, data) => cb(data)),
  onOwnPresenceUpdated: (cb) => ipcRenderer.on('social:ownPresenceUpdated', (_e, data) => cb(data)),

  // Graceful close
  onBeforeClose: (callback) => ipcRenderer.on('app:before-close', callback),
  closeReady: () => ipcRenderer.send('app:close-ready'),

  // i18n
  getLocale: () => ipcRenderer.invoke('app:getLocale'),
  setLocale: (locale) => ipcRenderer.invoke('app:setLocale', locale),

  // Auto-updater
  getVersion: () => ipcRenderer.invoke('app:getVersion'),
  getChangelog: (version) => ipcRenderer.invoke('app:getChangelog', version),
  getRecentReleases: () => ipcRenderer.invoke('app:getRecentReleases'),
  checkForUpdates: () => ipcRenderer.invoke('updater:check'),
  installUpdate: () => ipcRenderer.send('updater:install'),
  onUpdateStatus: (callback) => {
    ipcRenderer.on('updater:status', (_event, data) => callback(data));
  },

  // Debug logs
  getLogs: () => ipcRenderer.invoke('app:getLogs'),
  appendLog: (entry) => ipcRenderer.invoke('app:appendLog', entry),

  // Plugins
  getPluginRegistry: () => ipcRenderer.invoke('plugins:getRegistry'),
  getInstalledPlugins: () => ipcRenderer.invoke('plugins:getInstalled'),
  installPlugin: (entry) => ipcRenderer.invoke('plugins:install', entry),
  uninstallPlugin: (id) => ipcRenderer.invoke('plugins:uninstall', id),
  getPluginFiles: (id) => ipcRenderer.invoke('plugins:getFiles', id),
  restartApp: () => ipcRenderer.invoke('app:restart'),
});
