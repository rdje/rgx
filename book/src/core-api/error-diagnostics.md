# Error Diagnostics

When a regex pattern fails to compile, you need more than "syntax error" -- you
need to know *what* went wrong, *where* in the pattern the problem is, and
ideally see a visual pointer to the offending character. RGX provides all of
this through the `CompileError` type and its caret-highlighting formatter.

## The error hierarchy

RGX uses a single top-level error type, `RgxError`, with two variants:

```text
RgxError
  +-- Compile(CompileError)   // pattern failed to compile
  +-- Engine(String)          // runtime engine failure
```

The `Compile` variant is what you encounter during `Regex::compile` and
related constructors. It wraps a `CompileError` that carries structured
diagnostic information.

## `CompileError` fields

| Field | Type | Description |
|-------|------|-------------|
| `message` | `String` | Human-readable error description |
| `pattern` | `Option<String>` | The original pattern string, when available |
| `offset` | `Option<usize>` | Byte offset into the pattern where the error was detected |

When both `pattern` and `offset` are present, the `format()` method
produces a multi-line diagnostic with a caret pointing to the problem:

```rust,ignore
# use rgx_core::Regex;
let err = Regex::compile(r"(abc[def").unwrap_err();
let message = format!("{err}");

// The output looks like:
// regex compile error: ...
//   (abc[def
//       ^
assert!(message.contains('^'));
```

## Anatomy of an error message

Here is what a typical compilation error looks like:

```text
regex compile error: unclosed character class
  (abc[def
      ^
```

The three lines are:

1. **Summary line**: `regex compile error: <description>`
2. **Pattern line**: the full pattern string, indented by two spaces
3. **Caret line**: spaces aligned to the error offset, then `^`

The caret points to the exact byte where the parser detected the problem.
For multi-byte characters, it aligns with the start of the character.

## Extracting error details programmatically

When you need to process errors in code (not just display them), match on
the `RgxError::Compile` variant and inspect the `CompileError`:

```rust,ignore
# use rgx_core::{Regex, RgxError};
let result = Regex::compile(r"(unclosed");

match result {
    Ok(_) => println!("compiled successfully"),
    Err(RgxError::Compile(ce)) => {
        println!("Error: {}", ce.message);
        if let Some(ref pat) = ce.pattern {
            println!("Pattern: {pat}");
        }
        if let Some(offset) = ce.offset {
            println!("At byte offset: {offset}");
        }
    }
    Err(RgxError::Engine(msg)) => {
        println!("Engine error: {msg}");
    }
}
```

### Using the `Display` impl

Both `CompileError` and `RgxError` implement `Display` and produce the
formatted output with the caret. You can use them with `println!`,
`format!`, `eprintln!`, or any logging framework:

```rust,ignore
# use rgx_core::Regex;
if let Err(e) = Regex::compile(r"[z-a]") {
    eprintln!("{e}");
    // Prints the multi-line diagnostic to stderr
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## How PGEN provides `byte_offset`

The RGX parser (powered by PGEN, the parser generator) tracks source
positions as byte offsets during tokenization. When the parser encounters
an unexpected token -- an unmatched bracket, an invalid escape, a quantifier
with no preceding atom -- it records the byte offset of the offending token
in the error.

This means the caret in the diagnostic points to where the parser *detected*
the problem, which is usually the right place. In some cases the actual
mistake might be earlier (e.g., a missing `\` before a metacharacter), but
the offset is always the parser's best guess.

### Errors without offsets

Some errors are detected after parsing, during compilation to bytecode. These
errors have a message but no `pattern` or `offset`:

```rust,ignore
# use rgx_core::{Regex, RgxError};
// This is a valid parse but might trigger a compilation-phase error
// in certain edge cases. Normally both fields are present.
let result = Regex::compile(r"\d+");
assert!(result.is_ok());
```

When `pattern` or `offset` is `None`, the `format()` method omits the
pattern and caret lines and produces just the summary.

## Common error patterns

### Unclosed groups

```text
regex compile error: unclosed group
  (abc(def
       ^
```

The caret points to the opening `(` that was never closed, or to the end
of the pattern where the parser expected `)`.

### Unclosed character classes

```text
regex compile error: unclosed character class
  [abc
  ^
```

### Invalid escape sequences

```text
regex compile error: invalid escape sequence
  \q
   ^
```

### Quantifier without atom

```text
regex compile error: quantifier without preceding atom
  +abc
  ^
```

### Invalid character range

```text
regex compile error: invalid character range
  [z-a]
    ^
```

The range `z-a` is backward (z > a in Unicode), which is an error.

## Using errors for user feedback

If your application accepts user-provided patterns, you can present the
diagnostic directly:

```rust,ignore
# use rgx_core::Regex;
fn try_compile(pattern: &str) -> String {
    match Regex::compile(pattern) {
        Ok(_) => "Pattern is valid".to_string(),
        Err(e) => format!("{e}"),
    }
}

let feedback = try_compile(r"(hello");
assert!(feedback.contains("regex compile error"));
assert!(feedback.contains('^'));
```

### Building a richer error response

For web APIs or IDEs, you might want to return structured JSON rather than
a text diagnostic:

```rust,ignore
# use rgx_core::{Regex, RgxError};
fn compile_with_diagnostic(pattern: &str) -> Result<(), (String, Option<usize>)> {
    match Regex::compile(pattern) {
        Ok(_) => Ok(()),
        Err(RgxError::Compile(ce)) => {
            Err((ce.message.clone(), ce.offset))
        }
        Err(e) => Err((format!("{e}"), None)),
    }
}

match compile_with_diagnostic(r"[unclosed") {
    Ok(()) => println!("valid"),
    Err((msg, offset)) => {
        println!("error: {msg}");
        if let Some(off) = offset {
            println!("offset: {off}");
            // An IDE could highlight the character at this offset
        }
    }
}
```

## `RegexSet` error diagnostics

When a `RegexSet` fails to compile because one of its patterns is invalid,
the error message includes the pattern index:

```rust,ignore
# use rgx_core::RegexSet;
let result = RegexSet::new(&[r"\d+", r"(bad", r"[ok]"]);
let err = result.unwrap_err();
let msg = format!("{err}");

// The message identifies which pattern failed:
// "pattern 1 ("(bad"): regex compile error: ..."
assert!(msg.contains("pattern 1"));
```

This makes it easy to identify the offending pattern in a large set.

## Tab handling in the caret line

If your pattern contains tab characters, the caret line preserves them
for correct visual alignment in terminal output:

```text
regex compile error: ...
  abc\tdef(
          ^
```

The `format()` method replaces characters before the caret position with
spaces (to maintain alignment) but preserves tabs as tabs, so the caret
lines up correctly regardless of your terminal's tab width.

## The `thiserror` integration

`RgxError` derives from `thiserror::Error`, so it integrates cleanly with
the Rust error ecosystem:

- It implements `std::error::Error`.
- It implements `Display`.
- It can be used with `?` in functions returning `Result<T, Box<dyn Error>>`.
- It works with `anyhow`, `eyre`, and other error-handling crates.

```rust,ignore
# use rgx_core::Regex;
fn process(pattern: &str) -> Result<(), Box<dyn std::error::Error>> {
    let re = Regex::compile(pattern)?;  // ? converts RgxError automatically
    assert!(re.is_match("test"));
    Ok(())
}
# process(r"test").unwrap();
```
