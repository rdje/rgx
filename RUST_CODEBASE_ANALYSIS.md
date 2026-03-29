# RUST CODEBASE ANALYSIS
Live roadmap-grounded analysis of the Rust workspace in `rgx`.

## Why this file exists
- Capture what the Rust codebase actually ships today versus what `ROADMAP.md` is asking for next.
- Separate verified implementation state from older aspirations and stale guidance.
- Give future sessions one accurate Rust-specific status document to refresh when behavior changes.

## Current verified snapshot
- `README.md` remains the canonical repository entry point and onboarding map.
- Validation snapshot:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core full_mode_native_code_block_can_access_match_metadata -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua safe_mode_lua_code_block_can_access_match_metadata -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript safe_mode_javascript_code_block_can_access_match_metadata -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm safe_mode_wasm_code_block_can_read_match_metadata -- --nocapture` => pass
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` => pass
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` => pass with warnings
  - `./scripts/run-local-ci.sh` => pass (with the `subs/pgen` submodule initialized)
- Current large-file concentration is still dominated by `rgx-core`:
  - `rgx-core/src/vm.rs`: 4603 lines
  - `rgx-core/src/lib.rs`: 2976 lines
  - `rgx-core/src/execution.rs`: 2099 lines
  - `rgx-core/src/lexer.rs`: 1877 lines
  - `rgx-core/src/parser.rs`: 1246 lines
- Current scaffold concentration remains visible in several near-empty modules/crates:
  - `rgx-core/src/javascript.rs`
  - `rgx-core/src/wasm.rs`
  - `rgx-core/src/cache.rs`
  - `rgx-core/src/simd.rs`
  - `rgx-bench/src/lib.rs`
  - `rgx-wasm/src/lib.rs`

## Executive summary
- The default Rust workspace is real, green, and centered on `rgx-core`.
- The strongest shipped path is still `lexer/parser -> AST -> compiler -> VM -> engine/API`, and the default local build now routes that parser stage through the real submodule-backed PGEN backend.
- Unicode property classes are now part of that shipped default path:
  - parser-path and AST-first compilation resolve `\p{...}` / `\P{...}` through Unicode property tables instead of treating them as a compile boundary
  - invalid property names now fail explicitly at compile time
  - PCRE2 differential coverage now treats representative Unicode property behavior as supported rather than as a known gap
- Numeric backreferences are now part of that shipped default path:
  - the compiler validates that numbered backreferences only target capture groups that actually exist
  - the VM now emits/decodes/executes `Backref` bytecode in both top-level and subexpression execution paths
  - parity and capability tests now treat numeric backreferences as supported behavior rather than a compile-boundary gap
- Possessive quantifiers are now part of that shipped default path:
  - both parser backends lower `*+`, `++`, `?+`, and counted possessive forms into atomic-wrapped greedy quantified AST nodes
  - runtime behavior now blocks backtracking into the possessive piece while still allowing ordinary success cases that need no suffix backtracking
  - parity and capability tests now treat possessive quantifiers as supported behavior rather than as a parser-adapter gap
- The default PGEN-backed parser path is no longer a recursive-descent placeholder:
  - `rgx-core/src/parsing.rs` now calls into the PGEN embedding API
  - the stable regex AST dump is converted into canonical RGX AST structure for groups, lookarounds, conditionals, concatenation/alternation/pieces, and quantifiers
  - leaf atoms are re-parsed from exact source slices through the recursive-descent parser so RGX AST semantics stay aligned for literals, classes, escapes, code blocks, recursion leaves, and related terminals
  - local backend choice under the default PGEN-backed build is intentionally controlled by one constant (`PGEN_FEATURE_BACKEND`) so RGX can flip between the real PGEN backend and the recursive-descent reference backend without changing call sites
- Embedded code-block execution is implemented in the public path for Lua, JavaScript, Rust-native callbacks, and registered wasm modules:
  - parser recognizes `(?{lang:code})`
  - compiler validates code blocks against `ExecutionMode` and cargo features
  - VM lowers code blocks into inline opcodes and executes them during matching
  - engine/runtime materialize current match text, current match start/end/length metadata, top-level branch number when available, numbered captures, named captures, and host-provided variables into the execution context
  - winning-path non-boolean Lua/JavaScript/native/wasm results are surfaced through `MatchResult.code_result`
  - `Regex::find_first_numeric_with_code(...)` / `Regex::find_all_numeric_with_code(...)` collect winning-path numeric payloads
  - `Regex::replace_first_with_code(...)` / `Regex::replace_all_with_code(...)` consume winning-path replacement payloads
- The biggest remaining gaps are now narrower and clearer:
  - `ExecutionMode::Pure` still rejects all code blocks by design
  - `native` and `wasm` code blocks are still Rust-API-only; the CLI has no registration/configuration surface for them
  - the current wasm ABI now has initial richer-result emission, but it is still intentionally narrow compared with the Lua/JavaScript/native surface
  - the real PGEN backend is green locally through pinned submodule commit `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77`
  - hosted validation now has the right repository shape, but the private-submodule checkout may still need explicit CI credentials (`RGX_SUBMODULES_TOKEN`) if the default `GITHUB_TOKEN` cannot read `rdje/pgen`
  - automated validation still misses benchmark trend capture

## What is shipped today
### Default public regex path
- Literals, concatenation, alternation
- Anchors including `^`, `$`, `\A`, `\Z`, and `\z`
- Shorthand and custom character classes, including negated shorthand classes
- Unicode property classes (`\p{...}`, `\P{...}`)
- Greedy, lazy, and possessive `?`, `*`, `+`, `{n,m}`, and `{n,}` quantifiers
- Capturing, non-capturing, named, and atomic groups
- Numeric backreferences (`\1`, `\2`, ...)
- Positive and negative lookahead/lookbehind
- Top-level alternation branch reporting

### Execution-mode / feature-gated path
- `(?{lua:...})` is shipped as a predicate checkpoint in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `lua` feature is enabled.
- `(?{js:...})` and `(?{javascript:...})` are shipped as predicate checkpoints in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `javascript` feature is enabled.
- `(?{native:...})` is shipped on the Rust API path in `ExecutionMode::Full` after registering a callback on the compiled `Regex`.
- `(?{wasm:...})` is shipped on the Rust API path in `ExecutionMode::Safe` or `ExecutionMode::Full` after registering a named wasm module on the compiled `Regex`.
- Current execution-context contract for this slice:
  - capture slot `0` is the current overall match prefix for the current match attempt
  - current match start/end/length metadata plus the 1-based top-level branch number are now available to code-block runtimes when applicable
  - numbered captures, named captures, and host-provided variables are available when their groups have completed or have been set through the Rust API
  - code blocks participate in backtracking and may execute multiple times during one overall match search
  - Lua/JavaScript/native/wasm `Numeric` and `Replacement` results now continue matching and the last winning-path non-boolean value is exposed through `MatchResult.code_result`
  - wasm keeps `module:function` plus exported `() -> i32` predicates and `rgx` imports for position, current match metadata, full input text, numbered captures, named captures, variables, `emit_numeric(...)`, and `emit_replacement(...)`

### Parser interoperability / PGEN path
- `docs/PARSER_CONTRACT.md` is the parser-boundary source of truth.
- The active parser and the direct PGEN backend are both checked against the recursive-descent reference AST on widened fixtures covering:
  - empty patterns
  - anchors
  - range quantifiers
  - possessive quantifiers
  - shorthand and Unicode property classes
  - group families
  - lookarounds
  - conditionals with and without false branches
  - code-block tags (`lua`, `js`, `javascript`, `native`, `wasm`)
  - recursion and numeric backreferences
- Direct local validation confirms the four previously reported PGEN transport bugs are fixed in the local `1.1.1` checkout.

## Explicit boundaries that remain in place
- `ExecutionMode::Pure` rejects code blocks with an explicit compile error.
- `ExecutionMode::Safe` still rejects `native` code blocks; they require `ExecutionMode::Full`.
- The CLI still has no native- or wasm-registration surface, so those shipped slices are currently Rust-API-only.
- The current wasm ABI is intentionally smaller than the Lua/JavaScript/native context surface and still limits richer-result transport to host-emitted numeric and UTF-8 replacement payloads.
- Current recursion / subroutine calls are runtime-integrated on the default path, while newer returned-capture subroutine forms remain future work.

## Codebase realities that matter for roadmap prioritization
- `Compiler::feature_validation_message()` remains a critical safety boundary because `OptimizingCompiler::codegen_pass()` still carries placeholder branches for unsupported AST families.
- The declared opcode surface in `rgx-core/src/vm.rs` still exceeds the emitted/decoded/runtime-used surface; several opcode families remain aspirational or only partially wired.
- `PatternAnalysis` and `ParserConfig` remain unused scaffolding even after the real PGEN backend rollout.
- The default local CI path now validates the default PGEN-backed RGX-scoped `fmt` and workspace tests, `rgx-cli --features pgen-parser`, the local `rgx-core` feature matrix (`pgen-parser`, `lua`, `javascript`, `wasm`), combined-language build coverage (`all-languages`), and `clippy`.
- The PGEN dependency is now pinned as `subs/pgen` at commit `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77`.
- The root Cargo workspace explicitly excludes `subs/pgen/rust`, which keeps RGX validation scoped to RGX even though the parser dependency now lives under the repository tree.
- Hosted GitHub CI now checks out submodules recursively; because `subs/pgen` is private, it may still require `RGX_SUBMODULES_TOKEN` if `github.token` cannot access `rdje/pgen`.
- Benchmark infrastructure exists in `rgx-bench`, but benchmark trend capture is still ad hoc and separate from automated validation.

## Roadmap alignment
### Now
- PCRE2 parity hardening remains active and well-supported by tests and docs.
- Capability hardening improved again because the real PGEN parser backend now participates in local validation instead of remaining a placeholder.
- Capability hardening improved again because recursion moved from a parser-only boundary into real compiler/VM/runtime support with API and PCRE2 differential coverage.
- Capability hardening improved again because conditionals moved from parsed-only status to shipped default-path behavior with API and parity coverage.
- Capability hardening improved again because numeric backreferences moved from parsed-only status to shipped default-path behavior with explicit parity coverage.
- Capability hardening improved again because possessive quantifiers moved from a parser-adapter gap to shipped default-path behavior with API and parity coverage.
- Embedded code execution is no longer parsed-only scaffolding; Lua/JavaScript/native/wasm are real shipped slices on the documented Rust API path.

### Next
- Design the next higher-value wasm/runtime slice beyond the current position/match-metadata/text/numbered-capture/named-capture/variable imports plus the initial `emit_numeric` / `emit_replacement` result helpers.
- Decide whether native/wasm registration should remain Rust-API-only or gain configured CLI/external surfaces later.
- Tighten the private-submodule CI auth story so hosted builds can always fetch `subs/pgen` without operator intervention.
- Operationalize benchmark trend capture instead of relying on manual runs.

### Later
- Finish larger regex-surface gaps: newer PCRE2 advanced forms and the still-declared-but-unwired opcode families.

## Practical engineering notes
- Inline code blocks are encoded directly into VM bytecode, which avoids an external callout table and keeps subprogram lowering simple.
- Named-capture metadata is derived once during compilation and stored with the compiled program for runtime callout access.
- Lua execution is sandboxed per invocation rather than reusing one mutable runtime, which makes speculative execution under backtracking/probing safer.
- JavaScript execution is still per-invocation via QuickJS and is wrapped so documented `return ...` style code works inside `(?{js:...})` blocks.
- Native callback storage uses shared interior mutability, so the `Arc<ExecutionManager>` attached to the VM can receive post-compilation registrations without swapping runtime instances.
- Host-provided execution variables now live on the shared `ExecutionManager` and are snapshotted into each per-call `ExecContext`, which keeps callout inputs deterministic under backtracking while still allowing Rust API updates between matches.
- Wasm module storage follows the same shared-runtime model, with compiled modules registered once and instantiated on demand through wasmtime; per-call store data now also retains the last emitted wasm result payload until predicate completion.
- Unicode property classes are resolved through a small `unicode_support.rs` bridge backed by `regex-syntax`, which keeps RGX aligned with current Unicode property tables without hard-coding those tables locally.
- Inline subexpression compilation now has to merge and rebase child char-class tables back into the parent compiler state; that fix matters for Unicode property classes inside quantified/lookaround subprograms and closes a broader latent char-class bug.
- Root `rgx-core/src/javascript.rs` and `rgx-core/src/wasm.rs`, plus `rgx-core/src/cache.rs`, `rgx-core/src/simd.rs`, `rgx-bench/src/lib.rs`, and `rgx-wasm/src/lib.rs`, remain scaffold-level placeholders despite the real execution logic living elsewhere.

## High-confidence next actions
1. Design and ship the next wasm/runtime layer beyond the current import-based context slice plus current match metadata and the initial `emit_numeric` / `emit_replacement` helpers.
2. Decide whether native/wasm registration should stay Rust-API-only or gain configured CLI/external surfaces.
3. Tighten the private-submodule CI auth story so hosted builds can always fetch `subs/pgen`.
4. Add automated benchmark-trend capture to the default validation loop.
5. Reduce warning debt in `vm.rs`, `execution.rs`, `parser.rs`, `lexer.rs`, `lib.rs`, `ast.rs`, and `token.rs`.
