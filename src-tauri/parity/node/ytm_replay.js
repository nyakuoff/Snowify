// Differential runner for the ytmusic port: src/main/ytmusic.js + media.js (spotify:matchTrack)
// unmodified, with https.request (under follow-redirects, so axios keeps its production
// config) answered from a recorded session plus per-scenario overrides.
// Input: { recording: path.json.gz | null, overrides: [{ method, url, bodyIncludes?, response }],
//          resolveProxy: string | { error: message }, ops: [[channel, [wire args]] | ["$init"]] }
// response: { status, statusText, headers: {..}, data } | { error: { message, code } }
// Output: { results, logs: [[level, msg]], requests: [[method, url, headers [[k, v]], body, proxy]] }
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const Module = require('module');
const https = require('https');
const { PassThrough, Writable } = require('stream');

const input = JSON.parse(fs.readFileSync(0, 'utf8'));
for (const k of ['log', 'warn', 'error']) console[k] = () => {};
const MAIN = path.resolve(__dirname, '../../../src/main');
const codecSrc = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { encode, decode } = new Function('window', `${codecSrc}\nreturn { encode, decode };`)(globalThis);

const recorded = input.recording ? JSON.parse(zlib.gunzipSync(fs.readFileSync(input.recording))).exchanges : [];
// Requests are matched without the clock-dependent utcOffsetMinutes.
const key = (method, url, body) => `${method.toUpperCase()} ${url} ${(body || '').replace(/"utcOffsetMinutes":-?[0-9.e+-]+/, '')}`;
const pool = new Map();
for (const e of recorded) {
  const k = key(e.method, e.url, e.body);
  if (!pool.has(k)) pool.set(k, []);
  pool.get(k).push(e);
}
function answer(method, url, body) {
  for (const o of input.overrides || []) {
    if (o.method.toUpperCase() === method.toUpperCase() && url.startsWith(o.url) && (!o.bodyIncludes || (body || '').includes(o.bodyIncludes))) return o.response;
  }
  const list = pool.get(key(method, url, body));
  if (!list) return { status: 404, statusText: 'Not Found', headers: { 'content-type': 'text/plain' }, data: 'no recording' };
  const e = list.length > 1 ? list.shift() : list[0];
  return { status: e.status, statusText: e.statusText, headers: e.resHeaders, data: e.data };
}

const requests = [];
https.request = (options, cb) => {
  const chunks = [];
  const req = new Writable({ write(c, _e, done) { chunks.push(Buffer.from(c)); done(); } });
  req.setTimeout = () => req;
  req.abort = () => {};
  req.on('finish', () => setImmediate(() => {
    const url = `${options.protocol}//${options.hostname}${options.path}`;
    const body = chunks.length ? Buffer.concat(chunks).toString('utf8') : null;
    const headers = Object.entries(options.headers || {}).map(([k, v]) => [k, String(v)]).sort((a, b) => (a[0] < b[0] ? -1 : 1));
    const agent = options.agent;
    const proxy = !agent || !agent.proxy ? null : agent.proxy.href ?? `socks${agent.proxy.type}://${agent.proxy.host}:${agent.proxy.port}`;
    requests.push([options.method, url, headers, body, proxy]);
    const r = answer(options.method, url, body);
    if (r.error) return req.emit('error', Object.assign(new Error(r.error.message), { code: r.error.code }));
    const res = new PassThrough();
    res.statusCode = r.status;
    res.statusMessage = r.statusText || '';
    res.headers = {};
    for (const [k, v] of Object.entries(r.headers || {})) {
      if (!['content-encoding', 'content-length', 'transfer-encoding'].includes(k.toLowerCase())) res.headers[k.toLowerCase()] = v;
    }
    cb(res);
    res.end(Buffer.from(r.data || '', 'utf8'));
  }));
  return req;
};

const handlers = {};
const electron = {
  app: { getPath: () => '/tmp/snowify-ytm-replay', isPackaged: false, getLocale: () => 'en-US' },
  session: { defaultSession: { resolveProxy: async () => {
    const p = input.resolveProxy ?? 'DIRECT';
    if (p && typeof p === 'object') throw new Error(p.error);
    return p;
  } } },
  ipcMain: { handle: (ch, fn) => { handlers[ch] = fn; }, on: () => {} },
  dialog: {}, shell: {}, BrowserWindow: { getAllWindows: () => [] },
};
const load = Module._load;
Module._load = function (req, ...rest) { return req === 'electron' ? electron : load.call(this, req, ...rest); };
process.on('uncaughtException', () => {});

const { _logBuffer } = require(path.join(MAIN, 'logger'));
const ytmusic = require(path.join(MAIN, 'ytmusic'));
const ctx = {};
ytmusic.register(electron.ipcMain, ctx);
require(path.join(MAIN, 'media')).register(electron.ipcMain, ctx);

const rejection = (ch, err) => `Error invoking remote method '${ch}': ${err == null ? 'reply was never sent' : String(err)}`;
(async () => {
  const results = [];
  for (const [ch, args] of input.ops) {
    if (ch === '$init') {
      // main.js: `try { await initYTMusic(ctx); } catch (err) { console.error(...) }`
      try { await ytmusic.initYTMusic(ctx); } catch (err) { console.error('[YTMusic] Initialization failed (will retry on first use):', err.message); }
      results.push(null);
      continue;
    }
    try { results.push({ ok: encode(await handlers[ch]({}, ...args.map(decode))) }); } catch (e) { results.push({ err: encode(rejection(ch, e)) }); }
    // A request a handler started but stopped waiting for (Promise.all rejected early) still
    // goes out, as it would in the app: let it land before the next op.
    await new Promise((r) => setTimeout(r, 30));
  }
  process.stdout.write(JSON.stringify({ results, logs: _logBuffer.map((e) => [e.level, e.msg]), requests }), () => process.exit(0));
})();
