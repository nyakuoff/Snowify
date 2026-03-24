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
  saveSong: (videoUrl, title, artist) => ipcRenderer.invoke('song:saveTo', videoUrl, title, artist),
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

  // Library export/import
  exportLibrary: (jsonStr) => ipcRenderer.invoke('library:export', jsonStr),
  importLibrary: () => ipcRenderer.invoke('library:import'),

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

  // Deep links
  onDeepLink: (cb) => ipcRenderer.on('app:deepLink', (_e, data) => cb(data)),
  getPendingDeepLink: () => ipcRenderer.invoke('app:getPendingDeepLink'),

  // Local audio
  pickAudioFiles: () => ipcRenderer.invoke('local:pickAudioFiles'),
  pickAudioFolder: () => ipcRenderer.invoke('local:pickFolder'),
  scanAudioFolder: (folderPath) => ipcRenderer.invoke('local:scanFolder', folderPath),
  copyToPlaylistFolder: (filePath, folderPath) => ipcRenderer.invoke('local:copyToPlaylistFolder', filePath, folderPath),

  // Track info (for deep links)
  getTrackInfo: (videoId) => ipcRenderer.invoke('yt:getTrackInfo', videoId),

  // Marketplace themes
  getInstalledMarketplaceThemes: () => ipcRenderer.invoke('themes:getInstalled'),
  installMarketplaceTheme: (entry) => ipcRenderer.invoke('themes:install', entry),
  uninstallMarketplaceTheme: (id) => ipcRenderer.invoke('themes:uninstallMarketplace', id),
});
