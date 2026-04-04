import state from './state.js';
import { openArtistPage } from './artist.js';
import { escapeHtml } from './utils.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);

const NP_SIDE_COLLAPSED_KEY = 'snowify_np_side_collapsed';
const QUEUE_CHANGED_EVENT = 'snowify:queue-changed';
const ASYNC_FETCH_TIMEOUT_MS = 6500;

const npSidePanel = $('#np-side-panel');
const npSideVideo = $('#np-side-video');
const npSideCover = $('#np-side-cover');
const npSideArtistLink = $('#np-side-artist-link');
const npSideTrackTitle = $('#np-side-track-title');
const npSideTrackArtist = $('#np-side-track-artist');
const npSideArtistAvatar = $('#np-side-artist-avatar');
const npSideAboutName = $('#np-side-about-name');
const npSideAboutAudience = $('#np-side-about-audience');
const npSideCreditsCard = $('#np-side-credits-card');
const npSideCreditsList = $('#np-side-credits-list');
const npSideNextCard = $('#np-side-next-card');
const npSideNextThumb = $('#np-side-next-thumb');
const npSideNextTitle = $('#np-side-next-title');
const npSideNextArtist = $('#np-side-next-artist');
const btnToggleNPSide = $('#btn-toggle-np-side-panel');
const btnOpenNPSide = $('#btn-open-np-side-panel');

let _isMobileRuntime = false;
let _npSideRequestGen = 0;
let _currentTrack = null;
let _getCurrentTrack = () => state.queue[state.queueIndex] || null;
let _initialized = false;
let _lastNextCardKey = '';
let _loopStartTime = 0;
let _loopEndTime = 0;

const _npSideArtistInfoCache = new Map();
const _npSideArtistInfoInflight = new Map();
const _npSideVideoIdCache = new Map();
const _npSideImagePrefetchCache = new Set();

let _npSideCollapsed = false;
try { _npSideCollapsed = localStorage.getItem(NP_SIDE_COLLAPSED_KEY) === '1'; } catch {}

function _resolveImageUrl(url) {
  if (!url) return url;
  return window.snowify?.resolveImageUrl?.(url) || url;
}

function _preloadImage(url) {
  const resolved = _resolveImageUrl(url || '');
  if (!resolved || _npSideImagePrefetchCache.has(resolved)) return;
  _npSideImagePrefetchCache.add(resolved);
  const img = new Image();
  img.decoding = 'async';
  img.loading = 'eager';
  img.src = resolved;
}

function _withTimeout(promise, ms, fallback = null) {
  let timeoutId;
  const timeoutPromise = new Promise((resolve) => {
    timeoutId = setTimeout(() => resolve(fallback), ms);
  });
  return Promise.race([
    promise.then((v) => {
      clearTimeout(timeoutId);
      return v;
    }).catch(() => {
      clearTimeout(timeoutId);
      return fallback;
    }),
    timeoutPromise,
  ]);
}

function _hideNPSideVideo() {
  if (!npSideVideo || !npSideCover) return;
  try { npSideVideo.pause(); } catch {}
  npSideVideo.removeAttribute('src');
  npSideVideo.load();
  npSideVideo.classList.add('hidden');
  npSideCover.classList.remove('hidden');
  _loopStartTime = 0;
  _loopEndTime = 0;
}

function _computeHighlightLoopWindow(duration) {
  const total = Number(duration) || 0;
  if (!total || !isFinite(total) || total < 18) return { start: 0, end: 0 };

  const minimumStart = Math.min(12, total * 0.2);
  const preferredStart = Math.max(minimumStart, total * 0.3);
  const targetLength = Math.min(22, Math.max(10, total * 0.16));
  const maxEnd = Math.max(preferredStart + 6, total - Math.min(8, total * 0.12));
  const end = Math.min(total - 2, preferredStart + targetLength, maxEnd);
  const start = Math.max(0, Math.min(preferredStart, end - 6));

  if (end - start < 6) return { start: 0, end: 0 };
  return { start, end };
}

function _applyHighlightLoopWindow() {
  if (!npSideVideo?.duration || !isFinite(npSideVideo.duration)) return;
  const { start, end } = _computeHighlightLoopWindow(npSideVideo.duration);
  _loopStartTime = start;
  _loopEndTime = end;
  if (_loopEndTime > _loopStartTime && npSideVideo.currentTime < _loopStartTime) {
    npSideVideo.currentTime = _loopStartTime;
  }
}

function _updateNPSideOpenBtn() {
  if (!btnOpenNPSide || !npSidePanel || _isMobileRuntime) return;
  const shouldShow = _npSideCollapsed && !npSidePanel.classList.contains('hidden');
  btnOpenNPSide.classList.toggle('hidden', !shouldShow);
}

function _applyNPSideCollapsed(collapsed) {
  _npSideCollapsed = !!collapsed;
  document.body.classList.toggle('np-side-collapsed', _npSideCollapsed);
  btnToggleNPSide?.setAttribute('aria-expanded', String(!_npSideCollapsed));
  if (_npSideCollapsed) _hideNPSideVideo();
  _updateNPSideOpenBtn();
}

function _collectTrackArtists(track) {
  if (!track || typeof track !== 'object') return [];
  if (Array.isArray(track.artists) && track.artists.length) {
    return track.artists
      .map(a => ({ name: String(a?.name || '').trim(), id: a?.id || a?.artistId || null }))
      .filter(a => a.name);
  }
  const single = String(track.artist || '').trim();
  return single ? [{ name: single, id: track.artistId || null }] : [];
}

function _setSingleArtistClickTarget(el, artistName, artistId) {
  if (!el) return;
  const name = String(artistName || '').trim() || I18n.t('common.unknownArtist');
  el.textContent = name;
  el.onclick = null;
  el.classList.toggle('clickable', !!artistId);
  if (artistId) {
    el.onclick = (e) => {
      e.preventDefault();
      openArtistPage(artistId);
    };
  }
}

async function _ensureArtistInfoCached(artistId) {
  if (!artistId || !window.snowify?.artistInfo) return null;
  if (_npSideArtistInfoCache.has(artistId)) return _npSideArtistInfoCache.get(artistId);
  if (_npSideArtistInfoInflight.has(artistId)) return _npSideArtistInfoInflight.get(artistId);

  const pending = _withTimeout(window.snowify.artistInfo(artistId), ASYNC_FETCH_TIMEOUT_MS, null)
    .then((info) => {
      _npSideArtistInfoCache.set(artistId, info || null);
      _npSideArtistInfoInflight.delete(artistId);
      if (info?.avatar) _preloadImage(info.avatar);
      return info || null;
    })
    .catch(() => {
      _npSideArtistInfoInflight.delete(artistId);
      _npSideArtistInfoCache.set(artistId, null);
      return null;
    });

  _npSideArtistInfoInflight.set(artistId, pending);
  return pending;
}

function _prefetchTrackPanelAssets(track) {
  if (!track || typeof track !== 'object') return;
  if (track.thumbnail) _preloadImage(track.thumbnail);
  const artists = _collectTrackArtists(track);
  const primaryArtistId = artists[0]?.id || track.artistId || null;
  if (primaryArtistId) {
    _ensureArtistInfoCached(primaryArtistId).catch(() => {});
  }
}

function _renderArtistList(container, artists) {
  if (!container) return;
  container.textContent = '';
  const list = Array.isArray(artists) && artists.length
    ? artists
    : [{ name: I18n.t('common.unknownArtist'), id: null }];

  list.forEach((artist, index) => {
    const el = document.createElement(artist.id ? 'a' : 'span');
    el.className = `np-side-inline-artist${artist.id ? ' clickable' : ''}`;
    el.textContent = artist.name;
    if (artist.id) {
      el.href = '#';
      el.addEventListener('click', (e) => {
        e.preventDefault();
        openArtistPage(artist.id);
      });
    }
    container.appendChild(el);

    if (index < list.length - 1) {
      const sep = document.createElement('span');
      sep.className = 'np-side-inline-sep';
      sep.textContent = '•';
      container.appendChild(sep);
    }
  });
}

function _extractTrackCredits(track) {
  const out = [];
  const pushPair = (role, name) => {
    const r = String(role || '').trim();
    const n = String(name || '').trim();
    if (!r || !n) return;
    out.push({ role: r, name: n });
  };

  if (!track || typeof track !== 'object') return out;

  if (Array.isArray(track.credits)) {
    track.credits.forEach(c => {
      if (!c) return;
      if (typeof c === 'object') pushPair(c.role || c.type || c.label, c.name || c.value);
      else if (typeof c === 'string') pushPair(I18n.t('player.credits'), c);
    });
  }

  const creditMap = track.creditMap || track.creditRoles || null;
  if (creditMap && typeof creditMap === 'object') {
    for (const [role, value] of Object.entries(creditMap)) {
      if (Array.isArray(value)) pushPair(role, value.filter(Boolean).join(', '));
      else pushPair(role, value);
    }
  }

  return out.slice(0, 6);
}

function _renderCredits(track) {
  if (!npSideCreditsCard || !npSideCreditsList) return;
  const credits = _extractTrackCredits(track);
  if (!credits.length) {
    npSideCreditsCard.classList.add('hidden');
    npSideCreditsList.innerHTML = '';
    return;
  }

  npSideCreditsCard.classList.remove('hidden');
  npSideCreditsList.innerHTML = credits.map(c => `
    <div class="np-side-credit-row">
      <span class="np-side-credit-role">${escapeHtml(c.role)}</span>
      <span class="np-side-credit-name">${escapeHtml(c.name)}</span>
    </div>
  `).join('');
}

function _normalizeTextForMatch(text) {
  return String(text || '').toLowerCase().replace(/[^a-z0-9\s]+/g, ' ').replace(/\s+/g, ' ').trim();
}

function _isOfficialMv(videoTitle) {
  const t = String(videoTitle || '').toLowerCase();
  const hasOfficial = /official/.test(t) && /(music video|\bmv\b)/.test(t);
  if (!hasOfficial) return false;
  const banned = /(lyric|audio|live|performance|dance practice|teaser|trailer|reaction|cover|fancam|behind|sped up|slowed|instrumental|karaoke|fanmade|mashup|remix|roblox|minecraft|gacha|nightcore|edit)/;
  return !banned.test(t);
}

function _matchesOfficialMv(video, titleNorm, artistsNorm) {
  if (!video?.id || !_isOfficialMv(video.title)) return false;

  const vTitle = _normalizeTextForMatch(video?.title || '');
  const vArtist = _normalizeTextForMatch(video?.artist || '');
  const titleWords = titleNorm.split(' ').filter(w => w.length > 2);
  const matchedTitleWords = titleWords.filter(w => vTitle.includes(w)).length;
  const titleRatio = titleWords.length ? matchedTitleWords / titleWords.length : 0;
  const artistMatched = artistsNorm.some((artist) => artist && (vArtist.includes(artist) || vTitle.includes(artist)));

  if (!artistMatched) return false;
  if (titleNorm && !vTitle.includes(titleNorm) && titleRatio < 0.72) return false;
  if (vArtist && !artistsNorm.some((artist) => artist && vArtist.includes(artist))) return false;
  return true;
}

function _scoreOfficialVideoCandidate(video, titleNorm, artistsNorm) {
  const vTitle = _normalizeTextForMatch(video?.title || '');
  const vArtist = _normalizeTextForMatch(video?.artist || '');
  let score = 0;

  if (titleNorm && vTitle.includes(titleNorm)) score += 8;
  const titleWords = titleNorm.split(' ').filter(Boolean);
  const titleOverlap = titleWords.filter(w => w.length > 2 && vTitle.includes(w)).length;
  score += Math.min(6, titleOverlap);

  artistsNorm.forEach((a) => {
    if (!a) return;
    if (vTitle.includes(a)) score += 4;
    if (vArtist.includes(a)) score += 3;
  });

  if (_isOfficialMv(video?.title)) score += 8;
  if (/\(official\)/i.test(video?.title || '')) score += 2;

  return score;
}

async function _resolveOfficialMvVideoId(track) {
  if (!track || track.isLocal || !window.snowify?.searchVideos) return null;

  const cacheKey = `${track.id || ''}|${track.title || ''}|${track.artist || ''}`;
  if (_npSideVideoIdCache.has(cacheKey)) return _npSideVideoIdCache.get(cacheKey);

  const artists = _collectTrackArtists(track);
  const title = String(track.title || '').trim();
  const primaryArtist = artists[0]?.name || String(track.artist || '').trim();
  const titleNorm = _normalizeTextForMatch(title);
  const artistsNorm = artists.map(a => _normalizeTextForMatch(a.name)).filter(Boolean);

  const queries = [
    `${title} ${primaryArtist} official mv`.trim(),
    `${title} ${primaryArtist} official music video`.trim(),
  ].filter(Boolean);

  for (const query of queries) {
    try {
      const results = await _withTimeout(window.snowify.searchVideos(query), ASYNC_FETCH_TIMEOUT_MS, []);
      if (!Array.isArray(results) || !results.length) continue;
      const official = results.filter(v => _matchesOfficialMv(v, titleNorm, artistsNorm));
      if (!official.length) continue;
      official.sort((a, b) =>
        _scoreOfficialVideoCandidate(b, titleNorm, artistsNorm) - _scoreOfficialVideoCandidate(a, titleNorm, artistsNorm)
      );
      const picked = official[0] || null;
      if (picked?.id) {
        _npSideVideoIdCache.set(cacheKey, picked.id);
        return picked.id;
      }
    } catch {
      // Ignore and continue trying next query.
    }
  }

  _npSideVideoIdCache.set(cacheKey, null);
  return null;
}

function _renderNextQueueCard() {
  if (!npSideNextCard || !npSideNextTitle || !npSideNextArtist || !npSideNextThumb) return;

  const next = state.queue[state.queueIndex + 1] || null;
  const cardKey = next
    ? `${state.queueIndex + 1}|${next.id || ''}|${next.title || ''}|${next.artist || ''}|${next.thumbnail || ''}`
    : 'none';
  if (cardKey === _lastNextCardKey) return;
  _lastNextCardKey = cardKey;

  if (!next) {
    npSideNextCard.classList.add('hidden');
    npSideNextTitle.textContent = I18n.t('player.noNextTrack');
    npSideNextArtist.textContent = '';
    npSideNextThumb.src = '';
    return;
  }

  npSideNextCard.classList.remove('hidden');
  npSideNextTitle.textContent = next.title || '—';
  npSideNextThumb.src = _resolveImageUrl(next.thumbnail || '');
  _renderArtistList(npSideNextArtist, _collectTrackArtists(next));
}

function _updatePanelShell(track) {
  if (!npSidePanel || !track) return;

  const artists = _collectTrackArtists(track);
  const primaryArtist = artists[0] || { name: track.artist || I18n.t('common.unknownArtist'), id: track.artistId || null };
  const cachedArtistInfo = primaryArtist.id ? _npSideArtistInfoCache.get(primaryArtist.id) : null;

  npSidePanel.classList.remove('hidden');
  _updateNPSideOpenBtn();

  npSideCover.src = _resolveImageUrl(track.thumbnail || '');
  npSideTrackTitle.textContent = track.title || '—';
  _renderArtistList(npSideTrackArtist, artists);

  _setSingleArtistClickTarget(npSideArtistLink, primaryArtist.name, primaryArtist.id);
  _setSingleArtistClickTarget(npSideAboutName, primaryArtist.name, primaryArtist.id);

  npSideAboutAudience.textContent = cachedArtistInfo?.monthlyListeners || '';
  npSideArtistAvatar.src = _resolveImageUrl(cachedArtistInfo?.avatar || track.thumbnail || '');

  _renderCredits(track);
  _renderNextQueueCard();
}

async function _updatePanelAsync(track, reqGen) {
  const artists = _collectTrackArtists(track);
  const primaryArtist = artists[0] || { name: track.artist || I18n.t('common.unknownArtist'), id: track.artistId || null };

  try {
    _hideNPSideVideo();

    if (!_npSideCollapsed) {
      try {
        const mvVideoId = await _resolveOfficialMvVideoId(track);
        if (reqGen !== _npSideRequestGen) return;
        if (mvVideoId) {
          const stream = await _withTimeout(
            window.snowify.getVideoStreamUrl(mvVideoId, '360', true),
            ASYNC_FETCH_TIMEOUT_MS,
            null
          );
          if (reqGen !== _npSideRequestGen) return;
          if (stream?.videoUrl) {
            npSideVideo.src = stream.videoUrl;
            npSideVideo.loop = false;
            npSideVideo.classList.remove('hidden');
            npSideCover.classList.add('hidden');
            const playPromise = npSideVideo.play();
            if (playPromise && typeof playPromise.catch === 'function') playPromise.catch(() => {});
          }
        }
      } catch {
        // Keep cover fallback.
      }
    }

    const artistInfo = primaryArtist.id ? await _ensureArtistInfoCached(primaryArtist.id) : null;
    if (reqGen !== _npSideRequestGen) return;

    const avatar = _resolveImageUrl(artistInfo?.avatar || track.thumbnail || '');
    npSideArtistAvatar.src = avatar;
    npSideAboutAudience.textContent = artistInfo?.monthlyListeners || '';
  } catch {
    // About metadata can fail silently.
  }
}

function _refreshCurrentTrackPanel() {
  if (_currentTrack) updateNowPlayingSidePanel(_currentTrack, { deferAsync: true });
}

export function initNowPlayingSidePanel({ isMobileRuntime = false, getCurrentTrack } = {}) {
  if (_initialized) return;
  _isMobileRuntime = !!isMobileRuntime;
  if (typeof getCurrentTrack === 'function') _getCurrentTrack = getCurrentTrack;

  if (!npSidePanel || _isMobileRuntime) return;
  _initialized = true;

  _applyNPSideCollapsed(_npSideCollapsed);

  btnToggleNPSide?.addEventListener('click', () => {
    _applyNPSideCollapsed(!_npSideCollapsed);
    try { localStorage.setItem(NP_SIDE_COLLAPSED_KEY, _npSideCollapsed ? '1' : '0'); } catch {}
  });

  btnOpenNPSide?.addEventListener('click', () => {
    _applyNPSideCollapsed(false);
    try { localStorage.setItem(NP_SIDE_COLLAPSED_KEY, '0'); } catch {}
    _currentTrack = _getCurrentTrack();
    if (_currentTrack) _refreshCurrentTrackPanel();
  });

  npSideVideo?.addEventListener('error', () => {
    npSideVideo.classList.add('hidden');
    npSideCover?.classList.remove('hidden');
    _loopStartTime = 0;
    _loopEndTime = 0;
  });

  npSideVideo?.addEventListener('loadedmetadata', () => {
    _applyHighlightLoopWindow();
  });

  npSideVideo?.addEventListener('ended', () => {
    npSideVideo.currentTime = _loopEndTime > _loopStartTime ? _loopStartTime : 0;
    const p = npSideVideo.play();
    if (p && typeof p.catch === 'function') p.catch(() => {});
  });

  npSideVideo?.addEventListener('timeupdate', () => {
    if (!npSideVideo.duration || !isFinite(npSideVideo.duration)) return;
    if (_loopEndTime > _loopStartTime) {
      if (npSideVideo.currentTime < _loopStartTime) npSideVideo.currentTime = _loopStartTime;
      if (npSideVideo.currentTime >= _loopEndTime - 0.08) npSideVideo.currentTime = _loopStartTime;
      return;
    }
    if (npSideVideo.duration - npSideVideo.currentTime <= 0.12) npSideVideo.currentTime = 0;
  });

  document.addEventListener(QUEUE_CHANGED_EVENT, () => {
    if (_currentTrack) {
      _renderNextQueueCard();
      _prefetchTrackPanelAssets(state.queue[state.queueIndex + 1] || null);
    }
  });
}

export function updateNowPlayingSidePanel(track, { deferAsync = true } = {}) {
  if (!npSidePanel || _isMobileRuntime || !track) return;
  _currentTrack = track;
  _lastNextCardKey = '';
  const reqGen = ++_npSideRequestGen;

  _prefetchTrackPanelAssets(track);
  _prefetchTrackPanelAssets(state.queue[state.queueIndex + 1] || null);

  _updatePanelShell(track);

  const runAsyncUpdate = () => {
    _updatePanelAsync(track, reqGen).catch(() => {});
  };

  if (deferAsync) {
    setTimeout(runAsyncUpdate, 0);
  } else {
    runAsyncUpdate();
  }
}
