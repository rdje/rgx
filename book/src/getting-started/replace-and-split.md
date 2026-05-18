# Replace & Split

Two of the most common operations after finding matches: replacing matched text and splitting strings by a pattern.

## Replace

### Template replacement

Use `$1`, `$name`, `${name}`, `$&`, and `$$` in the replacement string:

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

### Closure replacement

For dynamic replacement logic, pass a closure:

```rust
# use rgx_core::{Regex, Captures};
let re = Regex::compile(r"\w+")?;

let result = re.replace_all("hello world", |caps: &Captures| {
    caps[0].to_uppercase()
});
assert_eq!(result, "HELLO WORLD");
# Ok::<(), Box<dyn std::error::Error>>(())
```

The closure receives a `Captures` object — full access to all groups:

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

### Literal replacement (NoExpand)

When your replacement string contains `$` that you don't want interpreted:

```rust
# use rgx_core::{Regex, NoExpand};
let re = Regex::compile(r"\d+")?;
let result = re.replace("price 42", NoExpand("$$$"));
assert_eq!(result, "price $$$");  // literal $$$, not interpolated
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Replace first vs all vs N

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\d+")?;

// Replace first occurrence only
assert_eq!(re.replace("a1 b2 c3", "X"), "aX b2 c3");

// Replace all occurrences
assert_eq!(re.replace_all("a1 b2 c3", "X"), "aX bX cX");

// Replace up to N occurrences
assert_eq!(re.replacen("a1 b2 c3", 2, "X"), "aX bX c3");
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Zero allocation on no match

`replace` and `replace_all` return `Cow<str>`. When there's no match, they return a borrowed reference to the original string — zero allocation:

```rust
# use rgx_core::Regex;
use std::borrow::Cow;

let re = Regex::compile(r"\d+")?;
let result = re.replace("no digits here", "X");
assert!(matches!(result, Cow::Borrowed(_)));  // no allocation
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Split

### Basic split

Split a string using a regex as the delimiter:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"[,\s]+")?;
let parts = re.split("one, two,  three");
assert_eq!(parts, vec!["one", "two", "three"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Split with limit

`splitn` stops after producing `limit` parts. The last part contains the unsplit remainder:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r",")?;
let parts = re.splitn("a,b,c,d,e", 3);
assert_eq!(parts, vec!["a", "b", "c,d,e"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Lazy split iterators

For large inputs, use the iterator versions to avoid allocating a `Vec`:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"\n")?;

for line in re.split_iter("line1\nline2\nline3") {
    println!("{line}");
}

// With limit
for part in re.splitn_iter("a\nb\nc\nd", 3) {
    println!("{part}");
}
// a
// b
// c\nd
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Edge cases

Empty strings between adjacent delimiters are preserved:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r",")?;
let parts = re.split(",a,,b,");
assert_eq!(parts, vec!["", "a", "", "b", ""]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

No match returns the whole string:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r",")?;
let parts = re.split("no commas");
assert_eq!(parts, vec!["no commas"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```
