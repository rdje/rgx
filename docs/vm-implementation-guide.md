# RGX VM Implementation Guide

This document provides a comprehensive guide to the RGX regex virtual machine implementation, designed to help new developers quickly understand and continue building on the current codebase.

## 🎯 Current State (as of 2025-09-07)

**STATUS**: ✅ **MAJOR MILESTONE COMPLETED** - Full VM implementation with comprehensive testing

### What's Working
- **Complete 132-opcode instruction set** with cache-optimized layout
- **Advanced backtracking engine** with proper state restoration
- **Full alternation support** with Split opcodes and alternative tracking
- **Character matching**: literals, classes (`\d`, `\w`, `\s`), anchors (`^`, `$`), word boundaries
- **Quantifiers**: `*`, `+`, `?`, `{n,m}` with greedy support
- **Capture groups** with proper numbering and position tracking
- **SIMD capability detection** (SSE2, AVX2, NEON) - framework ready
- **12/12 VM tests passing** - all core functionality verified

### Test Results
```bash
cargo test -p rgx-core vm::
# Result: 12 passed; 0 failed
```

Working patterns include:
- `cat|dog` (alternation with tracking)
- `(a*)(b+)` (capture groups with quantifiers)
- `\d+\w*` (character classes)
- `^start.*end$` (anchors)
- Complex nested patterns

## 🏗️ VM Architecture Overview

### Core Components

#### 1. OpCode Enum (132 opcodes)
Located in `rgx-core/src/vm.rs`, lines 15-140

**Key opcode categories**:
- **Literal Matching**: `Char`, `Any`, `String`
- **Character Classes**: `DigitAscii`, `WordAscii`, `SpaceAscii`
- **SIMD Operations**: `SimdFind`, `SimdString`, `SimdCharClass`
- **Anchors**: `StartLine`, `EndLine`, `WordBoundary`
- **Control Flow**: `Jump`, `Split`, `Call`, `Return`
- **Capture Groups**: `SaveStart`, `SaveEnd`
- **Quantifiers**: `StarGreedy`, `PlusGreedy`, `QuestionGreedy`
- **Alternative Tracking**: `SetAlternative`
- **Termination**: `Match`, `Fail`, `Accept`

#### 2. Execution Context (`ExecContext`)
Lines 263-281

```rust
pub struct ExecContext {
    pub text: Vec<u8>,                    // UTF-8 bytes for SIMD
    pub pos: usize,                       // Current position (bytes!)
    pub end: usize,                       // End position
    pub captures: Vec<Option<usize>>,     // Group positions
    pub memo_cache: HashMap<(usize, usize), bool>, // Memoization
    pub call_stack: Vec<usize>,           // Recursion
    pub backtrack_stack: Vec<BacktrackFrame>, // Backtracking
    pub current_alternative: Option<usize>,   // Alternative tracking
}
```

#### 3. RegexVM Main Engine
Lines 308-950

**Key methods**:
- `find_first()` - Main entry point with adaptive execution
- `execute_at()` - Core bytecode execution loop (lines 426-726)
- `find_first_scanning()` - Try match at each position
- Utility methods: `current_char()`, `advance_char()`, `reset_captures()`

## 🔧 How the VM Works

### Compilation Pipeline
```
Regex AST → OptimizingCompiler → VM Bytecode → RegexVM execution
```

### Execution Flow
1. **Input**: Text + compiled VM program
2. **Strategy selection**: SIMD, anchored, or scanning
3. **Position loop**: Try match at each position
4. **Bytecode execution**: `execute_at()` processes opcodes
5. **Backtracking**: On failure, restore saved states
6. **Result**: Match object with positions and alternatives

### Key Implementation Details

#### Alternation Handling (Lines 1138-1200)
```rust
// Pattern: cat|dog
// Generates:
Split L1          // Try first alternative
SetAlternative 0  // Mark as alternative 0
<cat bytecode>
Jump END
L1: SetAlternative 1  // Mark as alternative 1
<dog bytecode>
END: Match
```

#### Jump Offset Calculation
**CRITICAL**: Jump instruction advances IP by 2 bytes (for offset operand) then adds offset:
```rust
OpCode::Jump => {
    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
    ip += 2; // Skip the 2-byte offset operand  
    ip += offset; // Then add the offset
}
```

#### Character Processing
- All text stored as UTF-8 bytes for SIMD compatibility
- `current_char()` converts bytes to char for matching
- `advance_char()` advances by char.len_utf8() bytes

#### Capture Groups
- Groups stored as [start1, end1, start2, end2, ...] in context
- Group 0 = overall match (handled specially)
- SaveStart/SaveEnd opcodes manage positions

## 🧪 Testing Strategy

### VM Test Structure
Located in `rgx-core/src/vm.rs`, lines 1421-1690

**Test coverage**:
- `test_simple_char_match` - Basic character matching
- `test_digit_class` - Character classes
- `test_sequence` - Multiple characters
- `test_anchor_start` - Anchor handling
- `test_star_quantifier` - Quantifier behavior
- `test_alternation` - Basic alternation
- `test_alternation_with_tracking` - Alternative index tracking
- `test_capture_groups` - Group capturing
- `test_complex_alternation` - Multi-way alternation

### Test Pattern
```rust
#[test]
fn test_feature() {
    let mut compiler = OptimizingCompiler::new();
    let ast = /* build AST */;
    let program = compiler.compile(&ast);
    let vm = RegexVM::new(program);
    
    // Test matching
    assert!(vm.is_match("expected_match"));
    assert!(!vm.is_match("expected_fail"));
    
    // Test detailed results
    if let Some(m) = vm.find_first("text") {
        assert_eq!(m.start, expected_start);
        assert_eq!(m.end, expected_end);
        assert_eq!(m.matched_alternative, Some(expected_alt));
    }
}
```

## 🚀 What to Work on Next

### Priority 1: High-Level API Integration
**File**: `rgx-core/src/lib.rs`
**Issue**: Regex struct still uses placeholder engine, not the VM

**Steps**:
1. Update `Regex::new()` to use OptimizingCompiler + RegexVM
2. Implement `is_match()`, `find()`, `find_all()` using VM methods
3. Update tests in main lib to work with real VM

### Priority 2: SIMD Implementation
**Files**: `rgx-core/src/vm.rs`, `rgx-core/src/simd.rs`
**Current**: SIMD detection works, but no acceleration implemented

**Steps**:
1. Implement `find_first_simd()` method (currently stub)
2. Add SIMD character class matching
3. Add SIMD literal string search
4. Use detected capabilities (SSE2, AVX2, NEON)

### Priority 3: Advanced Opcodes
**Missing opcodes** (marked as TODOs):
- Lookahead/lookbehind assertions
- Recursive patterns
- Conditional patterns
- More character class operations

## 🔍 Debugging Tips

### Common Issues
1. **Jump offset miscalculation** - Remember IP advances by 2 for offset operand
2. **UTF-8 vs byte positions** - Context uses byte positions, not char positions
3. **Alternative tracking reset** - Reset `current_alternative` on new match attempts
4. **Capture group indexing** - Groups use [start, end, start, end] flat array

### Debug Tools
- Enable debug prints in VM execution loop
- Use `cargo test -- --nocapture` to see debug output
- Check bytecode generation with compiler debug output

## 📋 Code Structure Quick Reference

### Key Files
- `rgx-core/src/vm.rs` - Complete VM implementation (1,700+ lines)
- `rgx-core/src/ast.rs` - AST definitions
- `rgx-core/src/parsing.rs` - Parser implementation
- `rgx-core/src/lexer.rs` - Lexer implementation
- `rgx-core/src/lib.rs` - High-level API (needs VM integration)

### Key Structs
- `RegexVM` - Main execution engine
- `OptimizingCompiler` - AST to bytecode compiler
- `ExecContext` - Execution state
- `Program` - Compiled bytecode program
- `Match` - Match result with details

### Key Methods
- `RegexVM::find_first()` - Main matching entry point
- `RegexVM::execute_at()` - Core execution loop
- `OptimizingCompiler::compile()` - AST to bytecode
- `OptimizingCompiler::codegen_pass()` - Code generation

This VM implementation represents a major milestone and provides a solid foundation for building a high-performance regex engine that can compete with PCRE2!
