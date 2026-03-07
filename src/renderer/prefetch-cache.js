/**
 * Prefetch Cache — sliding window audio pre-downloader
 *
 * Downloads upcoming tracks to disk so playback continues offline.
 * Uses a configurable window size (prefetchCount) to determine how
 * many tracks ahead to cache. Downloads are sequential (one at a time).
 *
 * Usage:
 *   const cache = window.PrefetchCache({ getState, downloadAudio, deleteCachedAudio, clearCache, cancelDownload });
 *   cache.setCount(3);
 *   cache.onTrackChanged(queueIndex, queue);
 *   const path = cache.getCachedPath(trackId); // file path or null
 */
window.PrefetchCache = function PrefetchCache(opts) {
  'use strict';

  const { getState, downloadAudio, deleteCachedAudio, clearCache, cancelDownload } = opts;
  let _onCacheUpdate = null;

  // ─── Internal state ───
  const _cache = new Map(); // videoId → { path: string, status: 'ready'|'downloading' }
  let _downloadQueue = [];  // videoIds pending download
  let _isDownloading = false;
  let _currentDownloadId = null;
  let _prefetchCount = 0;   // 0 = off
  let _destroyed = false;
  let _generation = 0;      // incremented on clear/onTrackChanged to invalidate stale loops

  // ─── Public API ───

  /** Returns the local file path if the track is cached, or null */
  function getCachedPath(trackId) {
    const entry = _cache.get(trackId);
    return (entry && entry.status === 'ready') ? entry.path : null;
  }

  /** Called when the current track changes — recalculates the cache window */
  function onTrackChanged(queueIndex, queue) {
    if (_destroyed || _prefetchCount === 0) return;

    const windowIds = _computeWindow(queueIndex, queue);

    // Evict entries outside the window
    for (const [id, entry] of _cache) {
      if (!windowIds.has(id)) {
        if (entry.status === 'ready' && entry.path) {
          deleteCachedAudio(entry.path).catch(() => {});
        }
        _cache.delete(id);
      }
    }

    // Cancel active download if it's no longer in window
    if (_currentDownloadId && !windowIds.has(_currentDownloadId)) {
      cancelDownload().catch(() => {});
      _cache.delete(_currentDownloadId);
      _currentDownloadId = null;
    }

    // Invalidate any running _processQueue loop
    _generation++;
    _isDownloading = false;

    // Build download queue — IDs in window that aren't cached yet
    _downloadQueue = [];
    for (const id of windowIds) {
      if (!_cache.has(id)) {
        _downloadQueue.push(id);
      }
    }

    _processQueue();
  }

  /** Called when a track finishes playing — removes it from cache */
  function onTrackFinished(trackId) {
    if (_destroyed) return;
    const entry = _cache.get(trackId);
    if (entry && entry.path) {
      deleteCachedAudio(entry.path).catch(() => {});
    }
    _cache.delete(trackId);
  }

  /** Update the prefetch count (0 = off, N = tracks ahead) */
  function setCount(n) {
    _prefetchCount = n;
  }

  /** Register callback for when a track finishes downloading */
  function onCacheUpdateCb(fn) {
    _onCacheUpdate = fn;
  }

  /** Cancel all downloads and delete all cached files */
  function clear() {
    _generation++; // invalidate any running _processQueue loop
    if (_isDownloading) {
      cancelDownload().catch(() => {});
      _isDownloading = false;
      _currentDownloadId = null;
    }
    _downloadQueue = [];

    // Delete individual files
    for (const [, entry] of _cache) {
      if (entry.path) deleteCachedAudio(entry.path).catch(() => {});
    }
    _cache.clear();
  }

  /** Full cleanup — clear cache + wipe directory */
  function destroy() {
    _destroyed = true;
    clear();
    clearCache().catch(() => {});
  }

  // ─── Internals ───

  /** Compute the set of videoIds that should be in the cache window */
  function _computeWindow(queueIndex, queue) {
    const ids = new Set();
    if (!queue || !queue.length) return ids;

    // Include current track so its file isn't evicted while playing
    const current = queue[queueIndex];
    if (current && current.id) ids.add(current.id);

    const start = queueIndex + 1;
    const end = _prefetchCount === -1
      ? queue.length
      : Math.min(queue.length, start + _prefetchCount);

    for (let i = start; i < end; i++) {
      const track = queue[i];
      if (track && track.id) ids.add(track.id);
    }
    return ids;
  }

  /** Process the download queue sequentially */
  async function _processQueue() {
    if (_isDownloading || _destroyed || !_downloadQueue.length) return;
    _isDownloading = true;
    const gen = _generation;

    while (_downloadQueue.length && !_destroyed && gen === _generation) {
      const videoId = _downloadQueue.shift();

      // Skip if already cached (might have been added while waiting)
      if (_cache.has(videoId)) continue;

      // Find the track in the current queue to get its URL
      const currentQueue = getState().queue;
      const track = currentQueue.find(t => t.id === videoId);
      if (!track || !track.url) continue;

      _currentDownloadId = videoId;
      _cache.set(videoId, { path: null, status: 'downloading' });

      try {
        const state = getState();
        const result = await downloadAudio(track.url, state.audioQuality, videoId);
        if (_destroyed || gen !== _generation) break;

        // Verify the entry still exists (might have been evicted)
        if (_cache.has(videoId)) {
          _cache.set(videoId, { path: result.path, status: 'ready' });
          if (_onCacheUpdate) _onCacheUpdate(videoId);
        }
      } catch (err) {
        // 'cancelled' is expected when evicted — anything else is a real error
        if (err !== 'cancelled') {
          console.warn('Prefetch download failed:', videoId, err);
        }
        _cache.delete(videoId);
      }

      _currentDownloadId = null;
    }

    if (gen === _generation) _isDownloading = false;
  }

  return { getCachedPath, onTrackChanged, onTrackFinished, setCount, onCacheUpdateCb, clear, destroy };
};
