// Differential runner for src/node/child_process.rs: Node's own execFile.
// Input:  { cases: [{ file, args: [wire], timeout, killAfter }] }
// Output: [{ threw: err } | { err: err | null, stdout, stderr }], err = { string, props }
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const codecSrc = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { encode, decode } = new Function('window', `${codecSrc}\nreturn { encode, decode };`)(globalThis);
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
const show = (e) => e && { string: String(e), props: encode(Object.fromEntries(Object.keys(e).map((k) => [k, e[k]]))) };
(async () => {
  const out = [];
  for (const c of input.cases) {
    out.push(await new Promise((resolve) => {
      let proc;
      try {
        proc = execFile(c.file, c.args.map(decode), { timeout: c.timeout }, (err, stdout, stderr) => resolve({ err: show(err), stdout, stderr }));
      } catch (e) { return resolve({ threw: show(e) }); }
      if (c.killAfter) setTimeout(() => proc.kill('SIGTERM'), c.killAfter);
    }));
  }
  process.stdout.write(JSON.stringify(out), () => process.exit(0));
})();
