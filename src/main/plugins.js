const path = require('path');
const fs = require('fs');
const { app } = require('electron');

const MARKETPLACE_REGISTRY_URL = 'https://raw.githubusercontent.com/nyakuoff/Snowify/main/plugins/registry.json';

function getPluginsDir() {
  const dir = path.join(app.getPath('userData'), 'plugins');
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function sanitizePluginId(id) {
  const value = String(id || '').trim();
  if (!value) return null;
  if (!/^[a-z0-9._-]+$/i.test(value)) return null;
  return value;
}

function safeJoinUnder(baseDir, relativePath) {
  const normalized = path.posix.normalize(String(relativePath || ''));
  if (!normalized || normalized === '.' || normalized.startsWith('../') || normalized.includes('/../')) {
    throw new Error('Invalid path');
  }
  const target = path.resolve(baseDir, normalized);
  const root = path.resolve(baseDir) + path.sep;
  if (!target.startsWith(root)) throw new Error('Invalid path');
  return target;
}

function encodePathForUrl(relPath) {
  return String(relPath)
    .split('/')
    .map(seg => encodeURIComponent(seg))
    .join('/');
}

function resolveLogoUrl(manifest, pluginDir) {
  if (!manifest.logoUrl) return manifest;
  const logo = manifest.logoUrl;
  if (logo.startsWith('http://') || logo.startsWith('https://') || logo.startsWith('data:')) return manifest;
  // Relative path — read and encode as data URL
  const logoPath = path.join(pluginDir, logo);
  if (!fs.existsSync(logoPath)) return manifest;
  try {
    const ext = path.extname(logo).slice(1).toLowerCase();
    const mime = ext === 'svg' ? 'image/svg+xml' : ext === 'jpg' || ext === 'jpeg' ? 'image/jpeg' : ext === 'webp' ? 'image/webp' : 'image/png';
    const data = fs.readFileSync(logoPath).toString('base64');
    return { ...manifest, logoUrl: `data:${mime};base64,${data}` };
  } catch { return manifest; }
}

function getInstalledPlugins() {
  const plugins = [];
  try {
    for (const name of fs.readdirSync(getPluginsDir())) {
      const pluginDir = path.join(getPluginsDir(), name);
      if (!fs.statSync(pluginDir).isDirectory()) continue;
      const manifestPath = path.join(pluginDir, 'snowify-plugin.json');
      if (fs.existsSync(manifestPath)) {
        try {
          const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
          plugins.push(resolveLogoUrl(manifest, pluginDir));
        } catch {}
      }
    }
  } catch {}
  return plugins;
}

function httpsGet(url, binary = false) {
  const https = require('https');
  return new Promise((resolve, reject) => {
    const doRequest = (u, redirects = 0) => {
      if (redirects > 5) return reject(new Error('Too many redirects'));
      const req = https.get(u, { headers: { 'User-Agent': 'Snowify', 'Accept': 'application/vnd.github+json' } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) return doRequest(res.headers.location, redirects + 1);
        const chunks = [];
        res.on('data', chunk => chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)));
        res.on('end', () => {
          if (res.statusCode !== 200) return reject(new Error(`HTTP ${res.statusCode}`));
          const buf = Buffer.concat(chunks);
          resolve(binary ? buf : buf.toString('utf-8'));
        });
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
        if (fs.existsSync(localPath)) {
          const registry = JSON.parse(fs.readFileSync(localPath, 'utf-8'));
          // Resolve relative logoUrls from local plugin directories
          if (registry.plugins) {
            registry.plugins = registry.plugins.map(p => {
              if (!p.logoUrl || p.logoUrl.startsWith('http') || p.logoUrl.startsWith('data:')) return p;
              const localPluginDir = path.join(__dirname, '..', '..', 'plugins', p.path || p.id);
              return resolveLogoUrl(p, localPluginDir);
            });
          }
          return registry;
        }
      } catch {}
    }
    try {
      const registry = JSON.parse(await httpsGet(MARKETPLACE_REGISTRY_URL));
      // Resolve relative logoUrls to GitHub raw URLs
      if (registry.plugins) {
        registry.plugins = registry.plugins.map(p => {
          if (!p.logoUrl || p.logoUrl.startsWith('http') || p.logoUrl.startsWith('data:')) return p;
          const repoPath = p.path ? `/${p.path}` : '';
          return { ...p, logoUrl: `https://raw.githubusercontent.com/${p.repo}/${p.branch || 'main'}${repoPath}/${p.logoUrl}` };
        });
      }
      return registry;
    } catch (err) { console.error('Failed to fetch plugin registry:', err); return { version: 1, plugins: [], themes: [] }; }
  });

  ipcMain.handle('plugins:getInstalled', () => getInstalledPlugins());

  ipcMain.handle('plugins:install', async (_event, registryEntry) => {
    try {
      const { repo } = registryEntry;
      const safeEntryId = sanitizePluginId(registryEntry?.id);
      if (!safeEntryId) return { error: 'Invalid plugin id' };
      const branch = registryEntry.branch || 'main';
      const subPath = registryEntry.path || '';
      const localSrc = path.join(__dirname, '..', '..', 'plugins', safeEntryId);
      const useLocal = isDev && fs.existsSync(path.join(localSrc, 'snowify-plugin.json'));
      const rawBase = `https://raw.githubusercontent.com/${repo}/${branch}${subPath ? '/' + subPath : ''}`;

      let manifestRaw;
      if (useLocal) manifestRaw = fs.readFileSync(path.join(localSrc, 'snowify-plugin.json'), 'utf-8');
      else manifestRaw = await httpsGet(`${rawBase}/snowify-plugin.json`);
      const manifest = JSON.parse(manifestRaw);

      const safeManifestId = sanitizePluginId(manifest.id || safeEntryId);
      if (!safeManifestId) return { error: 'Invalid plugin manifest id' };
      const pluginDir = safeJoinUnder(getPluginsDir(), safeManifestId);
      if (fs.existsSync(pluginDir)) fs.rmSync(pluginDir, { recursive: true });
      fs.mkdirSync(pluginDir, { recursive: true });
      fs.writeFileSync(path.join(pluginDir, 'snowify-plugin.json'), manifestRaw, 'utf-8');

      if (manifest.renderer) {
        const rendererPath = String(manifest.renderer);
        const dstRenderer = safeJoinUnder(pluginDir, rendererPath);
        fs.mkdirSync(path.dirname(dstRenderer), { recursive: true });
        if (useLocal) {
          const srcRenderer = safeJoinUnder(localSrc, rendererPath);
          fs.copyFileSync(srcRenderer, dstRenderer);
        } else {
          fs.writeFileSync(dstRenderer, await httpsGet(`${rawBase}/${encodePathForUrl(rendererPath)}`), 'utf-8');
        }
      }
      if (manifest.styles) {
        try {
          const stylesPath = String(manifest.styles);
          const dstStyles = safeJoinUnder(pluginDir, stylesPath);
          fs.mkdirSync(path.dirname(dstStyles), { recursive: true });
          if (useLocal) {
            const srcStyles = safeJoinUnder(localSrc, stylesPath);
            fs.copyFileSync(srcStyles, dstStyles);
          } else {
            fs.writeFileSync(dstStyles, await httpsGet(`${rawBase}/${encodePathForUrl(stylesPath)}`), 'utf-8');
          }
        } catch {}
      }
      // Download logo file if logoUrl is a relative path
      if (manifest.logoUrl && !manifest.logoUrl.startsWith('http') && !manifest.logoUrl.startsWith('data:')) {
        try {
          const logoPath = String(manifest.logoUrl);
          const dstLogo = safeJoinUnder(pluginDir, logoPath);
          fs.mkdirSync(path.dirname(dstLogo), { recursive: true });
          if (useLocal) {
            const srcLogo = safeJoinUnder(localSrc, logoPath);
            if (fs.existsSync(srcLogo)) fs.copyFileSync(srcLogo, dstLogo);
          } else {
            const logoData = await httpsGet(`${rawBase}/${encodePathForUrl(logoPath)}`, true);
            fs.writeFileSync(dstLogo, logoData);
          }
        } catch {}
      }
      return { success: true, plugin: manifest };
    } catch (err) { console.error('Plugin install error:', err); return { error: err.message || 'Install failed' }; }
  });

  ipcMain.handle('plugins:uninstall', async (_event, pluginId) => {
    try {
      const safeId = sanitizePluginId(pluginId);
      if (!safeId) return { error: 'Invalid plugin id' };
      const dir = safeJoinUnder(getPluginsDir(), safeId);
      if (fs.existsSync(dir)) fs.rmSync(dir, { recursive: true });
      return { success: true };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('plugins:getFiles', (_event, pluginId) => {
    try {
      const safeId = sanitizePluginId(pluginId);
      if (!safeId) return null;
      let dir = safeJoinUnder(getPluginsDir(), safeId);
      if (isDev) {
        const localDir = safeJoinUnder(path.join(__dirname, '..', '..', 'plugins'), safeId);
        if (fs.existsSync(path.join(localDir, 'snowify-plugin.json'))) dir = localDir;
      }
      const manifestPath = path.join(dir, 'snowify-plugin.json');
      if (!fs.existsSync(manifestPath)) return null;
      const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf-8'));
      const result = { manifest };
      if (manifest.renderer) {
        const jsPath = safeJoinUnder(dir, manifest.renderer);
        if (fs.existsSync(jsPath)) result.js = fs.readFileSync(jsPath, 'utf-8');
      }
      if (manifest.styles) {
        const cssPath = safeJoinUnder(dir, manifest.styles);
        if (fs.existsSync(cssPath)) result.css = fs.readFileSync(cssPath, 'utf-8');
      }
      return result;
    } catch (err) { console.error('Get plugin files error:', err); return null; }
  });
}

module.exports = { register };
