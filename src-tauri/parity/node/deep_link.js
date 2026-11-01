// Differential runner for the deep-link handling in src/main.js, loaded unmodified: Electron is
// mocked, src/main/* modules other than context and logger are stubbed.
// Input:  [op] where op is
//   ["openUrl", url] | ["secondInstance", argv] | ["getPending"]
//   | ["window", null | { webContents: bool, minimized: bool, failAt: null | method }]
// Output: { results: [per-op result], logs: [msg] }
//   openUrl/secondInstance -> the window calls made, in order (a failing call is recorded as
//   "!name"); getPending -> the value; window -> null.
const path = require('path');
const fs = require('fs');
const Module = require('module');

const ops = JSON.parse(fs.readFileSync(0, 'utf8'));
for (const k of ['log', 'warn', 'error']) console[k] = () => {};

const handlers = {};
const ipc = {};
const electron = {
  app: {
    commandLine: { appendSwitch() {} },
    requestSingleInstanceLock: () => true,
    on: (ev, fn) => { handlers[ev] = fn; },
    whenReady: () => new Promise(() => {}),
    quit() {},
  },
  BrowserWindow: { getAllWindows: () => [] },
  ipcMain: { handle: (ch, fn) => { ipc[ch] = fn; }, on() {} },
};

const MAIN_DIR = path.resolve(__dirname, '../../../src/main') + path.sep;
const stub = new Proxy({}, { get: () => () => {} });
const load = Module._load;
Module._load = function (req, parent, ...rest) {
  if (req === 'electron') return electron;
  const file = Module._resolveFilename(req, parent, ...rest);
  if (file.startsWith(MAIN_DIR) && !/[\\/](context|logger)\.js$/.test(file)) return stub;
  return load.call(this, req, parent, ...rest);
};

require(path.resolve(__dirname, '../../../src/main.js'));
const ctx = require(path.join(MAIN_DIR, 'context'));
const { _logBuffer } = require(path.join(MAIN_DIR, 'logger'));

let calls = [];
function makeWindow({ webContents, minimized, failAt }) {
  const call = (name, ret) => {
    if (failAt === name.split(' ')[0]) { calls.push('!' + name); throw new Error('Object has been destroyed'); }
    calls.push(name);
    return ret;
  };
  return {
    get webContents() {
      return call('webContents', webContents ? { send: (ch, d) => call(`send ${ch} ${d.type}/${d.id}`) } : null);
    },
    isMinimized: () => call('isMinimized', minimized),
    restore: () => call('restore'),
    focus: () => call('focus'),
  };
}

const results = ops.map(([op, arg]) => {
  calls = [];
  switch (op) {
    case 'window': ctx.mainWindow = arg && makeWindow(arg); return null;
    case 'openUrl': handlers['open-url']({ preventDefault() {} }, arg); return calls;
    case 'secondInstance':
      try { handlers['second-instance']({}, arg); } catch { calls.push('<threw>'); }
      return calls;
    case 'getPending': return ipc['app:getPendingDeepLink']();
  }
});
process.stdout.write(JSON.stringify({ results, logs: _logBuffer.map((e) => e.msg) }));
