# Async I/O

Sometimes a predicate callback needs to check something that isn't available synchronously -- a database lookup, an HTTP request, a file read. rgx supports this through *suspendable matching*: the engine pauses at a code block, hands control back to the caller, and resumes after the external operation completes.

## The core idea

Normal matching is synchronous: the engine runs start to finish and returns a result. Suspendable matching adds a pause/resume protocol:

1. The engine encounters an unregistered native callback
2. Instead of treating it as an error, it **suspends** -- freezing its entire internal state
3. It returns a `MatchOutcome::Suspended` with a `MatchContinuation`
4. The caller resolves the callback externally (async DB query, HTTP call, etc.)
5. The caller calls `resume()` with the result
6. The engine continues exactly where it left off

## find_first_suspendable

The entry point is `find_first_suspendable`, which returns a `MatchOutcome` instead of `Option<MatchResult>`:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, MatchOutcome};
let re = Regex::with_mode(
    r"user:(\w+)(?{native:check_db})",
    ExecutionMode::Full,
)?;

// Note: we intentionally do NOT register "check_db" as a native callback.
// This causes the engine to suspend when it reaches the code block.

let mut outcome = re.find_first_suspendable("user:alice");

loop {
    match outcome {
        MatchOutcome::Completed(result) => {
            // Matching is done. result is Option<MatchResult>.
            if let Some(m) = result {
                println!("matched at {}..{}", m.start, m.end);
            } else {
                println!("no match");
            }
            break;
        }
        MatchOutcome::Suspended(continuation) => {
            // The engine paused at an unregistered callback.
            let name = &continuation.pending_callback_name;
            println!("engine suspended for callback: {name}");

            // Resolve the callback externally.
            // In real code, this would be an async DB query, HTTP call, etc.
            let resolved = if name == "check_db" {
                ExecResult::Success  // user exists in DB
            } else {
                ExecResult::Failure
            };

            // Resume matching with the resolved result.
            outcome = re.resume(*continuation, resolved);
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## MatchOutcome

```rust,ignore
pub enum MatchOutcome {
    /// Match completed synchronously.
    Completed(Option<MatchResult>),
    /// Match suspended -- an async callback needs resolution.
    Suspended(Box<MatchContinuation>),
}
```

## MatchContinuation

The continuation carries everything needed to resume:

| Field | Type | Description |
|-------|------|-------------|
| `pending_callback_name` | `String` | Name of the callback to resolve |
| `pending_context` | `ExecContextSnapshot` | Match state at suspension point |

The `ExecContextSnapshot` gives the resolver enough information to make its decision:

| Field | Type | Description |
|-------|------|-------------|
| `position` | `usize` | Current byte position |
| `match_start` | `usize` | Start of the current match attempt |
| `captures` | `Vec<Option<usize>>` | Capture group byte-offset slots |
| `variables` | `HashMap<String, String>` | Host variables snapshot |

## Use case: database-backed matching

Imagine validating usernames against a database during pattern matching:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, MatchOutcome};
let re = Regex::with_mode(
    r"@(\w+)(?{native:user_exists})",
    ExecutionMode::Full,
)?;

fn lookup_user(username: &str) -> bool {
    // Simulate a database lookup
    ["alice", "bob", "charlie"].contains(&username)
}

let text = "@alice mentioned @dave";
let mut outcome = re.find_first_suspendable(text);

loop {
    match outcome {
        MatchOutcome::Completed(result) => {
            match result {
                Some(m) => println!("found valid user mention at {}..{}", m.start, m.end),
                None => println!("no valid user mention found"),
            }
            break;
        }
        MatchOutcome::Suspended(continuation) => {
            // Extract the captured username from the snapshot
            let username_captures = &continuation.pending_context.captures;
            let username = if let (Some(Some(start)), Some(Some(end))) =
                (username_captures.get(2), username_captures.get(3))
            {
                &text[*start..*end]
            } else {
                ""
            };

            let result = if lookup_user(username) {
                ExecResult::Success
            } else {
                ExecResult::Failure
            };

            outcome = re.resume(*continuation, result);
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## find_first_async helper

For async runtimes (tokio, async-std, smol), `find_first_async` drives the suspend/resume loop automatically:

```rust,ignore
use rgx_core::{Regex, ExecutionMode, ExecResult, ExecContextSnapshot};

let re = Regex::with_mode(
    r"@(\w+)(?{native:user_exists})",
    ExecutionMode::Full,
)?;

let result = re.find_first_async("@alice mentioned @dave", |name, ctx| async move {
    match name.as_str() {
        "user_exists" => {
            // This is where you'd do your async work:
            // let exists = db.query_user(&username).await?;
            ExecResult::Success
        }
        _ => ExecResult::Failure,
    }
}).await;
```

The signature:

```rust,ignore
pub async fn find_first_async<F, Fut>(
    &self,
    text: &str,
    resolver: F,
) -> Option<MatchResult>
where
    F: Fn(String, ExecContextSnapshot) -> Fut,
    Fut: std::future::Future<Output = ExecResult>,
```

The resolver receives the callback name and context snapshot, returns a future that resolves to an `ExecResult`. The helper loops internally, handling chained suspensions automatically.

## Chained suspensions

A pattern can contain multiple unregistered callbacks. Each one causes a separate suspension:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, MatchOutcome};
let re = Regex::with_mode(
    r"(\w+):(\w+)(?{native:check_auth})(?{native:check_quota})",
    ExecutionMode::Full,
)?;

let mut outcome = re.find_first_suspendable("admin:write");
let mut resolved = 0;

loop {
    match outcome {
        MatchOutcome::Completed(_) => break,
        MatchOutcome::Suspended(continuation) => {
            let name = continuation.pending_callback_name.clone();
            eprintln!("resolving: {name}");
            resolved += 1;

            // Both callbacks succeed
            outcome = re.resume(*continuation, ExecResult::Success);
        }
    }
}

assert_eq!(resolved, 2);  // two suspensions, two resolutions
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Mixing registered and unregistered callbacks

You can register some callbacks and leave others for async resolution. Only unregistered native callbacks cause suspension; registered ones execute synchronously:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, MatchOutcome};
let re = Regex::with_mode(
    r"(\d+)(?{native:validate})(?{native:check_remote})",
    ExecutionMode::Full,
)?;

// Register "validate" synchronously
re.register_native("validate", |ctx| {
    let n: i64 = ctx.group(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    if n > 0 { ExecResult::Success } else { ExecResult::Failure }
})?;

// "check_remote" is NOT registered -- it will suspend
let mut outcome = re.find_first_suspendable("42");

match outcome {
    MatchOutcome::Completed(_) => panic!("should have suspended"),
    MatchOutcome::Suspended(continuation) => {
        assert_eq!(continuation.pending_callback_name, "check_remote");
        // Resolve and continue
        outcome = re.resume(*continuation, ExecResult::Success);
    }
}

match outcome {
    MatchOutcome::Completed(result) => assert!(result.is_some()),
    MatchOutcome::Suspended(_) => panic!("should be done"),
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Error handling in async resolution

If your resolver returns `ExecResult::Failure`, the engine backtracks normally -- it may try alternative branches or subsequent start positions. If you return `ExecResult::Error(msg)`, the engine also backtracks but the error is available for logging.

Returning `ExecResult::Steer(SteerResult::Abort)` from a resolver will stop the search entirely, which is useful if the external system is unavailable and you want to fail fast.

## Thread safety

`MatchContinuation` is `Send + Sync`. You can safely move it across threads -- for example, sending it to a task pool for async resolution and receiving the result on a different thread.
