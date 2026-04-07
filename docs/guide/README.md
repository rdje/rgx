# The RGX Guide

A practical guide to building with rgx — the programmable regex engine.

## What if your regex could do this?

**Validate an IP address while matching it:**
```rust
let re = Regex::with_mode(
    r"(?<a>\d{1,3})\.(?<b>\d{1,3})\.(?<c>\d{1,3})\.(?<d>\d{1,3})(?{native:valid_ip})",
    ExecutionMode::Full,
)?;
re.register_native("valid_ip", |ctx| {
    let valid = ["a","b","c","d"].iter().all(|g|
        ctx.named(g).and_then(|s| s.parse::<u32>().ok()).map_or(false, |n| n <= 255)
    );
    if valid { ExecResult::Success } else { ExecResult::Failure }
})?;
assert!(re.is_match("192.168.1.1"));
assert!(!re.is_match("999.999.999.999")); // not just structurally wrong — rejected during matching
```

**Build a tokenizer in one line:**
```rust
let lexer = Regex::compile(r"(?<num>\d+)|(?<id>[a-zA-Z_]\w*)|(?<op>[+\-*/=])|(?<str>\"[^\"]*\")")?;
for token in lexer.find_all(source) {
    let kind = match token.matched_branch_number {
        Some(1) => "NUMBER", Some(2) => "IDENT", Some(3) => "OP", Some(4) => "STRING", _ => "?",
    };
    // Branch number tells you the token type — no capture group tricks needed
}
```

**Scan a log file and alert on errors — one pattern, one line:**
```rust
re.scan_file_lines("app.log")?; // callbacks fire on every match
```

**Query a database mid-match:**
```rust
match re.find_first_suspendable("user: alice@example.com") {
    MatchOutcome::Suspended(continuation) => {
        // Engine paused — go check your database
        let allowed = db.check_user(&continuation.pending_context).await;
        let result = re.resume(continuation, if allowed { ExecResult::Success } else { ExecResult::Failure });
    }
    MatchOutcome::Completed(result) => { /* done */ }
}
```

**Debug why a pattern isn't matching:**
```rust
re.on_event(|event| println!("{:?}", event))?;
// MatchAttemptStarted { position: 0 }
// BacktrackOccurred { position: 5, stack_depth: 3 }
// MatchAttemptCompleted { position: 0, matched: false }
// ...now you can SEE what the engine is doing
```

All of this with **one engine, one API, one pattern language**. No glue code. No post-processing. No leaving the regex world.

## Who this guide is for

You know what regular expressions are. You've used them to find patterns in text. Maybe you've wished they could do more — validate data against a database, call your application's functions, or process files reactively. That's what rgx does.

This guide doesn't assume you know rgx's internals. Each chapter introduces one concept, explains why it matters, shows you how to use it, and gives you enough examples to build real things.

## How to read this guide

Start with **Chapter 0** if you're new to rgx. Then pick any chapter that solves your problem — they're designed to be read independently, though each one builds on the ideas before it.

If you want to see what's possible first, jump to [Chapter 7: Real-World Patterns](07-real-world.md) — it has complete working examples for a log monitor, tokenizer, data validation pipeline, config file parser, and a WAF rule engine.

## Chapters

### Part I — Foundations
- [Chapter 0: Your First Match](00-first-match.md) — The basics: compile a pattern, find matches, understand the result
- [Chapter 1: Passing Data In and Out](01-data-exchange.md) — Host variables, result values, and branch identification

### Part II — Code Inside Patterns
- [Chapter 2: Predicate Callbacks](02-predicate-callbacks.md) — Run code during matching, validate on the fly, four language options
- [Chapter 3: Steering the Match](03-match-steering.md) — Accept, reject, skip, or abort from a callback

### Part III — Observability and I/O
- [Chapter 4: Watching the Engine](04-structured-events.md) — Debug, profile, and monitor matching with zero-overhead events
- [Chapter 5: Async Callbacks](05-async-io.md) — Suspend a match, do I/O, resume — works with any async runtime
- [Chapter 6: Working with Files](06-file-matching.md) — Match against files, scan line by line, trigger callbacks per match

### Part IV — Putting It Together
- [Chapter 7: Real-World Patterns](07-real-world.md) — Complete examples: log monitor, tokenizer, data pipeline, config parser, WAF rules

### Reference
- [Quick Reference](quick-reference.md) — One-page cheat sheet for common tasks
- [Execution Modes](execution-modes.md) — Pure, Safe, Full — when to use each
- [Context Reference](context-reference.md) — Everything available inside a callback

## What rgx can do that others can't

| Scenario | Traditional regex | rgx |
|----------|------------------|-----|
| Validate IP octets | Match structure, validate after | Validate DURING matching — invalid IPs aren't matches |
| Filter logs by severity | Two patterns or post-filter | One pattern + host variable sets the threshold |
| Build a tokenizer | Capture groups + check which is non-empty | Branch number tells you the token type directly |
| Check a database mid-match | Match first, then query | Engine suspends, you query, engine resumes |
| Process a log file | Read file, split lines, match each | `re.scan_file_lines("app.log")` — one call |
| Debug a failing pattern | Stare at the pattern and guess | Watch every step with `re.on_event(...)` |
| Limit match count | Count externally, stop when done | `SteerResult::Abort` from a callback |
| Apply business rules | Match then validate | Rules run inside the match — invalid data backtracks |
| Transform matches | Match, extract, transform, reassemble | `replace_all_with_code` — transform inline |
| Use runtime config | Build pattern dynamically | Set variables, same compiled pattern |
