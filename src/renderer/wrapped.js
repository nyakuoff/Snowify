// ─── Snowify Wrapped ───
// Computes yearly listening stats and renders an animated, full-screen slide experience.
// Exposed as window.WrappedManager = { show(year, devMode), hide() }.

(function () {
  'use strict';

  const MONTHS = [
    'January', 'February', 'March', 'April', 'May', 'June',
    'July', 'August', 'September', 'October', 'November', 'December',
  ];

  // Per-slide accent color (used for glow + tint)
  const SLIDE_ACCENTS = [
    '#7c4dff',  // 0 intro        – violet
    '#2979ff',  // 1 totals        – blue
    '#00c853',  // 2 top-artist    – green
    '#00bcd4',  // 3 top-artists   – teal
    '#ff6d00',  // 4 top-song      – amber
    '#e91e8c',  // 5 top-songs     – rose
    '#7c4dff',  // 6 personality   – violet
    '#43a047',  // 7 best-month    – earth green
    '#aa00ff',  // 8 outro         – purple
  ];

  // ─── Statistics computation ───────────────────────────────────────────────

  function computeWrapped(playLog, year) {
    const entries = playLog.filter(e => new Date(e.ts).getFullYear() === year);
    if (entries.length < 5) return null; // not enough data

    // Build thumbnail lookup from live state to cover old playLog entries that predate thumbnail storage
    const thumbLookup = new Map();
    const liveState = window.__snowifyState;
    [...(liveState?.recentTracks || []), ...(liveState?.likedSongs || [])]
      .forEach(t => { if (t.id && t.thumbnail) thumbLookup.set(t.id, t.thumbnail); });

    // — Songs —
    const songMap = new Map();
    for (const e of entries) {
      const thumb = e.thumbnail || thumbLookup.get(e.id) || '';
      if (!songMap.has(e.id)) songMap.set(e.id, { id: e.id, title: e.title, artist: e.artist, thumbnail: thumb, count: 0, totalMs: 0 });
      const s = songMap.get(e.id);
      s.count++;
      s.totalMs += e.durationMs || 0;
      if (!s.thumbnail && thumb) s.thumbnail = thumb;
    }
    const topSongs = [...songMap.values()].sort((a, b) => b.count - a.count).slice(0, 5);

    // — Artists —
    const artistMap = new Map();
    for (const e of entries) {
      const key = (e.artist || 'Unknown Artist').toLowerCase();
      const thumb = e.thumbnail || thumbLookup.get(e.id) || '';
      if (!artistMap.has(key)) artistMap.set(key, { name: e.artist || 'Unknown Artist', thumbnail: thumb, count: 0, totalMs: 0 });
      const a = artistMap.get(key);
      a.count++;
      a.totalMs += e.durationMs || 0;
      if (!a.thumbnail && thumb) a.thumbnail = thumb;
    }
    const topArtists = [...artistMap.values()].sort((a, b) => b.count - a.count).slice(0, 5);

    // — Time —
    const totalMs = entries.reduce((sum, e) => sum + (e.durationMs || 0), 0);
    const totalMinutes = Math.round(totalMs / 60000);
    const totalHours = Math.round(totalMinutes / 60);

    // — Best month —
    const monthCounts = new Array(12).fill(0);
    for (const e of entries) monthCounts[new Date(e.ts).getMonth()]++;
    const bestMonthIdx = monthCounts.indexOf(Math.max(...monthCounts));

    // — Personality (time-of-day distribution) —
    const hourCounts = new Array(24).fill(0);
    for (const e of entries) hourCounts[new Date(e.ts).getHours()]++;
    const total = hourCounts.reduce((s, c) => s + c, 0) || 1;

    const nightPct  = [22,23,0,1,2,3].reduce((s,h) => s + hourCounts[h], 0) / total;
    const morningPct= Array.from({length:5},(_,i)=>i+6).reduce((s,h)=>s+hourCounts[h],0)/total;
    const afternoonPct=Array.from({length:5},(_,i)=>i+12).reduce((s,h)=>s+hourCounts[h],0)/total;
    const eveningPct =Array.from({length:5},(_,i)=>i+17).reduce((s,h)=>s+hourCounts[h],0)/total;
    const maxPct = Math.max(nightPct, morningPct, afternoonPct, eveningPct);

    let personality, personalityEmoji, personalityKey;
    if (maxPct === nightPct && nightPct > 0.2)           { personality = 'Night Owl';          personalityEmoji = '🌙'; personalityKey = 'wrapped.personalityNight'; }
    else if (maxPct === morningPct && morningPct > 0.2)  { personality = 'Early Bird';         personalityEmoji = '🌅'; personalityKey = 'wrapped.personalityMorning'; }
    else if (maxPct === afternoonPct && afternoonPct > 0.2){ personality = 'Afternoon Listener';personalityEmoji = '☀️'; personalityKey = 'wrapped.personalityAfternoon'; }
    else if (maxPct === eveningPct && eveningPct > 0.2)  { personality = 'Evening Listener';   personalityEmoji = '🌆'; personalityKey = 'wrapped.personalityEvening'; }
    else                                                  { personality = 'All-Day Listener';   personalityEmoji = '🎵'; personalityKey = 'wrapped.personalityAllday'; }

    // — Listening streak (consecutive days) —
    const daySet = new Set();
    for (const e of entries) {
      const d = new Date(e.ts);
      daySet.add(new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime());
    }
    const sortedDays = [...daySet].sort((a, b) => a - b);
    const DAY_MS = 86400000;
    let maxStreak = 1, curStreak = 1;
    for (let i = 1; i < sortedDays.length; i++) {
      curStreak = sortedDays[i] - sortedDays[i - 1] === DAY_MS ? curStreak + 1 : 1;
      if (curStreak > maxStreak) maxStreak = curStreak;
    }

    return {
      year,
      totalPlays: entries.length,
      totalMinutes,
      totalHours,
      uniqueSongs: songMap.size,
      uniqueArtists: artistMap.size,
      topSongs,
      topArtists,
      bestMonth: bestMonthIdx,
      bestMonthPlays: monthCounts[bestMonthIdx],
      personality,
      personalityKey,
      personalityEmoji,
      maxStreak,
    };
  }

  // ─── Slide builders ───────────────────────────────────────────────────────

  function t(key, fallback) {
    if (window.I18n && typeof window.I18n.t === 'function') {
      const result = window.I18n.t(key);
      return result !== key ? result : (fallback || key);
    }
    return fallback || key;
  }

  function escapeHtml(str) {
    return String(str || '')
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  // Build a blurred mosaic background from an array of thumbnail URLs.
  // Tiles are arranged in a 3×3 (or whatever fits) grid, each blurred + darkened.
  function makeMosaicBg(thumbUrls, accent) {
    const bg = document.createElement('div');
    bg.className = 'wrapped-mosaic-bg';

    // Fill up to 9 tiles, cycling through available images
    const tiles = [];
    for (let i = 0; i < 9; i++) {
      const url = thumbUrls[i % thumbUrls.length];
      if (url) tiles.push(url);
    }
    tiles.forEach(url => {
      const tile = document.createElement('div');
      tile.className = 'wrapped-mosaic-tile';
      tile.style.backgroundImage = `url('${url}')`;
      bg.appendChild(tile);
    });

    // Radial glow accent overlay
    const glow = document.createElement('div');
    glow.className = 'wrapped-bg-glow';
    glow.style.background = `radial-gradient(ellipse 70% 60% at 50% 40%, ${accent}55 0%, transparent 70%)`;
    bg.appendChild(glow);

    // Dark scrim so text is always readable
    const scrim = document.createElement('div');
    scrim.className = 'wrapped-bg-scrim';
    bg.appendChild(scrim);

    return bg;
  }

  function buildSlides(stats) {
    // Collect all thumbnail URLs from top songs for mosaic backgrounds
    const allThumbs = stats.topSongs.map(s => s.thumbnail).filter(Boolean);
    // Fallback tiles if no thumbnails available
    const thumbs = allThumbs.length ? allThumbs : [];

    return [
      buildIntro(stats, thumbs),
      buildTotals(stats, thumbs),
      buildTopArtist(stats, thumbs),
      buildTopArtists(stats, thumbs),
      buildTopSong(stats, thumbs),
      buildTopSongs(stats, thumbs),
      buildPersonality(stats, thumbs),
      buildBestMonth(stats, thumbs),
      buildOutro(stats, thumbs),
    ];
  }

  function el(tag, cls, html) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html !== undefined) e.innerHTML = html;
    return e;
  }

  function makeSlide(idx, thumbs, children) {
    const slide = el('div', 'wrapped-slide');
    slide.dataset.accent = SLIDE_ACCENTS[idx];

    // Mosaic background if we have thumbnails, else a solid dark gradient
    if (thumbs.length) {
      slide.appendChild(makeMosaicBg(thumbs, SLIDE_ACCENTS[idx]));
    } else {
      slide.style.background = '#0d0520';
    }

    const content = el('div', 'wrapped-content');
    children.forEach((c, i) => {
      if (c) {
        c.style.animationDelay = (i * 0.12) + 's';
        content.appendChild(c);
      }
    });
    slide.appendChild(content);
    return slide;
  }

  // Build an avatar image element (for track/artist art)
  function makeArt(url, cls) {
    const wrap = el('div', cls || 'wrapped-art');
    if (url) {
      const img = document.createElement('img');
      img.src = url;
      img.alt = '';
      img.loading = 'lazy';
      img.onerror = () => { wrap.classList.add('wrapped-art-fallback'); };
      wrap.appendChild(img);
    } else {
      wrap.classList.add('wrapped-art-fallback');
    }
    return wrap;
  }

  function buildIntro(stats, thumbs) {
    const slide = makeSlide(0, thumbs, []);
    const content = slide.querySelector('.wrapped-content');
    content.innerHTML = `
      <img class="wrapped-intro-logo wrapped-anim-scale" style="animation-delay:0.05s" src="../../assets/snowify-logo-text.png" alt="Snowify" draggable="false" />
      <div class="wrapped-hero-year wrapped-anim-up" style="animation-delay:0.22s">${stats.year}</div>
      <div class="wrapped-hero-title wrapped-anim-up" style="animation-delay:0.34s">${escapeHtml(t('wrapped.wrapped', 'Wrapped'))}</div>
      <div class="wrapped-sub wrapped-anim-up" style="animation-delay:0.46s">${escapeHtml(t('wrapped.introSub', 'Your year in music'))}</div>
    `;
    return slide;
  }

  function buildTotals(stats, thumbs) {
    const slide = makeSlide(1, thumbs, []);
    const content = slide.querySelector('.wrapped-content');
    content.innerHTML = `
      <div class="wrapped-eyebrow wrapped-anim-up" style="animation-delay:0.05s">${escapeHtml(t('wrapped.thisYear', 'This Year'))}</div>
      <div class="wrapped-stat-number wrapped-anim-up" style="animation-delay:0.15s">${stats.totalPlays.toLocaleString()}</div>
      <div class="wrapped-stat-label wrapped-anim-up" style="animation-delay:0.26s">${escapeHtml(t('wrapped.songsPlayed', 'songs played'))}</div>
      <div class="wrapped-stat-grid wrapped-anim-up" style="animation-delay:0.38s">
        <div class="wrapped-stat-chip">
          <div class="wrapped-chip-value">${stats.totalHours.toLocaleString()}</div>
          <div class="wrapped-chip-label">${escapeHtml(t('wrapped.hours', 'hours'))}</div>
        </div>
        <div class="wrapped-stat-chip">
          <div class="wrapped-chip-value">${stats.uniqueSongs.toLocaleString()}</div>
          <div class="wrapped-chip-label">${escapeHtml(t('wrapped.uniqueSongs', 'unique songs'))}</div>
        </div>
        <div class="wrapped-stat-chip">
          <div class="wrapped-chip-value">${stats.uniqueArtists.toLocaleString()}</div>
          <div class="wrapped-chip-label">${escapeHtml(t('wrapped.uniqueArtists', 'artists'))}</div>
        </div>
      </div>
    `;
    return slide;
  }

  function buildTopArtist(stats, thumbs) {
    const artist = stats.topArtists[0];
    if (!artist) return buildTopArtists(stats, thumbs);

    // Use artist thumbnail if available, else fall back to first song thumb
    const artUrl = artist.thumbnail || thumbs[0] || null;
    const slide = makeSlide(2, thumbs, []);
    const content = slide.querySelector('.wrapped-content');

    const artEl = makeArt(artUrl, 'wrapped-hero-art wrapped-anim-scale');
    artEl.style.animationDelay = '0.1s';
    content.appendChild(artEl);

    const textWrap = el('div', 'wrapped-hero-text');
    textWrap.innerHTML = `
      <div class="wrapped-eyebrow wrapped-anim-up" style="animation-delay:0.22s">${escapeHtml(t('wrapped.yourTopArtist', 'Your #1 Artist'))}</div>
      <div class="wrapped-hero-artist wrapped-anim-up" style="animation-delay:0.32s">${escapeHtml(artist.name)}</div>
      <div class="wrapped-artist-plays wrapped-anim-up" style="animation-delay:0.44s">${artist.count.toLocaleString()} ${escapeHtml(t('wrapped.plays', 'plays'))}</div>
      <div class="wrapped-artist-time wrapped-anim-up" style="animation-delay:0.54s">${Math.round(artist.totalMs / 60000).toLocaleString()} ${escapeHtml(t('wrapped.minutes', 'minutes'))}</div>
    `;
    content.appendChild(textWrap);
    return slide;
  }

  function buildTopArtists(stats, thumbs) {
    const slide = makeSlide(3, thumbs, []);
    const content = slide.querySelector('.wrapped-content');

    const eyebrow = el('div', 'wrapped-eyebrow wrapped-anim-up');
    eyebrow.style.animationDelay = '0.05s';
    eyebrow.textContent = t('wrapped.topArtists', 'Top Artists');
    content.appendChild(eyebrow);

    const list = el('div', 'wrapped-list');
    stats.topArtists.forEach((a, i) => {
      const artUrl = a.thumbnail || thumbs[i] || null;
      const item = el('div', 'wrapped-list-item wrapped-anim-up');
      item.style.animationDelay = (0.15 + i * 0.1) + 's';
      item.innerHTML = `
        <span class="wrapped-list-rank">${i + 1}</span>
        <div class="wrapped-list-avatar">${artUrl ? `<img src="${escapeHtml(artUrl)}" alt="" loading="lazy" />` : '<div class="wrapped-list-avatar-fallback"></div>'}</div>
        <span class="wrapped-list-title">${escapeHtml(a.name)}</span>
        <span class="wrapped-list-meta">${a.count.toLocaleString()} ${escapeHtml(t('wrapped.plays', 'plays'))}</span>
      `;
      list.appendChild(item);
    });
    content.appendChild(list);
    return slide;
  }

  function buildTopSong(stats, thumbs) {
    const song = stats.topSongs[0];
    if (!song) return buildTopSongs(stats, thumbs);

    const artUrl = song.thumbnail || thumbs[0] || null;
    const slide = makeSlide(4, thumbs, []);
    const content = slide.querySelector('.wrapped-content');

    const artEl = makeArt(artUrl, 'wrapped-hero-art wrapped-anim-scale');
    artEl.style.animationDelay = '0.1s';
    content.appendChild(artEl);

    const textWrap = el('div', 'wrapped-hero-text');
    textWrap.innerHTML = `
      <div class="wrapped-eyebrow wrapped-anim-up" style="animation-delay:0.22s">${escapeHtml(t('wrapped.yourTopSong', 'Your #1 Song'))}</div>
      <div class="wrapped-hero-song wrapped-anim-up" style="animation-delay:0.32s">${escapeHtml(song.title)}</div>
      <div class="wrapped-hero-artist-small wrapped-anim-up" style="animation-delay:0.42s">${escapeHtml(song.artist)}</div>
      <div class="wrapped-song-plays wrapped-anim-up" style="animation-delay:0.54s">${escapeHtml(t('wrapped.playedXTimes', 'Played'))} ${song.count.toLocaleString()} ${escapeHtml(t('wrapped.times', 'times'))}</div>
    `;
    content.appendChild(textWrap);
    return slide;
  }

  function buildTopSongs(stats, thumbs) {
    const slide = makeSlide(5, thumbs, []);
    const content = slide.querySelector('.wrapped-content');

    const eyebrow = el('div', 'wrapped-eyebrow wrapped-anim-up');
    eyebrow.style.animationDelay = '0.05s';
    eyebrow.textContent = t('wrapped.topSongs', 'Top Songs');
    content.appendChild(eyebrow);

    const list = el('div', 'wrapped-list');
    stats.topSongs.forEach((s, i) => {
      const artUrl = s.thumbnail || thumbs[i] || null;
      const item = el('div', 'wrapped-list-item wrapped-anim-up');
      item.style.animationDelay = (0.15 + i * 0.1) + 's';
      item.innerHTML = `
        <span class="wrapped-list-rank">${i + 1}</span>
        <div class="wrapped-list-avatar">${artUrl ? `<img src="${escapeHtml(artUrl)}" alt="" loading="lazy" />` : '<div class="wrapped-list-avatar-fallback"></div>'}</div>
        <div class="wrapped-list-track">
          <span class="wrapped-list-title">${escapeHtml(s.title)}</span>
          <span class="wrapped-list-artist">${escapeHtml(s.artist)}</span>
        </div>
        <span class="wrapped-list-meta">${s.count.toLocaleString()}×</span>
      `;
      list.appendChild(item);
    });
    content.appendChild(list);
    return slide;
  }

  function buildPersonality(stats, thumbs) {
    const slide = makeSlide(6, thumbs, []);
    const content = slide.querySelector('.wrapped-content');
    content.innerHTML = `
      <div class="wrapped-eyebrow wrapped-anim-up" style="animation-delay:0.05s">${escapeHtml(t('wrapped.youAre', "You're a"))}</div>
      <div class="wrapped-personality-emoji wrapped-anim-pop" style="animation-delay:0.18s">${stats.personalityEmoji}</div>
      <div class="wrapped-hero-personality wrapped-anim-up" style="animation-delay:0.3s">${escapeHtml(t(stats.personalityKey, stats.personality))}</div>
      <div class="wrapped-personality-streak wrapped-anim-up" style="animation-delay:0.44s">
        ${escapeHtml(t('wrapped.streakPrefix', 'Your longest streak:'))} <strong>${stats.maxStreak}</strong> ${escapeHtml(t('wrapped.days', 'days'))}
      </div>
    `;
    return slide;
  }

  function buildBestMonth(stats, thumbs) {
    const slide = makeSlide(7, thumbs, []);
    const content = slide.querySelector('.wrapped-content');

    // Month bar chart
    // Compute monthly play counts to show context
    const playLog = (window.__snowifyState?.playLog) || [];
    const year = stats.year;
    const monthCounts = new Array(12).fill(0);
    playLog.filter(e => new Date(e.ts).getFullYear() === year)
           .forEach(e => monthCounts[new Date(e.ts).getMonth()]++);
    const maxCount = Math.max(...monthCounts, 1);

    const bars = monthCounts.map((c, i) => {
      const pct = Math.round((c / maxCount) * 100);
      const isActive = i === stats.bestMonth;
      return `<div class="wrapped-month-bar-wrap">
        <div class="wrapped-month-bar${isActive ? ' active' : ''}" style="height:${Math.max(4, pct)}%; animation-delay:${0.35 + i * 0.04}s"></div>
        <div class="wrapped-month-label${isActive ? ' active' : ''}">${MONTHS[i].slice(0,1)}</div>
      </div>`;
    }).join('');

    content.innerHTML = `
      <div class="wrapped-eyebrow wrapped-anim-up" style="animation-delay:0.05s">${escapeHtml(t('wrapped.bestMonth', 'Your Most Active Month'))}</div>
      <div class="wrapped-hero-month wrapped-anim-up" style="animation-delay:0.18s">${escapeHtml(MONTHS[stats.bestMonth])}</div>
      <div class="wrapped-month-plays wrapped-anim-up" style="animation-delay:0.3s">${stats.bestMonthPlays.toLocaleString()} ${escapeHtml(t('wrapped.plays', 'plays'))}</div>
      <div class="wrapped-month-chart wrapped-anim-up" style="animation-delay:0.38s">${bars}</div>
    `;
    return slide;
  }

  function buildOutro(stats, thumbs) {
    const slide = makeSlide(8, thumbs, []);
    const content = slide.querySelector('.wrapped-content');

    // Album art collage for the outro
    const collage = el('div', 'wrapped-outro-collage wrapped-anim-scale');
    collage.style.animationDelay = '0.1s';
    const collageThumbs = thumbs.slice(0, 4);
    while (collageThumbs.length < 4 && thumbs.length > 0) collageThumbs.push(thumbs[collageThumbs.length % thumbs.length]);
    collageThumbs.forEach(url => {
      const img = document.createElement('img');
      img.src = url || '';
      img.alt = '';
      img.loading = 'lazy';
      collage.appendChild(img);
    });
    if (collageThumbs.length === 0) collage.classList.add('hidden');
    content.appendChild(collage);

    const textWrap = el('div', 'wrapped-hero-text');
    textWrap.innerHTML = `
      <div class="wrapped-outro-note wrapped-anim-up" style="animation-delay:0.24s">${stats.year}</div>
      <div class="wrapped-hero-outro wrapped-anim-up" style="animation-delay:0.34s">${escapeHtml(t('wrapped.outroTitle', 'Thanks for\nlistening.'))}</div>
      <div class="wrapped-outro-sub wrapped-anim-up" style="animation-delay:0.46s">${escapeHtml(t('wrapped.outroSub', "Here's to"))} ${stats.year + 1} ♪</div>
    `;
    content.appendChild(textWrap);
    return slide;
  }

  // ─── Manager ─────────────────────────────────────────────────────────────

  let _currentSlide = 0;
  let _slides = [];
  let _slideEls = [];
  let _overlay = null;
  let _container = null;
  let _dots = null;
  let _isVisible = false;
  let _currentYear = null;  let _autoTimer = null;
  const AUTO_DELAY = 6000; // ms per slide
  function getOverlay() {
    if (!_overlay) _overlay = document.getElementById('wrapped-overlay');
    return _overlay;
  }

  function updateDots() {
    if (!_dots) return;
    // Rebuild segment bar
    _dots.innerHTML = _slides.map((_, i) => {
      const state = i < _currentSlide ? 'done' : i === _currentSlide ? 'active' : '';
      return `<div class="wrapped-progress-seg ${state}" data-idx="${i}" style="--seg-duration:${AUTO_DELAY}ms"><div class="wrapped-progress-seg-fill"></div></div>`;
    }).join('');
    _dots.querySelectorAll('.wrapped-progress-seg').forEach(seg => {
      seg.addEventListener('click', () => { _resetAutoTimer(); goToSlide(parseInt(seg.dataset.idx)); });
    });

    // Update slide counter
    const counter = document.getElementById('wrapped-slide-counter');
    if (counter) counter.textContent = `${_currentSlide + 1} / ${_slides.length}`;
  }

  function goToSlide(idx, direction) {
    if (!_slideEls.length) return;
    const prev = _currentSlide;
    _currentSlide = Math.max(0, Math.min(_slides.length - 1, idx));
    if (prev === _currentSlide) return;

    const dir = direction ?? (idx > prev ? 1 : -1);

    _slideEls[prev].classList.remove('active');
    _slideEls[prev].classList.add(dir > 0 ? 'exit-left' : 'exit-right');

    _slideEls[_currentSlide].classList.remove('exit-left', 'exit-right', 'enter-right', 'enter-left');
    _slideEls[_currentSlide].classList.add(dir > 0 ? 'enter-right' : 'enter-left');

    requestAnimationFrame(() => {
      _slideEls[_currentSlide].classList.add('active');
      _slideEls[_currentSlide].classList.remove('enter-right', 'enter-left');
    });

    setTimeout(() => {
      _slideEls[prev].classList.remove('exit-left', 'exit-right');
    }, 400);

    updateDots();
  }

  function show(year, devMode = false) {
    const overlay = getOverlay();
    if (!overlay) { console.warn('[Wrapped] overlay element not found'); return; }
    _currentYear = year;

    // Get playLog from state (or fallback to empty)
    const playLog = (window.__snowifyState?.playLog) || [];
    const stats = computeWrapped(playLog, year);

    if (!stats) {
      if (devMode) {
        // Show with dummy stats for dev preview (so developers can see the UI)
        showWithStats({
          year, totalPlays: 0, totalMinutes: 0, totalHours: 0,
          uniqueSongs: 0, uniqueArtists: 0, topSongs: [], topArtists: [],
          bestMonth: new Date().getMonth(), bestMonthPlays: 0,
          personality: 'Night Owl', personalityKey: 'wrapped.personalityNight',
          personalityEmoji: '🌙', maxStreak: 0,
        }, overlay, devMode);
      }
      return;
    }

    showWithStats(stats, overlay, devMode);
  }

  function showWithStats(stats, overlay, devMode) {
    _slides = buildSlides(stats);

    // Build container
    _container = document.getElementById('wrapped-slides-container');
    _dots = document.getElementById('wrapped-progress-dots');
    if (!_container) return;

    _container.innerHTML = '';
    _slideEls = _slides.map((slide, i) => {
      if (i === 0) slide.classList.add('active');
      _container.appendChild(slide);
      return slide;
    });
    _currentSlide = 0;
    updateDots();

    // Wire navigation
    document.getElementById('wrapped-prev')?.addEventListener('click', handlePrev);
    document.getElementById('wrapped-next')?.addEventListener('click', handleNext);
    document.getElementById('wrapped-close')?.addEventListener('click', hide);

    // Keyboard navigation
    const keyHandler = (e) => {
      if (!_isVisible) return;
      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') { _resetAutoTimer(); handleNext(); }
      if (e.key === 'ArrowLeft'  || e.key === 'ArrowUp')   { _resetAutoTimer(); handlePrev(); }
      if (e.key === 'Escape') hide();
    };
    document.addEventListener('keydown', keyHandler);
    overlay._wrappedKeyHandler = keyHandler;

    // Touch swipe
    let touchStartX = 0;
    const touchStart = (e) => { touchStartX = e.touches[0].clientX; };
    const touchEnd = (e) => {
      const dx = touchStartX - e.changedTouches[0].clientX;
      if (Math.abs(dx) > 50) { _resetAutoTimer(); dx > 0 ? handleNext() : handlePrev(); }
    };
    overlay.addEventListener('touchstart', touchStart, { passive: true });
    overlay.addEventListener('touchend', touchEnd, { passive: true });
    overlay._wrappedTouchStart = touchStart;
    overlay._wrappedTouchEnd = touchEnd;

    overlay.classList.remove('hidden');
    overlay.classList.add('visible');
    _isVisible = true;

    // Start auto-advance
    _startAutoTimer();
  }

  function _startAutoTimer() {
    _stopAutoTimer();
    _autoTimer = setInterval(() => {
      if (!_isVisible) { _stopAutoTimer(); return; }
      if (_currentSlide < _slides.length - 1) {
        goToSlide(_currentSlide + 1);
      } else {
        hide();
      }
    }, AUTO_DELAY);
  }

  function _stopAutoTimer() {
    if (_autoTimer) { clearInterval(_autoTimer); _autoTimer = null; }
  }

  function _resetAutoTimer() {
    _startAutoTimer();
  }

  function handleNext() {
    _resetAutoTimer();
    if (_currentSlide < _slides.length - 1) {
      goToSlide(_currentSlide + 1);
    } else {
      hide();
    }
  }

  function handlePrev() {
    _resetAutoTimer();
    if (_currentSlide > 0) goToSlide(_currentSlide - 1);
  }

  function hide() {
    const overlay = getOverlay();
    if (!overlay) return;
    overlay.classList.remove('visible');
    setTimeout(() => overlay.classList.add('hidden'), 350);
    _isVisible = false;
    _stopAutoTimer();

    // Mark as shown for this year and persist
    if (_currentYear && window.__snowifyState) {
      window.__snowifyState.wrappedShownYear = _currentYear;
      window.__snowifySaveState?.();
    }

    // Cleanup event listeners
    if (overlay._wrappedKeyHandler) {
      document.removeEventListener('keydown', overlay._wrappedKeyHandler);
      delete overlay._wrappedKeyHandler;
    }
    if (overlay._wrappedTouchStart) {
      overlay.removeEventListener('touchstart', overlay._wrappedTouchStart);
      delete overlay._wrappedTouchStart;
    }
    if (overlay._wrappedTouchEnd) {
      overlay.removeEventListener('touchend', overlay._wrappedTouchEnd);
      delete overlay._wrappedTouchEnd;
    }

    // Re-wire navigation buttons (remove old listeners cleanly via clone)
    ['wrapped-prev','wrapped-next','wrapped-close'].forEach(id => {
      const btn = document.getElementById(id);
      if (btn) { const clone = btn.cloneNode(true); btn.parentNode.replaceChild(clone, btn); }
    });
    _slides = [];
    _slideEls = [];
    _currentYear = null;
  }

  // Expose state reference so Wrapped can read playLog / write wrappedShownYear
  // This is set by app.js after state is loaded.
  window.WrappedManager = { show, hide };
})();
