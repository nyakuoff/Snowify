// Differential runner for src/js.rs: asks V8 (Electron 33) directly.
// Input:  { nums: [hex f64 bits], values: [JSON text], dates: [ms], parse: [[utf16 units]] }
// Output: { nums: [String(n)], strings: [String(v)], json: [JSON.stringify(v)], dates: [toISOString] }
const fs = require('fs');
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
const path = require('path');
const codecSrc = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { encode } = new Function('window', `${codecSrc}\nreturn { encode };`)(globalThis);
const view = new DataView(new ArrayBuffer(8));
const num = (hex) => { view.setBigUint64(0, BigInt('0x' + hex)); return view.getFloat64(0); };
const values = input.values.map((t) => JSON.parse(t));
process.stdout.write(JSON.stringify({
  localDates: (input.localDates || []).map((s) => { const t = Date.parse(s); return Number.isNaN(t) ? null : t; }),
  utcOffsets: (input.offsetTimes || []).map((ms) => -new Date(ms).getTimezoneOffset()),
  zone: new Intl.DateTimeFormat().resolvedOptions().timeZone,
  nums: input.nums.map((h) => String(num(h))),
  strings: values.map((v) => String(v)),
  json: values.map((v) => JSON.stringify(v)),
  dates: input.dates.map((ms) => new Date(ms).toISOString()),
  // Texts arrive as UTF-16 unit arrays so lone surrogates survive the trip.
  inspect: (input.inspect || []).map((units) => { const r = require('util').inspect(String.fromCharCode(...units)); const a = []; for (let i = 0; i < r.length; i++) a.push(r.charCodeAt(i)); return a; }),
  parse: (input.parse || []).map((units) => {
    try { return { ok: encode(JSON.parse(String.fromCharCode(...units))) }; } catch (e) { return { err: encode(e.message) }; }
  }),
}));
