const path = require('path');
const fs = require('fs');
const os = require('os');

function getYtDlpPath() {
  const isWin = process.platform === 'win32';
  const isMac = process.platform === 'darwin';
  const binName = isWin ? 'yt-dlp.exe' : 'yt-dlp';

  // macOS: check common install locations (Electron from Finder has limited PATH)
  if (isMac) {
    const macPaths = [
      '/opt/homebrew/bin/yt-dlp',
      '/usr/local/bin/yt-dlp',
      path.join(os.homedir(), '.local/bin/yt-dlp'),
    ];
    try {
      const pyLibDir = path.join(os.homedir(), 'Library', 'Python');
      if (fs.existsSync(pyLibDir)) {
        fs.readdirSync(pyLibDir)
          .filter(d => /^\d+\.\d+$/.test(d))
          .sort((a, b) => parseFloat(b) - parseFloat(a))
          .forEach(v => macPaths.push(path.join(pyLibDir, v, 'bin', 'yt-dlp')));
      }
    } catch (_) {}
    for (const p of macPaths) {
      if (fs.existsSync(p)) return p;
    }
    return binName;
  }

  const subDir = isWin ? 'win' : 'linux';
  const bundled = path.join(process.resourcesPath, 'bin', subDir, binName);
  if (fs.existsSync(bundled)) return bundled;
  const dev = path.join(__dirname, '..', '..', 'bin', subDir, binName);
  if (fs.existsSync(dev)) return dev;
  return binName;
}

module.exports = { getYtDlpPath };
