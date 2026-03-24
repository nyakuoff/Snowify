const firebase = require('../firebase');

// Realtime listener unsubscribers
let _friendsUnsub = null;
let _ownPresenceUnsub = null;
const _presenceUnsubs = new Map();

function teardownSocialListeners() {
  if (_friendsUnsub) { _friendsUnsub(); _friendsUnsub = null; }
  if (_ownPresenceUnsub) { _ownPresenceUnsub(); _ownPresenceUnsub = null; }
  for (const unsub of _presenceUnsubs.values()) unsub();
  _presenceUnsubs.clear();
}

function generateFriendCode() {
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
  let code = '';
  for (let i = 0; i < 6; i++) code += chars[Math.floor(Math.random() * chars.length)];
  return code;
}

function register(ipcMain, ctx) {
  ipcMain.handle('social:savePublicPlaylists', async (_event, playlists) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: 'Not signed in' };
    try {
      await firebase.setDoc(firebase.doc(firebase.db, 'users', user.uid), { publicPlaylists: playlists }, { merge: true });
      return { success: true };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('social:getPublicPlaylists', async (_event, uid) => {
    try {
      const snap = await firebase.getDoc(firebase.doc(firebase.db, 'users', uid));
      return snap.exists() ? (snap.data().publicPlaylists || []) : [];
    } catch { return []; }
  });

  ipcMain.handle('social:requestListenAlong', async (_event, targetUid) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: 'Not signed in' };
    try {
      const myPresRef = firebase.doc(firebase.db, 'presence', user.uid);
      const mySnap = await firebase.getDoc(myPresRef);
      if (mySnap.exists() && mySnap.data().listenAlong) return { error: 'Already in a listen along session' };
      await firebase.updateDoc(myPresRef, { listenAlongRequest: { toUid: targetUid, fromName: user.displayName || 'Someone', timestamp: Date.now() } });
      return { success: true };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('social:respondListenAlong', async (_event, fromUid, accepted) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: 'Not signed in' };
    try {
      const myPresRef = firebase.doc(firebase.db, 'presence', user.uid);
      if (accepted) {
        const mySnap = await firebase.getDoc(myPresRef);
        if (mySnap.exists() && mySnap.data().listenAlong) return { error: 'Already in a listen along session', accepted: false };
        await firebase.updateDoc(myPresRef, { listenAlong: { peerUid: fromUid, role: 'guest' } });
      }
      return { success: true, accepted };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('social:endListenAlong', async () => {
    const user = firebase.auth.currentUser;
    if (!user) return;
    try { await firebase.updateDoc(firebase.doc(firebase.db, 'presence', user.uid), { listenAlong: null }); }
    catch (err) { console.error('endListenAlong error:', err); }
  });

  ipcMain.handle('social:getFriendCode', async () => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: 'Not signed in' };
    try {
      const socialRef = firebase.doc(firebase.db, 'users', user.uid, 'social', 'info');
      const snap = await firebase.getDoc(socialRef);
      if (snap.exists() && snap.data().friendCode) return { code: snap.data().friendCode };
      const code = generateFriendCode();
      await firebase.setDoc(socialRef, { friendCode: code }, { merge: true });
      await firebase.setDoc(firebase.doc(firebase.db, 'friendCodes', code), { uid: user.uid, displayName: user.displayName || '', photoURL: user.photoURL || '' });
      return { code };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('social:addFriend', async (_event, code) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: 'Not signed in' };
    code = (code || '').toUpperCase().trim();
    if (!code || code.length !== 6) return { error: 'Invalid friend code' };
    try {
      const lookupSnap = await firebase.getDoc(firebase.doc(firebase.db, 'friendCodes', code));
      if (!lookupSnap.exists()) return { error: 'Friend code not found' };
      const friendData = lookupSnap.data();
      if (friendData.uid === user.uid) return { error: "That's your own code!" };
      const existing = await firebase.getDoc(firebase.doc(firebase.db, 'users', user.uid, 'friends', friendData.uid));
      if (existing.exists()) return { error: 'Already friends!' };

      let fProfile = {};
      try {
        const friendProfileSnap = await firebase.getDoc(firebase.doc(firebase.db, 'users', friendData.uid));
        if (friendProfileSnap.exists()) fProfile = friendProfileSnap.data()?.profile || {};
      } catch (_) {}

      let myPhotoURL = '';
      try {
        const mySnap = await firebase.getDoc(firebase.doc(firebase.db, 'users', user.uid));
        if (mySnap.exists()) myPhotoURL = mySnap.data()?.profile?.photoURL || '';
      } catch (_) {}

      const myInfo = { displayName: user.displayName || '', photoURL: myPhotoURL };
      const friendInfo = { displayName: fProfile.displayName || friendData.displayName || '', photoURL: fProfile.photoURL || friendData.photoURL || '' };

      await firebase.setDoc(firebase.doc(firebase.db, 'users', user.uid, 'friends', friendData.uid), { ...friendInfo, uid: friendData.uid, addedAt: Date.now() });
      await firebase.setDoc(firebase.doc(firebase.db, 'users', friendData.uid, 'friends', user.uid), { ...myInfo, uid: user.uid, addedAt: Date.now() });
      return { success: true, friend: { ...friendInfo, uid: friendData.uid } };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('social:removeFriend', async (_event, friendUid) => {
    const user = firebase.auth.currentUser;
    if (!user) return { error: 'Not signed in' };
    try {
      await firebase.deleteDoc(firebase.doc(firebase.db, 'users', user.uid, 'friends', friendUid));
      await firebase.deleteDoc(firebase.doc(firebase.db, 'users', friendUid, 'friends', user.uid));
      return { success: true };
    } catch (err) { return { error: err.message }; }
  });

  ipcMain.handle('social:getFriends', async () => {
    const user = firebase.auth.currentUser;
    if (!user) return [];
    try {
      const snap = await firebase.getDocs(firebase.collection(firebase.db, 'users', user.uid, 'friends'));
      return snap.docs.map(d => ({ uid: d.id, ...d.data() }));
    } catch { return []; }
  });

  ipcMain.handle('social:updatePresence', async (_event, data) => {
    const user = firebase.auth.currentUser;
    if (!user) return;
    try {
      await firebase.setDoc(firebase.doc(firebase.db, 'presence', user.uid), { ...data, displayName: user.displayName || '', updatedAt: Date.now() }, { merge: true });
    } catch (err) { console.error('updatePresence error:', err); }
  });

  ipcMain.handle('social:clearPresence', async () => {
    const user = firebase.auth.currentUser;
    if (!user) return;
    try {
      await firebase.setDoc(firebase.doc(firebase.db, 'presence', user.uid), { isPlaying: false, isOnline: false, updatedAt: Date.now() }, { merge: true });
    } catch (err) { console.error('clearPresence error:', err); }
  });

  ipcMain.handle('social:getPresence', async (_event, friendUid) => {
    try {
      const snap = await firebase.getDoc(firebase.doc(firebase.db, 'presence', friendUid));
      return snap.exists() ? snap.data() : null;
    } catch { return null; }
  });

  ipcMain.handle('social:getFriendsPresence', async (_event, friendUids) => {
    if (!Array.isArray(friendUids) || !friendUids.length) return {};
    const result = {};
    await Promise.all(friendUids.map(async (uid) => {
      try {
        const snap = await firebase.getDoc(firebase.doc(firebase.db, 'presence', uid));
        if (snap.exists()) result[uid] = snap.data();
      } catch (_) {}
    }));
    return result;
  });

  ipcMain.handle('social:startListening', async () => {
    const user = firebase.auth.currentUser;
    if (!user || !ctx.mainWindow) return;

    teardownSocialListeners();
    const myPresRef = firebase.doc(firebase.db, 'presence', user.uid);
    await firebase.updateDoc(myPresRef, { listenAlong: null, listenAlongRequest: null }).catch(() => {});

    _ownPresenceUnsub = firebase.onSnapshot(myPresRef, (docSnap) => {
      if (!docSnap.exists() || !ctx.mainWindow || ctx.mainWindow.isDestroyed()) return;
      const data = docSnap.data();
      ctx.mainWindow.webContents.send('social:ownPresenceUpdated', { listenAlong: data.listenAlong || null });
    }, (err) => console.error('Own presence listener error:', err.message));

    _friendsUnsub = firebase.onSnapshot(firebase.collection(firebase.db, 'users', user.uid, 'friends'), (snap) => {
      const friends = snap.docs.map(d => ({ uid: d.id, ...d.data() }));
      if (ctx.mainWindow && !ctx.mainWindow.isDestroyed()) ctx.mainWindow.webContents.send('social:friendsUpdated', friends);

      const currentUids = new Set(friends.map(f => f.uid));
      for (const [uid, unsub] of _presenceUnsubs) {
        if (!currentUids.has(uid)) { unsub(); _presenceUnsubs.delete(uid); }
      }

      for (const uid of currentUids) {
        if (_presenceUnsubs.has(uid)) continue;
        const presRef = firebase.doc(firebase.db, 'presence', uid);
        const unsub = firebase.onSnapshot(presRef, (docSnap) => {
          const data = docSnap.exists() ? docSnap.data() : null;
          if (!ctx.mainWindow || ctx.mainWindow.isDestroyed()) return;

          if (data?.listenAlongRequest?.toUid === user.uid) {
            ctx.mainWindow.webContents.send('social:listenAlongRequest', { fromUid: uid, fromName: data.displayName || data.listenAlongRequest.fromName || 'Friend', timestamp: data.listenAlongRequest.timestamp });
          }

          if (data?.listenAlong?.peerUid === user.uid && data.listenAlong.role === 'guest') {
            const myRef = firebase.doc(firebase.db, 'presence', user.uid);
            firebase.getDoc(myRef).then(mySnap => {
              if (mySnap.exists() && mySnap.data().listenAlong) return;
              firebase.updateDoc(myRef, { listenAlong: { peerUid: uid, role: 'host' }, listenAlongRequest: null });
            }).catch(err => console.error('Auto-set host error:', err));
          }

          ctx.mainWindow.webContents.send('social:presenceUpdated', { uid, presence: data });
        }, (err) => console.error(`Presence listener error for ${uid}:`, err.message));
        _presenceUnsubs.set(uid, unsub);
      }
    }, (err) => console.error('Friends listener error:', err.message));
  });

  ipcMain.handle('social:stopListening', async () => { teardownSocialListeners(); });
}

module.exports = { teardownSocialListeners, register };
