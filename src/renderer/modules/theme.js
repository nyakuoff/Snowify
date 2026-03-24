// ─── Theme management ───

export const BUILTIN_THEMES = ['dark', 'light', 'ocean', 'forest', 'sunset', 'rose', 'midnight'];

export function isCustomTheme(theme) {
  return theme && theme.startsWith('custom:');
}

export function customThemeId(theme) {
  return theme.slice('custom:'.length);
}

export function applyCustomThemeCss(css) {
  // Remove existing to force full re-parse (including @import)
  removeCustomThemeCss();
  const el = document.createElement('style');
  el.id = 'custom-theme-style';
  el.textContent = css;
  document.head.appendChild(el);
}

export function removeCustomThemeCss() {
  const el = document.getElementById('custom-theme-style');
  if (el) el.remove();
}

export async function loadAndApplyThemeFile(themeValue) {
  if (!isCustomTheme(themeValue)) { removeCustomThemeCss(); return false; }
  const css = await window.snowify.loadTheme(customThemeId(themeValue));
  if (css) { applyCustomThemeCss(css); return true; }
  removeCustomThemeCss();
  return false;
}

export function applyTheme(theme) {
  if (theme === 'dark' || isCustomTheme(theme)) {
    document.documentElement.removeAttribute('data-theme');
  } else {
    document.documentElement.setAttribute('data-theme', theme);
  }
  return loadAndApplyThemeFile(theme);
}

export async function populateCustomThemes(selectEl, currentValue) {
  // Remove old custom options
  selectEl.querySelectorAll('option[data-custom]').forEach(o => o.remove());
  const themes = await window.snowify.scanThemes();
  if (themes.length) {
    const sep = document.createElement('option');
    sep.disabled = true;
    sep.textContent = I18n.t('settings.customThemeSeparator');
    sep.dataset.custom = '1';
    selectEl.appendChild(sep);
    for (const t of themes) {
      const opt = document.createElement('option');
      opt.value = 'custom:' + t.id;
      opt.textContent = t.name;
      opt.dataset.custom = '1';
      selectEl.appendChild(opt);
    }
  }
  selectEl.value = currentValue;
  // If the value didn't match (theme was removed), fall back to dark
  if (selectEl.value !== currentValue && isCustomTheme(currentValue)) {
    selectEl.value = 'dark';
  }
}
