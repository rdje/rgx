# Pattern Syntax

Quick-reference tables for rgx pattern syntax. For the full PCRE2 compatibility matrix, see the [PCRE2 Compatibility](./pcre2-compatibility.md) appendix.

## Literals and escaping

| Pattern | Matches | Notes |
|---------|---------|-------|
| `a` | Literal `a` | Most characters match themselves |
| `\.` | Literal `.` | Backslash escapes metacharacters |
| `\\` | Literal `\` | |
| `\t` | Tab (U+0009) | |
| `\n` | Newline (U+000A) | |
| `\r` | Carriage return (U+000D) | |
| `\f` | Form feed (U+000C) | |
| `\a` | Bell (U+0007) | |
| `\e` | Escape (U+001B) | |
| `\xHH` | Byte value `0xHH` | `\x41` = `A` |
| `\x{HHHH}` | Unicode codepoint | `\x{1F600}` = U+1F600 |
| `\uHHHH` | Unicode codepoint | `\u00E9` = e with acute |
| `\0` | Null (U+0000) | |
| `\cX` | Control character | `\cA` = U+0001 |

## Character classes

| Pattern | Matches | Notes |
|---------|---------|-------|
| `[abc]` | Any of `a`, `b`, `c` | |
| `[a-z]` | Any letter `a` through `z` | |
| `[^abc]` | Any character except `a`, `b`, `c` | |
| `[a-zA-Z0-9]` | Alphanumeric | |
| `[\[\]]` | `[` or `]` | Escape brackets inside class |

## Shorthand classes

| Pattern | Matches | Equivalent |
|---------|---------|------------|
| `\d` | Digit | `[0-9]` (ASCII) or Unicode digit |
| `\D` | Non-digit | `[^0-9]` |
| `\w` | Word character | `[a-zA-Z0-9_]` (ASCII) or Unicode word |
| `\W` | Non-word character | `[^a-zA-Z0-9_]` |
| `\s` | Whitespace | `[ \t\n\r\f\v]` (ASCII) or Unicode space |
| `\S` | Non-whitespace | `[^ \t\n\r\f\v]` |
| `\h` | Horizontal whitespace | Space, tab, and Unicode horizontal spaces |
| `\H` | Non-horizontal whitespace | |
| `\v` | Vertical whitespace | Newline, carriage return, form feed, etc. |
| `\V` | Non-vertical whitespace | |
| `\N` | Any character except newline | Like `.` without `DOTALL` |

## Special escape sequences

| Pattern | Matches | Notes |
|---------|---------|-------|
| `\R` | Any newline sequence | `\n`, `\r\n`, `\r`, or Unicode line break |
| `\X` | Extended grapheme cluster | Full user-perceived character |
| `\K` | Reset match start | Text before `\K` is excluded from `$&` |
| `\G` | End of previous match | For contiguous matching |

## POSIX classes (inside `[...]`)

| Pattern | Matches |
|---------|---------|
| `[:alpha:]` | Alphabetic characters |
| `[:digit:]` | Digits |
| `[:alnum:]` | Alphanumeric |
| `[:upper:]` | Uppercase letters |
| `[:lower:]` | Lowercase letters |
| `[:space:]` | Whitespace |
| `[:punct:]` | Punctuation |
| `[:print:]` | Printable characters |
| `[:graph:]` | Visible characters (non-space printable) |
| `[:cntrl:]` | Control characters |
| `[:xdigit:]` | Hexadecimal digits |
| `[:ascii:]` | ASCII characters (0-127) |
| `[:word:]` | Word characters (like `\w`) |
| `[:blank:]` | Horizontal whitespace (space, tab) |

## Unicode properties

| Pattern | Matches |
|---------|---------|
| `\p{L}` | Any Unicode letter |
| `\p{Lu}` | Uppercase letter |
| `\p{Ll}` | Lowercase letter |
| `\p{N}` | Any Unicode number |
| `\p{P}` | Any Unicode punctuation |
| `\p{S}` | Any Unicode symbol |
| `\p{Z}` | Any Unicode separator |
| `\p{Greek}` | Greek script characters |
| `\p{Cyrillic}` | Cyrillic script characters |
| `\P{L}` | Negated: non-letter |

## Quantifiers

| Pattern | Meaning | Greedy | Lazy | Possessive |
|---------|---------|--------|------|------------|
| `*` | 0 or more | `a*` | `a*?` | `a*+` |
| `+` | 1 or more | `a+` | `a+?` | `a++` |
| `?` | 0 or 1 | `a?` | `a??` | `a?+` |
| `{n}` | Exactly n | `a{3}` | | |
| `{n,}` | n or more | `a{3,}` | `a{3,}?` | `a{3,}+` |
| `{n,m}` | Between n and m | `a{3,5}` | `a{3,5}?` | `a{3,5}+` |

Greedy quantifiers match as much as possible. Lazy (`?` suffix) match as little as possible. Possessive (`+` suffix) match as much as possible and never backtrack.

## Anchors

| Pattern | Matches |
|---------|---------|
| `^` | Start of string (or line in multiline mode) |
| `$` | End of string (or line in multiline mode) |
| `\A` | Start of string (always, ignoring multiline) |
| `\Z` | End of string (before optional trailing newline) |
| `\z` | End of string (absolute) |
| `\b` | Word boundary |
| `\B` | Non-word boundary |

## Groups

| Pattern | Meaning |
|---------|---------|
| `(...)` | Capturing group |
| `(?P<name>...)` | Named capturing group |
| `(?:...)` | Non-capturing group |
| `(?P=name)` | Backreference to named group |
| `\1`, `\2` | Backreference by number |
| `(?>...)` | Atomic group (no backtracking) |

## Lookaround

| Pattern | Meaning | Width |
|---------|---------|-------|
| `(?=...)` | Positive lookahead | Zero |
| `(?!...)` | Negative lookahead | Zero |
| `(?<=...)` | Positive lookbehind | Zero |
| `(?<!...)` | Negative lookbehind | Zero |

Lookbehind requires a fixed-width pattern in most cases.

## Alternation

| Pattern | Meaning |
|---------|---------|
| `a\|b` | Match `a` or `b` |
| `(?:a\|b\|c)` | Non-capturing alternation |
| `(a\|b\|c)` | Capturing alternation (branch number tracked) |

## Flags (inline modifiers)

| Flag | Meaning | Builder method |
|------|---------|---------------|
| `(?i)` | Case insensitive | `.case_insensitive()` |
| `(?m)` | Multiline (`^`/`$` match line boundaries) | `.multi_line()` |
| `(?s)` | Dotall (`.` matches `\n`) | `.dot_matches_new_line()` |
| `(?x)` | Extended (ignore whitespace, `#` comments) | `.ignore_whitespace()` |
| `(?U)` | Swap greed (quantifiers lazy by default) | `.swap_greed()` |

Flags can be scoped: `(?i:text)` applies case-insensitive only within the group.

## Conditional patterns

| Pattern | Meaning |
|---------|---------|
| `(?(1)yes\|no)` | If group 1 matched, try `yes`; else try `no` |
| `(?(name)yes\|no)` | If named group matched, try `yes`; else try `no` |

## Code blocks

| Pattern | Meaning |
|---------|---------|
| `(?{lua:code})` | Execute Lua code |
| `(?{js:code})` | Execute JavaScript code |
| `(?{rhai:code})` | Execute Rhai code |
| `(?{wasm:module:fn})` | Call WASM function |
| `(?{native:name})` | Call registered native callback |
