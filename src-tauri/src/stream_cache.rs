//! Port of `src/main/stream-cache.js`: resolved stream URLs, kept for 4 hours.

use std::collections::HashMap;

pub const STREAM_CACHE_TTL_MS: i64 = 4 * 60 * 60 * 1000;

/// Times are `Date.now()` milliseconds, passed in by the caller.
#[derive(Debug)]
pub struct StreamCache<V> {
    entries: HashMap<String, (V, i64)>,
}

impl<V> Default for StreamCache<V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<V: Clone> StreamCache<V> {
    /// `getCachedUrl`: the value, unless older than the TTL, in which case it is removed.
    pub fn get(&mut self, key: &str, now: i64) -> Option<V> {
        let (value, ts) = self.entries.get(key)?;
        if now - ts > STREAM_CACHE_TTL_MS {
            self.entries.remove(key);
            return None;
        }
        Some(value.clone())
    }

    /// `setCachedUrl`: stores or replaces. Above 200 entries, sweeps out the expired ones
    /// (unexpired entries are never evicted, so the cache can stay above 200).
    pub fn set(&mut self, key: String, value: V, now: i64) {
        self.entries.insert(key, (value, now));
        if self.entries.len() > 200 {
            self.entries
                .retain(|_, (_, ts)| now - *ts <= STREAM_CACHE_TTL_MS);
        }
    }

    /// Entry count (the source's `_streamCache.size`).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
