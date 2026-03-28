# ROADMAP
Live forward-looking tracker for rgx.

## Purpose
- Track what we are actively building, what is next, and what is deferred.
- Keep planning separate from shipped history (`CHANGES.md`).
- Reduce handoff friction across sessions.

## How to maintain this file
- Update at least when scope or priorities materially change.
- Keep entries concrete and implementation-oriented.
- Move items across sections (`Now`, `Next`, `Later`, `Done`) as status changes.
- Link validation and shipped details in `CHANGES.md` once completed.

## Status legend
- `planned`: scoped but not started
- `in-progress`: active implementation
- `blocked`: cannot proceed yet
- `done`: completed and validated (then move to `CHANGES.md`)

## Now (active)
### PCRE2 parity program (features, speed, accuracy)
- Status: `in-progress`
- Goal: converge toward practical parity with PCRE2 in capabilities and runtime behavior.
- Scope:
  - maintain a compatibility matrix against PCRE2 feature areas
  - use differential tests to catch semantic mismatches
  - track benchmark parity trends in `rgx-bench`
  - baseline established: `docs/PCRE2_COMPATIBILITY_MATRIX.md` + `rgx-bench/tests/pcre2_parity.rs`
### Parser-independent engine maturity
- Status: `in-progress`
- Goal: continue delivering advanced regex semantics through AST-first paths while parser syntax catches up.
- Scope:
  - extend assertion/group behavior in VM/compiler
  - add API-level tests for behavior guarantees

### Parser completeness path (toward PGEN integration)
- Status: `in-progress`
- Goal: support advanced group/assertion syntax in parser path to match AST-first capabilities.
- Scope:
  - align parser tokenization/AST output with VM-supported constructs
  - keep parser behavior consistent with API tests
  - incrementally close remaining syntax gaps (initial conditional parsing subset now wired)

### Parser interoperability contract and conformance harness
- Status: `in-progress`
- Goal: define and enforce a stable parser boundary so PGEN integration is seamless.
- Scope:
  - maintain a versioned contract in `docs/PARSER_CONTRACT.md`
  - keep fixture-based parser conformance tests in `rgx-core/src/parsing.rs`
  - enforce parse-success/compile-unsupported boundary checks for unintegrated runtime features

### Capability matrix hardening
- Status: `in-progress`
- Goal: document and test exactly what is shipped vs scaffolded.
- Scope:
  - maintain `docs/CAPABILITY_MATRIX.md` as source of truth for shipped-vs-scaffolded status
  - expand integration tests for user-facing APIs
  - keep docs synchronized with verified behavior

## Next (near-term)
### Performance validation loop
- Status: `planned`
- Goal: tighten benchmark-driven optimization workflow.
- Scope:
  - run and track `rgx-bench` baselines against recent changes
  - prioritize optimizations with measurable impact

### Embedded code-path expansion beyond phase 1
- Status: `planned`
- Goal: extend the newly shipped predicate code-block slice beyond the current Lua/JavaScript/native/wasm surface.
- Scope:
  - richer wasm ABI beyond the current `module:function` / `() -> i32` predicate contract plus `rgx` imports for position, full input text, numbered captures, and named captures
  - richer result semantics beyond predicate success/failure
  - decide whether native/wasm configuration should expand beyond the current Rust-API-only surface

### Multi-language code-block runtime expansion
- Status: `planned`
- Goal: extend code-block runtime support beyond initial languages while preserving deterministic behavior and safety guarantees.
- Scope:
  - language runtime integration sequence after the current shipped Lua/JavaScript/native/wasm slice
  - shared execution contracts, resource limits, and sandbox controls

## Later (strategic)
### Broader feature coverage
- Status: `planned`
- Scope: deeper advanced regex features beyond current verified set.

### Binding/runtime expansion
- Status: `planned`
- Scope: production-ready external bindings and runtime targets after core stability gates.

## Done recently (snapshot)
- Expanded the wasm ABI with `rgx` host imports for named captures, including deterministic named-capture ordering and regression coverage for name/value reads.
- Expanded the wasm ABI with `rgx` host imports for current position, full input text, and numbered captures, including safe guest-memory failure handling and regression coverage.
- Rust-API wasm module registration and dispatch for `(?{wasm:module:function})` in `ExecutionMode::Safe` / `ExecutionMode::Full`, including runtime wiring, tests, and doc refreshes.
- Rust-API native callback registration for `(?{native:...})` in `ExecutionMode::Full`, including runtime wiring, tests, and doc refreshes.
- Phase-1 embedded code-block execution for `(?{lua:...})` and `(?{js:...})` / `(?{javascript:...})` in `ExecutionMode::Safe` / `ExecutionMode::Full` with feature-gated validation.
- Built-in top-level branch reporting with user-facing 1-based branch number.
- AST-first lookahead support in compiler/VM and API tests.
- AST-first lookbehind support in compiler/VM and API tests.

Detailed implementation history and validation remain in `CHANGES.md`.
