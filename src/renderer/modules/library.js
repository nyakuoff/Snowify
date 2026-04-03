/**
 * library.js
 * Playlist management, library view, sidebar playlist list, and track-drag helpers.
 */

import state from './state.js';
import { escapeHtml, showToast, showInputModal, resolveImageUrl } from './utils.js';
import { callbacks } from './callbacks.js';
// Circular imports — safe at runtime: all usage is inside function bodies.
import {
  playFromList, togglePlay, updatePlayAllBtn, isCollectionPlaying,
  toggleLike, updateLikedCount, updatePlaylistHighlight,
  SIDEBAR_PLAY_SVG, NOW_PLAYING_ICON_SVG, playTrack,
} from './player.js';
import { getLikedSongsPlaylist, handlePlayNext, handleAddToQueue, startRadio } from './queue.js';
import { renderTrackList, showContextMenu, removeContextMenu, findTrackAlternatives } from './context-menus.js';
import { bindArtistLinks, openArtistPage } from './artist.js';
import { openSpotifyImport } from './csv-import.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);

// ─── Drag state (shared with context-menus via exports) ───────────────────────
let _dragActive  = false;
let _draggedTrack = null;

export function isDragActive()           { return _dragActive; }
export function startTrackDrag(e, track) {
  _draggedTrack = track;
  _dragActive   = true;
  e.dataTransfer.effectAllowed = 'copy';
  e.dataTransfer.setData('text/plain', track.title);
  const el = e.target.closest('.track-row, .track-card');
  if (el) el.classList.add('dragging');
  document.querySelectorAll('.playlist-item').forEach(p => p.classList.add('drop-target'));
}

document.addEventListener('dragend', () => {
  _dragActive   = false;
  _draggedTrack = null;
  document.querySelectorAll('.dragging').forEach(el   => el.classList.remove('dragging'));
  document.querySelectorAll('.drop-target').forEach(el => el.classList.remove('drop-target'));
  document.querySelectorAll('.drag-over').forEach(el  => el.classList.remove('drag-over'));
});

function handleTrackDrop(e, playlistId) {
  const track = _draggedTrack;
  if (!track || !track.id) return;

  if (playlistId === 'liked') {
    if (state.likedSongs.some(t => t.id === track.id)) {
      showToast(I18n.t('toast.alreadyInLiked'));
      return;
    }
    state.likedSongs.push(track);
    callbacks.saveState();
    updateLikedCount();
    showToast(I18n.t('toast.addedToLiked'));
  } else {
    const pl = state.playlists.find(p => p.id === playlistId);
    if (!pl) return;
    if (pl.tracks.some(t => t.id === track.id)) {
      showToast(I18n.t('toast.alreadyInPlaylist', { name: pl.name }));
      return;
    }
    pl.tracks.push(track);
    callbacks.saveState();
    renderPlaylists();
    showToast(I18n.t('toast.addedToPlaylist', { name: pl.name }));
  }
}

// ─── Cover helpers ────────────────────────────────────────────────────────────

export function getPlaylistCoverHtml(playlist, size = 'normal') {
  const sizeClass = size === 'large' ? ' playlist-cover-lg' : '';
  if (playlist.coverImage) {
    const normalized = playlist.coverImage.replace(/\\/g, '/');
    const fileUrl    = normalized.startsWith('/') ? `file://${encodeURI(normalized)}` : `file:///${encodeURI(normalized)}`;
    return `<img src="${fileUrl}" alt="" />`;
  }
  if (playlist.tracks.length >= 4) {
    const thumbs = playlist.tracks.slice(0, 4).map(t => t.thumbnail);
    return `<div class="playlist-cover-grid${sizeClass}">${thumbs.map(t => `<img data-src="${escapeHtml(t)}" alt="" />`).join('')}</div>`;
  }
  if (playlist.tracks.length > 0) {
    return `<img data-src="${escapeHtml(playlist.tracks[0].thumbnail)}" alt="" />`;
  }
  const iconSize = size === 'large' ? 64 : size === 'lib' ? 32 : 20;
  return `<svg width="${iconSize}" height="${iconSize}" viewBox="0 0 24 24" fill="#535353"><path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55C7.79 13 6 14.79 6 17s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"/></svg>`;
}

// ─── Playlist CRUD ────────────────────────────────────────────────────────────

export function createPlaylist(name) {
  const id       = 'pl_' + Date.now();
  const playlist = { id, name: name || `My Playlist #${state.playlists.length + 1}`, tracks: [] };
  state.playlists.push(playlist);
  callbacks.saveState();
  renderPlaylists();
  renderLibrary();
  showToast(I18n.t('toast.createdPlaylist', { name: playlist.name }));
  return playlist;
}

export async function changePlaylistCover(playlist) {
  const filePath   = await window.snowify.pickImage();
  if (!filePath) return;
  if (playlist.coverImage) await window.snowify.deleteImage(playlist.coverImage);
  const savedPath  = await window.snowify.saveImage(playlist.id, filePath);
  if (savedPath) {
    playlist.coverImage = savedPath;
    callbacks.saveState();
    renderPlaylists();
    renderLibrary();
    showToast(I18n.t('toast.coverUpdated'));
    if (state.currentPlaylistId === playlist.id) showPlaylistDetail(playlist, false);
  } else {
    showToast(I18n.t('toast.failedSaveImage'));
  }
}

export async function removePlaylistCover(playlist) {
  if (playlist.coverImage) {
    await window.snowify.deleteImage(playlist.coverImage);
    delete playlist.coverImage;
    callbacks.saveState();
    renderPlaylists();
    renderLibrary();
    showToast(I18n.t('toast.coverRemoved'));
    if (state.currentPlaylistId === playlist.id) showPlaylistDetail(playlist, false);
  }
}

function refreshPlaylistUi(playlist) {
  renderPlaylists();
  renderLibrary();
  if (state.currentView === 'playlist' && state.currentPlaylistId === playlist.id) {
    showPlaylistDetail(playlist, false);
  }
}

async function renameUserPlaylist(playlist) {
  const newName = await showInputModal(I18n.t('modal.renamePlaylist'), playlist.name);
  if (!newName || newName === playlist.name) return false;
  playlist.name = newName;
  callbacks.saveState();
  refreshPlaylistUi(playlist);
  showToast(I18n.t('toast.renamedTo', { name: playlist.name }));
  return true;
}

async function exportUserPlaylist(playlist) {
  if (!playlist.tracks.length) { showToast(I18n.t('toast.playlistEmpty')); return false; }
  const ok = await window.snowify.exportPlaylistCsv(playlist.name, playlist.tracks);
  if (ok) showToast(I18n.t('toast.playlistExported'));
  return ok;
}

async function importFilesIntoPlaylist(playlist) {
  const picked = await window.snowify.pickAudioFiles();
  if (!picked || !picked.length) return 0;
  let added = 0;
  for (const t of picked) {
    let track = t;
    if (playlist.folderPath) {
      const copied = await window.snowify.copyToPlaylistFolder(t.localPath, playlist.folderPath);
      if (copied) track = copied;
    }
    if (!playlist.tracks.some(pt => pt.id === track.id)) { playlist.tracks.push(track); added++; }
  }
  if (!added) { showToast('Files already in playlist'); return 0; }
  callbacks.saveState();
  refreshPlaylistUi(playlist);
  showToast(playlist.folderPath
    ? I18n.t('toast.fileCopiedToFolder')
    : `Added ${added} local file${added > 1 ? 's' : ''}`);
  return added;
}

export async function refreshFolderPlaylist(playlist, { silent = false } = {}) {
  if (!playlist.folderPath) return 0;
  const scanned = await window.snowify.scanAudioFolder(playlist.folderPath).catch(() => null);
  if (!scanned) {
    if (!silent) showToast(I18n.t('toast.folderNotFound'));
    return 0;
  }
  const existing  = new Set(playlist.tracks.map(t => t.id));
  const newTracks = scanned.filter(t => !existing.has(t.id));
  if (!newTracks.length) {
    if (!silent) showToast(I18n.t('toast.folderNoNewTracks'));
    return 0;
  }
  playlist.tracks.push(...newTracks);
  callbacks.saveState();
  refreshPlaylistUi(playlist);
  if (!silent) showToast(I18n.t('toast.folderRefreshed', { count: newTracks.length }));
  return newTracks.length;
}

// ─── Sidebar playlist menu ────────────────────────────────────────────────────

function showSidebarPlaylistMenu(e, playlist, isLiked = false) {
  removeContextMenu();
  const isMobile = document.documentElement.classList.contains('platform-mobile');
  const menu     = document.createElement('div');
  menu.className = 'context-menu';
  if (!isMobile) {
    menu.style.left = e.clientX + 'px';
    menu.style.top  = e.clientY + 'px';
  }

  const manageHtml = isLiked ? '' : `
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="import-files">${I18n.t('playlist.importLocalFiles')}</div>
    ${playlist.folderPath ? `<div class="context-menu-item" data-action="update-files">${I18n.t('playlist.updateFiles')}</div>` : ''}
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="change-cover">${I18n.t('context.changeCover')}</div>
    ${playlist.coverImage ? `<div class="context-menu-item" data-action="remove-cover">${I18n.t('context.removeCover')}</div>` : ''}
    <div class="context-menu-item" data-action="rename">${I18n.t('context.rename')}</div>
    <div class="context-menu-item" data-action="delete" style="color:var(--red)">${I18n.t('context.delete')}</div>`;

  let mobileHeader = '';
  if (isMobile) {
    const coverHtml = isLiked
      ? `<svg width="40" height="40" viewBox="0 0 24 24" fill="#fff"><path d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z"/></svg>`
      : getPlaylistCoverHtml(playlist, 'normal');
    const trackCount = isLiked ? I18n.tp('sidebar.songCount', playlist.tracks.length) : I18n.tp('sidebar.songCount', playlist.tracks.length);
    mobileHeader = `
      <div class="ctx-sheet-handle"></div>
      <div class="ctx-sheet-header">
        <div class="ctx-sheet-track">
          <div class="ctx-sheet-thumb" style="overflow:hidden;border-radius:6px;display:flex;align-items:center;justify-content:center;background:var(--surface2)">${coverHtml}</div>
          <div class="ctx-sheet-info">
            <div class="ctx-sheet-title">${escapeHtml(playlist.name)}</div>
            <div class="ctx-sheet-artist">${trackCount}</div>
          </div>
        </div>
      </div>
      <div class="context-menu-divider"></div>`;
  }

  menu.innerHTML = mobileHeader + `
    <div class="context-menu-item" data-action="play">${I18n.t('context.play')}</div>
    <div class="context-menu-item" data-action="shuffle">${I18n.t('context.shufflePlay')}</div>
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="export-csv">${I18n.t('playlist.exportCsv')}</div>
    ${manageHtml}
  `;

  document.body.appendChild(menu);

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
    const rect = menu.getBoundingClientRect();
    if (rect.right  > window.innerWidth)  menu.style.left = (window.innerWidth  - rect.width  - 8) + 'px';
    if (rect.bottom > window.innerHeight) menu.style.top  = (window.innerHeight - rect.height - 8) + 'px';
    setTimeout(() => { document.addEventListener('click', removeContextMenu, { once: true }); }, 10);
  }

  menu.addEventListener('click', async (ev) => {
    const item = ev.target.closest('.context-menu-item');
    if (!item) return;
    switch (item.dataset.action) {
      case 'play':
        if (playlist.tracks.length) playFromList(playlist.tracks, 0, playlist.id);
        else showToast(I18n.t('toast.playlistEmpty'));
        break;
      case 'shuffle':
        if (playlist.tracks.length) {
          const shuffled = [...playlist.tracks];
          for (let i = shuffled.length - 1; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
          }
          playFromList(shuffled, 0, playlist.id);
        } else showToast(I18n.t('toast.playlistEmpty'));
        break;
      case 'rename': { removeContextMenu(); await renameUserPlaylist(playlist); return; }
      case 'import-files': { removeContextMenu(); await importFilesIntoPlaylist(playlist); return; }
      case 'update-files': { removeContextMenu(); await refreshFolderPlaylist(playlist); return; }
      case 'change-cover': { removeContextMenu(); await changePlaylistCover(playlist); return; }
      case 'remove-cover': { removeContextMenu(); await removePlaylistCover(playlist); return; }
      case 'export-csv':   { removeContextMenu(); await exportUserPlaylist(playlist);    return; }
      case 'delete':
        if (confirm(I18n.t('playlist.confirmDelete', { name: playlist.name }))) {
          if (playlist.coverImage) window.snowify.deleteImage(playlist.coverImage);
          state.playlists = state.playlists.filter(p => p.id !== playlist.id);
          if (state.playingPlaylistId === playlist.id) state.playingPlaylistId = null;
          callbacks.saveState();
          renderPlaylists();
          renderLibrary();
          if (state.currentPlaylistId === playlist.id) callbacks.switchView('library');
          showToast(I18n.t('toast.deletedPlaylist', { name: playlist.name }));
        }
        break;
    }
    removeContextMenu();
  });
}

// ─── renderPlaylists ──────────────────────────────────────────────────────────

export function renderPlaylists() {
  const container = $('#playlist-list');
  let html = `
    <div class="playlist-item" data-playlist="liked">
      <div class="playlist-cover liked-cover">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="rgba(255,255,255,0.88)"><path d="M12 2L14.8 9.2L22 12L14.8 14.8L12 22L9.2 14.8L2 12L9.2 9.2Z"/></svg>
        <div class="playlist-cover-overlay">${SIDEBAR_PLAY_SVG}</div>
      </div>
      <div class="playlist-info">
        <span class="playlist-name">${I18n.t('sidebar.likedSongs')}</span>
        <span class="playlist-count">${I18n.tp('sidebar.songCount', state.likedSongs.length)}</span>
      </div>
      ${NOW_PLAYING_ICON_SVG}
    </div>`;

  state.playlists.forEach(pl => {
    html += `
      <div class="playlist-item" data-playlist="${pl.id}">
        <div class="playlist-cover">
          ${getPlaylistCoverHtml(pl, 'normal')}
          <div class="playlist-cover-overlay">${SIDEBAR_PLAY_SVG}</div>
        </div>
        <div class="playlist-info">
          <span class="playlist-name">${escapeHtml(pl.name)}</span>
          <span class="playlist-count">${I18n.tp('sidebar.songCount', pl.tracks.length)}</span>
        </div>
        ${NOW_PLAYING_ICON_SVG}
      </div>`;
  });

  container.innerHTML = html;
  container.querySelectorAll('.playlist-item').forEach(item => {
    item.addEventListener('click', () => {
      if (_dragActive) return;
      const pid = item.dataset.playlist;
      if (pid === 'liked') {
        showPlaylistDetail(getLikedSongsPlaylist(), true);
      } else {
        const pl = state.playlists.find(p => p.id === pid);
        if (pl) showPlaylistDetail(pl, false);
      }
    });

    item.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      const pid = item.dataset.playlist;
      if (pid === 'liked') {
        showSidebarPlaylistMenu(e, getLikedSongsPlaylist(), true);
      } else {
        const pl = state.playlists.find(p => p.id === pid);
        if (pl) showSidebarPlaylistMenu(e, pl, false);
      }
    });

    // Long-press on mobile triggers the playlist menu.
    let _lpTimer = null;
    item.addEventListener('touchstart', (e) => {
      _lpTimer = setTimeout(() => {
        _lpTimer = null;
        const pid = item.dataset.playlist;
        if (pid === 'liked') {
          showSidebarPlaylistMenu(e, getLikedSongsPlaylist(), true);
        } else {
          const pl = state.playlists.find(p => p.id === pid);
          if (pl) showSidebarPlaylistMenu(e, pl, false);
        }
      }, 500);
    }, { passive: true });
    item.addEventListener('touchmove',  () => { clearTimeout(_lpTimer); _lpTimer = null; }, { passive: true });
    item.addEventListener('touchend',   () => { clearTimeout(_lpTimer); _lpTimer = null; }, { passive: true });
    item.addEventListener('touchcancel',() => { clearTimeout(_lpTimer); _lpTimer = null; }, { passive: true });

    const overlay = item.querySelector('.playlist-cover-overlay');
    if (overlay) {
      overlay.addEventListener('click', (e) => {
        e.stopPropagation();
        const pid    = item.dataset.playlist;
        const tracks = pid === 'liked' ? state.likedSongs : state.playlists.find(p => p.id === pid)?.tracks;
        if (!tracks || !tracks.length) return;
        if (state.playingPlaylistId === pid) {
          togglePlay();
        } else {
          playFromList(tracks, 0, pid);
        }
      });
    }

    item.addEventListener('dragover',  (e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'copy'; item.classList.add('drag-over'); });
    item.addEventListener('dragenter', (e) => { e.preventDefault(); item.classList.add('drag-over'); });
    item.addEventListener('dragleave', ()  => { item.classList.remove('drag-over'); });
    item.addEventListener('drop',      (e) => { e.preventDefault(); item.classList.remove('drag-over'); handleTrackDrop(e, item.dataset.playlist); });
  });
  updatePlaylistHighlight();
}

// ─── renderSidebarArtists ─────────────────────────────────────────────────────

export function renderSidebarArtists() {
  const section   = document.getElementById('section-followed-artists');
  const container = document.getElementById('artist-list');
  if (!section || !container) return;
  if (!state.followedArtists.length) {
    section.style.display = 'none';
    return;
  }
  section.style.display = '';
  container.innerHTML = state.followedArtists.map(a => `
    <div class="artist-item" data-artist-id="${escapeHtml(a.artistId)}">
      <div class="artist-item-avatar">
        ${a.avatar
          ? `<img src="${escapeHtml(a.avatar)}" alt="" />`
          : `<svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z"/></svg>`}
      </div>
      <span class="artist-item-name">${escapeHtml(a.name)}</span>
    </div>
  `).join('');
  container.querySelectorAll('.artist-item').forEach(item => {
    item.addEventListener('click', () => openArtistPage(item.dataset.artistId));
  });
}

// ─── showPlaylistDetail ───────────────────────────────────────────────────────

export function showPlaylistDetail(playlist, isLiked) {
  state.currentPlaylistId = playlist.id;
  callbacks.switchView('playlist');

  const heroName    = $('#playlist-hero-name');
  const heroCount   = $('#playlist-hero-count');
  const heroCover   = $('#playlist-hero-cover');
  const heroHeader  = $('#view-playlist .playlist-header');
  const tracksContainer = $('#playlist-tracks');

  heroName.textContent  = playlist.name;
  heroCount.textContent = I18n.tp('sidebar.songCount', playlist.tracks.length);

  if (isLiked) {
    heroCover.innerHTML = `<svg width="64" height="64" viewBox="0 0 24 24" fill="rgba(255,255,255,0.88)"><path d="M12 2L14.8 9.2L22 12L14.8 14.8L12 22L9.2 14.8L2 12L9.2 9.2Z"/></svg>`;
    heroCover.style.background = 'linear-gradient(135deg, #0e0820 0%, #2c0f65 55%, #6d28d9 100%)';
    heroCover.classList.add('liked-hero-cover');
  } else {
    heroCover.classList.remove('liked-hero-cover');
    const coverContent = getPlaylistCoverHtml(playlist, 'large');
    const hasCover     = playlist.coverImage || playlist.tracks.length > 0;
    heroCover.innerHTML        = coverContent;
    heroCover.style.background = hasCover ? '' : 'linear-gradient(135deg, #450af5, #8e2de2)';
  }
  heroCover.classList.toggle('playlist-cover-editable', !isLiked);
  heroCover.title  = isLiked ? '' : I18n.t('playlist.changeCover');
  heroCover.onclick = isLiked ? null : async () => { await changePlaylistCover(playlist); };
  if (heroHeader) {
    heroHeader.oncontextmenu = isLiked ? null : (e) => { e.preventDefault(); showSidebarPlaylistMenu(e, playlist, false); };
  }

  const deleteBtn = $('#btn-delete-playlist');
  const folderBtn = $('#btn-import-folder');
  deleteBtn.style.display = isLiked ? 'none' : '';
  folderBtn.textContent    = I18n.t('playlist.updateFiles');
  folderBtn.title          = I18n.t('playlist.updateFiles');
  folderBtn.style.display  = (!isLiked && playlist.folderPath) ? '' : 'none';

  if (!isLiked && playlist.folderPath) {
    refreshFolderPlaylist(playlist, { silent: true });
  }

  if (playlist.tracks.length) {
    renderTrackList(tracksContainer, playlist.tracks, 'playlist', playlist.id);
    tracksContainer.querySelectorAll('.track-row').forEach(row => {
      row.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        const idx   = parseInt(row.dataset.index);
        const track = playlist.tracks[idx];
        showPlaylistTrackMenu(e, track, playlist, isLiked, idx);
      });
    });
  } else {
    tracksContainer.innerHTML = `
      <div class="empty-state">
        <p>${I18n.t('playlist.empty')}</p>
        <p>${I18n.t('playlist.emptyHint')}</p>
      </div>`;
  }

  const playAllBtn = $('#btn-play-all');
  updatePlayAllBtn(playAllBtn, playlist.tracks, playlist.id);
  playAllBtn.onclick = () => {
    if (!playlist.tracks.length) return;
    if (isCollectionPlaying(playlist.tracks, playlist.id)) {
      togglePlay();
      updatePlayAllBtn(playAllBtn, playlist.tracks, playlist.id);
    } else {
      playFromList(playlist.tracks, 0, playlist.id);
      updatePlayAllBtn(playAllBtn, playlist.tracks, playlist.id);
    }
  };

  $('#btn-shuffle-playlist').onclick = () => {
    if (playlist.tracks.length) {
      const shuffled = [...playlist.tracks];
      for (let i = shuffled.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
      }
      playFromList(shuffled, 0, playlist.id);
    }
  };

  folderBtn.onclick = async () => { await refreshFolderPlaylist(playlist); };

  deleteBtn.onclick = () => {
    if (isLiked) return;
    if (confirm(I18n.t('playlist.confirmDelete', { name: playlist.name }))) {
      if (playlist.coverImage) window.snowify.deleteImage(playlist.coverImage);
      state.playlists = state.playlists.filter(p => p.id !== playlist.id);
      if (state.playingPlaylistId === playlist.id) state.playingPlaylistId = null;
      callbacks.saveState();
      renderPlaylists();
      renderLibrary();
      callbacks.switchView('library');
      showToast(I18n.t('toast.deletedPlaylist', { name: playlist.name }));
    }
  };
}

// ─── showPlaylistTrackMenu ────────────────────────────────────────────────────

export function showPlaylistTrackMenu(e, track, playlist, isLiked, idx) {
  removeContextMenu();
  const isMobile = document.documentElement.classList.contains('platform-mobile');
  const liked    = state.likedSongs.some(t => t.id === track.id);
  const isLocal  = !!track.isLocal;

  const menu     = document.createElement('div');
  menu.className = 'context-menu';
  if (!isMobile) {
    menu.style.left = e.clientX + 'px';
    menu.style.top  = e.clientY + 'px';
  }

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
    <div class="context-menu-item" data-action="play-next">${I18n.t('context.playNext')}</div>
    <div class="context-menu-item" data-action="add-queue">${I18n.t('context.addToQueue')}</div>
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="like">${liked ? I18n.t('context.unlike') : I18n.t('context.like')}</div>
    <div class="context-menu-item" data-action="find-alt">${I18n.t('context.findOtherVersions')}</div>
    ${isLocal ? '' : `<div class="context-menu-item" data-action="start-radio">${I18n.t('context.startRadio')}</div>`}
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="remove">${isLiked ? I18n.t('context.removeFromLiked') : I18n.t('context.removeFromPlaylist')}</div>
    ${!isLiked && idx > 0 ? `<div class="context-menu-item" data-action="move-up">${I18n.t('context.moveUp')}</div>` : ''}
    ${!isLiked && idx < playlist.tracks.length - 1 ? `<div class="context-menu-item" data-action="move-down">${I18n.t('context.moveDown')}</div>` : ''}
    ${isLocal ? '' : `<div class="context-menu-divider"></div><div class="context-menu-item" data-action="share">${I18n.t('context.copyLink')}</div>`}
  `;

  if (isMobile) {
    document.body.appendChild(menu);
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
      touchStartY = ev.touches[0].clientY;
      touchLastY  = touchStartY;
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
    document.body.appendChild(menu);
    const rect = menu.getBoundingClientRect();
    if (rect.right  > window.innerWidth)  menu.style.left = (window.innerWidth  - rect.width  - 8) + 'px';
    if (rect.bottom > window.innerHeight) menu.style.top  = (window.innerHeight - rect.height - 8) + 'px';
    setTimeout(() => { document.addEventListener('click', removeContextMenu, { once: true }); }, 10);
  }

  menu.addEventListener('click', async (ev) => {
    const item = ev.target.closest('.context-menu-item');
    if (!item) return;
    const action = item.dataset.action;
    switch (action) {
      case 'play':
        state.playingPlaylistId = null;
        playTrack(track);
        updatePlaylistHighlight();
        break;
      case 'play-next':  handlePlayNext(track); break;
      case 'add-queue':  handleAddToQueue(track); break;
      case 'like':       toggleLike(track); break;
      case 'find-alt':   findTrackAlternatives(track, { playlist, isLiked, idx }); break;
      case 'start-radio': await startRadio(track); break;
      case 'remove':
        if (isLiked) {
          state.likedSongs = state.likedSongs.filter(t => t.id !== track.id);
          callbacks.saveState();
          updateLikedCount();
          showPlaylistDetail(getLikedSongsPlaylist(), true);
          showToast(I18n.t('toast.removedFromLiked'));
        } else {
          playlist.tracks.splice(idx, 1);
          callbacks.saveState();
          renderPlaylists();
          showPlaylistDetail(playlist, false);
          showToast(I18n.t('toast.removedFromPlaylistShort'));
        }
        break;
      case 'move-up':
        [playlist.tracks[idx - 1], playlist.tracks[idx]] = [playlist.tracks[idx], playlist.tracks[idx - 1]];
        callbacks.saveState();
        showPlaylistDetail(playlist, false);
        break;
      case 'move-down':
        [playlist.tracks[idx], playlist.tracks[idx + 1]] = [playlist.tracks[idx + 1], playlist.tracks[idx]];
        callbacks.saveState();
        showPlaylistDetail(playlist, false);
        break;
      case 'share':
        navigator.clipboard.writeText(`https://snowify.cc/track/${track.id}`);
        showToast(I18n.t('toast.linkCopied'));
        break;
    }
    removeContextMenu();
  });
}

// ─── Sidebar button listeners ─────────────────────────────────────────────────

$('#btn-create-playlist').addEventListener('click', async () => {
  const name = await showInputModal(I18n.t('modal.createPlaylist'), I18n.t('modal.defaultPlaylistName'));
  if (name) createPlaylist(name);
});
$('#btn-lib-create-playlist')?.addEventListener('click', async () => {
  const name = await showInputModal(I18n.t('modal.createPlaylist'), I18n.t('modal.defaultPlaylistName'));
  if (name) createPlaylist(name);
});
$('#btn-lib-create-playlist-top')?.addEventListener('click', async () => {
  const name = await showInputModal(I18n.t('modal.createPlaylist'), I18n.t('modal.defaultPlaylistName'));
  if (name) createPlaylist(name);
});
$('#btn-import-folder-playlist')?.addEventListener('click', async () => {
  const result = await window.snowify.pickAudioFolder();
  if (!result) return;
  const { folderPath, name, tracks } = result;
  if (!tracks.length) return showToast('No audio files found in that folder');
  const id       = 'pl_' + Date.now();
  const playlist = { id, name, tracks, folderPath };
  state.playlists.push(playlist);
  callbacks.saveState();
  renderPlaylists();
  renderLibrary();
  showToast(I18n.t('toast.folderImported', { count: tracks.length }));
  showPlaylistDetail(playlist, false);
  callbacks.switchView('playlist');
});
$('#btn-spotify-import').addEventListener('click', () => openSpotifyImport({ createPlaylist, renderPlaylists, renderLibrary }));
$('#btn-lib-spotify-import')?.addEventListener('click', () => openSpotifyImport({ createPlaylist, renderPlaylists, renderLibrary }));

// ─── renderLibrary ────────────────────────────────────────────────────────────

export function renderLibrary() {
  const container   = $('#library-content');
  const allPlaylists = [
    { ...getLikedSongsPlaylist(), isLiked: true },
    ...state.playlists.map(p => ({ ...p, isLiked: false })),
  ];

  if (!allPlaylists.some(p => p.tracks.length) && state.playlists.length === 0) {
    container.innerHTML = `
      <div class="empty-state">
        <svg width="64" height="64" viewBox="0 0 24 24" fill="#535353"><path d="M4 4h2v16H4V4zm5 0h2v16H9V4zm5 2h2v14h-2V6zm5-2h2v16h-2V4z"/></svg>
        <h3>${I18n.t('library.emptyTitle')}</h3>
        <p>${I18n.t('library.emptyDesc')}</p>
        <button class="btn-primary" id="btn-lib-create-playlist-2">${I18n.t('library.createPlaylist')}</button>
      </div>`;
    $('#btn-lib-create-playlist-2')?.addEventListener('click', async () => {
      const name = await showInputModal(I18n.t('modal.createPlaylist'), I18n.t('modal.defaultPlaylistName'));
      if (name) { createPlaylist(name); renderLibrary(); }
    });
    return;
  }

  container.innerHTML = `<div class="library-grid">${allPlaylists.map(p => {
    const coverHtml = p.isLiked
      ? `<div class="lib-card-cover liked-cover"><svg width="32" height="32" viewBox="0 0 24 24" fill="#fff"><path d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z"/></svg></div>`
      : `<div class="lib-card-cover">${getPlaylistCoverHtml(p, 'lib')}</div>`;
    return `
      <div class="lib-card" data-playlist="${p.id}">
        ${coverHtml}
        <div class="lib-card-name">${escapeHtml(p.name)}</div>
        <div class="lib-card-meta">${I18n.t('common.playlist')} \u00b7 ${I18n.tp('sidebar.songCount', p.tracks.length)}</div>
      </div>`;
  }).join('')}</div>`;

  container.querySelectorAll('.lib-card').forEach(card => {
    card.addEventListener('click', () => {
      const pid = card.dataset.playlist;
      if (pid === 'liked') {
        showPlaylistDetail(getLikedSongsPlaylist(), true);
      } else {
        const pl = state.playlists.find(p => p.id === pid);
        if (pl) showPlaylistDetail(pl, false);
      }
    });
    card.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      const pid = card.dataset.playlist;
      if (pid === 'liked') {
        showSidebarPlaylistMenu(e, { id: 'liked', name: 'Liked Songs', tracks: state.likedSongs }, true);
      } else {
        const pl = state.playlists.find(p => p.id === pid);
        if (pl) showSidebarPlaylistMenu(e, pl, false);
      }
    });

    // Long-press on mobile triggers the playlist menu.
    let _lpTimer = null;
    card.addEventListener('touchstart', (e) => {
      _lpTimer = setTimeout(() => {
        _lpTimer = null;
        const pid = card.dataset.playlist;
        if (pid === 'liked') {
          showSidebarPlaylistMenu(e, { id: 'liked', name: 'Liked Songs', tracks: state.likedSongs }, true);
        } else {
          const pl = state.playlists.find(p => p.id === pid);
          if (pl) showSidebarPlaylistMenu(e, pl, false);
        }
      }, 500);
    }, { passive: true });
    card.addEventListener('touchmove',  () => { clearTimeout(_lpTimer); _lpTimer = null; }, { passive: true });
    card.addEventListener('touchend',   () => { clearTimeout(_lpTimer); _lpTimer = null; }, { passive: true });
    card.addEventListener('touchcancel',() => { clearTimeout(_lpTimer); _lpTimer = null; }, { passive: true });
  });
}
