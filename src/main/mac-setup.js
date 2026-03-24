const path = require('path');
const fs = require('fs');
const { app, dialog, BrowserWindow, shell } = require('electron');
const { execFileSync, spawn } = require('child_process');
const { mt } = require('./i18n');
const { getYtDlpPath } = require('./ytdlp');

async function checkMacYtDlp(mainWindow) {
  if (process.platform !== 'darwin') return;

  const ytdlp = getYtDlpPath();
  try { execFileSync(ytdlp, ['--version'], { stdio: 'ignore', timeout: 5000 }); return; } catch (_) {}

  function verifyYtDlp() {
    try { execFileSync(getYtDlpPath(), ['--version'], { stdio: 'ignore', timeout: 5000 }); return true; } catch (_) { return false; }
  }

  function runBrewInstall() {
    return new Promise((resolve) => {
      const progressWin = new BrowserWindow({
        width: 480, height: 260, parent: mainWindow, modal: true,
        resizable: false, minimizable: false, maximizable: false,
        show: true, frame: false, transparent: false, backgroundColor: '#0a0a0a',
        webPreferences: { nodeIntegration: false, contextIsolation: true }
      });

      progressWin.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(`<!DOCTYPE html>
<html><head><style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    background: #121212; color: #b3b3b3; padding: 24px; display: flex;
    flex-direction: column; height: 100vh; -webkit-app-region: drag; user-select: none;
    border-radius: 16px; overflow: hidden; }
  h2 { font-size: 15px; font-weight: 600; margin-bottom: 6px; color: #fff; }
  .status { font-size: 13px; color: #b3b3b3; margin-bottom: 16px; }
  .spinner { display: inline-block; width: 14px; height: 14px;
    border: 2px solid rgba(255,255,255,0.1); border-top-color: #aa55e6;
    border-radius: 50%; animation: spin 0.8s linear infinite; vertical-align: middle; margin-right: 8px; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .log-toggle { -webkit-app-region: no-drag; background: transparent; border: 1px solid rgba(255,255,255,0.1);
    color: #b3b3b3; font-size: 12px; padding: 4px 12px; border-radius: 10px;
    cursor: pointer; margin-bottom: 10px; align-self: flex-start; transition: all 0.15s; }
  .log-toggle:hover { border-color: #aa55e6; color: #fff; }
  .log-area { flex: 1; background: #0a0a0a; border-radius: 10px; padding: 10px;
    font-family: "SF Mono", Menlo, monospace; font-size: 11px; color: #666;
    overflow-y: auto; white-space: pre-wrap; word-break: break-all;
    display: none; min-height: 0; border: 1px solid rgba(255,255,255,0.06); }
  .log-area.visible { display: block; }
  .done { color: #aa55e6; } .fail { color: #e74c3c; }
</style></head><body>
  <h2><span class="spinner" id="spinner"></span>${mt('dialog.installingYtdlp')}</h2>
  <p class="status" id="status">${mt('dialog.brewRunning')}</p>
  <button class="log-toggle" id="logBtn" onclick="toggleLogs()">${mt('dialog.showLogs')}</button>
  <div class="log-area" id="logs"></div>
  <script>
    const _i18n = { hideLogs: '${mt('dialog.hideLogs').replace(/'/g, "\\'")}', showLogs: '${mt('dialog.showLogs').replace(/'/g, "\\'")}' };
    function toggleLogs() {
      const el = document.getElementById('logs'); const btn = document.getElementById('logBtn');
      const visible = el.classList.toggle('visible');
      btn.textContent = visible ? _i18n.hideLogs : _i18n.showLogs;
    }
    function addLog(text) { const el = document.getElementById('logs'); el.textContent += text; el.scrollTop = el.scrollHeight; }
    function setDone(ok, msg) {
      document.getElementById('spinner').style.display = 'none';
      const st = document.getElementById('status'); st.textContent = msg; st.className = 'status ' + (ok ? 'done' : 'fail');
    }
  </script>
</body></html>`));

      progressWin.webContents.once('did-finish-load', () => {
        const child = spawn('brew', ['install', 'yt-dlp'], {
          env: { ...process.env, PATH: '/opt/homebrew/bin:/usr/local/bin:' + (process.env.PATH || '') },
          stdio: ['ignore', 'pipe', 'pipe']
        });
        const sendLog = (data) => {
          const text = data.toString().replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/\n/g, '\\n');
          try { progressWin.webContents.executeJavaScript(`addLog('${text}')`); } catch (_) {}
        };
        child.stdout.on('data', sendLog);
        child.stderr.on('data', sendLog);
        const timeout = setTimeout(() => { try { child.kill(); } catch (_) {} }, 120000);
        child.on('close', (code) => {
          clearTimeout(timeout);
          const ok = verifyYtDlp();
          const msg = ok ? mt('dialog.ytdlpInstalled') : mt('dialog.ytdlpInstallFailed', { code });
          try { progressWin.webContents.executeJavaScript(`setDone(${ok}, '${msg.replace(/'/g, "\\'")}')`); } catch (_) {}
          setTimeout(() => { try { progressWin.close(); } catch (_) {} resolve(ok); }, ok ? 1500 : 4000);
        });
        child.on('error', () => {
          clearTimeout(timeout);
          try { progressWin.webContents.executeJavaScript(`setDone(false, '${mt('dialog.brewFailed').replace(/'/g, "\\'")}')`); } catch (_) {}
          setTimeout(() => { try { progressWin.close(); } catch (_) {} resolve(false); }, 4000);
        });
      });
    });
  }

  const hasBrew = (() => { try { execFileSync('which', ['brew'], { stdio: 'ignore' }); return true; } catch (_) { return false; } })();
  if (hasBrew) {
    const { response } = await dialog.showMessageBox(mainWindow, {
      type: 'info', title: mt('dialog.firstTimeSetup'), message: mt('dialog.installingYtdlp'),
      detail: mt('dialog.ytdlpRequired'), buttons: [mt('dialog.install'), mt('modal.cancel')], defaultId: 0, noLink: true
    });
    if (response === 0) {
      const ok = await runBrewInstall();
      if (ok) return;
      await dialog.showMessageBox(mainWindow, {
        type: 'error', title: mt('dialog.installFailed'), message: mt('dialog.installFailedMsg'),
        detail: mt('dialog.installFailedDetail'), buttons: [mt('modal.ok')]
      });
    }
    return;
  }

  const { response } = await dialog.showMessageBox(mainWindow, {
    type: 'warning', title: mt('dialog.ytdlpNotFound'), message: mt('dialog.setupRequired'),
    detail: mt('dialog.ytdlpManualInstall'), buttons: [mt('dialog.openBrewSh'), mt('modal.ok')], defaultId: 1, noLink: true
  });
  if (response === 0) shell.openExternal('https://brew.sh');
}

module.exports = { checkMacYtDlp };
