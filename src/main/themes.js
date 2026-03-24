const path = require('path');
const fs = require('fs');
const { app, dialog, shell } = require('electron');

function getThemesDir() {
  const dir = path.join(app.getPath('userData'), 'themes');
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function getThemeSourcesPath() { return path.join(getThemesDir(), '_sources.json'); }

function readThemeSources() {
  try {
    const p = getThemeSourcesPath();
    if (fs.existsSync(p)) return JSON.parse(fs.readFileSync(p, 'utf-8'));
  } catch {}
  return {};
}

function writeThemeSources(map) {
  try { fs.writeFileSync(getThemeSourcesPath(), JSON.stringify(map, null, 2), 'utf-8'); }
  catch (err) { console.error('Failed to write theme sources:', err); }
}

function parseThemeName(css, filename) {
  const match = css.match(/\/\*\s*@name\s+(.+?)\s*\*\//i);
  if (match) return match[1].trim();
  return filename.replace(/\.css$/i, '').replace(/[-_]/g, ' ').replace(/\b\w/g, c => c.toUpperCase());
}

function getMarketplaceThemeMeta() {
  const metaPath = path.join(getThemesDir(), '_marketplace.json');
  try { return JSON.parse(fs.readFileSync(metaPath, 'utf-8')); } catch { return {}; }
}

function saveMarketplaceThemeMeta(meta) {
  fs.writeFileSync(path.join(getThemesDir(), '_marketplace.json'), JSON.stringify(meta, null, 2), 'utf-8');
}

function httpsGet(url) {
  const https = require('https');
  return new Promise((resolve, reject) => {
    const doRequest = (u, redirects = 0) => {
      if (redirects > 5) return reject(new Error('Too many redirects'));
      const req = https.get(u, { headers: { 'User-Agent': 'Snowify' } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) return doRequest(res.headers.location, redirects + 1);
        let data = '';
        res.on('data', chunk => data += chunk);
        res.on('end', () => res.statusCode === 200 ? resolve(data) : reject(new Error(`HTTP ${res.statusCode}`)));
      });
      req.on('error', reject);
      req.setTimeout(15000, () => { req.destroy(); reject(new Error('Timeout')); });
    };
    doRequest(url);
  });
}

function register(ipcMain, ctx) {
  ipcMain.handle('theme:scan', async () => {
    try {
      const dir = getThemesDir();
      return fs.readdirSync(dir).filter(f => f.endsWith('.css')).sort().map(f => {
        try { const css = fs.readFileSync(path.join(dir, f), 'utf-8'); return { id: f, name: parseThemeName(css, f) }; }
        catch { return { id: f, name: f.replace(/\.css$/i, '') }; }
      });
    } catch (err) { console.error('Theme scan error:', err); return []; }
  });

  ipcMain.handle('theme:load', async (_event, id) => {
    try {
      const p = path.join(getThemesDir(), path.basename(id));
      return fs.existsSync(p) ? fs.readFileSync(p, 'utf-8') : null;
    } catch (err) { console.error('Theme load error:', err); return null; }
  });

  ipcMain.handle('theme:add', async () => {
    const result = await dialog.showOpenDialog(ctx.mainWindow, {
      title: 'Add custom theme (.css)', filters: [{ name: 'CSS Files', extensions: ['css'] }], properties: ['openFile', 'multiSelections']
    });
    if (result.canceled || !result.filePaths.length) return null;
    const added = [];
    const dir = getThemesDir();
    const sources = readThemeSources();
    for (const src of result.filePaths) {
      try {
        const filename = path.basename(src);
        const dest = path.join(dir, filename);
        fs.copyFileSync(src, dest);
        sources[filename] = src;
        const css = fs.readFileSync(dest, 'utf-8');
        added.push({ id: filename, name: parseThemeName(css, filename) });
      } catch (err) { console.error('Theme add error:', err); }
    }
    writeThemeSources(sources);
    return added.length ? added : null;
  });

  ipcMain.handle('theme:reload', async (_event, id) => {
    try {
      const filename = path.basename(id);
      const dest = path.join(getThemesDir(), filename);
      const sources = readThemeSources();
      const srcPath = sources[filename];
      if (srcPath && fs.existsSync(srcPath)) fs.copyFileSync(srcPath, dest);
      return fs.existsSync(dest) ? fs.readFileSync(dest, 'utf-8') : null;
    } catch (err) { console.error('Theme reload error:', err); return null; }
  });

  ipcMain.handle('theme:remove', async (_event, id) => {
    try {
      const filename = path.basename(id);
      const p = path.join(getThemesDir(), filename);
      if (fs.existsSync(p)) fs.unlinkSync(p);
      const sources = readThemeSources();
      if (sources[filename]) { delete sources[filename]; writeThemeSources(sources); }
      return true;
    } catch (err) { console.error('Theme remove error:', err); return false; }
  });

  ipcMain.handle('theme:openFolder', async () => { shell.openPath(getThemesDir()); return true; });

  ipcMain.handle('themes:getInstalled', () => getMarketplaceThemeMeta());

  ipcMain.handle('themes:install', async (_event, registryEntry) => {
    try {
      const { repo, id } = registryEntry;
      const branch = registryEntry.branch || 'main';
      const subPath = registryEntry.path || '';
      const file = registryEntry.file || 'theme.css';
      const localSrc = path.join(__dirname, '..', '..', 'plugins', 'themes', id);
      const useLocal = !app.isPackaged && fs.existsSync(path.join(localSrc, file));
      const rawBase = `https://raw.githubusercontent.com/${repo}/${branch}${subPath ? '/' + subPath : ''}`;
      const css = useLocal ? fs.readFileSync(path.join(localSrc, file), 'utf-8') : await httpsGet(`${rawBase}/${file}`);
      const filename = `marketplace-${id}.css`;
      fs.writeFileSync(path.join(getThemesDir(), filename), css, 'utf-8');
      const meta = getMarketplaceThemeMeta();
      meta[id] = { name: registryEntry.name, version: registryEntry.version || '1.0.0', author: registryEntry.author || '', filename, official: registryEntry.official || false };
      saveMarketplaceThemeMeta(meta);
      return { success: true, themeId: `custom:${filename}` };
    } catch (err) { console.error('Theme install error:', err); return { error: err.message || 'Install failed' }; }
  });

  ipcMain.handle('themes:uninstallMarketplace', async (_event, themeId) => {
    try {
      const meta = getMarketplaceThemeMeta();
      const entry = meta[themeId];
      if (entry) {
        const filePath = path.join(getThemesDir(), entry.filename);
        if (fs.existsSync(filePath)) fs.unlinkSync(filePath);
        delete meta[themeId];
        saveMarketplaceThemeMeta(meta);
      }
      return { success: true };
    } catch (err) { console.error('Theme uninstall error:', err); return { error: err.message }; }
  });
}

module.exports = { getThemesDir, register };
