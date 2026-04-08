# The RGX Book

Welcome to **rgx** — a high-performance, programmable regex engine for Rust.

rgx does everything a traditional regex engine does: find patterns, extract captures, replace text, split strings. But it goes further. With rgx, you can run code *inside* a match, steer the engine's behavior from callbacks, watch the engine work with structured events, suspend matching for async I/O, and monitor files in real time.

## Who is this book for?

- **Rust developers** who need regex and want an API that feels native
- **Systems programmers** who need safety limits, byte-level matching, or file watching
- **Application developers** who want programmable patterns with embedded Lua, JavaScript, or Rhai
- **Anyone migrating from PCRE2** — rgx covers 98% of PCRE2's feature set

## How this book is organized

**Part I: Getting Started** covers what you need for 90% of regex work — finding, capturing, replacing, splitting. If you've used regex before, you'll be productive in minutes.

**Part II: Core API** dives into the full type system — `Match`, `Captures`, iterators, `RegexSet`, caching, byte matching, safety limits, and error diagnostics.

**Part III: Advanced Features** covers Unicode semantics, match semantics, partial matching for streaming, zero-allocation capture loops, and the `Replacer` trait.

**Part IV: Host Integration** is where rgx becomes unique — passing data into patterns, running callbacks during matching, steering the engine, structured events, async I/O, and file watching.

**Part V: Real World** has complete, copy-paste-ready examples: log monitor, tokenizer, HTTP router, data pipeline, WAF engine.

**Appendices** provide quick-lookup references for pattern syntax, PCRE2 compatibility, callback context, execution modes, and the CLI.

## Quick taste

```rust
use rgx_core::*;

// Find and replace with capture groups
let re = Regex::compile(r"(?P<first>\w+)\s(?P<last>\w+)")?;
let result = re.replace("Jane Doe", "$last, $first");
assert_eq!(result, "Doe, Jane");

// Closure-based replacement
let result = re.replace("Jane Doe", |caps: &Captures| {
    format!("{}. {}", &caps["last"], &caps["first"].chars().next().unwrap())
});
assert_eq!(result, "Doe. J");

// Case-insensitive with RegexBuilder
let re = RegexBuilder::new(r"hello").case_insensitive().build()?;
assert!(re.is_match("HELLO WORLD"));

// Multi-pattern matching
let set = RegexSet::new(&[r"\d+", r"[a-z]+", r"[A-Z]+"])?;
let m = set.matches("abc 123 XYZ");
assert!(m.matched_all());
# Ok::<(), Box<dyn std::error::Error>>(())
```

Ready? Let's start with [your first match](./getting-started/first-match.md).
