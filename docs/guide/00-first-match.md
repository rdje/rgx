# Chapter 0: Your First Match

Before we explore everything rgx can do, let's start with what every regex engine does — finding patterns in text. If you've used regex before, this will feel familiar. If not, this chapter gives you everything you need.

## What is a regex?

A regular expression (regex) is a pattern that describes a set of strings. Instead of searching for an exact word, you describe the *shape* of what you're looking for:

- `\d+` matches one or more digits: "42", "7", "12345"
- `[a-z]+` matches one or more lowercase letters: "hello", "cat", "xyz"
- `\d{3}-\d{4}` matches a phone number pattern: "555-1234"

## Compiling a pattern

In rgx, you first compile a pattern into a reusable regex object, then use it to match against text:

```rust
use rgx_core::Regex;

// Compile once, use many times
let re = Regex::compile(r"\d{3}-\d{4}")?;
```

The `r"..."` syntax is a Rust raw string — backslashes are literal, which is exactly what regex patterns need. You don't have to double-escape `\d` as `\\d`.

If the pattern has a syntax error, `compile` returns an error instead of panicking:

```rust
let result = Regex::compile(r"[unterminated");
assert!(result.is_err());
```

## Finding the first match

```rust
let re = Regex::compile(r"\d{3}-\d{4}")?;

if let Some(m) = re.find_first("Call 555-1234 for info") {
    println!("Found at position {}..{}", m.start, m.end);
    // Found at position 5..13

    // Extract the matched text
    let text = "Call 555-1234 for info";
    println!("Matched: {}", &text[m.start..m.end]);
    // Matched: 555-1234
}
```

`find_first` returns `Option<MatchResult>` — `Some` if there's a match, `None` if not.

A `MatchResult` tells you:
- `start` — byte position where the match begins
- `end` — byte position where the match ends (exclusive)
- `matched_branch_number` — which alternative matched (more on this later)
- `code_result` — a value returned by code blocks (more on this later)

## Finding all matches

```rust
let re = Regex::compile(r"\b\w+\b")?;

let text = "hello world foo";
let matches = re.find_all(text);

for m in &matches {
    println!("{}", &text[m.start..m.end]);
}
// hello
// world
// foo
```

`find_all` returns all non-overlapping matches from left to right. Each match starts after the previous one ends.

## Testing if a pattern matches

If you only need yes/no:

```rust
let re = Regex::compile(r"\d+")?;

assert!(re.is_match("abc 123"));
assert!(!re.is_match("no digits here"));
```

`is_match` is slightly faster than `find_first` because it doesn't need to build the match result.

## Named capture groups

Capture groups let you extract specific parts of a match. Named groups make your code readable:

```rust
let re = Regex::compile(r"(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})")?;

let m = re.find_first("Date: 2026-04-06").unwrap();
let text = "Date: 2026-04-06";

// Group 0 is always the full match
println!("Full match: {}", &text[m.start..m.end]);
// Full match: 2026-04-06

// Named groups are in the groups array (1-based)
// groups[1] = year, groups[2] = month, groups[3] = day
if let Some((start, end)) = m.groups.get(1).and_then(|g| *g) {
    println!("Year: {}", &text[start..end]);
    // Year: 2026
}
```

## Common patterns

Here are some patterns you'll use often:

| Pattern | Matches | Example |
|---------|---------|---------|
| `\d+` | One or more digits | "42", "7890" |
| `\w+` | Word characters | "hello", "foo_bar" |
| `\s+` | Whitespace | " ", "\t\n" |
| `[a-zA-Z]+` | Letters only | "Hello", "world" |
| `\b\w+\b` | Whole words | "cat" in "the cat sat" |
| `.+` | Anything (except newline) | "hello world" |
| `^start` | Anchored to beginning | "start" at position 0 |
| `end$` | Anchored to end | "end" at last position |
| `a\|b\|c` | Alternatives | "a" or "b" or "c" |
| `(group)` | Capture group | captured for extraction |
| `(?:group)` | Non-capturing group | grouping without capture |
| `a{2,4}` | 2 to 4 repetitions | "aa", "aaa", "aaaa" |

## What makes rgx different

So far, everything above works in any regex engine. Here's where rgx stands apart:

1. **You can pass data INTO the match** — host variables let the same pattern behave differently based on runtime context
2. **You can run code DURING matching** — callbacks validate, transform, and decide mid-match
3. **You can control what happens next** — callbacks can accept, reject, skip, or abort
4. **You can watch the engine work** — structured events for debugging and profiling
5. **Callbacks can do async I/O** — suspend the match, query a database, resume
6. **You can match against files directly** — no manual file reading

Each of these is a chapter in this guide. Start with [Chapter 1: Passing Data In and Out](01-data-exchange.md).

## Next

[Chapter 1: Passing Data In and Out >>>](01-data-exchange.md)
