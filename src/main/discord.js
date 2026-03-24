const { Client } = require('@xhayper/discord-rpc');

const DISCORD_CLIENT_ID = '1473620585832517644';
let rpcClient = null;
let rpcReady = false;

async function connectDiscordRPC() {
  if (rpcClient) return;
  try {
    rpcClient = new Client({ clientId: DISCORD_CLIENT_ID });
    rpcClient.on('ready', () => { rpcReady = true; });
    rpcClient.on('disconnected', () => { rpcReady = false; rpcClient = null; });
    await rpcClient.login();
  } catch (_) {
    rpcReady = false;
    rpcClient = null;
  }
}

function disconnectDiscordRPC() {
  if (rpcClient) {
    rpcClient.destroy().catch(() => {});
    rpcClient = null;
    rpcReady = false;
  }
}

function register(ipcMain) {
  ipcMain.handle('discord:connect', async () => {
    await connectDiscordRPC();
    return rpcReady;
  });

  ipcMain.handle('discord:disconnect', async () => {
    disconnectDiscordRPC();
  });

  ipcMain.handle('discord:updatePresence', async (_event, data) => {
    if (!rpcClient || !rpcReady) return;
    try {
      await rpcClient.user?.setActivity({
        type: 2,
        details: data.title || 'Unknown',
        state: data.artist || 'Unknown Artist',
        largeImageKey: data.thumbnail || 'logo',
        largeImageText: data.title || 'Snowify',
        smallImageKey: 'logo',
        smallImageText: 'Snowify',
        startTimestamp: data.startTimestamp ? new Date(data.startTimestamp) : undefined,
        endTimestamp: data.endTimestamp ? new Date(data.endTimestamp) : undefined,
        buttons: [
          { label: 'Get Snowify', url: 'https://snowify.cc' },
          ...(data.videoId ? [{ label: 'Listen on Snowify', url: `https://snowify.cc/track/${data.videoId}` }] : [])
        ],
        instance: false
      });
    } catch (_) {}
  });

  ipcMain.handle('discord:clearPresence', async () => {
    if (!rpcClient || !rpcReady) return;
    try { await rpcClient.user?.clearActivity(); } catch (_) {}
  });
}

module.exports = { connectDiscordRPC, disconnectDiscordRPC, register };
