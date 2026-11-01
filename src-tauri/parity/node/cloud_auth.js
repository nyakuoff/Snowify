// Differential runner for src/cloud_auth.rs: src/main/cloud-auth.js unmodified, with
// `../firebase` and `firebase/auth` replaced by a scripted in-memory Firebase, and electron's
// app.getPath / safeStorage mocked. tests/parity_cloud_auth.rs drives an identical Rust fake.
// Input: { paths: {userData, pictures, downloads, desktop}, keyring: {available},
//          users: [{uid, email, password, displayName, photoURL}], docs: {"coll/id": data},
//          failures: {"get|set|update:coll/id": message}, authErrors: {op: [message...]},
//          now, ops: [[channel, [wire args]] | ["$wait"]] }
// Output: { results, logs: [[level, msg]], sends: [[channel, wire payload]], docs, credentials }
const fs = require('fs');
const path = require('path');
const Module = require('module');
const EventEmitter = require('events');

const input = JSON.parse(fs.readFileSync(0, 'utf8'));
for (const k of ['log', 'warn', 'error']) console[k] = () => {};
const MAIN = path.resolve(__dirname, '../../../src/main');
const codecSrc = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { encode, decode } = new Function('window', `${codecSrc}\nreturn { encode, decode };`)(globalThis);
const RealDate = Date;
globalThis.Date = class extends RealDate {
  constructor(...a) { super(...(a.length ? a : [input.now])); }
  static now() { return input.now; }
};
const tick = () => new Promise((r) => setTimeout(r, 5));

// ─── Fake Firebase (mirrored by FakeBackend in tests/parity_cloud_auth.rs) ───
const auth = { currentUser: null };
const listeners = [];
const users = input.users.map((u) => ({ ...u }));
const authErrors = structuredClone(input.authErrors || {});
const fire = () => { for (const cb of listeners) cb(auth.currentUser); };
const scripted = (op) => { const q = authErrors[op]; if (q && q.length) throw new Error(q.shift()); };
const asUser = (u) => ({ uid: u.uid, email: u.email, displayName: u.displayName ?? null, photoURL: u.photoURL ?? null });
const authModule = {
  async signInWithEmailAndPassword(_a, email, password) {
    await tick();
    scripted('signIn');
    const u = users.find((x) => x.email === email && x.password === password);
    if (!u) throw new Error('Firebase: Error (auth/invalid-credential).');
    auth.currentUser = asUser(u);
    fire();
    return { user: auth.currentUser };
  },
  async createUserWithEmailAndPassword(_a, email, password) {
    await tick();
    scripted('signUp');
    if (users.some((x) => x.email === email)) throw new Error('Firebase: Error (auth/email-already-in-use).');
    const u = { uid: `new-${users.length}`, email, password, displayName: null, photoURL: null };
    users.push(u);
    auth.currentUser = asUser(u);
    fire();
    return { user: auth.currentUser };
  },
  async sendPasswordResetEmail(_a, _email) { await tick(); scripted('reset'); },
};
const docs = structuredClone(input.docs || {});
const failures = input.failures || {};
const fail = (op, ref) => { const m = failures[`${op}:${ref.path}`]; if (m) throw new Error(m); };
const isMap = (v) => Object.prototype.toString.call(v) === '[object Object]';
function deepMerge(target, data) {
  for (const [k, v] of Object.entries(data)) {
    if (isMap(v) && isMap(target[k])) deepMerge(target[k], v);
    else target[k] = structuredClone(v);
  }
}
const firebase = {
  auth,
  db: {},
  doc(_db, coll, id) {
    if (typeof id !== 'string' || !id) throw new Error('Invalid document reference');
    return { path: `${coll}/${id}` };
  },
  async getDoc(ref) {
    await tick();
    fail('get', ref);
    const has = Object.prototype.hasOwnProperty.call(docs, ref.path);
    return { exists: () => has, data: () => (has ? structuredClone(docs[ref.path]) : undefined) };
  },
  async setDoc(ref, data) {
    await tick();
    fail('set', ref);
    if (!docs[ref.path]) docs[ref.path] = {};
    deepMerge(docs[ref.path], data);
  },
  async updateDoc(ref, data) {
    await tick();
    fail('update', ref);
    if (!docs[ref.path]) throw new Error(`No document to update: projects/snowify-dcda0/databases/(default)/documents/${ref.path}`);
    for (const [k, v] of Object.entries(data)) {
      const parts = k.split('.');
      let o = docs[ref.path];
      for (const p of parts.slice(0, -1)) { if (!isMap(o[p])) o[p] = {}; o = o[p]; }
      o[parts[parts.length - 1]] = structuredClone(v);
    }
  },
  async signOut() { await tick(); auth.currentUser = null; fire(); },
  onAuthStateChanged(_a, cb) { listeners.push(cb); setImmediate(() => cb(auth.currentUser)); },
  async updateProfile(user, { displayName }) {
    await tick();
    scripted('profile');
    user.displayName = displayName || null;
  },
};

const sends = [];
const handlers = {};
const electron = {
  app: { getPath: (n) => input.paths[n], getLocale: () => 'en-US' },
  safeStorage: {
    isEncryptionAvailable: () => input.keyring.available,
    encryptString: (s) => Buffer.from(`ENC:${s}`),
    decryptString: (b) => b.toString().slice(4),
  },
  ipcMain: { handle: (ch, fn) => { handlers[ch] = fn; } },
};
const load = Module._load;
Module._load = function (req, parent, ...rest) {
  if (req === 'electron') return electron;
  if (req === 'firebase/auth') return authModule;
  if (req === '../firebase') return firebase;
  return load.call(this, req, parent, ...rest);
};

const { _logBuffer } = require(path.join(MAIN, 'logger'));
// main.js loads the main-process translations at startup.
require(path.join(MAIN, 'i18n')).loadMainTranslations('en');
const ctx = { mainWindow: { webContents: { send: (ch, payload) => sends.push([ch, encode(payload)]) } } };
const rejection = (ch, err) => `Error invoking remote method '${ch}': ${err == null ? 'reply was never sent' : String(err)}`;

// The credentials file, decoded: what a user would get back from it.
function credentials() {
  const p = path.join(input.paths.userData, 'auth_credentials.json');
  if (!fs.existsSync(p)) return null;
  const raw = fs.readFileSync(p, 'utf8');
  try {
    const j = JSON.parse(raw);
    const stored = j.passwordEnc && j.enc === 'safeStorage-v1' ? Buffer.from(j.passwordEnc, 'base64').toString().slice(4) : null;
    return { email: j.email ?? null, stored, legacy: j.password ?? null };
  } catch { return { raw }; }
}

(async () => {
  require(path.join(MAIN, 'cloud-auth')).register(electron.ipcMain, ctx);
  await new Promise((r) => setTimeout(r, 60));
  const results = [];
  for (const [ch, args] of input.ops) {
    try { results.push({ ok: encode(await handlers[ch]({}, ...args.map(decode))) }); } catch (e) { results.push({ err: encode(rejection(ch, e)) }); }
    // Let the auth-state listener's async send land before the next op.
    await new Promise((r) => setTimeout(r, 60));
  }
  process.stdout.write(JSON.stringify({ results, logs: _logBuffer.map((e) => [e.level, e.msg]), sends, docs: encode(docs), credentials: credentials() }), () => process.exit(0));
})();
