# rgx - Next-Generation Regex Engine

## 🚀 Mission Statement
Create a high-performance, open-source regex engine that **surpasses PCRE2** while enabling safe **Lua and JavaScript code execution** within regex patterns.

## ✅ Core Features

### Performance First
- **Faster than PCRE2** for pure regex operations
- **SIMD-optimized** state machines  
- **Zero-cost abstractions** - unused features = zero overhead

### Code Execution Languages
- **Lua**: `(?{lua: return tonumber(match[1]) > 10})`
- **JavaScript**: `(?{js: return match[1].toUpperCase()})`
- **Full captured group access** - code blocks can read `match[1]`, `match[2]`, etc.
- **Stateful execution** - variables persist across matches within the same operation

### Perl Regex Compatibility
- **100% Perl regex support** as baseline
- All existing Perl patterns work unchanged
- Enhanced with safe code execution on top

### Drop-in Replacement Ready
- **Familiar API** compatible with existing regex libraries
- **Easy migration** from PCRE2, Python re, JavaScript RegExp
- **Progressive enhancement** - start with regex, add code execution when needed

### Multi-Language Bindings
- **Python**: `import rgx`
- **Node.js**: `const rgx = require('rgx')`
- **Go**: `import "rgx"`
- **Ruby, C#, Java, C/C++**: Any language where bindings are feasible

## 🎨 Usage Examples

### Pure Regex (Maximum Speed)
```rust
let regex = rgx::compile(r"\b\w+@\w+\.\w+\b")?;
let emails = regex.find_all(text); // Faster than PCRE2
```

### Enhanced with Lua
```rust
let validator = rgx::compile(r"(\d+)(?{lua: return tonumber(match[1]) >= 18})")?;
let adults = validator.find_all("Ages: 15, 25, 12, 30, 16"); // Finds: 25, 30
```

### Enhanced with JavaScript  
```rust
let formatter = rgx::compile(r"(\w+)(?{js: return match[1].toUpperCase()})")?;
let result = formatter.replace_all("hello world", "${code_result}"); // "HELLO WORLD"
```

### Stateful Processing
```rust
let counter = rgx::compile(r"(\w+)(?{lua: count = (count or 0) + 1; return 'item_' .. count})")?;
let result = counter.replace_all("hello world foo bar", "${code_result}");
// Result: "item_1 item_2 item_3 item_4"
```

### Cross-Language Usage
```python
# Python code using rgx with Lua execution
import rgx

pattern = rgx.compile(r'(\d+)(?{lua: return tonumber(match[1]) * 2})')
results = pattern.find_all("Process 5 items, handle 10 records") 
# Lua doubles the numbers: 10, 20
```

## 🏗️ Technical Architecture

### Safety Model
- **Sandboxed execution** - Lua/JS code runs in isolated environments
- **Memory safety** - Rust prevents buffer overflows
- **Resource limits** - CPU/memory controls for code execution  
- **No system access** - Code blocks cannot touch filesystem/network

### Execution Modes
- `ExecutionMode::Pure` - No code execution, maximum performance
- `ExecutionMode::Safe` - Sandboxed Lua/JS execution only
- `ExecutionMode::Full` - All features enabled

### Performance Layers
1. **Pure Regex** - SIMD-optimized, beats PCRE2
2. **+ Lua Code** - Minimal overhead, fast interpreter
3. **+ JavaScript Code** - V8 engine, controlled overhead

## 📅 Development Roadmap

### Phase 1: Core Foundation
- ✅ **Lua + JavaScript** execution engines
- ✅ **Beat PCRE2** performance benchmarks
- ✅ **Full Perl regex** compatibility
- ✅ **Primary language bindings** (Python, Node.js, Go)

### Phase 2: Community Expansion
- 🔮 **Additional languages**: Tcl, Scheme/Lisp, Forth (based on demand)
- 🔮 **Extended bindings**: Ruby, C#, Java, etc.
- 🔮 **Advanced features**: JIT compilation, streaming regex
- 🔮 **Enterprise features**: Performance monitoring, debugging tools

## 📜 License & Philosophy

- **License**: Apache License 2.0 (open source, always!)
- **Development**: Community-driven, performance-obsessed
- **Target**: Universal appeal - any developer using regex
- **Philosophy**: Maximum performance + maximum safety + maximum flexibility

## 🎯 Success Metrics

1. **Performance**: Consistently faster than PCRE2 for common patterns
2. **Compatibility**: 100% Perl regex feature support  
3. **Adoption**: Active usage across multiple programming languages
4. **Safety**: Zero security vulnerabilities from code execution
5. **Community**: Thriving contributor base with comprehensive documentation

## 🚫 Design Constraints

### Languages NOT Supported in `(?{...})`
- **Python** - Whitespace/indentation sensitivity makes inline usage impractical
- **WebAssembly** - Not human-readable/writable for inline code
- **Compiled languages** (Rust, C++, Go) - Require compilation, not suitable for inline execution

### Focus Areas
- **Dynamic languages only** for `(?{...})` execution
- **Performance never compromised** for features
- **Open source** development model exclusively
- **Safety first** - no system access from code blocks

---

**The vision is clear: Build the ultimate regex engine that developers will love to use!** 🚀

*Repository: https://github.com/rdje/rgx*
