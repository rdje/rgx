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
  - `docs/USER_GUIDE.md`
  - `ROADMAP.md`
  - `docs/CAPABILITY_MATRIX.md`
  - `docs/PCRE2_COMPATIBILITY_MATRIX.md`
  - `docs/PARSER_CONTRACT.md`

## Fast resume checklist
1. Read this file top-to-bottom.
2. Check current working tree and branch state (`git --no-pager status --short`).
3. Read newest entries in `CHANGES.md` and `ROADMAP.md`.
4. Confirm current known gaps and active priorities from:
   - `DEVELOPMENT_NOTES.md`
   - `docs/PCRE2_COMPATIBILITY_MATRIX.md`
5. Continue with the next concrete task, then update this file before commit workflow.

## Persistent workflow agreements with user
- Always run `git --no-pager status` before every commit.
- Stage from that exact status output (no hidden extras).
- Use `git_message_brief.txt` with `git commit -F git_message_brief.txt`.
- Run `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` before commit and fix all clippy errors first (warnings tolerated for now).
- Include `Co-Authored-By: Oz <oz-agent@warp.dev>` in commit messages.
- After commit:
  - clear `git_message_brief.txt`
  - verify `git_message_brief.txt` stays untracked (`TRACKED:1` check).

## Current technical snapshot
- Parity program with PCRE2 differential tests is active and operational in `rgx-bench/tests/pcre2_parity.rs`.
- End-anchor (`$`) parity mismatch was fixed and reclassified as supported.
- Absolute text-anchor parity for `\A`, `\Z`, and `\z` is now fixed end-to-end, including runtime execution, parser-path/API regression coverage, PCRE2 differential tests, and direct CLI smoke verification.
- `{n,m}` range-quantifier scanning/earliest-match parity gap has now been fixed and reclassified as supported.
- Unbounded range quantifier (`{n,}`) parity is now differential-tested and aligned for scanning and suffix-sensitive behavior.
- Negated shorthand character-class parity for `\D`, `\W`, and `\S` is now fixed end-to-end, including quantified VM execution, API regressions, differential parity tests, and direct CLI smoke coverage.
- Capability and parser-boundary guardrails are actively enforced in:
  - `rgx-core/src/lib.rs`
  - `rgx-core/src/parsing.rs`
  - `docs/CAPABILITY_MATRIX.md`
  - `docs/PCRE2_COMPATIBILITY_MATRIX.md`

## Next likely tasks
- Continue expanding differential parity coverage for additional backtracking-sensitive quantifier and grouped-pattern combinations.
- Continue closing remaining parsed-but-unintegrated parity gaps (backreferences, recursion, conditionals).
- Maintain strict compile-boundary explicit errors for parsed-but-unintegrated advanced features.

## Session memory entries (newest first)
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
