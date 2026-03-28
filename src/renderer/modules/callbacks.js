/**
 * callbacks.js
 * Shared callback registry for app-level functions that cannot be imported
 * directly (because app.js is the entry point and modules cannot import from it).
 *
 * app.js populates these in finishInit() after all modules have loaded.
 * Modules import this object and call the registered callbacks.
 */
export const callbacks = {
  /** localStorage + cloud save — set by app.js */
  saveState: () => {},

  /** Non-blocking Spotify metadata enrichment for queued tracks — set by app.js */
  maybeEnrichTrackMeta: (_track) => {},

  /** Switch the main view — set by app.js */
  switchView: (_view) => {},
};
