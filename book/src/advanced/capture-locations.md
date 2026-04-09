# CaptureLocations

When you call `captures()` on a regex, it allocates a new `Captures` object (including a `Vec` for group positions and an `Arc` for the named-group map) for every single match. For a one-off match, this is fine. But if you are processing millions of lines in a tight loop, those allocations add up.

`CaptureLocations` solves this by giving you a reusable buffer for capture group positions. You allocate it once, then pass it into `captures_read()` on every iteration. The buffer is overwritten in place -- zero allocation per match.

---

## The pattern

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(\d+)-(\w+)")?;

// 1. Create the buffer once
let mut locs = re.capture_locations();

// 2. Reuse it for every match
if re.captures_read("item 42-abc", &mut locs).is_some() {
    assert_eq!(locs.get(0), Some((5, 11)));   // "42-abc"
    assert_eq!(locs.get(1), Some((5, 7)));     // "42"
    assert_eq!(locs.get(2), Some((8, 11)));    // "abc"
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The return value of `captures_read()` is `Option<Match>` -- the overall match, borrowed from the input text. The group positions are written into `locs`.

---

## Why it matters for performance

Consider a log processor that extracts timestamps from millions of lines:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})")?;
let mut locs = re.capture_locations();

let lines = [
    "2025-03-15 10:30:45 INFO startup",
    "2025-03-15 10:30:46 DEBUG query",
    "2025-03-15 10:30:47 ERROR timeout",
];

for line in &lines {
    if let Some(m) = re.captures_read(line, &mut locs) {
        // locs is reused -- no allocation here
        let year = locs.get(1).map(|(s, e)| &line[s..e]);
        let month = locs.get(2).map(|(s, e)| &line[s..e]);
        let day = locs.get(3).map(|(s, e)| &line[s..e]);
        // process the date...
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

With `captures()`, each iteration would allocate a `Vec<Option<(usize, usize)>>`, an `Arc<HashMap<String, u32>>`, and the `Captures` struct itself. With `captures_read()`, the only allocation is the initial `capture_locations()` call.

### Ballpark impact

For a pattern with 6 capture groups run over 1 million lines:

| Method | Allocations per match | Total allocations |
|---|---|---|
| `captures()` | ~3 (Vec, Arc, Captures) | ~3,000,000 |
| `captures_read()` | 0 | 1 (the initial buffer) |

The actual performance gain depends on your allocator, but eliminating millions of small allocations in a hot loop is a well-known optimization.

---

## API reference

### `capture_locations()`

Creates a `CaptureLocations` buffer sized for this regex:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(a)(b)(c)")?;
let locs = re.capture_locations();

// 4 slots: group 0 (overall match) + groups 1, 2, 3
assert_eq!(locs.len(), 4);
assert!(!locs.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `captures_read(text, &mut locs)`

Fills `locs` with capture positions for the first match, returning the overall match:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(\w+)=(\d+)")?;
let mut locs = re.capture_locations();

let m = re.captures_read("key=42 other=7", &mut locs).unwrap();
assert_eq!(m.as_str(), "key=42");

// Group positions are byte offset pairs
assert_eq!(locs.get(0), Some((0, 6)));   // "key=42"
assert_eq!(locs.get(1), Some((0, 3)));   // "key"
assert_eq!(locs.get(2), Some((4, 6)));   // "42"
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `captures_read_at(text, start, &mut locs)`

Same as `captures_read`, but starts the scan at byte position `start`. Positions in the result are absolute (relative to the beginning of `text`, not relative to `start`):

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(\d+)")?;
let mut locs = re.capture_locations();

let m = re.captures_read_at("aa 11 bb 22", 5, &mut locs).unwrap();
assert_eq!(m.as_str(), "22");
assert_eq!(locs.get(1), Some((9, 11)));   // absolute position of "22"
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `CaptureLocations::get(i)`

Returns `Some((start, end))` for group `i`, or `None` if the group did not participate:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"a(b)?(c)")?;
let mut locs = re.capture_locations();

re.captures_read("ac", &mut locs).unwrap();
assert_eq!(locs.get(0), Some((0, 2)));   // "ac"
assert_eq!(locs.get(1), None);            // (b)? did not participate
assert_eq!(locs.get(2), Some((1, 2)));   // "c"
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Reuse across multiple inputs

The whole point is reuse. Here is the canonical loop:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(\w+)")?;
let mut locs = re.capture_locations();

let inputs = ["hello", "world"];
for input in &inputs {
    if let Some(m) = re.captures_read(input, &mut locs) {
        let word_start = locs.get(1).unwrap().0;
        let word_end = locs.get(1).unwrap().1;
        assert_eq!(&input[word_start..word_end], *input);
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Each call to `captures_read` overwrites the previous contents of `locs`. There is no need to clear or reset it.

---

## When a match fails

If `captures_read` returns `None` (no match), the contents of `locs` are **not modified**. They retain whatever values they had from the previous successful match. Always check the return value before reading from `locs`:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(\d+)")?;
let mut locs = re.capture_locations();

// First call succeeds
re.captures_read("42", &mut locs).unwrap();
assert_eq!(locs.get(1), Some((0, 2)));

// Second call fails -- locs still has old data
assert!(re.captures_read("abc", &mut locs).is_none());
// Don't read locs here -- the values are stale!
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Comparison with `captures()`

| Feature | `captures()` | `captures_read()` |
|---|---|---|
| Returns | `Option<Captures>` | `Option<Match>` + fills `locs` |
| Access by index | `caps.get(i)` returns `Match` | `locs.get(i)` returns `(usize, usize)` |
| Access by name | `caps.name("foo")` | Not available -- use the offset |
| Allocation | New `Vec` + `Arc` per call | Zero (buffer reused) |
| Best for | One-off matches, named groups | Hot loops, millions of matches |
| Ergonomics | High -- direct string access | Lower -- manual slicing |

### When to use which

- **Use `captures()`** when you are doing a single match or a small number of matches, and you want ergonomic access to matched text by name or index.
- **Use `captures_read()`** when you are processing a large volume of data in a tight loop and allocation overhead matters.

---

## Extracting text from `CaptureLocations`

Since `CaptureLocations` stores byte offset pairs, not string slices, you need to index into the original text yourself:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})-(?P<day>\d{2})")?;
let mut locs = re.capture_locations();
let text = "date: 2025-03-15";

if re.captures_read(text, &mut locs).is_some() {
    let year = locs.get(1).map(|(s, e)| &text[s..e]).unwrap();
    let month = locs.get(2).map(|(s, e)| &text[s..e]).unwrap();
    let day = locs.get(3).map(|(s, e)| &text[s..e]).unwrap();
    assert_eq!(year, "2025");
    assert_eq!(month, "03");
    assert_eq!(day, "15");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Note that named groups are accessed by their numeric index (1, 2, 3), not by name. If you need name-based access, use `captures()` instead, or maintain your own name-to-index mapping.

---

## A complete high-performance example

Here is a realistic example: extracting key-value pairs from a large number of log lines.

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"(\w+)=(\S+)")?;
let mut locs = re.capture_locations();
let mut results: Vec<(String, String)> = Vec::new();

let log_lines = [
    "level=INFO msg=startup",
    "level=ERROR msg=timeout",
    "level=DEBUG msg=query",
];

for line in &log_lines {
    // Use captures_read_at to find all pairs in each line
    let mut start = 0;
    while let Some(m) = re.captures_read_at(line, start, &mut locs) {
        let key = locs.get(1).map(|(s, e)| &line[s..e]).unwrap();
        let val = locs.get(2).map(|(s, e)| &line[s..e]).unwrap();
        results.push((key.to_string(), val.to_string()));
        start = m.end();
    }
}

assert_eq!(results.len(), 6);
assert_eq!(results[0], ("level".to_string(), "INFO".to_string()));
assert_eq!(results[1], ("msg".to_string(), "startup".to_string()));
# Ok::<(), Box<dyn std::error::Error>>(())
```

This loop processes an arbitrary number of log lines with exactly one allocation for capture storage.
