/**
 * now-playing.js
 * Maximized "Now Playing" screen — track-change handler, dominant-color
 * extraction, max-NP UI, and the shared max-NP lyrics panel.
 */

import { audioRef } from './audio-ref.js';
import state from './state.js';
import { escapeHtml, renderArtistLinks } from './utils.js';

// Circular imports — all usage is inside function/event-handler bodies,
// which only execute after every module has finished initialising.
// ES module circular deps are safe in this pattern.
import { lyricsState, parseLRC, renderSyncedLyrics, renderPlainLyrics, startLyricsSyncLoop } from './lyrics.js';
import { updatePlayButton, updateProgress, setVolume, togglePlay, playPrev, playNext,
         isShowTimeRemaining, toggleShowTimeRemaining, getPrevVolume, setPrevVolume } from './player.js';
import { toggleLike } from './player.js';
import { buildPlaylistSectionHtml, removeContextMenu, positionContextMenu, handleTogglePlaylist } from './context-menus.js';
import { startRadio, handlePlayNext, handleAddToQueue, switchQueueTab, renderQueue } from './queue.js';
import { openArtistPage, bindArtistLinks } from './artist.js';
import { openVideoPlayer } from './video-player.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);

// ─── DOM refs ────────────────────────────────────────────────────────────────
const maxNP               = $('#max-np');
const maxNPBgA            = $('#max-np-bg-a');
const maxNPBgB            = $('#max-np-bg-b');
let   _maxNPBgFront       = 'a';
const maxNPArt            = $('#max-np-art');
const maxNPTitle          = $('#max-np-title');
const maxNPArtist         = $('#max-np-artist');
const maxNPTopbarArt      = $('#max-np-topbar-art');
const maxNPTopbarTitle    = $('#max-np-topbar-title');
const maxNPTopbarArtist   = $('#max-np-topbar-artist');
const maxNPLike           = $('#max-np-like');
const maxNPLyricsToggle   = $('#max-np-lyrics-toggle');
const maxNPQueueBtn       = $('#max-np-queue');
const maxNPRight          = $('#max-np-right');
const maxNPLyrics         = $('#max-np-lyrics');
const maxNPPlay           = $('#max-np-play');
const maxNPShuffleBtn     = $('#max-np-shuffle');
const maxNPRepeatBtn      = $('#max-np-repeat');
const maxNPProgressBar    = $('#max-np-progress-bar');
const maxNPProgressFill   = $('#max-np-progress-fill');
const maxNPTimeCurrent    = $('#max-np-time-current');
const maxNPTimeTotal      = $('#max-np-time-total');

// ─── Shared mutable state (readable by lyrics.js's sync loop) ────────────────
export const maxNPState = {
  open:            false,
  lyricsVisible:   false,
  lyricsSyncActive: false,
  lastActiveIdx:   -1,
  cachedEls:       null,
};

// ─── onTrackChanged ──────────────────────────────────────────────────────────

export function onTrackChanged(track) {
  // Reset all lyrics state for the new track so there's no stale data
  lyricsState.lastActiveIdx   = -1;
  maxNPState.lastActiveIdx    = -1;
  lyricsState.cachedEls       = null;
  maxNPState.cachedEls        = null;
  lyricsState.trackId         = null; // cancel any in-flight fetch guard for old track
  if (lyricsState.visible) {
    import('./lyrics.js').then(({ fetchAndShowLyrics }) => fetchAndShowLyrics(track));
  } else if (maxNPState.open && maxNPState.lyricsVisible) {
    fetchMaxNPLyrics(track);
  }
  updateMaxNP(track);
}

// ─── Color extraction ────────────────────────────────────────────────────────

export function extractDominantColor(imgEl) {
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      try {
        const canvas = document.createElement('canvas');
        const size = 64;
        canvas.width = size; canvas.height = size;
        const ctx = canvas.getContext('2d');
        ctx.drawImage(img, 0, 0, size, size);
        const data = ctx.getImageData(0, 0, size, size).data;

        const buckets = {};
        for (let i = 0; i < data.length; i += 16) {
          const r = data[i], g = data[i + 1], b = data[i + 2];
          const max = Math.max(r, g, b), min = Math.min(r, g, b);
          const l   = (max + min) / 2;
          const sat = max === 0 ? 0 : (max - min) / max;
          if (l < 40 || l > 230 || sat < 0.2) continue;
          const qr = Math.round(r / 32) * 32;
          const qg = Math.round(g / 32) * 32;
          const qb = Math.round(b / 32) * 32;
          const key = `${qr},${qg},${qb}`;
          if (!buckets[key]) buckets[key] = { r: 0, g: 0, b: 0, count: 0, satSum: 0 };
          buckets[key].r += r; buckets[key].g += g; buckets[key].b += b;
          buckets[key].count++; buckets[key].satSum += sat;
        }

        const entries = Object.values(buckets);
        if (!entries.length) { resolve(null); return; }

        entries.sort((a, b) => (b.count * (b.satSum / b.count)) - (a.count * (a.satSum / a.count)));
        const best  = entries[0];
        const r     = Math.round(best.r / best.count);
        const g     = Math.round(best.g / best.count);
        const b2    = Math.round(best.b / best.count);
        const maxC  = Math.max(r, g, b2);
        const boost = maxC < 140 ? 140 / maxC : 1;
        resolve({
          r: Math.min(255, Math.round(r  * boost)),
          g: Math.min(255, Math.round(g  * boost)),
          b: Math.min(255, Math.round(b2 * boost)),
        });
      } catch (_) { resolve(null); }
    };
    img.onerror = () => resolve(null);
    img.src = imgEl.src;
  });
}

export function applyMaxNPLyricsColor(color) {
  if (color) {
    maxNP.style.setProperty('--lyrics-color', `rgb(${color.r}, ${color.g}, ${color.b})`);
    maxNP.style.setProperty('--lyrics-glow',  `rgba(${color.r}, ${color.g}, ${color.b}, 0.35)`);
  } else {
    maxNP.style.removeProperty('--lyrics-color');
    maxNP.style.removeProperty('--lyrics-glow');
  }
}

// ─── Max NP open / close / update ────────────────────────────────────────────

export function syncMaxNPLayout() {
  maxNP.classList.toggle('lyrics-active', maxNPState.lyricsVisible);
}

export function openMaxNP() {
  const current = state.queue[state.queueIndex];
  if (!current) return;
  maxNPState.open = true;
  maxNP.classList.remove('hidden');

  const thumbUrl = current.thumbnail ? current.thumbnail.replace(/=w\d+-h\d+/, '=w800-h800') : '';
  const bgUrl    = `url('${thumbUrl || current.thumbnail}')`;
  maxNPBgA.style.backgroundImage = bgUrl;
  maxNPBgB.style.backgroundImage = bgUrl;
  maxNPBgA.style.opacity = '1';
  maxNPBgB.style.opacity = '0';
  _maxNPBgFront = 'a';

  void maxNP.offsetHeight;
  maxNP.classList.add('visible');
  syncMaxNPLayout();
  updateMaxNP(current);
  syncMaxNPControls();
  $('#max-np-vol-fill').style.width = (state.volume * 100) + '%';
  const isMuted = state.volume === 0;
  const mvb = $('#max-np-vol-btn');
  $('.vol-icon', mvb).classList.toggle('hidden', isMuted);
  $('.vol-mute-icon', mvb).classList.toggle('hidden', !isMuted);
  renderMaxNPLyrics();
  startMaxLyricsSync();
}

export function closeMaxNP() {
  maxNPState.open = false;
  maxNPState.lyricsVisible = false;
  maxNP.classList.remove('visible', 'lyrics-active');
  maxNPRight.classList.add('hidden');
  maxNPRight.classList.remove('visible');
  stopMaxLyricsSync();
  setTimeout(() => { if (!maxNPState.open) maxNP.classList.add('hidden'); }, 500);
}

export function updateMaxNP(track) {
  if (!track || !maxNPState.open) return;
  const resolveImageUrl = window.snowify?.resolveImageUrl?.bind(window.snowify) || (u => u);
  const thumbUrl = track.thumbnail ? track.thumbnail.replace(/=w\d+-h\d+/, '=w800-h800') : '';
  const imgSrc   = resolveImageUrl(thumbUrl || track.thumbnail);
  maxNPArt.src         = imgSrc;
  maxNPTopbarArt.src   = resolveImageUrl(track.thumbnail || imgSrc);

  extractDominantColor(maxNPArt).then(applyMaxNPLyricsColor);

  const bgUrl = `url('${imgSrc}')`;
  if (_maxNPBgFront === 'a') {
    maxNPBgB.style.transition = 'none';
    maxNPBgB.style.opacity    = '0';
    maxNPBgB.style.backgroundImage = bgUrl;
    void maxNPBgB.offsetHeight;
    maxNPBgB.style.transition = 'opacity 1.2s ease';
    maxNPBgB.style.opacity    = '1';
    _maxNPBgFront = 'b';
    setTimeout(() => { if (_maxNPBgFront === 'b') maxNPBgA.style.backgroundImage = bgUrl; }, 1300);
  } else {
    maxNPBgA.style.backgroundImage = maxNPBgB.style.backgroundImage;
    maxNPBgB.style.transition = 'none';
    maxNPBgB.style.opacity    = '0';
    maxNPBgB.style.backgroundImage = bgUrl;
    void maxNPBgB.offsetHeight;
    maxNPBgB.style.transition = 'opacity 1.2s ease';
    maxNPBgB.style.opacity    = '1';
  }

  maxNPTitle.textContent    = track.title;
  maxNPArtist.innerHTML     = renderArtistLinks(track);
  bindArtistLinks(maxNPArtist);
  maxNPTopbarTitle.textContent  = track.title;
  maxNPTopbarArtist.textContent = track.artist || '';

  const isLiked = state.likedSongs.some(t => t.id === track.id);
  maxNPLike.classList.toggle('liked', isLiked);
}

// ─── Max NP lyrics ───────────────────────────────────────────────────────────

export function renderMaxNPLyrics() {
  if (!maxNPState.open) return;
  maxNPState.lastActiveIdx = -1;

  const audio = audioRef.audio;

  if (lyricsState.lines.length > 0) {
    maxNPLyrics.innerHTML = `<div class="lyrics-content synced">
      <div class="lyrics-spacer"></div>
      ${lyricsState.lines.map((line, i) =>
        `<div class="lyrics-line" data-index="${i}" data-time="${line.time}">${escapeHtml(line.text)}</div>`
      ).join('')}
      <div class="lyrics-spacer"></div>
    </div>`;
    maxNPState.cachedEls = null;

    maxNPLyrics.querySelectorAll('.lyrics-line').forEach(el => {
      el.addEventListener('click', () => {
        const time = parseFloat(el.dataset.time);
        if (audio.duration && !isNaN(time)) {
          audio.currentTime = time;
          if (audio.paused) {
            audio.play();
            state.isPlaying = true;
            updatePlayButton();
          }
        }
      });
    });
  } else if (lyricsState.trackId) {
    const plainContent = $('#lyrics-body .lyrics-content.plain');
    if (plainContent) {
      maxNPLyrics.innerHTML = plainContent.outerHTML;
    } else {
      const emptyOrLoading = $('#lyrics-body .lyrics-empty, #lyrics-body .lyrics-loading');
      maxNPLyrics.innerHTML = emptyOrLoading ? emptyOrLoading.outerHTML :
        `<div class="lyrics-empty"><svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.3)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg><p style="color:rgba(255,255,255,0.4)">${I18n.t('lyrics.notFound')}</p></div>`;
    }
  } else {
    const current = state.queue[state.queueIndex];
    if (current) {
      maxNPLyrics.innerHTML = `<div class="lyrics-loading"><div class="spinner"></div><p>${I18n.t('lyrics.searching')}</p></div>`;
      fetchMaxNPLyrics(current);
    }
  }
}

export async function fetchMaxNPLyrics(track) {
  if (!track) return;

  if (lyricsState.trackId === track.id && lyricsState.lines.length > 0) {
    renderMaxNPLyrics();
    return;
  }

  const audio = audioRef.audio;
  lyricsState.trackId       = track.id;
  lyricsState.lines         = [];
  lyricsState.lastActiveIdx = -1;

  maxNPLyrics.innerHTML = `<div class="lyrics-loading"><div class="spinner"></div><p>${I18n.t('lyrics.searching')}</p></div>`;

  const resolveDuration = () => {
    if (audio.duration && !isNaN(audio.duration) && audio.duration > 0) return Math.round(audio.duration);
    if (track.durationMs && track.durationMs > 0) return Math.round(track.durationMs / 1000);
    if (track.duration) {
      const parts = track.duration.split(':');
      if (parts.length === 2) return parseInt(parts[0]) * 60 + parseInt(parts[1]);
      if (parts.length === 3) return parseInt(parts[0]) * 3600 + parseInt(parts[1]) * 60 + parseInt(parts[2]);
    }
    return null;
  };
  let durationSec = resolveDuration();
  if (!durationSec) {
    await new Promise(resolve => {
      if (audio.readyState >= 1) { durationSec = resolveDuration(); resolve(); return; }
      const onMeta = () => { audio.removeEventListener('loadedmetadata', onMeta); durationSec = resolveDuration(); resolve(); };
      audio.addEventListener('loadedmetadata', onMeta, { once: true });
      setTimeout(() => { audio.removeEventListener('loadedmetadata', onMeta); resolve(); }, 4000);
    });
    if (lyricsState.trackId !== track.id) return;
  }

  try {
    const result = await window.snowify.getLyrics(track.title, track.artist, track.album || '', durationSec);
    if (lyricsState.trackId !== track.id) return;

    if (!result) {
      maxNPLyrics.innerHTML = `<div class="lyrics-empty"><svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.3)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg><p style="color:rgba(255,255,255,0.4)">${I18n.t('lyrics.notFound')}</p></div>`;
      return;
    }

    if (result.synced) {
      lyricsState.lines = parseLRC(result.synced);
      if (lyricsState.visible) renderSyncedLyrics();
      renderMaxNPLyrics();
      startMaxLyricsSync();
    } else if (result.plain) {
      const lines = result.plain.split('\n').filter(l => l.trim());
      maxNPLyrics.innerHTML = `<div class="lyrics-content plain">
        <div class="lyrics-spacer"></div>
        ${lines.map(l => `<div class="lyrics-line plain-line">${escapeHtml(l)}</div>`).join('')}
        <div class="lyrics-spacer"></div>
      </div>`;
      if (lyricsState.visible) renderPlainLyrics(result.plain);
    } else {
      maxNPLyrics.innerHTML = `<div class="lyrics-empty"><svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.3)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg><p style="color:rgba(255,255,255,0.4)">${I18n.t('lyrics.notFound')}</p></div>`;
    }
  } catch (err) {
    console.error('Max NP lyrics error:', err);
    maxNPLyrics.innerHTML = `<div class="lyrics-empty"><p style="color:rgba(255,255,255,0.4)">${I18n.t('lyrics.failed')}</p></div>`;
  }
}

// ─── Max lyrics sync ─────────────────────────────────────────────────────────

export function startMaxLyricsSync() {
  maxNPState.lyricsSyncActive = true;
  startLyricsSyncLoop();
}

export function stopMaxLyricsSync() {
  maxNPState.lyricsSyncActive = false;
}

export function syncMaxLyrics() {
  if (!lyricsState.lines.length || !maxNPState.open || !maxNPState.lyricsVisible) return;
  const ct = audioRef.engine.getActiveSource().currentTime;

  let activeIdx = -1;
  for (let i = lyricsState.lines.length - 1; i >= 0; i--) {
    if (ct >= lyricsState.lines[i].time) { activeIdx = i; break; }
  }

  if (activeIdx === maxNPState.lastActiveIdx) return;
  maxNPState.lastActiveIdx = activeIdx;

  if (!maxNPState.cachedEls) {
    maxNPState.cachedEls = [...maxNPLyrics.querySelectorAll('.lyrics-line')];
  }
  const allLines = maxNPState.cachedEls;
  allLines.forEach((el, i) => {
    el.classList.toggle('active', i === activeIdx);
    const dist = Math.abs(i - activeIdx);
    if (activeIdx < 0)      el.style.opacity = '0.35';
    else if (dist === 0)    el.style.opacity = '1';
    else if (dist === 1)    el.style.opacity = '0.5';
    else if (dist === 2)    el.style.opacity = '0.3';
    else                    el.style.opacity = '0.15';
  });

  if (activeIdx >= 0) {
    const activeLine = allLines[activeIdx];
    if (activeLine) {
      const lineTop    = activeLine.offsetTop;
      const lineHeight = activeLine.offsetHeight;
      const cHeight    = maxNPLyrics.clientHeight;
      maxNPLyrics.scrollTop = lineTop - (cHeight / 2) + (lineHeight / 2);
    }
  }
}

// ─── Progress & controls ─────────────────────────────────────────────────────

export function updateMaxNPProgress() {
  if (!maxNPState.open) return;
  const src = audioRef.engine?.getActiveSource();
  if (!src?.duration) return;
  const pct = (src.currentTime / src.duration) * 100;
  maxNPProgressFill.style.width = pct + '%';
  maxNPTimeCurrent.textContent  = formatTime(src.currentTime);
  maxNPTimeTotal.textContent    = isShowTimeRemaining()
    ? '-' + formatTime(Math.max(0, src.duration - src.currentTime))
    : formatTime(src.duration);
}

export function syncMaxNPControls() {
  const playIcon  = maxNPPlay.querySelector('.icon-play');
  const pauseIcon = maxNPPlay.querySelector('.icon-pause');
  if (state.isPlaying) {
    playIcon.classList.add('hidden');
    pauseIcon.classList.remove('hidden');
  } else {
    playIcon.classList.remove('hidden');
    pauseIcon.classList.add('hidden');
  }
  maxNPShuffleBtn.classList.toggle('active', state.shuffle);
  maxNPRepeatBtn.classList.toggle('active', state.repeat !== 'off');
  if (state.repeat === 'one') {
    maxNPRepeatBtn.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 014-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 01-4 4H3"/><text x="12" y="15" text-anchor="middle" font-size="8" fill="currentColor" stroke="none" font-weight="bold">1</text></svg>`;
  } else if (state.repeat === 'all') {
    maxNPRepeatBtn.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 014-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 01-4 4H3"/><text x="12" y="15" text-anchor="middle" font-size="7" fill="currentColor" stroke="none" font-weight="bold">∞</text></svg>`;
  } else {
    maxNPRepeatBtn.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 014-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 01-4 4H3"/></svg>`;
  }
}

/** Update the volume knob UI in the max NP panel (called by setVolume in player.js). */
export function syncMaxNPVolume(vol) {
  const fill = $('#max-np-vol-fill');
  if (fill) fill.style.width = (vol * 100) + '%';
  const isMuted = vol === 0;
  const mvb = $('#max-np-vol-btn');
  if (mvb) {
    $('.vol-icon', mvb).classList.toggle('hidden', isMuted);
    $('.vol-mute-icon', mvb).classList.toggle('hidden', !isMuted);
  }
}

// ─── Event listeners ─────────────────────────────────────────────────────────

maxNPLyricsToggle.addEventListener('click', () => {
  maxNPState.lyricsVisible = !maxNPState.lyricsVisible;
  maxNPRight.classList.toggle('hidden',  !maxNPState.lyricsVisible);
  maxNPRight.classList.toggle('visible',  maxNPState.lyricsVisible);
  maxNPLyricsToggle.classList.toggle('active', maxNPState.lyricsVisible);
  syncMaxNPLayout();
  if (maxNPState.lyricsVisible) {
    renderMaxNPLyrics();
    startMaxLyricsSync();
  } else {
    stopMaxLyricsSync();
  }
});

maxNPQueueBtn.addEventListener('click', () => {
  const queuePanel = $('#queue-panel');
  queuePanel.classList.remove('hidden');
  queuePanel.classList.add('visible');
  switchQueueTab('queue');
  renderQueue();
});

maxNPLike.addEventListener('click', () => {
  const current = state.queue[state.queueIndex];
  if (!current) return;
  toggleLike(current);
  const isLiked = state.likedSongs.some(t => t.id === current.id);
  maxNPLike.classList.toggle('liked', isLiked);
  $('#np-like').classList.toggle('liked', isLiked);
});

// Volume control
const maxNPVolSlider = $('#max-np-vol-slider');
const maxNPVolBtn    = $('#max-np-vol-btn');
let   _maxNPDraggingVol = false;

maxNPVolSlider.addEventListener('mousedown', (e) => { _maxNPDraggingVol = true; _maxNPUpdateVolume(e); });
document.addEventListener('mousemove', (e) => { if (_maxNPDraggingVol) _maxNPUpdateVolume(e); });
document.addEventListener('mouseup', () => { _maxNPDraggingVol = false; });

function _maxNPUpdateVolume(e) {
  const rect = maxNPVolSlider.getBoundingClientRect();
  const pct  = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
  setVolume(pct);
}

maxNPVolBtn.addEventListener('click', () => {
  if (state.volume > 0) {
    setPrevVolume(state.volume);
    setVolume(0);
  } else {
    setVolume(getPrevVolume());
  }
});

// Now-playing bar context menu
$('.np-track-info').addEventListener('contextmenu', (e) => {
  e.preventDefault();
  const track = state.queue[state.queueIndex];
  if (!track) return;
  removeContextMenu();
  const isLiked  = state.likedSongs.some(t => t.id === track.id);
  const isLocal  = !!track.isLocal;
  const menu     = document.createElement('div');
  menu.className = 'context-menu';
  menu.style.left = e.clientX + 'px';
  menu.style.top  = e.clientY + 'px';

  const playlistSection = buildPlaylistSectionHtml(track);

  menu.innerHTML = `
    ${isLocal ? '' : `<div class="context-menu-item" data-action="start-radio">${I18n.t('context.startRadio')}</div>`}
    ${isLocal ? '' : `<div class="context-menu-item" data-action="watch-video">${I18n.t('context.watchVideo')}</div>`}
    <div class="context-menu-item" data-action="like">${isLiked ? I18n.t('context.unlike') : I18n.t('context.like')}</div>
    ${playlistSection}
    ${track.artistId ? `<div class="context-menu-divider"></div><div class="context-menu-item" data-action="go-to-artist">${I18n.t('context.goToArtist')}</div>` : ''}
    ${isLocal ? '' : `<div class="context-menu-divider"></div><div class="context-menu-item" data-action="share">${I18n.t('context.copyLink')}</div>`}
  `;

  positionContextMenu(menu);

  menu.addEventListener('click', async (ev) => {
    const item = ev.target.closest('[data-action]');
    if (!item) return;
    const action = item.dataset.action;
    if (action === 'none') return;
    switch (action) {
      case 'start-radio': await startRadio(track); break;
      case 'watch-video': openVideoPlayer(track.id, track.title, track.artist); break;
      case 'like': toggleLike(track); break;
      case 'toggle-playlist': handleTogglePlaylist(item.dataset.pid, track); break;
      case 'go-to-artist': openArtistPage(track.artistId); break;
      case 'share':
        navigator.clipboard.writeText(`https://snowify.cc/track/${track.id}`);
        import('./utils.js').then(({ showToast }) => showToast(I18n.t('toast.linkCopied')));
        break;
    }
    removeContextMenu();
  });

  setTimeout(() => { document.addEventListener('click', removeContextMenu, { once: true }); }, 10);
});

// Thumbnail click → open/close max NP
$('#np-thumbnail').addEventListener('click', () => {
  if (maxNPState.open) closeMaxNP(); else openMaxNP();
});

$('#max-np-x').addEventListener('click', closeMaxNP);

// Progress bar seek
let _maxNPDragging = false;
maxNPProgressBar.addEventListener('mousedown', (e) => { _maxNPDragging = true; maxNPSeekTo(e); });
document.addEventListener('mousemove', (e) => { if (_maxNPDragging) maxNPSeekTo(e); });
document.addEventListener('mouseup', () => { _maxNPDragging = false; });

maxNPProgressBar.addEventListener('touchstart', (e) => { maxNPSeekTo(e.touches[0]); }, { passive: true });
maxNPProgressBar.addEventListener('touchmove',  (e) => { maxNPSeekTo(e.touches[0]); }, { passive: true });

export function maxNPSeekTo(e) {
  const audio  = audioRef.audio;
  const engine = audioRef.engine;
  if (engine.isInProgress()) { engine.instantComplete(); audioRef.audio = engine.getActiveAudio(); }
  const rect = maxNPProgressBar.getBoundingClientRect();
  const pct  = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
  if (audio.duration) {
    const newTime   = pct * audio.duration;
    const remaining = audio.duration - newTime;
    if (remaining > state.crossfade) engine.resetTrigger();
    else engine.markTriggered();
    audio.currentTime = newTime;
    maxNPProgressFill.style.width = (pct * 100) + '%';
    $('#progress-fill').style.width = (pct * 100) + '%';
  }
}

maxNPTimeTotal.addEventListener('click', () => {
  toggleShowTimeRemaining();
  updateProgress();
  updateMaxNPProgress();
});

$('#max-np-prev').addEventListener('click', playPrev);
$('#max-np-next').addEventListener('click', playNext);
maxNPPlay.addEventListener('click', togglePlay);

maxNPShuffleBtn.addEventListener('click', () => {
  $('#btn-shuffle').click();
  syncMaxNPControls();
});

maxNPRepeatBtn.addEventListener('click', () => {
  $('#btn-repeat').click();
  syncMaxNPControls();
});

// Close max NP on Escape
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && maxNPState.open) {
    closeMaxNP();
    e.stopPropagation();
  }
});
