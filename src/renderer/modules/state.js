// Shared mutable app state — imported as a singleton by all renderer modules.
// All modules that mutate state do so on this object directly (property assignment),
// which is visible to every other module holding the same reference.

const state = {
  currentView: 'home',
  queue: [],
  originalQueue: [],
  queueIndex: -1,
  isPlaying: false,
  shuffle: false,
  repeat: 'off',
  volume: 0.7,
  playlists: [],
  likedSongs: [],
  recentTracks: [],
  followedArtists: [],
  currentPlaylistId: null,
  playingPlaylistId: null,
  isLoading: false,
  musicOnly: true,
  autoplay: false,
  audioQuality: 'bestaudio',
  videoQuality: '720',
  videoPremuxed: true,
  animations: true,
  effects: true,
  miniplayerGlow: true,
  theme: 'dark',
  discordRpc: false,
  country: '',
  searchHistory: [],
  crossfade: 0,
  normalization: false,
  normalizationTarget: -14,
  prefetchCount: 0,
  showListeningActivity: true,
  minimizeToTray: false,
  launchOnStartup: false,
  // Sources (ordered by priority; first enabled = primary, rest = fallbacks)
  songSources: ['youtube'],
  metadataSources: ['youtube'],
  // Play log for Wrapped — stored in a separate localStorage key to avoid quota issues
  playLog: [],
  wrappedShownYear: null,
  // Spotify genre cache — stored in a separate localStorage key
  trackGenreCache: {}
};

export default state;
