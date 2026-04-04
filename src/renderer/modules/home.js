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

  addScrollArrows(container);
  container.innerHTML = state.recentTracks.map(track => `
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

// ─── Quick Picks: module-level state ──────────────────────────────────────────
// Keyed by mix id — updated in-place so click handlers always see latest tracks.
const _picksMap = new Map();
// Artist radio cache: artistId → { avatar, tracks, ts }
const _qpArtistCache = new Map();
const _QP_ARTIST_TTL = 30 * 60 * 1000; // 30 min

// ─── renderQuickPicks ─────────────────────────────────────────────────────────

export function renderQuickPicks() {
  const container = document.querySelector('#quick-picks');
  const recent    = state.recentTracks;
  const liked     = state.likedSongs || [];

  if (!recent.length && !liked.length) { container.innerHTML = ''; _picksMap.clear(); return; }

  _picksMap.clear();
  const mixes     = [];
  const allTracks = [...recent, ...liked];

  // Mix 1: Recently Played shuffle
  if (recent.length) {
    const m = {
      id: 'mix-recent', title: I18n.t('home.mixRecent'), sub: I18n.t('home.mixShuffle'),
      avatar: null, thumbs: _qpCollage(recent), tracks: _qpShuffle([...recent]),
    };
    mixes.push(m); _picksMap.set(m.id, m);
  }

  // Mix 2: Liked Songs shuffle
  if (liked.length >= 2) {
    const m = {
      id: 'mix-liked', title: I18n.t('home.mixLiked'), sub: I18n.t('home.mixShuffle'),
      avatar: null, thumbs: _qpCollage(liked), tracks: _qpShuffle([...liked]),
    };
    mixes.push(m); _picksMap.set(m.id, m);
  }

  // Artist radios: collect top artists by play frequency
  const artistMap = {};
  allTracks.forEach(t => {
    // Expand all individual artists so "Artist A, Artist B" never becomes a combined radio
    const individuals = t.artists?.length
      ? t.artists
      : (t.artistId ? [{ id: t.artistId, name: t.artist }] : []);
    individuals.forEach(a => {
      if (!a.id || !a.name) return;
      if (!artistMap[a.id]) artistMap[a.id] = { id: a.id, name: a.name, localTracks: [] };
      if (!artistMap[a.id].localTracks.find(x => x.id === t.id)) artistMap[a.id].localTracks.push(t);
    });
  });

  const slotsLeft  = 8 - mixes.length;
  const topArtists = Object.values(artistMap)
    .sort((a, b) => b.localTracks.length - a.localTracks.length)
    .slice(0, slotsLeft);

  topArtists.forEach(a => {
    const cached = _qpArtistCache.get(a.id);
    const m = {
      id: `mix-artist-${a.id}`,
      title: a.name,
      sub: I18n.t('home.mixArtistRadio'),
      avatar: cached?.avatar ?? null,
      thumbs: _qpCollage(a.localTracks),
      tracks: cached ? [...cached.tracks] : _qpShuffle([...a.localTracks]),
      artistId: a.id,
    };
    mixes.push(m); _picksMap.set(m.id, m);
  });

  // Paint all tiles immediately (no network wait)
  const shuffleSvg = `<svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor" style="opacity:.7"><path d="M10.59 9.17L5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 6.46 20 8.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z"/></svg>`;

  container.innerHTML = mixes.map(mix => {
    const thumb = mix.avatar
      ? `<img class="qp-avatar" data-src="${escapeHtml(mix.avatar)}" alt="" />`
      : `<div class="qp-collage">${mix.thumbs.map(s => `<img data-src="${escapeHtml(s)}" alt="" />`).join('')}</div>`;
    return `<div class="quick-pick-card" data-mix-id="${escapeHtml(mix.id)}">${thumb}<div class="qp-info"><span class="qp-title">${escapeHtml(mix.title)}</span><span class="qp-sub">${shuffleSvg}${escapeHtml(mix.sub)}</span></div><button class="qp-play" title="${I18n.t('player.play')}"><svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></button></div>`;
  }).join('');

  // Click handlers read from _picksMap at click-time so they see hydrated queues
  container.querySelectorAll('.quick-pick-card').forEach(card => {
    const play = () => { const mix = _picksMap.get(card.dataset.mixId); if (mix) playFromList(mix.tracks, 0); };
    card.addEventListener('click', play);
    card.querySelector('.qp-play')?.addEventListener('click', e => { e.stopPropagation(); play(); });
  });

  // Background hydration: fetch full artist catalog for stale/uncached artist tiles
  topArtists.forEach(a => {
    const cached = _qpArtistCache.get(a.id);
    if (cached && Date.now() - cached.ts < _QP_ARTIST_TTL) return;
    _hydrateArtistTile(a.id, container).catch(() => {});
  });
}

async function _hydrateArtistTile(artistId, container) {
  const info = await window.snowify.artistInfo(artistId).catch(() => null);
  if (!info) return;

  // Expand catalog: top 3 albums + top 3 singles album tracks
  const albumIds = [
    ...(info.topAlbums  || []).slice(0, 3).map(a => a.albumId),
    ...(info.topSingles || []).slice(0, 3).map(a => a.albumId),
  ].filter(Boolean);

  const albumResults = await Promise.allSettled(
    albumIds.map(id => window.snowify.albumTracks(id).catch(() => null))
  );
  const fromAlbums = albumResults.flatMap(r =>
    r.status === 'fulfilled' && r.value?.tracks ? r.value.tracks : []
  );

  const seen     = new Set();
  const allSongs = [...(info.topSongs || []), ...fromAlbums].filter(t => {
    if (!t?.id || seen.has(t.id)) return false;
    seen.add(t.id); return true;
  });

  const tracks = _qpShuffle(allSongs.slice(0, 50));
  const avatar = info.avatar || info.banner || null;
  _qpArtistCache.set(artistId, { avatar, tracks, ts: Date.now() });

  // Update in-place so future clicks use the full queue
  const mixId = `mix-artist-${artistId}`;
  const mix   = _picksMap.get(mixId);
  if (!mix) return;
  mix.tracks = tracks;
  mix.avatar = avatar;

  // Swap collage → avatar image if we now have one
  if (avatar) {
    const card = container.querySelector(`[data-mix-id="${CSS.escape(mixId)}"]`);
    if (!card) return;
    const collage = card.querySelector('.qp-collage');
    if (collage) {
      const img = document.createElement('img');
      img.className = 'qp-avatar'; img.alt = ''; img.src = avatar;
      collage.replaceWith(img);
    }
  }
}

function _qpCollage(tracks) {
  const seen = new Set(), thumbs = [];
  for (const t of tracks) {
    if (t.thumbnail && !seen.has(t.thumbnail)) { seen.add(t.thumbnail); thumbs.push(t.thumbnail); }
    if (thumbs.length === 4) break;
  }
  if (thumbs.length) while (thumbs.length < 4) thumbs.push(...thumbs.slice(0, 4 - thumbs.length));
  return thumbs;
}

function _qpShuffle(arr) {
  for (let i = arr.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [arr[i], arr[j]] = [arr[j], arr[i]];
  }
  return arr;
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
