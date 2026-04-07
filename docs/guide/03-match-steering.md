# Chapter 3: Steering the Match

This is what makes rgx different from every other regex engine.

In [Chapter 2](02-predicate-callbacks.md) you learned that callbacks answer a simple question: does this match pass or fail? That's enough for validation, but real programs need more nuanced control. What if you want to accept a match immediately without letting the engine try other possibilities? What if you want to skip ahead? What if you want to stop the entire search?

Most regex engines give you two outcomes: match or no match. rgx gives you five. Your callback can say "yes," "no," "yes and stop looking," "jump forward," or "stop everything." That's match steering, and once you've used it, you'll wonder how you ever lived without it.

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

Let's explore each one with examples, and for each, we'll show how you'd solve the problem *without* steering so you can see the difference.

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

**Without steering:** You'd use a plain predicate callback that returns `Failure`. The result is the same, but when you're mixing `Fail` with `Accept` or `Abort` in the same callback, using `Steer` for all branches keeps the code consistent.

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

**Without steering:** You'd call `find_first`, get a result, then stop using it. But the engine still explores alternatives before returning -- you're paying for work you don't need. Or, with `find_all`, you'd collect all matches and take the first. `Accept` short-circuits this: the engine stops the moment your callback says so.

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

**Without steering:** You'd find all log levels, then sort by priority, then take the highest. With `Accept`, the engine commits the moment it sees `FATAL` or `ERROR` -- no sorting, no post-processing.

#### Example 3: Accepting after validation

Combine validation with early acceptance -- validate a credit card number format and accept immediately if it passes the Luhn check:

```rust
let re = Regex::with_mode(
    r"(?<cc>\d{4}[- ]?\d{4}[- ]?\d{4}[- ]?\d{4})(?{native:luhn_accept})",
    ExecutionMode::Full,
)?;

re.register_native("luhn_accept", |ctx| {
    let digits: String = ctx.named("cc").unwrap_or("")
        .chars().filter(|c| c.is_ascii_digit()).collect();
    if passes_luhn(&digits) {
        ExecResult::Steer(SteerResult::Accept)  // valid card, commit now
    } else {
        ExecResult::Steer(SteerResult::Fail)    // not a valid number
    }
})?;
```

### Skip

`Skip(n)` advances the engine's input position by `n` bytes before continuing. This is different from failing -- it doesn't backtrack. Instead, it tells the engine to leap forward, bypassing text that can't possibly contribute to a match.

**Without steering:** You'd either let the engine grind through every character (slow), or preprocess the input to remove the uninteresting sections (complicated and memory-intensive). `Skip` gives you surgical control.

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

#### Example 3: Skipping known-safe sections in structured data

You're scanning a file where sections are delimited and some are known-safe:

```rust
let re = Regex::with_mode(
    r"(?<header>---SAFE-SECTION:(?<len>\d+)---)(?{native:skip_safe})|(?<pattern>sensitive-\w+)",
    ExecutionMode::Full,
)?;

re.register_native("skip_safe", |ctx| {
    let len: usize = ctx.named("len").unwrap_or("0").parse().unwrap_or(0);
    // Jump past the safe section entirely
    ExecResult::Steer(SteerResult::Skip(len))
})?;
```

### Abort

`Abort` stops the entire match search. No more matches will be returned, no more start positions will be tried. The engine halts as if it reached the end of the input.

**Without steering:** You'd wrap your matching code in a loop with a manual break condition. But the engine doesn't know about your break -- it completes a full `find_all` before you can check. With `Abort`, the engine stops from the inside.

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

#### Example 3: Stopping on a sentinel value

Stop scanning as soon as you hit an end-of-data marker:

```rust
let re = Regex::with_mode(
    r"(?<token>[^\s,]+)(?{native:stop_at_end})",
    ExecutionMode::Full,
)?;

re.register_native("stop_at_end", |ctx| {
    let token = ctx.named("token").unwrap_or("");
    if token == "END" || token == "---" {
        ExecResult::Steer(SteerResult::Abort)  // stop the entire search
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;

let text = "apple, banana, cherry, END, dragonfruit, elderberry";
let matches = re.find_all(text);
// Only matches apple, banana, cherry -- stops at END
```

## Which steering action should I use?

Here's a decision guide. Start at the top and follow the first condition that applies:

```
Do you need to stop the ENTIRE search (not just this match)?
  Yes --> Abort
  No  |
      v
Do you want to commit to this match immediately, no backtracking?
  Yes --> Accept
  No  |
      v
Do you want to skip past a known-uninteresting region?
  Yes --> Skip(n)
  No  |
      v
Should this match path be rejected (backtrack, try alternatives)?
  Yes --> Fail
  No  |
      v
Everything is fine, keep going?
  Yes --> Continue
```

And here's the same logic as a quick-reference table:

| Your situation | Action | Example use case |
|---------------|--------|-----------------|
| "I found what I need, stop everything" | `Abort` | Time budget exceeded, sentinel found |
| "This match is perfect, commit now" | `Accept` | First valid error line, highest-priority match |
| "Jump over this section" | `Skip(n)` | Binary data block, base64 region |
| "This isn't right, try something else" | `Fail` | Failed validation, match inside a comment |
| "So far so good, keep matching" | `Continue` | Default path in a multi-branch callback |

## Before/after: solving problems with and without steering

### Input sanitization

**Without steering:** Find all matches, then filter in a second pass.

```rust
// Two passes, extra allocation, can't influence backtracking
let all_inputs = re.find_all(text);
let safe_inputs: Vec<_> = all_inputs.into_iter()
    .filter(|m| is_safe(&text[m.start..m.end]))
    .collect();
```

**With steering:** Reject unsafe inputs during matching. The engine backtracks and tries alternatives.

```rust
re.register_native("sanitize", |ctx| {
    let input = ctx.named("value").unwrap_or("");
    if is_safe(input) {
        ExecResult::Steer(SteerResult::Continue)
    } else {
        ExecResult::Steer(SteerResult::Fail)  // backtrack, try next
    }
})?;
```

### Rate-limited field extraction

**Without steering:** Extract everything, then truncate the result.

```rust
// Extracts ALL matches, even if there are millions, then takes 10
let all = re.find_all(huge_text);
let first_ten = &all[..10.min(all.len())];
```

**With steering:** Stop after 10 matches. No wasted work.

```rust
let count = Arc::new(AtomicUsize::new(0));
let counter = count.clone();
re.register_native("rate_limit", move |_ctx| {
    if counter.fetch_add(1, Ordering::Relaxed) >= 10 {
        ExecResult::Steer(SteerResult::Abort)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
let first_ten = re.find_all(huge_text);
```

### Field extraction with validation

**Without steering:** Match the structure, then validate fields in a separate step.

```rust
// Phase 1: extract candidates
let candidates = re.find_all(csv_row);
// Phase 2: validate each candidate (separate logic, separate pass)
for c in &candidates {
    let value = &csv_row[c.start..c.end];
    if !validate_field(value) { /* skip */ }
}
```

**With steering:** Validate inline. Invalid fields cause backtracking to try the next interpretation.

```rust
re.register_native("validate_field", |ctx| {
    let field = ctx.named("field").unwrap_or("");
    if validate_field(field) {
        ExecResult::Steer(SteerResult::Continue)
    } else {
        ExecResult::Steer(SteerResult::Fail)
    }
})?;
```

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

## Real scenario: steering + callbacks + variables working together

Here's a complete example that combines host variables (Chapter 1), predicate callbacks (Chapter 2), and steering (this chapter) into a configurable security scanner:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::collections::HashSet;

let re = Regex::with_mode(
    r"(?<ip>\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})(?{native:security_scan})",
    ExecutionMode::Full,
)?;

let seen_ips = Arc::new(std::sync::Mutex::new(HashSet::new()));
let seen = seen_ips.clone();
let alert_count = Arc::new(AtomicUsize::new(0));
let alerts = alert_count.clone();

re.register_native("security_scan", move |ctx| {
    let ip = ctx.named("ip").unwrap_or("").to_string();

    // Use a host variable to set the maximum number of alerts
    let max_alerts: usize = ctx.variable("max_alerts")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);

    // Check the alert budget
    let current = alerts.load(Ordering::Relaxed);
    if current >= max_alerts {
        return ExecResult::Steer(SteerResult::Abort);  // budget exhausted
    }

    // Skip IPs we've already seen (deduplication)
    let mut set = seen.lock().unwrap();
    if set.contains(&ip) {
        return ExecResult::Steer(SteerResult::Fail);  // skip duplicate
    }
    set.insert(ip.clone());

    // Use a host variable to decide the scan mode
    let mode = ctx.variable("scan_mode").unwrap_or_else(|| "normal".to_string());
    match mode.as_str() {
        "strict" => {
            // In strict mode, accept the first suspicious IP immediately
            if is_suspicious(&ip) {
                alerts.fetch_add(1, Ordering::Relaxed);
                ExecResult::Steer(SteerResult::Accept)
            } else {
                ExecResult::Steer(SteerResult::Fail)
            }
        }
        _ => {
            // In normal mode, just record it and continue
            if is_suspicious(&ip) {
                alerts.fetch_add(1, Ordering::Relaxed);
            }
            ExecResult::Steer(SteerResult::Continue)
        }
    }
})?;

// Configure via variables -- no recompilation needed
re.set_variable("scan_mode", "strict")?;
re.set_variable("max_alerts", "50")?;

let matches = re.find_all(log_text);
```

This single pattern does deduplication, budget enforcement, configurable scan modes, and early termination -- all during matching.

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

## Patterns and recipes

Here are copy-paste-ready patterns for common steering scenarios. Each one is self-contained.

### Recipe 1: First N matches

Stop after collecting a specific number of matches.

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, SteerResult};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let re = Regex::with_mode(r"(?<email>[\w.+-]+@[\w.-]+\.\w{2,})(?{native:first_n})", ExecutionMode::Full)?;
let count = Arc::new(AtomicUsize::new(0));
let c = count.clone();
let limit = 5;
re.register_native("first_n", move |_ctx| {
    if c.fetch_add(1, Ordering::Relaxed) >= limit {
        ExecResult::Steer(SteerResult::Abort)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
```

### Recipe 2: Timeout guard

Abort if the match takes too long, suitable for user-facing applications.

```rust
use std::time::Instant;
use std::sync::Arc;

let re = Regex::with_mode(r"(?<m>.+?)(?{native:timeout})", ExecutionMode::Full)?;
let start = Arc::new(Instant::now());
let t = start.clone();
let max_ms: u128 = 50;
re.register_native("timeout", move |_ctx| {
    if t.elapsed().as_millis() > max_ms {
        ExecResult::Steer(SteerResult::Abort)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
```

### Recipe 3: Deduplicated matches

Skip values you've already seen.

```rust
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

let re = Regex::with_mode(r"(?<word>\b\w+\b)(?{native:dedup})", ExecutionMode::Full)?;
let seen = Arc::new(Mutex::new(HashSet::new()));
let s = seen.clone();
re.register_native("dedup", move |ctx| {
    let word = ctx.named("word").unwrap_or("").to_lowercase();
    let mut set = s.lock().unwrap();
    if set.insert(word) {
        ExecResult::Steer(SteerResult::Continue)  // new word, keep it
    } else {
        ExecResult::Steer(SteerResult::Fail)      // duplicate, skip
    }
})?;
```

### Recipe 4: Accept-on-condition with fallback

Accept high-priority matches immediately, continue on lower-priority ones.

```rust
re.register_native("priority", |ctx| {
    let severity = ctx.named("severity").unwrap_or("info");
    match severity {
        "critical" | "fatal" => ExecResult::Steer(SteerResult::Accept),
        "error" | "warn"     => ExecResult::Steer(SteerResult::Continue),
        _                    => ExecResult::Steer(SteerResult::Fail),
    }
})?;
```

### Recipe 5: Skip-to-delimiter

Jump to the next section boundary instead of scanning through irrelevant data.

```rust
re.register_native("skip_section", |ctx| {
    let section_len: usize = ctx.named("len").unwrap_or("0").parse().unwrap_or(0);
    if section_len > 0 {
        ExecResult::Steer(SteerResult::Skip(section_len))
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
```

### Recipe 6: Combined budget (time + count + size)

A production-ready budget that limits by time, count, and bytes scanned.

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

let re = Regex::with_mode(r"(?<m>\S+)(?{native:budget})", ExecutionMode::Full)?;
let match_count = Arc::new(AtomicUsize::new(0));
let bytes_scanned = Arc::new(AtomicUsize::new(0));
let start = Arc::new(Instant::now());
let (mc, bs, st) = (match_count.clone(), bytes_scanned.clone(), start.clone());

re.register_native("budget", move |ctx| {
    // Time check
    if st.elapsed().as_millis() > 100 { return ExecResult::Steer(SteerResult::Abort); }
    // Count check
    if mc.fetch_add(1, Ordering::Relaxed) > 10_000 { return ExecResult::Steer(SteerResult::Abort); }
    // Bytes check
    bs.fetch_add(ctx.match_end - ctx.match_start, Ordering::Relaxed);
    if bs.load(Ordering::Relaxed) > 1_000_000 { return ExecResult::Steer(SteerResult::Abort); }

    ExecResult::Steer(SteerResult::Continue)
})?;
```

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
