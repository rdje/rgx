# RGX Engine Architecture

The RGX regex engine is built with a modular, decoupled architecture that separates parsing, compilation, and execution concerns. This design provides flexibility, maintainability, and performance optimization opportunities.

## 📋 Architecture Layers

```text
┌─────────────────────────────────────────────┐
│                 Regex API                   │  ← User-facing interface
├─────────────────────────────────────────────┤
│                  Engine                     │  ← Execution coordinator  
├─────────────────────────────────────────────┤
│                    VM                       │  ← Bytecode execution
├─────────────────────────────────────────────┤
│                 Compiler                    │  ← AST → Bytecode
├─────────────────────────────────────────────┤
│                 Parser                      │  ← Text → AST
├─────────────────────────────────────────────┤
│                 Lexer                       │  ← Text → Tokens
└─────────────────────────────────────────────┘
```

## 🔄 Decoupling Benefits

**1. Parser Independence:**
- The VM operates on bytecode, not AST or raw patterns
- You could swap out the parser entirely (PCRE → RE2 → custom grammar)
- Multiple frontend parsers can target the same VM backend

**2. Engine Modularity:**
- The `Engine` is just a coordinator that:
  - Manages execution modes
  - Converts between byte arrays and strings  
  - Adapts VM results to API format
- Could easily swap VM implementations

**3. VM Backend Flexibility:**
- The `RegexVM` only knows about bytecode operations
- Completely independent of regex syntax
- Could be used for other pattern matching domains

## 🔧 Data Flow

```rust
"\\d{3}-\\d{2}-\\d{4}"           // Raw pattern
    ↓ (Lexer)
[Digit, LeftBrace, Char('3'), ...]  // Tokens
    ↓ (Parser)  
Sequence([Quantified{...}, ...])     // AST
    ↓ (Compiler)
[0x10, 0x86, 0x03, ...]             // VM Bytecode
    ↓ (VM)
Match { start: 5, end: 16, ... }     // Execution Result
```

## 🏗️ Component Details

### Lexer (`lexer.rs`)
- **Input**: Raw regex pattern string
- **Output**: Stream of tokens
- **Responsibility**: Character-level parsing, Unicode handling
- **Decoupling**: Token types are independent of execution strategy

### Parser (`parser.rs`)
- **Input**: Token stream from lexer
- **Output**: Abstract Syntax Tree (AST)
- **Responsibility**: Grammar validation, structure building
- **Decoupling**: AST represents semantic meaning, not execution details

### Compiler (`compiler.rs`) 
- **Input**: AST from parser
- **Output**: VM bytecode program
- **Responsibility**: Optimization, code generation
- **Decoupling**: Multiple compilation strategies can target same VM

### VM (`vm.rs`)
- **Input**: Bytecode program and input text
- **Output**: Match results
- **Responsibility**: Pattern execution, capture groups, backtracking
- **Decoupling**: Pure bytecode interpreter, syntax-agnostic

### Engine (`engine.rs`)
- **Input**: Compiled pattern and text
- **Output**: User-friendly match results
- **Responsibility**: Execution coordination, mode management
- **Decoupling**: Adapter layer between VM and public API

## 🔧 Benefits of This Design

### Flexibility
- **Replace parsers**: Use different regex dialects
- **Swap VMs**: Try different execution strategies  
- **Mix engines**: Use different backends for different patterns
- **Extend easily**: Add new language features without touching the VM

### Performance
- Each layer can be optimized independently
- JIT compilation can target just the VM layer
- SIMD optimizations can be applied at the appropriate level
- Hot path specialization per component

### Testing
- Unit test each layer in isolation
- Mock interfaces for focused testing
- Property-based testing at each boundary
- Performance benchmarking per component

### Future Extensions
- Multi-language inline code execution (Lua, JavaScript)
- Alternative parsing frontends (PCRE2 compatibility layer)
- Specialized VMs (DFA-only, NFA-only, hybrid)
- Advanced optimizations (constant folding, dead code elimination)

## 📝 Key Insight

The VM is a pure bytecode interpreter that knows nothing about regex syntax - it just executes the compiled program efficiently! This separation allows the regex engine to be both highly optimized and easily extensible.
