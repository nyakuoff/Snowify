/**
 * plugins.js
 * Plugin system: state persistence, loading, rendering plugin marketplace + themes tab.
 */

import state from './state.js';
import { escapeHtml, showToast } from './utils.js';
import { callbacks } from './callbacks.js';
import { applyTheme, populateCustomThemes } from './theme.js';

const $  = (sel, ctx = document) => ctx.querySelector(sel);
const $$ = (sel, ctx = document) => [...ctx.querySelectorAll(sel)];

// ─── resolvePluginArtistMeta ──────────────────────────────────────────────────
// Called by artist.js (fire-and-forget enrichment).

export async function resolvePluginArtistMeta(artistName) {
  for (const sourceId of state.metadataSources) {
    const handler = window.SnowifySources?._artistMetaHandlers?.[sourceId];
    if (!handler) continue;
    try {
      const meta = await handler(artistName);
      if (meta) return meta;
    } catch (_) { /* best-effort */ }
  }
  return null;
}

// ─── Plugin state helpers ─────────────────────────────────────────────────────

export function getPluginState() {
  try { return JSON.parse(localStorage.getItem('snowify_plugins') || '{}'); } catch { return {}; }
}

export function savePluginState(ps) {
  localStorage.setItem('snowify_plugins', JSON.stringify(ps));
}

// ─── Plugin loading ───────────────────────────────────────────────────────────

export async function loadPlugin(id) {
  if (document.querySelector(`script[data-plugin-id="${id}"], style[data-plugin-id="${id}"]`)) return;
  try {
    const files = await window.snowify.getPluginFiles(id);
    if (!files) return;
    if (files.css) {
      const style = document.createElement('style');
      style.dataset.pluginId = id;
      style.textContent = files.css;
      document.head.appendChild(style);
    }
    if (files.js) {
      const script = document.createElement('script');
      script.dataset.pluginId = id;
      script.textContent = files.js;
      document.head.appendChild(script);
    }
  } catch (err) {
    console.error(`Failed to load plugin "${id}":`, err);
  }
}

export async function loadEnabledPlugins() {
  const ps = getPluginState();
  for (const [id, info] of Object.entries(ps)) {
    if (!info.enabled) continue;
    await loadPlugin(id);
  }
}

// ─── Marketplace renderer ─────────────────────────────────────────────────────

export async function renderPlugins() {
  const grid             = $('#plugins-available-grid');
  const installedSection = $('#plugins-installed-section');
  const installedList    = $('#plugins-installed-list');
  const ps               = getPluginState();

  // Marketplace tabs
  const tabs        = $$('.marketplace-tab');
  const tabContents = $$('.marketplace-tab-content');
  tabs.forEach(tab => {
    tab.onclick = () => {
      tabs.forEach(t => t.classList.remove('active'));
      tabContents.forEach(c => c.classList.remove('active'));
      tab.classList.add('active');
      const target = tab.dataset.tab === 'themes' ? '#marketplace-themes' : '#marketplace-plugins';
      $(target)?.classList.add('active');
    };
  });

  // Fetch registry
  let registry = { plugins: [], themes: [] };
  try { registry = await window.snowify.getPluginRegistry(); } catch {}
  const registryMap = {};
  (registry.plugins || []).forEach(rp => { registryMap[rp.id] = rp; });

  // Installed plugins
  let installed = [];
  try { installed = await window.snowify.getInstalledPlugins(); } catch {}

  if (installed.length > 0) {
    installedSection.style.display = '';
    installedList.innerHTML = installed.map(p => {
      const enabled   = ps[p.id]?.enabled ?? false;
      const regEntry  = registryMap[p.id];
      const isOfficial = regEntry?.official || p.official;
      const tagClass  = isOfficial ? 'plugin-tag-official' : 'plugin-tag-community';
      const tagLabel  = isOfficial ? I18n.t('plugins.official') : I18n.t('plugins.community');
      return `
        <div class="plugin-installed-item" data-plugin-id="${escapeHtml(p.id)}">
          <div class="plugin-installed-icon">${p.logoUrl ? `<img class="plugin-logo-img" src="${escapeHtml(p.logoUrl)}" alt="${escapeHtml(p.name)}" onerror="this.style.display='none'">` : (p.icon || '🧩')}</div>
          <div class="plugin-installed-info">
            <span class="plugin-installed-name">${escapeHtml(p.name)} <span class="plugin-tag ${tagClass}">${tagLabel}</span></span>
            <span class="plugin-installed-meta">${escapeHtml(p.author || '')}${p.version ? ' · v' + escapeHtml(p.version) : ''}</span>
          </div>
          <div class="plugin-installed-actions">
            <label class="toggle-switch" title="${enabled ? I18n.t('plugins.disable') : I18n.t('plugins.enable')}">
              <input type="checkbox" data-action="toggle" ${enabled ? 'checked' : ''}>
              <span class="toggle-slider"></span>
            </label>
            <button class="plugin-uninstall-btn" data-action="uninstall" title="${I18n.t('plugins.uninstall')}">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z"/></svg>
            </button>
          </div>
        </div>`;
    }).join('');

    installedList.querySelectorAll('[data-action="toggle"]').forEach(cb => {
      cb.addEventListener('change', async () => {
        const id  = cb.closest('[data-plugin-id]').dataset.pluginId;
        const lps = getPluginState();
        if (!lps[id]) lps[id] = {};
        lps[id].enabled = cb.checked;
        savePluginState(lps);
        if (cb.checked) {
          await loadPlugin(id);
          showToast(I18n.t('plugins.enabledOk'));
        } else {
          showToast(I18n.t('plugins.disabledRestart'), { label: I18n.t('plugins.restart'), onClick: () => window.snowify.restartApp() });
        }
      });
    });

    installedList.querySelectorAll('[data-action="uninstall"]').forEach(btn => {
      btn.addEventListener('click', async (e) => {
        e.stopPropagation();
        const item   = btn.closest('[data-plugin-id]');
        const id     = item.dataset.pluginId;
        const result = await window.snowify.uninstallPlugin(id);
        if (result?.error) { showToast(I18n.t('plugins.errorUninstall')); return; }
        const lps = getPluginState();
        delete lps[id];
        savePluginState(lps);
        document.querySelectorAll(`[data-plugin-id="${id}"]`).forEach(el => {
          if (el.tagName === 'STYLE' || el.tagName === 'SCRIPT') el.remove();
        });
        showToast(I18n.t('plugins.uninstalled'));
        renderPlugins();
      });
    });
  } else {
    installedSection.style.display = 'none';
  }

  // Available plugins from registry
  const installedIds   = new Set(installed.map(p => p.id));
  const available      = registry.plugins || [];
  const allCategories  = [...new Set(available.map(p => p.category).filter(Boolean))];
  let _activeCategory  = 'all';

  const pluginSearchInput = $('#marketplace-search-plugins');
  function renderPluginGrid(query) {
    const q = (query || '').trim().toLowerCase();
    let filtered = q ? available.filter(p =>
      p.name.toLowerCase().includes(q) || (p.description || '').toLowerCase().includes(q) || (p.author || '').toLowerCase().includes(q)
    ) : available;
    if (_activeCategory !== 'all') filtered = filtered.filter(p => p.category === _activeCategory);

    const filterContainer = $('#plugin-category-filters');
    if (filterContainer && allCategories.length > 0) {
      filterContainer.innerHTML = ['all', ...allCategories].map(cat => {
        const label = cat === 'all' ? 'All' : cat.charAt(0).toUpperCase() + cat.slice(1);
        return `<button class="plugin-category-pill${_activeCategory === cat ? ' active' : ''}" data-category="${escapeHtml(cat)}">${label}</button>`;
      }).join('');
      filterContainer.querySelectorAll('.plugin-category-pill').forEach(pill => {
        pill.onclick = () => { _activeCategory = pill.dataset.category; renderPluginGrid(pluginSearchInput ? pluginSearchInput.value : ''); };
      });
    }

    if (filtered.length === 0) {
      grid.innerHTML = `<div class="plugins-empty">
        <svg width="48" height="48" viewBox="0 0 24 24" fill="currentColor" opacity="0.12"><path d="M20.5 11H19V7c0-1.1-.9-2-2-2h-4V3.5C13 2.12 11.88 1 10.5 1S8 2.12 8 3.5V5H4c-1.1 0-1.99.9-1.99 2v3.8H3.5c1.49 0 2.7 1.21 2.7 2.7s-1.21 2.7-2.7 2.7H2V20c0 1.1.9 2 2 2h3.8v-1.5c0-1.49 1.21-2.7 2.7-2.7 1.49 0 2.7 1.21 2.7 2.7V22H17c1.1 0 2-.9 2-2v-4h1.5c1.38 0 2.5-1.12 2.5-2.5S21.88 11 20.5 11z"/></svg>
        <p>${q ? 'No plugins match your search.' : I18n.t('plugins.noPlugins')}</p>
      </div>`;
      return;
    }

    grid.innerHTML = filtered.map(p => {
      const isInstalled = installedIds.has(p.id);
      const tagClass = p.official ? 'plugin-tag-official' : 'plugin-tag-community';
      const tagLabel = p.official ? I18n.t('plugins.official') : I18n.t('plugins.community');
      return `
        <div class="plugin-card" data-plugin-id="${escapeHtml(p.id)}">
          <div class="plugin-card-header">
            <div class="plugin-card-icon">${p.logoUrl ? `<img class="plugin-logo-img" src="${escapeHtml(p.logoUrl)}" alt="${escapeHtml(p.name)}" onerror="this.style.display='none'">` : (p.icon || '🧩')}</div>
            <span class="plugin-tag ${tagClass}">${tagLabel}</span>
          </div>
          <div class="plugin-card-name">${escapeHtml(p.name)}</div>
          <div class="plugin-card-desc">${escapeHtml(p.description || '')}</div>
          <button class="plugin-card-readmore" data-action="readmore" style="display:none">Read more</button>
          <div class="plugin-card-meta">${escapeHtml(p.author || '')}${p.version ? ' · v' + escapeHtml(p.version) : ''}</div>
          <button class="plugin-card-btn ${isInstalled ? 'installed' : ''}" ${isInstalled ? 'disabled' : ''} data-action="install">
            ${isInstalled ? I18n.t('plugins.installed') : I18n.t('plugins.install')}
          </button>
        </div>`;
    }).join('');

    requestAnimationFrame(() => {
      grid.querySelectorAll('.plugin-card-desc').forEach(desc => {
        const btn = desc.nextElementSibling;
        if (!btn || btn.dataset.action !== 'readmore') return;
        desc.classList.add('expanded');
        const fullH = desc.scrollHeight;
        desc.classList.remove('expanded');
        const lineH = parseFloat(getComputedStyle(desc).lineHeight) || 18;
        btn.style.display = fullH > lineH * 3 + 2 ? '' : 'none';
      });
    });
    grid.querySelectorAll('[data-action="readmore"]').forEach(btn => {
      btn.addEventListener('click', () => {
        const desc = btn.previousElementSibling;
        const expanded = desc.classList.toggle('expanded');
        btn.textContent = expanded ? 'Show less' : 'Read more';
      });
    });

    grid.querySelectorAll('[data-action="install"]:not([disabled])').forEach(btn => {
      btn.addEventListener('click', async () => {
        const card  = btn.closest('[data-plugin-id]');
        const id    = card.dataset.pluginId;
        const entry = available.find(p => p.id === id);
        if (!entry) return;
        btn.disabled    = true;
        btn.textContent = I18n.t('plugins.installing');
        const result    = await window.snowify.installPlugin(entry);
        if (result?.error) {
          btn.disabled    = false;
          btn.textContent = I18n.t('plugins.install');
          showToast(I18n.t('plugins.errorInstall'));
          return;
        }
        const lps = getPluginState();
        lps[id] = { enabled: true, installedVersion: entry.version || '1.0.0' };
        savePluginState(lps);
        await loadPlugin(id);
        showToast(I18n.t('plugins.installedOk'));
        renderPlugins();
      });
    });
  }

  renderPluginGrid(pluginSearchInput ? pluginSearchInput.value : '');
  if (pluginSearchInput) {
    if (pluginSearchInput._searchHandler) {
      pluginSearchInput.removeEventListener('input', pluginSearchInput._searchHandler);
    }
    pluginSearchInput._searchHandler = () => renderPluginGrid(pluginSearchInput.value);
    pluginSearchInput.addEventListener('input', pluginSearchInput._searchHandler);
  }

  // Themes tab
  await renderMarketplaceThemes(registry);

  // Footer links open externally (use onclick to avoid duplicates on re-render)
  document.querySelectorAll('.plugins-footer a[href]').forEach(link => {
    link.onclick = (e) => { e.preventDefault(); window.snowify.openExternal(link.href); };
  });
}

// ─── Themes marketplace tab ───────────────────────────────────────────────────

export async function renderMarketplaceThemes(registry) {
  const themesGrid             = $('#themes-available-grid');
  const themesInstalledSection = $('#themes-installed-section');
  const themesInstalledList    = $('#themes-installed-list');

  const availableThemes = (registry.themes || []).filter(t => !t.disabled);

  let installedMeta = {};
  try { installedMeta = await window.snowify.getInstalledMarketplaceThemes(); } catch {}
  const installedThemeIds = new Set(Object.keys(installedMeta));

  // Installed marketplace themes
  if (installedThemeIds.size > 0) {
    themesInstalledSection.style.display = '';
    themesInstalledList.innerHTML = Object.entries(installedMeta).map(([id, t]) => {
      const regEntry  = availableThemes.find(th => th.id === id);
      const isOfficial = regEntry?.official || t.official;
      const tagClass  = isOfficial ? 'plugin-tag-official' : 'plugin-tag-community';
      const tagLabel  = isOfficial ? I18n.t('plugins.official') : I18n.t('plugins.community');
      const isActive  = state.theme === 'custom:' + t.filename;
      return `
        <div class="plugin-installed-item" data-theme-id="${escapeHtml(id)}">
          <div class="plugin-installed-icon">🎨</div>
          <div class="plugin-installed-info">
            <span class="plugin-installed-name">${escapeHtml(t.name)} <span class="plugin-tag ${tagClass}">${tagLabel}</span></span>
            <span class="plugin-installed-meta">${escapeHtml(t.author || '')}${t.version ? ' · v' + escapeHtml(t.version) : ''}</span>
          </div>
          <div class="plugin-installed-actions">
            ${isActive ? `<span class="theme-active-badge">${I18n.t('plugins.themeActive')}</span>` : ''}
            <button class="plugin-uninstall-btn" data-action="uninstall-theme" title="${I18n.t('plugins.uninstall')}">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z"/></svg>
            </button>
          </div>
        </div>`;
    }).join('');

    themesInstalledList.querySelectorAll('[data-action="uninstall-theme"]').forEach(btn => {
      btn.addEventListener('click', async (e) => {
        e.stopPropagation();
        const item  = btn.closest('[data-theme-id]');
        const id    = item.dataset.themeId;
        const entry = installedMeta[id];
        if (entry && state.theme === 'custom:' + entry.filename) {
          state.theme = 'dark';
          applyTheme('dark');
          callbacks.saveState();
          const themeSelect = $('#theme-select');
          if (themeSelect) {
            await populateCustomThemes(themeSelect, 'dark');
            themeSelect.value = 'dark';
          }
        }
        const result = await window.snowify.uninstallMarketplaceTheme(id);
        if (result?.error) { showToast(I18n.t('plugins.errorUninstall')); return; }
        showToast(I18n.t('plugins.themeUninstalled'));
        await renderMarketplaceThemes(registry);
      });
    });
  } else {
    themesInstalledSection.style.display = 'none';
  }

  // Available themes
  const themeSearchInput = $('#marketplace-search-themes');
  function renderThemeGrid(query) {
    const q = (query || '').trim().toLowerCase();
    const filtered = q ? availableThemes.filter(t =>
      t.name.toLowerCase().includes(q) || (t.description || '').toLowerCase().includes(q) || (t.author || '').toLowerCase().includes(q)
    ) : availableThemes;

    if (filtered.length === 0) {
      themesGrid.innerHTML = `<div class="plugins-empty">
        <svg width="48" height="48" viewBox="0 0 24 24" fill="currentColor" opacity="0.12"><path d="M12 3c-4.97 0-9 4.03-9 9s4.03 9 9 9c.83 0 1.5-.67 1.5-1.5 0-.39-.15-.74-.39-1.01-.23-.26-.38-.61-.38-1 0-.83.67-1.5 1.5-1.5H16c2.76 0 5-2.24 5-5 0-4.42-4.03-8-9-8zm-5.5 9c-.83 0-1.5-.67-1.5-1.5S5.67 9 6.5 9 8 9.67 8 10.5 7.33 12 6.5 12zm3-4C8.67 8 8 7.33 8 6.5S8.67 5 9.5 5s1.5.67 1.5 1.5S10.33 8 9.5 8zm5 0c-.83 0-1.5-.67-1.5-1.5S13.67 5 14.5 5s1.5.67 1.5 1.5S15.33 8 14.5 8zm3 4c-.83 0-1.5-.67-1.5-1.5S16.67 9 17.5 9s1.5.67 1.5 1.5-.67 1.5-1.5 1.5z"/></svg>
        <p>${q ? 'No themes match your search.' : I18n.t('plugins.noThemes')}</p>
      </div>`;
      return;
    }

    themesGrid.innerHTML = filtered.map(t => {
      const isInstalled    = installedThemeIds.has(t.id);
      const tagClass       = t.official ? 'plugin-tag-official' : 'plugin-tag-community';
      const tagLabel       = t.official ? I18n.t('plugins.official') : I18n.t('plugins.community');
      const previewColors  = (t.preview || []).slice(0, 5);
      const bgColor        = previewColors[0] || '#1a1a2e';
      return `
        <div class="theme-card" data-theme-id="${escapeHtml(t.id)}">
          <div class="theme-card-preview" style="background:${escapeHtml(bgColor)}">
            ${previewColors.slice(1).map(c => `<div class="theme-preview-dot" style="background:${escapeHtml(c)}"></div>`).join('')}
          </div>
          <div class="theme-card-body">
            <div class="theme-card-header">
              <div class="theme-card-name">${escapeHtml(t.name)}</div>
              <span class="plugin-tag ${tagClass}">${tagLabel}</span>
            </div>
            <div class="theme-card-desc">${escapeHtml(t.description || '')}</div>
            <button class="theme-card-readmore" data-action="readmore" style="display:none">Read more</button>
            <div class="theme-card-meta">${escapeHtml(t.author || '')}${t.version ? ' · v' + escapeHtml(t.version) : ''}</div>
            <button class="theme-card-btn ${isInstalled ? 'installed' : ''}" ${isInstalled ? 'disabled' : ''} data-action="install-theme">
              ${isInstalled ? I18n.t('plugins.installed') : I18n.t('plugins.install')}
            </button>
          </div>
        </div>`;
    }).join('');

    requestAnimationFrame(() => {
      themesGrid.querySelectorAll('.theme-card-desc').forEach(desc => {
        const btn = desc.nextElementSibling;
        if (!btn || btn.dataset.action !== 'readmore') return;
        desc.classList.add('expanded');
        const fullH = desc.scrollHeight;
        desc.classList.remove('expanded');
        const lineH = parseFloat(getComputedStyle(desc).lineHeight) || 18;
        btn.style.display = fullH > lineH * 3 + 2 ? '' : 'none';
      });
    });
    themesGrid.querySelectorAll('[data-action="readmore"]').forEach(btn => {
      btn.addEventListener('click', () => {
        const desc = btn.previousElementSibling;
        const expanded = desc.classList.toggle('expanded');
        btn.textContent = expanded ? 'Show less' : 'Read more';
      });
    });

    themesGrid.querySelectorAll('[data-action="install-theme"]:not([disabled])').forEach(btn => {
      btn.addEventListener('click', async () => {
        const card  = btn.closest('[data-theme-id]');
        const id    = card.dataset.themeId;
        const entry = availableThemes.find(t => t.id === id);
        if (!entry) return;
        btn.disabled    = true;
        btn.textContent = I18n.t('plugins.installing');
        const result    = await window.snowify.installMarketplaceTheme(entry);
        if (result?.error) {
          btn.disabled    = false;
          btn.textContent = I18n.t('plugins.install');
          showToast(I18n.t('plugins.errorInstall'));
          return;
        }
        state.theme = result.themeId;
        await applyTheme(state.theme);
        callbacks.saveState();
        const themeSelect = $('#theme-select');
        if (themeSelect) await populateCustomThemes(themeSelect, state.theme);
        showToast(I18n.t('plugins.themeInstalled'));
        await renderMarketplaceThemes(registry);
      });
    });
  }

  renderThemeGrid(themeSearchInput ? themeSearchInput.value : '');
  if (themeSearchInput) {
    if (themeSearchInput._searchHandler) {
      themeSearchInput.removeEventListener('input', themeSearchInput._searchHandler);
    }
    themeSearchInput._searchHandler = () => renderThemeGrid(themeSearchInput.value);
    themeSearchInput.addEventListener('input', themeSearchInput._searchHandler);
  }
}
