// Differential runner for src/main/stream-cache.js with Date.now mocked. The module's private
// Map is observed through a Map subclass so eviction (not visible via get) is compared too.
// Input:  [["set", key, value, now] | ["get", key, now]]
// Output: [[result (get) | null (set), cache size after the op]]
const fs = require('fs');
const path = require('path');
const ops = JSON.parse(fs.readFileSync(0, 'utf8'));

let now = 0;
Date.now = () => now;
let cache;
const RealMap = Map;
global.Map = class extends RealMap {
  constructor(...a) { super(...a); cache ??= this; }
};
const { getCachedUrl, setCachedUrl } = require(path.resolve(__dirname, '../../../src/main/stream-cache'));
global.Map = RealMap;

process.stdout.write(JSON.stringify(ops.map(([op, key, a, b]) => {
  if (op === 'set') { now = b; setCachedUrl(key, a); return [null, cache.size]; }
  now = a;
  return [getCachedUrl(key), cache.size];
})));
