/**
 * settings.js
 * Settings panel, changelog, updater UI, developer tools, source lists.
 *
 * Cloud-sync-related functions (forceCloudSave, cloudLoadAndMerge, etc.) and
 * welcome-overlay helpers are injected at app start via `settingsCallbacks`.
 */

import state from './state.js';
import { escapeHtml, showToast, setupSliderTooltip } from './utils.js';
import { callbacks } from './callbacks.js';
import { audioRef } from './audio-ref.js';
import { getNormalizer, getPrefetchCache } from './player.js';
import { applyTheme, populateCustomThemes, isCustomTheme, customThemeId, applyCustomThemeCss, removeCustomThemeCss } from './theme.js';
import { renderPlugins } from './plugins.js';
import { renderHome } from './home.js';
import { invalidateExploreCache } from './explore.js';

const $ = (sel, ctx = document) => ctx.querySelector(sel);
const IS_MOBILE_RUNTIME = window.snowify?.platform === 'android' || window.snowify?.platform === 'ios' ||
  document.documentElement.classList.contains('platform-mobile');
const RESET_COOLDOWN_MS = 60_000;

// ─── settingsCallbacks ────────────────────────────────────────────────────────
// Populated in app.js finishInit().

export const settingsCallbacks = {
  forceCloudSave:      async () => {},
  cloudLoadAndMerge:   async (_opts) => {},
  updateSyncStatus:    (_msg) => {},
  getCloudUser:        () => null,
  getCloudSyncPaused:  () => false,
  setCloudSyncPaused:  (_v) => {},
  clearCloudSaveTimeout: () => {},
  showWelcomeOverlay:  () => {},
  hideWelcomeOverlay:  (_opts) => {},
  setWelcomeError:     (_msg, _isAccent) => {},
  updateAccountUI:     (_user) => {},
};

// ─── Helpers (only used in settings) ─────────────────────────────────────────

function generateDefaultAvatar(name) {
  const letter = String(name || 'U').charAt(0).toUpperCase();
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = 128;
  const ctx = canvas.getContext('2d');
  const accent = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#aa55e6';
  ctx.fillStyle = accent;
  ctx.beginPath();
  ctx.arc(64, 64, 64, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = '#ffffff';
  ctx.font = 'bold 56px Inter, sans-serif';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(letter, 64, 67);
  return canvas.toDataURL();
}

function openCropModal({ dataUrl, title, circle, aspectRatio, outputWidth, outputHeight, quality = 0.85 }) {
  return new Promise((resolve) => {
    const modal      = $('#image-crop-modal');
    const titleEl    = $('#crop-modal-title');
    const sourceImg  = $('#crop-source-img');
    const selection  = $('#crop-selection');
    const container  = $('#crop-container');
    const applyBtn   = $('#crop-modal-apply');
    const cancelBtn  = $('#crop-modal-cancel');
    const cancelX    = $('#crop-modal-cancel-x');
    const backdrop   = modal.querySelector('.crop-modal-backdrop');

    titleEl.textContent = title || 'Crop Image';
    if (circle) selection.classList.add('circle');
    else selection.classList.remove('circle');

    let imgW = 0, imgH = 0;
    let sx = 0, sy = 0, sw = 0, sh = 0;
    let dragging = null;
    let dragStart = { x: 0, y: 0, sx: 0, sy: 0, sw: 0, sh: 0 };
    const MIN_SIZE = 32;

    const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));

    function updateSelectionEl() {
      selection.style.left   = sx + 'px';
      selection.style.top    = sy + 'px';
      selection.style.width  = sw + 'px';
      selection.style.height = sh + 'px';
    }

    function initSelection() {
      imgW = sourceImg.clientWidth;
      imgH = sourceImg.clientHeight;
      let w, h;
      if (imgW / imgH > aspectRatio) {
        h = imgH * 0.85; w = h * aspectRatio;
      } else {
        w = imgW * 0.85; h = w / aspectRatio;
      }
      sw = Math.round(w); sh = Math.round(h);
      sx = Math.round((imgW - sw) / 2); sy = Math.round((imgH - sh) / 2);
      updateSelectionEl();
    }

    sourceImg.onload = () => { modal.classList.remove('hidden'); requestAnimationFrame(initSelection); };
    sourceImg.src = dataUrl;

    function onPointerDown(e) {
      if (e.button !== 0) return;
      if (e.target.classList.contains('crop-handle')) {
        dragging = e.target.dataset.handle;
      } else if (e.target === selection || selection.contains(e.target)) {
        dragging = 'move';
      } else { return; }
      dragStart = { x: e.clientX, y: e.clientY, sx, sy, sw, sh };
      e.preventDefault();
    }

    function onPointerMove(e) {
      if (!dragging) return;
      const dx = e.clientX - dragStart.x;
      const dy = e.clientY - dragStart.y;
      if (dragging === 'move') {
        sx = clamp(dragStart.sx + dx, 0, imgW - sw);
        sy = clamp(dragStart.sy + dy, 0, imgH - sh);
      } else {
        let newW = dragStart.sw, newH = dragStart.sh, newX = dragStart.sx, newY = dragStart.sy;
        if (dragging === 'se') {
          newW = clamp(dragStart.sw + dx, MIN_SIZE, imgW - dragStart.sx); newH = newW / aspectRatio;
          if (newY + newH > imgH) { newH = imgH - newY; newW = newH * aspectRatio; }
        } else if (dragging === 'sw') {
          newW = clamp(dragStart.sw - dx, MIN_SIZE, dragStart.sx + dragStart.sw); newH = newW / aspectRatio;
          newX = dragStart.sx + dragStart.sw - newW;
          if (newY + newH > imgH) { newH = imgH - newY; newW = newH * aspectRatio; newX = dragStart.sx + dragStart.sw - newW; }
        } else if (dragging === 'ne') {
          newW = clamp(dragStart.sw + dx, MIN_SIZE, imgW - dragStart.sx); newH = newW / aspectRatio;
          newY = dragStart.sy + dragStart.sh - newH;
          if (newY < 0) { newY = 0; newH = dragStart.sy + dragStart.sh; newW = newH * aspectRatio; }
        } else if (dragging === 'nw') {
          newW = clamp(dragStart.sw - dx, MIN_SIZE, dragStart.sx + dragStart.sw); newH = newW / aspectRatio;
          newX = dragStart.sx + dragStart.sw - newW; newY = dragStart.sy + dragStart.sh - newH;
          if (newY < 0) { newY = 0; newH = dragStart.sy + dragStart.sh; newW = newH * aspectRatio; newX = dragStart.sx + dragStart.sw - newW; }
        }
        sx = Math.max(0, Math.round(newX)); sy = Math.max(0, Math.round(newY));
        sw = Math.round(newW); sh = Math.round(newH);
      }
      updateSelectionEl();
    }

    function onPointerUp() { dragging = null; }

    function cleanupCrop() {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('pointermove', onPointerMove);
      document.removeEventListener('pointerup', onPointerUp);
      modal.classList.add('hidden');
      sourceImg.src = '';
    }

    function cancel() { cleanupCrop(); resolve(null); }

    function apply() {
      const natW = sourceImg.naturalWidth, natH = sourceImg.naturalHeight;
      const scaleX = natW / imgW, scaleY = natH / imgH;
      const canvas = document.createElement('canvas');
      canvas.width = outputWidth; canvas.height = outputHeight;
      const ctx = canvas.getContext('2d');
      ctx.drawImage(sourceImg, Math.round(sx * scaleX), Math.round(sy * scaleY), Math.round(sw * scaleX), Math.round(sh * scaleY), 0, 0, outputWidth, outputHeight);
      cleanupCrop();
      resolve(canvas.toDataURL('image/jpeg', quality));
    }

    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('pointermove', onPointerMove);
    document.addEventListener('pointerup', onPointerUp);

    applyBtn.onclick = apply;
    cancelBtn.onclick = cancel;
    cancelX.onclick = cancel;
    backdrop.onclick = cancel;
  });
}

// ─── Markdown → HTML (for changelog) ─────────────────────────────────────────

function renderMarkdown(md) {
  const tokens = [];
  let tIdx = 0;
  const stash = (html) => { const k = `\x00T${tIdx++}\x00`; tokens.push({ k, html }); return k; };

  md = md.replace(/`([^`]+)`/g, (_, c) => stash(`<code>${escapeHtml(c)}</code>`));
  md = md.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_, alt, url) => stash(`<img src="${escapeHtml(url)}" alt="${escapeHtml(alt)}" />`));
  md = md.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, text, url) => stash(`<a href="${escapeHtml(url)}" target="_blank" rel="noopener">${escapeHtml(text)}</a>`));
  md = md.replace(/(^|[\s(])((https?:\/\/)[^\s)<]+)/gm, (_, pre, url) => pre + stash(`<a href="${escapeHtml(url)}" target="_blank" rel="noopener">${escapeHtml(url)}</a>`));
  md = md.replace(/(^|[\s(])@([a-zA-Z0-9_-]+)/gm, (_, pre, user) => pre + stash(`<a href="https://github.com/${escapeHtml(user)}" target="_blank" rel="noopener">@${escapeHtml(user)}</a>`));

  let html = escapeHtml(md);
  for (const { k, html: v } of tokens) html = html.split(k).join(v);

  html = html.replace(/^### (.+)$/gm, '<h3>$1</h3>').replace(/^## (.+)$/gm, '<h2>$1</h2>').replace(/^# (.+)$/gm, '<h1>$1</h1>');
  html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/\*(.+?)\*/g, '<em>$1</em>');
  html = html.replace(/^---$/gm, '<hr>');
  html = html.replace(/^[*\-] (.+)$/gm, '<li>$1</li>').replace(/((?:<li>.+<\/li>\n?)+)/g, '<ul>$1</ul>');
  html = html.replace(/^&gt; (.+)$/gm, '<blockquote>$1</blockquote>');
  html = html.replace(/\n{2,}/g, '</p><p>');
  html = '<p>' + html + '</p>';
  html = html.replace(/<p>\s*<\/p>/g, '').replace(/<p>\s*(<h[123]>)/g, '$1').replace(/(<\/h[123]>)\s*<\/p>/g, '$1')
    .replace(/<p>\s*(<ul>)/g, '$1').replace(/(<\/ul>)\s*<\/p>/g, '$1')
    .replace(/<p>\s*(<hr>)\s*<\/p>/g, '$1').replace(/<p>\s*(<blockquote>)/g, '$1').replace(/(<\/blockquote>)\s*<\/p>/g, '$1');
  return html;
}

function compareSemver(a, b) {
  const pa = a.split('.').map(Number), pb = b.split('.').map(Number);
  for (let i = 0; i < 3; i++) {
    const va = pa[i] || 0, vb = pb[i] || 0;
    if (va < vb) return -1; if (va > vb) return 1;
  }
  return 0;
}

// ─── initSettings ─────────────────────────────────────────────────────────────

let _settingsInitialized = false;
let _resetEmailLastSent  = 0;

export async function initSettings() {
  if (_settingsInitialized) return;
  _settingsInitialized = true;

  const { engine, audio } = audioRef;
  const normalizer        = getNormalizer();
  const prefetchCache     = getPrefetchCache();

  // ── Tabs ──
  const settingsTabs = document.querySelector('.settings-tabs');
  if (settingsTabs) {
    const activateSettingsTab = (tabId) => {
      document.querySelectorAll('.settings-tab-btn').forEach(b =>
        b.classList.toggle('active', b.dataset.settingsTab === tabId));
      document.querySelectorAll('.settings-tab-pane').forEach(p =>
        p.classList.toggle('active', p.dataset.tab === tabId));
      sessionStorage.setItem('settings-tab', tabId);
      if (tabId === 'marketplace') renderPlugins();
    };
    settingsTabs.addEventListener('click', e => {
      const btn = e.target.closest('.settings-tab-btn');
      if (btn) activateSettingsTab(btn.dataset.settingsTab);
    });
    const preferred = sessionStorage.getItem('settings-tab') || 'account';
    const tabExists = document.querySelector(`.settings-tab-btn[data-settings-tab="${preferred}"]`);
    activateSettingsTab(tabExists ? preferred : 'account');

    if (IS_MOBILE_RUNTIME) {
      document.querySelector('.settings-tab-btn[data-settings-tab="behavior"]')?.classList.add('hidden');
    }
  }

  const autoplayToggle       = $('#setting-autoplay');
  const qualitySelect        = $('#setting-quality');
  const videoQualitySelect   = $('#setting-video-quality');
  const videoPremuxedToggle  = $('#setting-video-premuxed');
  const animationsToggle     = $('#setting-animations');
  const effectsToggle        = $('#setting-effects');
  const miniplayerGlowToggle = $('#setting-miniplayer-glow');
  const miniplayerGlowRow    = $('#row-miniplayer-glow');
  const discordRpcToggle     = $('#setting-discord-rpc');
  const countrySelect        = $('#setting-country');
  const crossfadeToggle      = $('#setting-crossfade-toggle');
  const crossfadeSlider      = $('#crossfade-slider');
  const crossfadeFill        = $('#crossfade-fill');
  const crossfadeSliderRow   = $('#crossfade-slider-row');
  const crossfadeValueLabel  = $('#crossfade-value');
  let _cfDragging = false;
  let _cfValue    = state.crossfade > 0 ? state.crossfade : 5;

  autoplayToggle.checked   = state.autoplay;
  discordRpcToggle.checked = state.discordRpc;
  qualitySelect.value      = state.audioQuality;

  crossfadeToggle.checked = state.crossfade > 0;
  crossfadeSliderRow.classList.toggle('hidden', state.crossfade <= 0);
  $('.crossfade-label-max').textContent = engine.CROSSFADE_MAX + 's';
  updateCrossfadeSlider(_cfValue);
  videoQualitySelect.value       = state.videoQuality;
  videoPremuxedToggle.checked    = state.videoPremuxed;
  videoQualitySelect.disabled    = state.videoPremuxed;
  animationsToggle.checked       = state.animations;
  effectsToggle.checked          = state.effects;
  if (miniplayerGlowToggle) miniplayerGlowToggle.checked = state.miniplayerGlow;
  if (miniplayerGlowRow) miniplayerGlowRow.classList.toggle('hidden', !state.effects);
  if (countrySelect) countrySelect.value = state.country || '';
  if (state.country) window.snowify.setCountry(state.country);
  document.documentElement.classList.toggle('no-animations', !state.animations);
  document.documentElement.classList.toggle('no-effects', !state.effects);
  document.documentElement.classList.toggle('no-miniplayer-glow', !state.miniplayerGlow);

  // ── Minimize to tray ──
  const minimizeToTrayToggle = $('#setting-minimize-to-tray');
  if (minimizeToTrayToggle) {
    minimizeToTrayToggle.checked = state.minimizeToTray;
    minimizeToTrayToggle.addEventListener('change', () => {
      state.minimizeToTray = minimizeToTrayToggle.checked;
      window.snowify.setMinimizeToTray(state.minimizeToTray);
      callbacks.saveState();
    });
  }

  // ── Launch on startup ──
  const launchOnStartupToggle = $('#setting-launch-on-startup');
  if (launchOnStartupToggle) {
    launchOnStartupToggle.checked = state.launchOnStartup;
    launchOnStartupToggle.addEventListener('change', () => {
      state.launchOnStartup = launchOnStartupToggle.checked;
      window.snowify.setOpenAtLogin(state.launchOnStartup);
      callbacks.saveState();
    });
  }

  applyTheme(state.theme);

  // ── Language ──
  const languageSelect = $('#setting-language');
  if (languageSelect) {
    const savedLocale = localStorage.getItem('snowify_locale') || '';
    languageSelect.value = savedLocale;
    languageSelect.addEventListener('change', () => {
      const lang = languageSelect.value;
      if (lang) I18n.changeLanguage(lang);
      else { localStorage.removeItem('snowify_locale'); I18n.changeLanguage(navigator.language || 'en'); }
    });
  }

  // ── Theme ──
  const themeSelect = $('#theme-select');
  await populateCustomThemes(themeSelect, state.theme);
  themeSelect.addEventListener('change', () => {
    state.theme = themeSelect.value; applyTheme(state.theme); callbacks.saveState();
  });

  const btnAddTheme    = $('#btn-add-theme');
  const btnReloadTheme = $('#btn-reload-theme');
  const btnOpenThemes  = $('#btn-open-themes');
  const btnRemoveTheme = $('#btn-remove-theme');

  if (btnAddTheme) {
    btnAddTheme.onclick = async () => {
      const added = await window.snowify.addTheme();
      if (added && added.length) {
        await populateCustomThemes(themeSelect, state.theme);
        themeSelect.value = 'custom:' + added[0].id;
        state.theme = themeSelect.value;
        applyTheme(state.theme); callbacks.saveState();
        showToast(I18n.tp('toast.themeAdded', added.length));
      }
    };
  }
  if (btnReloadTheme) {
    btnReloadTheme.onclick = async () => {
      if (isCustomTheme(state.theme)) {
        const css = await window.snowify.reloadTheme(customThemeId(state.theme));
        if (css) { applyCustomThemeCss(css); showToast(I18n.t('toast.themeReloaded')); }
        else showToast(I18n.t('toast.themeNotFound'));
      } else {
        await populateCustomThemes(themeSelect, state.theme);
        showToast(I18n.t('toast.themeListRefreshed'));
      }
    };
  }
  if (btnOpenThemes) { btnOpenThemes.onclick = async () => { await window.snowify.openThemesFolder(); }; }
  if (btnRemoveTheme) {
    btnRemoveTheme.onclick = async () => {
      if (!isCustomTheme(state.theme)) { showToast(I18n.t('toast.selectCustomTheme')); return; }
      const id = customThemeId(state.theme);
      if (!confirm(I18n.t('settings.confirmRemoveTheme', { id }))) return;
      await window.snowify.removeTheme(id);
      removeCustomThemeCss();
      state.theme = 'dark'; themeSelect.value = 'dark';
      applyTheme(state.theme); callbacks.saveState();
      await populateCustomThemes(themeSelect, state.theme);
      showToast(I18n.t('toast.themeRemoved'));
    };
  }

  autoplayToggle.addEventListener('change', () => { state.autoplay = autoplayToggle.checked; callbacks.saveState(); });

  discordRpcToggle.addEventListener('change', async () => {
    state.discordRpc = discordRpcToggle.checked; callbacks.saveState();
    if (state.discordRpc) {
      const ok = await window.snowify.connectDiscord();
      if (!ok) {
        showToast(I18n.t('toast.discordError'));
        state.discordRpc = false; discordRpcToggle.checked = false; callbacks.saveState(); return;
      }
      const track = state.queue[state.queueIndex];
      if (track && state.isPlaying) window.__updateDiscordPresence?.(track);
    } else {
      window.__clearDiscordPresence?.();
      window.snowify.disconnectDiscord();
    }
  });

  qualitySelect.addEventListener('change', () => {
    state.audioQuality = qualitySelect.value;
    normalizer.clearCache(); prefetchCache.clear(); callbacks.saveState();
  });

  function updateCrossfadeSlider(val) {
    _cfValue = Math.max(1, Math.min(engine.CROSSFADE_MAX, val));
    const pct = ((_cfValue - 1) / (engine.CROSSFADE_MAX - 1)) * 100;
    crossfadeFill.style.width    = pct + '%';
    crossfadeValueLabel.textContent = I18n.t('settings.seconds', { value: _cfValue });
  }

  function setCrossfadeFromPointer(e) {
    const rect = crossfadeSlider.getBoundingClientRect();
    const pct  = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    updateCrossfadeSlider(Math.round(1 + pct * (engine.CROSSFADE_MAX - 1)));
    state.crossfade = _cfValue; callbacks.saveState();
  }

  crossfadeSlider.addEventListener('mousedown', (e) => { _cfDragging = true; setCrossfadeFromPointer(e); });
  document.addEventListener('mousemove', (e) => { if (_cfDragging) setCrossfadeFromPointer(e); });
  document.addEventListener('mouseup', () => { _cfDragging = false; });

  setupSliderTooltip(crossfadeSlider, (pct) => {
    const val = Math.round(1 + pct * (engine.CROSSFADE_MAX - 1));
    return I18n.t('settings.seconds', { value: val });
  });

  crossfadeToggle.addEventListener('change', () => {
    if (crossfadeToggle.checked) { state.crossfade = _cfValue; crossfadeSliderRow.classList.remove('hidden'); }
    else { state.crossfade = 0; crossfadeSliderRow.classList.add('hidden'); }
    callbacks.saveState();
  });

  // ── Normalization ──
  const normToggle     = $('#setting-normalization');
  const normTargetRow  = $('#normalization-target-row');
  const normTargetSel  = $('#setting-normalization-target');
  const normRow        = normToggle?.closest('.setting-row');

  if (IS_MOBILE_RUNTIME) {
    state.normalization = false; normToggle.checked = false; normToggle.disabled = true;
    normTargetRow.classList.add('hidden'); normRow?.classList.add('hidden'); callbacks.saveState();
  }

  normToggle.checked = state.normalization;
  normTargetRow.classList.toggle('hidden', !state.normalization);
  normTargetSel.value = String(state.normalizationTarget);

  normToggle.addEventListener('change', async () => {
    if (IS_MOBILE_RUNTIME) return;
    state.normalization = normToggle.checked;
    normalizer.setEnabled(state.normalization);
    normTargetRow.classList.toggle('hidden', !state.normalization);
    if (state.normalization) {
      await normalizer.initAudioContext();
      if (!normalizer.isWorkletReady()) showToast(I18n.t('toast.normalizationFailed'));
      normalizer.setTarget(state.normalizationTarget);
      const track = state.queue[state.queueIndex];
      if (track && state.isPlaying && audio.src) normalizer.analyzeAndApply(audio, audio.src, track.id);
    }
    callbacks.saveState();
  });

  normTargetSel.addEventListener('change', () => {
    if (IS_MOBILE_RUNTIME) return;
    state.normalizationTarget = parseInt(normTargetSel.value, 10);
    normalizer.setTarget(state.normalizationTarget);
    const track = state.queue[state.queueIndex];
    if (track && state.normalization) normalizer.applyGain(audio, track.id);
    callbacks.saveState();
  });

  // ── Prefetch ──
  const prefetchSelect = $('#setting-prefetch-count');
  prefetchSelect.value = String(state.prefetchCount);
  prefetchSelect.addEventListener('change', () => {
    const val = parseInt(prefetchSelect.value, 10);
    state.prefetchCount = val;
    if (val === 0) prefetchCache.clear();
    else {
      prefetchCache.setCount(val);
      if (state.queue.length && state.queueIndex >= 0) prefetchCache.onTrackChanged(state.queueIndex, state.queue);
    }
    callbacks.saveState();
  });

  videoQualitySelect.addEventListener('change', () => { state.videoQuality = videoQualitySelect.value; callbacks.saveState(); });
  videoPremuxedToggle.addEventListener('change', () => {
    state.videoPremuxed = videoPremuxedToggle.checked; videoQualitySelect.disabled = state.videoPremuxed; callbacks.saveState();
  });
  animationsToggle.addEventListener('change', () => {
    state.animations = animationsToggle.checked;
    document.documentElement.classList.toggle('no-animations', !state.animations); callbacks.saveState();
  });
  effectsToggle.addEventListener('change', () => {
    state.effects = effectsToggle.checked;
    document.documentElement.classList.toggle('no-effects', !state.effects);
    if (miniplayerGlowRow) miniplayerGlowRow.classList.toggle('hidden', !state.effects);
    callbacks.saveState();
  });

  if (miniplayerGlowToggle) {
    miniplayerGlowToggle.addEventListener('change', () => {
      state.miniplayerGlow = miniplayerGlowToggle.checked;
      document.documentElement.classList.toggle('no-miniplayer-glow', !state.miniplayerGlow);
      callbacks.saveState();
    });
  }

  if (countrySelect) {
    countrySelect.addEventListener('change', () => {
      state.country = countrySelect.value;
      window.snowify.setCountry(state.country);
      invalidateExploreCache();
      callbacks.saveState();
      showToast(state.country
        ? I18n.t('toast.exploreRegionSet', { region: countrySelect.options[countrySelect.selectedIndex].text })
        : I18n.t('toast.exploreRegionCleared'));
    });
  }

  // ── Auth ──
  const btnSignIn         = $('#btn-sign-in');
  const btnSignUp         = $('#btn-sign-up');
  const btnSignOut        = $('#btn-sign-out');
  const btnForgot         = $('#btn-forgot-settings');
  const btnSyncNow        = $('#btn-sync-now');
  const btnWelcomeSignIn  = $('#btn-welcome-sign-in');
  const btnWelcomeSignUp  = $('#btn-welcome-sign-up');
  const btnWelcomeForgot  = $('#btn-welcome-forgot');
  const btnWelcomeSkip    = $('#btn-welcome-skip');

  btnWelcomeSignIn?.addEventListener('click', async () => {
    const email = $('#welcome-auth-email')?.value.trim();
    const password = $('#welcome-auth-password')?.value;
    if (!email || !password) { settingsCallbacks.setWelcomeError(I18n.t('welcome.enterEmailPassword')); return; }
    const result = await window.snowify.signInWithEmail(email, password);
    if (result?.error) settingsCallbacks.setWelcomeError(result.error);
    else { settingsCallbacks.hideWelcomeOverlay({ remember: true }); showToast(I18n.t('toast.signedIn')); }
  });

  btnWelcomeSignUp?.addEventListener('click', async () => {
    const email = $('#welcome-auth-email')?.value.trim();
    const password = $('#welcome-auth-password')?.value;
    if (!email || !password) { settingsCallbacks.setWelcomeError(I18n.t('welcome.enterEmailPassword')); return; }
    if (password.length < 6) { settingsCallbacks.setWelcomeError(I18n.t('welcome.passwordMinLength')); return; }
    const result = await window.snowify.signUpWithEmail(email, password);
    if (result?.error) settingsCallbacks.setWelcomeError(result.error);
    else { settingsCallbacks.hideWelcomeOverlay({ remember: true }); showToast(I18n.t('toast.accountCreated')); }
  });

  btnWelcomeForgot?.addEventListener('click', async () => {
    const email = $('#welcome-auth-email')?.value.trim();
    if (!email) { settingsCallbacks.setWelcomeError(I18n.t('welcome.enterEmailForReset')); return; }
    const remaining = Math.ceil((RESET_COOLDOWN_MS - (Date.now() - _resetEmailLastSent)) / 1000);
    if (remaining > 0) { settingsCallbacks.setWelcomeError(`Please wait ${remaining}s before sending another reset email.`); return; }
    const result = await window.snowify.sendPasswordReset(email);
    if (result?.error) { settingsCallbacks.setWelcomeError(result.error); return; }
    _resetEmailLastSent = Date.now();
    settingsCallbacks.setWelcomeError(I18n.t('welcome.resetEmailSent'), true);
  });

  btnWelcomeSkip?.addEventListener('click', () => { settingsCallbacks.hideWelcomeOverlay({ remember: true }); });

  btnSignIn?.addEventListener('click', async () => {
    const email = $('#auth-email')?.value.trim(), password = $('#auth-password')?.value;
    const errorEl = $('#auth-error'); if (errorEl) errorEl.classList.add('hidden');
    if (!email || !password) { if (errorEl) { errorEl.textContent = I18n.t('welcome.enterEmailPassword'); errorEl.classList.remove('hidden'); } return; }
    const result = await window.snowify.signInWithEmail(email, password);
    if (result?.error) { if (errorEl) { errorEl.textContent = result.error; errorEl.classList.remove('hidden'); } }
    else showToast(I18n.t('toast.signedIn'));
  });

  btnSignUp?.addEventListener('click', async () => {
    const email = $('#auth-email')?.value.trim(), password = $('#auth-password')?.value;
    const errorEl = $('#auth-error'); if (errorEl) errorEl.classList.add('hidden');
    if (!email || !password) { if (errorEl) { errorEl.textContent = I18n.t('welcome.enterEmailPassword'); errorEl.classList.remove('hidden'); } return; }
    if (password.length < 6) { if (errorEl) { errorEl.textContent = I18n.t('welcome.passwordMinLength'); errorEl.classList.remove('hidden'); } return; }
    const result = await window.snowify.signUpWithEmail(email, password);
    if (result?.error) { if (errorEl) { errorEl.textContent = result.error; errorEl.classList.remove('hidden'); } }
    else showToast(I18n.t('toast.accountCreated'));
  });

  btnSignOut?.addEventListener('click', async () => {
    settingsCallbacks.setCloudSyncPaused(true);
    settingsCallbacks.clearCloudSaveTimeout();
    try { await settingsCallbacks.forceCloudSave(); } catch (_) {}
    await window.snowify.authSignOut();
    settingsCallbacks.setCloudSyncPaused(false);
    showToast(I18n.t('toast.signedOut'));
  });

  btnForgot?.addEventListener('click', async () => {
    const email = $('#auth-email')?.value.trim();
    const errorEl = $('#auth-error');
    if (errorEl) { errorEl.style.color = ''; errorEl.classList.add('hidden'); }
    if (!email) { if (errorEl) { errorEl.textContent = I18n.t('welcome.enterEmailForReset'); errorEl.classList.remove('hidden'); } return; }
    const remaining = Math.ceil((RESET_COOLDOWN_MS - (Date.now() - _resetEmailLastSent)) / 1000);
    if (remaining > 0) { if (errorEl) { errorEl.textContent = `Please wait ${remaining}s before sending another reset email.`; errorEl.classList.remove('hidden'); } return; }
    btnForgot.disabled = true;
    const result = await window.snowify.sendPasswordReset(email);
    if (result?.error) {
      btnForgot.disabled = false;
      if (errorEl) { errorEl.textContent = result.error; errorEl.classList.remove('hidden'); }
      return;
    }
    _resetEmailLastSent = Date.now();
    if (errorEl) { errorEl.style.color = 'var(--accent)'; errorEl.textContent = I18n.t('welcome.resetEmailSent'); errorEl.classList.remove('hidden'); }
    let secs = 60;
    const iv = setInterval(() => {
      if (--secs <= 0) { clearInterval(iv); btnForgot.disabled = false; btnForgot.textContent = I18n.t('welcome.forgotPassword'); }
      else btnForgot.textContent = `Resend in ${secs}s`;
    }, 1000);
  });

  btnSyncNow?.addEventListener('click', async () => {
    if (!settingsCallbacks.getCloudUser()) return;
    settingsCallbacks.updateSyncStatus(I18n.t('sync.syncing'));
    await settingsCallbacks.cloudLoadAndMerge({ forceCloud: true });
    await settingsCallbacks.forceCloudSave();
    settingsCallbacks.updateSyncStatus(I18n.t('sync.syncedJustNow'));
  });

  // ── Profile ──
  const btnEditName     = $('#btn-edit-name');
  const btnCancelName   = $('#btn-cancel-name');
  const btnSaveName     = $('#btn-save-name');
  const profileNameInput = $('#profile-name-input');
  const btnChangeAvatar = $('#btn-change-avatar');
  const btnChangeBanner = $('#btn-change-banner');
  const btnRemoveBanner = $('#btn-remove-banner');
  const bioInput        = $('#profile-bio-input');
  const bioCount        = $('#profile-bio-count');
  const btnSaveBio      = $('#btn-save-bio');
  const bannerPreview   = $('#profile-banner-preview');

  btnEditName?.addEventListener('click', () => {
    const row = $('#profile-edit-name-row');
    if (!row || !profileNameInput) return;
    row.classList.remove('hidden');
    profileNameInput.value = $('#profile-display-name')?.textContent || '';
    profileNameInput.focus(); profileNameInput.select();
  });
  btnCancelName?.addEventListener('click', () => { $('#profile-edit-name-row')?.classList.add('hidden'); });

  btnSaveName?.addEventListener('click', async () => {
    const name = (profileNameInput?.value || '').trim(); if (!name) return;
    const result = await window.snowify.updateProfile({ displayName: name });
    if (result?.error) { showToast(I18n.t('toast.failedUpdateName')); return; }
    const nameEl = $('#profile-display-name'); if (nameEl) nameEl.textContent = name;
    const avatarEl = $('#profile-avatar'); if (avatarEl) avatarEl.src = result.photoURL || generateDefaultAvatar(name);
    $('#profile-edit-name-row')?.classList.add('hidden');
    showToast(I18n.t('toast.nameUpdated'));
  });
  profileNameInput?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') btnSaveName?.click(); if (e.key === 'Escape') btnCancelName?.click();
  });

  btnChangeAvatar?.addEventListener('click', async () => {
    const filePath = await window.snowify.pickImage(); if (!filePath) return;
    if (/\.gif$/i.test(filePath)) { showToast('GIFs are not supported'); return; }
    try {
      const dataUrl = await window.snowify.readImage(filePath);
      if (!dataUrl) { showToast(I18n.t('toast.failedLoadImage')); return; }
      const cropped = await openCropModal({ dataUrl, title: 'Crop Profile Picture', circle: true, aspectRatio: 1, outputWidth: 256, outputHeight: 256, quality: 0.85 });
      if (!cropped) return;
      const updateResult = await window.snowify.updateProfile({ photoURL: cropped });
      if (updateResult?.error) { showToast(I18n.t('toast.failedUpdateAvatarMsg', { error: updateResult.error })); return; }
      const avatarEl = $('#profile-avatar'); if (avatarEl) avatarEl.src = cropped;
      showToast(I18n.t('toast.avatarUpdated'));
    } catch (_) { showToast(I18n.t('toast.failedLoadImage')); }
  });

  bioInput?.addEventListener('input', () => { if (bioCount && bioInput) bioCount.textContent = `${bioInput.value.length}/200`; });

  btnChangeBanner?.addEventListener('click', async () => {
    const filePath = await window.snowify.pickImage(); if (!filePath) return;
    if (/\.gif$/i.test(filePath)) { showToast('GIFs are not supported'); return; }
    const dataUri = await window.snowify.readImage(filePath);
    if (!dataUri) { showToast(I18n.t('toast.failedLoadImage')); return; }
    const cropped = await openCropModal({ dataUrl: dataUri, title: 'Crop Banner', circle: false, aspectRatio: 3, outputWidth: 960, outputHeight: 320, quality: 0.85 });
    if (!cropped) return;
    const res = await window.snowify.updateProfileExtras({ banner: cropped });
    if (!res?.success) { showToast(res?.error || I18n.t('toast.failedUpdateBanner')); return; }
    if (bannerPreview) bannerPreview.innerHTML = `<img src="${escapeHtml(cropped)}" alt="" draggable="false" />`;
    if (btnRemoveBanner) btnRemoveBanner.style.display = '';
    showToast(I18n.t('toast.bannerUpdated'));
  });

  btnRemoveBanner?.addEventListener('click', async () => {
    const res = await window.snowify.updateProfileExtras({ banner: '' }); if (!res?.success) return;
    if (bannerPreview) bannerPreview.innerHTML = `<span class="profile-banner-placeholder">${escapeHtml(I18n.t('settings.noBanner'))}</span>`;
    btnRemoveBanner.style.display = 'none'; showToast(I18n.t('toast.bannerRemoved'));
  });

  btnSaveBio?.addEventListener('click', async () => {
    const bio = (bioInput?.value || '').trim().slice(0, 200);
    const res = await window.snowify.updateProfileExtras({ bio });
    if (res?.success) showToast(I18n.t('toast.bioSaved')); else showToast(res?.error || I18n.t('toast.failedSaveBio'));
  });

  settingsCallbacks.updateAccountUI(await window.snowify.getUser());

  // ── Data management ──
  $('#setting-clear-history').addEventListener('click', () => {
    if (confirm(I18n.t('settings.confirmClearHistory'))) {
      state.recentTracks = []; callbacks.saveState(); renderHome(); showToast(I18n.t('toast.historyCleared'));
    }
  });

  $('#setting-clear-search-history').addEventListener('click', () => {
    if (confirm(I18n.t('settings.confirmClearSearchHistory'))) {
      state.searchHistory = []; callbacks.saveState(); showToast(I18n.t('toast.searchHistoryCleared'));
    }
  });

  $('#setting-reset-all').addEventListener('click', () => {
    if (confirm(I18n.t('settings.confirmResetAll'))) { localStorage.removeItem('snowify_state'); location.reload(); }
  });

  // ── Changelog ──
  async function openChangelog(version, sinceVersion) {
    const modal = $('#changelog-modal'), body = $('#changelog-body'), meta = $('#changelog-meta'), title = $('#changelog-title');
    body.innerHTML = `<div class="changelog-loading"><div class="spinner"></div><p>${I18n.t('changelog.loading')}</p></div>`;
    meta.textContent = ''; title.textContent = I18n.t('changelog.title');
    modal.classList.remove('hidden');

    if (sinceVersion && compareSemver(sinceVersion, version) < 0) {
      const releases = await window.snowify.getRecentReleases();
      const missed = releases
        .filter(r => r.version && compareSemver(r.version, sinceVersion) > 0 && compareSemver(r.version, version) <= 0)
        .sort((a, b) => compareSemver(b.version, a.version));

      if (!missed.length) { return openChangelog(version); }

      title.textContent = missed.length === 1
        ? (missed[0].name || I18n.t('changelog.whatsNewVersion', { version }))
        : I18n.t('changelog.title');
      meta.textContent = missed.length > 1 ? I18n.t('changelog.updatesSince', { count: missed.length, version: sinceVersion }) : '';

      let html = '';
      missed.forEach((rel, i) => {
        if (missed.length > 1) {
          const dateStr = rel.date ? new Date(rel.date).toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' }) : '';
          html += `<div class="changelog-version-section${i > 0 ? ' changelog-version-divider' : ''}">`;
          html += `<h2 class="changelog-version-heading">${escapeHtml(rel.name || `v${rel.version}`)}</h2>`;
          if (dateStr) html += `<p class="changelog-version-date">${dateStr}</p>`;
        }
        html += renderMarkdown(rel.body || '');
        if (missed.length > 1) html += '</div>';
      });
      body.innerHTML = html;

      if (missed.length === 1) {
        const rel = missed[0];
        title.textContent = rel.name || I18n.t('changelog.whatsNewVersion', { version: rel.version });
        if (rel.date) {
          const d = new Date(rel.date);
          meta.textContent = I18n.t('changelog.released', { date: d.toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' }) });
        }
      }
    } else {
      const data = await window.snowify.getChangelog(version);
      if (!data || !data.body) { body.innerHTML = `<div class="changelog-empty"><p>${I18n.t('changelog.noChangelog')}</p></div>`; meta.textContent = `v${version}`; return; }
      title.textContent = data.name || I18n.t('changelog.whatsNewVersion', { version: data.version });
      if (data.date) { const d = new Date(data.date); meta.textContent = I18n.t('changelog.released', { date: d.toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' }) }); }
      body.innerHTML = renderMarkdown(data.body);
    }

    body.querySelectorAll('a[href]').forEach(a => {
      a.addEventListener('click', (e) => { e.preventDefault(); window.snowify.openExternal(a.href); });
    });
  }

  function closeChangelog() { $('#changelog-modal').classList.add('hidden'); }
  $('#changelog-close').addEventListener('click', closeChangelog);
  $('#changelog-ok').addEventListener('click', closeChangelog);
  $('#changelog-modal').addEventListener('click', (e) => { if (e.target === e.currentTarget) closeChangelog(); });

  $('#btn-open-changelog').addEventListener('click', async () => {
    openChangelog(await window.snowify.getVersion());
  });
  $('#btn-discord-server').addEventListener('click', () => { window.snowify.openExternal('https://discord.gg/JHDZraE5TD'); });

  // Show changelog after update
  (async () => {
    const version        = await window.snowify.getVersion();
    const lastSeenVersion = localStorage.getItem('snowify_last_changelog_version');
    if (lastSeenVersion && lastSeenVersion !== version) setTimeout(() => openChangelog(version, lastSeenVersion), 1500);
    localStorage.setItem('snowify_last_changelog_version', version);
  })();

  // ── Version label ──
  (async () => { $('#app-version-label').textContent = `v${await window.snowify.getVersion()}`; })();

  // ── Update banner ──
  const _updateBanner    = $('#update-banner');
  const _updateBannerMsg = $('#update-banner-msg');
  function showUpdateBanner(msg, version) {
    const key = `snowify_update_dismissed_${version}`;
    if (localStorage.getItem(key)) return;
    _updateBannerMsg.textContent = msg;
    _updateBanner.style.display = '';
    $('#update-banner-go').onclick = () => callbacks.switchView('settings');
    $('#update-banner-dismiss').onclick = () => { _updateBanner.style.display = 'none'; localStorage.setItem(key, '1'); };
  }

  const btnCheckUpdate   = $('#btn-check-update');
  const btnInstallUpdate = $('#btn-install-update');
  const updateStatusRow  = $('#update-status-row');
  const updateStatusLabel = $('#update-status-label');
  const updateStatusDesc = $('#update-status-desc');

  btnCheckUpdate.addEventListener('click', async () => {
    btnCheckUpdate.disabled    = true;
    btnCheckUpdate.textContent = I18n.t('settings.checking');
    updateStatusRow.style.display = ''; updateStatusLabel.textContent = I18n.t('update.checking');
    updateStatusDesc.textContent = ''; btnInstallUpdate.style.display = 'none';
    await window.snowify.checkForUpdates();
    setTimeout(() => { btnCheckUpdate.disabled = false; btnCheckUpdate.textContent = I18n.t('settings.checkUpdates'); }, 3000);
  });

  btnInstallUpdate.addEventListener('click', () => {
    btnInstallUpdate.disabled = true; btnInstallUpdate.textContent = I18n.t('settings.downloading');
    window.snowify.installUpdate();
  });

  window.snowify.onUpdateStatus((data) => {
    updateStatusRow.style.display = '';
    switch (data.status) {
      case 'checking':
        updateStatusLabel.textContent = I18n.t('update.checking'); updateStatusDesc.textContent = ''; btnInstallUpdate.style.display = 'none'; break;
      case 'available':
        updateStatusLabel.textContent = I18n.t('update.available', { version: data.version }); updateStatusDesc.textContent = I18n.t('update.availableDesc');
        btnInstallUpdate.style.display = ''; btnInstallUpdate.disabled = false; btnInstallUpdate.textContent = I18n.t('settings.downloadInstall');
        showUpdateBanner(`Update available: v${data.version} — download it in Settings.`, data.version); break;
      case 'up-to-date':
        updateStatusLabel.textContent = I18n.t('update.upToDate'); updateStatusDesc.textContent = ''; btnInstallUpdate.style.display = 'none'; break;
      case 'downloading':
        updateStatusLabel.textContent = I18n.t('update.downloading', { percent: data.percent }); updateStatusDesc.textContent = ''; btnInstallUpdate.style.display = 'none'; break;
      case 'downloaded':
        updateStatusLabel.textContent = I18n.t('update.downloaded', { version: data.version }); updateStatusDesc.textContent = I18n.t('update.downloadedDesc');
        btnInstallUpdate.style.display = ''; btnInstallUpdate.disabled = false; btnInstallUpdate.textContent = I18n.t('settings.restartUpdate');
        btnInstallUpdate.onclick = () => window.snowify.installUpdate();
        showUpdateBanner(`v${data.version} is ready — restart Snowify to apply the update.`, data.version); break;
      case 'error': {
        updateStatusLabel.textContent = I18n.t('update.error');
        let errMsg = data.message || '';
        if (errMsg.includes('latest.yml') || errMsg.includes('latest-linux.yml')) errMsg = I18n.t('update.errorNoMetadata');
        else if (errMsg.includes('net::') || errMsg.includes('ENOTFOUND')) errMsg = I18n.t('update.errorNoConnection');
        else if (errMsg.length > 120) errMsg = errMsg.slice(0, 120) + '\u2026';
        updateStatusDesc.textContent = errMsg; btnInstallUpdate.style.display = 'none'; break;
      }
    }
  });

  // ── Developer section ──
  const _rendererLogs = [];
  const _maxRendererLogs = 200;
  function pushRendererLog(level, msg) {
    const ts = new Date().toISOString().slice(11, 23);
    _rendererLogs.push({ ts, level, msg, source: 'renderer' });
    if (_rendererLogs.length > _maxRendererLogs) _rendererLogs.shift();
  }
  const _origLog = console.log.bind(console), _origWarn = console.warn.bind(console), _origErr = console.error.bind(console);
  console.log   = (...args) => { pushRendererLog('log',  args.map(a => typeof a === 'string' ? a : JSON.stringify(a)).join(' ')); _origLog(...args);  };
  console.warn  = (...args) => { pushRendererLog('warn', args.map(a => typeof a === 'string' ? a : JSON.stringify(a)).join(' ')); _origWarn(...args); };
  console.error = (...args) => { pushRendererLog('error',args.map(a => typeof a === 'string' ? a : JSON.stringify(a)).join(' ')); _origErr(...args);  };

  const devModeToggle  = $('#setting-dev-mode');
  const devModeContent = $('#dev-mode-content');
  devModeToggle.checked = localStorage.getItem('snowify_dev_mode') === '1';
  devModeContent.style.display = devModeToggle.checked ? '' : 'none';
  devModeToggle.addEventListener('change', () => {
    localStorage.setItem('snowify_dev_mode', devModeToggle.checked ? '1' : '0');
    devModeContent.style.display = devModeToggle.checked ? '' : 'none';
    if (devModeToggle.checked) { refreshLogs(); loadVersions(); }
  });

  const _VM_COLLAPSED = 5;
  let _vmLoaded = false;
  async function loadVersions() {
    if (_vmLoaded) return;
    const listEl = $('#version-manager-list');
    const currentVersion = await window.snowify.getVersion();
    try {
      const releases = await window.snowify.getRecentReleases();
      if (!releases || !releases.length) { listEl.innerHTML = `<div class="version-manager-empty">${I18n.t('settings.noReleases')}</div>`; _vmLoaded = true; return; }
      const platform = window.snowify.platform;
      let html = '';
      releases.forEach((r, idx) => {
        const isCurrent = r.version === currentVersion;
        const dateStr = r.date ? new Date(r.date).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) : '';
        let downloadAsset = null;
        if (r.assets?.length) {
          for (const a of r.assets) {
            const n = a.name.toLowerCase();
            if (platform === 'linux'  && (n.endsWith('.appimage') || n.endsWith('.deb'))) { downloadAsset = a; break; }
            if (platform === 'win32'  && n.endsWith('.exe'))  { downloadAsset = a; break; }
            if (platform === 'darwin' && (n.endsWith('.dmg') || n.endsWith('.zip'))) { downloadAsset = a; break; }
          }
          if (!downloadAsset) downloadAsset = r.assets.find(a => !a.name.endsWith('.yml') && !a.name.endsWith('.yaml') && !a.name.endsWith('.blockmap'));
        }
        const sizeStr     = downloadAsset ? `${(downloadAsset.size / 1024 / 1024).toFixed(1)} MB` : '';
        const tag         = isCurrent ? `<span class="version-tag version-tag-current">${I18n.t('settings.currentVersion')}</span>` : '';
        const downloadBtn = !isCurrent && downloadAsset
          ? `<button class="btn-setting-action btn-version-download" data-url="${escapeHtml(downloadAsset.url)}">${I18n.t('settings.download')}</button>`
          : !isCurrent && r.url ? `<button class="btn-setting-action btn-version-download" data-url="${escapeHtml(r.url)}">${I18n.t('settings.viewRelease')}</button>` : '';
        const hidden = idx >= _VM_COLLAPSED ? ' style="display:none" data-vm-extra' : '';
        html += `<div class="version-manager-item${isCurrent ? ' version-current' : ''}"${hidden}><div class="version-manager-info"><span class="version-manager-name">v${escapeHtml(r.version)} ${tag}</span><span class="version-manager-meta">${escapeHtml(dateStr)}${sizeStr ? ' · ' + sizeStr : ''}</span></div>${downloadBtn}</div>`;
      });
      if (releases.length > _VM_COLLAPSED) html += `<button class="version-manager-show-more" id="btn-vm-show-more">${I18n.t('settings.showOlderVersions', { count: releases.length - _VM_COLLAPSED })}</button>`;
      listEl.innerHTML = html;
      listEl.querySelectorAll('.btn-version-download').forEach(btn => btn.addEventListener('click', () => window.snowify.openExternal(btn.dataset.url)));
      const showMoreBtn = listEl.querySelector('#btn-vm-show-more');
      if (showMoreBtn) showMoreBtn.addEventListener('click', () => { listEl.querySelectorAll('[data-vm-extra]').forEach(el => el.style.display = ''); showMoreBtn.remove(); });
      _vmLoaded = true;
    } catch (_) { listEl.innerHTML = `<div class="version-manager-empty">${I18n.t('settings.releasesError')}</div>`; }
  }

  const logsOutput    = $('#debug-logs-output');
  const logsContainer = $('#debug-logs-container');

  async function refreshLogs() {
    try {
      const mainLogs = await window.snowify.getLogs();
      const all = [...mainLogs.map(l => ({ ...l, source: 'main' })), ..._rendererLogs].sort((a, b) => a.ts.localeCompare(b.ts));
      logsOutput.innerHTML = all.map(l => {
        const lt = l.level === 'error' ? 'ERR' : l.level === 'warn' ? 'WRN' : 'LOG';
        return `<span class="log-line log-${l.level}">[${l.ts}] [${l.source === 'main' ? 'main' : 'renderer'}] [${lt}] ${escapeHtml(l.msg)}</span>`;
      }).join('\n') || `<span class="log-empty">${I18n.t('settings.noLogs')}</span>`;
      logsContainer.scrollTop = logsContainer.scrollHeight;
    } catch (_) { logsOutput.textContent = 'Failed to load logs'; }
  }

  const _logsObserver = new MutationObserver(() => {
    if ($('#view-settings').classList.contains('active') && devModeToggle.checked) { refreshLogs(); loadVersions(); }
  });
  _logsObserver.observe($('#view-settings'), { attributes: true, attributeFilter: ['class'] });
  if (devModeToggle.checked) { refreshLogs(); loadVersions(); }

  $('#btn-copy-logs').addEventListener('click', async () => {
    await refreshLogs(); navigator.clipboard.writeText(logsOutput.textContent).then(() => showToast(I18n.t('settings.logsCopied')));
  });
  $('#btn-clear-logs').addEventListener('click', () => { _rendererLogs.length = 0; logsOutput.innerHTML = ''; showToast(I18n.t('settings.logsCleared')); });

  // ── Wrapped dev preview ──
  const devWrappedBtn = document.createElement('div');
  devWrappedBtn.className = 'setting-row';
  devWrappedBtn.innerHTML = `
    <div class="setting-info">
      <span class="setting-label" data-i18n="settings.devWrapped">${I18n.t('settings.devWrapped')}</span>
      <span class="setting-desc" data-i18n="settings.devWrappedDesc">${I18n.t('settings.devWrappedDesc')}</span>
    </div>
    <button class="btn-setting-action" id="btn-preview-wrapped" data-i18n="settings.devWrappedBtn">${I18n.t('settings.devWrappedBtn')}</button>`;
  devModeContent.appendChild(devWrappedBtn);
  document.getElementById('btn-preview-wrapped')?.addEventListener('click', () => {
    const year = new Date().getFullYear();
    const targetYear = state.playLog.some(e => new Date(e.ts).getFullYear() === year) ? year : year - 1;
    window.WrappedManager?.show(targetYear, true);
  });

  // ── Source lists ──
  function renderSourceList(listEl, available, enabled, onChange) {
    listEl.innerHTML = '';
    const enabledSet = new Set(enabled);
    const ordered = [
      ...enabled.filter(id => available.find(s => s.id === id)),
      ...available.filter(s => !enabledSet.has(s.id)).map(s => s.id),
    ];
    ordered.forEach((sourceId) => {
      const def = available.find(s => s.id === sourceId); if (!def) return;
      const isEnabled  = enabledSet.has(sourceId);
      const enabledPos = enabled.indexOf(sourceId);
      const isPrimary  = enabledPos === 0;
      const label = def.label ?? (def.labelKey ? I18n.t(def.labelKey) : sourceId);
      const desc  = def.desc  ?? (def.descKey  ? I18n.t(def.descKey)  : '');
      const item  = document.createElement('div');
      item.className     = 'source-item' + (isEnabled ? ' source-enabled' : ' source-disabled');
      item.dataset.sourceId = sourceId;
      item.innerHTML = `
        <div class="source-item-arrows">
          <button class="source-arrow-btn" data-dir="up" ${enabledPos <= 0 ? 'disabled' : ''} aria-label="Move up"><svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M7 14l5-5 5 5z"/></svg></button>
          <button class="source-arrow-btn" data-dir="down" ${!isEnabled || enabledPos >= enabled.length - 1 ? 'disabled' : ''} aria-label="Move down"><svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M7 10l5 5 5-5z"/></svg></button>
        </div>
        <div class="source-item-info"><span class="source-item-name">${escapeHtml(label)}</span><span class="source-item-desc">${escapeHtml(desc)}</span></div>
        ${isEnabled ? `<span class="source-badge ${isPrimary ? 'source-badge-primary' : 'source-badge-fallback'}">${I18n.t(isPrimary ? 'settings.sourcePrimary' : 'settings.sourceFallback')}</span>` : ''}
        <label class="toggle-switch source-item-toggle"><input type="checkbox" ${isEnabled ? 'checked' : ''}><span class="toggle-slider"></span></label>`;
      listEl.appendChild(item);

      item.querySelector('input[type=checkbox]').addEventListener('change', (e) => {
        const newEnabled = [...enabled];
        if (e.target.checked) { if (!newEnabled.includes(sourceId)) newEnabled.push(sourceId); }
        else {
          if (newEnabled.length <= 1) { e.target.checked = true; showToast(I18n.t('toast.atLeastOneSource')); return; }
          const i = newEnabled.indexOf(sourceId); if (i !== -1) newEnabled.splice(i, 1);
        }
        enabled = newEnabled; onChange(newEnabled); renderSourceList(listEl, available, enabled, onChange);
      });

      item.querySelectorAll('.source-arrow-btn').forEach(btn => {
        btn.addEventListener('click', () => {
          const dir = btn.dataset.dir, newEnabled = [...enabled], i = newEnabled.indexOf(sourceId); if (i === -1) return;
          if (dir === 'up' && i > 0) { [newEnabled[i], newEnabled[i-1]] = [newEnabled[i-1], newEnabled[i]]; }
          else if (dir === 'down' && i < newEnabled.length - 1) { [newEnabled[i], newEnabled[i+1]] = [newEnabled[i+1], newEnabled[i]]; }
          enabled = newEnabled; onChange(newEnabled); renderSourceList(listEl, available, enabled, onChange);
        });
      });
    });
  }

  const songListEl = $('#song-sources-list');
  const metaListEl = $('#meta-sources-list');
  function renderAllSourceLists() {
    if (songListEl) renderSourceList(songListEl, window.SnowifySources._song, state.songSources, (v) => { state.songSources = v; callbacks.saveState(); });
    if (metaListEl) renderSourceList(metaListEl, window.SnowifySources._meta, state.metadataSources, (v) => { state.metadataSources = v; callbacks.saveState(); });
  }
  window.SnowifySources._refreshSources = renderAllSourceLists;
  renderAllSourceLists();
}

// ─── resettable for I18n.onChange ────────────────────────────────────────────
export function resetSettingsInitialized() {
  _settingsInitialized = false;
}
