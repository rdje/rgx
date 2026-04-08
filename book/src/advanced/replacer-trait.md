# The Replacer Trait

The `replace`, `replace_all`, and `replacen` methods on `Regex` all accept any type that implements the `Replacer` trait. This gives you a pluggable replacement strategy: from simple string templates to closures to custom types that can do anything.

---

## The trait definition

```rust,ignore
pub trait Replacer {
    /// Append the replacement text for `caps` to `dst`.
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String);

    /// If the replacement is a fixed string with no capture references,
    /// return it here. This lets the engine skip capture extraction entirely.
    fn no_expansion(&mut self) -> Option<Cow<'_, str>> {
        None
    }
}
```

Two methods:

- **`replace_append`**: the core method. Given the capture groups from a match, append the replacement text to `dst`. Every `Replacer` must implement this.
- **`no_expansion`**: an optional optimization. If your replacement is a fixed string that does not reference any capture groups, return `Some(text)` here. The engine will use the string directly and skip the overhead of building `Captures` objects.

---

## Built-in implementations

rgx provides `Replacer` implementations for the types you use most often.

### `&str` and `String` -- template replacement

String replacements support `$1`, `$name`, `${name}`, `$&`, and `$$` interpolation:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<first>\w+)\s(?P<last>\w+)")?;

// Named group interpolation
let result = re.replace("Jane Doe", "$last, $first");
assert_eq!(result, "Doe, Jane");

// $& is the full match
let re = Regex::compile(r"\w+")?;
let result = re.replace_all("hello world", "[$&]");
assert_eq!(result, "[hello] [world]");

// $$ is a literal dollar sign
let re = Regex::compile(r"\d+")?;
let result = re.replace("price 42", "$$$&");
assert_eq!(result, "price $42");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Template syntax reference:

| Syntax | Meaning |
|---|---|
| `$1`, `$2`, ... | Numbered capture group |
| `$name` | Named capture group |
| `${name}` | Named group (delimited, for disambiguation) |
| `$&` | The entire match |
| `$$` | A literal `$` character |

**The `no_expansion` fast path**: when a `&str` replacement contains no `$` character at all, the `no_expansion` method returns `Some(text)`, and the engine skips capture extraction. This means a replacement like `"X"` is faster than `"$1"`, because no `Captures` object needs to be built.

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;

// Fast path: "X" has no '$', so no_expansion() fires
let result = re.replace_all("a1 b2 c3", "X");
assert_eq!(result, "aX bX cX");
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Closures -- `FnMut(&Captures) -> T`

For dynamic replacement logic, pass a closure. The closure receives a `Captures` object and returns anything that implements `AsRef<str>`:

```rust
# use rgx_core::{Regex, Captures};
let re = Regex::compile(r"\w+")?;

let result = re.replace_all("hello world", |caps: &Captures| {
    caps[0].to_uppercase()
});
assert_eq!(result, "HELLO WORLD");
# Ok::<(), Box<dyn std::error::Error>>(())
```

You get full access to all capture groups inside the closure:

```rust
# use rgx_core::{Regex, Captures};
let re = Regex::compile(r"(?P<n>\d+)")?;

let result = re.replace_all("items: 3, 7, 12", |caps: &Captures| {
    let n: i32 = caps["n"].parse().unwrap();
    (n * 2).to_string()
});
assert_eq!(result, "items: 6, 14, 24");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Closures never trigger the `no_expansion` fast path -- the engine always builds `Captures` and calls the closure for every match. This is by design: the closure needs the captures to do its work.

### `NoExpand` -- literal replacement

When your replacement string contains `$` characters that you do *not* want interpreted as capture references, wrap it in `NoExpand`:

```rust
# use rgx_core::{Regex, NoExpand};
let re = Regex::compile(r"\d+")?;

let result = re.replace("price 42", NoExpand("$$$"));
assert_eq!(result, "price $$$");   // literal $$$, not interpolated
# Ok::<(), Box<dyn std::error::Error>>(())
```

`NoExpand` always returns `Some` from `no_expansion()`, so the engine skips capture extraction entirely. This makes it the fastest possible replacement strategy.

Compare with a plain `&str`:

```rust
# use rgx_core::{Regex, NoExpand};
let re = Regex::compile(r"(\d+)")?;

// &str: "$1" is interpreted as capture group 1
let result = re.replace("price 42", "$1");
assert_eq!(result, "price 42");   // $1 expands to "42"

// NoExpand: "$1" is treated as literal text
let result = re.replace("price 42", NoExpand("$1"));
assert_eq!(result, "price $1");   // literal $1
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## The `no_expansion` fast path

Understanding the `no_expansion` optimization helps you write faster replacement code.

When the engine calls `replace_all`, it first checks `rep.no_expansion()`. If the replacer returns `Some(literal)`, the engine enters a fast path:

1. No `Captures` objects are constructed.
2. No named-group map is cloned.
3. The literal string is copied directly into the result for every match.

This matters when you are replacing thousands of matches with a fixed string.

### Which replacers trigger the fast path?

| Replacer type | `no_expansion()` returns | Fast path? |
|---|---|---|
| `&str` without `$` | `Some(text)` | Yes |
| `&str` with `$` | `None` | No |
| `String` without `$` | `Some(text)` | Yes |
| `String` with `$` | `None` | No |
| `NoExpand` | Always `Some(text)` | Always |
| Closure | Always `None` | Never |

If you know your replacement is literal (no capture references), either ensure it contains no `$` characters or wrap it in `NoExpand`.

---

## Writing a custom `Replacer`

You can implement `Replacer` on your own types for specialized replacement logic. Here is an example: a replacer that wraps every match in HTML tags based on a configurable tag name.

```rust
# use rgx_core::{Regex, Replacer, Captures};
struct HtmlWrapper {
    tag: String,
}

impl Replacer for HtmlWrapper {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        dst.push('<');
        dst.push_str(&self.tag);
        dst.push('>');
        dst.push_str(&caps[0]);
        dst.push_str("</");
        dst.push_str(&self.tag);
        dst.push('>');
    }
    // no_expansion() returns None (default) because we need the match text
}

let re = Regex::compile(r"\b\w+\b")?;
let wrapper = HtmlWrapper { tag: "b".to_string() };

let result = re.replace_all("hello world", wrapper);
assert_eq!(result, "<b>hello</b> <b>world</b>");
# Ok::<(), Box<dyn std::error::Error>>(())
```

### A stateful replacer

Since `Replacer` takes `&mut self`, your type can maintain state across replacements:

```rust
# use rgx_core::{Regex, Replacer, Captures};
struct Counter {
    count: usize,
}

impl Replacer for Counter {
    fn replace_append(&mut self, _caps: &Captures<'_>, dst: &mut String) {
        self.count += 1;
        dst.push_str(&self.count.to_string());
    }
}

let re = Regex::compile(r"\w+")?;
let counter = Counter { count: 0 };

let result = re.replace_all("a b c d", counter);
assert_eq!(result, "1 2 3 4");
# Ok::<(), Box<dyn std::error::Error>>(())
```

### A replacer with `no_expansion`

If your custom replacer always produces the same output regardless of the match, implement `no_expansion` for the fast path:

```rust
# use rgx_core::{Regex, Replacer, Captures};
# use std::borrow::Cow;
struct Redactor;

impl Replacer for Redactor {
    fn replace_append(&mut self, _caps: &Captures<'_>, dst: &mut String) {
        dst.push_str("[REDACTED]");
    }

    fn no_expansion(&mut self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed("[REDACTED]"))
    }
}

let re = Regex::compile(r"\d{3}-\d{2}-\d{4}")?;
let result = re.replace_all("SSN: 123-45-6789 and 987-65-4321", Redactor);
assert_eq!(result, "SSN: [REDACTED] and [REDACTED]");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Because `no_expansion` returns `Some`, the engine never calls `replace_append` -- it uses the literal string directly. The `replace_append` implementation is still required by the trait, but it serves as a fallback for callers that do not check `no_expansion`.

---

## `replace` vs `replace_all` vs `replacen`

All three methods accept any `Replacer`:

| Method | Behavior |
|---|---|
| `replace(text, rep)` | Replace only the first match |
| `replace_all(text, rep)` | Replace all non-overlapping matches |
| `replacen(text, n, rep)` | Replace at most `n` matches (0 means all) |

All return `Cow<str>` -- `Cow::Borrowed(text)` when there are no matches (zero allocation), `Cow::Owned(result)` otherwise.

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;

// Replace first
assert_eq!(re.replace("a1 b2 c3", "X"), "aX b2 c3");

// Replace all
assert_eq!(re.replace_all("a1 b2 c3", "X"), "aX bX cX");

// Replace first 2
assert_eq!(re.replacen("a1 b2 c3", 2, "X"), "aX bX c3");

// No match -- returns Cow::Borrowed, zero allocation
let result = re.replace("no digits", "X");
assert_eq!(result, "no digits");
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Edge cases

### Empty matches

If the pattern can match the empty string, `replace_all` will insert the replacement at every position between characters (but not twice at the same position):

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"")?;   // empty pattern matches everywhere

let result = re.replace_all("ab", "-");
// Inserts "-" before a, between a and b, and after b
assert_eq!(result, "-a-b-");
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Replacement references non-participating group

If a template references a capture group that did not participate in the match, it expands to the empty string:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"a(b)?c")?;

let result = re.replace("ac", "$1");
assert_eq!(result, "");   // group 1 did not participate
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Closure returning different types

The closure can return `String`, `&str`, `Cow<str>`, or anything that implements `AsRef<str>`:

```rust
# use rgx_core::{Regex, Captures};
let re = Regex::compile(r"\d+")?;

// Returning String
let result = re.replace_all("a1 b2", |caps: &Captures| {
    format!("[{}]", &caps[0])
});
assert_eq!(result, "a[1] b[2]");

// Returning &str
let result = re.replace_all("a1 b2", |_caps: &Captures| {
    "X"
});
assert_eq!(result, "aX bX");
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Summary

| Replacer | Use case | Fast path? |
|---|---|---|
| `&str` / `String` | Template with `$1`, `$name` | Yes, when no `$` present |
| Closure | Dynamic logic, conditional replacement | No |
| `NoExpand` | Literal replacement with `$` characters | Always |
| Custom type | Stateful replacement, domain-specific logic | If you implement `no_expansion` |

The `Replacer` trait gives you the flexibility to handle any replacement scenario, from the simplest literal swap to complex transformations, while keeping the door open for performance optimizations through `no_expansion`.
