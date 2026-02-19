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
- Public API (`Regex::compile`, `is_match`, `find_first`, `find_all`) connected to the compiler/VM path
- Public match results expose top-level alternation branch choice as a 1-based `matched_branch_number`
- Parser support for capturing groups, non-capturing groups `(?:...)`, named groups `(?<name>...)`, and atomic groups `(?>...)`
- Atomic-group runtime semantics implemented to block backtracking into successful atomic groups
- VM test suite coverage for core behavior

## Known engineering gaps
- Parser support for advanced group syntaxes is incomplete
  - conditionals/recursion are not fully wired
- Code-block execution is not yet integrated into the VM runtime path (compile currently returns explicit unsupported error)
- VM/compiler contain declared advanced features/opcodes that are only partial or placeholder
- Inline code execution infrastructure exists but is not fully integrated into parser-to-VM user path
- JavaScript/WASM modules remain scaffold-level in user-facing flow

## Immediate priorities
1. Build and maintain a PCRE2 compatibility matrix with explicit exceptions/gaps
2. Expand differential and integration tests to improve semantic parity and accuracy confidence
3. Track benchmark parity trends against PCRE2 in `rgx-bench` and prioritize measurable wins
4. Parser completeness for advanced grouping/assertion/code-block syntax (in parallel with PGEN readiness)
5. Remove/finish placeholder VM/compiler paths and TODO opcode branches
6. Define staged rollout for multi-language code-block runtime support with shared safety controls

## Documentation policy
- `CHANGES.md` is the living progress ledger
- `ROADMAP.md` is the live forward-looking planning tracker
- `docs/USER_GUIDE.md` is the live end-user guide with layered depth
- This file is for technical understanding and implementation notes
- `PROJECT_VISION.md` is aspirational; it should not be used to infer shipped features
