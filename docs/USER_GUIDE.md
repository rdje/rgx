# USER GUIDE
Live end-user guide for rgx.

This guide is intentionally layered so you can start simple and go as deep as needed.
## Living-document policy
- Last updated: 2026-03-27
- This is a live document and should be updated as user-visible behavior changes.
- Keep examples and feature-status notes aligned with current shipped behavior.
- For recent changes and validation details, cross-check `CHANGES.md` and `RUST_CODEBASE_ANALYSIS.md`.
## Guide levels
- Level 0: quick start
- Level 1: practical day-to-day usage
- Level 2: advanced patterns and execution modes
- Level 3: deep behavior notes ("gory details")
## Level 0 - Quick start
Build and run a simple match:

```bash
cargo build
cargo run --bin rgx-cli -- "cat|dog" "I have a cat"
```

Run tests:

```bash
cargo test --workspace
```
## Level 1 - Practical usage
### CLI
Basic form:

```bash
cargo run --bin rgx-cli -- "<pattern>" "<text>"
```

Examples:

```bash
cargo run --bin rgx-cli -- "\d+" "abc123def"
cargo run --bin rgx-cli -- "(?:cat|dog)" "pet dog"
```
### Rust API
Use the high-level API for normal matching:

```rust
use rgx_core::Regex;

let re = Regex::compile(r"\d+")?;
assert!(re.is_match("abc123"));

if let Some(m) = re.find_first("abc123def") {
    assert_eq!((m.start, m.end), (3, 6));
}
# Ok::<(), rgx_core::RgxError>(())
```
### Match results
- Positions are byte offsets.
- `find_all` returns non-overlapping matches.
- For top-level alternation patterns, `matched_branch_number` is 1-based.
- When Lua/JavaScript/native code blocks return non-boolean values on the winning path, `find_first` / `find_all` expose them through `code_result`.
## Level 2 - Advanced usage
### AST-first workflows
When parser syntax is not yet complete for a feature family, you can still compile from AST directly.

```rust
use rgx_core::Regex;
use rgx_core::ast::Regex as RegexAst;

let ast = RegexAst::Alternation(vec![
    RegexAst::Sequence(vec![RegexAst::Char('c'), RegexAst::Char('a'), RegexAst::Char('t')]),
    RegexAst::Sequence(vec![RegexAst::Char('d'), RegexAst::Char('o'), RegexAst::Char('g')]),
]);

let re = Regex::from_ast(ast)?;
assert!(re.is_match("dog"));
# Ok::<(), rgx_core::RgxError>(())
```
### Predicate code blocks
Embedded code-block execution is now available for Lua, JavaScript, Rust-native callbacks, and registered wasm modules.

Requirements:
- `ExecutionMode::Pure` rejects all code blocks.
- Use `Regex::with_mode(..., ExecutionMode::Safe)` or `ExecutionMode::Full` for `lua` / `js` / `javascript` / `wasm`.
- Use `ExecutionMode::Full` for `native`.
- Enable the matching cargo feature:
  - `lua` for `(?{lua:...})`
  - `javascript` for `(?{js:...})` / `(?{javascript:...})`
  - `wasm` for `(?{wasm:...})`
- Register native callbacks or wasm modules on the compiled `Regex` before matching.
- Optional host-provided variables can be set on the compiled `Regex` via `set_variable(...)`.
- Write code as a predicate body and use `return ...` style in both Lua and JavaScript.

Lua example:

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<word>cat)(?{lua:return named.word == "cat"})"#,
    ExecutionMode::Safe,
)?;
assert!(re.is_match("cat"));
# Ok::<(), rgx_core::RgxError>(())
```
Host-provided variables come from the Rust API:

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(r#"(?{js:return vars.env === "prod";})"#, ExecutionMode::Safe)?;
re.set_variable("env", "prod")?;
assert!(re.is_match(""));
# Ok::<(), rgx_core::RgxError>(())
```

Wasm example:

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode("(?{wasm:truthy:evaluate})", ExecutionMode::Safe)?;
re.register_wasm_module("truthy", include_bytes!("truthy.wasm"))?;
assert!(re.is_match(""));
# Ok::<(), rgx_core::RgxError>(())
```
For the current wasm slice, `truthy.wasm` must export `evaluate() -> i32`, where `0` means predicate failure and any non-zero value means success.

JavaScript example:

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<word>cat)(?{js:return named.word === "cat";})"#,
    ExecutionMode::Safe,
)?;
assert!(re.is_match("cat"));
# Ok::<(), rgx_core::RgxError>(())
```

Native callback example:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<word>cat)(?{native:validate_word})"#,
    ExecutionMode::Full,
)?;
re.register_native("validate_word", |ctx| {
    if ctx.named("word") == Some("cat") {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;
assert!(re.is_match("cat"));
# Ok::<(), rgx_core::RgxError>(())
```

What the execution context exposes today:
- `arg[0]`: current overall match prefix for the current match attempt
- `arg[1]`, `arg[2]`, ...: completed numbered captures
- `named.<group_name>` / `named[group_name]`: completed named captures
- `vars.<name>` / `vars[name]`: host-provided execution variables set through `Regex::set_variable(...)`
- `pos`: current byte position in the input
- `text`: full input text
- Lua/JavaScript bindings expose `arg`, `named`, `vars`, `pos`, and `text`; native callbacks receive the same data through `ExecContext` helpers such as `current_match()`, `group()`, `named()`, and `variable()`.
- Wasm currently exposes a smaller import-based context slice through the `rgx` namespace:
  - `position() -> i32`
  - `text_length() -> i32`
  - `text_read(ptr, offset, len) -> i32`
  - `capture_count() -> i32`
  - `capture_length(index) -> i32` (`-1` when the capture slot is unavailable)
  - `capture_read(index, ptr, offset, len) -> i32` (`-1` when the capture slot is unavailable)
  - `named_capture_count() -> i32`
  - `named_capture_name_length(index) -> i32` (`-1` when the named-capture slot is unavailable)
  - `named_capture_name_read(index, ptr, offset, len) -> i32` (`-1` when the named-capture slot is unavailable)
  - `named_capture_value_length(index) -> i32` (`-1` when the named-capture slot is unavailable)
  - `named_capture_value_read(index, ptr, offset, len) -> i32` (`-1` when the named-capture slot is unavailable)
  - `variable_count() -> i32`
  - `variable_name_length(index) -> i32` (`-1` when the variable slot is unavailable)
  - `variable_name_read(index, ptr, offset, len) -> i32` (`-1` when the variable slot is unavailable)
  - `variable_value_length(index) -> i32` (`-1` when the variable slot is unavailable)
  - `variable_value_read(index, ptr, offset, len) -> i32` (`-1` when the variable slot is unavailable)
- Named captures and host-provided variables are exposed to wasm through deterministic lexicographic ordering by name.
- `text_read`, `capture_read`, `named_capture_name_read`, `named_capture_value_read`, `variable_name_read`, and `variable_value_read` require the wasm module to export linear memory as `memory`.
- Capture slot `0` in the wasm ABI is still the current overall match prefix for the current match attempt.

Current limits for this slice:
- The CLI does not yet expose native or wasm registration.
- The current wasm ABI is still limited to `module:function` with an exported `() -> i32` predicate plus the read-only import helpers above.
- Host-provided variables are read-only snapshots for each code-block evaluation, and richer non-boolean result handling is still not exposed to wasm modules.
- Unknown native callback names and malformed/unresolved wasm call specs fail the current match path at runtime.
- Malformed wasm context reads, missing exported memory, and invalid guest-memory writes also fail the current match path at runtime.
- Lua/JavaScript/native numeric and replacement return values are surfaced through `MatchResult.code_result`; wasm still remains predicate-only.
- Code blocks may execute multiple times during backtracking or scanning, so they should be treated as side-effect-free predicates.
### Current parsed-but-unintegrated syntax
The parser still recognizes several advanced constructs that are not runtime-integrated yet:
- backreferences
- recursion
- conditionals
- Unicode property classes
These continue to fail with explicit compile-time messages.
## Level 3 - Gory details
### Execution model
Pipeline:
- pattern text -> lexer -> parser -> AST -> compiler -> VM bytecode -> VM execution

In AST-first mode, parser steps are bypassed and AST goes directly to compiler/VM.
### Assertion semantics
- Lookahead and lookbehind are assertions: they do not consume input themselves.
- Positive assertion requires assertion sub-expression to match.
- Negative assertion requires assertion sub-expression to not match.
- Lookbehind requires a sub-expression match that ends exactly at the current position.
### Atomic-group semantics
- Atomic groups `(?>...)` are supported.
- Once an atomic group succeeds, rgx does not backtrack into alternatives/paths created inside that group.
### Predicate code-block semantics
- Code blocks are zero-width predicate checkpoints in the VM path.
- They can fail the current path and allow normal regex backtracking to continue.
- Boolean-style success/failure still drives path control.
- Lua/JavaScript/native `return 123` or `return "..."` also keep the current path successful and store the last winning-path non-boolean value in `MatchResult.code_result`.
- Wasm currently remains predicate-only because its shipped ABI is still `module:function` with an exported `() -> i32`.
### Branch reporting semantics
- Branch reporting is intentionally scoped to top-level alternations.
- `matched_branch_number` is 1-based and nested alternations do not override it.
## Troubleshooting checklist
- If a code block compiles in one build and not another, check whether the corresponding cargo feature is enabled.
- If a code block behaves unexpectedly, verify the execution mode first (`Pure` vs `Safe` / `Full`).
- If a pattern compiles but behavior is surprising, compare it against `docs/CAPABILITY_MATRIX.md` and `RUST_CODEBASE_ANALYSIS.md`.
## Related docs
- `README.md`: project overview and navigation
- `docs/CAPABILITY_MATRIX.md`: shipped vs parsed-only status
- `RUST_CODEBASE_ANALYSIS.md`: live implementation assessment
- `ROADMAP.md`: forward-looking tracker
- `CHANGES.md`: shipped changes and validation history
