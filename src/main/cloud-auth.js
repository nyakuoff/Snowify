const fs = require('fs');
const path = require('path');
const { mt } = require('./i18n');
const firebase = require('../firebase');

let _wiredAuthState = false;
const SYNC_COLLECTION_V2 = 'users_v2';
const SYNC_COLLECTION_LEGACY = 'users';
const _syncReadyUsers = new Set();

function _isPlainObject(value) {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function _mergeForBackfill(v2Data = {}, legacyData = {}) {
  const merged = { ...v2Data };
  let changed = false;

  for (const [key, legacyValue] of Object.entries(legacyData || {})) {
    const v2Value = merged[key];

    if (v2Value === undefined || v2Value === null) {
      merged[key] = legacyValue;
      changed = true;
      continue;
    }

    if (Array.isArray(legacyValue) && Array.isArray(v2Value) && v2Value.length === 0 && legacyValue.length > 0) {
      merged[key] = legacyValue;
      changed = true;
      continue;
    }

    if (_isPlainObject(legacyValue) && _isPlainObject(v2Value)) {
      const nested = _mergeForBackfill(v2Value, legacyValue);
      if (nested.changed) {
        merged[key] = nested.merged;
        changed = true;
      }
    }
  }

  return { merged, changed };
}

function _docRef(uid, useLegacy = false) {
  return firebase.doc(firebase.db, useLegacy ? SYNC_COLLECTION_LEGACY : SYNC_COLLECTION_V2, uid);
}

async function _migrateLegacyDocIfNeeded(uid) {
  if (_syncReadyUsers.has(uid)) return null;
  try {
    const v2Snap = await firebase.getDoc(_docRef(uid));
    const v2Data = v2Snap.exists() ? (v2Snap.data() || {}) : null;

    const legacySnap = await firebase.getDoc(_docRef(uid, true));
    if (!legacySnap.exists()) {
      if (v2Data) _syncReadyUsers.add(uid);
      return v2Data;
    }

    const legacyData = legacySnap.data() || {};
    if (!v2Data) {
      await firebase.setDoc(_docRef(uid), {
        ...legacyData,
        migratedFrom: SYNC_COLLECTION_LEGACY,
        migratedAt: Date.now(),
      }, { merge: true });

      _syncReadyUsers.add(uid);
      return legacyData;
    }

    const { merged, changed } = _mergeForBackfill(v2Data, legacyData);
    if (changed) {
      await firebase.setDoc(_docRef(uid), {
        ...merged,
        migratedFrom: SYNC_COLLECTION_LEGACY,
        migratedAt: Date.now(),
      }, { merge: true });
    }

    _syncReadyUsers.add(uid);
    return changed ? merged : v2Data;
  } catch {
    return null;
  }
}

async function _readUserData(uid) {
  try {
    const v2Snap = await firebase.getDoc(_docRef(uid));
    if (v2Snap.exists()) {
      _syncReadyUsers.add(uid);
      return v2Snap.data();
    }
  } catch {
    // Fall through to legacy read path.
  }

  return _migrateLegacyDocIfNeeded(uid);
}

function _getCredentialsPath() {
  const { app } = require('electron');
  return path.join(app.getPath('userData'), 'auth_credentials.json');
}

function _saveCredentials(email, password) {
  try {
    fs.writeFileSync(_getCredentialsPath(), JSON.stringify({ email, password }), 'utf8');
  } catch (err) {
    console.warn('[Auth] Failed to save credentials:', err.message);
  }
}

function _loadCredentials() {
  try {
    const p = _getCredentialsPath();
    if (!fs.existsSync(p)) return null;
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch {
    return null;
  }
}

function _clearCredentials() {
  try {
    const p = _getCredentialsPath();
    if (fs.existsSync(p)) fs.unlinkSync(p);
  } catch (err) {
    console.warn('[Auth] Failed to clear credentials:', err.message);
  }
}

async function _getUserInfo(user) {
  const info = {
    uid: user.uid,
    email: user.email,
    displayName: user.displayName,
    photoURL: user.photoURL || null,
  };

  try {
    const data = await _readUserData(user.uid);
    if (data) {
      const profile = data.profile;
      if (profile?.photoURL) info.photoURL = profile.photoURL;
      if (profile?.displayName && !info.displayName) info.displayName = profile.displayName;
    }
  } catch {
    // Auth data is good enough if Firestore is unavailable.
  }

  return info;
}

async function _autoSignIn() {
  const creds = _loadCredentials();
  if (!creds?.email || !creds?.password) return;
  try {
    const { signInWithEmailAndPassword } = require('firebase/auth');
    await signInWithEmailAndPassword(firebase.auth, creds.email, creds.password);
  } catch {
    _clearCredentials();
  }
}

function register(ipcMain, ctx) {
  if (!_wiredAuthState) {
    _wiredAuthState = true;
    firebase.onAuthStateChanged(firebase.auth, async (user) => {
      if (!ctx.mainWindow?.webContents) return;
      if (user) {
        const info = await _getUserInfo(user);
        ctx.mainWindow.webContents.send('auth:stateChanged', info);
      } else {
        ctx.mainWindow.webContents.send('auth:stateChanged', null);
      }
    });
  }

  _autoSignIn().catch(() => {});

  ipcMain.handle('auth:signInWithEmail', async (_event, email, password) => {
    try {
      const { signInWithEmailAndPassword } = require('firebase/auth');
      const result = await signInWithEmailAndPassword(firebase.auth, email, password);
      _saveCredentials(email, password);
      return _getUserInfo(result.user);
    } catch (err) {
      return { error: err.message };
    }
  });

  ipcMain.handle('auth:signUpWithEmail', async (_event, email, password) => {
    try {
      const { createUserWithEmailAndPassword } = require('firebase/auth');
      const result = await createUserWithEmailAndPassword(firebase.auth, email, password);
      _saveCredentials(email, password);
      return _getUserInfo(result.user);
    } catch (err) {
      return { error: err.message };
    }
  });

  ipcMain.handle('auth:sendPasswordReset', async (_event, email) => {
    try {
      const { sendPasswordResetEmail } = require('firebase/auth');
      await sendPasswordResetEmail(firebase.auth, email);
      return { ok: true };
    } catch (err) {
      return { error: err.message };
    }
  });

  ipcMain.handle('auth:signOut', async () => {
    try {
      await firebase.signOut(firebase.auth);
      _clearCredentials();
      return true;
    } catch (err) {
      return { error: err.message };
    }
  });

  ipcMain.handle('auth:getUser', async () => {
    const user = firebase.auth.currentUser;
    if (!user) return null;
    return _getUserInfo(user);
  });

  ipcMain.handle('profile:update', async (_event, { displayName, photoURL }) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: mt('auth.notSignedIn') };

    try {
      if (displayName !== undefined) {
        await firebase.updateProfile(user, { displayName });
      }

      const profileData = { displayName: user.displayName || '' };
      if (photoURL !== undefined) profileData.photoURL = photoURL;

      await _migrateLegacyDocIfNeeded(user.uid);
      const docRef = _docRef(user.uid);
      await firebase.setDoc(docRef, { profile: profileData }, { merge: true });

      return {
        uid: user.uid,
        email: user.email,
        displayName: user.displayName,
        photoURL: photoURL !== undefined ? photoURL : user.photoURL,
      };
    } catch (err) {
      return { error: err.message };
    }
  });

  ipcMain.handle('profile:readImage', async (_event, filePath) => {
    try {
      const ext = path.extname(filePath).toLowerCase();
      if (ext === '.gif') return null;
      const buffer = fs.readFileSync(filePath);
      const mime = ext === '.png' ? 'image/png' : ext === '.webp' ? 'image/webp' : 'image/jpeg';
      return `data:${mime};base64,${buffer.toString('base64')}`;
    } catch {
      return null;
    }
  });

  ipcMain.handle('profile:updateExtras', async (_event, { banner, bio }) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: mt('auth.notSignedIn') };

    try {
      if (banner !== undefined && banner !== '' && !String(banner).startsWith('data:image/')) {
        return { error: 'Invalid banner format' };
      }
      if (banner && String(banner).length > 800000) {
        return { error: 'Banner image is too large. Please use a smaller image.' };
      }

      await _migrateLegacyDocIfNeeded(user.uid);
      const docRef = _docRef(user.uid);
      const updateData = {};
      if (banner !== undefined) updateData['profile.banner'] = banner;
      if (bio !== undefined) updateData['profile.bio'] = String(bio).slice(0, 200);

      try {
        await firebase.updateDoc(docRef, updateData);
      } catch {
        const profile = {};
        if (banner !== undefined) profile.banner = banner;
        if (bio !== undefined) profile.bio = String(bio).slice(0, 200);
        await firebase.setDoc(docRef, { profile }, { merge: true });
      }

      return { success: true };
    } catch (err) {
      return { error: err.message };
    }
  });

  ipcMain.handle('profile:get', async (_event, uid) => {
    try {
      const data = await _readUserData(uid);
      if (!data) return null;
      const profile = data.profile || {};
      if (data['profile.banner'] && !profile.banner) profile.banner = data['profile.banner'];
      if (data['profile.bio'] && !profile.bio) profile.bio = data['profile.bio'];
      return profile;
    } catch {
      return null;
    }
  });

  ipcMain.handle('cloud:save', async (_event, data) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: mt('auth.notSignedIn') };

    try {
      await _migrateLegacyDocIfNeeded(user.uid);
      const docRef = _docRef(user.uid);
      await firebase.setDoc(docRef, {
        ...data,
        updatedAt: Date.now(),
      }, { merge: true });
      return true;
    } catch (err) {
      return { error: err.message };
    }
  });

  ipcMain.handle('cloud:load', async () => {
    const user = firebase.auth.currentUser;
    if (!user) return null;

    try {
      const data = await _readUserData(user.uid);
      return data || null;
    } catch {
      return null;
    }
  });
}

module.exports = { register };
