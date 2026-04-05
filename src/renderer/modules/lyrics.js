/**
 * lyrics.js
 * Lyrics panel — fetching, rendering, syncing, and the shared RAF sync loop.
 * The sync loop is shared with now-playing.js (max NP lyrics), which imports
 * `startLyricsSyncLoop` from here; circular deps are safe because all
 * cross-module calls happen only inside function bodies, never at init time.
 */

import { audioRef } from './audio-ref.js';
import state from './state.js';
import { showToast, escapeHtml } from './utils.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);

// ─── DOM refs ───────────────────────────────────────────────────────────────
const lyricsPanel = $('#lyrics-panel');
const lyricsBody  = $('#lyrics-body');
const btnLyrics   = $('#btn-lyrics');

// ─── Shared mutable state (readable by now-playing.js via export) ───────────
export const lyricsState = {
  lines:          [],    // parsed LRC lines [{time, text}]
  trackId:        null,  // id of track whose lyrics are loaded/loading
  visible:        false, // whether lyrics panel is open
  syncActive:     false, // whether lyrics sync loop should sync this panel
  lastActiveIdx:  -1,    // last highlighted line index
  cachedEls:      null,  // cached .lyrics-line NodeList
};

// ─── Shared sync-loop state ──────────────────────────────────────────────────
let _lyricsSyncRAF    = null;
let _lyricsSyncLastTime = 0;

// ─── Event listeners ────────────────────────────────────────────────────────

btnLyrics.addEventListener('click', () => {
  lyricsState.visible = !lyricsState.visible;
  lyricsPanel.classList.toggle('hidden',  !lyricsState.visible);
  lyricsPanel.classList.toggle('visible',  lyricsState.visible);
  btnLyrics.classList.toggle('active',     lyricsState.visible);

  // Close queue panel if lyrics opens
  if (lyricsState.visible) {
    const queuePanel = $('#queue-panel');
    queuePanel.classList.add('hidden');
    queuePanel.classList.remove('visible');
  }

  const current = state.queue[state.queueIndex];
  if (lyricsState.visible && current && lyricsState.trackId !== current.id) {
    fetchAndShowLyrics(current);
  }
  if (lyricsState.visible) startLyricsSync();
  else stopLyricsSync();
});

$('#btn-close-lyrics').addEventListener('click', () => {
  closeLyricsPanel();
});

// Close lyrics when queue opens
$('#btn-queue').addEventListener('click', () => {
  if (lyricsState.visible) closeLyricsPanel();
});

// ─── Public helpers ──────────────────────────────────────────────────────────

/** Close the lyrics panel and stop sync (used by switchView in app.js). */
export function closeLyricsPanel() {
  lyricsState.visible = false;
  lyricsPanel.classList.add('hidden');
  lyricsPanel.classList.remove('visible');
  btnLyrics.classList.remove('active');
  stopLyricsSync();
}

// ─── Fetch & render ──────────────────────────────────────────────────────────

export async function fetchAndShowLyrics(track) {
  if (!track) return;
  lyricsState.trackId   = track.id;
  lyricsState.lines     = [];
  lyricsState.lastActiveIdx = -1;

  lyricsBody.innerHTML = `<div class="lyrics-loading"><div class="spinner"></div><p>${I18n.t('lyrics.searching')}</p></div>`;

  // Resolve duration opportunistically. Do not block lyrics fetch on metadata.
  const audio = audioRef.audio;
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
  const durationSec = resolveDuration();

  try {
    const result = await window.snowify.getLyrics(track.title, track.artist, track.album || '', durationSec);
    if (lyricsState.trackId !== track.id) return;

    if (!result) { showLyricsEmpty(); return; }

    if (result.synced) {
      lyricsState.lines = parseLRC(result.synced);
      renderSyncedLyrics();
      startLyricsSync();
    } else if (result.plain) {
      renderPlainLyrics(result.plain);
      showToast(I18n.t('toast.lyricsNotAvailable'));
    } else {
      showLyricsEmpty();
    }

    // Also update max NP if it is open (avoids needing a monkey-patch in now-playing.js)
    // Circular import: safe at runtime because this code only runs inside an async function body.
    const { maxNPState, renderMaxNPLyrics, startMaxLyricsSync } = await import('./now-playing.js');
    if (maxNPState.open) {
      renderMaxNPLyrics();
      if (maxNPState.lyricsVisible) startMaxLyricsSync();
    }
  } catch (err) {
    console.error('Lyrics error:', err);
    if (lyricsState.trackId === track.id) {
      lyricsBody.innerHTML = `<div class="lyrics-empty"><p>${I18n.t('lyrics.failed')}</p></div>`;
    }
  }
}

export function showLyricsEmpty() {
  lyricsBody.innerHTML = `<div class="lyrics-empty">
    <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--text-subdued)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
    <p>${I18n.t('lyrics.notFound')}</p>
  </div>`;
}

export function parseLRC(lrcText) {
  const lines = [];
  const regex = /\[(\d{2}):(\d{2})\.(\d{2,3})\]\s*(.*)/;
  lrcText.split('\n').forEach(line => {
    const match = line.match(regex);
    if (match) {
      const min = parseInt(match[1]);
      const sec = parseInt(match[2]);
      let ms = parseInt(match[3]);
      if (match[3].length === 2) ms *= 10;
      const time = min * 60 + sec + ms / 1000;
      const text = match[4].trim();
      if (text) lines.push({ time, text });
    }
  });
  return lines.sort((a, b) => a.time - b.time);
}

export function renderSyncedLyrics() {
  const audio = audioRef.audio;
  lyricsBody.innerHTML = `<div class="lyrics-content synced">
    <div class="lyrics-spacer"></div>
    ${lyricsState.lines.map((line, i) =>
      `<div class="lyrics-line" data-index="${i}" data-time="${line.time}">${escapeHtml(line.text)}</div>`
    ).join('')}
    <div class="lyrics-spacer"></div>
  </div>`;
  lyricsState.cachedEls = null; // invalidate cache after re-render

  // Click a line to seek
  lyricsBody.querySelectorAll('.lyrics-line').forEach(el => {
    el.addEventListener('click', () => {
      const time = parseFloat(el.dataset.time);
      if (audio.duration && !isNaN(time)) {
        audio.currentTime = time;
        if (audio.paused) {
          audio.play();
          state.isPlaying = true;
          // Circular import: safe at runtime
          import('./player.js').then(({ updatePlayButton }) => updatePlayButton());
        }
      }
    });
  });
}

export function renderPlainLyrics(text) {
  const lines = text.split('\n').filter(l => l.trim());
  lyricsBody.innerHTML = `<div class="lyrics-content plain">
    <div class="lyrics-spacer"></div>
    ${lines.map(l => `<div class="lyrics-line plain-line">${escapeHtml(l)}</div>`).join('')}
    <div class="lyrics-spacer"></div>
  </div>`;
}

// ─── Sync loop ───────────────────────────────────────────────────────────────

export function startLyricsSync() {
  lyricsState.syncActive = true;
  startLyricsSyncLoop();
}

export function stopLyricsSync() {
  lyricsState.syncActive = false;
}

export function syncLyrics() {
  if (!lyricsState.lines.length || !lyricsState.visible) return;
  const ct = audioRef.engine.getActiveSource().currentTime;

  let activeIdx = -1;
  for (let i = lyricsState.lines.length - 1; i >= 0; i--) {
    if (ct >= lyricsState.lines[i].time) { activeIdx = i; break; }
  }

  if (activeIdx === lyricsState.lastActiveIdx) return;
  lyricsState.lastActiveIdx = activeIdx;

  if (!lyricsState.cachedEls) {
    lyricsState.cachedEls = [...lyricsBody.querySelectorAll('.lyrics-line')];
  }
  const allLines = lyricsState.cachedEls;
  allLines.forEach((el, i) => {
    el.classList.toggle('active', i === activeIdx);
    const dist = Math.abs(i - activeIdx);
    if (activeIdx < 0)      el.style.opacity = '0.35';
    else if (dist === 0)    el.style.opacity = '1';
    else if (dist === 1)    el.style.opacity = '0.5';
    else if (dist === 2)    el.style.opacity = '0.3';
    else                    el.style.opacity = '0.15';
  });

  // Use getBoundingClientRect for a viewport-relative calculation so the scroll
  // target is correct regardless of offsetParent (critical on Android WebView).
  if (activeIdx >= 0) {
    const activeLine   = allLines[activeIdx];
    if (activeLine) {
      const cRect  = lyricsBody.getBoundingClientRect();
      const lRect  = activeLine.getBoundingClientRect();
      const target = lyricsBody.scrollTop + lRect.top - cRect.top
                     - (lyricsBody.clientHeight / 2) + (activeLine.offsetHeight / 2);
      lyricsBody.scrollTo({ top: target, behavior: 'smooth' });
    }
  }
}

/**
 * Shared RAF loop for both lyrics panel and max NP lyrics.
 * Called by startLyricsSync() and by startMaxLyricsSync() in now-playing.js.
 * Only one RAF loop runs regardless of how many panels are open.
 */
export function startLyricsSyncLoop() {
  if (_lyricsSyncRAF) return; // already running

  // Import now-playing state lazily — safe because this fn is only called after init.
  // We use a synchronously-evaluated import here: by the time startLyricsSyncLoop
  // is first invoked, now-playing.js is already fully initialized.
  let _nowPlaying = null;
  let _npLoading = false;
  const getNP = () => {
    if (!_nowPlaying) {
      if (_npLoading) return null;
      _npLoading = true;
      // Use the module cache — this is a synchronous access after first load.
      // The dynamic import() call below is a one-time async bootstrap.
      import('./now-playing.js').then(m => { _nowPlaying = m; }).finally(() => { _npLoading = false; });
    }
    return _nowPlaying;
  };

  const tick = (now) => {
    const np = getNP();
    const maxSyncActive = np ? np.maxNPState.lyricsSyncActive : false;

    // If now-playing module is still loading, keep the loop alive; max lyrics
    // sync can otherwise stop before we can read maxNPState.
    if (!lyricsState.syncActive && !maxSyncActive && !_npLoading) {
      _lyricsSyncRAF = null;
      return; // both stopped — exit loop
    }
    if (now - _lyricsSyncLastTime >= 50) { // ~20 fps
      _lyricsSyncLastTime = now;
      if (!audioRef.audio?.paused) {
        if (lyricsState.syncActive && lyricsState.lines.length) syncLyrics();
        if (np && maxSyncActive && lyricsState.lines.length && np.maxNPState.open && np.maxNPState.lyricsVisible) {
          np.syncMaxLyrics();
        }
      }
    }
    _lyricsSyncRAF = requestAnimationFrame(tick);
  };
  _lyricsSyncRAF = requestAnimationFrame(tick);
}
