# RegexCache

Compiling a regex is the expensive operation in any regex engine. If your
application uses the same patterns repeatedly -- or worse, constructs patterns
dynamically from user input -- you want to avoid recompiling the same string
over and over. `RegexCache` solves this: it stores compiled `Regex` instances
in a thread-safe LRU cache, returning cheap `Arc<Regex>` handles on cache
hits.

## Creating a cache

```rust
# use rgx_core::RegexCache;
let cache = RegexCache::new(128);  // room for 128 compiled patterns
```

The capacity determines how many patterns can be cached simultaneously.
When the cache is full and a new pattern is inserted, the oldest entry is
evicted.

## Getting a compiled regex

### `get` -- compile-on-miss with Pure mode

The most common operation: give me a compiled regex for this pattern, using
`ExecutionMode::Pure` (maximum performance, no code blocks):

```rust
# use rgx_core::RegexCache;
let cache = RegexCache::new(64);

let re = cache.get(r"\d+")?;        // compiles on first call
let re2 = cache.get(r"\d+")?;       // instant -- returns cached Arc

// Both handles point to the same compiled regex
assert!(std::sync::Arc::ptr_eq(&re, &re2));
# Ok::<(), Box<dyn std::error::Error>>(())
```

The returned `Arc<Regex>` can be cloned cheaply and used from any thread.

### `get_with_mode` -- compile with a specific execution mode

If your patterns contain code blocks, you need `ExecutionMode::Safe` or
`ExecutionMode::Full`. The cache keys on both the pattern string *and* the
mode, so the same pattern compiled in different modes produces separate
entries:

```rust
# use rgx_core::{RegexCache, ExecutionMode};
let cache = RegexCache::new(64);

let pure = cache.get(r"\d+")?;
let safe = cache.get_with_mode(r"\d+", ExecutionMode::Safe)?;

// Different modes = different compiled regexes
assert!(!std::sync::Arc::ptr_eq(&pure, &safe));
assert_eq!(cache.len(), 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Error handling

If a pattern is invalid, `get` returns an `Err`. Invalid patterns are
**not** cached -- the next call with the same pattern will attempt
compilation again:

```rust
# use rgx_core::RegexCache;
let cache = RegexCache::new(64);

assert!(cache.get(r"(unclosed").is_err());
assert_eq!(cache.len(), 0);  // nothing was cached
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is intentional: a transient compilation failure (e.g., due to a
bug in dynamic pattern construction) should not permanently poison the
cache.

## Inspecting the cache

```rust
# use rgx_core::RegexCache;
let cache = RegexCache::new(64);
assert!(cache.is_empty());

cache.get(r"\d+")?;
cache.get(r"\w+")?;
assert_eq!(cache.len(), 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Clearing the cache

```rust
# use rgx_core::RegexCache;
let cache = RegexCache::new(64);
cache.get(r"\d+")?;
cache.get(r"\w+")?;

cache.clear();
assert!(cache.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

After clearing, subsequent `get` calls will recompile.

## LRU eviction

When the cache reaches capacity, the **least-recently-inserted** entry is
evicted to make room for the new one:

```rust
# use rgx_core::RegexCache;
let cache = RegexCache::new(2);  // tiny cache for demonstration

cache.get(r"a")?;  // cache: [a]
cache.get(r"b")?;  // cache: [a, b]
cache.get(r"c")?;  // evicts "a", cache: [b, c]

assert_eq!(cache.len(), 2);
// "a" was evicted, so the next get will recompile it
# Ok::<(), Box<dyn std::error::Error>>(())
```

Choose your capacity based on how many distinct patterns your application
uses concurrently. A good heuristic: set it to 2-3x the number of patterns
you expect in a steady-state workload.

## Thread safety

`RegexCache` is fully thread-safe. Internally it uses a `RwLock` so that
multiple threads can read from the cache simultaneously, and compilation
(the slow path) only holds a write lock for the duration of the insert:

```rust
# use rgx_core::RegexCache;
use std::sync::Arc;
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
```

The `Arc<Regex>` handles returned by `get` can be sent to other threads and
used concurrently -- no further synchronization is needed.

### Double-check on the slow path

When two threads try to compile the same pattern simultaneously, the cache
uses a double-check pattern: after compiling under the write lock, it checks
whether another thread inserted the same key in the meantime and reuses that
entry. This prevents duplicate compilation work.

## When to use `RegexCache`

### Dynamic patterns from user input

If your application accepts regex patterns from users (e.g., a search box,
a log filter, a routing rule), `RegexCache` prevents the same search term
from being recompiled on every request:

```rust
# use rgx_core::RegexCache;
let cache = RegexCache::new(256);

fn search(cache: &RegexCache, pattern: &str, text: &str) -> bool {
    match cache.get(pattern) {
        Ok(re) => re.is_match(text),
        Err(_) => false,  // invalid pattern
    }
}

// First search compiles; subsequent searches with the same pattern are instant
assert!(search(&cache, r"\bfoo\b", "foo bar"));
assert!(search(&cache, r"\bfoo\b", "baz foo qux"));
```

### Configuration-driven patterns

Applications that load patterns from config files or databases benefit from
caching because the same config may be re-read on hot-reload:

```rust
# use rgx_core::RegexCache;
let cache = RegexCache::new(128);

// Simulate loading patterns from config
let patterns = vec![r"\berror\b", r"\bwarn\b", r"\bfatal\b"];
for pat in &patterns {
    let re = cache.get(pat)?;
    // Use re for matching...
}

// On config reload, cached patterns return instantly
for pat in &patterns {
    let re = cache.get(pat)?;  // cache hit
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When NOT to use it

If all your patterns are known at compile time and constructed once during
initialization, a simple `Regex::compile` stored in a struct or a `lazy`
static is simpler and avoids the overhead of hash lookups. `RegexCache`
shines specifically when pattern strings arrive at runtime.

## Sizing guide

| Workload | Suggested capacity |
|----------|--------------------|
| Fixed set of 10-20 patterns | 32 |
| User-facing search with moderate diversity | 128-256 |
| Log analysis pipeline with many pattern variants | 512-1024 |
| High-cardinality dynamic patterns (e.g., per-user rules) | 2048+ |

The memory cost per cached entry is the compiled regex plus the pattern
string. For typical patterns this is a few kilobytes, so even a capacity of
1024 uses only a few megabytes.
