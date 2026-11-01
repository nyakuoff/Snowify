// Differential runner for src/main/logger.js with the clock mocked and terminal output muted.
// Input:  [[via, level, [arg...], now]] where via is "console" (console[level](...args)) or
//         "capture" (_captureLog(level, args), as app:appendLog calls it).
// Args:   JSON values, or {"$": "undefined" | "fn" | "symbol" | "nan" | "inf" | "negzero"},
//         {"$bigint": "10"}, {"$date": ms}, {"$error": "msg"}, {"$circular": 1}.
// Output: { each: [newest entry after each op], final: _logBuffer }
const fs = require('fs');
const path = require('path');
const ops = JSON.parse(fs.readFileSync(0, 'utf8'));

for (const k of ['log', 'warn', 'error']) console[k] = () => {};
let now = 0;
const RealDate = Date;
global.Date = class extends RealDate {
  constructor(...a) { super(...(a.length ? a : [now])); }
  static now() { return now; }
};
const { _logBuffer, _captureLog } = require(path.resolve(__dirname, '../../../src/main/logger'));

const decode = (v) => {
  if (!v || typeof v !== 'object') return v;
  if (Array.isArray(v)) return v.map(decode);
  if ('$' in v) return { undefined, fn: () => 1, symbol: Symbol('s'), nan: NaN, inf: Infinity, negzero: -0 }[v.$];
  if ('$bigint' in v) return BigInt(v.$bigint);
  if ('$date' in v) return new RealDate(v.$date);
  if ('$error' in v) return new Error(v.$error);
  if ('$circular' in v) { const o = { a: 1 }; o.self = o; return o; }
  return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, decode(x)]));
};

const each = ops.map(([via, level, args, t]) => {
  now = t;
  if (via === 'console') console[level](...args.map(decode));
  else _captureLog(decode(level), args.map(decode));
  return _logBuffer[_logBuffer.length - 1];
});
process.stdout.write(JSON.stringify({ each, final: _logBuffer }));
