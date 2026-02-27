// ─── LUFS AudioWorklet Processor (ITU-R BS.1770 / EBU R128) ───
// Loaded via: audioContext.audioWorklet.addModule('lufs-processor.js')
// Measures integrated LUFS in real-time. Pass-through audio (non-destructive).
// Port messages IN:  reset(trackId), finalize(partial), stop, volumeCompensation(value)
// Port messages OUT: result({ trackId, lufs, peak, blockCount, partial })

'use strict';

// ─── K-Weighting Coefficients (pre-computed, validated against ITU-R BS.1770) ───
const K_WEIGHT = {
  48000: {
    shelf: { b0: 1.53512485958697, b1: -2.69169618940638, b2: 1.19839281085285,
             a1: -1.69065929318241, a2: 0.73248077421585 },
    hp:    { b0: 1.0, b1: -2.0, b2: 1.0,
             a1: -1.99004745483398, a2: 0.99007225036621 }
  },
  44100: {
    shelf: { b0: 1.53090959966428, b1: -2.65116903469122, b2: 1.16903097776360,
             a1: -1.66363794709474, a2: 0.71238064688380 },
    hp:    { b0: 1.0, b1: -2.0, b2: 1.0,
             a1: -1.98916967210520, a2: 0.98919159781614 }
  }
};

// Compute K-weighting coefficients for arbitrary sample rates (Audio EQ Cookbook)
function computeKWeightCoeffs(sr) {
  const PI2 = 2 * Math.PI;
  // Stage 1: High shelf (f0=1681.97Hz, G=4.0dB, Q=0.7072)
  const A1 = Math.pow(10, 3.999843853973347 / 40);
  const w1 = PI2 * 1681.974450955533 / sr;
  const alpha1 = Math.sin(w1) / (2 * 0.7071752369554196);
  const cosw1 = Math.cos(w1), sqA1 = Math.sqrt(A1);
  const sa0 = (A1+1)-(A1-1)*cosw1+2*sqA1*alpha1;
  const shelf = {
    b0: (A1*((A1+1)+(A1-1)*cosw1+2*sqA1*alpha1))/sa0,
    b1: (-2*A1*((A1-1)+(A1+1)*cosw1))/sa0,
    b2: (A1*((A1+1)+(A1-1)*cosw1-2*sqA1*alpha1))/sa0,
    a1: (2*((A1-1)-(A1+1)*cosw1))/sa0,
    a2: ((A1+1)-(A1-1)*cosw1-2*sqA1*alpha1)/sa0
  };
  // Stage 2: High pass (f0=38.14Hz, Q=0.5003)
  const w2 = PI2 * 38.13547087602444 / sr;
  const alpha2 = Math.sin(w2) / (2 * 0.5003270373238773);
  const cosw2 = Math.cos(w2), ha0 = 1 + alpha2;
  const hp = {
    b0: ((1+cosw2)/2)/ha0, b1: (-(1+cosw2))/ha0, b2: ((1+cosw2)/2)/ha0,
    a1: (-2*cosw2)/ha0, a2: (1-alpha2)/ha0
  };
  return { shelf, hp };
}

// ─── LUFS Processor ───
class LUFSProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    const sr = sampleRate; // AudioWorkletGlobalScope global
    const coeffs = K_WEIGHT[sr] || computeKWeightCoeffs(sr);
    this._sc = coeffs.shelf;
    this._hc = coeffs.hp;

    // Per-channel biquad filter states (2 channels max)
    this._f = [];
    for (let ch = 0; ch < 2; ch++) {
      this._f.push({ s_z1: 0, s_z2: 0, h_z1: 0, h_z2: 0 });
    }

    // Block accumulation: 400ms block, 100ms step
    this._blockSamples = Math.round(0.4 * sr);
    this._stepSamples = Math.round(0.1 * sr);

    // Ring buffer for K-weighted squared samples per channel
    this._ringSize = this._blockSamples + 256;
    this._ringA = new Float64Array(this._ringSize);
    this._ringB = new Float64Array(this._ringSize);
    this._ringPos = 0;
    this._totalSamples = 0;
    this._stepCounter = 0;

    // Block power history for integrated LUFS (up to ~1h at 10 blocks/s)
    this._maxBlocks = 36000;
    this._blockPowers = new Float64Array(this._maxBlocks);
    this._blockCount = 0;

    // True peak tracking
    this._truePeak = 0;

    // Absolute gate threshold: -70 LUFS in linear power
    this._absGate = Math.pow(10, (-70 + 0.691) / 10);

    // Volume compensation (1 / audio.volume) to undo volume changes before measuring
    this._volComp = 1.0;

    // Current track being measured
    this._trackId = null;
    this._active = false;

    this.port.onmessage = (e) => {
      const d = e.data;
      switch (d.type) {
        case 'reset':
          this._reset();
          this._trackId = d.trackId || null;
          this._active = true;
          break;
        case 'finalize':
          if (this._active) this._finalize(!!d.partial);
          break;
        case 'stop':
          this._active = false;
          this._trackId = null;
          break;
        case 'volumeCompensation':
          this._volComp = d.value || 1.0;
          break;
      }
    };
  }

  process(inputs, outputs) {
    const input = inputs[0];
    const output = outputs[0];

    // Pass-through: copy input to output regardless of measurement state
    if (input && output) {
      for (let ch = 0; ch < output.length; ch++) {
        const inCh = ch < input.length ? input[ch] : null;
        if (inCh) {
          output[ch].set(inCh);
        } else {
          output[ch].fill(0);
        }
      }
    }

    // Only measure if active
    if (!this._active || !input || input.length === 0) return true;

    const numCh = Math.min(input.length, 2);
    const len = input[0].length; // typically 128

    const sc = this._sc;
    const hc = this._hc;
    const comp = this._volComp;

    for (let i = 0; i < len; i++) {
      for (let ch = 0; ch < numCh; ch++) {
        let x = input[ch][i] * comp;

        // Track true peak (on compensated signal)
        const abx = x < 0 ? -x : x;
        if (abx > this._truePeak) this._truePeak = abx;

        // K-weighting Stage 1: High shelf biquad (Direct Form II transposed)
        const f = this._f[ch];
        let y = sc.b0 * x + f.s_z1;
        f.s_z1 = sc.b1 * x - sc.a1 * y + f.s_z2;
        f.s_z2 = sc.b2 * x - sc.a2 * y;

        // K-weighting Stage 2: High pass biquad
        x = y;
        y = hc.b0 * x + f.h_z1;
        f.h_z1 = hc.b1 * x - hc.a1 * y + f.h_z2;
        f.h_z2 = hc.b2 * x - hc.a2 * y;

        // Store K-weighted squared sample in ring buffer
        const ring = ch === 0 ? this._ringA : this._ringB;
        ring[this._ringPos % this._ringSize] = y * y;
      }

      this._ringPos++;
      this._totalSamples++;
      this._stepCounter++;

      // Every step (100ms), compute a block if we have enough samples
      if (this._stepCounter >= this._stepSamples) {
        this._stepCounter = 0;
        if (this._totalSamples >= this._blockSamples) {
          this._computeBlock(numCh);
        }
      }
    }

    return true;
  }

  _computeBlock(numCh) {
    const bs = this._blockSamples;
    const ringSize = this._ringSize;
    // Sum K-weighted squared samples over the block for each channel
    let blockPower = 0;
    for (let ch = 0; ch < numCh; ch++) {
      const ring = ch === 0 ? this._ringA : this._ringB;
      let sumSq = 0;
      for (let j = 0; j < bs; j++) {
        const idx = (this._ringPos - bs + j + ringSize * 2) % ringSize;
        sumSq += ring[idx];
      }
      // Channel weight: 1.0 for L/R (channels 0,1), 1.41 for surround (3+)
      blockPower += sumSq / bs;
    }

    if (this._blockCount < this._maxBlocks) {
      this._blockPowers[this._blockCount] = blockPower;
      this._blockCount++;
    }
  }

  _finalize(partial) {
    const trackId = this._trackId;
    this._active = false;

    if (this._blockCount === 0) {
      this.port.postMessage({
        type: 'result',
        trackId,
        lufs: -Infinity,
        peak: this._truePeak,
        blockCount: 0,
        partial
      });
      return;
    }

    // Step 1: Absolute gate — keep blocks above -70 LUFS
    const absGate = this._absGate;
    let ungatedSum = 0;
    let ungatedCount = 0;
    for (let i = 0; i < this._blockCount; i++) {
      if (this._blockPowers[i] > absGate) {
        ungatedSum += this._blockPowers[i];
        ungatedCount++;
      }
    }

    if (ungatedCount === 0) {
      this.port.postMessage({
        type: 'result',
        trackId,
        lufs: -Infinity,
        peak: this._truePeak,
        blockCount: this._blockCount,
        partial
      });
      return;
    }

    // Ungated loudness
    const ungatedMean = ungatedSum / ungatedCount;
    const ungatedLUFS = -0.691 + 10 * Math.log10(ungatedMean);

    // Step 2: Relative gate (ungated LUFS - 10 LU)
    const relThreshold = Math.pow(10, (ungatedLUFS - 10 + 0.691) / 10);
    let gatedSum = 0;
    let gatedCount = 0;
    for (let i = 0; i < this._blockCount; i++) {
      if (this._blockPowers[i] > relThreshold) {
        gatedSum += this._blockPowers[i];
        gatedCount++;
      }
    }

    let lufs;
    if (gatedCount > 0) {
      lufs = -0.691 + 10 * Math.log10(gatedSum / gatedCount);
    } else {
      lufs = ungatedLUFS;
    }

    this.port.postMessage({
      type: 'result',
      trackId,
      lufs,
      peak: this._truePeak,
      blockCount: this._blockCount,
      partial
    });
  }

  _reset() {
    // Clear filter states
    for (let ch = 0; ch < this._f.length; ch++) {
      this._f[ch].s_z1 = 0;
      this._f[ch].s_z2 = 0;
      this._f[ch].h_z1 = 0;
      this._f[ch].h_z2 = 0;
    }
    // Clear ring buffers
    this._ringA.fill(0);
    this._ringB.fill(0);
    this._ringPos = 0;
    this._totalSamples = 0;
    this._stepCounter = 0;
    // Clear block powers
    this._blockCount = 0;
    // Reset peak
    this._truePeak = 0;
    // Reset volume compensation
    this._volComp = 1.0;
  }
}

registerProcessor('lufs-processor', LUFSProcessor);
