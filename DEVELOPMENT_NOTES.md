# DEVELOPMENT NOTES
Technical knowledge base for day-to-day engineering work in rgx.

## Purpose
- Preserve implementation context across sessions
- Document practical architecture insights and constraints
- Keep a concise list of known gaps and immediate priorities

## Strategic goal clarification
- rgx targets practical parity with PCRE2 for:
  - feature coverage
  - runtime performance
  - matching accuracy
- rgx also targets broader code-block language support over time, but the current preferred inline-language direction is:
  - first-class source-body languages: JavaScript, Lua, and Rhai
  - advanced reference-style backends: native and wasm
  - explicitly deferred heavier runtimes for later evaluation: Julia and Python
  - all with explicit safety and sandbox guarantees

## Current architecture (practical view)
Pipeline in `rgx-core`:
1. `lexer.rs` tokenizes pattern text
2. `parser.rs` builds AST
3. `compiler.rs` + `vm.rs::OptimizingCompiler` generate VM bytecode
4. `vm.rs::RegexVM` executes against input text
5. `engine.rs` and `lib.rs` expose user-facing API

## What is currently reliable
- Core compile-and-run flow for basic regex patterns
- Parser-independent compile-and-run flow from AST via `Compiler::compile_ast` and `Regex::from_ast`
- VM execution paths for literals, alternation, anchors (including `\\A`, `\\Z`, `\\z`), word boundaries, shorthand/custom character classes (including `\\D`, `\\W`, `\\S`), and greedy/lazy/possessive quantifiers
- Unicode property classes (`\\p{...}`, `\\P{...}`) are now integrated through the compiler/VM path, including invalid-property compile errors and representative PCRE2 differential coverage
- AST-first VM/compiler support for positive and negative lookahead/lookbehind assertions
- Parser-path support for positive/negative lookahead and lookbehind syntax
- Parser-path support for code-block syntax tokenization/parsing (`(?{lang:code})`)
- Public-path predicate execution for `(?{lua:...})`, `(?{js:...})` / `(?{javascript:...})`, and `(?{rhai:...})` in `ExecutionMode::Safe` / `ExecutionMode::Full` when the matching cargo feature is enabled
- The CLI now exposes host-provided variables for code-block-enabled patterns through repeated `--var NAME=VALUE`, can register named wasm modules through repeatable `--wasm-module NAME=PATH`, and can optionally show top-level branch plus winning-path code-block details via `--show-details`
- Public-path native callback execution for `(?{native:...})` in `ExecutionMode::Full` through `Regex::register_native(...)` on the Rust API path
- Public-path wasm module execution for `(?{wasm:...})` in `ExecutionMode::Safe` / `ExecutionMode::Full` through `Regex::register_wasm_module(...)` on the Rust API path
- Host-provided execution variables can now be registered through `Regex::set_variable(...)` and are snapshotted into each code-block evaluation
- Wasm predicates can now read current position, full input text, numbered captures, named captures, and host-provided variables through `rgx` host imports while keeping the exported `() -> i32` predicate entrypoint stable
- Code-block execution contexts now also expose current match start/end/length metadata plus top-level branch number when available across native/Lua/JavaScript/Rhai, with matching wasm host imports for the same metadata
- Wasm modules can now also emit winning-path numeric and replacement payloads through `rgx.emit_numeric(...)` and `rgx.emit_replacement(...)` while still using the exported `() -> i32` predicate to decide success/failure
- Code-block execution contexts now expose current overall match text, numbered captures, named captures, and host-provided variables to the execution layer
- Code blocks now participate in normal VM backtracking and can be used inside the supported regex pipeline rather than being parser-only scaffolding
- Public match results now expose `code_result`, which preserves the last winning-path numeric or replacement value from Lua/JavaScript/Rhai/native/wasm code blocks
- Public numeric-result helper APIs now exist through `Regex::find_first_numeric_with_code(...)` and `Regex::find_all_numeric_with_code(...)`, which collect winning-path `Numeric(f64)` payloads in match order and skip non-numeric matches
- Public replacement-oriented APIs now exist through `Regex::replace_first_with_code(...)` and `Regex::replace_all_with_code(...)`, which consume winning-path `Replacement(String)` payloads and leave non-replacement matches unchanged
- Parser-path support for recursion syntax tokenization/parsing (`(?R)`, `(?1)`, `(?&name)`)
- Numeric backreferences (`\1`, `\2`, ...) are now integrated through the compiler/VM path, including capture-aware runtime matching and compile-time rejection for references to missing capture groups
- Parser-path support for conditional syntax tokenization/parsing:
  - group-exists forms (`(?(1)...)`)
  - relative-group-exists forms (`(?(+1)...)`, `(?(-1)...)`)
  - named-group-exists forms (`(?(<name>)...)`, `(?(name)...)`)
  - lookaround condition forms (`(?(?=...)...)`, `(?(?!...)...)`, `(?(?<=...)...)`, `(?(?<!...)...)`)
- Conditional runtime semantics are now integrated through the compiler/VM path, including missing-group and missing-name compile-time validation plus API/parity coverage
- Relative conditional group references now parse into dedicated AST nodes on both parser backends and resolve to shipped runtime behavior through compile-time rewriting to absolute group-exists checks
- Recursion / subroutine runtime semantics are now integrated through the compiler/VM path for `(?R)`, `(?1)`, and `(?&name)`, including explicit compile-time validation for missing numbered and named recursion targets plus API/parity coverage
- API/conformance guardrails explicitly verify compile-boundary errors for invalid Unicode property classes and disallowed code-block modes/languages
- Public API (`Regex::compile`, `is_match`, `find_first`, `find_all`) connected to the compiler/VM path
- Public match results expose top-level alternation branch choice as a 1-based `matched_branch_number`
- Parser support for capturing groups, non-capturing groups `(?:...)`, named groups `(?<name>...)`, and atomic groups `(?>...)`
- Atomic-group runtime semantics implemented to block backtracking into successful atomic groups
- Parser-path support for possessive quantifiers (`*+`, `++`, `?+`, `{n,m}+`) now lowers through the same atomic-group semantics used by explicit `(?>...)`
- A formal parser interoperability contract is maintained in the repo
- PGEN parser issue handoff is now constrained to the published upstream reporting protocol
- Live shipped-vs-scaffolded matrix at `docs/CAPABILITY_MATRIX.md`
- Live rgx-vs-PCRE2 parity matrix at `docs/PCRE2_COMPATIBILITY_MATRIX.md`
- Parser conformance harness scaffolding is in place
- Differential parity harness baseline in `rgx-bench/tests/pcre2_parity.rs`
- Differential known-gap parity checks currently cover recursion
- Differential supported-syntax parity now includes representative Unicode property classes
- Differential supported-syntax parity now includes numeric backreferences, including backtracking-sensitive and no-match cases
- Differential parity now verifies `{n,m}` scanning/earliest-match behavior against PCRE2
- Differential supported-syntax parity now includes absolute text anchors (`\A`, `\Z`, `\z`) including final-newline behavior for `\Z`
- Differential supported-syntax parity now includes bounded-range suffix backtracking scenarios (`{2,3}3`) in both first-match and find-all coverage
- Differential supported-syntax parity now also includes unbounded range coverage (`{n,}`) including suffix-sensitive `{n,}3` behavior
- Differential supported-syntax parity now includes dedicated suffix-backtracking guardrails for greedy `*`, `+`, and `?` quantifiers
- Differential supported-syntax parity now includes lazy quantifiers and lazy counted-range suffix behavior
- Differential supported-syntax parity now includes possessive quantifiers, including both success cases and suffix-sensitive no-backtracking behavior
- Differential supported-syntax parity now includes negated shorthand character classes (`\D`, `\W`, `\S`) for first-match, find-all, and explicit no-match behavior
- Parser-path regressions now explicitly cover suffix backtracking for greedy `*`, `+`, and `?` quantifiers
- Parser-path regressions now explicitly cover lazy `??`, `*?`, `+?`, `{n,m}?`, and `{n,}?`
- Parser-path regressions now explicitly cover possessive `*+`, `++`, `?+`, and `{n,m}+`
- The default `rgx-core` build now drives a real PGEN-backed parser adapter in `rgx-core/src/parsing.rs` instead of a recursive-descent placeholder path
- The PGEN adapter currently converts the stable PGEN regex AST dump into RGX AST nodes for:
  - groups
  - lookarounds
  - conditionals
  - quantifiers / concatenation / alternation structure
  - possessive quantifiers via atomic-wrapped quantified RGX AST lowering
  - leaf atoms via exact-slice fallback into the recursive-descent parser so RGX AST semantics stay aligned
- The local backend choice under the default PGEN-backed build is intentionally controlled by one constant (`PGEN_FEATURE_BACKEND`) so RGX can force either the real PGEN backend or the recursive-descent reference backend without changing callers
- `cargo check -p rgx-core --features javascript` and `cargo check -p rgx-core --features all-languages` now pass again
- Local-first CI path now exists:
  - `.github/workflows/ci.yml` delegates to `./scripts/run-local-ci.sh`
  - `./scripts/run-local-ci.sh` now covers the default PGEN-backed workspace plus the local `rgx-core` feature matrix (`pgen-parser`, `lua`, `javascript`, `rhai`, `wasm`, `all-languages`) and `rgx-cli --features pgen-parser`
  - shared CI is now expected to initialize the committed `subs/pgen` submodule before running that same validation path
  - `scripts/check-ci-paths.sh` verifies CI-critical paths are git-controlled, rejects absolute filesystem paths in Rust source and CI execution files, and reports compile-time `include!`-style macro usage
- `Cargo.lock` is intentionally tracked so local validation and GitHub CI share the same dependency resolution
- Core/CLI logging now supports UVM-style verbosity control and file routing:
  - `RGX_VERBOSITY=none|low|medium|high|debug`
  - `rgx-cli --verbosity <level>` with legacy aliases `--debug` (high) and `--trace` (debug)
  - `RGX_TRACE_FILE` / `rgx-cli --trace-log` for sink routing into `trace.log`
  - structured trace helpers for function entry/exit and decision reasoning (`trace_enter!`, `trace_exit!`, `trace_decision!`)
- Parser-path tracing now covers parser frontend boundaries:
  - parser stack hotspots (`Parser::new/parse/parse_alternation/parse_sequence/parse_quantified/parse_atom`)
  - parser token-cursor boundary (`Parser::advance`) including lexer-fetch decision and consumed/next token summaries
  - parser token-inspection helpers (`Parser::peek`, `Parser::current_token_snapshot`, `Parser::regex_kind`) with token-availability decisions and snapshot exits
  - compile-time parsing entry (`parsing::parse_pattern`) and trait adapter path (`RecursiveDescentParser::parse_pattern`)
  - backend-selection and parse-boundary decision logs visible at medium/high/debug verbosity levels
- Lexer-path tracing now covers tokenization boundaries and high-branch parse helpers:
  - token flow (`Lexer::new`, `Lexer::next_token`, `Lexer::parse_escape`)
  - escape helpers (`parse_unicode_class`, `parse_backreference`, `parse_hex_escape`, `parse_octal_escape`) with explicit decision/error exits
  - quantifier and class parsing (`parse_star`, `parse_plus`, `parse_question`, `parse_repeat_quantifier`, `parse_character_class`)
  - group/conditional parsing boundaries (`parse_group`, `parse_conditional_start`, `parse_conditional_subexpression_ast`)
- API/engine-path tracing now covers high-level public dispatch and boundary decisions:
  - API boundaries in `rgx-core/src/lib.rs` (`Regex::compile`, `with_mode`, `from_ast`, `from_ast_with_mode`, `find_all`, `find_first`, `is_match`)
  - engine boundaries in `rgx-core/src/engine.rs` (`Engine::new`, `find_all`, `find_first`, `is_match`)
  - explicit decision logs for UTF-8 validity gates and boolean/cardinality outcome summaries
- Execution-layer tracing now covers code-execution runtime boundaries:
  - context and lookup boundaries in `rgx-core/src/execution.rs` (`ExecContext::new/current_match/group/named/variable/variables_snapshot`)
  - callback-registry boundaries (`NativeCallbackRegistry::new/register/call/has`)
  - execution-variable registry boundaries (`ExecutionVariableRegistry::new/set/snapshot`)
  - manager dispatch boundaries (`ExecutionManager::new/execute/register_native/register_wasm_module/set_variable/variable_snapshot/is_language_available`)
  - explicit decision logs for callback replacement/lookup outcomes and language backend routing choices
- VM compile-path tracing now covers optimizing compiler boundaries:
  - `OptimizingCompiler::new` and `OptimizingCompiler::compile` in `rgx-core/src/vm.rs`
  - compile-boundary summaries include AST kind, bytecode size, string/class counts, group count, and JIT-worthiness
  - decision logging now exposes post-analysis JIT heuristic outcome (`jit_worthy`) with supporting stats
- VM runtime initialization tracing now covers startup boundaries:
  - `RegexVM::new` and `RegexVM::detect_simd_support` emit structured entry/exit traces
  - startup traces include bytecode/context metadata and detected SIMD capabilities (`sse2`, `avx2`, `neon`)
  - decision logging now explicitly indicates whether SIMD capability is available on the runtime host
- CLI-path tracing now covers top-level command execution boundaries:
  - `rgx-cli/src/main.rs` `main()` emits structured entry/exit traces and branch decisions
  - branch decisions include execution mode path (`pure` vs others), input source (stdin vs positional arg), and boolean match outcome
  - tracing is emitted after log environment initialization to preserve expected verbosity semantics
- Compiler/parser utility tracing now covers constructor and capability-selection boundaries:
  - compiler constructors in `rgx-core/src/compiler.rs` (`Compiler::new`, `Compiler::with_mode`)
  - parser utility helpers in `rgx-core/src/parsing.rs` (`parser_name`, `parser_capabilities`, `ParserConfig::default`)
  - parser object constructor/capability boundaries (`RecursiveDescentParser::*`, feature-gated `PgenParser::*`)
  - capability traces explicitly expose advanced-feature flags (e.g., `perl_advanced`) for backend-selection diagnostics
- AST/token utility tracing now covers constructor and parse-context boundaries:
  - AST utilities in `rgx-core/src/ast.rs` (`CharRange::single/range`, `ParseContext::new/next_group_number/register_named_group/get_named_group`)
  - token/position utilities in `rgx-core/src/token.rs` (`Position::new/start`, `TokenWithPos::new`)
  - decision traces now expose range-order checks, named-group replacement behavior, and named-group lookup-hit outcomes
- VM test suite coverage for core behavior

## Parser interoperability contract (RGX <-> PGEN)
- Current downstream review is constrained to the published PGEN regex contract surface.
- Current conformance baseline:
  - fixture parity checks between active parser and recursive-descent reference output
  - parser AST metadata invariants required by downstream compiler/runtime
  - parse-fail error mapping consistency (`RgxError::Compile`)
  - explicit parse-success/compile-fail guardrails for unintegrated runtime features
- Current widened local reference fixtures now cover:
  - empty patterns
  - anchors (`$`, `\A`, `\Z`, `\z`)
  - range quantifiers
  - possessive quantifiers
  - shorthand / Unicode property classes
  - group families
  - lookarounds
  - conditionals with and without false branches, including relative group-exists transport
  - code-block tags (`lua`, `js`, `javascript`, `rhai`, `native`, `wasm`)
  - recursion and numeric backreferences
- Suspected PGEN parser misbehavior should be reported with the structured bundle described by `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`.
- The current live PGEN regex caveats are narrower than the original complaint set:
  - the contract is now integration-ready for basic RGX rollout,
  - but AST consumers still need release pinning because the stable JSON schema does not freeze detailed `rule_name` taxonomy across upgrades,
  - and the current embedded code-block contract is now structurally specified for opaque generic / `lua` / `js` / `javascript` payloads, while RGX now also ships a local `rhai` backend on top of generic tag transport pending explicit upstream marker publication; the published contract still excludes `native` / `wasm` support.
- The current PGEN-backed integration now depends on the committed `subs/pgen` submodule:
  - the pinned fix commit is `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77`
  - fresh clones must initialize submodules before building (`git submodule update --init --recursive`)
  - hosted CI for the private submodule may need an explicit token such as `RGX_SUBMODULES_TOKEN` if the default `GITHUB_TOKEN` cannot read `rdje/pgen`
- The current forwardable recommendation for PGEN lives in `PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md`:
  - keep parser guarantees structural,
  - treat `lua` / `js` / `javascript` and `rhai` as source-body tags best validated by the downstream backend,
  - and keep `native` / `wasm` reference-shaped rather than implying arbitrary inline source support.
- Any backend swap that changes parser behavior must update the parser contract statement, conformance tests, and changelog entries together.

## Known engineering gaps
- Parser/VM support for advanced regex syntax still has meaningful remaining gaps in newer PCRE families beyond the currently covered recursion, conditional condition forms, Unicode property classes, lookaround syntax, and possessive quantifiers
- Native registration is still Rust-API-only, while wasm registration now also has a file-backed CLI path through `--wasm-module NAME=PATH`
- The wasm ABI now exposes position/match-metadata/text/numbered-capture/named-capture/variable imports plus first richer-result emission imports (`emit_numeric`, `emit_replacement`)
- The first richer non-boolean result slice now includes match metadata (`MatchResult.code_result`) plus dedicated numeric-result and replacement-oriented Rust APIs across Lua/JavaScript/Rhai/native/wasm, but richer wasm ABI work beyond this initial emission slice remains open
- The shipped inline-language lane is now tighter: Lua, JavaScript, and Rhai all accept either bare expression bodies or explicit `return ...` bodies, and Lua/JavaScript/Rhai helper-API behavior is covered explicitly in `rgx-core` regression tests
- The CLI no longer does a boolean pre-check before collecting matches, which matters for code-block patterns because it avoids one extra round of callback/script execution on successful runs
- The current product direction is to avoid using wasm as the benchmark for everyday inline code-block ergonomics; it remains supported, but future inline-language prioritization should compare against the shipped Lua/JavaScript/Rhai lane first
- VM/compiler contain declared advanced features/opcodes that are only partial or placeholder
- Julia/Python embedding remain intentionally deferred until after the Lua/JavaScript/Rhai direction is clearer
- JavaScript/WASM root modules remain scaffold-level in user-facing flow even though feature builds now compile
- Quick benchmark trend capture is now part of the default validation loop through `scripts/capture-benchmark-trends.sh`; each run archives a timestamped local snapshot and reports deltas versus the most recent prior archived capture, while deeper release-profile tracking remains a separate follow-up

## Immediate priorities
1. Expand and maintain the PCRE2 compatibility matrix with explicit exceptions/gaps and executable differential tests
2. Expand differential and integration tests to improve semantic parity and accuracy confidence
3. Deepen the new quick benchmark-trend capture into a fuller release-profile / longitudinal comparison story and prioritize measurable wins
4. Expand parser contract and conformance fixtures to reduce PGEN integration risk
5. Exercise the eventual real PGEN backend using the published PGEN reporting protocol so parser bugs can be handed upstream cleanly
6. Parser completeness for advanced grouping/assertion/code-block syntax (in parallel with PGEN readiness), including newer PCRE advanced families beyond the now-shipped relative conditional references
7. Remove/finish placeholder VM/compiler paths and TODO opcode branches
8. Expand the staged code-block rollout with the preferred inline-language direction in mind: prioritize Lua/JavaScript/Rhai ergonomics first, while treating further wasm ABI/result work as secondary unless product needs force it higher

## Documentation policy
- `CHANGES.md` is the living progress ledger
- `README.md` is the single entry point for project onboarding/navigation and should be updated when objective/onboarding/path maps change (not required on every commit)
- `COMMIT.md` is the authoritative commit-workflow contract and should be followed for every commit
- Commit formatting is intentionally scoped to the RGX workspace packages so optional external parser dependencies do not leak into RGX validation.
- Commit workflow now includes `cargo clippy --workspace --all-targets` with a hard gate: no clippy errors before commit (warnings currently tolerated).
- Keep `subs/pgen/rust` excluded from the root Cargo workspace so the private parser submodule remains a separate project even while RGX builds against it.
- `RUST_CODEBASE_ANALYSIS.md` is the live roadmap-grounded assessment of the Rust workspace and should be updated when implementation status, feature-flag readiness, or roadmap alignment materially changes.
- `MEMORY.md` is the live cross-session continuity memory and must be updated after completed tasks before commit workflow
- `ROADMAP.md` is the live forward-looking planning tracker
- `docs/USER_GUIDE.md` is the live end-user guide with layered depth
- The parser interoperability contract is the parser-boundary source of truth
- `docs/CAPABILITY_MATRIX.md` is the shipped-vs-scaffolded capability source of truth
- `docs/PCRE2_COMPATIBILITY_MATRIX.md` is the rgx-vs-PCRE2 parity source of truth
- This file is for technical understanding and implementation notes
- `PROJECT_VISION.md` is aspirational; it should not be used to infer shipped features
