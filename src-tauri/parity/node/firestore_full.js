// Full, unmodified Firebase SDK over local AF_UNIX gRPC. No TCP or live Firebase.
// From repo root: ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron \
//   src-tauri/parity/node/firestore_full.js
// --self-test checks proto/fixture encoding without opening a socket.
// --only=<scenario-id> is for diagnosis; partial corpora cannot pass the replay gate.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const crypto = require('crypto');
const grpc = require('@grpc/grpc-js');
const loader = require('@grpc/proto-loader');
const assert = require('assert/strict');
const { performance } = require('perf_hooks');
const { EventEmitter } = require('events');
const net = require('net');
const { encode } = new Function('window', fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8')
  + ';return {encode}',)(globalThis);
const OUTPUT = path.resolve(__dirname, '../corpus/firestore_full.json');
const config = {
  apiKey: 'AIzaSyCNuw8kqgbULTLjC890BzKWvnmdvFCX0og',
  projectId: 'snowify-dcda0',
  appId: '1:925903561467:web:c4298e4f47ac80d1c9c65e',
};
const ROOT = `projects/${config.projectId}/databases/(default)`;
const NAME = `${ROOT}/documents/users/u`;
const STAMP = { seconds: '1700000000', nanos: 0 };
const DOCUMENT = { name: NAME, fields: { value: { stringValue: 'fixture 雪' } },
  createTime: STAMP, updateTime: STAMP };
const BUDGET = 12000; // Real wait, not fake/virtual time; SDK offline timeout is 10 s.

// Read ONLY the SDK's bundled proto literal, not a handwritten approximation.
// This neither patches nor re-evaluates SDK implementation functions. The exact
// same fromJSON options as loadProtos() are used. Fail closed on a bundle change.
const sdkDir = path.dirname(require.resolve('@firebase/firestore'));
const bundles = fs.readdirSync(sdkDir).filter(n => /^common-.*\.node\.cjs\.js$/.test(n));
const bundle = bundles.map(n => path.join(sdkDir, n)).find(p =>
  fs.readFileSync(p, 'utf8').includes('function loadProtos()'));
assert(bundle, 'Cannot locate installed full SDK proto bundle');
const source = fs.readFileSync(bundle, 'utf8');
const start = source.indexOf('const nested = {');
const end = source.indexOf('\nvar protos = {', start);
assert(start >= 0 && end > start, 'SDK proto literal layout changed');
const protoJSON = vm.runInNewContext(`${source.slice(start, end)}; ({nested})`, {}, { timeout: 1000 });
const definitions = loader.fromJSON(protoJSON, { longs: String, enums: String, defaults: true, oneofs: false });
const service = grpc.loadPackageDefinition(definitions).google.firestore.v1.Firestore.service;

// All outbound Auth fetches are replayed; unknown endpoints fail closed.
let clockOffset = 0, activeAuthCalls = [], stage = 'setup', operationOrigin = 0;
const RealDate = Date;
globalThis.Date = class extends RealDate {
  constructor(...args) { super(...(args.length ? args : [RealDate.now() + clockOffset])); }
  static now() { return RealDate.now() + clockOffset; }
};
globalThis.fetch = async (url, init) => {
  const headers = [...new Headers(init.headers)];
  activeAuthCalls.push({ stage, elapsedMs: Math.round(performance.now() - operationOrigin), method: init.method || 'GET', url, headers, body: init.body ?? null });
  let body;
  if (url.startsWith('https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?')) {
    body = { localId: 'u', idToken: 'oracle-token', refreshToken: 'oracle-refresh +/', expiresIn: '3600' };
  } else if (url.startsWith('https://identitytoolkit.googleapis.com/v1/accounts:lookup?')) {
    body = { users: [{ localId: 'u', email: 'oracle@example.invalid', displayName: 'Oracle' }] };
  } else if (url.startsWith('https://securetoken.googleapis.com/v1/token?')) {
    body = { access_token: 'oracle-fresh-token', refresh_token: 'oracle-refresh +/', expires_in: '3600' };
  } else { throw new Error(`Unexpected fetch (live access forbidden): ${url}`); }
  return new Response(JSON.stringify(body), { status: 200 });
};
const appSDK = require('firebase/app');
const authSDK = require('firebase/auth');
const sdk = require('firebase/firestore');
sdk.setLogLevel('silent');

function scenarios() {
  const out = [];
  const statuses = ['NOT_FOUND', 'PERMISSION_DENIED', 'UNAUTHENTICATED', 'UNAVAILABLE',
    'RESOURCE_EXHAUSTED', 'INVALID_ARGUMENT', 'FAILED_PRECONDITION', 'INTERNAL', 'DEADLINE_EXCEEDED'];
  for (const op of ['get', 'set', 'update']) {
    out.push({ id: `${op}-success`, op });
    out.push({ id: `${op}-offline`, op, offline: true });
    for (const auth of ['signed-in', 'refresh']) out.push({ id: `${op}-${auth}`, op, auth });
    for (const status of statuses) {
      // failFor=null: fail persistently; failFor=2: two failures then success.
      for (const failFor of [null, 2]) out.push({
        id: `${op}-${status}-${failFor === null ? 'always' : 'recover'}`, op,
        status, failFor, message: `fixture ${status}: custom 雪`,
      });
    }
  }
  // Listen target rejection is distinct from a failed gRPC stream. Both occur
  // in real Firestore; do not conflate their messages or retry policies.
  for (const status of statuses) out.push({ id: `get-target-${status}`, op: 'get',
    status, failFor: null, targetError: true, message: `fixture ${status}: target 雪` });
  out.push({ id: 'get-missing', op: 'get', missing: true });
  out.push({ id: 'get-reference', op: 'get', reference: true });
  return out;
}
function docFor(s) {
  return s.reference ? { ...DOCUMENT, fields: { value: { referenceValue: `${ROOT}/documents/artists/a` } } } : DOCUMENT;
}
function metadata(call) {
  return Object.fromEntries(Object.entries(call.metadata.getMap()).sort(([a], [b]) => a.localeCompare(b))
    .map(([k, v]) => [k, Buffer.isBuffer(v) ? { base64: v.toString('base64') } : v]));
}
function handlers(s, trace, origin) {
  const attempts = {};
  const record = (kind, call, request) => trace.push({
    kind, elapsedMs: Math.round(performance.now() - origin.started),
    ...(request ? { request } : { metadata: metadata(call) }),
  });
  const fail = kind => {
    attempts[kind] = (attempts[kind] || 0) + 1;
    return s.status && (s.failFor === null || attempts[kind] <= s.failFor);
  };
  const error = () => ({ code: grpc.status[s.status], details: s.message });
  return {
    batchGetDocuments(call) {
      record('BatchGetDocuments', call);
      record('BatchGetDocuments.request', call, call.request);
      if (fail('BatchGetDocuments')) return call.destroy(error());
      call.write(s.missing ? { missing: NAME, readTime: STAMP } : { found: docFor(s), readTime: STAMP });
      call.end();
    },
    commit(call, callback) {
      record('Commit', call);
      record('Commit.request', call, call.request);
      if (fail('Commit')) return callback(error());
      callback(null, { commitTime: STAMP, writeResults: [{ updateTime: STAMP }] });
    },
    listen(call) {
      record('Listen', call);
      call.on('error', () => {});
      call.on('data', req => {
        record('Listen.request', call, req);
        if (!req.addTarget) return;
        const id = req.addTarget.targetId;
        if (fail('Listen')) {
          if (s.targetError) call.write({ targetChange: { targetChangeType: 'REMOVE', targetIds: [id],
            cause: { code: grpc.status[s.status], message: s.message } } });
          else call.destroy(error());
          return;
        }
        call.write({ targetChange: { targetChangeType: 'ADD', targetIds: [id] } });
        if (s.missing) call.write({ documentDelete: { document: NAME, removedTargetIds: [id], readTime: STAMP } });
        else call.write({ documentChange: { document: docFor(s), targetIds: [id] } });
        call.write({ targetChange: { targetChangeType: 'CURRENT', targetIds: [id] } });
        // A snapshot boundary must have empty targetIds and a readTime.
        call.write({ targetChange: { targetChangeType: 'NO_CHANGE', readTime: STAMP } });
      });
      call.on('end', () => call.end());
    },
    write(call) {
      record('Write', call);
      call.on('error', () => {});
      call.on('data', req => {
        record('Write.request', call, req);
        if (!req.writes?.length) {
          if (!req.streamToken?.length) call.write({ streamId: 'fixture-stream', streamToken: Buffer.from('handshake') });
          return;
        }
        // Reject AFTER handshake: a handshake error is always retried, even
        // when a write would be rejected with a permanent error.
        if (fail('Write')) return call.destroy(error());
        call.write({ streamId: 'fixture-stream', streamToken: Buffer.from('written'),
          commitTime: STAMP, writeResults: req.writes.map(() => ({ updateTime: STAMP })) });
      });
      call.on('end', () => call.end());
    },
  };
}
function cloneObservation(data) {
  try { return { status: 'fulfilled', value: structuredClone(data) }; }
  catch (e) { return { status: 'rejected', name: e.name, message: e.message, code: e.code ?? null }; }
}
function readResult(snap, s) {
  const data = snap.data();
  if (!snap.exists()) return { exists: false, data: { $: 'u' } };
  if (s.reference) {
    const ref = data.value;
    return { exists: true, reference: { constructor: ref.constructor.name, type: ref.type,
      path: ref.path, id: ref.id, parentPath: ref.parent.path, ownKeys: Object.keys(ref) },
    structuredClone: cloneObservation(data) };
  }
  return { exists: true, data: encode(data) };
}
async function bounded(promise, ms) {
  let timer;
  try {
    return await Promise.race([
      promise.then(value => ({ status: 'fulfilled', value }),
        e => ({ status: 'rejected', message: e.message, code: e.code ?? null })),
      new Promise(resolve => { timer = setTimeout(() => resolve({ status: 'pending', observedForMs: ms }), ms); }),
    ]);
  } finally { clearTimeout(timer); }
}
async function run(s, index) {
  clockOffset = 0;
  stage = 'sign-in';
  activeAuthCalls = [];
  operationOrigin = performance.now();
  const origin = { started: operationOrigin };
  const dir = fs.mkdtempSync('/tmp/snowify-full-');
  const target = `unix:${dir}/fs.sock`;
  const trace = [];
  const server = new grpc.Server();
  const app = appSDK.initializeApp(config, `firestore-full-${index}`);
  let db;
  try {
    if (!s.offline) {
      server.addService(service, handlers(s, trace, origin));
      await new Promise((resolve, reject) => server.bindAsync(target,
        grpc.ServerCredentials.createInsecure(), e => e ? reject(e) : resolve()));
      // grpc-js 1.9 refuses every stream (GOAWAY) until start() is called.
      server.start();
    }
    if (s.auth) {
      const auth = authSDK.getAuth(app);
      await authSDK.signInWithEmailAndPassword(auth, 'oracle@example.invalid', 'fixture-password');
      if (s.auth === 'refresh') clockOffset = 3600001;
    }
    stage = 'firestore';
    operationOrigin = performance.now();
    origin.started = operationOrigin;
    db = sdk.initializeFirestore(app, { host: target, ssl: false });
    const ref = sdk.doc(db, 'users', 'u');
    const started = performance.now();
    const promise = s.op === 'get' ? sdk.getDoc(ref).then(snap => readResult(snap, s))
      : s.op === 'set' ? sdk.setDoc(ref, { profile: { bio: 'fixture 雪' } }, { merge: true }).then(() => null)
        : sdk.updateDoc(ref, { 'profile.bio': 'fixture 雪' }).then(() => null);
    const outcome = await bounded(promise, BUDGET);
    const elapsedMs = Math.round(performance.now() - started);
    // Capture before terminating: stream teardown is not an operation retry.
    const counts = Object.fromEntries(['Listen', 'Write', 'BatchGetDocuments', 'Commit'].map(method =>
      [method, trace.filter(t => t.kind === method).length]));
    return { id: s.id, outcome, elapsedMs, elapsedVirtualMs: null, counts,
      trace: structuredClone(trace), authCalls: structuredClone(activeAuthCalls) };
  } finally {
    if (db) await sdk.terminate(db);
    server.forceShutdown();
    await appSDK.deleteApp(app);
    fs.rmSync(dir, { recursive: true, force: true });
    clockOffset = 0;
  }
}
function save(corpus) {
  fs.mkdirSync(path.dirname(OUTPUT), { recursive: true });
  fs.writeFileSync(`${OUTPUT}.tmp`, `${JSON.stringify(corpus, null, 2)}\n`);
  fs.renameSync(`${OUTPUT}.tmp`, OUTPUT);
}
function selfTest() {
  // Encode every fixture through the exact SDK service serializers. Exercise
  // handlers without sockets too; this does not count as an SDK observation.
  const roundtrip = (name, value) => service[name].responseDeserialize(service[name].responseSerialize(value));
  for (const s of scenarios()) {
    const trace = [];
    const h = handlers(s, trace, { started: performance.now() });
    const call = new EventEmitter();
    call.metadata = new grpc.Metadata();
    call.end = () => {};
    call.destroy = error => assert.equal(typeof error.code, 'number');
    const request = (method, value) => service[method].requestDeserialize(service[method].requestSerialize(value));
    call.write = msg => { roundtrip('Listen', msg); };
    h.listen(call);
    call.emit('data', request('Listen', { database: ROOT, addTarget: {
      targetId: 2, documents: { documents: [NAME] } } }));
    call.removeAllListeners();
    call.write = msg => { roundtrip('Write', msg); };
    h.write(call);
    call.emit('data', request('Write', { database: ROOT }));
    call.emit('data', request('Write', { streamToken: Buffer.from('x'), writes: [{
      update: { name: NAME, fields: { profile: { mapValue: { fields: { bio: { stringValue: 'fixture 雪' } } } } } },
      updateMask: { fieldPaths: ['profile.bio'] },
    }] }));
    roundtrip('BatchGetDocuments', s.missing ? { missing: NAME, readTime: STAMP }
      : { found: docFor(s), readTime: STAMP });
    roundtrip('Commit', { commitTime: STAMP, writeResults: [{ updateTime: STAMP }] });
  }
  assert.equal(scenarios().length, 77);
  console.log('Full oracle self-test: 77 scenarios; SDK proto fixtures encode successfully (no SDK observations).');
}
(async () => {
  if (process.argv.includes('--self-test')) return selfTest();
  const only = process.argv.find(a => a.startsWith('--only='))?.slice(7);
  const all = scenarios();
  const selected = only ? all.filter(s => s.id === only) : all;
  assert(selected.length, 'No matching scenario');
  const corpus = { schema: 1, status: 'recording', sdk: 'firebase/firestore',
    sdkVersion: require('firebase/app').SDK_VERSION, nodeVersion: process.versions.node,
    bundle: path.basename(bundle), bundleSha256: crypto.createHash('sha256').update(source).digest('hex'),
    clock: { mode: 'bounded-real', budgetMs: BUDGET, virtualTime: false,
      note: 'Real timers and unmodified SDK backoff/jitter. Auth-only Date offset forces expiry; elapsed uses performance.now().' },
    cloneTransport: 'Node structuredClone of full-SDK getDoc().data(); actual Electron IPC not measured',
    scenarios: selected, results: [] };
  // Preflight EVEN for --only=*-offline: an unavailable socket facility must
  // not be mistaken for a Firebase offline result.
  const probeDir = fs.mkdtempSync('/tmp/snowify-full-probe-');
  const probe = net.createServer();
  try {
    await new Promise((resolve, reject) => {
      probe.once('error', reject);
      probe.listen(`${probeDir}/fs.sock`, resolve);
    });
  } catch (e) {
    corpus.status = 'blocked';
    corpus.blocker = { message: e.message, code: e.code ?? null,
      note: 'No observed SDK results. Run this generator outside the sandbox.' };
    save(corpus);
    throw e;
  } finally {
    if (probe.listening) await new Promise(resolve => probe.close(resolve));
    fs.rmSync(probeDir, { recursive: true, force: true });
  }
  for (const [i, s] of selected.entries()) {
    console.error(`[${i + 1}/${selected.length}] ${s.id}`);
    try { corpus.results.push(await run(s, i)); }
    catch (e) {
      corpus.status = 'failed'; corpus.failure = { id: s.id, message: e.message }; save(corpus); throw e;
    }
    save(corpus);
  }
  corpus.status = only ? 'partial' : 'complete';
  save(corpus);
  console.log(`Wrote ${corpus.results.length} full SDK observations to ${OUTPUT}`);
})().then(() => process.exit(0), e => { console.error(e); process.exit(2); });
