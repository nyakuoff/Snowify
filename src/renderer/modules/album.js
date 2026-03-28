/**
 * album.js
 * Album detail view and external playlist detail view.
 */

import state from './state.js';
import { escapeHtml, renderArtistLinks, resolveImageUrl } from './utils.js';
import { callbacks } from './callbacks.js';
// Circular imports — safe at runtime: all usage is inside function bodies.
import {
  playFromList, togglePlay, updatePlayAllBtn, isCollectionPlaying,
} from './player.js';
import { renderTrackList, setupSaveButton } from './context-menus.js';
import { bindArtistLinks } from './artist.js';

// ─── showAlbumDetail ──────────────────────────────────────────────────────────

export async function showAlbumDetail(albumId, albumMeta) {
  callbacks.switchView('album');

  const saveBtn         = document.querySelector('#btn-album-save');
  const heroName        = document.querySelector('#album-hero-name');
  const heroMeta        = document.querySelector('#album-hero-meta');
  const heroCover       = document.querySelector('#album-hero-img');
  const heroType        = document.querySelector('#album-hero-type');
  const tracksContainer = document.querySelector('#album-tracks');

  setupSaveButton(saveBtn, albumId, albumMeta?.name || I18n.t('album.type'), []);

  heroName.textContent  = albumMeta?.name || I18n.t('common.loading');
  heroMeta.textContent  = '';
  heroType.textContent  = (albumMeta?.type || I18n.t('album.type')).toUpperCase();
  heroCover.src         = resolveImageUrl(albumMeta?.thumbnail || '') || '';
  tracksContainer.innerHTML = `<div class="loading"><div class="spinner"></div></div>`;

  const album = await window.snowify.albumTracks(albumId);
  if (!album || !album.tracks.length) {
    tracksContainer.innerHTML = `<div class="empty-state"><p>${I18n.t('album.couldNotLoad')}</p></div>`;
    return;
  }

  heroName.textContent = album.name || albumMeta?.name || I18n.t('album.type');
  const parts = [];
  if (album.artist) parts.push(renderArtistLinks(album));
  if (albumMeta?.year) parts.push(escapeHtml(String(albumMeta.year)));
  parts.push(I18n.tp('sidebar.songCount', album.tracks.length));
  heroMeta.innerHTML = parts.join(' \u00B7 ');
  bindArtistLinks(heroMeta);
  if (album.thumbnail) heroCover.src = resolveImageUrl(album.thumbnail) || album.thumbnail;

  renderTrackList(tracksContainer, album.tracks, 'album');

  const albumPlayBtn = document.querySelector('#btn-album-play-all');
  updatePlayAllBtn(albumPlayBtn, album.tracks, null);
  albumPlayBtn.onclick = () => {
    if (!album.tracks.length) return;
    if (isCollectionPlaying(album.tracks, null)) {
      togglePlay();
      updatePlayAllBtn(albumPlayBtn, album.tracks, null);
    } else {
      playFromList(album.tracks, 0);
      updatePlayAllBtn(albumPlayBtn, album.tracks, null);
    }
  };
  document.querySelector('#btn-album-shuffle').onclick = () => {
    if (album.tracks.length) {
      playFromList([...album.tracks].sort(() => Math.random() - 0.5), 0);
      updatePlayAllBtn(albumPlayBtn, album.tracks, null);
    }
  };

  setupSaveButton(saveBtn, albumId, album.name || albumMeta?.name || I18n.t('album.type'), album.tracks);
}

// ─── showExternalPlaylistDetail ───────────────────────────────────────────────

export async function showExternalPlaylistDetail(playlistId, meta) {
  callbacks.switchView('album');

  const saveBtn         = document.querySelector('#btn-album-save');
  const heroName        = document.querySelector('#album-hero-name');
  const heroMeta        = document.querySelector('#album-hero-meta');
  const heroCover       = document.querySelector('#album-hero-img');
  const heroType        = document.querySelector('#album-hero-type');
  const tracksContainer = document.querySelector('#album-tracks');

  setupSaveButton(saveBtn, playlistId, meta?.name || I18n.t('playlist.type'), []);

  heroName.textContent  = meta?.name || I18n.t('common.loading');
  heroMeta.textContent  = '';
  heroType.textContent  = I18n.t('playlist.type');
  heroCover.src         = resolveImageUrl(meta?.thumbnail || '') || '';
  tracksContainer.innerHTML = `<div class="loading"><div class="spinner"></div></div>`;

  const tracks = await window.snowify.getPlaylistVideos(playlistId);
  if (!tracks?.length) {
    tracksContainer.innerHTML = `<div class="empty-state"><p>${I18n.t('toast.failedLoadPlaylist')}</p></div>`;
    return;
  }

  heroMeta.textContent = I18n.tp('sidebar.songCount', tracks.length);
  renderTrackList(tracksContainer, tracks, 'playlist');

  const extPlayBtn = document.querySelector('#btn-album-play-all');
  updatePlayAllBtn(extPlayBtn, tracks, null);
  extPlayBtn.onclick = () => {
    if (isCollectionPlaying(tracks, null)) {
      togglePlay();
      updatePlayAllBtn(extPlayBtn, tracks, null);
    } else {
      playFromList(tracks, 0);
      updatePlayAllBtn(extPlayBtn, tracks, null);
    }
  };
  document.querySelector('#btn-album-shuffle').onclick = () => {
    playFromList([...tracks].sort(() => Math.random() - 0.5), 0);
    updatePlayAllBtn(extPlayBtn, tracks, null);
  };

  setupSaveButton(saveBtn, playlistId, meta?.name || I18n.t('playlist.type'), tracks);
}
