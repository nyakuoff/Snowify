// Run with GUI Electron, never ELECTRON_RUN_AS_NODE. Uses ONLY a fresh temp profile.
const fs = require('fs');
const os = require('os');
const path = require('path');
const worker = process.argv.indexOf('--fixture-worker');
if (worker < 0) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'snowify-ls-'));
  if (!process.env.ELECTRON_RUN_AS_NODE) require('electron').app.setPath('userData', path.join(temp,'coordinator'));
  const env = {...process.env}; delete env.ELECTRON_RUN_AS_NODE;
  const child = require('child_process').spawnSync(process.execPath,
    [__filename, '--fixture-worker', temp, '--user-data-dir='+path.join(temp,'profile')],
    {env, stdio:'inherit'});
  let status=child.status ?? 1;
  if (status===0) {
    // Worker has fully exited: no database writer remains when taking the fixture.
    const output=path.resolve(__dirname,'../corpus/local_storage');
    const db=path.join(temp,'profile','Local Storage','leveldb');
    const files=fs.readdirSync(db);
    if (!files.some(f=>/\.(ldb|sst)$/.test(f))) throw Error('fixture did not flush to SST');
    fs.mkdirSync(output,{recursive:true});
    fs.rmSync(path.join(output,'leveldb'),{recursive:true,force:true});
    fs.cpSync(db,path.join(output,'leveldb'),{recursive:true,filter:s=>!/^(LOCK|LOG.*)$/.test(path.basename(s))});
    // Gzipped: the 5 MiB value as code-unit arrays is ~20 MB of JSON.
    fs.writeFileSync(path.join(output,'expected.json.gz'),require('zlib').gzipSync(fs.readFileSync(path.join(temp,'expected.json'))));
    fs.writeFileSync(path.join(output,'provenance.json'),JSON.stringify({electron:process.versions.electron,files},null,2));
    console.log('local_storage fixture: '+output);
  }
  fs.rmSync(temp,{recursive:true,force:true});
  if (process.env.ELECTRON_RUN_AS_NODE) process.exit(status);
  else require('electron').app.exit(status);
} else {
const {app, BrowserWindow} = require('electron');
const temp=process.argv[worker+1];
app.setPath('userData', path.join(temp, 'profile'));
app.commandLine.appendSwitch('disable-gpu');
// The 5MiB string plus churn deliberately exceeds the ordinary localStorage quota.
app.commandLine.appendSwitch('unlimited-storage');
const page = path.join(temp, 'fixture.html');
fs.writeFileSync(page, '<!doctype html><meta charset="utf-8">');
let expected;
// Destroying the writer window must not quit the app before the reader window loads.
app.on('window-all-closed', () => {});
app.whenReady().then(async () => {
  const win = new BrowserWindow({show:false, webPreferences:{contextIsolation:true}});
  await win.loadFile(page);
  await win.webContents.executeJavaScript(`(async () => {
    localStorage.clear();
    const pairs = [ ['snowify_state','{"playlists":[]}'], ['snowfy_queue','legacy'],
      ['ascii','quote" slash\\\\ newline\\n </script>'], ['latin-é','éÿ\\x80'],
      ['日本語','音楽'], ['emoji-😀','🎵'], ['lone-\\ud800','\\ud800X\\udfff'],
      ['', ''], ['empty',''], ['nul\\0key','nul\\0value'], ['__proto__','safe'],
      ['large', 'x'.repeat(5*1024*1024)] ];
    for (const [k,v] of pairs) localStorage.setItem(k,v);
  })()`);
  // Explicit flushes prevent Chromium's delayed commit from coalescing the churn.
  for (let round=0;round<24;round++) {
    await win.webContents.executeJavaScript(`localStorage.setItem('churn', '${round}:'+String.fromCharCode(${65}+${round}).repeat(1024*1024)); localStorage.setItem('overwritten','old-${round}'); localStorage.setItem('removed','gone-${round}');`);
    win.webContents.session.flushStorageData();
    await new Promise(r=>setTimeout(r,250));
  }
  await win.webContents.executeJavaScript(`localStorage.removeItem('churn'); localStorage.removeItem('removed'); localStorage.setItem('overwritten','final');`);
  win.webContents.session.flushStorageData();
  await new Promise(r=>setTimeout(r,1000));
  win.destroy();
  const read = new BrowserWindow({show:false});
  await read.loadFile(page);
  // JSON.stringify in V8 preserves lone surrogates; store code units for Rust serde_json.
  expected = await read.webContents.executeJavaScript(`JSON.stringify(Array.from({length:localStorage.length},(_,i)=>{const k=localStorage.key(i);const units=s=>Array.from({length:s.length},(_,j)=>s.charCodeAt(j));return [units(k),units(localStorage.getItem(k))]}))`);
  read.destroy();
  fs.writeFileSync(path.join(temp,'expected.json'),expected);
  app.quit();
}).catch(e => { console.error(e); app.exit(1); });
}
