# Context Reference

This page is a complete reference for everything available inside a callback. Use it when you need to look up the exact name of a field or function in a specific language.

## Native Rust (`ExecContext`)

Native callbacks receive a reference to `ExecContext`. All fields are read-only during callback execution.

### Fields

| Field / Method | Return type | Description |
|---------------|-------------|-------------|
| `ctx.text` | `String` | The entire input text being matched |
| `ctx.position` | `usize` | Current byte offset of the engine's cursor |
| `ctx.match_start` | `usize` | Byte offset where the current match attempt started |
| `ctx.match_end` | `usize` | Byte offset of the engine's current position (end of matched portion so far) |
| `ctx.match_length()` | `usize` | `match_end - match_start` |
| `ctx.matched_branch_number()` | `Option<usize>` | 1-based top-level alternation branch number, or `None` if no top-level alternation |
| `ctx.captures` | `Vec<Option<String>>` | All capture groups (0-indexed). `None` if a group didn't participate. |
| `ctx.named_captures` | `HashMap<String, String>` | Named capture groups. Only populated groups are present. |
| `ctx.variables` | `Arc<RwLock<HashMap<String, String>>>` | Host-provided variables (prefer accessor methods below) |

### Accessor methods

| Method | Return type | Description |
|--------|-------------|-------------|
| `ctx.current_match()` | `Option<&str>` | Shorthand for `ctx.captures[0]` -- the full match text |
| `ctx.group(n)` | `Option<&str>` | Capture group `n` (0 = full match, 1+ = sub-groups) |
| `ctx.named("name")` | `Option<&str>` | Named capture group by name |
| `ctx.variable("key")` | `Option<String>` | Host variable by name |
| `ctx.variables_snapshot()` | `HashMap<String, String>` | Clone of all host variables |

### Example

```rust
re.register_native("example", |ctx| {
    // Full input
    let text: &str = &ctx.text;

    // Position info
    let pos: usize = ctx.position;
    let start: usize = ctx.match_start;
    let end: usize = ctx.match_end;
    let len: usize = ctx.match_length();

    // Captures
    let full_match: Option<&str> = ctx.current_match();
    let group_1: Option<&str> = ctx.group(1);
    let named: Option<&str> = ctx.named("my_group");

    // Host variables
    let var: Option<String> = ctx.variable("threshold");

    // Branch
    let branch: Option<usize> = ctx.matched_branch_number();

    ExecResult::Success
})?;
```

## Lua

Lua code blocks receive their context through global variables set before execution.

### Globals

| Global | Lua type | Description |
|--------|----------|-------------|
| `arg` | table (0-indexed) | Capture groups. `arg[0]` = full match, `arg[1]` = group 1, etc. |
| `named` | table | Named capture groups. `named.my_group` or `named["my_group"]` |
| `vars` | table | Host variables. `vars.threshold` or `vars["threshold"]` |
| `text` | string | The entire input text |
| `pos` | number | Current byte position |
| `match_start` | number | Match attempt start position |
| `match_end` | number | Current end position |
| `match_length` | number | `match_end - match_start` |
| `branch_number` | number or nil | 1-based branch number, `nil` if no alternation |
| `rgx` | table | Utility namespace (see below) |

### rgx namespace

| Function | Description |
|----------|-------------|
| `rgx.emit_numeric(n)` | Emit a numeric value for the match result |
| `rgx.emit_replacement(s)` | Emit a replacement string for the match result |
| `rgx.steer_continue()` | Continue matching normally |
| `rgx.steer_fail()` | Reject this path and backtrack |
| `rgx.steer_accept()` | Commit the match immediately |
| `rgx.steer_skip(n)` | Advance `n` bytes before continuing |
| `rgx.steer_abort()` | Stop the entire search |

Steer actions take highest priority — if called, they override the return value.

### Return value interpretation

| Return value | Interpreted as |
|-------------|---------------|
| `true` | `ExecResult::Success` |
| `false` | `ExecResult::Failure` |
| integer | `ExecResult::Numeric(n as f64)` |
| number (float) | `ExecResult::Numeric(n)` |
| string | `ExecResult::Replacement(s)` |
| `nil` | `ExecResult::Success` |
| anything else | `ExecResult::Success` |

### Sandboxed: removed globals

These standard Lua libraries are removed in rgx's sandbox:

- `io` (file I/O)
- `os` (operating system)
- `debug` (debug interface)
- `require` (module loading)
- `loadfile` (file loading)
- `dofile` (file execution)
- `package` (package system)

### Example

```
(?{lua:
    local full = arg[0]
    local group1 = arg[1]
    local name = named.my_group
    local threshold = tonumber(vars.threshold) or 100
    local at_start = match_start == 0
    return tonumber(group1) <= threshold
})
```

## JavaScript

JavaScript code blocks receive their context through global variables set before execution.

### Globals

| Global | JS type | Description |
|--------|---------|-------------|
| `arg` | Array | Capture groups. `arg[0]` = full match, `arg[1]` = group 1, etc. |
| `named` | Object | Named capture groups. `named.my_group` or `named["my_group"]` |
| `vars` | Object | Host variables. `vars.threshold` or `vars["threshold"]` |
| `text` | string | The entire input text |
| `pos` | number | Current byte position |
| `match_start` | number | Match attempt start position |
| `match_end` | number | Current end position |
| `match_length` | number | `match_end - match_start` |
| `branch_number` | number or undefined | 1-based branch number, `undefined` if no alternation |
| `rgx` | Object | Utility namespace (see below) |

### rgx namespace

| Function | Description |
|----------|-------------|
| `rgx.emit_numeric(n)` | Emit a numeric value for the match result |
| `rgx.emit_replacement(s)` | Emit a replacement string for the match result |
| `rgx.steerContinue()` | Continue matching normally |
| `rgx.steerFail()` | Reject this path and backtrack |
| `rgx.steerAccept()` | Commit the match immediately |
| `rgx.steerSkip(n)` | Advance `n` bytes before continuing |
| `rgx.steerAbort()` | Stop the entire search |

Steer actions take highest priority — if called, they override the return value.

### Return value interpretation

| Return value | Interpreted as |
|-------------|---------------|
| `true` | `ExecResult::Success` |
| `false` | `ExecResult::Failure` |
| number | `ExecResult::Numeric(n)` |
| string | `ExecResult::Replacement(s)` |
| `null` | `ExecResult::Success` |
| `undefined` | `ExecResult::Success` |
| anything else | `ExecResult::Success` |

### Code evaluation

JavaScript code blocks are evaluated in two stages:
1. Direct evaluation: the code is evaluated as an expression. If it produces a value, that value is used.
2. IIFE fallback: if direct evaluation fails (e.g., because the code uses `return`), the code is wrapped in an immediately-invoked function expression: `(function(){ <code> })()`.

This means both styles work:

```javascript
// Expression style (no return needed)
(?{js: named.amount > 100 })

// Block style (with return)
(?{js:
    var x = parseInt(arg[1]);
    if (x > vars.threshold) return true;
    return false;
})
```

### Sandboxed: removed globals

These are removed or set to `undefined` in rgx's sandbox:

- `eval`
- `Function` (constructor)
- `fetch`
- `XMLHttpRequest`

Additionally, memory limit (10MB) and stack size limit (256KB) are enforced.

### Example

```
(?{js:
    var full = arg[0];
    var group1 = arg[1];
    var name = named.my_group;
    var threshold = parseInt(vars.threshold) || 100;
    return parseInt(group1) <= threshold;
})
```

## Rhai

Rhai code blocks receive their context through variables injected into the evaluation scope.

### Scope variables

| Variable | Rhai type | Description |
|----------|-----------|-------------|
| `arg` | Array | Capture groups. `arg[0]` = full match, `arg[1]` = group 1, etc. Unmatched groups are `()` (unit). |
| `named` | Map | Named capture groups. `named["my_group"]` |
| `vars` | Map | Host variables. `vars["threshold"]` |
| `text` | String | The entire input text |
| `pos` | i64 | Current byte position |
| `match_start` | i64 | Match attempt start position |
| `match_end` | i64 | Current end position |
| `match_length` | i64 | `match_end - match_start` |
| `branch_number` | i64 or `()` | 1-based branch number, `()` (unit) if no alternation |

### Built-in functions

| Function | Description |
|----------|-------------|
| `emit_numeric(n)` | Emit a numeric value for the match result (accepts i64 or f64) |
| `emit_replacement(s)` | Emit a replacement string for the match result |
| `steer_continue()` | Continue matching normally |
| `steer_fail()` | Reject this path and backtrack |
| `steer_accept()` | Commit the match immediately |
| `steer_skip(n)` | Advance `n` bytes before continuing (n: i64) |
| `steer_abort()` | Stop the entire search |

Note: In Rhai, all functions are top-level (not on a namespace). Steer actions take highest priority.

### Return value interpretation

| Return value | Interpreted as |
|-------------|---------------|
| `true` | `ExecResult::Success` |
| `false` | `ExecResult::Failure` |
| i64 | `ExecResult::Numeric(n as f64)` |
| f64 | `ExecResult::Numeric(n)` |
| String | `ExecResult::Replacement(s)` |
| `()` (unit) | `ExecResult::Success` |
| anything else | `ExecResult::Success` |

### Sandboxed properties

- `print` outputs are suppressed (no-op)
- `debug` outputs are suppressed (no-op)
- No external module resolver is configured
- Each evaluation gets a fresh engine and scope (no leaked state between evaluations)

### Example

```
(?{rhai:
    let full = arg[0];
    let group1 = arg[1];
    let name = named["my_group"];
    let threshold = parse_int(vars["threshold"]);
    parse_int(group1) <= threshold
})
```

Note: Rhai uses `parse_int()` for string-to-integer conversion, and the last expression in the block is the return value (explicit `return` is also supported).

## WASM

WASM callbacks are invoked via `(?{wasm:module_name:function_name})`. The module must be registered with `register_wasm_module` before matching.

### Module requirements

The WASM module must export a function matching the registered name. The function should:
- Accept no parameters
- Return an `i32`: `1` for success (pass), `0` for failure

### Available imports

The module can import functions from the `rgx` namespace:

| Import | Signature | Description |
|--------|-----------|-------------|
| `rgx.emit_numeric` | `(f64) -> ()` | Emit a numeric value |
| `rgx.emit_replacement` | `(i32, i32) -> ()` | Emit a replacement string (pointer, length) |

For `emit_replacement`, the pointer and length refer to the module's own linear memory. The host reads the bytes from the module's memory at the given offset and length.

### Example (WAT format)

```wasm
(module
    (import "rgx" "emit_numeric" (func $emit_numeric (param f64)))
    (func (export "validate") (result i32)
        ;; Emit a numeric value
        f64.const 42.0
        call $emit_numeric
        ;; Return success
        i32.const 1
    )
)
```

### Registering

```rust
let wasm_bytes = std::fs::read("validator.wasm")?;
re.register_wasm_module("validator", wasm_bytes)?;
```

### Limitations

- WASM callbacks cannot directly access the match context (captures, position, variables). They are best used for stateless validation logic or as portable computation modules.
- The WASM execution environment uses wasmtime with no WASI access.

## Cross-language comparison

### Accessing capture group 1

| Language | Syntax | Returns |
|----------|--------|---------|
| Native | `ctx.group(1)` | `Option<&str>` |
| Lua | `arg[1]` | string or nil |
| JavaScript | `arg[1]` | string or undefined |
| Rhai | `arg[1]` | string or `()` |

### Accessing named capture "foo"

| Language | Syntax | Returns |
|----------|--------|---------|
| Native | `ctx.named("foo")` | `Option<&str>` |
| Lua | `named.foo` or `named["foo"]` | string or nil |
| JavaScript | `named.foo` or `named["foo"]` | string or undefined |
| Rhai | `named["foo"]` | string or `()` |

### Accessing host variable "max"

| Language | Syntax | Returns |
|----------|--------|---------|
| Native | `ctx.variable("max")` | `Option<String>` |
| Lua | `vars.max` or `vars["max"]` | string or nil |
| JavaScript | `vars.max` or `vars["max"]` | string or undefined |
| Rhai | `vars["max"]` | string or `()` |

### Emitting a numeric result

| Language | Syntax |
|----------|--------|
| Native | `return ExecResult::Numeric(42.0)` |
| Lua | `rgx.emit_numeric(42.0)` then `return true` |
| JavaScript | `rgx.emit_numeric(42.0); return true;` |
| Rhai | `emit_numeric(42.0); return true;` |
| WASM | Import and call `rgx.emit_numeric` |

### Emitting a replacement result

| Language | Syntax |
|----------|--------|
| Native | `return ExecResult::Replacement("text".into())` |
| Lua | `rgx.emit_replacement("text")` then `return true` |
| JavaScript | `rgx.emit_replacement("text"); return true;` |
| Rhai | `emit_replacement("text"); return true;` |
| WASM | Import and call `rgx.emit_replacement` with pointer and length |

Note: In Lua, JavaScript, and Rhai, you can also return a numeric or string value directly instead of using the emit functions. The emit functions are useful when you need to perform additional logic after setting the result value but before returning.

### Returning directly vs emitting

| Style | When to use |
|-------|-------------|
| Return a number directly | `(?{lua:return 42})` -- simple, single expression |
| Return a string directly | `(?{js:return "replaced"})` -- simple transformation |
| Emit then return true | `(?{lua:rgx.emit_numeric(42); return true})` -- when you need multiple statements |
| Return true/false | `(?{rhai: x > 10})` -- pure predicate, no value to surface |

If you both return a value and emit a value, the returned value takes precedence for `Numeric` and `Replacement` results. If you return `true` (Success), the emitted value is used. If you return `false` (Failure), the emitted value is discarded (the match failed).

### Steering the match (accept immediately)

| Language | Syntax |
|----------|--------|
| Native | `return ExecResult::Steer(SteerResult::Accept)` |
| Lua | `rgx.steer_accept()` |
| JavaScript | `rgx.steerAccept()` |
| Rhai | `steer_accept()` |

Other actions: `continue`, `fail`, `skip(n)`, `abort` — same naming pattern per language.
