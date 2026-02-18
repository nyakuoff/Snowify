// ─── Firebase Configuration ───
// Replace the values below with your Firebase project config.
// Get them from: Firebase Console → Project Settings → General → Your apps → Web app

const { initializeApp } = require('firebase/app');
const { getAuth, signInWithCredential, GoogleAuthProvider, signOut, onAuthStateChanged, updateProfile } = require('firebase/auth');
const { getFirestore, doc, getDoc, setDoc } = require('firebase/firestore');

const firebaseConfig = {
    apiKey: "AIzaSyCNuw8kqgbULTLjC890BzKWvnmdvFCX0og",
    authDomain: "snowify-dcda0.firebaseapp.com",
    projectId: "snowify-dcda0",
    storageBucket: "snowify-dcda0.firebasestorage.app",
    messagingSenderId: "925903561467",
    appId: "1:925903561467:web:c4298e4f47ac80d1c9c65e"
};
const firebaseApp = initializeApp(firebaseConfig);
const auth = getAuth(firebaseApp);
const db = getFirestore(firebaseApp);

module.exports = { auth, db, doc, getDoc, setDoc, signInWithCredential, GoogleAuthProvider, signOut, onAuthStateChanged, updateProfile };
