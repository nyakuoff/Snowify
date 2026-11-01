// Pairs up the Electron and Tauri transport-probe outputs field by field. The Tauri side's
// "what the handler received" is wire-encoded JsValues; it is decoded with the shim's own codec
// and described with the same describe() Electron's main process used.
// Input:  { electron: <probe output line>, tauri: <probe output line> } (raw JSON text)
// Output: [[what, electron JSON text, tauri JSON text]]
const fs = require('fs');
const path = require('path');
const { describe } = require('../describe.js');

const codecSrc = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { decode } = new Function('window', `${codecSrc}\nreturn { encode, decode };`)(globalThis);

const raw = JSON.parse(fs.readFileSync(0, 'utf8'));
const electron = JSON.parse(raw.electron);
const tauri = JSON.parse(raw.tauri);
const er = JSON.parse(electron.results);
const tr = JSON.parse(tauri.results);
const pairs = [];
// Values are compared as JSON text: they may hold lone surrogates, which serde rejects.
const add = (what, a, b) => pairs.push([what, JSON.stringify(a ?? null), JSON.stringify(b ?? null)]);

for (const key of ['apiShape', 'arity', 'rawEventArgs', 'missingHandler']) add(key, er[key], tr[key]);
const corpus = require('../corpus/transport.json');
const skipEvent = (i) => corpus.mainCannotProduce.includes(corpus.values[i]);
for (const key of ['invoke', 'throws', 'sendSync', 'events']) {
  const n = Math.max(er[key].length, tr[key].length);
  for (let i = 0; i < n; i++) if (!(key === 'events' && skipEvent(i))) add(`${key}[${i}]`, er[key][i], tr[key][i]);
}
add('eventFails', er.eventFails, tr.eventFails);

const tauriSeen = tauri.seen.map((s) => ({ ...s, args: s.args.map((w) => describe(decode(w))) }));
const n = Math.max(electron.seen.length, tauriSeen.length);
for (let i = 0; i < n; i++) add(`handlerReceived[${i}]`, electron.seen[i], tauriSeen[i]);

process.stdout.write(JSON.stringify(pairs));
