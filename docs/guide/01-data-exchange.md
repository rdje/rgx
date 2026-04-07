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

## Getting data out: result values

### The problem with traditional regex

After a traditional match, you have positions. If you want computed values, you extract the matched text and process it:

```rust
// Traditional approach: match, extract, process
let m = re.find_first("price: $42.50").unwrap();
let price_text = &text[m.start..m.end];
let price: f64 = price_text.parse().unwrap();
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

## Knowing which branch matched

### The problem

You have a pattern with alternatives: `error|warning|info`. When it matches, which one won? Traditional engines make you wrap each alternative in a capture group and check which one is non-empty:

```rust
// Traditional approach: capture groups for branch detection
let re = Regex::compile(r"(error)|(warning)|(info)")?;
let m = re.find_first("got a warning").unwrap();
// Check m.groups[1], m.groups[2], m.groups[3]...
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
