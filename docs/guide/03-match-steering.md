# Chapter 3: Steering the Match

In [Chapter 2](02-predicate-callbacks.md) you learned that callbacks answer a simple question: does this match pass or fail? That's enough for validation, but real programs need more nuanced control. What if you want to accept a match immediately without letting the engine try other possibilities? What if you want to skip ahead? What if you want to stop the entire search?

This chapter introduces **match steering** -- a set of actions that give your callback fine-grained control over how the engine proceeds.

## Why pass/fail isn't always enough

### Scenario: scanning a large document

You're scanning a 100MB server log for the first occurrence of a critical error. Your pattern matches error lines, and your callback checks whether the error is critical. With only pass/fail:

- The callback says **pass** on the first critical error
- The engine records the match... then keeps scanning the remaining 99MB for more matches (if you called `find_all`)
- Or, if you used `find_first`, it still continues exploring alternative ways to match the same region (backtracking) before committing to the result

What you really want is: "I found what I need. Stop now."

### Scenario: budget-limited scanning

You're running a web application firewall. Each incoming request is scanned against hundreds of patterns. You have a time budget of 5ms per request. If scanning takes too long, you need to give up gracefully rather than blocking the request.

Pass/fail can't express "I've spent too long, abort."

### Scenario: skipping known-good regions

You're processing a mixed-format file where large sections are base64-encoded data. You know those sections can't contain what you're looking for. Instead of letting the regex engine chew through them character by character, you'd like to jump past them.

## The five steering actions

Callbacks can return `ExecResult::Steer(action)` where `action` is one of:

| Action | What it does | When to use it |
|--------|-------------|----------------|
| `SteerResult::Continue` | Proceed normally (same as `Success`) | When the match is fine so far, keep going |
| `SteerResult::Fail` | Backtrack (same as `Failure`) | When the match should not succeed on this path |
| `SteerResult::Accept` | Accept the match immediately | When you want to commit without further exploration |
| `SteerResult::Skip(n)` | Advance the input position by `n` bytes | When you know the next `n` bytes can't contribute to a match |
| `SteerResult::Abort` | Stop the entire search | When you want no more matches from this call |

Let's explore each one with examples.

### Continue

`Continue` is equivalent to returning `ExecResult::Success`. The engine records the callback as passing and continues matching the rest of the pattern. It exists for completeness -- when your steering logic sometimes needs to continue normally and other times needs to steer, `Continue` keeps your code consistent.

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};

let re = Regex::with_mode(
    r"(?<word>[a-z]+)(?{native:maybe_skip})",
    ExecutionMode::Full,
)?;

re.register_native("maybe_skip", |ctx| {
    let word = ctx.named("word").unwrap_or("");
    if word == "skip" {
        ExecResult::Steer(SteerResult::Fail)  // reject this one
    } else {
        ExecResult::Steer(SteerResult::Continue)  // proceed normally
    }
})?;
```

This is functionally identical to returning `Success`/`Failure`, but reads more clearly when it's part of a larger match/case with other steering actions.

### Fail

`Fail` tells the engine to backtrack, just like `ExecResult::Failure`. The engine pretends the pattern didn't match at this point and tries alternatives.

```rust
let re = Regex::with_mode(
    r"(?<num>\d+)(?{native:reject_even})",
    ExecutionMode::Full,
)?;

re.register_native("reject_even", |ctx| {
    let num: i64 = ctx.named("num").unwrap_or("0").parse().unwrap_or(0);
    if num % 2 == 0 {
        ExecResult::Steer(SteerResult::Fail)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;

assert!(!re.is_match("Number: 42"));   // even, rejected
assert!(re.is_match("Number: 43"));    // odd, accepted
```

A second example -- rejecting matches inside comments:

```rust
let re = Regex::with_mode(
    r"(?<keyword>TODO|FIXME|HACK)(?{native:not_in_comment})",
    ExecutionMode::Full,
)?;

re.register_native("not_in_comment", |ctx| {
    let line_start = ctx.text[..ctx.match_start].rfind('\n')
        .map_or(0, |pos| pos + 1);
    let line_prefix = &ctx.text[line_start..ctx.match_start];
    if line_prefix.contains("//") || line_prefix.trim_start().starts_with('#') {
        ExecResult::Steer(SteerResult::Fail)  // inside a comment, skip
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
```

### Accept

`Accept` is the "I'm done" signal. It tells the engine to immediately accept the match at the current position. The engine will not try further alternatives, will not backtrack to explore other paths, and will not try to extend the match. It commits.

This is powerful when you know the first valid match is the one you want.

#### Example 1: First-match optimization

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let re = Regex::with_mode(
    r"(?<line>[^\n]*ERROR[^\n]*)(?{native:accept_first})",
    ExecutionMode::Full,
)?;

re.register_native("accept_first", |_ctx| {
    // Don't bother exploring alternatives -- take the first error line
    ExecResult::Steer(SteerResult::Accept)
})?;

let text = "INFO: ok\nERROR: disk full\nERROR: timeout\n";
let m = re.find_first(text);
// The engine finds the first ERROR line and commits immediately
assert!(m.is_some());
let matched = &text[m.as_ref().unwrap().start..m.as_ref().unwrap().end];
assert!(matched.contains("disk full"));
```

#### Example 2: Priority-based acceptance

When you have multiple alternatives and want to accept the highest-priority one immediately:

```rust
let re = Regex::with_mode(
    r"(?<level>FATAL|ERROR|WARN|INFO)(?{native:priority_accept})",
    ExecutionMode::Full,
)?;

re.register_native("priority_accept", |ctx| {
    let level = ctx.named("level").unwrap_or("");
    match level {
        "FATAL" | "ERROR" => ExecResult::Steer(SteerResult::Accept),
        _ => ExecResult::Steer(SteerResult::Continue),
    }
})?;
```

### Skip

`Skip(n)` advances the engine's input position by `n` bytes before continuing. This is different from failing -- it doesn't backtrack. Instead, it tells the engine to leap forward, bypassing text that can't possibly contribute to a match.

#### Example 1: Skipping binary data

You're scanning a file that mixes text and binary blocks. Binary blocks start with a length-prefixed header:

```rust
let re = Regex::with_mode(
    r"(?<marker>BIN:(?<len>\d+):)(?{native:skip_binary})",
    ExecutionMode::Full,
)?;

re.register_native("skip_binary", |ctx| {
    let len: usize = ctx.named("len").unwrap_or("0").parse().unwrap_or(0);
    ExecResult::Steer(SteerResult::Skip(len))
})?;
```

When the engine finds `BIN:1024:`, the callback reads the length (1024) and tells the engine to jump 1024 bytes past the marker. The binary data is never examined character by character.

#### Example 2: Skipping quoted strings

When scanning for identifiers, you want to skip over the contents of quoted strings rather than matching fragments inside them:

```rust
let re = Regex::with_mode(
    r#"(?:"(?<quoted>[^"]*)")(?{native:skip_quoted})|(?<ident>[a-zA-Z_]\w*)"#,
    ExecutionMode::Full,
)?;

re.register_native("skip_quoted", |ctx| {
    // We matched a quoted string -- skip past it so the identifier
    // branch never sees its contents
    ExecResult::Steer(SteerResult::Continue)
})?;
```

### Abort

`Abort` stops the entire match search. No more matches will be returned, no more start positions will be tried. The engine halts as if it reached the end of the input.

#### Example 1: Budget-limited scanning

```rust
use std::time::Instant;
use std::sync::Arc;

let re = Regex::with_mode(
    r"(?<pattern>sensitive-data-\d+)(?{native:budget_check})",
    ExecutionMode::Full,
)?;

let start_time = Arc::new(Instant::now());
let start = start_time.clone();

re.register_native("budget_check", move |_ctx| {
    if start.elapsed().as_millis() > 5 {
        // Time budget exceeded -- abort the search
        ExecResult::Steer(SteerResult::Abort)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
```

This pattern will match as many instances of `sensitive-data-\d+` as it can within 5 milliseconds, then stop. No panic, no timeout exception -- just a clean halt.

#### Example 2: Match-count limiter

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let re = Regex::with_mode(
    r"(?<word>\b\w+\b)(?{native:limit_matches})",
    ExecutionMode::Full,
)?;

let count = Arc::new(AtomicUsize::new(0));
let counter = count.clone();

re.register_native("limit_matches", move |_ctx| {
    let n = counter.fetch_add(1, Ordering::Relaxed);
    if n >= 100 {
        ExecResult::Steer(SteerResult::Abort)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;

// find_all will return at most 100 matches
let matches = re.find_all(huge_text);
```

Note: because of backtracking, the counter might be incremented on paths that don't lead to a final match. If you need an exact count of *accepted* matches, track that in a separate variable and only increment when the engine ultimately accepts the path. For a hard cap, an atomic counter is a reasonable approximation.

## Real scenario: building a resource-budgeted scanner

Let's combine several steering actions into a complete scanner that respects resource budgets:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

fn build_budgeted_scanner(
    pattern: &str,
    max_matches: usize,
    max_millis: u128,
) -> Result<Regex, Box<dyn std::error::Error>> {
    let re = Regex::with_mode(
        &format!(r"(?<m>{pattern})(?{{native:budget}})"),
        ExecutionMode::Full,
    )?;

    let match_count = Arc::new(AtomicUsize::new(0));
    let counter = match_count.clone();
    let start = Arc::new(Instant::now());
    let timer = start.clone();

    re.register_native("budget", move |_ctx| {
        // Check time budget
        if timer.elapsed().as_millis() > max_millis {
            return ExecResult::Steer(SteerResult::Abort);
        }

        // Check match budget
        let n = counter.fetch_add(1, Ordering::Relaxed);
        if n >= max_matches {
            return ExecResult::Steer(SteerResult::Abort);
        }

        ExecResult::Steer(SteerResult::Continue)
    })?;

    Ok(re)
}

// Usage: find email addresses, but stop after 50 matches or 10ms
let scanner = build_budgeted_scanner(
    r"\b[\w.+-]+@[\w.-]+\.\w{2,}\b",
    50,
    10,
)?;

let matches = scanner.find_all(large_email_corpus);
// matches.len() <= 50, and the scan took <= ~10ms
```

This scanner is safe to deploy in latency-sensitive paths. It won't hang on pathological input. It won't consume unbounded memory building a match list. And the caller sees clean results -- just a shorter list.

## Real scenario: header-based early acceptance

You're parsing HTTP responses. The header section ends at `\r\n\r\n`. Once you find the header you care about, you want to stop scanning immediately rather than reading through a potentially large body:

```rust
let re = Regex::with_mode(
    r"(?<header>Content-Type:\s*(?<value>[^\r\n]+))(?{native:accept_header})",
    ExecutionMode::Full,
)?;

re.register_native("accept_header", |ctx| {
    let value = ctx.named("value").unwrap_or("");
    if value.starts_with("application/json") {
        // Found what we need -- stop scanning the rest of the response
        ExecResult::Steer(SteerResult::Accept)
    } else {
        // Not the content type we want, but keep looking
        ExecResult::Steer(SteerResult::Continue)
    }
})?;

let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"big\": \"body\"...}";
let m = re.find_first(response);
assert!(m.is_some());
```

The engine finds `Content-Type: application/json`, the callback says `Accept`, and the engine immediately returns without scanning the JSON body.

## How steering interacts with backtracking

Understanding the interaction between steering and backtracking is important for writing correct callbacks.

### Continue and Fail

These behave identically to `Success` and `Failure` from [Chapter 2](02-predicate-callbacks.md). The engine's backtracking behavior is unchanged.

- `Continue`: the engine proceeds forward. If a later part of the pattern fails, the engine backtracks normally and might re-run your callback with different captures.
- `Fail`: the engine backtracks immediately. It tries the next alternative or the next start position.

### Accept

`Accept` short-circuits backtracking. Once a callback returns `Accept`, the engine commits to the current match path. Even if there were untried alternatives that might have produced a longer or "better" match, they are not explored.

This means `Accept` can change the *content* of the match compared to letting the engine run to completion. Use it when you know the current match is good enough.

### Skip

`Skip(n)` modifies the engine's position but does not prevent backtracking. If the rest of the pattern fails after the skip, the engine can still backtrack to before the skip point and try alternatives.

Be careful with skip distances. If `n` is larger than the remaining input, the engine treats it as a failure (nothing left to match).

### Abort

`Abort` is the most drastic action. It terminates the entire search, not just the current match attempt. No more start positions are tried. `find_all` returns whatever matches were found so far. `find_first` returns the last successfully completed match (if any) or `None`.

Because `Abort` can fire on a backtracking path, it's possible for `Abort` to fire even though the match wouldn't have ultimately succeeded. In practice, this is usually fine -- `Abort` is used for resource limits, and an occasional false positive on "we've spent too long" is acceptable.

## Summary

| Action | Effect | Backtracks? | Stops search? |
|--------|--------|-------------|---------------|
| `Continue` | Match proceeds normally | Yes, if later steps fail | No |
| `Fail` | This path is rejected | Yes, immediately | No |
| `Accept` | This match is committed | No | No (next `find_all` iteration continues) |
| `Skip(n)` | Jump `n` bytes forward | Yes, if later steps fail | No |
| `Abort` | Entire search halts | N/A | Yes |

## Next

[Chapter 4: Watching the Engine >>>](04-structured-events.md)
