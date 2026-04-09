# Tokenizer / Lexer

This example builds a fast tokenizer using rgx's branch identification feature. Each top-level alternation arm corresponds to a token type, and `matched_branch_number` tells you which arm won -- no secondary parsing needed.

## The idea

A tokenizer (or lexer) breaks input text into a sequence of classified tokens. Traditional approaches use separate regexes or manual character-by-character scanning. With rgx, you write a single alternation pattern where each branch is a token type:

```text
(?:NUMBER|IDENT|STRING|OPERATOR|WHITESPACE)
```

Branch numbers tell you which token type matched.

## A simple expression tokenizer

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult};
#[derive(Debug, PartialEq)]
enum TokenKind {
    Number,
    Ident,
    String,
    Operator,
    Paren,
    Whitespace,
    Unknown,
}

#[derive(Debug)]
struct Token {
    kind: TokenKind,
    text: String,
    offset: usize,
}

fn tokenize(input: &str) -> Vec<Token> {
    // Each top-level alternation arm is a token type.
    // Branch 1: number literals (int and float)
    // Branch 2: identifiers and keywords
    // Branch 3: double-quoted strings
    // Branch 4: operators
    // Branch 5: parentheses and brackets
    // Branch 6: whitespace
    // Branch 7: anything else (single char)
    let re = Regex::compile(
        r#"(?:(\d+(?:\.\d+)?)|([a-zA-Z_]\w*)|("(?:[^"\\]|\\.)*")|([+\-*/=<>!&|]+)|([()\[\]{}])|(\s+)|(\S))"#,
    ).unwrap();

    let mut tokens = Vec::new();

    for m in re.find_all(input) {
        let kind = match m.matched_branch_number {
            Some(1) => TokenKind::Number,
            Some(2) => TokenKind::Ident,
            Some(3) => TokenKind::String,
            Some(4) => TokenKind::Operator,
            Some(5) => TokenKind::Paren,
            Some(6) => TokenKind::Whitespace,
            _ => TokenKind::Unknown,
        };

        tokens.push(Token {
            kind,
            text: input[m.start..m.end].to_string(),
            offset: m.start,
        });
    }

    tokens
}

let tokens = tokenize(r#"let x = 42 + foo("hello")"#);

// Filter out whitespace for display
let meaningful: Vec<_> = tokens.iter()
    .filter(|t| t.kind != TokenKind::Whitespace)
    .collect();

assert_eq!(meaningful[0].kind, TokenKind::Ident);   // "let"
assert_eq!(meaningful[0].text, "let");
assert_eq!(meaningful[1].kind, TokenKind::Ident);   // "x"
assert_eq!(meaningful[2].kind, TokenKind::Operator); // "="
assert_eq!(meaningful[3].kind, TokenKind::Number);   // "42"
assert_eq!(meaningful[4].kind, TokenKind::Operator); // "+"
assert_eq!(meaningful[5].kind, TokenKind::Ident);   // "foo"
assert_eq!(meaningful[6].kind, TokenKind::Paren);   // "("
assert_eq!(meaningful[7].kind, TokenKind::String);   // "\"hello\""
assert_eq!(meaningful[8].kind, TokenKind::Paren);   // ")"
```

## How branch identification works

When the pattern `(?:A|B|C)` matches, the `MatchResult` carries a `matched_branch_number` field:

- Branch 1 if `A` matched
- Branch 2 if `B` matched
- Branch 3 if `C` matched

Branch numbers are 1-based and correspond to left-to-right order of the top-level alternation arms. This is set automatically by the engine -- no callbacks needed.

## Adding keyword recognition with callbacks

To distinguish keywords from identifiers, add a native callback:

```rust,ignore
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(
    r"(?:(\d+)|([a-zA-Z_]\w*)(?{native:classify_ident})|(\s+)|(\S))",
    ExecutionMode::Full,
)?;

let keywords = ["let", "if", "else", "fn", "return", "while", "for"];

re.register_native("classify_ident", move |ctx| {
    let ident = ctx.group(2).unwrap_or("");
    if keywords.contains(&ident) {
        ExecResult::Replacement("keyword".into())
    } else {
        ExecResult::Replacement("ident".into())
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The `Replacement` value on the `MatchResult` tells the caller whether the matched identifier is a keyword or a plain identifier.

## Position-aware tokenization

Use `find_first_at` for cursor-based tokenization where you control the scan position:

```rust,ignore
# use rgx_core::Regex;
let re = Regex::compile(
    r"(?:(\d+)|([a-zA-Z_]\w*)|(\s+)|(\S))"
)?;

let input = "x = 42";
let mut pos = 0;
let mut tokens = Vec::new();

while pos < input.len() {
    if let Some(m) = re.find_first_at(input, pos) {
        tokens.push((m.matched_branch_number, &input[m.start..m.end]));
        pos = m.end;
    } else {
        break;
    }
}

assert_eq!(tokens.len(), 5);  // "x", " ", "=", " ", "42"
# Ok::<(), Box<dyn std::error::Error>>(())
```

This approach gives you full control over the scan cursor and lets you handle errors (no match at current position) gracefully.

## Performance notes

- The single-pattern approach compiles once and processes all token types in a single pass
- `find_all` uses the engine's internal iteration, which avoids per-match compilation overhead
- Branch identification is zero-cost -- it's computed during normal matching with no extra work
- For very large inputs, `find_iter` provides lazy iteration without allocating a `Vec`

## Complete tokenizer with error reporting

```rust,ignore
# use rgx_core::Regex;
#[derive(Debug)]
struct LexError {
    offset: usize,
    char: char,
}

fn lex(input: &str) -> Result<Vec<(usize, &str, &str)>, LexError> {
    let re = Regex::compile(
        r#"(?:(\d+(?:\.\d+)?)|([a-zA-Z_]\w*)|("(?:[^"\\]|\\.)*")|([+\-*/=<>!]+)|([()\[\]{},:;])|(\s+)|(\S))"#
    ).unwrap();

    let mut tokens = Vec::new();

    for m in re.find_all(input) {
        let kind = match m.matched_branch_number {
            Some(1) => "number",
            Some(2) => "ident",
            Some(3) => "string",
            Some(4) => "operator",
            Some(5) => "punct",
            Some(6) => continue,  // skip whitespace
            Some(7) => {
                let ch = input[m.start..].chars().next().unwrap_or('?');
                return Err(LexError { offset: m.start, char: ch });
            }
            _ => "unknown",
        };
        tokens.push((m.start, kind, &input[m.start..m.end]));
    }

    Ok(tokens)
}

let tokens = lex("fn add(a, b) { a + b }").unwrap();
for (offset, kind, text) in &tokens {
    println!("{offset:3} {kind:8} {text:?}");
}
```

Output:

```text
  0 ident    "fn"
  3 ident    "add"
  6 punct    "("
  7 ident    "a"
  8 punct    ","
 10 ident    "b"
 11 punct    ")"
 13 punct    "{"
 15 ident    "a"
 17 operator "+"
 19 ident    "b"
 21 punct    "}"
```
