# DEVELOPMENT NOTES
Technical knowledge base for day-to-day engineering work in rgx.

## Purpose
- Preserve implementation context across sessions
- Document practical architecture insights and constraints
- Keep a concise list of known gaps and immediate priorities

## Current architecture (practical view)
Pipeline in `rgx-core`:
1. `lexer.rs` tokenizes pattern text
2. `parser.rs` builds AST
3. `compiler.rs` + `vm.rs::OptimizingCompiler` generate VM bytecode
4. `vm.rs::RegexVM` executes against input text
5. `engine.rs` and `lib.rs` expose user-facing API

## What is currently reliable
- Core compile-and-run flow for basic regex patterns
- VM execution paths for literals, alternation, anchors, word boundaries, basic classes, and core quantifiers
- Public API (`Regex::compile`, `is_match`, `find_first`, `find_all`) connected to the compiler/VM path
- Parser support for capturing groups, non-capturing groups `(?:...)`, and named groups `(?<name>...)`
- VM test suite coverage for core behavior

## Known engineering gaps
- Parser support for advanced group syntaxes is incomplete
  - lookarounds and inline code-block constructs are not fully wired
- VM/compiler contain declared advanced features/opcodes that are only partial or placeholder
- Inline code execution infrastructure exists but is not fully integrated into parser-to-VM user path
- JavaScript/WASM modules remain scaffold-level in user-facing flow

## Immediate priorities
1. Parser completeness for advanced grouping and assertion syntax
2. Remove/finish placeholder VM/compiler paths and TODO opcode branches
3. Define and enforce a stable capability matrix in docs + tests
4. Expand integration tests from API entry points (not only VM unit tests)

## Documentation policy
- `CHANGES.md` is the living progress ledger
- This file is for technical understanding and implementation notes
- `PROJECT_VISION.md` is aspirational; it should not be used to infer shipped features
