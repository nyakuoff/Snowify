/**
 * player.js
 * Core playback engine — initializes DualAudioEngine + normalizer + prefetch
 * cache, handles all playback controls, and owns the Now Playing bar state.
 */

import { audioRef, VOLUME_SCALE } from './audio-ref.js';
import state from './state.js';
import { showToast, pathToFileUrl, setupSliderTooltip, formatTime } from './utils.js';
import { callbacks } from './callbacks.js';
// Circular imports — safe at runtime: all usage is inside function bodies.
import { maxNPState, onTrackChanged, updateMaxNPProgress, syncMaxNPControls,
         extractDominantColor } from './now-playing.js';
import { syncVideoAudioVolume } from './video-player.js';
import { renderQueue, addToRecent } from './queue.js';
import { renderPlaylists, renderLibrary } from './library.js';
import { showAlbumDetail } from './album.js';
import { openArtistPage } from './artist.js';

const $  = (sel, ctx = document) => ctx.querySelector(sel);
const $$ = (sel, ctx = document) => [...ctx.querySelectorAll(sel)];

// ─── Shared SVG constants (exported for other modules) ───────────────────────
export const PLAY_SVG   = '<svg width="24" height="24" viewBox="0 0 24 24" fill="#000"><path d="M8 5v14l11-7L8 5z"/></svg>';
export const PAUSE_SVG  = '<svg width="24" height="24" viewBox="0 0 24 24" fill="#000"><path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/></svg>';
export const SIDEBAR_PLAY_SVG   = '<svg viewBox="0 0 24 24"><path d="M8 5v14l11-7L8 5z"/></svg>';
export const SIDEBAR_PAUSE_SVG  = '<svg viewBox="0 0 24 24"><path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/></svg>';
export const ICON_BROKEN_HEART  = '<svg width="20" height="20" viewBox="0 0 24 24" fill="var(--accent)"><path d="M2 8.5C2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09V21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5z"/><path d="M12 5.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35V5.09z" transform="translate(1.5, 2) rotate(8, 12, 12)"/></svg>';
export const LOCAL_THUMB_FALLBACK = 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4" fill="%231e1e2e"/><path d="M12 3v10.55A4 4 0 1 0 14 17V7h4V3h-6z" fill="%23888"/></svg>');
export const NOW_PLAYING_ICON_SVG = '<svg class="now-playing-icon" width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><path d="M10.016 1.125A.75.75 0 0 0 8.99.85l-4.2 3.43H1.75A.75.75 0 0 0 1 5.03v5.94a.75.75 0 0 0 .75.75h3.04l4.2 3.43a.75.75 0 0 0 1.026-.275.75.75 0 0 0 .1-.375V1.5a.75.75 0 0 0-.1-.375z"/><path class="sound-wave wave-1" opacity="0" d="M12.25 3.17a.75.75 0 0 0-.917 1.19 3.56 3.56 0 0 1 0 7.28.75.75 0 0 0 .918 1.19 5.06 5.06 0 0 0 0-9.66z"/><path class="sound-wave wave-2" opacity="0" d="M14.2 1.5a.75.75 0 0 0-.917 1.19 5.96 5.96 0 0 1 0 10.62.75.75 0 0 0 .918 1.19 7.46 7.46 0 0 0 0-13z"/></svg>';
export const NOW_PLAYING_EQ_HTML  = '<div class="track-eq"><span class="eq-bar"></span><span class="eq-bar"></span><span class="eq-bar"></span></div>';

const IS_MOBILE_RUNTIME =
  window.snowify?.platform === 'android' ||
  window.snowify?.platform === 'ios' ||
  document.documentElement.classList.contains('platform-mobile') ||
  /Android|iPhone|iPad|iPod/i.test(navigator.userAgent || '');

function resolveImageUrl(url) {
  if (!url) return url;
  return window.snowify?.resolveImageUrl?.(url) || url;
}

// ─── Audio elements ──────────────────────────────────────────────────────────
const audioA = $('#audio-player');
const audioB = $('#audio-player-b');
const appEl  = $('#app');

audioRef.audioA = audioA;
audioRef.audioB = audioB;

// ─── Private state ───────────────────────────────────────────────────────────
let _playGeneration         = 0;
let _consecutiveFailures    = 0;
const MAX_CONSECUTIVE_FAILURES = 3;
let _smartQueueFillInFlight = false;
let _showTimeRemaining      = false;
let prevVolume              = 0.7;
let isDraggingProgress      = false;
let isDraggingVolume        = false;
let lastPositionUpdate      = 0;

// ─── Exported getters/setters for _showTimeRemaining & prevVolume ────────────
export function isShowTimeRemaining()    { return _showTimeRemaining; }
export function toggleShowTimeRemaining() { _showTimeRemaining = !_showTimeRemaining; }
export function getPrevVolume()           { return prevVolume; }
export function setPrevVolume(v)          { prevVolume = v; }

// ─── Engine init ─────────────────────────────────────────────────────────────

let engine;
let audio;

// Initialise engine synchronously (DOM + globals already ready) ─────────────
function _initEngine() {
  engine = window.DualAudioEngine(audioA, audioB, {
    getState: () => state,
    getStreamUrl: async (url, q) => {
      const videoId = url.includes('watch?v=') ? new URL(url).searchParams.get('v') : null;
      if (videoId) {
        const cached = prefetchCache.getCachedPath(videoId);
        if (cached) return pathToFileUrl(cached);
      }
      const track     = state.queue.find(t => t.url === url || (videoId && t.id === videoId));
      const trackMeta = track ? { title: track.title, artist: track.artist } : {};
      return window.snowify.getStreamUrl(url, q, trackMeta, state.songSources);
    },
    onTransition: handleEngineTransition,
    onTimeUpdate:  handleEngineTimeUpdate,
    onStall: () => { showToast(I18n.t('toast.streamStalled')); playNext(); },
  });
  audio = engine.getActiveAudio();
  audioRef.engine = engine;
  audioRef.audio  = audio;
}

// ─── Loudness normalizer ─────────────────────────────────────────────────────

let normalizer;

function _initNormalizer() {
  normalizer = IS_MOBILE_RUNTIME
    ? {
        setEnabled() {}, setTarget() {}, initAudioContext: async () => {},
        isWorkletReady: () => true, analyzeAndApply() {}, finalizeMeasurement() {},
        applyGain() {}, preAnalyze() {}, getCachedLUFS: () => null,
        startMeasurement() {}, updateVolumeCompensation() {},
      }
    : window.LoudnessNormalizer(audioA, audioB);
  if (!IS_MOBILE_RUNTIME && state.normalization) {
    normalizer.setEnabled(true);
    normalizer.setTarget(state.normalizationTarget);
    normalizer.initAudioContext();
  }
}
export function getNormalizer() { return normalizer; }

// ─── Prefetch cache ──────────────────────────────────────────────────────────

let prefetchCache;

function updateCachedCount() {
  const el = $('#queue-cached-count');
  if (!el) return;
  if (state.prefetchCount === 0) { el.textContent = ''; return; }
  let count = 0;
  for (let i = state.queueIndex + 1; i < state.queue.length; i++) {
    if (prefetchCache.getCachedPath(state.queue[i].id)) count++;
  }
  el.textContent = count > 0 ? I18n.t('queue.cached', { count }) : '';
}

function _initPrefetchCache() {
  prefetchCache = window.PrefetchCache({
    getState: () => state,
    downloadAudio:     (url, q, id) => window.snowify.downloadAudio(url, q, id),
    deleteCachedAudio: (p)          => window.snowify.deleteCachedAudio(p),
    clearCache:        ()           => window.snowify.clearAudioCache(),
    cancelDownload:    ()           => window.snowify.cancelDownload(),
  });
  if (state.prefetchCount !== 0) {
    prefetchCache.setCount(state.prefetchCount);
    if (state.queue.length && state.queueIndex >= 0) {
      prefetchCache.onTrackChanged(state.queueIndex, state.queue);
    }
  }
  prefetchCache.onCacheUpdateCb(() => updateCachedCount());
}

export function getPrefetchCache() { return prefetchCache; }

// ─── Module-level initialisation ─────────────────────────────────────────────
// Runs synchronously when this module is first imported.
_initEngine();
_initNormalizer();
_initPrefetchCache();

// ─── Playback functions ──────────────────────────────────────────────────────

export async function playTrack(track) {
  const gen = ++_playGeneration;
  if (engine.isInProgress()) { engine.instantComplete(); audio = engine.getActiveAudio(); audioRef.audio = audio; }
  normalizer.finalizeMeasurement(audio, true);
  engine.resetTrigger();

  const preloaded    = engine.getPreloaded();
  const usePreloaded = preloaded && preloaded.track.id === track.id;
  if (!usePreloaded) engine.clearPreload();

  state.isLoading = true;
  updatePlayButton();
  showNowPlaying(track);

  try {
    if (usePreloaded) {
      const newAudio = engine.consumePreloaded(track.id);
      if (newAudio) { audio = newAudio; audioRef.audio = audio; }
    } else if (track.isLocal) {
      const directUrl = pathToFileUrl(track.localPath);
      engine.setSource(directUrl);
      audio = engine.getActiveAudio();
      audioRef.audio = audio;
    } else {
      const cachedPath = prefetchCache.getCachedPath(track.id);
      if (!cachedPath) showToast(I18n.t('toast.loadingTrack', { title: track.title }));
      const directUrl = cachedPath
        ? pathToFileUrl(cachedPath)
        : await window.snowify.getStreamUrl(track.url, state.audioQuality, { title: track.title, artist: track.artist }, state.songSources);
      if (gen !== _playGeneration) return;
      engine.setSource(directUrl);
      audio = engine.getActiveAudio();
      audioRef.audio = audio;
    }

    audio.volume = state.volume * engine.VOLUME_SCALE;
    await audio.play();
    if (gen !== _playGeneration) return;

    _consecutiveFailures = 0;
    state.isPlaying      = true;
    state.isLoading      = false;
    addToRecent(track);
    updateDiscordPresence(track);
    callbacks.maybeEnrichTrackMeta(track);
    renderQueue();
    updatePositionState();
    callbacks.saveState();
    engine.resetPreloadFlag();
    normalizer.analyzeAndApply(audio, audio.src, track.id);
    if (state.prefetchCount !== 0) prefetchCache.onTrackChanged(state.queueIndex, state.queue);
  } catch (err) {
    if (gen !== _playGeneration) return;
    if (err && err.name === 'AbortError') return;
    console.error('Playback error:', err && (err.name + ': ' + err.message));
    const msg = typeof err === 'string' ? err : (err.message || 'unknown error');
    showToast(I18n.t('toast.playbackFailed', { error: msg }));
    state.isPlaying  = false;
    state.isLoading  = false;
    updatePlayButton();
    updateTrackHighlight();
    _consecutiveFailures++;
    if (_consecutiveFailures < MAX_CONSECUTIVE_FAILURES) {
      const nextIdx = state.queueIndex + 1;
      if (nextIdx < state.queue.length) {
        state.queueIndex = nextIdx;
        playTrack(state.queue[nextIdx]);
        renderQueue();
      }
    } else {
      showToast(I18n.t('toast.multipleFailures'));
      _consecutiveFailures = 0;
    }
    return;
  }
  updatePlayButton();
  updateTrackHighlight();

  if (state.autoplay && state.queueIndex >= state.queue.length - 1) {
    smartQueueFill({ silent: true });
  }
}

export function playFromList(tracks, index, sourcePlaylistId = null) {
  state.playingPlaylistId = sourcePlaylistId;
  if (engine.isInProgress()) { engine.instantComplete(); audio = engine.getActiveAudio(); audioRef.audio = audio; }
  engine.clearPreload();
  prefetchCache.clear();
  state.originalQueue = [...tracks];
  if (state.shuffle) {
    const picked = tracks[index];
    const rest   = tracks.filter((_, i) => i !== index);
    for (let i = rest.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [rest[i], rest[j]] = [rest[j], rest[i]];
    }
    state.queue      = [picked, ...rest];
    state.queueIndex = 0;
  } else {
    state.queue      = [...tracks];
    state.queueIndex = index;
  }
  playTrack(state.queue[state.queueIndex]);
  renderQueue();
  updatePlaylistHighlight();
}

export function playNext({ respectRepeatOne = false } = {}) {
  if (engine.isInProgress()) { engine.instantComplete(); audio = engine.getActiveAudio(); audioRef.audio = audio; }
  if (!state.queue.length) return;

  if (respectRepeatOne && state.repeat === 'one') {
    engine.clearPreload();
    audio.currentTime = 0;
    audio.play();
    state.isPlaying = true;
    updatePlayButton();
    return;
  }

  if (state.repeat === 'all') {
    let nextIdx = state.queueIndex + 1;
    if (nextIdx >= state.queue.length) nextIdx = 0;
    state.queueIndex = nextIdx;
    playTrack(state.queue[nextIdx]);
    renderQueue();
    return;
  }

  let nextIdx = state.queueIndex + 1;
  if (nextIdx >= state.queue.length) {
    engine.clearPreload();
    if (state.autoplay) { smartQueueFill(); return; }
    state.isPlaying = false;
    updatePlayButton();
    return;
  }
  state.queueIndex = nextIdx;
  playTrack(state.queue[nextIdx]);
  renderQueue();
}

async function smartQueueFill({ silent = false } = {}) {
  if (_smartQueueFillInFlight) return;
  const current = state.queue[state.queueIndex];
  if (!current || current.isLocal) return;
  _smartQueueFillInFlight = true;

  try {
    const queueIds = new Set(state.queue.map(t => t.id));
    const seen = new Set();
    let pool = [];

    const addToPool = (tracks) => {
      tracks.forEach(t => {
        if (!queueIds.has(t.id) && !seen.has(t.id)) { seen.add(t.id); pool.push(t); }
      });
    };

    const upNexts = await window.snowify.getUpNexts(current.id);
    addToPool(upNexts);

    if (pool.length < 10 && current.artistId) {
      const info = await window.snowify.artistInfo(current.artistId);
      if (info) addToPool(info.topSongs || []);
    }

    if (!pool.length) {
      if (!silent) { state.isPlaying = false; updatePlayButton(); }
      return;
    }

    for (let i = pool.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [pool[i], pool[j]] = [pool[j], pool[i]];
    }
    const maxAdd = Math.min(20, 200 - state.queue.length);
    if (maxAdd <= 0) {
      const trim = Math.min(state.queueIndex, state.queue.length - 100);
      if (trim > 0) { state.queue.splice(0, trim); state.queueIndex -= trim; }
    }
    const newTracks = pool.slice(0, Math.max(maxAdd, 10));
    state.queue.push(...newTracks);
    if (state.prefetchCount !== 0) prefetchCache.onTrackChanged(state.queueIndex, state.queue);

    if (silent) {
      renderQueue();
    } else {
      state.queueIndex++;
      state.playingPlaylistId = null;
      updatePlaylistHighlight();
      playTrack(state.queue[state.queueIndex]);
      renderQueue();
    }
  } catch (err) {
    console.error('Autoplay error:', err);
    if (!silent) { state.isPlaying = false; updatePlayButton(); }
  } finally {
    _smartQueueFillInFlight = false;
  }
}

export function playPrev() {
  if (engine.isInProgress()) { engine.instantComplete(); audio = engine.getActiveAudio(); audioRef.audio = audio; }
  if (!state.queue.length) return;
  if (audio.currentTime > 3) { audio.currentTime = 0; renderQueue(); return; }
  engine.clearPreload();
  let prevIdx = state.queueIndex - 1;
  if (prevIdx < 0) prevIdx = 0;
  state.queueIndex = prevIdx;
  playTrack(state.queue[prevIdx]);
  renderQueue();
}

export function togglePlay() {
  if (state.isLoading) return;

  if (engine.isInProgress()) {
    if (engine.isFadePaused()) {
      engine.resumeFade();
      audio = engine.getActiveAudio(); audioRef.audio = audio;
      state.isPlaying = true;
      const track = state.queue[state.queueIndex];
      if (track) updateDiscordPresence(track);
    } else {
      engine.pauseFade();
      state.isPlaying = false;
      clearDiscordPresence();
    }
    updatePlayButton();
    updatePositionState();
    return;
  }

  if (!audio.src) {
    const track = state.queue[state.queueIndex];
    if (!track) return;
    playTrack(track);
    return;
  }
  if (audio.paused) {
    audio.play();
    state.isPlaying = true;
    const track = state.queue[state.queueIndex];
    if (track) updateDiscordPresence(track);
  } else {
    audio.pause();
    state.isPlaying = false;
    clearDiscordPresence();
  }
  updatePlayButton();
  updatePositionState();
}

// ─── Engine callbacks ────────────────────────────────────────────────────────

function handleEngineTransition(evt) {
  switch (evt.type) {
    case 'gapless-complete':
      normalizer.finalizeMeasurement(audio, false);
      audio = engine.getActiveAudio(); audioRef.audio = audio;
      showNowPlaying(evt.track);
      addToRecent(evt.track);
      updateDiscordPresence(evt.track);
      updatePositionState();
      updatePlayButton();
      updateTrackHighlight();
      renderQueue();
      callbacks.saveState();
      normalizer.analyzeAndApply(audio, audio.src, evt.track.id);
      if (state.prefetchCount !== 0) {
        const prev = state.queue[state.queueIndex - 1];
        if (prev) prefetchCache.onTrackFinished(prev.id);
        prefetchCache.onTrackChanged(state.queueIndex, state.queue);
      }
      break;
    case 'gapless-play-failed':
      playNext();
      break;
    case 'crossfade-start':
      normalizer.finalizeMeasurement(audio, false);
      showNowPlaying(evt.track);
      addToRecent(evt.track);
      updateDiscordPresence(evt.track);
      updateTrackHighlight();
      renderQueue();
      callbacks.saveState();
      normalizer.applyGain(engine.getActiveSource(), evt.track.id);
      break;
    case 'preload-ready':
      normalizer.preAnalyze(evt.url, evt.track.id);
      break;
    case 'crossfade-complete': {
      audio = engine.getActiveAudio(); audioRef.audio = audio;
      updatePositionState();
      updatePlayButton();
      if (evt.track) {
        const cached = normalizer.getCachedLUFS(evt.track.id);
        if (!cached || cached.partial) normalizer.startMeasurement(audio, evt.track.id);
      }
      if (state.prefetchCount !== 0) {
        const prev = state.queue[state.queueIndex - 1];
        if (prev) prefetchCache.onTrackFinished(prev.id);
        prefetchCache.onTrackChanged(state.queueIndex, state.queue);
      }
      break;
    }
    case 'crossfade-cancel':
      audio = engine.getActiveAudio(); audioRef.audio = audio;
      if (evt.track) { showNowPlaying(evt.track); updateTrackHighlight(); renderQueue(); }
      break;
    case 'ended-no-preload':
      normalizer.finalizeMeasurement(audio, false);
      playNext({ respectRepeatOne: true });
      break;
    case 'seeked':
      updatePositionState();
      if (state.isPlaying) {
        const track = state.queue[state.queueIndex];
        if (track) updateDiscordPresence(track);
      }
      break;
    case 'error':
      state.isPlaying = false; state.isLoading = false;
      updatePlayButton(); clearDiscordPresence();
      showToast(evt.errorMsg ? `Audio error: ${evt.errorMsg}` : I18n.t('toast.audioError'));
      if (evt.hasNext) {
        state.queueIndex = evt.nextIndex;
        playTrack(state.queue[evt.nextIndex]);
        renderQueue();
      }
      break;
  }
}

function handleEngineTimeUpdate() {
  updateProgress();
  const now = Date.now();
  if (now - lastPositionUpdate >= 1000) { lastPositionUpdate = now; updatePositionState(); }
}

// ─── Discord ─────────────────────────────────────────────────────────────────

function updateDiscordPresence(track) {
  if (!state.discordRpc || !track) return;
  if (document.body.classList.contains('radio-plugin-active')) return;
  const src    = engine.getActiveSource();
  const startMs = Date.now() - Math.floor((src.currentTime || 0) * 1000);
  const durationMs = track.durationMs || (src.duration ? Math.round(src.duration * 1000) : 0);
  const data = {
    title: track.title, artist: track.artist,
    thumbnail: track.isLocal ? '' : (track.thumbnail || ''),
    startTimestamp: startMs, videoId: track.isLocal ? '' : (track.id || ''),
  };
  if (durationMs) data.endTimestamp = startMs + durationMs;
  window.snowify.updatePresence(data);
}

function clearDiscordPresence() {
  if (!state.discordRpc) return;
  if (document.body.classList.contains('radio-plugin-active')) return;
  window.snowify.clearPresence();
}

// ─── Progress ─────────────────────────────────────────────────────────────────

const progressBar  = $('#progress-bar');
const progressFill = $('#progress-fill');

export function updateProgress() {
  const src = engine.getActiveSource();
  if (!src.duration) return;
  const pct = (src.currentTime / src.duration) * 100;
  progressFill.style.width = pct + '%';
  $('#time-current').textContent = formatTime(src.currentTime);
  $('#time-total').textContent   = _showTimeRemaining
    ? '-' + formatTime(Math.max(0, src.duration - src.currentTime))
    : formatTime(src.duration);
  if (maxNPState.open) updateMaxNPProgress();
}

progressBar.addEventListener('mousedown', (e) => { isDraggingProgress = true; seekTo(e); });
document.addEventListener('mousemove',   (e) => { if (isDraggingProgress) seekTo(e); });
document.addEventListener('mouseup',     ()  => { isDraggingProgress = false; });

$('#time-total').addEventListener('click', () => {
  _showTimeRemaining = !_showTimeRemaining;
  updateProgress();
});

setupSliderTooltip(progressBar, (pct) => formatTime(pct * (engine.getActiveSource().duration || 0)));

function seekTo(e) {
  if (engine.isInProgress()) { engine.instantComplete(); audio = engine.getActiveAudio(); audioRef.audio = audio; }
  const rect = progressBar.getBoundingClientRect();
  const pct  = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
  if (audio.duration) {
    const newTime   = pct * audio.duration;
    const remaining = audio.duration - newTime;
    if (remaining > state.crossfade) engine.resetTrigger();
    else engine.markTriggered();
    audio.currentTime        = newTime;
    progressFill.style.width = (pct * 100) + '%';
  }
}

// ─── Volume ──────────────────────────────────────────────────────────────────

const volumeSlider = $('#volume-slider');
const volumeFill   = $('#volume-fill');
const btnVolume    = $('#btn-volume');

export function setVolume(vol) {
  state.volume = Math.max(0, Math.min(1, vol));
  engine.applyVolume(state.volume);
  audio.volume = state.volume * VOLUME_SCALE;
  syncVideoAudioVolume(state.volume);
  normalizer.updateVolumeCompensation(engine.getActiveAudio());
  volumeFill.style.width = (state.volume * 100) + '%';
  const isMuted = state.volume === 0;
  $('.vol-icon', btnVolume).classList.toggle('hidden', isMuted);
  $('.vol-mute-icon', btnVolume).classList.toggle('hidden', !isMuted);
  // Sync maximized NP volume knob
  import('./now-playing.js').then(({ syncMaxNPVolume }) => syncMaxNPVolume(state.volume)).catch(() => {});
  callbacks.saveState();
}

let isDraggingVol_ = false;
volumeSlider.addEventListener('mousedown', (e) => { isDraggingVol_ = true;  _updateVolumeFromEvent(e); });
document.addEventListener('mousemove',   (e) => { if (isDraggingVol_) _updateVolumeFromEvent(e); });
document.addEventListener('mouseup',     ()  => { isDraggingVol_ = false; });

function _updateVolumeFromEvent(e) {
  const rect = volumeSlider.getBoundingClientRect();
  setVolume(Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width)));
}

btnVolume.addEventListener('click', () => {
  if (state.volume > 0) { prevVolume = state.volume; setVolume(0); }
  else setVolume(prevVolume);
});

// ─── Shuffle & repeat ────────────────────────────────────────────────────────

const btnShuffle = $('#btn-shuffle');
btnShuffle.addEventListener('click', () => {
  state.shuffle = !state.shuffle;
  btnShuffle.classList.toggle('active', state.shuffle);
  if (state.queue.length > 1) {
    const current = state.queue[state.queueIndex];
    if (state.shuffle) {
      state.originalQueue = [...state.queue];
      const rest = state.queue.filter((_, i) => i !== state.queueIndex);
      for (let i = rest.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1)); [rest[i], rest[j]] = [rest[j], rest[i]];
      }
      state.queue = [current, ...rest]; state.queueIndex = 0;
    } else {
      const idx = state.originalQueue.findIndex(t => t.id === current.id);
      state.queue = [...state.originalQueue]; state.queueIndex = idx >= 0 ? idx : 0;
    }
    engine.clearPreload(); prefetchCache.clear(); renderQueue();
  }
  callbacks.saveState();
});

const btnRepeat = $('#btn-repeat');
btnRepeat.addEventListener('click', () => {
  const modes = ['off', 'all', 'one'];
  const i = (modes.indexOf(state.repeat) + 1) % modes.length;
  state.repeat = modes[i];
  btnRepeat.classList.toggle('active', state.repeat !== 'off');
  updateRepeatButton();
  engine.clearPreload();
  callbacks.saveState();
});

export function updateRepeatButton() {
  if (state.repeat === 'one') {
    btnRepeat.title = I18n.t('player.repeatOne');
    btnRepeat.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 014-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 01-4 4H3"/><text x="12" y="15" text-anchor="middle" font-size="8" fill="currentColor" stroke="none" font-weight="bold">1</text></svg>`;
  } else if (state.repeat === 'all') {
    btnRepeat.title = I18n.t('player.repeatAll');
    btnRepeat.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 014-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 01-4 4H3"/><text x="12" y="15" text-anchor="middle" font-size="7" fill="currentColor" stroke="none" font-weight="bold">∞</text></svg>`;
  } else {
    btnRepeat.title = I18n.t('player.repeat');
    btnRepeat.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 014-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 01-4 4H3"/></svg>`;
  }
}

// ─── Playback controls ────────────────────────────────────────────────────────

$('#btn-play-pause').addEventListener('click', togglePlay);
$('#btn-next').addEventListener('click', playNext);
$('#btn-prev').addEventListener('click', playPrev);

if (window.snowify.onThumbarPrev) {
  window.snowify.onThumbarPrev(() => playPrev());
  window.snowify.onThumbarPlayPause(() => togglePlay());
  window.snowify.onThumbarNext(() => playNext());
}

// ─── Now-playing bar ─────────────────────────────────────────────────────────

export function showNowPlaying(track) {
  const bar = $('#now-playing-bar');
  bar.classList.remove('hidden');
  bar.dataset.trackId     = track.id    || '';
  bar.dataset.trackUrl    = track.url   || '';
  bar.dataset.trackTitle  = track.title  || '';
  bar.dataset.trackArtist = track.artist || '';
  appEl.classList.remove('no-player');

  $('#np-thumbnail').src = resolveImageUrl(track.thumbnail || (track.isLocal ? LOCAL_THUMB_FALLBACK : ''));

  const ambientSrc = resolveImageUrl(track.thumbnail);
  if (ambientSrc) {
    extractDominantColor({ src: ambientSrc }).then(color => {
      const rgb = color ? `${color.r}, ${color.g}, ${color.b}` : '170, 85, 230';
      document.documentElement.style.setProperty('--ambient-rgb', rgb);
    });
  } else {
    document.documentElement.style.setProperty('--ambient-rgb', '170, 85, 230');
  }

  const npTitle = $('#np-title');
  npTitle.textContent = track.title;
  if (track.albumId) {
    npTitle.classList.add('clickable');
    npTitle.onclick = () => showAlbumDetail(track.albumId, { name: track.album, thumbnail: track.thumbnail });
  } else {
    npTitle.classList.remove('clickable');
    npTitle.onclick = null;
  }

  const npArtist = $('#np-artist');
  npArtist.textContent = track.artist || '';
  if (!IS_MOBILE_RUNTIME && track.artistId) {
    npArtist.classList.add('clickable');
    npArtist.onclick = () => openArtistPage(track.artistId);
  } else {
    npArtist.classList.remove('clickable');
    npArtist.onclick = null;
  }

  const isLiked = state.likedSongs.some(t => t.id === track.id);
  $('#np-like').classList.toggle('liked', isLiked);

  updateMediaSession(track);
  onTrackChanged(track);
}

export function updatePlayButton() {
  const playIcon  = $('.icon-play',  $('#btn-play-pause'));
  const pauseIcon = $('.icon-pause', $('#btn-play-pause'));
  if (state.isPlaying) { playIcon.classList.add('hidden'); pauseIcon.classList.remove('hidden'); }
  else                 { playIcon.classList.remove('hidden'); pauseIcon.classList.add('hidden'); }
  document.body.classList.toggle('audio-playing', state.isPlaying);
  if (window.snowify.updateThumbar) window.snowify.updateThumbar(state.isPlaying);
  if ('mediaSession' in navigator) navigator.mediaSession.playbackState = state.isPlaying ? 'playing' : 'paused';
  syncViewPlayAllBtns();
  if (maxNPState.open) syncMaxNPControls();
  updatePlaylistHighlight();
}

function syncViewPlayAllBtns() {
  const playAllBtn   = $('#btn-play-all');
  const albumPlayBtn = $('#btn-album-play-all');
  const artistPlayBtn = $('#btn-artist-play-all');
  if (playAllBtn) {
    const playing = state.playingPlaylistId && state.playingPlaylistId === state.currentPlaylistId && state.isPlaying;
    playAllBtn.innerHTML = playing ? PAUSE_SVG : PLAY_SVG;
    playAllBtn.title     = playing ? I18n.t('player.pause') : I18n.t('playlist.playAll');
  }
  if (albumPlayBtn) {
    const isActive = state.queue.length > 0 && !state.playingPlaylistId && state.isPlaying;
    albumPlayBtn.innerHTML = isActive ? PAUSE_SVG : PLAY_SVG;
    albumPlayBtn.title     = isActive ? I18n.t('player.pause') : I18n.t('playlist.playAll');
  }
  if (artistPlayBtn) {
    const isActive = state.queue.length > 0 && !state.playingPlaylistId && state.isPlaying;
    artistPlayBtn.innerHTML = isActive ? PAUSE_SVG : PLAY_SVG;
    artistPlayBtn.title     = isActive ? I18n.t('player.pause') : I18n.t('playlist.playAll');
  }
}

export function updateTrackHighlight() {
  const current = state.queue[state.queueIndex];
  $$('.track-row').forEach(row => {
    row.classList.toggle('playing', !!current && row.dataset.trackId === current.id);
  });
  updatePlaylistHighlight();
}

export function updatePlaylistHighlight() {
  $$('.playlist-item').forEach(item => {
    const isPlaying = state.isPlaying && state.playingPlaylistId != null && item.dataset.playlist === state.playingPlaylistId;
    const wasPlaying = item.classList.contains('playing');
    item.classList.toggle('playing', isPlaying);
    if (isPlaying && !wasPlaying) {
      const icon = item.querySelector('.now-playing-icon');
      if (icon) { icon.classList.remove('animate-waves'); void icon.offsetWidth; icon.classList.add('animate-waves'); }
    }
    const overlay = item.querySelector('.playlist-cover-overlay');
    if (overlay) overlay.innerHTML = isPlaying ? SIDEBAR_PAUSE_SVG : SIDEBAR_PLAY_SVG;
  });
}

export function isCollectionPlaying(tracks, sourcePlaylistId) {
  if (!state.queue.length || !tracks.length) return false;
  if (sourcePlaylistId && state.playingPlaylistId === sourcePlaylistId) return true;
  if (!sourcePlaylistId && state.queue[0]?.id === tracks[0]?.id && state.queue.length === tracks.length) return true;
  return false;
}

export function updatePlayAllBtn(btn, tracks, sourcePlaylistId) {
  if (!btn) return;
  const playing = isCollectionPlaying(tracks, sourcePlaylistId) && state.isPlaying;
  btn.innerHTML = playing ? PAUSE_SVG : PLAY_SVG;
  btn.title     = playing ? I18n.t('player.pause') : I18n.t('playlist.playAll');
}

// ─── Media Session ────────────────────────────────────────────────────────────

function updateMediaSession(track) {
  if (!('mediaSession' in navigator)) return;
  navigator.mediaSession.metadata = new MediaMetadata({
    title: track.title, artist: track.artist,
    artwork: [
      { src: track.thumbnail, sizes: '96x96',   type: 'image/jpeg' },
      { src: track.thumbnail, sizes: '256x256', type: 'image/jpeg' },
      { src: track.thumbnail, sizes: '512x512', type: 'image/jpeg' },
    ],
  });
  navigator.mediaSession.setActionHandler('play', togglePlay);
  navigator.mediaSession.setActionHandler('pause', togglePlay);
  navigator.mediaSession.setActionHandler('previoustrack', playPrev);
  navigator.mediaSession.setActionHandler('nexttrack', playNext);
  navigator.mediaSession.setActionHandler('seekto', (details) => {
    if (engine.isInProgress()) { engine.instantComplete(); audio = engine.getActiveAudio(); audioRef.audio = audio; }
    if (audio.duration) {
      audio.currentTime = details.seekTime;
      const remaining   = audio.duration - details.seekTime;
      if (remaining > state.crossfade) engine.resetTrigger(); else engine.markTriggered();
      updatePositionState();
    }
  });
}

export function updatePositionState() {
  if (!('mediaSession' in navigator)) return;
  const src = engine.getActiveSource();
  if (!src.duration || !isFinite(src.duration)) return;
  try {
    navigator.mediaSession.setPositionState({
      duration: src.duration, playbackRate: src.playbackRate,
      position: Math.min(src.currentTime, src.duration),
    });
  } catch (_) {}
}

// ─── Like / heart effects ─────────────────────────────────────────────────────

const npLike = $('#np-like');
npLike.addEventListener('click', () => {
  const track = state.queue[state.queueIndex];
  if (track) {
    const wasLiked = toggleLike(track);
    if (wasLiked) spawnHeartParticles(npLike); else spawnBrokenHeart(npLike);
    const liked = state.likedSongs.some(t => t.id === track.id);
    $('#max-np-like').classList.toggle('liked', liked);
  }
});

export function toggleLike(track) {
  const idx = state.likedSongs.findIndex(t => t.id === track.id);
  if (idx >= 0) {
    state.likedSongs.splice(idx, 1);
    showToast(I18n.t('toast.removedFromLiked'));
  } else {
    state.likedSongs.push(track);
    showToast(I18n.t('toast.addedToLiked'));
  }
  callbacks.saveState();
  updateLikedCount();
  const current = state.queue[state.queueIndex];
  if (current?.id === track.id) {
    npLike.classList.toggle('liked', state.likedSongs.some(t => t.id === track.id));
  }
  return idx < 0; // true = liked
}

export function spawnHeartParticles(originEl) {
  const rect = originEl.getBoundingClientRect();
  const cx = rect.left + rect.width / 2, cy = rect.top + rect.height / 2;
  for (let i = 0; i < 7; i++) {
    const heart  = document.createElement('div');
    heart.className  = 'heart-particle';
    heart.textContent = '\u2764';
    const angle = (Math.PI * 2 * i) / 7 + (Math.random() - 0.5) * 0.6;
    const dist  = 20 + Math.random() * 25;
    heart.style.left = cx + 'px'; heart.style.top = cy + 'px';
    heart.style.setProperty('--dx', (Math.cos(angle) * dist) + 'px');
    heart.style.setProperty('--dy', (Math.sin(angle) * dist - 15) + 'px');
    heart.style.setProperty('--s', 0.6 + Math.random() * 0.5);
    document.body.appendChild(heart);
    heart.addEventListener('animationend', () => heart.remove());
  }
}

export function spawnBrokenHeart(originEl) {
  const rect = originEl.getBoundingClientRect();
  const el   = document.createElement('div');
  el.className = 'broken-heart';
  el.innerHTML = ICON_BROKEN_HEART;
  el.style.left = rect.left + rect.width / 2 + 'px';
  el.style.top  = rect.top  + rect.height / 2 + 'px';
  document.body.appendChild(el);
  el.addEventListener('animationend', () => el.remove());
}

const _likedCountEl = document.querySelector('[data-playlist="liked"] .playlist-count');
export function updateLikedCount() {
  if (_likedCountEl) _likedCountEl.textContent = I18n.tp('sidebar.songCount', state.likedSongs.length);
}
