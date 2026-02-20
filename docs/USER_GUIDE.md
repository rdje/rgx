# USER GUIDE
Live end-user guide for rgx.

This guide is intentionally layered so you can start simple and go as deep as needed.

## Living-document policy
- Last updated: 2026-02-20
- This is a live document and should be updated as user-visible behavior changes.
- Keep examples and feature-status notes aligned with current shipped behavior.
- Update cadence: revise this guide whenever behavior changes land, and at minimum once per roadmap milestone.
- For recent changes and validation details, cross-check `CHANGES.md`.

## Guide levels
- Level 0: quick start
- Level 1: practical day-to-day usage
- Level 2: advanced patterns and AST-first workflows
- Level 3: deep behavior notes ("gory details")

## Level 0 - Quick start
Build and run a simple match:

```bash
cargo build
cargo run --bin rgx-cli -- "cat|dog" "I have a cat"
```

Run tests:

```bash
cargo test --workspace
```

## Level 1 - Practical usage
### CLI
Basic form:

```bash
cargo run --bin rgx-cli -- "<pattern>" "<text>"
```

Examples:

```bash
cargo run --bin rgx-cli -- "\\d+" "abc123def"
cargo run --bin rgx-cli -- "(?:cat|dog)" "pet dog"
```

### Rust API
Use the high-level API for normal matching:

```rust
use rgx_core::Regex;

let re = Regex::compile(r"\d+")?;
assert!(re.is_match("abc123"));

if let Some(m) = re.find_first("abc123def") {
    assert_eq!((m.start, m.end), (3, 6));
}
# Ok::<(), rgx_core::RgxError>(())
```

### Match results
- Positions are byte offsets.
- `find_all` returns non-overlapping matches.
- For top-level alternation patterns, `matched_branch_number` is:
  - 1-based
  - `None` if no top-level alternation branch selection applies

## Level 2 - Advanced usage (current best path)
When parser syntax is not yet complete for advanced constructs, use AST-first APIs.

### Compile from AST directly
```rust
use rgx_core::{Regex, RegexAst};

let ast = RegexAst::Alternation(vec![
    RegexAst::Sequence(vec![RegexAst::Char('c'), RegexAst::Char('a'), RegexAst::Char('t')]),
    RegexAst::Sequence(vec![RegexAst::Char('d'), RegexAst::Char('o'), RegexAst::Char('g')]),
]);

let re = Regex::from_ast(ast)?;
assert!(re.is_match("dog"));
# Ok::<(), rgx_core::RgxError>(())
```

### Lookaround status
- Parser syntax supports positive/negative lookahead and lookbehind:
  - `(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`
- AST-first lookahead/lookbehind support remains available via `Regex::from_ast`.
- Parser also recognizes code-block syntax `(?{lang:code})`, but execution is not yet wired into VM runtime.
- Parser also recognizes recursion syntax (`(?R)`, `(?1)`, `(?&name)`), but execution is not yet wired into VM runtime.

## Level 3 - Gory details (behavior semantics)
### Execution model
Pipeline:
- pattern text -> lexer -> parser -> AST -> compiler -> VM bytecode -> VM execution

In AST-first mode, parser steps are bypassed and AST goes directly to compiler/VM.

### Assertion semantics
- Lookahead and lookbehind are assertions: they do not consume input themselves.
- Positive assertion requires assertion sub-expression to match.
- Negative assertion requires assertion sub-expression to not match.
- Lookbehind specifically requires a sub-expression match that ends exactly at the current position.

### Atomic-group semantics
- Atomic groups `(?>...)` are supported.
- Once an atomic group succeeds, rgx does not backtrack into alternatives/paths created inside that group.
- Example consequence:
  - `(?>a|ab)c` does not match `abc`
  - `(a|ab)c` can match `abc`

### Branch reporting semantics
- Branch reporting is intentionally scoped to top-level alternations.
- User-facing field is `matched_branch_number` and is 1-based.
- Nested alternations do not override top-level branch selection in the reported value.

### Current constraints to keep in mind
- Some advanced parser syntaxes are still incomplete (for example conditionals).
- Backreference, recursion, and code-block syntax are parsed, but currently return explicit compile-time unsupported errors.
- Declared opcodes/features should be treated as shipped only when parser/compiler/VM/API paths are all validated.

## Troubleshooting checklist
- If a pattern compiles but behavior is surprising, verify whether you are using parser path or AST-first path.
- Validate with API-level tests (`Regex::compile` / `Regex::from_ast`) before assuming feature parity.
- Check `CHANGES.md` for recently shipped behavior and `ROADMAP.md` for what is planned next.

## Related docs
- `README.md`: project overview
- `ROADMAP.md`: forward-looking tracker
- `CHANGES.md`: shipped changes and validation history
- `DEVELOPMENT_NOTES.md`: engineering context
- `docs/architecture.md`: architectural responsibilities
- `docs/TECHNICAL_DECISIONS.md`: key design tradeoffs
