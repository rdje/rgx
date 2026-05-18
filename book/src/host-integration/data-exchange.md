# Data Exchange

Regex patterns in rgx can read host-provided data and produce structured output. This chapter covers every way to pass variables **into** a pattern and extract values **out** of it.

## Setting string variables

The simplest form of data exchange is `set_variable`, which stores a string that code blocks can read:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(
    r"(?P<price>\d+\.\d{2})(?{native:check_currency})",
    ExecutionMode::Full,
)?;

re.set_variable("currency", "USD")?;

re.register_native("check_currency", |ctx| {
    let currency = ctx.variable("currency").unwrap_or_default();
    if currency == "USD" || currency == "EUR" {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("19.99"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

String variables are accessible in code blocks through `ctx.variable("name")` in native callbacks, and through the `vars` table in scripted languages.

## Typed variables with the Value enum

For richer data, use `set_typed_variable` or its shorthand `set_var`. These store a `Value`, which can be any of:

| Variant | Rust type | Example |
|---------|-----------|---------|
| `Value::Int` | `i64` | `42` |
| `Value::Float` | `f64` | `3.14` |
| `Value::Bool` | `bool` | `true` |
| `Value::String` | `String` | `"hello"` |
| `Value::Array` | `Vec<Value>` | `[1, 2, 3]` |
| `Value::Map` | `Vec<(String, Value)>` | `{"host": "localhost"}` |
| `Value::Null` | - | no value |

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult, Value};
let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full)?;

// set_typed_variable — explicit Value construction
re.set_typed_variable("threshold", Value::Int(100))?;
re.set_typed_variable("rate", Value::Float(0.08))?;
re.set_typed_variable("tags", Value::Array(vec![
    Value::String("web".into()),
    Value::String("api".into()),
]))?;

// set_var — automatic Into<Value> conversion
re.set_var("debug", true)?;
re.set_var("name", "alice")?;
re.set_var("port", 8080_i64)?;

re.register_native("check", |ctx| {
    let threshold = ctx.var_int("threshold").unwrap_or(0);
    let rate = ctx.var_float("rate").unwrap_or(0.0);
    let debug = ctx.var_bool("debug").unwrap_or(false);
    let name = ctx.var_str("name").unwrap_or_default();
    let tags = ctx.var_array("tags").unwrap_or_default();

    if debug {
        eprintln!("{name}: threshold={threshold}, rate={rate}, tags={}", tags.len());
    }
    ExecResult::Success
})?;

assert!(re.is_match("x"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Convenience accessors on ExecContext

| Method | Return type | Extracts |
|--------|-------------|----------|
| `ctx.var_str("k")` | `Option<String>` | String value |
| `ctx.var_int("k")` | `Option<i64>` | Integer value |
| `ctx.var_float("k")` | `Option<f64>` | Float (or int widened to f64) |
| `ctx.var_bool("k")` | `Option<bool>` | Boolean value |
| `ctx.var_array("k")` | `Option<Vec<Value>>` | Array clone |
| `ctx.var_map("k")` | `Option<Vec<(String, Value)>>` | Map clone |
| `ctx.typed_variable("k")` | `Option<Value>` | Raw Value clone |

## The vars() fluent builder

When you need to set many variables at once, `vars()` provides a chainable builder:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(r".", ExecutionMode::Full)?;

re.vars()
    .set("env", "production")
    .set("max_retries", 3_i64)
    .set("timeout_ms", 5000_i64)
    .hash("database")
        .set("host", "db.example.com")
        .set("port", 5432_i64)
        .hash("tls")
            .set("enabled", true)
            .set("cert", "/etc/ssl/cert.pem")
            .done()
        .list("replicas")
            .push("r1.example.com")
            .push("r2.example.com")
            .done()
        .done()
    .list("allowed_origins")
        .push("https://example.com")
        .push("https://api.example.com")
        .done();
# Ok::<(), Box<dyn std::error::Error>>(())
```

The builder writes each value eagerly. `hash("name")` opens a nested map scope, `list("name")` opens an array scope, and `done()` commits the scope and returns to the parent.

## The vars! and value! macros

For maximum brevity, the `vars!` macro sets multiple variables at once using declarative syntax:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, vars};
let re = Regex::with_mode(r".", ExecutionMode::Full)?;

vars!(re, {
    "env" => "prod",
    "port" => 8080_i64,
    "debug" => false,
    "server" => {
        "host" => "localhost",
        "port" => 443_i64,
        "tls" => {
            "enabled" => true
        }
    },
    "allowed_origins" => ["https://example.com", "https://api.example.com"],
    "max_retries" => 3_i64
});
# Ok::<(), Box<dyn std::error::Error>>(())
```

The `value!` macro builds a standalone `Value`:

```rust,no_run
# use rgx_core::{Value, value};
let config = value!({
    "host" => "localhost",
    "port" => 8080_i64,
    "tags" => ["web", "api"],
    "tls" => { "enabled" => true }
});

assert!(matches!(config, Value::Map(_)));
```

You can pass a `value!` map directly to `set_vars` for bulk assignment:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, value};
let re = Regex::with_mode(r".", ExecutionMode::Full)?;

re.set_vars(value!({
    "env" => "staging",
    "port" => 3000_i64,
    "features" => ["auth", "logging"]
}));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Accessing variables from scripted languages

### Lua

Variables are exposed as global tables in Lua code blocks:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"\d+(?{lua:return tonumber(vars["threshold"]) > 0})"#,
    ExecutionMode::Safe,
)?;
re.set_variable("threshold", "50")?;
assert!(re.is_match("42"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### JavaScript

Variables are available on the global `vars` object:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"\d+(?{js:return parseInt(vars["threshold"]) > 0})"#,
    ExecutionMode::Safe,
)?;
re.set_variable("threshold", "50")?;
assert!(re.is_match("42"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Rhai

Variables are accessible through the `vars` map:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode};
let re = Regex::with_mode(
    r#"\d+(?{rhai:vars["threshold"].parse_int() > 0})"#,
    ExecutionMode::Safe,
)?;
re.set_variable("threshold", "50")?;
assert!(re.is_match("42"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Native callbacks

Native callbacks receive the full `ExecContext`:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(r"(?{native:validate})", ExecutionMode::Full)?;
re.set_var("min", 10_i64)?;
re.set_var("max", 100_i64)?;

re.register_native("validate", |ctx| {
    let min = ctx.var_int("min").unwrap_or(0);
    let max = ctx.var_int("max").unwrap_or(i64::MAX);
    let len = ctx.match_length() as i64;
    if len >= min && len <= max {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Code block return values

Code blocks can produce values beyond simple pass/fail. These are captured as `CodeBlockValue` on the winning match path.

### Numeric returns

A code block that returns a number produces `CodeBlockValue::Numeric`:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(
    r"(\d+)(?{native:score})",
    ExecutionMode::Full,
)?;

re.register_native("score", |ctx| {
    let n: f64 = ctx.group(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    ExecResult::Numeric(n * 2.5)
})?;

let result = re.find_first("item 42 here");
// The MatchResult carries the code block value
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Replacement returns

A code block returning a string replacement value produces `CodeBlockValue::Replacement`:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(
    r"(\w+)(?{native:transform})",
    ExecutionMode::Full,
)?;

re.register_native("transform", |ctx| {
    let word = ctx.group(1).unwrap_or("");
    ExecResult::Replacement(word.to_uppercase())
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Structured returns

For complex output, return a full `Value`:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult, Value};
let re = Regex::with_mode(
    r"(?P<key>\w+)=(?P<val>\w+)(?{native:parse})",
    ExecutionMode::Full,
)?;

re.register_native("parse", |ctx| {
    let key = ctx.named("key").unwrap_or("").to_string();
    let val = ctx.named("val").unwrap_or("").to_string();
    ExecResult::Structured(Value::map(vec![
        ("key", Value::from(key)),
        ("value", Value::from(val)),
    ]))
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Branch identification

When your pattern is a **bare top-level alternation** (not wrapped in
`(?:…)`) and the code block sits *inside an arm*, `matched_branch_number`
tells you which branch won:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult};
// Bare top-level alternation, one code block per arm. A `(?:…)`
// wrapper around the whole alternation, or a single code block
// before/after it, would suppress branch tracking (the engine
// only tracks the *top-level* alternation — see the regression
// `matched_branch_number_in_code_block_requires_bare_top_level_alternation`).
let re = Regex::with_mode(
    r"(\d+)(?{native:tag})|([a-z]+)(?{native:tag})|([A-Z]+)(?{native:tag})",
    ExecutionMode::Full,
)?;

re.register_native("tag", |ctx| {
    match ctx.matched_branch_number() {
        Some(1) => ExecResult::Replacement("number".into()),
        Some(2) => ExecResult::Replacement("lower".into()),
        Some(3) => ExecResult::Replacement("upper".into()),
        _ => ExecResult::Success,
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Branch numbers are 1-based and correspond to the top-level alternation arms in left-to-right order. This is particularly useful for building tokenizers (see the [Tokenizer](../real-world/tokenizer.md) example).

## Summary

| Task | API |
|------|-----|
| Set a string variable | `re.set_variable("k", "v")` |
| Set a typed variable | `re.set_var("k", 42_i64)` |
| Set with explicit Value | `re.set_typed_variable("k", Value::Int(42))` |
| Fluent builder | `re.vars().set("k", v).hash("h").set(...).done()` |
| Macro (many at once) | `vars!(re, { "k" => v, ... })` |
| Bulk from Value::Map | `re.set_vars(value!({ ... }))` |
| Read in native callback | `ctx.var_int("k")`, `ctx.var_str("k")`, etc. |
| Read in Lua/JS/Rhai | `vars["k"]` |
| Return numeric | `ExecResult::Numeric(n)` |
| Return replacement | `ExecResult::Replacement(s)` |
| Return structured | `ExecResult::Structured(v)` |
| Get branch number | `ctx.matched_branch_number()` |
