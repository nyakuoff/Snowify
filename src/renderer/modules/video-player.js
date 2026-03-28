/**
 * video-player.js
 * In-app video overlay — opening, controls, mini-player drag, and context menu.
 */

import { audioRef, VOLUME_SCALE } from './audio-ref.js';
import state from './state.js';
import { showToast } from './utils.js';
// Circular imports — safe at runtime (used only inside function bodies):
import { updatePlayButton, playFromList } from './player.js';
import { buildPlaylistSectionHtml, removeContextMenu, positionContextMenu, handleTogglePlaylist } from './context-menus.js';
import { startRadio, handlePlayNext, handleAddToQueue } from './queue.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);

// ─── DOM refs & state ────────────────────────────────────────────────────────
const videoOverlay   = $('#video-overlay');
const videoPlayer    = $('#video-player');
const videoLoading   = $('#video-loading');
const videoTitle     = $('#video-overlay-title');
const videoArtist    = $('#video-overlay-artist');
const miniProgressFill = $('#video-mini-progress-fill');

let _wasPlayingBeforeVideo = false;
let _videoAudio   = null;
let _currentVideoId = null;
let _isVideoMini  = false;

// ─── Button wiring ───────────────────────────────────────────────────────────
$('#btn-close-video').addEventListener('click', closeVideoPlayer);
$('#btn-video-minimize').addEventListener('click', minimizeVideoPlayer);
$('#btn-video-expand').addEventListener('click', expandVideoPlayer);
$('#btn-video-mini-close').addEventListener('click', closeVideoPlayer);
$('#btn-video-listen').addEventListener('click', listenOnlyFromVideo);
$('#btn-video-mini-listen').addEventListener('click', listenOnlyFromVideo);

// ─── Mini-player drag ────────────────────────────────────────────────────────
let _miniDragState = null;
let _miniWasDragged = false;

videoOverlay.addEventListener('mousedown', (e) => {
  if (!_isVideoMini) return;
  if (!e.target.closest('.video-container')) return;
  if (e.target.closest('.video-mini-btn')) return;
  _miniWasDragged = false;
  _miniDragState  = { startX: e.clientX, startY: e.clientY, moved: false };
  videoOverlay.classList.add('dragging');
  e.preventDefault();
});

document.addEventListener('mousemove', (e) => {
  if (!_miniDragState) return;
  const dx = e.clientX - _miniDragState.startX;
  const dy = e.clientY - _miniDragState.startY;
  if (!_miniDragState.moved && Math.abs(dx) + Math.abs(dy) > 5) _miniDragState.moved = true;
  if (_miniDragState.moved) videoOverlay.style.transform = `translate(${dx}px, ${dy}px)`;
});

document.addEventListener('mouseup', (e) => {
  if (!_miniDragState) return;
  _miniWasDragged = _miniDragState.moved;
  if (_miniWasDragged) {
    const draggedRect = videoOverlay.getBoundingClientRect();
    const midX = window.innerWidth / 2, midY = window.innerHeight / 2;
    const isRight  = e.clientX > midX;
    const isBottom = e.clientY > midY;

    videoOverlay.classList.add('dragging');
    videoOverlay.style.transform = '';
    videoOverlay.style.right  = isRight  ? '16px' : 'auto';
    videoOverlay.style.left   = isRight  ? 'auto' : '16px';
    videoOverlay.style.bottom = isBottom ? `calc(var(--now-playing-height) + 16px)` : 'auto';
    videoOverlay.style.top    = isBottom ? 'auto' : '16px';

    void videoOverlay.offsetHeight;
    const targetRect = videoOverlay.getBoundingClientRect();
    const offsetX    = draggedRect.left - targetRect.left;
    const offsetY    = draggedRect.top  - targetRect.top;
    videoOverlay.style.transform = `translate(${offsetX}px, ${offsetY}px)`;
    void videoOverlay.offsetHeight;
    videoOverlay.classList.remove('dragging');
    videoOverlay.style.transform = '';
  } else {
    videoOverlay.style.transform = '';
    videoOverlay.classList.remove('dragging');
  }
  _miniDragState = null;
});

videoOverlay.addEventListener('click', (e) => {
  if (_isVideoMini) {
    if (_miniWasDragged) { _miniWasDragged = false; return; }
    if (e.target.closest('.video-mini-btn')) return;
    if (e.target === videoPlayer || e.target.closest('.video-container')) expandVideoPlayer();
    return;
  }
  if (e.target === videoOverlay) minimizeVideoPlayer();
});

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && !videoOverlay.classList.contains('hidden')) {
    if (_isVideoMini) closeVideoPlayer();
    else minimizeVideoPlayer();
  }
});

// ─── Core functions ──────────────────────────────────────────────────────────

export async function openVideoPlayer(videoId, name, artist) {
  const audio = audioRef.audio;
  if (videoOverlay.classList.contains('hidden')) {
    _wasPlayingBeforeVideo = state.isPlaying;
  }

  _isVideoMini = false;
  videoOverlay.classList.remove('mini');
  videoPlayer.setAttribute('controls', '');
  resetVideoOverlayPosition();

  _currentVideoId = videoId;
  videoTitle.textContent  = name   || I18n.t('video.musicVideo');
  videoArtist.textContent = artist || '';
  videoPlayer.src  = '';
  videoPlayer.poster = '';
  if (_videoAudio) { _videoAudio.pause(); _videoAudio = null; }
  videoLoading.classList.remove('hidden');
  videoOverlay.classList.remove('hidden');

  if (state.isPlaying) {
    audio.pause();
    state.isPlaying = false;
    updatePlayButton();
  }

  videoPlayer.removeEventListener('timeupdate', updateMiniProgress);
  videoPlayer.removeEventListener('seeked',    syncVideoAudio);
  videoPlayer.removeEventListener('timeupdate', syncVideoAudio);
  videoPlayer.removeEventListener('pause',     onVideoPause);
  videoPlayer.removeEventListener('play',      onVideoPlay);

  try {
    const result = await window.snowify.getVideoStreamUrl(videoId, state.videoQuality, state.videoPremuxed);
    videoPlayer.src = result.videoUrl;
    videoLoading.classList.add('hidden');
    videoPlayer.addEventListener('timeupdate', updateMiniProgress);

    if (result.audioUrl) {
      _videoAudio = new Audio(result.audioUrl);
      _videoAudio.volume = state.volume * VOLUME_SCALE;
      videoPlayer.muted = true;
      const onVideoPlaying = () => {
        videoPlayer.removeEventListener('playing', onVideoPlaying);
        if (_videoAudio) { _videoAudio.currentTime = videoPlayer.currentTime; _videoAudio.play(); }
      };
      videoPlayer.addEventListener('playing', onVideoPlaying);
      videoPlayer.play();
      videoPlayer.addEventListener('seeked',    syncVideoAudio);
      videoPlayer.addEventListener('pause',     onVideoPause);
      videoPlayer.addEventListener('play',      onVideoPlay);
      videoPlayer.addEventListener('timeupdate', syncVideoAudio);
    } else {
      videoPlayer.muted = false;
      videoPlayer.play();
    }
  } catch (err) {
    console.error('Video playback error:', err);
    videoLoading.classList.add('hidden');
    showToast(I18n.t('toast.failedLoadVideo'));
    closeVideoPlayer();
  }
}

function syncVideoAudio() {
  if (_videoAudio && Math.abs(videoPlayer.currentTime - _videoAudio.currentTime) > 0.3) {
    _videoAudio.currentTime = videoPlayer.currentTime;
  }
}

function onVideoPause() { _videoAudio?.pause(); }
function onVideoPlay() {
  if (_videoAudio) { _videoAudio.currentTime = videoPlayer.currentTime; _videoAudio.play(); }
}

function updateMiniProgress() {
  if (videoPlayer.duration) {
    miniProgressFill.style.width = (videoPlayer.currentTime / videoPlayer.duration * 100) + '%';
  }
}

function resetVideoOverlayPosition() {
  videoOverlay.style.right     = '';
  videoOverlay.style.left      = '';
  videoOverlay.style.top       = '';
  videoOverlay.style.bottom    = '';
  videoOverlay.style.transform = '';
}

export function closeVideoPlayer() {
  videoOverlay.classList.add('hidden');
  videoOverlay.classList.remove('mini');
  videoPlayer.setAttribute('controls', '');
  _currentVideoId = null;
  _isVideoMini    = false;
  _miniDragState  = null;
  _miniWasDragged = false;
  videoPlayer.pause();
  videoPlayer.removeEventListener('seeked',    syncVideoAudio);
  videoPlayer.removeEventListener('timeupdate', syncVideoAudio);
  videoPlayer.removeEventListener('timeupdate', updateMiniProgress);
  videoPlayer.removeEventListener('pause',     onVideoPause);
  videoPlayer.removeEventListener('play',      onVideoPlay);
  videoPlayer.src = '';
  miniProgressFill.style.width = '0%';
  resetVideoOverlayPosition();
  if (_videoAudio) { _videoAudio.pause(); _videoAudio.src = ''; _videoAudio = null; }

  if (_wasPlayingBeforeVideo && state.queue[state.queueIndex]) {
    audioRef.audio.play().then(() => {
      state.isPlaying = true;
      updatePlayButton();
    }).catch(() => {});
  }
}

export function minimizeVideoPlayer() {
  _isVideoMini = true;
  videoOverlay.classList.add('mini');
  videoPlayer.removeAttribute('controls');
}

export function expandVideoPlayer() {
  _isVideoMini    = false;
  _miniDragState  = null;
  _miniWasDragged = false;
  videoOverlay.classList.remove('mini');
  videoPlayer.setAttribute('controls', '');
  resetVideoOverlayPosition();
}

function listenOnlyFromVideo() {
  if (!_currentVideoId) return;
  const seekTime = videoPlayer.currentTime;
  const track    = makeTrackFromVideo({
    id:     _currentVideoId,
    title:  videoTitle.textContent,
    artist: videoArtist.textContent,
  });
  _wasPlayingBeforeVideo = false;
  closeVideoPlayer();
  playFromList([track], 0);
  if (seekTime > 1) {
    const audio = audioRef.audio;
    const onPlaying = () => { audio.removeEventListener('playing', onPlaying); audio.currentTime = seekTime; };
    audio.addEventListener('playing', onPlaying);
  }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function normalizeVideo(video) {
  return {
    id:        video.videoId || video.id,
    title:     video.name   || video.title,
    artist:    video.artist || '',
    thumbnail: video.thumbnail || `https://i.ytimg.com/vi/${video.videoId || video.id}/hqdefault.jpg`,
  };
}

function makeTrackFromVideo(video) {
  const v = normalizeVideo(video);
  return {
    id:        v.id,
    title:     v.title,
    artist:    v.artist,
    url:       `https://music.youtube.com/watch?v=${v.id}`,
    thumbnail: v.thumbnail,
  };
}

// ─── Context menu ─────────────────────────────────────────────────────────────

export function showVideoContextMenu(e, video) {
  removeContextMenu();
  const v     = normalizeVideo(video);
  const track = makeTrackFromVideo(video);
  const menu  = document.createElement('div');
  menu.className = 'context-menu';
  menu.style.left = e.clientX + 'px';
  menu.style.top  = e.clientY + 'px';

  const playlistSection = buildPlaylistSectionHtml(track);

  menu.innerHTML = `
    <div class="context-menu-item" data-action="watch-video">${I18n.t('context.playVideo')}</div>
    <div class="context-menu-item" data-action="play-audio">${I18n.t('context.playAudio')}</div>
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="play-next">${I18n.t('context.playNext')}</div>
    <div class="context-menu-item" data-action="add-queue">${I18n.t('context.addToQueue')}</div>
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="start-radio">${I18n.t('context.startRadio')}</div>
    ${playlistSection}
    <div class="context-menu-divider"></div>
    <div class="context-menu-item" data-action="share">${I18n.t('context.copyLink')}</div>
  `;

  positionContextMenu(menu);

  menu.addEventListener('click', async (ev) => {
    const item = ev.target.closest('[data-action]');
    if (!item) return;
    const action = item.dataset.action;
    if (action === 'none') return;
    switch (action) {
      case 'watch-video': openVideoPlayer(v.id, v.title, v.artist); break;
      case 'play-audio':  playFromList([track], 0); break;
      case 'play-next':   handlePlayNext(track); break;
      case 'add-queue':   handleAddToQueue(track); break;
      case 'toggle-playlist': handleTogglePlaylist(item.dataset.pid, track); break;
      case 'start-radio': await startRadio(track); break;
      case 'share':
        navigator.clipboard.writeText(`https://snowify.cc/track/${v.id}`);
        showToast(I18n.t('toast.linkCopied'));
        break;
    }
    removeContextMenu();
  });

  setTimeout(() => { document.addEventListener('click', removeContextMenu, { once: true }); }, 10);
}

/** Sync secondary video-audio volume when master volume changes (called from setVolume). */
export function syncVideoAudioVolume(vol) {
  if (_videoAudio) _videoAudio.volume = vol * VOLUME_SCALE;
}
