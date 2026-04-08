# Predicate Callbacks

A predicate callback is code that runs *during* matching. When the regex engine reaches a code block, it evaluates the callback and uses its result to decide whether matching should continue or backtrack. This makes patterns programmable without leaving the regex.

## Native callbacks

Native callbacks are Rust closures registered by name. They receive an `ExecContext` with full match state and return an `ExecResult`.

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(
    r"(\d{4})-(\d{2})-(\d{2})(?{native:validate_date})",
    ExecutionMode::Full,
)?;

re.register_native("validate_date", |ctx| {
    let month: u32 = ctx.group(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let day: u32 = ctx.group(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    if (1..=12).contains(&month) && (1..=31).contains(&day) {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("2026-04-08"));   // valid date
assert!(!re.is_match("2026-13-01"));  // month 13 is invalid
assert!(!re.is_match("2026-04-32"));  // day 32 is invalid
# Ok::<(), Box<dyn std::error::Error>>(())
```

The pattern `(?{native:validate_date})` tells the engine: "at this point, call the native callback named `validate_date`." If it returns `Success`, matching continues. If `Failure`, the engine backtracks as if the pattern didn't match.

## Inline Lua

Lua code blocks are embedded directly in the pattern with `(?{lua:...})`. No registration needed -- the code is the callback:

```rust
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"(\d+)(?{lua:return tonumber(arg[1]) % 2 == 0})"#,
    ExecutionMode::Safe,
)?;

// Only matches even numbers
assert!(re.is_match("42"));
assert!(!re.is_match("43"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Inside Lua code blocks:
- `arg[0]` is the full match text
- `arg[1]`, `arg[2]`, ... are captured groups
- `vars["name"]` accesses host-provided variables
- Return `true` for success, `false` for failure

## Inline JavaScript

JavaScript code blocks use `(?{js:...})`:

```rust
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"([\w.+-]+)@([\w.-]+)(?{js:return arg[2].split('.').length >= 2})"#,
    ExecutionMode::Safe,
)?;

assert!(re.is_match("user@example.com"));
assert!(!re.is_match("user@localhost"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Inside JavaScript code blocks:
- `arg[0]` is the full match text
- `arg[1]`, `arg[2]`, ... are captured groups
- `vars["name"]` accesses host-provided variables
- Return `true`/`false` for pass/fail

## Inline Rhai

Rhai code blocks use `(?{rhai:...})`:

```rust
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"(\d+)(?{rhai:parse_int(arg[1]) >= 18})"#,
    ExecutionMode::Safe,
)?;

assert!(re.is_match("21"));
assert!(!re.is_match("15"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Inside Rhai code blocks:
- `arg[0]` is the full match text
- `arg[1]`, `arg[2]`, ... are captured groups
- `vars["name"]` accesses host-provided variables
- The last expression is the return value (truthy/falsy)

## WASM callbacks

WebAssembly callbacks call an exported function from a registered WASM module:

```text
(?{wasm:module_name:function_name})
```

The WASM function receives the match context and returns a boolean. WASM modules are registered on the CLI with `--wasm-module name=path.wasm`.

WASM callbacks are the most portable option -- compile once, run anywhere -- and the most restricted. They cannot access the host filesystem or network.

## Execution modes

The `ExecutionMode` enum controls which callback types are allowed:

| Mode | Pure regex | Lua/JS/Rhai | Native callbacks | WASM |
|------|-----------|-------------|-----------------|------|
| `Pure` | Yes | No | No | No |
| `Safe` | Yes | Yes | No | Yes |
| `Full` | Yes | Yes | Yes | Yes |

```rust
# use rgx_core::{Regex, ExecutionMode};
// Pure mode — code blocks are syntax errors
assert!(Regex::with_mode(r"\d+", ExecutionMode::Pure).is_ok());
assert!(Regex::with_mode(r"\d+(?{lua:true})", ExecutionMode::Pure).is_err());

// Safe mode — scripted callbacks only
assert!(Regex::with_mode(r"\d+(?{lua:true})", ExecutionMode::Safe).is_ok());

// Full mode — everything, including native callbacks
let re = Regex::with_mode(r"\d+(?{native:check})", ExecutionMode::Full)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The separation exists for security: `Safe` mode guarantees that all code runs in sandboxed environments with no filesystem, network, or system access. `Full` mode enables native callbacks, which run as Rust closures with full access to the host process.

## Return values

Callbacks can return more than pass/fail:

### ExecResult variants

| Variant | Effect |
|---------|--------|
| `ExecResult::Success` | Match continues normally |
| `ExecResult::Failure` | Backtrack from this point |
| `ExecResult::Numeric(f64)` | Success + attach a numeric value |
| `ExecResult::Replacement(String)` | Success + attach replacement text |
| `ExecResult::Structured(Value)` | Success + attach a structured value |
| `ExecResult::Error(String)` | Treated as failure (logged) |
| `ExecResult::Steer(SteerResult)` | Match steering (see next chapter) |
| `ExecResult::Suspend(String)` | Pause for async resolution (see Async I/O) |

### emit_numeric and emit_replacement

In scripted languages, use the `rgx` helper object to emit values alongside a boolean result:

**Lua:**
```text
(\d+)(?{lua:rgx.emit_numeric(tonumber(arg[1]) * 2.5); return true})
```

**JavaScript:**
```text
(\d+)(?{js:rgx.emitNumeric(parseInt(arg[1]) * 2.5); return true})
```

**Rhai:**
```text
(\d+)(?{rhai:emit_numeric(parse_int(arg[1]) * 2); true})
```

The emitted value is captured on the winning match path as a `CodeBlockValue` on the `MatchResult`. This is useful when you want the callback to both validate *and* produce a computed result.

## Sandboxing details

All scripted code blocks (Lua, JavaScript, Rhai) run in fully sandboxed environments:

- **No filesystem access** -- `io.open`, `require`, `fs.readFileSync` are not available
- **No network access** -- no sockets, HTTP, or DNS
- **No system calls** -- no `os.execute`, `process.exit`, or similar
- **Memory limits** -- each execution context has bounded memory
- **Time limits** -- infinite loops are terminated by the engine

Native callbacks (`Full` mode) run as regular Rust closures and are not sandboxed. The host application is responsible for ensuring native callbacks are safe.

## Multiple callbacks in one pattern

You can chain multiple code blocks. Each acts as a gate:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(
    r"(\d{4})-(\d{2})-(\d{2})(?{native:valid_range})(?{native:not_weekend})",
    ExecutionMode::Full,
)?;

re.register_native("valid_range", |ctx| {
    let month: u32 = ctx.group(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    if (1..=12).contains(&month) { ExecResult::Success } else { ExecResult::Failure }
})?;

re.register_native("not_weekend", |ctx| {
    // Simplified: reject specific dates
    let day: u32 = ctx.group(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    if day == 0 { ExecResult::Failure } else { ExecResult::Success }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Both callbacks must succeed for the overall match to succeed. If the first fails, the second is never called.

## Mixing languages

You can mix callback types in a single pattern:

```text
(\d+)(?{lua:return tonumber(arg[1]) > 0})(?{native:log_match})
```

The Lua block validates the number is positive. If it passes, the native callback runs for side-effect logging. This works because the engine evaluates code blocks left to right, and each must succeed for matching to continue.
