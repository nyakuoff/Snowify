// ─── LUFS Loudness Normalizer (ITU-R BS.1770 + EBU R128) ───
// Closure-based module. Created via LoudnessNormalizer(audioA, audioB).
// Measures LUFS in real-time via AudioWorklet (no extra downloads).
// Audio graph per element: source → workletNode (meter, pass-through) → gainNode → destination

window.LoudnessNormalizer = function LoudnessNormalizer(audioA, audioB) {
  'use strict';

  // ─── Constants ───
  const DEFAULT_TARGET = -14; // LUFS (Spotify standard)
  const RAMP_TIME = 0.4;     // seconds — smooth gain transition
  const PEAK_CEILING = -0.5; // dBFS — gain cap to prevent clipping
  const CACHE_KEY = 'snowify_lufs_cache';
  const CACHE_MAX = 500;

  // ─── State ───
  let _enabled = false;
  let _target = DEFAULT_TARGET;
  let _audioCtx = null;
  let _sourceA = null;
  let _sourceB = null;
  let _gainA = null;
  let _gainB = null;
  let _workletNodeA = null;
  let _workletNodeB = null;
  let _workletReady = false;
  const _cache = new Map();       // trackId → { lufs, peak, partial? }

  // Track which element is currently being measured and for which trackId
  let _measuringElA = false;      // is audioA currently measuring?
  let _measuringElB = false;      // is audioB currently measuring?
  let _measuringTrackIdA = null;
  let _measuringTrackIdB = null;

  // ─── localStorage Cache ───

  function loadCache() {
    try {
      const raw = localStorage.getItem(CACHE_KEY);
      if (!raw) return;
      const entries = JSON.parse(raw);
      if (Array.isArray(entries)) {
        for (const [k, v] of entries) {
          _cache.set(k, v);
        }
      }
    } catch (_) { /* ignore corrupt cache */ }
  }

  function persistCache() {
    try {
      // LRU: keep only last CACHE_MAX entries
      const entries = [..._cache.entries()];
      const trimmed = entries.slice(-CACHE_MAX);
      localStorage.setItem(CACHE_KEY, JSON.stringify(trimmed));
    } catch (_) { /* quota exceeded or similar */ }
  }

  // Load cache on module creation
  loadCache();

  // ─── Audio Context + Graph ───

  async function initAudioContext() {
    if (_audioCtx) {
      if (_audioCtx.state === 'suspended') _audioCtx.resume();
      // If worklet already loaded, nothing to do
      if (_workletReady) return;
    }

    if (!_audioCtx) {
      _audioCtx = new (window.AudioContext || window.webkitAudioContext)();
    }
    if (_audioCtx.state === 'suspended') _audioCtx.resume();

    // Load AudioWorklet module
    try {
      await _audioCtx.audioWorklet.addModule('lufs-processor.js');
      _workletReady = true;
    } catch (err) {
      console.warn('[Normalizer] AudioWorklet failed to load:', err);
      _workletReady = false;
    }

    // Build audio graph: source → workletNode → gainNode → destination
    // MediaElementSource can only be created ONCE per element — never close this context
    _sourceA = _audioCtx.createMediaElementSource(audioA);
    _sourceB = _audioCtx.createMediaElementSource(audioB);
    _gainA = _audioCtx.createGain();
    _gainB = _audioCtx.createGain();

    if (_workletReady) {
      _workletNodeA = new AudioWorkletNode(_audioCtx, 'lufs-processor');
      _workletNodeB = new AudioWorkletNode(_audioCtx, 'lufs-processor');
      _workletNodeA.port.onmessage = handleWorkletMessage;
      _workletNodeB.port.onmessage = handleWorkletMessage;
      _sourceA.connect(_workletNodeA).connect(_gainA).connect(_audioCtx.destination);
      _sourceB.connect(_workletNodeB).connect(_gainB).connect(_audioCtx.destination);
    } else {
      // Fallback: no worklet, just source → gain → destination
      _sourceA.connect(_gainA).connect(_audioCtx.destination);
      _sourceB.connect(_gainB).connect(_audioCtx.destination);
    }
  }

  function getGainNode(audioEl) {
    if (!_audioCtx) return null;
    return audioEl === audioA ? _gainA : audioEl === audioB ? _gainB : null;
  }

  function getWorkletNode(audioEl) {
    if (!_workletReady) return null;
    return audioEl === audioA ? _workletNodeA : audioEl === audioB ? _workletNodeB : null;
  }

  // ─── Worklet Communication ───

  function handleWorkletMessage(e) {
    const d = e.data;
    if (d.type !== 'result') return;

    const { trackId, lufs, peak, blockCount, partial } = d;

    if (lufs === -Infinity || blockCount === 0) {
      console.log(`[Normalizer] ${trackId}: no measurable audio (${blockCount} blocks)`);
      return;
    }

    // If partial and we already have a full measurement, keep the full one
    if (partial && _cache.has(trackId)) {
      const existing = _cache.get(trackId);
      if (!existing.partial) return;
    }

    const result = { lufs, peak, partial: !!partial };
    _cache.set(trackId, result);
    persistCache();

    const peakDB = 20 * Math.log10(peak || 1e-10);
    console.log(`[Normalizer] ${trackId}: ${lufs.toFixed(1)} LUFS, peak ${peakDB.toFixed(1)} dBFS${partial ? ' (partial)' : ''}`);

    // If this track is currently playing on an element, apply gain
    if (!partial && _enabled) {
      if (_measuringTrackIdA === trackId) applyGain(audioA, trackId);
      if (_measuringTrackIdB === trackId) applyGain(audioB, trackId);
    }
  }

  function startMeasurement(audioEl, trackId) {
    const node = getWorkletNode(audioEl);
    if (!node) return;

    // If already measuring on this element, finalize the old measurement first
    const isMeasuring = audioEl === audioA ? _measuringElA : _measuringElB;
    if (isMeasuring) finalizeMeasurement(audioEl, true);

    // Send volume compensation before reset
    const vol = audioEl.volume;
    const comp = vol > 0.001 ? 1 / vol : 1.0;
    node.port.postMessage({ type: 'volumeCompensation', value: comp });
    node.port.postMessage({ type: 'reset', trackId });

    if (audioEl === audioA) {
      _measuringElA = true;
      _measuringTrackIdA = trackId;
    } else {
      _measuringElB = true;
      _measuringTrackIdB = trackId;
    }
  }

  function finalizeMeasurement(audioEl, partial) {
    const node = getWorkletNode(audioEl);
    if (!node) return;

    const isMeasuring = audioEl === audioA ? _measuringElA : _measuringElB;
    if (!isMeasuring) return;

    node.port.postMessage({ type: 'finalize', partial: !!partial });

    if (audioEl === audioA) {
      _measuringElA = false;
      _measuringTrackIdA = null;
    } else {
      _measuringElB = false;
      _measuringTrackIdB = null;
    }
  }

  function stopMeasurement(audioEl) {
    const node = getWorkletNode(audioEl);
    if (!node) return;

    node.port.postMessage({ type: 'stop' });

    if (audioEl === audioA) {
      _measuringElA = false;
      _measuringTrackIdA = null;
    } else {
      _measuringElB = false;
      _measuringTrackIdB = null;
    }
  }

  function updateVolumeCompensation(audioEl) {
    const node = getWorkletNode(audioEl);
    if (!node) return;
    const vol = audioEl.volume;
    const comp = vol > 0.001 ? 1 / vol : 1.0;
    node.port.postMessage({ type: 'volumeCompensation', value: comp });
  }

  // ─── Gain Computation ───

  function computeGain(trackId) {
    const data = _cache.get(trackId);
    if (!data || data.lufs === -Infinity) return 1.0;

    let gainDB = _target - data.lufs;
    // Cap gain to prevent clipping: peak + gain <= PEAK_CEILING
    const peakDBFS = 20 * Math.log10(data.peak || 1e-10);
    const maxGainDB = PEAK_CEILING - peakDBFS;
    if (gainDB > maxGainDB) gainDB = maxGainDB;
    // Never boost silence — if gain would be absurdly high, skip
    if (gainDB > 24) return 1.0;
    return Math.pow(10, gainDB / 20);
  }

  // ─── Apply / Reset ───

  function applyGain(audioEl, trackId) {
    if (!_enabled || !_audioCtx) return;
    const node = getGainNode(audioEl);
    if (!node) return;
    const gain = computeGain(trackId);
    node.gain.setTargetAtTime(gain, _audioCtx.currentTime, RAMP_TIME / 3);
  }

  function resetGain(audioEl) {
    if (!_audioCtx) return;
    const node = getGainNode(audioEl);
    if (!node) return;
    node.gain.setTargetAtTime(1.0, _audioCtx.currentTime, RAMP_TIME / 3);
  }

  // ─── Main Entry Points ───

  function analyzeAndApply(audioEl, url, trackId) {
    if (!_enabled) return;
    if (!_audioCtx) return; // initAudioContext must be called first
    if (_audioCtx.state === 'suspended') _audioCtx.resume();

    // Cache hit with full measurement: apply immediately
    if (_cache.has(trackId)) {
      const cached = _cache.get(trackId);
      if (!cached.partial) {
        applyGain(audioEl, trackId);
        return;
      }
    }

    // No cache or partial: play at unity, start real-time measurement
    const node = getGainNode(audioEl);
    if (node) node.gain.setTargetAtTime(1.0, _audioCtx.currentTime, 0.05);
    startMeasurement(audioEl, trackId);
  }

  function preAnalyze() {
    // No-op: cannot measure without playing in real-time AudioWorklet mode
  }

  // ─── Configuration ───

  function setEnabled(val) {
    _enabled = !!val;
    if (!_enabled) {
      resetGain(audioA);
      resetGain(audioB);
    }
  }

  function setTarget(lufs) {
    _target = lufs;
  }

  function clearCache() {
    _cache.clear();
    try { localStorage.removeItem(CACHE_KEY); } catch (_) {}
  }

  function destroy() {
    stopMeasurement(audioA);
    stopMeasurement(audioB);
    clearCache();
    // Do NOT close _audioCtx — MediaElementSources can't be recreated
  }

  // ─── Public API ───
  return {
    initAudioContext,
    setEnabled,
    setTarget,
    isEnabled()            { return _enabled; },
    isWorkletReady()       { return _workletReady; },
    getTarget()            { return _target; },
    getCachedLUFS(trackId) { return _cache.get(trackId) || null; },
    analyzeAndApply,
    preAnalyze,
    applyGain,
    resetGain,
    startMeasurement,
    finalizeMeasurement,
    stopMeasurement,
    updateVolumeCompensation,
    clearCache,
    destroy
  };
};
