# Finding Matches

This chapter covers every way to find matches in rgx, from the simplest to the most specialized.

## The two match types

rgx has two match return types:

| Type | What it carries | When to use |
|------|----------------|-------------|
| `Match<'t>` | Text reference + positions | 90% of the time — `m.as_str()` just works |
| `MatchResult` | Positions + groups + branch number + code result | When you need capture groups or code block values |

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;

// Match — ergonomic, borrows the text
let m = re.find("abc 42").unwrap();
assert_eq!(m.as_str(), "42");

// MatchResult — detailed, with groups
let mr = re.find_first("abc 42").unwrap();
assert_eq!(mr.start, 4);
assert_eq!(mr.end, 6);
assert_eq!(mr.groups[0], Some((4, 6)));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Lazy iteration with find_iter

`find_iter` returns matches one at a time without allocating a `Vec`. This is the idiomatic Rust way:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;

// Process matches lazily
let sum: i64 = re.find_iter("1 plus 2 plus 3")
    .filter_map(|m| m.as_str().parse::<i64>().ok())
    .sum();
assert_eq!(sum, 6);
# Ok::<(), Box<dyn std::error::Error>>(())
```

You can stop early — the engine won't compute matches you don't consume:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\w+")?;
let first_three: Vec<_> = re.find_iter("a b c d e f")
    .take(3)
    .map(|m| m.as_str().to_string())
    .collect();
assert_eq!(first_three, vec!["a", "b", "c"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Collecting all matches with find_all

When you need a `Vec` (for counting, random access, or passing to another function):

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let matches = re.find_all("a1 b22 c333");
assert_eq!(matches.len(), 3);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Shortest match

When you only need the *end position* of the first match (not the matched text), `shortest_match` is faster:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;

// Returns just the end byte offset
assert_eq!(re.shortest_match("abc 42 xyz"), Some(6));
assert_eq!(re.shortest_match("no digits"), None);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Useful for tokenizers and validators where you care about *where* a match ends, not *what* it contains.

## Position-aware matching

Start scanning from a specific byte offset instead of position 0:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "12 abc 34 xyz 56";

// From position 0: finds "12"
let m = re.find_first(text).unwrap();
assert_eq!(&text[m.start..m.end], "12");

// From position 5: skips "12", finds "34"
let m = re.find_first_at(text, 5).unwrap();
assert_eq!(&text[m.start..m.end], "34");

// Positions are always absolute (relative to start of text)
assert_eq!(m.start, 7);

// Also available: find_all_at, is_match_at, shortest_match_at
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is essential for building tokenizers and parsers where you control the scan cursor.

> **Important:** The `start` parameter must be on a UTF-8 character boundary. The methods panic if it's not.

## Summary

| What you want | Method | Returns |
|---------------|--------|---------|
| First match (ergonomic) | `re.find(text)` | `Option<Match>` |
| First match (detailed) | `re.find_first(text)` | `Option<MatchResult>` |
| All matches (lazy) | `re.find_iter(text)` | `FindIter` (iterator) |
| All matches (Vec) | `re.find_all(text)` | `Vec<MatchResult>` |
| Boolean test | `re.is_match(text)` | `bool` |
| End position only | `re.shortest_match(text)` | `Option<usize>` |
| From byte offset | `re.find_first_at(text, n)` | `Option<MatchResult>` |
