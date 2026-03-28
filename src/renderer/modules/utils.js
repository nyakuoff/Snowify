// ─── Renderer utilities ───

const MOBILE_PROXY_PREFIX = 'http://127.0.0.1:17890/stream?url=';

export function deproxyUrl(url) {
  if (typeof url !== 'string' || !url.startsWith(MOBILE_PROXY_PREFIX)) return url;
  try {
    const parsed = new URL(url);
    return parsed.searchParams.get('url') || url;
  } catch {
    return url;
  }
}

export function resolveImageUrl(url) {
  if (!url) return url;
  return window.snowify?.resolveImageUrl?.(url) || deproxyUrl(url);
}

export function setupSliderTooltip(sliderEl, formatValue) {
  const tip = document.createElement('div');
  tip.className = 'slider-tooltip';
  sliderEl.style.position = 'relative';
  sliderEl.appendChild(tip);
  sliderEl.addEventListener('mouseenter', () => tip.classList.add('visible'));
  sliderEl.addEventListener('mouseleave', () => tip.classList.remove('visible'));
  sliderEl.addEventListener('mousemove', (e) => {
    const rect = sliderEl.getBoundingClientRect();
    const pct = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    tip.textContent = formatValue(pct);
    tip.style.left = (pct * rect.width) + 'px';
  });
}

export function showInputModal(title, defaultValue = '') {
  return new Promise((resolve) => {
    const overlay = document.querySelector('#input-modal');
    const input = document.querySelector('#input-modal-input');
    const titleEl = document.querySelector('#input-modal-title');

    titleEl.textContent = title;
    input.value = defaultValue;
    overlay.classList.remove('hidden');
    setTimeout(() => { input.focus(); input.select(); }, 50);

    function cleanup(result) {
      overlay.classList.add('hidden');
      input.removeEventListener('keydown', onKey);
      document.querySelector('#input-modal-ok').removeEventListener('click', onOk);
      document.querySelector('#input-modal-cancel').removeEventListener('click', onCancel);
      overlay.removeEventListener('click', onOverlay);
      resolve(result);
    }

    function onOk() { cleanup(input.value.trim() || null); }
    function onCancel() { cleanup(null); }
    function onKey(e) {
      if (e.key === 'Enter') onOk();
      if (e.key === 'Escape') onCancel();
    }
    function onOverlay(e) { if (e.target === overlay) onCancel(); }

    input.addEventListener('keydown', onKey);
    document.querySelector('#input-modal-ok').addEventListener('click', onOk);
    document.querySelector('#input-modal-cancel').addEventListener('click', onCancel);
    overlay.addEventListener('click', onOverlay);
  });
}

let _toastTimeout = null;
export function showToast(message, action) {
  const toast = document.querySelector('#toast');
  toast.textContent = message;
  if (action) {
    const link = document.createElement('span');
    link.className = 'toast-action';
    link.textContent = action.label;
    link.addEventListener('click', action.onClick);
    toast.append(' ', link);
  }
  toast.classList.remove('hidden');
  clearTimeout(_toastTimeout);
  requestAnimationFrame(() => toast.classList.add('show'));
  _toastTimeout = setTimeout(() => {
    toast.classList.remove('show');
    setTimeout(() => toast.classList.add('hidden'), 300);
  }, action ? 5000 : 2500);
}

export function escapeHtml(str) {
  if (!str) return '';
  return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

export function pathToFileUrl(p) {
  const normalized = p.replace(/\\/g, '/');
  return normalized.startsWith('/') ? 'file://' + normalized : 'file:///' + normalized;
}

/** Wrap a scroll element with left/right arrow buttons if not already wrapped. */
export function addScrollArrows(scrollEl) {
  if (!scrollEl || scrollEl.parentElement?.classList.contains('scroll-container')) return;
  const wrapper = document.createElement('div');
  wrapper.className = 'scroll-container';
  scrollEl.parentNode.insertBefore(wrapper, scrollEl);
  const leftBtn = document.createElement('button');
  leftBtn.className = 'scroll-arrow scroll-arrow-left';
  leftBtn.dataset.dir = 'left';
  leftBtn.innerHTML = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>';
  const rightBtn = document.createElement('button');
  rightBtn.className = 'scroll-arrow scroll-arrow-right';
  rightBtn.dataset.dir = 'right';
  rightBtn.innerHTML = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>';
  wrapper.appendChild(leftBtn);
  wrapper.appendChild(scrollEl);
  wrapper.appendChild(rightBtn);
  leftBtn.addEventListener('click', () => scrollEl.scrollBy({ left: -400, behavior: 'smooth' }));
  rightBtn.addEventListener('click', () => scrollEl.scrollBy({ left: 400, behavior: 'smooth' }));
}

/** Render clickable artist links as HTML string from a track object. */
export function renderArtistLinks(track) {
  if (track.artists?.length) {
    return track.artists.map((a, i, arr) => {
      const sep = i < arr.length - 1 ? ', ' : '';
      return (a.id
        ? `<span class="artist-link" data-artist-id="${escapeHtml(a.id)}">${escapeHtml(a.name)}</span>`
        : escapeHtml(a.name)) + sep;
    }).join('');
  }
  if (track.artistId) {
    return `<span class="artist-link" data-artist-id="${escapeHtml(track.artistId)}">${escapeHtml(track.artist)}</span>`;
  }
  return escapeHtml(track.artist || I18n.t('common.unknownArtist'));
}

export function formatTime(s) {
  if (!s || isNaN(s)) return '0:00';
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${sec.toString().padStart(2, '0')}`;
}

export function formatFollowers(n) {
  if (n >= 1_000_000_000) return (n / 1_000_000_000).toFixed(1).replace(/\.0$/, '') + 'B';
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1).replace(/\.0$/, '') + 'M';
  if (n >= 1_000) return (n / 1_000).toFixed(1).replace(/\.0$/, '') + 'K';
  return n.toString();
}
