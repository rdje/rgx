# PCRE2 COMPATIBILITY MATRIX
Live compatibility tracker for `rgx` against PCRE2.

## Purpose
- Keep parity claims concrete and verifiable.
- Separate verified behavior from aspirational parity targets.
- Track known divergences explicitly until they are closed.
- Provide a single source of truth for what works and what doesn't.

## Status labels
- `shipped`: feature works on the default regex path with differential parity tests.
- `rgx-gap`: PCRE2 supports this but rgx does not yet.
- `partial`: rgx has limited support (details noted).
- `out-of-scope`: not a parity target.

## Rough progress estimate
- Tracked parity estimate: about `95%` of PCRE2 feature families.
- Broader real-world pattern estimate: about `90%` of PCRE2 patterns would work in rgx.
- These percentages are intentionally rough and hand-maintained.
- Update them when whole feature families move, not for tiny edge-case wins.

## Feature-by-feature parity table

### Literals, classes, and escapes

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Literal characters | Yes | Yes | `shipped` |
| Concatenation | Yes | Yes | `shipped` |
| Alternation `\|` | Yes | Yes | `shipped` |
| Dot `.` | Yes | Yes | `shipped` |
| Character classes `[abc]`, `[a-z]`, `[^...]` | Yes | Yes | `shipped` |
| POSIX classes `[[:alpha:]]`, `[[:^digit:]]` | Yes | Yes | `shipped` |
| Shorthand `\d`, `\D`, `\w`, `\W`, `\s`, `\S` | Yes | Yes | `shipped` |
| Horizontal whitespace `\h`, `\H` | Yes | Yes | `shipped` |
| Vertical whitespace `\v`, `\V` | Yes | Yes | `shipped` |
| Unicode property `\p{L}`, `\P{Greek}` | Yes | Yes | `shipped` |
| Newline sequence `\R` | Yes | Yes | `shipped` |
| Non-newline `\N` | Yes | Yes | `shipped` |
| Extended grapheme cluster `\X` | Yes | No | `rgx-gap` |
| Hex escapes `\x41`, `\x{41}` | Yes | Yes | `shipped` |
| Octal escapes `\040`, `\o{101}` | Yes | Yes | `shipped` |
| Control escapes `\cA` | Yes | Yes | `shipped` |
| Literal escapes `\n`, `\t`, `\r`, `\a`, `\e`, `\f` | Yes | Yes | `shipped` |
| Perl extended char classes `(?[...])` | Yes | Yes | `shipped` (subset with set algebra) |

### Quantifiers

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Greedy `*`, `+`, `?` | Yes | Yes | `shipped` |
| Lazy `*?`, `+?`, `??` | Yes | Yes | `shipped` |
| Possessive `*+`, `++`, `?+` | Yes | Yes | `shipped` |
| Counted `{n}`, `{n,}`, `{n,m}` | Yes | Yes | `shipped` |
| Lazy counted `{n,m}?` | Yes | Yes | `shipped` |
| Possessive counted `{n,m}+` | Yes | Yes | `shipped` |

### Groups

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Capturing `(...)` | Yes | Yes | `shipped` |
| Non-capturing `(?:...)` | Yes | Yes | `shipped` |
| Named `(?<name>...)` | Yes | Yes | `shipped` |
| Python-style named `(?P<name>...)` | Yes | Yes | `shipped` |
| Atomic `(?>...)` | Yes | Yes | `shipped` |
| Branch-reset `(?|...)` | Yes | Yes | `shipped` |
| Comment `(?#...)` | Yes | Yes | `shipped` |
| Duplicate names `(?J)` | Yes | Yes | `shipped` |

### Anchors and boundaries

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| `^` start of line/string | Yes | Yes | `shipped` (single-line default, multiline with `(?m)`) |
| `$` end of line/string | Yes | Yes | `shipped` (single-line default, multiline with `(?m)`) |
| `\A` absolute start | Yes | Yes | `shipped` |
| `\Z` end before final newline | Yes | Yes | `shipped` |
| `\z` absolute end | Yes | Yes | `shipped` |
| `\b` word boundary | Yes | Yes | `shipped` |
| `\B` non-word boundary | Yes | Yes | `shipped` |
| `\G` end of previous match | Yes | Yes | `shipped` |

### Lookarounds

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Positive lookahead `(?=...)` | Yes | Yes | `shipped` |
| Negative lookahead `(?!...)` | Yes | Yes | `shipped` |
| Positive lookbehind `(?<=...)` | Yes | Yes | `shipped` |
| Negative lookbehind `(?<!...)` | Yes | Yes | `shipped` |

### Backreferences

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Numeric `\1`, `\2` | Yes | Yes | `shipped` |
| Named `\k<name>`, `\k'name'`, `\k{name}` | Yes | Yes | `shipped` |
| Python-style `(?P=name)` | Yes | Yes | `shipped` |
| `\g{N}`, `\g{name}`, `\g<name>` | Yes | Yes | `shipped` |
| `\g<+1>`, `\g<-1>` relative | Yes | Yes | `shipped` |

### Inline flags

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Case-insensitive `(?i)`, `(?i:...)` | Yes | Yes | `shipped` (ASCII case folding) |
| Multiline `(?m)`, `(?m:...)` | Yes | Yes | `shipped` |
| Dotall `(?s)`, `(?s:...)` | Yes | Yes | `shipped` |
| Extended `(?x)`, `(?x:...)` | Yes | Yes | `shipped` |
| Flag negation `(?-i)`, `(?-i:...)` | Yes | Yes | `shipped` |
| Combined `(?ims)`, `(?i-s:...)` | Yes | Yes | `shipped` |
| Full Unicode case folding for `(?i)` | Yes | No | `partial` — ASCII only |

### Recursion and subroutines

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Whole-pattern `(?R)` | Yes | Yes | `shipped` |
| Numbered `(?1)` | Yes | Yes | `shipped` |
| Named `(?&name)`, `(?P>name)` | Yes | Yes | `shipped` |
| Returned-capture subroutines `(?1(grouplist))` | Yes | No | `rgx-gap` — PCRE2 10.47+ |
| Relative subroutines `(?+1)`, `(?-1)` | Yes | Yes | `shipped` |

### Conditionals

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Group-exists `(?(1)yes\|no)` | Yes | Yes | `shipped` |
| Relative `(?(+1)yes\|no)` | Yes | Yes | `shipped` |
| Named `(?(<name>)yes\|no)` | Yes | Yes | `shipped` |
| Recursion `(?(R)...)`, `(?(R1)...)`, `(?(R&name)...)` | Yes | Yes | `shipped` |
| DEFINE `(?(DEFINE)...)` | Yes | Yes | `shipped` |
| Lookaround conditions | Yes | Yes | `shipped` |
| VERSION conditionals `(?(VERSION>=...)...)` | Yes | No | `rgx-gap` — very rare |

### Match control

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| `\K` match-reset | Yes | Yes | `shipped` |
| `(*ACCEPT)` | Yes | Yes | `shipped` |
| `(*FAIL)` / `(*F)` | Yes | Yes | `shipped` |
| `(*SKIP)` | Yes | Yes | `shipped` |
| `(*SKIP:name)` | Yes | No | `rgx-gap` — requires `(*MARK:name)` interaction |
| `(*PRUNE)` | Yes | Yes | `shipped` |
| `(*THEN)` | Yes | Yes | `partial` — simplified as `(*PRUNE)`; full alternation-aware behavior not yet implemented |
| `(*COMMIT)` | Yes | Yes | `shipped` |
| `(*MARK:name)` / `(*:name)` | Yes | Yes | `partial` — parses and compiles as no-op; mark names not queryable |

### Mode settings

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| `(*UTF)` | Yes | No | `rgx-gap` — rgx is always UTF-8 |
| `(*UCP)` | Yes | No | `rgx-gap` |
| `(*BSR_ANYCRLF)`, `(*BSR_UNICODE)` | Yes | No | `rgx-gap` |
| `(*CRLF)`, `(*LF)`, `(*CR)`, `(*ANY)`, `(*ANYCRLF)`, `(*NUL)` | Yes | No | `rgx-gap` |

### Other

| Feature | PCRE2 | RGX | Status |
|---------|-------|-----|--------|
| Callouts `(?C)`, `(?C123)` | Yes | Yes | `shipped` (via native callback system) |
| Partial matching API | Yes | No | `rgx-gap` |
| JIT compilation | Yes | No | `rgx-gap` — speed, not functionality |
| Embedded code blocks `(?{lang:code})` | No | Yes | `out-of-scope` — rgx extension |

## Parity-verified baseline
Backed by `rgx-bench/tests/pcre2_parity.rs` with ~250 differential test cases verifying both first-match and find-all span parity.

## Known design differences
- rgx operates on **UTF-8 codepoints** by default; PCRE2 default mode operates on **bytes**. This causes differences for multi-byte characters when using byte-level classes like `\h` on non-ASCII input. In PCRE2 UTF mode (`(*UTF)`), both engines agree.

## Maintenance workflow
- When a feature moves from `rgx-gap` to `shipped`:
  1. Update this table.
  2. Add differential parity tests in `rgx-bench/tests/pcre2_parity.rs`.
  3. Update `docs/CAPABILITY_MATRIX.md`.
  4. Add a `CHANGES.md` entry.
- Keep this matrix synchronized with the live codebase state.
