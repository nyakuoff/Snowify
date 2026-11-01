// Generic differential runner for an ipcMain-registering module under src/main, loaded
// unmodified with Electron, dialogs, shell and https.get mocked from the scenario.
// Input: {
//   module, userData, isPackaged,
//   dialogs: [{ canceled, filePaths }],             // consumed in order by showOpenDialog
//   http: { [url]: [response...] },                 // consumed in order; the last one repeats
//   fetch: { [url prefix]: [fresponse...] },        // global fetch; first matching prefix
//   opTimeoutMs,                                    // per-op wait before "pending" (2000)
//   ops: [[channel, [wire args...]] | ["$parallel", [[channel, [args]]...]] | ["$sleep", ms]]
// }
// fresponse: { status, statusText, headers: [[name, value]], body } | { b64 } | { error: true }
// response: { status, headers, body } | { b64 } | { error: { name, message, props } } | { timeout: true }
// Output: { results: [{ ok: wire } | { err: rejection message } | "pending"],
//           logs: [[level, msg]], dialogCalls, openPathCalls, httpCalls: [[url, headers]],
//           fetchCalls: [[url, [[name, value]] as sent, redirect]] }
const fs = require('fs');
const path = require('path');
const Module = require('module');
const EventEmitter = require('events');

const input = JSON.parse(fs.readFileSync(0, 'utf8'));
for (const k of ['log', 'warn', 'error']) console[k] = () => {};
const MAIN = path.resolve(__dirname, '../../../src/main');
const codecSrc = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { encode, decode } = new Function('window', `${codecSrc}\nreturn { encode, decode };`)(globalThis);

const dialogCalls = [];
const openPathCalls = [];
const httpCalls = [];
const handlers = {};
const electron = {
  app: {
    getPath: (name) => (name === 'userData' ? input.userData : `/mock/${name}`),
    isPackaged: input.isPackaged,
    getLocale: () => 'en-US',
    getVersion: () => '2.0.0',
  },
  dialog: {
    showOpenDialog: async (_win, opts) => {
      dialogCalls.push(opts);
      return input.dialogs.shift() || { canceled: true, filePaths: [] };
    },
    showSaveDialog: async (_win, opts) => {
      dialogCalls.push(opts);
      return (input.saveDialogs || []).shift() || { canceled: true };
    },
  },
  shell: { openPath: async (p) => { openPathCalls.push(p); return ''; }, openExternal: async () => {} },
  ipcMain: { handle: (ch, fn) => { handlers[ch] = fn; }, on: (ch, fn) => { handlers[ch] = fn; } },
  BrowserWindow: { getAllWindows: () => [] },
};
// A fixed clock: Date.now() and new Date() return `now`.
if (input.now !== undefined) {
  const RealDate = Date;
  global.Date = class extends RealDate {
    constructor(...a) { super(...(a.length ? a : [input.now])); }
    static now() { return input.now; }
  };
}
const load = Module._load;
Module._load = function (req, ...rest) {
  if (req === 'electron-updater') return { autoUpdater: new EventEmitter() }; // dev mode never uses it
  return req === 'electron' ? electron : load.call(this, req, ...rest);
};

const https = require('https');
https.get = (u, opts, cb) => {
  // Node's own synchronous argument checks.
  let parsed;
  try { parsed = new URL(u); } catch {
    throw Object.assign(new TypeError('Invalid URL'), { code: 'ERR_INVALID_URL', input: String(u) });
  }
  if (parsed.protocol !== 'https:') {
    throw Object.assign(new TypeError(`Protocol "${parsed.protocol}" not supported. Expected "https:"`), { code: 'ERR_INVALID_PROTOCOL' });
  }
  httpCalls.push([String(u), opts.headers]);
  const req = new EventEmitter();
  let onTimeout = () => {};
  req.setTimeout = (_ms, fn) => { onTimeout = fn; return req; };
  req.destroy = () => {};
  const list = input.http[u];
  const r = list ? (list.length > 1 ? list.shift() : list[0]) : { status: 404, body: '' };
  setImmediate(() => {
    if (r.timeout) return onTimeout();
    if (r.error) return req.emit('error', Object.assign(new (r.error.name === 'TypeError' ? TypeError : Error)(r.error.message), r.error.props || {}));
    const res = new EventEmitter();
    res.statusCode = r.status;
    res.headers = r.headers || {};
    cb(res);
    const body = r.b64 !== undefined ? Buffer.from(r.b64, 'base64') : Buffer.from(r.body || '', 'utf8');
    if (body.length) res.emit('data', body);
    res.emit('end');
  });
  return req;
};
// https.request (net:httpGet): the same http map and call log as https.get, keyed by
// `https://hostname + path`; a timeout fires the request's own setTimeout callback.
https.request = (o, cb) => {
  const u = `https://${o.hostname}${o.path}`;
  httpCalls.push([u, o.headers]);
  const req = new EventEmitter();
  let onTimeout = () => {};
  req.setTimeout = (_ms, fn) => { onTimeout = fn; return req; };
  req.destroy = (err) => { if (err) setImmediate(() => req.emit('error', err)); };
  req.end = () => {
    const list = input.http[u];
    const r = list ? (list.length > 1 ? list.shift() : list[0]) : { status: 404, body: '' };
    setImmediate(() => {
      if (r.timeout) return onTimeout();
      if (r.error) return req.emit('error', Object.assign(new Error(r.error.message), r.error.props || {}));
      const res = new EventEmitter();
      res.statusCode = r.status;
      res.headers = r.headers || {};
      cb(res);
      const body = r.b64 !== undefined ? Buffer.from(r.b64, 'base64') : Buffer.from(r.body || '', 'utf8');
      if (body.length) res.emit('data', body);
      res.emit('end');
    });
  };
  return req;
};
// undici's own Response/Headers, so .ok, .json() and getSetCookie() behave natively.
const fetchCalls = [];
globalThis.fetch = async (u, init = {}) => {
  fetchCalls.push([String(u), [...new Headers(init.headers || {})], init.redirect || 'follow']);
  // The first key (in order) the URL starts with.
  const key = Object.keys(input.fetch || {}).find((k) => String(u).startsWith(k));
  const list = key === undefined ? undefined : input.fetch[key];
  const r = list ? (list.length > 1 ? list.shift() : list[0]) : { status: 404, body: '' };
  await new Promise(setImmediate);
  if (r.error) throw new TypeError('fetch failed');
  const headers = new Headers();
  for (const [k, v] of r.headers || []) headers.append(k, v);
  const nullBody = [101, 204, 205, 304].includes(r.status);
  const body = nullBody ? null : r.b64 !== undefined ? Buffer.from(r.b64, 'base64') : (r.body ?? '');
  return new Response(body, { status: r.status, statusText: r.statusText || '', headers });
};
process.on('uncaughtException', () => {}); // the app's main process survives these too

const { _logBuffer } = require(path.join(MAIN, 'logger'));
require(path.join(MAIN, input.module)).register(electron.ipcMain, { mainWindow: null });

const rejection = (ch, err) => `Error invoking remote method '${ch}': ${err == null ? 'reply was never sent' : String(err)}`;

(async () => {
  const results = [];
  const run = (ch, args) => {
    const call = Promise.resolve().then(() => handlers[ch]({}, ...args.map(decode)));
    const timeout = new Promise((r) => setTimeout(() => r('pending'), input.opTimeoutMs || 2000));
    return Promise.race([call.then((v) => ({ ok: encode(v) }), (e) => ({ err: encode(rejection(ch, e)) })), timeout]);
  };
  for (const [ch, args] of input.ops) {
    if (ch === '$sleep') { await new Promise((r) => setTimeout(r, args)); results.push(null); continue; }
    results.push(ch === '$parallel' ? await Promise.all(args.map(([c, a]) => run(c, a))) : await run(ch, args));
  }
  // Exit only once the pipe has taken everything (a bare exit truncates at 64 KiB).
  process.stdout.write(JSON.stringify({ results, logs: _logBuffer.map((e) => [e.level, e.msg]), dialogCalls, openPathCalls, httpCalls, fetchCalls }), () => process.exit(0));
})();
