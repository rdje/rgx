# Chapter 4: Watching the Engine

You've built a pattern with callbacks, configured variables, and it's not matching what you expect. The input looks right. The pattern looks right. But somewhere between the first character and the final result, something goes wrong.

In a traditional regex engine, you'd be stuck. You'd stare at the pattern, mentally simulate the engine, add print statements around your match calls, and hope for insight. With rgx, you can watch the engine work.

## Why you'd want to see inside the engine

### A debugging story

Let's say you wrote a pattern to match timestamps:

```rust
let re = Regex::with_mode(
    r"(\d{2}):(\d{2}):(\d{2})(?{native:valid_time})",
    ExecutionMode::Full,
)?;

re.register_native("valid_time", |ctx| {
    let h: u32 = ctx.group(1).unwrap_or("99").parse().unwrap_or(99);
    let m: u32 = ctx.group(2).unwrap_or("99").parse().unwrap_or(99);
    let s: u32 = ctx.group(3).unwrap_or("99").parse().unwrap_or(99);
    if h < 24 && m < 60 && s < 60 {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;
```

It works perfectly on `"12:30:00"`. But it fails on `"timestamp: 12:30:00 UTC"`. Why? Is the regex not finding the digits? Is the callback returning failure? Is the engine trying the wrong start position?

Without visibility into the engine, you're guessing. With events, you know.

## Setting up an observer

An observer is a function that receives events as the engine runs. Setting one up takes one line:

```rust
re.on_event(|event| {
    println!("{:?}", event);
})?;
```

That's it. Now every match call on this regex will print events to stdout. Let's see what happens with our timestamp example:

```rust
let result = re.find_first("timestamp: 12:30:00 UTC");
```

You'd see output like:

```
MatchAttemptStarted { position: 0 }
MatchAttemptCompleted { position: 0, matched: false }
MatchAttemptStarted { position: 1 }
MatchAttemptCompleted { position: 1, matched: false }
...
MatchAttemptStarted { position: 11 }
CaptureCompleted { group: 1, start: 11, end: 13 }
CaptureCompleted { group: 2, start: 14, end: 16 }
CaptureCompleted { group: 3, start: 17, end: 19 }
CodeBlockEvaluated { language: "native", succeeded: true, position: 19 }
MatchAttemptCompleted { position: 11, matched: true }
```

Now you can see the engine tried positions 0 through 10 (failing at each), then succeeded at position 11. The captures were extracted correctly, the callback passed. If something were wrong, you'd see exactly where.

## Event types

rgx emits six types of events. Each one fires at a specific point in the matching process.

### MatchAttemptStarted

```rust
MatchEvent::MatchAttemptStarted { position: usize }
```

Fires when the engine begins trying to match at a new input position. For `find_first`, this fires once per starting position the engine tries. For `find_all`, it fires for each non-overlapping match attempt.

**When it fires:** At the very beginning of each attempt, before any pattern elements are tested.

```rust
re.on_event(|event| {
    if let MatchEvent::MatchAttemptStarted { position } = event {
        println!("Trying position {}", position);
    }
})?;
```

### MatchAttemptCompleted

```rust
MatchEvent::MatchAttemptCompleted { position: usize, matched: bool }
```

Fires when a match attempt at a given position finishes. The `matched` field tells you whether the attempt succeeded.

**When it fires:** After all backtracking for a given start position is exhausted, or after a match is found.

```rust
re.on_event(|event| {
    if let MatchEvent::MatchAttemptCompleted { position, matched } = event {
        if *matched {
            println!("Match found starting at position {}", position);
        }
    }
})?;
```

### BranchEntered

```rust
MatchEvent::BranchEntered { branch: u32, position: usize }
```

Fires when the engine enters a top-level alternation branch. The `branch` field is zero-based (branch 0 is the first alternative).

**When it fires:** At the moment the engine commits to trying a specific alternative in a top-level `|` alternation.

```rust
let re = Regex::compile(r"cat|dog|bird")?;

re.on_event(|event| {
    if let MatchEvent::BranchEntered { branch, position } = event {
        let name = match branch {
            0 => "cat",
            1 => "dog",
            2 => "bird",
            _ => "unknown",
        };
        println!("Trying '{}' at position {}", name, position);
    }
})?;

re.find_first("I have a dog");
// Output:
// Trying 'cat' at position 0
// Trying 'dog' at position 0
// Trying 'bird' at position 0
// Trying 'cat' at position 1
// ...
// Trying 'dog' at position 9
// (match found)
```

### CaptureCompleted

```rust
MatchEvent::CaptureCompleted { group: u32, start: usize, end: usize }
```

Fires when a capture group successfully captures text. Group 0 is the overall match. Groups 1+ are the parenthesized sub-expressions.

**When it fires:** At the moment a `SaveEnd` instruction executes, recording the end of a capture.

```rust
re.on_event(|event| {
    if let MatchEvent::CaptureCompleted { group, start, end } = event {
        println!("Group {} captured bytes {}..{}", group, start, end);
    }
})?;
```

Note: during backtracking, a capture might be completed and then "undone." The event fires on every completion, even on paths that are later abandoned. If you're building a tool that only cares about the *final* captures, correlate capture events with the `MatchAttemptCompleted { matched: true }` event.

### BacktrackOccurred

```rust
MatchEvent::BacktrackOccurred { position: usize, stack_depth: usize }
```

Fires when the engine backtracks. The `position` is where the engine ends up after backtracking. The `stack_depth` is the backtrack stack depth *before* the frame was popped.

**When it fires:** Every time the engine pops a backtrack frame and rewinds.

```rust
re.on_event(|event| {
    if let MatchEvent::BacktrackOccurred { position, stack_depth } = event {
        println!("Backtrack to position {}, stack depth was {}", position, stack_depth);
    }
})?;
```

This is invaluable for diagnosing performance problems. Excessive backtracking is the most common cause of slow regex matching. If you see thousands of backtrack events for a single match attempt, your pattern likely has an exponential backtracking problem.

### CodeBlockEvaluated

```rust
MatchEvent::CodeBlockEvaluated { language: String, succeeded: bool, position: usize }
```

Fires after a code block (callback) runs. The `language` field tells you which backend executed it (`"native"`, `"lua"`, `"js"`, `"rhai"`, `"wasm"`). The `succeeded` field tells you whether the callback returned a passing result.

**When it fires:** Immediately after the callback returns and the engine has interpreted the result.

```rust
re.on_event(|event| {
    if let MatchEvent::CodeBlockEvaluated { language, succeeded, position } = event {
        println!(
            "[{}] callback at position {}: {}",
            language,
            position,
            if *succeeded { "PASS" } else { "FAIL" }
        );
    }
})?;
```

## Building a debugger

Let's build a simple interactive debugger that traces every step of matching:

```rust
use rgx_core::{ExecutionMode, MatchEvent, Regex};

fn debug_match(pattern: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let re = Regex::with_mode(pattern, ExecutionMode::Safe)?;

    re.on_event(|event| {
        match event {
            MatchEvent::MatchAttemptStarted { position } => {
                println!("--- Attempt at position {} ---", position);
            }
            MatchEvent::BranchEntered { branch, position } => {
                println!("  Branch {} entered at position {}", branch, position);
            }
            MatchEvent::CaptureCompleted { group, start, end } => {
                println!("  Capture group {} = [{}..{}]", group, start, end);
            }
            MatchEvent::BacktrackOccurred { position, stack_depth } => {
                println!("  << Backtrack to position {} (stack: {}) >>", position, stack_depth);
            }
            MatchEvent::CodeBlockEvaluated { language, succeeded, position } => {
                let icon = if *succeeded { "PASS" } else { "FAIL" };
                println!("  [{}] {} at position {}", language, icon, position);
            }
            MatchEvent::MatchAttemptCompleted { position, matched } => {
                if *matched {
                    println!("=== MATCH at position {} ===\n", position);
                } else {
                    println!("--- No match at position {} ---\n", position);
                }
            }
        }
    })?;

    let result = re.find_first(text);
    match result {
        Some(m) => println!("Final result: matched bytes {}..{}", m.start, m.end),
        None => println!("Final result: no match"),
    }

    Ok(())
}
```

Usage:

```rust
debug_match(
    r"(\d{2}):(\d{2})",
    "Time is 14:30 now",
)?;
```

This prints a complete execution trace. You can pipe it to a file, search it for specific events, or build a visual tool on top of it.

## Building a profiler (backtrack counter)

Backtracking is the number-one cause of regex performance problems. A pattern like `(a+)+b` on the input `"aaaaaaaaaaaac"` can cause exponential backtracking. Let's build a profiler that counts backtracks and warns you:

```rust
use rgx_core::{ExecutionMode, MatchEvent, Regex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct MatchProfile {
    attempts: Arc<AtomicUsize>,
    backtracks: Arc<AtomicUsize>,
    code_evals: Arc<AtomicUsize>,
    captures: Arc<AtomicUsize>,
}

impl MatchProfile {
    fn new() -> Self {
        Self {
            attempts: Arc::new(AtomicUsize::new(0)),
            backtracks: Arc::new(AtomicUsize::new(0)),
            code_evals: Arc::new(AtomicUsize::new(0)),
            captures: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn attach(&self, re: &Regex) -> Result<(), Box<dyn std::error::Error>> {
        let attempts = self.attempts.clone();
        let backtracks = self.backtracks.clone();
        let code_evals = self.code_evals.clone();
        let captures = self.captures.clone();

        re.on_event(move |event| {
            match event {
                MatchEvent::MatchAttemptStarted { .. } => {
                    attempts.fetch_add(1, Ordering::Relaxed);
                }
                MatchEvent::BacktrackOccurred { .. } => {
                    backtracks.fetch_add(1, Ordering::Relaxed);
                }
                MatchEvent::CodeBlockEvaluated { .. } => {
                    code_evals.fetch_add(1, Ordering::Relaxed);
                }
                MatchEvent::CaptureCompleted { .. } => {
                    captures.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
        })?;

        Ok(())
    }

    fn report(&self) {
        let attempts = self.attempts.load(Ordering::Relaxed);
        let backtracks = self.backtracks.load(Ordering::Relaxed);
        let code_evals = self.code_evals.load(Ordering::Relaxed);
        let captures = self.captures.load(Ordering::Relaxed);

        println!("Match Profile:");
        println!("  Attempts:    {}", attempts);
        println!("  Backtracks:  {}", backtracks);
        println!("  Code evals:  {}", code_evals);
        println!("  Captures:    {}", captures);

        if backtracks > attempts * 10 {
            println!("  WARNING: High backtrack ratio ({:.1}x attempts)",
                backtracks as f64 / attempts.max(1) as f64);
        }
    }
}

// Usage
let re = Regex::compile(r"(a+)+b")?;
let profile = MatchProfile::new();
profile.attach(&re)?;

re.find_first("aaaaaaaaaaaac");

profile.report();
// Match Profile:
//   Attempts:    13
//   Backtracks:  4094  (or similar high number)
//   Code evals:  0
//   Captures:    ...
//   WARNING: High backtrack ratio (315.0x attempts)
```

That warning tells you the pattern has a catastrophic backtracking problem before it becomes a production outage.

## Building a coverage tool (branch tracker)

When you have a pattern with multiple alternatives, you might want to know which branches actually match during testing. This helps you find dead branches in complex patterns:

```rust
use rgx_core::{ExecutionMode, MatchEvent, Regex};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

struct BranchCoverage {
    entered: Arc<Mutex<HashSet<u32>>>,
    matched: Arc<Mutex<HashSet<u32>>>,
}

impl BranchCoverage {
    fn new() -> Self {
        Self {
            entered: Arc::new(Mutex::new(HashSet::new())),
            matched: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn attach(&self, re: &Regex) -> Result<(), Box<dyn std::error::Error>> {
        let entered = self.entered.clone();
        let matched = self.matched.clone();
        let last_branch: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        let branch_for_match = last_branch.clone();

        re.on_event(move |event| {
            match event {
                MatchEvent::BranchEntered { branch, .. } => {
                    entered.lock().unwrap().insert(*branch);
                    *last_branch.lock().unwrap() = Some(*branch);
                }
                MatchEvent::MatchAttemptCompleted { matched: true, .. } => {
                    if let Some(branch) = *branch_for_match.lock().unwrap() {
                        matched.lock().unwrap().insert(branch);
                    }
                }
                _ => {}
            }
        })?;

        Ok(())
    }

    fn report(&self, branch_names: &[&str]) {
        let entered = self.entered.lock().unwrap();
        let matched = self.matched.lock().unwrap();

        println!("Branch Coverage:");
        for (i, name) in branch_names.iter().enumerate() {
            let idx = i as u32;
            let tried = if entered.contains(&idx) { "tried" } else { "never tried" };
            let hit = if matched.contains(&idx) { "matched" } else { "never matched" };
            println!("  Branch {} ({}): {}, {}", i, name, tried, hit);
        }
    }
}

// Usage
let re = Regex::compile(r"ERROR|WARN|INFO|DEBUG|TRACE")?;
let coverage = BranchCoverage::new();
coverage.attach(&re)?;

// Run your test suite
for line in test_log_lines {
    re.find_all(line);
}

coverage.report(&["ERROR", "WARN", "INFO", "DEBUG", "TRACE"]);
// Branch Coverage:
//   Branch 0 (ERROR): tried, matched
//   Branch 1 (WARN): tried, matched
//   Branch 2 (INFO): tried, matched
//   Branch 3 (DEBUG): tried, matched
//   Branch 4 (TRACE): tried, never matched   <-- dead branch in tests?
```

If TRACE is never matched in your test suite, either your tests are incomplete or the branch is dead code.

## Zero overhead

Events use a zero-overhead design. Here's what that means concretely:

**When no observer is registered:**
- The engine checks a single boolean/pointer to see if an observer exists
- Finding none, it skips the event entirely
- No allocation, no formatting, no function call

**When an observer is registered:**
- Events are constructed only at the moment they fire
- They're passed by reference to the observer (no cloning)
- The observer is a single function pointer -- no dynamic dispatch chain

This means you can deploy rgx with event capability compiled in, register no observer, and pay zero cost. In benchmarks, the difference between "events compiled in, no observer" and "events compiled out entirely" is within measurement noise.

When you *do* register an observer, the cost is whatever your observer function does. A simple counter-increment costs nanoseconds. A println costs microseconds. The engine itself adds no overhead beyond the event construction (a small struct copy).

## Combining events with callbacks

Events and callbacks serve different purposes but work together. Callbacks *influence* matching (they can accept, reject, steer). Events *observe* matching (they can't change anything). Use them together:

```rust
use rgx_core::{ExecResult, ExecutionMode, MatchEvent, Regex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let re = Regex::with_mode(
    r"(\d+)(?{native:validate})",
    ExecutionMode::Full,
)?;

// The callback: influences matching
re.register_native("validate", |ctx| {
    let n: i64 = ctx.group(1).unwrap_or("0").parse().unwrap_or(0);
    if n > 0 && n < 1000 {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

// The observer: just watches
let pass_count = Arc::new(AtomicUsize::new(0));
let fail_count = Arc::new(AtomicUsize::new(0));
let passes = pass_count.clone();
let fails = fail_count.clone();

re.on_event(move |event| {
    if let MatchEvent::CodeBlockEvaluated { succeeded, .. } = event {
        if *succeeded {
            passes.fetch_add(1, Ordering::Relaxed);
        } else {
            fails.fetch_add(1, Ordering::Relaxed);
        }
    }
})?;

re.find_all("Values: 42, 9999, 100, -5, 500");

println!("Callback pass rate: {}/{}",
    pass_count.load(Ordering::Relaxed),
    pass_count.load(Ordering::Relaxed) + fail_count.load(Ordering::Relaxed));
```

## Summary

| What you want | How |
|---------------|-----|
| Watch all events | `re.on_event(\|e\| println!("{:?}", e))?` |
| Count backtracks | Increment a counter on `BacktrackOccurred` |
| Count callback passes/fails | Increment counters on `CodeBlockEvaluated` |
| Track branch coverage | Collect branch numbers from `BranchEntered` |
| Build a debugger | Print formatted output for each event type |
| Disable observation | Don't register an observer (zero cost) |

## Next

[Chapter 5: Async Callbacks >>>](05-async-io.md)
