// ─── SoundCloud Source Plugin ───────────────────────────────────────────────
// Registers SoundCloud as a fallback song source in Snowify.
// The actual stream resolution is handled by the built-in yt-dlp integration
// via the `scsearch1:` URL prefix. This plugin simply adds the source to the UI
// so users can enable/disable and reorder it in Settings → Playback → Sources.
//
// When SoundCloud is in the user's enabled song sources list (and not primary),
// Snowify will try it automatically if the primary source (YouTube) fails.
// ─────────────────────────────────────────────────────────────────────────────
(function () {
  'use strict';

  function register() {
    if (!window.SnowifySources) {
      // SnowifySources initializes in finishInit — retry shortly if not ready yet
      setTimeout(register, 100);
      return;
    }

    window.SnowifySources.registerSongSource({
      id: 'soundcloud',
      label: 'SoundCloud',
      desc: 'Stream from SoundCloud as a fallback when YouTube is unavailable.',
    });
  }

  register();
})();
