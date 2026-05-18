# RegexSet

When you need to test a single input against many patterns -- routing an HTTP
request, classifying log lines, filtering events -- compiling and running each
regex one at a time works, but `RegexSet` makes it cleaner and gives you a
single entry point for the whole battery of tests.

## Creating a RegexSet

`RegexSet::new` takes anything that yields pattern strings and compiles them
all at once. If *any* pattern is invalid, the entire construction fails with
a diagnostic that identifies which pattern was the problem:

```rust
# use rgx_core::RegexSet;
let set = RegexSet::new(&[
    r"\d+",           // pattern 0: digits
    r"[a-z]+",        // pattern 1: lowercase letters
    r"[A-Z]+",        // pattern 2: uppercase letters
])?;

assert_eq!(set.len(), 3);
# Ok::<(), Box<dyn std::error::Error>>(())
```

You can also create an empty set that matches nothing:

```rust
# use rgx_core::RegexSet;
let set = RegexSet::empty();
assert!(set.is_empty());
assert!(!set.is_match("anything"));
```

### Error handling

When a pattern fails to compile, the error message includes the pattern
index and the original text:

```rust
# use rgx_core::RegexSet;
let result = RegexSet::new(&[r"\d+", r"(unclosed"]);
assert!(result.is_err());
// Error message: "pattern 1 ("(unclosed"): ..."
```

## Quick boolean check with `is_match`

The simplest operation: does *any* pattern in the set match?

```rust
# use rgx_core::RegexSet;
let set = RegexSet::new(&[r"error", r"warn", r"fatal"])?;

assert!(set.is_match("disk error on sda1"));
assert!(set.is_match("warn: low memory"));
assert!(!set.is_match("info: all systems normal"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is useful for quick filtering -- for example, deciding whether a log
line is worth processing further.

## Detailed results with `matches`

When you need to know *which* patterns matched, use `matches`. It returns a
`SetMatches` object:

```rust
# use rgx_core::RegexSet;
let set = RegexSet::new(&[
    r"\d+",
    r"[a-z]+",
    r"[A-Z]+",
])?;

let result = set.matches("abc 123 XYZ");

// Query individual patterns by index
assert!(result.matched(0));   // \d+ matched
assert!(result.matched(1));   // [a-z]+ matched
assert!(result.matched(2));   // [A-Z]+ matched

// Out-of-bounds index returns false, not a panic
assert!(!result.matched(99));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `SetMatches` methods

| Method | Returns | Description |
|--------|---------|-------------|
| `matched(i)` | `bool` | Whether pattern `i` matched |
| `matched_any()` | `bool` | Whether at least one pattern matched |
| `matched_all()` | `bool` | Whether every pattern matched |
| `len()` | `usize` | Number of patterns in the set |
| `is_empty()` | `bool` | Whether the set has no patterns |
| `iter()` | `SetMatchesIter` | Iterator over indices of matched patterns |

### Iterating over matched indices

`SetMatches::iter()` yields only the indices of patterns that matched. This
is cleaner than checking each index manually:

```rust
# use rgx_core::RegexSet;
let set = RegexSet::new(&[r"a", r"b", r"c", r"d", r"e"])?;
let result = set.matches("ace");

let matched_indices: Vec<usize> = result.iter().collect();
assert_eq!(matched_indices, vec![0, 2, 4]);  // a, c, e
# Ok::<(), Box<dyn std::error::Error>>(())
```

`SetMatches` also implements `IntoIterator`, so you can consume it directly
in a `for` loop:

```rust
# use rgx_core::RegexSet;
let set = RegexSet::new(&[r"error", r"warn", r"critical"])?;
let result = set.matches("critical error in subsystem");

for idx in result {
    match idx {
        0 => println!("Error detected"),
        2 => println!("Critical detected"),
        _ => {}
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `matched_any` and `matched_all`

These convenience predicates answer two common questions:

```rust
# use rgx_core::RegexSet;
let set = RegexSet::new(&[r"\d", r"[a-z]", r"[A-Z]"])?;

// All three character classes present
let m = set.matches("aA1");
assert!(m.matched_any());
assert!(m.matched_all());

// Only digits
let m = set.matches("42");
assert!(m.matched_any());
assert!(!m.matched_all());

// Nothing matches
let m = set.matches("!@#");
assert!(!m.matched_any());
assert!(!m.matched_all());
# Ok::<(), Box<dyn std::error::Error>>(())
```

Note that `matched_all` returns `false` for an empty set -- there is nothing
for "all" to be true about.

## Accessing the original patterns

The `patterns()` method returns the pattern strings in their original order:

```rust
# use rgx_core::RegexSet;
let set = RegexSet::new(&[r"\d+", r"\w+"])?;
assert_eq!(set.patterns(), &[r"\d+".to_string(), r"\w+".to_string()]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Use case: HTTP routing

A classic use for `RegexSet` is matching incoming request paths against a
table of routes. Rather than testing each route sequentially and stopping at
the first hit, `RegexSet` tells you which routes match, and you can pick
the most specific one:

```rust
# use rgx_core::RegexSet;
let routes = RegexSet::new(&[
    r"^/api/users/\d+$",     // 0: specific user
    r"^/api/users$",          // 1: user list
    r"^/api/posts",           // 2: posts (prefix)
    r"^/static/",             // 3: static files
    r"^/health$",             // 4: health check
])?;

// Dispatch based on which pattern matched
let path = "/api/users/42";
let result = routes.matches(path);

if result.matched(0) {
    println!("Serving specific user");
} else if result.matched(1) {
    println!("Serving user list");
} else if result.matched(2) {
    println!("Serving posts");
} else if result.matched(3) {
    println!("Serving static file");
} else if result.matched(4) {
    println!("Health check OK");
} else {
    println!("404 Not Found");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Priority routing

When multiple patterns can match the same path, you might want the most
specific one. A simple approach: assign priorities by index order (lower
index = higher priority) and take the first matched index:

```rust
# use rgx_core::RegexSet;
let routes = RegexSet::new(&[
    r"^/api/users/\d+$",  // most specific first
    r"^/api/users",        // then less specific
    r"^/api",              // then even less
])?;

let result = routes.matches("/api/users/42");
let best = result.iter().next();
assert_eq!(best, Some(0));  // most specific pattern wins
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Use case: log classification

Another natural fit is classifying text into categories. Here we tag log
lines with severity levels:

```rust
# use rgx_core::RegexSet;
let severity = RegexSet::new(&[
    r"(?i)\b(fatal|panic)\b",    // 0: critical
    r"(?i)\b(error|err)\b",      // 1: error
    r"(?i)\b(warn|warning)\b",   // 2: warning
    r"(?i)\b(info|notice)\b",    // 3: info
    r"(?i)\b(debug|trace)\b",    // 4: debug
])?;

let line = "WARN: disk usage at 92%";
let result = severity.matches(line);

let label = result.iter().next().map(|i| match i {
    0 => "CRITICAL",
    1 => "ERROR",
    2 => "WARNING",
    3 => "INFO",
    4 => "DEBUG",
    _ => "UNKNOWN",
});

assert_eq!(label, Some("WARNING"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Use case: input validation

Check whether an input string satisfies multiple constraints simultaneously:

```rust
# use rgx_core::RegexSet;
// Password must have: uppercase, lowercase, digit, special char, length >= 8
let rules = RegexSet::new(&[
    r"[A-Z]",           // 0: has uppercase
    r"[a-z]",           // 1: has lowercase
    r"\d",              // 2: has digit
    r"[^a-zA-Z\d\s]",  // 3: has special char
    r".{8,}",           // 4: at least 8 chars
])?;

let password = "MyP@ss99";
let result = rules.matches(password);

if result.matched_all() {
    println!("Password meets all requirements");
} else {
    for i in 0..rules.len() {
        if !result.matched(i) {
            println!("Failed rule {i}: {}", rules.patterns()[i]);
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Performance notes

`RegexSet` compiles each pattern independently and tests them sequentially.
For small to moderate sets (dozens of patterns), this is straightforward and
fast. The main advantage over hand-rolled loops is not raw speed but
*clarity* -- a single `matches()` call replaces a chain of `if` statements.

If you have hundreds of patterns, consider whether some can be combined into
a single regex with alternation (`a|b|c`), which the engine can optimize
more aggressively.
