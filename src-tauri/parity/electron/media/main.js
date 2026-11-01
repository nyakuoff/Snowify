// Media probe under real Electron 33: a file:// page with the app's googlevideo CORS hook.
// Fixtures as JSON in SNOWIFY_MEDIA_FIXTURES. Prints {"seen":[],"results":"<json>"}.
const { app, BrowserWindow, ipcMain, session } = require('electron');
const path = require('path');
ipcMain.handle('done', (_e, results) => {
  process.stdout.write(JSON.stringify({ seen: [], results }) + '\n');
  app.exit(0);
});
app.whenReady().then(() => {
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
    const responseHeaders = { ...details.responseHeaders };
    if (details.url.includes('.googlevideo.com/')) responseHeaders['Access-Control-Allow-Origin'] = ['*'];
    callback({ responseHeaders });
  });
  const win = new BrowserWindow({
    show: false,
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: false, backgroundThrottling: false, autoplayPolicy: 'no-user-gesture-required' },
  });
  win.loadFile(path.join(__dirname, 'index.html'));
});
