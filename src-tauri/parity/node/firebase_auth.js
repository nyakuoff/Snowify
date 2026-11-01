// Runs the real Firebase Auth Node SDK under Electron with offline fetch replay.
// Input: [{ fetch: {urlPrefix: [response, ...]}, ops: [[operation, ...args], ...] }].
// Responses contain status/body or error:true; the final response repeats.
// Output: [{ calls: [[method,url,headers,body]], events: [user|null],
//            results: [{ok:value}|{error:message,code}] }].
console.log = (...args) => console.error(...args);
const fs = require('fs');
const RealDate = Date;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [1700000000000]));
  }
  static now() {
    return 1700000000000;
  }
};
const scenarios = JSON.parse(fs.readFileSync(0, 'utf8'));
const { initializeApp, deleteApp } = require('firebase/app');
let mockFetch;
globalThis.fetch = (...args) => mockFetch(...args);
const sdk = require('firebase/auth');
require('firebase/firestore');
const config = {
  apiKey: 'AIzaSyCNuw8kqgbULTLjC890BzKWvnmdvFCX0og',
  projectId: 'snowify-dcda0',
  appId: '1:925903561467:web:c4298e4f47ac80d1c9c65e',
};
const user = u => u
  ? Object.fromEntries(['uid', 'email', 'displayName', 'photoURL'].map(k => [k, u[k]]))
  : null;

(async () => {
  const out = [];
  for (const s of scenarios) {
    const calls = [], events = [], results = [];
    const queues = structuredClone(s.fetch);
    mockFetch = async (url, init) => {
      calls.push([init.method || 'GET', url, [...new Headers(init.headers)], init.body ?? null]);
      const key = Object.keys(queues).find(k => url.startsWith(k));
      const q = queues[key];
      const r = (q?.length > 1 ? q.shift() : q?.[0]) || { status: 404, body: '' };
      if (r.error) throw new TypeError('fetch failed');
      return new Response(r.body, { status: r.status || 200 });
    };
    // SDK inMemoryPersistence is process-global and survives deleteApp. Unique app names
    // isolate scenarios as independent app lifetimes, without changing persistence itself.
    const app = initializeApp(config, `parity-${out.length}`);
    const auth = sdk.getAuth(app);
    const unsub = sdk.onAuthStateChanged(auth, u => events.push(user(u)));
    await auth.authStateReady();
    for (const op of s.ops) {
      try {
        let result;
        switch (op[0]) {
          case 'signIn':
            result = user((await sdk.signInWithEmailAndPassword(auth, ...op.slice(1))).user);
            break;
          case 'signUp':
            result = user((await sdk.createUserWithEmailAndPassword(auth, ...op.slice(1))).user);
            break;
          case 'reset':
            await sdk.sendPasswordResetEmail(auth, op[1]);
            result = null;
            break;
          case 'out':
            await sdk.signOut(auth);
            result = null;
            break;
          case 'profile':
            await sdk.updateProfile(auth.currentUser, op[1]);
            result = user(auth.currentUser);
            break;
          case 'token':
            result = await auth.currentUser.getIdToken(op[1]);
            break;
          case 'current':
            result = user(auth.currentUser);
            break;
        }
        results.push({ ok: result });
      } catch (e) {
        results.push({ error: e.message, code: e.code });
      }
      await Promise.resolve();
    }
    out.push({ calls, events, results });
    unsub();
    await deleteApp(app);
  }
  process.stdout.write(JSON.stringify(out), () => process.exit(0));
})().catch(e => {
  console.error(e);
  process.exit(1);
});
