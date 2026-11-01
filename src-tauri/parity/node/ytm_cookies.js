// Differential runner for ytm/cookies.rs against tough-cookie 4.1.4 (the jar ytmusic-api
// uses). Input:  [["set", cookieString, url, nowMs] | ["get", null, url, nowMs]]
// Output: [["ok"] | ["null"] | ["error"] | ["header", cookieString]]
const fs = require('fs');
const { Cookie, CookieJar } = require('tough-cookie');
const ops = JSON.parse(fs.readFileSync(0, 'utf8'));
const jar = new CookieJar();
process.stdout.write(JSON.stringify(ops.map(([op, value, url, now]) => {
  if (op === 'set') {
    const cookie = Cookie.parse(value);
    if (!cookie) return ['null'];
    try {
      jar.setCookieSync(cookie, url, { now: new Date(now) });
      return ['ok'];
    } catch (_) {
      return ['error'];
    }
  }
  try {
    return ['header', jar.getCookieStringSync(url, { now })];
  } catch (_) {
    return ['error'];
  }
})));
