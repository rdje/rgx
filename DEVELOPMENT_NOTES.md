# DEVELOPMENT NOTES

Vision: Build a zero-baggage, performance-first regex engine (rgx) that beats PCRE2 for pure patterns and adds safe Lua + JavaScript code execution.

## Project Vision Finalized ✅
- Lua + JavaScript execution in (?{...}) patterns
- Beat PCRE2 performance for pure regex
- Full Perl regex compatibility
- Apache 2.0 license (open source)
- Multi-language bindings (Python, Node.js, Go, etc.)

## Initial scaffolding created ✅
- Workspace with rgx-core and rgx-cli
- Core API stubs for Compiler, Engine, Regex
- Placeholder modules for SIMD, caches, and execution backends
- Working CLI tool with basic functionality

## Next Development Priorities
1. **Real regex engine implementation**: Replace placeholder matching with actual regex parsing and execution
2. **SIMD optimization**: Implement vectorized state machine for pure regex performance
3. **Lua integration**: Add mlua-based code execution for (?{lua:...}) patterns
4. **JavaScript integration**: Add V8-based code execution for (?{js:...}) patterns
5. **PCRE2 benchmarks**: Establish performance baselines and targets

## Architecture Notes
- Performance-layered design: Pure regex (fastest) → +Lua → +JavaScript
- Sandboxed execution: No filesystem/network access from code blocks
- Zero-cost abstractions: Unused features have zero performance impact

