const { getAuth } = require('./firebaseAdmin');

function getBearerToken(req) {
  const header = req.headers.authorization || req.headers.Authorization;
  if (!header || typeof header !== 'string') return null;
  const [scheme, token] = header.split(' ');
  if (!scheme || scheme.toLowerCase() !== 'bearer' || !token) return null;
  return token;
}

async function requireUser(req) {
  const token = getBearerToken(req);
  if (!token) throw new Error('Missing bearer token');
  const decoded = await getAuth().verifyIdToken(token);
  return { uid: decoded.uid, email: decoded.email || '' };
}

module.exports = { requireUser };
