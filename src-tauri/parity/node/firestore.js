// Runs the real Firestore Lite parser, REST transport and decoder under Electron.
// Input: [{op:'get'|'set'|'update', collection?, id?, value?:IPC-wire, reply?:JSON, response?:{status?,statusText?,body?,error?}}].
// Fetch is entirely offline; response replays raw errors/bodies, reply replays JSON success.
// Output: [{calls:[REST-body], ok:null|{exists,data:IPC-wire}} |
//          {calls:[REST-body], error:message, code}].
const fs = require('fs');
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
let calls = [], reply, response;
globalThis.fetch = async (url, init) => {
  calls.push(JSON.parse(init.body));
  if (response?.error) throw new TypeError('fetch failed');
  return new Response(response ? response.body : JSON.stringify(reply || {
    commitTime: '2023-01-01T00:00:00Z',
    writeResults: [{ updateTime: '2023-01-01T00:00:00Z' }],
  }), { status: response?.status || 200, statusText: response?.statusText || '' });
};
const { initializeApp } = require('firebase/app');
const sdk = require('firebase/firestore/lite');
sdk.setLogLevel('silent');
const { decode, encode } = new Function(
  'window',
  fs.readFileSync(require('path').join(__dirname, '../../codec.js'), 'utf8')
    + ';return {decode,encode}',
)(globalThis);
const db = sdk.getFirestore(initializeApp({ projectId: 'snowify-dcda0', apiKey: 'test' }));

(async () => {
  const out = [];
  for (const c of input) {
    calls = [];
    reply = c.reply;
    response = c.response;
    try {
      const ref = sdk.doc(db, c.collection || 'users', c.id || 'u');
      let result = null;
      if (c.op === 'get') {
        const snap = await sdk.getDoc(ref);
        result = { exists: snap.exists(), data: encode(structuredClone(snap.data())) };
      } else if (c.op === 'update') {
        await sdk.updateDoc(ref, decode(c.value));
      } else {
        await sdk.setDoc(ref, decode(c.value), { merge: true });
      }
      out.push({ calls, ok: result });
    } catch (e) {
      out.push({ calls, error: e.message, code: e.code });
    }
  }
  process.stdout.write(JSON.stringify(out), () => process.exit(0));
})();
