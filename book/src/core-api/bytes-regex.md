# BytesRegex

The standard `Regex` type requires valid UTF-8 input. That covers most text
processing, but the real world is messier: binary protocols embed readable
text in byte streams, log files may contain corrupted encoding, and network
traffic is just raw bytes. `BytesRegex` lets you match against `&[u8]` input
without any UTF-8 requirement.

## Why a separate type?

Rust's `&str` guarantees valid UTF-8 at the type level. The main `Regex`
takes `&str` parameters, which means the compiler will reject `&[u8]` at
compile time. `BytesRegex` relaxes this: it accepts `&[u8]` and handles the
encoding boundary internally.

The tradeoff is subtle: `.` matches individual **bytes** (not Unicode scalar
values), `\w`, `\d`, `\s` operate on **ASCII** ranges only, and Unicode
properties like `\p{L}` may not behave as expected on non-UTF-8 input. If
your data is valid UTF-8, prefer the standard `Regex` for correct Unicode
behavior.

## Creating a `BytesRegex`

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\d+")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Like `Regex`, you can also specify an execution mode for patterns with code
blocks:

```rust,ignore
# use rgx_core::bytes::BytesRegex;
# use rgx_core::ExecutionMode;
let re = BytesRegex::with_mode(r"\d+", ExecutionMode::Pure)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Basic matching

### `is_match` -- boolean test

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\d+")?;

assert!(re.is_match(b"abc 123"));
assert!(!re.is_match(b"no digits here"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `find` -- first match

`find` returns `Option<BytesMatch>`:

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\d+")?;
let m = re.find(b"abc 123 def").unwrap();

assert_eq!(m.as_bytes(), b"123");
assert_eq!(m.start(), 4);
assert_eq!(m.end(), 7);
assert_eq!(m.range(), 4..7);
assert_eq!(m.len(), 3);
assert!(!m.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `find_all` -- all matches

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\d+")?;
let matches = re.find_all(b"a1 b22 c333");

assert_eq!(matches.len(), 3);
assert_eq!(matches[0].as_bytes(), b"1");
assert_eq!(matches[1].as_bytes(), b"22");
assert_eq!(matches[2].as_bytes(), b"333");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## The `BytesMatch` type

`BytesMatch<'t>` is the byte-oriented counterpart to `Match<'t>`. Instead of
`as_str()`, it provides `as_bytes()`:

| Method | Returns | Description |
|--------|---------|-------------|
| `as_bytes()` | `&'t [u8]` | The matched byte slice |
| `start()` | `usize` | Start byte offset |
| `end()` | `usize` | End byte offset (exclusive) |
| `range()` | `Range<usize>` | Byte range `start..end` |
| `len()` | `usize` | Length in bytes |
| `is_empty()` | `bool` | Whether the match is zero-length |

## Matching non-UTF-8 input

This is the primary reason `BytesRegex` exists. When your input contains
arbitrary bytes, the standard `Regex` would reject it. `BytesRegex` handles
it gracefully:

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"abc")?;

// Input with invalid UTF-8 bytes surrounding the match
let input: &[u8] = &[0xFF, 0xFE, b'a', b'b', b'c', 0xFF];
let m = re.find(input).unwrap();

assert_eq!(m.as_bytes(), b"abc");
assert_eq!(m.start(), 2);
assert_eq!(m.end(), 5);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Matching raw byte patterns

You can use hex escapes to match specific byte values:

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\x00\x01\x02")?;
let input: &[u8] = &[0x00, 0x01, 0x02, 0x03];
let m = re.find(input).unwrap();

assert_eq!(m.as_bytes(), &[0x00, 0x01, 0x02]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Behavior on non-UTF-8 input

Understanding how the engine processes non-UTF-8 bytes is important for
writing correct patterns:

- **`.`** matches any single byte, not a Unicode scalar value. On valid UTF-8
  input this is the same as matching one character. On non-UTF-8 input, `.`
  matches exactly one byte, even if that byte is the middle of what would
  be a multi-byte sequence.

- **`\w`, `\d`, `\s`** match only their ASCII counterparts (`[a-zA-Z0-9_]`,
  `[0-9]`, `[ \t\n\r]`). They will not match Unicode letters or digits in
  non-UTF-8 mode.

- **Unicode properties** (`\p{L}`, `\p{N}`, etc.) rely on UTF-8 decoding.
  On non-UTF-8 input, they may match incorrectly or skip bytes. Avoid
  Unicode properties in byte-oriented patterns.

- **Anchors** (`^`, `$`, `\b`) work on byte positions, not character
  positions.

## Inspecting the pattern

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\d+")?;
assert_eq!(re.as_str(), r"\d+");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Use case: binary protocol parsing

Many binary protocols embed human-readable text in fixed-format frames.
For example, consider a simple protocol where messages are delimited by
`\x02` (STX) and `\x03` (ETX) with a type byte and ASCII payload:

```rust,ignore
# use rgx_core::bytes::BytesRegex;
// Match: STX, any type byte, then ASCII payload, then ETX
let re = BytesRegex::compile(r"\x02(.)([^\x03]*)\x03")?;

let frame: &[u8] = &[
    0x02,                              // STX
    b'A',                              // type = 'A'
    b'H', b'E', b'L', b'L', b'O',     // payload
    0x03,                              // ETX
    0xFF,                              // trailing garbage
];

let m = re.find(frame).unwrap();
assert_eq!(m.start(), 0);
assert_eq!(m.end(), 7);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Extracting fields from binary data

When combined with hex escapes, you can match structured binary patterns.
Here we find TLV (Type-Length-Value) entries where the type is 0x42:

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\x42")?;
let data: &[u8] = &[0x41, 0x03, b'f', b'o', b'o', 0x42, 0x02, b'h', b'i'];

let m = re.find(data).unwrap();
assert_eq!(m.start(), 5);  // found the 0x42 type byte
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Use case: scanning mixed-encoding log files

Some log files contain a mix of UTF-8, Latin-1, and binary data. Rather
than failing on invalid sequences, `BytesRegex` scans through them:

```rust,ignore
# use rgx_core::bytes::BytesRegex;
let re = BytesRegex::compile(r"\d{4}-\d{2}-\d{2}")?;

// Simulated log line with Latin-1 byte (0xE9 = e-accent in Latin-1, invalid UTF-8 start)
let line: &[u8] = b"2025-04-08 caf\xe9 service started";
let m = re.find(line).unwrap();

assert_eq!(m.as_bytes(), b"2025-04-08");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Use case: network packet inspection

When sniffing network traffic or parsing captured packets, data is raw bytes:

```rust,ignore
# use rgx_core::bytes::BytesRegex;
// Look for HTTP method at the start of a payload
let re = BytesRegex::compile(r"^(GET|POST|PUT|DELETE|PATCH) ")?;

let payload: &[u8] = b"GET /index.html HTTP/1.1\r\n";
assert!(re.is_match(payload));

let binary_payload: &[u8] = &[0x00, 0x01, 0x02];
assert!(!re.is_match(binary_payload));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## When to use `BytesRegex` vs `Regex`

| Input type | Use |
|------------|-----|
| `&str` (guaranteed UTF-8) | `Regex` |
| `String` | `Regex` |
| `&[u8]` from file I/O | `BytesRegex` |
| `&[u8]` from network socket | `BytesRegex` |
| `&[u8]` with known UTF-8 content | Either works; `Regex` is more precise |
| Mixed encoding / binary data | `BytesRegex` |

If you have a `&[u8]` that you *know* is valid UTF-8, you can convert it
with `std::str::from_utf8()` and use the standard `Regex`. This gives you
correct Unicode behavior for free. Reserve `BytesRegex` for cases where
UTF-8 validity is not guaranteed.
