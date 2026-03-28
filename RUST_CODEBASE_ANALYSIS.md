# RUST CODEBASE ANALYSIS
Live roadmap-grounded analysis of the Rust workspace in `rgx`.
## Why this file exists
- Capture what the Rust codebase actually ships today versus what `ROADMAP.md` is asking for next.
- Separate verified implementation state from older aspirations and stale guidance.
- Give future sessions one accurate Rust-specific status document to refresh when behavior changes.
## Current verified snapshot
- `README.md` remains the canonical repository entry point and onboarding map.
- Validation snapshot:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` => pass (125 tests)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm` => pass (135 tests)
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages` => pass
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` => pass with warnings
- Current large-file concentration is still dominated by `rgx-core`:
  - `rgx-core/src/vm.rs`: 3600+ lines
  - `rgx-core/src/lexer.rs`: 1800+ lines
  - `rgx-core/src/parser.rs`: 1100+ lines
  - `rgx-core/src/lib.rs`: 1000+ lines
  - `rgx-core/src/execution.rs`: 1400+ lines
## Executive summary
- The default Rust workspace is real, green, and centered on `rgx-core`.
- The strongest shipped path is still `lexer/parser -> AST -> compiler -> VM -> engine/API`, but it is no longer limited to pure regex only.
- Embedded predicate execution is now implemented in the public path for Lua, JavaScript, Rust-native callbacks, and registered wasm modules:
  - parser already recognized `(?{lang:code})`
  - compiler now validates code blocks against `ExecutionMode` and cargo features
  - VM now lowers code blocks into a first-class inline opcode and executes them during matching
  - engine now attaches a shared `ExecutionManager` when compiled programs actually contain code blocks
  - current match text, numbered captures, and named captures are materialized into the execution context
  - native callbacks can now be registered on compiled regex objects through the public Rust API and dispatched through the shared runtime
  - wasm modules can now be registered on compiled regex objects through the public Rust API and dispatched through a wasmtime-backed runtime with read-only `rgx` host imports for position, full input text, and numbered captures
- The biggest remaining gaps are now narrower and clearer:
  - `ExecutionMode::Pure` still rejects all code blocks by design
  - `native` code blocks are shipped only on the Rust API path in `ExecutionMode::Full`; the CLI still has no native-registration surface
  - `wasm` code blocks are shipped only on the Rust API path in `ExecutionMode::{Safe, Full}` with a smaller import-based ABI than Lua/JavaScript/native; the CLI still has no wasm-registration surface
  - numeric/replacement return kinds are explicitly rejected in match mode
  - `pgen-parser` is still a contract-validation path, not a true alternative parser backend
  - benchmark/process maturity still lags correctness maturity
## What is shipped today
### Default public regex path
- Literals, concatenation, alternation
- Anchors including `^`, `$`, `\A`, `\Z`, and `\z`
- Shorthand and custom character classes, including negated shorthand classes
- Greedy and lazy `?`, `*`, `+`, `{n,m}`, and `{n,}` quantifiers
- Capturing, non-capturing, named, and atomic groups
- Positive and negative lookahead/lookbehind
- Top-level alternation branch reporting
### Execution-mode / feature-gated path
- `(?{lua:...})` is shipped as a predicate checkpoint in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `lua` feature is enabled.
- `(?{js:...})` and `(?{javascript:...})` are shipped as predicate checkpoints in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `javascript` feature is enabled.
- `(?{native:...})` is shipped on the Rust API path in `ExecutionMode::Full` after registering a callback on the compiled `Regex`.
- `(?{wasm:...})` is shipped on the Rust API path in `ExecutionMode::Safe` or `ExecutionMode::Full` after registering a named wasm module on the compiled `Regex`.
- Current execution-context contract for this slice:
  - `arg[0]` / capture slot `0` is the current overall match prefix for the current match attempt
  - numbered captures and named captures are available when their groups have completed
  - code blocks participate in backtracking and may execute multiple times during one overall match search
  - `ExecResult::Success` continues matching
  - `Failure` and `Error` fail the current path
  - `Numeric` and `Replacement` are rejected in match mode for now
  - the current wasm ABI keeps `module:function` with an exported `() -> i32` predicate and adds `rgx` imports for `position`, `text_length` / `text_read`, and `capture_count` / `capture_length` / `capture_read`
  - wasm capture slot `0` is still the current overall match prefix, and `text_read` / `capture_read` require exported linear memory named `memory`
## Explicit boundaries that remain in place
- `ExecutionMode::Pure` rejects code blocks with an explicit compile error.
- `ExecutionMode::Safe` still rejects `native` code blocks; they require `ExecutionMode::Full`.
- The CLI still has no native- or wasm-registration surface, so those shipped slices are currently Rust-API-only.
- The current wasm ABI is intentionally smaller than the Lua/JavaScript/native context surface and still does not expose named captures or variables to wasm modules.
- Backreferences, recursion, conditionals, and Unicode property classes remain parsed-but-unintegrated and continue to fail explicitly at compile time.
- Registering a native callback on a regex that has no attached execution manager still returns an explicit engine error.
- Registering a wasm module on a regex that has no attached execution manager still returns an explicit engine error.
## Roadmap alignment
### Now
- PCRE2 parity hardening remains active and well-supported by tests and docs.
- Capability hardening materially improved because `ExecutionMode::{Safe, Full}` now unlock a real shipped slice instead of pure scaffolding.
- Embedded code execution moved from “parsed-only” to a truthful, validated shipped slice for Lua/JavaScript predicates plus Rust-API native/wasm registration.
### Next
- Expand the wasm ABI beyond the current position/text/numbered-capture import slice.
- Design richer result semantics beyond boolean predicate checkpoints.
- Decide whether native/wasm registration should remain Rust-API-only or gain configured CLI/external surfaces later.
- Replace the fallback-backed `pgen-parser` contract path with a real parser backend.
- Operationalize benchmark trend capture instead of relying on ad hoc benches.
### Later
- Finish larger regex-surface gaps: backreferences, recursion, conditionals, Unicode property classes, and the still-declared-but-unwired opcode families.
## Practical engineering notes
- Inline code blocks are encoded directly into VM bytecode, which avoided adding an external callout table and keeps subprogram lowering simple.
- Named-capture metadata is now derived once during compilation and stored with the compiled program for runtime callout access.
- Lua execution is now sandboxed per invocation rather than reusing one mutable runtime, which makes speculative execution under backtracking/probing safer.
- JavaScript execution is still per-invocation via QuickJS and is now wrapped so documented `return ...` style code works inside `(?{js:...})` blocks.
- Native callback storage now uses shared interior mutability, so the `Arc<ExecutionManager>` attached to the VM can receive post-compilation registrations without swapping runtime instances.
- Wasm module storage now follows the same shared-runtime model, with compiled modules registered once and instantiated on demand through wasmtime.
- Wasm execution now uses a linker plus per-call store data so host imports can expose read-only regex context and copy bytes into guest memory while trapping explicit malformed guest interactions.
- Root `rgx-core/src/javascript.rs` and `rgx-core/src/wasm.rs` remain scaffold-level despite the real execution logic living in `execution.rs`.
## High-confidence next actions
1. Expand the wasm ABI beyond the current position/text/numbered-capture import slice, most likely with named-capture support next.
2. Decide whether predicate code blocks should remain strictly boolean or grow a first-class replacement/evaluation API.
3. Decide whether native/wasm registration should stay Rust-API-only or gain configured CLI/external surfaces.
4. Replace the fallback `pgen-parser` feature path with a real parser implementation.
5. Reduce warning debt in `vm.rs`, `execution.rs`, `parser.rs`, `lexer.rs`, `ast.rs`, and `token.rs`.
