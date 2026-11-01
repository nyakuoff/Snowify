// Differential runner for src/ytdlp.rs: src/main/ytdlp.js unmodified, with process.platform,
// process.resourcesPath and app.getPath('userData') faked.
// Input:  { platform, resourcesPath, userData, ops: ["path" | "update"] }
// Output: { results: [path | null], logs: [[level, msg]] }
const fs = require('fs');
const path = require('path');
const Module = require('module');
const cp = require('child_process');

const input = JSON.parse(fs.readFileSync(0, 'utf8'));
for (const k of ['log', 'warn', 'error']) console[k] = () => {};
const MAIN = path.resolve(__dirname, '../../../src/main');
const electron = { app: { getPath: (n) => (n === 'userData' ? input.userData : `/mock/${n}`) } };
const load = Module._load;
Module._load = function (req, ...rest) { return req === 'electron' ? electron : load.call(this, req, ...rest); };
Object.defineProperty(process, 'platform', { value: input.platform });
Object.defineProperty(process, 'resourcesPath', { value: input.resourcesPath });

// Track the fire-and-forget execFile so each op finishes before the next.
let pending = Promise.resolve();
const realExecFile = cp.execFile;
cp.execFile = (file, args, opts, cb) => {
  let done;
  pending = new Promise((r) => { done = r; });
  return realExecFile(file, args, opts, (...a) => { cb(...a); done(); });
};

const { _logBuffer } = require(path.join(MAIN, 'logger'));
const { getYtDlpPath, updateYtDlp } = require(path.join(MAIN, 'ytdlp'));
(async () => {
  const results = [];
  for (const op of input.ops) {
    if (op === 'path') results.push(getYtDlpPath());
    else { pending = Promise.resolve(); updateYtDlp(); await pending; results.push(null); }
  }
  process.stdout.write(JSON.stringify({ results, logs: _logBuffer.map((e) => [e.level, e.msg]) }), () => process.exit(0));
})();
