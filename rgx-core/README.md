# rgx-core

[![crates.io](https://img.shields.io/crates/v/rgx-core.svg)](https://crates.io/crates/rgx-core)
[![docs.rs](https://docs.rs/rgx-core/badge.svg)](https://docs.rs/rgx-core)

**rgx-core** is a high-performance, programmable regex engine for Rust. It covers ~99% of PCRE2's feature surface, ships a Cranelift-backed JIT, and lets you run real code — Lua, JavaScript, Rhai, WebAssembly, or native Rust closures — *inside* a match.

```rust
use rgx_core::*;

// Familiar regex API
let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})-(?P<day>\d{2})")?;
for caps in re.captures_iter("built 2026-04-13, updated 2026-05-01") {
    println!("{}/{}/{}", &caps["year"], &caps["month"], &caps["day"]);
}

// ...with closure-based replacement
let re = Regex::compile(r"(\w+)@(\w+)")?;
let out = re.replace_all("contact alice@acme", |caps: &Captures| {
    format!("{} [at] {}", &caps[1], &caps[2])
});

// ...and tail a log file with OS-native watching (kqueue/inotify)
let _handle = re.tail_file("app.log", TailOptions::default(), |fm| {
    eprintln!("line {}: {}", fm.line_number, fm.line);
});
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Highlights

- **~99% PCRE2 feature parity** — every PCRE2 10.47 feature except JIT binary format compatibility
- **Cranelift-backed JIT** — default-on; automatically routes JIT-eligible patterns to native code
- **4-tier dispatch chain** — DFA → Pike-VM → JIT → backtracking VM, picked per pattern
- **Programmable** — embed Lua, JavaScript, Rhai, WASM, or native Rust callbacks inside patterns
- **6-layer host integration** — data exchange, predicates, steering, events, async I/O, file matching
- **Production safety** — `set_max_steps`, `set_max_backtrack_frames`, `set_max_recursion_depth`
- **Live file watching** — `tail_file` uses kqueue/inotify with zero idle CPU
- **Multi-pattern matching** — `RegexSet` for routing/classification
- **Idiomatic Rust API** — `Match`, `Captures`, lazy iterators, `Cow<str>` returns, fluent `RegexBuilder`

## Feature flags

| Flag | Default | What it adds |
|------|---------|--------------|
| `std` | ✅ | Standard library (always on) |
| `pgen-parser` | ✅ | PGEN-backed regex grammar (the shipping parser) |
| `jit` | ✅ | Cranelift JIT compilation (~2 MiB dep closure) |
| `lua` | — | Lua code blocks `(?{lua:...})` via mlua |
| `javascript` | — | JavaScript code blocks `(?{js:...})` via QuickJS |
| `rhai` | — | Rhai code blocks `(?{rhai:...})` |
| `wasm` | — | WebAssembly module dispatch via wasmtime |
| `all-languages` | — | All four scripting backends |
| `trace` | — | Verbose execution tracing for debugging |

Opt out of Cranelift: `rgx-core = { version = "0.1", default-features = false, features = ["pgen-parser"] }`.

## Documentation

- **[The RGX Book](https://github.com/rdje/rgx/tree/main/book/src)** — 45 chapters covering every feature with examples
- **[API docs](https://docs.rs/rgx-core)** — generated from doc comments
- **[PCRE2 compatibility matrix](https://github.com/rdje/rgx/blob/main/docs/PCRE2_COMPATIBILITY_MATRIX.md)** — feature-by-feature parity status

## MSRV

Rust 1.88 (driven by PGEN's edition 2024 requirement).

## License

Apache-2.0.
