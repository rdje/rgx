# Technical Decisions and Architecture Notes

This document captures key technical decisions made during RGX development, providing context for future maintainers and contributors.

## 🏗️ Core Architecture Decisions

### VM-Based Execution Engine
**Decision**: Use bytecode virtual machine for regex execution  
**Date**: 2025-09-03 to 2025-09-07  
**Rationale**:
- Decouples parsing from execution (can swap parsers without changing VM)
- Enables advanced optimizations (JIT compilation, SIMD integration)
- Provides predictable performance characteristics
- Allows complex features like backtracking, memoization, and alternative tracking
- Cache-friendly bytecode layout for better performance

**Implementation**: 132-opcode instruction set with specialized opcodes for:
- Literal matching, character classes, quantifiers
- Control flow (Jump, Split for alternation)
- Capture groups (SaveStart, SaveEnd)
- SIMD hints and optimization markers

### Backtracking vs NFA/DFA
**Decision**: Backtracking engine with memoization  
**Rationale**:
- Required for full Perl regex compatibility (recursion, lookarounds, backreferences)
- NFA/DFA cannot handle all regex features needed
- Memoization mitigates exponential blowup risks
- Allows advanced features like conditional patterns and code execution

**Trade-offs**: Slower than pure DFA on simple patterns, but necessary for feature completeness

### Multi-Pass Compilation
**Decision**: Analysis → Optimization → Code Generation → Peephole Optimization  
**Implementation**: `OptimizingCompiler` in `vm.rs` lines 954-1400+  
**Rationale**:
- Enables sophisticated optimizations
- Gathers statistics for adaptive execution strategies
- Allows pattern-specific code generation
- Prepares for JIT compilation integration

## 🔧 Implementation Choices

### UTF-8 Byte Processing
**Decision**: Store text as Vec<u8> in execution context  
**Rationale**:
- SIMD instructions work on bytes, not Unicode scalars
- Character-based processing handled by `current_char()` and `advance_char()`
- Balances Unicode correctness with performance

**Implementation**:
```rust
pub struct ExecContext {
    pub text: Vec<u8>,  // UTF-8 bytes for SIMD
    pub pos: usize,     // Byte position (not char position!)
    // ...
}
```

### Alternative Tracking
**Decision**: Track which alternation branch matched during execution  
**Implementation**: `SetAlternative` opcode + `current_alternative` in context  
**Rationale**:
- Essential for debugging and match result reporting
- Enables advanced features like conditional execution based on which branch matched
- Minimal performance overhead (single usize field)

### Jump Offset Encoding
**Decision**: 16-bit little-endian offsets for Jump/Split instructions  
**Implementation**:
```rust
OpCode::Jump => {
    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
    ip += 2; // Skip offset operand bytes
    ip += offset; // Apply jump
}
```
**Rationale**:
- 16-bit allows 64KB program size (sufficient for most patterns)
- Little-endian matches target architecture (x86/ARM)
- Consistent with other offset calculations in VM

### Capture Group Storage
**Decision**: Flat array [start1, end1, start2, end2, ...] for capture groups  
**Rationale**:
- Cache-friendly linear memory layout
- Direct indexing: group N = [2*N, 2*N+1]
- Minimizes allocations during matching
- Compatible with standard regex APIs

**Group 0 Special Handling**: Overall match handled separately from numbered groups

### SIMD Capability Detection
**Decision**: Runtime detection with compile-time feature availability  
**Implementation**: `detect_simd_support()` using std::arch feature detection  
**Rationale**:
- Maximizes performance on capable hardware
- Graceful fallback on older hardware
- Enables SIMD-specific optimizations when available

## 🚀 Performance Optimizations

### Adaptive Execution Strategy
**Decision**: Choose execution strategy based on pattern characteristics  
**Implementation**: `find_first()` → SIMD/anchored/scanning selection  
**Rationale**:
- SIMD for long texts with literal content
- Anchored optimization for start-anchored patterns
- Scanning fallback for general patterns

### Instruction Set Design
**Decision**: Specialized opcodes for common operations  
**Examples**:
- `DigitAscii`, `WordAscii`, `SpaceAscii` - Direct character class matching
- `StarGreedy`, `PlusGreedy`, `QuestionGreedy` - Optimized quantifiers
- `SimdFind`, `SimdString` - SIMD operation hints

**Rationale**: Reduces instruction count and enables specialized optimizations

### Memoization Framework
**Decision**: HashMap-based memoization in execution context  
**Implementation**: `memo_cache: HashMap<(usize, usize), bool>`  
**Rationale**:
- Prevents exponential blowup in pathological cases
- Key = (position, instruction_pointer) for cache hits
- Optional feature (can be disabled for simple patterns)

## 🧪 Testing Strategy Decisions

### VM-First Testing
**Decision**: Test VM directly rather than through high-level API  
**Rationale**:
- VM is the core engine - must be rock-solid
- Faster test execution (no parsing overhead)
- Direct control over test inputs (AST construction)
- Easier debugging of VM-specific issues

### Manual AST Construction in Tests
**Decision**: Build AST manually in VM tests rather than parsing strings  
**Example**:
```rust
let ast = Regex::Alternation(vec![
    Regex::Sequence(vec![Regex::Char('c'), Regex::Char('a'), Regex::Char('t')]),
    Regex::Sequence(vec![Regex::Char('d'), Regex::Char('o'), Regex::Char('g')]),
]);
```
**Rationale**:
- Tests VM independently of parser
- Precise control over test cases
- Exercises specific code paths
- Faster than string parsing in tests

## 🔄 Integration Patterns

### Lexer → Parser → AST → VM Pipeline
**Decision**: Clean separation between compilation phases  
**Benefits**:
- Each component testable independently
- Can swap implementations (e.g., different parsers)
- Clear error boundaries and reporting
- Enables incremental optimization

### Engine as Adapter Layer
**Decision**: Engine coordinates between VM and public API  
**Implementation**: `Engine` struct in `engine.rs`  
**Rationale**:
- Abstraction layer for different execution backends
- Handles string ↔ bytes conversion
- Manages execution modes (Pure, Safe, Full)
- Provides stable API independent of VM changes

## 🔮 Future-Proofing Decisions

### JIT Compilation Preparation
**Decision**: Bytecode designed for easy JIT compilation  
**Features**:
- `HotPath` opcode for marking frequently executed code
- Memoization points for optimization decisions
- SIMD hints in instruction stream
- Performance statistics gathering during compilation

### Multi-Language Code Execution Framework
**Decision**: Prepare VM for inline code execution (`(?{lang:...})`)  
**Implementation**: Reserved opcodes and execution context hooks  
**Rationale**:
- Core differentiator vs other regex engines
- Enables advanced pattern matching with custom logic
- Framework ready for Lua/JavaScript integration

### Extension Points
**Decision**: Design for extensibility  
**Examples**:
- Plugin architecture for new opcodes
- Configurable execution strategies
- Modular character class implementations
- Custom optimization passes

## ⚠️ Known Limitations and Trade-offs

### 16-bit Jump Offsets
**Limitation**: Maximum 64KB bytecode program size  
**Mitigation**: Pattern splitting for extremely large patterns  
**Future**: Could extend to 32-bit offsets if needed

### Single-threaded Execution
**Current State**: VM execution is single-threaded  
**Future**: Parallel matching for very large texts planned

### Memory Usage
**Trade-off**: Memoization cache uses additional memory  
**Benefit**: Prevents exponential time complexity  
**Control**: Cache can be disabled for memory-constrained environments

## 📝 Design Principles

1. **Performance First**: Every decision optimized for execution speed
2. **Correctness Always**: Never sacrifice correctness for performance  
3. **Extensibility**: Design for future feature additions
4. **Testability**: Every component must be unit testable
5. **Zero-Cost Abstractions**: Unused features have no performance impact
6. **Memory Safety**: Leverage Rust's safety guarantees throughout

These decisions form the foundation of RGX's architecture and should guide future development to maintain consistency and performance characteristics.
