const path = require('path');
const fs = require('fs');
const { app, dialog } = require('electron');
const firebase = require('../firebase');
const { mt } = require('./i18n');

// ─── Credential persistence via safeStorage ───
const { safeStorage } = require('electron');
const credentialsPath = path.join(app.getPath('userData'), '.auth');

function saveCredentials(email, password) {
  try {
    if (!safeStorage.isEncryptionAvailable()) {
      fs.writeFileSync(credentialsPath, JSON.stringify({ email, password }));
      return;
    }
    fs.writeFileSync(credentialsPath, safeStorage.encryptString(JSON.stringify({ email, password })));
  } catch (_) {}
}

function loadCredentials() {
  try {
    if (!fs.existsSync(credentialsPath)) return null;
    const raw = fs.readFileSync(credentialsPath);
    return JSON.parse(safeStorage.isEncryptionAvailable() ? safeStorage.decryptString(raw) : raw.toString());
  } catch (_) { return null; }
}

function clearCredentials() {
  try { fs.unlinkSync(credentialsPath); } catch (_) {}
}

async function autoSignIn() {
  const creds = loadCredentials();
  if (!creds) return;
  try {
    const { signInWithEmailAndPassword } = require('firebase/auth');
    await signInWithEmailAndPassword(firebase.auth, creds.email, creds.password);
  } catch (_) {
    clearCredentials();
  }
}

async function getUserInfo(user) {
  const info = { uid: user.uid, email: user.email, displayName: user.displayName, photoURL: user.photoURL || null };
  try {
    const snap = await firebase.getDoc(firebase.doc(firebase.db, 'users', user.uid));
    if (snap.exists()) {
      const profile = snap.data()?.profile;
      if (profile?.photoURL) info.photoURL = profile.photoURL;
      if (profile?.displayName && !info.displayName) info.displayName = profile.displayName;
    }
  } catch (_) {}
  return info;
}

// Propagate profile changes to all friend subdocs + friendCode lookup
async function propagateProfileToFriends(uid, updatedFields) {
  const friendsSnap = await firebase.getDocs(firebase.collection(firebase.db, 'users', uid, 'friends'));
  const promises = friendsSnap.docs.map(friendDoc =>
    firebase.setDoc(firebase.doc(firebase.db, 'users', friendDoc.id, 'friends', uid), updatedFields, { merge: true })
  );
  const socialSnap = await firebase.getDoc(firebase.doc(firebase.db, 'users', uid, 'social', 'info'));
  if (socialSnap.exists() && socialSnap.data().friendCode) {
    promises.push(firebase.setDoc(firebase.doc(firebase.db, 'friendCodes', socialSnap.data().friendCode), updatedFields, { merge: true }));
  }
  await Promise.all(promises);
}

function register(ipcMain, ctx) {
  // Auth state listener
  firebase.onAuthStateChanged(firebase.auth, async (user) => {
    ctx.currentUser = user;
    if (user) {
      const info = await getUserInfo(user);
      ctx.mainWindow?.webContents?.send('auth:stateChanged', info);
    } else {
      ctx.mainWindow?.webContents?.send('auth:stateChanged', null);
    }
  });

  ipcMain.handle('auth:signInWithEmail', async (_event, email, password) => {
    try {
      const { signInWithEmailAndPassword } = require('firebase/auth');
      const result = await signInWithEmailAndPassword(firebase.auth, email, password);
      saveCredentials(email, password);
      return { uid: result.user.uid, email: result.user.email, displayName: result.user.displayName, photoURL: result.user.photoURL };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('auth:signUpWithEmail', async (_event, email, password) => {
    try {
      const { createUserWithEmailAndPassword } = require('firebase/auth');
      const result = await createUserWithEmailAndPassword(firebase.auth, email, password);
      saveCredentials(email, password);
      return { uid: result.user.uid, email: result.user.email, displayName: result.user.displayName, photoURL: result.user.photoURL };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('auth:sendPasswordReset', async (_event, email) => {
    try {
      const { sendPasswordResetEmail } = require('firebase/auth');
      await sendPasswordResetEmail(firebase.auth, email);
      return { ok: true };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('auth:signOut', async () => {
    try {
      // Clear presence before signing out while currentUser is still valid
      const user = firebase.auth.currentUser;
      if (user) {
        await firebase.setDoc(
          firebase.doc(firebase.db, 'presence', user.uid),
          { isPlaying: false, isOnline: false, updatedAt: Date.now() },
          { merge: true }
        ).catch(() => {});
      }
      await firebase.signOut(firebase.auth);
      ctx.currentUser = null;
      clearCredentials();
      return true;
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('auth:getUser', async () => {
    const user = firebase.auth.currentUser;
    if (!user) return null;
    return getUserInfo(user);
  });

  ipcMain.handle('profile:update', async (_event, { displayName, photoURL }) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: mt('auth.notSignedIn') };
    try {
      if (displayName !== undefined) await firebase.updateProfile(user, { displayName });
      const profileData = { displayName: user.displayName };
      if (photoURL !== undefined) profileData.photoURL = photoURL;
      await firebase.setDoc(firebase.doc(firebase.db, 'users', user.uid), { profile: profileData }, { merge: true });
      const updatedInfo = { displayName: user.displayName || '' };
      if (photoURL !== undefined) updatedInfo.photoURL = photoURL;
      propagateProfileToFriends(user.uid, updatedInfo).catch(err => console.error('Profile fan-out error:', err.message));
      return { uid: user.uid, email: user.email, displayName: user.displayName, photoURL: photoURL !== undefined ? photoURL : user.photoURL };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('profile:readImage', async (_event, filePath) => {
    try {
      const ext = path.extname(filePath).toLowerCase();
      if (ext === '.gif') return null;
      const buffer = fs.readFileSync(filePath);
      const mime = ext === '.png' ? 'image/png' : ext === '.webp' ? 'image/webp' : 'image/jpeg';
      return `data:${mime};base64,${buffer.toString('base64')}`;
    } catch (_) { return null; }
  });

  ipcMain.handle('profile:updateExtras', async (_event, { banner, bio }) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: 'Not signed in' };
    try {
      if (banner !== undefined && banner !== '' && !banner.startsWith('data:image/')) return { error: 'Invalid banner format' };
      if (banner && banner.length > 800000) return { error: 'Banner image is too large. Please use a smaller image.' };
      const updateData = {};
      if (banner !== undefined) updateData['profile.banner'] = banner;
      if (bio !== undefined) updateData['profile.bio'] = String(bio).slice(0, 200);
      await firebase.updateDoc(firebase.doc(firebase.db, 'users', user.uid), updateData);
      return { success: true };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('profile:get', async (_event, uid) => {
    try {
      const snap = await firebase.getDoc(firebase.doc(firebase.db, 'users', uid));
      if (!snap.exists()) return null;
      const data = snap.data();
      const profile = data.profile || {};
      if (data['profile.banner'] && !profile.banner) profile.banner = data['profile.banner'];
      if (data['profile.bio'] && !profile.bio) profile.bio = data['profile.bio'];
      return profile;
    } catch (err) { return null; }
  });

  ipcMain.handle('cloud:save', async (_event, data) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: mt('auth.notSignedIn') };
    try {
      await firebase.setDoc(firebase.doc(firebase.db, 'users', user.uid), { ...data, updatedAt: Date.now() }, { merge: true });
      return true;
    } catch (err) { console.error('Cloud save error:', err); return { error: err.message }; }
  });

  ipcMain.handle('cloud:load', async () => {
    const user = firebase.auth.currentUser;
    if (!user) return null;
    try {
      const snap = await firebase.getDoc(firebase.doc(firebase.db, 'users', user.uid));
      return snap.exists() ? snap.data() : null;
    } catch (err) { console.error('Cloud load error:', err); return null; }
  });
}

module.exports = { autoSignIn, register, getUserInfo };
