# Safety Limits

Some regex patterns exhibit pathological backtracking behavior. The classic
example is `(a+)+b` matched against a string of `a`s with no trailing `b`:
the engine tries exponentially many ways to partition the `a`s among the
nested quantifiers before concluding there is no match. In a web server or
data pipeline, this becomes a denial-of-service vector -- a crafted input
can hang a thread indefinitely.

RGX provides configurable safety limits that cap the work the engine
will do per match attempt. When a limit is exceeded, the attempt fails
gracefully (returns no-match) rather than running forever. Separately, a
fixed [compile-time nesting limit](#compile-time-nesting-limit----parse-time-dos-protection)
protects the *compilation* of adversarially nested patterns -- see the
end of this chapter.

## `set_max_steps` -- opcode step budget

Every match attempt executes a sequence of VM opcodes: match a character,
try a branch, push a backtrack frame, and so on. `set_max_steps` caps the
total number of opcode dispatches per attempt:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(a+)+b")?;
re.set_max_steps(Some(10_000));

// On pathological input, the engine gives up instead of hanging
let result = re.find_first("aaaaaaaaaaaaaaaaaaaac");
assert!(result.is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### What counts as a step?

Each opcode dispatch increments the step counter by one. A simple literal
match, a character class check, a branch decision, and a group
open/close all count as individual steps. The cost is roughly proportional
to the work the engine actually does.

### Choosing a limit

The right limit depends on your patterns and input sizes:

| Scenario | Suggested limit |
|----------|----------------|
| Short patterns on short input (< 1 KB) | 10,000 - 100,000 |
| Complex patterns on moderate input (1 KB - 1 MB) | 1,000,000 |
| User-supplied patterns (untrusted) | 10,000 - 50,000 |
| Known-safe patterns (e.g., `\d+`) | `None` (unlimited) |

When in doubt, start with `Some(100_000)` and increase if legitimate matches
are being cut short.

### Removing the limit

Pass `None` to revert to the default (unlimited):

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
re.set_max_steps(Some(1000));

// Later, remove the limit
re.set_max_steps(None);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Interaction with the scanning loop

The step budget applies to each **match attempt** individually, not to the
entire scanning loop. When the engine starts scanning at position 0, it gets
a fresh budget. If that attempt exceeds the limit, it fails, and the engine
moves to position 1 with a new budget. This means a step limit will never
cause the engine to miss a match that a single non-pathological attempt
could have found.

## `set_max_backtrack_frames` -- backtrack stack depth

Backtracking-based regex engines maintain a stack of saved states so they
can undo choices when a branch fails. Deeply nested quantifiers and
alternations can produce enormous stacks. `set_max_backtrack_frames` caps
the depth:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(a|b)*c")?;
re.set_max_backtrack_frames(Some(5_000));

// If the backtrack stack grows beyond 5000 frames, the attempt fails
let result = re.find_first("aaabbbaaabbbaaabbbx");
assert!(result.is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When to use it

This limit is most useful when you want to specifically guard against stack
blowup (memory exhaustion) rather than CPU time. A pattern might execute
many opcodes without deep backtracking, or it might create deep backtracking
with few opcodes. The two limits are complementary:

- `max_steps` guards against **CPU time** abuse.
- `max_backtrack_frames` guards against **memory** abuse.

### Choosing a limit

| Scenario | Suggested limit |
|----------|----------------|
| General safety net | 10,000 |
| Memory-constrained environment | 1,000 - 5,000 |
| Trusted patterns only | `None` (unlimited) |

## `set_max_recursion_depth` -- recursion depth cap

RGX supports recursive patterns (like `(?R)` for matching balanced
parentheses). Each recursive invocation consumes stack space and processing
time. `set_max_recursion_depth` limits how deep the recursion can go:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\(([^()]*|(?R))*\)")?;
re.set_max_recursion_depth(Some(50));

// Deeply nested parentheses beyond depth 50 will fail to match
let shallow = "(((a)))";
assert!(re.is_match(shallow));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### The default

When no explicit limit is set, RGX uses a hard default of **1024** levels
of recursion. This is generous enough for virtually all legitimate patterns
but prevents runaway recursion from crashing the process.

Pass `None` to revert to this default:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?R)?")?;
re.set_max_recursion_depth(Some(10));

// Revert to the default (1024)
re.set_max_recursion_depth(None);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## `set_max_trail_entries` -- capture-trail length cap

Every capture-group write is recorded in a **capture trail** so the engine
can undo the write on backtrack. On pathological patterns with many nested
captures, the trail can grow far beyond what `set_max_backtrack_frames`
alone defends against — a single backtrack frame can carry an arbitrarily
long trail. `set_max_trail_entries` caps the trail length:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(.)*x")?;
re.set_max_trail_entries(Some(100));

// Pathological input without 'x': the trail would otherwise grow
// to one entry per input byte. The limit short-circuits it.
let result = re.find_first(&"a".repeat(10_000));
assert!(result.is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When to use it

`set_max_trail_entries` is the third axis of memory-bounded matching:

- `max_backtrack_frames` bounds the **number** of pending states.
- `max_trail_entries` bounds the **per-state** undo cost.
- Together they bound total trail memory across all live states.

A pattern can be safe under one but not the other. `(a|b)*c` on adversarial
input grows the frame count; `(.)*x` on adversarial input grows the trail
within a small frame count. For untrusted input you usually want both.

### The default

The default is `None` (unbounded). Set a limit explicitly if you accept
patterns or input from untrusted sources.

### Removing the limit

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(.)*x")?;
re.set_max_trail_entries(Some(1_000));

// Later, remove the limit
re.set_max_trail_entries(None);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Interior mutability with `AtomicU64`

You may have noticed that `set_max_steps`, `set_max_backtrack_frames`,
`set_max_recursion_depth`, and `set_max_trail_entries` all take `&self`,
not `&mut self`. This is by design: all four limits are stored as
`AtomicU64` values inside the VM, allowing them to be changed at any time
without requiring exclusive access to the `Regex`.

This means you can:

- **Share a `Regex` across threads** and adjust limits from any thread.
- **Change limits between calls** without recompiling the pattern.
- **Use `Arc<Regex>`** from `RegexCache` and still configure safety limits.

```rust
# use rgx_core::Regex;
use std::sync::Arc;
use std::thread;

let re = Arc::new(Regex::compile(r"(a+)+b")?);

// Thread 1: sets a tight limit for untrusted input
let re1 = re.clone();
let h1 = thread::spawn(move || {
    re1.set_max_steps(Some(10_000));
    re1.find_first("aaaaaac");
});

// Thread 2: uses a looser limit
let re2 = re.clone();
let h2 = thread::spawn(move || {
    re2.set_max_steps(Some(1_000_000));
    re2.find_first("aab");
});

h1.join().unwrap();
h2.join().unwrap();
# Ok::<(), Box<dyn std::error::Error>>(())
```

Note: because the limit is a single `AtomicU64` shared by all users of the
`Regex`, the last writer wins. If different threads need different limits on
the *same* regex, compile separate instances.

## Combining all four limits

For maximum protection against adversarial input, set all four. Each
defends a different resource axis: CPU time (`max_steps`), backtrack-state
count (`max_backtrack_frames`), recursion depth (`max_recursion_depth`),
and per-state undo memory (`max_trail_entries`).

```rust
# use rgx_core::Regex;
fn compile_safe(pattern: &str) -> Result<Regex, Box<dyn std::error::Error>> {
    let re = Regex::compile(pattern)?;
    re.set_max_steps(Some(50_000));
    re.set_max_backtrack_frames(Some(5_000));
    re.set_max_recursion_depth(Some(100));
    re.set_max_trail_entries(Some(10_000));
    Ok(re)
}

let re = compile_safe(r"(a+)+b")?;
// This is now safe to use with untrusted input
assert!(re.find_first("aaaaaaaaaaaaaaac").is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What happens when a limit is hit?

When any limit is exceeded, the *current match attempt* fails -- it returns
no-match for that starting position. The scanning loop may still try
subsequent starting positions, each with a fresh budget. This means:

1. Legitimate short matches at other positions are still found.
2. The pathological attempt is terminated quickly.
3. The engine never hangs, even on adversarial input.

There is no exception, panic, or error return -- the match simply does not
succeed at that position. If you need to distinguish "no match" from "budget
exhausted", check whether the pattern *should* have matched a simpler input;
RGX does not currently expose the exhaustion reason in the return value.

## Practical example: safe user-facing search

```rust
# use rgx_core::{Regex, RegexCache};
use std::sync::Arc;

let cache = RegexCache::new(256);

fn safe_search(cache: &RegexCache, user_pattern: &str, text: &str) -> Vec<String> {
    let re = match cache.get(user_pattern) {
        Ok(re) => re,
        Err(_) => return vec![],  // invalid pattern
    };
    // Apply safety limits for untrusted patterns
    re.set_max_steps(Some(50_000));
    re.set_max_backtrack_frames(Some(5_000));

    re.find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

let results = safe_search(&cache, r"\d+", "abc 123 def 456");
assert_eq!(results, vec!["123", "456"]);

// Pathological pattern safely returns empty
let results = safe_search(&cache, r"(a+)+b", "aaaaaaaaaaaac");
assert!(results.is_empty());
```

## Compile-time nesting limit -- parse-time DoS protection

The limits above all bound work done *per match attempt*. They do nothing
for a pattern that is dangerous to **compile** in the first place. A
deeply nested pattern such as

```text
(((((( ... (a)* ... )*)*)*)*)*)*     // N levels deep
```

drives the parser and compiler to recurse once per nesting level. Without
a ceiling, a sufficiently nested pattern would exhaust the thread stack
and **abort the whole process** -- the worst possible failure mode for a
library, and a trivial denial-of-service vector for any service that
compiles user-supplied regexes.

RGX therefore enforces a fixed **compile-time nesting limit**. A pattern
nested deeper than the limit fails to compile with a clean, deterministic
error instead of crashing:

```rust
# use rgx_core::Regex;
// Pathologically nested input is rejected, not crashed:
let mut pattern = String::from("a");
for _ in 0..5_000 { pattern = format!("({pattern})*"); }
// `Regex` is not `Debug`, so use a match rather than `unwrap_err()`.
let err = match Regex::compile(&pattern) {
    Err(e) => e,
    Ok(_) => unreachable!("a 5000-deep pattern must hit the nesting limit"),
};
assert!(err.to_string().contains("nesting too deep"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

The limit is **1000 levels** -- four times PCRE2's default
parenthesis-nesting limit of 250 (`PCRE2_CONFIG_PARENSLIMIT`) and the
Rust `regex` crate's default `nest_limit` of 250. No realistic pattern,
and no pattern in PCRE2's own conformance corpus, comes anywhere near it;
the bound exists purely so adversarial input cannot exhaust the stack. It
is the **compile-time analog** of the runtime limits above: `set_max_*`
protect *matching*, the nesting limit protects *compilation*.

Within the limit, RGX additionally runs the recursive parse/compile on a
growable stack (the same mechanism used for the JSON deserialization of
the parser's output), so a legitimately deep pattern compiles correctly
regardless of the calling thread's stack size rather than aborting.

This protection is always on and requires no configuration -- it is not a
`set_max_*` knob because, unlike match-time budgets, there is no
legitimate use case for compiling a pattern nested past the limit.

## Default values summary

| Limit | Default | Method | Phase |
|-------|---------|--------|-------|
| Max steps | Unlimited (`None` / 0) | `set_max_steps` | match |
| Max backtrack frames | Unlimited (`None` / 0) | `set_max_backtrack_frames` | match |
| Max recursion depth | 1024 | `set_max_recursion_depth` | match |
| Max trail entries | Unlimited (`None` / 0) | `set_max_trail_entries` | match |
| Nesting depth | 1000 (fixed) | automatic | compile |

The step and backtrack limits default to unlimited because most patterns are
not pathological, and imposing a limit on well-behaved patterns adds
complexity for no benefit. Set limits explicitly when accepting untrusted
patterns or defending against adversarial input.
