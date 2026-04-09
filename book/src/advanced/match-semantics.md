# Match Semantics

When a regex has alternation (`a|ab`), a fundamental question arises: if both alternatives match at the same position, which one wins? The answer depends on the *match semantics*.

rgx supports two modes, controlled by the `MatchSemantics` enum:

| Variant | Rule | Convention |
|---|---|---|
| `MatchSemantics::LeftmostFirst` | First alternative that matches wins | PCRE2, Perl, Python, Java, JavaScript |
| `MatchSemantics::LeftmostLongest` | Longest match at each position wins | POSIX (awk, grep -E, lex) |

`LeftmostFirst` is the default. You switch with `set_match_semantics()`.

---

## LeftmostFirst (the default)

With leftmost-first semantics, the engine tries each alternative in order and returns the first one that succeeds. The pattern `a|ab` on the input `"ab"` matches `"a"` -- because `a` is the first branch and it matches at position 0.

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(r"a|ab")?;

let m = re.find("ab").unwrap();
assert_eq!(m.as_str(), "a");   // first alternative wins
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is the behavior you are used to if you have worked with Perl, PCRE2, Python `re`, Java `Pattern`, or JavaScript `RegExp`.

### Why leftmost-first is the default

Leftmost-first semantics are predictable and match how most developers think about alternation: "try this first, then try that." The order of branches matters, which gives you fine control over priority. It is also the convention used by the vast majority of regex libraries today.

---

## LeftmostLongest (POSIX semantics)

With leftmost-longest semantics, the engine still finds matches at the leftmost position, but among all possible matches at that position, it picks the longest one. The pattern `a|ab` on `"ab"` would match `"ab"` under POSIX rules because it is longer than `"a"`.

```rust,ignore
# use rgx_core::{Regex, MatchSemantics};
let re = Regex::compile(r"ab|a")?;
re.set_match_semantics(MatchSemantics::LeftmostLongest);

let m = re.find("ab").unwrap();
assert_eq!(m.as_str(), "ab");   // longest match wins
# Ok::<(), Box<dyn std::error::Error>>(())
```

### When to use LeftmostLongest

- **Lexer/tokenizer construction**: when you have `keyword|identifier` and a longer match should always win, regardless of branch order.
- **POSIX compatibility**: when porting patterns from `awk`, `grep -E`, or `lex` that depend on longest-match semantics.
- **Greedy by default**: when you want the engine to explore all branches and give you the maximal match.

---

## Setting the mode

Use `set_match_semantics()` on a compiled `Regex`:

```rust,ignore
# use rgx_core::{Regex, MatchSemantics};
let re = Regex::compile(r"\w+|[a-z]")?;

// Default: leftmost-first
re.set_match_semantics(MatchSemantics::LeftmostFirst);
let m = re.find("hello").unwrap();
assert_eq!(m.as_str(), "hello");   // \w+ is first, matches greedily

// Switch to leftmost-longest
re.set_match_semantics(MatchSemantics::LeftmostLongest);
let m = re.find("hello").unwrap();
assert_eq!(m.as_str(), "hello");   // same result here -- \w+ already longest
# Ok::<(), Box<dyn std::error::Error>>(())
```

The semantics can be changed at any time -- it is a runtime flag, not a compile-time decision.

---

## Patterns without alternation

For patterns that do not use `|`, the two modes behave identically. Greedy quantifiers (`+`, `*`, `{n,m}`) already produce the longest match at each position:

```rust,ignore
# use rgx_core::{Regex, MatchSemantics};
let re = Regex::compile(r"\d+")?;
re.set_match_semantics(MatchSemantics::LeftmostLongest);

let m = re.find("abc 123 def").unwrap();
assert_eq!(m.as_str(), "123");   // greedy quantifier already gives longest
# Ok::<(), Box<dyn std::error::Error>>(())
```

You only see a difference when alternation creates competing match candidates of different lengths at the same position.

---

## No match is unaffected

When the pattern cannot match at all, both modes return `None`:

```rust,ignore
# use rgx_core::{Regex, MatchSemantics};
let re = Regex::compile(r"\d+")?;
re.set_match_semantics(MatchSemantics::LeftmostLongest);

assert!(re.find("abc").is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Current limitation: alternation reordering

Today, `set_match_semantics(LeftmostLongest)` stores the flag and influences how the VM evaluates matches, but the compiler does not yet reorder alternation branches to achieve true POSIX longest-match behavior for all patterns. This means that for `a|ab` with `LeftmostLongest`, the engine currently still returns `"a"` -- because it encounters the `a` branch first and the branch-reordering optimization has not yet been implemented.

```rust,ignore
# use rgx_core::{Regex, MatchSemantics};
let re = Regex::compile(r"a|ab")?;
re.set_match_semantics(MatchSemantics::LeftmostLongest);

// Current behavior: still returns "a" (first branch)
// Full POSIX reordering is a compiler-level follow-up.
let m = re.find("ab").unwrap();
assert_eq!(m.as_str(), "a");
# Ok::<(), Box<dyn std::error::Error>>(())
```

This is tracked as a compiler-level enhancement. The `MatchSemantics` flag is stored so that when the reordering pass lands, existing code using `LeftmostLongest` will automatically benefit.

### Workaround: put longer branches first

Until the compiler reorders branches automatically, you can get POSIX-style behavior by manually placing longer alternatives before shorter ones:

```rust,ignore
# use rgx_core::{Regex, MatchSemantics};
// Instead of: a|ab  (short branch first)
// Write:      ab|a  (long branch first)
let re = Regex::compile(r"ab|a")?;
re.set_match_semantics(MatchSemantics::LeftmostLongest);

let m = re.find("ab").unwrap();
assert_eq!(m.as_str(), "ab");   // longest match, as intended
# Ok::<(), Box<dyn std::error::Error>>(())
```

This workaround works with both `LeftmostFirst` and `LeftmostLongest` semantics -- it is simply good practice for POSIX-style patterns.

---

## Practical example: building a tokenizer

A tokenizer typically has rules like "match a keyword, or an identifier, or a number." With leftmost-first, branch order determines priority:

```rust,ignore
# use rgx_core::Regex;
// Keywords first, then identifiers -- keyword branches win on ties
let re = Regex::compile(r"if|else|while|[a-zA-Z_]\w*|\d+")?;

let tokens: Vec<&str> = re.find_iter("if x42 else 7")
    .map(|m| m.as_str())
    .collect();
assert_eq!(tokens, ["if", "x42", "else", "7"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

With `LeftmostFirst`, `"if"` is matched by the `if` branch (not the identifier branch), because `if` appears earlier in the alternation. This is the expected behavior for a tokenizer with keyword priority.

If you switched to `LeftmostLongest`, the identifier branch `[a-zA-Z_]\w*` could win for inputs where it produces a longer match, which might not be what you want. Choose the semantics that match your use case.

---

## Summary

| Scenario | Recommended mode |
|---|---|
| General-purpose matching | `LeftmostFirst` (default) |
| POSIX-compatible tools | `LeftmostLongest` + longer branches first |
| Tokenizers with keyword priority | `LeftmostFirst` |
| Lexers where longest token wins | `LeftmostLongest` + longer branches first |

The mode is a runtime setting on the compiled regex, so you can experiment freely without recompiling.
