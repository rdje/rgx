# Context Reference

This appendix documents every field and method available to callbacks in each language backend.

## Native callbacks (Rust)

Native callbacks receive an `ExecContext` reference:

```rust,ignore
re.register_native("name", |ctx: &ExecContext| -> ExecResult {
    // ...
});
```

### ExecContext fields

| Field / Method | Type | Description |
|---------------|------|-------------|
| `ctx.text` | `String` | The full input text being matched |
| `ctx.position` | `usize` | Current byte position in the input |
| `ctx.match_start` | `usize` | Start byte offset of the current match attempt |
| `ctx.match_end` | `usize` | End byte offset of the current match attempt |
| `ctx.match_start()` | `usize` | Getter for match start |
| `ctx.match_end()` | `usize` | Getter for match end |
| `ctx.match_length()` | `usize` | `match_end - match_start` |

### Capture access

| Method | Return type | Description |
|--------|-------------|-------------|
| `ctx.current_match()` | `Option<&str>` | Group 0 (the full match text) |
| `ctx.group(n)` | `Option<&str>` | Capture group by index (0-based) |
| `ctx.named("name")` | `Option<&str>` | Named capture group |
| `ctx.captures` | `Vec<Option<String>>` | All capture slots (direct field access) |
| `ctx.named_captures` | `HashMap<String, String>` | All named captures (direct field access) |

### Variable access

| Method | Return type | Description |
|--------|-------------|-------------|
| `ctx.variable("name")` | `Option<String>` | String variable |
| `ctx.typed_variable("name")` | `Option<Value>` | Typed variable (raw Value) |
| `ctx.var_str("name")` | `Option<String>` | String value |
| `ctx.var_int("name")` | `Option<i64>` | Integer value |
| `ctx.var_float("name")` | `Option<f64>` | Float value (int widened to f64) |
| `ctx.var_bool("name")` | `Option<bool>` | Boolean value |
| `ctx.var_array("name")` | `Option<Vec<Value>>` | Array clone |
| `ctx.var_map("name")` | `Option<Vec<(String, Value)>>` | Map clone |
| `ctx.variables_snapshot()` | `HashMap<String, String>` | Clone all string variables |

### Branch and metadata

| Method | Return type | Description |
|--------|-------------|-------------|
| `ctx.matched_branch_number()` | `Option<usize>` | 1-based top-level branch number |

## Lua callbacks

Lua code blocks have access to the following globals:

### arg table

| Variable | Type | Description |
|----------|------|-------------|
| `arg[0]` | string | Full match text (group 0) |
| `arg[1]` | string | Capture group 1 |
| `arg[2]` | string | Capture group 2 |
| ... | string | Capture group N |

### vars table

| Variable | Type | Description |
|----------|------|-------------|
| `vars["name"]` | string | Host-provided string variable |

### rgx table

| Function | Description |
|----------|-------------|
| `rgx.emit_numeric(n)` | Emit a numeric value on the winning path |
| `rgx.emit_replacement(s)` | Emit a replacement string on the winning path |
| `rgx.steer_continue()` | Continue matching normally |
| `rgx.steer_fail()` | Backtrack from this point |
| `rgx.steer_accept()` | Force-accept the match |
| `rgx.steer_skip(n)` | Skip n bytes |
| `rgx.steer_abort()` | Abort the entire match search |

### Match metadata

| Variable | Type | Description |
|----------|------|-------------|
| `match_start` | number | Start byte offset |
| `match_end` | number | End byte offset |
| `match_length` | number | Match length in bytes |
| `position` | number | Current byte position |
| `branch` | number or nil | 1-based branch number (nil if not in alternation) |

### Return values

| Return | Effect |
|--------|--------|
| `return true` | Success -- matching continues |
| `return false` | Failure -- engine backtracks |
| `return <number>` | Numeric result (treated as success) |
| `return <string>` | Replacement result (treated as success) |

## JavaScript callbacks

JavaScript code blocks have access to the following globals:

### arg array

| Variable | Type | Description |
|----------|------|-------------|
| `arg[0]` | string | Full match text (group 0) |
| `arg[1]` | string | Capture group 1 |
| `arg[2]` | string | Capture group 2 |
| ... | string | Capture group N |

### vars object

| Variable | Type | Description |
|----------|------|-------------|
| `vars["name"]` | string | Host-provided string variable |
| `vars.name` | string | Dot notation also works |

### rgx object

| Function | Description |
|----------|-------------|
| `rgx.emitNumeric(n)` | Emit a numeric value |
| `rgx.emitReplacement(s)` | Emit a replacement string |
| `rgx.steerContinue()` | Continue matching normally |
| `rgx.steerFail()` | Backtrack |
| `rgx.steerAccept()` | Force-accept |
| `rgx.steerSkip(n)` | Skip n bytes |
| `rgx.steerAbort()` | Abort search |

### Match metadata

| Variable | Type | Description |
|----------|------|-------------|
| `match_start` | number | Start byte offset |
| `match_end` | number | End byte offset |
| `match_length` | number | Match length in bytes |
| `position` | number | Current byte position |
| `branch` | number or undefined | 1-based branch number |

### Return values

| Return | Effect |
|--------|--------|
| `return true` | Success |
| `return false` | Failure |
| `return <number>` | Numeric result (success) |
| `return <string>` | Replacement result (success) |

## Rhai callbacks

Rhai code blocks have access to the following:

### arg array

| Variable | Type | Description |
|----------|------|-------------|
| `arg[0]` | string | Full match text |
| `arg[1]` | string | Capture group 1 |
| ... | string | Capture group N |

### vars map

| Variable | Type | Description |
|----------|------|-------------|
| `vars["name"]` | string | Host-provided string variable |

### Global functions

| Function | Description |
|----------|-------------|
| `emit_numeric(n)` | Emit a numeric value |
| `emit_replacement(s)` | Emit a replacement string |
| `steer_continue()` | Continue normally |
| `steer_fail()` | Backtrack |
| `steer_accept()` | Force-accept |
| `steer_skip(n)` | Skip n bytes |
| `steer_abort()` | Abort search |

### Match metadata

| Variable | Type | Description |
|----------|------|-------------|
| `match_start` | i64 | Start byte offset |
| `match_end` | i64 | End byte offset |
| `match_length` | i64 | Match length |
| `position` | i64 | Current byte position |
| `branch` | i64 or () | 1-based branch number |

### Return values

The last expression in the Rhai block is the return value:

| Value | Effect |
|-------|--------|
| `true` | Success |
| `false` | Failure |
| integer or float | Numeric result (success) |
| string | Replacement result (success) |

## WASM callbacks

WASM callbacks receive a serialized context and return a boolean:

| Input | Type | Description |
|-------|------|-------------|
| Match text | bytes | The matched text |
| Captures | byte array | Serialized capture data |
| Variables | byte array | Serialized variable map |

| Output | Type | Description |
|--------|------|-------------|
| Result | i32 | 1 = success, 0 = failure |

WASM callbacks currently support only pass/fail results. Numeric, replacement, and steering results are not available in WASM.

## ExecContextSnapshot (async)

When using `find_first_suspendable`, the continuation carries an `ExecContextSnapshot`:

| Field | Type | Description |
|-------|------|-------------|
| `position` | `usize` | Current byte position |
| `match_start` | `usize` | Start of the current match attempt |
| `captures` | `Vec<Option<usize>>` | Capture group byte-offset slots |
| `variables` | `HashMap<String, String>` | Host variables snapshot |
