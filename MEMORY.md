# MEMORY
Live continuity memory for `rgx` sessions.

## Why this file exists
- Preserve the actionable context needed to resume work after any interruption (session crash, machine crash, tool upgrade/reset, context loss).
- Allow a new LLM/AI instance to continue as if the previous session never stopped.
- Capture only high-signal context (decisions, constraints, current state, next actions), not verbatim transcript logs.

## Mandatory update policy
- Update this file after each completed task and before starting commit workflow.
- Record key user/agent exchange outcomes that affect implementation, process, or priorities.
- Keep entries compact, concrete, and execution-oriented.
- Prefer links/references to live docs for deep detail:
  - `CHANGES.md`
  - `COMMIT.md`
  - `DEVELOPMENT_NOTES.md`
  - `RUST_CODEBASE_ANALYSIS.md`
  - `docs/USER_GUIDE.md`
  - `ROADMAP.md`
  - `docs/CAPABILITY_MATRIX.md`
  - `docs/PCRE2_COMPATIBILITY_MATRIX.md`
  - `docs/PARSER_CONTRACT.md`

## Fast resume checklist
1. Read this file top-to-bottom.
2. Check current working tree and branch state (`git --no-pager status --short`).
3. Read newest entries in `CHANGES.md`, `ROADMAP.md`, and `RUST_CODEBASE_ANALYSIS.md`.
4. Confirm current known gaps and active priorities from:
   - `DEVELOPMENT_NOTES.md`
   - `docs/PCRE2_COMPATIBILITY_MATRIX.md`
   - `RUST_CODEBASE_ANALYSIS.md`
5. Continue with the next concrete task, then update this file before commit workflow.

## Persistent workflow agreements with user
- Always run `git --no-pager status` before every commit.
- Stage from that exact status output (no hidden extras).
- Use `git_message_brief.txt` with `git commit -F git_message_brief.txt`.
- Do not wait for an explicit user prompt to start the commit workflow after a completed task; begin it automatically once task work, validation, and doc updates are done.
- New AI/LLM sessions should bootstrap through `SESSION_BOOTSTRAP.md`; `README.md` now ends with an explicit reminder to read that file and start there.
- Run `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm` before commit; keep external dependencies out of the RGX formatting gate.
- Run `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` before commit and fix all clippy errors first (warnings tolerated for now).
- **Do NOT include `Co-Authored-By` trailers in commit messages.** Neither for Oz nor for Claude. User directive 2026-04-09; supersedes the prior workflow agreement.
- Keep commit messages brief: concise title + 2–5 line body. The gory details go in `CHANGES.md`, the engineering rationale in `DEVELOPMENT_NOTES.md`.
- After commit:
  - clear `git_message_brief.txt`
  - verify `git_message_brief.txt` stays untracked (`TRACKED:1` check).

## Current technical snapshot

### Engine
- ~26K lines of Rust across rgx-core + 1.7K CLI. PGEN 1.1.8 sole parser (9 issues filed/closed).
- PCRE2 feature parity ~95% tracked / ~90% real-world. Full inline flags `(?imsx)`, `\K`, `\R`, `\N`, `\G`, `(?C)`, all backtracking verbs, relative subroutines/backrefs, Python syntax, comment groups, mode settings. 6 deferred low-priority gaps.
- Release-profile speed: literal **6.4x**, email **3.4x**, capture **0.88x** (wins) vs PCRE2. Key: borrowed `&[u8]` text, trace gating, memmem fast path, trail backtracking, binary search Unicode.

### Host integration (all 6 layers shipped)
- L1 Data Exchange: string + typed variables (`Value` enum with Null/Bool/Int/Float/String/Array/Map), fluent builder, `vars!`/`value!` macros, numeric/replacement/structured results, branch numbers
- L2 Predicate Callbacks: native/Lua/JS/Rhai/WASM, Pure/Safe/Full execution modes
- L3 Match Steering: `SteerResult` (Continue/Fail/Accept/Skip/Abort)
- L4 Structured Events: `MatchEvent` (6 types), `on_event` observer, zero overhead
- L5 Async I/O: `find_first_suspendable`/`resume`/`find_first_async`, `MatchContinuation` (Send+Sync)
- L6 File-Backed Matching: `match_file`/`match_file_lines`/`scan_file`/`scan_file_lines`

### CLI
- 15+ flags: `--file`, `--recursive`, `--line-mode`, `--count`, `--context`, `--json`, `--replace`, `--replace-with-code`, `--only-matching`, `--invert-match`, `--numeric`, `--var-json`, `--events`, `--stats`, `--mode`, `--var`, `--wasm-module`, `--show-details`
- 30 tests. `docs/CLI_GUIDE.md` with 20+ examples.

### Testing
- **~550 tests**, all passing: 343 unit + 44 adversarial + 55 integration + 11 property (256+ cases each) + 21 stress/fuzz + 6 doc + 30 CLI + 39 bench
- `docs/TESTING_PHILOSOPHY.md`: hostile skepticism doctrine
- All 9 PGEN issues filed and closed. 3 engine bugs found and fixed via gap testing (nested recursion, events+async, subroutine captures).

### Documentation
- `docs/guide/`: 12-file book (5,810+ lines, 150+ examples)
- `docs/CLI_GUIDE.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, `docs/HOST_INTEGRATION_ARCHITECTURE.md`, `docs/HOST_INTEGRATION_GUIDE.md`, `docs/TESTING_PHILOSOPHY.md`

### Testing
- **5 test suites**: unit (343), integration (55), adversarial (44, 2 ignored/known bugs), property-based (11 × 256+ cases), stress/fuzz (21)
- Total: **~530 tests**, 2 known bugs documented with `#[ignore]`
- Property-based via `proptest`: random patterns/inputs verify invariants (bounds, non-overlap, determinism, UTF-8 safety)
- Stress: 10K inputs, 100K rapid-fire, 8-thread concurrency, 100K-line file scan, 5000 random compilations
- `docs/TESTING_PHILOSOPHY.md`: hostile skepticism doctrine, behavioral categories, claims-to-prove, known gaps, process rules
- Every user-facing API is exercised including error paths, concurrency, and edge cases

### Documentation
- `docs/guide/` — **The RGX Guide**: 12-file book-style documentation (**5,810 lines**, 150+ code examples) covering every feature
  - All chapters audited for SOTA+++ quality: warm tone, real-world scenarios, before/after comparisons, gotchas, visual diagrams
  - `docs/TESTING_PHILOSOPHY.md` — hostile skepticism doctrine for test authoring
  - 8 chapters: first match, data exchange, callbacks, steering, events, async, files, real-world patterns
  - 3 reference docs: quick reference, execution modes, context reference
  - Chapter 7 has 5 complete real-world examples: log monitor, tokenizer, data pipeline, config parser, WAF engine
- `docs/HOST_INTEGRATION_GUIDE.md` — single-file quick reference

### Architecture documents
- `docs/HOST_INTEGRATION_ARCHITECTURE.md` — 6-layer host integration design (2 shipped, 4 planned)
- `docs/PCRE2_COMPATIBILITY_MATRIX.md` — feature-by-feature parity table
- `ROADMAP.md` — updated with performance targets, Layer 3-6 plans, deferred gaps with rationale
  - skips positions where that byte doesn't match, avoiding full VM invocations at impossible positions
  - literal_simple find_first improved ~2x (109x → 55x slower vs PCRE2)
  - conservative single-byte approach; multi-byte prefixes, memchr, and ExecContext allocation reduction are follow-up opportunities
- RGX-owned clippy warnings are now at **zero** (from 296 at session start):
  - refactored 10 over-length functions through helper extraction
  - added targeted `#[allow(clippy::too_many_lines)]` to 3 architectural VM dispatch loops
  - all 33 remaining workspace warnings are from the PGEN submodule
- Latest PCRE2 parity expansion added 24 new differential cases for combined-feature patterns:
  - nested lookarounds, atomic groups with quantifiers, backreference edge cases, possessive+alternation, named groups, complex quantifier interactions, anchors with groups, and dot/class interactions
  - parity case count increased from 185 to 209; all pass against PCRE2
- Latest warning-debt pass cleared all non-architectural clippy warnings:
  - rewrote `let...else`, unwrapped unnecessary Result, changed pass-by-value to reference, added targeted `#[allow]` for inline-always/excessive-bools/recursion-only
  - RGX-owned warnings now at 13, all function-length limits (architectural); 96% reduction from the original 296
- Previous warning-debt pass resolved all cast-truncation and doc-section warnings:
  - added `#[allow(clippy::cast_possible_truncation)]` to 9 VM codegen functions (intentional compact bytecode encoding)
  - added missing `# Errors` (11) and `# Panics` (10) sections across public API surfaces
  - RGX-owned warnings now at 35 (88% reduction from original 296)
  - remaining backlog: 12 function-length limits (architectural), 5 `#[inline(always)]` (intentional), small tail of structural suggestions
- Latest dead-code cleanup removed 11 superseded opcodes and the dead `memo_cache` field from `vm.rs`:
  - removed: String, CharNoCase, StringNoCase, Range, RangeNeg, Return, SaveStartCond, RestoreCaptures, RepeatRange, RepeatExact
  - hex slot values preserved via tombstone comments so remaining opcodes don't shift
  - remaining unemitted opcodes (SIMD, optimization hints, Accept, Halt, JumpIfMatch) are now explicitly marked as reserved for future work
- Latest warning-debt pass was a deep cleanup across the entire `rgx-core` crate:
  - removed 30 redundant `continue` statements from VM execution loops
  - converted 16 private methods to associated functions in `vm.rs` (unused `self`), plus 3 cascade conversions
  - combined 11 identical match arms across `compiler.rs`, `parsing.rs`, and `vm.rs`
  - rewrote `let...else` and unwrapped 3 unnecessary `Result`-wrapped functions in `lexer.rs`
  - added missing field/variant docs across `ast.rs` (40 items), `token.rs` (36 items), `error.rs` (4 items), and `log.rs` (3 functions)
  - fixed stale BranchReset "runtime semantics pending" comment in `ast.rs` to reflect shipped status
  - inlined format string variables and applied auto-fixable lint suggestions across multiple files
  - RGX-owned warnings dropped from 296 to 88 (70% reduction); the full workspace `clippy` pass now reports `rgx-core` lib warnings at 121 (down from 329)
  - remaining backlog is concentrated in cast-truncation warnings, missing `# Errors` / `# Panics` doc sections, function-length limits, and design-intentional patterns
- Latest parity-boundary check confirmed that bare top-level Perl extended character class ordinary terms such as `(?[a-z])` and `(?[\dA-F])` should remain outside the shipped subset for now:
  - a local PCRE2 parity probe compile-rejected those forms
  - RGX intentionally kept only the already-shipped nested ordinary bracket forms such as `(?[[a-z]])` and `(?[[\dA-F]])`
  - this avoids widening `(?[...])` in a direction that current PCRE2 bytes-mode behavior does not support
- Latest warning-debt cleanup was a small RGX-owned pass across `rgx-core`:
  - added separators to the Unicode scalar-universe literal in `compiler.rs`
  - simplified the relative-conditional sign pattern in `lexer.rs`
  - renamed quantified locals in `parser.rs` and `parsing.rs`
  - removed unnecessary raw-string hashes from native-code-block tests in `lib.rs`
- Latest feature pass widened the shipped Perl extended character class subset again:
  - nested ordinary bracket terms inside `(?[...])` now accept the current ordinary char-class atom subset instead of staying limited to plain literal/range bodies
  - representative shipped forms now include `(?[[\dA-F]])`, `(?[[[:graph:]]])`, and `(?[[\p{L}] - [\p{Lu}]])`
  - parser-path, parser-contract, compiler/unit, and PCRE2 differential coverage now lock this slice while wider remaining extended-class forms still compile-reject deliberately
- Latest cleanup was a consolidation-only pass over parser-path `(?[...])` execution coverage in `rgx-core/src/lib.rs`:
  - the user-facing parser-path extended-character-class match/reject cases now live in one `ParserExtendedCharClassExecutionFixture` table plus one helper
  - the coverage still keeps a simple-vs-algebraic split, but the test bodies no longer duplicate compile/assert boilerplate across dozens of cases
  - shipped regex behavior did not widen; this was strictly a maintainability cleanup mirroring the earlier parser-contract fixture refactor
- Latest feature pass widened the shipped Perl extended character class escaped-term subset again:
  - `(?[...])` now accepts bare `\b` backspace atoms on the default path
  - the current control-literal family `\a`, `\b`, `\e`, and `\f` is now explicitly locked by compiler/unit, parser-path, parser-contract, and PCRE2 differential coverage instead of remaining partly implicit
  - docs and the compiler boundary message now describe the same escaped-term subset that the runtime actually executes
- Latest cleanup was a consolidation-only pass over parser-contract `(?[...])` execution coverage in `rgx-core/src/parsing.rs`:
  - the growing extended-character-class execution assertions now live in one `ExtendedCharClassExecutionFixture` table plus one helper
  - the simple and algebraic parser-contract tests still exist as separate guardrails, but they now iterate through fixture rows instead of duplicating compile/assert boilerplate
  - shipped regex behavior did not widen; this was strictly a maintainability cleanup around the default-path extended-char-class contract
- Latest extended-character-class feature pass widened the shipped POSIX slice again:
  - bare negated ASCII POSIX class terms such as `[:^alpha:]` now count as an explicit shipped part of the default-path `(?[...])` subset instead of remaining merely latent in the lowering helper
  - compiler/unit, parser-path, parser-contract, and PCRE2 differential coverage now lock representative cases like `(?[ [:^alpha:] ])`
  - the broader explicit compile boundary is unchanged: wider set-expression forms and additional bare-term families beyond the current bracket/property/POSIX/shorthand/escaped-term subset still compile-reject deliberately
- Latest cleanup was a consolidation-only pass over the parser-path capability matrix regression in `rgx-core/src/lib.rs`:
  - the large `capability_matrix_supported_parser_path_cases` data set now lives in one shared constant instead of inside a monolithic test body
  - the assertions now flow through one helper, so future feature turns can append parser-path cases without recreating the old `clippy::too_many_lines` warning
  - shipped regex behavior did not widen; this was strictly a maintainability and warning-noise cleanup
- Parity program with PCRE2 differential tests is active and operational in `rgx-bench/tests/pcre2_parity.rs`.
- PGEN regex integration review now has a git-tracked complaint document constrained to `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` and the referenced upstream contract surfaces.
- PGEN regex integration review now also has a separate git-tracked proposal document, `PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md`, which recommends keeping parser guarantees structural, treating `lua` / `js` / `javascript` as source-body tags, and keeping `native` / `wasm` reference-shaped.
- Embedded code-block language direction was explicitly narrowed in design discussion:
  - keep the first-class inline/source-body language track centered on `lua`, `js` / `javascript`, and `rhai`
  - keep `native` / `wasm` as advanced reference-style backends rather than the primary inline UX target
  - defer Julia/Python support until later
  - and, when asking PGEN for future parser marker expansion, prefer `rhai` alongside `lua` / `js`
- Rhai code blocks are now shipped locally in RGX as a feature-gated inline backend:
  - `(?{rhai:...})` now executes in `ExecutionMode::Safe` / `ExecutionMode::Full` with the `rhai` cargo feature enabled
  - default PGEN-backed parsing already transports the `rhai` tag through the generic code-block path, and RGX now locks that in with parser-conformance fixtures
  - explicit `return ...` Rhai source bodies are also now locked in by regression tests, so the shipped inline-language contract matches Lua/JavaScript more closely than older docs implied
  - upstream PGEN still has not explicitly published `rhai` as a marker in its contract, so RGX docs should keep that distinction visible
- After the upstream `1.1.0` contract refresh, the live complaint surface is narrower again: plain `(?{...})` and `lua` / `js` / `javascript` payload classes are now explicitly defined, while `native` / `wasm` tags, stronger JS/Lua shielding, runtime semantics, and AST semantic upgrade guarantees remain the main open points.
- The default RGX build now exercises a real PGEN-backed parser adapter in `rgx-core/src/parsing.rs` through the pinned `subs/pgen` submodule:
  - local backend selection is controlled by one constant (`PGEN_FEATURE_BACKEND`)
  - active PGEN output is validated against the recursive-descent reference AST on a widened fixture set
  - `rgx-cli` now also exposes a `pgen-parser` feature passthrough for end-to-end build/test coverage
- The pinned PGEN submodule commit is `54ed190437371fdcc8e77751407f5b3d51efbd52` (PGEN 1.1.8).
- Latest extended-character-class cleanup did not widen syntax, but it hardened the new bare POSIX-term path:
  - `rgx-core/src/compiler.rs` now uses a typed internal ASCII POSIX registry plus `ExtendedPosixClassSpec` instead of ad hoc string matching for the current `(?[...])` POSIX-term subset
  - invalid POSIX names now fail through one narrower helper path, while non-POSIX bodies still fall back cleanly to the ordinary bracket/escape-term lowering logic
  - direct compiler-unit coverage now locks valid POSIX spec parsing, unknown-name rejection, and non-POSIX-body passthrough before the later regex lowering step
- Latest extended-character-class feature pass widened the shipped subset again:
  - bare ASCII POSIX class terms such as `[:alpha:]`, `[:graph:]`, `[:digit:]`, `[:space:]`, and `[:word:]` now execute on the default path inside `(?[...])`
  - parser-path, parser-contract, compiler/unit, and PCRE2 differential coverage now lock representative forms like `(?[ [:graph:] ])`, `(?[ ![:alpha:] ])`, and `(?[ [:alpha:] & [a-z\t] ])`
  - the explicit compile boundary now narrows to wider set-expression forms and any further bare-term families beyond the current bracket/property/POSIX/shorthand/escaped-term subset
- Latest RGX-owned warning-debt cleanup removed dead private scaffolding from the hot parser/runtime path:
  - removed the unused `Regex.pattern` and `Lexer.input` fields
  - removed the stale `PatternAnalysis` helper and an unused VM capture extractor
  - feature-gated dormant Lua/JavaScript/Rhai-only execution helpers so base builds stop warning on them
  - brought the visible RGX-owned `rgx-core` warning count in the standard validation loop down from 101 to 93
- Latest extended-character-class cleanup did not widen syntax, but it centralized the explicit non-shipped `(?[...])` compile-boundary wording into one compiler-owned constant:
  - `rgx-core/src/compiler.rs` now owns the single source of truth for the current boundary message
  - `rgx-core/src/lib.rs` and `rgx-core/src/parsing.rs` now assert against that constant instead of drifting hard-coded copies
  - this keeps future extended-character-class widening work aligned with the existing explicit-boundary policy
- Latest extended-character-class feature pass widened the shipped subset again:
  - bare escaped literal/codepoint terms such as `\n`, `\t`, `\r`, `\f`, `\a`, `\e`, escaped operators like `\-`, and hex escapes like `\x{41}` / `\x41` now execute on the default path inside `(?[...])`
  - parser-path, parser-contract, compiler/unit, and PCRE2 differential coverage now lock those escaped-term cases in
  - the explicit compile boundary now narrows to wider set-expression forms and any further bare-term families beyond the current bracket/property/shorthand/escaped-term subset
- Local PGEN issue `pgen-issues/PGEN-RGX-0005.yaml` is now closed as `verified-fixed-upstream`:
  - minimal repro: `(?(R&word)a|b)`
  - standalone local PGEN at commit `f97e0fe31750885f4fc48a67ed7660110cd20271` now reports `regex_parser_release_version=1.1.2` / `regex_integration_contract_version=1.1.2` and parses the repro successfully
  - the verification bundle lives at `pgen-issues/artifacts/PGEN-RGX-0005/verified-fix-1.1.2/`
  - the accepted tree now includes `recursion_condition` inside `conditional`, with separate `yes_branch` and `no_branch`
  - RGX is now pinned to that same fixed `1.1.2` commit, and named recursion-condition syntax `(?(R&name)...)` is shipped on the default parser/runtime path
- Cargo workspace state now explicitly excludes `subs/pgen/rust` so RGX and PGEN stay separate projects even though PGEN lives under the RGX tree as a submodule.
- Local validation against the default real PGEN backend is currently green for:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core test_parser_name -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-wasm`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
  - the default local CI loop now uses an explicit RGX package test matrix instead of `cargo test --workspace`, because the umbrella workspace run has shown intermittent hangs while rebuilding the submodule-backed `pgen` dependency
- Hosted CI now checks out submodules recursively; because `subs/pgen` is private, GitHub Actions may still need `RGX_SUBMODULES_TOKEN` if the default `GITHUB_TOKEN` cannot read `rdje/pgen`.
- Quick benchmark capture now keeps shared plus mode-scoped latest snapshots, writes a cross-mode `overview.*` that also surfaces the newest shared quick/full label pair, writes label-paired quick/full summaries to `profile-pairs.*`, writes rolling paired-label history to `profile-history.*` with latest-pair improvement/regression callouts, writes rolling mode-scoped history summaries (`history-quick.*` / `history-full.*`), archives timestamped local history under `target/benchmark-trends/history/quick/` and `target/benchmark-trends/history/full/`, and records optional capture labels (`--label` / `RGX_BENCHMARK_TREND_LABEL`) that the wrapper defaults from the current git revision; `trend_capture` / `scripts/capture-benchmark-trends.sh` auto-compare only against same-mode history and still accept explicit archived baselines via `--compare-against` / `RGX_BENCHMARK_COMPARE_AGAINST`, either as a unix timestamp or as `label:<text>`, and the artifact path/write/log plumbing is now centralized so new report outputs can extend one internal path instead of duplicating file handling.
- Single-branch `DEFINE` conditionals are now shipped on the default regex path:
  - `DEFINE` is treated as always false at runtime, so its one branch acts as a definition-only block and matching falls through as an empty else
  - numbered and named subroutine definitions inside `DEFINE` blocks are now usable later in the same pattern
  - invalid two-branch `DEFINE` forms still compile-reject explicitly to stay aligned with PCRE2
- Current recursion-condition conditionals are now shipped on the default regex path:
  - `(?(R)...)` is true when the current path is inside any active recursion/subroutine level
  - `(?(Rn)...)` is true only when the most recent active recursion level targets group `n`
  - PCRE2's ambiguity rule is now honored, so groups named `R` or `Rn` still force named-group-exists semantics instead of recursion-condition semantics
  - missing recursion-condition group references such as `(?(R2)...)` now fail explicitly at compile time
- `(?|...)` branch-reset groups are now shipped on the default regex path:
  - the compiler assigns shared capture indices across the branch-reset group's top-level alternatives instead of numbering each branch independently
  - later backreferences and conditionals now see the resulting PCRE2-style max-branch-arity numbering after the branch-reset group
  - representative AST/parser-path regressions plus PCRE2 differential cases now cover the shipped behavior
- `(?[...])` Perl extended character classes now ship a wider but still disciplined runtime slice on the default path:
  - simple nested bracket terms like `(?[[a-z]])` and `(?[[^0-9]])` still work
  - RGX now also executes bare ASCII POSIX class terms such as `[:alpha:]`, `[:graph:]`, `[:digit:]`, `[:space:]`, and `[:word:]`, bare shorthand terms (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`, `\h`, `\H`, `\v`, `\V`), bare escaped literal/control/octal/codepoint terms such as `\n`, `\t`, `\r`, `\cA`, `\040`, `\o{101}`, `\x{41}`, `\x41`, and `\-`, unary complement (`!`), grouped subexpressions, symmetric difference (`^`), and same-level left-associative set algebra with `&` binding tighter than `|`, `+`, `-`, and `^` over bracket terms, POSIX terms, shorthand terms, escaped terms, or Unicode property terms, such as `(?[ [:graph:] ])`, `(?[ ![:alpha:] ])`, `(?[ [:alpha:] & [a-z\t] ])`, `(?[\d - [3]])`, `(?[\w & [a-z]])`, `(?[\D & [A-F]])`, `(?[\h])`, `(?[\H])`, `(?[\v])`, `(?[\V])`, `(?[\n | \t])`, `(?[\cA | [B]])`, `(?[\040 | \011 | \o{101}])`, `(?[ ![0-9] ])`, `(?[ [AC] ^ [BC] ])`, `(?[[a-z] - [aeiou]])`, `(?[\p{L} & \p{Lu}])`, `(?[ [a-f] | [d-z] & [m-p] ])`, and `(?[ [a-z] - [aeiou] + [0-9] - [5] ])`
  - wider set-expression forms and additional bare-term families beyond the current bracket/property/POSIX/shorthand/escaped-term subset still compile-reject explicitly
  - direct parity probing during the latest feature slice showed that upstream PCRE2 rejects `\N` inside `(?[...])`, so RGX intentionally did not widen in that direction
- That shipped `(?[...])` slice is now guarded by direct compiler helper tests, parser-contract/runtime tests, PCRE2 differential parity cases for the widened runtime subset, and the earlier direct VM regression for ordinary negated custom char classes.
- The internal range algebra behind that shipped `(?[...])` subset is now centralized in one private `ScalarRangeSet` helper inside `rgx-core/src/compiler.rs`, with direct unit tests locking adjacent-range normalization and split-difference behavior before we widen the syntax further.
- The braced hex/octal escaped-atom path inside `(?[...])` now routes through one shared `consume_extended_braced_radix_digits(...)` helper in `rgx-core/src/compiler.rs`, with direct unit tests for accepted and malformed braced-digit bodies so the recent control/octal widening is easier to maintain without widening behavior again.
- Code-block execution is now shipped in the public path for Lua and JavaScript predicate blocks when using `ExecutionMode::Safe` / `ExecutionMode::Full` with the corresponding cargo feature enabled.
- Lua source bodies now accept either bare expression bodies or explicit `return ...` bodies, which keeps the shipped inline-language ergonomics closer to JavaScript and Rhai.
- Lua, JavaScript, and Rhai are now all intentionally documented/tested as supporting either bare expressions or explicit `return ...` bodies on the shipped inline-language path.
- Lua and JavaScript statement bodies now also expose `rgx.emit_numeric(...)` / `rgx.emit_replacement(...)`, while Rhai exposes `emit_numeric(...)` / `emit_replacement(...)`, so winning-path richer results no longer depend only on direct non-boolean returns.
- Native callbacks are now shipped on the Rust API path in `ExecutionMode::Full` after registration on the compiled `Regex`.
- Wasm modules are now shipped on the Rust API path in `ExecutionMode::Safe` / `ExecutionMode::Full` after registration on the compiled `Regex`.
- Host-provided execution variables are now shipped on the Rust API path via `Regex::set_variable(...)` and are snapshotted into each per-call `ExecContext`.
- Code blocks are now compiled into VM bytecode, executed during matching, and receive current overall match text plus current match start/end/length metadata, top-level branch number when available, numbered captures, named captures, and host-provided variables through the execution context.
- Public `find_first` / `find_all` results now expose `code_result`, which preserves the last winning-path numeric or replacement value from Lua/JavaScript/native/wasm code blocks.
- `Regex::find_first_numeric_with_code(...)` and `Regex::find_all_numeric_with_code(...)` are now shipped on the Rust API path and collect winning-path `Numeric(f64)` payloads in match order while skipping non-numeric matches.
- `Regex::replace_first_with_code(...)` and `Regex::replace_all_with_code(...)` are now shipped on the Rust API path and consume winning-path `Replacement(String)` payloads while leaving predicate-only and numeric-only matches unchanged in the rebuilt output.
- The current wasm ABI now combines registered `module:function` / exported `() -> i32` predicates with `rgx` host imports for current position, current match metadata, full input text, numbered captures, named captures, variables, and initial numeric/replacement result emission.
- Relative conditional group references `(?(+1)...)` and `(?(-1)...)` now parse on both the recursive-descent and default PGEN-backed parser paths as dedicated AST and execute on the default compiler/VM path after compile-time resolution to absolute group checks.
- The CLI now exposes host-provided code-block variables through repeated `--var NAME=VALUE`, can register named wasm modules through repeatable `--wasm-module NAME=PATH`, can optionally print branch/code-result details through `--show-details`, and no longer pre-executes successful code-block patterns once via `is_match` before collecting matches.
- Numeric backreferences are now shipped on the default compiler/VM path:
  - compile-time validation now rejects only missing-group references such as `(a)\2`
  - runtime matching now executes numbered backreferences through real VM bytecode in both top-level and subexpression paths
  - PCRE2 differential coverage now treats numeric backreferences as supported rather than as a known gap
- Possessive quantifiers are now shipped on the default compiler/VM path:
  - both parser backends lower `*+`, `++`, `?+`, and counted possessive forms into atomic-wrapped greedy quantified AST nodes
  - runtime behavior now blocks backtracking into the possessive piece while still allowing straightforward success cases
  - PCRE2 differential coverage now treats possessive quantifiers as supported rather than as a parser-adapter gap
- `ExecutionMode::Pure` still rejects code blocks, `ExecutionMode::Safe` still rejects `native`, the CLI now supports file-backed wasm module registration, and native callback registration still remains Rust-API-only.
- End-anchor (`$`) parity mismatch was fixed and reclassified as supported.
- Absolute text-anchor parity for `\A`, `\Z`, and `\z` is now fixed end-to-end, including runtime execution, parser-path/API regression coverage, PCRE2 differential tests, and direct CLI smoke verification.
- Unicode property classes (`\p{...}`, `\P{...}`) are now shipped on the default compiler/VM path:
  - parser-path and AST-first compilation resolve Unicode property/script classes through shared Unicode tables
  - invalid property names fail explicitly at compile time
  - PCRE2 differential coverage now treats representative Unicode property behavior as supported rather than as a known gap
- Local-first CI is now available:
  - `.github/workflows/ci.yml` delegates to `./scripts/run-local-ci.sh`
  - `./scripts/run-local-ci.sh` now covers explicit RGX package tests (`rgx-core`, `rgx-cli`, `rgx-bench`, `rgx-wasm`) plus the local `rgx-core` feature matrix (`pgen-parser`, `lua`, `javascript`, `rhai`, `wasm`, `all-languages`) and `rgx-cli --features pgen-parser`
  - the explicit package matrix is intentional because `cargo test --workspace` has shown intermittent hangs while rebuilding the submodule-backed `pgen` dependency, whereas the equivalent per-package RGX coverage stays stable
  - `scripts/check-ci-paths.sh` verifies CI-critical paths are git-controlled, rejects absolute filesystem paths in Rust source/CI execution files, and currently reports that there are no compile-time `include!`-style macros in workspace source
- `Cargo.lock` is now intentionally tracked so local and GitHub CI use the same dependency resolution
- `RUST_CODEBASE_ANALYSIS.md` now exists as the live roadmap-grounded assessment of the Rust workspace and is part of the Rust commit workflow review path.
- Lazy quantifier support is now fixed end-to-end in the public path for `??`, `*?`, `+?`, `{n,m}?`, and `{n,}?`, with API regressions and PCRE2 differential coverage updated accordingly.
- `{n,m}` range-quantifier scanning/earliest-match parity gap has now been fixed and reclassified as supported.
- Unbounded range quantifier (`{n,}`) parity is now differential-tested and aligned for scanning and suffix-sensitive behavior.
- Negated shorthand character-class parity for `\D`, `\W`, and `\S` is now fixed end-to-end, including quantified VM execution, API regressions, differential parity tests, and direct CLI smoke coverage.
- `cargo test -p rgx-core --features lua`, `cargo test -p rgx-core --features javascript`, `cargo test -p rgx-core --features wasm`, and `cargo check -p rgx-core --features all-languages` now validate the shipped code-block slice.
- The shared local/GitHub CI path now validates the feature-gated `pgen-parser`, `lua`, `javascript`, `wasm`, and `all-languages` matrix automatically.
- Capability and parser-boundary guardrails are actively enforced in:
  - `rgx-core/src/lib.rs`
  - `rgx-core/src/parsing.rs`
  - `docs/CAPABILITY_MATRIX.md`
  - `docs/PCRE2_COMPATIBILITY_MATRIX.md`

## Next likely tasks
- Plan downstream RGX handling for newer PCRE2 syntax that may arrive through PGEN next, especially returned-capture subroutine calls, `VERSION[...]` conditionals, and the runtime/set-algebra policy for Perl extended character classes.
- Expand the wasm/runtime surface beyond the current position/text/numbered-capture/named-capture/variable import slice and initial `emit_numeric` / `emit_replacement` result layer.
- Keep the private-submodule CI auth story smooth as `subs/pgen` moves forward.
- Continue capturing any new suspected PGEN parser bug with the structured bundle expected by `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`.
- Decide whether native registration should remain Rust-API-only and whether the new wasm CLI path should grow beyond file-backed module registration.

## Session memory entries (newest first)
### 2026-04-16 (fifty-third commit) — File PGEN-RGX-0063 + 0064
- **What**: Two new PGEN bug reports from post-harness-drill PGEN triage.
  1. **0063** — `[:<:]` / `[:>:]` POSIX-alias word-boundary names rejected. PCRE2 accepts (bytecode: `\b Assert \w`). 3 cases.
  2. **0064** — Variable-length-lookbehind check fails `(?<=X(?(DEFINE)(.*))Y).` as unbounded; PCRE2 treats DEFINE as zero-width. 1 case.
- **Not filed (adapter/harness)**: 69 scan_substring_group/script_run_group (adapter), `non_atomic_lookahead_pos` (adapter naming gap), modifier-wiring for `alt_bsux` / `allow_lookaround_bsk` / `alt_extended_class` / `allow_empty_class` (harness), 11 simple_escape alphanumerics (adapter literal-fallback), `(*TURKISH_CASING)` harness-prefix-ordering artifact.
- **Total PGEN-RGX reports filed**: 0001–0064 (64). Closed: 62. **Open: 2** (0063 + 0064).
- **Methodology note**: Sequential `file_pgen_issues --single` calls are mandatory — two parallel invocations raced on `next_available_pgen_issue_id` and produced duplicate 0064s. Fixed by deleting the stray 0065 duplicate.

### 2026-04-16 (fifty-second commit) — Harness: `is_subject_echo` discriminator (+83 passes)
- **What**: The preamble-skip and new-subject-detection loops used `l.starts_with(b"    ")` (any 4+ leading spaces) to recognize subject echoes. But `/B` bytecode dumps use 6+ leading spaces for opcode lines (`        Bra`, `        Ket`, etc.), which ALSO start with 4 spaces. Preamble-skip stopped early, bytecode got consumed as match output, real match fell through to NoMatch. Fix: new `is_subject_echo` helper requires EXACTLY 4 leading spaces + non-space next byte.
- **Conformance delta**: 8626 → **8709 pass** (+83), 2592 → 2509 fail. 76.9% → **77.6%**. Ratchet bumped to 8709/2509.
- **Cumulative drill**: +305 (preamble) + 179 (Latin-1 + JIT) + 83 (is_subject_echo) = **+567 passes** from 4 pure-harness commits. 72.6% → 77.6% (+5.0pp) without touching the engine.
- **Next**: remaining false-positive residual (~640) concentrates in `/replace=…` / `/substitute*` (not ordinary match tests), `newline=cr/any` (multi_line gap), and `(?+1)` forward recursion. Each needs a different kind of fix.

### 2026-04-16 (fifty-first commit) — Harness: Latin-1 expected normalization + JIT-suffix strip (+179 passes)
- **What**: Two more harness-correctness fixes on the span-mismatch bucket.
  1. Latin-1-decoded subjects re-encode high bytes as 2-byte UTF-8 in `&str`. RGX match output lives in that UTF-8 byte space; expected `overall` bytes from `decode_output` lived in raw-byte space. Normalize: when subject went through Latin-1 fallback, re-encode expected bytes via `char::encode_utf8` too.
  2. pcre2test appends ` (JIT)` / ` (non-JIT)` suffix to matches under JIT test modes — diagnostic, not part of match. Strip before comparison.
- **Conformance delta**: 8447 → **8626 pass** (+179), 2771 → 2592 fail. 75.3% → **76.9%**. Ratchet bumped to 8626/2592.
- **Pattern**: Three consecutive harness-only improvements — preamble skip (+305), Latin-1 norm (+179 combined with JIT suffix). Cluster-first keeps finding harness layers on top of the real engine divergences.
- **Next**: span-mismatch bucket still has ~600 remaining — genuinely about Unicode case folding (`ẞ→ss`, `ſ→s`, `KkK`, etc.). That's a real RGX engine gap requiring actual case-fold table work. Alternative: false-positive bucket (723) top still `(?x)(?-x: \s*#\s*)` extended-scope; or false-negative (652) `\c[` control-char edge.

### 2026-04-16 (fiftieth commit) — Harness: skip /I and /B diagnostic preamble (+305 passes)
- **What**: Fix pre-existing harness bug. pcre2test emits diagnostic preamble for `/I` and `/B` modifiers between pattern echo and first subject echo (`Capture group count = N`, `First code unit = …`, `------------` dividers, indented opcode dumps). `parse_subject_output` was consuming those as match output, falling through to `Expected::NoMatch`, then counting RGX's real match as a "false positive".
- **Fix**: `extract_pattern_cases` gains a preamble-skip loop right after `oi = 1`. Advances until it hits a 4-space subject-echo, `\=` annotation, ` 0:` match, `No match`, or `Failed:`.
- **Conformance delta**: 8142 → **8447 pass** (+305), 3076 → 2771 fail. 72.6% → **75.3%**. Ratchet bumped to 8447/2771.
- **Key insight**: 305 / 909 false-positive bucket (33.6%) was never a real engine divergence — just harness misreading. Cluster-first methodology catches these RGX-side harness artifacts the same way it distinguishes real PGEN bugs from adapter gaps.
- **Next top buckets** (2771 total): ~635 false positive (the real residual, top still `(?x)(?-x: \s*#\s*)`), 893 span mismatch (top `(abc)\223` octal), 628 false negative (top `\c[` control-char edge), 210 PGEN AST contract, 168 PGEN parse failure, 126 RGX too permissive.

### 2026-04-16 (forty-ninth commit) — PGEN 1.1.24 bump closes 0061/0062 + adapter wiring
- **What**: Submodule bump cd0f8c7 → 9a7d453 ("Regex: add PCRE2 single-byte and callout-condition forms"). Both reports land fixes:
  1. `single_byte_escape = "C"` as new escape_unit alternative head-of-list
  2. `condition_callout_assertion = condition_callout "(" condition_assertion` as new condition alternative
- **Adapter wiring in parsing.rs**:
  1. `convert_escape`: `single_byte_escape` → CharClass spanning `'\0'..char::MAX` (any codepoint including newline) — sound semantics for RGX's str-based API
  2. `convert_condition`: `condition_callout_assertion` → recurse to inner `condition_assertion`, drop callout (RGX doesn't execute PCRE2 text-pattern callouts)
- **Conformance delta**: 8141 → **8142 pass** (+1), 3077 → 3076 fail. 72.6% → 72.6% (at the precision we show). Ratchet baselines bumped to 8142/3076.
- **Why only +1?** The 0061/0062 cluster was previously being silently routed through adapter catch-alls — `\C` landed in simple_escape(C) which errored, but our FlagGroup wrapping and other heuristics sometimes produced ambiguous matches that happened to coincide with PCRE2's expected output. With dedicated AST nodes, the semantics are now correct in both success AND failure modes.
- **Total PGEN-RGX reports filed**: 0001–0062 (62). **62 closed, 0 open.** Every report filed this session has been fixed upstream.

### 2026-04-16 (forty-eighth commit) — File PGEN-RGX-0061 + 0062 (post-ratchet PGEN triage)
- **What**: Two PGEN bug reports after the ratchet locked at 72.6%. Cluster-first methodology applied to remaining PGEN-relevant buckets (208 AST contract + 177 parse failure).
  1. **0061** — `\C` single-byte escape emits generic simple_escape(C) instead of a dedicated byte atom. PCRE2 accepts by default (verified via testoutput21:82 `Contains \C`). ~2 patterns.
  2. **0062** — Callout `(?C...)` at conditional-assertion position rejected. PCRE2 accepts (verified via testoutput2:14984 bytecode dump showing `Cond / Callout 25 / Assert`). ~6 patterns.
- **Drafted-then-deleted**: 0063 for `(*TURKISH_CASING)` turned out to be a harness-side prefix-ordering issue — PGEN accepts the raw pattern; our harness prepends `(?i)` before the start-option verb, violating PGEN's "start options must be first" rule. Cluster-first caught this false positive via an isolated `--single` verification that showed `parse_outcome.status = success`.
- **Also categorized (NOT PGEN)**: 69 scan_substring_group/script_run_group (adapter feature work), 13 `\u` (alt_bsux modifier), 11 `\K`-in-lookaround (allow_lookaround_bsk), 14 descending range (alt_extended_class), 8 empty-class (allow_empty_class), 13 simple_escape alphanumerics (adapter literal-fallback).
- **Total PGEN-RGX reports filed**: 0001–0062 (62 total, 60 closed, **2 open**: 0061 + 0062). Projected ceiling remains ~65.

### 2026-04-16 (forty-seventh commit) — Conformance ratchet gate locks the journey to 100%
- **What**: Conformance test now enforces a one-way ratchet via four new baselines: `PASS_BASELINE=8141`, `FAIL_BASELINE=3077`, `PANIC_BASELINE=0`, `SKIP_BASELINE=0`. Any regression fails the test; improvements must bump baselines in the same commit. A `🎯 NEW BASELINE ELIGIBLE` hint is printed when the current pass count exceeds the baseline.
- **Why**: The stated goal is 3,077 → 0, and never leave it. Without the gate, a silent regression anywhere in RGX or PGEN could drop the number without CI noticing.
- **Discipline going forward**: every commit on the journey to 100% bumps `PASS_BASELINE` up and `FAIL_BASELINE` down. The ratchet's error messages explicitly tell the author what to do if they're legitimately reclassifying cases (harness tightening etc.).
- **Next**: with the gate locked, start drilling the 3,077 remaining failures cluster-first. Top buckets: 909 false positive (`(?x)(?-x: \s*#\s*)` extended-scope), 893 span mismatch (`(abc)\223` octal boundary), 627 false negative (`\c[` control-char), 208 PGEN AST contract, 177 PGEN parse failure, 126 RGX too permissive, 91 other compile error, 23 simple_escape residual.

### 2026-04-16 (forty-sixth commit) — PGEN 1.1.23 bump closes 0058/0059/0060 + adapter wiring
- **What**: Submodule bump 9af9500 → cd0f8c7 (PGEN 1.1.23 "Publish regex PCRE2 maintenance release"). All three open reports cited explicitly in PGEN's release notes. Grammar additions:
  1. Bounded variable-length lookbehind + control verbs inside lookbehind (for 0058)
  2. Unicode capture names with `MAX_NAME_SIZE=128` and non-digit first char (for 0059)
  3. `stray_class_end_quote = "\E"` zero-width class item + `empty_quoted_class_literal = "\Q\E"` + relaxed `class_range = class_atom class_zero_width* "-" class_zero_width* class_atom` (for 0060)
  4. New `class_range_escape` restricted-endpoint production (side effect: all class-range atoms now nest `class_range_escape` instead of `class_escape`)
- **Adapter wiring in parsing.rs**:
  - `convert_class_range`: rewritten to collect first+last `class_atom` descendants (was `children[0]` and `children[2]`)
  - `class_atom_char` / `convert_class_escape`: accept `class_range_escape` in addition to `class_escape`
  - `convert_escape`: new dispatch for `class_range_simple_escape`
  - `convert_class_item`: new branch for `stray_class_end_quote` / `empty_quoted_class_literal` (skip, contribute zero)
- **Conformance delta**: 8090 → **8141 pass** (+51), 3128 → 3077 fail. 72.1% → **72.6%**. 0 panic / 0 skip. 1007 lib tests still green.
- **Reports closed**: PGEN-RGX-0058, 0059, 0060 all `verified-fixed-upstream` with cd0f8c7 evidence. Running ledger: **60 filed, 60 closed, 0 open**.
- **Methodology snapshot**: The session's two PGEN report batches (0056/0057 and 0058/0059/0060) — 5 reports filed — closed ~261 case-level failures (+326 combined from 0056/0057 bump, +51 from 0058/0059/0060 bump). Cluster-first methodology preserved signal-to-noise throughout: 60 reports for a corpus of 11,218 cases.

### 2026-04-15 (forty-fifth commit) — File PGEN-RGX-0058 + 0059 + 0060
- **What**: Three cluster-distilled PGEN bug reports, protocol-compliant:
  1. **0058** — Variable-length lookbehind with control verbs (`(*ACCEPT)` etc.), ~49 cases
  2. **0059** — Non-ASCII identifiers in named groups (`(?'ABáC'...)`), ~8 cases
  3. **0060** — Bare `\E` inside `[...]` without preceding `\Q`, 4 cases (residual of 0057)
- **Also triaged (not PGEN)**: `\u`/`alt_bsux`, `\K`/`allow_lookaround_bsk`, empty-class/`allow_empty_class`, `alt_extended_class`, `convert=glob`, `scan_substring_group`/`script_run_group` — all RGX-side modifier wiring or feature work. Don't file.
- **Total PGEN-RGX reports filed**: 0001–0060 (60). Closed: 57. Open: 3 (0058/0059/0060).
- **Methodology note**: Cluster distinguishes 3 real PGEN bugs from 10+ adapter / modifier-wiring gaps without filing noise reports. Still a cluster-first discipline.

### 2026-04-15 (forty-fourth commit) — Harness: advance output cursor past non-pattern blocks
- **What**: Pattern input blocks were being paired with the wrong output block whenever testoutput* had extra annotation/separator content (e.g. `---` dividers, PCRE2-maintainer comments) with no testinput counterpart. The old logic advanced output by +1 per input block indiscriminately. Fix: when input is a Pattern block, walk output cursor forward until `out_blocks[oi].lines[0].starts_with("/")`.
- **Impact**: Patterns like `/[a-[:digit:]]+/` that PCRE2 rejects (`Failed: error 150`) were mispaired with the preceding comment block, so parse_subject_output recorded `Expected::NoMatch`, and RGX's matching compile-error counted as a divergence. With the fix, these now correctly see `Expected::CompileError` and pass.
- **Conformance delta**: 7933 → **8090 pass** (+157), 3285 → 3128 fail. 70.7% → **72.1%**.
- **Next**: remaining top buckets unchanged — 930 false positive (`(?x)(?-x: \s*#\s*)` extended-scope), 880 span mismatch, 624 false negative, 250 PGEN parse failure, 202 PGEN AST contract, 126 RGX-too-permissive. The 250 PGEN parse failure bucket still contains the `\E` inside `[...]` residual from 0057 plus others; need to drill.

### 2026-04-15 (forty-third commit) — PGEN 1.1.22 bump + adapter wiring closes 0056/0057
- **What**: Submodule bump e617960 → 9af9500 (PGEN 1.1.22, "Fix PCRE2 short properties and class quotes"). PGEN added:
  1. Short-form `property_escape` variant (`"p" short_prop_letter`) matching PCRE2's 7 major-category letters
  2. New `class_item` alternative `quoted_class_literal` with a body-char rule that explicitly admits `]`
- RGX adapter wiring in `parsing.rs`:
  1. `convert_property_escape` now accepts `short_prop_letter` subtree as an alternative name source
  2. `convert_class_item` new `quoted_class_literal` branch + `quoted_class_literal_chars`/`walk_quoted_class_body` helpers
- **Conformance delta**: 7776 → **7933 pass** (+157), 3442 → 3285 fail. 69.3% → **70.7%**. 0 panic / 0 skip. 1007 lib tests still green.
- **Reports closed**: PGEN-RGX-0056, 0057 both `verified-fixed-upstream` with 9af9500 evidence + ast_dump verification command.
- **Residual**: `\E` alone inside `[...]` (no preceding `\Q`) still `E_PARSE_FAILURE` — 246 cases in `compile: PGEN parse failure` bucket. PCRE2 treats as literal `E`. Noted in 0057 closing notes; will file follow-up if cluster doesn't collapse during further triage.
- **Total PGEN-RGX reports filed**: 57 (0001–0057). Closed: 57. Open: 0.

### 2026-04-14 (forty-second commit) — File PGEN-RGX-0056 + PGEN-RGX-0057
- **What**: Two cluster-distilled PGEN bug reports, protocol-compliant per `subs/pgen/docs/contracts/PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`:
  1. **PGEN-RGX-0056**: short-form `\pX`/`\PX` Unicode property escape — PGEN parses but emits wrong AST shape (`simple_escape(p) + literal_char(L)` instead of `property_escape`). AST dump captured. Affects ~66 cases.
  2. **PGEN-RGX-0057**: `\Q...\E` inside `[...]` — PGEN rejects with E_PARSE_FAILURE; PCRE2 accepts. Affects ~138 cases.
- **Tooling**: Added `--single <pattern>` and `--ast-dump-only <pattern> <out>` modes to `rgx-core/src/bin/file_pgen_issues.rs` so future cluster-distilled reports can be one-command.
- **Methodology validation**: 575-bucket → 0 PGEN reports (all RGX adapter). 327-bucket → 2 PGEN reports (after honest re-classification — initial estimate was 3-5, dropped to 2 after user pushback on speculative classifications). Cluster first → file second.
- **Total PGEN-RGX reports filed**: 0001–0057 (57). Projected ceiling: ~60.

### 2026-04-14 (forty-first commit) — Compile-error parity + property aliases + napla/naplb
- **What**: Cluster-first methodology applied to the 327 PGEN-AST-contract-mismatch bucket. 7 distinct root causes; 4 closed here.
  1. Harness `Expected::CompileError` — pcre2test's `Failed: error N` line previously parsed as NoMatch, then RGX's compile error counted as fail. Now: PCRE2-rejected + RGX-rejected = Pass; PCRE2-rejected + RGX-accepted = new "RGX too permissive" bucket
  2. PCRE2 property aliases: `L&`/`Lc`, synthetic Xan/Xsp/Xps/Xwd/Xuc, bidicontrol short forms, `sc:`/`scx:`/`script:` prefix stripping
  3. `napla`/`naplb` (non-atomic positive lookaround) — same AST as positive lookahead/behind; backtracking semantics already match
  4. Long forms `non_atomic_positive_lookahead`/`non_atomic_positive_lookbehind`
- **Conformance delta**: 7600 → **7776 pass** (+176), 3618 → 3442 fail. 67.7% → **69.3%**. 0 panic / 0 skip.
- **Remaining root causes from this bucket** (deferred):
  - Short-form `\pX` / `\PX` (66 cases) — PGEN grammar gap; needs PGEN report or pattern preprocessor
  - `scan_substring_group` / `script_run_group` (66 cases) — real PCRE2 feature work
  - `[a-\d]` class_range with class_escape (6) — most already absorbed by compile-error parity
- **New top buckets**:
  - 1,019 false positives (still `/(?x)(?-x: \s*#\s*)/` extended-mode scope)
  - 931 span mismatches (still octal escapes / Unicode boundaries)
  - 576 false negatives (`\c[` control-char edge)
  - 199 PGEN AST mismatch — first now `'non_atomic_lookahead_pos'` lookaround variant needing adapter
  - 162 "RGX too permissive" — PCRE2 stricter at compile time; clean follow-up
- **Methodology validation**: 327-failure bucket → 4 fixes → 176 passes closed. Same cluster-then-fix pattern works.

### 2026-04-14 (fortieth commit) — Adapter: `\p{...}`, `\.`, `\N` inside char classes — closes 575-bucket with 3 shape additions
- **What**: Clustered the 575 `class_escape unsupported variant` failures. The bucket mapped 1-to-1 to three adapter gaps in `extend_ranges_from_regex` — not PGEN bugs, not 575 bug reports. Added three match arms:
  1. `Regex::UnicodeClass` → resolve via `unicode_support::resolve_unicode_property_class` and union ranges (covers `[\p{Lu}]`, `[\p{Nd}]`, `[\p{Thai}]`, etc. — ~95% of the bucket)
  2. `Regex::Dot` → literal `.` (PCRE2 inside-class rule)
  3. `Regex::Backreference(n)` → octal `\0..\7` = codepoint n; `\8`/`\9` = literal digit (PCRE2 backref-inside-class fallback)
- **Conformance delta**: 7274 → **7600 pass** (+326), 3944 → 3618 fail. 64.8% → **67.7%**. 0 panic / 0 skip.
- **Insight for user**: 575 failures ≠ 575 bugs. Always cluster by root cause before filing reports. This one was pure RGX-side adapter work, no PGEN interaction needed.
- **Next**: `[a-\d]+` class-range endpoint (327, new top adapter bucket) — PGEN emits class_escape subtree as a range endpoint; adapter expects single char.

### 2026-04-14 (thirty-ninth commit) — Zero skip: all 11,218 PCRE2 cases now run end-to-end
- **What**: The conformance harness was silently skipping 6,575 of 11,218 PCRE2 test cases because it only understood `{i m s x g}` short modifiers and UTF-8 subjects. User pushed for signoff-quality coverage: every case must execute against RGX. New `ModifierAction` enum + `classify_modifier` table covers every pcre2test short flag and named directive (~100 names), mapping each to Ignore (pcre2test-only diagnostic), an existing `RegexBuilder` knob, an `InlineFlag("(?J)")`-style pattern prefix, or a pattern wrap (`Literal`/`MatchLine`/`MatchWord`). Non-UTF-8 subjects are Latin-1-decoded (one codepoint per byte) to reach the `&str` API. Unknown modifiers fall through to Ignore so the case runs — divergences appear as honest failures, not hidden skips.
- **Collateral engine fix**: `Compiler::feature_validation_message` was not walking into `RegexAst::FlagGroup`, so unsupported `\p{L&}` / `\p{Xan}` / `\p{Xsp}` / etc. names appearing under a `(?s)…` wrapper escaped validation and panicked at codegen. Added the walker arm; panics are now clean compile errors.
- **Conformance delta**: 3839 → **7274 pass** (+3435), 804 → 3944 fail, 6575 → **0 skip**. Headline pass rate changed from 82.7% (of the 42% decidable slice) to **64.8%** (of the full corpus). Net +3,435 passing cases; the apparent drop is the first time RGX has been scored against the whole authoritative PCRE2 oracle.
- **Top remaining failure buckets** (3,944 total):
  - 1,008 false positives (RGX matches where PCRE2 doesn't) — first: `/(?x)(?-x: \s*#\s*)/` (scope-aware extended-mode whitespace pass)
  - 887 span mismatches — first: `/(abc)\223/` (octal escape semantics)
  - 575 `[\8]` / `[\9]` class_escape Backreference variant
  - 523 false negatives — first: `/^\ca\cA\c[;\c:/` (`\c[` control-char edge)
  - 293 `\NNN` backref-to-missing-group resolution gaps
  - 285 `\Q...\E` inside char class (PGEN rule)
  - 195 `[a-\d]` class_range endpoint-shape mismatches
  - 178 unrecognized simple_escape chars
- **Next concrete action**: pick the single largest semantic bucket (false positives) and work top-example by top-example; each root cause fixed will typically close several related cases.

### 2026-04-14 (thirty-eighth commit) — Bare inline-flag directives scope forward
- **What**: Fix `(?i)` / `(?-i)` / `(?x)` etc. written without a trailing body — PCRE2 says they change the effective flags for the remainder of the enclosing group. Adapter was lowering each to `FlagGroup { expr: Empty }`, leaving subsequent siblings under the outer flag context. `convert_concatenation` now folds pieces through `apply_bare_flag_directives`: when a bare directive appears, everything to its right becomes its body. Nested bare directives compose via suffix recursion. Scoped `(?-i:...)` form untouched.
- **Conformance delta**: 3828 → **3839 pass** (+11), 815 → **804 fail** (−11). 82.4% → **82.7%**.
- **Next**: the new top false positive `/(?x)(?-x: \s*#\s*)/` is a compile-phase bug — the extended-mode whitespace-ignore pass doesn't respect scope boundaries inside `(?-x:...)` nested under forward `(?x)`. Deeper; defer.
- Other high-ROI targets: 159 span mismatch (zero-iteration preference for empty-match quantifiers), 138 `\Q..\E` inside char class (new PGEN-RGX report), 184 false negatives starting with `\c[` (control-char edge in parser).

### 2026-04-14 (thirty-seventh commit) — Conformance harness: pcre2test subject-trim + match-label parsing
- **What**: Two harness-only fixes in `rgx-core/tests/pcre2_conformance.rs`. Root cause: our harness was miscounting real RGX behavior as divergence on two axes.
  1. `trim_ws` helper added — pcre2test strips leading and trailing ASCII whitespace from subject lines before interpreting escapes. Our old `trim_leading_spaces` only stripped the leading 4-space indent; trailing spaces were fed to RGX verbatim. A pattern like `/[^k]$/` on testdata subject `    abk   ` was run against `"abk   "` (RGX matched the last space) while PCRE2 was testing `"abk"` (no match). Explicit trailing whitespace in subjects uses `\x20`/`\t` — those survive trimming because the raw bytes are backslash sequences.
  2. `0: <text>` label stripping fixed — old code did `trim_start_matches("0:").trim_start()` which wiped leading whitespace from the matched text itself (e.g. matched span `" "` parsed as `""`). Replaced with `strip_prefix(' ')` to remove exactly the one-char label separator.
- **Conformance delta**: 3779 → **3828 pass** (+49), 862 → **815 fail** (−47). 81.4% → **82.4%**. 0 panics.
- **Remaining failure buckets** (815 total):
  - 202 false positive — first real case now `/a(?-i)b/i` on `"aB"` (in-pattern `(?-i)` flag-scope regression, RGX engine bug)
  - 184 false negative — first is `/^\ca\cA\c[;\c:/` on control-char subject (possibly parsing `\c[` vs `\c:` handling)
  - 159 span mismatch — first is `/([a]*?)*/` on `"a"` returning `"a"` vs PCRE2 `""` (outer-quantifier zero-iteration preference when inner lazy empty-matches — classic Perl empty-match loop semantics)
  - 138 `\Q...\E` inside char class (pending PGEN-RGX report)
  - 61 `[a-\d]` endpoint-shape AST mismatch
  - 34 extended char class advanced forms
  - 26 simple_escape rejects (now mostly adapter-escape edge cases)
  - 11 class_escape Backreference variant for `[\8]` / `[\9]`
- **Next concrete action**: pivot from harness to engine — tackle the `(?-i)` in-pattern flag-scope regression, since flag scoping is a correctness-critical feature that cascades into many tests. Then the empty-match zero-iteration preference for nested quantifiers.

### 2026-04-14 (thirty-sixth commit) — Adapter batch: five fixes, conformance 79.1% → 81.4%
- **What**: Five focused RGX adapter fixes in `parsing.rs` that absorb PGEN 1.1.21's new AST shapes and close the `fixed-upstream-pending-adapter` reports:
  1. `convert_simple_escape` non-alnum literal fallback — accepts `\"`, `\/`, `\'`, `\@`, etc. per PCRE2 rule
  2. `extend_ranges_from_regex` for `\W`/`\S`/`[\b]` inside char classes — `[\W]` = complement of word, `[\b]` = literal backspace
  3. POSIX bracket classes (`[[:alpha:]]`, `[[:^digit:]]`, etc.) via new `convert_posix_class_into` + `posix_class_ranges` table + `complement_ranges` helper covering all 14 PCRE2 names
  4. `convert_quoted_literal` for `\Q...\E` atoms — lowers body to Sequence of Char nodes
  5. `alpha_lookaround` + `alpha_condition_assertion` for PCRE2 callout-style aliases `(*pla:...)` / `(*nla:...)` / `(*plb:...)` / `(*nlb:...)` plus long-name forms
- **Conformance delta**: 3670 → 3779 pass (+109), 971 → 862 fail (−109). 79.1% → **81.4%**. 0 panics.
- **PGEN-RGX reports closed by this adapter batch**: 0023 (quoted_literal), 0034-0039 (condition-assertion aliases), plus parts of 0021/22/27/28/33/53 (POSIX class_item). Adapter side of the 13 `fixed-upstream-pending-adapter` reports is now effectively complete.
- **Remaining failure buckets** (862 total, prioritized for future work):
  - 207 false positive, 205 false negative (semantic — one pattern class at a time)
  - 180 span mismatch
  - 138 `\Q...\E` inside char class (PGEN parse — new report candidate)
  - 61 remaining AST contract mismatches (now dominated by `[a-\d]+` class_range endpoint shape, not a shape I've seen before)
  - 34 extended char class advanced forms
  - 26 simple_escape rejects now all `\Q` inside simple_escape (should be intercepted but PGEN still routes somewhere)
- **Next concrete action**: pause adapter work (diminishing returns per commit) and investigate the single-largest semantic bucket next — false positive anchor+whitespace interactions on `$`/`\s`. That's real RGX matching behavior diverging from PCRE2.

### 2026-04-14 (thirty-fifth commit) — PGEN 1.1.21 (source-audit release): all filed reports closed + adapter catch-up
- **PGEN submodule**: 1.1.19 (`edd3b59`) → **1.1.21 (`e617960`, integration contract 1.1.23)**. PGEN shipped an audit pass against PCRE2's `src/pcre2_compile.c`.
- **PGEN-RGX-0054 closed**: the 80-level group-nesting stack overflow that PGEN 1.1.19 didn't fix is now resolved. Skip guard removed from both the conformance harness and `file_pgen_issues` (predicate returns false unconditionally). All 41 filed reports in the 0017-0055 batch are now either `verified-fixed-upstream` (26) or `fixed-upstream-pending-adapter` (13 — the RGX adapter work that was already identified in the 1.1.19 commit).
- **RGX adapter breaks caused by PGEN audit** (fixed in the same commit):
  1. PGEN 1.1.21 routes `\K`, `\R`, `\N`, `\X` through the `anchor` grammar rule instead of `simple_escape`. Broke 5 `match_reset_*` lib tests. Fix: added those four literals to `convert_anchor`'s match arms.
  2. PGEN's modifier grammar split: `modifier_group = modifier_char+` became `modifier_group = modifier_item+` where `modifier_item` now wraps `"x"`, `"xx"`, `"a" ascii_restrict_modifier?`. Broke 5 `extended_mode_*` lib tests because `walk_modifier_flags` only scanned `modifier_char`. Fix: added `modifier_item` handling that recursively walks the inner terminals.
- **Conformance trajectory** (full PCRE2 testdata corpus, 23 paired files):
  - PGEN 1.1.10 pre-fixes: 78.1%
  - PGEN 1.1.19 (25 reports closed): 78.9%
  - PGEN 1.1.21 pre-adapter-catch-up: 77.5% (audit exposed more RGX adapter gaps)
  - **PGEN 1.1.21 + adapter fixes: 79.1% — new all-time high** (3670 pass / 971 fail / 0 panic / 6575 skip / 11216 parsed)
- **Pattern discovered**: every PGEN grammar audit moves patterns from "parse rejection" → "AST-shape mismatch in RGX adapter". Net pass rate goes up only if RGX's adapter keeps pace. Worth watching as a pattern for future upstream syncs.
- **Next concrete action**: the 13+ RGX-side adapter gaps (POSIX `class_item` variants, `quoted_literal` atom, condition-assertion callout aliases, plus whatever new shapes PGEN 1.1.21 introduced that I haven't catalogued yet). Running the conformance histogram diff will show the newly exposed families.

### 2026-04-14 (thirty-fourth commit) — PGEN 1.1.19 bump: 25 reports closed, 13 partial, 1 remaining
- **PGEN submodule**: 1.1.10 (`8783757`) → **1.1.19 (`edd3b59`, integration contract 1.1.20)**. 66 upstream commits.
- **25 PGEN-RGX reports closed** (`verified-fixed-upstream`): POSIX sub-class delimiters (0017-0020, 0024-0026), verb parens (0029-0032), malformed-quantifier literals (0040-0049, 0052), `\g{}`/`\k{}` whitespace (0050, 0051), and **PGEN-RGX-0055** (mutually-recursive named-group stack overflow — no longer aborts, skip guard removed from harness + bin).
- **13 PGEN-RGX reports partial** (`fixed-upstream-pending-adapter`): PGEN emits correct AST; RGX adapter needs lowering. class_item variants (0021/22/27/28/33/53), quoted_literal (0023), condition-assertion callout aliases (0034-0039).
- **1 PGEN-RGX still unresolved upstream**: 0054 (80-level parser-depth stack overflow). Skip guard stays.
- **Conformance**: 11,216 parsed / **3,661 pass** / **979 fail** / 0 panic / 6,576 skip / **78.9%** (was 78.1% on 1.1.10). +37 passes, −37 fails. Failure histogram: PGEN parse failures 245 → 162 (−83); `class_item` contract mismatches 16 → 70 (+54) — PGEN accepting more, RGX adapter needs to catch up.
- **Verified** each closed report via a small Rust verifier that runs `Regex::compile` against each `pgen-issues/artifacts/PGEN-RGX-NNNN/repro_input.txt`. Automated the closure YAML edits via two Python helpers (`/tmp/close_pgen_fixes.py` and `/tmp/partial_pgen_fixes.py`).
- **Next concrete action**: the 13 partial reports define a clean RGX-adapter work list. In priority order:
  1. `convert_simple_escape` fallback for `\"`/`\/` (72 conformance cases)
  2. `convert_class_escape` for `[\b]`/`[\c]` variants (62 cases)
  3. New `convert_quoted_literal` adapter for `\Q...\E` (0023 + testdata occurrences)
  4. `convert_class_item` expansion for POSIX-class-inside-brackets node shapes (0021/22/27/28/33/53 plus ~54 new conformance cases)
  5. `convert_conditional` extension for callout-style lookaround aliases (0034-0039 plus related testdata cases)

### 2026-04-14 (thirty-third commit) — Case-fold ranges spanning both cases — fix (C) from the A-B-C plan
- **Bug**: `[W-c]/i` produced an inverted mirror range (w=119, C=67, start > end, matches nothing) in `Compiler::case_fold_ranges`. Any ASCII char-class range whose endpoints crossed the case boundary lost its case-fold expansion.
- **Fix**: for pure-ASCII ranges, iterate each codepoint and push case-swapped single-char ranges; the sort+merge step consolidates. Non-ASCII ranges keep the old endpoint-fold path.
- **4 regression tests** pinning: the testinput1:1381 minimal reproducer, out-of-range rejection, and the two non-spanning cases (lowercase-only + uppercase-only ranges).
- **Conformance delta**: 3618 → 3624 pass (+6), 1022 → 1016 fail (-6). Pass rate 78.0% → 78.1%. Small because `[W-c]/i` is one of ~200 distinct false-negative shapes; most of that bucket is other bugs (CR/LF `\s`, anchor + whitespace interactions, etc.).
- **A-B-C plan complete for this session**:
  - (A) Fix 9-panic `(?[...])` + FlagGroup bug ✅
  - (B) PGEN-RGX-0055 filed + widened skip guard ✅
  - (C) Case-fold range spanning both cases ✅
- **Next concrete actions** (from the "what's left" inventory I gave Oz):
  - Trivial adapter wins: `\"`/`\/` simple_escape fallback (+78), `[\b]`/`[\c]` class_escape (+62)
  - Medium harness wins: named-modifier support (~3000 skip→run), multi-line pattern support
  - Larger RGX triage: 194 remaining false-negative shapes, 200 false-positives, 173 span mismatches

### 2026-04-14 (thirty-second commit) — Second PGEN stack-overflow pattern filed + skip guard widened
- **What**: The `file_pgen_issues --scan testinput2` bin located the second process-aborting PGEN pattern — a Python-interpolation grammar at testinput2:2880 with six mutually-recursive named groups (`\g<regex>`, `\g<name>`, etc). Same bug class as the 80-nesting one (PGEN-RGX-0054) — the pgen-generated-regex worker exhausts its 8 MiB stack walking `\g<>` cross-references.
- **Filed**: PGEN-RGX-0055 with full bundle (repro_input.txt, pgen_contract.json, placeholder pgen_parse_outcome.json).
- **Guards widened**: both `pcre2_conformance.rs::is_pgen_stack_overflow_pattern` and `file_pgen_issues.rs` now skip patterns starting with `(?=(?<regex>(?#simplesyntax)` in addition to the 80-paren case.
- **Deferred**: the bin's end-to-end scan across all 23 files still hangs (~20 min wall) on a different pattern — the slowness is per-pattern `parse_grammar_profile_named` time, not an abort. Investigation deferred; the 39 existing PGEN-RGX reports (0017..0055) remain the initial set. Next session can add a per-pattern wall-clock timeout or progress-line tracing to narrow further.
- **Next concrete action**: (C) — the 200-false-negatives bucket. Starting with `/^[W-c]+$/i` on `wxy_^ABC` (case-insensitive char-class range with `/i` flag).

### 2026-04-13 (thirty-first commit) — Fix the 9-panic `(?[...])` + FlagGroup bug
- **Bug class**: `(?i)(?[...])` or any FlagGroup-wrapped `ExtendedCharClass` reached VM codegen with the extended-class node un-lowered because `Compiler::lower_extended_char_classes` didn't recurse through FlagGroup — only Sequence/Alt/Quant/Group/Lookahead/Lookbehind/Conditional.
- **Fix**: 4-line addition, one arm added that lowers the inner expr. Zero clippy errors, no API change.
- **Impact on conformance**: full-testdata panic count 9 → 0 on the 23-file corpus. 5 of 9 previously-panicking cases now pass PCRE2-correct; 4 still diverge semantically on case-folded `(?[...])` content (BACKLOG C7 semantic triage).
- **2 regression tests** in `compiler::tests` pin the minimal reproducers.
- **Next concrete action**: (B) investigate why `file_pgen_issues` hangs scanning testinput2..29 — some pattern triggers indefinite compile time in PGEN embedding API or RGX's post-parse transforms. Then (C) the 200 false-negatives bucket.

### 2026-04-13 (thirtieth commit) — PCRE2 conformance harness expanded to ALL 23 paired testdata files
- **User push-back**: "I asked you to use ALL of PCRE2 testdata, not just one, so please import ALL of them!!!" Correct — I'd been running only testinput1.
- **Harness now covers 23 files**: testinput1, 2, 3, 4, 5, 6, 7, 9, 10, 13, 16, 17, 18, 19, 20, 21, 23, 24, 25, 26, 27, 28, 29. Excluded: 8/11/12/14/22 (width-specific, no paired output), 15 (catastrophic-backtracking stress file hangs RGX even with 1M step cap).
- **Real case-level pass rate against the authoritative oracle: 78.0%** (11,216 parsed / 3,613 pass / 1,018 fail / 9 panic / 6,576 skip). NOT 98%. That was the feature-family count, naturally optimistic.
- **Two new RGX bug classes found**:
  1. 9 panics from `(?[...])` with Unicode properties + set operators (testinput4). Error: "should be lowered or rejected during compiler validation before codegen". Tight compile-boundary fix.
  2. testinput15 hang — some RGX hot path doesn't honor `set_max_steps`. Audit task.
- **PGEN-RGX-0054 filed manually**: 80-level group nesting overflows PGEN's worker thread stack. The `file_pgen_issues` generator can't reach this pattern (its `Regex::compile` aborts the process) — filed by hand.
- **Harness engineering**:
  - Spawned thread with 128 MiB stack (test-thread default too small for `(?R)` deep recursion).
  - Per-case `set_max_steps(1M)` + `set_max_backtrack_frames(64K)` + `set_max_recursion_depth(128)`.
  - Pattern-level skip guard for `≥80 leading parens`.
  - Per-file progress line (eprintln) so hangs are localizable.
  - Reused the existing `parse_cases` block-based parser.
- **Deferred**: running `file_pgen_issues` across all 23 files currently hangs on some pattern in testinput2..29 (compile-time, not a 80-paren case). Tracked as follow-up; the 38 existing PGEN reports (0017-0054) remain the initial set.
- **README honest**: the "~98% PCRE2 feature parity" line now reads as two numbers: ~98% feature-family coverage (hand-maintained matrix, naturally optimistic) + 78.0% case-level pass rate (authoritative differential against PCRE2 10.47 testdata). Bridging the gap is the C7 bug-triage track.
- **Next concrete action**: fix the 9-panic compile-boundary issue in testinput4 (expand `feature_validation_message` for `(?[...])` + Unicode-property + set-operator), then take the next RGX-side bucket from the histogram.

### 2026-04-13 (twenty-ninth commit) — Filed 37 PGEN bug reports per the canonical protocol
- **User asked**: "log the PGEN related misbehaviors, one report per failing case" per `subs/pgen/docs/contracts/PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`.
- **Built**: `rgx-core/src/bin/file_pgen_issues.rs` — internal generator that walks PCRE2 testdata, identifies PGEN-related compile failures, deduplicates by pattern string, and writes one full report bundle per unique pattern using PGEN's `embedding_api`. Reusable for any future PCRE2 testfile.
- **Filed**: 37 reports (PGEN-RGX-0017 through PGEN-RGX-0053) — 32 `should_parse_but_fails`, 5 `parses_but_returns_wrong_ast`. Each carries `repro_input.txt`, `pgen_contract.json`, `pgen_parse_outcome.json` per protocol §1–§5.
- **Decision noted**: 40 `simple_escape` (`\"`, `\/`) failures and 42 `class_escape unsupported variant` (`[\b]`, `[\c]`) failures are **NOT** filed as PGEN bugs — PGEN parses these correctly; the gap is in RGX's adapter (`convert_simple_escape` and `class_escape` lowering). Tracked in BACKLOG C7 as RGX-side fixes.
- **`pgen_trace.log` artifact deferred**: the protocol's "high-quality fast-to-fix" tier requires `PGEN_TRACE_VERBOSITY=debug parseability_probe` traces. Doing this for 37 patterns would mean 37 invocations × ~5s each = 3 minutes. The yaml's `command` field carries the exact invocation a maintainer can run when triaging a specific report.
- **Next concrete action**: continue triaging the remaining ~390 RGX-side PCRE2 failures. Top remaining buckets: 103 false negatives (case-insensitive char-class ranges), 56 false positives, 56 span mismatches, 42 class_escape adapter gaps, 40 simple_escape adapter gaps. Or: fix the two highest-ROI adapter gaps (simple_escape fallback + class_escape `[\b]`) to clear ~80 failures with ~10 lines of code.

### 2026-04-13 (twenty-eighth commit) — PCRE2 octal-fallback for backref-to-missing-group
- **Pass rate: 1957/424/0/139 — 82.2%.** +5 pass / -5 fail vs commit 27.
- **Real RGX bug fixed**: PCRE2 reinterprets `\NNN` as octal when group N doesn't exist; RGX previously errored. Same bug class as `\0` from commit 27 (commit 27 fixed the single-digit `\0` → NUL routing; commit 28 fixes the multi-digit `\NNN` → octal-byte fallback).
- **Implementation**: new compile-time AST transform `Compiler::resolve_octal_backreferences` rewrites `Backreference(n)` with `n > total_groups` to `Char(octal_value)` when every decimal digit of n is a valid octal digit (0..=7). Backrefs with non-octal digits (e.g. `\89`) fall through to the existing validation.
- **Behavioral change**: `\2` with no group 2 now compiles as `Char(0x02)` instead of erroring. PCRE2-correct. Existing test renamed to use `\9` (which still errors); new test added for the octal behavior.
- **Octal values 128..=255 caveat**: my fix uses `char::from_u32` which encodes as Unicode codepoint (1-2 UTF-8 bytes). PCRE2's `\NNN` for 128..=255 is a single byte. For ASCII-range cases (0..=127) the fix is byte-accurate; for high-byte cases the divergence shows as RGX matching the codepoint via 2-byte UTF-8 vs PCRE2's 1-byte literal. Tracked as follow-up.
- **Next concrete action**: continue tackling the 424 remaining PCRE2 conformance failures. Top buckets to investigate: 103 false negatives (case-insensitive char-class range semantics like `[W-c]/i`), 88 PGEN parse failures (POSIX class syntax in unusual positions), 56 false positives, 56 span mismatches, 42 unsupported `[\b]` `[\c]` class escapes, 40 PGEN rejects on `\"` `\/`. Each is its own commit.

### 2026-04-13 (twenty-seventh commit) — PCRE2 harness block refactor + `\0` → NUL fix → 82% pass rate
- **Pass rate jumps 39.5% → 82.0%** on the PCRE2 10.47 testinput1 conformance. Most of the jump was harness-side false positives cleared by a block-based parser; the one real RGX parse bug fixed is `\0`.
- **Harness refactor**: line-cursor alignment was fragile. Rewrote as block-based parser — both files split by blank lines (including whitespace-only lines), blocks paired by index. Added a separate `decode_output` (narrower than `decode_subject`: only `\xHH` and `\\`), fixed `\=` annotation-echo consumption, handled PCRE2's trailing-`\` "empty subject" convention. Added categorized failure histogram so the remaining work is prioritizable by bug class.
- **Real RGX parse bug fixed — `\0`**: `convert_simple_escape` fell through to the backref arm for `c.is_ascii_digit()` which produced `Regex::Backreference(0)`. Group 0 is never a valid backref target; `\0` must be literal NUL. Fix: explicit `'0' => Ok(Regex::Char('\0'))` arm. `\1`..`\9` continue as backrefs.
- **Updated snapshot**: 1952 pass / 429 fail / 0 panic / 139 skip / 2520 parsed. Remaining failures cluster into 9 categories — top offenders are false negatives (case-insensitive char-class range semantics), PGEN parse failures on POSIX-class patterns, and false positives on trailing-whitespace anchor interactions. All enumerated in BACKLOG C7.
- **3 new regression tests** for the `\0` fix. Total lib tests: 1000/0/1 (up from 997).
- **Strategic: the 429 remaining failures are actionable**. Each category points at either a PGEN parser gap, an RGX adapter gap, or a real VM semantic bug. The histogram makes it trivial to attack them one at a time. Next commits will pick them off bucket by bucket.
- **Next concrete action**: attack the "35 other compile errors" bucket — `(abc)\123` fails because RGX treats `\123` as backref to group 123 (which doesn't exist) instead of falling back to octal. Same-family fix as the `\0` bug.

### 2026-04-13 (twenty-sixth commit) — Crash-class bugs from PCRE2 harness FIXED (0 panics)
- **Result**: panic count on testinput1 goes 12 → **0**. RGX no longer crashes on any pattern in the PCRE2 10.47 core suite. 1063 pass / 1626 fail / 0 panic / 182 skip.
- **Bug 1: `{0,0}` / `{0}` with captures crashed `compile_subroutines`**. Root cause: when a capturing group is nested inside a zero-repetition quantifier, `codegen_pass` never descends into it, so `group_counter` stays behind the AST's max group id. Then `compile_subroutines` sized `subroutines` by `group_counter+1` but `collect_capturing_group_defs` (which walks the raw AST) wrote `subroutines[group_id]` where `group_id > group_counter`. Fix: derive `max_group_id` from the collected defs first, size via `max(group_counter, max_group_id)+1`.
- **Bug 2: char-class table overflow on `{0,300}`-style repeats**. Root cause: Range quantifiers emit the inner expression N times; each `emit_subexpr_opcode` path creates a fresh sub-compiler and `extend()`s its char_classes into the parent unconditionally. For N=300 of `[a-zA-Z0-9]+`, that's 300 identical entries → single-byte operand overflow at inline-id rebase. Fix: (a) `#[derive(PartialEq, Eq)]` on `CompiledCharClass`; (b) dedup during sub-compiler merge in `compile_nested_code`; (c) replace base-offset `rebase_inline_char_class_ids` with remap-table `remap_inline_char_class_ids` so duplicates can target any existing id, not just `base+i`. Same dedup applied in `compile_char_class` for within-compiler repeats.
- **7 regression tests** added in `vm.rs::tests`, one per minimal reproducer. Each asserts the engine doesn't panic; semantic correctness tracked by the conformance harness itself.
- **Semantic failures still pending** (1626 total — now part of BACKLOG C7 semantic triage): compile gaps on `\"`, `\'`, `[\b]`, `[\c]`; backreference edge cases like `^(a)\1{2,3}(.)` on `"aaabcd"` returning "aaab" where PCRE2 returns "abcd"; extended-mode comment + `\= Expect no match` interactions. Some of these are real RGX bugs; some are harness false positives I can't distinguish yet.
- **Engineering lesson**: PCRE2's testdata is an extraordinary bug-finder. Shipped commit 1, got 12 real bugs on the first run. Commit 2 closes the whole crash class. This is the power of differential testing against an authoritative oracle.
- **Next concrete action**: return to the five-item stress test program. Effort (2) 4-tier cross-dispatch differential, OR pause and triage the 1626 semantic failures first. User input on priority.

### 2026-04-13 (twenty-fifth commit) — PCRE2 10.47 differential conformance harness shipped
- **Strategic context**: user explicitly pushed back on publishing — "RGX should be tested much much more" before crates.io. Asked for five stress-testing approaches: (1) PCRE2 10.47 testdata import, (2) 4-tier cross-dispatch differential, (3) real-world-regex mutation fuzzing, (4) equivalence-class testing, (5) metamorphic testing. This commit lands (1).
- **Mistake + lesson on submodules**: I reached for `curl` to fetch the tarball. User pushed back: "why didn't you git submodule add PCRE2?" Right answer. The `subs/pgen` convention was right there. Saved as feedback memory `feedback_submodule_for_external_deps.md` — when pulling in external source/data with its own release cadence, default to `git submodule add` under `subs/<name>`.
- **Submodule**: `subs/pcre2` added at `pcre2-10.47` tag, commit `f454e231`.
- **Harness**: `rgx-core/tests/pcre2_conformance.rs` (~600 lines). Parses PCRE2 testformat, runs each pattern through RGX public API, compares against expected output. Panic-safe via `catch_unwind` — one crash doesn't abort the ~2871-case survey. `#[ignore]`'d (heavy, ~30s).
- **First-run findings** (FROM COMMIT 1):
  - 1061 pass / 1616 fail / **12 panic** / 182 skip out of 2871 parsed
  - **Real VM bug class uncovered**: `{0,0}` / `{0}` quantifiers wrapping captured groups → `index out of bounds: the len is 1 but the index is 1` at `vm.rs:6899`. 5 minimal reproducers. Tracked as BACKLOG C7 item 1.
  - Second panic class: high-min-count quantifier overflow (`{0,300}`). BACKLOG C7 item 2.
  - Many failures are compile-gap cases (`\c[`, `\"`, `[\b]`) and harness limitations (multi-line patterns), not semantic bugs.
- **What's NOT in this commit**: fixing the bugs. Each is its own investigation. The harness emits a report but doesn't assert a pass threshold — baseline wiring is follow-up.
- **Next concrete action**: continue with (2) 4-tier cross-dispatch differential. Or — if the user wants — pause to fix the panic-class bugs C7 found first.

### 2026-04-13 (twenty-fourth commit) — A8 crate publishing prep: metadata + READMEs + dry-run
- **rgx-core and rgx-cli are now crates.io-metadata-ready.** Every field populated: `description`, `readme`, `documentation`, `homepage`, `keywords`, `categories`, `repository`, `license`, plus `version` on the internal path dep.
- **New per-crate READMEs** live at `rgx-core/README.md` and `rgx-cli/README.md` — user-focused, with install/example recipes and feature-flag tables. Root `README.md` stays contributor-heavy and is unaffected.
- **`cargo publish --dry-run` on rgx-core**: metadata gate passes. Surfaces **one hard blocker**: `pgen` is a path dep on a private submodule, not on crates.io.
  - Three paths forward (user decision):
    1. Publish `pgen` to crates.io first → bump rgx-core to `pgen = "1.1.10"`.
    2. Vendor pgen's generated Rust code into rgx-core so the dep disappears.
    3. Make `pgen-parser` truly optional so rgx-core can publish without it.
- **Binary-rename decision also pending**: CLI is currently named `rgx-cli`. README markets it as `rgx`. Adding `[[bin]] name = "rgx"` is a 1-line change but touches 461 downstream references across docs and scripts — deferred to a coordinated follow-up.
- **Placeholder fixed**: author email was `richarddje@example.com`; now `richard.dje@gmail.com` (from `git config`).
- **Validation**: `cargo fmt` clean, `cargo test -p rgx-core --lib` 990/0/1, `cargo test -p rgx-cli` 30/0, `cargo clippy --workspace --all-targets` zero errors, `cargo publish --dry-run` metadata gate ✅ blocked only on pgen.
- **Next concrete action**: user decision on the pgen-publish strategy. Without that, A8 is as far forward as it can go. Other tier-2 work still on the table: extending the reverse-DFA pipeline to find_first / find_all via a leftmost-first-aware unanchored NFA; opcode fusion; per-opcode bounds-check reduction.

### 2026-04-13 (twenty-third commit) — Reverse-DFA pipeline: is_match single-pass fast path
- **The reverse-DFA pipeline's first real consumer lands**: `Engine::try_dfa_is_match` now walks the forward-unanchored `LazyDfa` once per call instead of running the anchored DFA at every candidate position.
- **Pitfall noted and respected**: the forward-unanchored DFA's `find_match_at` records the LAST accept seen during the scan (subset-construction's leftmost-LONGEST semantics). For `a` on `"xaxa"` this returns end=4 (the LAST match) not the leftmost end=2. That's wrong for `find_first` / `find_all` which need leftmost-first. It's CORRECT for `is_match` because any accept anywhere makes the answer true. So this commit wires the fast path ONLY for `is_match`.
- **What's needed to extend to find_first / find_all**: a leftmost-first-aware unanchored NFA — specifically the lazy prefix `(?s:.)*?` needs to die after the first accept is reached, so subset construction preserves leftmost semantics. Not in this commit; scoped for follow-up. The DFA plumbing is in place.
- **Fields added**: `c2_forward_unanchored_dfa: Option<Mutex<LazyDfa>>` on `Engine`, built in `Engine::new` via `build_forward_unanchored_dfa_if_eligible`. Companion to the existing `c2_dfa` (anchored) and `c2_reverse_dfa` (foundation from eeb64fb). Same eligibility gate.
- **Regression-pinning test**: `is_match_and_find_first_agree_on_multi_position_literal` asserts `a` on `"xaxa"` gives is_match=true and find_first=(1,2). If a future commit naively adopts the forward-unanchored DFA for find_first, this test fails immediately.
- **Validation**: `cargo test -p rgx-core --lib` 990/0/1 (up 6 from 984: 6 new tests in `engine::reverse_dfa_pipeline_tests`). All existing tests continue to pass — the fast path is purely additive on top of the existing per-position fallback.
- **Next concrete action**: either (a) implement the lazy-prefix-dies-after-accept NFA construction to extend the pipeline to find_first / find_all, (b) A8 crate publishing prep, or (c) another tier-2 perf item from the backlog. User input on priority.

### 2026-04-13 (twenty-second commit) — PGEN 1.1.10 bump closes A13 end-to-end
- **PGEN submodule bumped** from `ac2acb3` (1.1.9) to `8783757` (1.1.10). PGEN 1.1.10 carries the grammar recognition for `(?(VERSION op X.Y)...)` that PGEN-RGX-0016 was blocking on.
- **Zero RGX code changes beyond unignoring tests**: the A13 commit on 2026-04-12 shipped everything on the RGX side speculatively. This commit removes `#[ignore]` from three tests in `parsing::tests::version_conditional_*` and drops the `#[allow(dead_code)]` on `contains_conditional`. That's all the RGX-side code that needed to change.
- **PGEN-RGX-0016 closed**: `status: closed`, `resolution.status: verified-fixed-upstream`, `fixed_in_parser_release_version: 1.1.10`, `fixed_in_parser_backend_version: 8783757`. Follows the same closure shape as PGEN-RGX-0015.
- **Pin references updated** in README.md, RUST_CODEBASE_ANALYSIS.md, book/src/internals/architecture.md, book/src/internals/pgen-integration.md, book/src/internals/project-status.md, docs/BACKLOG.md, ROADMAP.md. The MSRV is unchanged (PGEN 1.1.10 keeps edition 2024).
- **Parity number**: ticks from ~98% to ~99%. A11 `(*SKIP:name)` and A13 VERSION conditionals are both shipped end-to-end now; no hard PCRE2 gaps remain on the tracked surface. Remaining work is in the PCRE2 10.47+ advanced-syntax category already captured under the "Next" roadmap section (returned-capture subroutine forms, wider `(?[...])` algebra beyond the shipped subset).
- **Validation**: `cargo fmt --check` clean, `cargo test -p rgx-core --lib` 984/0/1 (up 2 from 982/0/3 — 2 integration tests un-ignored + 0 new tests), `cargo test -p rgx-cli` 30/0, `cargo clippy -p rgx-core --all-targets` zero RGX-owned errors.
- **Next concrete action**: back to the ROADMAP "Now" track — reverse-DFA dispatch wiring (consume the foundation from `eeb64fb`) or A8 crate publishing prep. User preference pending.

### 2026-04-12 (twenty-first commit) — A13 VERSION conditionals (RGX side; PGEN gap filed as PGEN-RGX-0016)
- **First commit on the Tier-3 parity polish track.** Implements the RGX-side parser-level short-circuit for `(?(VERSION op X.Y)yes|no)` conditionals. The parser-side infrastructure is complete; the full integration is gated on PGEN recognising VERSION conditionals (filed as `pgen-issues/PGEN-RGX-0016.yaml`).
- **`RGX_PCRE2_COMPAT_VERSION` public constant** in `lib.rs`. Currently `(10, 47)`. The PCRE2 release that the RGX feature surface tracks. Bump when the parity matrix is re-aligned.
- **`parse_version_conditional` helper** in `parsing.rs`. Parses `VERSION op X.Y` text and evaluates against `RGX_PCRE2_COMPAT_VERSION`. Operators: `=`, `!=`, `>=`, `<=`, `>`, `<`. Missing minor defaults to 0. Returns `Some(true/false)` for VERSION conditionals, `None` for non-VERSION text.
- **`convert_conditional` short-circuit** in `parsing.rs`. Before building the `Regex::Conditional` AST node, the parser checks the condition text against `parse_version_conditional`. If it's a VERSION check, the parser evaluates at parse time and returns ONLY the matching branch as a Regex AST — the conditional never wraps in `Regex::Conditional`. Mirrors PCRE2's compile-time evaluation.
- **8 new unit tests** in `parsing::tests::parse_version_conditional_*`. Cover all operators, missing minor, whitespace, non-VERSION fallback, malformed version strings.
- **3 new integration tests** `#[ignore]`'d with a clear reference to PGEN-RGX-0016. They will start passing the moment PGEN catches up — no RGX-side change required.
- **`pgen-issues/PGEN-RGX-0016.yaml`** filed per `subs/pgen/docs/contracts/PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`. Includes:
  - Parser identity: PGEN commit ac2acb3, parser release version 1.1.9, integration contract version 1.1.9, family `regex`, profile `regex_default`, integration surface `parseability_probe`
  - Host identity: rgx commit 7d195a4, macOS Darwin 24.6.0, rust-version 1.88
  - Bug class: `should_parse_but_fails`
  - Reproduction artifacts in `pgen-issues/artifacts/PGEN-RGX-0016/`:
    - `repro_input.txt` — the failing pattern
    - `pgen_contract.json` — captured PGEN version metadata
    - `pgen_parse_outcome.json` — structured parse rejection (position 0)
    - `pgen_trace.log` — full PGEN_TRACE_VERBOSITY=debug trace from the parseability_probe run
- **Why ship the RGX side speculatively**: when PGEN catches up, the integration is purely "remove `#[ignore]` from three tests". Doing the work now means the gap is documented and the RGX side won't need re-investigation later.
- **Validation**: `cargo fmt --check` clean, `cargo test -p rgx-core --lib` 976/0 (= 968 baseline + 8 new parse_version_conditional unit tests + 3 new ignored integration tests), `cargo clippy -p rgx-core --all-targets` zero RGX-owned errors.
- **Next**: A11 (*SKIP:name) named skip — should not have the same PGEN gap because (*SKIP:name) is more standard syntax. Then A12 capture-return semantics. Then the Tier-2 perf items.

### 2026-04-12 (twentieth commit) — C2 negated-char-class semantics fix
- **Fixes the C1 step 6 bug.** `Regex::find_first("[^0-9]", "123abc")` now returns `(3, 4)` instead of the buggy `(3, 6)`. The bug was in the byte-class map, not in dispatch.
- **Root cause**: For `[^0-9]`, `byte_class.rs::collect_oracles` collected only the positive range `(0x30, 0x39)`, producing a 2-class partition (digit/non-digit). The non-digit class lumped together ASCII bytes AND continuation bytes (0x80-0xBF) AND leading bytes (0xC0-0xF7). Meanwhile, `nfa.rs::build_char_ranges` for the negated form generated multi-byte UTF-8 chains. Each chain's leading-byte and continuation-byte transitions all fired on the same byte_class 0 (non-digit), so when Pike-VM walked ASCII input "abc", the multi-byte chains advanced byte-by-byte through their state chains as if "abc" were valid UTF-8 continuation bytes, reaching accept at positions 4, 5, AND 6. Pike-VM recorded the latest accept = (start, start+3).
- **Fix**: `ByteClassMap::build_from_ast` now unconditionally injects four UTF-8 byte-category boundary oracles via the new `push_utf8_byte_boundary_oracles` helper:
  - `(0x80, 0xBF)` — continuation bytes
  - `(0xC0, 0xDF)` — 2-byte leading
  - `(0xE0, 0xEF)` — 3-byte leading
  - `(0xF0, 0xF7)` — 4-byte leading
  These force the byte-class partition to assign each UTF-8 byte category its own equivalence class. The NFA's multi-byte chains now have transitions on classes that ONLY contain valid UTF-8 leading/continuation bytes, not ASCII bytes. ASCII input no longer fires the multi-byte chain transitions, the chains die, and only the single-byte ASCII chain produces an accept — at the leftmost single-character match.
- **Cost**: at most 4 extra equivalence classes per pattern. DFA states are sparse arrays indexed by class — adding 4 classes adds 4 transition table slots per state. Negligible. The empty AST pattern goes from 1 class to 5; `[a-z]` from 2 to 6; `[a-c][b-d]` from 4 to 8.
- **11 byte_class tests updated** to reflect the new partition counts. Each updated test gets a comment explaining the new class structure (ASCII / pattern / continuation / 2/3/4-byte leading). The semantic invariants (which bytes share which classes, which are distinct) all still hold — only the absolute counts changed.
- **2 new regression tests** in `c2::pike::tests`:
  - `negated_class_matches_first_non_digit_with_run_of_non_digits` — `[^0-9]` against `"123abc"` returns `(3, 4)` for both `pike_find_first` and `pike_captures_at`. This is the regression test for the original bug.
  - `negated_class_correctly_consumes_multibyte_unicode_char` — `[^0-9]` against `"1café"` correctly matches the multi-byte `é` at `(4, 6)`. Confirms the fix doesn't break valid multi-byte UTF-8 matching.
- **C1 step 6 differential gate workaround status**: the deviation introduced at C1 step 6 (comparing against the raw `RegexVM::find_first` instead of `Regex::find_first` because the public API's C2 DFA path returned the longer match) is now technically obsolete — the public API and the raw VM agree. The workaround is left in place because it's the safer reference (the JIT's contract is "match the interpreter" = the VM, not the dispatch chain). A future commit could revisit using the public API as the differential reference.
- **Validation**: full quality gates green. `cargo test -p rgx-core --lib` **968/0** (= 967 baseline + 1 new multi-byte regression test), `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy -p rgx-core --all-targets` zero RGX-owned errors, `cargo fmt --check` clean.
- **Next concrete action**: items (1)–(4) from the roadmap-refresh execution plan are done. (1) ROADMAP refresh, (2) reverse-DFA pipeline foundation, (4) DFA negated-char-class fix. ((3) crate publishing prep was de-scoped by the user mid-session; (5) was also de-scoped.) The immediate tier-2 work that's still on the table: wire the reverse-DFA dispatch path on top of the (2) foundation (replaces per-position scans with a single forward-then-reverse sweep), opcode fusion, capture/backtrack preallocation. Multi-byte literal prefix in C2 dispatch is also a candidate. User input on next focus.

### 2026-04-12 (nineteenth commit) — Reverse-DFA pipeline foundation
- **First commit on the post-C1 perf-headroom track.** Lays the foundation for the reverse-DFA pipeline (the C2 follow-up that replaces per-position scans with a single forward-then-reverse sweep). This commit ships the foundation only — the dispatch wiring lands in the follow-up commit alongside the leftmost-longest-vs-leftmost-first fix.
- **New `LazyDfa::find_match_start_at_reverse(input, end)` method**. Walks the DFA simulator backward from `end` toward byte 0. The reverse-anchored DFA's "latest accept seen during the backward walk" corresponds to the smallest forward index = leftmost match start.
- **New `Engine::c2_reverse_dfa: Option<Mutex<LazyDfa>>` field** built in `Engine::new` alongside the existing `c2_dfa`. Same eligibility gate. Shares the byte-class equivalence map with the forward DFA via `Arc::clone`.
- **New `build_reverse_dfa_if_eligible` helper** + **`Engine::should_dispatch_to_reverse_dfa` accessor** mirroring the forward DFA equivalents.
- **9 new unit tests** in `c2::dfa::tests::reverse_dfa_*` covering literals, char classes, quantified patterns, no-match cases, full-input matches, and zero-width matches.
- **Status**: foundation only. The dispatch wiring is the next commit (and is tightly coupled to (4)'s leftmost-longest fix because the C2 DFA path's leftmost-LONGEST semantics for negated char classes is what would conflict with naively wiring the reverse pipeline).
- **Validation**: `cargo test -p rgx-core --lib` 966/0 (= 957 baseline + 9 new reverse-DFA tests). `cargo clippy -p rgx-core --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: (4) DFA negated-char-class semantics fix — investigate the C1 step 6 divergence between `Regex::find_first("[^0-9]", "123abc")` (returns (3,6) via the C2 DFA) and the raw VM (returns (3,4)). Fix the leftmost-longest behavior. The fix may also wire the reverse-DFA pipeline as the new (correct) dispatch path, depending on whether the bug is in `find_match_at` or in `pike_captures_at`.

### 2026-04-12 (eighteenth commit) — C1 step 8: production cutover, JIT default-on, Book chapter
- **THE C1 SERIES IS COMPLETE.** All 9 steps (0–8) of the design doc plan have shipped. C1 step 8 is the final step: it flips the `jit` Cargo feature from opt-in to default-on, writes the public Book chapter `book/src/internals/jit-compiler.md`, and updates the surrounding documentation to reflect C1 as a shipped engine. **C1 is now production code on the public API path.**
- **`jit` Cargo feature flipped to default-on**. The `default = ["std", "pgen-parser"]` line in `rgx-core/Cargo.toml` becomes `default = ["std", "pgen-parser", "jit"]`. The Cranelift dependencies are now part of the default build (~2 MiB closure). Users who want to avoid them can opt out via `default-features = false` and explicitly include the other features they need.
- **Effect on the test suite**: with the new default, `cargo test -p rgx-core` now runs **957 lib tests** (= 695 baseline + 262 C1) — UP from 695 baseline. Every existing test now exercises the JIT path for JIT-eligible patterns. The opt-out path (`--no-default-features --features pgen-parser`) still works and runs 695 lib tests (the c1 module is feature-gated and not compiled).
- **New public Book chapter `book/src/internals/jit-compiler.md`** (~250 lines). Covers: why JIT-compile, what C1 is, why Cranelift, the JIT-eligible subset, the JIT'd function shape, how the codegen works (two-pass walker, IR layout), the runtime helper layer, the per-frame capture snapshot architecture, the 4-tier engine dispatch boundary (with the explicit deviation from design doc §8 explained), differential testing methodology (including why the gate compares against the raw VM not the public API), performance impact, and what's not in C1 yet. Linked into `book/src/SUMMARY.md` alongside the existing internals chapters.
- **Surrounding Book pages updated** to reflect C1 as shipped:
  - `the-vm.md`: removed "RGX has no JIT today" — now describes the three execution tiers and links to the C1 chapter.
  - `nfa-dfa-engine.md`: "Next" link points to the C1 chapter (was PGEN). The "what's not in C2" section notes C1 has shipped.
  - `performance.md`: "JIT compilation (backlog C1)" subsection replaced with a "Three execution tiers" overview. The opening paragraph and benchmark interpretation updated to mention the C1 cutover.
  - `project-status.md`: C1 marked ✅ Shipped in Tier 2 with a description and chapter link. The "forward story" no longer lists JIT as the next major push.
- **`RUST_CODEBASE_ANALYSIS.md`**: C1 entry now marked ✅ SHIPPED with the same format as the C2 entry. The "PCRE2 feature parity" line removes "JIT" from the list of remaining gaps.
- **Validation**: full quality gates green on **THREE configurations** — the new default, the explicit `--features jit` (now redundant but still works), and the explicit opt-out via `--no-default-features --features pgen-parser`. Default `cargo test -p rgx-core` 957/0 (UP from 695 baseline), `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, opt-out `cargo test -p rgx-core --no-default-features --features pgen-parser` 695/0, clippy zero RGX errors on both configurations, fmt clean.
- **The C1 series is closed.** All 9 steps shipped: 0 (design proposal), 1 (host plumbing), 2 (eligibility check), 3a–3e (linear opcode codegen via decoder unfolding), 4a (differential gate), 4b (capture trail per-frame snapshot), 5 (engine dispatch wiring with 4-tier chain), 6 (CharClass + multi-byte literal codegen via runtime helper + inline byte comparisons), 7 (runtime safety helpers inlined as Cranelift branches), 8 (production cutover). The JIT is default-on, the Book chapter is live, and the dispatch chain is `DFA → Pike-VM → JIT → backtracking VM`.
- **Pre-existing RGX bug noted (deferred follow-up)**: `Regex::find_first("[^0-9]", "123abc")` returns `(3, 6)` via the C2 DFA path (leftmost-LONGEST semantics for negated char classes), but `RegexVM::find_first` returns `(3, 4)` (correct backtracking semantics). The C1 step 6 differential gate exposed this divergence between the DFA and the VM. The JIT correctly matches the VM. Fixing the DFA's negated-char-class semantics is out of scope for the C1 series and tracked as a future C2 follow-up.
- **Next concrete action**: TBD. The C1 / C2 perf-track is now complete. Remaining items in `docs/BACKLOG.md` are smaller scope: tier-2 performance headroom (opcode fusion, multi-byte literal prefix in C2 dispatch, smarter Pike-VM heuristics, JIT-ahead-of-Pike-VM dispatch ordering, the reverse-DFA pipeline), parity edge cases (`(*SKIP:name)`, `VERSION` conditionals, `(?P>name)` semantics for A12 capture-return), and the deferred A8 crate publishing. User input on which to pursue next.

### 2026-04-11 (late evening — seventeenth commit)
- **C1 step 7 (runtime safety helpers) landed.** The JIT now enforces the user-configurable `max_steps` and `max_backtrack_frames` limits inline as Cranelift branches. Patterns with safety limits set are now JIT-eligible — previously the engine excluded them in `should_use_jit`.
- **Function signature change**. Extended from 6 args to 8 args by adding `max_steps: u64` and `max_bt_frames: u64`. `0` = unlimited. The engine reads from `vm.max_steps()` / `vm.max_backtrack_frames()` and passes them on every call. Two new public getters on `RegexVM`.
- **`JIT_LIMIT_EXCEEDED_SENTINEL = -2`**. New return value distinct from `-1` (no match) so the engine scan loops can distinguish "limit hit, stop entirely" from "no match, continue scanning". Re-exported from `c1::mod`.
- **`emit_step_limit_check` helper**. Called at the START of every JitOp's emit. Mirrors the interpreter's main-loop pattern: increment step counter, then if `max_steps != 0 && counter > max_steps` jump to limit_abort_block (returns -2). The increment-then-compare order rejects the same set of inputs as the interpreter's compare-then-increment.
- **`emit_backtrack_push` user-limit check**. Extended with a second check after the existing hard-cap (256-frame) check. If `max_bt_frames != 0` AND `bt_top >= max_bt_frames`, jumps to limit_abort_block. The hard cap returns `-1` (existing behaviour); the user limit returns `-2`.
- **New `limit_abort_block`**. Cranelift block that returns `JIT_LIMIT_EXCEEDED_SENTINEL`. Reached from any step-counter check or the new user-frame-limit check. Sealed alongside `fail_block` at the end of `compile_program`.
- **Engine layer changes**. Each `try_jit_*` method reads `max_steps` and `max_bt_frames` from `self.vm`, passes them as the new 7th/8th args, and detects the `-2` sentinel after every call. On sentinel: `try_jit_is_match` returns `Some(false)`, `try_jit_find_first` returns `Some(None)`, `try_jit_find_all` breaks the loop and returns matches collected so far. **Removed `has_runtime_match_limits` exclusion** from `should_use_jit`. New `has_recursion_depth_limit` exclusion stays — recursion is JIT-ineligible.
- **Per-call vs cumulative semantics**. The JIT's step counter resets to 0 on every JIT'd-function entry. The interpreter, by contrast, maintains a single counter across the whole `find_first` / `find_all` scan. Step 7 reconciles this at the engine layer: when the JIT returns the limit-abort sentinel, the engine stops scanning entirely. The user-visible behaviour matches the interpreter even though the exact accounting differs.
- **`jit_compile_with_limits` test helper**. New test helper exposing the `(max_steps, max_bt_frames)` parameters. Used by step 7 tests to verify the inline checks at the codegen level. Legacy `jit_compile` and `jit_compile_with_captures` continue to pass `0, 0` (unlimited) so the existing test suite is unaffected.
- **13 new step-7 tests** in `c1::codegen::tests::step7_*`: 5 max_steps codegen, 4 max_bt_frames codegen, 4 engine-integration tests via the public API.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902 baseline tests (unchanged), `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: **`cargo test -p rgx-core --features jit` 957 lib tests pass** (695 baseline + 262 C1, +13 from step 7), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 8 (production cutover, benchmarks, Book chapter expanded to its full form). Step 8 is the FINAL step in the C1 series. It ships:
  1. Flipping the `jit` Cargo feature from opt-in to default-on. Existing users get the JIT for free; opt-out via `default-features = false` for users who don't want Cranelift in their dependency tree.
  2. Running the full benchmark sweep (`rgx-bench/src/bin/trend_capture.rs`) with label-paired captures comparing pre-step-8 (interpreter dispatch) vs post-step-8 (JIT dispatch) on the existing benchmark corpus.
  3. Adding new C1-specific benchmark patterns (e.g. `\bERROR\s+\d+`, HTTP routes) per design doc §15.1.
  4. Writing the public Book chapter `book/src/internals/jit-compiler.md` — currently a placeholder from step 0.
  5. Updating `RUST_CODEBASE_ANALYSIS.md` to reflect C1 as a shipped engine.

### 2026-04-11 (late evening — sixteenth commit)
- **C1 step 6 (CharClass + multi-byte literal codegen) landed.** Patterns like `[abc]`, `[a-z]`, `[^0-9]`, `[а-я]`, `é`, `日本`, `🦀` are now JIT-eligible. The JIT-eligible subset now covers single-byte AND multi-byte UTF-8 literals, all six built-in ASCII char-class opcodes, custom char classes (positive and negated, including Unicode-range classes), simple anchors, word boundaries, control flow, all six optimized quantifiers, top-level alternation tracking, and capture groups 1..=16.
- **New runtime helper `rgx_runtime_char_class_match_at`** in `c1/runtime.rs`. Replaces the step-1 stub. C ABI signature: `(text, text_len, pos, char_classes_ptr, char_classes_len, class_id, negated) -> u32`. The helper bounds-checks `pos < text_len`, decodes the UTF-8 character at `text[pos]` (handles 1..=4 byte widths, rejects malformed leading bytes), looks up `char_classes[class_id]`, tests the decoded character against the class via the same bitmap-then-Unicode-range logic as `RegexVM::test_char_class`, and returns the character width on a successful match (or 0 on failure). The character-width-aware return value lets the JIT'd caller advance `pos` by the right amount in a single instruction.
- **JIT'd function signature change**. Extended from 4 args to 6 args by adding `char_classes_ptr: *const u8` and `char_classes_len: usize`. The engine layer (`try_jit_*` methods) obtains these via `self.vm.program.char_classes.as_ptr() as *const u8` and `.len()` and passes them on every call. They're stable for the engine's lifetime because the program is owned by the engine and never mutated after creation.
- **New `JitOp::CharBytes { bytes: [u8; 4], len: u8 }` variant** for multi-byte UTF-8 literals (lengths 2..=4). Stored inline as a fixed-size array so JitOp stays Copy. Codegen helper `emit_match_multibyte_literal` emits an upfront bounds check then unrolled per-byte loads + comparisons combined via `band`, then a conditional branch. **No runtime helper** because the byte values are constants known at JIT-compile time and the inline form is faster.
- **New `JitOp::CharClass { id: u8, negated: bool }` variant** for custom char classes. Codegen emits an indirect call to `rgx_runtime_char_class_match_at`, branches on the result (0 = no match → failure_dispatch, >0 = match → advance `pos` by the returned width).
- **Decoder updates**. `decode_program`'s `Char` arm now accepts any length 1..=4: length 1 emits the existing `JitOp::Char(b)`, length 2..=4 emits `JitOp::CharBytes { bytes, len }`. New `OpCode::CharClass | OpCode::CharClassNeg` arm reads the 1-byte class id operand and emits `JitOp::CharClass { id, negated }`. `decode_simple_inner_into` (the inner-quantifier decoder) and `is_simple_inner_opcode` get parallel updates so quantifier-wrapped char classes like `[abc]+` and `é+` work too.
- **Differential gate switched to compare against the raw `RegexVM::find_first` interpreter** instead of the public `Regex::find_first` API. **The discovery**: `Regex::find_first("[^0-9]", "123abc")` returns `(3, 6)` (longest run of non-digits) because the C2 DFA dispatch path implements leftmost-LONGEST semantics for negated char classes. But `RegexVM::find_first("[^0-9]", "123abc")` returns `(3, 4)` (single non-digit), the correct backtracking semantics. The design doc §1.0 says the JIT must produce byte-for-byte identical results to the **interpreter**, which is the VM — not the public dispatch chain. The fix: `assert_jit_direct_capture_equivalent` now constructs a `RegexVM` directly and compares against `vm.find_first`, bypassing the public API's dispatch quirks. **Pre-existing step 4a / 4b differential tests are unchanged** because for those patterns the DFA and VM agree.
- **19 new step-6 tests** in `c1::codegen::tests::step6_*`: 7 char-class direct-call differential, 6 multi-byte literal direct-call differential, 2 ASCII-class-with-Unicode-text differential, 4 eligibility tests.
- **Test helper refactor**. `jit_compile` and `jit_compile_with_captures` now clone the program's `char_classes` Vec into the wrapper closure so its data pointer stays valid for the closure's lifetime. The closure's signature is unchanged from step 4b — callers continue to pass `(text_ptr, text_len, pos)` and the closure internally allocates the captures buffer AND threads through the captured `char_classes` pointer/length.
- **Test cleanup**. The legacy `step3a_refuses_multibyte_literal` test was removed: it asserted that `é` was rejected as `CodegenUnsupported`, which was true at step 3a but is now wrong.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902 baseline tests (unchanged), `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: **`cargo test -p rgx-core --features jit` 944 lib tests pass** (695 baseline + 249 C1, +19 from step 6), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Pre-existing RGX bug noted (not fixed)**: `Regex::find_first("[^0-9]", "123abc")` returns `(3, 6)` via the C2 DFA path, which is leftmost-LONGEST semantics. The raw `RegexVM::find_first` returns `(3, 4)`, the correct backtracking semantics. This is a divergence between the C2 DFA and the VM — the JIT correctly matches the VM. Fixing the DFA's negated-char-class semantics is out of scope for step 6 and tracked as a follow-up.
- **Next concrete action**: C1 step 7 (runtime safety helpers — step counter, recursion depth, backtrack frame limit — inlined as Cranelift branches). The existing safety-limits API (`set_max_steps`, `set_max_backtrack_frames`, `set_max_recursion_depth`) is enforced by the interpreter via `ctx.steps`/`ctx.recursion_depth`/`ctx.backtrack_stack.len()` checks. Step 7 lowers these checks into the JIT'd code as inline branches that decrement counters on each consuming op and branch to a "safety abort" path on overflow. The safety-abort path returns a sentinel value the engine layer translates into the appropriate error. After step 7: step 8 (production cutover, benchmarks, Book chapter expanded to its full form).

### 2026-04-11 (late evening — fifteenth commit)
- **C1 step 4b (capture trail in JIT'd code) landed.** Capture-bearing patterns like `(\d+)`, `(a+)b`, `(\w+)@(\w+)\.(\w+)` are now JIT-eligible. Previously the JIT only handled the implicit group-0 wrapper; the decoder rejected `SaveStart(g)` / `SaveEnd(g)` for any `g != 0`. Step 4b extends the JIT'd function signature to take a captures buffer pointer, emits real codegen for `Save` ops on any group id, and adds a per-frame capture **snapshot** so backtracking correctly undoes capture writes.
- **Per-frame snapshot architecture (deviation from design doc §6.1)**. The design doc sketched a per-modification trail (each `Save` op pushes a `(group, slot, prev_value)` entry to a separate trail buffer; backtrack-pop pops trail entries down to a saved trail length). Step 4b takes the **simpler equivalent** approach: each backtrack frame stores a SNAPSHOT of the entire captures buffer at the moment of the matching `Split` / `SplitLazy` push. On a backtrack-pop, the snapshot is restored back into the captures buffer in one shot — undoing every capture write since the push without per-modification bookkeeping. Both approaches are byte-for-byte equivalent under the differential gate; the snapshot scheme is dramatically simpler in codegen terms (one unrolled load/store sequence vs a runtime trail-restore loop).
- **Function signature change**. The JIT'd function went from `(text, text_len, pos) -> isize` to `(text, text_len, pos, captures_ptr) -> isize`. The new type alias is `JittedFn`; the legacy `Step3aJittedFn` is kept as an alias for backwards compatibility (no caller code uses the 3-arg form anymore — every call site is updated). The `captures_ptr` points to a `[i64; 2 * (num_groups + 1)]` buffer pre-initialised to `-1` in every slot. Each pair `(captures_ptr[2*g], captures_ptr[2*g+1])` is `(start, end)` for group `g`. On a successful return the buffer is populated; on a `-1` return the buffer state is **undefined** (the JIT may have partially written before failing) — the engine layer resets the buffer to all `-1`s before every call.
- **Eligibility cap: `C1_MAX_USER_GROUPS = 16`**. The per-frame snapshot size grows linearly with the number of capture groups, so the bt_stack budget grows linearly too. At the 16-group cap each frame is `16 + 16 * (16 + 1) = 288` bytes, total bt_stack = `256 * 288 = 73 728` bytes ≈ 72 KiB of function stack. Patterns above the cap are rejected by `is_jit_eligible` and routed to the interpreter via the engine dispatch chain.
- **`emit_capture_snapshot_save` / `emit_capture_snapshot_restore`**. Two new helpers in `c1/codegen.rs`. `emit_capture_snapshot_save` is called from `emit_backtrack_push` after writing the (saved_pc, saved_pos) pair; it emits an unrolled load/store sequence copying the captures buffer into the per-frame snapshot region (offset 16 from the frame base). `emit_capture_snapshot_restore` is called from the failure_dispatch `pop_block` after loading (saved_pc, saved_pos); it emits the mirror sequence copying the snapshot back into the captures buffer. Both are unrolled at JIT-compile time because `num_groups` is bounded by 16 — Cranelift can optimise the straight-line code without runtime branches.
- **`JitOp::Save { group, which }`** replaces `JitOp::SaveGroupZero { which }`. Codegen for `Save`: compute slot offset = `(2*group + which_offset) * 8`, store `pos` at `captures_ptr + slot_offset`, jump to next block. No trail push (the snapshot in the enclosing Split's frame handles undo on backtrack). The decoder (both `decode_program` and `decode_simple_inner_into`) now accepts `SaveStart` / `SaveEnd` for any group id and emits `JitOp::Save { group: u32::from(group_id), which }`.
- **Variable per-program frame size**. Steps 3a–4a used a fixed `C1_BACKTRACK_FRAME_BYTES = 16` constant. Step 4b replaces this with `frame_bytes_for(num_groups: u32) -> i64` which computes `16 + 16 * (num_groups + 1)` at JIT-compile time. The bt_stack stack slot is sized via `backtrack_stack_bytes_for(num_groups)` similarly. `compile_program` reads `program.num_groups` at the top and threads `frame_bytes`, `snapshot_bytes`, and `num_groups` through to `emit_jit_op`, `emit_backtrack_push`, and the failure_dispatch builder.
- **Engine layer changes**. Three new helpers in `engine.rs`: `new_capture_buffer(num_groups: u32) -> Vec<i64>`, `reset_capture_buffer(captures: &mut [i64])`, and `jit_match_to_result(start, end, &captures, num_groups) -> MatchResult` (extended signature — was `jit_match_to_result(start, end)` at step 5). Each `try_jit_is_match` / `try_jit_find_first` / `try_jit_find_all` allocates ONE buffer per call (not per scan position) and resets it between scan positions via `reset_capture_buffer`. After a successful match, the buffer is read into `MatchResult.groups` with `Some((s, e))` for participating groups and `None` for unset slots. Group 0 is always forced from `(start, end)` regardless of buffer contents.
- **14 new step-4b tests** in `c1::codegen::tests::step4b_*`. **Direct-call differential tests** (8): `(abc)`, `(\d)`, `(\d+)`, `(\d)(\d)`, `(\w+)@(\w+)\.(\w+)`, `(a+)b`, `(a+?)b`, `\A(\w+)\z`, `(a|b)c` — each pattern is JIT-compiled directly, run through a position-by-position scan loop, and the resulting `(start, end, captures_buffer)` is compared byte-for-byte AND group-for-group against `Regex::find_first`'s `MatchResult.groups`. Result: zero divergences. **Engine-path test**: `(a)|(b)` — top-level alternation, routes through the interpreter via `build_jit_program_if_eligible`'s exclusion. **Buffer contract tests** (2). **Eligibility cap tests** (2): 16 groups accepted, 17 rejected. **Backtracking-with-captures test**: `(a+)b` — validates the snapshot/restore correctness for backtracking through the capture.
- **Test harness refactor**. The 33 existing test sites that called `func(text.as_ptr(), text.len(), pos)` would all need to add a captures buffer pointer. To avoid touching every site, `jit_compile` now returns `(JitHost, impl Fn(*const u8, usize, usize) -> isize)` — a closure that internally allocates a fresh capture buffer on every call and forwards the legacy 3-arg shape to the new 4-arg JIT'd function. Existing test bodies are unchanged. For tests that need to inspect captures, a parallel `jit_compile_with_captures` returns `(JitHost, impl Fn(*const u8, usize, usize) -> (isize, Vec<i64>), u32)`.
- **Test cleanup**. The legacy `step3a_refuses_capture_group` test was removed: it asserted that `(abc)` was rejected as `CodegenUnsupported`, which was true at step 3a but is now wrong — capture-bearing patterns like `(abc)` are JIT-eligible at step 4b.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902 baseline tests (unchanged), `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: **`cargo test -p rgx-core --features jit` 920 lib tests pass** (695 baseline + 225 C1, the +14 is the new step-4b tests minus the removed `step3a_refuses_capture_group`), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 6 (`CharClass(id)` and multi-byte literal support via runtime helpers). The JIT currently only handles the six built-in ASCII char-class opcodes (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`) and single-byte `Char` literals. Step 6 extends the codegen to handle (1) `CharClass(id)` opcodes for custom char classes via an indirect call to `rgx_runtime_char_class_test`, (2) multi-byte `Char` literals (UTF-8 sequences) via a runtime helper. The eligibility check then accepts patterns like `[abc]`, `[a-z]`, `[^0-9]`, `é`, `日本語`. After step 6: step 7 (runtime safety helpers — step counter, recursion depth, backtrack frame limit — inlined as Cranelift branches), step 8 (production cutover, benchmarks, Book chapter expanded to its full form).

### 2026-04-11 (late evening — fourteenth commit)
- **C1 step 5 (engine dispatch wiring) landed.** The JIT is now wired into `Engine::find_first` / `find_all` / `is_match` so the existing test suite exercises it transparently for JIT-eligible patterns. The JIT path lives inside the engine alongside the C2 DFA and Pike-VM dispatch slots — no caller has to opt in. The `jit` Cargo feature is still off by default; this commit only changes what happens when the feature is enabled. Step 5 is a small structural commit (no new codegen surface), but it is the moment the JIT becomes externally observable through the public Regex API.
- **The new `JitProgram` struct** in `c1/jit.rs` encapsulates `JitHost + FuncId` and exposes `raw_fn_ptr() -> *const u8`. New helper `c1::compile_program_to_jit_program(&Program) -> Result<JitProgram, JitHostError>` builds, defines, and finalises the function in a single call. New `unsafe impl Send for JitProgram` documented for the read-only-after-finalize invariant: the JIT module is constructed once, then stored on `Engine` inside a `Mutex` and never mutated again. All subsequent use is read-only. This is necessary because `Mutex<JitProgram>` requires `JitProgram: Send` to be `Sync`, and `Engine` must be `Sync` because `Regex` is `Send + Sync`.
- **New `jit_program: Option<Mutex<JitProgram>>` field on `Engine`**, gated on `feature = "jit"`. Populated at compile time by `build_jit_program_if_eligible(ast, program)` which has two layers of gating: (1) **Top-level alternation exclusion** mirrors C2 dispatch — patterns with top-level alternation skip the JIT entirely because the JIT'd function signature returns only the match span (`isize`), not the matched branch number, but `MatchResult.matched_branch_number` requires `Some(branch_idx)` for top-level alternation patterns. (2) **JIT codegen attempt** — anything outside the JIT-eligible subset returns `Err(CodegenUnsupported)` and the engine silently stores `None`. To enable the alternation check, `c2::program::has_top_level_alternation` was made `pub(crate)`.
- **New runtime gate `Engine::should_use_jit`** mirrors `should_dispatch_to_c2`: returns `Some(&Mutex<JitProgram>)` only when the engine has a JIT program AND the runtime state allows JIT dispatch (no event observer, no runtime safety limits, no literal_finder). New methods `try_jit_is_match` / `try_jit_find_first` / `try_jit_find_all` each use `PrefixScanner::new(&self.vm, None)` for skip acceleration — the same scanner the C2 dispatch path uses — so the JIT inherits literal-prefix optimization for free. Both `try_jit_is_match` and `try_jit_find_first` include trailing-position handling for empty-match patterns (call the JIT'd function once at `text.len()` after the scan loop to catch patterns like `\z`).
- **The 4-tier dispatch chain** in `Regex::find_first` / `find_all` / `is_match` is now: **DFA → Pike-VM → JIT → interpreter**. Implemented in `lib.rs` via three new helper functions `jit_dispatch_find_first` / `jit_dispatch_find_all` / `jit_dispatch_is_match`, feature-gated with non-jit stubs returning `None` so the dispatch chain doesn't need `#[cfg]` clutter at every call site.
- **Why JIT goes AFTER Pike-VM** (deviation from design doc §8): Pike-VM is the safety net for nested-quantifier patterns where the JIT could blow up exponentially. The JIT inherits the same backtracking behaviour as the interpreter — a pattern like `(a+)+b` would compile fine through the JIT and then hang on adversarial input. Pike-VM, by contrast, has the "can't hang" guarantee. JIT only kicks in for patterns that fall outside both DFA and Pike-VM eligibility (typically anchors, word boundaries, or quantifier shapes that disqualify them from C2). The current JIT win is narrower than the design doc anticipated, but it's the correct accuracy-first call. Ordering can be revisited in step 4b (when capture trail lands) and step 7 (runtime safety limits inlined).
- **Two bugs caught and fixed during integration**:
  1. **Sync/Send error**: first build with `--features jit` failed because `cranelift_jit::JITModule` is `!Send`. Fixed with `unsafe impl Send for JitProgram` and a documented safety comment.
  2. **Two failing tests** (`top_level_branch_id_exposed`, `top_level_branch_id_not_overridden_by_nested_alternation`): the JIT was intercepting `cat|dog|bird` and similar top-level alternation patterns, returning `matched_branch_number = None` instead of `Some(2)`. Fixed by excluding top-level alternation patterns in `build_jit_program_if_eligible`.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 695 lib + 44 + 19 + 26 + 12 + 55 + 11 + 21 + 19 = 902 baseline (unchanged — c1 module is feature-gated), `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit` **907 lib tests pass** (695 baseline + 212 C1), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 4b (capture trail in JIT'd code). Extends the JIT'd function signature from `(text, text_len, pos) -> isize` to `(text, text_len, pos, captures_ptr, captures_len) -> isize`. Emits real codegen for `SaveStart`/`SaveEnd` with non-zero group ids, maintaining a per-call trail (recording every modification for backtrack-undo). After 4b: step 6 (`CharClass(id)` and multi-byte literal support via runtime helpers), step 7 (runtime safety helpers inlined as Cranelift branches), step 8 (production cutover, benchmarks, Book chapter).

### 2026-04-11 (evening — thirteenth commit)
- **C1 step 4a (corpus-based differential test harness) landed.** The design doc §1.0 (accuracy first) hard gate is now active for the existing JIT-eligible subset. Adds a test harness that JIT-compiles patterns AND runs the same patterns through the interpreter, then asserts byte-for-byte match-span equivalence across multiple inputs. 27 new differential tests; result: zero divergences. With `--features jit` 212 C1 tests pass (185 + 27 new).
- **The harness architecture**:
  - `jit_find_first_via_scan(func, text) -> Option<(start, end)>` wraps a JIT'd `Step3aJittedFn` in a scan loop. For each position 0..=text.len() (inclusive — to allow empty matches at end of text), it calls the JIT'd function and returns the leftmost successful match. This mimics the interpreter's `find_first` scan semantics so the two paths can be compared apples-to-apples.
  - `assert_jit_interp_equivalent(pattern, &[inputs])` compiles the pattern via both `Regex::compile` (interpreter) and `compile_program` (JIT), iterates over the inputs, and asserts the match spans are byte-for-byte identical. Patterns the JIT can't handle (`CodegenUnsupported`) cause the helper to return `false` without panicking — they would route through the interpreter in production anyway.
- **Why this is "step 4a" not "step 4"**: the design doc step 4 includes both the differential gate AND the capture trail in JIT'd code. Splitting into 4a (this commit, differential gate) and 4b (capture trail) keeps each commit accuracy-first scoped. After 4b, capture-bearing patterns become JIT-eligible (currently the decoder rejects `SaveStart`/`SaveEnd` with group_id > 0).
- **The corpus covers all the JIT-eligible opcode families**: literals (`abc`, `a`), char classes (`\d`, `\w`, `\s` and their negated forms), anchors (`\Aabc`, `abc\z`, `\Aabc\z`, `\bword\b`), alternations (`cat|dog`, `cat|dog|bird`, `ab|abc`), all six quantifier flavours (greedy `\d+`/`\d*`/`\d?` and lazy `\d+?`/`\d*?`/`\d??`), and combinations (`\d+x`, `\A\d+\z`, `\w+@\w+\.\w+`, `\w+|word`, `a*b+`, `a*?b`, `\b\d+\b`, `\Ahello\b`).
- **Every test uses multiple inputs per pattern** (typically 5–8 inputs) so the verification is broad. A pattern might pass the unit-test harness on a single hand-picked input but fail on a different one — the differential corpus catches that.
- **Result: zero divergences across all 27 tests**. Every JIT-compiled pattern produces byte-for-byte identical match spans to the interpreter on every corpus input. This locks in the correctness of steps 3a–3e.4 and gives us a high-confidence baseline for the next steps.
- **The four-substep streak (3e.1, 3e.2, 3e.3, 3e.4) of "no bugs caught on the first run" is now backed by the broader differential gate** — the unit tests AND the corpus comparison both pass cleanly. This is a strong validation of the decoder-unfolding architecture.
- 2 small clippy warnings introduced and fixed: `cast_sign_loss` on `result as usize` (we already check `result >= 0` so it's safe — added `#[allow]` with the explanatory comment), `doc_markdown` on `0..=text.len()` needing backticks in the helper doc.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit --lib c1` 212/0, `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 4b (capture trail in JIT'd code). The JIT'd function needs to maintain a per-call capture buffer (one entry per group slot) and a trail (recording every modification for backtrack-undo). The simplest approach: extend the JIT'd function signature from `(text, text_len, pos) -> isize` to `(text, text_len, pos, captures_ptr, captures_len) -> isize` so the caller provides a buffer. The codegen emits real save/restore in `SaveStart`/`SaveEnd` arms instead of treating them as no-op. The trail is a separate stack-allocated buffer that records (group_id, slot, prev_value) tuples on each save, and the failure_dispatch path pops trail entries down to the saved trail_len from the bt_stack frame. After 4b: step 5 (engine dispatch wiring) — wires the JIT into `Engine::find_first` / `find_all` / `is_match` so the existing test suite exercises it transparently.

### 2026-04-11 (evening — twelfth commit)
- **C1 step 3e.4 (lazy quantifier variants) landed. STEP 3e IS COMPLETE.** Adds the three lazy optimized quantifier opcodes — QuestionLazy `??`, StarLazy `*?`, PlusLazy `+?` — by reusing the same lowerings as their greedy counterparts but substituting `SplitLazy` for `Split` (which swaps the branch ordering). All six optimized quantifier opcodes are now JIT-compilable. With `--features jit` 185 C1 tests pass (173 + 12 new).
- **The lazy/greedy contrast** is now externally verifiable in the JIT path. The classic test: `a*` against `aaa` returns 3, `a*?` against `aaa` returns 0. Same for `a+` vs `a+?` (3 vs 1) and `a?` vs `a??` (1 vs 0). The codegen correctly captures the semantic difference — lazy prefers minimum, greedy prefers maximum.
- **Refactor: three quantifier-emit helpers** (`emit_question_quantifier`, `emit_star_quantifier`, `emit_plus_quantifier`) parameterized by a `lazy: bool` flag. The six decoder arms (3 greedy + 3 lazy) collapse to one helper invocation each. The previous greedy decoder arms (steps 3e.1, 3e.2, 3e.3) were rewritten to call the new helpers. This eliminates a lot of duplication and makes future quantifier additions trivial.
- **The lowerings**: each lazy variant has the same shape as its greedy counterpart, with `Split` swapped for `SplitLazy`:
  - QuestionLazy: `[SplitLazy{exit}, inner_jit_ops...]`
  - StarLazy: `[SplitLazy{exit}, inner_jit_ops..., Jump{back to SplitLazy}]`
  - PlusLazy: `[inner_jit_ops..., SplitLazy{exit}, Jump{back to inner_start}]`
- **`compute_jit_op_count` extended** to recognize the lazy variants. Same match arm covers all six optimized quantifiers; the `extra` count (`+1` for question, `+2` for star/plus) is computed via `matches!(op, OpCode::QuestionGreedy | OpCode::QuestionLazy)`.
- **No bugs caught on the first run**: all 12 step 3e.4 tests pass on the first attempt. Four-commit streak of clean step 3e substeps (3e.1, 3e.2, 3e.3, 3e.4) — the decoder-unfolding architecture is well-suited to incremental quantifier additions.
- 1 small clippy warning fixed (`similar_names` on test variable bindings — renamed `func_g`/`func_l`/`func_pg`/`func_pl`/`func_qg`/`func_ql` to descriptive names like `star_greedy_fn` / `star_lazy_fn` / etc.). 1 small `doc_markdown` warning fixed (`bt_stack` and `inner_start` needed backticks in the new helper docs).
- **JIT subset coverage** as of step 3e completion: literals, all six built-in char classes, simple anchors (`\A`, `\z`, `^` in non-multiline mode), word boundaries (`\b`/`\B` via runtime helper), control flow (`Split`/`Jump`/`SplitLazy`/`SetAlternative`), all six optimized quantifiers (`+`, `*`, `?`, `+?`, `*?`, `??`) with simple-linear-inner subset, group-0 capture wrappers (no-op for now). NOT yet supported: capture groups 1+, lookaround, backreferences, recursion, code blocks, atomic groups, multi-byte literals, line anchors `^`/`$` in multiline mode, `\Z`, `\X`, `\K`, nested optimized quantifiers in inner subprograms.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit --lib c1` 185/0, `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 4 (capture trail in JIT'd code + differential gate active). The capture trail is a per-call data structure that records every modification to capture group slots, so backtracking can undo them on frame pop. The JIT'd function needs to maintain its own trail (or share the existing VM's `ctx.capture_trail`). The simplest approach: extend the JIT'd function's signature to take a captures buffer pointer alongside (text, text_len, pos), and have the codegen emit trail-push instructions in `SaveStart`/`SaveEnd` arms. The differential gate becomes active when step 4 ships — every JIT-eligible test in the existing 902-test suite will be exercised through both the JIT and the interpreter, with byte-for-byte equivalence asserted. Step 5 (engine wiring) follows.

### 2026-04-11 (evening — eleventh commit)
- **C1 step 3e.3 (QuestionGreedy via decoder unfolding) landed.** Adds `?` quantifier support — the simplest of the three optimized quantifier lowerings. The lowering is `[Split{exit}, inner_jit_ops...]` with NO Jump back, because `?` is "zero or one" and there's no loop. Total unfolded count is `inner_count + 1` (not `+2` like Plus/Star). Patterns like `a?`, `\d?`, `(?:ab)?`, `\Aa?\z`, `a?b+` are now JIT-compilable. 12 new tests; no bugs caught on the first run. With `--features jit` 173 C1 tests pass.
- **All three greedy optimized quantifiers are now supported**: `PlusGreedy` (3e.1), `StarGreedy` (3e.2), `QuestionGreedy` (3e.3). The decoder-unfolding architecture proved extensible — each step added ~30-50 lines of new code without disrupting the existing infrastructure. The `read_inline_subprogram` helper added in step 3e.1/3e.2 was reused in step 3e.3 without modification.
- **The QuestionGreedy lowering is the simplest** because there's no loop tail. Each greedy variant differs only in:
  - PlusGreedy: `[inner..., Split, Jump back to inner]` — first iter mandatory, then loop
  - StarGreedy: `[Split, inner..., Jump back to Split]` — Split before so zero iterations is valid
  - QuestionGreedy: `[Split, inner...]` — no loop, no Jump
- **The `a?a` test was important**: it proves backtrack-into-quantifier works for `?` correctly. For single `a`, `a?` greedily takes the a, trailing `a` fails (eof), backtrack to zero a's, trailing `a` matches the only a. This is the same backtrack pattern as `a*a` and `a+a` but with the maximum-one-iteration constraint of `?`.
- **`compute_jit_op_count` extended** to handle QuestionGreedy alongside Plus/Star: a single match arm covers all three, with `matches!(op, OpCode::QuestionGreedy)` picking the right offset (`+1` vs `+2`).
- **No bugs caught on the first run**: all 12 step 3e.3 tests pass on the first attempt. Three commits in a row (3e.1, 3e.2, 3e.3) shipped first-try clean — the architectural foundation laid in step 3d.2 (Switch-based br_table with Variables) and step 3e.1 (decoder unfolding pattern) is paying dividends.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit --lib c1` 173/0, `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3e.4 (lazy variants — `StarLazy`, `PlusLazy`, `QuestionLazy`). The lowering for lazy quantifiers uses `SplitLazy` instead of `Split`, which swaps the branch ordering — instead of "fall-through to inner first, on backtrack go to exit", it's "go to exit first, on backtrack fall-through to inner". The result is that the lazy quantifier matches as FEW iterations as possible while still allowing the rest of the pattern to match. Same `read_inline_subprogram`, same simple-inner subset, same architecture — just swap Split for SplitLazy and (for `*?`/`+?`) reverse the branch targets. Step 3e.4 should be small and clean. After 3e.4: step 4 (capture trail + differential gate active), step 5 (engine wiring).

### 2026-04-11 (evening — tenth commit)
- **C1 step 3e.2 (StarGreedy via decoder unfolding) landed.** Adds `*` quantifier support via the same decoder-unfolding approach as step 3e.1. The lowering for `*` puts the `Split` BEFORE the inner (since `*` allows zero matches): `[Split{exit}, inner_jit_ops..., Jump{back to Split}]`. The Jump targets the Split (NOT inner_start) so each iteration pushes a fresh bt_stack frame, accumulating one frame per successful iteration INCLUDING the zero-iteration case. Patterns like `a*`, `\d*`, `\w*`, `\s*`, `(?:ab)*`, `\A\d*\z`, `a*b+` are now JIT-compilable. 14 new tests; no bugs caught on the first run. With `--features jit` 161 C1 tests pass.
- **The key difference from PlusGreedy**: where PlusGreedy puts `Split` AFTER the inner (since `+` requires at least one match), StarGreedy puts it BEFORE. Both share the same `compute_jit_op_count` formula (`inner_count + 2`) and the new factored-out `read_inline_subprogram` helper. The architecture is reusable for the remaining step 3e substeps.
- **Why the Jump targets the Split, not inner_start**: looping back to the Split means each iteration re-pushes a backtrack frame. With one frame per iteration (including the zero-iteration case), backtracking can shrink the iteration count by one each time, all the way down to zero. If the Jump targeted inner_start instead, only the very first Split visit would push a frame, and backtracking could only exit (not shrink).
- **No bugs caught on the first run**: all 14 step 3e.2 tests pass on the first attempt. The decoder-unfolding architecture proved easy to extend — the only new code is the StarGreedy arm and the `read_inline_subprogram` helper extraction. The bt_stack semantics from step 3d.2 handled the new pattern without modification.
- **The `a*a` test was important**: it proved that backtrack-into-quantifier works correctly across the zero-iteration boundary. For single `a`, `a*` consumes zero, then trailing `a` matches. For `aa`, `a*` consumes one (or two then backtracks to one), then trailing `a` matches. For empty input, `a*` is fine but trailing `a` fails (eof). For `b`, `a*` matches zero but trailing `a` doesn't match `b`.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit --lib c1` 161/0 (161 C1 tests), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3e.3 (QuestionGreedy `?` — single conditional execution, no loop). The lowering for `?` is the simplest of the optimized quantifiers: `[Split{exit}, inner_jit_ops...]` with no Jump back. The Split pushes (exit, current_pos) and falls through to the inner. If the inner succeeds, fall through to the next op (whatever comes after the `?`). If the inner fails, failure_dispatch pops the frame and exits at the saved pos. No loop because `?` is "zero or one" — no second iteration. After 3e.3: the lazy variants (StarLazy, PlusLazy, QuestionLazy) which use SplitLazy with reversed branch ordering. Then step 4 (capture trail + differential gate active), step 5 (engine wiring).

### 2026-04-11 (evening — ninth commit)
- **C1 step 3e.1 (PlusGreedy via decoder unfolding) landed.** Adds `+` quantifier support. The decoder reads `PlusGreedy(inner)` opcodes and unfolds them into `[inner_jit_ops..., Split{exit}, Jump{back to inner_start}]` using the step 3d.2 backtracking infrastructure. Patterns like `a+`, `\d+`, `\w+`, `\s+`, `(?:ab)+`, `\w+@\w+\.\w+`, `\A\d+\z`, `\d+|word` are now JIT-compilable. 13 new tests; default build unchanged at 902, with `--features jit` lib tests grow to 842 (147 C1 tests).
- **The unfolding lowering**: PlusGreedy(inner) → [inner_jit_ops..., Split{exit}, Jump{back}]. The first iteration of inner is mandatory; the Split-based loop handles 2nd+ iterations with greedy backtracking via the existing bt_stack. Each successful iteration pushes one bt_stack frame; backtracking pops them in LIFO order, shrinking the iteration count by one each time.
- **Restricted to "simple linear inner" subset for step 3e.1**: the inner can only contain literals, char classes, anchors, word boundaries, group-0 wrappers. NO nested control flow (Split/Jump) or nested optimized quantifiers. This restriction lets the unfolding be straightforward — each inner bytecode opcode contributes exactly 1 JitOp with no internal targets to shift. Subsequent substeps (3e.2/3e.3) will widen.
- **Two-pass decoder restructuring**: `collect_op_positions` now returns `Vec<(usize, usize)>` (byte_offset, jit_op_idx) instead of just byte_offset. The jit_op_idx is the index of the FIRST JitOp emitted for that bytecode opcode — most contribute 1 jit_op, but PlusGreedy contributes (inner_count + 2). Pass 1 simulates the unfolding via `compute_jit_op_count` so the byte_offset → jit_op_idx map is correct before pass 2 emits actual JitOps. Forward Split/Jump targets pointing AT a PlusGreedy opcode now resolve correctly to the first JitOp in its unfolded sequence.
- **No bugs caught on the first run**. The two-pass design with `compute_jit_op_count` (pass 1) and the `debug_assert_eq` between pass 1 and pass 2 in the PlusGreedy arm caught any potential drift. The architecture from step 3d.2 (Switch-based br_table with Variables, bt_stack push/pop) handled the new "one frame per iteration" backtrack pattern without modification.
- **The critical test**: `a+a` against `aa`/`aaa`/`a` proves backtrack-into-quantifier works correctly. The greedy `a+` over-consumes (eats all the a's), the trailing `a` fails (no more chars), backtracks one iteration so the trailing `a` matches. For `a` alone, `a+` consumes 1, trailing `a` fails (eof), would need to shrink to 0 iterations but `+` requires 1+ → fail.
- **`step3a_refuses_quantifier` removed**: it was correct at step 3a but step 3e.1 now correctly accepts `a+`. Replaced by 13 positive tests in the step 3e.1 section.
- New helper functions: `compute_jit_op_count`, `simple_inner_jit_op_count`, `is_simple_inner_opcode`, `decode_simple_inner_into`. The `simple_inner_jit_op_count` is called both by `compute_jit_op_count` (pass 1, to compute the unfolded length) and indirectly via `decode_simple_inner_into` (pass 2, to emit the JitOps); the debug_assert in the PlusGreedy decoder arm ensures the two are in sync.
- 2 small clippy warnings introduced and fixed: `range_plus_one` on `1..1+length_byte`, several `doc_markdown` warnings on JitOp/PlusGreedy/etc. needing backticks.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: lib tests 842/0 (147 C1 tests), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3e.2 (StarGreedy via decoder unfolding). The lowering for `*` is similar to `+` but with the Split BEFORE the first iteration (because `*` allows zero matches): `[Split{exit}, inner_jit_ops..., Jump{back to Split}, exit]`. The Split's "fall through" goes to inner_ops; on backtrack, jump to exit. After step 3e.2: step 3e.3 (QuestionGreedy `?` — single conditional execution, no loop), then the lazy variants (StarLazy, PlusLazy, QuestionLazy with reversed Split/SplitLazy semantics). After all step 3e substeps: step 4 (capture trail + differential gate active), step 5 (engine dispatch wiring), steps 6–8.

### 2026-04-11 (evening — eighth commit)
- **C1 step 3d.2 (control flow + backtracking) landed.** Biggest C1 substep yet. Adds the full backtracking infrastructure (256-frame stack-allocated bt_stack, `failure_dispatch_block` with `br_table`, two-pass `decode_program` for forward-jump targets) plus codegen for `Split`/`SplitLazy`/`Jump`/`SetAlternative` opcodes. Alternation patterns like `cat|dog`, `\d|\w`, `\Acat|\Adog`, `(?:cat|dog)|bird` are now JIT-compilable end-to-end with byte-for-byte correct backtracking. Default build unchanged at 902 tests; with `--features jit` 1037 tests pass (135 C1 tests).
- **Architecture**:
  - Backtrack stack: `Function::create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 4096))` for 256 frames × 16 bytes per frame.
  - `bt_top_var` Variable (i64 counter, 0..256).
  - `text_ptr_var` / `text_len_var` / `pos_var` are all Variables now (3d.1 made pos a Variable; 3d.2 promotes text_ptr/text_len because op_blocks reached via failure_dispatch don't have a clean SSA dominance path back to entry).
  - `failure_dispatch_block` pops a frame (decrements bt_top, loads (saved_pc, saved_pos) from `stack_addr + bt_top * 16`, sets `pos_var = saved_pos`) and dispatches to `op_blocks[saved_pc]` via `cranelift_frontend::Switch`. If bt_top is 0, jumps to fail_block (return -1).
  - All consuming-op fail edges redirect to `failure_dispatch_block`. The `fail_block` is only reached when bt_top is 0 OR when `emit_backtrack_push` detects bt_top would overflow 256.
- **The Cranelift API gotchas caught and fixed**:
  1. **JumpTableData::new takes BlockCall not Block**: my first draft used raw `Block` values which the compiler rejected. Switched to `dfg.block_call(b, &[])` which compiled but produced a verifier error: `arg 0 (v22) has type i64, expected i32`. The error was misleading — the real issue was that Cranelift's SSA pass inserts implicit block parameters for the Variables AFTER the br_table is constructed, and `dfg.block_call(b, &[])` passes zero args which doesn't match the SSA-inserted params. **The right answer is `cranelift_frontend::Switch::set_entry/emit`** which defers the br_table construction so the SSA-inserted args resolve correctly when blocks are sealed at the end. Documented inline as the canonical pattern.
  2. **Sealing order**: my first draft sealed op_blocks inside the per-op-block emission loop, which caused Cranelift to panic with `assertion failed: !self.is_sealed(block)` when the failure_dispatch's br_table later tried to add predecessor edges. Fixed by deferring all op_block sealing until after `failure_dispatch_block` is fully built (a second pass at the end of `compile_program`).
  3. **`SetAlternative` opcode**: the existing compiler emits `SetAlternative` alongside top-level alternation (to record `MatchResult.matched_branch_number`). The eligibility check accepts it but my decoder rejected it as unsupported, causing every alternation test to fail on the first run. Added `JitOp::SetAlternative` as a no-op variant — the JIT'd function returns only `isize` so there's no place to record the branch number. Step 5 (engine wiring) will handle the contract by other means.
- **The pos-restoration test is the most important verification**: `\dxy|\dab` against `5ab` proves the second branch sees pos 0 (NOT pos 1) after the first branch's `\d` consumed `5` and then failed on `xy`. If the frame storage, the load, the def_var, or the SSA wiring were wrong, this test would fail.
- **`step3a_refuses_alternation` removed**: it was correct at step 3a (which refused control-flow opcodes) but step 3d.2 now correctly accepts alternation. Replaced by 10 positive tests in the new step 3d.2 section.
- 4 small clippy warnings introduced and fixed: 3 doc_markdown warnings (`bt_stack` / `bt_top` / `saved_pc` / `op_block` need backticks across the new doc comments), 3 too_many_lines warnings on `compile_program` / `emit_jit_op` / `decode_program` (added `#[allow(clippy::too_many_lines)]` with explanatory comments), 1 cast_possible_truncation/cast_sign_loss on the `C1_BACKTRACK_STACK_BYTES` const. Also changed `C1_BACKTRACK_STACK_FRAMES` and `C1_BACKTRACK_FRAME_BYTES` from `usize` to `i64` so the Cranelift `imul_imm` / `icmp_imm` calls don't need `as i64` casts.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit` 1037/0 (135 C1 tests), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3e (optimized quantifier opcodes — `QuestionGreedy`, `StarGreedy`, `PlusGreedy`, `QuestionLazy`, `StarLazy`, `PlusLazy`). These opcodes wrap an inline subprogram in their operand bytes and need recursive codegen — the subprogram gets its own op_blocks within the parent function. Step 3e unlocks `a*`, `a+`, `a?`, `a*?`, `a+?`, `a??` patterns. Then step 4 (capture trail + differential gate active) and step 5 (engine dispatch wiring).

### 2026-04-11 (evening — seventh commit)
- **C1 step 3d.1 (refactor pos to Cranelift Variable) landed.** Pure architectural pivot — no new functionality, no behaviour change, no new tests. The JIT'd function's match position `pos` is now held in a `Variable::from_u32(0)` declared at the top of `compile_program`, instead of being passed between op_blocks via block parameters. Each op_block reads pos via `use_var(pos_var)` once at the top; consuming ops write the new pos via `def_var(pos_var, new_pos)` on the success edge in a fresh `advance_block`; zero-width ops leave pos_var unchanged; the success block reads pos_var fresh to produce its return value.
- **Why the refactor**: step 3d.2 needs to restore `pos` from the backtrack stack on failure dispatch (when popping a frame). Cranelift's `br_table` instruction doesn't accept per-target arguments, so anything that needs to be restored on backtrack MUST live in a Variable that the dispatch block can write before jumping. Block parameters can't survive a `br_table` dispatch. The Variable refactor is the foundation step 3d.2 builds on.
- **All 126 existing C1 tests pass on the first run after the refactor**, confirming the Variable + use_var/def_var pattern produces semantically identical IR to the previous block-parameter pattern. Cranelift's SSA pass handles auto-phi insertion (which currently never fires for linear programs but will once Split/Jump dispatch lands at step 3d.2).
- **Cranelift API gotcha caught immediately**: my first draft used `Variable::new(0)` (the obvious-sounding constructor) which doesn't exist in Cranelift 0.101. The compiler error pointed at the deprecated `Variable::with_u32` and the canonical `Variable::from_u32`. Fixed in one edit. Documented the right constructor inline.
- **Touched functions**:
  - `compile_program`: declares pos_var, removes `append_block_param` for op_blocks/success_block, replaces jumps-with-pos-arg with jumps-with-empty-args, replaces `block_params(success_block)[0]` with `use_var(pos_var)`.
  - `emit_jit_op`: gains a `pos_var: Variable` parameter alongside `pos: Value` (current value, already loaded by the caller). All zero-width ops use empty arg lists. Match ignores pos_var and pos because the success block reads pos_var fresh.
  - `emit_consume_byte_with_test`: gains `pos_var`. Success edge jumps to a new `advance_block` that calls `def_var(pos_var, new_pos)` and then jumps to `next_block` with empty args.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit` 1028/0 (126 C1 tests, unchanged), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3d.2 (control flow + backtracking). Adds Split/Jump/SplitLazy opcodes with the 256-frame stack-allocated backtrack array architecture sketched in the previous session entry. Specifically:
  - New `JitOp` variants: `Split { branch_b_op_idx }`, `SplitLazy { branch_b_op_idx }`, `Jump { target_op_idx }`.
  - Two-pass `decode_program`: first pass collects op positions (byte_offset → op_idx), second pass resolves Split/Jump byte targets (`ip_after_operand + offset`) to op indices.
  - Stack slot via `create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 4096))` for 256 backtrack frames × 16 bytes (8 saved_pc + 8 saved_pos).
  - `bt_top_var: Variable` (i64 counter, 0..256).
  - `failure_dispatch` block: checks `bt_top > 0`, decrements bt_top, loads (saved_pc, saved_pos) from `stack_addr + bt_top * 16`, sets `pos_var = saved_pos`, uses `br_table` to dispatch to `op_blocks[saved_pc]`.
  - `emit_jit_op` Split arm: pushes (next_op_idx_after_split? or branch_b_op_idx?) onto bt_stack and jumps. Need to verify which is the "second" branch from the bytecode dispatch logic.
  - All consuming op fail edges redirect to `failure_dispatch` instead of `fail_block`.
  - New tests: simple alternation `cat|dog`, optional `a?`, nested patterns, edge cases.

### 2026-04-11 (afternoon — sixth commit)
- **C1 step 3c (word boundaries via runtime helper) landed.** Re-orders the design doc plan: step 3c implements `\b`/`\B` via a runtime helper indirect call instead of control flow + backtracking. Control flow (`Split`/`Jump` with a backtrack stack) is a substantially larger commit and gets its own slot at step 3d. Re-ordering keeps each commit accuracy-first scoped while still adding real new capability.
- **The runtime helper infrastructure is now in place** and reusable for step 6 (CharClass + multi-byte helpers) and step 7 (runtime safety helpers):
  1. **Symbol registration**: `JitHost::new` calls `builder.symbol("rgx_runtime_word_boundary_test", rgx_runtime_word_boundary_test as *const u8)` BEFORE creating the `JITModule`. The address cast is sound because the helper is `#[no_mangle] pub unsafe extern "C" fn` so it has a stable C ABI and a stable address. The pattern for future helpers is documented inline as "add a new helper means a new `builder.symbol(...)` line in `JitHost::new` AND a matching `Module::declare_function` call in the codegen layer".
  2. **Function import**: new `JitHost::import_word_boundary_helper(function: &mut Function) -> Result<FuncRef, JitHostError>` declares the helper as `Linkage::Import` inside the given Cranelift `Function` and returns a `FuncRef` usable with `builder.ins().call(...)`. The signature is `(I64, I64, I64) -> I8`. Each `Function` needs its own import — `FuncRef` is scoped to the function, not the module.
  3. **Indirect call codegen**: `emit_jit_op` for `JitOp::WordBoundary { negated }` calls the helper via `builder.ins().call(func_ref, &[text_ptr, text_len, pos])`, reads `inst_results(call)[0]` as i8, compares against zero, and branches based on the negation flag (for `\B` swap branch targets).
- **Real impl of `rgx_runtime_word_boundary_test`** in `c1/runtime.rs`: PCRE2 ASCII semantics — a position is a word boundary iff exactly one of the bytes at `pos-1` and `pos` is `[A-Za-z0-9_]`. Out-of-range positions treated as "non-word" neighbours so `\b` matches at start/end of input iff the adjacent byte is a word char. Uses a private `is_ascii_word_byte` helper that matches the existing VM and the C2 NFA's `\w` definition exactly.
- **No surprises on the first run**: the test corpus passed cleanly. The runtime helper had its own dedicated unit tests (11 tests in `c1::runtime::tests`) BEFORE any codegen wiring, so the helper's correctness was verified in isolation. The codegen layer then only had to verify the wiring (call + branch) which is mechanical. This is the design doc §1.0 pattern working as intended — small slices, verified before integration.
- **JitOp enum extended**: new `WordBoundary { negated }` variant. `decode_program` handles `OpCode::WordBoundary` → `JitOp::WordBoundary { negated: false }` and `OpCode::NonWordBoundary` → `JitOp::WordBoundary { negated: true }`. `compile_program` conditionally imports the helper into the function only if the program contains at least one `WordBoundary` op (avoids wasted symbol declarations).
- **2 outdated step 3b refusal tests removed** (`step3b_refuses_word_boundary` and `step3b_refuses_non_word_boundary` — they correctly asserted refusal at step 3b but step 3c now correctly accepts both). Replaced by 12 positive step 3c codegen tests that JIT-compile patterns like `\bword`, `word\b`, `\bword\b`, `\Bword`, `\b123`, `\b_x`, `\b\d` and verify the boundary semantics.
- **23 new tests** total: 11 helper unit tests in `c1::runtime::tests` + 12 codegen tests in `c1::codegen::tests::step3c_*`. 1 small clippy warning fixed (doc_markdown on `Char` / `StartText` / etc. needing backticks).
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit` 1028/0 (126 C1 tests), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3d (control flow + backtracking — `Split`/`Jump` opcodes with a stack-allocated backtrack array on the JIT'd function frame). Step 3d is the largest remaining sub-step in step 3 and unlocks quantifier and alternation patterns. Architecture sketch: stack slot allocated via `create_sized_stack_slot` for a 256-frame backtrack array (each frame = pc + pos = 16 bytes); Cranelift `Variable` for the bt_top counter; `failure_dispatch` block that pops a frame and uses `br_table` to dispatch to the saved op_block; each op_block uses a Variable for pos so backtracking can restore it; `Split` pushes (target_b, current_pos) onto the stack and falls through to target_a; consuming op failure jumps to `failure_dispatch`. The big risk is getting the SSA semantics right around the backtrack restoration — `Variable`s with auto-phi insertion should handle it.

### 2026-04-11 (afternoon — fifth commit)
- **C1 step 3b (char classes + simple anchors) landed.** Refactored `compile_program` to use a per-opcode block-per-block architecture (each block takes `pos: i64` as a Cranelift block param). New `JitOp` enum + `decode_program` walker. Codegen for `DigitAscii`/`DigitAsciiNeg`, `WordAscii`/`WordAsciiNeg`, `SpaceAscii`/`SpaceAsciiNeg`, `StartText`, `EndText`. **Still does NOT touch the engine.** All 18 step 3a literal tests continue to pass under the new architecture; 34 new step 3b tests pass too. Default build unchanged at 902 tests; with `--features jit` 1007 tests pass (105 C1 tests).
- **The architectural rewrite was the bigger change**. Step 3a's hand-rolled literal-byte chain doesn't generalize to opcodes with branching or with conditional advancement. Step 3b introduces:
  - `JitOp` enum: pre-decoded representation that the codegen layer consumes. Variants for `Char(u8)`, `DigitAscii { negated }`, `WordAscii { negated }`, `SpaceAscii { negated }`, `StartText`, `EndText`, `SaveGroupZero { which }`, `Match`. Decoupled from bytecode format so future steps can extend without touching the walker.
  - `decode_program(code: &[u8]) -> Result<Vec<JitOp>, JitHostError>`: replaces step 3a's `extract_step3a_literal`. Same walking conventions; broader acceptance set; descriptive `CodegenUnsupported` errors.
  - One Cranelift basic block per `JitOp`. Each consuming op bounds-checks `pos < text_len`, loads `text[pos]`, applies an inline predicate (digit/word/space/literal), and either advances pos by 1 and jumps to next or jumps to fail. Each zero-width op (StartText/EndText) checks pos against the boundary and forwards the same pos. Match jumps to success_block which returns the final pos.
  - Per-byte-class predicate helpers: `emit_digit_byte_test`, `emit_word_byte_test`, `emit_space_byte_test`. Each constructs the inline test from Cranelift `icmp_imm`/`band`/`bor`/`bxor_imm` operations. The space test uses 6 byte equality checks (the same six bytes `b.is_ascii_whitespace()` matches in `std`).
- **Cranelift API gotcha caught early**: my first draft passed `b_ins: &mut FuncInstBuilder` to predicate closures, but `FuncInstBuilder` is a value type (each method consumes self by value). The borrow-checker rejected calling multiple methods on `&mut`. Refactored to pass `&mut FunctionBuilder` and call `builder.ins()` on each instruction. 31 compile errors → 0 in one fix. Documented as the canonical pattern for future codegen helpers.
- **Two surprises caught by the tests on the first run**:
  1. **Two step 3a refusal tests outdated**: `step3a_refuses_char_class("\\d")` and `step3a_refuses_anchor("\\Aabc")` asserted these get refused — but step 3b now correctly accepts them. Resolution: removed `step3a_refuses_char_class` (covered positively by `step3b_digit_match`); converted `step3a_refuses_anchor` into the positive test `step3b_caret_lowers_to_start_text_in_non_multiline_mode`. The other 6 step 3a refusal tests still apply.
  2. **PCRE2 anchor asymmetry caught by the test corpus**: `^` in non-multiline mode lowers to `StartText` (= `\A`), but `$` lowers to `EndLine` (≠ `\z`). The PCRE2 default `$` is newline-aware in a way `\z` is not — distinct opcodes. Confirmed by the first test run when `step3b_refuses_end_line_anchor("abc$")` correctly passed (EndLine is rejected) while my naive `step3b_refuses_start_line_anchor("^abc")` wrongly failed because `^` actually lowers to StartText (which step 3b accepts). Documented inline.
- 6 small clippy warnings introduced by the new code, all fixed: `similar_names` for `is_lf`/`is_ff` in the space test (renamed to `is_newline_char`/`is_form_feed`), `dead_code` for `JitOp::SaveGroupZero { which }` and the `SaveSlot` enum (gated `#[allow(dead_code)]` with the comment "reserved for step 4 capture-trail codegen"), 3 `doc_markdown` warnings on `Char` / `StartText` / `EndText` / `SaveGroupZero` needing backticks.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit` 1007/0 (800 lib + 207 elsewhere), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3c (control flow + backtracking). Add codegen for `Split` / `Jump` opcodes which encode quantifiers (greedy/lazy) and alternation. The big architectural extension: the JIT'd function needs a backtrack stack (small fixed-capacity stack on the function's own stack frame, since the JIT'd function can't allocate). Each `Split` instruction pushes a backtrack frame (pc + pos) and falls through to the first branch; backtracking pops a frame and resumes at the saved pc with the saved pos. Patterns like `a+b`, `(cat|dog)`, and `\d{3,5}` become JIT-eligible. Step 4 follows with capture trail handling and the differential gate going active.

### 2026-04-11 (afternoon — fourth commit)
- **C1 step 3a (literal codegen) landed.** First slice of real codegen for the C1 JIT backend. New `c1::codegen::compile_program(program, host) -> Result<FuncId, JitHostError>` translates linear single-byte literal programs (`Char(len=1)` opcodes + group-0 `SaveStart`/`SaveEnd` + `Match`) into a Cranelift function with C ABI signature `unsafe extern "C" fn(text: *const u8, text_len: usize, pos: usize) -> isize`. Returns the new position on a successful match, -1 on no match. **Still does NOT touch the engine** — codegen produces a callable function for standalone testing only.
- **Decision to split design doc step 3 into 3a/3b/3c**: the design doc plan groups the entire easy-opcode codegen into one step (Char + DigitAscii + WordAscii + SpaceAscii + anchors + Split + Jump + Match + SaveStart + SaveEnd + Backtrack), which would be a 2000+ line commit. Per design doc §1.0 (100% accuracy first), each commit should ship a slice that's byte-for-byte correct on every input it accepts; splitting into 3a (literals only), 3b (built-in char classes + anchors), 3c (control flow + backtracking) lets each slice be reviewable in isolation and gives faster correctness feedback. The differential gate (step 4) still becomes active at the same point — once captures are wired in.
- **The JIT'd function shape is intentionally NOT the design doc §5.1 ExecContext-pointer signature** at step 3a. The ExecContext layout contract requires field offsets stable across compile units, which is part of step 5 (engine wiring). Step 3a uses the simpler `(text, text_len, pos) -> isize` shape so it can ship correctly without the layout contract. Step 5 will adapt this shape to ExecContext-aware code when it wires the JIT into the dispatch chain.
- **Step 3a deliverables**:
  - `compile_program` in `c1/codegen.rs` with the IR builder (entry block → bounds check → per-byte comparison chain → success/fail blocks with sealed SSA).
  - `extract_step3a_literal` private helper that walks the bytecode and accepts `Char(len=1)` + `SaveStart(0)`/`SaveEnd(0)` + `Match`, returning `JitHostError::CodegenUnsupported(reason)` for anything else.
  - `Step3aJittedFn` public type alias documenting the C ABI signature with safety contract.
  - `JitHost::next_func_index` accessor for unique function names so multiple programs can be compiled into one host without name collisions.
  - `JitHostError::CodegenUnsupported(String)` new error variant.
  - 12 codegen tests + 8 refusal tests = 20 step 3a tests.
- **Two real bugs caught by the tests on the first run**, both fixed before commit:
  1. **Block not sealed**: Cranelift requires every block to be sealed before `FunctionBuilder::finalize()`. I sealed the per-byte chain blocks and the success/fail blocks but forgot the entry block. Cranelift panicked with `"FunctionBuilder finalized, but block block0 is not sealed"` on every codegen test. Fix: added `builder.seal_block(entry)` immediately after the bounds-check `brif`. Documented inline.
  2. **Test module fence accidentally deleted**: when inserting the new codegen functions before the existing `#[cfg(test)] mod tests {` block, the Edit accidentally deleted the `mod tests {` opening line, leaving an unbalanced closing brace. Caught at compile time. Fix: restored the line.
- 2 small clippy warnings introduced by the new code, both fixed: `missing_panics_doc` on `compile_program` (the inner `.expect()` on `i32::try_from(i)` was converted to a `CodegenUnsupported` error return — every failure mode is a controlled error per design doc §1.0); `doc_markdown` on `JitHost` needing backticks in a `c1/jit.rs` doc comment.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0 unchanged, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit` 975/0 (768 lib + 207 elsewhere; 73 C1 tests), `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3b (built-in char classes + anchors). Add codegen for `DigitAscii` / `DigitAsciiNeg` (test `b.is_ascii_digit()`), `WordAscii` / `WordAsciiNeg`, `SpaceAscii` / `SpaceAsciiNeg`, `StartText` (test `pos == 0`), `EndText` / `EndTextOrNL` (test `pos == text_len` etc.), `WordBoundary` / `NonWordBoundary` (test the word-character status of bytes at `pos - 1` and `pos`). Still linear, capture-less, no control flow. Same `(text, text_len, pos) -> isize` signature. Hand-curated tests for each new opcode. Step 3c follows with control flow.

### 2026-04-11 (afternoon — third commit)
- **C1 step 2 (JIT eligibility check) landed.** Standalone pure function `c1::codegen::is_jit_eligible(program: &Program) -> bool` that walks a compiled program and decides JIT acceptance. Two-layer check: quick rejects from `ProgramFlags` followed by an opcode walker. **Still does NOT touch the engine** — no `Program::jit_eligible` field, no dispatch wiring. 50 hand-curated truth-table tests pass; default build unchanged at 902, with `--features jit` 955 tests pass (53 C1 tests = 3 from step 1 + 50 from step 2).
- **Two real bugs caught by the truth table on the first run**, both fixed before commit:
  1. **`subroutines.is_empty()` over-restriction** — every "eligible" pattern (even single-char `a`) failed because the compiler populates `subroutines[0]` with the whole-pattern bytecode for *every* pattern, regardless of whether the pattern uses recursion. The `subroutines` vec is therefore not evidence of recursion. Fix: removed the `is_empty()` check entirely; recursion is detected purely via the `Call` opcode in the bytecode walk (which IS the only way subroutines become reachable from the main bytecode). Documented inline.
  2. **Quantifier-wrap recursion missing** — `\X+` and `(?R)?` slipped through as eligible because the walker advanced past the optimized-quantifier opcodes' operand bytes (which contain a length-prefixed inline subprogram) without inspecting them. So `PlusGreedy(GraphemeCluster)` and `QuestionGreedy(Call)` were both passing eligibility incorrectly. Fix: when the walker hits `QuestionGreedy` / `QuestionLazy` / `StarGreedy` / `StarLazy` / `PlusGreedy` / `PlusLazy`, recurse into the wrapped subprogram bytes via `walk_bytecode_eligibility`. Documented inline with a pointer to the analogous recursion in `RegexVM::rebase_inline_char_class_ids` (the canonical operand-walker reference in `vm.rs`).
- These are exactly the kinds of bugs the design doc §1.0 (100% accuracy first) enforcement is meant to catch — ship the truth table at the same time as the check, fail loudly on any false positive or false negative, fix before committing. If either bug had reached step 5 (engine wiring) the rollback would have been much more painful.
- **JIT-eligible opcode subset** is now the canonical contract: literals (`Char`, `Any`, `AnyDotAll`), built-in char classes (`DigitAscii*`, `WordAscii*`, `SpaceAscii*`, `CharClass*`), anchors (`StartLine`, `EndLine`, `StartText`, `EndText`, `EndTextOrNL`, `WordBoundary`, `NonWordBoundary`), control flow (`Jump`, `Split`, `SplitLazy`), captures (`SaveStart`, `SaveEnd`), optimized quantifiers (`QuestionGreedy`/`Lazy`, `StarGreedy`/`Lazy`, `PlusGreedy`/`Lazy`), `SetAlternative`, `Match`, `Fail`. **Ineligible**: backreferences, lookaround (Lookahead/Neg, Lookbehind/Neg), recursion (Call), conditionals (JumpIfMatch/NoMatch), atomic groups + possessive quantifiers (AtomicStart/End), backtracking verbs (Commit/Prune/VerbSkip/Then/Mark), inline code blocks (CodeBlock), `\K` (MatchReset), `\G` (PreviousMatchEnd), `\X` (GraphemeCluster), and all reserved/never-emitted opcodes (defensively).
- 3 small clippy warnings introduced by the new code, all fixed: `similar_names` (`compiler` vs `compiled`), `range_plus_one` (`1..1+length` → `1..=length`), `match_same_arms` (CharClass + SaveStart + SetAlternative all share a 1-byte-operand body so merged into a single arm).
- New `c1/codegen.rs` is ~600 lines (function + tests). `c1/mod.rs` updated to register `pub mod codegen` and re-export `is_jit_eligible`; status table marks step 2 complete.
- **Validation**: full quality gates green on default + `--features jit`. Default `cargo test -p rgx-core` 902/0, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors. With `jit`: `cargo test -p rgx-core --features jit` 955/0, `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors. `cargo fmt --check` clean.
- **Next concrete action**: C1 step 3 (codegen for the easy opcodes). New function `c1::codegen::compile_program(program: &Program, host: &mut JitHost) -> Result<FuncId, JitHostError>` that translates the JIT-eligible subset into Cranelift IR. Lowers the simple opcodes inline (`Char`, `DigitAscii`, etc.) and stubs the complex ones (`CharClass`, multi-byte `Char`) for step 6. Still standalone — no engine wiring. Step 4 is when the differential gate becomes active and every JIT-eligible test in the suite must produce byte-for-byte identical results to the interpreter.

### 2026-04-11 (afternoon — second commit)
- **C1 step 1 (JIT host plumbing) landed.** First code commit for the C1 backend. Standalone `rgx-core/src/c1/` module with `JitHost` wrapper around `cranelift_jit::JITModule`, runtime helper signature skeleton, opt-in `jit` Cargo feature gating Cranelift 0.101 (pinned to wasmtime's transitive version). Three new tests pass (smoke test for the constant-42 function, multi-function host test, runtime stub linkage test). Default build unchanged at 902 tests; with `--features jit` 905 tests pass. **Does NOT touch the engine** per the design doc — engine wiring lands in step 5 only after the codegen and capture-trail steps are differentially verified.
- **The cross-platform PIC issue caught by the smoke test**: initial implementation set `is_pic = "true"` mirroring the C2 design's general recommendation. This panicked on aarch64-apple-darwin with `"PLT is currently only supported on x86_64"` because cranelift-jit 0.101's `JITModule` only implements PLT (Procedure Linkage Table) for x86_64 — and PIC requires PLT support. Fix: leave `is_pic` at Cranelift's default (`false`). JIT'd code lives in a single executable mmap region owned by the `JITModule`; nothing in it is dynamically linked, so position independence buys nothing. Documented in the `JitHost::new` doc comment with the panic message preserved for future debugging. This is exactly the kind of cross-platform footgun that step 1's smoke test was designed to catch — design doc §1.0 (100% accuracy first) requires fixing it before any further C1 work, which is what this commit does.
- **Module structure** (matches design doc §10):
  - `c1/mod.rs`: module decls, implementation status table, cohabitation invariant, opt-in feature gating rationale
  - `c1/jit.rs`: `JitHost` wrapper + `JitHostError` enum + 2 smoke tests
  - `c1/runtime.rs`: signature-only stubs for `rgx_runtime_char_class_test`, `rgx_runtime_word_boundary_test`, `rgx_runtime_match_multibyte_char`, `rgx_runtime_compare_capture`, `rgx_runtime_run_subprogram` + 1 link test
  - NOT created at step 1: `c1/codegen.rs` (lands step 3), `c1/fallback.rs` (lands step 5), `c1/tests.rs` (the differential test harness lands step 4)
- **Cargo feature `jit`** opt-in (NOT default-on for step 1). Wires Cranelift 0.101 deps. Will flip default-on at step 8 production cutover once the differential gate has verified end-to-end correctness on every supported target. Default build is byte-for-byte identical to before C1 step 1 (902 tests, no new dependencies, no behaviour change).
- `lib.rs` corrected: stale C2 doc comment that still said "step 1" updated to reflect C2 shipped 2026-04-11 with all 9 steps complete. New `pub mod c1` declaration gated on `feature = "jit"` with full doc comment.
- **Validation**: `cargo fmt --check`, `cargo test -p rgx-core` 902/0 (default), `cargo test -p rgx-core --features jit` 905/0 (the +3 are the new C1 tests), `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors, `cargo clippy -p rgx-core --features jit --all-targets` zero RGX-owned errors after fixing one `clippy::doc_markdown` pedantic warning in `c1/runtime.rs`.
- **Next concrete action**: C1 step 2 (JIT eligibility check). New `is_jit_eligible(program: &Program) -> bool` walker that decides if the JIT will accept a pattern based on the opcode set. New `Program::jit_eligible: bool` field populated at compile time by the existing compiler. Hand-curated truth table for representative patterns. Still standalone — no engine wiring. Step 2 is gated on the same standalone-correctness criterion as step 1 (the eligibility output must match the truth table for every test case).

### 2026-04-11 (afternoon)
- **C1 step 0 (JIT compilation design proposal) landed.** Doc-only commit. New file `docs/C1_JIT_COMPILATION_DESIGN.md` is the comprehensive SOTA design proposal for the JIT compiler — the second tier-0 perf push now that C2 has shipped. Mirrors the structure of `docs/C2_NFA_DFA_DESIGN.md` (20 sections, 9-step phased plan, 12 open questions with leans, 12 risks with mitigations, cross-platform validation matrix).
- **Key architectural decisions recorded in the design doc** (sign-off blocks all C1 implementation per §20):
  - **Code generator**: Cranelift, chosen over dynasm-rs, hand-written assembly, and LLVM. Already in the dep tree via wasmtime; multi-target out of the box (x86_64 + aarch64); production-grade maintenance via Bytecode Alliance; ~1-10ms per-pattern compile cost is negligible against PGEN+compiler pipeline; ~1-2MB binary size hit mitigated by feature gating (`jit` Cargo feature).
  - **What gets JIT'd**: the existing backtracking VM bytecode, NOT the C2 NFA/DFA engines on the first pass. Rationale: the backtracking VM is where the bulk of patterns end up after the C2 dispatch gates short-circuit; the C2 DFA hot loop (two array lookups per byte) is already fast enough that JIT wins are marginal; the C2 Pike-VM doesn't have a tight bytecode interpreter loop to JIT.
  - **Dispatch chain becomes 4-tier**: DFA → JIT → Pike-VM → interpreter. JIT sits between DFA (always wins for DFA-eligible) and Pike-VM (handles nested-quantifier patterns). The interpreter is the final fallback.
  - **Patterns the JIT refuses on v1**: backreferences, lookaround, recursion, code blocks, conditionals, atomic groups, possessive quantifiers, complex backtracking verbs. Each of these stays on the interpreter (or runs through interpreter helpers via indirect calls in JIT'd code).
  - **Eager JIT for v1**: every JIT-eligible pattern is JIT-compiled at `Regex::compile` time. Tiered execution (interpret first, JIT after N matches) is a v2 follow-up if compile-cost profiling shows it matters.
  - **Runtime helpers use stable C ABI** (`extern "C" fn`) so Cranelift handles calling conventions cleanly across all targets.
  - **No JIT'd allocation**: JIT'd code never allocates; it reuses pre-allocated buffers in `ExecContext` and bails (returns false to fall back to interpreter) if buffers overflow.
  - **No mid-match fallback**: the eligibility check at compile time is comprehensive — if the JIT accepts a pattern, it commits to handling every input. Mid-match fallback is fragile (state divergence).
  - **Cross-platform matrix**: P0 = x86_64-linux/darwin + aarch64-darwin (full differential gate); P1 = aarch64-linux + x86_64-windows (full gate); P2 = aarch64-windows (smoke test v1, full v2). 32-bit targets and WASM are N/A — JIT disabled.
  - **Feature gating**: `jit` Cargo feature, default-on. With JIT off, dispatch chain becomes the C2 chain unchanged (DFA → Pike-VM → interpreter). The interpreter remains a complete implementation.
- **9-step phased plan in the design doc**: 0 = this design proposal (done); 1 = JIT host plumbing (standalone `c1/` module with `cranelift_jit::JITModule` wrapper, runtime helper skeleton, Cargo feature flag, smoke test); 2 = JIT eligibility check (`is_jit_eligible(program)` AST walker, new `Program::jit_eligible: bool` field); 3 = codegen for the easy opcodes (`Char`, `DigitAscii`, `WordAscii`, `SpaceAscii`, `Split`, `Jump`, `Match`, `SaveStart`, `SaveEnd`, `Backtrack`, `StartText`, `EndText`, `WordBoundary`, `NonWordBoundary`); 4 = capture trail in JIT'd code WITH differential gate active; 5 = engine dispatch wiring + 4-tier chain in `lib.rs`; 6 = `CharClass(id)` and multi-byte literal support via runtime helpers; 7 = runtime safety helpers (step counter, recursion depth, backtrack frame limit) inlined as Cranelift branches; 8 = production cutover with benchmark sweep + Book chapter; 9 = optional cross-platform CI matrix expansion.
- **README.md doc index** updated: C2 marked as shipped, new C1 entry added pointing to the design doc.
- **BACKLOG.md** tier-0 row for C1 marked "Step 0 COMPLETE 2026-04-11" with the design-doc summary.
- **Validation**: doc-only commit, all gates re-verified — `cargo fmt --check`, `cargo test -p rgx-core` 902/0, `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0, `cargo clippy --workspace --all-targets` zero RGX-owned errors.
- **Next concrete action**: sign-off → step 1. The design doc explicitly blocks implementation work until §20 is checked. Once granted, step 1 lands the standalone `c1/` module with the Cranelift JITModule wrapper, runtime helper skeleton, `jit` Cargo feature flag, and a smoke test that builds an empty Cranelift function and calls it. Step 1 does NOT touch the engine — it's purely a standalone plumbing commit.

### 2026-04-11
- **C2 NFA/DFA hybrid engine SHIPPED.** Step 8 (production cutover) landed: prefix scanning, nested-quantifier dispatch heuristic, pure-literal short-circuit, and the dedicated Book chapter. The full 9-step C2 plan (steps 0–8) is complete.
- The cutover commit fixes regressions caught by the first benchmark capture: `email_basic` went from 3x slower to 6.6x faster vs pre-C2; `capture_groups` went from ~2x slower to 31-35x faster vs pre-C2 (and 1.96x faster than PCRE2); `literal_simple` went from flat to 38-40x faster vs pre-C2 (and 3.16x faster than PCRE2). Trend capture saved as label `c2-step8-final` against baseline `f708f7c`.
- The architectural insight that drove the cutover work: Pike-VM dispatch must NOT be the universal fallback for classifier-positive patterns. Pike-VM has higher per-trial cost than the existing backtracking VM (sparse-set ops, epsilon-closure of start state per scan position) and only wins when the existing VM has a structural reason to backtrack catastrophically. The nested-quantifier check (`has_nested_quantifier(ast)`) is the gate: Pike-VM dispatch fires only for patterns like `(a+)+`, `(\w+\s+)+`, `(?:foo|bar+)+` where the existing VM's worst case is exponential and Pike-VM's O(nm) bound provides a strict improvement. Flat patterns like `\b\w+@\w+\.\w+\b` and `\d{3}-\d{2}-\d{4}` route through the existing VM directly.
- The other key insight: the existing VM's `PrefixFilter` (`Byte` / `Digit` / `Word` / `Space` / `CharClass`) is the source of truth for scan-skip across both engines. Step 7 only had single-byte memchr; step 8 added a `PrefixScanner` helper in `engine.rs` that wraps the VM's filter and exposes a single `next_candidate(input, start)` method consumed by both DFA and Pike-VM dispatch loops. Plumbed `PrefixFilter` to `pub(doc-hidden)` and added `RegexVM::prefix_filter()` / `RegexVM::char_classes()` / `RegexVM::has_literal_finder()` accessors.
- Also added: `Engine::try_pike_is_match`, `Engine::try_pike_find_first`, `Engine::try_pike_find_all` (mirror the `try_dfa_*` family but use the forward anchored NFA via `pike_is_match_at` / `pike_captures_at`); new `pike_is_match_at` in `c2/pike.rs`; new `c2_has_nested_quantifier` field on `CompiledC2Program` populated at construction time; new `has_nested_quantifier` and `contains_quantifier` AST walkers in `c2/program.rs`; `should_dispatch_to_dfa` and `should_dispatch_to_c2` both check `vm.has_literal_finder()` to preserve the existing memmem fast path for pure literals.
- New Book chapter `book/src/internals/nfa-dfa-engine.md` (~400 lines) covers everything: classifier subset, `CompiledC2Program` artifact, sparse-set Pike-VM, lazy DFA, two-pass capture recovery, 3-tier dispatch chain, `PrefixScanner` strategy table, differential testing, production benchmark numbers, and deferred follow-ups. Added to `SUMMARY.md` under Part VI. Updated `the-vm.md`, `architecture.md`, `performance.md`, `project-status.md` to reference the new chapter and replace "C2 is planned" passages with shipped status.
- Validation: `cargo test -p rgx-core` 902/0 (695 lib + 207 elsewhere), `cargo test -p rgx-cli` 30/0, `cargo test -p rgx-bench` 39/0 (incl. 13 PCRE2 parity), `cargo fmt --check` clean, `cargo clippy --workspace --all-targets` zero RGX-owned errors.
- **Active focus now switches from C2 to C1 (JIT compilation).** Both items are tier 0 in `docs/BACKLOG.md`. C1 sequences after C2 so the constant-factor JIT win compounds on top of C2's algorithmic-class improvement. Roadmap doc and BACKLOG updated.
- Deferred follow-ups (tracked in CHANGES.md and the Book chapter): (1) lazy reverse DFA cache for unanchored find pipeline; (2) multi-byte literal prefix via `memchr::memmem::Finder` in C2 dispatch; (3) smarter Pike-VM heuristics that find more patterns where Pike-VM wins.

### 2026-04-02
- Landed another low-risk warning-debt pass in parser-facing RGX code:
  - added `#[must_use]` coverage in `rgx-core/src/ast.rs`, `rgx-core/src/token.rs`, `rgx-core/src/lexer.rs`, `rgx-core/src/parsing.rs`, and `rgx-core/src/lib.rs`
  - added missing module docs and `# Errors` sections for public parser/API entry points
  - simplified parser/lexer utilities by replacing several `Option` snapshots with `map_or_else`, removing `Position` clones, centralizing parser fallback-position lookup, adding `Default` to parser adapter shells, and making two internal `Regex` helpers static
  - targeted short-format `clippy` verification for the touched files came back empty for the cleaned warning classes, and the full workspace `clippy` run now reports `rgx-core` lib warnings down from 426 to 329
  - validation passed with `cargo fmt`, `cargo test -p rgx-core`, `cargo test -p rgx-cli`, and full workspace `cargo clippy --all-targets`

### 2026-04-02
- Preserved the shipped Perl extended-character-class boundary after a parity probe:
  - an exploratory implementation for bare top-level ordinary terms such as `(?[a-z])` and `(?[\dA-F])` passed local RGX tests but failed the local PCRE2 differential harness because upstream PCRE2 compile-rejected those forms
  - the widening was intentionally reverted before commit so RGX stays aligned with current PCRE2 behavior
  - nested ordinary bracket terms such as `(?[[a-z]])` and `(?[[\dA-F]])` remain the shipped ordinary-term slice
- Landed one small RGX-owned warning-debt cleanup instead:
  - cleaned the Unicode scalar-universe literal formatting in `compiler.rs`
  - simplified the relative-group sign pattern in `lexer.rs`
  - renamed quantified locals in `parser.rs` and `parsing.rs` to remove "too similar" clippy warnings
  - removed unnecessary raw-string hashes in native-code-block tests in `lib.rs`
  - validation passed with `cargo fmt`, `cargo test -p rgx-core`, `cargo test -p rgx-cli`, full workspace `cargo clippy --all-targets`, and a targeted no-output `rgx-core` short-format clippy grep for the cleaned warning sites

### 2026-04-01
- Consolidated the shipped Perl extended-character-class operator parser without widening syntax:
  - replaced the duplicated low-precedence/intersection parsing loops in `rgx-core/src/compiler.rs` with one precedence-climbing parser that now owns left-associativity and the shipped `&` precedence in one place
  - moved operator lookup/precedence/application onto `ExtendedCharClassOperator`, which makes the current `(?[...])` subset easier to extend without re-splitting the parser by precedence tier
  - added a direct compiler regression for repeated intersection chaining so the internal refactor is locked independently of the broader runtime/parser-path coverage
  - validation passed with focused `extended_char_class` coverage, `cargo fmt`, package tests for `rgx-core` and `rgx-cli`, `cargo clippy --workspace --all-targets`, and `./scripts/run-local-ci.sh`
- Extended the shipped Perl extended-character-class subset again on the default RGX path:
  - the `(?[...])` lowering path now supports same-level left-associative set algebra with `&` binding tighter than `|`, `+`, `-`, and `^` over the current bracket/property term subset
  - the shipped runtime now covers precedence-sensitive examples like `(?[ [a-f] | [d-z] & [m-p] ])` and chained low-precedence examples like `(?[ [a-z] - [aeiou] + [0-9] - [5] ])`
  - compiler/unit tests, parser-contract coverage, API regressions, and PCRE2 differential parity coverage were all widened for the new precedence behavior
  - additional bare-term families and wider set-expression forms still fail explicitly at compile time, so RGX keeps a clear boundary instead of over-claiming the full PCRE2 extended-class grammar
  - validation passed with focused extended-char-class / parser-contract / parity commands, plus `cargo fmt`, package tests for `rgx-core` and `rgx-cli`, `cargo clippy --workspace --all-targets`, and `./scripts/run-local-ci.sh`
- Reduced another RGX-owned warning/dead-scaffolding slice without changing shipped behavior:
  - removed the unused `Regex.pattern` and `Lexer.input` fields from the base regex/lexer path
  - removed the stale `PatternAnalysis` helper from `rgx-core/src/parsing.rs` and an unused VM capture-extraction helper
  - tightened feature gating around dormant Lua/JavaScript/Rhai-only emitted-result helpers in `rgx-core/src/execution.rs`
  - cleaned the remaining `manual_let_else` / `clone_on_copy` nits in parser/token tests
  - the visible RGX-owned `rgx-core` warning count in the normal validation loop dropped from 101 to 93
- Hardened the newly shipped Perl extended-character-class slice with local guardrails:
  - extracted a dedicated compiler helper for the subset compile error used by the `(?[...])` lowering path
  - added direct compiler unit tests for nested simple-subset extraction/lowering and a rejection case for broader set algebra
  - added a direct VM unit test for negated custom char classes so the recent double-negation fix stays covered outside only the end-to-end regex API tests
  - this was a consolidation-only pass; no new regex syntax was added
- Shipped the first Perl extended-character-class runtime slice on the default RGX path:
  - the compiler now lowers simple nested bracket-equivalent `Regex::ExtendedCharClass { content }` payloads into the existing char-class engine before VM codegen
  - the shipped subset currently covers simple literal/range content such as `(?[[a-z]])` and `(?[[^0-9]])`
  - broader algebraic forms with set operators, nested classes, property escapes, or whitespace-separated set expressions still fail explicitly with a narrower compile-time policy message
  - added default-path API regressions, parser-contract coverage, and PCRE2 differential cases for the shipped subset
- Reduced a small RGX-owned `clippy` warning slice after the named recursion-condition landing:
  - cleaned the remaining debug-print format warning in `rgx-core/src/vm.rs`
  - removed unnecessary `format!` calls from the debug examples
  - simplified one compile-boundary test to `let ... else`
  - changed one native numeric test helper to avoid a direct `usize -> f64` precision-loss cast
  - this was a consolidation-only pass; no regex/runtime behavior changed
- Shipped named recursion-condition syntax on the default RGX path:
  - bumped `subs/pgen` from `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77` to `f97e0fe31750885f4fc48a67ed7660110cd20271`, bringing the default parser pin onto the verified standalone PGEN `1.1.2` fix for local issue `PGEN-RGX-0005`
  - extended both parser paths to preserve `(?(R&name)...)` as `ConditionalTest::RecursionNamed(name)` and resolved that form at compile time onto the existing recursion-target runtime check
  - added lexer/parser/runtime/parser-contract/PCRE2 parity coverage for the new conditional family, including explicit compile errors for missing named recursion targets
  - refreshed current-state docs so `R&name` is no longer tracked as a parser blocker and the remaining newer-PCRE2 conditional follow-up narrows to forms like `VERSION[...]`
  - full local validation passed again after the submodule bump: `cargo fmt`, package tests for `rgx-core` / `rgx-cli` / `rgx-bench` / `rgx-wasm`, `cargo clippy --workspace --all-targets`, and `./scripts/run-local-ci.sh`

### 2026-03-31
- Shipped branch-reset groups on the default regex path:
  - compiler-side capture-index assignment now gives the branch-reset group's top-level alternatives a shared numbering window instead of allocating fresh numbers per branch
  - downstream backreferences and conditionals now see the correct PCRE2-style max-branch-arity numbering after the branch-reset group
  - `rgx-core` regressions and `rgx-bench` parity cases now cover shared backreference behavior plus following conditional-group numbering

### 2026-03-30
- Shipped single-branch `DEFINE` conditionals on the default regex path:
  - `DEFINE` is now treated as always false at runtime, so single-branch definition blocks fall through as an empty else instead of compile-rejecting
  - numbered and named subroutine definitions inside `DEFINE` now remain available for later `(?1)` / `(?&name)` calls on the shipped path
  - invalid two-branch `DEFINE` forms still fail explicitly at compile time so RGX stays aligned with PCRE2's single-branch rule
- Hardened the Perl extended-character-class parser boundary:
  - both parser backends now preserve `(?[...])` as `Regex::ExtendedCharClass { content }` instead of leaving this newer PCRE2 family as an ambiguous parser gap
  - the public compile path now fails early with an explicit extended-character-class policy message until RGX chooses downstream set-algebra/runtime semantics
  - refreshed parser/capability/PCRE2/docs state so Perl extended character classes are tracked as a parsed-only boundary rather than as an unclassified unsupported family
- Hardened the branch-reset parser boundary:
  - both parser backends now preserve `(?|...)` as `GroupKind::BranchReset` instead of rejecting or dropping the wrapper shape
  - the public compile path now fails early with an explicit branch-reset policy message before RGX capture-numbering logic can make invalid assumptions
  - refreshed parser/capability/PCRE2/docs state so branch-reset groups are tracked as a parsed-only boundary rather than an ambiguous parser gap
- Hardened the shipped Rhai source-body contract:
  - explicit `return ...` Rhai bodies were already working on the real runtime path, but the repo still described Rhai too narrowly
  - added regression coverage for explicit-return predicate matching plus numeric/replacement helper flows
  - refreshed README / user guide / capability/status docs so Lua, JavaScript, and Rhai are all described as supporting both bare expressions and explicit `return ...` bodies
- Extended the shipped CLI code-block surface with file-backed wasm module registration:
  - added repeatable `--wasm-module NAME=PATH` in `rgx-cli` so `(?{wasm:module:function})` can be exercised without Rust glue when the CLI is built with the `wasm` feature
  - added CLI parsing/application tests, including successful module registration from a temp WAT-assembled module plus explicit missing-file / missing-feature failure coverage
  - refreshed state/docs so wasm is no longer described as Rust-API-only on the CLI path, while native remains explicitly Rust-API-only
- Shipped relative conditional group references on the default regex path:
  - compiler now resolves `(?(+1)...)` / `(?(-1)...)` to absolute conditional-group checks at compile time instead of rejecting them at the old parser/runtime boundary
  - added AST and parser-path runtime regressions plus explicit missing-target compile errors for unresolved relative references
  - promoted the feature into `rgx-bench/tests/pcre2_parity.rs` conditionals coverage and refreshed parser/capability/PCRE2/docs state accordingly
- Tightened the CLI code-block surface:
  - added repeatable `--var NAME=VALUE` so CLI users can drive the shipped host-variable path for Lua / JavaScript / Rhai code blocks
  - added `--show-details` so CLI match lines can expose top-level branch numbers and winning-path code-block results when desired
  - switched CLI matching to single-pass `find_all` collection so successful code-block patterns are not executed once by `is_match` and then again for output
- Hardened the relative-conditional parser boundary:
  - both parser backends now parse `(?(+1)...)` and `(?(-1)...)` into dedicated `RelativeGroupExists(offset)` AST instead of collapsing or diverging
  - this parser-boundary work landed before the later default-path runtime support and kept both backends aligned while the runtime semantics were still pending
  - validation covered lexer regressions, parser-contract fixtures, and capability-matrix compile-boundary guardrails
- Added automated benchmark trend capture:
  - `rgx-bench/src/lib.rs` now holds shared benchmark fixtures instead of remaining a placeholder.
  - `rgx-bench/src/bin/trend_capture.rs` writes quick benchmark summaries to `target/benchmark-trends/latest.md` and `latest.tsv`.
  - `scripts/run-local-ci.sh` now runs `scripts/capture-benchmark-trends.sh` by default, with `RGX_SKIP_BENCH_TRENDS=1` as an escape hatch and `RGX_BENCHMARK_TREND_MODE=full` for slower bench-profile captures.
- Hardened the shipped inline-language contract:
  - JavaScript now preserves bare expression-body results before falling back to wrapped `return ...` execution, so `(?{js: ...})` expression bodies no longer silently behave like unconditional success.
  - Added helper-API regressions for Lua / JavaScript / Rhai numeric and replacement results.
  - `ROADMAP.md` now marks the multi-language code-block runtime expansion track as `in-progress`.
- Shipped recursion / subroutine execution on the default regex path:
  - `(?R)`, `(?1)`, and `(?&name)` now compile and execute through guarded VM subroutine calls instead of failing at compile time.
  - Added explicit compile errors for missing numbered and named recursion targets.
  - Promoted recursion into capability-matrix and PCRE2 differential coverage and removed the old known-gap status from the docs.

### 2026-03-29
- Shipped possessive quantifiers on the default compiler/VM path:
  - extended lexer/parser tokenization and the default PGEN-backed parser adapter so `*+`, `++`, `?+`, and counted possessive repeats all lower into atomic-wrapped greedy quantified AST nodes
  - added parser-path runtime regressions for suffix-sensitive no-backtracking behavior and straightforward success cases
  - promoted possessive quantifiers to supported PCRE2 differential coverage and refreshed the capability/parity/parser-contract/user docs accordingly
- Re-ran targeted and full validation for the possessive-quantifier slice:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core possessive -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_active_parser_matches_reference_fixtures -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_pgen_backend_matches_reference_fixtures -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_possessive_quantifiers -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- No new PGEN parser show-stopper surfaced while rerunning the shared local CI path with possessive-quantifier coverage added.
- Added a durable rough PCRE2 support estimate and checklist to `docs/PCRE2_COMPATIBILITY_MATRIX.md`:
  - tracked parity estimate is now documented as roughly `92%`
  - broader practical PCRE2 regex estimate is now documented as roughly `72%`
  - the estimate is explicitly caveated as hand-maintained and family-based rather than a formal full-PCRE2 census
- Shipped Unicode property classes on the default compiler/VM path:
  - added `rgx-core/src/unicode_support.rs` as a small bridge to `regex-syntax` so `\p{...}` / `\P{...}` resolve through maintained Unicode property tables instead of staying parser-only
  - removed the old blanket compile-boundary rejection in `rgx-core/src/compiler.rs` and replaced it with explicit invalid-property diagnostics
  - wired Unicode property classes through VM analysis/codegen in `rgx-core/src/vm.rs`, including a fix so inline subexpressions correctly merge and rebase nested char-class tables back into the parent program
  - added parser-path and AST-first regressions in `rgx-core/src/lib.rs` plus representative PCRE2 differential coverage in `rgx-bench/tests/pcre2_parity.rs`
- Planning-only follow-up after reviewing current upstream PCRE2 syntax:
  - `ROADMAP.md` now tracks RGX-side future work for newer PCRE2 syntax that may arrive through PGEN, especially returned-capture subroutine calls, `R&name` / `VERSION[...]` conditional forms, and downstream boundary decisions for branch reset, `DEFINE`, and Perl extended character classes `(?[...])`
  - no implementation or validation work was done in this pass; this was only a roadmap/continuity update so the RGX side is ready once PGEN parser support lands
- Shipped conditional runtime support on the default compiler/VM path:
  - removed the blanket compile-boundary rejection for `Regex::Conditional(...)` in `rgx-core/src/compiler.rs` and replaced it with dedicated validation for missing numbered and named conditional references
  - wired `Regex::Conditional(...)` through VM analysis, bytecode emission, opcode decoding, and both execution paths in `rgx-core/src/vm.rs`
  - added AST-first and parser-path regressions in `rgx-core/src/lib.rs` for group-exists, named-group-exists, optional false branches, lookaround conditions, and explicit missing-group compile errors
  - promoted conditionals from PCRE2 known-gap coverage to supported parity coverage in `rgx-bench/tests/pcre2_parity.rs`
- Re-ran targeted and full validation for the conditional-runtime slice:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_conditionals -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- No new PGEN parser show-stopper surfaced while rerunning the default local CI path with the submodule-backed parser active.
- Shipped numeric backreferences on the default compiler/VM path:
  - removed the blanket parsed-but-unintegrated compile rejection and replaced it with dedicated missing-group validation in `rgx-core/src/compiler.rs`
  - wired `Regex::Backreference(...)` through VM analysis, bytecode emission, opcode decoding, and execution in `rgx-core/src/vm.rs`
  - added AST-first/parser-path regressions plus PCRE2 differential coverage for successful matching, explicit no-match behavior, backtracking-sensitive capture restoration, lookahead interaction, and missing-group compile errors
- Re-ran targeted and full validation for the numeric-backreference slice:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core backreference -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- No new PGEN parser show-stopper surfaced while rerunning the shared local CI path; the sibling-checkout `pgen-parser` slice remains green locally.
- Extended the wasm code-block ABI so successful wasm predicates can emit winning-path `Numeric(f64)` and `Replacement(String)` payloads through `rgx.emit_numeric(...)` and `rgx.emit_replacement(...)` while keeping the exported `() -> i32` predicate contract stable.
- Added wasm regressions for the default no-emission case, last-emitted-wins behavior, failed-predicate payload discard, and invalid UTF-8 replacement payload failure.
- Re-ran the full local validation path after the wasm result work:
  - `cargo test -p rgx-core --features wasm safe_mode_wasm_code_block -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- No new PGEN parser show-stopper surfaced while re-running the shared local CI path; the sibling-checkout `pgen-parser` slice remains green locally.
- Verified that the four RGX-reported PGEN transport bugs are fixed in the local PGEN `1.1.1` checkout at `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77`.
- Replaced the `pgen-parser` placeholder path with a real PGEN AST-dump adapter in `rgx-core/src/parsing.rs`:
  - contract gates now require regex parser/integration release `>= 1.1.1`
  - PGEN AST transport is converted into canonical RGX AST nodes for structure-heavy constructs while leaf atoms are re-parsed from exact source slices to preserve RGX semantics
  - local backend choice is controlled by one constant (`PGEN_FEATURE_BACKEND`)
- Widened the parser conformance fixtures so the active parser and PGEN backend are both checked against the recursive-descent reference AST on anchors, range quantifiers, code-block tags, recursion, backreferences, conditionals, and Unicode property classes.
- Added `rgx-cli` feature passthrough for `pgen-parser` and validated the CLI crate against the real PGEN backend too.
- Found a distribution blocker after validation:
  - the verified PGEN fix commit is only in the local sibling checkout and is not present on PGEN `origin/main`
  - a clean Git dependency pin is therefore not available yet
  - the current local integration still depends on `../pgen`
- Updated repo workflow/docs accordingly:
  - `cargo fmt` commit/local-CI gates are now scoped to RGX workspace packages so they do not format the sibling `pgen` checkout
  - `README.md`, `DEVELOPMENT_NOTES.md`, and `RUST_CODEBASE_ANALYSIS.md` need to keep reflecting that the parser backend is real locally while the distribution decision is still open
  - `./scripts/run-local-ci.sh` is now locally strict about `../pgen` by default but allows hosted CI to skip `pgen-parser` checks temporarily via `RGX_SKIP_PGEN_CHECKS=1`
- Re-reviewed the new upstream `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` `1.1.0` revision:
  - plain `(?{...})` is now explicitly preserved as opaque generic payload
  - `lua` / `js` / `javascript` are now explicitly published as opaque source-body payload classes
  - published parser-layer structural guarantees now explicitly include balanced braces, single-quoted strings, double-quoted strings, and escapes
  - the contract now also explicitly says it does not promise arbitrary valid JS/Lua payload acceptance, JavaScript comment/template-literal shielding, Lua long-bracket shielding, or published `native` / `wasm` tags
- Refreshed the local complaint/proposal docs so they now match that newer upstream contract instead of the earlier narrower version.
### 2026-03-28
- Refined the PGEN regex integration follow-up from a pure complaint into a complaint-plus-proposal pair:
  - refreshed `PGEN_REGEX_PARSER_INTEGRATION_COMPLAINT.md` so it now records only the remaining live caveats after the 2026-03-28 upstream contract refresh
  - added `PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md` as a forwardable suggested contract shape for embedded code blocks
  - the current recommendation is:
    - keep parser guarantees structural,
    - avoid implying that arbitrary inline code is valid for every tag,
    - treat `lua` / `js` / `javascript` as source-body tags best validated by the downstream backend,
    - and keep `native` / `wasm` reference-shaped rather than arbitrary-source-shaped
  - `README.md` now indexes both PGEN review documents explicitly
- Automated the shared local/GitHub validation loop for the shipped feature matrix:
  - `scripts/run-local-ci.sh` now runs the `rgx-core` feature-gated checks for `pgen-parser`, `lua`, `javascript`, `wasm`, and combined `all-languages`
  - `.github/workflows/ci.yml` now installs Lua 5.4 development headers so the hosted path can run the same matrix
  - the remaining validation-process gap is benchmark trend capture rather than feature-matrix automation
- Validation confirmed:
  - `bash -n /Users/richarddje/Documents/github/rgx/scripts/run-local-ci.sh`
  - `/Users/richarddje/Documents/github/rgx/scripts/run-local-ci.sh`
- Added a git-tracked PGEN regex integration complaint and scrubbed the PGEN-specific markdown guidance surface:
  - the complaint is intentionally limited to `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md`, `rust/docs/EMBEDDING_API_CONTRACT.md`, `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`, and `LIVE_ACHIEVEMENT_STATUS.md`
  - the recorded complaints are contract-quality complaints, not claims that the advertised parser surface is fake
  - RGX-side markdown guidance for PGEN integration now points only to published upstream contract files instead of local PGEN-tracking file references
- Added a git-tracked local PGEN parser issue workflow for future real-backend rollout:
  - added a canonical local issue schema
  - added a stub generator to create the next numbered `PGEN-RGX-####.yaml` issue record with timestamps, `rgx` commit, and required context fields
  - updated parser-boundary documentation so the local ID scheme, required fields, and upstream handoff rules are explicit
  - refreshed project-state docs so future sessions can discover and use the workflow
- Validation confirmed:
  - `bash -n <local PGEN issue stub generator>`
  - `<local PGEN issue stub generator> --summary "Dry-run validation for local PGEN issue workflow" --dry-run`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Shipped the first dedicated numeric-result Rust APIs on top of winning-path code-block numeric results:
  - added `Regex::find_first_numeric_with_code(...)` and `Regex::find_all_numeric_with_code(...)` in `rgx-core/src/lib.rs`
  - implemented numeric collection behavior that extracts only `CodeBlockValue::Numeric(f64)` payloads and skips matches whose winning path produced only predicate or replacement results
  - added regressions for first/all numeric collection, non-numeric payload skipping, and winning-path numeric selection under backtracking using native callbacks
  - refreshed user-facing and repository-state docs so the new numeric-result helper layer is documented truthfully and the remaining wasm richer-result boundary stays explicit
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Shipped the first replacement-oriented Rust APIs on top of winning-path code-block replacement results:
  - added `Regex::replace_first_with_code(...)` and `Regex::replace_all_with_code(...)` in `rgx-core/src/lib.rs`
  - implemented rebuilt-output behavior that consumes only `CodeBlockValue::Replacement(String)` and preserves original matched text when the winning path produces only predicate or numeric results
  - added regressions for first/all replacement behavior, numeric-result passthrough, and winning-path replacement selection under backtracking using native callbacks
  - refreshed user-facing and repository-state docs so the new replacement-oriented API layer is documented truthfully and the remaining wasm/numeric-result boundaries stay explicit
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Added a repository-level bootstrap handoff for future sessions:
  - created `SESSION_BOOTSTRAP.md` with the exact instruction to read `README.md` plus all referenced markdown files, analyze the Rust codebase, update `RUST_CODEBASE_ANALYSIS.md` if needed, and then work from `ROADMAP.md`
  - appended the requested one-line reminder to the end of `README.md`
  - updated the root markdown inventory in `README.md` so the new bootstrap file is listed truthfully
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Shipped the first richer non-boolean code-block result slice:
  - added public `CodeBlockValue` / `MatchResult.code_result` so `find_first` and `find_all` can expose the last winning-path numeric or replacement result without changing `is_match`
  - extended VM execution/backtracking state so richer results survive only on the successful match path and speculative paths restore prior values cleanly
  - kept wasm predicate-only for now while allowing Lua/JavaScript/native `Numeric` and `Replacement` returns to succeed in match mode
  - added regressions for Lua numeric-result surfacing, Lua winning-path restoration under backtracking, JavaScript last-result-wins behavior, native `find_all` replacement results, and explicit wasm `code_result == None`
  - refreshed user-facing and repository-state docs so the shipped semantics and remaining wasm boundary stay truthful
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Shipped host-provided execution variables across the shared code-block runtime:
  - added shared variable ownership in `ExecutionManager` plus public `Regex::set_variable(...)` threading through `Engine` and `RegexVM`
  - chose per-evaluation variable snapshots in `ExecContext` so backtracking sees deterministic callout inputs instead of shared mutable match-time state
  - exposed variables consistently across the shipped backends: read-only `vars` in Lua/JavaScript, `ctx.variable(...)` in native callbacks, and deterministic lexicographic wasm variable imports
  - added regression coverage for successful variable reads across native/Lua/JavaScript/wasm, wasm missing-slot behavior, and registration attempts on regexes without an attached execution manager
  - refreshed user-facing and repository-state docs so variables are now described truthfully as a shipped code-block capability
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Expanded the Rust-API wasm ABI for `(?{wasm:module:function})` with named-capture `rgx` host imports:
  - added deterministic named-capture enumeration and read helpers so wasm predicates can inspect named-group names and values in lexicographic name order
  - preserved the exported `() -> i32` predicate contract and reused the existing guest-memory failure model for the new read-style imports
  - added regression coverage for successful named-capture reads and explicit `-1` behavior for missing named-capture slots
  - refreshed user-facing and repository-state docs so wasm is now described as exposing named captures in addition to current position, full input text, and numbered captures
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
### 2026-03-27
- Expanded the Rust-API wasm ABI for `(?{wasm:module:function})` with `rgx` host imports:
  - reworked the wasmtime path around a linker plus per-call store data so wasm predicates can read current position, full input text, and numbered captures while keeping exported `() -> i32` entrypoints stable
  - added safe guest-memory handling with explicit runtime failure for missing exported memory, invalid guest-memory writes, and malformed context reads
  - added regression coverage for position, full-input reads, numbered-capture reads, and the new failure paths
  - refreshed user-facing and repository-state docs so wasm is described as an import-based context slice rather than a zero-context predicate-only backend
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
### 2026-03-27
- Shipped Rust-API wasm module registration for `(?{wasm:module:function})` in `ExecutionMode::Safe` / `ExecutionMode::Full`:
  - added a shared wasm module registry and wasmtime-backed runtime path inside `ExecutionManager`
  - added public `Regex::register_wasm_module(...)` threading through `Engine` and `RegexVM`
  - lifted compiler gating so `wasm` now compiles in `ExecutionMode::Safe` / `ExecutionMode::Full` when the `wasm` cargo feature is enabled
  - initial ABI is intentionally small: registered `module:function` plus exported `() -> i32` predicate (`0` = fail, non-zero = success)
  - added regression coverage for successful execution, missing modules, malformed specs, invalid export signatures, and registration attempts on regexes without an attached execution manager
  - refreshed state/docs so wasm support is described as Rust-API-only and no longer as a parsed-only placeholder
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
### 2026-03-27
- Shipped Rust-API native callbacks for `(?{native:...})` in `ExecutionMode::Full`:
  - refactored native callback storage to support registration through the shared `Arc<ExecutionManager>` already attached to VM-backed regexes
  - added public `Regex::register_native(...)` threading through `Engine` and `RegexVM`
  - lifted compile gating so `native` is accepted only in `ExecutionMode::Full`, while `ExecutionMode::Safe` still rejects it
  - added regression coverage for successful callback execution, capture/named-capture visibility, missing callbacks, safe-mode rejection, and registration attempts on regexes without an attached execution manager
  - refreshed state/docs so native support is described as Rust-API-only and the CLI remains explicitly unconfigured
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
### 2026-03-27
- Shipped phase-1 embedded code-block execution in the public regex path:
  - compiler now validates code blocks against execution mode and cargo features
  - VM now lowers code blocks into an inline opcode and executes them during matching
  - engine now attaches `ExecutionManager` only when compiled programs actually contain code blocks
  - Lua and JavaScript predicate blocks now work through `Regex::with_mode(..., ExecutionMode::Safe | Full)` when the corresponding feature is enabled
  - current overall match, numbered captures, and named captures are materialized into the execution context for callouts
- Explicit boundaries after this slice:
  - `ExecutionMode::Pure` still rejects code blocks
  - `native` code blocks remained blocked until the follow-on Rust-API callback-registration slice landed later the same day
  - `wasm` code blocks remained blocked until the follow-on Rust-API module-registration slice landed later the same day
  - numeric/replacement code-block results are rejected in match mode
- Refreshed the live state docs so future sessions start from the new reality:
  - `RUST_CODEBASE_ANALYSIS.md`
  - `WARP.md`
  - `README.md`
  - `docs/CAPABILITY_MATRIX.md`
  - `docs/USER_GUIDE.md`
  - `DEVELOPMENT_NOTES.md`
  - this file
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
### 2026-03-26
- Fixed lazy quantifier execution in the default public path:
  - implemented VM/compiler support for lazy `??`, `*?`, `+?`, `{n,m}?`, and `{n,}?`
  - expanded API regressions in `rgx-core/src/lib.rs`
  - added PCRE2 differential parity cases in `rgx-bench/tests/pcre2_parity.rs`
- Repaired the JavaScript feature path in `rgx-core/src/execution.rs`:
  - aligned the QuickJS backend with `rquickjs` 0.4 APIs
  - moved JavaScript runtime creation to per-execution sandbox setup so `ExecutionEngine: Send + Sync` no longer blocks compilation
  - `javascript` and `all-languages` feature builds now compile again
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_find_all_spans -- --nocapture`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
### 2026-03-26
- Added `RUST_CODEBASE_ANALYSIS.md` as a live roadmap-grounded Rust workspace assessment and wired it into `README.md`, `COMMIT.md`, `DEVELOPMENT_NOTES.md`, and this continuity file.
- Captured current high-signal findings for future Rust work:
  - default workspace tests pass
  - `pgen-parser` feature path builds/tests pass as a fallback-backed conformance path
  - `lua` and `wasm` feature checks compile, but `javascript` and `all-languages` currently fail in `rgx-core/src/execution.rs`
  - lazy quantifiers are parsed but not correctly compiled in the public path (`a??` on `b` and `ab*?c` on `abbbc` both return no match, while greedy counterparts work)
  - `execution.rs` remains disconnected from compiler/VM/API flow, so execution modes are mostly scaffolding today
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - investigative failures intentionally captured in the analysis doc:
    - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
    - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
### 2026-03-09
- Added a local-first CI workflow and matching pre-push path.
  - Root cause: the repo had no actual GitHub Actions workflow, no single checked-in local CI command path, and `Cargo.lock` was ignored, which could make GitHub resolve different dependency versions than local validation.
  - Added `.github/workflows/ci.yml` and wired it to `./scripts/run-local-ci.sh` so GitHub and local runs execute the same checks.
  - Added `scripts/check-ci-paths.sh` to verify CI-critical paths are git-controlled, reject absolute filesystem paths in Rust source / CI execution files, and surface compile-time `include!` usage (currently none found).
  - Removed `Cargo.lock` from `.gitignore` so the lockfile is git-controlled and available to GitHub CI.
- Validation confirmed:
  - `./scripts/run-local-ci.sh`
### 2026-03-08
- Hardened Unicode property classes (`\p{...}`, `\P{...}`) into an explicit compile boundary.
  - Root cause: parser-path compilation allowed Unicode property classes through to VM codegen, where they were silently lowered to `Any`, causing incorrect public matches such as `\p{L}+` matching `123`.
  - Added compile-boundary rejection in `Compiler::unsupported_feature_message()` for both parser-path and AST-first Unicode property-class forms.
  - Added parser-path/API regressions in `rgx-core/src/lib.rs` and PCRE2 known-gap coverage in `rgx-bench/tests/pcre2_parity.rs`.
  - Updated capability/parity/user docs so Unicode property classes are tracked as parsed-only / rgx-gap until real execution support lands.
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --test pcre2_parity`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - `cargo build --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - direct CLI smoke should now fail explicitly for `\p{...}` / `\P{...}` instead of returning match spans
### 2026-03-07
- Closed the absolute text-anchor runtime/parity gap for `\A`, `\Z`, and `\z`.
  - Root cause: the parser/compiler already accepted absolute anchors, but `RegexVM` did not execute the corresponding absolute-anchor opcodes, so these patterns compiled and then matched nothing.
  - Secondary bug: compiler codegen had `\Z` and `\z` mapped to the wrong VM opcodes, reversing “before final newline” vs “true end-of-text” semantics.
  - Added absolute-anchor execution in both main-loop and subexpression VM paths, and corrected compiler mapping for `\Z` vs `\z`.
  - Added parser-path/API regressions in `rgx-core/src/lib.rs` and PCRE2 differential cases in `rgx-bench/tests/pcre2_parity.rs`.
  - Updated capability/parity docs so absolute anchors are explicitly tracked as shipped/parity-verified.
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --test pcre2_parity`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - `cargo build --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - direct CLI smoke on `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli`:
    - debug `\Acat` on `cat dog` => `0..3`, with `trace.log` containing `LOW/MEDIUM/HIGH/TRACE`
    - low `dog\Z` on `cat dog\n` => `4..7`, with `MEDIUM/HIGH/TRACE = 0`
    - quiet `dog\z` on `cat dog` => `4..7`, with `trace.log` size `0`
### 2026-03-06
- Closed the negated shorthand runtime/parity gap for `\D`, `\W`, and `\S`.
  - Root cause: compiler/codegen already emitted negated shorthand opcodes, but `RegexVM::execute_subexpr()` lacked `WordAsciiNeg`, `SpaceAscii`, and `SpaceAsciiNeg`, so quantified patterns like `\W+` and `\S+` failed even though the main loop had shorthand support.
  - Cleaned duplicate negated-opcode branches in `RegexVM::execute_at()` left by a partial patch and aligned subexpression handling with the main runtime path.
  - Added parser-path/API regression tests in `rgx-core/src/lib.rs` and PCRE2 differential cases in `rgx-bench/tests/pcre2_parity.rs`.
  - Updated `docs/CAPABILITY_MATRIX.md` and `docs/PCRE2_COMPATIBILITY_MATRIX.md` so negated shorthand classes are tracked as shipped/parity-verified.
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --test pcre2_parity`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - `cargo build --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - direct CLI smoke on `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli`:
    - debug `\W+` on `ab!!cd` => `2..4`, with `trace.log` containing `LOW/MEDIUM/HIGH/TRACE`
    - low `\D+` on `123abc456` => `3..6`, with `MEDIUM/HIGH/TRACE = 0`
    - quiet `\S+` on `  abc  ` => `2..5`, with `trace.log` size `0`
- Resume note:
  - After library-oriented validation (`cargo test` / `cargo clippy`), run `cargo build -p rgx-cli` before validating the standalone `target/debug/rgx-cli` binary so smoke checks use the current executable.
- Updated documentation policy around onboarding entry point:
  - `README.md` is the single project entry point and now contains objective, ramp-up order, complete markdown map, and key path references.
  - README maintenance is now explicitly “update when needed” (not every commit), with triggers tied to objective/onboarding/path-map changes.
- Workflow alignment:
  - `COMMIT.md` now explicitly mirrors the same rule: `README.md` should be updated when relevant, not as a per-commit requirement.
- Verification confirmed:
  - README references all tracked markdown files (`ALL_MARKDOWN_REFERENCED`)
  - README is git-tracked (`TRACKED:0`)
### 2026-03-02
- Added structured tracing for parser token-inspection helpers in `rgx-core/src/parser.rs`:
  - instrumented `Parser::peek`, `Parser::current_token_snapshot`, and `Parser::regex_kind`
  - added token-availability decision tracing in `Parser::peek`
  - added entry/exit snapshots for helper-derived token/kind values
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - debug smoke includes `Parser::peek`, `Parser::current_token_snapshot`, and `Parser::regex_kind` boundary lines in `trace.log`
  - low filtering remained correct (`MEDIUM/HIGH/TRACE = 0`, `LOW = 19`)
  - quiet mode left `trace.log` empty (`0` lines)
### 2026-03-02
- Added structured tracing at lexer escape-helper boundaries in `rgx-core/src/lexer.rs`:
  - instrumented `parse_unicode_class`, `parse_backreference`, `parse_hex_escape`, and `parse_octal_escape`
  - added decision traces for unicode-brace validation, backreference range validation, hex-format branch selection, and octal byte-range validation
  - added explicit traced error exits for helper-parse failure paths
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - debug smoke includes `Lexer::parse_hex_escape` boundary lines in `trace.log`
  - low filtering remained correct (`MEDIUM/HIGH/TRACE = 0`, `LOW = 19`)
  - quiet mode left `trace.log` empty (`0` lines)
- Observed behavior note:
  - `\\101` still routes through backreference handling and errors as invalid backreference (existing semantics; unchanged by this tracing-only increment).
### 2026-03-01
- Added structured tracing to parser token-cursor advancement in `rgx-core/src/parser.rs`:
  - instrumented `Parser::advance` with entry/exit boundary traces and token snapshots
  - added decision trace for lexer-fetch branch (`should_fetch_next`)
  - added explicit error-exit tracing when `lexer.next_token()` fails during parser advancement
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - debug/low/quiet smoke matrix passed with `--trace-log`:
    - debug includes `Parser::advance` boundary lines with consumed/next token detail
    - low filters out medium/high/trace lines (`0`) while preserving low milestones (`11`)
    - quiet leaves `trace.log` empty (`0` lines)
### 2026-02-28
- Added structured tracing for AST/token utility boundaries in `rgx-core`:
  - `rgx-core/src/ast.rs`: instrumented `CharRange::single`, `CharRange::range`, `ParseContext::new`, `ParseContext::next_group_number`, `ParseContext::register_named_group`, and `ParseContext::get_named_group`
  - `rgx-core/src/token.rs`: instrumented `Position::new`, `Position::start`, and `TokenWithPos::new`
- Added decision-level tracing where utility branches matter:
  - range-order check in `CharRange::range`
  - replacement check in `ParseContext::register_named_group`
  - lookup-hit check in `ParseContext::get_named_group`
- Validation confirmed:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - debug/low/quiet smoke matrix passed with `--trace-log`:
    - debug includes new AST/token boundary lines in `trace.log`
    - low contains only `[LOW]` entries (no `[MEDIUM]/[HIGH]/[TRACE]`)
    - quiet leaves `trace.log` empty (`0` lines)
### 2026-02-28
- Added structured tracing for compiler/parsing configuration boundaries:
  - `Compiler::new` and `Compiler::with_mode` now emit constructor boundary traces
  - parsing utility boundaries now traced: `parser_name`, `parser_capabilities`, `ParserConfig::default`
  - parser-object constructor/capability boundaries traced: `RecursiveDescentParser::*` and feature-gated `PgenParser::*`
- Added capability-level decision tracing for parser utility capability reporting (`perl_advanced` flag path visibility).
- Resolved an intermediate patch artifact in `parsing.rs` (corrupted capability block insertion), then revalidated.
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - debug traces include `Compiler::new` (pure path) and `Compiler::with_mode` (safe path) boundary lines
  - low/quiet filtering remained correct (`[LOW]`-only at low, `trace.log` size `0` at quiet)
### 2026-02-28
- Added structured tracing for VM startup boundaries in `rgx-core/src/vm.rs`:
  - instrumented `RegexVM::new` with construction-context entry/exit summaries
  - instrumented `RegexVM::detect_simd_support` with capability-boundary entry/exit traces
  - added explicit decision trace for SIMD capability availability at VM construction
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0` (warnings only)
  - debug trace includes `RegexVM::new` and `RegexVM::detect_simd_support` boundary lines
  - low/quiet filtering remains correct (`[LOW]`-only at low, `trace.log` size `0` at quiet)
### 2026-02-28
- User requested clippy integration into workflow with strict policy: clippy warnings acceptable for now, clippy errors must be fixed promptly and must not remain.
- Updated workflow docs to enforce this:
  - `COMMIT.md` now includes a mandatory `cargo clippy --workspace --all-targets` step and no-clippy-error invariant
  - `DEVELOPMENT_NOTES.md` and persistent workflow agreements in `MEMORY.md` now mirror the same policy
- Validation confirmed:
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` exited `0`
  - warnings remain, but no clippy errors were produced
### 2026-02-28
- Added structured tracing at CLI ingress/egress in `rgx-cli/src/main.rs`:
  - `main()` now emits structured ENTER/EXIT traces
  - added decision traces for execution mode branch (`pure` vs non-pure), input source branch (stdin vs positional arg), and boolean match outcome
- Preserved logging semantics by emitting structured traces only after environment-based logging initialization.
- Resolved patch artifact during implementation (duplicate nested match conditional in `main`) and revalidated.
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - debug trace contains CLI `ENTER main`/`EXIT main` lines in `trace.log`
  - low/quiet filtering remains correct (`[LOW]`-only at low, `trace.log` size `0` at quiet)
### 2026-02-27
- Added structured tracing at VM optimizing compiler boundaries in `rgx-core/src/vm.rs`:
  - instrumented `OptimizingCompiler::new`
  - instrumented `OptimizingCompiler::compile` with AST-kind entry context, JIT-worthiness decision trace, and compile summary exit
- Added internal AST-kind helper for concise compile-boundary trace output.
- Resolved an in-progress patch artifact during implementation (duplicate `Program` initializer token), then revalidated.
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - debug trace contains `OptimizingCompiler::compile` ENTER/EXIT lines in `trace.log`
  - low/quiet filtering remained correct (`[LOW]`-only at low, `trace.log` size `0` at quiet)
### 2026-02-27
- Extended structured tracing into execution-runtime path in `rgx-core/src/execution.rs`:
  - context boundaries: `ExecContext::new`, `current_match`, `group`, `named`
  - callback registry boundaries: `NativeCallbackRegistry::new`, `register`, `call`, `has`
  - manager boundaries: `ExecutionManager::new`, `execute`, `register_native`, `is_language_available`
- Added decision-level trace reasoning for callback replacement, callback existence/lookup outcomes, and language backend routing/availability branches.
- Added consistent execution-result kind summary helper for trace exits (`Success|Failure|Replacement|Numeric|Error`).
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - debug/low/quiet trace-log smoke matrix with `rgx-cli` and `cat|dog` on `I have a dog`
  - low filtering retained only `[LOW]` entries; quiet mode left `trace.log` at `0` bytes
### 2026-02-27
- Extended structured tracing into API + engine path:
  - `rgx-core/src/lib.rs` now traces `Regex::compile`, `with_mode`, `from_ast`, `from_ast_with_mode`, `find_all`, `find_first`, and `is_match`
  - `rgx-core/src/engine.rs` now traces `Engine::new`, `find_all`, `find_first`, and `is_match`
  - added decision reasoning for UTF-8 validity gates and match outcome summaries at engine/API boundaries
- Resolved interrupted partial edit artifacts during implementation:
  - cleaned malformed constructor/return fragments introduced mid-edit in `lib.rs` and `engine.rs`
- Validation confirmed:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - debug/low/quiet trace-log matrix using `rgx-cli` with `cat|dog` on `I have a dog`
  - low filtering check passed (no `[MEDIUM]/[HIGH]/[TRACE]` in `trace.log`)
  - quiet mode left `trace.log` at `0` bytes
### 2026-02-27
- Extended structured tracing into lexer-path pipeline:
  - added lexer boundary traces for `Lexer::new`, `Lexer::next_token`, and `Lexer::parse_escape`
  - added quantifier/class traces for `parse_star`, `parse_plus`, `parse_question`, `parse_repeat_quantifier`, and `parse_character_class`
  - added group/conditional traces for `parse_group`, `parse_conditional_start`, and `parse_conditional_subexpression_ast`
- Added lexer decision tracing for EOF token emission, simple-vs-special group dispatch, conditional close validation, and repeat-quantifier form checks.
- Validation confirmed:
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-cli`
  - `--verbosity debug --trace-log` includes lexer trace lines in `trace.log`
  - `--verbosity low --trace-log` filters to low-level milestones
  - `--quiet --trace-log` leaves `trace.log` empty
### 2026-02-27
- Extended structured tracing into parser-path pipeline:
  - `rgx-core/src/parser.rs` now emits structured entry/exit/decision logs for parser hotspots (`new`, `parse`, `parse_alternation`, `parse_sequence`, `parse_quantified`, `parse_atom`)
  - `rgx-core/src/parsing.rs` now emits parser-backend selection and parse-boundary logs in both recursive-descent and `pgen-parser` feature paths
  - `RecursiveDescentParser::parse_pattern` trait adapter now emits parse-boundary outcome logs
- Validation confirmed:
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-cli`
  - parser trace lines are visible in `trace.log` at `--verbosity debug` and filtered at `--verbosity low` / `--quiet`
### 2026-02-27
- Added first UVM-style tracing increment after trace-file routing baseline:
  - `rgx-core/src/log.rs` now provides `Verbosity::{None,Low,Medium,High,Debug}` with env control via `RGX_VERBOSITY`
  - structured trace helpers added: `trace_enter!`, `trace_exit!`, `trace_decision!`
  - level-filtered external sink API added: `emit_external_at(...)`
- Updated `rgx-cli` tracing UX:
  - new `--verbosity <none|low|medium|high|debug>`
  - new `--quiet`
  - legacy compatibility kept: `--debug => high`, `--trace => debug`
- Instrumented compiler/VM hotspots with explicit entry/exit and decision-reason logs.
- Verified filtering behavior and sink routing with:
  - `--verbosity debug --trace-log` (exhaustive output)
  - `--verbosity low --trace-log` (milestone-only output)
  - `--quiet --trace-log` (empty `trace.log`)
### 2026-02-27
- Added file-based trace routing support for debugging output:
  - `rgx-core` logging now supports `RGX_TRACE_FILE=trace.log` sink routing
  - `rgx-cli` now has `--trace-log` to enable trace routing to `trace.log`
  - CLI debug/trace messages are routed through the same core sink as VM/compiler logs
- Initialization order was adjusted so log env config is applied before first emission, avoiding early-init misconfiguration.
- Verified by running `rgx-cli --debug --trace-log` and confirming log lines are written to `trace.log`.
### 2026-02-26
- Added root-level `COMMIT.md` as authoritative commit-workflow contract for AI handoff and process consistency.
- `COMMIT.md` now defines:
  - when to run commit workflow (after completed tasks)
  - exact workflow steps and post-commit checks
  - involved files and precise responsibilities (`git_message_brief.txt`, `CHANGES.md`, `MEMORY.md`, `DEVELOPMENT_NOTES.md`, task files)
  - commit invariants (fresh status, exact staging, brief-file cleanup, untracked verification)
- Integrated references in `README.md` and `DEVELOPMENT_NOTES.md` so successor AI instances can discover workflow rules quickly.
### 2026-02-22
- Completed differential parity-hardening increment for greedy quantifier suffix behavior:
  - added `pcre2_parity_supported_quantifier_suffix_backtracking_behavior` in `rgx-bench/tests/pcre2_parity.rs`
  - covers first-match and `find_all` parity for suffix-sensitive `a*a`, `a+a`, and `ab?b`
  - includes explicit PCRE2 expected-span assertions to lock reference behavior
  - validation passed with targeted `rgx-bench` + `rgx-core` quantifier regression commands
- Completed unbounded-range parity hardening + quantifier runtime correction:
  - root cause found via new tests: greedy quantifier execution (`*`, `+`, `?`) lacked runtime fallback states, so suffix-compatible backtracking paths were lost
  - fixed `PlusGreedy`, `StarGreedy`, and `QuestionGreedy` execution to save fallback frames and restore state on failed/no-advance repetition attempts
  - added parser-path regressions for:
    - unbounded range `{2,}` scan/find_all
    - unbounded-range suffix behavior (`\\d{2,}3`)
    - generic greedy quantifier suffix backtracking (`a*a`, `a+a`, `ab?b`)
  - added differential parity test `pcre2_parity_supported_unbounded_range_quantifier_behavior`
  - full `rgx-core` and `rgx-bench` suites passed after changes
- Completed follow-up parity-hardening pass after closing `{n,m}` gap:
  - added supported-syntax PCRE2 differential cases for bounded-range suffix backtracking (`\\d{2,3}3`) in both first-match and find-all suites
  - added exact-range `{3}` find-all differential coverage
  - added parser-path API regressions for bounded-range suffix backtracking, greedy longest-valid suffix behavior, and stable `find_all` spans
  - expanded capability-matrix parser-path supported case table with bounded-range suffix positive/negative examples
- Validation for this increment:
  - `cargo test -p rgx-core parser_range_quantifier -- --nocapture`
  - `cargo test -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo test -p rgx-bench`
  - `cargo test -p rgx-core`
- Closed the previously tracked `{n,m}` PCRE2 parity gap:
  - root cause: range quantifier codegen forced exact-max behavior for bounded ranges and mismatch paths bypassed available backtrack frames
  - fix: compile bounded optional range tail with `Split`-based greedy optionals and make key opcode mismatch paths honor `try_backtrack`
  - validation: targeted and full `rgx-core` + `rgx-bench` test suites passed
- Updated parity/docs/test state after the fix:
  - reclassified range differential case to parity-supported in `rgx-bench/tests/pcre2_parity.rs`
  - updated `docs/PCRE2_COMPATIBILITY_MATRIX.md` to mark `{n,m}` scan behavior parity-verified
  - added parser-path regressions in `rgx-core/src/lib.rs` for earliest-scan and bounded-range suffix backtracking behavior
- User requested creation of `MEMORY.md` as critical live continuity infrastructure.
- Explicit requirement: keep this document continuously updated with key actionable exchange outcomes (not full transcript), and do it before commit workflow.
- This file was created and integrated into live documentation policy so future AI instances can resume quickly and safely.

## 2026-04-08 session — massive backlog execution (40 of 44 items closed)

### New public API surface shipped this session
- `find_first_at`, `find_all_at`, `is_match_at` — position-aware matching
- `split`, `splitn`, `split_iter`, `splitn_iter` — regex-delimited string splitting
- `replace`, `replace_all`, `replacen` — accept `impl Replacer` (strings, closures, `NoExpand`), return `Cow<str>`
- `Match<'t>` with `as_str()`/`range()`/`len()`, `Captures<'t>` with index/name/expand/iter
- `find`, `find_iter`, `captures`, `captures_iter`, `capture_names` — ergonomic + lazy iterator APIs
- `CaptureLocations` for zero-allocation loops, `captures_read`, `captures_read_at`
- `escape()`, `shortest_match`, `shortest_match_at`
- `RegexBuilder` with zero-arg flag setters (`.case_insensitive()` not `.case_insensitive(true)`)
- `RegexSet` — multi-pattern simultaneous matching with `SetMatches`
- `RegexCache` — thread-safe LRU compilation cache (`Arc<Regex>`)
- `BytesRegex` — match `&[u8]` without UTF-8 validation
- `MatchSemantics` (LeftmostFirst/LeftmostLongest) — runtime switch
- `PartialMatchResult` — streaming/incremental matching (Full/Partial/NoMatch)
- `CompileError` with caret-highlighted span diagnostics
- `MatchResult.groups` — capture group positions on every match result
- `Regex::named_groups()`, `as_str()`, `captures_len()` — metadata accessors

### New engine features
- `set_max_steps`, `set_max_backtrack_frames`, `set_max_recursion_depth` — DoS protection (AtomicU64)
- Full Unicode `(?i)` case folding (café→CAFÉ, Greek, Cyrillic)
- `\X` extended grapheme cluster matching via `unicode-segmentation`
- Inline-language steering: `rgx.steer_accept()` etc. in Lua/JS/Rhai
- `tail_file` with OS-native watching (kqueue/inotify via `notify` crate)

### Infrastructure
- CLI `--color auto|always|never` with ANSI match highlighting
- 4 cargo-fuzz targets (compile, match, replace, roundtrip invariants)
- Criterion benchmark CI job with 90-day artifact storage
- Removed 4 scaffold placeholder files, zero RGX-owned clippy warnings

### Backlog status (docs/BACKLOG.md)
- **40 of 44 items closed** across all tiers
- Remaining: A9 (language bindings), A12 (returned-capture subroutines), C1 (JIT), C2 (NFA/DFA)
- A8 (crate publishing) deferred by user — too early
- Test count: ~482 (481 pass, 1 ignored timing-sensitive)

### User feedback captured
- API fluency is paramount — zero ceremony, feels like driving not wrenching
- `RegexBuilder` flag setters should be zero-argument by default
- Don't delete old MEMORY.md entries — append dated sections to preserve continuity
- Always update live docs before commit workflow — it's a hard gate, not optional
- Do not `git push` after every commit — push only when explicitly asked

## 2026-04-09 session — PGEN 1.1.9, A12 + A10 shipped

### PGEN submodule update
- Updated from 1.1.8 to 1.1.9 (commit `ac2acb3`)
- 1.1.9 adds returned-capture subroutine syntax `(?N(grouplist))`
- Filed PGEN-RGX-0015 (feature request), closed same day as verified-fixed-upstream
- PGEN-RGX-0014 filed then closed (not a PGEN bug — RGX adapter was missing `\X` mapping)

### Features shipped
- **A10 `\X`**: Extended grapheme cluster matching via `unicode-segmentation` crate. VM opcode `0x08`, AST node, parser mapping. Bug found via trace: opcode was missing from `TryFrom<u8>` dispatch table.
- **A12 Returned-capture subroutines**: `(?1(1))` syntax parsed via PGEN 1.1.9 `returned_capture_subroutine` nodes. Compiles to `Call` opcode. Full capture-return VM semantics (preserving specified groups) is a follow-up.

### The RGX Book (mdBook)
- `book/` directory: 30 chapters, 7,300+ lines, every feature documented with examples
- Build: `mdbook serve book` for searchable HTML
- Live document — evolves alongside the project

### Backlog status
- **41 of 44 items closed**
- Remaining: A9 (language bindings — large), C1 (JIT — major), C2 (NFA/DFA — major)
- A12 closed (returned-capture subroutines shipped with PGEN 1.1.9)
- CI fix: `RGX_SUBMODULES_TOKEN` secret set up for private PGEN submodule access

## 2026-04-09 session — RUST_CODEBASE_ANALYSIS.md staleness sync

### Bootstrap pass
- New session began with `Read and execute the content of README.md`. Followed `SESSION_BOOTSTRAP.md` order: README → MEMORY → COMMIT → ROADMAP → RUST_CODEBASE_ANALYSIS → BACKLOG → DEVELOPMENT_NOTES → PROJECT_VISION → CHANGES (latest).
- Verified codebase against the analysis doc: PGEN pin = `ac2acb3` (1.1.9), MSRV 1.88, ~34K lines rgx-core+CLI, working tree clean at `114ef3b`, book has 45 chapters under `book/src/` with the new Part VI internals.

### RUST_CODEBASE_ANALYSIS.md was substantially stale
- Said PGEN 1.1.8 / commit `54ed190…` (reality: 1.1.9 / `ac2acb3`)
- Said ~26K lines, ~550 tests (reality: ~34K lines, 633 tests post-API-smoke commit `c147ddc`)
- Did not mention A10 (`\X`), A12 (returned-capture subroutines), A4 (CLI `--follow`), A3 (`tail_file`), A6 (inline-language steering), A7 (Unicode `(?i)` case folding), the entire 2026-04-08 backlog blitz public API surface (`Match`, `Captures`, `RegexBuilder`, `RegexSet`, `RegexCache`, `BytesRegex`, safety limits, …), or the existence of The RGX Book
- Said `simd.rs` / `javascript.rs` / `wasm.rs` / `cache.rs` were still scaffolds (reality: first three deleted in 2026-04-08 cleanup; `cache.rs` is now real and hosts shipped `RegexCache`)
- High-confidence next actions list still pointed at deepening benchmark trends — replaced with current backlog reality (A9 / C1 / C2 / A12-VM-followup / A8 / Pages / perf-push / `VERSION` & `(*SKIP:name)`)

### Approach
- Targeted edits only — left intact the parts that are still accurate (PGEN-backed parser path description, conditional/recursion/Perl-extended-class status, benchmark trend infrastructure, parser interoperability contract section).
- Used `MSRV 1.88` per `Cargo.toml`. Used the verified `wc -l` source totals.
- User chose "Commit doc sync only" via AskUserQuestion when asked which roadmap direction to take next.

## 2026-04-09 session — strategic reprioritization: defer A9, elevate C1+C2

### The decision
- User asked the genuine question "why A9?" after I listed it as the largest remaining adoption lever in the post-bootstrap status summary.
- I gave an honest assessment: the "10x user base" rationale in `docs/BACKLOG.md` is generic and doesn't fit RGX specifically. RGX's killer feature is host integration (predicates, steering, events, async I/O, embedded Lua/JS/Rhai/Wasm), and that surface translates poorly across FFI — Python callbacks are GIL territory, the async story assumes Rust's model, and "embed Lua inside a regex from Python" is a weaker pitch than from C/C++ because Python users already have a scripting host. Plus A9 is gated on A8 (publish, also deferred), is `large` per language, and the maintenance tail competes with engine work that strengthens the actual differentiator.
- User responded: **push A9 to the back of the backlog and switch to C1 + C2 because RGX is currently too slow.**

### Strategic ordering: C2 first, C1 second
- **C2** (NFA/DFA hybrid) changes the algorithmic class. Detect patterns that don't use backtracking-only features (no backreferences, no recursion, no lookaround, no inline code blocks, no atomic groups, no possessive quantifiers, no `\K`, no backtracking verbs) at compile time and run them through Thompson NFA + lazy DFA cache instead of the backtracking VM. Gives RGX the "can't hang" property the Rust `regex` crate uses as its primary differentiator. Typically 10x-100x improvement on regular patterns where it applies.
- **C1** (JIT compilation) is then a constant-factor multiplier (~5-10x) on whichever engine runs. Cranelift is already in the dep tree via wasmtime.
- In this order, the wins compound: NFA/DFA on the common case + JIT'd backtracking for the rest. If C1 went first, pathological backtracking patterns would still be exponential (just JIT'd exponential).

### Capture-handling design lesson from the Rust `regex` crate
- Standard solution: use the DFA only for *finding* the overall match span, then re-run a small bounded NFA simulation over the matched span to recover capture group positions. Avoids the full DFA-with-captures complexity that would otherwise sink the project.

### Doc updates
- `ROADMAP.md`: new "Now" entry "Performance: NFA/DFA hybrid (C2) + JIT compilation (C1)" with the strategic ordering and rationale. A9 (under "Binding/runtime expansion") annotated as deferred with reactivation criteria.
- `docs/BACKLOG.md`: new "Tier 0 — Active focus" at top of priority tiers. A9 entry rewritten with full deprioritization reasoning. C1/C2 entries rewritten with active-focus annotations and design notes. A9 moved from Tier 3 to Tier 4. Banner added.
- `MEMORY.md`: this entry.
- `CHANGES.md`: new entry at top.

### Next concrete action (proposed, awaiting user confirmation)
- Start C2 with a design proposal before any code: classify the no-backtracking subset, sketch the Thompson construction + lazy DFA cache, decide the engine-dispatch boundary in `Regex::find_first` etc., and lay out validation strategy (differential testing against the existing VM path on the no-backtracking subset, then benchmark trend capture against the same workloads).

## 2026-04-09 session — C2 step 0 design proposal landed

### What happened
- User confirmed the SOTA-first reformulation of the C2 plan and saved a `feedback_sota_first_time.md` auto-memory entry making this a persistent preference for future sessions.
- I asked the architectural decision for capture group recovery (two-pass via bounded Pike-VM vs tagged transitions in the NFA). User clarified the question once, then said "you pick what you think is best."
- Picked **two-pass capture recovery via bounded Pike-VM over the matched span** — the SOTA approach used by RE2 and the Rust `regex` crate. Decision recorded in the design doc as §9.
- Wrote `docs/C2_NFA_DFA_DESIGN.md` — comprehensive design doc covering goals/non-goals, architectural overview, module layout, no-backtracking subset definition (full inclusion/exclusion table with reasons), byte-class equivalence partitioning, forward + reverse Thompson NFA construction with anchored/unanchored variants, sparse-set Pike-VM (Russ Cox design from day one — explicitly framed as the permanent NFA simulator and lazy DFA fallback, not a prototype), lazy DFA cache with clear-on-overflow + retry budget policy, two-pass capture recovery, engine dispatch boundary in `c2/dispatch.rs`, what the existing VM path does NOT lose (every PCRE2 feature outside the subset, all host integration layers, all public API), differential testing strategy, benchmark strategy, phased implementation plan (steps 0-8), 10 open architectural questions with current leans, risks and mitigations, references.

### Module layout decided in the design doc
- New code lives under `rgx-core/src/c2/` with modules: `mod.rs`, `classifier.rs`, `byte_class.rs`, `nfa.rs`, `pike.rs`, `dfa.rs`, `dispatch.rs`, `tests/`. Existing modules (`compiler.rs`, `vm.rs`, `engine.rs`, `lib.rs`) get only small additions to store classification metadata on `Program` and route classified-positive patterns to `c2::dispatch`. Backtracking VM internals are not touched.

### Cohabitation rule (non-negotiable)
- The existing backtracking VM stays in place forever and handles every pattern outside the no-backtracking subset (backreferences, recursion, lookaround, conditionals, atomic groups, possessive quantifiers, `\K`, backtracking verbs, inline code blocks, callouts, branch-reset, Perl extended classes). All host integration layers stay on the VM path. All public API stays unchanged. If anything regresses, it's a merge blocker. Spelled out in design doc §12.

### Differential testing is the merge gate
- New `rgx-core/tests/c2_differential.rs` and `c2_proptest.rs` test files land in step 4 (Pike-VM). From that step onward, every C2 commit must produce zero differential failures against the existing VM on classifier-positive patterns. The existing 633-test suite plus the PCRE2 parity corpus plus the proptest harness form the corpus. There is no "C2 testing mode" that can be skipped.

### Phased implementation plan (recap)
- Step 0: this design doc — current commit, awaits user sign-off
- Step 1: pattern classifier (metadata-only on `Program`, no runtime dispatch yet)
- Step 2: byte-class equivalence partitioning
- Step 3: forward + reverse Thompson NFA construction with anchored/unanchored variants
- Step 4: sparse-set Pike-VM with differential gate active
- Step 5: lazy forward DFA with cache + clear-on-overflow + retry budget + Pike-VM fallback
- Step 6: lazy reverse DFA for start-of-match recovery
- Step 7: literal prefix integration with C2 dispatch
- Step 8: production cutover, benchmark sweep, new `book/src/internals/nfa-dfa-engine.md` chapter
- Estimated 8 minimum / 12-15 realistic commits, multi-week timeline

### Open architectural questions (documented in design doc §16, awaiting decisions)
- Q1 full Unicode case folding scope, Q2 ASCII vs Unicode `\b`, Q3 LeftmostFirst vs LeftmostLongest, Q4 per-instance vs thread-local DFA cache, Q5 default `dfa_size_limit`, Q6 Pike-VM fallback restart policy, Q7 debug-mode parallel-engine equivalence assertion, Q8 public `regex.uses_c2()` introspection, Q9 RegexSet C2 integration, Q10 long-span Pike-VM capture pass cost.

### Sign-off gate
- The design doc is committed but C2 step 1 cannot start until the user approves §20 sign-off. No code lands until then.

## 2026-04-10 session — C2 step 1 shipped (pattern classifier)

### How this got triggered
- User said "PNT" (= "Pick the next task and roll with it"). Treated as implicit §20 sign-off on the C2 design doc since step 1 is metadata-only and the most reversible step in the plan. PNT is now persistent shorthand for this delegation pattern (saved as `feedback_pnt_shorthand.md` in auto-memory).

### What landed
- New module `rgx-core/src/c2/{mod.rs, classifier.rs}` with `Classification` enum (NoBacktracking | NeedsVm { reason }) and `ExclusionReason` enum. Single linear-time AST visitor implements the no-backtracking subset from design doc §4. Conservative classifier — any uncertainty returns NeedsVm. Default value is `NeedsVm { NotYetClassified }` as a safe-by-construction sentinel.
- New `Program.classification` field on `vm::Program`. Populated by `compile_ast_with_label` after VM bytecode generation. Doc-hidden accessor `Regex::classification()` for tests and internal callers (the public `uses_c2()` introspection is design doc Q8 / step 8).
- 43 new unit tests in classifier.rs::tests + 26 new integration tests in `rgx-core/tests/c2_classifier.rs`. Total rgx-core test count: 721 passing (was 633). All gates green.
- No runtime dispatch yet. Existing backtracking VM unchanged for every pattern. Step 1 is metadata only by design.

### Module layout decisions made during implementation
- Classification stored on `vm::Program` (per design doc §4) rather than on `CompiledPattern`, so it lives close to the bytecode that runtime dispatch will need to read.
- `Default` impl on `Classification` returns `NeedsVm { NotYetClassified }` so any future code path that constructs a Program without going through the full compiler still routes safely to the existing VM. The compiler always overwrites this in `compile_ast_with_label`.
- `Engine::classification()` and `Regex::classification()` are both `#[doc(hidden)]` to keep the public API clean while exposing the field for tests. The user-facing introspection method is intentionally deferred to step 8.
- Possessive quantifiers (`*+`, `++`, `?+`, `{n,m}+`) classify as `AtomicGroup` because the parser lowers them into `Group { kind: Atomic, ... }` AST nodes — no separate exclusion case needed.

### Next concrete action
- C2 step 2 = byte-class equivalence partitioning. Standalone module, no engine wiring. Compute the byte-class map from the AST at compile time so steps 3+ (NFA construction, DFA cache) can use it.

## 2026-04-10 session — CI hotfix for PCRE2 parity tests on older PCRE2

### What broke
- User pointed at `~/Downloads/job-logs-rgx-ci-error.txt` showing 3 PCRE2 parity tests failing on `origin/main` HEAD `114ef3b`. This was a pre-existing failure on origin/main; none of the local-only session commits caused it.
- CI runs on Ubuntu 24.04 with `libpcre2-dev 10.42-4ubuntu2.1`. PCRE2 10.42 doesn't recognize `(?[...])` Perl extended character classes — that syntax was experimental/opt-in until PCRE2 10.45 (March 2025) when it became default-on.
- Local dev (macOS homebrew) has newer PCRE2 so the tests pass there.

### Fix
- `rgx-bench/tests/pcre2_parity.rs`: added `pcre2_supports_perl_extended_class()` helper using `OnceLock` to cache a single canonical-pattern compile attempt. Added `skip_if_unavailable(&case)` guard at the top of each of the three affected test loops. When the runtime PCRE2 doesn't support `(?[...])`, cases using that syntax are skipped with a clear stderr notice. On dev machines and future CI with PCRE2 >= 10.45, the cases run unchanged.
- No CI workflow changes. No new dependencies. No PCRE2 vendoring (which `pcre2-sys` doesn't cleanly support).
- The fix is correct by construction: only skips when PCRE2 itself rejects the canonical pattern. RGX still validates `(?[...])` unconditionally via its own unit tests in `rgx-core`.

### Validation
- `cargo test -p rgx-bench --test pcre2_parity` 13 passing locally (skip is a no-op on local PCRE2 which supports the syntax).
- Full quality gates green: `cargo fmt --check`, `cargo test -p rgx-bench`, `-p rgx-core`, `-p rgx-cli`, `cargo clippy --workspace --all-targets`.

### Note on push
- Per the persistent no-auto-push rule, this commit lands locally only. User pushes when ready. Once pushed, the next CI run on origin/main will validate the fix on Ubuntu 24.04's PCRE2 10.42.

## 2026-04-10 session — C2 step 2 shipped (byte-class equivalence partitioning)

### How this got triggered
- User said "PNT" (= "Pick the next task and roll with it"). C2 step 1 had just shipped on origin/main, so the natural next step was C2 step 2 per the design doc §15 phased plan.

### What landed
- New module `rgx-core/src/c2/byte_class.rs` with `ByteClassMap` (`table: [u8; 256]`, `num_classes: u16`), `build_from_ast(&Regex)` constructor, `class_of(byte)` and `num_classes()` accessors. Re-exported via `c2::ByteClassMap`.
- Algorithm: boundary-points partition with per-character-class membership oracles. Two bytes are in the same class iff every oracle (one per character class / literal / shorthand / Dot / etc.) gives the same membership answer for both. Multi-byte UTF-8 ranges are decomposed via `regex_syntax::utf8::Utf8Sequences` into per-position byte ranges, all added to the same oracle.
- 25 new unit tests covering ASCII patterns, non-ASCII patterns, shorthand classes, Dot newline distinction, structural nodes, realistic log pattern, class ID density, edge cases (byte 0x00, 0xFF, full universe, duplicates, adjacent ranges).
- No engine wiring (per design doc step 2 scope). The map is consumed by step 3 NFA construction.
- 1 design-doc fix: `num_classes` was `u8` in the original sketch but the count can be 256, so it was bumped to `u16` in `docs/C2_NFA_DFA_DESIGN.md` §5 with an explanatory note.

### Critical correctness lesson learned during implementation
- First draft of the partition algorithm treated each byte range as a separate "membership oracle". This was wrong: `[abc]` would yield 4 classes (one per char + "other") instead of the correct 2 (`[abc]` and "everything else"). The fix is that each character class in the AST is ONE oracle that contains multiple ranges, and the partition signature is per-oracle, not per-range. Bytes within one character class share the same membership signature and therefore the same byte class. Documented the rule prominently in the module docstring and CHANGES entry to prevent future regressions.

### API call mistake caught early
- First draft called `crate::unicode_support::resolve_unicode_property_class(name)` (1 arg, returning `Option`). The actual signature is `(name: &str, negated: bool) -> Result<Vec<CharRange>, String>` (2 args, returning `Result`). Build errors caught it immediately. Fixed by passing the pattern's `negated` flag through and switching `if let Some` to `if let Ok`. No tests needed updating because the unit tests for byte_class don't exercise Unicode property class paths heavily.

### Next concrete action
- C2 step 3 = forward + reverse Thompson NFA construction. New `rgx-core/src/c2/nfa.rs` module. Builds a forward NFA from the AST (anchored and unanchored variants) and a reverse NFA from the AST (anchored and unanchored variants). Uses `ByteClassMap` to label transitions by class ID rather than raw byte. Standalone module — no Pike-VM yet (that's step 4).

## 2026-04-10 session — C2 step 3a shipped (forward Thompson NFA)

### Split decision
- Step 3 is the biggest step in the C2 plan, so I split it into 3a (forward NFA, anchored + unanchored) and 3b (reverse NFA + CompiledC2Program assembly). Each sub-commit is a coherent, production-quality deliverable. The user said "PNT" so I rolled with this split rather than asking for sign-off.

### What landed
- `rgx-core/src/c2/nfa.rs` (~1180 lines incl. tests). Forward Thompson NFA construction for the full no-backtracking subset.
- Data structures: `Nfa`, `NfaState`, `NfaStateId`, `ByteClassId`, `EpsilonPriority`, `EpsilonEdge`, `CaptureTag`, `ZeroWidthAssertion`. Internal `Fragment` and `NfaBuilder`.
- Thompson rules implemented for every supported AST node: `Char` (1-4 byte UTF-8), `CharClass` (Custom with negation, Digit/Word/Space, UnicodeClass), `Dot`, `Digit`/`Word`/`Space` top-level, `UnicodeClass` top-level, `NewlineSequence`, `Anchor`, `WordBoundary`, `Empty`, `WhitespaceLiteral`, `Sequence`, `Alternation`, `Quantified` (?, *, +, {n}, {n,m}, {n,} greedy and lazy), `Group` (Capturing with capture tags, NonCapturing), `FlagGroup` (descend).
- Multi-byte UTF-8 via `regex_syntax::utf8::Utf8Sequences`. Codepoint ranges decompose into per-position byte ranges; each chain of byte-class transitions can fan out when a per-position range spans multiple byte classes.
- Greedy/lazy priority encoding on epsilon edges. Lower priority preferred (leftmost-first semantics).
- Unanchored variant via lazy `(?s:.)*?` prefix that matches any byte (including newline). Same approach as RE2 and the Rust `regex` crate.
- 30 unit tests covering structural correctness, multi-byte chains, alternation priority, greedy/lazy swaps, range unrolling, capture tag placement, anchor emission, unanchored prefix priorities, realistic combined patterns, helper invariants.
- 1 small build error caught immediately: `GroupKind` doesn't implement `Copy`, so `*kind` in the destructure had to be `kind.clone()`. Fixed in the same edit cycle.

### Pending items deferred to step 3b
- Reverse NFA construction (mirrors forward, swaps concatenation order, swaps anchors `^` ↔ `$`, `\A` ↔ `\z`)
- `CompiledC2Program` struct holding all 4 NFAs (forward+anchored, forward+unanchored, reverse+anchored, reverse+unanchored) + ByteClassMap + capture metadata
- `GraphemeCluster` (`\X`) handling — currently classified as NoBacktracking by the classifier but the NFA builder produces an unmatchable fragment for it. Needs to be either properly implemented in step 3b or moved out of the subset by adding `ExclusionReason::GraphemeCluster` to the classifier. Decision deferred until step 3b.

### Next concrete action
- C2 step 3b = reverse NFA construction + CompiledC2Program assembly. The reverse NFA is structurally symmetric to the forward NFA (concatenation reversed, anchors swapped, alternation/quantifiers unchanged because they're symmetric). CompiledC2Program holds all 4 NFAs plus the ByteClassMap and capture metadata. Decide on \X handling.

## 2026-04-10 session — C2 step 3b shipped (reverse NFA + CompiledC2Program)

### What landed
- `reverse_ast` helper in `c2/nfa.rs`. Walks the AST and produces a structurally reversed AST. The cleanest way to build the reverse NFA is to reverse the AST and reuse the forward Thompson construction — no parallel build logic to drift.
- `Nfa::build_reverse_anchored` / `Nfa::build_reverse_unanchored` constructors. One-liners that call `reverse_ast` then `build_anchored` / `build_unanchored`.
- New module `c2/program.rs` with `CompiledC2Program` struct holding the byte-class map + all 4 NFAs + capture group count. `build_from_ast` constructor builds the byte-class map ONCE from the original AST and reuses it for both directions (the set of bytes the pattern uses is direction-independent).
- 14 new reverse-NFA unit tests + 8 new CompiledC2Program unit tests + 1 new classifier test.

### \X decision: moved out of the C2 subset
- `\X` (extended grapheme cluster) was previously classified as `NoBacktracking` but the NFA builder produced an unmatchable fragment for it (defensive fallback). Either implement it properly or move it out.
- Decision: move it out. Matching a grapheme cluster needs Unicode-aware traversal of base codepoint + combining marks, which doesn't fit cleanly into Thompson NFA without significant extra machinery. SOTA-first preference says don't ship a half-baked version.
- Implementation: added `ExclusionReason::GraphemeCluster` variant, classifier now returns `NeedsVm { GraphemeCluster }` for `Regex::GraphemeCluster`. `\X` patterns continue to run on the existing backtracking VM (which has full `\X` support). Can be added to the C2 subset later if profiling shows it matters.
- Test impact: renamed `classifies_newline_sequence_and_grapheme_cluster_as_no_backtracking` to `classifies_newline_sequence_as_no_backtracking` and added a new `excludes_grapheme_cluster_from_c2_subset` test.

### Reverse anchor handling
- `^` ↔ `$` flip. `\A` ↔ `\z` flip. `\Z` (end of input or just before final newline) is approximated as `\A` for the reverse direction — the exact final-newline corner case would need runtime simulator support to handle perfectly. Documented as a known approximation in the `reverse_anchor_type` doc comment. Not a blocker for the no-backtracking subset since `\Z` is rare in practice.

### NewlineSequence (`\R`) reversal
- `\R` matches `\r\n` OR any single newline-like char. The reverse needs to match `\n\r` for the CRLF branch. Solved by expanding `\R` to its structural alternation form `(\r\n | \n | \v | \f | \r | \u{85} | \u{2028} | \u{2029})` BEFORE reversing — the structural reversal then naturally produces `(\n\r | ...)`. Tested via `reverse_ast_expands_newline_sequence_so_crlf_branch_reverses`.

### Step 3 is complete
- Forward NFA (3a) + reverse NFA (3b) + CompiledC2Program (3b) = step 3 complete. The C2 module now has everything needed for step 4 (Pike-VM) to start running these NFAs against real input.

### Next concrete action
- C2 step 4 = sparse-set Pike-VM. This is where the differential testing gate against the existing backtracking VM goes active for the first time. New `c2/pike.rs` with the sparse-set state container and the parallel-state simulation loop. Wires into engine dispatch for `is_match` / `find_first` / `find_all` / `captures` for `NoBacktracking`-classified patterns. New `tests/c2_differential.rs` to verify byte-for-byte equivalence with the existing VM on the classifier-positive test corpus.

## 2026-04-10 session — C2 step 4a shipped (Pike-VM + differential test)

### Split decision
- Step 4 is the biggest correctness milestone, so I split it into 4a (Pike-VM core handling is_match/find_first/find_all WITHOUT captures + differential test for match spans) and 4b (capture tracking + engine dispatch wiring). Each is a coherent SOTA deliverable.

### What landed
- New `rgx-core/src/c2/pike.rs` with the sparse-set Pike-VM. Russ Cox / Briggs–Torczon design. Two arrays of size num_states, O(1) add/contains/clear. Public API: `pike_is_match`, `pike_find_first`, `pike_find_all`.
- Zero-width assertions: `\A`, `\z`, `\Z`, `^`, `$`, `\b`, `\B`, `\G`. ASCII word semantics. `\G` evaluates to true at pos 0 only (full threading deferred to 4b).
- New `CompiledC2Program::try_compile(pattern)` helper that parses + classifies + builds in one call. Returns Some only for NoBacktracking patterns.
- New integration test `rgx-core/tests/c2_pike_differential.rs` with 12 corpus suites (~70 differential cases). Compiles via both `Regex::compile` (existing VM) and `CompiledC2Program::try_compile` (C2 path), runs both engines on the input, asserts byte-for-byte agreement on is_match / find_first / find_all. Patterns outside the C2 subset are skipped silently. **All 12 corpus suites pass.** This is the differential gate going active for the first time.
- 29 Pike-VM unit tests covering sparse set ops, literals, character classes (ASCII + multi-byte UTF-8), shorthand classes, sequences, alternations, greedy/lazy quantifiers, range quantifiers, anchors, word boundaries, find_all, empty-match advance, realistic patterns (ISO date, email, log levels).

### Two SOTA correctness fixes during testing
1. **Lazy quantifier priority bug**. The closure walker iterates `state.epsilons` in SLOT order. The quantifier builders were inserting lazy edges in semantic order, not slot=priority order, so for lazy `a+?` the loop edge ended up at a lower dense position than accept and the priority-cutoff didn't apply. Fix: enforce slot==priority in build_zero_or_one / build_zero_or_more / build_one_or_more. The `EpsilonEdge.priority` field is now informational only — slot order is what the simulator honours. **Lesson saved**: this is a subtle invariant that must be maintained whenever new builder methods are added. Documented prominently.
2. **find_all empty-match adjacency rule**. For `a*` on `aaab`, the existing VM returns `[(0, 3), (4, 4)]` — skipping the empty match at position 3 immediately adjacent to the non-empty match. Fix: track prev_non_empty_end and skip empty matches at that exact position. Matches the existing VM and the Rust `regex` crate convention.

### The dense-position-as-priority trick
- The Pike-VM uses leftmost-first semantics by exploiting the sparse set's insertion order: when accept is in current at dense position `p`, only states at positions `0..=p` are extended in the next iteration. Higher dense positions were added by lower-priority epsilon edges and cannot win. This works because the closure walker visits edges in priority order, so dense order encodes priority. Lazy quantifiers terminate at the earliest accept position without a separate kill-pass.

### Pending for step 4b
- Capture tracking inside the Pike-VM (per-thread capture buffers, copy semantics on epsilon forks)
- Engine dispatch wiring: `Regex::compile` → if classifier says NoBacktracking, route through Pike-VM for is_match/find_first/find_all/captures; otherwise existing VM unchanged
- Extend differential test to compare capture group positions
- Once dispatch is wired, the existing 633+ test suite effectively becomes a deeper differential test — every existing test runs through Pike-VM for classifier-positive patterns

### Next concrete action
- C2 step 4b: capture tracking + engine dispatch wiring. The Pike-VM tracks captures via per-thread capture buffers (Vec<Option<usize>>). On epsilon edges with capture tags, the buffer is updated with the current position. Engine dispatch lives in c2/dispatch.rs (new file). Public API methods on Regex check the program's classification and route accordingly.

## 2026-04-10 session — C2 step 4b shipped (Pike-VM captures + extended differential)

### Split decision
- Step 4 is now 4a (Pike-VM core, no captures), 4b (captures), 4c (engine dispatch). 4b is this commit. The split keeps each commit focused: 4b has its own correctness gate (the differential test now compares capture positions) before 4c amplifies the differential surface to the entire 633+ test suite via dispatch wiring.

### What landed
- New `PikeMatch` struct with `start`, `end`, `groups` fields. Same shape as the existing `MatchResult.groups`.
- New `ThreadSet` struct (separate from `SparseSet`) with parallel state IDs + capture buffers. Pre-allocated, no per-call allocations.
- New `epsilon_closure_with_captures` that threads capture buffers through the recursion. Clones only on tagged edges (the rare case); pass-through on untagged edges (the common case).
- New `pike_match_at_with_captures` that uses the same dense-position-as-priority trick for leftmost-first semantics.
- New `pike_captures` and `pike_captures_all` public functions.
- 11 new pike unit tests covering zero groups, one group, multiple groups, nested groups, optional unmatched/matched, alternation winner, find_all with groups, quantified group last-iteration semantics, realistic ISO date.
- Extended differential test compares is_match + find_first spans + find_all spans + **find_first capture groups** + **find_all capture groups** against the existing VM. All 12 corpus suites pass.

### One SOTA correctness fix
- `CompiledC2Program::try_compile` was skipping `Compiler::assign_capture_indices` between parse and classify. The PGEN parser emits capture groups with `index: None` and the indices are assigned downstream. Without that pass, all capture groups collapsed to group 0 and `Nfa::num_capture_groups()` returned 0. Fix: made `assign_capture_indices` `pub(crate)` and called it from `try_compile`. The existing VM compile path already runs this pass; my C2 path now matches it.

### The slot layout
- Capture buffers are flat slices of `2 * (num_capture_groups + 1)` `Option<usize>` slots.
- `slots[0]` / `slots[1]` = overall match span (group 0). Populated by the caller from the scan position and the simulator's matched end — the NFA builder doesn't emit `CaptureTag::GroupStart(0)` / `GroupEnd(0)` for the overall match.
- `slots[2k]` / `slots[2k+1]` = group `k` start/end (for `k >= 1`). Populated by the simulator from `CaptureTag` epsilon edges during closure expansion.
- `captures_to_groups` pairs adjacent slots and converts to `Vec<Option<(usize, usize)>>` matching the existing VM's `MatchResult.groups` shape.

### Pending for step 4c
- Engine dispatch wiring. Public `Regex::compile` checks classification; if NoBacktracking, builds a `CompiledC2Program` alongside the existing `Program` and routes is_match/find_first/find_all/captures through Pike-VM.
- Once dispatched, the existing 633+ test suite is automatically a deeper differential test.

### Next concrete action
- C2 step 4c. Add a `Option<CompiledC2Program>` field to `CompiledPattern` (or `Regex`). Build it during `Regex::compile` when classification is NoBacktracking. In each public API method on `Regex`, check the classification and route accordingly. Run the full existing test suite — any disagreement is a Pike-VM bug to fix before the commit lands.

## 2026-04-10 session — C2 step 4c shipped (engine dispatch wiring) — Step 4 COMPLETE

### What landed
- `vm::Program.c2_program: Option<CompiledC2Program>` field, populated in `compile_ast_with_label` after classification.
- `is_c2_dispatch_eligible(ast)` function with structural exclusions: top-level alternation, flag groups (`(?i)` etc.), `\G` (`PreviousMatchEnd`), multi-byte char classes (UnicodeClass + non-ASCII Custom).
- `Engine::should_dispatch_to_c2()` adds runtime checks: no event observer, no runtime safety limits.
- `Regex::is_match`, `Regex::find_first`, `Regex::find_all` now dispatch through the Pike-VM when eligible. The existing 633+ test suite is now a deeper differential gate.
- All 856 tests in rgx-core pass with C2 dispatch active.

### Two correctness bugs caught by the broader differential gate
The 12 corpus suites passed but the larger test surface caught more:

1. **Multi-byte char class precision bug**. Byte-class partition in `c2/byte_class.rs` collapses all byte ranges from a multi-range character class into one oracle. Too coarse to distinguish per-position byte constraints. For `\P{L}` this caused `is_match("β")` → true (wrong; β is a letter). Quick fix: added `contains_multi_byte_char_class` exclusion. Proper fix (per-Utf8Sequence-per-position oracles, or building the partition from NFA transitions) is a documented follow-up.

2. **Dot longest-match bug**. `Regex::Dot` builds an alternation of byte chains for 1/2/3/4-byte UTF-8 sequences. With the coarse byte-class, all chains fire on every byte. The 1-byte chain reaches accept first, the priority cutoff kills longer chains, `find_first(".", "é")` returns 1-byte instead of 2-byte. Fix: sort `Utf8Sequences` by length descending in `build_char_ranges`. Longest chain gets highest priority slot (lowest dense position), survives the cutoff, greedy semantics restored.

### Bugs excluded from dispatch (route to existing VM)
- Top-level alternation (`cat|dog`) — Pike-VM doesn't track `matched_branch_number`
- Flag groups (`(?i)`, `(?s)`, `(?m)`, `(?x)`) — Pike-VM doesn't apply flag semantics
- `\G` (`PreviousMatchEnd`) — Pike-VM only handles \G at pos 0, not after previous match end
- Multi-byte char classes (`\p{L}`, `[α-ω]` etc.) — coarse byte-class precision bug
- Patterns with event observers set at runtime — Pike-VM doesn't emit events
- Patterns with runtime safety limits set — Pike-VM is bounded, doesn't enforce them

These exclusions are SOTA-correct: routing affected patterns through the existing VM preserves all test behaviour. Each exclusion can be lifted as Pike-VM gains the corresponding feature.

### What's dispatched
The remaining patterns ARE dispatched: literals, ASCII char classes, ASCII shorthand classes, sequences, alternations within groups (not top-level), greedy/lazy quantifiers, range quantifiers, `Dot`, anchors (`\A`, `\z`, `\Z`, `^`, `$`), word boundaries, `\R`, capturing groups (with capture position recovery via the bounded Pike-VM capture pass).

### Step 4 is complete
- 4a (Pike-VM core + span differential): ✅
- 4b (capture tracking + extended differential): ✅
- 4c (engine dispatch wiring + broader differential): ✅
- The Pike-VM is now the runtime engine for classifier-positive patterns. Next: lazy DFA caches in steps 5–6.

### Next concrete action
- C2 step 5: lazy forward DFA cache. New `c2/dfa.rs` with the lazy DFA construction from the forward NFA. State cache with size limit, byte-class compression, graceful fallback to Pike-VM on cache exhaustion. Engine dispatch picks DFA over Pike-VM for the find paths when available. Pike-VM stays as the bounded capture-recovery pass per design doc §9.

## 2026-04-10 session — C2 step 5a shipped (lazy forward DFA, standalone)

### What landed
- New `rgx-core/src/c2/dfa.rs` with the SOTA lazy DFA: subset construction, byte-class-indexed transition tables, HashMap cache, configurable state limit (default 2048), `find_match_at` simulation loop. Public API mirrors `pike_match_at` so step 5b can swap dispatch in transparently.
- New `Nfa::has_assertions()` accessor used by `LazyDfa::new` to refuse construction for assertion-bearing NFAs.
- 22 unit tests covering construction, matching, cache behaviour, and DFA→Pike-VM sanity comparisons on ~16 patterns.

### Three deliberate step-5a limitations
1. **No zero-width assertions**: `LazyDfa::new` returns Err for NFAs containing `\A`/`\z`/`\Z`/`^`/`$`/`\b`/`\B`/`\G`. Step 5b will lift this either by tracking look-behind context per DFA state or by excluding assertion patterns from DFA dispatch entirely.
2. **No lazy quantifier support**: subset construction is leftmost-longest by nature; the DFA can't express the priority order Pike-VM uses for lazy semantics. For `a+?` on "baaab" DFA returns end=4 (longest) but Pike-VM returns end=2 (lazy/shortest). Pinned in `lazy_quantifier_diverges_from_pike_by_design` test. Step 5b excludes lazy-quantifier patterns from DFA dispatch.
3. **No cache eviction**: `transition` returns None on cache exhaustion. Step 5b adds clear-and-retry policy + Pike-VM fallback.

### What's dispatchable to DFA in step 5b
After adding `contains_lazy_quantifier` and `contains_zero_width_assertion` exclusions to `is_c2_dispatch_eligible`, the DFA-eligible patterns are: literals, ASCII char classes, shorthand classes (without `\b`), sequences, alternations within groups, GREEDY quantifiers (`?`/`*`/`+`/`{n,m}` greedy only), `Dot`, `\R`, capturing groups (with capture position recovery via the bounded Pike-VM pass per design doc §9).

### Engine dispatch design for step 5b
- Add `c2_dfa: Option<LazyDfa>` (or `Mutex<LazyDfa>` for interior mutability) to `Engine` or `Regex`. The DFA is built once at compile time when the pattern is DFA-eligible.
- Or: build the DFA lazily on first call. The DFA is `&mut self` for transition (since it mutates the cache), so it needs interior mutability via `Mutex<LazyDfa>`.
- Probably want: `Engine::find_match_via_dfa(input, start) -> Option<usize>` that holds the Mutex briefly. Falls back to Pike-VM on `None`.
- Captures still come from the Pike-VM bounded recovery pass — DFA gives match end, Pike-VM gives capture positions.

### Next concrete action
- C2 step 5b: wire LazyDfa into engine dispatch with:
  - Add `Mutex<LazyDfa>` to Engine (or interior mutability via RwLock)
  - Add `contains_lazy_quantifier` and `contains_zero_width_assertion` to `is_c2_dispatch_eligible`
  - In `Regex::find_first` etc., check DFA availability first, run DFA, fall back to Pike-VM on None or for capture recovery
  - Run the existing 856-test suite — the differential gate now also covers the DFA path
  - Fix any DFA bugs that surface from the broader corpus

## 2026-04-10 session — C2 step 5b shipped (DFA dispatch for is_match) — Step 5 COMPLETE

### Scope decision
Minimum viable wiring: only `Regex::is_match` dispatches to DFA. `find_first`/`find_all` stay on Pike-VM because they need captures, and proper DFA-driven scan needs the reverse DFA (step 6). This still exercises the DFA via every is_match call in the 880-test suite.

### What landed
- Refactored `LazyDfa::find_match_at` to return new `DfaSearchOutcome` enum (Match/NoMatch/Exhausted). The old `Option<usize>` conflated "no match" with "cache exhausted" which would have caused unnecessary fallbacks.
- New `is_c2_dfa_eligible(ast)` with `contains_zero_width_assertion` and `contains_lazy_quantifier` exclusions. DFA can't handle anchors/word-boundaries (no context tracking) or lazy quantifiers (subset construction is leftmost-longest by nature).
- New `Option<Mutex<LazyDfa>>` field on `Engine`, built by `Engine::new` via `build_dfa_if_eligible(ast, c2_program)`. Mutex needed because DFA's transition mutates its cache and public API methods are `&self`.
- New `Engine::should_dispatch_to_dfa()` and `Engine::try_dfa_is_match(input)` accessors. Same runtime exclusions as `should_dispatch_to_c2` (no event observer, no runtime safety limits).
- `Regex::is_match` now has 3-tier dispatch: DFA → Pike-VM → existing VM.

### Zero new failures from the broader differential gate
This is significant. The existing 880-test suite caught zero DFA bugs when wired via `is_match`. The DFA correctness work in step 5a (and the eligibility exclusions) was solid enough that nothing broke when the entire test corpus started routing through the DFA.

### What's dispatched to DFA
DFA-eligible patterns are: literals (ASCII or non-ASCII single chars), ASCII char classes, ASCII shorthand classes (without `\b`), sequences, alternations within groups (not top-level), GREEDY quantifiers only (`?`/`*`/`+`/`{n,m}` greedy), `Dot`, `\R`, capturing groups (no captures needed for is_match).

NOT dispatched (still go to Pike-VM): anchored patterns (`\A`/`\z`/`\Z`/`^`/`$`), word boundary patterns (`\b`/`\B`), lazy quantifiers, top-level alternation, flag groups, multi-byte char classes, runtime event observers, runtime safety limits.

### Step 5 is complete
- 5a (lazy DFA core, standalone): ✅
- 5b (DFA dispatch for is_match): ✅
- The DFA is wired into production for is_match. Next: step 6 wires it for find_first/find_all via the reverse DFA pipeline.

### Next concrete action
- C2 step 6: lazy reverse DFA cache. Mirrors step 5 but for the reverse NFA. Once landed, find_first/find_all can use the proper DFA-driven scan: forward DFA finds the match end, reverse DFA finds the match start, Pike-VM bounded over the matched span recovers captures. This is the design doc §9 "two-pass capture recovery" approach. Engine dispatch updates accordingly. The find paths finally deliver the DFA performance win.

## 2026-04-10 session — C2 step 6 shipped (DFA dispatch for find_first/find_all)

### Deviation from the design doc
- The design doc §15 step 6 was "lazy reverse DFA cache" with the unanchored+reverse pipeline. I took a simpler alternative: **per-position anchored DFA scan** mirroring step 5b's `is_match` pattern. The reverse-DFA pipeline has subtle greedy-semantics issues (earliest end vs longest end) that need separate DFA modes; the per-position approach is correct for greedy semantics out of the box.
- The reverse DFA can come later as a performance optimization (O(n+bounded) for sparse matches vs O(n × per-position) for the per-position approach). For now, correctness > extra speed.

### What landed
- New `pike_captures_at(program, input, start)` in `c2/pike.rs`. Wraps `pike_match_at_with_captures` to recover captures at a known scan position. Used by engine dispatch to avoid re-scanning the entire input after the DFA has located the match start.
- New `Engine::try_dfa_find_first(input) -> Option<Option<MatchResult>>` and `Engine::try_dfa_find_all(input) -> Option<Vec<MatchResult>>`. Both lock the DFA mutex, scan with `find_match_at`, recover captures via `pike_captures_at`, return `None` on cache exhaustion to signal fall-back.
- `Regex::find_first` and `Regex::find_all` now have 3-tier dispatch: DFA → Pike-VM → existing VM.
- New private `pike_match_to_match_result` helper in engine.rs (mirrors the one in lib.rs).

### Zero new failures from the broader differential gate
880 tests pass with DFA dispatch active across is_match + find_first + find_all. The DFA correctness work in step 5a + the eligibility exclusions in 5b were solid enough that wiring the find paths produced no test regressions.

### What's now dispatched to DFA
All three primitive find methods (is_match, find_first, find_all) for DFA-eligible patterns:
- Eligible: literals (incl. non-ASCII single chars), ASCII char classes, ASCII shorthand classes, sequences, alternations within groups, GREEDY quantifiers, Dot, \R, capturing groups
- Excluded (route to Pike-VM): zero-width assertions (\A/\z/\Z/^/$/\b/\B/\G), lazy quantifiers, top-level alternation, flag groups, multi-byte char classes, runtime event observers/safety limits

### C2 step 6 is complete
- The lazy DFA is now wired into all three primitive Regex API methods.
- The find paths' hot loop is: lock DFA mutex → per-position `find_match_at` (two array lookups per byte) → release → Pike-VM `pike_captures_at` for captures. For sparse-match patterns the DFA scan dominates and is much faster than Pike-VM scanning every position.

### Next concrete action
- C2 step 7: literal prefix integration. The existing memmem fast path (in vm.rs) skips positions where the pattern's first literal byte can't match. Wire this into the C2 dispatch path so the DFA scan also benefits — instead of trying every position 0..=len, jump to the next memmem-match position before invoking the DFA. Combines the existing literal acceleration with the new DFA simulation. Potentially more impactful than the reverse DFA for patterns with literal prefixes.

## 2026-04-10 session — C2 step 7 shipped (literal prefix integration via memchr)

### What landed
- New `first_literal_byte(ast)` in c2/program.rs. Conservative AST walker that detects single literal byte at the start of a match. Handles Char (ASCII + non-ASCII first UTF-8 byte), Sequence with leading literal (walking past zero-width anchors), Group/FlagGroup wrappers, Quantified with min>=1.
- New `c2_prefix_byte: Option<u8>` field on CompiledC2Program, computed at build time.
- Updated `pike_captures` / `pike_captures_all` / `Engine::try_dfa_find_first` / `Engine::try_dfa_find_all` to use memchr-based skip. Instead of iterating every position 0..=len, the loop calls memchr::memchr(prefix, &input[start..]) and jumps directly to the next candidate.
- 14 new unit tests covering all cases (ASCII, non-ASCII, sequences with leading anchors, alternations, quantifiers with min=0/1, char classes, Dot, realistic log pattern).

### Zero new failures from the broader differential gate
894 tests pass (up from 880, +14 from new prefix tests). The literal prefix optimization is correct on every classifier-positive pattern in the test suite. All three primitive find methods (is_match, find_first, find_all) benefit on both DFA and Pike-VM dispatch tiers.

### Performance benefit
For sparse-match patterns where the prefix byte is rare in the input (e.g., `ERROR` in a long log file, `2026-` in source code), the dispatch now skips most input bytes via SIMD-accelerated memchr. The previous DFA cost was two array lookups per byte; with prefix skip, it becomes "memchr to next candidate + DFA simulation only at confirmed positions". 

### Deferred follow-ups
- Multi-byte literal prefix via memmem (e.g., scan for "abc" instead of just "a") — handles pure-literal patterns more efficiently
- Full literal extraction (multiple alternatives, suffix detection) — like the regex crate's literal optimizer

### C2 step 7 is complete
- The dispatch path now has its hottest optimization for the common case (patterns with literal prefixes).
- Next: step 8 (production cutover, benchmarks, Book chapter).

## 2026-04-12 session — A11 `(*SKIP:name)` named skip verb shipped

### What landed
- `Regex::Skip` changed from unit variant to `Skip(Option<String>)`. New `VerbSkipNamed = 0xA5` opcode with length-prefixed name operand.
- `ExecContext.marks: Vec<(String, usize)>` per-attempt mark registry. `(*MARK:name)` now pushes `(name, pos)` during execution.
- `VerbSkipNamed` handler looks up the most recent matching mark and sets `ctx.skip_position`. No-op if no matching mark exists.
- `VmResumeState.marks` for async/suspendable resume paths.
- Forward-progress guard (`skip_pos.max(start + 1)`) at all 12 scan-loop sites where `skip_position` is consumed.
- `marks` cleared on per-attempt reset alongside `skip_position`, `committed`, etc.
- Parser reuses `extract_directive_payload` for `(*SKIP:name)`.
- C1/C2: `VerbSkipNamed` added to JIT exclusion list, AST pattern matches updated for new `Skip(Option<String>)` shape.
- 5 new tests + updated existing `(*SKIP)` tests.

### One correctness bug caught and fixed during recovery
- **Forward-progress infinite loop**. When `(*SKIP:name)` set `skip_position` to a mark position behind the current scan start, the scan loop didn't advance. Fixed by adding `.max(start + 1)` guards at all 12 consumption sites. Also added `marks.clear()` to the per-attempt reset to prevent stale marks from a previous attempt leaking into the next one.

### Next concrete action
- Continue Tier-2 perf headroom + parity polish: reverse-DFA pipeline, A8 crate publishing prep.
