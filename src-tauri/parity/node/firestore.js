// Runs the real Firestore Lite parser, REST transport and decoder under Electron.
// Input: [{op:'get'|'set'|'update', collection?, id?, value?:IPC-wire, reply?:JSON}].
// Fetch is entirely offline; reply overrides the default successful commit response.
// Output: [{calls:[REST-body], ok:null|{exists,data:IPC-wire}} |
//          {calls:[REST-body], error:message, code}].
const fs = require('fs');
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
let calls = [], reply;
globalThis.fetch = async (url, init) => {
  calls.push(JSON.parse(init.body));
  return new Response(JSON.stringify(reply || {
    commitTime: '2023-01-01T00:00:00Z',
    writeResults: [{ updateTime: '2023-01-01T00:00:00Z' }],
  }), { status: 200 });
};
const { initializeApp } = require('firebase/app');
const sdk = require('firebase/firestore/lite');
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
