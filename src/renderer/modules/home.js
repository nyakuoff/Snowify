/**
 * home.js
 * Home view: recent tracks, quick picks, new releases, recommendations.
 */

import state from './state.js';
import { escapeHtml, addScrollArrows, renderArtistLinks } from './utils.js';
import { callbacks } from './callbacks.js';
// Circular imports — safe at runtime: all usage is inside function bodies.
import { playFromList } from './player.js';
import { showContextMenu } from './context-menus.js';
import { startTrackDrag } from './library.js';
import { bindArtistLinks } from './artist.js';
import { showAlbumDetail } from './album.js';
import { openArtistPage } from './artist.js';
import { showAlbumContextMenu } from './context-menus.js';

// ─── New-releases cache (invalidated when followed artists change) ─────────────
let _lastReleaseFetch = 0;
let _cachedReleases   = null;

export function invalidateReleasesCache() {
  _cachedReleases   = null;
  _lastReleaseFetch = 0;
}

// ─── renderHome ───────────────────────────────────────────────────────────────

export async function renderHome() {
  // Backfill missing artistIds in recent tracks
  const needsId = state.recentTracks.filter(t => t.artist && !t.artistId);
  if (needsId.length) {
    const uniqueNames = [...new Set(needsId.map(t => t.artist))];
    const lookups = await Promise.all(uniqueNames.map(n => window.snowify.searchArtists(n).catch(() => [])));
    const nameToId = {};
    uniqueNames.forEach((name, i) => {
      if (lookups[i]?.length) nameToId[name] = lookups[i][0].artistId;
    });
    let changed = false;
    state.recentTracks.forEach(t => {
      if (!t.artistId && nameToId[t.artist]) { t.artistId = nameToId[t.artist]; changed = true; }
    });
    if (changed) callbacks.saveState();
  }
  renderRecentTracks();
  renderQuickPicks();
  renderNewReleases();
  renderRecommendations();
}

// ─── renderNewReleases ────────────────────────────────────────────────────────

async function renderNewReleases() {
  const section    = document.querySelector('#new-releases-section');
  const container  = document.querySelector('#new-releases');

  if (!state.followedArtists.length) { section.style.display = 'none'; return; }

  const now = Date.now(), currentYear = new Date().getFullYear();

  if (_cachedReleases && now - _lastReleaseFetch < 30 * 60 * 1000) {
    if (_cachedReleases.length) { section.style.display = ''; renderReleaseCards(container, _cachedReleases); }
    else section.style.display = 'none';
    return;
  }

  section.style.display = '';
  container.innerHTML = `<div class="loading" style="padding:20px"><div class="spinner"></div></div>`;

  try {
    const results   = await Promise.allSettled(state.followedArtists.map(a => window.snowify.artistInfo(a.artistId)));
    const seen      = new Set(), releases = [];

    results.forEach((r, i) => {
      if (r.status !== 'fulfilled' || !r.value) return;
      const info = r.value, followedArtistId = state.followedArtists[i].artistId;
      [...(info.topAlbums || []), ...(info.topSingles || [])].forEach(rel => {
        if (rel.year >= currentYear && !seen.has(rel.albumId)) {
          seen.add(rel.albumId);
          releases.push({ ...rel, artistName: info.name, artistId: followedArtistId });
        }
      });
    });

    releases.sort((a, b) => (b.year || 0) - (a.year || 0));
    _cachedReleases   = releases;
    _lastReleaseFetch = now;

    if (releases.length) renderReleaseCards(container, releases);
    else section.style.display = 'none';
  } catch (err) {
    console.error('New releases error:', err);
    section.style.display = 'none';
  }
}

function renderReleaseCards(container, releases) {
  addScrollArrows(container);
  container.innerHTML = releases.map(a => `
    <div class="album-card" data-album-id="${a.albumId}">
      <img class="album-card-cover" data-src="${escapeHtml(a.thumbnail)}" alt="" />
      <button class="album-card-play" title="${I18n.t('player.play')}">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg>
      </button>
      <div class="album-card-name" title="${escapeHtml(a.name)}">${escapeHtml(a.name)}</div>
      <div class="album-card-meta">${a.artistId ? `<span class="album-card-artist clickable" data-artist-id="${escapeHtml(a.artistId)}">${escapeHtml(a.artistName || '')}</span>` : escapeHtml(a.artistName || '')}${a.year ? ' \u00B7 ' + a.year : ''}${a.type ? ' \u00B7 ' + a.type : ''}</div>
    </div>
  `).join('');

  container.querySelectorAll('.album-card').forEach(card => {
    const albumId = card.dataset.albumId;
    const meta    = releases.find(a => a.albumId === albumId);
    card.querySelector('.album-card-play').addEventListener('click', async (e) => {
      e.stopPropagation();
      const album = await window.snowify.albumTracks(albumId);
      if (album && album.tracks.length) playFromList(album.tracks, 0);
    });
    card.querySelector('.album-card-artist.clickable')?.addEventListener('click', (e) => {
      e.stopPropagation();
      openArtistPage(e.currentTarget.dataset.artistId);
    });
    card.addEventListener('click', () => showAlbumDetail(albumId, meta));
    card.addEventListener('contextmenu', (e) => { e.preventDefault(); showAlbumContextMenu(e, albumId, meta); });
  });
}

// ─── renderRecentTracks ───────────────────────────────────────────────────────

export function renderRecentTracks() {
  const container = document.querySelector('#recent-tracks');
  if (!state.recentTracks.length) {
    container.innerHTML = `
      <div class="empty-state">
        <p>${I18n.t('home.recentEmpty')}</p>
        <p>${I18n.t('home.recentEmptyHint')}</p>
      </div>`;
    return;
  }

  container.innerHTML = state.recentTracks.slice(0, 8).map(track => `
    <div class="track-card" data-track-id="${track.id}" draggable="true">
      <img class="card-thumb" data-src="${escapeHtml(track.thumbnail)}" alt="" />
      <button class="card-play" title="${I18n.t('player.play')}">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg>
      </button>
      <div class="card-title">${escapeHtml(track.title)}</div>
      <div class="card-artist">${renderArtistLinks(track)}</div>
    </div>
  `).join('');

  container.querySelectorAll('.track-card').forEach(card => {
    card.addEventListener('click', () => {
      const track = state.recentTracks.find(t => t.id === card.dataset.trackId);
      if (track) playFromList([track], 0);
    });
    bindArtistLinks(card);
    card.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      const track = state.recentTracks.find(t => t.id === card.dataset.trackId);
      if (track) showContextMenu(e, track);
    });
    card.addEventListener('dragstart', (e) => {
      const track = state.recentTracks.find(t => t.id === card.dataset.trackId);
      if (track) startTrackDrag(e, track);
    });
    card.querySelector('.card-play')?.addEventListener('click', (e) => {
      e.preventDefault(); e.stopPropagation();
      const track = state.recentTracks.find(t => t.id === card.dataset.trackId);
      if (track) playFromList([track], 0);
    });
  });
}

// ─── renderQuickPicks ─────────────────────────────────────────────────────────

export function renderQuickPicks() {
  const container = document.querySelector('#quick-picks');
  const picks = state.recentTracks.slice(0, 6);
  if (!picks.length) { container.innerHTML = ''; return; }

  container.innerHTML = picks.map(track => `
    <div class="quick-pick-card" data-track-id="${track.id}" draggable="true">
      <img data-src="${escapeHtml(track.thumbnail)}" alt="" />
      <span>${escapeHtml(track.title)}</span>
    </div>
  `).join('');

  container.querySelectorAll('.quick-pick-card').forEach(card => {
    card.addEventListener('click', () => {
      const track = state.recentTracks.find(t => t.id === card.dataset.trackId);
      if (track) playFromList([track], 0);
    });
    card.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      const track = state.recentTracks.find(t => t.id === card.dataset.trackId);
      if (track) showContextMenu(e, track);
    });
    card.addEventListener('dragstart', (e) => {
      const track = state.recentTracks.find(t => t.id === card.dataset.trackId);
      if (track) startTrackDrag(e, track);
    });
  });
}

// ─── renderRecommendations ────────────────────────────────────────────────────

async function renderRecommendations() {
  const songsSection    = document.querySelector('#recommended-songs-section');
  const songsContainer  = document.querySelector('#recommended-songs');
  const allTracks       = [...state.recentTracks, ...state.likedSongs];
  if (!allTracks.length) { songsSection.style.display = 'none'; return; }

  const artistCounts = {};
  allTracks.forEach(t => {
    const trackArtists = t.artists?.length ? t.artists : (t.artistId ? [{ name: t.artist, id: t.artistId }] : []);
    trackArtists.forEach(a => {
      if (a.id) {
        if (!artistCounts[a.id]) artistCounts[a.id] = { name: a.name, artistId: a.id, count: 0 };
        artistCounts[a.id].count++;
      }
    });
  });

  const topArtists = Object.values(artistCounts).sort((a, b) => b.count - a.count).slice(0, 3);
  if (!topArtists.length) return;

  const knownTrackIds     = new Set(allTracks.map(t => t.id));
  const recommendedSongs  = [];
  const results = await Promise.allSettled(topArtists.map(a => window.snowify.artistInfo(a.artistId)));
  results.forEach(r => {
    if (r.status !== 'fulfilled' || !r.value) return;
    const info = r.value;
    (info.topSongs || []).forEach(song => {
      if (!knownTrackIds.has(song.id) && recommendedSongs.length < 8) {
        recommendedSongs.push(song);
        knownTrackIds.add(song.id);
      }
    });
  });

  if (recommendedSongs.length) {
    songsSection.style.display = '';
    songsContainer.innerHTML = recommendedSongs.map(track => `
      <div class="track-card" data-track-id="${track.id}" draggable="true">
        <img class="card-thumb" data-src="${escapeHtml(track.thumbnail)}" alt="" />
        <button class="card-play" title="${I18n.t('player.play')}">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg>
        </button>
        <div class="card-title">${escapeHtml(track.title)}</div>
        <div class="card-artist">${renderArtistLinks(track)}</div>
      </div>
    `).join('');

    songsContainer.querySelectorAll('.track-card').forEach(card => {
      card.addEventListener('click', () => {
        const track = recommendedSongs.find(t => t.id === card.dataset.trackId);
        if (track) playFromList([track], 0);
      });
      bindArtistLinks(card);
      card.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        const track = recommendedSongs.find(t => t.id === card.dataset.trackId);
        if (track) showContextMenu(e, track);
      });
      card.addEventListener('dragstart', (e) => {
        const track = recommendedSongs.find(t => t.id === card.dataset.trackId);
        if (track) startTrackDrag(e, track);
      });
      card.querySelector('.card-play')?.addEventListener('click', (e) => {
        e.preventDefault(); e.stopPropagation();
        const track = recommendedSongs.find(t => t.id === card.dataset.trackId);
        if (track) playFromList([track], 0);
      });
    });
  } else {
    songsSection.style.display = 'none';
  }
}
