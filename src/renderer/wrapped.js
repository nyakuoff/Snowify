// ─── Snowify Wrapped ───
// Computes yearly listening stats and renders an animated, full-screen slide experience.
// Exposed as window.WrappedManager = { show(year, devMode), hide() }.

(function () {
  'use strict';

  const MONTHS = [
    'January', 'February', 'March', 'April', 'May', 'June',
    'July', 'August', 'September', 'October', 'November', 'December',
  ];

  // Slide gradient backgrounds (hsl pairs: from → to)
  const SLIDE_GRADIENTS = [
    ['#0d0520', '#1a0a3d'],  // 0 intro        – deep violet
    ['#081828', '#0a2848'],  // 1 totals        – deep blue
    ['#0a2010', '#062818'],  // 2 top-artist    – dark forest
    ['#061828', '#0a2038'],  // 3 top-artists   – deep teal
    ['#281008', '#200808'],  // 4 top-song      – dark ember
    ['#200810', '#180818'],  // 5 top-songs     – dark rose
    ['#080818', '#100830'],  // 6 personality   – midnight
    ['#0a1600', '#102000'],  // 7 best-month    – dark earth
    ['#100020', '#1a0030'],  // 8 outro         – deep purple
  ];

  // ─── Statistics computation ───────────────────────────────────────────────

  function computeWrapped(playLog, year) {
    const entries = playLog.filter(e => new Date(e.ts).getFullYear() === year);
    if (entries.length < 5) return null; // not enough data

    // — Songs —
    const songMap = new Map();
    for (const e of entries) {
      if (!songMap.has(e.id)) songMap.set(e.id, { id: e.id, title: e.title, artist: e.artist, count: 0, totalMs: 0 });
      const s = songMap.get(e.id);
      s.count++;
      s.totalMs += e.durationMs || 0;
    }
    const topSongs = [...songMap.values()].sort((a, b) => b.count - a.count).slice(0, 5);

    // — Artists —
    const artistMap = new Map();
    for (const e of entries) {
      const key = (e.artist || 'Unknown Artist').toLowerCase();
      if (!artistMap.has(key)) artistMap.set(key, { name: e.artist || 'Unknown Artist', count: 0, totalMs: 0 });
      const a = artistMap.get(key);
      a.count++;
      a.totalMs += e.durationMs || 0;
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

  function buildSlides(stats) {
    return [
      buildIntro(stats),
      buildTotals(stats),
      buildTopArtist(stats),
      buildTopArtists(stats),
      buildTopSong(stats),
      buildTopSongs(stats),
      buildPersonality(stats),
      buildBestMonth(stats),
      buildOutro(stats),
    ];
  }

  function el(tag, cls, html) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html !== undefined) e.innerHTML = html;
    return e;
  }

  function makeSlide(gradientIdx, children) {
    const slide = el('div', 'wrapped-slide');
    const [from, to] = SLIDE_GRADIENTS[gradientIdx] || SLIDE_GRADIENTS[0];
    slide.style.background = `radial-gradient(ellipse at 30% 20%, ${to}dd, ${from}) no-repeat center/cover`;
    children.forEach((c, i) => {
      if (c) {
        c.style.animationDelay = (i * 0.12) + 's';
        slide.appendChild(c);
      }
    });
    return slide;
  }

  function buildIntro(stats) {
    return makeSlide(0, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', t('wrapped.your', 'Your')),
      el('div', 'wrapped-hero-year wrapped-anim-up', String(stats.year)),
      el('div', 'wrapped-hero-title wrapped-anim-up', t('wrapped.wrapped', 'Wrapped')),
      el('div', 'wrapped-sub wrapped-anim-up', t('wrapped.introSub', 'Your year in music')),
    ]);
  }

  function buildTotals(stats) {
    return makeSlide(1, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', t('wrapped.thisYear', 'This Year')),
      el('div', 'wrapped-stat-number wrapped-anim-up', stats.totalPlays.toLocaleString()),
      el('div', 'wrapped-stat-label wrapped-anim-up', t('wrapped.songsPlayed', 'songs played')),
      el('div', 'wrapped-spacer'),
      el('div', 'wrapped-stat-grid wrapped-anim-up', `
        <div class="wrapped-stat-chip">
          <div class="wrapped-chip-value">${stats.totalHours.toLocaleString()}</div>
          <div class="wrapped-chip-label">${t('wrapped.hours', 'hours')}</div>
        </div>
        <div class="wrapped-stat-chip">
          <div class="wrapped-chip-value">${stats.uniqueSongs.toLocaleString()}</div>
          <div class="wrapped-chip-label">${t('wrapped.uniqueSongs', 'unique songs')}</div>
        </div>
        <div class="wrapped-stat-chip">
          <div class="wrapped-chip-value">${stats.uniqueArtists.toLocaleString()}</div>
          <div class="wrapped-chip-label">${t('wrapped.uniqueArtists', 'artists')}</div>
        </div>
      `),
    ]);
  }

  function buildTopArtist(stats) {
    const artist = stats.topArtists[0];
    if (!artist) return buildTopArtists(stats); // fallback
    return makeSlide(2, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', t('wrapped.yourTopArtist', 'Your #1 Artist')),
      el('div', 'wrapped-hero-artist wrapped-anim-up', artist.name),
      el('div', 'wrapped-artist-plays wrapped-anim-up',
        `${artist.count.toLocaleString()} ${t('wrapped.plays', 'plays')}`),
      el('div', 'wrapped-artist-time wrapped-anim-up',
        `${Math.round(artist.totalMs / 60000).toLocaleString()} ${t('wrapped.minutes', 'minutes')}`),
    ]);
  }

  function buildTopArtists(stats) {
    const items = stats.topArtists.map((a, i) =>
      `<div class="wrapped-list-item wrapped-anim-up" style="animation-delay:${0.15 + i * 0.1}s">
        <span class="wrapped-list-rank">${i + 1}</span>
        <span class="wrapped-list-title">${escapeHtml(a.name)}</span>
        <span class="wrapped-list-meta">${a.count.toLocaleString()} ${t('wrapped.plays','plays')}</span>
      </div>`
    ).join('');
    const slide = makeSlide(3, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', t('wrapped.topArtists', 'Top Artists')),
    ]);
    const list = el('div', 'wrapped-list');
    list.innerHTML = items;
    slide.appendChild(list);
    return slide;
  }

  function buildTopSong(stats) {
    const song = stats.topSongs[0];
    if (!song) return buildTopSongs(stats);
    return makeSlide(4, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', t('wrapped.yourTopSong', 'Your #1 Song')),
      el('div', 'wrapped-hero-song wrapped-anim-up', escapeHtml(song.title)),
      el('div', 'wrapped-hero-artist-small wrapped-anim-up', escapeHtml(song.artist)),
      el('div', 'wrapped-song-plays wrapped-anim-up',
        `${t('wrapped.playedXTimes', 'Played')} ${song.count.toLocaleString()} ${t('wrapped.times', 'times')}`),
    ]);
  }

  function buildTopSongs(stats) {
    const items = stats.topSongs.map((s, i) =>
      `<div class="wrapped-list-item wrapped-anim-up" style="animation-delay:${0.15 + i * 0.1}s">
        <span class="wrapped-list-rank">${i + 1}</span>
        <div class="wrapped-list-track">
          <span class="wrapped-list-title">${escapeHtml(s.title)}</span>
          <span class="wrapped-list-artist">${escapeHtml(s.artist)}</span>
        </div>
        <span class="wrapped-list-meta">${s.count.toLocaleString()}×</span>
      </div>`
    ).join('');
    const slide = makeSlide(5, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', t('wrapped.topSongs', 'Top Songs')),
    ]);
    const list = el('div', 'wrapped-list');
    list.innerHTML = items;
    slide.appendChild(list);
    return slide;
  }

  function buildPersonality(stats) {
    return makeSlide(6, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', t('wrapped.youAre', "You're a")),
      el('div', 'wrapped-personality-emoji wrapped-anim-up', stats.personalityEmoji),
      el('div', 'wrapped-hero-personality wrapped-anim-up', t(stats.personalityKey, stats.personality)),
      el('div', 'wrapped-personality-streak wrapped-anim-up',
        `${t('wrapped.streakPrefix', 'Your longest streak:')} ${stats.maxStreak} ${t('wrapped.days', 'days')}`),
    ]);
  }

  function buildBestMonth(stats) {
    return makeSlide(7, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', t('wrapped.bestMonth', 'Your Most Active Month')),
      el('div', 'wrapped-hero-month wrapped-anim-up', MONTHS[stats.bestMonth]),
      el('div', 'wrapped-month-plays wrapped-anim-up',
        `${stats.bestMonthPlays.toLocaleString()} ${t('wrapped.plays', 'plays')}`),
    ]);
  }

  function buildOutro(stats) {
    return makeSlide(8, [
      el('div', 'wrapped-eyebrow wrapped-anim-up', String(stats.year)),
      el('div', 'wrapped-outro-note wrapped-anim-up', t('wrapped.outroNotes', 'notes played')),
      el('div', 'wrapped-hero-outro wrapped-anim-up', t('wrapped.outroTitle', 'Thanks for\nlistening.')),
      el('div', 'wrapped-outro-sub wrapped-anim-up', `${t('wrapped.outroSub', "Here's to")} ${stats.year + 1} ♪`),
    ]);
  }

  function escapeHtml(str) {
    return String(str || '')
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
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
    _dots.innerHTML = _slides.map((_, i) =>
      `<span class="wrapped-dot${i === _currentSlide ? ' active' : ''}" data-idx="${i}"></span>`
    ).join('');
    _dots.querySelectorAll('.wrapped-dot').forEach(dot => {
      dot.addEventListener('click', () => { _resetAutoTimer(); goToSlide(parseInt(dot.dataset.idx)); });
    });

    // Restart progress animation on the active dot
    const activeDot = _dots.querySelector('.wrapped-dot.active');
    if (activeDot) {
      activeDot.style.animation = 'none';
      // Force reflow so the browser picks up the reset before re-applying
      void activeDot.offsetWidth;
      activeDot.style.animation = `wrappedDotProgress ${AUTO_DELAY}ms linear forwards`;
    }
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
