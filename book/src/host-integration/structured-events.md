# Structured Events

rgx can emit structured events at key points during matching. These events are fire-and-forget -- they don't affect match behavior. They're designed for debugging, profiling, and building observability tools.

## Registering an observer

Use `on_event` to register a callback that receives `MatchEvent` values:

```rust,no_run
# use rgx_core::{Regex, MatchEvent};
let re = Regex::compile(r"(\d+)-(\w+)")?;

re.on_event(|event| {
    eprintln!("{event:?}");
})?;

re.find("item 42-abc here");
// Events are printed to stderr as matching progresses
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Zero overhead when not registered

If you never call `on_event`, there is zero runtime cost. The event emission paths are behind a flag check that is nearly free when no observer is registered. You don't pay for what you don't use.

## MatchEvent variants

### MatchAttemptStarted

Fired when the engine begins trying to match at a new input position:

```text
MatchAttemptStarted { position: 5 }
```

| Field | Type | Meaning |
|-------|------|---------|
| `position` | `usize` | Byte offset where the attempt begins |

### MatchAttemptCompleted

Fired when an attempt at a position finishes:

```text
MatchAttemptCompleted { position: 5, matched: true }
```

| Field | Type | Meaning |
|-------|------|---------|
| `position` | `usize` | Byte offset of the attempt |
| `matched` | `bool` | Whether the attempt produced a match |

### BranchEntered

Fired when a top-level alternation branch is entered:

```text
BranchEntered { branch: 0, position: 5 }
```

| Field | Type | Meaning |
|-------|------|---------|
| `branch` | `u32` | Zero-based branch index |
| `position` | `usize` | Byte offset at entry |

### CaptureCompleted

Fired when a capture group finishes:

```text
CaptureCompleted { group: 1, start: 5, end: 7 }
```

| Field | Type | Meaning |
|-------|------|---------|
| `group` | `u32` | Group number (0 = overall match) |
| `start` | `usize` | Start byte offset |
| `end` | `usize` | End byte offset |

### BacktrackOccurred

Fired when the engine backtracks:

```text
BacktrackOccurred { position: 7, stack_depth: 3 }
```

| Field | Type | Meaning |
|-------|------|---------|
| `position` | `usize` | Position after backtrack |
| `stack_depth` | `usize` | Stack depth before popping |

### CodeBlockEvaluated

Fired after a code block runs:

```text
CodeBlockEvaluated { language: "lua", succeeded: true, position: 7 }
```

| Field | Type | Meaning |
|-------|------|---------|
| `language` | `String` | Language tag (`"lua"`, `"native"`, etc.) |
| `succeeded` | `bool` | Whether the code block returned success |
| `position` | `usize` | Position at time of evaluation |

## Use case: debugging a pattern

When a regex isn't matching as expected, events reveal exactly what the engine is doing:

```rust,no_run
# use rgx_core::{Regex, MatchEvent};
let re = Regex::compile(r"(a+)(b+)")?;

re.on_event(|event| {
    match event {
        MatchEvent::MatchAttemptStarted { position } => {
            eprintln!("  TRY at {position}");
        }
        MatchEvent::MatchAttemptCompleted { position, matched } => {
            eprintln!("  {} at {position}", if *matched { "HIT" } else { "MISS" });
        }
        MatchEvent::BacktrackOccurred { position, stack_depth } => {
            eprintln!("  BACKTRACK to {position} (depth {stack_depth})");
        }
        MatchEvent::CaptureCompleted { group, start, end } => {
            eprintln!("  CAPTURE group {group}: {start}..{end}");
        }
        _ => {}
    }
})?;

re.find("xxaabb");
# Ok::<(), Box<dyn std::error::Error>>(())
```

This might print:

```text
  TRY at 0
  MISS at 0
  TRY at 1
  MISS at 1
  TRY at 2
  CAPTURE group 1: 2..4
  CAPTURE group 2: 4..6
  CAPTURE group 0: 2..6
  HIT at 2
```

## Use case: profiling match performance

Count backtracks and attempts to identify expensive patterns:

```rust,no_run
# use rgx_core::{Regex, MatchEvent};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let re = Regex::compile(r"(a+)+b")?;

let attempts = Arc::new(AtomicUsize::new(0));
let backtracks = Arc::new(AtomicUsize::new(0));
let a = attempts.clone();
let b = backtracks.clone();

re.on_event(move |event| {
    match event {
        MatchEvent::MatchAttemptStarted { .. } => {
            a.fetch_add(1, Ordering::Relaxed);
        }
        MatchEvent::BacktrackOccurred { .. } => {
            b.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
})?;

re.find("aaaaaac");  // no match -- triggers backtracking

eprintln!(
    "attempts={}, backtracks={}",
    attempts.load(Ordering::Relaxed),
    backtracks.load(Ordering::Relaxed)
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Use case: branch coverage

Track which alternation branches are exercised across a test suite:

```rust,no_run
# use rgx_core::{Regex, MatchEvent};
use std::sync::{Arc, Mutex};
use std::collections::HashSet;

let re = Regex::compile(r"(?:GET|POST|PUT|DELETE|PATCH)\s+/\w+")?;

let branches_hit = Arc::new(Mutex::new(HashSet::new()));
let b = branches_hit.clone();

re.on_event(move |event| {
    if let MatchEvent::BranchEntered { branch, .. } = event {
        b.lock().unwrap().insert(*branch);
    }
})?;

re.find("GET /users");
re.find("POST /items");
re.find("DELETE /old");

let hit = branches_hit.lock().unwrap();
eprintln!("branches exercised: {:?}", *hit);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Use case: code block auditing

Log every code block evaluation for security auditing:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, MatchEvent};
let re = Regex::with_mode(
    r#"\d+(?{lua:return tonumber(arg[1]) > 0})"#,
    ExecutionMode::Safe,
)?;

re.on_event(|event| {
    if let MatchEvent::CodeBlockEvaluated { language, succeeded, position } = event {
        eprintln!(
            "[AUDIT] {language} block at pos {position}: {}",
            if *succeeded { "PASS" } else { "FAIL" }
        );
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Combining events with the CLI

The CLI exposes events via the `--events` flag:

```bash
rgx --events '\d+' 'abc 42 xyz'
```

This prints events to stderr while matches go to stdout, so you can pipe match output normally while watching the engine work.
