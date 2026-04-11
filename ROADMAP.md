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
### Tier-2 perf headroom + parity polish (post-C1/C2 cleanup)
- Status: `in-progress` (started 2026-04-12, after the C1 production cutover)
- Context: the C2 NFA/DFA hybrid (shipped 2026-04-11) and C1 JIT compilation (shipped 2026-04-12) closed the major-perf track. Both are default-on. The 4-tier dispatch chain (`DFA → Pike-VM → JIT → backtracking VM`) is in production and exercised by every test in the suite.
- Goal: pick off the smaller follow-up wins now that the major arcs are done. Three concrete focuses for this session:
  - **Reverse-DFA pipeline (C2 follow-up).** The reverse NFAs are already built and stored on `CompiledC2Program` but the dispatch path uses per-position anchored scans. Switching to a forward-DFA finds match end → reverse-DFA finds match start → bounded Pike-VM recovers captures pipeline is the biggest single perf win still on the table (per the C2 chapter's "what's not in C2 yet" section).
  - **DFA negated-char-class semantics fix.** The C1 step 6 differential gate exposed that `Regex::find_first("[^0-9]", "123abc")` returns `(3, 6)` (leftmost-LONGEST) via the C2 DFA path while the raw VM and the JIT correctly return `(3, 4)`. Small correctness improvement to C2.
  - **A8 crate publishing prep.** Cargo.toml metadata cleanup, README polish for crates.io display, license file check, dry-run via `cargo publish --dry-run`. Does not actually publish — that's gated on a final API-stability decision and explicit user authorization.
- Validation: existing benchmark trend infrastructure (label-paired quick/full captures under `target/benchmark-trends/`) for the perf items; the differential gate for the correctness fix; `cargo publish --dry-run` for the publishing prep.

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
  - decide explicit compile-boundary or runtime behavior for newer conditional forms such as `(?(VERSION[...])...)`, now that current recursion-condition variants `(?(R)...)` / `(?(Rn)...)` / `(?(R&name)...)` are shipped
  - extend Perl extended character classes `(?[...])` beyond the newly shipped grouped bracket/property/nested-ordinary/POSIX/shorthand/escaped-term subset, which now also includes nested ordinary bracket terms such as `[\dA-F]`, `[[:graph:]]`, and `[\p{L}]`, current control-literal escapes such as `\a`, `\b`, `\e`, `\f`, and control/octal/codepoint atoms such as `\cA`, `\040`, `\o{101}`, and `\x{41}`, with explicit runtime policy for wider set-expression forms and any further bare-term families
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

## Next (near-term) — continued

### Performance: close the PCRE2 gap to <10x
- Status: `planned`
- Goal: reduce the matching speed gap from ~20-60x to <10x for common patterns.
- Scope:
  - eliminate `ExecContext.text` Vec copy (switch to borrowed `&[u8]`)
  - pre-allocate and reuse capture and backtrack structures across match attempts
  - compile-time eliminate trace macros behind `#[cfg(feature = "trace")]`
  - opcode fusion for common sequences (Char+Char → string compare)
  - reduce per-opcode bounds checking overhead
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Performance target section

### Host integration Layer 6: File-Backed Matching
- Status: `done` (core API); `tail_file` and CLI integration planned as follow-up
- Shipped: `match_file`, `match_file_lines`, `scan_file`, `scan_file_lines` with `FileMatch` struct; 5 tests.
- Remaining: `tail_file` (streaming/watching), mmap for large files, CLI `--file` / `--line-mode` flags.
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 6

### Host integration Layer 3: Match Steering
- Status: `done`
- Shipped: `SteerResult` enum with `Continue`, `Fail`, `Accept`, `Skip(usize)`, `Abort`; `ExecResult::Steer` variant; VM dispatch for all actions; native callback API support; 5 tests.
- Inline-language steering (Lua/JS/Rhai helpers) planned as follow-up.
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 3

### Host integration Layer 4: Structured Events
- Status: `done`
- Shipped: `MatchEvent` enum with 6 variants; `Regex::on_event(observer)` API; zero overhead when no observer; instrumented all scanning strategies, `SetAlternative`, `SaveEnd`, `try_backtrack`, `execute_inline_code_block`.
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 4

### Host integration Layer 5: Async/External I/O
- Status: `done`
- Shipped: continuation-passing with `MatchOutcome`, `MatchContinuation` (Send+Sync, owns all data), `ExecResult::Suspend`, `find_first_suspendable`, `resume`, `find_first_async`; zero sync overhead; correct under backtracking/recursion; 12 tests.
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 5

## Later (strategic)

### GitHub Pages for The RGX Book
- Priority: `medium`
- Status: `blocked` (Pages on private repos requires GitHub Pro)
- Scope:
  - User plans to subscribe to GitHub Pro soon, which unlocks Pages on private repos.
  - **Re-add `.github/workflows/book.yml`** (was deleted to stop CI failures — git history has the working version, see commit that removed it).
  - Enable Pages in repo settings → Source: GitHub Actions.
  - Book will publish to `https://rdje.github.io/rgx`.

### Public release preparation
- Priority: `high` once codebase is ready
- Status: `planned`
- Scope:
  - **Publish `rgx-core` and `rgx-cli` to crates.io** (backlog item A8). Requires API stability decision and final review.
  - Open-source license + contribution guidelines.
  - Public README polish targeting first-time visitors.
  - Tag v0.1.0 release.

### Remaining PCRE2 feature gaps
The compatibility matrix is now at ~98% parity. JIT compilation has shipped as of the C1 production cutover (2026-04-12, default-on). The remaining gaps are:

#### VERSION conditionals `(?(VERSION>=...)...)`
- Priority: `very low`
- Status: `planned`
- Rationale: allows patterns to branch on the PCRE2 engine version. This is a PCRE2-specific construct with no semantic equivalent in other engines. Almost never seen in real-world patterns.

#### `(*SKIP:name)` named skip
- Priority: `low`
- Status: `planned`
- Rationale: `(*SKIP)` (without name) is already shipped. The named form `(*SKIP:name)` interacts with `(*MARK:name)` to skip back to the position of a specific mark. This requires wiring the mark name registry into the skip logic. The unnamed form covers the vast majority of use cases.

### Binding/runtime expansion (A9)
- Status: `deferred` (deprioritized 2026-04-09)
- Scope: production-ready external bindings and runtime targets (Python/Node/C) after core stability gates.
- Why deferred: the conventional argument for A9 ("10x user base because most regex users aren't Rust devs") is generic and doesn't fit RGX specifically. RGX's killer feature is host integration (predicates, steering, events, async I/O, embedded scripting), and that surface is hardest to translate cleanly across FFI — Python callbacks become GIL territory, the async story assumes Rust's model, and the "embed Lua inside a regex from Python" pitch is weaker than from C/C++ because Python users already have a scripting host. Meanwhile A9 is gated on A8 (publish), is `large` per language, and the maintenance tail (packaging, version skew, prebuilds, separate CI per binding) competes for time against engine work that strengthens the actual differentiator.
- Reactivation criteria: a real user or use case pulling for a specific binding. Without a demand signal, this is speculative work. If it does reactivate, **C bindings via cbindgen first** (cheapest, unlocks every other FFI host for free) — not Python.

## Done recently (snapshot)
- **C1 JIT compilation production cutover** (2026-04-12). All 9 steps (0–8) of the design doc plan complete. The `jit` Cargo feature is now default-on; existing users get the JIT for free at the next `cargo update`. The 4-tier dispatch chain (`DFA → Pike-VM → JIT → backtracking VM`) is in production and exercised by every test in the suite. Public design lives in `book/src/internals/jit-compiler.md` (new chapter, ~250 lines). With the new default, `cargo test -p rgx-core` runs 957 lib tests (= 695 baseline + 262 C1) — up from 695 baseline. Opt-out via `--no-default-features --features pgen-parser` still works (drops Cranelift entirely from the dependency closure, runs 695 baseline tests). See `CHANGES.md` 2026-04-12 entry for the full step-by-step history.
- **C2 NFA/DFA hybrid production cutover** (2026-04-11). All 9 steps (0–8) complete. Sparse-set Pike-VM, lazy DFA cache, byte-class equivalence partitioning, two-pass capture recovery, and the 3-tier dispatch chain wired into `Engine::find_first` / `find_all` / `is_match`. Patterns the DFA can handle run ~1.9x faster than PCRE2; pure-literal patterns ~3.2x faster. Public design lives in `book/src/internals/nfa-dfa-engine.md`.
- Extended Perl extended character classes again so nested ordinary bracket terms inside `(?[...])` now accept the current ordinary char-class atom subset, including representative shorthand/range, POSIX, and Unicode-property forms such as `(?[[\dA-F]])`, `(?[[[:graph:]]])`, and `(?[[\p{L}] - [\p{Lu}]])`, with parser-path, parser-contract, compiler/unit, and differential parity coverage.
- Extended Perl extended character classes again so the default path now also supports bare escaped literal/codepoint terms such as `\n`, `\t`, `\r`, `\x{41}`, `\x41`, and escaped operators like `\-` inside the shipped `(?[...])` subset, including differential parity coverage for hex/control-escape cases while still keeping broader remaining forms behind an explicit compile boundary.
- Extended Perl extended character classes again so the default path now also supports horizontal/vertical whitespace shorthands (`\h`, `\H`, `\v`, `\V`) inside the shipped `(?[...])` subset, including parser-path and differential parity coverage while still keeping broader remaining forms behind an explicit compile boundary.
- Extended Perl extended character classes again so the default path now also supports bare ASCII POSIX class terms such as `[:graph:]`, complemented `[:alpha:]`, and POSIX-class algebra cases like `(?[ [:alpha:] & [a-z\t] ])` inside the shipped `(?[...])` subset, including parser-path and differential parity coverage while still keeping broader remaining forms behind an explicit compile boundary.
- Extended Perl extended character classes again so the default path now also supports bare control escapes like `\cA` and bare octal escapes like `\040` / `\o{101}` inside the shipped `(?[...])` subset, including parser-path and differential parity coverage while deliberately keeping `\N` out because upstream PCRE2 rejects it inside extended classes.
- Extended Perl extended character classes again so the default path now also supports the current control-literal escape family inside the shipped `(?[...])` subset, explicitly including `\b` alongside `\a`, `\e`, and `\f`, with parser-path, compiler/unit, parser-contract, and PCRE2 differential coverage.
- Extended Perl extended character classes again so the default path now also supports bare shorthand terms (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`) inside the shipped `(?[...])` subset, including differential parity coverage for digit/word and negated-shorthand cases while still keeping broader remaining forms behind an explicit compile boundary.
- Extended Perl extended character classes again so the default path now also supports bare negated ASCII POSIX class terms such as `[:^alpha:]` inside the shipped `(?[...])` subset, including parser-path, parser-contract, compiler/unit, and differential parity coverage.
- Extended Perl extended character classes again so the default path now also supports same-level multi-operator algebra with `&` binding tighter than `|`, `+`, `-`, and `^`, while still keeping additional bare-term families and wider set-expression forms behind an explicit compile boundary.
- Consolidated the benchmark trend capture internals so artifact path planning, file writes, and summary logging now run through one shared path in `trend_capture.rs`, reducing duplication without changing the external report set.
- Extended Perl extended character classes again so the default path now also supports unary complement, symmetric difference, and grouped subexpressions over the existing bracket/property subset, laying the groundwork for the later same-level multi-operator precedence widening.
- Tightened benchmark trend capture again so `overview.*` now surfaces the newest shared-label quick/full pair alongside the latest per-mode quick/full state, making the cross-mode landing artifact more release-oriented.
- Tightened benchmark trend capture again so rolling `profile-history.*` summaries now call out the latest shared-label quick/full pair's biggest improvements and regressions instead of only exposing the raw pair-over-pair table.
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
- Shipped the first real Perl extended character class runtime slice on the default regex path by lowering simple nested bracket-equivalent literal/range content such as `(?[[a-z]])` and `(?[[^0-9]])` into the existing char-class engine, while keeping broader algebraic forms explicitly gated.
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
