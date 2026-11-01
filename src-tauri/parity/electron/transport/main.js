// Ground truth for the IPC seam: real Electron 33, real contextBridge + ipcRenderer.
// Run: node_modules/.bin/electron src-tauri/parity/electron/transport  (prints one JSON line)
const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');
const { describe } = require('../../describe.js');
const corpus = require('../../corpus/transport.json');

const seen = [];
ipcMain.handle('echo', (_e, ...args) => {
  seen.push({ argc: args.length, args: args.map((a) => describe(a)) });
  return args[0];
});
ipcMain.handle('echoArity', (_e, ...args) => args.length);
ipcMain.handle('throw', (_e, i) => { throw eval(corpus.throws[i]); });
ipcMain.on('send', (_e, ...args) => { seen.push({ send: true, args: args.map((a) => describe(a)) }); });

let win;
ipcMain.handle('emitAll', () => {
  corpus.values.forEach((expr, i) => {
    let v;
    try { v = eval(expr); } catch { v = undefined; }
    try { win.webContents.send('ev', i, v); } catch (e) { win.webContents.send('evFail', i, String(e.message)); }
  });
  win.webContents.send('evDone');
});
ipcMain.handle('done', (_e, results) => {
  process.stdout.write(JSON.stringify({ electron: process.versions.electron, seen, results }) + '\n');
  app.exit(0);
});

app.whenReady().then(() => {
  win = new BrowserWindow({
    show: false,
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, nodeIntegration: false, sandbox: false },
  });
  win.loadFile(path.join(__dirname, 'index.html'));
});
