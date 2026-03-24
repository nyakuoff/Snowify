// Shared mutable state for the main process.
// All modules import this to read/set mainWindow, ytmusic, etc.
const ctx = {
  mainWindow: null,
  ytmusic: null,
  ytmusicReady: false,
  currentUser: null,
  currentCountry: '',
};

module.exports = ctx;
