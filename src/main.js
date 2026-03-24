const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');

app.commandLine.appendSwitch('disable-renderer-backgrounding');

// Load context before anything else — all modules share this object
const ctx = require('./main/context');

// ─── Single-instance lock ───
const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
} else {
  app.on('second-instance', (_event, argv) => {
    if (ctx.mainWindow) {
      if (ctx.mainWindow.isMinimized()) ctx.mainWindow.restore();
      ctx.mainWindow.focus();
    }
    const deepUrl = argv.find(a => a.startsWith('snowify://'));
    if (deepUrl) handleDeepLink(deepUrl);
  });
}

// macOS: deep link arrives via 'open-url' before app is ready
app.on('open-url', (event, url) => {
  event.preventDefault();
  handleDeepLink(url);
});

// ─── Deep link handling ───
let _pendingDeepLink = null;

function handleDeepLink(url) {
  try {
    const cleaned = url.replace(/^snowify:\/?\/?/, '');
    const [type, ...rest] = cleaned.split('/').filter(Boolean);
    const id = rest.join('/');
    if (!id || !['track', 'album', 'artist'].includes(type)) return;
    console.log(`Deep link: ${type}/${id}`);
    if (ctx.mainWindow && ctx.mainWindow.webContents) {
      ctx.mainWindow.webContents.send('app:deepLink', { type, id });
      if (ctx.mainWindow.isMinimized()) ctx.mainWindow.restore();
      ctx.mainWindow.focus();
    } else {
      _pendingDeepLink = { type, id };
    }
  } catch (_) {}
}

ipcMain.handle('app:getPendingDeepLink', () => {
  const link = _pendingDeepLink;
  _pendingDeepLink = null;
  return link;
});

// ─── Module imports ───
require('./main/logger');  // side-effect: overrides console.*

const { loadMainTranslations } = require('./main/i18n');
const { cleanupCacheDir } = require('./main/audio-cache');
const { initYTMusic, register: registerYTMusic } = require('./main/ytmusic');
const { createWindow } = require('./main/window');
const { checkMacYtDlp } = require('./main/mac-setup');
const { autoSignIn, register: registerAuth } = require('./main/auth');
const { teardownSocialListeners, register: registerSocial } = require('./main/social');
const { register: registerDiscord } = require('./main/discord');
const { register: registerThumbar } = require('./main/thumbbar');
const { register: registerThemes } = require('./main/themes');
const { register: registerPlugins } = require('./main/plugins');
const { initAutoUpdater, register: registerUpdater } = require('./main/updater');
const { register: registerLyrics } = require('./main/lyrics');
const { register: registerMedia } = require('./main/media');

// ─── App lifecycle ───
app.whenReady().then(async () => {
  if (app.isPackaged) {
    app.setAsDefaultProtocolClient('snowify');
  } else {
    app.setAsDefaultProtocolClient('snowify', process.execPath, [path.resolve(process.argv[1] || '.')]);
  }

  loadMainTranslations();

  // Register all IPC handlers before creating the window
  registerDiscord(ipcMain);
  registerThumbar(ipcMain, ctx);
  registerAuth(ipcMain, ctx);
  registerSocial(ipcMain, ctx);
  registerYTMusic(ipcMain, ctx);
  registerThemes(ipcMain, ctx);
  registerPlugins(ipcMain, ctx);
  registerUpdater(ipcMain, ctx);
  registerLyrics(ipcMain);
  registerMedia(ipcMain, ctx);

  try {
    await initYTMusic(ctx);
  } catch (err) {
    console.error('[YTMusic] Initialization failed (will retry on first use):', err.message);
  }

  createWindow(ctx, { teardownSocialListeners });

  await checkMacYtDlp(ctx.mainWindow);
  autoSignIn();
  initAutoUpdater(ctx);

  // Handle cold-start deep link (Linux/Windows pass it as CLI argument)
  const coldLink = process.argv.find(a => a.startsWith('snowify://'));
  if (coldLink) handleDeepLink(coldLink);
});

app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit(); });
app.on('before-quit', () => { cleanupCacheDir(); });
app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow(ctx, { teardownSocialListeners });
});
