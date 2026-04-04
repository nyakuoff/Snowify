/**
 * queue.js
 * Queue management — play-next/add-to-queue, radio, queue/history panel,
 * drag-to-reorder, and the addToRecent bookkeeping.
 */

import { audioRef } from './audio-ref.js';
import state from './state.js';
import { showToast, escapeHtml, renderArtistLinks } from './utils.js';
import { callbacks } from './callbacks.js';
// Circular imports — safe at runtime: all usage is inside function bodies.
import { playTrack, playFromList, getPrefetchCache } from './player.js';
import { showContextMenu } from './context-menus.js';
import { bindArtistLinks } from './artist.js';
import { renderRecentTracks, renderQuickPicks } from './home.js';
import { LOCAL_THUMB_FALLBACK } from './player.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);
const $$ = (sel, ctx = document) => [...ctx.querySelectorAll(sel)];
const QUEUE_CHANGED_EVENT = 'snowify:queue-changed';

function emitQueueChanged() {
  document.dispatchEvent(new CustomEvent(QUEUE_CHANGED_EVENT, {
    detail: {
      queueIndex: state.queueIndex,
      queueLength: state.queue.length,
      nextTrackId: state.queue[state.queueIndex + 1]?.id || null,
    },
  }));
}

// ─── Queue panel state ────────────────────────────────────────────────────────
const queuePanel     = $('#queue-panel');
export let _queueActiveTab = 'queue'; // read by addToRecent

// ─── Scroll-to-top buttons ────────────────────────────────────────────────────
const queueUpNext = $('#queue-up-next');
const historyList = $('#history-list');

$('#queue-scroll-top').addEventListener('click', () => {
  queueUpNext.scrollTo({ top: 0, behavior: 'smooth' });
});
$('#history-scroll-top').addEventListener('click', () => {
  historyList.scrollTo({ top: 0, behavior: 'smooth' });
});
queueUpNext.addEventListener('scroll', () => {
  $('#queue-scroll-top').classList.toggle('visible', queueUpNext.scrollTop > 100);
});
historyList.addEventListener('scroll', () => {
  $('#history-scroll-top').classList.toggle('visible', historyList.scrollTop > 100);
});

// Queue open/close
$('#btn-queue').addEventListener('click', () => {
  queuePanel.classList.toggle('hidden');
  queuePanel.classList.toggle('visible');
  _queueActiveTab = 'queue';
  switchQueueTab('queue');
  renderQueue();
});
$('#btn-close-queue').addEventListener('click', () => {
  queuePanel.classList.add('hidden');
  queuePanel.classList.remove('visible');
});

export function closeQueuePanel() {
  queuePanel.classList.add('hidden');
  queuePanel.classList.remove('visible');
}

// Tab switching
$$('.queue-tab', queuePanel).forEach(tab => {
  tab.addEventListener('click', () => {
    const target = tab.dataset.tab;
    if (target === _queueActiveTab) return;
    _queueActiveTab = target;
    switchQueueTab(target);
  });
});

// Clear queue
$('#btn-clear-queue').addEventListener('click', () => {
  state.queue = state.queue.slice(0, state.queueIndex + 1);
  const remainingIds = new Set(state.queue.map(t => t.id));
  state.originalQueue = state.originalQueue.filter(t => remainingIds.has(t.id));
  getPrefetchCache()?.clear();
  renderQueue();
  callbacks.saveState();
  showToast(I18n.t('toast.queueCleared'));
});

// Clear history
$('#btn-clear-history').addEventListener('click', () => {
  state.recentTracks = [];
  callbacks.saveState();
  renderHistory();
  renderRecentTracks();
  renderQuickPicks();
  showToast(I18n.t('toast.historyViewCleared'));
});

// ─── Helpers ──────────────────────────────────────────────────────────────────

function updateCachedCount() {
  const el = $('#queue-cached-count');
  if (!el) return;
  if (state.prefetchCount === 0) { el.textContent = ''; return; }
  const pc = getPrefetchCache();
  let count = 0;
  for (let i = state.queueIndex + 1; i < state.queue.length; i++) {
    if (pc?.getCachedPath(state.queue[i].id)) count++;
  }
  el.textContent = count > 0 ? I18n.t('queue.cached', { count }) : '';
}

// ─── Core queue actions ───────────────────────────────────────────────────────

export function handlePlayNext(track) {
  const existIdx = state.queue.findIndex((t, i) => i > state.queueIndex && t.id === track.id);
  if (existIdx !== -1) state.queue.splice(existIdx, 1);
  if (state.queueIndex >= 0) state.queue.splice(state.queueIndex + 1, 0, track);
  else state.queue.push(track);
  audioRef.engine?.clearPreload();
  showToast(existIdx !== -1 ? I18n.t('toast.movedToPlayNext') : I18n.t('toast.addedToPlayNext'));
  renderQueue();
  const pc = getPrefetchCache();
  if (state.prefetchCount !== 0) pc?.onTrackChanged(state.queueIndex, state.queue);
}

export function handleAddToQueue(track) {
  if (state.queue.slice(state.queueIndex + 1).some(t => t.id === track.id)) {
    showToast(I18n.t('toast.alreadyInQueue'));
  } else {
    state.queue.push(track);
    showToast(I18n.t('toast.addedToQueue'));
    renderQueue();
    const pc = getPrefetchCache();
    if (state.prefetchCount !== 0) pc?.onTrackChanged(state.queueIndex, state.queue);
  }
}

export async function startRadio(seed, { fromArtistId } = {}) {
  if (fromArtistId) {
    showToast(I18n.t('toast.loadingRadio'));
    const info = await window.snowify.artistInfo(fromArtistId);
    seed = info?.topSongs?.[0];
    if (!seed) { showToast(I18n.t('toast.couldNotStartRadio')); return; }
  }
  const upNexts = await window.snowify.getUpNexts(seed.id);
  if (!upNexts.length) { showToast(I18n.t('toast.couldNotStartRadio')); return; }
  const alreadyPlaying = state.isPlaying && state.queue[state.queueIndex]?.id === seed.id;
  if (alreadyPlaying) {
    state.queue = [seed, ...upNexts.filter(t => t.id !== seed.id)];
    state.originalQueue = [...state.queue];
    state.queueIndex = 0;
    renderQueue();
  } else {
    playFromList([seed, ...upNexts.filter(t => t.id !== seed.id)], 0);
  }
  showToast(I18n.t('toast.radioStarted'));
}

export function getLikedSongsPlaylist() {
  return { id: 'liked', name: I18n.t('sidebar.likedSongs'), tracks: state.likedSongs };
}

// ─── Render functions ─────────────────────────────────────────────────────────

export function renderNowPlayingSection(container) {
  const current = state.queue[state.queueIndex];
  if (current) {
    container.innerHTML = renderQueueItem(current, true, false);
    const nowItem = container.querySelector('.queue-item');
    if (nowItem) {
      bindArtistLinks(nowItem);
      nowItem.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        showContextMenu(e, current, { hideAddQueue: true, hidePlayNext: true });
      });
    }
  } else {
    container.innerHTML = `<p style="color:var(--text-subdued);font-size:13px;">${I18n.t('queue.nothingPlaying')}</p>`;
  }
}

export function renderQueue() {
  const nowPlaying = $('#queue-now-playing');
  const upNext     = $('#queue-up-next');
  const clearBtn   = $('#btn-clear-queue');

  renderNowPlayingSection(nowPlaying);

  const upcoming = state.queue.slice(state.queueIndex + 1);
  clearBtn.style.display = upcoming.length ? '' : 'none';

  if (upcoming.length) {
    upNext.innerHTML = upcoming.map((t, i) => {
      const queueIdx = state.queueIndex + 1 + i;
      return renderQueueItem(t, false, true, queueIdx);
    }).join('');

    upNext.querySelectorAll('.queue-item').forEach(item => {
      const idx   = parseInt(item.dataset.queueIndex, 10);
      const track = state.queue[idx];
      if (!track) return;

      bindArtistLinks(item);

      item.addEventListener('click', (e) => {
        if (e.target.closest('.queue-item-remove') || e.target.closest('a')) return;
        state.queueIndex = idx;
        playTrack(state.queue[idx]);
        renderQueue();
      });

      item.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        const isNext = idx === state.queueIndex + 1;
        showContextMenu(e, track, { hideAddQueue: true, hidePlayNext: isNext });
      });

      const removeBtn = item.querySelector('.queue-item-remove');
      if (removeBtn) {
        removeBtn.addEventListener('click', (e) => {
          e.stopPropagation();
          state.queue.splice(idx, 1);
          state.originalQueue = state.originalQueue.filter(
            t => t.id !== track.id || state.queue.some(q => q.id === t.id)
          );
          audioRef.engine?.clearPreload();
          const pc = getPrefetchCache();
          if (state.prefetchCount !== 0) pc?.onTrackChanged(state.queueIndex, state.queue);
          renderQueue();
          callbacks.saveState();
        });
      }
    });

    bindQueueDragReorder();
  } else {
    upNext.innerHTML = `<p style="color:var(--text-subdued);font-size:13px;">${I18n.t('queue.empty')}</p>`;
  }
  updateCachedCount();
  emitQueueChanged();
}

export function renderQueueItem(track, isActive, showRemove, queueIndex) {
  const removeHtml  = showRemove ? `<button class="queue-item-remove" title="${I18n.t('queue.removeFromQueue')}"><svg width="14" height="14" viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg></button>` : '';
  const indexAttr   = queueIndex !== undefined ? ` data-queue-index="${queueIndex}"` : '';
  const draggable   = showRemove ? ' draggable="true"' : '';
  return `
    <div class="queue-item ${isActive ? 'active' : ''}" data-track-id="${track.id}"${indexAttr}${draggable}>
      <img data-src="${escapeHtml(track.thumbnail || (track.isLocal ? LOCAL_THUMB_FALLBACK : ''))}" alt="" />
      <div class="queue-item-info">
        <div class="queue-item-title">${escapeHtml(track.title)}</div>
        <div class="queue-item-artist">${renderArtistLinks(track)}</div>
      </div>
      ${removeHtml}
    </div>`;
}

export function renderHistory() {
  renderNowPlayingSection($('#history-now-playing'));
  const container = $('#history-list');
  $('#btn-clear-history').style.display = state.recentTracks.length ? '' : 'none';
  if (!state.recentTracks.length) {
    container.innerHTML = `<p style="color:var(--text-subdued);font-size:13px;">${I18n.t('queue.noRecentTracks')}</p>`;
    return;
  }
  container.innerHTML = state.recentTracks.map(t => renderQueueItem(t, false, false)).join('');
  container.querySelectorAll('.queue-item').forEach(item => {
    const track = state.recentTracks.find(t => t.id === item.dataset.trackId);
    if (!track) return;
    bindArtistLinks(item);
    item.addEventListener('click', (e) => { if (e.target.closest('a')) return; playFromList([track], 0); });
    item.addEventListener('contextmenu', (e) => { e.preventDefault(); showContextMenu(e, track); });
  });
}

export function switchQueueTab(tab) {
  $$('.queue-tab', queuePanel).forEach(t => t.classList.toggle('active', t.dataset.tab === tab));
  $('#queue-view').style.display   = tab === 'queue'   ? '' : 'none';
  $('#history-view').style.display = tab === 'history' ? '' : 'none';
  if (tab === 'history') renderHistory();
}

// ─── addToRecent ──────────────────────────────────────────────────────────────

export function addToRecent(track) {
  state.recentTracks = state.recentTracks.filter(t => t.id !== track.id);
  state.recentTracks.unshift(track);
  if (state.recentTracks.length > 20) state.recentTracks.pop();

  state.playLog.push({
    id: track.id, title: track.title, artist: track.artist || '',
    thumbnail: track.thumbnail || '', durationMs: track.durationMs || 0, ts: Date.now(),
  });
  if (state.playLog.length > 15000) {
    const twoYearsAgo = Date.now() - 2 * 365.25 * 24 * 3600 * 1000;
    state.playLog = state.playLog.filter(e => e.ts >= twoYearsAgo);
  }

  callbacks.saveState();
  renderRecentTracks();
  renderQuickPicks();
  if (_queueActiveTab === 'history') renderHistory();
}

// ─── Drag-to-reorder ──────────────────────────────────────────────────────────

let _dragScrollRAF  = null;
let _dragScrollSpeed = 0;
let _queueDragAbort = null;

document.addEventListener('dragend', () => stopDragScroll());

export function bindQueueDragReorder() {
  if (_queueDragAbort) _queueDragAbort.abort();
  _queueDragAbort = new AbortController();
  const signal    = _queueDragAbort.signal;
  const container = $('#queue-up-next');
  const EDGE_ZONE = 40, MAX_SPEED = 12;
  const items     = container.querySelectorAll('.queue-item[draggable="true"]');

  items.forEach(item => {
    item.addEventListener('dragstart', (e) => {
      item.classList.add('dragging');
      container.classList.add('is-dragging');
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', '');
    }, { signal });

    item.addEventListener('dragend', () => {
      item.classList.remove('dragging');
      container.classList.remove('is-dragging');
      stopDragScroll();
      const reordered = [];
      container.querySelectorAll('.queue-item').forEach(el => {
        const qIdx = parseInt(el.dataset.queueIndex, 10);
        if (state.queue[qIdx]) reordered.push(state.queue[qIdx]);
      });
      const before  = state.queue.slice(0, state.queueIndex + 1);
      state.queue   = [...before, ...reordered];
      audioRef.engine?.clearPreload();
      const pc = getPrefetchCache();
      if (state.prefetchCount !== 0) pc?.onTrackChanged(state.queueIndex, state.queue);
      renderQueue();
      callbacks.saveState();
    }, { signal });
  });

  container.addEventListener('dragover', (e) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    const afterElement = getDragAfterElement(container, e.clientY);
    const dragging = container.querySelector('.dragging');
    if (!dragging) return;
    if (afterElement) container.insertBefore(dragging, afterElement);
    else container.appendChild(dragging);

    const rect = container.getBoundingClientRect();
    const distTop    = e.clientY - rect.top;
    const distBottom = rect.bottom - e.clientY;
    if (distTop < EDGE_ZONE) {
      _dragScrollSpeed = -Math.round(MAX_SPEED * (1 - distTop / EDGE_ZONE));
      startDragScroll();
    } else if (distBottom < EDGE_ZONE) {
      _dragScrollSpeed = Math.round(MAX_SPEED * (1 - distBottom / EDGE_ZONE));
      startDragScroll();
    } else {
      stopDragScroll();
    }
  }, { signal });
}

function startDragScroll() {
  if (_dragScrollRAF) return;
  const scrollEl = $('#queue-up-next');
  const tick = () => { scrollEl.scrollTop += _dragScrollSpeed; _dragScrollRAF = requestAnimationFrame(tick); };
  _dragScrollRAF = requestAnimationFrame(tick);
}

function stopDragScroll() {
  if (_dragScrollRAF) { cancelAnimationFrame(_dragScrollRAF); _dragScrollRAF = null; }
}

function getDragAfterElement(container, y) {
  const elements = [...container.querySelectorAll('.queue-item:not(.dragging)')];
  return elements.reduce((closest, child) => {
    const box    = child.getBoundingClientRect();
    const offset = y - box.top - box.height / 2;
    if (offset < 0 && offset > closest.offset) return { offset, element: child };
    return closest;
  }, { offset: Number.NEGATIVE_INFINITY }).element;
}
