// Real Electron 33 app APIs: intentionally NOT ELECTRON_RUN_AS_NODE.
const { app } = require('electron');
const fs = require('fs');
const [inputFile, output] = process.argv.slice(-2);
const input = JSON.parse(fs.readFileSync(inputFile, 'utf8'));
fs.mkdirSync(input.userData, { recursive: true });
app.setPath('userData', input.userData);
app.setName('Snowify parity app name'); // AUMID, not this name, supplies the default value name.
app.setAppUserModelId(input.appId);
app.disableHardwareAcceleration();
app.commandLine.appendSwitch('disable-background-networking');
app.whenReady().then(() => {
  for (const settings of input.sets) app.setLoginItemSettings(settings);
  fs.writeFileSync(output, JSON.stringify({ exe: process.execPath, settings: input.gets.map(options => app.getLoginItemSettings(options)) }));
  app.exit(0);
}).catch(error => { fs.writeFileSync(output, JSON.stringify({ error: String(error) })); app.exit(1); });
