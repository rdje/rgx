# RUST CODEBASE ANALYSIS
Live roadmap-grounded analysis of the Rust workspace in `rgx`.
## Why this file exists
- Capture what the Rust codebase actually does today versus what `ROADMAP.md` says.
- Separate validated implementation status from aspirations in `PROJECT_VISION.md` and older repository guidance.
- Give the commit workflow a single Rust-specific place to update when implementation reality changes.
## Maintenance policy
- Update this file when Rust code changes alter architecture, shipped-vs-gap status, feature-flag readiness, validation results, or roadmap alignment.
- Review this file before Rust-focused commits alongside `CHANGES.md`, `MEMORY.md`, `ROADMAP.md`, and `COMMIT.md`.
- Treat command results here as snapshots; refresh them when they become stale.
## Evidence snapshot
- Docs reviewed before writing this analysis:
  - `README.md`
  - `ROADMAP.md`
  - `COMMIT.md`
  - `CHANGES.md`
  - `DEVELOPMENT_NOTES.md`
  - `MEMORY.md`
  - `PROJECT_VISION.md`
  - `WARP.md`
  - `docs/USER_GUIDE.md`
  - `docs/CAPABILITY_MATRIX.md`
  - `docs/PCRE2_COMPATIBILITY_MATRIX.md`
  - `docs/PARSER_CONTRACT.md`
  - `docs/TECHNICAL_DECISIONS.md`
  - `docs/architecture.md`
- Validation snapshot:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` => pass
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` => pass with 593 warnings
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser` => pass
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua` => pass
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm` => pass
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript` => pass
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_find_all_spans -- --nocapture` => pass
- Public-path regression coverage:
  - `rgx-core/src/lib.rs` now covers lazy `??`, `*?`, `+?`, `{n,m}?`, and `{n,}?` through the public API.
  - `rgx-bench/tests/pcre2_parity.rs` now includes lazy-quantifier and lazy-range PCRE2 differential cases.
## Executive summary
- The default Rust workspace is real, test-backed, and centered on `rgx-core`.
- The strongest delivered path today is parser/AST -> compiler -> VM -> API/CLI for a focused supported regex subset with strong regression and parity guardrails.
- The roadmap's active workstreams around PCRE2 parity, parser contract/conformance, and capability hardening are materially underway in code, tests, and docs.
- The biggest current gaps are specific and actionable:
  - code-execution infrastructure exists but is disconnected from compiler/VM/API flow
  - the `pgen-parser` feature is still a recursive-descent fallback, not a real backend
  - user-facing JavaScript/WASM root modules remain scaffold-level even though feature builds now compile
  - benchmark tooling exists, but there is no continuous performance-validation loop yet
## Workspace structure and maturity
### Crates
- `rgx-core`: real implementation center
- `rgx-cli`: thin, usable CLI wrapper over the public API
- `rgx-bench`: parity and benchmark support
- `rgx-wasm`: scaffold only
### Complexity concentration
- `rgx-core/src/vm.rs` is the largest file at about 3220 lines.
- `rgx-core/src/lexer.rs` is about 1833 lines.
- `rgx-core/src/parser.rs` is about 1191 lines.
- `rgx-core/src/lib.rs` is about 978 lines.
- `rgx-core/src/execution.rs` is about 865 lines.
### Test distribution in the current tree
- `rgx-core/src/lib.rs`: 49 tests
- `rgx-core/src/lexer.rs`: 22 tests
- `rgx-core/src/parser.rs`: 20 tests
- `rgx-core/src/vm.rs`: 14 tests
- `rgx-bench/tests/pcre2_parity.rs`: 10 tests
- `rgx-core/src/parsing.rs`: 8 tests
- `rgx-core/src/ast.rs`: 3 tests
- `rgx-core/src/token.rs`: 2 tests
- `rgx-core/src/log.rs`: 1 test
- `rgx-cli`, `rgx-wasm`, and `rgx-bench/src/lib.rs` have no direct unit tests.
### Placeholder or scaffold-heavy files
- `rgx-core/src/cache.rs`
- `rgx-core/src/simd.rs`
- `rgx-core/src/javascript.rs`
- `rgx-core/src/wasm.rs`
- `rgx-wasm/src/lib.rs`
- `rgx-bench/src/lib.rs`
### Tracked standalone Rust files outside the workspace crates
- `debug_char_class.rs`
- `test_alternation.rs`
- `test_char_class.rs`
- `examples/test_alternation.rs`
- These are useful ad hoc artifacts, but they are not part of the workspace crates or normal CI/test flow.
## Roadmap assessment
### Now (active)
#### PCRE2 parity program
- Status against roadmap: strong progress, still legitimately `in-progress`.
- Evidence that the workstream is real:
  - `docs/PCRE2_COMPATIBILITY_MATRIX.md` exists and is detailed.
  - `rgx-bench/tests/pcre2_parity.rs` provides executable differential checks.
  - default workspace tests pass, including the parity suite.
- What is already true:
  - literals, alternation, greedy and lazy quantifiers, anchors, shorthand classes, lookarounds, and atomic groups have real default-path parity coverage.
  - counted ranges now have parity coverage for greedy and lazy suffix-sensitive behavior.
  - known gaps are explicitly compile-blocked for backreferences, recursion, conditionals, and Unicode property classes.
- What still blocks closure:
  - the parity program is correctness-heavy; it is not yet a continuous speed-tracking loop.
#### Parser-independent engine maturity
- Status against roadmap: largely delivered for the supported path.
- Evidence:
  - `Regex::from_ast` and `Regex::from_ast_with_mode` are real.
  - AST-first lookahead/lookbehind and atomic-group semantics are tested through the public API.
- Remaining gaps:
  - the VM/compiler surface is still partial, with many declared opcodes not wired through decode/execute.
#### Parser completeness path (toward PGEN integration)
- Status against roadmap: parser coverage is ahead of runtime integration, but real backend replacement has not happened.
- Evidence:
  - lexer/parser accept named groups, lookarounds, atomic groups, recursion syntax, conditionals, and code-block syntax.
  - parser-path compile boundary explicitly rejects unintegrated advanced runtime features.
- Remaining gaps:
  - the `pgen-parser` feature is still backed by recursive-descent fallback behavior.
  - parser acceptance is ahead of execution support for several advanced families by design.
#### Parser interoperability contract and conformance harness
- Status against roadmap: materially delivered.
- Evidence:
  - `docs/PARSER_CONTRACT.md` is detailed and versioned.
  - `rgx-core/src/parsing.rs` contains conformance and compile-boundary tests.
  - `cargo test -p rgx-core --features pgen-parser` passes.
- Remaining gap:
  - the feature-gated PGEN path is not a separate parser implementation yet.
#### Capability matrix hardening
- Status against roadmap: good foundation, but not finished.
- Evidence:
  - `docs/CAPABILITY_MATRIX.md` exists and is paired with API/conformance tests.
  - unsupported advanced features fail explicitly rather than silently degrading.
- Remaining gaps:
  - `ExecutionMode` and code-execution support look broader in the API/docs than the actual public path currently delivers.
### Next (near-term)
#### Performance validation loop
- Status against roadmap: scaffolded, not operationalized.
- Evidence:
  - `rgx-bench/benches/throughput.rs` and `rgx-core/benches/core_performance.rs` exist.
- Remaining gaps:
  - no benchmark results are tracked in the repo.
  - no benchmark gate or comparison step exists in CI or commit workflow.
#### Embedded code-path integration clarity
- Status against roadmap: architecture exists, user-visible path does not.
- Evidence:
  - `rgx-core/src/execution.rs` contains Lua, JavaScript, and native-callback execution machinery.
  - `ExecutionMode::{Pure, Safe, Full}` exists in the public API and CLI.
- Remaining gaps:
  - compiler rejects code blocks before VM/runtime can use execution engines.
  - `crate::execution` is not wired into compiler/VM/engine/lib public flow.
  - `ExecutionMode::Safe` and `ExecutionMode::Full` are mostly scaffolding today because they do not unlock additional supported regex behavior.
#### Multi-language code-block runtime expansion
- Status against roadmap: mixed readiness.
- Evidence:
  - `cargo check -p rgx-core --features lua` passes.
  - `cargo check -p rgx-core --features wasm` passes.
  - `cargo check -p rgx-core --features javascript` passes.
  - `cargo check -p rgx-core --features all-languages` passes.
- Remaining gaps:
  - compiler still rejects code blocks before VM/runtime can invoke these backends.
  - public backend modules are inconsistent:
    - `rgx-core/src/lua.rs` re-exports the real Lua engine
    - `rgx-core/src/javascript.rs` is a stub even though JavaScript code lives inside `execution.rs`
    - `rgx-core/src/wasm.rs` is a stub
### Later (strategic)
#### Broader feature coverage
- Status against roadmap: still blocked by partial VM/compiler surface.
- Evidence:
  - `OpCode` currently defines 61 variants.
  - `TryFrom<u8>` maps only 33 of those variants.
  - several AST and opcode families are still placeholders or compile-gated.
#### Binding/runtime expansion
- Status against roadmap: very early.
- Evidence:
  - `rgx-wasm/src/lib.rs` is scaffold-level.
  - runtime-specific root modules are unevenly implemented.
## Component findings
### `rgx-core/src/compiler.rs`
- Strength:
  - explicit compile boundary protects the public API from silently executing unsupported advanced syntax.
- Limitation:
  - it delegates nearly all real behavior to `vm.rs`, so VM partiality still dominates capability.
### `rgx-core/src/vm.rs`
- Strengths:
  - real VM search, scanning, capture, alternation tracking, lookaround, atomic-group, and greedy/lazy quantifier behavior exist.
  - the default supported path is backed by API and parity tests.
- High-confidence gaps:
  - Unicode property-class codegen still contains a stale fallback that emits `Any`; this is currently masked by the higher-level compile boundary, but it remains dangerous implementation debt.
  - AST and opcode surface are larger than the actual decode/execute surface.
  - `optimize_ast()` and `peephole_optimize()` are still TODO stubs.
- Quantified VM surface:
  - 61 opcode variants are declared.
  - 38 opcode variants are mapped in `TryFrom<u8>`.
  - unmapped declarations still include `String`, case-insensitive literal ops, SIMD opcodes, `RepeatRange`, `RepeatExact`, `Backref`, memoization hints, and terminal ops like `Accept` and `Halt`.
### `rgx-core/src/parsing.rs`
- Strengths:
  - clear parser trait boundary
  - good conformance scaffolding
  - active parser behavior is well-covered by tests
- Gaps:
  - `pgen-parser` remains a recursive-descent fallback
  - feature-flag reporting is not fully internally consistent:
    - `parser_capabilities()` under the `pgen-parser` cfg advertises `error_recovery=true` and `syntax_highlighting=true`
    - `PgenParser::capabilities()` returns both as `false`
### `rgx-core/src/parser.rs` and `rgx-core/src/lexer.rs`
- Strengths:
  - parser/lexer support is ahead of runtime integration for advanced syntax, which is the right shape for the roadmap.
  - conditional and recursion parsing are more complete than the runtime path.
- Gaps:
  - parser completeness currently outpaces runtime completeness by a wide margin.
  - octal escape parsing remains suspect because digit escapes route through backreference handling first; this matches the existing continuity note that `\\101` still behaves like backreference parsing instead of octal parsing.
### `rgx-core/src/execution.rs`
- Strengths:
  - substantial amount of real code exists for Lua, JavaScript, native callbacks, sandboxing setup, and execution-manager orchestration.
- Gaps:
  - it is currently disconnected from the user-visible regex compilation and execution path.
  - the JavaScript backend now compiles by creating a fresh QuickJS runtime per execution, but that path is still not user-visible because code blocks remain compile-blocked.
### `rgx-core/src/log.rs`
- Strength:
  - structured tracing is real, broad, and already valuable for debugging the pipeline.
- Gap:
  - warning debt is still high, especially around docs and polish-level clippy concerns.
### `rgx-cli/src/main.rs`
- Strengths:
  - good observability controls
  - usable public CLI for the supported path
- Gaps:
  - no direct unit tests
  - execution mode currently mostly changes logging/constructor choice rather than real supported behavior
### `rgx-bench`
- Strength:
  - parity harness is the strongest correctness-evidence surface outside `rgx-core` tests.
- Gap:
  - benchmark code exists, but there is no tracked performance narrative yet.
## Quality snapshot
- Default `cargo clippy --workspace --all-targets` passes without errors, but it emits 593 warnings.
- Highest-warning files:
  - 137 warnings in `rgx-core/src/vm.rs`
  - 88 warnings in `rgx-core/src/parser.rs`
  - 88 warnings in `rgx-core/src/ast.rs`
  - 75 warnings in `rgx-core/src/token.rs`
  - 58 warnings in `rgx-core/src/lexer.rs`
  - 36 warnings in `rgx-core/src/log.rs`
- Dominant lint IDs:
  - 176 `missing_docs`
  - 76 `clippy::must_use_candidate`
  - 36 `clippy::cast_possible_truncation`
  - 34 `clippy::map_unwrap_or`
  - 29 `clippy::uninlined_format_args`
  - 26 `clippy::missing_errors_doc`
  - 24 `clippy::needless_continue`
  - 20 `clippy::too_many_lines`
  - 20 `clippy::unused_self`
- TODO markers in Rust source:
  - 6 in `rgx-core/src/vm.rs`
  - 2 in `rgx-core/src/parsing.rs`
  - 1 in `rgx-core/src/execution.rs`
- Manifest hygiene issue:
  - `rgx-wasm/Cargo.toml` uses `package.enhanced-description`, which Cargo warns is an unused manifest key.
## High-confidence next actions
1. Fix lazy quantifier correctness and then update `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, and API/parity tests to reflect the true state.
2. Either integrate `execution.rs` into compiler/VM/API flow or clearly quarantine it as non-runtime scaffolding until that roadmap item becomes active.
3. Make the `pgen-parser` feature truthful end-to-end or replace it with a real backend.
4. Repair the JavaScript feature and add explicit feature-gated validation for optional runtimes.
5. Put benchmark and parity trend capture into the workflow rather than leaving performance evidence as ad hoc benches.
6. Reduce warning debt in `vm.rs`, `parser.rs`, `ast.rs`, `token.rs`, and `lexer.rs` so future correctness work lands on a cleaner surface.
## Commit workflow tie-in
- `COMMIT.md` now treats this file as part of the Rust commit review path.
- For Rust-focused commits, review this file before commit and update it whenever touched code changes:
  - architecture or crate/module boundaries
  - shipped-vs-gap status
  - feature-flag build readiness
  - roadmap alignment
  - validation health
