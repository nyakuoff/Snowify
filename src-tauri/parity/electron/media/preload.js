const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('probe', { done: (r) => ipcRenderer.invoke('done', r) });
contextBridge.exposeInMainWorld('__MEDIA_FIXTURES__', JSON.parse(process.env.SNOWIFY_MEDIA_FIXTURES));
