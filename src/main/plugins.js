const path = require('path');
const fs = require('fs');
const { app } = require('electron');

const MARKETPLACE_REGISTRY_URL = 'https://raw.githubusercontent.com/nyakuoff/Snowify/main/plugins/registry.json';

function getPluginsDir() {
  const dir = path.join(app.getPath('userData'), 'plugins');
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function getInstalledPlugins() {
  const plugins = [];
  try {
    for (const name of fs.readdirSync(getPluginsDir())) {
      const pluginDir = path.join(getPluginsDir(), name);
      if (!fs.statSync(pluginDir).isDirectory()) continue;
      const manifestPath = path.join(pluginDir, 'snowify-plugin.json');
      if (fs.existsSync(manifestPath)) {
        try { plugins.push(JSON.parse(fs.readFileSync(manifestPath, 'utf-8'))); } catch {}
      }
    }
  } catch {}
  return plugins;
}

function httpsGet(url) {
  const https = require('https');
  return new Promise((resolve, reject) => {
    const doRequest = (u, redirects = 0) => {
      if (redirects > 5) return reject(new Error('Too many redirects'));
      const req = https.get(u, { headers: { 'User-Agent': 'Snowify', 'Accept': 'application/vnd.github+json' } }, (res) => {
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

function register(ipcMain) {
  const isDev = !app.isPackaged;

  ipcMain.handle('plugins:getRegistry', async () => {
    if (isDev) {
      try {
        const localPath = path.join(__dirname, '..', '..', 'plugins', 'registry.json');
        if (fs.existsSync(localPath)) return JSON.parse(fs.readFileSync(localPath, 'utf-8'));
      } catch {}
    }
    try { return JSON.parse(await httpsGet(MARKETPLACE_REGISTRY_URL)); }
    catch (err) { console.error('Failed to fetch plugin registry:', err); return { version: 1, plugins: [], themes: [] }; }
  });

  ipcMain.handle('plugins:getInstalled', () => getInstalledPlugins());

  ipcMain.handle('plugins:install', async (_event, registryEntry) => {
    try {
      const { repo, id } = registryEntry;
      const branch = registryEntry.branch || 'main';
      const subPath = registryEntry.path || '';
      const localSrc = path.join(__dirname, '..', '..', 'plugins', id);
      const useLocal = isDev && fs.existsSync(path.join(localSrc, 'snowify-plugin.json'));
      const rawBase = `https://raw.githubusercontent.com/${repo}/${branch}${subPath ? '/' + subPath : ''}`;

      let manifestRaw;
      if (useLocal) manifestRaw = fs.readFileSync(path.join(localSrc, 'snowify-plugin.json'), 'utf-8');
      else manifestRaw = await httpsGet(`${rawBase}/snowify-plugin.json`);
      const manifest = JSON.parse(manifestRaw);

      const pluginDir = path.join(getPluginsDir(), manifest.id || id);
      if (fs.existsSync(pluginDir)) fs.rmSync(pluginDir, { recursive: true });
      fs.mkdirSync(pluginDir, { recursive: true });
      fs.writeFileSync(path.join(pluginDir, 'snowify-plugin.json'), manifestRaw, 'utf-8');

      if (manifest.renderer) {
        if (useLocal) fs.copyFileSync(path.join(localSrc, manifest.renderer), path.join(pluginDir, manifest.renderer));
        else fs.writeFileSync(path.join(pluginDir, manifest.renderer), await httpsGet(`${rawBase}/${manifest.renderer}`), 'utf-8');
      }
      if (manifest.styles) {
        try {
          if (useLocal) fs.copyFileSync(path.join(localSrc, manifest.styles), path.join(pluginDir, manifest.styles));
          else fs.writeFileSync(path.join(pluginDir, manifest.styles), await httpsGet(`${rawBase}/${manifest.styles}`), 'utf-8');
        } catch {}
      }
      return { success: true, plugin: manifest };
    } catch (err) { console.error('Plugin install error:', err); return { error: err.message || 'Install failed' }; }
  });

  ipcMain.handle('plugins:uninstall', async (_event, pluginId) => {
    try {
      const dir = path.join(getPluginsDir(), pluginId);
      if (fs.existsSync(dir)) fs.rmSync(dir, { recursive: true });
      return { success: true };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('plugins:getFiles', (_event, pluginId) => {
    try {
      let dir = path.join(getPluginsDir(), pluginId);
      if (isDev) {
        const localDir = path.join(__dirname, '..', '..', 'plugins', pluginId);
        if (fs.existsSync(path.join(localDir, 'snowify-plugin.json'))) dir = localDir;
      }
      const manifestPath = path.join(dir, 'snowify-plugin.json');
      if (!fs.existsSync(manifestPath)) return null;
      const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
      const result = { manifest };
      if (manifest.renderer) { const jsPath = path.join(dir, manifest.renderer); if (fs.existsSync(jsPath)) result.js = fs.readFileSync(jsPath, 'utf-8'); }
      if (manifest.styles) { const cssPath = path.join(dir, manifest.styles); if (fs.existsSync(cssPath)) result.css = fs.readFileSync(cssPath, 'utf-8'); }
      return result;
    } catch (err) { console.error('Get plugin files error:', err); return null; }
  });
}

module.exports = { register };
