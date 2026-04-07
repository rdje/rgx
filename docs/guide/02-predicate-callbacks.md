# Chapter 2: Predicate Callbacks

In [Chapter 1](01-data-exchange.md) you learned how to push data into a pattern and pull results out. But those examples assumed the validation logic was simple enough to hard-code. In the real world, "does this match?" often depends on things no regex can express: Is this IP address in our allow-list? Is this user old enough? Does this product code follow our naming convention?

This chapter shows you how to run your own code *during* matching, so the engine makes smarter decisions without ever returning a false positive for you to filter out later.

## Why run code during matching?

### The traditional approach

Imagine you're scanning log files for IP addresses, but you only care about addresses in the `10.x.x.x` private range. With a traditional regex engine, you'd match all IP addresses first, then filter:

```rust
// Traditional: match everything, filter later
let re = Regex::compile(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b")?;
let all_ips = re.find_all(log_text);

// Now check each one manually
let private_ips: Vec<_> = all_ips
    .iter()
    .filter(|m| {
        let ip = &log_text[m.start..m.end];
        ip.starts_with("10.")
    })
    .collect();
```

Two passes: one to match, one to validate. For a 10GB log file, that means building a potentially enormous list of all IP addresses before discarding most of them.

### The rgx approach

With rgx, the validation happens *inside* the match. The engine only reports matches that pass your check:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"\b(?<ip>\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b(?{native:is_private})",
    ExecutionMode::Full,
)?;

re.register_native("is_private", |ctx| {
    let ip = ctx.named("ip").unwrap_or("");
    if ip.starts_with("10.") {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

// One pass. Only private IPs come back.
let private_ips = re.find_all(log_text);
```

One pass. No intermediate allocation. The engine checks each candidate as it finds it, and discards non-private addresses immediately.

## Zero-width checkpoints

A predicate callback is what rgx calls a **zero-width checkpoint**. It's a point in the pattern where execution pauses, your code runs, and the engine gets a yes-or-no answer. If the answer is yes, matching continues from exactly where it left off. If no, the engine backtracks as if the pattern itself had failed to match.

Think of it like a security checkpoint at an airport. The conveyor belt (the regex engine) moves passengers (characters) through. At certain points, a guard (your callback) inspects the current state. If everything checks out, the passenger continues. If not, they're sent back. The checkpoint itself doesn't move anyone forward or backward on the belt -- it's *zero-width*.

In pattern syntax, a checkpoint looks like this:

```
(?{language:code})
```

Where `language` is one of `native`, `lua`, `js`, `rhai`, or `wasm`, and `code` is either the inline source or a registered callback name. The checkpoint always appears *after* the subpattern it validates:

```
pattern-to-match(?{native:validate_it})
```

This reads naturally: "Match this pattern, then check it."

## Native Rust callbacks

Native callbacks are the most powerful and performant option. They're plain Rust closures that receive an `ExecContext` and return an `ExecResult`.

### Scenario 1: IP address validation

Let's build on the IP example. Instead of just checking the prefix, let's validate that each octet is 0-255:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"\b(?<ip>\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b(?{native:valid_ip})",
    ExecutionMode::Full,
)?;

re.register_native("valid_ip", |ctx| {
    let ip = ctx.named("ip").unwrap_or("");
    let valid = ip.split('.').all(|octet| {
        octet.parse::<u16>().map_or(false, |n| n <= 255)
    });
    if valid {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

// "999.999.999.999" matches the digits-and-dots pattern but fails the callback
assert!(!re.is_match("Server at 999.999.999.999"));
assert!(re.is_match("Server at 192.168.1.1"));
```

Without the callback, `999.999.999.999` would match the regex `\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}`. The callback catches what the regex cannot.

### Scenario 2: Age verification

You're processing form submissions and need to verify that ages are within a reasonable range:

```rust
let re = Regex::with_mode(
    r"(?<age>\d{1,3})(?{native:check_age})",
    ExecutionMode::Full,
)?;

re.register_native("check_age", |ctx| {
    let age: u32 = ctx.named("age")
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);

    if age >= 18 && age <= 120 {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("Age: 25"));
assert!(!re.is_match("Age: 12"));   // too young
assert!(!re.is_match("Age: 200"));  // not realistic
```

### Scenario 3: Business rule -- product code validation

Your company's product codes follow a convention: they start with a department prefix, then a dash, then a number. But the valid prefixes change per quarter, and they're stored in a configuration file. A pure regex can't know the current quarter's prefixes.

```rust
use std::collections::HashSet;
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"\b(?<code>[A-Z]{2,4})-\d{4}\b(?{native:valid_dept})",
    ExecutionMode::Full,
)?;

// In real code, this comes from a config file or database
let valid_depts: HashSet<String> = ["ENG", "MKT", "OPS", "FIN"]
    .iter().map(|s| s.to_string()).collect();

re.register_native("valid_dept", move |ctx| {
    let code = ctx.named("code").unwrap_or("");
    if valid_depts.contains(code) {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("Order ENG-1234 confirmed"));
assert!(re.is_match("Budget FIN-9001 approved"));
assert!(!re.is_match("Code ZZZ-0000 unknown"));  // ZZZ is not a valid dept
```

The closure *captures* the `valid_depts` set. When the set changes next quarter, you compile a new regex with an updated set. The pattern string never changes.

### Scenario 4: Cross-field validation

Sometimes a match is valid only if two captured groups have a specific relationship. For example, a date range where the start must precede the end:

```rust
let re = Regex::with_mode(
    r"(?<start>\d{4}-\d{2}-\d{2})\s+to\s+(?<end>\d{4}-\d{2}-\d{2})(?{native:ordered})",
    ExecutionMode::Full,
)?;

re.register_native("ordered", |ctx| {
    let start = ctx.named("start").unwrap_or("9999-99-99");
    let end = ctx.named("end").unwrap_or("0000-00-00");
    // ISO date strings sort lexicographically
    if start <= end {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("2026-01-01 to 2026-12-31"));
assert!(!re.is_match("2026-12-31 to 2026-01-01"));  // backwards
```

## Lua callbacks

Lua is embedded directly in the pattern string. No registration step -- just write Lua inside the code block. This is great for quick validations that don't need Rust's type system or access to external data.

### Example 1: Date validation

```rust
let re = Regex::with_mode(
    r#"(\d{4})-(\d{2})-(\d{2})(?{lua:
        local month = tonumber(arg[2])
        local day = tonumber(arg[3])
        return month >= 1 and month <= 12 and day >= 1 and day <= 31
    })"#,
    ExecutionMode::Safe,
)?;

assert!(re.is_match("Date: 2026-04-15"));
assert!(!re.is_match("Date: 2026-13-01"));  // month 13
assert!(!re.is_match("Date: 2026-04-32"));  // day 32
```

The Lua code receives capture groups in the `arg` table (0-indexed: `arg[0]` is the full match, `arg[1]` is group 1, etc.). It returns `true` or `false`.

### Example 2: Password strength check

```rust
let re = Regex::with_mode(
    r#"(?<pw>.{8,})(?{lua:
        local pw = named.pw
        local has_upper = string.find(pw, "[A-Z]")
        local has_lower = string.find(pw, "[a-z]")
        local has_digit = string.find(pw, "[0-9]")
        return has_upper and has_lower and has_digit
    })"#,
    ExecutionMode::Safe,
)?;

assert!(re.is_match("MyP4ssword"));
assert!(!re.is_match("alllowercase"));
assert!(!re.is_match("SHORT1A"));  // only 7 chars
```

Named captures are available through the `named` table. This pattern requires at least 8 characters containing uppercase, lowercase, and a digit.

### Example 3: Using host variables in Lua

```rust
let re = Regex::with_mode(
    r#"(?<amount>\d+)(?{lua:
        local amount = tonumber(named.amount)
        local limit = tonumber(vars.spending_limit) or 1000
        return amount <= limit
    })"#,
    ExecutionMode::Safe,
)?;

re.set_variable("spending_limit", "500")?;
assert!(re.is_match("Charge: 200"));
assert!(!re.is_match("Charge: 750"));

re.set_variable("spending_limit", "1000")?;
assert!(re.is_match("Charge: 750"));
```

## JavaScript callbacks

JavaScript code blocks use `(?{js:...})`. The API mirrors Lua: `arg` for captures, `named` for named captures, `vars` for host variables.

### Example 1: Email domain validation

```rust
let re = Regex::with_mode(
    r#"[\w.+-]+@(?<domain>[\w.-]+)(?{js:
        var parts = named.domain.split(".");
        var tld = parts[parts.length - 1];
        return parts.length >= 2 && tld.length >= 2;
    })"#,
    ExecutionMode::Safe,
)?;

assert!(re.is_match("user@example.com"));
assert!(re.is_match("user@sub.example.co.uk"));
assert!(!re.is_match("user@localhost"));  // single label, no TLD
```

### Example 2: JSON-like key validation

You're scanning config files and want to match key-value pairs, but only keys that follow a specific naming convention:

```rust
let re = Regex::with_mode(
    r#"(?<key>[a-zA-Z_][\w.]*)(?{js:
        var key = named.key;
        // Must be dot-separated segments, each starting with lowercase
        var segments = key.split(".");
        return segments.every(function(s) {
            return s.length > 0 && s[0] === s[0].toLowerCase() && s[0] !== s[0].toUpperCase();
        });
    })\s*=\s*"[^"]*""#,
    ExecutionMode::Safe,
)?;

assert!(re.is_match(r#"database.host = "localhost""#));
assert!(re.is_match(r#"app.server.port = "8080""#));
assert!(!re.is_match(r#"Database.host = "localhost""#));  // uppercase segment
```

### Example 3: Conditional matching with variables

```rust
let re = Regex::with_mode(
    r#"(?<level>ERROR|WARN|INFO|DEBUG)(?{js:
        var dominated = {
            "prod":  ["DEBUG", "INFO"],
            "staging": ["DEBUG"],
            "dev": []
        };
        var dominated_list = dominated[vars.env] || [];
        return !dominated_list.includes(named.level);
    })"#,
    ExecutionMode::Safe,
)?;

re.set_variable("env", "prod")?;
assert!(re.is_match("ERROR: something broke"));
assert!(re.is_match("WARN: disk space low"));
assert!(!re.is_match("INFO: startup complete"));   // suppressed in prod
assert!(!re.is_match("DEBUG: trace enabled"));      // suppressed in prod

re.set_variable("env", "dev")?;
assert!(re.is_match("DEBUG: trace enabled"));       // everything shows in dev
```

## Rhai callbacks

Rhai is a pure-Rust scripting language. No C dependencies, no FFI. It has a syntax similar to Rust, making it feel natural in a Rust project.

### Example 1: Hex color validation

```rust
let re = Regex::with_mode(
    r#"#(?<hex>[0-9a-fA-F]{6}|[0-9a-fA-F]{3})(?{rhai:
        let h = named["hex"];
        // Reject "pure" colors (all same digit)
        let first = h[0];
        let all_same = h.to_chars().all(|c| c == first);
        !all_same
    })"#,
    ExecutionMode::Safe,
)?;

assert!(re.is_match("color: #ff8800"));
assert!(re.is_match("color: #abc"));
assert!(!re.is_match("color: #ffffff"));  // all same digit 'f'
assert!(!re.is_match("color: #000"));     // all same digit '0'
```

### Example 2: Semantic version constraint

```rust
let re = Regex::with_mode(
    r#"(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)(?{rhai:
        let major = parse_int(named["major"]);
        let minor = parse_int(named["minor"]);
        let min_major = parse_int(vars["min_major"]);
        let min_minor = parse_int(vars["min_minor"]);
        if major > min_major { return true; }
        if major == min_major && minor >= min_minor { return true; }
        false
    })"#,
    ExecutionMode::Safe,
)?;

re.set_variable("min_major", "2")?;
re.set_variable("min_minor", "5")?;

assert!(re.is_match("version 3.0.0"));
assert!(re.is_match("version 2.5.1"));
assert!(!re.is_match("version 2.4.9"));
assert!(!re.is_match("version 1.9.0"));
```

### Example 3: Length-based filtering with Rhai

```rust
let re = Regex::with_mode(
    r#"(?<word>[a-zA-Z]+)(?{rhai:
        let w = named["word"];
        let min = parse_int(vars["min_len"]);
        let max = parse_int(vars["max_len"]);
        w.len() >= min && w.len() <= max
    })"#,
    ExecutionMode::Safe,
)?;

re.set_variable("min_len", "4")?;
re.set_variable("max_len", "8")?;

let text = "I am a developer who writes code";
let matches = re.find_all(text);
// Only words with 4-8 characters match
for m in &matches {
    let word = &text[m.start..m.end];
    assert!(word.len() >= 4 && word.len() <= 8);
}
```

## WASM callbacks

WebAssembly modules can be registered and called from patterns using `(?{wasm:module:function})`. WASM callbacks are portable, sandboxed, and useful when you want to ship validation logic separately from the host application.

```rust
let re = Regex::with_mode(
    r"\d+(?{wasm:validator:check})",
    ExecutionMode::Safe,
)?;

let wasm_bytes = std::fs::read("validator.wasm")?;
re.register_wasm_module("validator", wasm_bytes)?;

assert!(re.is_match("42"));
```

WASM modules communicate via the `rgx` import namespace. They can call `rgx.emit_numeric(f64)` and `rgx.emit_replacement(ptr, len)` to surface values, and return `1` for success or `0` for failure.

WASM is the right choice when you need to distribute validation logic to untrusted environments or share it across languages. For most use cases, native callbacks or an embedded language will be simpler.

## The execution context: what callbacks can see

Every callback -- regardless of language -- receives an execution context. Here's what's available:

| Field | Native Rust | Lua | JavaScript | Rhai | Description |
|-------|-------------|-----|------------|------|-------------|
| Full input text | `ctx.text` | `text` | `text` | `text` | The entire string being matched |
| Current position | `ctx.position` | `pos` | `pos` | `pos` | Byte offset of the engine's cursor |
| Match start | `ctx.match_start` | `match_start` | `match_start` | `match_start` | Byte offset where the current match attempt began |
| Match end | `ctx.match_end` | `match_end` | `match_end` | `match_end` | Byte offset where the engine is now (end of matched portion so far) |
| Match length | `ctx.match_length()` | `match_length` | `match_length` | `match_length` | `match_end - match_start` |
| Capture group N | `ctx.group(n)` | `arg[n]` | `arg[n]` | `arg[n]` | Text captured by group N (0 = full match) |
| Named capture | `ctx.named("name")` | `named.name` | `named.name` | `named["name"]` | Text captured by named group |
| Host variable | `ctx.variable("key")` | `vars.key` | `vars.key` | `vars["key"]` | Value set by `re.set_variable(...)` |
| Branch number | `ctx.matched_branch_number()` | `branch_number` | `branch_number` | `branch_number` | 1-based index of which top-level alternative matched |

### Example: using position information

```rust
re.register_native("at_line_start", |ctx| {
    // Check if the match starts at column 0 (beginning of a line)
    let before = &ctx.text[..ctx.match_start];
    if ctx.match_start == 0 || before.ends_with('\n') {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;
```

### Example: using the full text for lookaround logic

```rust
re.register_native("not_inside_quotes", |ctx| {
    // Count how many unescaped quotes appear before our position
    let before = &ctx.text[..ctx.match_start];
    let quote_count = before.chars().filter(|&c| c == '"').count();
    // If even number of quotes, we're outside a quoted string
    if quote_count % 2 == 0 {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;
```

## Execution modes

Every rgx regex has an execution mode that controls which callback types are allowed. This isn't a security nuisance -- it's a performance feature. If you don't use callbacks, you don't pay for them.

### Pure mode

```rust
let re = Regex::with_mode(r"\d+", ExecutionMode::Pure)?;
```

No code blocks at all. This is the fastest mode. The engine doesn't allocate callback infrastructure, doesn't check for code blocks during matching, and runs on a minimal code path. Use this for patterns that are purely structural.

If you accidentally include a code block in a Pure-mode pattern, it will be ignored.

### Safe mode

```rust
let re = Regex::with_mode(
    r#"\d+(?{lua:return tonumber(arg[0]) > 0})"#,
    ExecutionMode::Safe,
)?;
```

Enables inline code blocks in sandboxed languages: Lua, JavaScript, Rhai, and WASM. These run in isolated environments with no access to the filesystem, network, or system calls. Safe mode is the right choice when patterns come from semi-trusted sources (like configuration files) or when you want the convenience of inline code without worrying about what it can do.

### Full mode

```rust
let re = Regex::with_mode(
    r"\d+(?{native:validate})",
    ExecutionMode::Full,
)?;
```

Enables everything Safe mode offers, plus native Rust callbacks. Native callbacks are full-power Rust closures -- they can access files, make network calls, use any crate. Full mode is for patterns you control, running code you wrote.

The modes are explained in depth in the [Execution Modes reference](execution-modes.md).

## Backtracking behavior

Here's a subtlety that matters: when a callback runs, the engine might later *backtrack* past it and try a different path. If that different path reaches the same (or a different) callback, the callback runs again.

### What this means in practice

Consider this pattern matching a number followed by a validation:

```rust
let re = Regex::with_mode(
    r"(\d+)(?{native:check})\s",
    ExecutionMode::Full,
)?;
```

Against the text `"123 "`, the engine might:

1. Try matching `\d+` greedily, capturing `"123"`
2. Run `check` -- suppose it returns `Success`
3. Try matching `\s` -- succeeds on the space
4. Overall match succeeds

But consider `"123x "`:

1. Try matching `\d+` greedily, capturing `"123"`
2. Run `check` -- returns `Success`
3. Try matching `\s` on `"x"` -- fails
4. Engine backtracks: `\d+` now tries `"12"`
5. Run `check` again with `"12"`
6. Try matching `\s` on `"3"` -- fails
7. Engine backtracks again: `\d+` now tries `"1"`
8. Run `check` again with `"1"`
9. ... and so on

**Your callback might run multiple times for the same overall match attempt.** This is normal and correct -- the engine is exploring different paths. Each time it runs, the callback sees the current state of the captures for *that particular path*.

### Keep callbacks pure

Because of backtracking, callbacks should be **pure functions** -- given the same input, they produce the same output with no side effects. Specifically:

- Do not increment counters inside callbacks (you'll count wrong due to backtracking)
- Do not write to files or send network requests (they'll fire on paths that don't lead to a match)
- Do not modify shared mutable state

If you need to observe every callback invocation (including backtracked ones), use the event observer system described in [Chapter 4](04-structured-events.md).

## Common patterns and idioms

### Pattern: optional validation

Put the callback inside an optional group so the pattern still matches when the callback's precondition isn't met:

```rust
// Match numbers, but if there's a unit suffix, validate it
let re = Regex::with_mode(
    r#"\d+(?:(?<unit>[a-z]+)(?{lua:
        local valid = {kg=true, lb=true, oz=true, g=true}
        return valid[named.unit] ~= nil
    }))?"#,
    ExecutionMode::Safe,
)?;

assert!(re.is_match("100kg"));    // valid unit
assert!(re.is_match("100lb"));    // valid unit
assert!(re.is_match("100"));      // no unit, still matches
assert!(!re.is_match("100xyz"));  // invalid unit
```

### Pattern: multi-field extraction with validation

Combine named groups with a single callback that validates all fields together:

```rust
let re = Regex::with_mode(
    r"(?<h>\d{1,2}):(?<m>\d{2}):(?<s>\d{2})(?{native:valid_time})",
    ExecutionMode::Full,
)?;

re.register_native("valid_time", |ctx| {
    let h: u32 = ctx.named("h").unwrap_or("99").parse().unwrap_or(99);
    let m: u32 = ctx.named("m").unwrap_or("99").parse().unwrap_or(99);
    let s: u32 = ctx.named("s").unwrap_or("99").parse().unwrap_or(99);

    if h < 24 && m < 60 && s < 60 {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("Time: 23:59:59"));
assert!(re.is_match("Time: 00:00:00"));
assert!(!re.is_match("Time: 25:00:00"));
assert!(!re.is_match("Time: 12:60:00"));
```

### Pattern: callback that returns a value

Callbacks aren't limited to pass/fail. They can compute and return values:

```rust
let re = Regex::with_mode(
    r"(?<c>\d+)\s*(?<unit>F|C)(?{native:to_kelvin})",
    ExecutionMode::Full,
)?;

re.register_native("to_kelvin", |ctx| {
    let temp: f64 = ctx.named("c").unwrap_or("0").parse().unwrap_or(0.0);
    let unit = ctx.named("unit").unwrap_or("C");
    let kelvin = match unit {
        "C" => temp + 273.15,
        "F" => (temp - 32.0) * 5.0 / 9.0 + 273.15,
        _ => temp,
    };
    ExecResult::Numeric(kelvin)
})?;

let k = re.find_first_numeric_with_code("Water boils at 100 C");
assert!((k.unwrap() - 373.15).abs() < 0.01);
```

### Pattern: shared state via variables

When you need to change callback behavior between matches without recompiling:

```rust
let re = Regex::with_mode(
    r"(?<val>\d+)(?{native:threshold})",
    ExecutionMode::Full,
)?;

re.register_native("threshold", |ctx| {
    let val: i64 = ctx.named("val").unwrap_or("0").parse().unwrap_or(0);
    let thresh: i64 = ctx.variable("max")
        .unwrap_or_else(|| "100".to_string())
        .parse()
        .unwrap_or(100);
    if val <= thresh {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

re.set_variable("max", "50")?;
assert!(re.is_match("Value: 30"));
assert!(!re.is_match("Value: 75"));

re.set_variable("max", "100")?;
assert!(re.is_match("Value: 75"));
```

## Summary

| What you want | How |
|---------------|-----|
| Validate during matching | Add `(?{native:name})` after the subpattern |
| Inline Lua validation | `(?{lua:return condition})` |
| Inline JS validation | `(?{js:return condition;})` |
| Inline Rhai validation | `(?{rhai:condition})` |
| WASM callback | `(?{wasm:module:function})` |
| Access capture group 1 | Native: `ctx.group(1)`, Lua/JS: `arg[1]`, Rhai: `arg[1]` |
| Access named capture | Native: `ctx.named("x")`, Lua/JS: `named.x`, Rhai: `named["x"]` |
| Access host variable | Native: `ctx.variable("x")`, Lua/JS: `vars.x`, Rhai: `vars["x"]` |
| Return pass/fail | `ExecResult::Success` / `ExecResult::Failure` |
| Return a computed number | `ExecResult::Numeric(value)` |
| Return a replacement | `ExecResult::Replacement(text)` |

## Next

[Chapter 3: Steering the Match >>>](03-match-steering.md)
