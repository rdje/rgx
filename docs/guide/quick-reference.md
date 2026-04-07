# Quick Reference

One-liner solutions for common rgx tasks. For full explanations, see the chapter links.

## Compiling and matching

```rust
// Compile a pattern
let re = Regex::compile(r"\d+")?;

// Compile with execution mode
let re = Regex::with_mode(r"\d+(?{lua:return true})", ExecutionMode::Safe)?;
let re = Regex::with_mode(r"\d+(?{native:check})", ExecutionMode::Full)?;

// Test if it matches
re.is_match("hello 42")                          // true

// Find first match
re.find_first("hello 42 world 99")               // Some(MatchResult { start: 6, end: 8, ... })

// Find all matches
re.find_all("42 and 99")                         // Vec<MatchResult>

// Match a file (whole)
re.match_file("data.txt")?                        // Vec<MatchResult>

// Match a file (line by line, with line numbers)
re.match_file_lines("data.txt")?                  // Vec<FileMatch>

// Scan a file (triggers callbacks, returns count)
re.scan_file("data.txt")?                         // usize
re.scan_file_lines("data.txt")?                   // usize
```

## Host variables

```rust
re.set_variable("key", "value")?;
```

Access from callbacks:

| Language | Syntax |
|----------|--------|
| Native | `ctx.variable("key")` -> `Option<String>` |
| Lua | `vars.key` |
| JS | `vars.key` |
| Rhai | `vars["key"]` |

## Capture groups

Access from callbacks:

| Language | Indexed | Named |
|----------|---------|-------|
| Native | `ctx.group(1)` | `ctx.named("name")` |
| Lua | `arg[1]` | `named.name` |
| JS | `arg[1]` | `named.name` |
| Rhai | `arg[1]` | `named["name"]` |

## Registering callbacks

```rust
// Native callback
re.register_native("name", |ctx| ExecResult::Success)?;

// WASM module
re.register_wasm_module("module_name", wasm_bytes)?;

// Inline (no registration needed)
// Lua:  (?{lua:return true})
// JS:   (?{js:return true;})
// Rhai: (?{rhai:true})
```

## Callback return values

```rust
ExecResult::Success                    // Match passes
ExecResult::Failure                    // Match fails (backtrack)
ExecResult::Numeric(42.0)             // Pass + return a number
ExecResult::Replacement("new".into()) // Pass + return replacement text
ExecResult::Error("msg".into())       // Treated as failure
ExecResult::Steer(SteerResult::Continue)   // Same as Success
ExecResult::Steer(SteerResult::Fail)       // Same as Failure
ExecResult::Steer(SteerResult::Accept)     // Commit match immediately
ExecResult::Steer(SteerResult::Skip(n))    // Advance n bytes
ExecResult::Steer(SteerResult::Abort)      // Stop all matching
```

## Emitting values from inline languages

```lua
-- Lua
rgx.emit_numeric(42.0)
rgx.emit_replacement("REDACTED")
```

```javascript
// JavaScript
rgx.emit_numeric(42.0)
rgx.emit_replacement("REDACTED")
```

```rust
// Rhai
emit_numeric(42.0)
emit_replacement("REDACTED")
```

## Collecting results

```rust
// Get first numeric code result
re.find_first_numeric_with_code("text")           // Option<f64>

// Get all numeric code results
re.find_all_numeric_with_code("text")              // Vec<f64>

// Replace using code results
re.replace_first_with_code("text")                 // String
re.replace_all_with_code("text")                   // String

// Branch identification
m.matched_branch_number                            // Option<usize> (1-based)
```

## Events

```rust
re.on_event(|event| {
    match event {
        MatchEvent::MatchAttemptStarted { position } => { /* ... */ }
        MatchEvent::MatchAttemptCompleted { position, matched } => { /* ... */ }
        MatchEvent::BranchEntered { branch, position } => { /* ... */ }
        MatchEvent::CaptureCompleted { group, start, end } => { /* ... */ }
        MatchEvent::BacktrackOccurred { position, stack_depth } => { /* ... */ }
        MatchEvent::CodeBlockEvaluated { language, succeeded, position } => { /* ... */ }
    }
})?;
```

## Async matching

```rust
// Manual suspend/resume
let mut outcome = re.find_first_suspendable(text);
loop {
    match outcome {
        MatchOutcome::Completed(result) => break,
        MatchOutcome::Suspended(cont) => {
            outcome = re.resume(*cont, ExecResult::Success);
        }
    }
}

// Automatic with resolver
let result = re.find_first_async(text, |name, ctx| async move {
    ExecResult::Success
}).await;
```

## Execution modes at a glance

| Mode | Inline code | Native callbacks | Use case |
|------|-------------|------------------|----------|
| `Pure` | No | No | Maximum performance, structural matching only |
| `Safe` | Yes (Lua/JS/Rhai/WASM) | No | Untrusted or semi-trusted patterns |
| `Full` | Yes | Yes | Full power, patterns you control |

## Code block syntax

```
(?{lua:code})              -- Lua inline
(?{js:code})               -- JavaScript inline
(?{rhai:code})             -- Rhai inline
(?{native:callback_name})  -- Registered native callback
(?{wasm:module:function})  -- WASM module function
```

## Context available inside callbacks

| Field | Native | Lua | JS | Rhai |
|-------|--------|-----|----|----- |
| Full text | `ctx.text` | `text` | `text` | `text` |
| Position | `ctx.position` | `pos` | `pos` | `pos` |
| Match start | `ctx.match_start` | `match_start` | `match_start` | `match_start` |
| Match end | `ctx.match_end` | `match_end` | `match_end` | `match_end` |
| Match length | `ctx.match_length()` | `match_length` | `match_length` | `match_length` |
| Group N | `ctx.group(n)` | `arg[n]` | `arg[n]` | `arg[n]` |
| Named group | `ctx.named("x")` | `named.x` | `named.x` | `named["x"]` |
| Variable | `ctx.variable("x")` | `vars.x` | `vars.x` | `vars["x"]` |
| Branch # | `ctx.matched_branch_number()` | `branch_number` | `branch_number` | `branch_number` |
