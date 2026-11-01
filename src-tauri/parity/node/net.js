// Real https.get failures under Electron 33's Node, for src/real_host.rs's error mapping.
// Input: [url]. Output: [{ message, props: JSON.stringify(err) } | { sync: message } | { ok: status }]
const fs = require('fs');
const https = require('https');
const urls = JSON.parse(fs.readFileSync(0, 'utf8'));
(async () => {
  const out = [];
  for (const u of urls) {
    out.push(await new Promise((resolve) => {
      try {
        const req = https.get(u, { headers: { 'User-Agent': 'Snowify' } }, (res) => { res.resume(); resolve({ ok: res.statusCode }); });
        req.on('error', (e) => resolve({ message: e.message, props: JSON.stringify(e) }));
        req.setTimeout(15000, () => { req.destroy(); resolve({ message: 'Timeout' }); });
      } catch (e) {
        resolve({ sync: e.message });
      }
    }));
  }
  process.stdout.write(JSON.stringify(out));
})();
