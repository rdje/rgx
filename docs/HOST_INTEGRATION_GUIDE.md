# HOST INTEGRATION GUIDE

How to use rgx as a programmable matching engine.

Most regex engines are one-way: you give them a pattern and text, they give you positions. rgx goes further. Your application and the regex engine form a conversation — data flows both ways, callbacks fire during matching, and the host can steer what happens next.

This guide takes you from basic matching through building a real reactive system. Every section builds on the previous one, but you can jump to any section that interests you.

---

## Getting started: your first match

Before diving into host integration, let's make sure the basics work:

```rust
use rgx_core::Regex;

let re = Regex::compile(r"\d{3}-\d{4}")?;
let m = re.find_first("Call 555-1234 for info").unwrap();
println!("Phone: {}", &"Call 555-1234 for info"[m.start..m.end]);
// Output: Phone: 555-1234
```

That's the standard regex experience. Now let's make it do more.

---

## Layer 1 — Talking to the engine

### Sending data in: host variables

Imagine you're scanning log files and you want the same regex to behave differently depending on the environment. Instead of building different patterns, pass the environment as a variable:

```rust
use rgx_core::{ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<level>ERROR|WARN|INFO)(?{js:
        // Only match if the severity meets our threshold
        var dominated = {
            "prod":  {"INFO": true,  "WARN": false, "ERROR": false},
            "dev":   {"INFO": true,  "WARN": true,  "ERROR": false},
            "test":  {"INFO": true,  "WARN": true,  "ERROR": true}
        };
        var dominated_by = dominated[vars.env] || {};
        return !(dominated_by[named.level] || false);
    })"#,
    ExecutionMode::Safe,
)?;

// In production: only ERROR and WARN match
re.set_variable("env", "prod")?;
assert!(re.is_match("ERROR: disk full"));
assert!(re.is_match("WARN: disk 90%"));
assert!(!re.is_match("INFO: request handled"));

// In dev: only ERROR matches
re.set_variable("env", "dev")?;
assert!(re.is_match("ERROR: disk full"));
assert!(!re.is_match("WARN: disk 90%"));
assert!(!re.is_match("INFO: request handled"));
```

The same compiled pattern, different behavior. Variables are snapshotted into each code-block evaluation, so they're safe even when the engine backtracks and re-evaluates.

**When to use variables:**
- Configuration that changes between runs (environment, user roles, thresholds)
- Data that the regex can't compute itself (database lookups, config values)
- Parameterizing patterns without recompiling them

### Getting data out: result values

A match doesn't have to just say "yes, it matched." Code blocks can return structured values that your application reads after the match:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, CodeBlockValue};

// Parse prices and apply tax in one pass
let re = Regex::with_mode(
    r#"\$(?<price>\d+\.?\d*)(?{native:with_tax})"#,
    ExecutionMode::Full,
)?;
re.register_native("with_tax", |ctx| {
    let price: f64 = ctx.named("price")
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    ExecResult::Numeric(price * 1.08) // 8% tax
})?;

let m = re.find_first("Total: $49.99 USD").unwrap();
assert_eq!(m.code_result, Some(CodeBlockValue::Numeric(53.9892)));
```

You can also collect numeric values across all matches:

```rust
let prices = re.find_all_numeric_with_code("Items: $10, $25, $5.50");
// prices = [10.8, 27.0, 5.94]
```

Or use replacement values for find-and-replace driven by code:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r#"(?<word>cat)(?{native:upper})"#,
    ExecutionMode::Full,
)?;
re.register_native("upper", |ctx| {
    ExecResult::Replacement(ctx.named("word").unwrap_or("").to_uppercase())
})?;

assert_eq!(re.replace_all_with_code("the cat sat on the cat"), "the CAT sat on the CAT");
```

### Knowing which branch matched

When your pattern has alternatives, you often need to know which one won. Most regex engines force you to wrap each alternative in a capturing group and check which one is non-empty. rgx tells you directly:

```rust
use rgx_core::Regex;

let re = Regex::compile(r"(?<err>ERROR .+)|(?<warn>WARN .+)|(?<info>INFO .+)")?;

let m = re.find_first("WARN disk usage high").unwrap();
assert_eq!(m.matched_branch_number, Some(2)); // Branch 2 = WARN

// Use the branch number to dispatch without inspecting captures
match m.matched_branch_number {
    Some(1) => eprintln!("ERROR detected!"),
    Some(2) => println!("Warning noted"),
    Some(3) => {}, // info, ignore
    _ => {},
}
```

This is especially powerful for tokenizers:

```rust
let lexer = Regex::compile(
    r"(?<number>\d+)|(?<ident>[a-zA-Z_]\w*)|(?<op>[+\-*/=])|(?<ws>\s+)"
)?;
for token in lexer.find_all("x = 42 + y") {
    let kind = match token.matched_branch_number {
        Some(1) => "NUMBER",
        Some(2) => "IDENT",
        Some(3) => "OP",
        Some(4) => "WS",
        _ => "UNKNOWN",
    };
    println!("{}: {:?}", kind, &"x = 42 + y"[token.start..token.end]);
}
// Output:
// IDENT: "x"
// WS: " "
// OP: "="
// WS: " "
// NUMBER: "42"
// WS: " "
// OP: "+"
// WS: " "
// IDENT: "y"
```

---

## Layer 2 — Running code during matching

This is where rgx diverges from every other regex engine. Code blocks are zero-width checkpoints that run your code *during* matching. If the code returns false, the engine backtracks as if the text didn't match — your code becomes part of the pattern.

### Why this matters

Consider validating an IP address. The regex `\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}` matches the structure, but also matches `999.999.999.999`. Traditionally you'd match first, then validate. With rgx, validation happens *inside* the match:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<a>\d{1,3})\.(?<b>\d{1,3})\.(?<c>\d{1,3})\.(?<d>\d{1,3})(?{native:valid_ip})",
    ExecutionMode::Full,
)?;
re.register_native("valid_ip", |ctx| {
    let valid = ["a", "b", "c", "d"].iter().all(|name| {
        ctx.named(name)
            .and_then(|s| s.parse::<u32>().ok())
            .map_or(false, |n| n <= 255)
    });
    if valid { ExecResult::Success } else { ExecResult::Failure }
})?;

assert!(re.is_match("192.168.1.1"));
assert!(!re.is_match("999.999.999.999"));
assert!(!re.is_match("256.1.1.1"));
```

The invalid IPs don't just fail a post-match check — they're **not matches at all**. If the IP is part of a larger pattern with alternatives, the engine backtracks and tries the next alternative.

### Four languages, one interface

rgx supports code blocks in multiple languages. Pick the one that fits your project:

**Native Rust** — fastest, full access to your application's types:

```rust
re.register_native("check", |ctx| {
    // Full Rust — call any function, access any data
    if my_database.is_blocked(ctx.named("ip").unwrap()) {
        ExecResult::Failure
    } else {
        ExecResult::Success
    }
})?;
```

**Lua** — lightweight, sandboxed, great for user-provided rules:

```rust
let re = Regex::with_mode(
    r#"(?<n>\d+)(?{lua:
        local n = tonumber(named.n)
        if n > 100 then
            rgx.emit_replacement("HIGH")
            return true
        end
        return n > 10
    })"#,
    ExecutionMode::Safe,
)?;
```

**JavaScript** — familiar syntax, sandboxed via QuickJS:

```rust
let re = Regex::with_mode(
    r#"(?<email>\S+@\S+)(?{js:
        // Validate email domain
        var parts = named.email.split("@");
        var domain = parts[parts.length - 1];
        return domain.endsWith(".com") || domain.endsWith(".org");
    })"#,
    ExecutionMode::Safe,
)?;
```

**Rhai** — pure-Rust embedded scripting:

```rust
let re = Regex::with_mode(
    r#"(?<word>\w+)(?{rhai:
        let w = named["word"];
        w.len() >= 3 && w.len() <= 10
    })"#,
    ExecutionMode::Safe,
)?;
```

### What callbacks can see

Every callback receives a rich execution context. Here's a native callback that uses all of it:

```rust
re.register_native("inspect", |ctx| {
    // Current match state
    println!("Position: {}", ctx.pos());
    println!("Match attempt: {}..{}", ctx.match_start(), ctx.match_end());

    // Captured groups
    println!("Group 1: {:?}", ctx.group(1));
    println!("Named 'user': {:?}", ctx.named("user"));
    println!("Current match text: {:?}", ctx.current_match());

    // Host variables
    println!("Environment: {:?}", ctx.variable("env"));

    // Branch info (if inside alternation)
    println!("Branch: {:?}", ctx.matched_branch_number());

    // Full input
    println!("Full text: {:?}", ctx.text());

    ExecResult::Success
})?;
```

### Execution modes

rgx has three safety levels:

| Mode | What's allowed | Use case |
|------|---------------|----------|
| `Pure` | No code blocks at all | Untrusted patterns |
| `Safe` | Sandboxed languages only (Lua, JS, Rhai, WASM) | User-provided patterns with controlled code |
| `Full` | Everything including native Rust callbacks | Your own patterns with full application access |

```rust
// Untrusted pattern — code blocks are rejected at compile time
let re = Regex::compile(r"(?{lua:os.execute('rm -rf /')})")?; // This compiles in Pure mode...
// ...but the Lua code never runs because Pure mode rejects all code blocks.

// Safe mode — sandboxed, no filesystem/network/OS access
let re = Regex::with_mode(r"(?{lua:return true})", ExecutionMode::Safe)?; // OK

// Full mode — native callbacks allowed
let re = Regex::with_mode(r"(?{native:my_fn})", ExecutionMode::Full)?; // OK
```

### Backtracking-safe

Code blocks participate in backtracking. If the engine backtracks past a code block, it may run the block again on a different path. This means:

- Code blocks should be **side-effect-free predicates** when possible
- If you need side effects (logging, metrics), understand that they may fire multiple times for one match
- Variables and captures are always consistent — the engine restores them on backtrack

---

## Layer 3 — Steering the match

Sometimes pass/fail isn't enough. Your callback knows something the regex can't express — like "skip the next 100 bytes" or "we've found enough, stop searching." Match steering lets your code control what the engine does next.

### "We're done here" — Accept

You're parsing HTTP headers and want to match as soon as you see the right header name, without consuming the value:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};

let re = Regex::with_mode(
    r"(?<header>[A-Za-z-]+):\s*(?{native:check_header}).+",
    ExecutionMode::Full,
)?;
re.register_native("check_header", |ctx| {
    match ctx.named("header").unwrap_or("") {
        "Authorization" | "X-API-Key" => {
            // Found a security header — accept immediately
            ExecResult::Steer(SteerResult::Accept)
        }
        _ => ExecResult::Steer(SteerResult::Continue),
    }
})?;

// Matches at "Authorization" without needing to consume the token value
let m = re.find_first("Authorization: Bearer secret-token-here").unwrap();
```

### "Skip ahead" — Skip

You're scanning a binary log format where you know the next N bytes are a binary blob. Tell the engine to jump past them:

```rust
re.register_native("skip_binary_payload", |ctx| {
    // Read the payload length from the matched header
    let len: usize = ctx.named("payload_len")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    ExecResult::Steer(SteerResult::Skip(len))
})?;
```

### "Stop searching" — Abort

You have a resource budget. After finding 10 matches, stop:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};

let re = Regex::with_mode(
    r"(?<match>\w+)(?{native:budget})",
    ExecutionMode::Full,
)?;
let found = AtomicUsize::new(0);
re.register_native("budget", move |_ctx| {
    let n = found.fetch_add(1, Ordering::Relaxed);
    if n >= 10 {
        ExecResult::Steer(SteerResult::Abort) // stop the entire search
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;

// find_all will stop after 10 matches instead of scanning the whole file
let matches = re.find_all(&large_text);
assert!(matches.len() <= 10);
```

### "Not this one" — Fail with backtracking

Your callback rejects a match, and the engine tries the next alternative:

```rust
let re = Regex::with_mode(
    r"(?<word>cat|dog|bird)(?{native:filter})",
    ExecutionMode::Full,
)?;
re.register_native("filter", |ctx| {
    if ctx.named("word") == Some("dog") {
        ExecResult::Steer(SteerResult::Fail) // reject "dog", try next
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;

// In "the dog and cat", skips "dog" and finds "cat"
let m = re.find_first("the dog and cat").unwrap();
assert_eq!(&"the dog and cat"[m.start..m.end], "cat");
```

---

## Layer 4 — Watching the engine work

The event system lets you observe what the engine is doing without affecting it. Think of it as a one-way mirror into the matching process.

### Debugging a pattern

Not sure why your pattern isn't matching? Watch the engine try:

```rust
use rgx_core::{Regex, MatchEvent};

let re = Regex::compile(r"ab+c")?;
re.on_event(|event| {
    match event {
        MatchEvent::MatchAttemptStarted { position } => {
            println!("  Trying at position {position}...");
        }
        MatchEvent::MatchAttemptCompleted { position, matched } => {
            println!("  Position {position}: {}", if *matched { "MATCH" } else { "no" });
        }
        MatchEvent::BacktrackOccurred { position, stack_depth } => {
            println!("  Backtrack at {position} (stack depth: {stack_depth})");
        }
        _ => {}
    }
})?;

re.find_first("xabbbc");
```

Output:
```
  Trying at position 0...
  Position 0: no
  Trying at position 1...
  Backtrack at 5 (stack depth: 0)
  Position 1: MATCH
```

Now you can see that the engine tried position 0 (failed because 'x' isn't 'a'), then tried position 1, matched 'a', greedily consumed 'bbb', hit 'c' — success.

### Profiling: finding expensive patterns

Count backtracks to identify patterns that cause excessive work:

```rust
use rgx_core::{Regex, MatchEvent};
use std::sync::atomic::{AtomicUsize, Ordering};

fn profile_pattern(pattern: &str, text: &str) {
    let re = Regex::compile(pattern).unwrap();
    let backtracks = AtomicUsize::new(0);
    let attempts = AtomicUsize::new(0);

    re.on_event(move |event| {
        match event {
            MatchEvent::BacktrackOccurred { .. } => {
                backtracks.fetch_add(1, Ordering::Relaxed);
            }
            MatchEvent::MatchAttemptStarted { .. } => {
                attempts.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }).unwrap();

    re.find_first(text);
    println!("Pattern: {pattern}");
    println!("  Attempts: {}", attempts.load(Ordering::Relaxed));
    println!("  Backtracks: {}", backtracks.load(Ordering::Relaxed));
}

profile_pattern(r"a*ab", "aaaaab");
// Pattern: a*ab
//   Attempts: 1
//   Backtracks: 5   <-- the greedy a* has to give back characters one by one
```

### Coverage: which branches were exercised

```rust
use rgx_core::{Regex, MatchEvent};
use std::sync::{Arc, Mutex};
use std::collections::HashSet;

let re = Regex::compile(r"error|warning|info|debug")?;
let branches = Arc::new(Mutex::new(HashSet::new()));
let branches_clone = branches.clone();

re.on_event(move |event| {
    if let MatchEvent::BranchEntered { branch, .. } = event {
        branches_clone.lock().unwrap().insert(*branch);
    }
})?;

re.find_all("error occurred, then info logged, another error");
let covered = branches.lock().unwrap();
println!("Branches exercised: {:?}", covered);
// {1, 3} — branches 1 (error) and 3 (info) were matched
// Branches 2 (warning) and 4 (debug) were never reached
```

### Zero overhead

When no observer is registered, the event system adds exactly **zero** overhead. The check is a single `Option::is_some()` branch that the CPU's branch predictor handles perfectly. You never pay for what you don't use.

---

## Layer 5 — Async callbacks

Sometimes your callback needs to talk to the outside world — check a database, call an API, read from a cache. With async callbacks, the match **pauses**, you do your I/O, and the match **resumes** exactly where it left off.

### The problem

You want to validate email addresses against a live blocklist:

```rust
// Pattern: match email, then check if it's blocked
let re = Regex::with_mode(
    r"(?<email>[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})(?{native:check_blocked})",
    ExecutionMode::Full,
)?;
```

But `check_blocked` needs to query a database. You can't do async I/O inside a synchronous callback.

### The solution: suspendable matching

Don't register the callback. Instead, use `find_first_suspendable`:

```rust
use rgx_core::{ExecResult, ExecutionMode, MatchOutcome, Regex};

let re = Regex::with_mode(
    r"(?<email>[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+)(?{native:check_blocked})",
    ExecutionMode::Full,
)?;
// Note: we deliberately DON'T register "check_blocked"

let text = "Contact: alice@example.com for details";

match re.find_first_suspendable(text) {
    MatchOutcome::Completed(result) => {
        // No async callbacks were hit — this won't happen here
        // because "check_blocked" isn't registered
        println!("Completed: {:?}", result);
    }
    MatchOutcome::Suspended(continuation) => {
        // The engine paused! It found "alice@example.com" and now
        // wants us to resolve "check_blocked".
        println!("Callback needed: {}", continuation.pending_callback_name);
        // "check_blocked"

        // The continuation has a snapshot of what the engine captured:
        println!("Captures: {:?}", continuation.pending_context.captures);

        // Now we can do our async work. Maybe we query a database:
        let is_blocked = false; // async_db.check_blocklist("alice@example.com").await;

        let callback_result = if is_blocked {
            ExecResult::Failure  // reject the match — the engine will backtrack
        } else {
            ExecResult::Success  // accept — the match continues
        };

        // Resume the match with our answer
        match re.resume(continuation, callback_result) {
            MatchOutcome::Completed(Some(m)) => {
                println!("Match: {}..{}", m.start, m.end);
            }
            MatchOutcome::Completed(None) => {
                println!("No match after callback resolution");
            }
            MatchOutcome::Suspended(another) => {
                println!("Another callback needed: {}", another.pending_callback_name);
                // Handle the chain...
            }
        }
    }
}
```

### With an async runtime

The `find_first_async` method wraps the suspend/resume loop for you:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<user>\w+)@(?<domain>[\w.]+)(?{native:check_domain})",
    ExecutionMode::Full,
)?;

// In your async context (tokio, async-std, smol, etc.):
let result = re.find_first_async("alice@example.com", |callback_name, context| async move {
    match callback_name.as_str() {
        "check_domain" => {
            // Your async logic here:
            // let domain = extract_domain_from_context(&context);
            // let valid = dns_resolver.check(domain).await;
            ExecResult::Success
        }
        _ => ExecResult::Success,
    }
}).await;
```

### Multiple async callbacks in one pattern

If your pattern has several async checkpoints, each one suspends independently:

```rust
let re = Regex::with_mode(
    r"(?<user>\w+)(?{native:check_user})@(?<domain>[\w.]+)(?{native:check_domain})",
    ExecutionMode::Full,
)?;

// First suspension: "check_user"
// You resolve it, call resume
// Second suspension: "check_domain"
// You resolve it, call resume
// Final: Completed(Some(match)) or Completed(None)
```

### Thread safety

The `MatchContinuation` is `Send + Sync`. You can:
- Move it to another thread
- Send it across a channel
- Store it in a task queue
- Serialize it (if you implement serde for your use case)

The engine doesn't hold any locks while a match is suspended. Multiple matches on different threads can be suspended and resumed independently.

### When to use async vs sync callbacks

| Scenario | Approach |
|----------|----------|
| Pure computation (validate, transform) | Register a normal native callback |
| Database query | Async: `find_first_suspendable` + resume |
| HTTP API call | Async: `find_first_async` with your runtime |
| File I/O | Async or sync depending on size |
| In-memory cache lookup | Register a normal native callback (fast enough) |

---

## Layer 6 — Working with files

Match directly against files without loading them into strings yourself.

### Finding matches in a file

```rust
use rgx_core::Regex;

let re = Regex::compile(r"\b[A-Z]{2,}\b")?; // all-caps words

let matches = re.match_file("document.txt")?;
println!("Found {} acronyms", matches.len());

for m in &matches {
    // m.start and m.end are byte positions in the file
    println!("  at position {}..{}", m.start, m.end);
}
```

### Line-by-line: the log-scanning pattern

For log files, you usually want to know *which line* a match is on:

```rust
use rgx_core::Regex;

let re = Regex::compile(r"ERROR|FATAL")?;

for m in re.match_file_lines("application.log")? {
    println!("Line {}: {}", m.line_number, m.line.trim());
    // Line 42: 2026-04-06 10:15:33 ERROR database connection timeout
    // Line 187: 2026-04-06 10:22:01 FATAL out of memory
}
```

### Reactive scanning: callbacks fire on each match

This is where file scanning meets host integration. Register a callback, scan a file, and your code runs on every match:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<severity>ERROR|WARN)\s+(?<message>.+)(?{native:handle})",
    ExecutionMode::Full,
)?;

re.register_native("handle", |ctx| {
    let severity = ctx.named("severity").unwrap_or("UNKNOWN");
    let message = ctx.named("message").unwrap_or("");

    match severity {
        "ERROR" => {
            eprintln!("ALERT: {message}");
            // send_to_pagerduty(message);
        }
        "WARN" => {
            println!("Warning: {message}");
            // log_to_metrics(message);
        }
        _ => {}
    }

    ExecResult::Success
})?;

// One line scans the file and triggers all callbacks
let count = re.scan_file_lines("app.log")?;
println!("Processed {count} entries");
```

### Available file methods

| Method | What it does | Returns |
|--------|-------------|---------|
| `match_file("path")` | Find all matches in a file | `Vec<MatchResult>` |
| `match_file_lines("path")` | Find matches with line numbers | `Vec<FileMatch>` |
| `scan_file("path")` | Match + trigger callbacks | match count |
| `scan_file_lines("path")` | Line-by-line + trigger callbacks | match count |

---

## Putting it all together: a complete example

Here's a log monitoring system built entirely with rgx. It:
- Reads a log file line by line (Layer 6)
- Extracts timestamp, level, and message (Layer 2)
- Filters by minimum severity via host variable (Layer 1)
- Alerts on errors via callback (Layer 2)
- Tracks which severity levels were seen (Layer 4)
- Returns the branch number for each match (Layer 1)

```rust
use rgx_core::{
    ExecResult, ExecutionMode, MatchEvent, Regex,
};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

fn main() -> rgx_core::Result<()> {
    // The pattern: timestamp, severity, message
    let re = Regex::with_mode(
        r"(?<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\s+(?<level>ERROR|WARN|INFO|DEBUG)\s+(?<msg>.+)(?{native:filter})",
        ExecutionMode::Full,
    )?;

    // Configure minimum severity via variable
    re.set_variable("min_level", "WARN")?;

    // Register the severity filter
    re.register_native("filter", |ctx| {
        let level = ctx.named("level").unwrap_or("DEBUG");
        let min = ctx.variable("min_level").unwrap_or_else(|| "DEBUG".to_string());

        let severity = |l: &str| match l {
            "ERROR" => 3, "WARN" => 2, "INFO" => 1, _ => 0,
        };

        if severity(level) >= severity(&min) {
            ExecResult::Success
        } else {
            ExecResult::Failure // skip entries below threshold
        }
    })?;

    // Track statistics via events
    let stats = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
    let stats_clone = stats.clone();
    re.on_event(move |event| {
        if let MatchEvent::MatchAttemptCompleted { matched: true, .. } = event {
            // We can't easily get the level here, but we can count successful matches
            *stats_clone.lock().unwrap().entry("total".to_string()).or_insert(0) += 1;
        }
    })?;

    // Scan the file
    let matches = re.match_file_lines("app.log")?;

    for m in &matches {
        println!("[Line {}] {}", m.line_number, m.line.trim());
    }

    let total = stats.lock().unwrap().get("total").copied().unwrap_or(0);
    println!("\n{} entries matched (severity >= WARN)", total);

    Ok(())
}
```

---

## Tips and best practices

### Keep code blocks simple
Code blocks should be short predicates, not business logic. If your callback is more than 10 lines, consider moving the logic into a registered native function.

### Use native callbacks for performance-critical paths
Lua/JS/Rhai create a fresh runtime per evaluation. Native Rust callbacks have near-zero overhead.

### Understand backtracking
Code blocks may execute multiple times for one match. Design them as pure predicates when possible. If you need side effects (logging, counters), use `AtomicUsize` or similar thread-safe primitives.

### Choose the right execution mode
- Use `Pure` for untrusted patterns (user input)
- Use `Safe` for patterns with sandboxed code (user-provided rules)
- Use `Full` only when you control the patterns yourself

### Variables are snapshots
Variables set via `set_variable` are captured at the start of each code-block evaluation. Changing a variable between `find_first` calls works as expected. Changing it during a match (from a callback) doesn't affect other callbacks in the same match.

### Events are fire-and-forget
Event observers can never block, fail, or influence matching. They run synchronously in the matching thread but must be fast. For heavy processing, queue events and process them asynchronously.

---

## What's next

- **`tail_file`**: Watch files for new content with live matching (planned)
- **Inline-language steering**: `rgx.steer_skip(n)` from Lua/JS/Rhai (planned)
- **CLI integration**: `rgx-cli --file path --line-mode` (planned)
