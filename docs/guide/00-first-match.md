# Chapter 0: Your First Match

Welcome to rgx! You're about to learn a regex engine that does everything a normal regex engine does -- and then a whole lot more. But first things first: let's find some patterns in text.

If you've used regex before, this chapter will feel like home. If regex is new to you, you're in the right place -- we'll start from zero and build up gently. By the end of this chapter, you'll be finding matches, extracting data, and feeling confident enough to explore the rest of the guide.

## What is a regex?

A regular expression (regex) is a pattern that describes a set of strings. Instead of searching for an exact word, you describe the *shape* of what you're looking for:

- `\d+` matches one or more digits: "42", "7", "12345"
- `[a-z]+` matches one or more lowercase letters: "hello", "cat", "xyz"
- `\d{3}-\d{4}` matches a phone number pattern: "555-1234"

Think of it like a template with blanks: "I want three digits, then a dash, then four digits." The regex engine reads your template and finds every place in the text that fits.

## Compiling a pattern

In rgx, you first compile a pattern into a reusable regex object, then use it to match against text:

```rust
use rgx_core::Regex;

// Compile once, use many times
let re = Regex::compile(r"\d{3}-\d{4}")?;
```

The `r"..."` syntax is a Rust raw string -- backslashes are literal, which is exactly what regex patterns need. You don't have to double-escape `\d` as `\\d`.

If the pattern has a syntax error, `compile` returns an error instead of panicking:

```rust
let result = Regex::compile(r"[unterminated");
assert!(result.is_err());
```

Compilation is the expensive part. Once compiled, a regex can be used millions of times with no extra cost. So compile once and reuse.

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

`find_first` returns `Option<MatchResult>` -- `Some` if there's a match, `None` if not.

A `MatchResult` tells you:
- `start` -- byte position where the match begins
- `end` -- byte position where the match ends (exclusive)
- `matched_branch_number` -- which alternative matched (more on this later)
- `code_result` -- a value returned by code blocks (more on this later)

Don't worry about `matched_branch_number` and `code_result` yet -- they become important when you start writing callbacks and multi-branch patterns. For now, `start` and `end` are all you need.

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

Here's what the return value looks like conceptually:

```
text:    "hello world foo"
          ^^^^^       ^^^
          |   |       | |
matches: [             ]
  [0]: start=0,  end=5   -> "hello"
  [1]: start=6,  end=11  -> "world"
  [2]: start=12, end=15  -> "foo"
```

Each `MatchResult` gives you byte offsets. Slice the original text with `&text[m.start..m.end]` to get the matched string.

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

## More patterns in action

Let's try some real scenarios you'll encounter in practice.

### Extracting dates

```rust
let re = Regex::compile(r"\b\d{4}-\d{2}-\d{2}\b")?;

let text = "Created: 2026-01-15, Updated: 2026-04-04, Expires: 2027-12-31";
let matches = re.find_all(text);

for m in &matches {
    println!("Date: {}", &text[m.start..m.end]);
}
// Date: 2026-01-15
// Date: 2026-04-04
// Date: 2027-12-31
```

### Finding email addresses

```rust
let re = Regex::compile(r"\b[\w.+-]+@[\w.-]+\.\w{2,}\b")?;

let text = "Contact alice@example.com or bob.jones+work@company.co.uk for help";
let matches = re.find_all(text);

for m in &matches {
    println!("Email: {}", &text[m.start..m.end]);
}
// Email: alice@example.com
// Email: bob.jones+work@company.co.uk
```

### Extracting URLs

```rust
let re = Regex::compile(r"https?://[^\s)>\]]+\b")?;

let text = "Visit https://example.com or http://docs.rgx.dev/guide for more info.";
let matches = re.find_all(text);

for m in &matches {
    println!("URL: {}", &text[m.start..m.end]);
}
// URL: https://example.com
// URL: http://docs.rgx.dev/guide
```

### Splitting words out of mixed content

```rust
let re = Regex::compile(r"[a-zA-Z]+")?;

let text = "error_404: file-not-found (retry in 30s)";
let matches = re.find_all(text);

for m in &matches {
    print!("{} ", &text[m.start..m.end]);
}
// error file not found retry in s
```

### Matching multi-part structures with groups

```rust
let re = Regex::compile(
    r"(?<name>[a-zA-Z_]\w*)\s*=\s*(?<value>[^\n;]+)"
)?;

let text = "host = 127.0.0.1\nport = 8080\nmode = production";
let matches = re.find_all(text);

for m in &matches {
    if let (Some((ns, ne)), Some((vs, ve))) = (
        m.groups.get(1).and_then(|g| *g),
        m.groups.get(2).and_then(|g| *g),
    ) {
        println!("{} => {}", &text[ns..ne], &text[vs..ve]);
    }
}
// host => 127.0.0.1
// port => 8080
// mode => production
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

## Try it yourself

Here are a few patterns to experiment with. Try predicting what they match before running them:

**Pattern 1:** Extract hashtags from a tweet.

```rust
let re = Regex::compile(r"#\w+")?;
let text = "Loving #rust and #regex today! #programming";
let matches = re.find_all(text);
// What do you expect? Try it!
```

**Pattern 2:** Find all numbers (including decimals).

```rust
let re = Regex::compile(r"\d+\.?\d*")?;
let text = "Price: 19.99, Qty: 3, Tax: 1.60";
let matches = re.find_all(text);
// How many matches? What are they?
```

**Pattern 3:** Match lines that start with a comment character.

```rust
let re = Regex::compile(r"^[#;].*")?;   // ^ anchors to line start
let text = "# This is a comment\ncode = 42\n; Another comment\nmore = stuff";
// Hint: by default, ^ matches the start of the ENTIRE string.
// For per-line matching, use match_file_lines (Chapter 6).
```

**Pattern 4:** Extract key-value pairs from a query string.

```rust
let re = Regex::compile(r"([^&=]+)=([^&]*)")?;
let text = "name=alice&age=30&city=portland";
let matches = re.find_all(text);
// Each match has two capture groups. Can you extract them?
```

## Common gotchas

A few things that trip up newcomers:

**Raw strings matter.** Always use `r"..."` for regex patterns in Rust. Without the `r`, Rust interprets `\d` as an escape sequence (and complains). Compare:

```rust
// Correct: raw string, backslash is literal
let re = Regex::compile(r"\d+")?;

// Wrong: Rust tries to interpret \d as an escape character
// let re = Regex::compile("\d+")?;  // compiler warning or error
```

**Byte positions, not character positions.** `start` and `end` are byte offsets, not character indices. For pure ASCII text this doesn't matter. For text with multi-byte UTF-8 characters (like emoji or accented letters), be careful when slicing:

```rust
let text = "cafe\u{0301}";  // "cafe" + combining accent = "caf\u{e9}"
// Byte offsets work correctly with &text[m.start..m.end] because
// rgx always returns valid UTF-8 boundaries.
```

**Greedy by default.** Quantifiers like `+` and `*` match as much as possible. If you want the shortest match, use `+?` or `*?`:

```rust
let re = Regex::compile(r"<.+>")?;       // greedy: matches "<b>bold</b>"
let re = Regex::compile(r"<.+?>")?;      // lazy: matches "<b>", then "</b>"
```

**`find_all` returns non-overlapping matches.** Once a match is found, the engine advances past it. It won't find overlapping occurrences. If you need overlapping matches, you'll need a different search strategy.

## What makes rgx different

So far, everything above works in any regex engine. Here's where rgx stands apart:

1. **You can pass data INTO the match** -- host variables let the same pattern behave differently based on runtime context
2. **You can run code DURING matching** -- callbacks validate, transform, and decide mid-match
3. **You can control what happens next** -- callbacks can accept, reject, skip, or abort
4. **You can watch the engine work** -- structured events for debugging and profiling
5. **Callbacks can do async I/O** -- suspend the match, query a database, resume
6. **You can match against files directly** -- no manual file reading

Each of these is a chapter in this guide. Here's a taste of what's ahead:

- **[Chapter 1: Passing Data In and Out](01-data-exchange.md)** -- Make one compiled pattern behave differently by changing a variable at runtime. Like feature flags for your regex.
- **[Chapter 2: Predicate Callbacks](02-predicate-callbacks.md)** -- Run your own code *inside* the match. Validate an IP address while the engine is still matching it. Yes, really.
- **[Chapter 3: Steering the Match](03-match-steering.md)** -- Tell the engine "stop searching, I found what I need" or "skip ahead 1000 bytes." You're the pilot now.
- **[Chapter 4: Watching the Engine](04-structured-events.md)** -- See every step the engine takes. Like a debugger for regex, but without the headache.
- **[Chapter 5: Async Callbacks](05-async-io.md)** -- Pause the match, go query a database, come back with the answer. The engine waits patiently.
- **[Chapter 6: Working with Files](06-file-matching.md)** -- Scan log files, config files, and CSV files with one method call. Callbacks fire on every match.
- **[Chapter 7: Real-World Patterns](07-real-world.md)** -- Complete working examples you can copy, paste, and adapt.

Start with [Chapter 1: Passing Data In and Out](01-data-exchange.md).

## Next

[Chapter 1: Passing Data In and Out >>>](01-data-exchange.md)
