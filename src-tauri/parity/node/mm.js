// Differential runner for src/mm: music-metadata 7.14's parseFile(path, { duration: true }),
// reduced to what media.js parseLocalFile reads. Input: [path]. Output per path:
// { err: true } or { ok: { title, artist, album (wire values), duration (String), picture } }.
const fs = require('fs');
const path = require('path');
const mm = require(path.resolve(__dirname, '../../../node_modules/music-metadata'));
const codecSrc = fs.readFileSync(path.join(__dirname, '../../codec.js'), 'utf8');
const { encode } = new Function('window', `${codecSrc}\nreturn { encode };`)(globalThis);
process.on('unhandledRejection', () => {}); // ID3v1Parser fires an un-awaited ignore()

const paths = JSON.parse(fs.readFileSync(0, 'utf8'));
(async () => {
  const out = [];
  for (const p of paths) {
    try {
      const m = await mm.parseFile(p, { duration: true });
      const c = m.common || {};
      const pic = c.picture && c.picture[0];
      out.push({
        ok: {
          title: encode(c.title), artist: encode(c.artist), album: encode(c.album),
          duration: String(m.format.duration),
          picture: pic ? { format: pic.format, data: Buffer.from(pic.data).toString('base64') } : null,
        },
      });
    } catch {
      out.push({ err: true });
    }
  }
  process.stdout.write(JSON.stringify(out), () => process.exit(0));
})();
