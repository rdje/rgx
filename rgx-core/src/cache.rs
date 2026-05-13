//! Thread-safe compilation cache for regex patterns.
//!
//! Avoids recompiling the same pattern string repeatedly.
//!
//! ```rust,no_run
//! # use rgx_core::RegexCache;
//! let cache = RegexCache::new(128);
//! let re = cache.get(r"\d+").unwrap();   // compiles
//! let re2 = cache.get(r"\d+").unwrap();  // instant — returns cached Arc
//! ```

use crate::engine::ExecutionMode;
use crate::error::Result;
use crate::Regex;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Thread-safe LRU compilation cache for regex patterns.
///
/// Stores compiled [`Regex`] instances behind `Arc` so they can be shared
/// cheaply across threads. When the cache reaches capacity, the
/// least-recently-inserted entry is evicted.
pub struct RegexCache {
    inner: RwLock<CacheInner>,
}

struct CacheInner {
    map: HashMap<CacheKey, Arc<Regex>>,
    order: Vec<CacheKey>,
    capacity: usize,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    pattern: String,
    mode: u8, // ExecutionMode discriminant
}

impl CacheKey {
    fn new(pattern: &str, mode: ExecutionMode) -> Self {
        Self {
            pattern: pattern.to_string(),
            mode: match mode {
                ExecutionMode::Pure => 0,
                ExecutionMode::Safe => 1,
                ExecutionMode::Full => 2,
            },
        }
    }
}

impl RegexCache {
    /// Create a new cache with the given maximum capacity.
    ///
    /// When the cache is full, the oldest entry is evicted to make room.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: RwLock::new(CacheInner {
                map: HashMap::with_capacity(capacity),
                order: Vec::with_capacity(capacity),
                capacity,
            }),
        }
    }

    /// Get a compiled regex for the pattern, compiling it if not cached.
    ///
    /// Returns a shared `Arc<Regex>` that can be used across threads.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`](crate::error::RgxError) if the pattern is invalid
    /// and this is the first time it's been compiled.
    pub fn get(&self, pattern: &str) -> Result<Arc<Regex>> {
        self.get_with_mode(pattern, ExecutionMode::Pure)
    }

    /// Get a compiled regex with a specific execution mode.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`](crate::error::RgxError) if the pattern is invalid.
    pub fn get_with_mode(&self, pattern: &str, mode: ExecutionMode) -> Result<Arc<Regex>> {
        let key = CacheKey::new(pattern, mode);

        // Fast path: read lock
        {
            let inner = self
                .inner
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(cached) = inner.map.get(&key) {
                return Ok(cached.clone());
            }
        }

        // Slow path: compile and insert under write lock
        let regex = if mode == ExecutionMode::Pure {
            Regex::compile(pattern)?
        } else {
            Regex::with_mode(pattern, mode)?
        };
        let arc = Arc::new(regex);

        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Double-check: another thread may have inserted while we compiled.
        if let Some(cached) = inner.map.get(&key) {
            return Ok(cached.clone());
        }

        // Evict oldest if at capacity.
        if inner.order.len() >= inner.capacity && inner.capacity > 0 {
            let evicted = inner.order.remove(0);
            inner.map.remove(&evicted);
        }

        inner.order.push(key.clone());
        inner.map.insert(key, arc.clone());
        Ok(arc)
    }

    /// Number of cached entries.
    #[must_use]
    pub fn len(&self) -> usize {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.map.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove all cached entries.
    pub fn clear(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.map.clear();
        inner.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_returns_same_arc() {
        let cache = RegexCache::new(16);
        let r1 = cache.get(r"\d+").unwrap();
        let r2 = cache.get(r"\d+").unwrap();
        assert!(Arc::ptr_eq(&r1, &r2));
    }

    #[test]
    fn cache_different_patterns() {
        let cache = RegexCache::new(16);
        let r1 = cache.get(r"\d+").unwrap();
        let r2 = cache.get(r"\w+").unwrap();
        assert!(!Arc::ptr_eq(&r1, &r2));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_evicts_oldest() {
        let cache = RegexCache::new(2);
        cache.get(r"a").unwrap();
        cache.get(r"b").unwrap();
        assert_eq!(cache.len(), 2);
        cache.get(r"c").unwrap(); // evicts "a"
        assert_eq!(cache.len(), 2);
        // "a" should now recompile (not cached)
        let r1 = cache.get(r"a").unwrap();
        let r2 = cache.get(r"a").unwrap();
        assert!(Arc::ptr_eq(&r1, &r2)); // but now it's cached again
    }

    #[test]
    fn cache_invalid_pattern_returns_error() {
        let cache = RegexCache::new(16);
        assert!(cache.get(r"(unclosed").is_err());
        assert_eq!(cache.len(), 0); // errors are not cached
    }

    #[test]
    fn cache_clear() {
        let cache = RegexCache::new(16);
        cache.get(r"\d+").unwrap();
        cache.get(r"\w+").unwrap();
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_mode_separation() {
        let cache = RegexCache::new(16);
        let pure = cache.get(r"\d+").unwrap();
        let safe = cache.get_with_mode(r"\d+", ExecutionMode::Safe).unwrap();
        assert!(!Arc::ptr_eq(&pure, &safe)); // different modes = different entries
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_compiled_regex_works() {
        let cache = RegexCache::new(16);
        let re = cache.get(r"\d+").unwrap();
        assert!(re.is_match("42"));
        assert!(!re.is_match("abc"));
        let m = re.find("abc 123").unwrap();
        assert_eq!(m.as_str(), "123");
    }

    #[test]
    fn cache_thread_safe() {
        use std::thread;
        let cache = Arc::new(RegexCache::new(64));
        let mut handles = vec![];
        for i in 0..8 {
            let cache = cache.clone();
            handles.push(thread::spawn(move || {
                let pattern = format!(r"\d{{{i}}}");
                let re = cache.get(&pattern).unwrap();
                assert!(re.as_str().contains(&i.to_string()));
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(cache.len(), 8);
    }
}
