# Match Steering

Standard predicate callbacks answer a binary question: does this match pass or fail? Match steering goes further -- it lets a callback *direct* the engine. Instead of just "yes" or "no", you can say "skip ahead", "accept immediately", or "abort the entire search."

## SteerResult variants

| Variant | Effect |
|---------|--------|
| `SteerResult::Continue` | Proceed normally (same as `ExecResult::Success`) |
| `SteerResult::Fail` | Backtrack (same as `ExecResult::Failure`) |
| `SteerResult::Accept` | Force-accept the match at the current position, skipping any remaining pattern |
| `SteerResult::Skip(n)` | Advance the input cursor by `n` bytes, then continue matching |
| `SteerResult::Abort` | Stop the entire match search -- no more start positions will be tried |

## Native steering

Return `ExecResult::Steer(...)` from a native callback:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
let re = Regex::with_mode(
    r"(\w+)(?{native:route})",
    ExecutionMode::Full,
)?;

re.register_native("route", |ctx| {
    let word = ctx.group(1).unwrap_or("");
    match word {
        "STOP" => ExecResult::Steer(SteerResult::Abort),
        "SKIP" => ExecResult::Steer(SteerResult::Skip(4)),
        "OK"   => ExecResult::Steer(SteerResult::Accept),
        _      => ExecResult::Steer(SteerResult::Continue),
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Lua steering

In Lua code blocks, call functions on the `rgx` global table:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"(\w+)(?{lua:
        if arg[1] == "STOP" then
            rgx.steer_abort()
        elseif arg[1] == "SKIP" then
            rgx.steer_skip(4)
        elseif arg[1] == "ACCEPT" then
            rgx.steer_accept()
        end
        return true
    })"#,
    ExecutionMode::Safe,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Available Lua steering functions:
- `rgx.steer_continue()` -- proceed normally
- `rgx.steer_fail()` -- backtrack
- `rgx.steer_accept()` -- force-accept
- `rgx.steer_skip(n)` -- skip n bytes
- `rgx.steer_abort()` -- abort search

The steer call sets the steering action, and the `return true` lets the engine know the code block itself completed successfully. The steering action takes priority over the return value.

## JavaScript steering

In JavaScript code blocks, use camelCase on the `rgx` object:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"(\w+)(?{js:
        if (arg[1] === "STOP") {
            rgx.steerAbort();
        } else if (arg[1] === "ACCEPT") {
            rgx.steerAccept();
        }
        return true;
    })"#,
    ExecutionMode::Safe,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Available JavaScript steering functions:
- `rgx.steerContinue()`
- `rgx.steerFail()`
- `rgx.steerAccept()`
- `rgx.steerSkip(n)`
- `rgx.steerAbort()`

## Rhai steering

In Rhai code blocks, call the steering functions directly (they are registered as global functions):

```rust,ignore
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"(\w+)(?{rhai:
        if arg[1] == "STOP" { steer_abort(); }
        true
    })"#,
    ExecutionMode::Safe,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Available Rhai steering functions:
- `steer_continue()`
- `steer_fail()`
- `steer_accept()`
- `steer_skip(n)` where n is an integer
- `steer_abort()`

## WASM steering

WebAssembly modules call steering through the `rgx` import namespace. Five imports are wired into every module instantiation:

```wat
(module
    (import "rgx" "steer_continue" (func $steer_continue))
    (import "rgx" "steer_fail"     (func $steer_fail))
    (import "rgx" "steer_accept"   (func $steer_accept))
    (import "rgx" "steer_skip"     (func $steer_skip (param i32)))
    (import "rgx" "steer_abort"    (func $steer_abort))

    (func (export "evaluate") (result i32)
        ;; Force-accept regardless of the i32 return value.
        call $steer_accept
        i32.const 0
    )
)
```

The WASM module's exported function still returns `i32` (its predicate result), but **the steer takes priority** — if any `rgx.steer_*` import is called, the eventual `ExecResult` is `Steer(...)` with the corresponding variant, ignoring the return value entirely. This matches the precedence used by the Lua / JS / Rhai hosts.

`rgx.steer_skip` takes an `i32` byte count. Negative values are an error (the import returns a wasm trap, surfacing as `ExecResult::Error`).

A WASM-side helper can be exposed via a small wrapper if you want function-style ergonomics in your guest language (Rust, AssemblyScript, etc.); the imports themselves are the underlying primitive.

## Decision guide

When should you use each variant?

### Continue

Use when the callback is purely informational or when the match should proceed normally after side-effect processing:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
# use std::sync::{Arc, Mutex};
let re = Regex::with_mode(r"(\w+)(?{native:log})", ExecutionMode::Full)?;
let log = Arc::new(Mutex::new(Vec::new()));
let log_clone = log.clone();

re.register_native("log", move |ctx| {
    log_clone.lock().unwrap().push(ctx.group(1).unwrap_or("").to_string());
    ExecResult::Steer(SteerResult::Continue)
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Fail

Use when the callback detects an invalid match that should be rejected:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
let re = Regex::with_mode(r"(\d+)(?{native:even_only})", ExecutionMode::Full)?;

re.register_native("even_only", |ctx| {
    let n: i64 = ctx.group(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    if n % 2 == 0 {
        ExecResult::Steer(SteerResult::Continue)
    } else {
        ExecResult::Steer(SteerResult::Fail)
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Accept

Use to short-circuit matching when you've seen enough. This is valuable for "find the first valid thing" patterns where the remaining pattern structure is just for validation:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
let re = Regex::with_mode(
    r"(?{native:gate}).*",
    ExecutionMode::Full,
)?;

re.register_native("gate", |ctx| {
    // Accept the match immediately at the current position
    ExecResult::Steer(SteerResult::Accept)
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Skip(n)

Use to jump past known uninteresting content. This is useful for protocols or binary-like formats where you know the next N bytes can be skipped:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
let re = Regex::with_mode(
    r"HEADER:(\d+):(?{native:skip_payload})",
    ExecutionMode::Full,
)?;

re.register_native("skip_payload", |ctx| {
    let payload_len: usize = ctx.group(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    ExecResult::Steer(SteerResult::Skip(payload_len))
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Abort

Use when you know no further matches are possible or desirable. This stops the engine from trying subsequent start positions, which can be a significant performance win:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
let re = Regex::with_mode(
    r"(END_MARKER|(\w+))(?{native:check_end})",
    ExecutionMode::Full,
)?;

re.register_native("check_end", |ctx| {
    if ctx.group(1) == Some("END_MARKER") {
        ExecResult::Steer(SteerResult::Abort)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Example: smart tokenizer with early termination

Combining steering with branch identification:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
let re = Regex::with_mode(
    r"(?:(\d+)|([a-zA-Z_]\w*)|(\S))(?{native:classify})",
    ExecutionMode::Full,
)?;

re.register_native("classify", |ctx| {
    match ctx.matched_branch_number() {
        Some(1) => {
            // Number token -- continue scanning
            ExecResult::Steer(SteerResult::Continue)
        }
        Some(2) => {
            let ident = ctx.group(2).unwrap_or("");
            if ident == "EOF" {
                // Stop scanning when we see EOF token
                ExecResult::Steer(SteerResult::Abort)
            } else {
                ExecResult::Steer(SteerResult::Continue)
            }
        }
        Some(3) => {
            // Unknown character -- skip it
            ExecResult::Steer(SteerResult::Continue)
        }
        _ => ExecResult::Steer(SteerResult::Continue),
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```
