// Differential runner for src/main/audio-cache.js. One case per process: env (TMPDIR etc.) and
// cwd are set by the caller, since AUDIO_CACHE_DIR is computed when the module loads.
// Input:  ["ensure" | "cleanup", ...]
// Output: { dir, results: [{ok: returned} | {err: [code, message]} | null], logs: [[level, msg]] }
const fs = require('fs');
const path = require('path');
const ops = JSON.parse(fs.readFileSync(0, 'utf8'));

for (const k of ['log', 'warn', 'error']) console[k] = () => {};
const MAIN = path.resolve(__dirname, '../../../src/main');
const { _logBuffer } = require(path.join(MAIN, 'logger')); // loaded first, as in main.js
const cache = require(path.join(MAIN, 'audio-cache'));

const results = ops.map((op) => {
  if (op === 'cleanup') { cache.cleanupCacheDir(); return null; }
  try { return { ok: cache.ensureCacheDir() }; } catch (e) { return { err: [e.code, e.message] }; }
});
process.stdout.write(JSON.stringify({
  dir: cache.AUDIO_CACHE_DIR, results, logs: _logBuffer.map((e) => [e.level, e.msg]),
}));
