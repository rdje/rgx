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
- VM execution paths for literals, alternation, anchors, word boundaries, basic classes, and core quantifiers
- AST-first VM/compiler support for positive and negative lookahead/lookbehind assertions
- Parser-path support for positive/negative lookahead and lookbehind syntax
- Parser-path support for code-block syntax tokenization/parsing (`(?{lang:code})`)
- Parser-path support for recursion syntax tokenization/parsing (`(?R)`, `(?1)`, `(?&name)`)
- Parser-path support for conditional syntax tokenization/parsing:
  - group-exists forms (`(?(1)...)`)
  - named-group-exists forms (`(?(<name>)...)`, `(?(name)...)`)
  - lookaround condition forms (`(?(?=...)...)`, `(?(?!...)...)`, `(?(?<=...)...)`, `(?(?<!...)...)`)
- API/conformance guardrails explicitly verify compile-boundary errors for parsed-but-unintegrated recursion and conditional syntax variants
- Public API (`Regex::compile`, `is_match`, `find_first`, `find_all`) connected to the compiler/VM path
- Public match results expose top-level alternation branch choice as a 1-based `matched_branch_number`
- Parser support for capturing groups, non-capturing groups `(?:...)`, named groups `(?<name>...)`, and atomic groups `(?>...)`
- Atomic-group runtime semantics implemented to block backtracking into successful atomic groups
- Formal parser interoperability contract at `docs/PARSER_CONTRACT.md`
- Live shipped-vs-scaffolded matrix at `docs/CAPABILITY_MATRIX.md`
- Live rgx-vs-PCRE2 parity matrix at `docs/PCRE2_COMPATIBILITY_MATRIX.md`
- Parser conformance harness scaffolding in `rgx-core/src/parsing.rs` tests
- Differential parity harness baseline in `rgx-bench/tests/pcre2_parity.rs`
- Differential known-gap parity checks currently cover backreference, recursion, and conditional syntax families
- Differential parity now verifies `{n,m}` scanning/earliest-match behavior against PCRE2
- Differential supported-syntax parity now includes bounded-range suffix backtracking scenarios (`{2,3}3`) in both first-match and find-all coverage
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
- Backreference, recursion, and code-block execution are not yet integrated into the VM runtime path (compile currently returns explicit unsupported errors)
- VM/compiler contain declared advanced features/opcodes that are only partial or placeholder
- Inline code execution infrastructure exists but is not fully integrated into parser-to-VM user path
- JavaScript/WASM modules remain scaffold-level in user-facing flow

## Immediate priorities
1. Expand and maintain the PCRE2 compatibility matrix with explicit exceptions/gaps and executable differential tests
2. Expand differential and integration tests to improve semantic parity and accuracy confidence
3. Track benchmark parity trends against PCRE2 in `rgx-bench` and prioritize measurable wins
4. Expand parser contract and conformance fixtures to reduce PGEN integration risk
5. Parser completeness for advanced grouping/assertion/code-block syntax (in parallel with PGEN readiness)
6. Remove/finish placeholder VM/compiler paths and TODO opcode branches
7. Define staged rollout for multi-language code-block runtime support with shared safety controls

## Documentation policy
- `CHANGES.md` is the living progress ledger
- `MEMORY.md` is the live cross-session continuity memory and must be updated after completed tasks before commit workflow
- `ROADMAP.md` is the live forward-looking planning tracker
- `docs/USER_GUIDE.md` is the live end-user guide with layered depth
- `docs/PARSER_CONTRACT.md` is the parser interoperability source of truth
- `docs/CAPABILITY_MATRIX.md` is the shipped-vs-scaffolded capability source of truth
- `docs/PCRE2_COMPATIBILITY_MATRIX.md` is the rgx-vs-PCRE2 parity source of truth
- This file is for technical understanding and implementation notes
- `PROJECT_VISION.md` is aspirational; it should not be used to infer shipped features
