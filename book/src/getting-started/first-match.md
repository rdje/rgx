# Installation & First Match

## Adding rgx to your project

```toml
[dependencies]
rgx-core = "0.1"
```

## Compiling a pattern

In rgx, you compile a pattern once and reuse it:

```rust,ignore
use rgx_core::Regex;

let re = Regex::compile(r"\d{3}-\d{4}")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The `r"..."` raw string syntax means backslashes are literal — no double-escaping needed. Compilation is the expensive step; once compiled, matching is fast.

If the pattern has a syntax error, you get a helpful diagnostic:

```text
regex compile error: E_PARSE_FAILURE: ...
  (abc[def
  ^
```

## Finding the first match

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"\d{3}-\d{4}")?;

if let Some(m) = re.find("Call 555-1234 for info") {
    println!("{}", m.as_str());     // "555-1234"
    println!("{}..{}", m.start(), m.end());  // 5..13
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`find` returns `Option<Match>`. The `Match` type borrows the input text — use `m.as_str()` to get the matched substring directly.

## Testing if a pattern matches

When you only need yes/no:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;

assert!(re.is_match("abc 123"));
assert!(!re.is_match("no digits"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Finding all matches

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"\b\w+\b")?;

for m in re.find_iter("hello world foo") {
    println!("{}", m.as_str());
}
// hello
// world
// foo
# Ok::<(), Box<dyn std::error::Error>>(())
```

`find_iter` is a lazy iterator — it finds matches one at a time without allocating a `Vec`. If you need random access to all matches, use `find_all` instead.

## Escaping user input

If you're building patterns from user-provided strings, escape metacharacters first:

```rust,ignore
use rgx_core::{Regex, escape};

let user_input = "price is $3.50";
let pattern = escape(user_input);
let re = Regex::compile(&pattern)?;
assert!(re.is_match("price is $3.50"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What's next

You now know how to compile, find, and test. Next: [Finding Matches](./finding-matches.md) in depth.
