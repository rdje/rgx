# Position-Aware Matching

Most regex APIs start scanning from the beginning of the input. That works
for simple searches, but falls short when you are building a tokenizer,
resuming from a previous match, or implementing a custom parser. RGX provides
a set of `*_at` methods that let you control *where* scanning begins, plus
`shortest_match` variants for when you only need to know *that* a match ends
somewhere, not what it contains.

## The `*_at` family

### `find_first_at` -- find one match starting at an offset

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "abc 123 def 456";

// Start scanning at byte 8 -- skips "abc 123 "
let m = re.find_first_at(text, 8).unwrap();
assert_eq!(m.start, 12);  // found "456"
assert_eq!(m.end, 15);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Key point: the returned positions are **absolute** -- they refer to offsets
in the original `text`, not relative to the `start` parameter. This means
you can use them directly to slice the input string without adjustment.

### `find_all_at` -- find all matches starting at an offset

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "11 22 33 44 55";

// Start scanning at byte 6 -- skips "11 22 "
let matches = re.find_all_at(text, 6);
let values: Vec<&str> = matches.iter()
    .map(|m| &text[m.start..m.end])
    .collect();

assert_eq!(values, vec!["33", "44", "55"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `is_match_at` -- boolean test from an offset

When you only need to know whether a pattern matches *somewhere* from a given
position:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "abc 123";

assert!(!re.is_match_at(text, 0));  // no digit at position 0
assert!(re.is_match_at(text, 4));   // "123" starts at position 4
# Ok::<(), Box<dyn std::error::Error>>(())
```

Note that `is_match_at` checks whether a match exists starting the scan
*at or after* the given position -- the match itself may begin later.

## `shortest_match` and `shortest_match_at`

Sometimes you do not need the matched text or even the start position -- you
just need the end offset (or confirmation that a match exists). This is
common in tokenizers that use the end offset to advance a cursor.

### `shortest_match` -- end offset of the first match

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"[a-z]+")?;
let text = "  hello world";

let end = re.shortest_match(text).unwrap();
assert_eq!(end, 7);  // "hello" ends at byte 7
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `shortest_match_at` -- end offset from a specific position

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"[a-z]+")?;
let text = "  hello world";

let end = re.shortest_match_at(text, 8).unwrap();
assert_eq!(end, 13);  // "world" ends at byte 13
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Why these exist: building a tokenizer

The core motivation for position-aware matching is **tokenization**. A
tokenizer reads a stream of characters and produces a stream of typed tokens.
The classic approach is a loop:

1. At the current cursor position, try each token pattern.
2. The first pattern that matches produces a token.
3. Advance the cursor past the match.
4. Repeat.

Here is a minimal tokenizer built with `find_first_at`:

```rust
# use rgx_core::Regex;
#[derive(Debug, PartialEq)]
enum Token<'a> {
    Number(&'a str),
    Ident(&'a str),
    Plus,
    Whitespace,
}

let patterns = vec![
    (Regex::compile(r"\d+")?, "number"),
    (Regex::compile(r"[a-zA-Z_]\w*")?, "ident"),
    (Regex::compile(r"\+")?, "plus"),
    (Regex::compile(r"\s+")?, "ws"),
];

let input = "x + 42";
let mut cursor = 0;
let mut tokens = Vec::new();

while cursor < input.len() {
    let mut matched = false;
    for (re, kind) in &patterns {
        if let Some(mr) = re.find_first_at(input, cursor) {
            // Only accept matches that start exactly at the cursor
            if mr.start == cursor {
                let text = &input[mr.start..mr.end];
                let tok = match *kind {
                    "number" => Token::Number(text),
                    "ident"  => Token::Ident(text),
                    "plus"   => Token::Plus,
                    "ws"     => Token::Whitespace,
                    _        => unreachable!(),
                };
                tokens.push(tok);
                cursor = mr.end;
                matched = true;
                break;
            }
        }
    }
    if !matched {
        panic!("unexpected character at position {cursor}");
    }
}

assert_eq!(tokens, vec![
    Token::Ident("x"),
    Token::Whitespace,
    Token::Plus,
    Token::Whitespace,
    Token::Number("42"),
]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The key detail is `if mr.start == cursor` -- we only accept a match that
begins exactly at our scan position. Without `find_first_at`, we would have
to re-slice the input on every iteration, which changes byte offsets and
makes error reporting harder.

### Using `shortest_match_at` for faster tokenizer probing

If your tokenizer only needs to know *which* pattern matches (not the match
text), `shortest_match_at` avoids constructing a full `MatchResult`:

```rust
# use rgx_core::Regex;
let number = Regex::compile(r"\d+")?;
let ident  = Regex::compile(r"[a-zA-Z_]\w*")?;

let text = "  count42";
let cursor = 2;

// Probe each pattern -- shortest_match_at is the lightest check
if let Some(end) = number.shortest_match_at(text, cursor) {
    // Only accept if the match starts at cursor
    if let Some(mr) = number.find_first_at(text, cursor) {
        if mr.start == cursor {
            assert_eq!(&text[cursor..end], "count42");
            // ... this won't match because "count42" starts with a letter
        }
    }
}
if let Some(end) = ident.shortest_match_at(text, cursor) {
    assert_eq!(end, 9);  // "count42" ends at 9
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## The UTF-8 boundary requirement

All `*_at` methods take a byte offset as the `start` parameter. This offset
**must** fall on a UTF-8 character boundary. If it lands in the middle of a
multi-byte sequence, the method will panic:

```rust,should_panic
# use rgx_core::Regex;
let re = Regex::compile(r".")?;
let text = "\u{00e9}tude";  // "etude" with e-accent: 2 bytes for the e

// Byte 1 is in the middle of the 2-byte e-accent -- this panics!
let _ = re.find_first_at(text, 1);
```

To stay safe, always advance your cursor using the `end` offset of the
previous match (which is always on a boundary), or use
`str::is_char_boundary()` to validate:

```rust
# use rgx_core::Regex;
let text = "\u{00e9}tude";

// The first character is 2 bytes, so the next boundary is at 2
assert!(!text.is_char_boundary(1));
assert!(text.is_char_boundary(2));

let re = Regex::compile(r"\w+")?;
let m = re.find_first_at(text, 2).unwrap();
assert_eq!(&text[m.start..m.end], "tude");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Absolute positions in practice

The fact that positions are absolute (not relative to `start`) has a nice
consequence: you can build a list of all matches across multiple scan passes
and their positions will be consistent:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;
let text = "a1b22c333";

// Scan in two phases
let first = re.find_first_at(text, 0).unwrap();
let second = re.find_first_at(text, first.end).unwrap();
let third = re.find_first_at(text, second.end).unwrap();

// Positions are all relative to the start of `text`
assert_eq!(&text[first.start..first.end], "1");
assert_eq!(&text[second.start..second.end], "22");
assert_eq!(&text[third.start..third.end], "333");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Combining with anchors

Position-aware matching works naturally with start-of-string anchors. When
you use `^` (without multi-line mode), it only matches at position 0.
However, `find_first_at(text, 5)` starts scanning at 5 -- the `^` will not
match there. If you want "match exactly at this position", use a pattern
without the `^` anchor and check `mr.start == cursor` yourself, as shown
in the tokenizer example above.

If you are using `\A` (absolute start-of-text), the behavior is the same --
`\A` only matches at byte 0 regardless of where scanning begins.

## Summary

| Method | Returns | Use when... |
|--------|---------|-------------|
| `find_first_at(text, n)` | `Option<MatchResult>` | You need the first match from offset `n` |
| `find_all_at(text, n)` | `Vec<MatchResult>` | You need all matches from offset `n` |
| `is_match_at(text, n)` | `bool` | You only need yes/no from offset `n` |
| `shortest_match(text)` | `Option<usize>` | You only need the end offset of the first match |
| `shortest_match_at(text, n)` | `Option<usize>` | End offset of the first match from offset `n` |

These methods turn RGX from a search tool into a parsing primitive. Every
tokenizer, lexer, and incremental scanner in this book relies on them.
