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
- rgx also targets broader code-block language support over time (e.g., JavaScript, Lua, Julia, and additional runtimes), with explicit safety and sandbox guarantees.

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
- VM execution paths for literals, alternation, anchors (including `\\A`, `\\Z`, `\\z`), word boundaries, shorthand/custom character classes (including `\\D`, `\\W`, `\\S`), and greedy/lazy quantifiers
- AST-first VM/compiler support for positive and negative lookahead/lookbehind assertions
- Parser-path support for positive/negative lookahead and lookbehind syntax
- Parser-path support for code-block syntax tokenization/parsing (`(?{lang:code})`)
- Public-path predicate execution for `(?{lua:...})` and `(?{js:...})` / `(?{javascript:...})` in `ExecutionMode::Safe` / `ExecutionMode::Full` when the matching cargo feature is enabled
- Public-path native callback execution for `(?{native:...})` in `ExecutionMode::Full` through `Regex::register_native(...)` on the Rust API path
- Public-path wasm module execution for `(?{wasm:...})` in `ExecutionMode::Safe` / `ExecutionMode::Full` through `Regex::register_wasm_module(...)` on the Rust API path
- Host-provided execution variables can now be registered through `Regex::set_variable(...)` and are snapshotted into each code-block evaluation
- Wasm predicates can now read current position, full input text, numbered captures, named captures, and host-provided variables through `rgx` host imports while keeping the exported `() -> i32` predicate entrypoint stable
- Code-block execution contexts now expose current overall match text, numbered captures, named captures, and host-provided variables to the execution layer
- Code blocks now participate in normal VM backtracking and can be used inside the supported regex pipeline rather than being parser-only scaffolding
- Public match results now expose `code_result`, which preserves the last winning-path numeric or replacement value from Lua/JavaScript/native code blocks
- Public numeric-result helper APIs now exist through `Regex::find_first_numeric_with_code(...)` and `Regex::find_all_numeric_with_code(...)`, which collect winning-path `Numeric(f64)` payloads in match order and skip non-numeric matches
- Public replacement-oriented APIs now exist through `Regex::replace_first_with_code(...)` and `Regex::replace_all_with_code(...)`, which consume winning-path `Replacement(String)` payloads and leave non-replacement matches unchanged
- Parser-path support for recursion syntax tokenization/parsing (`(?R)`, `(?1)`, `(?&name)`)
- Parser-path support for conditional syntax tokenization/parsing:
  - group-exists forms (`(?(1)...)`)
  - named-group-exists forms (`(?(<name>)...)`, `(?(name)...)`)
  - lookaround condition forms (`(?(?=...)...)`, `(?(?!...)...)`, `(?(?<=...)...)`, `(?(?<!...)...)`)
- API/conformance guardrails explicitly verify compile-boundary errors for parsed-but-unintegrated recursion, conditional syntax, Unicode property classes, and disallowed code-block modes/languages
- Public API (`Regex::compile`, `is_match`, `find_first`, `find_all`) connected to the compiler/VM path
- Public match results expose top-level alternation branch choice as a 1-based `matched_branch_number`
- Parser support for capturing groups, non-capturing groups `(?:...)`, named groups `(?<name>...)`, and atomic groups `(?>...)`
- Atomic-group runtime semantics implemented to block backtracking into successful atomic groups
- Formal parser interoperability contract at `docs/PARSER_CONTRACT.md`
- Live shipped-vs-scaffolded matrix at `docs/CAPABILITY_MATRIX.md`
- Live rgx-vs-PCRE2 parity matrix at `docs/PCRE2_COMPATIBILITY_MATRIX.md`
- Parser conformance harness scaffolding in `rgx-core/src/parsing.rs` tests
- Differential parity harness baseline in `rgx-bench/tests/pcre2_parity.rs`
- Differential known-gap parity checks currently cover backreference, recursion, conditional syntax families, and Unicode property classes
- Differential parity now verifies `{n,m}` scanning/earliest-match behavior against PCRE2
- Differential supported-syntax parity now includes absolute text anchors (`\A`, `\Z`, `\z`) including final-newline behavior for `\Z`
- Differential supported-syntax parity now includes bounded-range suffix backtracking scenarios (`{2,3}3`) in both first-match and find-all coverage
- Differential supported-syntax parity now also includes unbounded range coverage (`{n,}`) including suffix-sensitive `{n,}3` behavior
- Differential supported-syntax parity now includes dedicated suffix-backtracking guardrails for greedy `*`, `+`, and `?` quantifiers
- Differential supported-syntax parity now includes lazy quantifiers and lazy counted-range suffix behavior
- Differential supported-syntax parity now includes negated shorthand character classes (`\D`, `\W`, `\S`) for first-match, find-all, and explicit no-match behavior
- Parser-path regressions now explicitly cover suffix backtracking for greedy `*`, `+`, and `?` quantifiers
- Parser-path regressions now explicitly cover lazy `??`, `*?`, `+?`, `{n,m}?`, and `{n,}?`
- `cargo check -p rgx-core --features javascript` and `cargo check -p rgx-core --features all-languages` now pass again
- Local-first CI path now exists:
  - `.github/workflows/ci.yml` delegates to `./scripts/run-local-ci.sh`
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
- Contract source of truth: `docs/PARSER_CONTRACT.md`
- Integration seam: `rgx-core/src/parsing.rs` (`RegexParser` trait + compile-time parser selection functions)
- Current conformance baseline:
  - fixture parity checks between active parser and recursive-descent reference output
  - parser AST metadata invariants required by downstream compiler/runtime
  - parse-fail error mapping consistency (`RgxError::Compile`)
  - explicit parse-success/compile-fail guardrails for unintegrated runtime features
- Any backend swap that changes parser behavior must update the contract version, conformance tests, and changelog entries together.

## Known engineering gaps
- Parser support for advanced regex syntax remains incomplete beyond the currently covered conditional condition forms and lookaround syntax
- Unicode property classes (`\\p{...}`, `\\P{...}`) are parsed but not yet integrated into VM execution (compile currently returns explicit unsupported errors)
- Backreference, recursion, and conditional syntax are still parsed-but-unintegrated at runtime
- Native and wasm registration are currently Rust-API-only; the CLI does not expose callback/module registration
- The wasm ABI now exposes position/text/numbered-capture/named-capture/variable imports, but richer result handling is still not exposed to wasm modules
- The first richer non-boolean result slice now includes match metadata (`MatchResult.code_result`) plus dedicated numeric-result and replacement-oriented Rust APIs, but wasm richer-result handling remains open
- VM/compiler contain declared advanced features/opcodes that are only partial or placeholder
- JavaScript/WASM root modules remain scaffold-level in user-facing flow even though feature builds now compile
- Local-first CI currently validates only the default-feature workspace path; feature-gated `pgen-parser`, `lua`, `javascript`, `wasm`, and `all-languages` checks still rely on the manual validation matrix

## Immediate priorities
1. Expand and maintain the PCRE2 compatibility matrix with explicit exceptions/gaps and executable differential tests
2. Expand differential and integration tests to improve semantic parity and accuracy confidence
3. Track benchmark parity trends against PCRE2 in `rgx-bench` and prioritize measurable wins
4. Expand parser contract and conformance fixtures to reduce PGEN integration risk
5. Parser completeness for advanced grouping/assertion/code-block syntax (in parallel with PGEN readiness)
6. Remove/finish placeholder VM/compiler paths and TODO opcode branches
7. Expand the staged code-block rollout beyond the current first richer-result plus numeric/replacement helper slice, especially additional wasm result work and any future non-Rust configuration surface

## Documentation policy
- `CHANGES.md` is the living progress ledger
- `README.md` is the single entry point for project onboarding/navigation and should be updated when objective/onboarding/path maps change (not required on every commit)
- `COMMIT.md` is the authoritative commit-workflow contract and should be followed for every commit
- Commit workflow now includes `cargo clippy --workspace --all-targets` with a hard gate: no clippy errors before commit (warnings currently tolerated).
- `RUST_CODEBASE_ANALYSIS.md` is the live roadmap-grounded assessment of the Rust workspace and should be updated when implementation status, feature-flag readiness, or roadmap alignment materially changes.
- `MEMORY.md` is the live cross-session continuity memory and must be updated after completed tasks before commit workflow
- `ROADMAP.md` is the live forward-looking planning tracker
- `docs/USER_GUIDE.md` is the live end-user guide with layered depth
- `docs/PARSER_CONTRACT.md` is the parser interoperability source of truth
- `docs/CAPABILITY_MATRIX.md` is the shipped-vs-scaffolded capability source of truth
- `docs/PCRE2_COMPATIBILITY_MATRIX.md` is the rgx-vs-PCRE2 parity source of truth
- This file is for technical understanding and implementation notes
- `PROJECT_VISION.md` is aspirational; it should not be used to infer shipped features
