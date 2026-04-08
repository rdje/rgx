# Quick Reference

One-liner solutions for common rgx tasks. For full explanations, see the chapter links.

## Compiling

```rust
use rgx_core::*;

// Simple compilation
let re = Regex::compile(r"\d+")?;

// With execution mode (for code blocks)
let re = Regex::with_mode(r"(?{lua:return true})", ExecutionMode::Safe)?;

// Fluent builder with flag overrides
let re = RegexBuilder::new(r"hello world")
    .case_insensitive()
    .multi_line()
    .dot_matches_new_line()
    .build()?;

// Escape user input for safe literal matching
let safe = escape("price is $3.50");  // "price is \$3\.50"
```

## Finding matches

```rust
// Ergonomic match (borrows the text)
let m = re.find("hello 42 world")?;
m.as_str()    // "42"
m.start()     // 6
m.end()       // 8
m.range()     // 6..8

// Raw match (returns MatchResult with groups)
let m = re.find_first("hello 42")?;         // Option<MatchResult>
let all = re.find_all("42 and 99");          // Vec<MatchResult>

// Position-aware matching (start from byte offset)
let m = re.find_first_at("aaa 42 bbb 99", 7)?;  // finds "99"

// Boolean test
re.is_match("hello 42")                     // true

// Shortest match (end position only — faster)
re.shortest_match("hello 42")               // Some(8)
```

## Lazy iterators (zero allocation)

```rust
// Iterate matches without collecting into Vec
for m in re.find_iter("a1 b2 c3") {
    println!("{}", m.as_str());
}

// Iterate with capture groups
for caps in re.captures_iter("x=1 y=2") {
    println!("{}={}", &caps[1], &caps[2]);
}

// Lazy split
for part in re.split_iter("one,two,three") {
    println!("{part}");
}
```

## Capture groups

```rust
// Ergonomic captures with index and name access
let caps = re.captures("2025-03-15")?;
&caps[0]              // "2025-03-15" (full match)
&caps[1]              // "2025"
&caps["year"]         // "2025" (named group)
caps.get(2)           // Option<Match>
caps.name("month")    // Option<Match>

// Expand a template
let mut out = String::new();
caps.expand("$month/$year", &mut out);

// Zero-allocation capture loop
let mut locs = re.capture_locations();
if let Some(m) = re.captures_read("2025-03-15", &mut locs) {
    locs.get(1)  // Some((0, 4))
}

// Metadata
re.captures_len()               // number of groups (including group 0)
re.capture_names().collect()    // [None, Some("year"), Some("month"), ...]
re.as_str()                     // original pattern string
```

## Replace

```rust
// Template interpolation ($1, $name, ${name}, $&, $$)
re.replace("hello world", "$2 $1")           // Cow::Owned("world hello")
re.replace_all("a1 b2", "[$&]")              // "[a1] [b2]"
re.replacen("a1 b2 c3", 2, "X")             // "X X c3"

// Closure-based replacement
re.replace_all("hello", |caps: &Captures| {
    caps[0].to_uppercase()
})

// Literal replacement (no $1 interpolation)
re.replace("price 42", NoExpand("$$$"))       // "price $$$"

// Returns Cow::Borrowed when no match (zero allocation)
let result = re.replace("no match here", "X");  // Cow::Borrowed
```

## Split

```rust
re.split("one,two,three")                    // vec!["one", "two", "three"]
re.splitn("a,b,c,d", 3)                     // vec!["a", "b", "c,d"]

// Lazy versions (no Vec allocation)
re.split_iter("a,b,c")
re.splitn_iter("a,b,c,d", 3)
```

## Multi-pattern matching (RegexSet)

```rust
let set = RegexSet::new(&[r"^/api/", r"^/static/", r"^/health$"])?;
let matches = set.matches("/api/users/123");
matches.matched(0)    // true (first pattern)
matches.matched_any()
matches.matched_all()
for idx in matches.iter() { /* matched pattern indices */ }
```

## Compilation cache

```rust
let cache = RegexCache::new(128);             // LRU, thread-safe
let re = cache.get(r"\d+")?;                  // Arc<Regex>, compiles once
let re2 = cache.get(r"\d+")?;                 // instant — same Arc
```

## Byte matching (no UTF-8 required)

```rust
use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\d+")?;
let m = re.find(b"\xFF\xFE123\xFF")?;
m.as_bytes()  // b"123"
```

## Safety limits (DoS protection)

```rust
re.set_max_steps(Some(10_000));               // abort after N opcodes
re.set_max_backtrack_frames(Some(1_000));     // cap backtrack stack
re.set_max_recursion_depth(Some(50));         // cap recursion depth
```

## Match semantics

```rust
re.set_match_semantics(MatchSemantics::LeftmostFirst);    // default (PCRE2)
re.set_match_semantics(MatchSemantics::LeftmostLongest);  // POSIX
```

## Partial matching (streaming)

```rust
match re.find_first_partial("hello wor") {
    PartialMatchResult::Full(m) => println!("matched"),
    PartialMatchResult::Partial(pos) => println!("need more input from {pos}"),
    PartialMatchResult::NoMatch => println!("impossible"),
}
```

## File matching

```rust
re.match_file("data.txt")?                    // Vec<MatchResult>
re.match_file_lines("data.txt")?              // Vec<FileMatch> (with line numbers)
re.scan_file("data.txt")?                     // usize (match count)

// Live file watching (kqueue/inotify, zero idle CPU)
let handle = re.tail_file("app.log", TailOptions::default(), |fm| {
    eprintln!("line {}: {}", fm.line_number, fm.line);
});
handle.stop();
```

## Host variables

```rust
re.set_variable("key", "value")?;
```

Access from callbacks:

| Language | Syntax |
|----------|--------|
| Native | `ctx.variable("key")` -> `Option<String>` |
| Lua | `vars.key` |
| JS | `vars.key` |
| Rhai | `vars["key"]` |

## Registering callbacks

```rust
// Native callback
re.register_native("name", |ctx| ExecResult::Success)?;

// WASM module
re.register_wasm_module("module_name", wasm_bytes)?;

// Inline (no registration needed)
// Lua:  (?{lua:return true})
// JS:   (?{js:return true;})
// Rhai: (?{rhai:true})
```

## Steering from inline languages

```lua
-- Lua
rgx.steer_continue()
rgx.steer_fail()
rgx.steer_accept()
rgx.steer_skip(4)
rgx.steer_abort()
```

```javascript
// JavaScript
rgx.steerContinue()
rgx.steerFail()
rgx.steerAccept()
rgx.steerSkip(4)
rgx.steerAbort()
```

```rust
// Rhai
steer_continue()
steer_fail()
steer_accept()
steer_skip(4)
steer_abort()
```

## Emitting values from inline languages

```lua
-- Lua
rgx.emit_numeric(42.0)
rgx.emit_replacement("REDACTED")
```

```javascript
// JavaScript
rgx.emit_numeric(42.0)
rgx.emit_replacement("REDACTED")
```

```rust
// Rhai
emit_numeric(42.0)
emit_replacement("REDACTED")
```

## Events

```rust
re.on_event(|event| {
    match event {
        MatchEvent::MatchAttemptStarted { position } => { /* ... */ }
        MatchEvent::MatchAttemptCompleted { position, matched } => { /* ... */ }
        MatchEvent::BranchEntered { branch, position } => { /* ... */ }
        MatchEvent::CaptureCompleted { group, start, end } => { /* ... */ }
        MatchEvent::BacktrackOccurred { position, stack_depth } => { /* ... */ }
        MatchEvent::CodeBlockEvaluated { language, succeeded, position } => { /* ... */ }
    }
})?;
```

## Async matching

```rust
let mut outcome = re.find_first_suspendable(text);
loop {
    match outcome {
        MatchOutcome::Completed(result) => break,
        MatchOutcome::Suspended(cont) => {
            outcome = re.resume(*cont, ExecResult::Success);
        }
    }
}
```

## Execution modes at a glance

| Mode | Inline code | Native callbacks | Use case |
|------|-------------|------------------|----------|
| `Pure` | No | No | Maximum performance, structural matching only |
| `Safe` | Yes (Lua/JS/Rhai/WASM) | No | Untrusted or semi-trusted patterns |
| `Full` | Yes | Yes | Full power, patterns you control |

## Special escapes

| Escape | Meaning |
|--------|---------|
| `\X` | Extended grapheme cluster (base + combining marks) |
| `\R` | Newline sequence (any platform) |
| `\N` | Non-newline (same as `.` without dotall) |
| `\K` | Reset match start |
| `\G` | Previous match end anchor |

## Context available inside callbacks

| Field | Native | Lua | JS | Rhai |
|-------|--------|-----|----|----- |
| Full text | `ctx.text` | `text` | `text` | `text` |
| Position | `ctx.position` | `pos` | `pos` | `pos` |
| Match start | `ctx.match_start` | `match_start` | `match_start` | `match_start` |
| Match end | `ctx.match_end` | `match_end` | `match_end` | `match_end` |
| Group N | `ctx.group(n)` | `arg[n]` | `arg[n]` | `arg[n]` |
| Named group | `ctx.named("x")` | `named.x` | `named.x` | `named["x"]` |
| Variable | `ctx.variable("x")` | `vars.x` | `vars.x` | `vars["x"]` |
| Branch # | `ctx.matched_branch_number()` | `branch_number` | `branch_number` | `branch_number` |
