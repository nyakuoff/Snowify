// Packaged updater differential runner. Loads the source unchanged; only Electron,
// electron-updater and the startup timer are replaced. No network or real installer.
const fs = require('fs');
const path = require('path');
const Module = require('module');
const { EventEmitter } = require('events');
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
for (const k of ['log', 'warn', 'error']) console[k] = () => {};
const MAIN = path.resolve(__dirname, '../../../src/main');
const codec = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { encode } = new Function('window', `${codec}\nreturn { encode };`)(globalThis);
const handlers = {}, sends = [], calls = [], results = [], timers = [];
let timer, downloaded = false, quitInstalled = false;
const app = new EventEmitter();
Object.assign(app, { isPackaged: input.packaged, getVersion: () => '2.0.0', getLocale: () => 'en-US' });
const autoUpdater = new EventEmitter();
const error = (message) => message === null ? null : new Error(message);
const checks = [...(input.checks || [])], downloads = [...(input.downloads || [])];
autoUpdater.checkForUpdates = async () => {
  calls.push(['check']);
  autoUpdater.emit('checking-for-update');
  const r = checks.shift();
  if ('error' in r) { const e = error(r.error); autoUpdater.emit('error', e); throw e; }
  if (r.empty) { autoUpdater.emit('update-not-available'); return null; }
  const info = { version: r.version, releaseNotes: r.notes };
  autoUpdater.emit(r.available ? 'update-available' : 'update-not-available', info);
  return { updateInfo: info };
};
autoUpdater.downloadUpdate = async () => {
  calls.push(['download']);
  const r = downloads.shift();
  for (const percent of r.progress || []) autoUpdater.emit('download-progress', { percent });
  if ('error' in r) { const e = error(r.error); autoUpdater.emit('error', e); throw e; }
  downloaded = true;
  autoUpdater.emit('update-downloaded', { version: r.version });
};
const installs = [...(input.installs || [])];
autoUpdater.quitAndInstall = (...args) => {
  calls.push(['install', ...args]); quitInstalled = true;
  const r = installs.shift();
  if (r && 'error' in r) autoUpdater.emit('error', error(r.error));
};
// Model electron-updater's autoInstallOnAppQuit listener, outside Snowify's module.
app.on('before-quit', () => {
  if (autoUpdater.autoInstallOnAppQuit && downloaded && !quitInstalled) autoUpdater.quitAndInstall(true, false);
});
const ipcMain = { handle: (ch, fn) => { handlers[ch] = fn; }, on: (ch, fn) => { handlers[ch] = fn; } };
const load = Module._load;
Module._load = function(req, ...rest) {
  if (req === 'electron') return { app, ipcMain };
  if (req === 'electron-updater') return { autoUpdater };
  return load.call(this, req, ...rest);
};
const { _logBuffer } = require(path.join(MAIN, 'logger'));
const updater = require(path.join(MAIN, 'updater'));
const ctx = { mainWindow: { isDestroyed: () => !!input.destroyed, webContents: {
  send: (ch, payload) => sends.push([ch, encode(payload)]),
} } };
updater.register(ipcMain, ctx);
const realTimeout = global.setTimeout;
global.setTimeout = (fn, ms) => { timers.push(ms); timer = fn; };
updater.initAutoUpdater(ctx);
global.setTimeout = realTimeout;
(async () => {
  for (const action of input.actions) {
    if (action === 'timer') { if (timer) timer(); results.push(null); }
    else if (action === 'quit') { app.emit('before-quit'); results.push(null); }
    else if (action === 'error') { autoUpdater.emit('error', error(input.eventError)); results.push(null); }
    else results.push({ ok: encode(await handlers[`updater:${action}`]({})) });
    // send handlers are fire-and-forget; flush their rejection catch before snapshot.
    await new Promise(setImmediate);
  }
  process.stdout.write(JSON.stringify({ results, sends, calls, timers,
    config: [autoUpdater.autoDownload ?? null, autoUpdater.autoInstallOnAppQuit ?? null],
    logs: _logBuffer.map(e => [e.level, e.msg]),
  }), () => process.exit(0));
})();
