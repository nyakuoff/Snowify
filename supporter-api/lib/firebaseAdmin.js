const admin = require('firebase-admin');

let _app;

function getPrivateKey() {
  const raw = process.env.FIREBASE_PRIVATE_KEY || '';
  return raw.replace(/\\n/g, '\n');
}

function getAdminApp() {
  if (_app) return _app;

  _app = admin.initializeApp({
    credential: admin.credential.cert({
      projectId: process.env.FIREBASE_PROJECT_ID,
      clientEmail: process.env.FIREBASE_CLIENT_EMAIL,
      privateKey: getPrivateKey(),
    }),
  });

  return _app;
}

function getFirestore() {
  return getAdminApp().firestore();
}

function getAuth() {
  return getAdminApp().auth();
}

module.exports = { getFirestore, getAuth };
