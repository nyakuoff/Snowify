/**
 * search.js
 * Search UI: suggestions, history, result sections, floating search visibility.
 */

import state from './state.js';
import { escapeHtml, addScrollArrows, showToast, renderArtistLinks } from './utils.js';
import { callbacks } from './callbacks.js';
// Circular imports — safe inside function bodies only
import { playFromList } from './player.js';
import { renderTrackList, showArtistContextMenu, showAlbumContextMenu, showPlaylistContextMenu } from './context-menus.js';
import { openVideoPlayer, showVideoContextMenu } from './video-player.js';
import { openArtistPage, bindArtistLinks } from './artist.js';
import { showAlbumDetail } from './album.js';
import { showExternalPlaylistDetail } from './album.js';

// ─── Helpers ─────────────────────────────────────────────────────────────────

const $  = (sel, ctx = document) => ctx.querySelector(sel);
const $$ = (sel, ctx = document) => [...ctx.querySelectorAll(sel)];

// ─── SVG constants ────────────────────────────────────────────────────────────

const ICON_CLOCK  = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>';
const ICON_SEARCH = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="M16 16l4.5 4.5" stroke-linecap="round"/></svg>';
const ICON_TRASH  = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>';

// ─── DOM refs ─────────────────────────────────────────────────────────────────

const searchInput       = $('#search-input');
const searchClear       = $('#search-clear');
const searchResults     = $('#search-results');
const searchSuggestions = $('#search-suggestions');
const floatingSearch    = $('#floating-search');
const searchShortcutHint = $('#search-shortcut-hint');

// ─── Timers / state ───────────────────────────────────────────────────────────

let searchTimeout       = null;
let suggestionsTimeout  = null;
let activeSuggestionIndex = -1;
let _searchGeneration   = 0;

state.musicOnly = true;

// ─── History ──────────────────────────────────────────────────────────────────

export function addToSearchHistory(query) {
  const q = query.trim();
  if (!q) return;
  state.searchHistory = state.searchHistory.filter(h => h.toLowerCase() !== q.toLowerCase());
  state.searchHistory.unshift(q);
  callbacks.saveState();
}

// ─── Floating search bar visibility ──────────────────────────────────────────

export function syncSearchHint() {
  if (searchShortcutHint) searchShortcutHint.classList.toggle('hidden', !!searchInput.value.trim());
}

export function updateFloatingSearch() {
  const show = ['home', 'explore', 'library', 'artist', 'album', 'playlist'].includes(state.currentView);
  floatingSearch.classList.toggle('hidden', !show);
}

// ─── Suggestions ─────────────────────────────────────────────────────────────

export function closeSuggestions() {
  searchSuggestions.classList.add('hidden');
  searchSuggestions.innerHTML = '';
  activeSuggestionIndex = -1;
}

function clearSearch() {
  searchInput.value = '';
  searchClear.classList.add('hidden');
  syncSearchHint();
  closeSuggestions();
}

function renderSuggestionDropdown(items) {
  if (!items.length) { closeSuggestions(); return; }
  activeSuggestionIndex = -1;

  const lastTextIdx = items.reduce((acc, item, i) =>
    (item.type === 'history' || item.type === 'text') ? i : acc, -1);
  const hasDirectResults = items.some(item => item.type === 'artist' || item.type === 'album' || item.type === 'song');

  let idx = 0;
  searchSuggestions.innerHTML = items.map((item, i) => {
    let separator = '';
    if (hasDirectResults && lastTextIdx >= 0 && i === lastTextIdx + 1) {
      separator = '<div class="suggestion-separator"></div>';
    }
    const dataIdx = idx++;
    if (item.type === 'artist') {
      return separator + `<div class="search-suggestion-item" data-index="${dataIdx}" data-type="artist" data-artist-id="${escapeHtml(item.artistId || '')}">
        <img class="suggestion-thumb suggestion-thumb-round" data-src="${escapeHtml(item.thumbnail || '')}" alt="" />
        <div class="suggestion-info">
          <div class="suggestion-title">${escapeHtml(item.name)}</div>
          <div class="suggestion-subtitle">${I18n.t('search.artist')}${item.subtitle ? ' \u00b7 ' + escapeHtml(item.subtitle) : ''}</div>
        </div>
      </div>`;
    }
    if (item.type === 'album') {
      return separator + `<div class="search-suggestion-item" data-index="${dataIdx}" data-type="album" data-album-id="${escapeHtml(item.albumId || '')}" data-item-idx="${i}">
        <img class="suggestion-thumb" data-src="${escapeHtml(item.thumbnail || '')}" alt="" />
        <div class="suggestion-info">
          <div class="suggestion-title">${escapeHtml(item.name)}</div>
          <div class="suggestion-subtitle">${I18n.t('search.album')}${item.subtitle ? ' \u00b7 ' + escapeHtml(item.subtitle) : ''}</div>
        </div>
      </div>`;
    }
    if (item.type === 'song') {
      return separator + `<div class="search-suggestion-item" data-index="${dataIdx}" data-type="song" data-song-idx="${i}">
        <img class="suggestion-thumb" data-src="${escapeHtml(item.thumbnail || '')}" alt="" />
        <div class="suggestion-info">
          <div class="suggestion-title">${escapeHtml(item.title)}</div>
          <div class="suggestion-subtitle">${I18n.t('search.song')} \u00b7 ${renderArtistLinks(item)}</div>
        </div>
      </div>`;
    }
    return `<div class="search-suggestion-item" data-index="${dataIdx}" data-type="${item.type}" data-text="${escapeHtml(item.text)}">
      <span class="search-suggestion-icon">${item.type === 'history' ? ICON_CLOCK : ICON_SEARCH}</span>
      <span class="search-suggestion-text">${escapeHtml(item.text)}</span>
      ${item.type === 'history' ? `<button class="search-suggestion-delete" data-query="${escapeHtml(item.text)}" title="${I18n.t('common.remove')}">${ICON_TRASH}</button>` : ''}
    </div>`;
  }).join('');

  searchSuggestions.insertAdjacentHTML('beforeend',
    '<div class="suggestions-hint-bar">' +
      `<span class="suggestions-hint"><kbd>\u2191</kbd><kbd>\u2193</kbd> ${I18n.t('search.hintNavigate')}</span>` +
      `<span class="suggestions-hint"><kbd>Enter</kbd> ${I18n.t('search.hintSearch')}</span>` +
    '</div>');
  searchSuggestions.classList.remove('hidden');

  // Bind artist-link clicks inside song suggestions
  bindArtistLinks(searchSuggestions);
  searchSuggestions.querySelectorAll('.artist-link[data-artist-id]').forEach(link => {
    link.addEventListener('click', () => {
      const q = searchInput.value.trim();
      if (q) addToSearchHistory(q);
      clearSearch();
    });
  });

  $$('.search-suggestion-item', searchSuggestions).forEach(el => {
    el.addEventListener('click', (e) => {
      if (e.target.closest('.search-suggestion-delete')) return;
      const type = el.dataset.type;
      const q = searchInput.value.trim();
      if (type === 'artist') {
        if (q) addToSearchHistory(q);
        clearSearch();
        openArtistPage(el.dataset.artistId);
      } else if (type === 'album') {
        const albumItem = items[parseInt(el.dataset.itemIdx)];
        if (q) addToSearchHistory(q);
        clearSearch();
        showAlbumDetail(el.dataset.albumId, albumItem ? { name: albumItem.name, thumbnail: albumItem.thumbnail } : null);
      } else if (type === 'song') {
        const songItem = items[parseInt(el.dataset.songIdx)];
        if (songItem) {
          if (q) addToSearchHistory(q);
          clearSearch();
          playFromList([songItem], 0);
        }
      } else {
        const text = el.dataset.text;
        searchInput.value = text;
        searchClear.classList.toggle('hidden', !text);
        syncSearchHint();
        closeSuggestions();
        addToSearchHistory(text);
        performSearch(text);
      }
    });
  });

  $$('.search-suggestion-delete', searchSuggestions).forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const query = btn.dataset.query;
      state.searchHistory = state.searchHistory.filter(h => h.toLowerCase() !== query.toLowerCase());
      callbacks.saveState();
      updateSuggestions(searchInput.value.trim());
    });
  });
}

async function updateSuggestions(query) {
  if (!query) {
    const historyItems = state.searchHistory.slice(0, 5).map(h => ({ text: h, type: 'history' }));
    renderSuggestionDropdown(historyItems);
    return;
  }
  const lowerQ = query.toLowerCase();
  const historyMatches = state.searchHistory
    .filter(h => h.toLowerCase().includes(lowerQ))
    .slice(0, 3)
    .map(h => ({ text: h, type: 'history' }));

  renderSuggestionDropdown(historyMatches);

  const snapshotQuery = searchInput.value.trim();
  const apiResponse = await window.snowify.searchSuggestions(query);
  if (searchInput.value.trim() !== snapshotQuery) return;

  const textSuggestions = Array.isArray(apiResponse) ? apiResponse : (apiResponse.textSuggestions ?? []);
  const directResults   = Array.isArray(apiResponse) ? [] : (apiResponse.directResults ?? []);

  const shownSet = new Set(historyMatches.map(h => h.text.toLowerCase()));
  const textItems = textSuggestions
    .filter(s => !shownSet.has(s.toLowerCase()))
    .slice(0, 3)
    .map(s => ({ text: s, type: 'text' }));

  const artistItems = directResults.filter(r => r.type === 'artist').slice(0, 1);
  const albumItems  = directResults.filter(r => r.type === 'album').slice(0, 1);
  const songItems   = directResults.filter(r => r.type === 'song').slice(0, 3);

  renderSuggestionDropdown([...historyMatches, ...textItems, ...artistItems, ...albumItems, ...songItems]);
}

// ─── Search execution ─────────────────────────────────────────────────────────

export function renderSearchEmpty() {
  searchResults.innerHTML = `
    <div class="empty-state search-empty">
      <svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="#535353" stroke-width="1.5"><circle cx="11" cy="11" r="7"/><path d="M16 16l4.5 4.5" stroke-linecap="round"/></svg>
      <p>${I18n.t('search.empty')}</p>
    </div>`;
}

export async function performSearch(query) {
  const gen = ++_searchGeneration;
  searchResults.innerHTML = `<div class="loading"><div class="spinner"></div></div>`;
  try {
    const [results, artists, albums, playlists, videos] = await Promise.all([
      window.snowify.search(query, state.musicOnly),
      window.snowify.searchArtists(query),
      window.snowify.searchAlbums(query),
      window.snowify.searchPlaylists(query),
      window.snowify.searchVideos(query),
    ]);
    if (gen !== _searchGeneration) return;
    if (!results.length && !artists.length && !albums.length && !playlists.length && !videos.length) {
      searchResults.innerHTML = `<div class="empty-state"><p>${I18n.t('search.noResultsFor', { query: escapeHtml(query) })}</p></div>`;
      return;
    }
    searchResults.innerHTML = '';
    if (artists.length)   renderSearchArtists(artists.slice(0, 3));
    if (results.length)   renderSearchSongs(results.slice(0, 10));
    if (albums.length)    renderSearchAlbums(albums);
    if (playlists.length) renderSearchPlaylists(playlists);
    if (videos.length)    renderSearchVideos(videos);
  } catch {
    if (gen !== _searchGeneration) return;
    searchResults.innerHTML = `<div class="empty-state"><p>${I18n.t('search.searchFailed')}</p></div>`;
  }
}

// ─── Result section renderers ─────────────────────────────────────────────────

function renderSearchArtists(artists) {
  const section = document.createElement('div');
  section.innerHTML = `<h3 class="search-section-header">${I18n.t('search.artists')}</h3>`;
  const scroll = document.createElement('div');
  scroll.className = 'similar-artists-scroll';
  scroll.innerHTML = artists.map((a, i) => `
    <div class="search-artist-card${i === 0 ? ' search-artist-top' : ''}" data-artist-id="${escapeHtml(a.artistId)}">
      <img class="search-artist-avatar" data-src="${escapeHtml(a.thumbnail || '')}" alt="" />
      <div class="search-artist-name" title="${escapeHtml(a.name)}">${escapeHtml(a.name)}</div>
      <div class="search-artist-label">${I18n.t('artist.type')}</div>
    </div>
  `).join('');
  section.appendChild(scroll);
  searchResults.appendChild(section);
  scroll.querySelectorAll('.search-artist-card').forEach(card => {
    card.addEventListener('click', () => { if (card.dataset.artistId) openArtistPage(card.dataset.artistId); });
    card.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      showArtistContextMenu(e, card.dataset.artistId, card.querySelector('.search-artist-name')?.textContent || '');
    });
  });
}

function renderSearchAlbums(albums) {
  const section = document.createElement('div');
  section.innerHTML = `<h3 class="search-section-header">${I18n.t('search.albums')}</h3>`;
  const scroll = document.createElement('div');
  scroll.className = 'album-scroll';
  scroll.innerHTML = albums.map(a => `
    <div class="album-card" data-album-id="${escapeHtml(a.albumId)}">
      <img class="album-card-cover" data-src="${escapeHtml(a.thumbnail)}" alt="" />
      <button class="album-card-play" title="${I18n.t('player.play')}">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg>
      </button>
      <div class="album-card-name" title="${escapeHtml(a.name)}">${escapeHtml(a.name)}</div>
      <div class="album-card-meta">${escapeHtml(a.artist || '')}${a.year ? ' \u00B7 ' + a.year : ''}</div>
    </div>
  `).join('');
  section.appendChild(scroll);
  searchResults.appendChild(section);
  addScrollArrows(scroll);
  scroll.querySelectorAll('.album-card').forEach(card => {
    const albumId = card.dataset.albumId;
    const meta = albums.find(al => al.albumId === albumId);
    card.querySelector('.album-card-play').addEventListener('click', async (e) => {
      e.stopPropagation();
      const album = await window.snowify.albumTracks(albumId);
      if (album && album.tracks.length) playFromList(album.tracks, 0);
      else showToast(I18n.t('toast.failedLoadAlbum'));
    });
    card.addEventListener('click', () => showAlbumDetail(albumId, meta));
    card.addEventListener('contextmenu', (e) => { e.preventDefault(); showAlbumContextMenu(e, albumId, meta); });
  });
}

function renderSearchPlaylists(playlists) {
  const section = document.createElement('div');
  section.innerHTML = `<h3 class="search-section-header">${I18n.t('search.playlists')}</h3>`;
  const scroll = document.createElement('div');
  scroll.className = 'album-scroll';
  scroll.innerHTML = playlists.map(p => `
    <div class="album-card" data-playlist-id="${escapeHtml(p.playlistId)}">
      <img class="album-card-cover" data-src="${escapeHtml(p.thumbnail)}" alt="" />
      <button class="album-card-play" title="${I18n.t('player.play')}">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg>
      </button>
      <div class="album-card-name" title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</div>
      <div class="album-card-meta">${escapeHtml(p.author || '')}</div>
    </div>
  `).join('');
  section.appendChild(scroll);
  searchResults.appendChild(section);
  addScrollArrows(scroll);
  scroll.querySelectorAll('.album-card').forEach(card => {
    const pid = card.dataset.playlistId;
    const meta = playlists.find(pl => pl.playlistId === pid);
    card.querySelector('.album-card-play').addEventListener('click', async (e) => {
      e.stopPropagation();
      const tracks = await window.snowify.getPlaylistVideos(pid);
      if (tracks && tracks.length) playFromList(tracks, 0);
      else showToast(I18n.t('toast.couldNotLoadPlaylist'));
    });
    card.addEventListener('click', () => showExternalPlaylistDetail(pid, meta));
    card.addEventListener('contextmenu', (e) => { e.preventDefault(); showPlaylistContextMenu(e, pid, meta); });
  });
}

function renderSearchVideos(videos) {
  const section = document.createElement('div');
  section.innerHTML = `<h3 class="search-section-header">${I18n.t('search.videos')}</h3>`;
  const scroll = document.createElement('div');
  scroll.className = 'album-scroll';
  scroll.innerHTML = videos.map(v => `
    <div class="video-card" data-video-id="${escapeHtml(v.id)}">
      <img class="video-card-thumb" data-src="${escapeHtml(v.thumbnail)}" alt="" />
      <button class="video-card-play" title="${I18n.t('video.watch')}">
        <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg>
      </button>
      <div class="video-card-name" title="${escapeHtml(v.title)}">${escapeHtml(v.title)}</div>
      ${v.duration ? `<div class="video-card-duration">${escapeHtml(v.duration)}</div>` : ''}
    </div>
  `).join('');
  section.appendChild(scroll);
  searchResults.appendChild(section);
  addScrollArrows(scroll);
  scroll.querySelectorAll('.video-card').forEach(card => {
    const vid = card.dataset.videoId;
    const video = videos.find(v => v.id === vid);
    card.addEventListener('click', () => { if (video) openVideoPlayer(video.id, video.title, video.artist); });
    card.addEventListener('contextmenu', (e) => { e.preventDefault(); if (video) showVideoContextMenu(e, video); });
  });
}

function renderSearchSongs(results) {
  const header = document.createElement('div');
  header.innerHTML = `<h3 class="search-section-header">${I18n.t('search.songs')}</h3>`;
  searchResults.appendChild(header);
  const tracksWrapper = document.createElement('div');
  searchResults.appendChild(tracksWrapper);
  renderTrackList(tracksWrapper, results, 'search');
}

// ─── Event listeners ──────────────────────────────────────────────────────────

searchInput.addEventListener('input', () => {
  const q = searchInput.value.trim();
  searchClear.classList.toggle('hidden', !q);
  syncSearchHint();
  clearTimeout(searchTimeout);
  clearTimeout(suggestionsTimeout);
  if (!q) { renderSearchEmpty(); updateSuggestions(''); return; }
  searchTimeout      = setTimeout(() => performSearch(q), 400);
  suggestionsTimeout = setTimeout(() => updateSuggestions(q), 250);
});

searchInput.addEventListener('keydown', (e) => {
  const items = $$('.search-suggestion-item', searchSuggestions);
  if (e.key === 'ArrowDown') {
    e.preventDefault();
    if (!items.length) return;
    activeSuggestionIndex = Math.min(activeSuggestionIndex + 1, items.length - 1);
    items.forEach((el, i) => el.classList.toggle('active', i === activeSuggestionIndex));
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    if (!items.length) return;
    activeSuggestionIndex = Math.max(activeSuggestionIndex - 1, 0);
    items.forEach((el, i) => el.classList.toggle('active', i === activeSuggestionIndex));
  } else if (e.key === 'Escape') {
    closeSuggestions();
  } else if (e.key === 'Enter') {
    clearTimeout(searchTimeout);
    clearTimeout(suggestionsTimeout);
    if (activeSuggestionIndex >= 0 && items[activeSuggestionIndex]) {
      const el = items[activeSuggestionIndex];
      const type = el.dataset.type;
      if (type === 'artist' || type === 'album' || type === 'song') {
        el.click();
      } else {
        const text = el.dataset.text;
        searchInput.value = text;
        searchClear.classList.toggle('hidden', !text);
        syncSearchHint();
        closeSuggestions();
        addToSearchHistory(text);
        performSearch(text);
      }
    } else {
      const q = searchInput.value.trim();
      if (q) { closeSuggestions(); addToSearchHistory(q); performSearch(q); }
    }
  }
});

searchInput.addEventListener('focus', () => {
  updateSuggestions(searchInput.value.trim());
});

document.addEventListener('click', (e) => {
  if (!e.target.closest('.search-input-wrap')) closeSuggestions();
});

searchClear.addEventListener('click', () => {
  searchInput.value = '';
  searchClear.classList.add('hidden');
  syncSearchHint();
  renderSearchEmpty();
  closeSuggestions();
  searchInput.focus();
});
