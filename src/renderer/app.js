import state from './modules/state.js';
import { showToast, escapeHtml } from './modules/utils.js';
import { isCustomTheme, applyTheme, populateCustomThemes } from './modules/theme.js';
import { audioRef, VOLUME_SCALE } from './modules/audio-ref.js';
import { callbacks } from './modules/callbacks.js';
import { closeLyricsPanel } from './modules/lyrics.js';
import { extractDominantColor, openMaxNP, closeMaxNP } from './modules/now-playing.js';
import { setVolume, togglePlay, playNext, playPrev, playTrack, updateRepeatButton, showNowPlaying, getPrefetchCache } from './modules/player.js';
import { renderQueue } from './modules/queue.js';
import { renderPlaylists, renderLibrary } from './modules/library.js';
import { renderHome } from './modules/home.js';
import { renderExplore } from './modules/explore.js';
import { showAlbumDetail } from './modules/album.js';
import { openArtistPage } from './modules/artist.js';
import { syncSearchHint, closeSuggestions } from './modules/search.js';
import { initSettings, settingsCallbacks, resetSettingsInitialized } from './modules/settings.js';
import { loadEnabledPlugins } from './modules/plugins.js';

'use strict';

const $ = (sel, ctx = document) => ctx.querySelector(sel);
const $$ = (sel, ctx = document) => [...ctx.querySelectorAll(sel)];
const IS_MOBILE_RUNTIME =
  window.snowify?.platform === 'android' ||
  window.snowify?.platform === 'ios' ||
  document.documentElement.classList.contains('platform-mobile') ||
  /Android|iPhone|iPad|iPod/i.test(navigator.userAgent || '');

function resolveImageUrl(url) {
  if (!url) return url;
  return window.snowify?.resolveImageUrl?.(url) || deproxyUrl(url);
}

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

function normalizeForCloud(value) {
  if (Array.isArray(value)) return value.map(normalizeForCloud);
  if (value && typeof value === 'object') {
    const out = {};
    for (const [k, v] of Object.entries(value)) out[k] = normalizeForCloud(v);
    return out;
  }
  if (typeof value === 'string') return deproxyUrl(value);
  return value;
}

// ─── Throttled image loader — prevents 429 from simultaneous thumbnail requests ───
// Images use `data-src` in templates; a MutationObserver feeds them into this queue.
const _imgQ = (() => {
  const CONCURRENCY = IS_MOBILE_RUNTIME ? 1 : 2; // mobile is more aggressive with per-host throttling
  const RETRY_MS = IS_MOBILE_RUNTIME ? [4000, 11000, 22000] : [2500, 7000, 18000];
  const START_GAP = IS_MOBILE_RUNTIME ? 180 : 80;
  let _active = 0;
  let _drainPending = false;
  const _queue = [];
  const _queued = new Set();
  const _loaded = new Set();                // URLs known to be loaded successfully
  const _inFlight = new Map();              // src -> { els:Set<HTMLImageElement>, attempt:number }
  let _errorStreak = 0;
  let _resumeAt = 0;

  const _jitter = ms => ms * (0.75 + Math.random() * 0.5); // ±25% jitter

  function _applySrc(el, src) {
    if (!el || !el.isConnected) return;
    if (el.dataset.src === src) el.removeAttribute('data-src');
    const resolvedSrc = resolveImageUrl(src);
    if (el.getAttribute('src') === resolvedSrc || el.currentSrc === resolvedSrc) return;
    el.loading = 'lazy';
    el.decoding = 'async';
    el.src = resolvedSrc;
  }

  function _drain() {
    _drainPending = false;
    if (_resumeAt > Date.now()) {
      if (!_drainPending) {
        _drainPending = true;
        setTimeout(_drain, Math.max(40, _resumeAt - Date.now()));
      }
      return;
    }
    if (_active >= CONCURRENCY || !_queue.length) return;
    const next = _queue.shift();
    if (!next) return;
    _queued.delete(next.src);
    _start(next);
    // Schedule the next slot after a small gap to prevent simultaneous burst
    if (!_drainPending && _queue.length && _active < CONCURRENCY) {
      _drainPending = true;
      setTimeout(_drain, START_GAP);
    }
  }

  function _start({ src, attempt }) {
    _active++;
    const probe = new Image();
    probe.decoding = 'async';
    const resolvedSrc = resolveImageUrl(src);
    probe.onload = () => {
      _active--;
      _errorStreak = 0;
      _resumeAt = 0;
      _loaded.add(src);
      const entry = _inFlight.get(src);
      _inFlight.delete(src);
      entry?.els.forEach(el => _applySrc(el, src));
      _drain();
    };
    probe.onerror = () => {
      _active--;
      _errorStreak = Math.min(10, _errorStreak + 1);
      if (IS_MOBILE_RUNTIME) {
        // Brief global cooldown helps avoid repeated host throttling bursts.
        const cooldown = Math.min(12000, 1200 + (_errorStreak * 900));
        _resumeAt = Math.max(_resumeAt, Date.now() + cooldown);
      }
      const entry = _inFlight.get(src);
      _inFlight.delete(src);
      const liveEls = [...(entry?.els || [])].filter(el => el && el.isConnected);
      if (attempt < RETRY_MS.length && liveEls.length) {
        setTimeout(() => {
          if (!liveEls.some(el => el.isConnected)) return;
          _inFlight.set(src, { els: new Set(liveEls.filter(el => el.isConnected)), attempt: attempt + 1 });
          if (!_queued.has(src)) {
            _queued.add(src);
            _queue.push({ src, attempt: attempt + 1 });
          }
          _drain();
        }, _jitter(RETRY_MS[attempt]));
      } else if (liveEls.length) {
        // Keep slow-retrying every ~18s indefinitely instead of giving up
        setTimeout(() => {
          const stillLive = liveEls.filter(el => el.isConnected);
          if (!stillLive.length) return;
          if (_loaded.has(src)) { stillLive.forEach(el => _applySrc(el, src)); return; }
          _inFlight.set(src, { els: new Set(stillLive), attempt: RETRY_MS.length - 1 });
          if (!_queued.has(src)) {
            _queued.add(src);
            _queue.push({ src, attempt: RETRY_MS.length - 1 });
          }
          _drain();
        }, _jitter(RETRY_MS[RETRY_MS.length - 1]));
      }
      _drain();
    };
    probe.src = resolvedSrc;
  }

  // IntersectionObserver: only enqueue when image enters extended viewport.
  // 300px root margin means images start loading just before scrolling into view.
  // This prevents the burst of 40+ simultaneous requests on track list renders.
  const _io = new IntersectionObserver(entries => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const el = entry.target;
      _io.unobserve(el);
      const src = el.dataset.src;
      if (!src) continue;
      if (el.getAttribute('src') === src || el.currentSrc === src) {
        el.removeAttribute('data-src');
        _loaded.add(src);
        continue;
      }
      if (_loaded.has(src)) {
        _applySrc(el, src);
        continue;
      }
      const existing = _inFlight.get(src);
      if (existing) {
        existing.els.add(el);
        el.removeAttribute('data-src');
        continue;
      }
      el.removeAttribute('data-src');
      _inFlight.set(src, { els: new Set([el]), attempt: 0 });
      if (!_queued.has(src)) {
        _queued.add(src);
        _queue.push({ src, attempt: 0 });
      }
      _drain();
    }
  }, { rootMargin: '300px 0px' });

  return {
    enqueue(el) {
      if (!el.dataset.src) return;
      const src = el.dataset.src;
      if (!src) return;
      if (el.getAttribute('src') === src || el.currentSrc === src || _loaded.has(src)) {
        _applySrc(el, src);
        return;
      }
      const existing = _inFlight.get(src);
      if (existing) {
        existing.els.add(el);
        el.removeAttribute('data-src');
        return;
      }
      if (_queued.has(src)) {
        _io.observe(el);
        return;
      }
      _io.observe(el); // defer until near viewport
    }
  };
})();

// Auto-process any img[data-src] inserted into the DOM
new MutationObserver(muts => {
  for (const m of muts)
    for (const n of m.addedNodes) {
      if (n.nodeType !== 1) continue;
      if (n.tagName === 'IMG' && n.dataset.src) _imgQ.enqueue(n);
      else n.querySelectorAll?.('img[data-src]').forEach(e => _imgQ.enqueue(e));
    }
}).observe(document.documentElement, { childList: true, subtree: true });

// ─── Auto marquee for truncated titles/artists ───
const _MARQUEE_SELECTORS = [
  '.np-title',
  '.np-artist',
  '.max-np-title',
  '.max-np-artist',
  '.max-np-topbar-title',
  '.max-np-topbar-artist',
  '.track-title',
  '.track-artist-col',
  '.card-title',
  '.card-artist',
  '.queue-item-title',
  '.queue-item-artist',
  '.suggestion-title',
  '.suggestion-subtitle',
  '.search-suggestion-text',
  '.playlist-name',
  '.album-card-name',
  '.album-card-meta',
  '.lib-card-name',
  '.video-card-name',
  '.top-song-title',
  '.top-song-artist',
  '.similar-artist-name'
].join(', ');

let _marqueeRefreshRAF = 0;

function _applyAutoMarquee(el) {
  if (!el || !el.isConnected) return;
  el.classList.add('auto-marquee-target');

  let inner = el.querySelector(':scope > .auto-marquee-inner');
  if (!inner) {
    inner = document.createElement('span');
    inner.className = 'auto-marquee-inner';
    while (el.firstChild) inner.appendChild(el.firstChild);
    el.appendChild(inner);
  }

  // Hidden/collapsed elements should not animate.
  if (el.offsetParent === null || el.clientWidth <= 0) {
    el.classList.remove('auto-marquee-active');
    return;
  }

  const overflowPx = Math.ceil(inner.scrollWidth - el.clientWidth);
  if (overflowPx > 12) {
    const distance = Math.min(overflowPx + 18, 680);
    const duration = Math.max(6, Math.min(18, distance / 22));
    el.style.setProperty('--marquee-distance', `${distance}px`);
    el.style.setProperty('--marquee-duration', `${duration}s`);
    el.classList.add('auto-marquee-active');
  } else {
    el.classList.remove('auto-marquee-active');
    el.style.removeProperty('--marquee-distance');
    el.style.removeProperty('--marquee-duration');
  }
}

function refreshAutoMarquee() {
  _marqueeRefreshRAF = 0;
  if (window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    document.querySelectorAll('.auto-marquee-active').forEach(el => el.classList.remove('auto-marquee-active'));
    return;
  }
  document.querySelectorAll(_MARQUEE_SELECTORS).forEach(_applyAutoMarquee);
}

function scheduleAutoMarqueeRefresh() {
  if (_marqueeRefreshRAF) return;
  _marqueeRefreshRAF = requestAnimationFrame(refreshAutoMarquee);
}

new MutationObserver(() => {
  scheduleAutoMarqueeRefresh();
}).observe(document.documentElement, {
  childList: true,
  subtree: true,
  characterData: true
});

window.addEventListener('resize', scheduleAutoMarqueeRefresh);
window.addEventListener('orientationchange', scheduleAutoMarqueeRefresh);
setInterval(scheduleAutoMarqueeRefresh, 4000);
setTimeout(scheduleAutoMarqueeRefresh, 250);

  const appEl = $('#app');

  const views = $$('.view');
  const navBtns = $$('.nav-btn');

  $('#btn-minimize').onclick = () => window.snowify.minimize();
  $('#btn-maximize').onclick = () => window.snowify.maximize();
  $('#btn-close').onclick = () => window.snowify.close();

  let _cloudSaveTimeout = null;
  let _cloudUser = null;
  let _cloudSyncPaused = false;
  let _cloudLastPayloadHash = null;
  let _cloudLastSentAt = 0;
  let _cloudLastErrorToastAt = 0;
  let _resetEmailLastSent = 0;
  let _welcomeDismissed = false;
  const RESET_COOLDOWN_MS = 60000;
  const CLOUD_SAVE_DEBOUNCE_MS = 12000;
  const CLOUD_SAVE_MIN_INTERVAL_MS = 30000;
  const WELCOME_SEEN_KEY = 'snowify_welcome_seen_v2';

  function shouldShowWelcome() {
    if (_welcomeDismissed || _cloudUser) return false;
    try {
      return localStorage.getItem(WELCOME_SEEN_KEY) !== '1';
    } catch {
      return true;
    }
  }

  function hideWelcomeOverlay({ remember = true } = {}) {
    const overlay = $('#welcome-overlay');
    if (!overlay) return;
    _welcomeDismissed = true;
    if (remember) {
      try { localStorage.setItem(WELCOME_SEEN_KEY, '1'); } catch {}
    }
    overlay.classList.add('fade-out');
    setTimeout(() => {
      overlay.classList.add('hidden');
      overlay.classList.remove('fade-out');
    }, 380);
  }

  function showWelcomeOverlay() {
    const overlay = $('#welcome-overlay');
    if (!overlay || !shouldShowWelcome()) return;
    const errorEl = $('#welcome-auth-error');
    if (errorEl) {
      errorEl.style.color = '';
      errorEl.classList.add('hidden');
      errorEl.textContent = '';
    }
    overlay.classList.remove('hidden');
  }

  function setWelcomeError(message, isAccent = false) {
    const errorEl = $('#welcome-auth-error');
    if (!errorEl) return;
    errorEl.style.color = isAccent ? 'var(--accent)' : '';
    errorEl.textContent = message;
    errorEl.classList.remove('hidden');
  }

  function updateSyncStatus(text) {
    const el = $('#account-sync-status');
    if (el) el.textContent = text;
  }

  function _buildCloudPayload() {
    const stripLocal = (tracks = []) => tracks.filter(t => !t?.isLocal);
    return normalizeForCloud({
      playlists: state.playlists.map(p => ({ ...p, tracks: stripLocal(p.tracks) })),
      likedSongs: stripLocal(state.likedSongs),
      recentTracks: stripLocal(state.recentTracks),
      followedArtists: state.followedArtists,
      volume: state.volume,
      shuffle: state.shuffle,
      repeat: state.repeat,
      musicOnly: state.musicOnly,
      autoplay: state.autoplay,
      audioQuality: state.audioQuality,
      videoQuality: state.videoQuality,
      videoPremuxed: state.videoPremuxed,
      animations: state.animations,
      effects: state.effects,
      theme: isCustomTheme(state.theme) ? 'dark' : state.theme,
      discordRpc: state.discordRpc,
      country: state.country,
      crossfade: state.crossfade,
      normalization: state.normalization,
      normalizationTarget: state.normalizationTarget,
      prefetchCount: state.prefetchCount,
      searchHistory: state.searchHistory,
      songSources: state.songSources,
      metadataSources: state.metadataSources,
      wrappedShownYear: state.wrappedShownYear,
      showListeningActivity: state.showListeningActivity,
      minimizeToTray: state.minimizeToTray,
      launchOnStartup: state.launchOnStartup,
    });
  }

  function _hashPayload(payload) {
    try {
      return JSON.stringify(payload);
    } catch {
      return '';
    }
  }

  async function _performCloudSave({ force = false } = {}) {
    if (!_cloudUser || _cloudSyncPaused) return false;

    const payload = _buildCloudPayload();
    const payloadHash = _hashPayload(payload);
    const now = Date.now();

    if (!force && payloadHash && payloadHash === _cloudLastPayloadHash) return false;
    // NOTE: No minimum-interval gate here — the 12s debounce in cloudSaveDebounced
    // already throttles writes.  A hard interval would silently drop pending saves
    // (no retry is scheduled when the check fires) which loses user data on mobile.

    const result = await window.snowify.cloudSave(payload);
    if (result?.error) {
      console.error('Cloud save failed:', result.error);
      const nowTs = Date.now();
      if (nowTs - _cloudLastErrorToastAt > 15000) {
        _cloudLastErrorToastAt = nowTs;
        showToast(`Cloud sync failed: ${result.error}`);
      }
      return false;
    }

    _cloudLastPayloadHash = payloadHash;
    _cloudLastSentAt = now;
    updateSyncStatus(I18n.t('sync.syncedJustNow'));
    return true;
  }

  function cloudSaveDebounced() {
    if (!_cloudUser || _cloudSyncPaused) return;
    clearTimeout(_cloudSaveTimeout);
    _cloudSaveTimeout = setTimeout(() => {
      _cloudSaveTimeout = null;
      _performCloudSave().catch(() => {});
    }, CLOUD_SAVE_DEBOUNCE_MS);
  }

  async function forceCloudSave() {
    if (_cloudSaveTimeout) {
      clearTimeout(_cloudSaveTimeout);
      _cloudSaveTimeout = null;
    }
    return _performCloudSave({ force: true });
  }

  async function cloudLoadAndMerge({ forceCloud = false } = {}) {
    const cloudRaw = await window.snowify.cloudLoad();
    if (!cloudRaw) return false;
    const cloud = normalizeForCloud(cloudRaw);

    const localTime = parseInt(localStorage.getItem('snowify_lastSave') || '0', 10);
    const shouldApply = forceCloud || (cloud.updatedAt && cloud.updatedAt > localTime);
    if (!shouldApply) return false;

    const localTracksByPlaylist = new Map();
    for (const p of state.playlists) {
      const locals = (p.tracks || []).filter(t => t.isLocal);
      if (locals.length) localTracksByPlaylist.set(p.id, locals);
    }
    const localLiked = state.likedSongs.filter(t => t.isLocal);
    const localRecent = state.recentTracks.filter(t => t.isLocal);

    state.playlists = cloud.playlists || state.playlists;
    state.likedSongs = cloud.likedSongs || state.likedSongs;
    state.recentTracks = cloud.recentTracks || state.recentTracks;
    state.followedArtists = cloud.followedArtists || state.followedArtists;

    for (const p of state.playlists) {
      const locals = localTracksByPlaylist.get(p.id);
      if (!locals?.length) continue;
      const ids = new Set((p.tracks || []).map(t => t.id));
      for (const lt of locals) if (!ids.has(lt.id)) p.tracks.push(lt);
    }
    if (localLiked.length) {
      const likedIds = new Set(state.likedSongs.map(t => t.id));
      for (const lt of localLiked) if (!likedIds.has(lt.id)) state.likedSongs.push(lt);
    }
    if (localRecent.length) {
      const recentIds = new Set(state.recentTracks.map(t => t.id));
      for (const lt of localRecent) {
        if (!recentIds.has(lt.id)) state.recentTracks.unshift(lt);
      }
      if (state.recentTracks.length > 20) state.recentTracks = state.recentTracks.slice(0, 20);
    }

    state.volume = cloud.volume ?? state.volume;
    state.shuffle = cloud.shuffle ?? state.shuffle;
    state.repeat = cloud.repeat || state.repeat;
    state.musicOnly = cloud.musicOnly ?? state.musicOnly;
    state.autoplay = cloud.autoplay ?? state.autoplay;
    state.audioQuality = cloud.audioQuality || state.audioQuality;
    state.videoQuality = cloud.videoQuality || state.videoQuality;
    state.videoPremuxed = cloud.videoPremuxed ?? state.videoPremuxed;
    state.animations = cloud.animations ?? state.animations;
    state.effects = cloud.effects ?? state.effects;
    state.miniplayerGlow = cloud.miniplayerGlow ?? state.miniplayerGlow;
    if (cloud.theme && !isCustomTheme(cloud.theme) && !isCustomTheme(state.theme)) {
      state.theme = cloud.theme;
    }
    state.discordRpc = cloud.discordRpc ?? state.discordRpc;
    state.country = cloud.country || state.country;
    state.crossfade = cloud.crossfade ?? state.crossfade;
    state.normalization = cloud.normalization ?? state.normalization;
    state.normalizationTarget = cloud.normalizationTarget ?? state.normalizationTarget;
    state.prefetchCount = cloud.prefetchCount ?? state.prefetchCount;
    state.searchHistory = cloud.searchHistory || state.searchHistory;
    state.songSources = cloud.songSources || state.songSources;
    state.metadataSources = cloud.metadataSources || state.metadataSources;
    state.wrappedShownYear = cloud.wrappedShownYear ?? state.wrappedShownYear;
    state.showListeningActivity = cloud.showListeningActivity ?? state.showListeningActivity;
    state.minimizeToTray = cloud.minimizeToTray ?? state.minimizeToTray;
    state.launchOnStartup = cloud.launchOnStartup ?? state.launchOnStartup;

    _cloudSyncPaused = true;
    saveState();
    _cloudSyncPaused = false;

    renderPlaylists();
    renderHome();
    applyTheme(state.theme);

    const aq = $('#setting-quality'); if (aq) aq.value = state.audioQuality;
    const vq = $('#setting-video-quality'); if (vq) vq.value = state.videoQuality;
    const at = $('#setting-autoplay'); if (at) at.checked = state.autoplay;
    const vp = $('#setting-video-premuxed'); if (vp) vp.checked = state.videoPremuxed;
    const an = $('#setting-animations'); if (an) an.checked = state.animations;
    const ef = $('#setting-effects'); if (ef) ef.checked = state.effects;
    const dr = $('#setting-discord-rpc'); if (dr) dr.checked = state.discordRpc;
    const co = $('#setting-country'); if (co) co.value = state.country || '';
    const ts = $('#theme-select'); if (ts) await populateCustomThemes(ts, state.theme);
    const cft = $('#setting-crossfade-toggle'); if (cft) cft.checked = state.crossfade > 0;
    const cfsl = $('#crossfade-slider-row'); if (cfsl) cfsl.classList.toggle('hidden', state.crossfade <= 0);
    const cff = $('#crossfade-fill');
    const cfvl = $('#crossfade-value');
    if (cff && cfvl) {
      const v = state.crossfade > 0 ? state.crossfade : 5;
      cff.style.width = ((v - 1) / (audioRef.engine.CROSSFADE_MAX - 1) * 100) + '%';
      cfvl.textContent = I18n.t('settings.seconds', { value: v });
    }
    const nt = $('#setting-normalization'); if (nt) nt.checked = state.normalization;
    const ntr = $('#normalization-target-row'); if (ntr) ntr.classList.toggle('hidden', !state.normalization);
    const nts = $('#setting-normalization-target'); if (nts) nts.value = String(state.normalizationTarget);
    const pfc = $('#setting-prefetch-count'); if (pfc) pfc.value = String(state.prefetchCount);
    const mtt = $('#setting-minimize-to-tray'); if (mtt) mtt.checked = state.minimizeToTray;
    const los = $('#setting-launch-on-startup'); if (los) los.checked = state.launchOnStartup;

    if (state.country) window.snowify.setCountry(state.country);
    document.documentElement.classList.toggle('no-animations', !state.animations);
    document.documentElement.classList.toggle('no-effects', !state.effects);
    document.documentElement.classList.toggle('no-miniplayer-glow', !state.miniplayerGlow);
    audioRef.engine.applyVolume(state.volume);
    audioRef.audio.volume = state.volume * VOLUME_SCALE;
    showToast(I18n.t('toast.syncedFromCloud'));
    return true;
  }

  async function loadProfileExtras() {
    if (!_cloudUser) return;
    const bannerPreview = $('#profile-banner-preview');
    const btnRemoveBanner = $('#btn-remove-banner');
    const bioInput = $('#profile-bio-input');
    const bioCount = $('#profile-bio-count');
    if (!bannerPreview || !bioInput || !bioCount || !btnRemoveBanner) return;
    try {
      const profile = await window.snowify.getProfile(_cloudUser.uid);
      if (!profile) {
        bannerPreview.innerHTML = `<span class="profile-banner-placeholder">${escapeHtml(I18n.t('settings.noBanner'))}</span>`;
        btnRemoveBanner.style.display = 'none';
        bioInput.value = '';
        bioCount.textContent = '0/200';
        return;
      }
      if (profile.banner) {
        bannerPreview.innerHTML = `<img src="${escapeHtml(profile.banner)}" alt="" draggable="false" />`;
        btnRemoveBanner.style.display = '';
      } else {
        bannerPreview.innerHTML = `<span class="profile-banner-placeholder">${escapeHtml(I18n.t('settings.noBanner'))}</span>`;
        btnRemoveBanner.style.display = 'none';
      }

      const bio = profile.bio || '';
      bioInput.value = bio;
      bioCount.textContent = `${bio.length}/200`;
    } catch (_) {}
  }

  function updateAccountUI(user) {
    _cloudUser = user;
    const signedOut = $('#account-signed-out');
    const signedIn = $('#account-signed-in');
    const profileEmail = $('#profile-email');
    const profileName = $('#profile-display-name');
    const profileAvatar = $('#profile-avatar');

    if (user) {
      hideWelcomeOverlay({ remember: true });
      signedOut?.classList.add('hidden');
      signedIn?.classList.remove('hidden');
      if (profileName) profileName.textContent = user.displayName || I18n.t('common.user');
      if (profileEmail) profileEmail.textContent = user.email || '';
      if (profileAvatar) {
        profileAvatar.src = user.photoURL || generateDefaultAvatar(user.displayName || user.email || 'U');
      }
      updateSyncStatus(I18n.t('sync.connected'));
      loadProfileExtras();
    } else {
      signedOut?.classList.remove('hidden');
      signedIn?.classList.add('hidden');
      if (profileEmail) profileEmail.textContent = '';
      if (profileName) profileName.textContent = I18n.t('common.user');
      if (profileAvatar) profileAvatar.src = '';
      updateSyncStatus('');
      showWelcomeOverlay();
    }
  }

  let _saveStateTimer = null;
  function saveState() {
    if (_saveStateTimer) return; // already scheduled
    _saveStateTimer = setTimeout(() => {
      _saveStateTimer = null;
      _flushSaveState();
      cloudSaveDebounced();
    }, 300);
  }
  function _flushSaveState() {
    if (_saveStateTimer) { clearTimeout(_saveStateTimer); _saveStateTimer = null; }
    try {
      localStorage.setItem('snowify_state', JSON.stringify({
        playlists: state.playlists,
        likedSongs: state.likedSongs,
        recentTracks: state.recentTracks,
        followedArtists: state.followedArtists,
        volume: state.volume,
        shuffle: state.shuffle,
        repeat: state.repeat,
        musicOnly: state.musicOnly,
        autoplay: state.autoplay,
        audioQuality: state.audioQuality,
        videoQuality: state.videoQuality,
        videoPremuxed: state.videoPremuxed,
        animations: state.animations,
        effects: state.effects,
        theme: state.theme,
        discordRpc: state.discordRpc,
        country: state.country,
        searchHistory: state.searchHistory,
        crossfade: state.crossfade,
        normalization: state.normalization,
        normalizationTarget: state.normalizationTarget,
        prefetchCount: state.prefetchCount,
        showListeningActivity: state.showListeningActivity,
        minimizeToTray: state.minimizeToTray,
        launchOnStartup: state.launchOnStartup,
        songSources: state.songSources,
        metadataSources: state.metadataSources,
        wrappedShownYear: state.wrappedShownYear,
      }));
      localStorage.setItem('snowify_lastSave', String(Date.now()));
    } catch (e) {
      console.error('State save failed (storage quota exceeded):', e);
    }
    // Queue persistence (local-only)
    try {
      localStorage.setItem('snowify_queue', JSON.stringify({
        queue: state.queue,
        originalQueue: state.originalQueue,
        queueIndex: state.queueIndex,
        playingPlaylistId: state.playingPlaylistId
      }));
    } catch (e) {
      console.error('Queue save failed (storage quota exceeded):', e);
    }
    // Play log — stored separately to avoid bloating the main state key
    try {
      localStorage.setItem('snowify_play_log', JSON.stringify(state.playLog));
    } catch (e) {
      console.warn('Play log save failed (quota?):', e);
    }
    // Genre cache — stored separately
    try {
      localStorage.setItem('snowify_genre_cache', JSON.stringify(state.trackGenreCache));
    } catch (e) {
      console.warn('Genre cache save failed (quota?):', e);
    }
  }

  function loadState() {
    try {
      // Migrate old 'snowfy' localStorage keys to 'snowify'
      if (localStorage.getItem('snowfy_state') && !localStorage.getItem('snowify_state')) {
        localStorage.setItem('snowify_state', localStorage.getItem('snowfy_state'));
        localStorage.removeItem('snowfy_state');
      }
      if (localStorage.getItem('snowfy_migrated_v2') && !localStorage.getItem('snowify_migrated_v2')) {
        localStorage.setItem('snowify_migrated_v2', localStorage.getItem('snowfy_migrated_v2'));
        localStorage.removeItem('snowfy_migrated_v2');
      }
      // One-time migration: clear data from old yt-dlp implementation
      if (!localStorage.getItem('snowify_migrated_v2')) {
        localStorage.removeItem('snowify_state');
        localStorage.setItem('snowify_migrated_v2', '1');
        return;
      }
      const rawSaved = JSON.parse(localStorage.getItem('snowify_state'));
      const saved = rawSaved ? normalizeForCloud(rawSaved) : null;
      if (saved) {
        state.playlists = saved.playlists || [];
        state.likedSongs = saved.likedSongs || [];
        state.recentTracks = saved.recentTracks || [];
        state.followedArtists = saved.followedArtists || [];
        state.volume = saved.volume ?? 0.7;
        state.shuffle = saved.shuffle ?? false;
        state.repeat = saved.repeat || 'off';
        state.musicOnly = saved.musicOnly ?? true;
        state.autoplay = saved.autoplay ?? true;
        state.audioQuality = saved.audioQuality || 'bestaudio';
        state.videoQuality = saved.videoQuality || '720';
        state.videoPremuxed = saved.videoPremuxed ?? true;
        state.animations = saved.animations ?? true;
        state.effects = saved.effects ?? true;
        state.miniplayerGlow = saved.miniplayerGlow ?? true;
        state.theme = saved.theme || 'dark';
        state.discordRpc = saved.discordRpc ?? false;
        state.country = saved.country || '';
        state.searchHistory = saved.searchHistory || [];
        state.crossfade = saved.crossfade ?? 0;
        state.normalization = saved.normalization ?? false;
        state.normalizationTarget = saved.normalizationTarget ?? -14;
        state.prefetchCount = saved.prefetchCount ?? 0;
        state.minimizeToTray = saved.minimizeToTray ?? false;
        state.launchOnStartup = saved.launchOnStartup ?? false;
        state.songSources = saved.songSources || ['youtube'];
        state.metadataSources = saved.metadataSources || ['youtube'];
        state.wrappedShownYear = saved.wrappedShownYear ?? null;

        // Persist once after deproxying old mobile proxy URLs in local state.
        if (JSON.stringify(rawSaved) !== JSON.stringify(saved)) {
          localStorage.setItem('snowify_state', JSON.stringify(saved));
        }
      }
      if (IS_MOBILE_RUNTIME) {
        state.normalization = false;
      }
      // Restore queue (local-only, separate from cloud sync)
      const rawSavedQueue = JSON.parse(localStorage.getItem('snowify_queue'));
      const savedQueue = rawSavedQueue ? normalizeForCloud(rawSavedQueue) : null;
      if (savedQueue) {
        state.queue = savedQueue.queue || [];
        state.originalQueue = savedQueue.originalQueue || [];
        state.queueIndex = savedQueue.queueIndex ?? -1;
        state.playingPlaylistId = savedQueue.playingPlaylistId || null;

        if (JSON.stringify(rawSavedQueue) !== JSON.stringify(savedQueue)) {
          localStorage.setItem('snowify_queue', JSON.stringify(savedQueue));
        }
      }
      // Play log, genre cache, and backfill are loaded async in loadPlayLogAsync()
      // to avoid blocking the main thread on startup.
    } catch (_) {}
  }

  function updateGreeting() {
    const h = new Date().getHours();
    const key = h < 12 ? 'morning' : h < 18 ? 'afternoon' : 'evening';
    $('#greeting-text').textContent = I18n.t('home.greeting.' + key);
  }

  function switchView(name) {
    const targetView = $(`#view-${name}`);
    const alreadyActive = state.currentView === name && targetView && targetView.classList.contains('active');

    state.currentView = name;
    views.forEach(v => v.classList.toggle('active', v.id === `view-${name}`));
    navBtns.forEach(b => b.classList.toggle('active', b.dataset.view === name));

    if (alreadyActive && targetView && state.animations) {
      targetView.style.animation = 'none';
      targetView.offsetHeight;
      targetView.style.animation = '';
    }

    closeLyricsPanel();

    if (name === 'home') {
      renderHome();
    }
    if (name === 'explore') {
      renderExplore();
    }
    if (name === 'search') {
      syncSearchHint();
      setTimeout(() => $('#search-input').focus(), 100);
    }
    if (name === 'library') {
      renderLibrary();
    }
    // Social listeners remain active while signed in; only stopped on sign-out
    if (name === 'settings') {
      const ts = $('#theme-select');
      if (ts) populateCustomThemes(ts, state.theme);
    }

    updateFloatingSearch();
  }

  navBtns.forEach(btn => {
    btn.addEventListener('click', () => switchView(btn.dataset.view));
  });

  // ── Floating search pill ──
  const floatingSearch = $('#floating-search');
  floatingSearch.addEventListener('click', () => switchView('search'));

  // ── Search shortcut hint (Ctrl+K / ⌘K) ──
  const isMac = navigator.platform.includes('Mac');
  const searchShortcutHint = $('#search-shortcut-hint');
  const searchShortcutMod = $('#search-shortcut-mod');
  const floatingSearchMod = $('#floating-search-mod');
  if (searchShortcutMod) searchShortcutMod.textContent = isMac ? '⌘' : 'Ctrl';
  if (floatingSearchMod) floatingSearchMod.textContent = isMac ? '⌘' : 'Ctrl';

  function updateFloatingSearch() {
    const show = ['home', 'explore', 'library', 'artist', 'album', 'playlist'].includes(state.currentView);
    floatingSearch.classList.toggle('hidden', !show);

    // ─── Global keyboard shortcuts ───────────────────────────────────────────
    document.addEventListener('keydown', (e) => {
      // Ctrl+K / Cmd+K  or  /  → open search
      if (e.key === 'k' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        $('#search-input').value = '';
        $('#search-clear').classList.add('hidden');
        closeSuggestions();
        switchView('search');
        return;
      }
      if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') {
        if (e.key === 'Escape') e.target.blur();
        return;
      }
      switch (e.key) {
        case ' ':
          e.preventDefault();
          togglePlay();
          break;
        case 'ArrowRight':
          if (e.ctrlKey) { playNext(); break; }
          if (audioRef.audio.duration) {
            if (audioRef.engine.isInProgress()) { audioRef.engine.instantComplete(); }
            const newTimeR = Math.min(audioRef.audio.duration, audioRef.audio.currentTime + 5);
            const remainingR = audioRef.audio.duration - newTimeR;
            if (remainingR > state.crossfade) audioRef.engine.resetTrigger();
            else audioRef.engine.markTriggered();
            audioRef.audio.currentTime = newTimeR;
          }
          break;
        case 'ArrowLeft':
          if (e.ctrlKey) { playPrev(); break; }
          if (audioRef.audio.duration) {
            if (audioRef.engine.isInProgress()) { audioRef.engine.instantComplete(); }
            const newTimeL = Math.max(0, audioRef.audio.currentTime - 5);
            const remainingL = audioRef.audio.duration - newTimeL;
            if (remainingL > state.crossfade) audioRef.engine.resetTrigger();
            else audioRef.engine.markTriggered();
            audioRef.audio.currentTime = newTimeL;
          }
          break;
        case 'ArrowUp':
          e.preventDefault();
          setVolume(state.volume + 0.05);
          break;
        case 'ArrowDown':
          e.preventDefault();
          setVolume(state.volume - 0.05);
          break;
        case '/':
          e.preventDefault();
          switchView('search');
          break;
      }
    });
  }

  async function init() {
    const systemLocale = await window.snowify.getLocale();
    await I18n.init(systemLocale);
    loadState();
    finishInit();
    loadPlayLogAsync(); // fire-and-forget — avoids blocking startup with large JSON parse
  }

  // ─── Wrapped trigger ───
  let _playLogReady = false;

  async function loadPlayLogAsync() {
    // Yield control back to the renderer so the UI can paint before we touch localStorage
    await new Promise(r => setTimeout(r, 0));

    // Parse play log — can be several MB for heavy listeners
    try {
      const raw = localStorage.getItem('snowify_play_log');
      if (raw) {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) state.playLog = parsed;
      }
    } catch (_) {}

    // Yield again before genre cache
    await new Promise(r => setTimeout(r, 0));
    try {
      const raw = localStorage.getItem('snowify_genre_cache');
      if (raw) {
        const parsed = JSON.parse(raw);
        if (parsed && typeof parsed === 'object') state.trackGenreCache = parsed;
      }
    } catch (_) {}

    // One-time backfill from recentTracks for users predating the Wrapped feature.
    // v2: clear stale v1 backfill (spread wrongly across multiple years) and re-seed within current year.
    const backfillVer = localStorage.getItem('snowify_playlog_backfill_ver');
    if (backfillVer !== '2' && state.recentTracks.length > 0) {
      state.playLog = []; // discard any stale backfill
    }
    if (state.playLog.length === 0 && state.recentTracks.length > 0) {
      await new Promise(r => setTimeout(r, 0));
      const now = Date.now();
      // Spread entries across the current calendar year so they all count in Wrapped
      const yearStart = new Date(new Date().getFullYear(), 0, 1).getTime();
      const span = now - yearStart;
      const n = state.recentTracks.length;
      // recentTracks[0] = most recent → assign closest timestamp
      state.playLog = state.recentTracks.map((t, i) => ({
        id: t.id,
        title: t.title,
        artist: t.artist || '',
        thumbnail: t.thumbnail || '',
        durationMs: t.durationMs || 210000, // fall back to 3.5 min average if unknown
        ts: now - (i / n) * span,
      }));
      // Persist without blocking UI (write in next task)
      await new Promise(r => setTimeout(r, 0));
      try {
        localStorage.setItem('snowify_play_log', JSON.stringify(state.playLog));
        localStorage.setItem('snowify_playlog_backfill_ver', '2');
      } catch (_) {}
    }

    _playLogReady = true;
    checkWrappedTrigger();
  }

  function checkWrappedTrigger() {
    if (!_playLogReady) return; // data not loaded yet — loadPlayLogAsync() will re-call us
    const now = new Date();
    const month = now.getMonth(); // 0 = Jan, 11 = Dec
    let targetYear = null;
    if (month === 11) targetYear = now.getFullYear();       // December → this year's data
    else if (month === 0) targetYear = now.getFullYear() - 1; // January → last year's data
    if (targetYear === null) return;
    if (state.wrappedShownYear === targetYear) return;
    if (!state.playLog.some(e => new Date(e.ts).getFullYear() === targetYear)) return;
    window.WrappedManager?.show(targetYear);
  }

  // ─── Plugin metadata helpers ───

  // If the currently playing track has a richer album art from a plugin, update the now-playing UI.
  function _maybeUpdateNowPlayingArt(track) {
    const current = state.queue[state.queueIndex];
    if (current?.id !== track.id) return;
    const cached = state.trackGenreCache[track.id];
    if (!cached?.albumArt) return;
    const thumb = $('#np-thumbnail');
    const resolvedAlbumArt = resolveImageUrl(cached.albumArt);
    if (thumb && thumb.src !== resolvedAlbumArt) {
      const originalSrc = thumb.src;
      const onError = () => {
        thumb.src = originalSrc; // restore original thumbnail if cached art fails
        thumb.removeEventListener('error', onError);
      };
      thumb.addEventListener('error', onError);
      thumb.addEventListener('load', () => thumb.removeEventListener('error', onError), { once: true });
      thumb.src = resolvedAlbumArt;
      extractDominantColor(resolvedAlbumArt).then(color => {
        const rgb = color ? `${color.r}, ${color.g}, ${color.b}` : '170, 85, 230';
        document.documentElement.style.setProperty('--ambient-rgb', rgb);
      }).catch(() => {});
    }
  }

  // ─── Track metadata enrichment (background, fire-and-forget) ───
  async function maybeEnrichTrackMeta(track) {
    if (!track?.id || !track.title) return;
    if (state.trackGenreCache[track.id]) {
      _maybeUpdateNowPlayingArt(track); // still apply cached art on repeat plays
      return;
    }
    for (const sourceId of state.metadataSources) {
      const handler = window.SnowifySources?._metaHandlers?.[sourceId];
      if (!handler) continue;
      try {
        const meta = await handler(track.title, track.artist || '');
        if (meta) {
          state.trackGenreCache[track.id] = meta;
          try { localStorage.setItem('snowify_genre_cache', JSON.stringify(state.trackGenreCache)); } catch (_) {}
          _maybeUpdateNowPlayingArt(track);
          break; // first successful source wins
        }
      } catch (_) { /* enrichment is best-effort */ }
    }
  }

  function finishInit() {
    // Expose state reference and save function for wrapped.js + plugins
    window.__snowifyState = state;
    window.__snowifySaveState = _flushSaveState;
    updateGreeting();
    // ─── Wire cross-module callbacks ───
    callbacks.saveState = saveState;
    callbacks.maybeEnrichTrackMeta = maybeEnrichTrackMeta;
    callbacks.switchView = switchView;
    settingsCallbacks.forceCloudSave = forceCloudSave;
    settingsCallbacks.cloudLoadAndMerge = cloudLoadAndMerge;
    settingsCallbacks.updateSyncStatus = updateSyncStatus;
    settingsCallbacks.getCloudUser = () => _cloudUser;
    settingsCallbacks.getCloudSyncPaused = () => _cloudSyncPaused;
    settingsCallbacks.setCloudSyncPaused = v => { _cloudSyncPaused = v; };
    settingsCallbacks.clearCloudSaveTimeout = () => { if (_cloudSaveTimeout) { clearTimeout(_cloudSaveTimeout); _cloudSaveTimeout = null; } };
    settingsCallbacks.showWelcomeOverlay = showWelcomeOverlay;
    settingsCallbacks.hideWelcomeOverlay = hideWelcomeOverlay;
    settingsCallbacks.setWelcomeError = setWelcomeError;
    settingsCallbacks.updateAccountUI = updateAccountUI;
    setVolume(state.volume);
    if (state.discordRpc) window.snowify.connectDiscord();
    if (state.minimizeToTray) window.snowify.setMinimizeToTray(true);
    if (state.launchOnStartup) window.snowify.setOpenAtLogin(true);
    $('#btn-shuffle').classList.toggle('active', state.shuffle);
    $('#btn-repeat').classList.toggle('active', state.repeat !== 'off');
    updateRepeatButton();
    renderPlaylists();
    renderHome();
    initSettings().catch(err => {
      console.error('[initSettings crashed]', err);
      showToast('Settings error: ' + err.message);
    });

    // ─── Export / Import local data (wired here, outside the async initSettings) ───
    $('#btn-export-data').addEventListener('click', async () => {
      try {
        const data = localStorage.getItem('snowify_state');
        if (!data) { showToast(I18n.t('toast.nothingToExport')); return; }
        showToast(I18n.t('toast.exportingSave'));
        const ok = await window.snowify.exportLibrary(data);
        if (ok) showToast(I18n.t('toast.libraryExported'));
      } catch (err) {
        console.error('[Export]', err);
        showToast('Export failed: ' + err.message);
      }
    });

    $('#btn-import-data').addEventListener('click', async () => {
      try {
        const text = await window.snowify.importLibrary();
        if (!text) return;
        try { JSON.parse(text); } catch { showToast(I18n.t('toast.importInvalidFile')); return; }
        if (!confirm(I18n.t('settings.confirmImportLibrary'))) return;
        localStorage.setItem('snowify_state', text);
        location.reload();
      } catch (err) {
        console.error('[Import]', err);
        showToast('Import failed: ' + err.message);
      }
    });

    // ─── Source registration API (available to plugins via window.SnowifySources) ───
    window.SnowifySources = {
      _song: [
        { id: 'youtube', label: I18n.t('settings.sourceYouTube'), desc: I18n.t('settings.sourceYouTubeDesc') },
      ],
      _meta: [
        { id: 'youtube', label: I18n.t('settings.sourceYTMeta'), desc: I18n.t('settings.sourceYTMetaDesc') },
      ],
      _metaHandlers: {},
      _artistMetaHandlers: {},
      registerSongSource(def) {
        if (!this._song.find(s => s.id === def.id)) {
          this._song.push(def);
          this._refreshSources?.();
        }
      },
      registerMetaSource(def) {
        if (!this._meta.find(s => s.id === def.id)) {
          this._meta.push(def);
          if (typeof def.enrich === 'function') this._metaHandlers[def.id] = def.enrich;
          if (typeof def.getArtistMeta === 'function') this._artistMetaHandlers[def.id] = def.getArtistMeta;
          this._refreshSources?.();
        }
      },
      _refreshSources: null,
    };

    loadEnabledPlugins();
        // ─── Windows taskbar thumbbar buttons ───
        if (window.snowify.onThumbarPrev) {
          window.snowify.onThumbarPrev(() => playPrev());
          window.snowify.onThumbarPlayPause(() => togglePlay());
          window.snowify.onThumbarNext(() => playNext());
        }
    // Restore queue display (but don't auto-play)
    const restoredTrack = state.queue[state.queueIndex];
    if (restoredTrack) {
      showNowPlaying(restoredTrack);
      appEl.classList.remove('no-player');
    } else {
      appEl.classList.add('no-player');
    }

    // Wrapped trigger is now fired by loadPlayLogAsync() once data is ready
  }


  function generateDefaultAvatar(name) {
    const letter = String(name || 'U').charAt(0).toUpperCase();
    const canvas = document.createElement('canvas');
    canvas.width = 128;
    canvas.height = 128;
    const ctx = canvas.getContext('2d');
    const accent = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#aa55e6';
    ctx.fillStyle = accent;
    ctx.beginPath();
    ctx.arc(64, 64, 64, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = '#ffffff';
    ctx.font = 'bold 56px Inter, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(letter, 64, 67);
    return canvas.toDataURL();
  }

  // ─── Deep link handler ───
  async function handleAppDeepLink({ type, id }) {
    if (type === 'track') {
      const track = await window.snowify.getTrackInfo(id).catch(() => null);
      if (track) {
        state.queue = [track];
        state.queueIndex = 0;
        playTrack(track);
      } else {
        showToast('Could not load track');
      }
    } else if (type === 'album') {
      showAlbumDetail(id, null);
    } else if (type === 'artist') {
      openArtistPage(id);
    }
  }

  if (window.snowify.onDeepLink) {
    window.snowify.onDeepLink(handleAppDeepLink);
  }

  // Check for a buffered deep link from cold start
  if (window.snowify.getPendingDeepLink) {
    window.snowify.getPendingDeepLink().then(link => {
      if (link) handleAppDeepLink(link);
    });
  }

  if (window.snowify.onAuthStateChanged) {
    window.snowify.onAuthStateChanged(async (user) => {
      updateAccountUI(user);
      if (user) {
        updateSyncStatus(I18n.t('sync.syncing'));
        await cloudLoadAndMerge({ forceCloud: true });
        updateSyncStatus(I18n.t('sync.syncedJustNow'));
      }
    });
  }

  // getUser provides a fallback for the case where onAuthStateChanged fired in the
  // main process before the renderer window existed (firebase auth restored from cache
  // before createWindow ran). In that case the IPC 'auth:stateChanged' was dropped and
  // _cloudUser is still null — we trigger the initial sync here instead.
  window.snowify.getUser?.().then(async (user) => {
    if (user) {
      if (!_cloudUser) {
        // onAuthStateChanged was missed — bootstrap auth state and cloud sync now.
        updateAccountUI(user);
        updateSyncStatus(I18n.t('sync.syncing'));
        await cloudLoadAndMerge({ forceCloud: true });
        updateSyncStatus(I18n.t('sync.syncedJustNow'));
      }
    } else {
      showWelcomeOverlay();
    }
  }).catch(() => {
    showWelcomeOverlay();
  });

  // Flush any pending saves before the window closes
  window.snowify.onBeforeClose(async () => {
    getPrefetchCache().destroy();
    _flushSaveState();
    await forceCloudSave();
    window.snowify.closeReady();
  });

  // visibilitychange fires in the WebView when Android backgrounds the app.
  // Always flush and attempt cloud save — don't gate on _cloudSaveTimeout being set:
  // if the user changed something and closed within the 300ms saveState debounce window,
  // _cloudSaveTimeout is still null but the change still needs to be cloud-saved.
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') {
      _flushSaveState(); // sync localStorage write, also clears the 300ms timer
      if (_cloudSaveTimeout) {
        clearTimeout(_cloudSaveTimeout);
        _cloudSaveTimeout = null;
      }
      _performCloudSave({ force: false }).catch(() => {}); // hash-check guards redundant writes
    }
  });
  I18n.onChange(() => {
    updateGreeting();
    renderPlaylists();
    renderHome();
    renderQueue();
    const view = state.currentView;
    if (view === 'settings') {
      resetSettingsInitialized();
      initSettings().catch(console.error);
    }
    if (view === 'library') renderLibrary();
    if (view === 'explore') renderExplore();
  });

  // ─── Mobile-specific interactions ─────────────────────────────────────────
  if (window.snowify?.platform === 'android' || window.snowify?.platform === 'ios' ||
      document.documentElement.classList.contains('platform-mobile')) {

    // Tap the mini player bar (outside the controls) → expand to full screen
    const npBar = $('#now-playing-bar');
    if (npBar) {
      npBar.addEventListener('click', (e) => {
        // Don't intercept clicks on the transport buttons
        if (e.target.closest('.np-controls')) return;
        openMaxNP();
      });
    }

    // Swipe-down gesture on max-NP to dismiss
    const maxNPEl = $('#max-np');
    if (maxNPEl) {
      let _touchStartY = 0;
      let _lastTouchY  = 0;
      let _touchActive = false;
      let _touchFromHandle = false;

      maxNPEl.addEventListener('touchstart', (e) => {
        const target = e.target;
        _touchFromHandle = !!(target && target.closest && target.closest('.max-np-topbar'));
        if (!_touchFromHandle) {
          _touchActive = false;
          return;
        }
        _touchStartY = e.touches[0].clientY;
        _lastTouchY  = _touchStartY;
        _touchActive = true;
        maxNPEl.style.transition = 'none';
      }, { passive: true });

      maxNPEl.addEventListener('touchmove', (e) => {
        if (!_touchActive || !_touchFromHandle) return;
        const dy = e.touches[0].clientY - _touchStartY;
        _lastTouchY = e.touches[0].clientY;
        if (dy > 0) {
          maxNPEl.style.transform = `translateY(${dy}px)`;
        }
      }, { passive: true });

      maxNPEl.addEventListener('touchend', () => {
        if (!_touchActive || !_touchFromHandle) return;
        _touchActive = false;
        _touchFromHandle = false;
        const dy = _lastTouchY - _touchStartY;
        maxNPEl.style.transition = '';
        maxNPEl.style.transform  = '';
        if (dy > 80) {
          closeMaxNP();
        }
      }, { passive: true });
    }

    // Swipe-down on queue panel to close (only when content is scrolled to top)
    const queuePanelEl = $('#queue-panel');
    if (queuePanelEl) {
      let _qTouchStart = 0;
      let _qScrollAtStart = 0;
      queuePanelEl.addEventListener('touchstart', (e) => {
        _qTouchStart = e.touches[0].clientY;
        const activeView = queuePanelEl.querySelector('#queue-view:not([style*="display: none"]), #history-view:not([style*="display: none"])');
        _qScrollAtStart = activeView ? activeView.scrollTop : 0;
      }, { passive: true });
      queuePanelEl.addEventListener('touchend', (e) => {
        const dy = e.changedTouches[0].clientY - _qTouchStart;
        if (dy > 80 && _qScrollAtStart < 5) {
          queuePanelEl.classList.add('hidden');
          queuePanelEl.classList.remove('visible');
        }
      }, { passive: true });
    }

    // Swipe-down on lyrics panel to close
    const lyricsPanelEl = $('#lyrics-panel');
    if (lyricsPanelEl) {
      let _lTouchStart = 0;
      lyricsPanelEl.addEventListener('touchstart', (e) => {
        _lTouchStart = e.touches[0].clientY;
      }, { passive: true });
      lyricsPanelEl.addEventListener('touchend', (e) => {
        const dy = e.changedTouches[0].clientY - _lTouchStart;
        if (dy > 80) {
          lyricsPanelEl.classList.add('hidden');
          lyricsPanelEl.classList.remove('visible');
        }
      }, { passive: true });
    }
  }

  init();
