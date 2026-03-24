const path = require('path');
const { nativeImage } = require('electron');
const { mt } = require('./i18n');

const thumbIcons = process.platform === 'win32' ? (() => {
  const load = name => nativeImage.createFromPath(
    path.join(__dirname, '..', '..', 'assets', 'thumbbar', `${name}.png`)
  );
  return { prev: load('prev'), play: load('play'), pause: load('pause'), next: load('next') };
})() : null;

function updateThumbarButtons(win, isPlaying) {
  if (process.platform !== 'win32' || !win) return;
  win.setThumbarButtons([
    { tooltip: mt('player.previous'), icon: thumbIcons.prev, click: () => win.webContents.send('thumbar:prev') },
    { tooltip: mt(isPlaying ? 'player.pause' : 'player.play'), icon: isPlaying ? thumbIcons.pause : thumbIcons.play, click: () => win.webContents.send('thumbar:playPause') },
    { tooltip: mt('player.next'), icon: thumbIcons.next, click: () => win.webContents.send('thumbar:next') }
  ]);
}

function register(ipcMain, ctx) {
  ipcMain.on('thumbar:updateState', (_event, isPlaying) => {
    updateThumbarButtons(ctx.mainWindow, isPlaying);
  });
}

module.exports = { updateThumbarButtons, register };
