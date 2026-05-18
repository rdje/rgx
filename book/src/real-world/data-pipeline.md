# Data Pipeline

This example builds a data transformation pipeline using `replace_all` with native callbacks and typed variables. The pattern extracts fields from structured text, transforms them using host-provided configuration, and produces clean output.

## The scenario

We have CSV-like log data with timestamps, amounts, and currencies that need normalization:

```text
2026-04-08|USD|1234.56|purchase
2026-04-08|EUR|789.00|refund
2026-04-08|GBP|45.99|purchase
```

We want to:
1. Convert all amounts to USD using exchange rates
2. Reformat dates from `YYYY-MM-DD` to `DD/MM/YYYY`
3. Uppercase the transaction type
4. Output clean JSON-like records

## Setting up typed variables

Exchange rates and configuration are passed as typed variables:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, Value, vars};
let re = Regex::with_mode(
    r"(?P<date>\d{4}-\d{2}-\d{2})\|(?P<currency>[A-Z]{3})\|(?P<amount>\d+\.\d{2})\|(?P<type>\w+)(?{native:transform})",
    ExecutionMode::Full,
)?;

// Set exchange rates as a nested map
vars!(re, {
    "rates" => {
        "USD" => 1.0_f64,
        "EUR" => 1.08_f64,
        "GBP" => 1.27_f64,
        "JPY" => 0.0067_f64
    },
    "output_currency" => "USD",
    "date_format" => "dd/mm/yyyy"
});
# Ok::<(), Box<dyn std::error::Error>>(())
```

## The transform callback

The native callback reads the captured fields and host variables, then produces a replacement string:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, Value, vars};
# let re = Regex::with_mode(
#     r"(?P<date>\d{4}-\d{2}-\d{2})\|(?P<currency>[A-Z]{3})\|(?P<amount>\d+\.\d{2})\|(?P<type>\w+)(?{native:transform})",
#     ExecutionMode::Full,
# )?;
# vars!(re, { "rates" => { "USD" => 1.0_f64, "EUR" => 1.08_f64, "GBP" => 1.27_f64 }, "output_currency" => "USD" });
re.register_native("transform", |ctx| {
    let date = ctx.named("date").unwrap_or("");
    let currency = ctx.named("currency").unwrap_or("");
    let amount: f64 = ctx.named("amount")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let tx_type = ctx.named("type").unwrap_or("");

    // Look up exchange rate from typed variables
    let rate = ctx.typed_variable("rates")
        .and_then(|rates| {
            rates.as_map()?.iter()
                .find(|(k, _)| k == currency)
                .and_then(|(_, v)| v.as_f64())
        })
        .unwrap_or(1.0);

    // Convert amount
    let converted = amount * rate;

    // Reformat date: YYYY-MM-DD -> DD/MM/YYYY
    let date_parts: Vec<&str> = date.split('-').collect();
    let formatted_date = if date_parts.len() == 3 {
        format!("{}/{}/{}", date_parts[2], date_parts[1], date_parts[0])
    } else {
        date.to_string()
    };

    // Build output record
    let output = format!(
        r#"{{"date":"{}","amount":{:.2},"currency":"USD","type":"{}"}}"#,
        formatted_date,
        converted,
        tx_type.to_uppercase()
    );

    ExecResult::Replacement(output)
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Running the pipeline

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, Value, vars};
# let re = Regex::with_mode(
#     r"(?P<date>\d{4}-\d{2}-\d{2})\|(?P<currency>[A-Z]{3})\|(?P<amount>\d+\.\d{2})\|(?P<type>\w+)(?{native:transform})",
#     ExecutionMode::Full,
# )?;
# vars!(re, { "rates" => { "USD" => 1.0_f64, "EUR" => 1.08_f64, "GBP" => 1.27_f64 }, "output_currency" => "USD" });
# re.register_native("transform", |ctx| {
#     let currency = ctx.named("currency").unwrap_or("");
#     let amount: f64 = ctx.named("amount").and_then(|s| s.parse().ok()).unwrap_or(0.0);
#     let rate = ctx.typed_variable("rates").and_then(|r| r.as_map()?.iter().find(|(k,_)| k == currency).and_then(|(_, v)| v.as_f64())).unwrap_or(1.0);
#     let converted = amount * rate;
#     let date = ctx.named("date").unwrap_or("");
#     let dp: Vec<&str> = date.split('-').collect();
#     let fd = if dp.len() == 3 { format!("{}/{}/{}", dp[2], dp[1], dp[0]) } else { date.to_string() };
#     let tx = ctx.named("type").unwrap_or("").to_uppercase();
#     ExecResult::Replacement(format!(r#"{{"date":"{}","amount":{:.2},"currency":"USD","type":"{}"}}"#, fd, converted, tx))
# })?;
let input = "2026-04-08|USD|1234.56|purchase
2026-04-08|EUR|789.00|refund
2026-04-08|GBP|45.99|purchase";

let output = re.replace_all(input, |caps: &rgx_core::Captures| {
    // The replacement callback can access the full match
    caps[0].to_string()
});

// Each line is transformed by the native callback
println!("{output}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Batch processing with match_file_lines

Process a file line by line:

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult, vars};
# let re = Regex::with_mode(
#     r"(?P<date>\d{4}-\d{2}-\d{2})\|(?P<currency>[A-Z]{3})\|(?P<amount>\d+\.\d{2})\|(?P<type>\w+)",
#     ExecutionMode::Full,
# )?;
let matches = re.match_file_lines("data/transactions.csv")?;

for fm in &matches {
    println!("line {}: matched at {}..{}",
        fm.line_number,
        fm.match_result.start,
        fm.match_result.end
    );
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Dynamic configuration updates

Since variables are set on the compiled regex, you can update them between runs without recompiling:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, Value};
# let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full)?;
# re.register_native("check", |ctx| {
#     let threshold = ctx.var_float("threshold").unwrap_or(0.0);
#     ExecResult::Success
# })?;
// First run with one threshold
re.set_var("threshold", 100.0_f64)?;
re.is_match("test");

// Update threshold for second run -- no recompilation
re.set_var("threshold", 200.0_f64)?;
re.is_match("test");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Multi-stage pipeline

Chain multiple regexes for multi-stage transformations:

```rust
# use rgx_core::Regex;
// Stage 1: Normalize whitespace
let ws = Regex::compile(r"\s+")?;

// Stage 2: Extract key=value pairs
let kv = Regex::compile(r"(?P<key>\w+)=(?P<val>[^\s,]+)")?;

let input = "  name=Alice   age=30,  city=Paris  ";

// Stage 1
let normalized = ws.replace_all(input.trim(), " ");

// Stage 2
let mut record = std::collections::HashMap::new();
for caps in kv.captures_iter(&normalized) {
    record.insert(
        caps["key"].to_string(),
        caps["val"].to_string(),
    );
}

assert_eq!(record["name"], "Alice");
assert_eq!(record["age"], "30");
assert_eq!(record["city"], "Paris");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Key takeaways

- Typed variables (`vars!`, `set_var`) pass configuration into patterns without recompilation
- Native callbacks transform matched text on the fly with `ExecResult::Replacement`
- Named captures make field extraction self-documenting
- Variables can be updated between runs for dynamic configuration
- Chain multiple regexes for multi-stage pipelines
- `match_file_lines` processes files without loading them entirely into memory
