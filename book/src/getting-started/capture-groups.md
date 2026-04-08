# Capture Groups

Capture groups extract specific parts of a match. They're the bridge between "I found a pattern" and "I extracted the data."

## Named groups

Named groups are self-documenting:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})-(?P<day>\d{2})")?;

if let Some(caps) = re.captures("Event on 2026-04-09") {
    println!("{}", &caps["year"]);   // "2026"
    println!("{}", &caps["month"]);  // "04"
    println!("{}", &caps["day"]);    // "09"
    println!("{}", &caps[0]);        // "2026-04-09" (full match)
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`captures` returns `Option<Captures>`. Index by name (`caps["year"]`) or number (`caps[1]`). Group 0 is always the full match.

## The Captures type

`Captures` provides several ways to access groups:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(\w+)@(\w+)\.(\w+)")?;
let caps = re.captures("user@example.com").unwrap();

// By index — returns Option<Match>
let domain = caps.get(2).unwrap();
assert_eq!(domain.as_str(), "example");

// By name — returns Option<Match>
// (only works with named groups)

// Number of groups (including group 0)
assert_eq!(caps.len(), 4);

// Iterate all groups
for (i, group) in caps.iter().enumerate() {
    match group {
        Some(m) => println!("Group {i}: {}", m.as_str()),
        None => println!("Group {i}: (not captured)"),
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Optional groups

Groups that didn't participate in the match return `None`:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(\w+)(?: (\w+))?")?;

let caps = re.captures("hello").unwrap();
assert_eq!(&caps[1], "hello");
assert!(caps.get(2).is_none());  // optional group didn't match

let caps = re.captures("hello world").unwrap();
assert_eq!(&caps[1], "hello");
assert_eq!(&caps[2], "world");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Expanding templates

`expand` fills in a template with captured values:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<first>\w+)\s(?P<last>\w+)")?;
let caps = re.captures("Jane Doe").unwrap();

let mut out = String::new();
caps.expand("$last, $first", &mut out);
assert_eq!(out, "Doe, Jane");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Template syntax: `$1`, `$name`, `${name}`, `$&` (full match), `$$` (literal `$`).

## Iterating captures across multiple matches

`captures_iter` combines `find_iter` with capture extraction:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<key>\w+)=(?P<val>\w+)")?;

for caps in re.captures_iter("a=1 b=2 c=3") {
    println!("{} => {}", &caps["key"], &caps["val"]);
}
// a => 1
// b => 2
// c => 3
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Regex metadata

Query capture group information on the compiled regex:

```rust
# use rgx_core::Regex;
let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})-(\d{2})")?;

// Total groups (including group 0)
assert_eq!(re.captures_len(), 4);

// Group names
let names: Vec<_> = re.capture_names().collect();
assert_eq!(names, vec![None, Some("year"), Some("month"), None]);

// Original pattern
assert_eq!(re.as_str(), r"(?P<year>\d{4})-(?P<month>\d{2})-(\d{2})");

// Named group map
let ng = re.named_groups();
assert_eq!(ng["year"], 1);
assert_eq!(ng["month"], 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```
