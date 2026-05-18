# Unicode

rgx is a Unicode-native engine. Every pattern operates on Unicode code points, not raw bytes, and the Unicode-aware features described in this chapter are available out of the box -- no feature flags, no special modes.

This chapter covers five capabilities:

1. Full Unicode case folding with `(?i)`
2. Extended grapheme clusters with `\X`
3. Unicode property classes with `\p{...}` and `\P{...}`
4. The newline sequence `\R`
5. The non-newline escape `\N`

---

## Full Unicode case folding with `(?i)`

When you enable case-insensitive matching -- either inline with `(?i)` or via `RegexBuilder::case_insensitive()` -- rgx applies full Unicode case folding, not just ASCII `a-z` / `A-Z` toggling. This means accented Latin, Greek, Cyrillic, and other scripts all fold correctly.

### Accented Latin

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?i)café")?;

assert!(re.is_match("café"));   // exact
assert!(re.is_match("CAFÉ"));   // all upper
assert!(re.is_match("Café"));   // title case
assert!(re.is_match("caFÉ"));   // mixed
# Ok::<(), Box<dyn std::error::Error>>(())
```

The accented `é` (U+00E9) folds to `É` (U+00C9) and vice versa. This is not something you get from a naive `to_ascii_lowercase` check -- it requires the Unicode case folding tables.

### Greek

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?i)αβγ")?;

assert!(re.is_match("αβγ"));   // lowercase
assert!(re.is_match("ΑΒΓ"));   // uppercase
assert!(re.is_match("Αβγ"));   // mixed
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Cyrillic

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?i)москва")?;

assert!(re.is_match("москва"));   // lowercase
assert!(re.is_match("МОСКВА"));   // uppercase
assert!(re.is_match("Москва"));   // title case
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Using `RegexBuilder`

If you prefer the builder API over inline flags:

```rust
# use rgx_core::RegexBuilder;
let re = RegexBuilder::new(r"café")
    .case_insensitive()
    .build()?;

assert!(re.is_match("CAFÉ"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

The builder simply prepends `(?i)` before compilation, so the result is identical.

### Case folding inside character classes

Case folding also applies inside `[...]` character classes:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?i)[àéîöü]")?;

assert!(re.is_match("À"));
assert!(re.is_match("É"));
assert!(re.is_match("Î"));
assert!(re.is_match("Ö"));
assert!(re.is_match("Ü"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When to use `(?i)`

- **User-facing search**: users expect "paris" to match "Paris" and "PARIS". Case folding gives them that for free.
- **Internationalized data**: if your data mixes scripts (logs from multilingual systems, names in different alphabets), `(?i)` makes your patterns script-agnostic.
- **Configuration parsing**: config values like `TRUE`, `True`, `true` all fold together.

### Simple case folding (not simple case mapping)

rgx implements PCRE2's `/i` semantic, which is **simple case folding** per the Unicode Character Database's `CaseFolding.txt` (`C + S` rows) — *not* the simple case mapping that `char::to_lowercase` / `char::to_uppercase` exposes. The difference is visible on a handful of codepoints whose "fold partner" is neither the upper nor lower form:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?i)s")?;

assert!(re.is_match("s"));   // obviously
assert!(re.is_match("S"));   // obviously
assert!(re.is_match("ſ"));   // LATIN SMALL LETTER LONG S (U+017F) folds to s under /i
# Ok::<(), Box<dyn std::error::Error>>(())
```

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?i)k")?;

assert!(re.is_match("k"));
assert!(re.is_match("K"));
assert!(re.is_match("K"));   // KELVIN SIGN (U+212A) folds to k under /i
# Ok::<(), Box<dyn std::error::Error>>(())
```

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?i)σ")?;

assert!(re.is_match("σ"));   // Greek small sigma
assert!(re.is_match("Σ"));   // Greek capital sigma
assert!(re.is_match("ς"));   // Greek small final sigma — all three fold together
# Ok::<(), Box<dyn std::error::Error>>(())
```

Under the hood, rgx consults `regex-syntax`'s simple-fold table (UCD `CaseFolding.txt`, `C + S` rows) to expand every character literal and character-class endpoint into its full equivalence class at compile time.

### Character-class range closure

Case folding inside a character class closes *bidirectionally* over the whole range, not just per-character. `[R-T]/i` contains not only `R`-`T` / `r`-`t` but also every character whose simple fold lands in the class — including `ſ` (U+017F, LATIN SMALL LETTER LONG S) which folds to `s`:

```rust
# use rgx_core::RegexBuilder;
let re = RegexBuilder::new(r"[R-T]+").case_insensitive().build()?;
let m = re.find_first("Ssſ").unwrap();
assert_eq!(&"Ssſ"[m.start..m.end], "Ssſ");  // full match — ſ is in the /i-closed class
# Ok::<(), Box<dyn std::error::Error>>(())
```

The same applies to Kelvin sign (U+212A → `k`), Angstrom sign (U+212B → `Å`), final sigma (U+03C2 → σ), and every other simple-fold equivalence class: if *any* char in the class folds to *any* other, both end up in the /i-closed class. rgx runs `ClassUnicode::try_case_fold_simple` over the whole class at compile time, so the closure is exact regardless of range shape.

### Edge cases

- **ASCII-only data**: `(?i)` still works correctly on pure ASCII -- `a` folds to `A` as expected. There is no performance penalty for Unicode folding when the input is ASCII.
- **Scoped flags**: `(?i)` can be scoped to a group: `(?i:hello) world` makes only "hello" case-insensitive, while "world" must match exactly.
- **Flag negation**: `(?-i)` inside a `(?i)` region disables folding for the remainder of that scope.

---

## Extended grapheme clusters with `\X`

A Unicode "character" as perceived by a human is called an *extended grapheme cluster*. It can be a single code point (`a`, `e`, `$`), or it can be a base code point followed by one or more combining marks, or an entire emoji ZWJ (Zero Width Joiner) sequence.

The `\X` escape matches exactly one extended grapheme cluster, regardless of how many code points it contains.

### Basic usage

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\X")?;

let m = re.find("hello").unwrap();
assert_eq!(m.as_str(), "h");   // simple ASCII character
# Ok::<(), Box<dyn std::error::Error>>(())
```

For plain ASCII, `\X` behaves like `.` -- it matches a single character. The power shows when combining marks are involved.

### Combining marks

The string `"e\u{0301}"` is the letter `e` followed by a combining acute accent (U+0301). Visually it renders as a single glyph. A plain `.` would match only the `e`, leaving the combining accent orphaned. `\X` matches the entire cluster:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\X")?;

let text = "e\u{0301}x";   // e + combining acute, then x
let m = re.find(text).unwrap();
assert_eq!(m.as_str(), "e\u{0301}");   // the full cluster
assert_eq!(m.len(), 3);                // e(1 byte) + U+0301(2 bytes)
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Emoji ZWJ sequences

Modern emoji like family groups are composed of multiple emoji code points joined by U+200D (Zero Width Joiner). `\X` treats the entire ZWJ sequence as one grapheme cluster:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\X")?;

let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
let m = re.find(family).unwrap();
assert_eq!(m.as_str(), family);   // entire ZWJ sequence is one grapheme
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Counting user-perceived characters

`\X` with `find_iter` gives you a reliable count of user-visible "characters":

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\X")?;

let text = "cafe\u{0301}";   // "cafe" + combining accent on the e
let count = re.find_iter(text).count();
assert_eq!(count, 4);   // c, a, f, e+accent = 4 graphemes (not 5 code points)
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When to use `\X`

- **Text truncation**: when you need to cut a string to N user-visible characters without breaking grapheme clusters.
- **Input validation**: when the requirement is "at most 20 characters" and that means perceived characters, not bytes or code points.
- **Text processing**: any time you iterate over "characters" and need to handle combining marks and emoji correctly.

### `\X` vs `.`

| Feature | `.` | `\X` |
|---|---|---|
| Matches one code point | Yes | No -- matches one *grapheme cluster* |
| Handles combining marks | No -- orphans them | Yes -- absorbs them |
| Handles ZWJ emoji | No -- matches one piece | Yes -- matches the full sequence |
| Matches newline | Only with `(?s)` | Yes (grapheme boundary rules) |

---

## Unicode property classes: `\p{...}` and `\P{...}`

Unicode assigns every code point to categories, scripts, and other properties. The `\p{Name}` escape matches any code point with that property; `\P{Name}` matches any code point *without* it.

### General categories

```rust
# use rgx_core::Regex;
// \p{L} — any Unicode letter (Latin, Greek, Cyrillic, CJK, ...)
let re = Regex::compile(r"\p{L}+")?;
assert!(re.is_match("abc"));
assert!(re.is_match("é"));
assert!(re.is_match("β"));
assert!(!re.is_match("123"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Common general categories:

| Property | Meaning | Examples |
|---|---|---|
| `\p{L}` | Letter | a, é, ñ, Ω, Д |
| `\p{N}` | Number | 0, 9, ², ¾ |
| `\p{P}` | Punctuation | ., !, ;, -- |
| `\p{S}` | Symbol | $, +, =, emoji |
| `\p{Z}` | Separator | space, non-breaking space |
| `\p{M}` | Mark | combining accents |

### Script properties

You can match by Unicode script to target a specific writing system:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\p{Greek}+")?;
assert!(re.is_match("β"));
assert!(re.is_match("Ω"));
assert!(!re.is_match("abc"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Some useful scripts: `Latin`, `Greek`, `Cyrillic`, `Arabic`, `Han`, `Hiragana`, `Katakana`, `Thai`, `Devanagari`.

### Negation with `\P{...}`

`\P{L}` matches anything that is *not* a letter:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\P{L}+")?;
assert!(re.is_match("123"));
assert!(re.is_match("!"));
assert!(!re.is_match("abc"));
assert!(!re.is_match("β"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Finding Unicode letters in mixed text

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\p{L}+")?;
let m = re.find("123β45").unwrap();
assert_eq!(m.start(), 3);
assert_eq!(m.end(), 5);   // β is 2 bytes in UTF-8
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Invalid properties

If you specify a property name that does not exist, compilation fails with a clear error:

```rust
# use rgx_core::Regex;
let result = Regex::compile(r"\p{Definitely_Not_A_Real_Property}");
assert!(result.is_err());
// Error message contains "invalid Unicode property class"
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When to use Unicode properties

- **Internationalized validation**: `\p{L}` instead of `[a-zA-Z]` when you need to accept names in any script.
- **Script detection**: `\p{Greek}`, `\p{Cyrillic}`, etc., to identify which writing system a string uses.
- **Sanitization**: `\P{L}` to strip non-letter characters while preserving all scripts.
- **Number handling**: `\p{N}` captures not just `0-9` but also superscripts, fractions, and digits from other scripts.

### `\p{L}` vs `\w`

`\w` matches `[a-zA-Z0-9_]` -- ASCII word characters only. `\p{L}` matches letters from every Unicode script. If your data is multilingual, prefer `\p{L}` (or combine: `[\p{L}\p{N}_]` for a Unicode-aware "word character").

---

## The newline sequence `\R`

Different platforms use different newline conventions: `\n` (Unix), `\r\n` (Windows), `\r` (old Mac), and Unicode defines additional line terminators. The `\R` escape matches any of them in a single token.

`\R` is equivalent to `(?:\r\n|\r|\n|\x0B|\x0C|\x85|\u{2028}|\u{2029})`, but it is expressed as a single escape for convenience.

The full list:

| Sequence | Name |
|---|---|
| `\r\n` | Carriage return + line feed (Windows) |
| `\r` | Carriage return (classic Mac) |
| `\n` | Line feed (Unix / macOS) |
| `\x0B` | Vertical tab |
| `\x0C` | Form feed |
| `\x85` | Next line (NEL, Unicode) |
| `\u{2028}` | Line separator (Unicode) |
| `\u{2029}` | Paragraph separator (Unicode) |

Note that `\R` tries `\r\n` first, so a Windows-style line ending is consumed as a single match, not as two separate matches.

### Splitting lines portably

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\R")?;

let text = "line1\nline2\r\nline3\rline4";
let lines: Vec<&str> = re.split(text);
assert_eq!(lines, ["line1", "line2", "line3", "line4"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Counting line breaks

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\R")?;

let text = "a\nb\r\nc\rd";
let breaks = re.find_iter(text).count();
assert_eq!(breaks, 3);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When to use `\R`

- **Cross-platform log parsing**: logs from Windows, Linux, and legacy systems mixed together.
- **User-submitted text**: web forms, file uploads, and copy-paste often introduce inconsistent line endings.
- **Document processing**: Unicode documents may use NEL or line/paragraph separators.

---

## The non-newline escape `\N`

`\N` matches any single character *except* a newline. It behaves like `.` in the default (non-dotall) mode, but its meaning is always "non-newline" regardless of whether `(?s)` is active.

This is useful when you explicitly want "match anything except newline" and want that intention to survive even if someone later wraps your pattern in `(?s)`.

### Basic usage

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\N+")?;

assert!(re.is_match("hello"));       // all non-newline
let m = re.find("hello\nworld").unwrap();
assert_eq!(m.as_str(), "hello");      // stops at newline
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `\N` is immune to dotall mode

```rust
# use rgx_core::Regex;
// With (?s), dot matches newlines. But \N never does.
let re = Regex::compile(r"(?s)\N+")?;

let m = re.find("hello\nworld").unwrap();
assert_eq!(m.as_str(), "hello");   // \N still stops at newline
# Ok::<(), Box<dyn std::error::Error>>(())
```

Compare with `.+` under `(?s)`:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?s).+")?;

let m = re.find("hello\nworld").unwrap();
assert_eq!(m.as_str(), "hello\nworld");   // dot crosses newline in dotall
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When to use `\N`

- **Explicit intent**: when your pattern means "non-newline" and you want that to be clear to future readers.
- **Dotall-safe patterns**: when your pattern might be embedded inside a `(?s)` context and you need to preserve line-boundary behavior.
- **Self-documenting regex**: `\N` makes it obvious that newlines are intentionally excluded, whereas `.` is ambiguous (does the author want dotall or not?).

---

## Combining Unicode features

These features compose naturally. Here is an example that uses several together:

```rust
# use rgx_core::Regex;
// Match a Unicode letter word, case-insensitively, followed by
// any newline sequence, followed by another word.
let re = Regex::compile(r"(?i)\p{L}+\R\p{L}+")?;

assert!(re.is_match("Hello\nworld"));
assert!(re.is_match("CAFÉ\r\nΩμέγα"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

And counting grapheme clusters in internationalized text:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\X")?;

// Korean Hangul syllable + combining mark example
let text = "cafe\u{0301}";
let graphemes: Vec<&str> = re.find_iter(text).map(|m| m.as_str()).collect();
assert_eq!(graphemes.len(), 4);   // c, a, f, e\u{0301}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The Unicode features in rgx follow the PCRE2 conventions, so patterns written for PCRE2 will work here with the same semantics.
