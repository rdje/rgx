# HOST INTEGRATION GUIDE
Practical guide to using rgx's host integration features — with examples.

rgx is more than a regex engine. It's a **programmable matching engine** where your application and the regex work together. You write patterns, register callbacks, and the engine connects them — no need to leave the regex world.

This guide walks through each integration layer with real examples. Start simple, go as deep as you need.

---

## Layer 1 — Passing Data In and Out

### Host variables

Pass data from your application into the regex. Code blocks can read it during matching.

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?{js:vars.env === "prod"})"#,
    ExecutionMode::Safe,
)?;
re.set_variable("env", "prod")?;
assert!(re.is_match(""));

re.set_variable("env", "dev")?;
assert!(!re.is_match(""));
```

Variables are read-only snapshots — safe under backtracking.

### Getting results back

Code blocks can return values. The match result carries them.

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<price>\d+)(?{native:parse_price})"#,
    ExecutionMode::Full,
)?;
re.register_native("parse_price", |ctx| {
    let price: f64 = ctx.named("price").unwrap().parse().unwrap();
    ExecResult::Numeric(price * 1.1) // add 10% tax
})?;

let result = re.find_first("item costs 50 dollars");
assert_eq!(result.unwrap().code_result, Some(rgx_core::CodeBlockValue::Numeric(55.0)));
```

### Branch identification

Know which alternative matched — no capture group tricks needed.

```rust
use rgx_core::Regex;

let re = Regex::compile("error|warning|info")?;
let m = re.find_first("this is a warning message").unwrap();
assert_eq!(m.matched_branch_number, Some(2)); // "warning" is branch 2
```

---

## Layer 2 — Predicate Callbacks

Code blocks act as zero-width checkpoints in the pattern. They run during matching and decide pass/fail.

### Native Rust callbacks

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<port>\d+)(?{native:valid_port})",
    ExecutionMode::Full,
)?;
re.register_native("valid_port", |ctx| {
    match ctx.named("port").and_then(|p| p.parse::<u16>().ok()) {
        Some(p) if p > 0 => ExecResult::Success,
        _ => ExecResult::Failure,
    }
})?;

assert!(re.is_match("port 8080"));
assert!(!re.is_match("port 0"));
assert!(!re.is_match("port 99999"));
```

### Inline Lua

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<age>\d+)(?{lua:tonumber(named.age) >= 18})"#,
    ExecutionMode::Safe,
)?;
assert!(re.is_match("age 21"));
assert!(!re.is_match("age 12"));
```

### Inline JavaScript

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<email>\S+@\S+)(?{js:named.email.endsWith(".com")})"#,
    ExecutionMode::Safe,
)?;
assert!(re.is_match("user@example.com"));
assert!(!re.is_match("user@example.org"));
```

### Inline Rhai

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<n>\d+)(?{rhai: let n = parse_int(named["n"]); n % 2 == 0})"#,
    ExecutionMode::Safe,
)?;
assert!(re.is_match("42"));
assert!(!re.is_match("43"));
```

### Available context in callbacks

Every callback (native, Lua, JS, Rhai, WASM) can access:

| Field | Description | Example |
|-------|-------------|---------|
| `pos` | Current byte position in input | `42` |
| `match_start` | Start of current match attempt | `0` |
| `match_end` | End of current match attempt | `10` |
| `match_length` | Length of current match attempt | `10` |
| `branch_number` | 1-based top-level branch (if applicable) | `2` |
| `text` | Full input text | `"hello world"` |
| `arg[0]` | Current overall match prefix | `"hello"` |
| `arg[1]`, `arg[2]`... | Numbered captures | `"world"` |
| `named.name` | Named captures | `"hello"` |
| `vars.name` | Host-provided variables | `"prod"` |

---

## Layer 3 — Match Steering

Callbacks can do more than pass/fail. They can **control how matching proceeds**.

### Accept: force a match

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};

let re = Regex::with_mode(
    r"(?<header>[A-Z]+:)(?{native:accept_header}).*",
    ExecutionMode::Full,
)?;
re.register_native("accept_header", |ctx| {
    // Accept immediately if we recognize the header
    if ctx.named("header") == Some("AUTH:") {
        ExecResult::Steer(SteerResult::Accept)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;

let m = re.find_first("AUTH: secret-token").unwrap();
// Match accepted at the header — doesn't need to consume the rest
assert_eq!(m.start, 0);
```

### Skip: jump ahead

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};

let re = Regex::with_mode(
    r"(?{native:skip_whitespace})\S+",
    ExecutionMode::Full,
)?;
re.register_native("skip_whitespace", |ctx| {
    // Count leading whitespace and skip past it
    let skip = ctx.text().bytes().skip(ctx.pos()).take_while(|b| b.is_ascii_whitespace()).count();
    ExecResult::Steer(SteerResult::Skip(skip))
})?;
```

### Abort: stop searching

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};

let re = Regex::with_mode(
    r"(?<word>\w+)(?{native:check_limit})",
    ExecutionMode::Full,
)?;
let count = std::sync::atomic::AtomicUsize::new(0);
re.register_native("check_limit", |_ctx| {
    // Stop after finding 3 matches
    if count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 3 {
        ExecResult::Steer(SteerResult::Abort)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
```

### All steering actions

| Action | What it does | Use case |
|--------|-------------|----------|
| `Continue` | Keep matching normally | Default, same as `Success` |
| `Fail` | Fail this path, backtrack | Reject based on external logic |
| `Accept` | Force immediate match | Header/prefix recognition |
| `Skip(n)` | Advance `n` bytes and continue | Skip known irrelevant content |
| `Abort` | Stop the entire search | Resource limits, early termination |

---

## Layer 4 — Structured Events

Watch what the engine is doing during matching — for debugging, profiling, or telemetry.

### Basic observer

```rust
use rgx_core::{Regex, MatchEvent};

let re = Regex::compile(r"\d+")?;
re.on_event(|event| {
    println!("{:?}", event);
})?;
re.find_first("abc 123 def");
```

This prints events like:
```
MatchAttemptStarted { position: 0 }
MatchAttemptCompleted { position: 0, matched: false }
MatchAttemptStarted { position: 1 }
...
MatchAttemptStarted { position: 4 }
MatchAttemptCompleted { position: 4, matched: true }
```

### Counting backtracks (profiling)

```rust
use rgx_core::{Regex, MatchEvent};
use std::sync::atomic::{AtomicUsize, Ordering};

let re = Regex::compile(r"a*ab")?;
let backtracks = AtomicUsize::new(0);
re.on_event(move |event| {
    if matches!(event, MatchEvent::BacktrackOccurred { .. }) {
        backtracks.fetch_add(1, Ordering::Relaxed);
    }
})?;
re.find_first("aaaaaab");
println!("Backtracks: {}", backtracks.load(Ordering::Relaxed));
```

### All event types

| Event | When it fires |
|-------|--------------|
| `MatchAttemptStarted { position }` | Before trying a match at each input position |
| `MatchAttemptCompleted { position, matched }` | After each attempt succeeds or fails |
| `BranchEntered { branch, position }` | When entering a top-level alternation branch |
| `CaptureCompleted { group, start, end }` | When a capture group finishes |
| `BacktrackOccurred { position, stack_depth }` | When the engine backtracks |
| `CodeBlockEvaluated { language, succeeded, position }` | After a code block runs |

Events are **fire-and-forget** — they never affect match behavior. Zero overhead when no observer is registered.

---

## Layer 5 — Async Callbacks

Callbacks can suspend the match, do async work (HTTP calls, database queries, file I/O), and resume.

### Basic async pattern

```rust
use rgx_core::{ExecResult, ExecutionMode, MatchOutcome, Regex};

let re = Regex::with_mode(
    r"(?<ip>\d+\.\d+\.\d+\.\d+)(?{native:check_blocklist})",
    ExecutionMode::Full,
)?;
// Don't register "check_blocklist" — it will suspend

match re.find_first_suspendable("request from 192.168.1.1") {
    MatchOutcome::Completed(result) => {
        // No async callback — completed synchronously
        println!("Match: {:?}", result);
    }
    MatchOutcome::Suspended(continuation) => {
        // Engine paused — resolve the callback externally
        println!("Need to check: {}", continuation.pending_callback_name);
        println!("IP captured: {:?}", continuation.pending_context.captures);

        // Do your async work here (HTTP call, DB query, etc.)
        let is_blocked = false; // ... your async logic ...

        let result = if is_blocked {
            ExecResult::Failure
        } else {
            ExecResult::Success
        };

        // Resume the match
        match re.resume(continuation, result) {
            MatchOutcome::Completed(result) => println!("Final: {:?}", result),
            MatchOutcome::Suspended(_) => println!("Another callback needed"),
        }
    }
}
```

### With tokio (or any async runtime)

```rust
use rgx_core::{ExecResult, ExecutionMode, ExecContextSnapshot, Regex};

let re = Regex::with_mode(
    r"(?<user>\w+)(?{native:check_permissions})",
    ExecutionMode::Full,
)?;

let result = re.find_first_async("admin", |callback_name, context| async move {
    // This closure runs in your async runtime
    match callback_name.as_str() {
        "check_permissions" => {
            // Simulate async permission check
            let user = context.captures.get(1).and_then(|c| c.map(|(s, e)| &text[s..e]));
            // let allowed = permission_service.check(user).await;
            let allowed = true;
            if allowed { ExecResult::Success } else { ExecResult::Failure }
        }
        _ => ExecResult::Success,
    }
}).await;
```

### How it works

1. You call `find_first_suspendable("text")` instead of `find_first("text")`
2. When the engine hits an unregistered native callback, it **pauses** and returns a `MatchContinuation`
3. The continuation carries everything needed to resume — it's `Send + Sync`, so you can move it across threads
4. You resolve the callback however you want (sync, async, on another thread)
5. You call `resume(continuation, result)` to continue matching
6. If another callback is hit, you get another suspension — chain as needed

**Key properties:**
- Registered callbacks still run synchronously (zero overhead)
- Only unregistered callbacks in `find_first_suspendable` trigger suspension
- `find_first` (the normal method) is completely unaffected
- Continuations are thread-safe (`Send + Sync`)

---

## Layer 6 — File Matching

Match directly against files — no need to load them into strings yourself.

### Find all matches in a file

```rust
use rgx_core::Regex;

let re = Regex::compile(r"\b\w+@\w+\.\w+\b")?;
let matches = re.match_file("contacts.txt")?;
println!("Found {} email addresses", matches.len());
```

### Line-by-line matching (with line numbers)

```rust
use rgx_core::Regex;

let re = Regex::compile(r"ERROR|FATAL")?;
for m in re.match_file_lines("app.log")? {
    println!("Line {}: {}", m.line_number, m.line.trim());
}
```

Output:
```
Line 42: 2026-04-06 ERROR: connection timeout
Line 187: 2026-04-06 FATAL: out of memory
```

### Scan with callbacks

When combined with code blocks, file scanning becomes a reactive pipeline:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<level>ERROR|WARN)\s+(?<msg>.+)(?{native:alert})",
    ExecutionMode::Full,
)?;
re.register_native("alert", |ctx| {
    if ctx.named("level") == Some("ERROR") {
        eprintln!("ALERT: {}", ctx.named("msg").unwrap_or("unknown"));
    }
    ExecResult::Success
})?;

let count = re.scan_file_lines("app.log")?;
println!("Processed {} log entries", count);
```

### File matching methods

| Method | Description |
|--------|-------------|
| `match_file(path)` | All matches in a file, returns `Vec<MatchResult>` |
| `match_file_lines(path)` | Line-by-line, returns `Vec<FileMatch>` with line numbers |
| `scan_file(path)` | All matches, triggers callbacks, returns count |
| `scan_file_lines(path)` | Line-by-line scan, triggers callbacks, returns count |

---

## Combining Layers

The real power is combining layers. Here's a log monitor that:
- Scans a log file line by line (Layer 6)
- Uses inline regex to extract fields (Layer 2)
- Checks severity with a callback (Layer 2)
- Returns structured data (Layer 1)
- Reports via events (Layer 4)

```rust
use rgx_core::{ExecResult, ExecutionMode, MatchEvent, Regex};

let re = Regex::with_mode(
    r"(?<ts>\d{4}-\d{2}-\d{2})\s+(?<level>ERROR|WARN|INFO)\s+(?<msg>.+)(?{native:process})",
    ExecutionMode::Full,
)?;

re.set_variable("min_level", "WARN")?;

re.register_native("process", |ctx| {
    let level = ctx.named("level").unwrap_or("INFO");
    let min = ctx.variable("min_level").unwrap_or_else(|| "INFO".to_string());
    let dominated = match (level, min.as_str()) {
        ("INFO", "WARN") | ("INFO", "ERROR") | ("WARN", "ERROR") => true,
        _ => false,
    };
    if dominated {
        ExecResult::Failure // skip entries below minimum level
    } else {
        ExecResult::Success
    }
})?;

re.on_event(|event| {
    if let MatchEvent::MatchAttemptCompleted { matched: true, .. } = event {
        // Track successful matches for metrics
    }
})?;

let matches = re.match_file_lines("app.log")?;
for m in &matches {
    println!("[{}] {}", m.line_number, m.line.trim());
}
```

---

## Quick Reference

| What you want | How to do it |
|---------------|-------------|
| Pass data into regex | `re.set_variable("key", "value")` |
| Get numeric result | `re.find_first_numeric_with_code(text)` |
| Get replacement | `re.replace_all_with_code(text)` |
| Know which branch matched | `match_result.matched_branch_number` |
| Run Rust code mid-match | `(?{native:name})` + `re.register_native(...)` |
| Run Lua/JS/Rhai mid-match | `(?{lua:code})`, `(?{js:code})`, `(?{rhai:code})` |
| Force accept a match | `ExecResult::Steer(SteerResult::Accept)` |
| Skip ahead | `ExecResult::Steer(SteerResult::Skip(n))` |
| Stop searching | `ExecResult::Steer(SteerResult::Abort)` |
| Watch match execution | `re.on_event(\|event\| { ... })` |
| Async callback | `re.find_first_suspendable(text)` + `re.resume(...)` |
| Match a file | `re.match_file("path")` |
| Match file by lines | `re.match_file_lines("path")` |
| Scan file with callbacks | `re.scan_file_lines("path")` |
