# USER GUIDE
Live end-user guide for rgx.

This guide is intentionally layered so you can start simple and go as deep as needed.
## Living-document policy
- Last updated: 2026-03-30
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
- When Lua/JavaScript/Rhai/native code blocks return non-boolean values on the winning path, `find_first` / `find_all` expose them through `code_result`.
### Code-driven numeric helpers
When a winning match path emits `CodeBlockValue::Numeric`, the Rust API can collect those numeric payloads directly:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<digit>\d)(?{native:emit_digit})"#,
    ExecutionMode::Full,
)?;
re.register_native("emit_digit", |ctx| {
    let value = ctx
        .named("digit")
        .and_then(|digit| digit.parse::<f64>().ok())
        .unwrap_or_default();
    ExecResult::Numeric(value)
})?;

assert_eq!(re.find_first_numeric_with_code("7a8"), Some(7.0));
assert_eq!(re.find_all_numeric_with_code("7a8"), vec![7.0, 8.0]);
# Ok::<(), rgx_core::RgxError>(())
```

Current behavior:
- `find_first_numeric_with_code` returns the first winning-path `Numeric(f64)` surfaced by any match.
- `find_all_numeric_with_code` collects all winning-path numeric payloads in match order.
- Matches without a winning-path `Numeric(f64)` payload are skipped.
### Code-driven replacement
When a winning match path emits `CodeBlockValue::Replacement`, the Rust API can rebuild output text directly:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<word>cat)(?{native:emit_upper})"#,
    ExecutionMode::Full,
)?;
re.register_native("emit_upper", |ctx| {
    ExecResult::Replacement(ctx.named("word").unwrap_or_default().to_uppercase())
})?;

assert_eq!(re.replace_first_with_code("cat dog cat"), "CAT dog cat");
assert_eq!(re.replace_all_with_code("cat dog cat"), "CAT dog CAT");
# Ok::<(), rgx_core::RgxError>(())
```

Current behavior:
- `replace_first_with_code` replaces only the first match.
- `replace_all_with_code` replaces every non-overlapping match.
- Matches without a winning-path `Replacement(String)` payload are copied through unchanged.
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
Embedded code-block execution is now available for Lua, JavaScript, Rhai, Rust-native callbacks, and registered wasm modules.

Requirements:
- `ExecutionMode::Pure` rejects all code blocks.
- Use `Regex::with_mode(..., ExecutionMode::Safe)` or `ExecutionMode::Full` for `lua` / `js` / `javascript` / `rhai` / `wasm`.
- Use `ExecutionMode::Full` for `native`.
- Enable the matching cargo feature:
  - `lua` for `(?{lua:...})`
  - `javascript` for `(?{js:...})` / `(?{javascript:...})`
  - `rhai` for `(?{rhai:...})`
  - `wasm` for `(?{wasm:...})`
- Register native callbacks or wasm modules on the compiled `Regex` before matching.
- Optional host-provided variables can be set on the compiled `Regex` via `set_variable(...)`.
- Write code as a predicate/source body:
  - Lua commonly uses `return ...`
  - JavaScript supports either a bare expression body or explicit `return ...`
  - Rhai can use a final expression directly

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

Wasm richer-result example:

```rust
use rgx_core::{CodeBlockValue, ExecutionMode, Regex};

let re = Regex::with_mode("cat(?{wasm:emit:cat_upper})", ExecutionMode::Safe)?;
re.register_wasm_module(
    "emit",
    include_bytes!("emit.wasm"),
)?;

let first = re.find_first("cat dog").expect("expected wasm match");
assert_eq!(
    first.code_result,
    Some(CodeBlockValue::Replacement("CAT".to_string()))
);
assert_eq!(re.replace_first_with_code("cat dog"), "CAT dog");
# Ok::<(), rgx_core::RgxError>(())
```

For this richer-result slice, the wasm module still exports `cat_upper() -> i32`, but it may also import:
- `rgx.emit_numeric(value: f64)`
- `rgx.emit_replacement(ptr, len)`

Those imports set the current code block's winning-path payload when the exported predicate returns non-zero.

JavaScript example:

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<word>cat)(?{js:named.word === "cat"})"#,
    ExecutionMode::Safe,
)?;
assert!(re.is_match("cat"));
# Ok::<(), rgx_core::RgxError>(())
```

Rhai example:

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<word>cat)(?{rhai: named["word"] == "cat"})"#,
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
- `match_start`: current match-attempt start byte offset
- `match_end`: current match-attempt end byte offset
- `match_length`: current match-attempt length in bytes
- `branch_number`: 1-based top-level branch number when the current path is inside a top-level alternation arm
- `text`: full input text
- Lua/JavaScript/Rhai bindings expose `arg`, `named`, `vars`, `pos`, `match_start`, `match_end`, `match_length`, `branch_number`, and `text`; native callbacks receive the same data through `ExecContext` helpers such as `current_match()`, `match_start()`, `match_end()`, `match_length()`, `matched_branch_number()`, `group()`, `named()`, and `variable()`.
- Wasm currently exposes a smaller import-based context/result slice through the `rgx` namespace:
  - `position() -> i32`
  - `match_start() -> i32`
  - `match_end() -> i32`
  - `match_length() -> i32`
  - `branch_number() -> i32` (`-1` when the current path is not inside a top-level alternation arm)
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
  - `emit_numeric(value: f64)`
  - `emit_replacement(ptr, len)`
- Named captures and host-provided variables are exposed to wasm through deterministic lexicographic ordering by name.
- `text_read`, `capture_read`, `named_capture_name_read`, `named_capture_value_read`, `variable_name_read`, `variable_value_read`, and `emit_replacement` require the wasm module to export linear memory as `memory`.
- Capture slot `0` in the wasm ABI is still the current overall match prefix for the current match attempt.
- `branch_number` is intentionally aligned with `MatchResult.matched_branch_number`: it is 1-based when available and absent / `-1` when the current path is not inside a top-level alternation arm.

Current limits for this slice:
- The CLI does not yet expose native or wasm registration.
- The current wasm ABI still uses `module:function` with an exported `() -> i32` predicate; richer results are emitted indirectly through `rgx.emit_numeric(...)` / `rgx.emit_replacement(...)`.
- Host-provided variables are read-only snapshots for each code-block evaluation.
- Unknown native callback names and malformed/unresolved wasm call specs fail the current match path at runtime.
- Malformed wasm context reads, missing exported memory, invalid guest-memory reads/writes, and invalid UTF-8 replacement payloads also fail the current match path at runtime.
- Lua/JavaScript/Rhai/native/wasm numeric and replacement return values are surfaced through `MatchResult.code_result`; numeric values can also be collected through `find_first_numeric_with_code` / `find_all_numeric_with_code`, and replacement values are consumed by `replace_first_with_code` / `replace_all_with_code`.
- Code blocks may execute multiple times during backtracking or scanning, so they should be treated as side-effect-free predicates.
### Current advanced syntax status
Unicode property classes are now part of the shipped runtime path. Patterns such as `\p{L}+`, `\P{L}+`, and `\p{Greek}+` compile and execute on the default path, and invalid property names fail explicitly at compile time.
Recursion / subroutine calls are also part of the shipped runtime path for the current supported forms `(?R)`, `(?1)`, and `(?&name)`. Missing numbered or named recursion targets fail explicitly at compile time.
Numeric backreferences (`\1`, `\2`, ...) are now part of the shipped runtime path. They match the exact bytes captured by the referenced numbered group, and compilation fails explicitly if the referenced group does not exist.
Conditionals are also part of the shipped runtime path for group-exists, named-group-exists, and lookaround conditions. Missing conditional group/name references fail explicitly at compile time.
Possessive quantifiers are also part of the shipped runtime path. Forms such as `a*+`, `a++`, `a?+`, and `a{2,3}+` behave like their greedy equivalents wrapped in an atomic group, so they do not backtrack once that quantified piece has matched.
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
- Possessive quantifiers use the same no-backtracking rule internally, so `a*+`, `a++`, `a?+`, and counted possessive forms behave like atomic-wrapped greedy repeats.
### Predicate code-block semantics
- Code blocks are zero-width predicate checkpoints in the VM path.
- They can fail the current path and allow normal regex backtracking to continue.
- Boolean-style success/failure still drives path control.
- Lua/JavaScript/Rhai/native numeric or string results also keep the current path successful and store the last winning-path non-boolean value in `MatchResult.code_result`.
- Wasm keeps the exported `() -> i32` predicate contract, and may additionally call `rgx.emit_numeric(...)` or `rgx.emit_replacement(...)` before returning non-zero to surface a winning-path payload.
- `replace_first_with_code` / `replace_all_with_code` consume only winning-path `Replacement(String)` values; numeric-only matches continue to round-trip unchanged in replacement mode.
- If a wasm module emits more than one non-boolean payload during one code-block execution, the last emitted payload wins.
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
