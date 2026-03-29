/**
 * context-menus.js
 * Track context menus, playlist toggle, track replacement, renderTrackList,
 * and collection (album / playlist) context menus.
 */

import state from './state.js';
import { escapeHtml, showToast, renderArtistLinks, resolveImageUrl } from './utils.js';
import { callbacks } from './callbacks.js';
// Circular imports — safe at runtime: all usage is inside function bodies.
import {
  playFromList, playTrack, toggleLike, updatePlaylistHighlight,
  spawnHeartParticles, spawnBrokenHeart,
  NOW_PLAYING_EQ_HTML, LOCAL_THUMB_FALLBACK,
  showNowPlaying, updateLikedCount,
} from './player.js';
import { handlePlayNext, handleAddToQueue, startRadio, getLikedSongsPlaylist, renderQueue } from './queue.js';
import { openVideoPlayer } from './video-player.js';
import { bindArtistLinks } from './artist.js';
import { startTrackDrag } from './library.js';
import { renderPlaylists, renderLibrary, showPlaylistDetail, createPlaylist } from './library.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);

// ─── SVG constants for save button ───────────────────────────────────────────
const SAVE_SVG_CHECK = '<span class="save-burst"></span><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>';
const SAVE_SVG_PLUS  = '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>';

function trackPlaysLabel(track) {
  if (!track) return '';
  return String(track.plays || track.viewCount || track.views || track.shortViewCountText || '').trim();
}

// ─── renderTrackList ──────────────────────────────────────────────────────────

export function renderTrackList(container, tracks, context, sourcePlaylistId = null) {
  const isArtistCtx = context === 'artist-popular';

  let html = `
    <div class="track-list-header">
      <span>#</span>
      <span>${I18n.t('trackList.title')}</span>
      ${!isArtistCtx ? `<span>${I18n.t('trackList.artist')}</span>` : ''}
      <span></span>
    </div>`;

  const _currentId = state.queue[state.queueIndex]?.id;
  const _likedIds  = new Set(state.likedSongs.map(t => t.id));
  tracks.forEach((track, i) => {
    const isPlaying = _currentId === track.id;
    const isLiked   = _likedIds.has(track.id);
    const playsText = trackPlaysLabel(track);
    html += `
      <div class="track-row ${isPlaying ? 'playing' : ''}${isArtistCtx ? ' track-row--artist' : ''}"
           data-track-id="${track.id}" data-context="${context}" data-index="${i}" draggable="true">
        <div class="track-num">
          <span class="track-num-text">${i + 1}</span>
          ${NOW_PLAYING_EQ_HTML}
          <span class="track-num-play">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg>
          </span>
        </div>
        <div class="track-main">
          <img class="track-thumb" data-src="${escapeHtml(resolveImageUrl(track.thumbnail || (track.isLocal ? LOCAL_THUMB_FALLBACK : '')) || '')}" alt="" />
          <div class="track-details">
            <div class="track-title">${escapeHtml(track.title)}${track.isLocal ? '<span class="local-badge">LOCAL</span>' : ''}</div>
            ${playsText ? `<div class="track-inline-plays">${escapeHtml(playsText)}</div>` : ''}
            ${!isArtistCtx && track.artist ? `<div class="track-artist-sub">${escapeHtml(track.artist)}</div>` : ''}
          </div>
        </div>
        ${!isArtistCtx ? `<div class="track-artist-col">${renderArtistLinks(track)}</div>` : ''}
        <div class="track-like-col">
          <button class="track-like-btn${isLiked ? ' liked' : ''}" title="${I18n.t('player.like')}">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z"/>
            </svg>
          </button>
        </div>
      </div>`;
  });

  container.innerHTML = html;

  container.querySelectorAll('.track-row').forEach(row => {
    const idx   = parseInt(row.dataset.index);
    const track = tracks[idx];
    row.addEventListener('click', () => {
      if (context === 'playlist' || context === 'album') {
        playFromList(tracks, idx, sourcePlaylistId);
      } else {
        playFromList([track], 0);
      }
    });
    row.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      showContextMenu(e, tracks[idx]);
    });
    row.addEventListener('dragstart', (e) => {
      if (track) startTrackDrag(e, track);
    });
    const likeBtn = row.querySelector('.track-like-btn');
    if (likeBtn && track) {
      likeBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        const wasLiked = toggleLike(track);
        likeBtn.classList.toggle('liked', state.likedSongs.some(t => t.id === track.id));
        if (wasLiked) spawnHeartParticles(likeBtn); else spawnBrokenHeart(likeBtn);
      });
    }
  });

  bindArtistLinks(container);
}

// ─── Playlist section HTML ────────────────────────────────────────────────────

export function buildPlaylistSectionHtml(track) {
  if (!state.playlists.length) return '';
  const alreadyIn = (pl) => pl.tracks.some(t => t.id === track.id);
  const checkIcon = '<svg class="playlist-toggle-icon is-added" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>';
  const plusIcon  = '<svg class="playlist-toggle-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>';
  const subItems  = state.playlists.map(p =>
    `<div class="context-menu-item context-sub-item" data-action="toggle-playlist" data-pid="${p.id}"><span>${escapeHtml(p.name)}</span>${alreadyIn(p) ? checkIcon : plusIcon}</div>`
  ).join('');
  return `
    <div class="context-menu-divider"></div>
    <div class="context-menu-item context-menu-has-sub" data-action="none">
      <span>${I18n.t('context.addToPlaylist')}</span>
      <svg class="sub-arrow" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
      <div class="context-submenu">${subItems}</div>
    </div>`;
}

// ─── handleTogglePlaylist ─────────────────────────────────────────────────────

export function handleTogglePlaylist(playlistId, track) {
  const pl = state.playlists.find(p => p.id === playlistId);
  if (!pl) return;
  const idx = pl.tracks.findIndex(t => t.id === track.id);
  if (idx !== -1) {
    pl.tracks.splice(idx, 1);
    showToast(I18n.t('toast.removedFromPlaylist', { name: pl.name }));
  } else {
    pl.tracks.push(track);
    showToast(I18n.t('toast.addedToPlaylist', { name: pl.name }));
  }
  callbacks.saveState();
  renderPlaylists();
}

// ─── replaceTrackEverywhere ───────────────────────────────────────────────────

export function replaceTrackEverywhere(oldTrack, newTrack) {
  const oldId = oldTrack?.id;
  if (!oldId || !newTrack?.id) return 0;
  let replaced = 0;

  const replaceIn = (arr) => {
    let count = 0;
    for (let i = 0; i < arr.length; i++) {
      if (arr[i]?.id === oldId) { arr[i] = newTrack; count++; }
    }
    return count;
  };

  state.playlists.forEach(pl => { replaced += replaceIn(pl.tracks); });
  replaced += replaceIn(state.likedSongs);
  replaced += replaceIn(state.recentTracks);
  replaced += replaceIn(state.queue);

  if (replaced > 0) {
    callbacks.saveState();
    renderPlaylists();
    if (state.currentView === 'library') renderLibrary();
    if (state.currentView === 'playlist') {
      if (state.currentPlaylistId === 'liked') showPlaylistDetail(getLikedSongsPlaylist(), true);
      else {
        const pl = state.playlists.find(p => p.id === state.currentPlaylistId);
        if (pl) showPlaylistDetail(pl, false);
      }
    }
    renderQueue();
    showNowPlaying();
  }

  return replaced;
}

// ─── findTrackAlternatives ────────────────────────────────────────────────────

export async function findTrackAlternatives(track, replaceCtx = null) {
  if (!track?.title) return;

  const modal      = $('#track-replace-modal');
  const titleEl    = $('#track-replace-title');
  const subtitleEl = $('#track-replace-subtitle');
  const statusEl   = $('#track-replace-status');
  const listEl     = $('#track-replace-list');
  const cancelBtn  = $('#track-replace-cancel');

  titleEl.textContent    = I18n.t('replace.title');
  subtitleEl.textContent = `${track.title}${track.artist ? ` - ${track.artist}` : ''}`;
  statusEl.textContent   = I18n.t('replace.searching');
  listEl.innerHTML       = '';
  modal.classList.remove('hidden');

  const close = () => {
    modal.classList.add('hidden');
    cancelBtn.onclick = null;
    modal.onclick = null;
  };

  cancelBtn.onclick = close;
  modal.onclick = (e) => { if (e.target === modal) close(); };

  let results = [];
  try {
    const query = track.title.trim();
    const found = await window.snowify.search(query, true);
    results = (found || []).filter(r => r?.id && r.id !== track.id).slice(0, 30);
  } catch { results = []; }

  if (!results.length) { statusEl.textContent = I18n.t('replace.noResults'); return; }

  statusEl.textContent = I18n.t('replace.pickOne');
  listEl.innerHTML = results.map((r, i) => `
    <div class="spotify-track-item selectable" data-idx="${i}">
      <span class="spotify-track-status">
        <img src="${escapeHtml(resolveImageUrl(r.thumbnail || '') || '')}" alt="" style="width:16px;height:16px;border-radius:3px;object-fit:cover;" />
      </span>
      <span class="spotify-track-title">${escapeHtml(r.title || '')}</span>
      <span class="spotify-track-artist">${escapeHtml(r.artist || '')}</span>
    </div>
  `).join('');

  listEl.querySelectorAll('.spotify-track-item.selectable').forEach(el => {
    el.addEventListener('click', () => {
      const picked = results[parseInt(el.dataset.idx, 10)];
      if (!picked) return;

      if (replaceCtx?.playlist && Number.isInteger(replaceCtx?.idx)) {
        if (replaceCtx.isLiked) {
          state.likedSongs[replaceCtx.idx] = picked;
          callbacks.saveState();
          updateLikedCount();
          showPlaylistDetail(getLikedSongsPlaylist(), true);
        } else {
          replaceCtx.playlist.tracks[replaceCtx.idx] = picked;
          callbacks.saveState();
          renderPlaylists();
          showPlaylistDetail(replaceCtx.playlist, false);
        }
        showToast(I18n.t('toast.trackReplaced'));
      } else {
        const count = replaceTrackEverywhere(track, picked);
        if (count > 0) showToast(I18n.t('toast.trackReplacedEverywhere', { count }));
        else {
          playFromList([picked], 0);
          showToast(I18n.t('toast.playingSelectedVersion'));
        }
      }

      close();
    });
  });
}

// ─── positionContextMenu ──────────────────────────────────────────────────────

export function positionContextMenu(menu) {
  document.body.appendChild(menu);
  const rect = menu.getBoundingClientRect();
  if (rect.right  > window.innerWidth)  menu.style.left = (window.innerWidth  - rect.width  - 8) + 'px';
  if (rect.bottom > window.innerHeight) menu.style.top  = (window.innerHeight - rect.height - 8) + 'px';

  const isMobile = document.documentElement.classList.contains('platform-mobile');
  menu.querySelectorAll('.context-menu-has-sub').forEach(parentItem => {
    const subMenuEl = parentItem.querySelector('.context-submenu');
    if (isMobile) {
      parentItem.addEventListener('click', (ev) => {
        if (ev.target.closest('.context-sub-item')) return; // let sub-item clicks bubble to main handler
        ev.stopPropagation();
        const isOpen = parentItem.classList.contains('submenu-open');
        menu.querySelectorAll('.context-menu-has-sub.submenu-open').forEach(el => el.classList.remove('submenu-open'));
        if (!isOpen) parentItem.classList.add('submenu-open');
      });
    } else {
      let hideTimeout = null;
      const show = () => {
        clearTimeout(hideTimeout);
        menu.querySelectorAll('.context-menu-has-sub.submenu-open').forEach(el => {
          if (el !== parentItem) el.classList.remove('submenu-open');
        });
        parentItem.classList.add('submenu-open');
        const subRect = subMenuEl.getBoundingClientRect();
        if (subRect.right  > window.innerWidth)  subMenuEl.classList.add('open-left'); else subMenuEl.classList.remove('open-left');
        if (subRect.bottom > window.innerHeight)  { subMenuEl.style.top = 'auto'; subMenuEl.style.bottom = '0'; }
      };
      const hide = () => { hideTimeout = setTimeout(() => parentItem.classList.remove('submenu-open'), 250); };
      parentItem.addEventListener('mouseenter', show);
      parentItem.addEventListener('mouseleave', hide);
      subMenuEl.addEventListener('mouseenter', () => clearTimeout(hideTimeout));
      subMenuEl.addEventListener('mouseleave', hide);
    }
  });
}

// ─── removeContextMenu ────────────────────────────────────────────────────────

export function removeContextMenu() {
  const isMobile = document.documentElement.classList.contains('platform-mobile');
  document.querySelectorAll('.context-menu').forEach(m => {
    if (isMobile) { m.classList.remove('ctx-open'); setTimeout(() => m.remove(), 300); } else { m.remove(); }
  });
  document.querySelectorAll('.ctx-backdrop').forEach(b => {
    b.classList.remove('ctx-open');
    setTimeout(() => b.remove(), 300);
  });
}

// ─── showContextMenu ──────────────────────────────────────────────────────────

export function showContextMenu(e, track, { hideAddQueue = false, hidePlayNext = false } = {}) {
  removeContextMenu();
  const isMobile = document.documentElement.classList.contains('platform-mobile');
  const isLiked  = state.likedSongs.some(t => t.id === track.id);
  const isLocal  = !!track.isLocal;
  const menu     = document.createElement('div');
  menu.className = 'context-menu';
  if (!isMobile) {
    menu.style.left = e.clientX + 'px';
    menu.style.top  = e.clientY + 'px';
  }

  const playlistSection = buildPlaylistSectionHtml(track);
  const addQueueHtml    = hideAddQueue ? '' : `<div class="context-menu-item" data-action="add-queue">${I18n.t('context.addToQueue')}</div>`;
  const playNextHtml    = hidePlayNext ? '' : `<div class="context-menu-item" data-action="play-next">${I18n.t('context.playNext')}</div>`;

  const mobileHeader = isMobile ? `
    <div class="ctx-sheet-handle"></div>
    <div class="ctx-sheet-header">
      <div class="ctx-sheet-track">
        <img class="ctx-sheet-thumb" src="${escapeHtml(resolveImageUrl(track.thumbnail || '') || '')}" alt="" />
        <div class="ctx-sheet-info">
          <div class="ctx-sheet-title">${escapeHtml(track.title)}</div>
          <div class="ctx-sheet-artist">${escapeHtml(track.artist || '')}</div>
        </div>
      </div>
    </div>
    <div class="context-menu-divider"></div>
  ` : '';

  menu.innerHTML = mobileHeader + `
    <div class="context-menu-item" data-action="play">${I18n.t('context.play')}</div>
    ${playNextHtml}
    ${addQueueHtml}
    <div class="context-menu-divider"></div>
    ${isLocal ? '' : `<div class="context-menu-item" data-action="start-radio">${I18n.t('context.startRadio')}</div>`}
    ${isLocal ? '' : `<div class="context-menu-item" data-action="watch-video">${I18n.t('context.watchVideo')}</div>`}
    <div class="context-menu-item" data-action="like">${isLiked ? I18n.t('context.unlike') : I18n.t('context.like')}</div>
    ${playlistSection}
    ${isLocal ? '' : `<div class="context-menu-divider"></div><div class="context-menu-item" data-action="share">${I18n.t('context.copyLink')}</div>`}
  `;

  positionContextMenu(menu);

  if (isMobile) {
    const backdrop     = document.createElement('div');
    backdrop.className = 'ctx-backdrop';
    document.body.appendChild(backdrop);
    requestAnimationFrame(() => requestAnimationFrame(() => {
      menu.classList.add('ctx-open');
      backdrop.classList.add('ctx-open');
    }));
    backdrop.addEventListener('click', removeContextMenu);

    let touchStartY = 0, touchLastY = 0;
    menu.addEventListener('touchstart', (ev) => {
      touchStartY = ev.touches[0].clientY; touchLastY = touchStartY;
      menu.style.transition = 'none';
    }, { passive: true });
    menu.addEventListener('touchmove', (ev) => {
      touchLastY = ev.touches[0].clientY;
      const delta = Math.max(0, touchLastY - touchStartY);
      menu.style.transform = `translateY(${delta}px)`;
      backdrop.style.opacity = String(Math.max(0, 1 - delta / 260));
    }, { passive: true });
    menu.addEventListener('touchend', () => {
      const delta = Math.max(0, touchLastY - touchStartY);
      if (delta > 90) {
        menu.style.transition = 'transform 0.22s ease';
        menu.style.transform  = 'translateY(100%)';
        backdrop.style.transition = 'opacity 0.22s ease';
        backdrop.style.opacity    = '0';
        setTimeout(() => { menu.remove(); backdrop.remove(); }, 230);
      } else {
        menu.style.transition = 'transform 0.3s cubic-bezier(0.32, 0.72, 0, 1)';
        menu.style.transform  = 'translateY(0)';
        backdrop.style.opacity = '1';
      }
    });
  } else {
    setTimeout(() => { document.addEventListener('click', removeContextMenu, { once: true }); }, 10);
  }

  menu.addEventListener('click', async (ev) => {
    const item = ev.target.closest('[data-action]');
    if (!item) return;
    const action = item.dataset.action;
    if (action === 'none') return;
    switch (action) {
      case 'play':
        state.playingPlaylistId = null;
        playTrack(track);
        updatePlaylistHighlight();
        break;
      case 'play-next':       handlePlayNext(track); break;
      case 'add-queue':       handleAddToQueue(track); break;
      case 'watch-video':     openVideoPlayer(track.id, track.title, track.artist); break;
      case 'start-radio':     await startRadio(track); break;
      case 'like':            toggleLike(track); break;
      case 'toggle-playlist': handleTogglePlaylist(item.dataset.pid, track); break;
      case 'share':
        navigator.clipboard.writeText(`https://snowify.cc/track/${track.id}`);
        showToast(I18n.t('toast.linkCopied'));
        break;
    }
    removeContextMenu();
  });
}

// ─── setupSaveButton ──────────────────────────────────────────────────────────

export function setupSaveButton(saveBtn, externalId, displayName, tracks) {
  const updateSaveBtn = (animate) => {
    const isSaved    = state.playlists.some(p => p.externalId === externalId);
    saveBtn.title    = isSaved ? I18n.t('context.removeFromLibrary') : I18n.t('context.saveToLibrary');
    saveBtn.classList.toggle('saved', isSaved);
    saveBtn.innerHTML = isSaved ? SAVE_SVG_CHECK : SAVE_SVG_PLUS;
    if (animate === 'save') {
      saveBtn.classList.add('saving');
      saveBtn.addEventListener('animationend', () => saveBtn.classList.remove('saving'), { once: true });
    }
  };

  saveBtn.style.display = '';
  saveBtn.classList.remove('saving', 'unsaving');
  updateSaveBtn();

  saveBtn.onclick = () => {
    const existing = state.playlists.find(p => p.externalId === externalId);
    if (existing) {
      state.playlists = state.playlists.filter(p => p.externalId !== externalId);
      saveBtn.classList.add('unsaving');
      saveBtn.addEventListener('animationend', () => {
        saveBtn.classList.remove('unsaving');
        updateSaveBtn();
      }, { once: true });
      showToast(I18n.t('toast.removedFromLibrary', { name: displayName }));
    } else {
      const pl      = createPlaylist(displayName);
      pl.externalId = externalId;
      pl.tracks     = tracks;
      updateSaveBtn('save');
      showToast(I18n.t('toast.savedToLibrary', { name: displayName, count: tracks.length }));
    }
    callbacks.saveState();
    renderPlaylists();
  };
}

// ─── showCollectionContextMenu ────────────────────────────────────────────────

export function showCollectionContextMenu(e, externalId, meta, options) {
  const {
    loadTracks, fallbackName = I18n.t('common.playlist'),
    playLabel = I18n.t('playlist.playAll'),
    errorMsg  = I18n.t('toast.couldNotLoadTracks'),
    copyLink  = null,
  } = options;

  removeContextMenu();
  const menu     = document.createElement('div');
  menu.className = 'context-menu';
  menu.style.left = e.clientX + 'px';
  menu.style.top  = e.clientY + 'px';

  const saved = state.playlists.find(p => p.externalId === externalId);

  menu.innerHTML = `
    <div class="context-menu-item" data-action="play">${playLabel}</div>
    <div class="context-menu-item" data-action="shuffle">${I18n.t('context.shufflePlay')}</div>
    <div class="context-menu-item" data-action="start-radio">${I18n.t('context.startRadio')}</div>
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="${saved ? 'remove' : 'save'}">${saved ? I18n.t('context.removeFromLibrary') : I18n.t('context.saveToLibrary')}</div>
    ${copyLink ? `<div class="context-menu-item" data-action="share">${I18n.t('context.copyLink')}</div>` : ''}
  `;

  document.body.appendChild(menu);
  const rect = menu.getBoundingClientRect();
  if (rect.right  > window.innerWidth)  menu.style.left = (window.innerWidth  - rect.width  - 8) + 'px';
  if (rect.bottom > window.innerHeight) menu.style.top  = (window.innerHeight - rect.height - 8) + 'px';

  menu.addEventListener('click', async (ev) => {
    const item = ev.target.closest('.context-menu-item');
    if (!item) return;
    const action = item.dataset.action;

    if (action === 'remove') {
      state.playlists = state.playlists.filter(p => p.externalId !== externalId);
      callbacks.saveState();
      renderPlaylists();
      showToast(I18n.t('toast.removedFromLibrary', { name: meta?.name || fallbackName }));
    } else if (action === 'share' && copyLink) {
      navigator.clipboard.writeText(copyLink);
      showToast(I18n.t('toast.linkCopied'));
    } else if (action === 'start-radio') {
      const tracks = await loadTracks();
      if (!tracks?.length) { showToast(errorMsg); removeContextMenu(); return; }
      await startRadio(tracks[0]);
    } else if (action === 'play' || action === 'shuffle' || action === 'save') {
      const tracks = await loadTracks();
      if (!tracks?.length) { showToast(errorMsg); removeContextMenu(); return; }
      if (action === 'play') {
        playFromList(tracks, 0);
      } else if (action === 'shuffle') {
        playFromList([...tracks].sort(() => Math.random() - 0.5), 0);
      } else if (action === 'save') {
        const name    = meta?.name || fallbackName;
        const pl      = createPlaylist(name);
        pl.externalId = externalId;
        pl.tracks     = tracks;
        callbacks.saveState();
        renderPlaylists();
        showToast(I18n.t('toast.savedToLibrary', { name, count: tracks.length }));
      }
    }
    removeContextMenu();
  });

  setTimeout(() => { document.addEventListener('click', removeContextMenu, { once: true }); }, 10);
}

// ─── showArtistContextMenu ────────────────────────────────────────────────────

export function showArtistContextMenu(e, artistId, artistName) {
  removeContextMenu();
  const menu     = document.createElement('div');
  menu.className = 'context-menu';
  menu.style.left = e.clientX + 'px';
  menu.style.top  = e.clientY + 'px';
  menu.innerHTML = `
    <div class="context-menu-item" data-action="start-radio">${I18n.t('context.startRadio')}</div>
    <div class="context-menu-item" data-action="share">${I18n.t('context.copyLink')}</div>
  `;
  document.body.appendChild(menu);
  const rect = menu.getBoundingClientRect();
  if (rect.right  > window.innerWidth)  menu.style.left = (window.innerWidth  - rect.width  - 8) + 'px';
  if (rect.bottom > window.innerHeight) menu.style.top  = (window.innerHeight - rect.height - 8) + 'px';

  menu.addEventListener('click', async (ev) => {
    const item = ev.target.closest('.context-menu-item');
    if (!item) return;
    if (item.dataset.action === 'start-radio') {
      await startRadio(null, { fromArtistId: artistId });
    } else if (item.dataset.action === 'share') {
      navigator.clipboard.writeText(`https://snowify.cc/artist/${artistId}`);
      showToast(I18n.t('toast.linkCopied'));
    }
    removeContextMenu();
  });

  setTimeout(() => { document.addEventListener('click', removeContextMenu, { once: true }); }, 10);
}

// ─── showAlbumContextMenu / showPlaylistContextMenu ───────────────────────────

export function showAlbumContextMenu(e, albumId, meta) {
  showCollectionContextMenu(e, albumId, meta, {
    loadTracks:   async () => { const a = await window.snowify.albumTracks(albumId); return a?.tracks || []; },
    fallbackName: I18n.t('album.type'),
    playLabel:    I18n.t('context.playAll'),
    errorMsg:     I18n.t('toast.failedLoadAlbum'),
    copyLink:     `https://snowify.cc/album/${albumId}`,
  });
}

export function showPlaylistContextMenu(e, playlistId, meta) {
  showCollectionContextMenu(e, playlistId, meta, {
    loadTracks:   () => window.snowify.getPlaylistVideos(playlistId),
    fallbackName: I18n.t('playlist.type'),
    playLabel:    I18n.t('context.play'),
    errorMsg:     I18n.t('toast.failedLoadPlaylist'),
  });
}
