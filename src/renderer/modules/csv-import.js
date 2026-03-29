/**
 * csv-import.js  (Spotify / TuneMyMusic CSV import)
 *
 * To avoid circular deps (library.js ↔ csv-import.js), the library helpers are
 * injected when the function is called.  See library.js for the call-site.
 */

import { escapeHtml, showToast } from './utils.js';
import { callbacks } from './callbacks.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);

/**
 * Open the Spotify / TuneMyMusic CSV import modal.
 *
 * @param {{ createPlaylist, renderPlaylists, renderLibrary }} helpers
 *   Functions from library.js, injected to break the circular dependency.
 */
export function openSpotifyImport({ createPlaylist, renderPlaylists, renderLibrary }) {
  const modal        = $('#spotify-modal');
  const stepSelect   = $('#spotify-step-url');
  const stepProgress = $('#spotify-step-progress');
  const errorEl      = $('#spotify-error');
  const fileListEl   = $('#spotify-file-list');
  const startBtn     = $('#spotify-start');

  let cancelled        = false;
  let pendingPlaylists = null;

  // Reset
  errorEl.classList.add('hidden');
  fileListEl.classList.add('hidden');
  fileListEl.innerHTML = '';
  stepSelect.classList.remove('hidden');
  stepProgress.classList.add('hidden');
  startBtn.disabled = true;
  modal.classList.remove('hidden');

  function resetModal() {
    startBtn.disabled       = true;
    startBtn.textContent    = I18n.t('spotify.import');
    $('#spotify-modal-title').textContent = I18n.t('spotify.title');
    $('#spotify-done-buttons').style.display = 'none';
    pendingPlaylists = null;
  }

  function cleanup() {
    cancelled = true;
    modal.classList.add('hidden');
    resetModal();
  }

  $('#spotify-cancel').onclick = cleanup;
  modal.onclick = (e) => { if (e.target === modal) cleanup(); };

  // Open TuneMyMusic in system browser
  $('#spotify-exportify-link').onclick = (e) => {
    e.preventDefault();
    window.snowify.openExternal('https://www.tunemymusic.com/transfer');
  };

  // Pick CSV files via system dialog
  $('#spotify-pick-files').onclick = async () => {
    const playlists = await window.snowify.spotifyPickCsv();
    if (!playlists || !playlists.length) return;
    pendingPlaylists = playlists;
    fileListEl.innerHTML = playlists.map(p =>
      `<div class="spotify-file-item"><span class="spotify-file-name">${escapeHtml(p.name)}</span><span class="spotify-file-count">${p.tracks.length} tracks</span></div>`
    ).join('');
    fileListEl.classList.remove('hidden');
    startBtn.disabled = false;
    errorEl.classList.add('hidden');
  };

  startBtn.onclick = async () => {
    if (!pendingPlaylists || !pendingPlaylists.length) {
      errorEl.textContent = I18n.t('spotify.selectAtLeastOne');
      errorEl.classList.remove('hidden');
      return;
    }

    errorEl.classList.add('hidden');
    startBtn.disabled    = true;
    startBtn.textContent = I18n.t('spotify.importing');

    stepSelect.classList.add('hidden');
    stepProgress.classList.remove('hidden');

    const trackList     = $('#spotify-track-list');
    const progressFill  = $('#spotify-progress-fill');
    const progressText  = $('#spotify-progress-text');
    const progressCount = $('#spotify-progress-count');

    let totalImported  = 0;
    let totalPlaylists = 0;
    const allFailedTracks = [];

    for (let pi = 0; pi < pendingPlaylists.length; pi++) {
      if (cancelled) break;
      const pl = pendingPlaylists[pi];

      if (pendingPlaylists.length > 1) {
        $('#spotify-modal-title').textContent = I18n.t('spotify.importingProgress', { current: pi + 1, total: pendingPlaylists.length, name: pl.name });
      } else {
        $('#spotify-modal-title').textContent = pl.name;
      }

      progressFill.style.width  = '0%';
      progressCount.textContent = '';
      progressText.textContent  = I18n.t('spotify.matching');
      trackList.innerHTML       = '';

      const total      = pl.tracks.length;
      const BATCH_SIZE = 3;

      trackList.innerHTML = pl.tracks.map((t, i) => `
        <div class="spotify-track-item pending" id="sp-track-${i}">
          <span class="spotify-track-status"><span class="dots">\u2022\u2022\u2022</span></span>
          <span class="spotify-track-title">${escapeHtml(t.title)}</span>
          <span class="spotify-track-artist">${escapeHtml(t.artist)}</span>
        </div>
      `).join('');

      const matchedTracks = [];
      const failedTracks  = [];
      let matched = 0;
      let failed  = 0;

      for (let i = 0; i < total; i += BATCH_SIZE) {
        if (cancelled) break;

        const batch    = pl.tracks.slice(i, Math.min(i + BATCH_SIZE, total));
        const promises = batch.map((t, bi) => {
          const idx = i + bi;
          return window.snowify.spotifyMatchTrack(t.title, t.artist)
            .catch(() => null)
            .then(result => ({ idx, result }));
        });

        const results = await Promise.all(promises);
        if (cancelled) break;

        for (const { idx, result } of results) {
          const t  = pl.tracks[idx];
          const el = $(`#sp-track-${idx}`);
          if (result) {
            matchedTracks.push(result);
            matched++;
            if (el) {
              el.classList.remove('pending');
              el.classList.add('matched');
              el.querySelector('.spotify-track-status').innerHTML = '<svg class="check" width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><path d="M6.5 12.5l-4-4 1.4-1.4 2.6 2.6 5.6-5.6 1.4 1.4-7 7z"/></svg>';
            }
          } else {
            failedTracks.push({ title: t.title, artist: t.artist });
            failed++;
            if (el) {
              el.classList.remove('pending');
              el.classList.add('unmatched');
              el.querySelector('.spotify-track-status').innerHTML = '<svg class="cross" width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/></svg>';
            }
          }
        }

        const done = Math.min(i + BATCH_SIZE, total);
        progressCount.textContent = `${done} / ${total}`;
        progressFill.style.width  = `${(done / total) * 100}%`;
        progressText.textContent  = pendingPlaylists.length > 1
          ? I18n.t('spotify.matchingPlaylist', { current: pi + 1, total: pendingPlaylists.length })
          : I18n.t('spotify.matching');

        const lastEl = $(`#sp-track-${Math.min(i + BATCH_SIZE, total) - 1}`);
        lastEl?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      }

      if (cancelled) {
        if (matchedTracks.length) {
          const playlist = createPlaylist(pl.name);
          playlist.tracks = matchedTracks;
          callbacks.saveState();
          renderPlaylists();
          renderLibrary();
        }
        break;
      }

      if (matchedTracks.length) {
        const playlist = createPlaylist(pl.name);
        playlist.tracks = matchedTracks;
        callbacks.saveState();
        renderPlaylists();
        renderLibrary();
        totalImported += matched;
        totalPlaylists++;
      }

      allFailedTracks.push(...failedTracks);
      progressText.textContent = I18n.t('spotify.matchedOf', { matched, total }) +
        (failed ? ` (${I18n.t('spotify.notFound', { count: failed })})` : '');
    }

    if (cancelled) { showToast(I18n.t('toast.importCancelled')); return; }

    // Final summary
    if (pendingPlaylists.length > 1) {
      $('#spotify-modal-title').textContent = I18n.t('spotify.importComplete');
      progressText.textContent  = I18n.t('toast.importedPlaylists', { playlistCount: totalPlaylists, trackCount: totalImported });
      progressFill.style.width  = '100%';
      progressCount.textContent = '';
      showToast(I18n.t('toast.importedPlaylists', { playlistCount: totalPlaylists, trackCount: totalImported }));
    } else if (totalPlaylists) {
      showToast(I18n.t('toast.importedTracks', { count: totalImported }));
    } else {
      showToast(I18n.t('toast.noTracksMatched'));
    }

    if (allFailedTracks.length) {
      trackList.innerHTML = `<div class="spotify-failed-header">${I18n.t('spotify.failedToMatch', { count: allFailedTracks.length })}</div>` +
        allFailedTracks.map(t =>
          `<div class="spotify-track-item unmatched"><span class="spotify-track-status"><svg class="cross" width="16" height="16" viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/></svg></span><span class="spotify-track-title">${escapeHtml(t.title)}</span><span class="spotify-track-artist">${escapeHtml(t.artist)}</span></div>`
        ).join('');
      trackList.scrollTop = 0;
    } else {
      trackList.innerHTML = '';
    }

    $('#spotify-done-buttons').style.display = '';
    $('#spotify-done').onclick = () => { cleanup(); resetModal(); };
  };
}
