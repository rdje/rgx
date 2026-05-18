# Partial Matching

Sometimes you do not have the complete input. Data arrives in chunks over a network socket, a user is still typing into a form field, or you are tailing a log file that has not finished writing a line. In these situations, "no match" is not the right answer -- you need to distinguish between "definitely no match" and "might match if more data arrives."

This is what `find_first_partial` is for.

---

## The `PartialMatchResult` enum

```rust,no_run
# use rgx_core::MatchResult;
pub enum PartialMatchResult {
    /// A full match was found.
    Full(MatchResult),
    /// The input ended mid-potential-match. More data might complete it.
    /// Contains the byte offset where the partial match started.
    Partial(usize),
    /// No match and no partial match -- the pattern cannot match this input
    /// even with more data appended.
    NoMatch,
}
```

Three outcomes:

| Variant | Meaning | What to do |
|---|---|---|
| `Full(m)` | Complete match found | Process the match normally |
| `Partial(offset)` | Input ended while the pattern was still matching | Buffer the data and try again when more arrives |
| `NoMatch` | No match is possible, even with more data | Discard or skip this chunk |

The `offset` in `Partial(offset)` tells you the byte position where the potential match began, so you know where to start buffering.

---

## Basic usage

```rust
# use rgx_core::{Regex, PartialMatchResult};
let re = Regex::compile(r"hello world")?;

// Full input -- full match
match re.find_first_partial("hello world") {
    PartialMatchResult::Full(m) => {
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 11);
    }
    other => panic!("expected Full, got {other:?}"),
}

// Truncated input -- partial match
match re.find_first_partial("hello wor") {
    PartialMatchResult::Partial(offset) => {
        assert_eq!(offset, 0);   // partial match starts at position 0
    }
    other => panic!("expected Partial, got {other:?}"),
}

// Completely unrelated input -- no match
match re.find_first_partial("xyz") {
    PartialMatchResult::NoMatch => {}   // correct
    other => panic!("expected NoMatch, got {other:?}"),
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Streaming use case: matching across chunks

The primary use case for partial matching is streaming data. Here is the pattern: try to match each chunk. If you get `Partial`, buffer the data and retry when the next chunk arrives. If you get `Full`, process the match and advance. If you get `NoMatch`, discard.

```rust
# use rgx_core::{Regex, PartialMatchResult};
let re = Regex::compile(r"\d{4}-\d{2}-\d{2}")?;

// Simulate data arriving in chunks
let chunks = ["log: 202", "5-03-15 ok"];
let mut buffer = String::new();

for chunk in &chunks {
    buffer.push_str(chunk);
    match re.find_first_partial(&buffer) {
        PartialMatchResult::Full(m) => {
            let matched = &buffer[m.start..m.end];
            assert_eq!(matched, "2025-03-15");
            break;
        }
        PartialMatchResult::Partial(_offset) => {
            // Keep buffering -- more data needed
            continue;
        }
        PartialMatchResult::NoMatch => {
            // No match possible, could discard buffer
            buffer.clear();
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Memory management in streaming

In a real streaming application, you want to avoid unbounded buffer growth. The `Partial(offset)` value tells you where the potential match starts. Everything before that offset can safely be discarded:

```text
match re.find_first_partial(&buffer) {
    PartialMatchResult::Partial(offset) => {
        // Discard bytes before the partial match start
        buffer.drain(..offset);
    }
    // ...
}
```

---

## How `hit_end` works internally

Under the hood, `find_first_partial` uses a `hit_end` flag in the VM execution context. Here is the logic:

1. First, the engine attempts a normal `find_first`. If it succeeds, the result is `Full(match)`.
2. If no full match is found, the engine rescans the input. At each starting position, it runs the match and checks whether the VM hit the end of input while the pattern was *actively progressing*.
3. If `hit_end` is `true` for any starting position, the result is `Partial(offset)` with that position.
4. If no position triggers `hit_end`, the result is `NoMatch`.

The key subtlety: `hit_end` only fires when the pattern was **actively matching** at the point where input ran out. If the engine reaches the end of input while backtracking or after a failed branch, that does not count as a partial match. This prevents false positives.

### Example: the difference matters

```rust
# use rgx_core::{Regex, PartialMatchResult};
let re = Regex::compile(r"hello")?;

// "xyz" -- the pattern never even starts matching, so no hit_end
assert!(matches!(
    re.find_first_partial("xyz"),
    PartialMatchResult::NoMatch
));

// "hel" -- the pattern starts matching "h", "e", "l" and then runs out of input
assert!(matches!(
    re.find_first_partial("hel"),
    PartialMatchResult::Partial(_)
));
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Validating incomplete input

Partial matching is excellent for real-time input validation. As the user types, you can give immediate feedback:

```rust
# use rgx_core::{Regex, PartialMatchResult};
let date_re = Regex::compile(r"\d{4}-\d{2}-\d{2}")?;

fn validate_input(re: &Regex, input: &str) -> &'static str {
    match re.find_first_partial(input) {
        PartialMatchResult::Full(_) => "valid",
        PartialMatchResult::Partial(_) => "keep typing...",
        PartialMatchResult::NoMatch => "invalid",
    }
}

assert_eq!(validate_input(&date_re, "2025"), "keep typing...");
assert_eq!(validate_input(&date_re, "2025-03"), "keep typing...");
assert_eq!(validate_input(&date_re, "2025-03-15"), "valid");
assert_eq!(validate_input(&date_re, "abc"), "invalid");
# Ok::<(), Box<dyn std::error::Error>>(())
```

This gives a much better user experience than waiting until the entire field is submitted.

---

## Matching at date boundaries

Partial matching handles structured patterns well:

```rust
# use rgx_core::{Regex, PartialMatchResult};
let re = Regex::compile(r"\d{4}-\d{2}-\d{2}")?;

// Full date matches
assert!(matches!(
    re.find_first_partial("2025-03-15"),
    PartialMatchResult::Full(_)
));

// Partial date -- input ends mid-match
assert!(matches!(
    re.find_first_partial("2025-03"),
    PartialMatchResult::Partial(_)
));

// No digits at all -- pattern cannot match
assert!(matches!(
    re.find_first_partial("abc"),
    PartialMatchResult::NoMatch
));
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Edge case: empty input

Empty input gets `NoMatch` or `Partial` depending on whether the pattern can start matching at position 0. For a literal pattern like `abc`, an empty string provides no evidence of a match, so the engine reports `NoMatch` or `Partial`:

```rust
# use rgx_core::{Regex, PartialMatchResult};
let re = Regex::compile(r"abc")?;

match re.find_first_partial("") {
    PartialMatchResult::NoMatch | PartialMatchResult::Partial(_) => {
        // Both are valid outcomes for empty input
    }
    other => panic!("unexpected: {other:?}"),
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Partial matching vs. anchored patterns

Partial matching works with anchored patterns too. An `\A`-anchored pattern that partially matches at the start will report `Partial(0)`:

```rust
# use rgx_core::{Regex, PartialMatchResult};
let re = Regex::compile(r"\Ahello world\z")?;

// Anchored pattern, truncated input
match re.find_first_partial("hello wor") {
    PartialMatchResult::Partial(0) => {}   // expected
    PartialMatchResult::Partial(n) => panic!("expected offset 0, got {n}"),
    other => panic!("expected Partial, got {other:?}"),
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## When to use partial matching

| Use case | Why partial matching helps |
|---|---|
| **Network protocols** | Match packet headers that may arrive split across TCP segments |
| **Real-time validation** | Give instant feedback as the user types |
| **Log tailing** | Handle lines that are still being written |
| **Streaming parsers** | Process data as it arrives without waiting for a complete buffer |
| **Interactive search** | Highlight potential matches before the query is complete |

---

## What partial matching is *not*

Partial matching does not give you "the best match so far." It is a boolean signal: "could this become a match?" If you need the actual partial match text, extract it using the offset:

```rust
# use rgx_core::{Regex, PartialMatchResult};
let re = Regex::compile(r"hello world")?;

if let PartialMatchResult::Partial(offset) = re.find_first_partial("hello wor") {
    let partial_text = &"hello wor"[offset..];
    assert_eq!(partial_text, "hello wor");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

It also does not resume matching from where it left off. Each call to `find_first_partial` starts a fresh match attempt on the full buffer you provide. The streaming pattern is: accumulate data in a buffer, retry the match on the growing buffer, and drain consumed bytes once you get a `Full` result.
