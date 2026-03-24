// Shared mutable state for the main process.
// All modules import this to read/set mainWindow, ytmusic, etc.
const ctx = {
  mainWindow: null,
  ytmusic: null,
  ytmusicReady: false,
  currentCountry: '',
  minimizeToTray: false,
  tray: null,
};

module.exports = ctx;
