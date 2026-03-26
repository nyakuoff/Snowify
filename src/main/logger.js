const _logBuffer = [];
const _LOG_MAX = 500;

function _captureLog(level, args) {
  const ts = new Date().toISOString().slice(11, 23);
  const msg = args.map(a => {
    if (typeof a === 'string') return a;
    try { return JSON.stringify(a); } catch { return String(a); }
  }).join(' ');
  _logBuffer.push({ ts, level, msg });
  if (_logBuffer.length > _LOG_MAX) _logBuffer.shift();
}

// Override console methods to capture logs for the in-app debug panel
const _origLog = console.log.bind(console);
const _origWarn = console.warn.bind(console);
const _origError = console.error.bind(console);

console.log = (...args) => { _captureLog('log', args); _origLog(...args); };
console.warn = (...args) => { _captureLog('warn', args); _origWarn(...args); };
console.error = (...args) => { _captureLog('error', args); _origError(...args); };

module.exports = { _logBuffer, _captureLog };
