# Chapter 5: Async Callbacks

Sometimes your validation needs the internet.

You've built a pattern that finds IP addresses. You've written a callback that validates their format. But now you need to check each IP against a threat intelligence API. Or you've matched a username and need to verify it exists in your database. Or you've found a URL and want to confirm it resolves before accepting the match.

These tasks require I/O -- network calls, database queries, file reads. They take milliseconds, not microseconds. And they can't run inside a synchronous callback.

This chapter shows you how rgx handles this: the engine pauses, you do your I/O, the engine resumes. No threads blocked, no awkward workarounds. It's simpler than it sounds.

## Regular matching is completely unaffected

Before we dive into async, let's be clear: **if your callbacks don't need I/O, nothing changes.** Sync callbacks work exactly as they did in Chapters 2 and 3. You don't need to import anything async. You don't need a runtime. The engine behaves identically.

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

// This is plain old sync matching. No async involved.
let re = Regex::with_mode(
    r"(?<num>\d+)(?{native:check})",
    ExecutionMode::Full,
)?;

re.register_native("check", |ctx| {
    let n: i64 = ctx.named("num").unwrap_or("0").parse().unwrap_or(0);
    if n > 100 { ExecResult::Success } else { ExecResult::Failure }
})?;

assert!(re.is_match("value: 200"));
assert!(!re.is_match("value: 50"));
// No async runtime needed. Nothing suspends. Business as usual.
```

Async is opt-in. You only use it when you need it.

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

## How it works: the suspend-resolve-resume flow

rgx solves this with **continuations**. Here's the flow, step by step:

```
                          Your Code                    rgx Engine
                          ─────────                    ──────────
  1. Start match ───────────────────────────────────> Engine begins matching
                                                      |
  2.                                                  Pattern matches text...
                                                      |
  3.                                                  Engine hits unregistered callback
                                                      |
  4. <── Engine SUSPENDS ─── returns continuation ─── Takes a snapshot of its state
     |
  5. You read the continuation:
     - Which callback? (name)
     - What was matched? (context)
     |
  6. You do your I/O:
     - Call an API
     - Query a database
     - Read a file
     - Ask a human
     |
  7. You decide: Success or Failure
     |
  8. RESUME ─── pass result back to engine ────────> Engine restores its snapshot
                                                      |
  9.                                                  Continues matching from
                                                      exactly where it left off
                                                      |
  10.                                                 More callbacks? → go to step 3
                                                      Done? → return final result
```

The key insight: the engine doesn't need to know *how* you resolve the callback. It doesn't care whether you call an HTTP API, query a database, read a file, or ask a human. It gives you a name ("which callback?") and context ("what did it match?"), and you give back a result. The engine's job is matching. Your job is resolution.

Think of it like ordering food at a restaurant with a number. You place your order (the engine encounters a callback). The kitchen gives you a number (a continuation). You sit down and wait (do your async work). When your food is ready, you bring the number back to the counter (resume the match). The kitchen doesn't hold up everyone else's orders while yours is being prepared.

## Step-by-step walkthrough

Let's build this from scratch, one step at a time.

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

This is the magic trigger: when the engine hits `(?{native:check_threat})` and finds no registered callback by that name, it **suspends** instead of failing. The name is how you'll identify which callback needs resolution.

### Step 2: Start a suspendable match

```rust
let text = "Alert from 192.168.1.100 at 10.0.0.5";
let mut outcome = re.find_first_suspendable(text);
```

Notice we're using `find_first_suspendable` instead of `find_first`. This is important -- the regular `find_first` can't handle suspensions. It returns `Option<MatchResult>`, which has no room for a continuation.

`find_first_suspendable` returns a `MatchOutcome`:

```rust
pub enum MatchOutcome {
    Completed(Option<MatchResult>),  // Match finished (with or without a result)
    Suspended(Box<MatchContinuation>),  // Engine paused, needs your input
}
```

### Step 3: Handle the outcome in a loop

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

            // ---- This is where you do your async work ----
            let ip = "192.168.1.100"; // You'd extract this from ctx
            let is_threat = check_threat_database(ip);
            // ---- End of async work ----

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

That's it. Three steps: start suspendable, check outcome, resume when ready. The loop handles the case where a pattern has multiple unregistered callbacks -- each one suspends, you resolve it, and the engine continues.

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

## Sync or async? A decision guide

Here's a visual decision flow:

```
Does your callback need network, disk, or external process I/O?
  No  --> Use a sync callback (register it normally)
  Yes |
      v
Is the I/O fast and predictable (< 1ms, local disk)?
  Yes --> Sync might still work (profile to be sure)
  No  |
      v
Use async: leave the callback unregistered and use
find_first_suspendable or find_first_async
```

And here's the reference table:

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

## Common mistakes

Here are the pitfalls that catch people. Read this section before writing your first async callback -- it'll save you time.

### Mistake 1: Using find_first instead of find_first_suspendable

```rust
// WRONG: find_first can't handle suspensions
let result = re.find_first(text);
// If the pattern has an unregistered callback, the engine can't suspend.
// Use find_first_suspendable or find_first_async instead.

// CORRECT: use the suspendable variant
let outcome = re.find_first_suspendable(text);
```

If you accidentally use `find_first` with a pattern that has unregistered callbacks, the engine treats the unregistered callback as an immediate failure -- it won't suspend, and you'll get unexpected "no match" results.

### Mistake 2: Forgetting to resume

```rust
// WRONG: you got a continuation but never resumed
match outcome {
    MatchOutcome::Suspended(continuation) => {
        let _result = do_async_work().await;
        // Oops -- forgot to call re.resume()!
        // The match is abandoned. No result is produced.
    }
    // ...
}

// CORRECT: always resume
match outcome {
    MatchOutcome::Suspended(continuation) => {
        let check = do_async_work().await;
        let result = if check { ExecResult::Success } else { ExecResult::Failure };
        outcome = re.resume(*continuation, result);  // don't forget this!
    }
    // ...
}
```

If you drop a continuation without resuming, the match simply stops. There's no panic or error -- the engine state is garbage-collected. But you'll never get a result.

### Mistake 3: Resuming with the wrong result type

```rust
// The callback expects Success/Failure, but you could also use Steer results.
// Make sure the result makes sense for your use case.

// Simple case: Success or Failure
let result = if is_valid { ExecResult::Success } else { ExecResult::Failure };

// You can also use steering actions in async callbacks!
let result = if is_critical {
    ExecResult::Steer(SteerResult::Accept)  // commit immediately
} else if timed_out {
    ExecResult::Steer(SteerResult::Abort)   // stop the search
} else {
    ExecResult::Success
};
outcome = re.resume(*continuation, result);
```

### Mistake 4: Assuming the resolver is called exactly once

```rust
// WRONG assumption: resolver runs once per match
let result = re.find_first_async(text, |name, ctx| async move {
    // This might be called MULTIPLE TIMES for the same callback name!
    // If the engine backtracks and retries, your resolver runs again
    // with different ctx values.
    write_to_database(&ctx).await;  // Careful: could insert duplicates
    ExecResult::Success
}).await;

// CORRECT: guard against duplicates if your resolver has side effects
let result = re.find_first_async(text, |name, ctx| async move {
    // Use upsert or check-before-write if idempotency matters
    upsert_to_database(&ctx).await;
    ExecResult::Success
}).await;
```

When the engine backtracks past a suspension point and tries again, it will suspend again. Your resolver may be called multiple times for the same callback name with different contexts.

## Complete example: URL validation during matching

Here's a complete, realistic example that ties everything together. We match URLs in a document and verify each one resolves (returns HTTP 200) before accepting it as a valid match:

```rust
use rgx_core::{ExecResult, ExecutionMode, MatchOutcome, Regex};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let re = Regex::with_mode(
        r"(?<url>https?://[^\s)>\]]+)(?{native:verify_url})",
        ExecutionMode::Full,
    )?;

    let text = "Check out https://example.com and https://nonexistent.invalid/page";

    let result = re.find_first_async(text, |name, ctx| async move {
        match name.as_str() {
            "verify_url" => {
                // In a real application, extract the URL from ctx.captures
                // and make an HTTP HEAD request
                let url = "https://example.com"; // simplified
                match reqwest::Client::new().head(url).send().await {
                    Ok(resp) if resp.status().is_success() => ExecResult::Success,
                    _ => ExecResult::Failure,
                }
            }
            _ => ExecResult::Failure,
        }
    }).await;

    match result {
        Some(m) => println!("First valid URL at {}..{}", m.start, m.end),
        None => println!("No valid URLs found"),
    }

    Ok(())
}
```

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
| Use regular (non-async) matching | Just use `find_first` / `find_all` as usual -- nothing changes |

## Next

[Chapter 6: Working with Files >>>](06-file-matching.md)
