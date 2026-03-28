/**
 * audio-ref.js
 * Shared mutable reference to the audio engine and active audio element.
 * Import this in any module that needs access to `engine` or `audio`.
 * app.js sets these after engine initialization and keeps them in sync.
 */
export const audioRef = {
  engine: null,
  audio: null,
  audioA: null,
  audioB: null,
};

/** Volume scale applied to secondary audio elements (video player desynced audio, etc.) */
export const VOLUME_SCALE = 0.3;
