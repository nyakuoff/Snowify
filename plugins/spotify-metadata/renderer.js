// ─── Spotify Metadata Plugin ─────────────────────────────────────────────────
// Registers Spotify as a metadata source in Snowify.
// When enabled, Snowify enriches each played track in the background with:
//   • Genre tags (up to 5)
//   • Popularity score (0–100)
//   • Artist images (thumbnail, standard, large)
//   • Album art URL
//
// Enriched data is cached locally in snowify_genre_cache and used by features
// like Wrapped. The actual API requests are handled in the main process by
// src/main/spotify-meta.js via the `meta:enrichTrack` IPC channel.
// ─────────────────────────────────────────────────────────────────────────────
(function () {
  'use strict';

  function register() {
    if (!window.SnowifySources) {
      setTimeout(register, 100);
      return;
    }

    window.SnowifySources.registerMetaSource({
      id: 'spotify',
      label: 'Spotify',
      desc: 'Enrich tracks with genres, popularity, and artist images via Spotify.',
    });
  }

  register();
})();
