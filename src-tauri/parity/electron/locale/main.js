// Ground truth for app.getLocale() under a given environment (prints one JSON line).
const { app } = require('electron');
app.whenReady().then(() => {
  process.stdout.write(JSON.stringify({ locale: app.getLocale(), system: app.getSystemLocale(), preferred: app.getPreferredSystemLanguages() }) + '\n');
  app.exit(0);
});
