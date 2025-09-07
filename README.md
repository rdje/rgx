# rgx

A next-generation, high-performance regex engine designed to surpass PCRE2 while enabling safe, multi-language code execution within patterns. Built in Rust for maximum speed and safety.

## 🎯 Project Vision

rgx will be the first regex engine outside of Perl to support safe inline code execution with the unique `(?{language:...})` syntax, initially supporting Lua and JavaScript within regex patterns.

## ✅ Current Status

**MILESTONE ACHIEVED**: Complete high-performance regex virtual machine with comprehensive bytecode execution engine!

### Core Features Working:
- ✅ **Full VM Implementation**: 132-opcode instruction set with SIMD optimization support
- ✅ **Backtracking Engine**: Advanced backtracking with memoization and alternative tracking
- ✅ **Optimizing Compiler**: Multi-pass compilation (analysis, optimization, codegen)
- ✅ **Comprehensive Regex Support**:
  - Character literals and classes (`a`, `\d`, `\w`, `\s`)
  - Quantifiers (`*`, `+`, `?`, `{n,m}`)
  - Alternation (`cat|dog`) with proper alternative tracking
  - Capture groups `(...)` with group numbering
  - Anchors (`^`, `$`) and word boundaries (`\b`, `\B`)
  - Sequences and complex patterns

### Test Results:
- **12/12 VM tests passing** covering all core regex features
- Successfully handles complex patterns: `cat|dog`, `(a*)(b+)`, `\d+`, anchored patterns
- Performance-optimized bytecode layout with runtime SIMD capability detection

## 🏗️ Architecture

- **Core crate**: `rgx-core` - High-performance regex VM and compiler
- **CLI**: `rgx-cli` - Command-line interface for testing and benchmarking
- **Future modules**: WASM executor, Lua/JS code execution, language bindings

## 🚀 Next Steps

1. Integration with high-level Regex API
2. SIMD string search implementation
3. JIT compilation for hot paths
4. Lua and JavaScript code execution backends
5. Performance benchmarking against PCRE2
6. Language bindings (Python, Node.js, Go, etc.)

Repository: https://github.com/rdje/rgx

