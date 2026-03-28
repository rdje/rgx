# RUST CODEBASE ANALYSIS
Live roadmap-grounded analysis of the Rust workspace in `rgx`.
## Why this file exists
- Capture what the Rust codebase actually ships today versus what `ROADMAP.md` is asking for next.
- Separate verified implementation state from older aspirations and stale guidance.
- Give future sessions one accurate Rust-specific status document to refresh when behavior changes.
## Current verified snapshot
- `README.md` remains the canonical repository entry point and onboarding map.
- Validation snapshot:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm` => pass
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` => pass
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` => pass with warnings
- Current large-file concentration is still dominated by `rgx-core`:
  - `rgx-core/src/vm.rs`: 3960 lines
  - `rgx-core/src/lib.rs`: 2437 lines
  - `rgx-core/src/execution.rs`: 1912 lines
  - `rgx-core/src/lexer.rs`: 1832 lines
  - `rgx-core/src/parser.rs`: 1190 lines
- Current scaffold concentration remains visible in several near-empty modules/crates:
  - `rgx-core/src/javascript.rs`
  - `rgx-core/src/wasm.rs`
  - `rgx-core/src/cache.rs`
  - `rgx-core/src/simd.rs`
  - `rgx-bench/src/lib.rs`
  - `rgx-wasm/src/lib.rs`
## Executive summary
- The default Rust workspace is real, green, and centered on `rgx-core`.
- The strongest shipped path is still `lexer/parser -> AST -> compiler -> VM -> engine/API`, but it is no longer limited to pure regex only.
- Embedded code-block execution is now implemented in the public path for Lua, JavaScript, Rust-native callbacks, and registered wasm modules:
  - parser already recognizes `(?{lang:code})`
  - compiler validates code blocks against `ExecutionMode` and cargo features
  - VM lowers code blocks into a first-class inline opcode and executes them during matching
  - engine attaches a shared `ExecutionManager` only when compiled programs actually contain code blocks
  - current match text, numbered captures, named captures, and host-provided variables are materialized into the execution context
  - winning-path non-boolean Lua/JavaScript/native results are surfaced through `MatchResult.code_result`
  - `Regex::find_first_numeric_with_code(...)` and `Regex::find_all_numeric_with_code(...)` now collect winning-path `Numeric(f64)` payloads in match order while skipping non-numeric matches
  - `Regex::replace_first_with_code(...)` and `Regex::replace_all_with_code(...)` now consume winning-path `Replacement(String)` payloads while copying non-replacement matches through unchanged
  - native callbacks can be registered on compiled regex objects through the public Rust API and dispatched through the shared runtime
  - host-provided variables can be registered on compiled regex objects through the public Rust API via `Regex::set_variable(...)` and are snapshotted into each code-block evaluation
  - wasm modules can be registered on compiled regex objects through the public Rust API and dispatched through a wasmtime-backed runtime with read-only `rgx` host imports for position, full input text, numbered captures, named captures, and host-provided variables
- The biggest remaining gaps are now narrower and clearer:
  - `ExecutionMode::Pure` still rejects all code blocks by design
  - `native` and `wasm` code blocks are shipped only on the Rust API path; the CLI still has no registration/configuration surface for them
  - the current wasm ABI still lacks richer non-boolean result handling even though Lua/JavaScript/native now expose a first richer-result slice plus dedicated numeric/replacement Rust APIs
  - `pgen-parser` is still a contract-validation path, not a true alternative parser backend
  - automated validation still misses the feature matrix and benchmark trend capture
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
  - numbered captures, named captures, and host-provided variables are available when their groups have completed or have been set through the Rust API
  - code blocks participate in backtracking and may execute multiple times during one overall match search
  - `ExecResult::Success` continues matching
  - `Failure` and `Error` fail the current path
  - Lua/JavaScript/native `Numeric` and `Replacement` results now continue matching and the last winning-path non-boolean value is exposed through `MatchResult.code_result`
  - `Regex::find_first_numeric_with_code(...)` / `Regex::find_all_numeric_with_code(...)` collect winning-path `Numeric(f64)` values in match order and skip matches whose winning path produced only predicate or replacement results
  - `Regex::replace_first_with_code(...)` / `Regex::replace_all_with_code(...)` consume winning-path `Replacement(String)` values and preserve matched text unchanged when the winning path produces only predicate or numeric results
  - the current wasm ABI keeps `module:function` with an exported `() -> i32` predicate and adds `rgx` imports for `position`, `text_length` / `text_read`, `capture_count` / `capture_length` / `capture_read`, `named_capture_count` / `named_capture_name_length` / `named_capture_name_read` / `named_capture_value_length` / `named_capture_value_read`, and `variable_count` / `variable_name_length` / `variable_name_read` / `variable_value_length` / `variable_value_read`
  - wasm capture slot `0` is still the current overall match prefix, named captures and host-provided variables are exposed in lexicographic order by name, all read-style imports require exported linear memory named `memory`, and wasm remains predicate-only on the result side
## Explicit boundaries that remain in place
- `ExecutionMode::Pure` rejects code blocks with an explicit compile error.
- `ExecutionMode::Safe` still rejects `native` code blocks; they require `ExecutionMode::Full`.
- The CLI still has no native- or wasm-registration surface, so those shipped slices are currently Rust-API-only.
- The current wasm ABI is intentionally smaller than the Lua/JavaScript/native context surface and still does not expose richer non-boolean result handling.
- Backreferences, recursion, conditionals, and Unicode property classes remain parsed-but-unintegrated and continue to fail explicitly at compile time.
- Registering a native callback on a regex that has no attached execution manager still returns an explicit engine error.
- Registering a wasm module on a regex that has no attached execution manager still returns an explicit engine error.
## Codebase realities that matter for roadmap prioritization
- `Compiler::feature_validation_message()` is a critical safety boundary because `OptimizingCompiler::codegen_pass()` still carries placeholder branches for unsupported AST families, including a dead `UnicodeClass -> Any` fallback and a default `_ => Fail` path.
- The declared opcode surface in `rgx-core/src/vm.rs` still exceeds the emitted/decoded/runtime-used surface; several opcode families remain aspirational or only partially wired.
- The `pgen-parser` feature path is still a recursive-descent fallback. `PatternAnalysis` and `ParserConfig` remain unused scaffolding, and `parsing::parser_capabilities()` under the feature flag still advertises `error_recovery` / `syntax_highlighting` differently from the actual fallback-backed `PgenParser::capabilities()` implementation.
- Local git-tracked PGEN parser issue recording is now prepared through `pgen-issues/`, `pgen-issues/TEMPLATE.yaml`, and `scripts/new-pgen-issue.sh`, so real-backend rollout can preserve RGX-side bug context and upstream links consistently.
- The default local CI path in `scripts/run-local-ci.sh` validates `fmt`, default-feature workspace tests, and `clippy`, but it does not continuously cover `pgen-parser`, `lua`, `javascript`, `wasm`, or `all-languages`. Those checks are still a manual matrix.
- Benchmark infrastructure exists in `rgx-bench`, but benchmark trend capture is still ad hoc and separate from automated validation.
## Roadmap alignment
### Now
- PCRE2 parity hardening remains active and well-supported by tests and docs.
- Capability hardening improved again because the wasm named-capture and variable imports extend shipped behavior without changing the public regex syntax or registration model.
- Embedded code execution is no longer parsed-only scaffolding; Lua/JavaScript/native/wasm are real shipped slices on the documented Rust API path.
### Next
- Design the next higher-value wasm/runtime slice beyond the current position/text/numbered-capture/named-capture/variable imports, `MatchResult.code_result`, and numeric/replacement Rust helper APIs, most likely richer wasm result handling.
- Decide whether native/wasm registration should remain Rust-API-only or gain configured CLI/external surfaces later.
- Replace the fallback-backed `pgen-parser` contract path with a real parser backend and make capability reporting fully truthful.
- Operationalize automated feature-matrix coverage and benchmark trend capture instead of relying on manual runs.
### Later
- Finish larger regex-surface gaps: backreferences, recursion, conditionals, Unicode property classes, and the still-declared-but-unwired opcode families.
## Practical engineering notes
- Inline code blocks are encoded directly into VM bytecode, which avoids an external callout table and keeps subprogram lowering simple.
- Named-capture metadata is derived once during compilation and stored with the compiled program for runtime callout access.
- Lua execution is sandboxed per invocation rather than reusing one mutable runtime, which makes speculative execution under backtracking/probing safer.
- JavaScript execution is still per-invocation via QuickJS and is wrapped so documented `return ...` style code works inside `(?{js:...})` blocks.
- Native callback storage uses shared interior mutability, so the `Arc<ExecutionManager>` attached to the VM can receive post-compilation registrations without swapping runtime instances.
- Host-provided execution variables now live on the shared `ExecutionManager` and are snapshotted into each per-call `ExecContext`, which keeps callout inputs deterministic under backtracking while still allowing Rust API updates between matches.
- Wasm module storage follows the same shared-runtime model, with compiled modules registered once and instantiated on demand through wasmtime.
- Wasm execution uses a linker plus per-call store data so host imports can expose read-only regex context and copy bytes into guest memory while trapping explicit malformed guest interactions.
- The named-capture and variable wasm imports materialize deterministic sorted views at host-call time, which keeps the guest-visible ABI stable without changing the underlying `HashMap` storage model.
- Root `rgx-core/src/javascript.rs` and `rgx-core/src/wasm.rs`, plus `rgx-core/src/cache.rs`, `rgx-core/src/simd.rs`, `rgx-bench/src/lib.rs`, and `rgx-wasm/src/lib.rs`, remain scaffold-level placeholders despite the real execution logic living elsewhere.
## High-confidence next actions
1. Design and ship the next richer-result layer beyond `MatchResult.code_result` plus the current numeric/replacement helper APIs, especially wasm result handling.
2. Decide whether native/wasm registration should stay Rust-API-only or gain configured CLI/external surfaces.
3. Replace the fallback `pgen-parser` feature path with a real parser implementation and align parser capability reporting with reality.
4. Add automated feature-matrix coverage and benchmark-trend capture to the default validation loop.
5. Reduce warning debt in `vm.rs`, `execution.rs`, `parser.rs`, `lexer.rs`, `lib.rs`, `ast.rs`, and `token.rs`.
