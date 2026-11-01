// Same shapes as src/preload.js: invoke-style, send-style, and on*-style members.
const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('probe', {
  echo: (...args) => ipcRenderer.invoke('echo', ...args),
  echoArity: (...args) => ipcRenderer.invoke('echoArity', ...args),
  throwAt: (i) => ipcRenderer.invoke('throw', i),
  send: (...args) => ipcRenderer.send('send', ...args),
  emitAll: () => ipcRenderer.invoke('emitAll'),
  onEv: (cb) => { ipcRenderer.on('ev', (_e, i, v) => cb(i, v)); },
  onRaw: (cb) => ipcRenderer.on('evDone', cb),
  onEvFail: (cb) => { ipcRenderer.on('evFail', (_e, i, m) => cb(i, m)); },
  missing: () => ipcRenderer.invoke('no:such:channel', 1),
  done: (results) => ipcRenderer.invoke('done', results),
});
