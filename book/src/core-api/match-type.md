# The Match Type

When you search for a pattern, you usually want to know three things: *what*
matched, *where* it matched, and *how* to get at the captured subgroups. RGX
provides two types that answer these questions at different levels of detail:
the lightweight `Match<'t>` and the richer `MatchResult`.

This chapter explains both, when to reach for each, and how they connect.

## `Match<'t>` -- the ergonomic handle

`Match<'t>` is what you get from [`Regex::find`] and from
[`Captures::get`]. It borrows the original input, so you can extract the
matched text without copying:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\b\d{4}\b")?;
let m = re.find("The year 2025 was interesting").unwrap();

assert_eq!(m.as_str(), "2025");
assert_eq!(m.start(), 9);
assert_eq!(m.end(), 13);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Method reference

| Method | Returns | Description |
|--------|---------|-------------|
| `as_str()` | `&'t str` | The matched substring, borrowed from the input |
| `start()` | `usize` | Byte offset of the match start |
| `end()` | `usize` | Byte offset *just past* the match end |
| `range()` | `Range<usize>` | The half-open byte range `start..end` |
| `len()` | `usize` | Length in bytes (`end - start`) |
| `is_empty()` | `bool` | Whether the match is zero-length |

### Using `range()` for slicing

Because `range()` returns a standard `Range<usize>`, you can use it directly
as a slice index:

```rust
# use rgx_core::Regex;
let text = "error at line 42: overflow";
let re = Regex::compile(r"\d+")?;
let m = re.find(text).unwrap();

// Slice the original string with the match range
assert_eq!(&text[m.range()], "42");
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Zero-length matches and `is_empty()`

Some patterns can match the empty string -- anchors, lookaheads, and
quantifiers with a `{0,n}` lower bound, for example:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"^")?;
let m = re.find("hello").unwrap();

assert_eq!(m.start(), 0);
assert_eq!(m.end(), 0);
assert!(m.is_empty());
assert_eq!(m.len(), 0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is entirely valid -- it means the pattern matched the *position* between
characters rather than any characters themselves. The `is_empty()` predicate
lets you distinguish positional matches from content matches in generic code.

### Multi-byte characters and byte offsets

All positions in `Match` are **byte** offsets, not character offsets. When your
input contains multi-byte UTF-8 sequences this distinction matters:

```rust
# use rgx_core::Regex;
let text = "cafe\u{0301}";  // "cafe" + combining accent = "caf\u{e9}" visual
let re = Regex::compile(r"\x{0301}")?;
let m = re.find(text).unwrap();

// The combining accent is 2 bytes in UTF-8
assert_eq!(m.start(), 4);
assert_eq!(m.end(), 6);
assert_eq!(m.len(), 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

If you need a character index, convert with
`text[..m.start()].chars().count()`.

## `MatchResult` -- the full picture

While `Match<'t>` is great for most use cases, sometimes you need more:
captured subgroups, the branch that won an alternation, or a code-block return
value. That is what `MatchResult` provides.

`MatchResult` is returned by lower-level methods like `find_first` and
`find_all`:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(\d{4})-(\d{2})-(\d{2})")?;
let mr = re.find_first("Born on 1990-05-17").unwrap();

assert_eq!(mr.start, 8);
assert_eq!(mr.end, 18);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Fields of `MatchResult`

| Field | Type | Description |
|-------|------|-------------|
| `start` | `usize` | Byte offset of the match start |
| `end` | `usize` | Byte offset just past the match end |
| `groups` | `Vec<Option<(usize, usize)>>` | Capture groups as `(start, end)` byte pairs |
| `matched_branch_number` | `Option<usize>` | 1-based index of the winning alternation branch |
| `code_result` | `Option<CodeBlockValue>` | Return value from inline code blocks |

### The `groups` field

Index 0 in `groups` is the overall match -- it always holds `Some((start,
end))` when a match succeeds. Indices 1 through N correspond to numbered
capture groups in the pattern. Groups that did not participate in the match
(e.g., inside an unmatched alternation branch) are `None`:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(a)|(b)")?;
let mr = re.find_first("b").unwrap();

// Group 0: overall match
assert_eq!(mr.groups[0], Some((0, 1)));
// Group 1: (a) -- did NOT participate
assert_eq!(mr.groups[1], None);
// Group 2: (b) -- matched
assert_eq!(mr.groups[2], Some((0, 1)));
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is essential for patterns with alternations where not every branch has the
same set of groups.

### Extracting matched text from groups

Because `MatchResult` does not borrow the input text, you need the original
string to extract substrings:

```rust
# use rgx_core::Regex;
let text = "2025-04-08";
let re = Regex::compile(r"(\d{4})-(\d{2})-(\d{2})")?;
let mr = re.find_first(text).unwrap();

let year  = mr.groups[1].map(|(s, e)| &text[s..e]);
let month = mr.groups[2].map(|(s, e)| &text[s..e]);
let day   = mr.groups[3].map(|(s, e)| &text[s..e]);

assert_eq!(year, Some("2025"));
assert_eq!(month, Some("04"));
assert_eq!(day, Some("08"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

If you find this boilerplate tedious, use `Regex::captures` instead -- it
returns a [`Captures`] type with `get(i)` and `name(n)` methods that hand
you a `Match<'t>` directly.

## `Match<'t>` vs `MatchResult` -- when to use which

| Scenario | Use | Why |
|----------|-----|-----|
| "What substring matched?" | `find` -> `Match` | Borrows text, easy `.as_str()` |
| "Where did groups match?" | `find_first` -> `MatchResult` | Groups as byte pairs |
| "Named group access" | `captures` -> `Captures` | `.name("foo")` returns `Match` |
| "High-throughput scanning" | `find_iter` -> `Match` stream | Lazy, no `Vec` allocation |
| "Branch detection in alternation" | `find_first` -> `MatchResult` | `.matched_branch_number` |

A good rule of thumb: start with `find` and `captures` for readability, and
drop to `find_first` / `find_all` when you need `MatchResult` fields that
`Match` does not expose.

## Converting between the two

`Regex::find` is a thin wrapper around `find_first` -- it maps `MatchResult`
into `Match<'t>` by attaching the text reference:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "item 42";

// These produce equivalent position information:
let via_find  = re.find(text).unwrap();
let via_first = re.find_first(text).unwrap();

assert_eq!(via_find.start(), via_first.start);
assert_eq!(via_find.end(), via_first.end);
assert_eq!(via_find.as_str(), &text[via_first.start..via_first.end]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Edge cases to keep in mind

### Empty input

```rust
# use rgx_core::Regex;
let re = Regex::compile(r".*")?;
let m = re.find("").unwrap();
assert!(m.is_empty());
assert_eq!(m.as_str(), "");
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Match at the very end of input

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+$")?;
let m = re.find("hello 99").unwrap();
assert_eq!(m.as_str(), "99");
assert_eq!(m.end(), 8);  // == text.len()
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Groups that did not participate

This comes up with optional groups and alternations. Always check for `None`
when accessing `MatchResult.groups`:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(a)(b)?(c)")?;
let mr = re.find_first("ac").unwrap();

assert_eq!(mr.groups[1], Some((0, 1)));  // (a) matched
assert_eq!(mr.groups[2], None);          // (b)? did not participate
assert_eq!(mr.groups[3], Some((1, 2)));  // (c) matched
# Ok::<(), Box<dyn std::error::Error>>(())
```
