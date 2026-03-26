const { BrowserWindow, session, nativeImage, Tray, Menu, app, ipcMain: _ipcMain } = require('electron');
const path = require('path');
const { mt } = require('./i18n');
const { updateThumbarButtons } = require('./thumbbar');

function createWindow(ctx) {
  const isMac = process.platform === 'darwin';
  ctx.mainWindow = new BrowserWindow({
    width: 1500,
    height: 860,
    minWidth: 900,
    minHeight: 600,
    frame: false,
    backgroundColor: '#121212',
    titleBarStyle: 'hidden',
    ...(isMac && { trafficLightPosition: { x: 16, y: 12 } }),
    webPreferences: {
      preload: path.join(__dirname, '..', 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
      backgroundThrottling: false
    },
    icon: nativeImage.createFromPath(path.join(__dirname, '..', '..', 'assets', 'logo.ico'))
  });

  ctx.mainWindow.loadFile(path.join(__dirname, '..', 'renderer', 'index.html'));

  if (isMac) {
    ctx.mainWindow.webContents.on('dom-ready', () => {
      ctx.mainWindow.webContents.executeJavaScript("document.documentElement.classList.add('platform-darwin');");
    });
  }

  updateThumbarButtons(ctx.mainWindow, false);

  // Intercept close to flush pending saves before quitting
  let _closeReady = false;
  let _forceQuit = false;

  function setupTray() {
    if (ctx.tray) return;
    const icon = nativeImage.createFromPath(path.join(__dirname, '..', '..', 'assets', 'logo.png')).resize({ width: 16, height: 16 });
    ctx.tray = new Tray(icon);
    ctx.tray.setToolTip('Snowify');
    ctx.tray.setContextMenu(Menu.buildFromTemplate([
      { label: 'Open Snowify', click: () => { ctx.mainWindow?.show(); ctx.mainWindow?.focus(); } },
      { type: 'separator' },
      { label: 'Quit Snowify', click: () => { _forceQuit = true; ctx.mainWindow?.close(); } }
    ]));
    ctx.tray.on('click', () => { ctx.mainWindow?.show(); ctx.mainWindow?.focus(); });
  }

  function destroyTray() {
    if (ctx.tray) { ctx.tray.destroy(); ctx.tray = null; }
  }

  _ipcMain.on('window:setMinimizeToTray', (_e, enabled) => {
    ctx.minimizeToTray = !!enabled;
    if (enabled) setupTray();
    else destroyTray();
  });

  _ipcMain.on('window:setOpenAtLogin', (_e, enabled) => {
    app.setLoginItemSettings({ openAtLogin: !!enabled });
  });

  ctx.mainWindow.on('close', (e) => {
    if (_closeReady) return;
    if (ctx.minimizeToTray && !_forceQuit) {
      e.preventDefault();
      ctx.mainWindow.hide();
      return;
    }
    e.preventDefault();
    ctx.mainWindow.webContents.send('app:before-close');
    setTimeout(() => { _closeReady = true; ctx.mainWindow?.close(); }, 2000);
  });
  _ipcMain.once('app:close-ready', () => { _closeReady = true; ctx.mainWindow?.close(); });

  session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
    const responseHeaders = { ...details.responseHeaders };
    if (details.url.includes('.googlevideo.com/')) {
      responseHeaders['Access-Control-Allow-Origin'] = ['*'];
    }
    responseHeaders['Content-Security-Policy'] = [
      "default-src 'self'; " +
      "script-src 'self' 'unsafe-inline'; " +
      "style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; " +
      "font-src 'self' https://fonts.gstatic.com; " +
      "img-src 'self' https: http: data: file:; " +
      "media-src 'self' blob: https: http: file:; " +
      "connect-src 'self' https: http:;"
    ];
    callback({ responseHeaders });
  });

  return ctx.mainWindow;
}

module.exports = { createWindow };
