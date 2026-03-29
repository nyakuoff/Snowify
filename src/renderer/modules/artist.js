/**
 * artist.js
 * Artist page, artist link binding, artist context menu, and track-drag helpers.
 */

import state from './state.js';
import { escapeHtml, addScrollArrows, formatFollowers, showToast, renderArtistLinks } from './utils.js';
import { callbacks } from './callbacks.js';
// Circular imports — safe at runtime: all usage is inside function bodies.
import {
  playFromList, togglePlay, updatePlayAllBtn, isCollectionPlaying,
} from './player.js';
import { renderTrackList, showContextMenu, showAlbumContextMenu, showPlaylistContextMenu } from './context-menus.js';
import { openVideoPlayer, showVideoContextMenu } from './video-player.js';
import { maxNPState, closeMaxNP } from './now-playing.js';
import { closeQueuePanel } from './queue.js';
import { showAlbumDetail } from './album.js';
import { showExternalPlaylistDetail } from './album.js';
import { invalidateReleasesCache } from './home.js';
import { resolvePluginArtistMeta } from './plugins.js';

const resolveImageUrl = url => window.snowify?.resolveImageUrl?.(url) || url;

// ─── bindArtistLinks ──────────────────────────────────────────────────────────
// Used by almost every module that renders artist names as clickable links.

export function bindArtistLinks(container) {
  container.querySelectorAll('.artist-link[data-artist-id]').forEach(link => {
    link.addEventListener('click', (e) => {
      e.stopPropagation();
      openArtistPage(link.dataset.artistId);
    });
  });
}

// ─── Liked-songs click in sidebar ────────────────────────────────────────────
// (imported in library.js for getLikedSongsPlaylist, registered once here)
document.querySelector('[data-playlist="liked"]')?.addEventListener('click', () => {
  import('./library.js').then(({ showPlaylistDetail, createPlaylist: _c }) => {
    import('./queue.js').then(({ getLikedSongsPlaylist }) => {
      showPlaylistDetail(getLikedSongsPlaylist(), true);
    });
  });
});

// ─── openArtistPage ───────────────────────────────────────────────────────────

export async function openArtistPage(artistId) {
  if (!artistId) return;

  if (maxNPState.open) closeMaxNP();
  closeQueuePanel();

  callbacks.switchView('artist');

  const avatar              = document.querySelector('#artist-avatar');
  const artistView          = document.querySelector('#view-artist');
  const bannerEl            = document.querySelector('#artist-banner');
  const bannerImg           = document.querySelector('#artist-banner-img');
  const nameEl              = document.querySelector('#artist-name');
  const followersEl         = document.querySelector('#artist-followers');
  const descEl              = document.querySelector('#artist-description');
  const tagsEl              = document.querySelector('#artist-tags');
  const aboutSection        = document.querySelector('#artist-about-section');
  const popularContainer    = document.querySelector('#artist-popular-tracks');
  const discographyContainer = document.querySelector('#artist-discography');
  const videosSection       = document.querySelector('#artist-videos-section');
  const videosContainer     = document.querySelector('#artist-videos');
  const liveSection         = document.querySelector('#artist-live-section');
  const liveContainer       = document.querySelector('#artist-live');
  const fansSection         = document.querySelector('#artist-fans-section');
  const fansContainer       = document.querySelector('#artist-fans');
  const featuredSection     = document.querySelector('#artist-featured-section');
  const featuredContainer   = document.querySelector('#artist-featured');

  function setArtistAvatar(url) {
    const src = resolveImageUrl(url || '');
    if (!src) { avatar.classList.remove('loaded', 'shimmer'); avatar.src = ''; return; }
    avatar.classList.add('shimmer');
    avatar.onload  = () => { avatar.classList.remove('shimmer'); avatar.classList.add('loaded'); };
    avatar.onerror = () => { avatar.classList.remove('shimmer', 'loaded'); };
    avatar.src = src;
  }

  // Reset
  avatar.src = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';
  avatar.classList.remove('loaded');
  avatar.classList.add('shimmer');
  bannerEl.style.display = 'none'; bannerImg.src = '';
  nameEl.textContent = I18n.t('common.loading'); followersEl.textContent = '';
  descEl.textContent = ''; tagsEl.innerHTML = '';
  aboutSection.style.display = 'none'; videosSection.style.display = 'none';
  liveSection.style.display = 'none'; fansSection.style.display = 'none';
  featuredSection.style.display = 'none';
  popularContainer.innerHTML = `<div class="loading"><div class="spinner"></div></div>`;
  discographyContainer.innerHTML = ''; videosContainer.innerHTML = '';
  liveContainer.innerHTML = ''; fansContainer.innerHTML = ''; featuredContainer.innerHTML = '';

  const info = await window.snowify.artistInfo(artistId);
  if (!info) {
    nameEl.textContent = I18n.t('artist.notFound');
    popularContainer.innerHTML = `<div class="empty-state"><p>${I18n.t('artist.couldNotLoad')}</p></div>`;
    return;
  }

  // Background playlist search
  const searchPlaylistsPromise = window.snowify.searchPlaylists(info.name).catch(() => []);

  nameEl.textContent = info.name;
  followersEl.textContent = info.monthlyListeners || '';

  const heroImage = resolveImageUrl(info.banner || info.avatar || '');
  if (heroImage) {
    bannerImg.src = heroImage; bannerEl.style.display = ''; artistView?.classList.add('has-hero');
  } else {
    bannerEl.style.display = 'none'; artistView?.classList.remove('has-hero');
  }

  setArtistAvatar(info.avatar || info.banner || '');
  aboutSection.style.display = 'none';

  // Plugin metadata overlay (fire-and-forget)
  resolvePluginArtistMeta(info.name).then(overlay => {
    if (!overlay) return;
    if (overlay.banner || overlay.avatar) {
      bannerImg.src = resolveImageUrl(overlay.banner || overlay.avatar);
      bannerEl.style.display = ''; artistView?.classList.add('has-hero');
    }
    if (overlay.avatar || overlay.banner) setArtistAvatar(overlay.avatar || overlay.banner);
    if (overlay.genres?.length) {
      tagsEl.innerHTML = overlay.genres.map(g => `<span class="artist-tag">${escapeHtml(g)}</span>`).join('');
      aboutSection.style.display = '';
    }
    if (overlay.bio && !descEl.textContent) { descEl.textContent = overlay.bio; aboutSection.style.display = ''; }
    if (overlay.followers && !followersEl.textContent) followersEl.textContent = formatFollowers(overlay.followers);
  }).catch(() => {});

  // Follow button
  const followBtn = document.querySelector('#btn-artist-follow');
  const isFollowed = () => state.followedArtists.some(a => a.artistId === artistId);
  const updateFollowBtn = () => {
    followBtn.textContent = isFollowed() ? I18n.t('artist.following') : I18n.t('artist.follow');
    followBtn.classList.toggle('following', isFollowed());
  };
  updateFollowBtn();
  followBtn.onclick = () => {
    if (isFollowed()) {
      state.followedArtists = state.followedArtists.filter(a => a.artistId !== artistId);
      showToast(I18n.t('toast.unfollowed', { name: info.name }));
    } else {
      state.followedArtists.push({ artistId, name: info.name, avatar: info.avatar || '' });
      showToast(I18n.t('toast.following', { name: info.name }));
    }
    invalidateReleasesCache();
    callbacks.saveState();
    updateFollowBtn();
  };

  // Share button
  document.querySelector('#btn-artist-share').onclick = () => {
    navigator.clipboard.writeText(`https://snowify.cc/artist/${artistId}`);
    showToast(I18n.t('toast.linkCopied'));
  };

  const popular = (info.topSongs || []).slice(0, 5);
  if (!popular.length) {
    popularContainer.innerHTML = `<div class="empty-state"><p>${I18n.t('artist.noTracks')}</p></div>`;
    discographyContainer.innerHTML = '';
    return;
  }

  renderTrackList(popularContainer, popular, 'artist-popular');

  // Discography
  const allReleases = [
    ...(info.topAlbums  || []),
    ...(info.topSingles || []),
  ].sort((a, b) => (b.year || 0) - (a.year || 0));

  function renderDiscography(filter) {
    const items = filter === 'all' ? allReleases : allReleases.filter(a => a.type === filter);
    if (!items.length) {
      discographyContainer.innerHTML = `<div class="empty-state"><p>${I18n.t('artist.noReleases')}</p></div>`;
      return;
    }
    discographyContainer.innerHTML = items.map(a => `
      <div class="album-card" data-album-id="${a.albumId}">
        <img class="album-card-cover" data-src="${escapeHtml(a.thumbnail)}" alt="" />
        <button class="album-card-play" title="${I18n.t('player.play')}"><svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></button>
        <div class="album-card-name" title="${escapeHtml(a.name)}">${escapeHtml(a.name)}</div>
        <div class="album-card-meta">${[a.year, a.type === 'Album' ? I18n.t('artist.typeAlbum') : a.type === 'Single' ? I18n.t('artist.typeSingle') : a.type].filter(Boolean).join(' \u00B7 ')}</div>
      </div>
    `).join('');

    addScrollArrows(discographyContainer);
    discographyContainer.querySelectorAll('.album-card').forEach(card => {
      const albumId = card.dataset.albumId;
      const meta    = items.find(a => a.albumId === albumId);
      card.querySelector('.album-card-play').addEventListener('click', async (e) => {
        e.stopPropagation();
        const album = await window.snowify.albumTracks(albumId);
        if (album && album.tracks.length) playFromList(album.tracks, 0);
      });
      card.addEventListener('click', () => showAlbumDetail(albumId, meta));
      card.addEventListener('contextmenu', (e) => { e.preventDefault(); showAlbumContextMenu(e, albumId, meta); });
    });
  }

  const filterBtns = document.querySelectorAll('#disco-filters .disco-filter');
  filterBtns.forEach(btn => {
    btn.onclick = () => {
      filterBtns.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      renderDiscography(btn.dataset.filter);
    };
  });
  filterBtns.forEach(b => b.classList.toggle('active', b.dataset.filter === 'all'));
  renderDiscography('all');

  // Music Videos
  const topVideos = info.topVideos || [];
  if (topVideos.length) {
    videosSection.style.display = '';
    videosContainer.innerHTML = topVideos.map(v => `
      <div class="video-card" data-video-id="${escapeHtml(v.videoId)}">
        <img class="video-card-thumb" data-src="${escapeHtml(v.thumbnail)}" alt="" />
        <button class="video-card-play" title="${I18n.t('video.watch')}"><svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></button>
        <div class="video-card-name" title="${escapeHtml(v.name)}">${escapeHtml(v.name)}</div>
        ${v.duration ? `<div class="video-card-duration">${v.duration}</div>` : ''}
      </div>
    `).join('');

    addScrollArrows(videosContainer);
    videosContainer.querySelectorAll('.video-card').forEach(card => {
      const video = topVideos.find(v => v.videoId === card.dataset.videoId);
      card.addEventListener('click', () => { if (video) openVideoPlayer(video.videoId, video.name, video.artist); });
      card.addEventListener('contextmenu', (e) => { e.preventDefault(); if (video) showVideoContextMenu(e, video); });
    });
  }

  // Live Performances
  const livePerfs = info.livePerformances || [];
  if (livePerfs.length) {
    liveSection.style.display = '';
    liveContainer.innerHTML = livePerfs.map(v => `
      <div class="video-card" data-video-id="${escapeHtml(v.videoId)}">
        <img class="video-card-thumb" data-src="${escapeHtml(v.thumbnail)}" alt="" />
        <button class="video-card-play" title="${I18n.t('video.watch')}"><svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></button>
        <div class="video-card-name" title="${escapeHtml(v.name)}">${escapeHtml(v.name)}</div>
      </div>
    `).join('');

    addScrollArrows(liveContainer);
    liveContainer.querySelectorAll('.video-card').forEach(card => {
      const video = livePerfs.find(v => v.videoId === card.dataset.videoId);
      card.addEventListener('click', () => { if (video) openVideoPlayer(video.videoId, video.name, video.artist); });
      card.addEventListener('contextmenu', (e) => { e.preventDefault(); if (video) showVideoContextMenu(e, video); });
    });
  }

  // Fans also like
  const fansAlsoLike = info.fansAlsoLike || [];
  if (fansAlsoLike.length) {
    fansSection.style.display = '';
    fansContainer.innerHTML = fansAlsoLike.map(a => `
      <div class="similar-artist-card" data-artist-id="${escapeHtml(a.artistId)}">
        <img class="similar-artist-avatar" data-src="${escapeHtml(a.thumbnail || '')}" alt="" />
        <div class="similar-artist-name" title="${escapeHtml(a.name)}">${escapeHtml(a.name)}</div>
      </div>
    `).join('');

    addScrollArrows(fansContainer);
    fansContainer.querySelectorAll('.similar-artist-card').forEach(card => {
      card.addEventListener('click', () => { if (card.dataset.artistId) openArtistPage(card.dataset.artistId); });
      card.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        showArtistContextMenu(e, card.dataset.artistId, card.querySelector('.similar-artist-name')?.textContent || '');
      });
    });
  }

  // Featured playlists
  const featuredOn = (info.featuredOn || []).map(p => ({ ...p, subtitle: I18n.t('artist.featuredOn') }));
  const searched   = (await searchPlaylistsPromise) || [];
  const seenPl     = new Set();
  const allPlaylists = [...featuredOn, ...searched].filter(p => {
    if (!p.playlistId || seenPl.has(p.playlistId)) return false;
    seenPl.add(p.playlistId); return true;
  });

  if (allPlaylists.length) {
    featuredSection.style.display = '';
    featuredContainer.innerHTML = allPlaylists.map(p => `
      <div class="album-card" data-playlist-id="${escapeHtml(p.playlistId)}">
        <img class="album-card-cover" data-src="${escapeHtml(p.thumbnail)}" alt="" />
        <button class="album-card-play" title="${I18n.t('player.play')}"><svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></button>
        <div class="album-card-name" title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</div>
        <div class="album-card-meta">${escapeHtml(p.subtitle || I18n.t('common.playlist'))}</div>
      </div>
    `).join('');

    addScrollArrows(featuredContainer);
    featuredContainer.querySelectorAll('.album-card').forEach(card => {
      const pid  = card.dataset.playlistId;
      const meta = allPlaylists.find(p => p.playlistId === pid);
      card.querySelector('.album-card-play').addEventListener('click', async (e) => {
        e.stopPropagation();
        try {
          const tracks = await window.snowify.getPlaylistVideos(pid);
          if (tracks?.length) playFromList(tracks, 0); else showToast(I18n.t('toast.couldNotLoadPlaylist'));
        } catch { showToast(I18n.t('toast.couldNotLoadPlaylist')); }
      });
      card.addEventListener('click', () => showExternalPlaylistDetail(pid, meta));
      card.addEventListener('contextmenu', (e) => { e.preventDefault(); showPlaylistContextMenu(e, pid, meta); });
    });
  }

  // Play all (popular tracks)
  const artistPlayBtn = document.querySelector('#btn-artist-play-all');
  updatePlayAllBtn(artistPlayBtn, popular, null);
  artistPlayBtn.onclick = () => {
    if (!popular.length) return;
    if (isCollectionPlaying(popular, null)) {
      togglePlay();
      updatePlayAllBtn(artistPlayBtn, popular, null);
    } else {
      playFromList(popular, 0);
      updatePlayAllBtn(artistPlayBtn, popular, null);
    }
  };
}

// ─── showArtistContextMenu (re-exported from context-menus for convenience) ───
export { showArtistContextMenu } from './context-menus.js';
