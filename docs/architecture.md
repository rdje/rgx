# Architecture
This document describes the current rgx architecture and responsibilities of each layer.

## Layered flow
`Pattern text -> Lexer -> Parser -> AST -> Compiler -> VM bytecode -> VM execution -> API result`

## Components
### `lexer.rs`
- Converts input pattern text into tokens
- Handles escapes, classes, anchors, quantifier tokens, and base grouping tokens

### `parser.rs` and `parsing.rs`
- Builds AST from lexer tokens
- `parsing.rs` provides parser-selection abstraction
- Current default is the recursive-descent parser path

### `compiler.rs` and `vm.rs::OptimizingCompiler`
- Converts AST into VM bytecode program
- Includes analysis/codegen scaffolding plus optimization hooks

### `vm.rs::RegexVM`
- Executes bytecode over UTF-8 input text
- Provides matching operations used by the public API
- Contains adaptive strategy hooks (scanning/anchored/SIMD-oriented paths)

### `engine.rs` and `lib.rs`
- Adapter and public API surface
- Exposes `Regex::compile`, `is_match`, `find_first`, `find_all`

## Important constraints
- Public behavior must be validated from API entry points, not only internal VM tests
- Declared opcode/feature availability must not be treated as shipped unless fully parsed + compiled + executed
- Documentation should track verified capability, not planned capability
