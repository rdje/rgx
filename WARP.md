# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Project Overview

rgx is a next-generation, high-performance regex engine designed to surpass PCRE2 while enabling safe, multi-language code execution within patterns. The project is built in Rust with a focus on maximum speed and safety.

**Current Status**: Complete high-performance regex VM with comprehensive bytecode execution engine (MILESTONE ACHIEVED 2025-09-07). The VM supports 132 opcodes with full backtracking, memoization, and SIMD framework.

## Common Commands

### Build & Test
```bash
# Full workspace build
cargo build

# Check compilation without building
cargo check

# Full workspace tests 
cargo test

# Test specific package 
cargo test -p rgx-core

# Test VM specifically (12 tests should all pass)
cargo test -p rgx-core vm::

# Test with debug output
cargo test -p rgx-core -- --nocapture

# Build in release mode
cargo build --release

# Run benchmarks
cargo bench -p rgx-core
```

### CLI Usage
```bash
# Basic pattern matching
cargo run --bin rgx-cli -- "cat|dog" "I have a cat"

# Run CLI with development features
cargo run --bin rgx-cli
```

### Development
```bash
# Clean build artifacts
cargo clean

# Format code
cargo fmt

# Run lints
cargo clippy

# Check for unused dependencies
cargo machete

# View dependency tree
cargo tree
```

## Workspace Architecture

The project uses a multi-crate workspace:

```
rgx/
├── rgx-core/       # Core regex engine and VM
├── rgx-cli/        # Command-line interface  
├── rgx-bench/      # Performance benchmarks
├── rgx-wasm/       # WebAssembly bindings
└── examples/       # Usage examples
```

### Component Flow
```
Pattern → Lexer → Parser → AST → Compiler → VM Bytecode → VM Execution → Match Results
```

**rgx-core** contains the complete implementation:
- `vm.rs`: High-performance 132-opcode VM with backtracking (1,700+ lines, COMPLETED)
- `parsing.rs`: AST generation from patterns
- `lexer.rs`: Tokenization of regex patterns
- `ast.rs`: Abstract syntax tree definitions
- `lib.rs`: High-level API (needs VM integration)
- `compiler.rs`: Bytecode compilation (integrated into vm.rs)

## VM-Based Regex Engine (Technical Deep-dive)

The core of rgx is a sophisticated virtual machine that executes compiled regex bytecode:

### Key Features
- **132-opcode instruction set** covering all regex features
- **SIMD optimization framework** with runtime capability detection (SSE2, AVX2, NEON)
- **Advanced backtracking** with memoization and alternative tracking
- **Capture groups** with proper numbering
- **Cache-friendly bytecode design** optimized for performance

### VM Architecture
The `RegexVM` supports:
- Literal matching (Char, Any, String)
- Character classes (DigitAscii, WordAscii, SpaceAscii, Custom)
- SIMD-optimized operations (SimdFind, SimdString, SimdCharClass) 
- Anchors & boundaries (StartLine, EndLine, WordBoundary)
- Control flow (Jump, Split, SplitLazy)
- Capture groups (SaveStart, SaveEnd)
- Quantifiers (QuestionGreedy, StarGreedy, PlusGreedy, RepeatRange)
- Alternative tracking for reporting which alternation branch matched

### Execution Strategy
The VM uses adaptive execution:
1. **SIMD pre-filtering** for long texts with literal content
2. **Anchored optimization** for patterns with start/end anchors
3. **Scanning approach** for general patterns (try at each position)

### Performance Features
- Runtime SIMD capability detection
- Memoization for backtracking optimization
- Cache-efficient bytecode layout
- UTF-8 optimized character handling

## Development Status & Current Priorities

### ✅ COMPLETED (Major Milestone)
- Complete VM implementation with 132 opcodes
- All core regex features working
- 12/12 VM tests passing
- Advanced compiler with bytecode optimization
- SIMD framework ready
- Alternative tracking for alternation patterns

### 🔧 IMMEDIATE PRIORITIES
1. **High-level API integration** (`rgx-core/src/lib.rs`) - Connect main Regex API to completed VM
2. **SIMD implementation** (`vm.rs:386-390`) - Implement `find_first_simd()` stub
3. **Engine integration** (`engine.rs`) - Update to use VM instead of placeholders

### 🚀 NEAR-TERM
- Advanced regex features (lookaheads, lookbehinds, recursive patterns)
- Lua integration with `mlua` for `(?{lua:...})` patterns
- JavaScript integration with V8 for `(?{js:...})` patterns
- PCRE2 performance benchmarks

### 📈 LONG-TERM
- JIT compilation for hot paths
- Multi-language bindings (Python, Node.js, Go)
- WebAssembly executor
- Production-ready release

## Patterns & Conventions

### Code Style
- Uses `thiserror` and `anyhow` for error handling
- Extensive documentation with `//!` module docs
- Lint settings: `#![warn(missing_docs, clippy::all, clippy::pedantic)]`
- Feature-gated functionality (wasm, lua, javascript, all-languages)

### Testing Patterns
- Comprehensive VM tests in `vm.rs` (lines 1421-1690)
- Property-based testing approach
- Benchmark tests using `criterion`
- Test pattern: AST → Compiler → VM → Assert results

### Performance Focus
- SIMD-first design with runtime detection
- Cache-friendly data structures
- Zero-cost abstractions
- Profile-guided optimization hints

### Development Files
- `DEVELOPMENT_NOTES.md`: Technical knowledge base and architecture insights
- `CHANGES.md`: Complete change history with root cause analysis
- `PROJECT_VISION.md`: Long-term project goals and vision

## Key Technical Insights

1. **VM Decoupling**: The VM operates on bytecode, completely independent of regex syntax - enables multiple frontend parsers
2. **Performance Layers**: Pure regex (fastest) → +Lua → +JavaScript execution
3. **Sandboxed Execution**: Code blocks have no filesystem/network access
4. **Adaptive Strategy**: VM chooses execution approach based on pattern characteristics
5. **Alternative Tracking**: Unique feature reporting which alternation branch matched

## Current Test Results
- **VM Tests**: 12/12 passing ✅
- **Complex Patterns**: `cat|dog`, `(a*)(b+)`, `\d+`, anchored patterns all working
- **Performance**: Optimized bytecode with SIMD capability detection ready

The foundation is complete - focus on API integration and SIMD implementation for immediate productivity gains.
