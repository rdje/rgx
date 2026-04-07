# Chapter 5: Async Callbacks

So far, every callback in this guide has been synchronous. Your function receives context, does some computation, and returns a result. The engine waits, gets the answer, and moves on. This works beautifully for pure computation -- validating a number, checking a string format, looking up a value in a HashMap.

But what if your callback needs to talk to the outside world? What if the answer to "does this match?" requires a database query, an HTTP request, or reading from a file that's too large to preload?

## The problem

Imagine you're scanning log files for IP addresses and want to check each one against a threat intelligence API:

```rust
// This is what you WANT to write... but can't with sync callbacks
re.register_native("check_threat_intel", |ctx| {
    let ip = ctx.named("ip").unwrap_or("");
    let response = http_client.get(&format!("https://api.threats.example/check/{}", ip)).await; // ERROR: can't await here
    if response.is_threat() {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;
```

This doesn't work. Native callbacks are synchronous closures -- they can't `await`. And even if they could, blocking the regex engine on a network call for every potential match would be unacceptably slow.

### How traditional engines force you to work around this

Without async callbacks, you'd have to split the work into two phases:

```rust
// Phase 1: Find all IP addresses (no validation)
let candidates = re.find_all(text);

// Phase 2: Validate each one asynchronously
let mut confirmed_threats = Vec::new();
for m in candidates {
    let ip = &text[m.start..m.end];
    if check_threat_api(ip).await {
        confirmed_threats.push(m);
    }
}
```

Two passes. The first pass returns false positives that the second pass filters out. If most IPs are benign, you've done a lot of wasted extraction work. And you've lost the ability to use the regex engine's backtracking to try alternative interpretations when a callback fails.

## The continuation-passing concept

rgx solves this with **continuations**. When the engine encounters a callback it can't resolve immediately, it takes a snapshot of its entire state -- like placing a bookmark in a book -- and hands it back to you. You go do your async work. When you have the answer, you hand the bookmark back to the engine, and it picks up exactly where it left off.

Think of it like ordering food at a restaurant with a number. You place your order (the engine encounters a callback). The kitchen gives you a number (a continuation). You sit down and wait (do your async work). When your food is ready, you bring the number back to the counter (resume the match). The kitchen doesn't hold up everyone else's orders while yours is being prepared.

### The key insight

The engine doesn't need to know *how* you resolve the callback. It doesn't care whether you call an HTTP API, query a database, read a file, or ask a human. It gives you a name ("which callback?") and context ("what did it match?"), and you give back a result. The engine's job is matching. Your job is resolution.

## Step-by-step walkthrough

### Step 1: Create a pattern with an unregistered native callback

```rust
use rgx_core::{ExecResult, ExecutionMode, MatchOutcome, Regex};

let re = Regex::with_mode(
    r"\b(?<ip>\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b(?{native:check_threat})",
    ExecutionMode::Full,
)?;

// Note: we do NOT register "check_threat" as a native callback.
// This tells the engine to SUSPEND when it reaches this callback.
```

When the engine hits `(?{native:check_threat})` and finds no registered callback by that name, it suspends instead of failing.

### Step 2: Start a suspendable match

```rust
let text = "Alert from 192.168.1.100 at 10.0.0.5";
let mut outcome = re.find_first_suspendable(text);
```

`find_first_suspendable` returns a `MatchOutcome` instead of `Option<MatchResult>`:

```rust
pub enum MatchOutcome {
    Completed(Option<MatchResult>),
    Suspended(Box<MatchContinuation>),
}
```

### Step 3: Handle the outcome

```rust
loop {
    match outcome {
        MatchOutcome::Completed(result) => {
            // Done! result is Option<MatchResult>, same as find_first
            match result {
                Some(m) => println!("Threat found at {}..{}", m.start, m.end),
                None => println!("No threats found"),
            }
            break;
        }
        MatchOutcome::Suspended(continuation) => {
            // The engine needs us to resolve a callback
            let callback_name = &continuation.pending_callback_name;
            let ctx = &continuation.pending_context;
            println!("Engine needs: {} (position: {})", callback_name, ctx.position);

            // Do your async work here (simplified as sync for clarity)
            let ip = "192.168.1.100"; // You'd extract this from ctx
            let is_threat = check_threat_database(ip);

            // Resume with the result
            let result = if is_threat {
                ExecResult::Success
            } else {
                ExecResult::Failure
            };
            outcome = re.resume(*continuation, result);
        }
    }
}
```

### What the continuation contains

A `MatchContinuation` carries:

| Field | Type | Description |
|-------|------|-------------|
| `pending_callback_name` | `String` | The name of the callback that needs resolution |
| `pending_context` | `ExecContextSnapshot` | A snapshot of the match state at suspension |

The `ExecContextSnapshot` gives you:

| Field | Type | Description |
|-------|------|-------------|
| `position` | `usize` | Byte offset at suspension point |
| `match_start` | `usize` | Where the current match attempt began |
| `captures` | `Vec<Option<usize>>` | Raw capture group byte-offset pairs |
| `variables` | `HashMap<String, String>` | Host variable snapshot |

## Using find_first_async with a resolver

The manual loop above works, but rgx provides a convenience method that drives the loop for you:

```rust
let result = re.find_first_async(text, |name, ctx| async move {
    match name.as_str() {
        "check_threat" => {
            // Extract the IP from the capture positions
            let is_threat = threat_api::check(&ctx).await;
            if is_threat {
                ExecResult::Success
            } else {
                ExecResult::Failure
            }
        }
        _ => ExecResult::Failure,
    }
}).await;
```

`find_first_async` takes a resolver function that maps `(callback_name, context_snapshot)` to a future that resolves to an `ExecResult`. It drives the suspend/resume loop internally.

This works with any async runtime -- tokio, async-std, smol, or anything else that supports `.await`.

### Example with tokio

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let re = Regex::with_mode(
        r"\b(?<user>[a-zA-Z_]\w*)(?{native:check_active})",
        ExecutionMode::Full,
    )?;

    let text = "Users: alice, bob, charlie";

    let result = re.find_first_async(text, |name, ctx| async move {
        match name.as_str() {
            "check_active" => {
                // Simulate an async database lookup
                let username = "alice"; // Extract from ctx.captures
                let is_active = db_check_user(username).await;
                if is_active {
                    ExecResult::Success
                } else {
                    ExecResult::Failure
                }
            }
            _ => ExecResult::Failure,
        }
    }).await;

    match result {
        Some(m) => println!("First active user at {}..{}", m.start, m.end),
        None => println!("No active users found"),
    }

    Ok(())
}
```

## Multiple suspensions in one match

A pattern can have more than one unregistered callback. The engine suspends at each one, and your resolver handles them independently.

```rust
let re = Regex::with_mode(
    r"(?<user>\w+)@(?<domain>[\w.]+)(?{native:check_user})(?{native:check_domain})",
    ExecutionMode::Full,
)?;

// Neither check_user nor check_domain is registered.
// The engine will suspend twice: once for each.

let result = re.find_first_async(text, |name, ctx| async move {
    match name.as_str() {
        "check_user" => {
            let valid = user_service::exists(&ctx).await;
            if valid { ExecResult::Success } else { ExecResult::Failure }
        }
        "check_domain" => {
            let valid = domain_service::is_allowed(&ctx).await;
            if valid { ExecResult::Success } else { ExecResult::Failure }
        }
        _ => ExecResult::Failure,
    }
}).await;
```

The execution flow:

1. Engine matches `\w+` and `@` and `[\w.]+`
2. Engine hits `check_user` -- suspends, returns continuation
3. Your resolver calls the user service, returns `Success`
4. Engine resumes, hits `check_domain` -- suspends again
5. Your resolver calls the domain service, returns `Success`
6. Engine resumes, match completes

If `check_user` returns `Failure`, the engine backtracks. It might try a different split of the input and hit `check_user` again with different captures. The suspend/resume cycle repeats.

## Thread safety

Continuations are `Send + Sync`. This means you can:

- Move a continuation to another thread for resolution
- Store continuations in a queue for batch processing
- Resolve multiple continuations concurrently

```rust
use std::sync::mpsc;
use std::thread;

// Create a channel for passing continuations to a worker thread
let (tx, rx) = mpsc::channel();

// Worker thread that resolves callbacks
thread::spawn(move || {
    while let Ok((continuation, response_tx)) = rx.recv() {
        let cont: MatchContinuation = continuation;
        let result = expensive_check(&cont.pending_callback_name);
        response_tx.send(result).ok();
    }
});
```

This enables architectures where the regex engine runs on one thread and callback resolution happens on specialized worker threads or in an async runtime.

## When to use async vs sync callbacks

Here's a decision guide:

| Situation | Use | Why |
|-----------|-----|-----|
| Pure computation (math, string ops) | Sync (native) | Fastest, simplest |
| In-memory lookup (HashMap, Vec) | Sync (native) | Data is local, no I/O needed |
| Inline scripting (Lua/JS/Rhai) | Sync (inline) | Embedded interpreters are sync |
| Database query | Async | Network I/O |
| HTTP API call | Async | Network I/O |
| File system read (large files) | Async | Disk I/O |
| External process | Async | Process I/O |
| Human-in-the-loop approval | Async | Unbounded wait |

**The rule of thumb:** if your callback completes in microseconds, use sync. If it might take milliseconds or more, use async.

### Mixing sync and async

You can have both registered (sync) and unregistered (async) callbacks in the same pattern:

```rust
let re = Regex::with_mode(
    r"(?<amount>\d+)(?{native:quick_check})(?{native:slow_verify})",
    ExecutionMode::Full,
)?;

// Register the fast check synchronously
re.register_native("quick_check", |ctx| {
    let amount: i64 = ctx.named("amount").unwrap_or("0").parse().unwrap_or(0);
    if amount > 0 {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

// Leave slow_verify unregistered for async resolution
let result = re.find_first_async(text, |name, ctx| async move {
    match name.as_str() {
        "slow_verify" => {
            let ok = fraud_check_api(&ctx).await;
            if ok { ExecResult::Success } else { ExecResult::Failure }
        }
        _ => ExecResult::Failure,
    }
}).await;
```

The engine runs `quick_check` synchronously (it's registered, so it runs inline). Only when it reaches `slow_verify` does it suspend. This is optimal -- cheap checks happen inline, expensive checks happen async.

## How async interacts with backtracking

When the engine backtracks past a suspension point and tries again, it will suspend again. Your resolver may be called multiple times for the same callback name with different contexts. This is identical to how sync callbacks work (see [Chapter 2](02-predicate-callbacks.md), "Backtracking behavior").

The key implication: your async resolver should be safe to call multiple times. Don't assume a single call per match attempt. If your resolver has side effects (like writing to a database), guard against duplicate calls.

## Summary

| What you want | How |
|---------------|-----|
| Suspend on a callback | Don't register it (leave it unregistered) |
| Start a suspendable match | `re.find_first_suspendable(text)` |
| Resume after resolution | `re.resume(continuation, result)` |
| Auto-drive the loop | `re.find_first_async(text, resolver).await` |
| Mix sync and async callbacks | Register sync ones, leave async ones unregistered |
| Access context during suspension | Read `continuation.pending_context` |
| Get the callback name | Read `continuation.pending_callback_name` |

## Next

[Chapter 6: Working with Files >>>](06-file-matching.md)
