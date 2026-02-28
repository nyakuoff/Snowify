// ─── Radio Engine (internet radio via radio-browser.info) ───
// Closure-based module. Created via RadioEngine(opts).
// Encapsulates radio state, playback, search, and UI rendering.

window.RadioEngine = function RadioEngine(opts) {
  'use strict';

  const {
    $, escapeHtml,
    getState, getEngine, getAudio, setAudio,
    showToast, updatePlayButton, clearDiscordPresence,
    renderQueue, saveState, togglePlay,
    ipc
  } = opts;

  // ─── Constants ───
  const FALLBACK_SVG = '<svg width="48" height="48" viewBox="0 0 16 16" fill="currentColor" style="color:var(--text-subdued)"><path d="M3.05 3.05a7 7 0 0 0 0 9.9.5.5 0 0 1-.707.707 8 8 0 0 1 0-11.314.5.5 0 0 1 .707.707m2.122 2.122a4 4 0 0 0 0 5.656.5.5 0 1 1-.708.708 5 5 0 0 1 0-7.072.5.5 0 0 1 .708.708m5.656-.708a.5.5 0 0 1 .708 0 5 5 0 0 1 0 7.072.5.5 0 1 1-.708-.708 4 4 0 0 0 0-5.656.5.5 0 0 1 0-.708m2.122-2.12a.5.5 0 0 1 .707 0 8 8 0 0 1 0 11.313.5.5 0 0 1-.707-.707 7 7 0 0 0 0-9.9.5.5 0 0 1 0-.707zM6 8a2 2 0 1 1 2.5 1.937V15.5a.5.5 0 0 1-1 0V9.937A2 2 0 0 1 6 8"/></svg>';
  const FALLBACK_IMG = 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 96 96"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="rgba(255,255,255,0.06)"/><stop offset="100%" stop-color="rgba(255,255,255,0.02)"/></linearGradient></defs><rect width="96" height="96" rx="8" fill="#1a1a1a"/><rect width="96" height="96" rx="8" fill="url(#g)"/><g transform="translate(24,24) scale(3)" fill="#6a6a6a"><path d="M3.05 3.05a7 7 0 0 0 0 9.9.5.5 0 0 1-.707.707 8 8 0 0 1 0-11.314.5.5 0 0 1 .707.707m2.122 2.122a4 4 0 0 0 0 5.656.5.5 0 1 1-.708.708 5 5 0 0 1 0-7.072.5.5 0 0 1 .708.708m5.656-.708a.5.5 0 0 1 .708 0 5 5 0 0 1 0 7.072.5.5 0 1 1-.708-.708 4 4 0 0 0 0-5.656.5.5 0 0 1 0-.708m2.122-2.12a.5.5 0 0 1 .707 0 8 8 0 0 1 0 11.313.5.5 0 0 1-.707-.707 7 7 0 0 0 0-9.9.5.5 0 0 1 0-.707zM6 8a2 2 0 1 1 2.5 1.937V15.5a.5.5 0 0 1-1 0V9.937A2 2 0 0 1 6 8"/></g></svg>');

  // ─── Private state ───
  let _geo = null;
  let _generation = 0;
  let _stationsCache = [];
  let _searchTimer = null;
  let _savedQueueBeforeRadio = null;

  // ─── Buffering event handlers ───
  function _onWaiting() {
    if (!getState().radioMode) return;
    $('#progress-bar').classList.add('buffering');
    $('#max-np-progress-bar').classList.add('buffering');
  }

  function _onPlaying() {
    $('#progress-bar').classList.remove('buffering');
    $('#max-np-progress-bar').classList.remove('buffering');
  }

  function _bindBuffering(el) {
    el.addEventListener('waiting', _onWaiting);
    el.addEventListener('playing', _onPlaying);
  }

  function _unbindBuffering(el) {
    el.removeEventListener('waiting', _onWaiting);
    el.removeEventListener('playing', _onPlaying);
    _onPlaying(); // clear any lingering buffering state
  }

  // ─── Cleanup ───
  function cleanup() {
    const state = getState();
    if (!state.radioMode) return;
    _unbindBuffering(getAudio());
    state.radioMode = false;
    state.currentStation = null;
    $('#now-playing-bar').classList.remove('radio-mode');
    $('#max-np').classList.remove('radio-mode');
    $('#time-total').classList.remove('live-badge');
    $('#max-np-time-total').classList.remove('live-badge');
    if (_savedQueueBeforeRadio) {
      state.queue = _savedQueueBeforeRadio.queue;
      state.originalQueue = _savedQueueBeforeRadio.originalQueue;
      state.queueIndex = _savedQueueBeforeRadio.queueIndex;
      state.playingPlaylistId = _savedQueueBeforeRadio.playingPlaylistId;
      _savedQueueBeforeRadio = null;
    }
  }

  // ─── Station card HTML ───
  function _buildCard(station) {
    const hasFavicon = station.favicon && station.favicon.trim();
    const faviconHtml = hasFavicon
      ? `<div class="station-cover-wrap"><img class="album-card-cover station-card-cover" src="${escapeHtml(station.favicon)}" alt="" loading="lazy" onerror="this.style.display='none';this.nextElementSibling.style.display=''" /><div class="station-cover-fallback station-fallback-icon" style="display:none">${FALLBACK_SVG}</div></div>`
      : `<div class="album-card-cover station-fallback-icon">${FALLBACK_SVG}</div>`;
    const meta = [station.tags, station.country, station.bitrate ? station.bitrate + ' kbps' : ''].filter(Boolean).join(' · ');
    return `
      <div class="album-card station-card" data-station-uuid="${escapeHtml(station.stationuuid)}">
        ${faviconHtml}
        <button class="album-card-play" title="Play">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg>
        </button>
        <div class="album-card-name" title="${escapeHtml(station.name)}">${escapeHtml(station.name)}</div>
        <div class="album-card-meta">${escapeHtml(meta)}</div>
      </div>`;
  }

  // ─── Section builders ───
  function _buildScrollSection(title, stations) {
    const cards = stations.map(s => _buildCard(s)).join('');
    return `<div class="explore-section"><h2>${escapeHtml(title)}</h2><div class="scroll-container"><button class="scroll-arrow scroll-arrow-left" data-dir="left"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg></button><div class="album-scroll">${cards}</div><button class="scroll-arrow scroll-arrow-right" data-dir="right"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg></button></div></div>`;
  }

  function _buildTrendingSection(title, stations) {
    const items = stations.map((s, i) => {
      const hasFav = s.favicon && s.favicon.trim();
      const faviconHtml = hasFav
        ? `<img class="top-song-thumb" src="${escapeHtml(s.favicon)}" alt="" loading="lazy" onerror="this.style.display='none';this.nextElementSibling.style.display=''" /><div class="top-song-thumb station-trending-fallback" style="display:none">${FALLBACK_SVG}</div>`
        : `<div class="top-song-thumb station-trending-fallback">${FALLBACK_SVG}</div>`;
      const meta = [s.country, s.bitrate ? s.bitrate + ' kbps' : ''].filter(Boolean).join(' · ');
      return `
        <div class="top-song-item station-trending-item" data-station-uuid="${escapeHtml(s.stationuuid)}">
          <div class="top-song-rank">${i + 1}</div>
          <div class="top-song-thumb-wrap">
            ${faviconHtml}
            <div class="top-song-play"><svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5z"/></svg></div>
          </div>
          <div class="top-song-info">
            <div class="top-song-title">${escapeHtml(s.name)}</div>
            <div class="top-song-artist">${escapeHtml(meta)}</div>
          </div>
        </div>`;
    }).join('');
    return `<div class="explore-section"><h2>${escapeHtml(title)}</h2><div class="top-songs-grid">${items}</div></div>`;
  }

  function _buildGenreGrid(tags) {
    const GENRE_COLORS = ['#e74c3c', '#e67e22', '#f1c40f', '#2ecc71', '#1abc9c', '#3498db', '#9b59b6', '#e84393', '#00cec9', '#fd79a8', '#6c5ce7', '#00b894'];
    const items = tags.map((t, i) => {
      const bg = GENRE_COLORS[i % GENRE_COLORS.length];
      return `<div class="mood-card radio-genre-card" data-tag="${escapeHtml(t.name)}" style="border-left-color:${bg}">${escapeHtml(t.name)} <span style="opacity:0.5;font-size:11px">${t.stationcount}</span></div>`;
    }).join('');
    return `<div class="explore-section"><h2>Browse by Tag</h2><div class="mood-grid">${items}</div></div>`;
  }

  // ─── Station lookup ───
  function findByUuid(uuid) {
    const state = getState();
    return state.favoriteStations.find(s => s.stationuuid === uuid)
      || _stationsCache.find(s => s.stationuuid === uuid)
      || null;
  }

  // ─── Attach listeners to rendered radio content ───
  function _attachListeners(content) {
    // Scroll arrows
    content.querySelectorAll('.scroll-container').forEach(container => {
      const scrollEl = container.querySelector('.album-scroll');
      if (!scrollEl) return;
      container.querySelectorAll('.scroll-arrow').forEach(btn => {
        btn.addEventListener('click', () => {
          const dir = btn.dataset.dir === 'left' ? -400 : 400;
          scrollEl.scrollBy({ left: dir, behavior: 'smooth' });
        });
      });
    });

    // Station card clicks
    content.querySelectorAll('.station-card').forEach(card => {
      card.addEventListener('click', () => {
        const uuid = card.dataset.stationUuid;
        const station = findByUuid(uuid);
        if (station) play(station);
      });
      card.querySelector('.album-card-play')?.addEventListener('click', (e) => {
        e.stopPropagation();
        const uuid = card.closest('.station-card')?.dataset?.stationUuid;
        const station = findByUuid(uuid);
        if (station) play(station);
      });
    });

    // Trending item clicks
    content.querySelectorAll('.station-trending-item').forEach(item => {
      item.addEventListener('click', () => {
        const uuid = item.dataset.stationUuid;
        const station = findByUuid(uuid);
        if (station) play(station);
      });
    });

    // Genre card clicks
    content.querySelectorAll('.radio-genre-card').forEach(card => {
      card.addEventListener('click', async () => {
        const tag = card.dataset.tag;
        if (!tag) return;
        content.innerHTML = '<div class="loading"><div class="spinner"></div></div>';
        const stations = await ipc.byTag(tag);
        _stationsCache = stations;
        let html = '';
        if (stations.length) {
          html += _buildScrollSection(tag, stations);
          html += _buildTrendingSection(`All "${tag}" Stations`, stations);
        } else {
          html += `<div class="empty-state"><p>No stations found for "${escapeHtml(tag)}".</p></div>`;
        }
        content.innerHTML = html;
        _attachListeners(content);
      });
    });
  }

  // ─── Render radio view ───
  async function render() {
    const state = getState();
    const content = $('#radio-content');
    content.innerHTML = '<div class="loading"><div class="spinner"></div></div>';

    try {
      if (!_geo) _geo = await ipc.detectGeo();

      const hasGeo = !!_geo.countryCode;
      const [local, trendingCountry, trendingWorld, tags] = await Promise.all([
        hasGeo ? ipc.byCountry(_geo.countryCode) : Promise.resolve([]),
        hasGeo ? ipc.trendingByCountry(_geo.countryCode, 20) : Promise.resolve([]),
        ipc.topClick(20),
        ipc.tags(),
      ]);

      let html = '';

      if (state.favoriteStations.length)
        html += _buildScrollSection('Your Stations', state.favoriteStations);

      if (local.length) {
        const label = _geo.city
          ? `Popular in ${_geo.city}, ${_geo.country}`
          : (_geo.country ? `Popular in ${_geo.country}` : 'Popular Stations');
        html += _buildScrollSection(label, local);
      }

      if (trendingCountry.length) {
        const countryLabel = _geo.country || 'Your Country';
        html += _buildTrendingSection(`Trending in ${countryLabel}`, trendingCountry);
      }

      if (trendingWorld.length)
        html += _buildTrendingSection('Trending Worldwide', trendingWorld);

      if (tags.length)
        html += _buildGenreGrid(tags.slice(0, 30));

      _stationsCache = [...state.favoriteStations, ...local, ...trendingCountry, ...trendingWorld];
      content.innerHTML = html || '<div class="empty-state"><p>Could not load radio stations.</p></div>';
      _attachListeners(content);
    } catch (err) {
      console.error('renderRadio error:', err);
      content.innerHTML = '<div class="empty-state"><p>Could not load radio stations.</p></div>';
    }
  }

  // ─── Search ───
  function initSearch() {
    const input = $('#radio-search-input');
    const clearBtn = $('#radio-search-clear');
    const label = $('#radio-search-label');
    const inputWrap = $('#radio-search-input-wrap');
    if (!input || !clearBtn) return;

    input.addEventListener('input', () => {
      const q = input.value.trim();
      clearBtn.classList.toggle('hidden', !q);
      clearTimeout(_searchTimer);
      if (!q) {
        if (getState().currentView === 'radio') render();
        return;
      }
      _searchTimer = setTimeout(async () => {
        const content = $('#radio-content');
        content.innerHTML = '<div class="loading"><div class="spinner"></div></div>';
        const results = await ipc.search(q);
        _stationsCache = results;
        if (!results.length) {
          content.innerHTML = `<div class="empty-state"><p>No stations found for "${escapeHtml(q)}".</p></div>`;
          return;
        }
        let html = _buildScrollSection(`Results for "${q}"`, results);
        html += _buildTrendingSection('All Results', results);
        content.innerHTML = html;
        _attachListeners(content);
      }, 400);
    });

    clearBtn.addEventListener('click', () => {
      input.value = '';
      clearBtn.classList.add('hidden');
      inputWrap.classList.add('hidden');
      label.classList.remove('hidden');
      if (getState().currentView === 'radio') render();
    });

    label.addEventListener('click', () => {
      label.classList.add('hidden');
      inputWrap.classList.remove('hidden');
      input.focus();
    });

    input.addEventListener('blur', () => {
      if (!input.value.trim()) {
        inputWrap.classList.add('hidden');
        label.classList.remove('hidden');
      }
    });
  }

  function cancelSearch() {
    if (_searchTimer) { clearTimeout(_searchTimer); _searchTimer = null; }
  }

  function resetSearchPill() {
    const ri = $('#radio-search-input');
    const rl = $('#radio-search-label');
    const rw = $('#radio-search-input-wrap');
    const rc = $('#radio-search-clear');
    if (ri) ri.value = '';
    if (rl) rl.classList.remove('hidden');
    if (rw) rw.classList.add('hidden');
    if (rc) rc.classList.add('hidden');
  }

  // ─── Play a radio station ───
  async function play(station) {
    const streamUrl = station.url_resolved || station.url;
    if (!streamUrl) {
      showToast('No stream URL for this station');
      return;
    }

    const state = getState();
    const engine = getEngine();
    const gen = ++_generation;

    state.radioMode = true;
    $('#now-playing-bar').classList.add('radio-mode');
    $('#max-np').classList.add('radio-mode');
    state.currentStation = station;
    state.isLoading = true;
    if (state.queue.length > 0) {
      _savedQueueBeforeRadio = {
        queue: [...state.queue],
        originalQueue: [...state.originalQueue],
        queueIndex: state.queueIndex,
        playingPlaylistId: state.playingPlaylistId
      };
    }
    state.queue = [];
    state.originalQueue = [];
    state.queueIndex = -1;
    state.playingPlaylistId = null;

    let audio = getAudio();
    if (engine.isInProgress()) { engine.instantComplete(); audio = engine.getActiveAudio(); setAudio(audio); }
    engine.clearPreload();

    showNowPlaying(station);
    renderQueue();
    updatePlayButton();
    ipc.click(station.stationuuid).catch(() => {});

    try {
      showToast(`Tuning in: ${station.name}`);
      audio = engine.getActiveAudio();
      setAudio(audio);
      audio.src = streamUrl;
      audio.load();
      audio.volume = state.volume * engine.VOLUME_SCALE;

      // Race play() against a timeout — radio streams can hang
      const playPromise = audio.play();
      const timeoutPromise = new Promise((_, reject) =>
        setTimeout(() => reject(new Error('Timeout')), 15000)
      );
      await Promise.race([playPromise, timeoutPromise]);

      if (gen !== _generation) return; // stale — user clicked another station
      state.isPlaying = true;
      state.isLoading = false;
      updatePlayButton();
      $('#time-total').classList.add('live-badge');
      $('#max-np-time-total').classList.add('live-badge');
      _bindBuffering(audio);
      updateDiscordPresence(station);
      saveState();
    } catch (err) {
      if (gen !== _generation) return;
      if (err?.name === 'AbortError') return;
      console.error('Radio play error:', err);
      showToast('Station unavailable — try another');
      cleanup();
      state.isPlaying = false;
      state.isLoading = false;
      updatePlayButton();
      renderQueue();
    }
  }

  // ─── Now playing UI ───
  function showNowPlaying(station) {
    const bar = $('#now-playing-bar');
    bar.classList.remove('hidden');
    document.querySelector('#app').classList.remove('no-player');

    const npThumb = $('#np-thumbnail');
    npThumb.src = station.favicon || FALLBACK_IMG;
    npThumb.onerror = station.favicon
      ? () => { npThumb.src = FALLBACK_IMG; npThumb.onerror = null; }
      : null;
    const npTitle = $('#np-title');
    npTitle.textContent = station.name;
    npTitle.classList.remove('clickable');
    npTitle.onclick = null;

    const npArtist = $('#np-artist');
    const meta = [station.tags, station.country, station.bitrate ? station.bitrate + ' kbps' : ''].filter(Boolean).join(' · ');
    npArtist.textContent = meta || 'Live Radio';
    npArtist.classList.remove('clickable');
    npArtist.onclick = null;

    const isFav = getState().favoriteStations.some(s => s.stationuuid === station.stationuuid);
    $('#np-like').classList.toggle('liked', isFav);

    updateMediaSession(station);
  }

  // ─── Favorites ───
  function toggleFavorite(station) {
    const state = getState();
    const idx = state.favoriteStations.findIndex(s => s.stationuuid === station.stationuuid);
    if (idx >= 0) {
      state.favoriteStations.splice(idx, 1);
      showToast(`Removed: ${station.name}`);
    } else {
      state.favoriteStations.push({
        stationuuid: station.stationuuid, name: station.name,
        url: station.url || '', url_resolved: station.url_resolved || '', favicon: station.favicon || '',
        tags: station.tags || '', country: station.country || '',
        countrycode: station.countrycode || '', bitrate: station.bitrate || 0,
        codec: station.codec || ''
      });
      showToast(`Added: ${station.name}`);
    }
    const isFav = idx < 0; // was NOT found = just added = is now favorite
    $('#np-like').classList.toggle('liked', isFav);
    const maxLike = $('#max-np-like');
    if (maxLike) maxLike.classList.toggle('liked', isFav);
    saveState();
    if (state.currentView === 'radio') render();
    return isFav;
  }

  function isFavorite(station) {
    if (!station) return false;
    return getState().favoriteStations.some(s => s.stationuuid === station.stationuuid);
  }

  // ─── Discord / MediaSession ───
  function updateDiscordPresence(station) {
    if (!getState().discordRpc || !station) return;
    ipc.updatePresence({
      title: station.name,
      artist: 'Live Radio',
      thumbnail: station.favicon || '',
      startTimestamp: Date.now()
    });
  }

  function updateMediaSession(station) {
    if (!('mediaSession' in navigator)) return;
    navigator.mediaSession.metadata = new MediaMetadata({
      title: station.name,
      artist: 'Live Radio',
      artwork: station.favicon ? [{ src: station.favicon, sizes: '96x96' }] : []
    });
    navigator.mediaSession.setActionHandler('play', () => togglePlay());
    navigator.mediaSession.setActionHandler('pause', () => togglePlay());
    navigator.mediaSession.setActionHandler('previoustrack', null);
    navigator.mediaSession.setActionHandler('nexttrack', null);
    navigator.mediaSession.setActionHandler('seekto', null);
  }

  // ─── Integration helpers (simplify guards in app.js) ───
  function isActive() {
    return getState().radioMode;
  }

  function getStation() {
    return getState().currentStation;
  }

  function handleStreamEnd() {
    showToast('Radio stream ended — try another station');
    cleanup();
    getState().isPlaying = false;
    updatePlayButton();
    clearDiscordPresence();
    renderQueue();
  }

  function handleStreamError() {
    showToast('Radio stream lost — try another station');
    cleanup();
    const state = getState();
    state.isPlaying = false;
    state.isLoading = false;
    updatePlayButton();
    clearDiscordPresence();
    renderQueue();
  }

  function handleStall() {
    showToast('Radio stream stalled — try another station');
  }

  function renderQueueCard() {
    const state = getState();
    if (!state.radioMode || !state.currentStation) return null;
    const s = state.currentStation;
    const nowPlayingHtml = `
        <div class="queue-item active">
          <img src="${s.favicon ? escapeHtml(s.favicon) : 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7'}" alt="" onerror="this.src='data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7'" />
          <div class="queue-item-info">
            <div class="queue-item-title">${escapeHtml(s.name)}</div>
            <div class="queue-item-artist">Live Radio</div>
          </div>
        </div>`;
    const upNextHtml = `<p style="color:var(--text-subdued);font-size:13px;">Live Radio — no queue</p>`;
    return { nowPlayingHtml, upNextHtml };
  }

  function getMaxNPTrack() {
    const state = getState();
    if (!state.currentStation) return null;
    return {
      title: state.currentStation.name,
      artist: 'Live Radio',
      thumbnail: state.currentStation.favicon || '',
      id: 'radio_' + state.currentStation.stationuuid
    };
  }

  function destroy() {
    cancelSearch();
    cleanup();
  }

  // ─── Public API ───
  return {
    FALLBACK_SVG,
    FALLBACK_IMG,
    isActive,
    getStation,
    play,
    cleanup,
    toggleFavorite,
    initSearch,
    render,
    resetSearchPill,
    cancelSearch,
    updateDiscordPresence,
    updateMediaSession,
    showNowPlaying,
    handleStreamEnd,
    handleStreamError,
    handleStall,
    renderQueueCard,
    getMaxNPTrack,
    isFavorite,
    findByUuid,
    destroy,
  };
};
