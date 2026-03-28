/**
 * explore.js
 * Explore view: new albums, trending charts, top artists, music videos, moods/genres.
 */

import state from './state.js';
import { escapeHtml, addScrollArrows, renderArtistLinks, showToast } from './utils.js';
// Circular imports — safe at runtime: all usage is inside function bodies.
import { playFromList } from './player.js';
import { showContextMenu, showAlbumContextMenu, showArtistContextMenu } from './context-menus.js';
import { bindArtistLinks, openArtistPage } from './artist.js';
import { showAlbumDetail } from './album.js';
import { openVideoPlayer, showVideoContextMenu } from './video-player.js';

// ─── Cache ────────────────────────────────────────────────────────────────────
let _exploreCache      = null;
let _chartsCache       = null;
let _exploreCacheTime  = 0;
let _chartsCacheTime   = 0;
const EXPLORE_CACHE_TTL = 30 * 60 * 1000;

export function invalidateExploreCache() {
  _exploreCache     = null;
  _chartsCache      = null;
  _exploreCacheTime = 0;
  _chartsCacheTime  = 0;
}

const MOOD_COLORS = [
  '#1db954', '#e13300', '#8c67ab', '#e8115b', '#1e90ff',
  '#f59b23', '#158a43', '#ba55d3', '#e05050', '#509bf5',
  '#ff6437', '#7358ff', '#27856a', '#e91e63', '#1db4e8',
  '#af2896', '#148a08', '#dc5b2e', '#5080ff', '#d84000',
];

const POPULAR_MOODS = new Set([
  'pop', 'hip-hop', 'r&b', 'rock', 'chill', 'workout', 'party',
  'focus', 'romance', 'sad', 'feel good', 'jazz', 'classical',
  'country', 'electronic', 'indie', 'sleep', 'energy booster',
  'commute', 'latin', 'k-pop', 'metal',
]);

async function fetchExploreData() {
  const now = Date.now();
  if (_exploreCache && now - _exploreCacheTime < EXPLORE_CACHE_TTL) return _exploreCache;
  _exploreCache     = await window.snowify.explore();
  _exploreCacheTime = now;
  return _exploreCache;
}

async function fetchChartsData() {
  const now = Date.now();
  if (_chartsCache && now - _chartsCacheTime < EXPLORE_CACHE_TTL) return _chartsCache;
  _chartsCache     = await window.snowify.charts();
  _chartsCacheTime = now;
  return _chartsCache;
}

// ─── renderExplore ────────────────────────────────────────────────────────────

export async function renderExplore() {
  const content = document.querySelector('#explore-content');
  content.innerHTML = `<div class="loading"><div class="spinner"></div></div>`;

  await window.snowify.setCountry(state.country || '');

  const [exploreData, chartsData] = await Promise.all([fetchExploreData(), fetchChartsData()]);

  if (!exploreData && !chartsData) {
    content.innerHTML = `<div class="empty-state"><p>${I18n.t('explore.couldNotLoad')}</p></div>`;
    return;
  }

  let html = '';

  if (!state.country) {
    html += `<div class="explore-country-hint">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2v1.93zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39z"/></svg>
      <span>Set your <a href="#" id="explore-country-link">country in Settings</a> for more relevant recommendations</span>
    </div>`;
  }

  if (exploreData?.newAlbums?.length) {
    html += `<div class="explore-section"><h2>${I18n.t('explore.newAlbums')}</h2><div class="scroll-container"><button class="scroll-arrow scroll-arrow-left" data-dir="left"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg></button><div class="album-scroll">`;
    html += exploreData.newAlbums.map(a => `
      <div class="album-card" data-album-id="${escapeHtml(a.albumId)}">
        <img class="album-card-cover" data-src="${escapeHtml(a.thumbnail)}" alt="" />
        <button class="album-card-play" title="${I18n.t('player.play')}"><svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></button>
        <div class="album-card-name" title="${escapeHtml(a.name)}">${escapeHtml(a.name)}</div>
        <div class="album-card-meta">${renderArtistLinks(a)}</div>
      </div>
    `).join('');
    html += `</div><button class="scroll-arrow scroll-arrow-right" data-dir="right"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg></button></div></div>`;
  }

  if (chartsData?.topSongs?.length) {
    html += `<div class="explore-section"><h2>${I18n.t('explore.trending')}</h2><div class="top-songs-grid">`;
    html += chartsData.topSongs.map((track, i) => `
      <div class="top-song-item" data-track-id="${escapeHtml(track.id)}">
        <div class="top-song-rank">${track.rank || i + 1}</div>
        <div class="top-song-thumb-wrap">
          <img class="top-song-thumb" data-src="${escapeHtml(track.thumbnail)}" alt="" />
          <div class="top-song-play"><svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></div>
        </div>
        <div class="top-song-info">
          <div class="top-song-title">${escapeHtml(track.title)}</div>
          <div class="top-song-artist">${renderArtistLinks(track)}</div>
        </div>
      </div>
    `).join('');
    html += `</div></div>`;
  }

  if (chartsData?.topArtists?.length) {
    html += `<div class="explore-section"><h2>${I18n.t('explore.topArtists')}</h2><div class="scroll-container"><button class="scroll-arrow scroll-arrow-left" data-dir="left"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg></button><div class="album-scroll top-artists-scroll">`;
    html += chartsData.topArtists.map((a, i) => `
      <div class="top-artist-card" data-artist-id="${escapeHtml(a.artistId)}">
        <img class="top-artist-avatar" data-src="${escapeHtml(a.thumbnail)}" alt="" />
        <div class="top-artist-name">${escapeHtml(a.name)}</div>
        <div class="top-artist-rank">#${i + 1}</div>
      </div>
    `).join('');
    html += `</div><button class="scroll-arrow scroll-arrow-right" data-dir="right"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg></button></div></div>`;
  }

  if (exploreData?.newMusicVideos?.length) {
    html += `<div class="explore-section"><h2>${I18n.t('explore.newMusicVideos')}</h2><div class="scroll-container"><button class="scroll-arrow scroll-arrow-left" data-dir="left"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg></button><div class="album-scroll music-video-scroll">`;
    html += exploreData.newMusicVideos.slice(0, 15).map(v => `
      <div class="video-card" data-video-id="${escapeHtml(v.id)}">
        <img class="video-card-thumb" data-src="${escapeHtml(v.thumbnail)}" alt="" />
        <button class="video-card-play" title="${I18n.t('video.watch')}"><svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></button>
        <div class="video-card-name" title="${escapeHtml(v.title)}">${escapeHtml(v.title)}</div>
        <div class="video-card-duration">${renderArtistLinks(v)}</div>
      </div>
    `).join('');
    html += `</div><button class="scroll-arrow scroll-arrow-right" data-dir="right"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg></button></div></div>`;
  }

  if (exploreData?.moods?.length) {
    const filteredMoods = exploreData.moods.filter(m => POPULAR_MOODS.has(m.label.toLowerCase()));
    const displayMoods  = filteredMoods.length ? filteredMoods : exploreData.moods.slice(0, 16);
    html += `<div class="explore-section" id="explore-moods-section"><h2>${I18n.t('explore.moodsAndGenres')}</h2><div class="mood-grid">`;
    html += displayMoods.map((m, i) => {
      const bg = MOOD_COLORS[i % MOOD_COLORS.length];
      return `<div class="mood-card" data-browse-id="${escapeHtml(m.browseId)}" data-params="${escapeHtml(m.params || '')}" style="border-left-color:${bg}">${escapeHtml(m.label)}</div>`;
    }).join('');
    html += `</div></div>`;
  }

  content.innerHTML = html || `<div class="empty-state"><p>${I18n.t('explore.noData')}</p></div>`;

  // Country link
  document.querySelector('#explore-country-link')?.addEventListener('click', (e) => {
    e.preventDefault();
    import('./callbacks.js').then(({ callbacks: cb }) => { cb.switchView('settings'); });
    setTimeout(() => document.querySelector('#setting-country')?.focus(), 100);
  });

  attachExploreAlbumListeners(content, exploreData?.newAlbums || []);

  // Top songs
  const topSongsList = chartsData?.topSongs || [];
  content.querySelectorAll('.top-song-item').forEach(item => {
    const track = topSongsList.find(t => t.id === item.dataset.trackId);
    if (!track) return;
    item.addEventListener('click', () => playFromList([track], 0));
    bindArtistLinks(item);
    item.addEventListener('contextmenu', (e) => { e.preventDefault(); showContextMenu(e, track); });
  });

  // Top artists
  content.querySelectorAll('.top-artist-card').forEach(card => {
    card.addEventListener('click', () => openArtistPage(card.dataset.artistId));
    card.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      showArtistContextMenu(e, card.dataset.artistId, card.querySelector('.top-artist-name')?.textContent || '');
    });
  });

  // Music videos
  content.querySelectorAll('.music-video-scroll .video-card').forEach(card => {
    const v = (exploreData?.newMusicVideos || []).find(t => t.id === card.dataset.videoId);
    if (v) {
      bindArtistLinks(card);
      card.addEventListener('click', () => openVideoPlayer(v.id, v.title, v.artist));
      card.addEventListener('contextmenu', (e) => { e.preventDefault(); showVideoContextMenu(e, v); });
    }
  });

  // Moods
  attachMoodListeners(content, exploreData?.moods || []);

  // Scroll arrows
  content.querySelectorAll('.scroll-container').forEach(ct => {
    const scrollEl = ct.querySelector('.album-scroll');
    if (!scrollEl) return;
    ct.querySelectorAll('.scroll-arrow').forEach(btn => {
      btn.addEventListener('click', () => {
        const dir = btn.dataset.dir === 'left' ? -1 : 1;
        scrollEl.scrollBy({ left: dir * 400, behavior: 'smooth' });
      });
    });
  });
}

// ─── attachMoodListeners ──────────────────────────────────────────────────────

function attachMoodListeners(container, moods) {
  container.querySelectorAll('.mood-card').forEach(card => {
    card.addEventListener('click', async () => {
      const moodsSection = document.querySelector('#explore-moods-section');
      if (!moodsSection) return;
      const savedHtml = moodsSection.innerHTML;
      moodsSection.innerHTML = `<div class="loading"><div class="spinner"></div></div>`;
      const playlists = await window.snowify.browseMood(card.dataset.browseId, card.dataset.params);
      if (!playlists?.length) {
        moodsSection.innerHTML = savedHtml;
        showToast(I18n.t('toast.noPlaylistsForMood'));
        attachMoodListeners(moodsSection.parentElement, moods);
        return;
      }
      let moodHtml = `<h2>${escapeHtml(card.textContent)}</h2>`;
      moodHtml += `<button class="explore-back-btn" id="explore-mood-back">${I18n.t('explore.backToMoods')}</button>`;
      moodHtml += `<div class="album-scroll">`;
      moodHtml += playlists.map(p => `
        <div class="album-card" data-playlist-id="${escapeHtml(p.playlistId)}">
          <img class="album-card-cover" data-src="${escapeHtml(p.thumbnail)}" alt="" />
          <div class="album-card-name" title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</div>
          <div class="album-card-meta">${escapeHtml(p.subtitle || '')}</div>
        </div>
      `).join('');
      moodHtml += `</div>`;
      moodsSection.innerHTML = moodHtml;

      const moodScroll = moodsSection.querySelector('.album-scroll');
      if (moodScroll) addScrollArrows(moodScroll);

      document.querySelector('#explore-mood-back')?.addEventListener('click', () => {
        moodsSection.innerHTML = savedHtml;
        attachMoodListeners(moodsSection.parentElement, moods);
      });

      moodsSection.querySelectorAll('.album-card').forEach(ac => {
        ac.addEventListener('click', async () => {
          try {
            const vids = await window.snowify.getPlaylistVideos?.(ac.dataset.playlistId);
            if (vids?.length) playFromList(vids, 0);
            else showToast(I18n.t('toast.couldNotLoadPlaylist'));
          } catch { showToast(I18n.t('toast.couldNotLoadPlaylist')); }
        });
      });
    });
  });
}

// ─── attachExploreAlbumListeners ──────────────────────────────────────────────

export function attachExploreAlbumListeners(container, albums) {
  container.querySelectorAll('.album-card').forEach(card => {
    const albumId = card.dataset.albumId;
    if (!albumId) return;
    const meta = albums.find(a => a.albumId === albumId);
    card.querySelector('.album-card-play')?.addEventListener('click', async (e) => {
      e.stopPropagation();
      const album = await window.snowify.albumTracks(albumId);
      if (album?.tracks?.length) playFromList(album.tracks, 0);
    });
    bindArtistLinks(card);
    card.addEventListener('click', () => showAlbumDetail(albumId, meta));
    card.addEventListener('contextmenu', (e) => { e.preventDefault(); showAlbumContextMenu(e, albumId, meta); });
  });
}
