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
  - incrementally close remaining syntax gaps (numeric backreferences, conditionals, Unicode property classes, and possessive quantifiers now shipped; recursion still gated)

### Parser interoperability contract and conformance harness
- Status: `in-progress`
- Goal: define and enforce a stable parser boundary so PGEN integration is seamless.
- Scope:
  - maintain a versioned parser interoperability contract
  - keep fixture-based parser conformance tests around the active parser boundary
  - enforce parse-success/compile-unsupported boundary checks for unintegrated runtime features
  - keep downstream integration guidance aligned to `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` and `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`

### Capability matrix hardening
- Status: `in-progress`
- Goal: document and test exactly what is shipped vs scaffolded.
- Scope:
  - maintain `docs/CAPABILITY_MATRIX.md` as source of truth for shipped-vs-scaffolded status
  - expand integration tests for user-facing APIs
  - keep docs synchronized with verified behavior

## Next (near-term)
### PCRE2 10.47+ downstream syntax alignment
- Status: `planned`
- Goal: prepare RGX for newer PCRE2 syntax that may arrive through the default PGEN parser path.
- Scope:
  - define RGX AST/interoperability handling for returned-capture subroutine forms such as `(?R(grouplist))`, `(?n(grouplist))`, `(?+n(grouplist))`, `(?-n(grouplist))`, `(?&name(grouplist))`, and `(?P>name(grouplist))`
  - decide explicit compile-boundary or runtime behavior for newer conditional forms such as `(?(R&name)...)` and `(?(VERSION[...])...)`
  - audit downstream RGX handling for branch-reset groups, `DEFINE` conditionals, and Perl extended character classes `(?[...])` once parser transport is available and verified
  - expand `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, and differential tests to reflect whichever boundary or support level is chosen

### Performance validation loop
- Status: `planned`
- Goal: tighten benchmark-driven optimization workflow.
- Scope:
  - run and track `rgx-bench` baselines against recent changes
  - prioritize optimizations with measurable impact

### Embedded code-path expansion beyond phase 1
- Status: `planned`
- Goal: extend the newly shipped code-block slice beyond the current Lua/JavaScript/native/wasm surface.
- Scope:
  - richer wasm ABI beyond the current `module:function` / `() -> i32` predicate contract plus `rgx` imports for position, full input text, numbered captures, named captures, host-provided variables, and initial `emit_numeric` / `emit_replacement` result helpers
  - richer non-boolean result handling on the wasm path beyond the current host-emitted numeric/replacement layer
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
- Shipped possessive quantifiers on the default compiler/VM path by lowering `*+`, `++`, `?+`, and counted possessive forms through atomic-group semantics, including parser-path regressions and PCRE2 differential coverage.
- Shipped Unicode property classes on the default compiler/VM path, including invalid-property compile errors, parser-path and AST-first regressions, and representative PCRE2 differential coverage.
- Switched the default RGX build over to the real submodule-backed PGEN parser so normal workspace builds now exercise PGEN by default.
- Shipped conditional runtime support on the default compiler/VM path, including group-exists, named-group-exists, and lookaround condition forms, missing-group compile errors, and PCRE2 differential coverage.
- Shipped numeric backreferences on the default compiler/VM path, including backtracking-safe runtime matching, missing-group compile errors, and PCRE2 differential coverage.
- Extended wasm code blocks with winning-path `Numeric` / `Replacement` result emission through `rgx.emit_numeric(...)` and `rgx.emit_replacement(...)`, including last-emitted-wins and invalid-payload failure coverage.
- Extended the shared local/GitHub CI path so `./scripts/run-local-ci.sh` now covers the `rgx-core` feature matrix (`pgen-parser`, `lua`, `javascript`, `wasm`, and `all-languages`) instead of leaving those checks manual.
- Added first dedicated numeric-result Rust APIs for code-block results by shipping `Regex::find_first_numeric_with_code(...)` / `Regex::find_all_numeric_with_code(...)` on top of winning-path `Numeric(f64)` payloads.
- Added the first replacement-oriented Rust APIs for code-block results by shipping `Regex::replace_first_with_code(...)` / `Regex::replace_all_with_code(...)` on top of winning-path `Replacement(String)` payloads.
- Added the first richer non-boolean code-block result slice by surfacing winning-path numeric/replacement values through `MatchResult.code_result`, now across Lua/JavaScript/native/wasm.
- Added host-provided execution variables to the shipped code-block slice, including `Regex::set_variable(...)`, cross-backend variable bindings, and wasm variable imports.
- Expanded the wasm ABI with `rgx` host imports for named captures, including deterministic named-capture ordering and regression coverage for name/value reads.
- Expanded the wasm ABI with `rgx` host imports for current position, full input text, and numbered captures, including safe guest-memory failure handling and regression coverage.
- Rust-API wasm module registration and dispatch for `(?{wasm:module:function})` in `ExecutionMode::Safe` / `ExecutionMode::Full`, including runtime wiring, tests, and doc refreshes.
- Rust-API native callback registration for `(?{native:...})` in `ExecutionMode::Full`, including runtime wiring, tests, and doc refreshes.
- Phase-1 embedded code-block execution for `(?{lua:...})` and `(?{js:...})` / `(?{javascript:...})` in `ExecutionMode::Safe` / `ExecutionMode::Full` with feature-gated validation.
- Built-in top-level branch reporting with user-facing 1-based branch number.
- AST-first lookahead support in compiler/VM and API tests.
- AST-first lookbehind support in compiler/VM and API tests.

Detailed implementation history and validation remain in `CHANGES.md`.
