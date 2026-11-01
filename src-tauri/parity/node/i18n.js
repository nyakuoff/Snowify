// Differential runner for src/main/i18n.js. Reads cases from stdin, prints results as JSON.
// Cases run in order against the one module instance, so state carries over exactly as in the
// app (a failed load keeps the previous locale).
//
// Case:   { override: any, system: string, files?: {locale: text|null}, calls: [[key, params|null]] }
//         `files`, when present, replaces the locales dir for that case (null/absent = missing).
// Params: ordered [name, value] pairs. Value is a JSON primitive, or {"undefined":1},
//         {"num":"NaN"|"Infinity"|"-Infinity"|"-0"}, or {"fn":"echo"}. Object.fromEntries hoists
//         integer-like names, so the corpus must not use numeric param names.
// Output: [{ threw: bool, calls: [{ t: typeof | "throw", s: String(result) | error class }] }]
const path = require('path');
const fs = require('fs');
const Module = require('module');

const cases = JSON.parse(fs.readFileSync(0, 'utf8'));

let systemLocale = '';
let files = null;
const LOCALES_DIR = path.resolve(__dirname, '../../../src/renderer/locales') + path.sep;
const realRead = fs.readFileSync;
fs.readFileSync = function (p, ...rest) {
  if (files && typeof p === 'string' && p.startsWith(LOCALES_DIR)) {
    const text = files[path.basename(p, '.json')];
    if (text == null) throw Object.assign(new Error(`ENOENT: ${p}`), { code: 'ENOENT' });
    return text;
  }
  return realRead.call(this, p, ...rest);
};

const resolve = Module._resolveFilename;
Module._resolveFilename = function (req, ...rest) {
  return req === 'electron' ? req : resolve.call(this, req, ...rest);
};
require.cache.electron = {
  id: 'electron', filename: 'electron', loaded: true,
  exports: { app: { getLocale: () => systemLocale } },
};

const { loadMainTranslations, mt } = require(path.join(__dirname, '../../../src/main/i18n'));

const fns = { echo: (m, p, s) => `${m}|${p}|${s}` };
const decode = (v) => (v && typeof v === 'object'
  ? ('undefined' in v ? undefined : 'num' in v ? Number(v.num) : fns[v.fn])
  : v);

const attempt = (fn) => {
  try { const r = fn(); return { t: typeof r, s: String(r) }; }
  catch (e) { return { t: 'throw', s: e.constructor.name }; }
};

const out = cases.map((c) => {
  systemLocale = c.system;
  files = c.files ?? null;
  let threw = false;
  try { loadMainTranslations(c.override ?? undefined); } catch { threw = true; }
  const calls = c.calls.map(([key, params]) => attempt(() =>
    mt(key, params ? Object.fromEntries(params.map(([k, v]) => [k, decode(v)])) : undefined)));
  return { threw, calls };
});
process.stdout.write(JSON.stringify(out));
