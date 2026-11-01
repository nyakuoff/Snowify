// Differential runner for src/node.rs: Node 20.18 (Electron 33) path, os.tmpdir, errno map and
// the fs calls the source makes.
// Input:  { path: [[fn, [args...]]], tmpdir: [{VAR: value}], fs: [[op, path]] }
//         fn: posix.join | posix.normalize | win32.join | win32.normalize; op: exists | mkdir | rm
// Output: { path: [string], tmpdir: [string], errors: [[errno, code, desc]], fs: [{ok} | {err: [code, message]}] }
const fs = require('fs');
const os = require('os');
const path = require('path');
const util = require('util');

const input = JSON.parse(fs.readFileSync(0, 'utf8'));

const ENV_VARS = ['TMPDIR', 'TMP', 'TEMP'];
const tmpdir = input.tmpdir.map((env) => {
  for (const k of ENV_VARS) delete process.env[k];
  Object.assign(process.env, env);
  return os.tmpdir();
});

const ops = {
  exists: (p) => fs.existsSync(p),
  mkdir: (p) => { fs.mkdirSync(p, { recursive: true }); return true; },
  rm: (p) => { fs.rmSync(p, { recursive: true, force: true }); return true; },
  rmNoForce: (p) => { fs.rmSync(p, { recursive: true }); return true; },
  read: (p) => fs.readFileSync(p, 'utf-8'),
  write: (p) => { fs.writeFileSync(p, 'written ✓', 'utf-8'); return true; },
  copy: (p, q) => { fs.copyFileSync(p, q); return true; },
  unlink: (p) => { fs.unlinkSync(p); return true; },
  readdir: (p) => fs.readdirSync(p),
  isDir: (p) => fs.statSync(p).isDirectory(),
};

process.stdout.write(JSON.stringify({
  path: input.path.map(([fn, args]) => {
    const [flavour, name] = fn.replace('basenameExt', 'basename').split('.');
    return path[flavour][name](...args);
  }),
  tmpdir,
  errors: [...util.getSystemErrorMap()].filter(([k]) => k < 0 && k > -1000)
    .map(([k, [code, desc]]) => [-k, code, desc]).sort((a, b) => a[0] - b[0]),
  fs: input.fs.map(([op, p, q]) => {
    try { return { ok: ops[op](p, q) }; } catch (e) { return { err: [e.code, e.message, JSON.stringify(e)] }; }
  }),
}));
