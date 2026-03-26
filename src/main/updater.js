const { app, ipcMain: _ipcMain } = require('electron');
const { autoUpdater } = require('electron-updater');

const isDev = !app.isPackaged;
let _updateDownloaded = false;

function sendUpdateStatus(ctx, status, info = {}) {
  if (ctx.mainWindow && !ctx.mainWindow.isDestroyed()) {
    ctx.mainWindow.webContents.send('updater:status', { status, ...info });
  }
}

function initAutoUpdater(ctx) {
  if (isDev) { console.log('Auto-updater disabled in dev mode'); return; }

  autoUpdater.autoDownload = false;
  autoUpdater.autoInstallOnAppQuit = true;

  autoUpdater.on('checking-for-update', () => sendUpdateStatus(ctx, 'checking'));
  autoUpdater.on('update-available', (info) => sendUpdateStatus(ctx, 'available', { version: info.version, releaseNotes: info.releaseNotes }));
  autoUpdater.on('update-not-available', () => sendUpdateStatus(ctx, 'up-to-date'));
  autoUpdater.on('download-progress', (progress) => sendUpdateStatus(ctx, 'downloading', { percent: Math.round(progress.percent) }));
  autoUpdater.on('update-downloaded', (info) => { _updateDownloaded = true; sendUpdateStatus(ctx, 'downloaded', { version: info.version }); });
  autoUpdater.on('error', (err) => sendUpdateStatus(ctx, 'error', { message: err?.message || 'Update check failed' }));

  setTimeout(() => { autoUpdater.checkForUpdates().catch(() => {}); }, 5000);
}

function register(ipcMain, ctx) {
  ipcMain.handle('app:getVersion', () => app.getVersion());
  ipcMain.handle('app:getLocale', () => app.getLocale());

  ipcMain.handle('app:getChangelog', async (_event, version) => {
    try {
      const tag = version.startsWith('v') ? version : `v${version}`;
      const https = require('https');
      const body = await new Promise((resolve, reject) => {
        const req = https.get(`https://api.github.com/repos/nyakuoff/Snowify/releases/tags/${tag}`, { headers: { 'User-Agent': 'Snowify', 'Accept': 'application/vnd.github+json' } }, (res) => {
          let data = '';
          res.on('data', chunk => data += chunk);
          res.on('end', () => res.statusCode === 200 ? resolve(data) : reject(new Error(`GitHub API ${res.statusCode}`)));
        });
        req.on('error', reject);
        req.setTimeout(10000, () => { req.destroy(); reject(new Error('Timeout')); });
      });
      const release = JSON.parse(body);
      return { version: release.tag_name?.replace(/^v/, '') || version, name: release.name || `v${version}`, body: release.body || '', date: release.published_at || '', url: release.html_url || '' };
    } catch (err) { console.error('Failed to fetch changelog:', err); return null; }
  });

  ipcMain.handle('app:getRecentReleases', async () => {
    try {
      const https = require('https');
      const body = await new Promise((resolve, reject) => {
        const req = https.get('https://api.github.com/repos/nyakuoff/Snowify/releases?per_page=20', { headers: { 'User-Agent': 'Snowify', 'Accept': 'application/vnd.github+json' } }, (res) => {
          let data = '';
          res.on('data', chunk => data += chunk);
          res.on('end', () => res.statusCode === 200 ? resolve(data) : reject(new Error(`GitHub API ${res.statusCode}`)));
        });
        req.on('error', reject);
        req.setTimeout(10000, () => { req.destroy(); reject(new Error('Timeout')); });
      });
      return JSON.parse(body).map(r => ({ version: r.tag_name?.replace(/^v/, '') || '', name: r.name || r.tag_name || '', body: r.body || '', date: r.published_at || '', url: r.html_url || '', assets: (r.assets || []).map(a => ({ name: a.name, size: a.size, url: a.browser_download_url })) }));
    } catch (err) { console.error('Failed to fetch releases:', err); return []; }
  });

  ipcMain.handle('updater:check', async () => {
    if (isDev) { sendUpdateStatus(ctx, 'error', { message: 'Auto-update is not available in dev mode' }); return null; }
    try {
      const result = await autoUpdater.checkForUpdates();
      return result?.updateInfo?.version || null;
    } catch (err) { console.error('Update check error:', err); return null; }
  });

  ipcMain.on('updater:install', () => {
    if (isDev) return;
    if (_updateDownloaded) {
      autoUpdater.quitAndInstall(false, true);
    } else {
      autoUpdater.downloadUpdate().catch(err => { console.error('Update download error:', err); sendUpdateStatus(ctx, 'error', { message: err?.message || 'Download failed' }); });
    }
  });

  ipcMain.handle('app:restart', () => { app.relaunch(); app.quit(); });
}

module.exports = { initAutoUpdater, register };
