# Execution Modes

rgx has three execution modes that control which features are available. Choose the minimum mode that covers your needs -- the engine is faster when fewer features are enabled.

## Comparison table

| Capability | Pure | Safe | Full |
|------------|:----:|:----:|:----:|
| Pattern matching | Yes | Yes | Yes |
| Capture groups | Yes | Yes | Yes |
| Find / Replace / Split | Yes | Yes | Yes |
| RegexSet | Yes | Yes | Yes |
| BytesRegex | Yes | Yes | Yes |
| File matching | Yes | Yes | Yes |
| tail_file | Yes | Yes | Yes |
| Structured events | Yes | Yes | Yes |
| Safety limits | Yes | Yes | Yes |
| Lua code blocks | No | Yes | Yes |
| JavaScript code blocks | No | Yes | Yes |
| Rhai code blocks | No | Yes | Yes |
| WASM callbacks | No | Yes | Yes |
| Native callbacks | No | No | Yes |
| Suspendable matching | No | No | Yes |
| Match steering (scripted) | No | Yes | Yes |
| Match steering (native) | No | No | Yes |
| Host variables | No | Yes | Yes |

## Pure mode

```rust
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(r"\d+", ExecutionMode::Pure)?;
// or equivalently:
let re = Regex::compile(r"\d+")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Pure mode is the default when you use `Regex::compile`. It provides maximum performance with zero code-execution overhead. Patterns containing code blocks (`(?{lua:...})`, `(?{native:...})`, etc.) are rejected at compile time.

**When to use:** Any regex work that doesn't need embedded code. This covers the vast majority of use cases.

**Performance:** Fastest. No execution manager is allocated. The engine takes the shortest code path for all operations.

## Safe mode

```rust,no_run
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"\d+(?{lua:return tonumber(arg[0]) > 0})"#,
    ExecutionMode::Safe,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Safe mode enables Lua, JavaScript, Rhai, and WASM code blocks. All scripted code runs in fully sandboxed environments:

- **No filesystem access**
- **No network access**
- **No system calls**
- **Memory bounded**
- **Time bounded** (infinite loops are terminated)

Native callbacks (`(?{native:...})`) are not allowed in Safe mode. This guarantees that all code running during matching is sandboxed.

**When to use:** When patterns need embedded logic but you want to guarantee that all code is sandboxed. Good for accepting user-provided patterns or running patterns from untrusted sources.

## Full mode

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(
    r"\d+(?{native:validate})",
    ExecutionMode::Full,
)?;

re.register_native("validate", |ctx| {
    ExecResult::Success
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Full mode enables everything: all scripted backends plus native callbacks. Native callbacks are Rust closures with full access to the host process -- they are not sandboxed.

Full mode also enables:
- **Suspendable matching** (`find_first_suspendable`, `resume`)
- **Async helpers** (`find_first_async`)
- **Native match steering** (`ExecResult::Steer(...)`)

**When to use:** When you need native callbacks for performance-critical logic, async I/O integration, or full host-process access during matching.

**Security note:** Only use Full mode with patterns you control. Never use Full mode with user-provided patterns -- a user could reference any registered native callback.

## Choosing the right mode

```text
Do you need code blocks in patterns?
  No  --> Pure
  Yes --> Do you need native callbacks?
            No  --> Safe
            Yes --> Full
```

Most applications fall into one of two patterns:

1. **Pure for user-facing patterns, Full for internal patterns.** User search queries compile in Pure mode. Internal patterns (log parsing, data pipelines) compile in Full mode with registered callbacks.

2. **Safe for plugin/extension patterns.** Patterns loaded from configuration files or plugins compile in Safe mode, guaranteeing sandbox isolation.

## Runtime mode detection

The mode is set at compilation time and cannot be changed afterward. You can query it on the engine:

```rust
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(r"\d+", ExecutionMode::Safe)?;
// The mode is baked into the compiled regex
# Ok::<(), Box<dyn std::error::Error>>(())
```

Attempting to register a native callback on a Safe-mode regex returns an error:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(r"\d+", ExecutionMode::Safe)?;
let result = re.register_native("cb", |_| ExecResult::Success);
assert!(result.is_err());  // native callbacks not available in Safe mode
# Ok::<(), Box<dyn std::error::Error>>(())
```
