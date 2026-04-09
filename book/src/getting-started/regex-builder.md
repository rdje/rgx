# RegexBuilder & Configuration

`Regex::compile` is the one-liner for the common case. When you need flags without embedding them in the pattern, `RegexBuilder` provides a fluent API.

## Why use RegexBuilder?

Compare:

```rust,ignore
# use rgx_core::Regex;
// Flags in the pattern — fine for hardcoded patterns
let re = Regex::compile(r"(?ims)hello.world")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

```rust,ignore
# use rgx_core::RegexBuilder;
// Flags via builder — better when flags come from runtime config
let re = RegexBuilder::new(r"hello.world")
    .case_insensitive()
    .multi_line()
    .dot_matches_new_line()
    .build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Same result. The builder is cleaner when flags come from CLI arguments, config files, or user input.

## Available settings

All flag setters take no argument (the name implies `true`). For programmatic toggle, use the `set_*` variants:

```rust,ignore
# use rgx_core::{RegexBuilder, ExecutionMode};
let re = RegexBuilder::new(r"pattern")
    .case_insensitive()          // (?i) — é matches É, α matches Α
    .multi_line()                // (?m) — ^ and $ match line boundaries
    .dot_matches_new_line()      // (?s) — . matches \n
    .ignore_whitespace()         // (?x) — whitespace and # comments ignored
    .swap_greed()                // quantifiers lazy by default
    .mode(ExecutionMode::Safe)   // enable code block execution
    .build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Programmatic flag control

When the flag value comes from a variable:

```rust,ignore
# use rgx_core::RegexBuilder;
let user_wants_case_insensitive = true;

let re = RegexBuilder::new(r"hello")
    .set_case_insensitive(user_wants_case_insensitive)
    .build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Extended mode with comments

`ignore_whitespace()` enables extended mode where whitespace is ignored and `#` starts a comment:

```rust,ignore
# use rgx_core::RegexBuilder;
let re = RegexBuilder::new(r"
    \d{3}     # area code
    -         # separator
    \d{3}     # exchange
    -         # separator
    \d{4}     # subscriber
")
.ignore_whitespace()
.build()?;

assert!(re.is_match("555-123-4567"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Unicode case folding

`case_insensitive()` folds all Unicode letters, not just ASCII:

```rust,ignore
# use rgx_core::RegexBuilder;
let re = RegexBuilder::new(r"café")
    .case_insensitive()
    .build()?;

assert!(re.is_match("CAFÉ"));
assert!(re.is_match("Café"));
assert!(re.is_match("café"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

This works for Greek, Cyrillic, and other scripts too:

```rust,ignore
# use rgx_core::RegexBuilder;
let re = RegexBuilder::new(r"москва")
    .case_insensitive()
    .build()?;
assert!(re.is_match("МОСКВА"));
# Ok::<(), Box<dyn std::error::Error>>(())
```
