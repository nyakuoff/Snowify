const { app, dialog, shell } = require('electron');
const fs = require('fs');
const path = require('path');
const mm = require('music-metadata');
const { _logBuffer, _captureLog } = require('./logger');
const { loadMainTranslations } = require('./i18n');
const { updateThumbarButtons } = require('./thumbbar');
const { mapSongToTrack } = require('./ytmusic');

// ─── CSV helpers ───

function parseCsvLine(line) {
  const fields = [];
  let current = '';
  let inQuotes = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (inQuotes) {
      if (ch === '"') {
        if (i + 1 < line.length && line[i + 1] === '"') { current += '"'; i++; }
        else inQuotes = false;
      } else current += ch;
    } else {
      if (ch === '"') inQuotes = true;
      else if (ch === ',') { fields.push(current); current = ''; }
      else current += ch;
    }
  }
  fields.push(current);
  return fields;
}

// ─── Spotify match scoring ───

function normalizeStr(s) {
  return (s || '').toLowerCase()
    .normalize('NFD').replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9 ]/g, ' ')
    .replace(/\s+/g, ' ').trim();
}

function tokenize(s) { return normalizeStr(s).split(' ').filter(Boolean); }

function tokenSimilarity(a, b) {
  const tA = new Set(tokenize(a));
  const tB = new Set(tokenize(b));
  if (!tA.size && !tB.size) return 1;
  if (!tA.size || !tB.size) return 0;
  let inter = 0;
  for (const t of tA) if (tB.has(t)) inter++;
  return inter / (tA.size + tB.size - inter);
}

const UNWANTED_TAGS = ['instrumental', 'karaoke', 'cover', '8d', '8d audio', 'remix', 'slowed', 'sped up', 'reverb', 'nightcore', 'acoustic', 'live'];

function scoreMatch(song, targetTitle, targetArtist) {
  const sTitle = song.name || '';
  const sArtist = song.artist?.name || '';
  const titleScore = tokenSimilarity(targetTitle, sTitle);

  let artistScore = tokenSimilarity(targetArtist, sArtist);
  if (song.artists && Array.isArray(song.artists)) {
    for (const a of song.artists) {
      const s = tokenSimilarity(targetArtist, a?.name || '');
      if (s > artistScore) artistScore = s;
    }
  }

  const normTarget = normalizeStr(targetTitle);
  const normResult = normalizeStr(sTitle);
  let penalty = 0;

  for (const tag of UNWANTED_TAGS) {
    if (normResult.includes(tag) && !normTarget.includes(tag)) penalty += 0.5;
  }
  const normAlbum = normalizeStr(song.album?.name || '');
  for (const tag of UNWANTED_TAGS) {
    if (normAlbum.includes(tag) && !normTarget.includes(tag)) penalty += 0.2;
  }
  const resultExtra = normResult.replace(normTarget, '').trim();
  if (resultExtra.length > 0 && normTarget.length > 0) {
    const extraTokens = resultExtra.split(' ').filter(Boolean);
    const targetTokens = normTarget.split(' ').filter(Boolean);
    penalty += 0.15 * (extraTokens.length / Math.max(targetTokens.length, 1));
  }
  if (normResult === normTarget) penalty -= 0.2;

  if (artistScore === 0 && normalizeStr(targetArtist).length > 0) penalty += 0.8;
  else if (artistScore < 0.3 && normalizeStr(targetArtist).length > 0) penalty += 0.3;
  if (artistScore >= 0.8) penalty -= 0.25;

  return (titleScore * 0.4) + (artistScore * 0.6) - penalty;
}

// ─── Playlist covers dir ───

function getCoversDir() {
  const dir = path.join(app.getPath('userData'), 'playlist-covers');
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function register(ipcMain, ctx) {
  // Window controls
  ipcMain.on('window:minimize', () => ctx.mainWindow?.minimize());
  ipcMain.on('window:maximize', () => {
    if (ctx.mainWindow?.isMaximized()) ctx.mainWindow.unmaximize();
    else ctx.mainWindow?.maximize();
  });
  ipcMain.on('window:close', () => ctx.mainWindow?.close());

  ipcMain.handle('shell:openExternal', async (_event, url) => {
    if (typeof url === 'string' && (url.startsWith('https://') || url.startsWith('http://'))) {
      await shell.openExternal(url);
    }
  });

  // App info + locale
  ipcMain.handle('app:getLogs', () => [..._logBuffer]);
  ipcMain.handle('app:appendLog', (_event, entry) => { _captureLog(entry.level || 'log', [entry.msg || '']); });
  ipcMain.handle('app:setLocale', (_event, locale) => {
    loadMainTranslations(locale);
    updateThumbarButtons(ctx.mainWindow, false);
  });

  // Spotify CSV import
  ipcMain.handle('spotify:pickCsv', async () => {
    const result = await dialog.showOpenDialog(ctx.mainWindow, {
      title: 'Select Spotify CSV export files',
      filters: [{ name: 'CSV Files', extensions: ['csv'] }],
      properties: ['openFile', 'multiSelections']
    });
    if (result.canceled || !result.filePaths.length) return null;

    const playlists = [];
    for (const filePath of result.filePaths) {
      try {
        const raw = fs.readFileSync(filePath, 'utf-8');
        const lines = raw.split(/\r?\n/).filter(l => l.trim());
        if (lines.length < 2) continue;
        const headers = parseCsvLine(lines[0]).map(h => h.trim());
        const titleIdx = headers.findIndex(h => /^(track.?name|title|song.?name|name)$/i.test(h));
        const artistIdx = headers.findIndex(h => /^(artist.?name|artists?\(?s?\)?|artist)$/i.test(h));
        if (titleIdx === -1) continue;
        const tracks = [];
        for (let i = 1; i < lines.length; i++) {
          const fields = parseCsvLine(lines[i]);
          const title = fields[titleIdx]?.trim();
          if (!title) continue;
          const artist = artistIdx !== -1 ? (fields[artistIdx]?.replace(/\\,/g, ', ').trim() || 'Unknown Artist') : 'Unknown Artist';
          tracks.push({ title, artist });
        }
        playlists.push({ name: path.basename(filePath, '.csv').replace(/_/g, ' '), tracks });
      } catch (err) { console.error(`Error parsing CSV ${filePath}:`, err.message); }
    }
    return playlists.length ? playlists : null;
  });

  ipcMain.handle('spotify:matchTrack', async (_event, title, artist) => {
    try {
      const songs = await ctx.ytmusic.searchSongs(`${title} ${artist}`);
      let candidates = songs.filter(s => s.videoId);
      if (candidates.length > 0) {
        const topArtistScore = Math.max(...candidates.slice(0, 5).map(s => {
          let best = tokenSimilarity(artist, s.artist?.name || '');
          if (s.artists && Array.isArray(s.artists)) {
            for (const a of s.artists) { const sc = tokenSimilarity(artist, a?.name || ''); if (sc > best) best = sc; }
          }
          return best;
        }));
        if (topArtistScore < 0.5) {
          try {
            const fallback = await ctx.ytmusic.searchSongs(`${artist} ${title}`);
            const extra = fallback.filter(s => s.videoId);
            const seenIds = new Set(candidates.map(c => c.videoId));
            for (const s of extra) { if (!seenIds.has(s.videoId)) { candidates.push(s); seenIds.add(s.videoId); } }
          } catch (_) { /* ignore */ }
        }
      }
      if (!candidates.length) return null;
      let bestSong = candidates[0], bestScore = -Infinity;
      for (const song of candidates) {
        const score = scoreMatch(song, title, artist);
        if (score > bestScore) { bestScore = score; bestSong = song; }
      }
      return mapSongToTrack(bestSong);
    } catch (err) { console.error('Spotify match error:', err); return null; }
  });

  // Playlist export CSV
  ipcMain.handle('playlist:exportCsv', async (_event, name, tracks) => {
    const result = await dialog.showSaveDialog(ctx.mainWindow, {
      title: 'Export playlist as CSV',
      defaultPath: name.replace(/[/\\?%*:|"<>]/g, '_') + '.csv',
      filters: [{ name: 'CSV Files', extensions: ['csv'] }]
    });
    if (result.canceled || !result.filePath) return false;
    const escCsv = v => {
      const s = String(v ?? '');
      return s.includes(',') || s.includes('"') || s.includes('\n') ? '"' + s.replace(/"/g, '""') + '"' : s;
    };
    const header = 'Title,Artist,Album,Duration';
    const rows = tracks.map(t => {
      const dur = t.durationMs ? `${Math.floor(t.durationMs / 60000)}:${String(Math.floor((t.durationMs % 60000) / 1000)).padStart(2, '0')}` : '';
      return [escCsv(t.title), escCsv(t.artist), escCsv(t.album), dur].join(',');
    });
    fs.writeFileSync(result.filePath, [header, ...rows].join('\n'), 'utf-8');
    return true;
  });

  // Library export/import
  ipcMain.handle('library:export', async (_event, jsonStr) => {
    const result = await dialog.showSaveDialog(ctx.mainWindow, {
      title: 'Export Snowify Library',
      defaultPath: `snowify-library-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: 'JSON', extensions: ['json'] }]
    });
    if (result.canceled || !result.filePath) return false;
    fs.writeFileSync(result.filePath, jsonStr, 'utf-8');
    return true;
  });

  ipcMain.handle('library:import', async () => {
    const result = await dialog.showOpenDialog(ctx.mainWindow, {
      title: 'Import Snowify Library',
      filters: [{ name: 'JSON', extensions: ['json'] }],
      properties: ['openFile']
    });
    if (result.canceled || !result.filePaths.length) return null;
    return fs.readFileSync(result.filePaths[0], 'utf-8');
  });

  // Playlist cover images
  ipcMain.handle('playlist:pickImage', async () => {
    const result = await dialog.showOpenDialog(ctx.mainWindow, {
      title: 'Choose playlist cover image',
      filters: [{ name: 'Images', extensions: ['jpg', 'jpeg', 'png', 'webp', 'bmp'] }],
      properties: ['openFile']
    });
    if (result.canceled || !result.filePaths.length) return null;
    return result.filePaths[0];
  });

  ipcMain.handle('playlist:saveImage', async (_event, playlistId, sourcePath) => {
    try {
      const coversDir = getCoversDir();
      const ext = path.extname(sourcePath) || '.jpg';
      const destName = `${playlistId}_${Date.now()}${ext}`;
      const destPath = path.join(coversDir, destName);
      fs.copyFileSync(sourcePath, destPath);
      return destPath;
    } catch (err) { console.error('Save cover image error:', err); return null; }
  });

  ipcMain.handle('playlist:deleteImage', async (_event, imagePath) => {
    try {
      if (imagePath && fs.existsSync(imagePath)) fs.unlinkSync(imagePath);
      return true;
    } catch (err) { console.error('Delete cover image error:', err); return false; }
  });

  // Local audio — pick individual files
  ipcMain.handle('local:pickAudioFiles', async () => {
    const result = await dialog.showOpenDialog(ctx.mainWindow, {
      title: 'Import local audio files',
      filters: [{ name: 'Audio', extensions: ['mp3', 'flac', 'ogg', 'wav', 'aac', 'm4a', 'opus', 'wma', 'aiff'] }],
      properties: ['openFile', 'multiSelections']
    });
    if (result.canceled || !result.filePaths.length) return [];
    const tracks = [];
    for (const filePath of result.filePaths) {
      const t = await parseLocalFile(filePath);
      if (t) tracks.push(t);
    }
    return tracks;
  });
  // Local audio — import a folder as a playlist
  ipcMain.handle('local:pickFolder', async () => {
    const result = await dialog.showOpenDialog(ctx.mainWindow, {
      title: 'Select audio folder',
      properties: ['openDirectory'],
    });
    if (result.canceled || !result.filePaths.length) return null;
    const folderPath = result.filePaths[0];
    const tracks = await scanAudioFolder(folderPath);
    return { folderPath, name: path.basename(folderPath), tracks };
  });

  // Re-scan a folder to pick up new files added since last import
  ipcMain.handle('local:scanFolder', async (_event, folderPath) => {
    if (!folderPath || !fs.existsSync(folderPath)) return [];
    return await scanAudioFolder(folderPath);
  });

  // Copy a file into a playlist's linked folder, then return its track data
  ipcMain.handle('local:copyToPlaylistFolder', async (_event, filePath, folderPath) => {
    if (!fs.existsSync(folderPath)) return null;
    const ext = path.extname(filePath);
    const base = path.basename(filePath);
    let destPath = path.join(folderPath, base);
    if (fs.existsSync(destPath)) {
      const name = path.basename(filePath, ext);
      destPath = path.join(folderPath, `${name}_${Date.now()}${ext}`);
    }
    fs.copyFileSync(filePath, destPath);
    return await parseLocalFile(destPath);
  });
}

const AUDIO_EXTS = new Set(['mp3','flac','ogg','wav','aac','m4a','opus','wma','aiff']);

async function scanAudioFolder(folderPath) {
  const filePaths = [];
  async function walk(dir) {
    let entries;
    try { entries = await fs.promises.readdir(dir, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      const full = path.join(dir, e.name);
      if (e.isDirectory()) await walk(full);
      else if (AUDIO_EXTS.has(path.extname(e.name).toLowerCase().slice(1))) filePaths.push(full);
    }
  }
  await walk(folderPath);
  const tracks = [];
  for (const fp of filePaths) {
    const t = await parseLocalFile(fp);
    if (t) tracks.push(t);
  }
  return tracks;
}

async function parseLocalFile(filePath) {
  try {
    const metadata = await mm.parseFile(filePath, { duration: true });
    const common = metadata.common || {};
    const title = common.title || path.basename(filePath, path.extname(filePath));
    const artist = common.artist || 'Unknown Artist';
    const album = common.album || '';
    const durationMs = metadata.format?.duration ? Math.round(metadata.format.duration * 1000) : 0;
    let thumbnail = '';
    const pic = common.picture?.[0];
    if (pic) thumbnail = `data:${pic.format || 'image/jpeg'};base64,${pic.data.toString('base64')}`;
    return {
      id: 'local_' + Buffer.from(filePath).toString('base64url'),
      title, artist, album, thumbnail, durationMs,
      isLocal: true, localPath: filePath,
    };
  } catch { return null; }
}

module.exports = { register };
