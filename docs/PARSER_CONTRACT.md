# PARSER CONTRACT
Canonical interoperability contract between `rgx` parser backends (current recursive-descent and future PGEN integration).

## Contract metadata
- Status: active
- Version: `v0.1.4`
- Last updated: `2026-03-28`
- Owners: `rgx-core` parser/compiler maintainers

## Why this exists
- Give RGX and PGEN teams a shared, concrete parser boundary early.
- Prevent integration-time impedance mismatch by locking expected AST/error behavior.
- Keep parser backend swaps testable and low-risk.

## Public contract surface
All parser backends must satisfy the behavior of these `rgx-core/src/parsing.rs` surfaces:
- `RegexParser::parse_pattern(&mut self, pattern: &str) -> Result<Regex>`
- `RegexParser::parser_name(&self) -> &'static str`
- `RegexParser::capabilities(&self) -> ParserCapabilities`
- `parse_pattern(pattern: &str) -> Result<Regex>` (compile-time-selected active parser)
- `parser_name() -> &'static str`
- `parser_capabilities() -> ParserCapabilities`

`Result<Regex>` here is `std::result::Result<Regex, RgxError>`.

## AST output contract
Parser output is the canonical `Regex` AST in `rgx-core/src/ast.rs`. Backends may differ internally, but output must be contract-equivalent.

Required invariants:
- Equivalent input pattern semantics must produce equivalent AST semantics.
- Parser must not assign capture group numbers. Group nodes are emitted with `index: None`; numbering is compiler/runtime responsibility.
- Group forms map as:
  - `(...)` -> `Regex::Group { kind: Capturing, name: None, index: None }`
  - `(?:...)` -> `Regex::Group { kind: NonCapturing, ... }`
  - `(?<name>...)` -> `Regex::Group { kind: Capturing, name: Some(name), index: None }`
  - `(?>...)` -> `Regex::Group { kind: Atomic, ... }`
- Lookaround forms map as:
  - `(?=...)` / `(?!...)` -> `Regex::Lookahead { positive: true/false, ... }`
  - `(?<=...)` / `(?<!...)` -> `Regex::Lookbehind { positive: true/false, ... }`
- Parsed advanced constructs with dedicated AST nodes must preserve payload content:
  - code blocks `(?{lang:code})` -> `Regex::CodeBlock { lang, code }`
  - recursion `(?R)`, `(?1)`, `(?&name)` -> `Regex::Recursion { target }`
  - backreferences like `\1` -> `Regex::Backreference(..)`
  - conditional (currently supported parser tests):
    - `(?(1)yes|no)` -> `Regex::Conditional { condition: GroupExists(1), ... }`
    - `(?(<name>)yes|no)` -> `Regex::Conditional { condition: NamedGroupExists(name), ... }`
    - `(?(name)yes|no)` -> `Regex::Conditional { condition: NamedGroupExists(name), ... }`
    - `(?(?=expr)yes|no)` -> `Regex::Conditional { condition: Lookahead { expr, positive: true }, ... }`
    - `(?(?!expr)yes|no)` -> `Regex::Conditional { condition: Lookahead { expr, positive: false }, ... }`
    - `(?(?<=expr)yes|no)` -> `Regex::Conditional { condition: Lookbehind { expr, positive: true }, ... }`
    - `(?(?<!expr)yes|no)` -> `Regex::Conditional { condition: Lookbehind { expr, positive: false }, ... }`

## Error contract
- Parse failures must return `Err(RgxError::Compile(message))`.
- Messages should be human-debuggable and include positional context when available.
- Parser must not silently degrade invalid syntax into unrelated AST nodes.

## Parse-vs-compile boundary contract
Some constructs are intentionally parser-recognized before VM runtime integration.

Current contract:
- Parser accepts and builds AST for:
  - code blocks
  - recursion
  - backreferences
  - conditionals (group/named-group/positive+negative-lookaround forms in parser tests)
- Compiler must then fail explicitly (not silently) for unintegrated runtime features.

This boundary enables parser progress without unsafe runtime behavior.

## Capability flag contract
`ParserCapabilities` values must reflect actual shipped parser behavior, not roadmap intent.

Important clarifications:
- Capability flags describe parser recognition/build behavior only.
- Capability flags do not imply runtime execution support in the VM/compiler path.

## Conformance harness
The initial conformance harness lives in `rgx-core/src/parsing.rs` tests and checks:
- Active parser output parity with recursive-descent reference fixtures.
- Group metadata invariants expected by downstream compiler/runtime.
- Error mapping invariants (`RgxError::Compile` path).
- Parse-success/compile-fail boundary for unintegrated runtime features.

When `pgen-parser` is enabled, the harness also checks the PGEN backend type against the same reference fixtures.

## PGEN issue recording and upstream handoff
When RGX exercises a real PGEN-backed parser path, any suspected PGEN parser bug or misbehavior must be recorded locally before or alongside upstream reporting.

Local recording contract:
- One local issue file per suspected bug under `pgen-issues/`.
- Local file name and local issue ID must match the form `PGEN-RGX-0001.yaml`.
- IDs are never reused.
- Each record must capture:
  - summary and status
  - `opened_at`, `first_seen_at`, and `last_updated_at`
  - parser backend/version information
  - current `rgx` commit
  - precise RGX-side manifestation context
  - minimal reproduction
  - expected vs actual behavior
  - impact on RGX integration or downstream behavior
  - upstream issue reference once reported
  - closing verification evidence when resolved
- `scripts/new-pgen-issue.sh` is the canonical stub generator.
- `pgen-issues/TEMPLATE.yaml` is the canonical schema/template.

## Backend change policy
Any parser backend change (including PGEN rollout) must do one of:
- Preserve this contract exactly, or
- Introduce a contract version bump and update:
  - this document,
  - conformance tests,
  - `CHANGES.md`,
  - and relevant roadmap/notes references.

## Suggested validation commands
- `cargo test -p rgx-core`
- `cargo test -p rgx-core --features pgen-parser`
