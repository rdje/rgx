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
  - incrementally close remaining syntax gaps (numeric backreferences, conditionals, Unicode property classes, possessive quantifiers, and current recursion forms are now shipped)

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
  - decide explicit compile-boundary or runtime behavior for newer conditional forms such as `(?(R&name)...)` and `(?(VERSION[...])...)`, now that current recursion-condition variants `(?(R)...)` / `(?(Rn)...)` are shipped
  - decide explicit compile-boundary versus runtime/set-algebra behavior for Perl extended character classes `(?[...])` now that parser transport and compile-boundary guardrails are in place on both parser backends
  - expand `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, and differential tests to reflect whichever boundary or support level is chosen

### Performance validation loop
- Status: `in-progress`
- Goal: tighten benchmark-driven optimization workflow.
- Scope:
  - run and track `rgx-bench` baselines against recent changes
  - keep the default local validation loop emitting a low-overhead quick trend summary under `target/benchmark-trends/`
  - preserve mode-scoped latest snapshots plus timestamped local history and surface delta summaries against the most recent prior archived capture from the same benchmark mode
  - keep mode-scoped rolling history summaries so the recent longitudinal ratio story is visible without manually opening individual archived captures
  - keep archived captures revision-aware with optional labels so longitudinal reports can be tied back to specific RGX commits or release candidates
  - keep label-paired quick/full summaries so the same RGX revision can be compared across low-overhead and bench-profile captures without manual report stitching
  - keep rolling label-pair history so shared-label quick/full comparisons can also be tracked across successive revisions without manual report stitching
  - preserve a higher-fidelity `full` mode for slower bench-profile captures when deeper measurement is needed
  - prioritize optimizations with measurable impact

### Embedded code-path expansion beyond phase 1
- Status: `planned`
- Goal: refine the post-phase-1 code-block surface so first-class inline languages and advanced reference-style backends are treated differently.
- Scope:
  - keep Lua and JavaScript as the primary shipped inline source-body languages
  - treat wasm as an advanced registered-module/reference-style path rather than the main everyday inline code-block UX target
  - decide later whether native should expand beyond the current Rust-API-only surface and whether wasm should grow beyond the new file-backed CLI module-registration path once the inline-language story is mature
  - only revisit richer wasm ABI/result work after the preferred inline-language expansion path is clearer

### Multi-language code-block runtime expansion
- Status: `in-progress`
- Goal: extend code-block runtime support beyond initial languages while preserving deterministic behavior and safety guarantees.
- Scope:
  - build on the now-shipped `lua`, `js` / `javascript`, and `rhai` inline/source-body slice
  - keep `lua`, `js`, `javascript`, and `rhai` aligned around the same source-body execution contract shape where practical
  - explicitly defer heavier embedded runtimes such as Python and Julia until after the current ergonomics/safety work on the shipped inline-language track
  - treat wasm and native as advanced reference-style backends rather than the primary model for new inline language growth
  - shared execution contracts, resource limits, and sandbox controls

## Later (strategic)
### Broader feature coverage
- Status: `planned`
- Scope: deeper advanced regex features beyond current verified set.

### Binding/runtime expansion
- Status: `planned`
- Scope: production-ready external bindings and runtime targets after core stability gates.

## Done recently (snapshot)
- Deepened benchmark trend capture again so shared-label quick/full pairs now also produce rolling `profile-history.*` summaries, making pair-over-pair revision deltas visible alongside the existing latest-pair snapshot.
- Deepened benchmark trend capture again so shared labels now also produce `profile-pairs.*` summaries pairing the latest quick/full captures for the same revision, including aggregate medians plus full-vs-quick deltas per benchmark kind.
- Deepened benchmark trend capture again so every run now also rewrites a compact cross-mode `overview.*` artifact summarizing the latest quick/full history state in one place, including latest labels, aggregate medians, and delta-vs-previous for each mode.
- Deepened benchmark trend capture again so explicit same-mode baseline selection can now target either a unix timestamp or a capture label via `label:<text>`, with newest-match resolution when multiple archived captures reuse the same label.
- Deepened benchmark trend capture again so each archived quick/full run can now carry an explicit label, the wrapper defaults that label from the current git revision, and the rolling history summaries surface those labels alongside timestamped ratio/delta rows.
- Extended benchmark trend capture again so each quick/full run now also writes mode-scoped rolling history summaries (`history-quick.*` / `history-full.*`) with aggregate median ratios and delta-vs-previous columns, not just one latest snapshot plus one comparison baseline.
- Shipped current recursion-condition conditionals on the default regex path by teaching both parser backends plus the compiler/VM to preserve `(?(R)...)` / `(?(Rn)...)`, honor PCRE2's `R` / `Rn` named-group ambiguity rule, and execute those conditionals against the active recursion level with explicit missing-group validation.
- Tightened the shipped inline-language result contract again by adding explicit emitted-result helpers to Lua/JavaScript/Rhai statement bodies, so Lua/JavaScript now expose `rgx.emit_numeric(...)` / `rgx.emit_replacement(...)`, Rhai exposes `emit_numeric(...)` / `emit_replacement(...)`, and winning-path richer-result emission no longer depends only on direct return values.
- Shipped branch-reset groups on the default regex path by assigning shared capture numbers across the branch-reset group's top-level alternatives, carrying that numbering through later backreferences/conditionals, and adding PCRE2 differential coverage.
- Stabilized the shared local/GitHub validation loop by replacing the flaky umbrella `cargo test --workspace` step with explicit RGX package tests (`rgx-core`, `rgx-cli`, `rgx-bench`, `rgx-wasm`) while keeping the existing feature-matrix coverage intact.
- Hardened Perl extended character classes as an explicit parser boundary so `(?[...])` now round-trips through both parser backends and compile-rejects cleanly instead of remaining an ambiguous parser gap.
- Hardened branch-reset groups as an explicit parser boundary so `(?|...)` now round-trips through both parser backends and compile-rejects cleanly instead of remaining an ambiguous parser gap.
- Shipped single-branch `DEFINE` conditionals on the default regex path by treating `DEFINE` as always false while keeping its branch available for numbered and named subroutine definitions, with explicit compile-time rejection for invalid false-branch forms.
- Hardened the shipped Rhai source-body contract so explicit `return ...` bodies are now locked in alongside final-expression authoring, with regression coverage and docs aligned to the actual runtime behavior.
- Separated benchmark trend artifacts into mode-scoped latest snapshots and history directories so auto-selected comparison baselines no longer mix quick-profile and full-profile captures, while still preserving explicit archived-baseline selection.
- Added file-backed CLI wasm module registration through repeatable `--wasm-module NAME=PATH`, so `(?{wasm:module:function})` no longer requires Rust glue just to exercise registered modules from the command line.
- Shipped relative conditional group references on the default regex path by resolving `(?(+1)...)` / `(?(-1)...)` to absolute conditional-group checks at compile time, with API, parser-contract, and PCRE2 differential coverage.
- Tightened the shipped inline-language CLI path by adding repeatable `--var NAME=VALUE`, optional `--show-details` match rendering, and single-pass match collection so CLI code blocks are not pre-executed twice before output.
- Stabilized relative conditional group references on both parser backends first by transporting `(?(+1)...)` and `(?(-1)...)` as dedicated AST before the later default-path runtime integration landed.
- Deepened the quick benchmark-trend loop again so each capture can now compare against either the most recent prior archived snapshot or an explicit archived baseline while still preserving timestamped history under `target/benchmark-trends/history/`.
- Tightened the shipped inline-language ergonomics again so Lua now accepts bare expression bodies as well as explicit `return ...`, matching the JavaScript/Rhai source-body direction more closely.
- Added automated quick benchmark-trend capture to the default local validation loop via `scripts/capture-benchmark-trends.sh` and `rgx-bench/src/bin/trend_capture.rs`.
- Hardened the shipped inline-language contract so JavaScript bare-expression bodies now drive predicate/result behavior instead of silently falling through, and added helper-API regression coverage across Lua/JavaScript/Rhai.
- Shipped Rhai code blocks on the default execution path in `ExecutionMode::Safe` / `ExecutionMode::Full`, including feature-gated runtime tests, parser-contract coverage, and CI/doc refreshes.
- Expanded code-block execution contexts with current match metadata (`match_start`, `match_end`, `match_length`, top-level `branch_number`) across native/Lua/JavaScript plus new wasm host imports.
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
