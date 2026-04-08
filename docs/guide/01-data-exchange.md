# Chapter 1: Passing Data In and Out

## The problem

Traditional regex is isolated. It takes a pattern and text, gives you positions, and that's it. If you need context from your application — a configuration value, a threshold, a user role — you have to build it into the pattern itself or check it after the match.

Consider scanning logs. You want to match errors in production but warnings in development. With a traditional regex, you'd need two patterns:

```rust
let prod_re = Regex::compile("ERROR")?;
let dev_re = Regex::compile("ERROR|WARN")?;

// Then pick which one to use based on environment
let re = if env == "prod" { &prod_re } else { &dev_re };
```

With rgx, one pattern handles both:

```rust
let re = Regex::with_mode(
    r#"(?<level>ERROR|WARN|INFO)(?{js:
        var dominated = {"prod": ["INFO", "WARN"], "dev": ["INFO"], "test": []};
        var dominated_list = dominated[vars.env] || [];
        return !dominated_list.includes(named.level);
    })"#,
    ExecutionMode::Safe,
)?;
re.set_variable("env", "prod")?;
```

One compiled pattern. The `env` variable controls which severity levels match. Change the variable, change the behavior — without recompiling.

## Sending data in: host variables

### Setting variables

Variables are key-value string pairs that you set on a compiled regex:

```rust
re.set_variable("threshold", "100")?;
re.set_variable("mode", "strict")?;
re.set_variable("user_role", "admin")?;
```

Variables persist on the regex until you change them. You can set them once and match many times, or change them between matches.

### Reading variables from code blocks

Inside any code block (Lua, JS, Rhai, native), variables are available via `vars`:

**JavaScript:**
```javascript
vars.threshold   // "100"
vars.mode        // "strict"
vars.user_role   // "admin"
```

**Lua:**
```lua
vars.threshold   -- "100"
vars.mode        -- "strict"
```

**Rhai:**
```rhai
vars["threshold"]   // "100"
vars["mode"]        // "strict"
```

**Native Rust:**
```rust
ctx.variable("threshold")   // Some("100")
ctx.variable("mode")        // Some("strict")
```

### Example: configurable number validation

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<num>\d+)(?{native:check_range})",
    ExecutionMode::Full,
)?;

re.register_native("check_range", |ctx| {
    let num: i64 = ctx.named("num").unwrap_or("0").parse().unwrap_or(0);
    let min: i64 = ctx.variable("min").unwrap_or_else(|| "0".to_string()).parse().unwrap_or(0);
    let max: i64 = ctx.variable("max").unwrap_or_else(|| "999999".to_string()).parse().unwrap_or(999999);

    if num >= min && num <= max {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

// Match ports (1-65535)
re.set_variable("min", "1")?;
re.set_variable("max", "65535")?;
assert!(re.is_match("Port: 8080"));
assert!(!re.is_match("Port: 99999"));

// Same pattern, now match percentages (0-100)
re.set_variable("min", "0")?;
re.set_variable("max", "100")?;
assert!(re.is_match("Usage: 85%"));
assert!(!re.is_match("Usage: 150%"));
```

One compiled pattern. Two completely different validation ranges. The pattern didn't change — only the variables.

### Variables are snapshots

When a code block executes, it sees a frozen copy of the variables. Even if the engine backtracks and re-evaluates the code block, the variables are consistent. You never see half-updated state.

### Beyond strings: typed variables

String variables work for simple cases, but parsing `"100"` into an integer inside every callback gets old fast. Typed variables let you pass integers, floats, booleans, arrays, and maps directly:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, Value};

let re = Regex::with_mode(
    r"(?<num>\d+)(?{native:check_range})",
    ExecutionMode::Full,
)?;

// Pass typed values — no string parsing needed in the callback
re.set_typed_variable("min", Value::Int(1))?;
re.set_typed_variable("max", Value::Int(65535))?;

re.register_native("check_range", |ctx| {
    let num: i64 = ctx.named("num").unwrap_or("0").parse().unwrap_or(0);
    let min = ctx.typed_variable("min").and_then(|v| v.as_i64()).unwrap_or(0);
    let max = ctx.typed_variable("max").and_then(|v| v.as_i64()).unwrap_or(i64::MAX);

    if num >= min && num <= max {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("Port: 8080"));
assert!(!re.is_match("Port: 99999"));
```

Compare this to the string version — no `.parse().unwrap_or()` chain for the threshold values. The callback reads them as integers directly.

### All supported value types

| Type | How to create | How to read |
|------|--------------|-------------|
| String | `Value::String("hello".into())` | `v.as_str()` → `Some("hello")` |
| Integer | `Value::Int(42)` | `v.as_i64()` → `Some(42)` |
| Float | `Value::Float(3.14)` | `v.as_f64()` → `Some(3.14)` |
| Boolean | `Value::Bool(true)` | `v.as_bool()` → `Some(true)` |
| Null | `Value::Null` | check with `matches!(v, Value::Null)` |
| Array | `Value::Array(vec![Value::Int(1), Value::Int(2)])` | `v.as_array()` → `Some(&[...])` |
| Map | `Value::Map(vec![("key".into(), Value::Int(1))])` | `v.as_map()` → `Some(&[...])` |

Shorthand constructors work thanks to `From` impls:

```rust
re.set_typed_variable("count", Value::from(42_i64))?;
re.set_typed_variable("rate", Value::from(0.08_f64))?;
re.set_typed_variable("debug", Value::from(true))?;
re.set_typed_variable("name", Value::from("alice"))?;
```

### Passing arrays: allowlists and blocklists

Pass a list of allowed values and check membership inside the callback:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, Value};

let re = Regex::with_mode(
    r"(?<word>\w+)(?{native:in_allowlist})",
    ExecutionMode::Full,
)?;

re.set_typed_variable("allowed", Value::Array(vec![
    Value::String("cat".into()),
    Value::String("dog".into()),
    Value::String("bird".into()),
]))?;

re.register_native("in_allowlist", |ctx| {
    let word = ctx.named("word").unwrap_or("");
    let allowed = ctx.typed_variable("allowed")
        .and_then(|v| v.as_array())
        .unwrap_or(&[]);

    if allowed.iter().any(|v| v.as_str() == Some(word)) {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("the cat sat"));
assert!(re.is_match("a dog barked"));
assert!(!re.is_match("a fish swam"));  // "fish" not in allowlist
```

One compiled pattern. The allowlist is a variable — change it without recompiling.

### Passing maps: lookup tables

Pass a key-value lookup table and use it for validation or enrichment:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, Value, CodeBlockValue};

let re = Regex::with_mode(
    r"(?<code>[A-Z]{2})(?{native:country_lookup})",
    ExecutionMode::Full,
)?;

re.set_typed_variable("countries", Value::Map(vec![
    ("US".into(), Value::String("United States".into())),
    ("UK".into(), Value::String("United Kingdom".into())),
    ("FR".into(), Value::String("France".into())),
    ("DE".into(), Value::String("Germany".into())),
]))?;

re.register_native("country_lookup", |ctx| {
    let code = ctx.named("code").unwrap_or("");
    let countries = ctx.typed_variable("countries")
        .and_then(|v| v.as_map())
        .unwrap_or(&[]);

    match countries.iter().find(|(k, _)| k == code) {
        Some((_, name)) => ExecResult::Replacement(
            name.as_str().unwrap_or("Unknown").to_string()
        ),
        None => ExecResult::Failure, // unknown country code — no match
    }
})?;

// "US" matches and the result carries the full country name
let m = re.find_first("Country: US").unwrap();
assert_eq!(m.code_result, Some(CodeBlockValue::Replacement("United States".into())));

// "XX" doesn't match — it's not in the lookup table
assert!(!re.is_match("Country: XX"));
```

### Passing configuration objects

For complex configuration, nest maps and arrays:

```rust
re.set_typed_variable("config", Value::Map(vec![
    ("thresholds".into(), Value::Map(vec![
        ("warn".into(), Value::Int(80)),
        ("error".into(), Value::Int(95)),
    ])),
    ("ignored_hosts".into(), Value::Array(vec![
        Value::String("localhost".into()),
        Value::String("127.0.0.1".into()),
    ])),
    ("strict_mode".into(), Value::Bool(true)),
]))?;
```

Your callback reads it like a config object:

```rust
re.register_native("check_threshold", |ctx| {
    let config = ctx.typed_variable("config").unwrap();
    let thresholds = config.as_map().unwrap();
    // Navigate: config.thresholds.warn
    let warn_threshold = thresholds.iter()
        .find(|(k, _)| k == "thresholds")
        .and_then(|(_, v)| v.as_map())
        .and_then(|m| m.iter().find(|(k, _)| k == "warn"))
        .and_then(|(_, v)| v.as_i64())
        .unwrap_or(80);
    // ... use warn_threshold
    ExecResult::Success
})?;
```

### Backward compatibility

The string API still works exactly as before:

```rust
// Old way — still works
re.set_variable("env", "prod")?;

// Old way of reading — still works
ctx.variable("env")  // Some("prod")

// New way of reading the same variable — also works
ctx.typed_variable("env")  // Some(Value::String("prod"))
```

When you set a typed variable, it's also readable as a string (the value is auto-converted via its `Display` representation). When you set a string variable, it's also readable as a typed `Value::String`. Both directions work seamlessly.

## Getting data out: result values

### The problem with traditional regex

After a traditional match, you have positions. If you want computed values, you extract the matched text and process it:

```rust
// Traditional approach: match, extract, process
let m = re.find("price: $42.50").unwrap();
let price: f64 = m.as_str().parse().unwrap();
let with_tax = price * 1.08;
```

Three steps. With rgx, the computation happens during matching:

### Numeric results

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, CodeBlockValue};

let re = Regex::with_mode(
    r#"\$(?<price>\d+\.?\d*)(?{native:with_tax})"#,
    ExecutionMode::Full,
)?;
re.register_native("with_tax", |ctx| {
    let price: f64 = ctx.named("price").unwrap_or("0").parse().unwrap_or(0.0);
    ExecResult::Numeric(price * 1.08)
})?;

let m = re.find_first("Total: $49.99").unwrap();
match m.code_result {
    Some(CodeBlockValue::Numeric(taxed)) => {
        println!("With tax: ${:.2}", taxed);
        // With tax: $53.99
    }
    _ => println!("No price computed"),
}
```

### Collecting numeric values across matches

```rust
let prices = re.find_all_numeric_with_code("Items: $10, $25.50, $5");
// prices = [10.8, 27.54, 5.4]
let total: f64 = prices.iter().sum();
println!("Total with tax: ${:.2}", total);
// Total with tax: $43.74
```

One pass through the text. Every price extracted, taxed, and collected.

### Replacement results

Code blocks can also return replacement strings, powering find-and-replace driven by logic:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<word>[a-z]+)(?{native:title_case})",
    ExecutionMode::Full,
)?;
re.register_native("title_case", |ctx| {
    let word = ctx.named("word").unwrap_or("");
    let mut chars = word.chars();
    let titled = match chars.next() {
        None => String::new(),
        Some(first) => {
            let upper: String = first.to_uppercase().collect();
            upper + chars.as_str()
        }
    };
    ExecResult::Replacement(titled)
})?;

let result = re.replace_all_with_code("hello world foo bar");
assert_eq!(result, "Hello World Foo Bar");
```

### Emitting values from inline languages

Lua, JS, and Rhai can also emit values without returning them directly:

**Lua:**
```lua
rgx.emit_numeric(42.0)
rgx.emit_replacement("REDACTED")
```

**JavaScript:**
```javascript
rgx.emitNumeric(42.0)
rgx.emitReplacement("REDACTED")
```

**Rhai:**
```rhai
emit_numeric(42.0)
emit_replacement("REDACTED")
```

These are useful when your code block needs to do work (multiple statements) and then emit a result, rather than computing it as a return value.

### Structured results: returning complex data

Sometimes a single number or string isn't enough. Your callback computes multiple values and you want all of them:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex, Value, CodeBlockValue};

let re = Regex::with_mode(
    r"(?<price>\d+\.?\d*)(?{native:analyze})",
    ExecutionMode::Full,
)?;
re.register_native("analyze", |ctx| {
    let price: f64 = ctx.named("price").unwrap_or("0").parse().unwrap_or(0.0);
    ExecResult::Structured(Value::Map(vec![
        ("original".into(), Value::Float(price)),
        ("with_tax".into(), Value::Float(price * 1.08)),
        ("discount_10".into(), Value::Float(price * 0.9)),
        ("is_expensive".into(), Value::Bool(price > 100.0)),
    ]))
})?;

let m = re.find_first("Price: 49.99").unwrap();
if let Some(CodeBlockValue::Structured(data)) = &m.code_result {
    let map = data.as_map().unwrap();
    for (key, val) in map {
        println!("{}: {:?}", key, val);
    }
    // original: Float(49.99)
    // with_tax: Float(53.9892)
    // discount_10: Float(44.991)
    // is_expensive: Bool(false)
}
```

One match. Multiple computed values. All carried in the result without any external state.

### What goes in vs what comes out: summary

| Direction | Type | How |
|-----------|------|-----|
| **In** | String | `set_variable("key", "value")` |
| **In** | Integer | `set_typed_variable("key", Value::Int(42))` |
| **In** | Float | `set_typed_variable("key", Value::Float(3.14))` |
| **In** | Boolean | `set_typed_variable("key", Value::Bool(true))` |
| **In** | Array | `set_typed_variable("key", Value::Array(vec![...]))` |
| **In** | Map | `set_typed_variable("key", Value::Map(vec![...]))` |
| **Out** | Number | `ExecResult::Numeric(f64)` → `CodeBlockValue::Numeric` |
| **Out** | String | `ExecResult::Replacement(String)` → `CodeBlockValue::Replacement` |
| **Out** | Structured | `ExecResult::Structured(Value)` → `CodeBlockValue::Structured` |

## Knowing which branch matched

### The problem

You have a pattern with alternatives: `error|warning|info`. When it matches, which one won? Traditional engines make you wrap each alternative in a capture group and check which one is non-empty:

```rust
// Traditional approach: capture groups for branch detection
let re = Regex::compile(r"(error)|(warning)|(info)")?;
let caps = re.captures("got a warning").unwrap();
// Check caps.get(1), caps.get(2), caps.get(3)...
// Whichever is Some(...) is the winner. Tedious.
```

### The rgx way

rgx tells you directly:

```rust
let re = Regex::compile(r"error|warning|info")?;
let m = re.find_first("got a warning").unwrap();
assert_eq!(m.matched_branch_number, Some(2));
// Branch 1 = error, Branch 2 = warning, Branch 3 = info
```

No extra capture groups. No checking which one is non-empty. Just a number.

### Building a tokenizer

This makes tokenizers trivial:

```rust
use rgx_core::Regex;

let lexer = Regex::compile(
    r"(?<num>\d+)|(?<id>[a-zA-Z_]\w*)|(?<op>[+\-*/=<>!]+)|(?<str>\"[^\"]*\")|(?<ws>\s+)|(?<unknown>.)"
)?;

let source = r#"let x = 42 + "hello""#;

for token in lexer.find_all(source) {
    let kind = match token.matched_branch_number {
        Some(1) => "NUMBER",
        Some(2) => "IDENT",
        Some(3) => "OPERATOR",
        Some(4) => "STRING",
        Some(5) => "WHITESPACE",
        Some(6) => "UNKNOWN",
        _ => "???",
    };
    let text = &source[token.start..token.end];
    if kind != "WHITESPACE" {
        println!("{:12} {:?}", kind, text);
    }
}
```

Output:
```
IDENT        "let"
IDENT        "x"
OPERATOR     "="
NUMBER       "42"
OPERATOR     "+"
STRING       "\"hello\""
```

A complete lexer in one regex. The branch number tells you the token type. No post-processing, no secondary dispatch.

### Branch numbers and code blocks

Inside code blocks, the branch number is available as `branch_number`:

```rust
re.register_native("log_branch", |ctx| {
    println!("Matched branch {}", ctx.matched_branch_number().unwrap_or(0));
    ExecResult::Success
})?;
```

This is useful when the same callback is used across multiple branches but needs to behave differently.

## Summary

| What you want | How |
|---------------|-----|
| Pass runtime config into regex | `re.set_variable("key", "value")` |
| Read variables in JS | `vars.key` |
| Read variables in Lua | `vars.key` |
| Read variables in Rust | `ctx.variable("key")` |
| Return a number from a match | `ExecResult::Numeric(value)` |
| Return a replacement string | `ExecResult::Replacement(text)` |
| Collect all numeric results | `re.find_all_numeric_with_code(text)` |
| Find-and-replace with logic | `re.replace_all_with_code(text)` |
| Know which branch matched | `m.matched_branch_number` |

## Next

[Chapter 2: Predicate Callbacks >>>](02-predicate-callbacks.md)
