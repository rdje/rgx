# Iterators

RGX provides a family of lazy iterators that let you consume matches one at a
time without collecting everything into a `Vec` up front. This matters when
you are scanning large texts, streaming data, or simply want to stop early
after finding what you need.

This chapter covers every iterator type, explains the `FusedIterator`
guarantee they all share, and shows you when to prefer iterators over the
`Vec`-returning methods.

## Why iterators instead of vectors?

The `Vec`-returning methods like `find_all` and `split` are convenient, but
they do all the work before returning:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\w+")?;
let words = re.find_all("a very long document ...");
// ^ Every match has already been found and stored in a Vec
# Ok::<(), Box<dyn std::error::Error>>(())
```

Iterators defer that work:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\w+")?;
for m in re.find_iter("a very long document ...") {
    // Each match is found only when .next() is called.
    // We can break early and skip the rest of the input.
    if m.as_str() == "long" {
        break;
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The benefits are:

- **Early termination** -- stop processing as soon as you have what you need.
- **Lower peak memory** -- only one match is alive at a time.
- **Composability** -- chain with `.map()`, `.filter()`, `.take()`, `.zip()`,
  and the rest of the `Iterator` ecosystem.

## `FindIter` -- successive match positions

Created by `Regex::find_iter`, this yields `Match<'t>` values for each
non-overlapping match:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "10 + 20 = 30";

let numbers: Vec<&str> = re.find_iter(text)
    .map(|m| m.as_str())
    .collect();

assert_eq!(numbers, vec!["10", "20", "30"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### How it advances

After each match, the iterator moves the scan cursor to the *end* of the
previous match. This means matches never overlap. When a zero-width match
is found at the same position as the end of the previous match, the iterator
automatically advances by one character to avoid an infinite loop:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\b")?;
let text = "hi";
let positions: Vec<usize> = re.find_iter(text)
    .map(|m| m.start())
    .collect();
// Word boundaries at: start of "h", between "i" and end, end of string
// The iterator yields distinct positions, never getting stuck.
assert!(!positions.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Comparing with `find_all`

`find_all` returns `Vec<MatchResult>` (with capture groups and code results),
while `find_iter` yields `Match<'t>` (just text, start, end). Choose
`find_iter` when you only need matched substrings and positions. Choose
`find_all` when you need the full `MatchResult` with groups.

## `CaptureIter` -- matches with capture groups

Created by `Regex::captures_iter`, this yields `Captures<'t>` values. Each
`Captures` gives you access to numbered and named groups:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<key>\w+)=(?P<val>\w+)")?;
let text = "host=localhost port=8080 debug=true";

for caps in re.captures_iter(text) {
    let key = caps.name("key").unwrap().as_str();
    let val = caps.name("val").unwrap().as_str();
    println!("{key} => {val}");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is particularly useful for log parsing, config-file extraction, and any
scenario where you need structured data from each match.

### When to use `CaptureIter` vs `FindIter`

If you only need the full match text, `FindIter` is lighter -- it skips
capture group bookkeeping. Reach for `CaptureIter` when you actually need
the subgroup contents.

## `SplitIter` -- splitting text lazily

Created by `Regex::split_iter`, this yields the substrings *between*
matches:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"[,;\s]+")?;
let text = "one, two; three   four";

let parts: Vec<&str> = re.split_iter(text).collect();
assert_eq!(parts, vec!["one", "two", "three", "four"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The final element always contains the text after the last match (or the
entire input if there was no match at all).

### Comparing with `split`

`Regex::split` returns a `Vec<&str>` directly. `split_iter` is the lazy
equivalent -- prefer it when you want to stop after a certain number of
pieces or when memory matters.

## `SplitNIter` -- splitting with a limit

Created by `Regex::splitn_iter`, this is like `SplitIter` but stops after
yielding `limit` pieces. The final piece contains the unsplit remainder:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r",")?;
let text = "a,b,c,d,e";

let parts: Vec<&str> = re.splitn_iter(text, 3).collect();
assert_eq!(parts, vec!["a", "b", "c,d,e"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is the lazy version of `Regex::splitn`. It is ideal for parsing
structured lines where you know the first N fields are fixed but the rest
is free-form text:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\s+")?;
let line = "ERROR 2025-04-08 10:30:00 disk full on /var/log";

// Split into at most 4 parts: level, date, time, message
let parts: Vec<&str> = re.splitn_iter(line, 4).collect();
assert_eq!(parts[0], "ERROR");
assert_eq!(parts[1], "2025-04-08");
assert_eq!(parts[2], "10:30:00");
assert_eq!(parts[3], "disk full on /var/log");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## `CaptureNames` -- enumerating group names

Created by `Regex::capture_names`, this iterates over all capture group slots
(0 through N). It yields `None` for unnamed groups and `Some("name")` for
named groups:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<year>\d{4})-(\d{2})-(?P<day>\d{2})")?;
let names: Vec<Option<&str>> = re.capture_names().collect();

assert_eq!(names[0], None);          // group 0 is always unnamed
assert_eq!(names[1], Some("year"));
assert_eq!(names[2], None);          // unnamed group
assert_eq!(names[3], Some("day"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is useful for building dynamic output -- for example, when you want to
produce a JSON object whose keys are the group names:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<host>[^:]+):(?P<port>\d+)")?;
if let Some(caps) = re.captures("localhost:8080") {
    for (i, name) in re.capture_names().enumerate() {
        if let Some(name) = name {
            let value = caps.get(i).map(|m| m.as_str()).unwrap_or("");
            println!("  \"{name}\": \"{value}\"");
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`CaptureNames` implements `ExactSizeIterator`, so you can call `.len()` to
know how many groups exist without iterating.

## `SubCaptureMatches` -- iterating within a `Captures`

Once you have a `Captures<'t>` object (from `captures()` or
`captures_iter()`), you can iterate over its groups using `Captures::iter()`,
which returns `SubCaptureMatches`:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(\d+)-(\d+)-(\d+)")?;
let caps = re.captures("2025-04-08").unwrap();

let parts: Vec<Option<&str>> = caps.iter()
    .map(|slot| slot.map(|m| m.as_str()))
    .collect();

assert_eq!(parts, vec![
    Some("2025-04-08"),  // group 0: full match
    Some("2025"),         // group 1
    Some("04"),           // group 2
    Some("08"),           // group 3
]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Each item is `Option<Match<'t>>` -- `None` for groups that did not
participate in the match.

`SubCaptureMatches` implements `ExactSizeIterator`, so `.len()` returns
the total number of groups (including group 0).

## The `FusedIterator` guarantee

All RGX iterators (`FindIter`, `CaptureIter`, `SplitIter`, `SplitNIter`)
implement `FusedIterator`. This means that once the iterator returns `None`,
it will *always* return `None` on subsequent calls to `.next()`. You never
need to worry about an exhausted iterator suddenly producing a value again.

This is important when you are using iterators in contexts that may call
`.next()` after exhaustion -- for example, `Iterator::fuse()` is a no-op
on a `FusedIterator`, and combinators like `.chain()` can optimize better.

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let mut iter = re.find_iter("42");

assert!(iter.next().is_some());
assert!(iter.next().is_none());
// Fused: calling again is safe and still returns None
assert!(iter.next().is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Lazy evaluation in practice

### Taking just the first N matches

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\w+")?;
let text = "the quick brown fox jumps over the lazy dog";

let first_three: Vec<&str> = re.find_iter(text)
    .take(3)
    .map(|m| m.as_str())
    .collect();

assert_eq!(first_three, vec!["the", "quick", "brown"]);
// The rest of the input was never scanned.
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Filtering matches

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "1 2 3 10 20 30 100 200 300";

let big_numbers: Vec<&str> = re.find_iter(text)
    .filter(|m| m.as_str().len() >= 2)  // only 2+ digit numbers
    .map(|m| m.as_str())
    .collect();

assert_eq!(big_numbers, vec!["10", "20", "30", "100", "200", "300"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Finding the first match that satisfies a condition

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "item 3 weighs 150 grams";

let over_100 = re.find_iter(text)
    .find(|m| m.as_str().parse::<u32>().unwrap_or(0) > 100);

assert_eq!(over_100.unwrap().as_str(), "150");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Summary: choosing the right approach

| Need | Method | Returns |
|------|--------|---------|
| All matches in a Vec | `find_all` | `Vec<MatchResult>` |
| Lazy match stream | `find_iter` | `FindIter` -> `Match<'t>` |
| Lazy matches with groups | `captures_iter` | `CaptureIter` -> `Captures<'t>` |
| Split into Vec | `split` | `Vec<&str>` |
| Lazy split | `split_iter` | `SplitIter` -> `&'t str` |
| Lazy split with limit | `splitn_iter` | `SplitNIter` -> `&'t str` |
| Group names | `capture_names` | `CaptureNames` -> `Option<&str>` |
| Groups inside a Captures | `Captures::iter` | `SubCaptureMatches` -> `Option<Match>` |

Start with the lazy iterators. Fall back to `Vec`-returning methods only when
you genuinely need random access to all results at once.
