# PARSER CONTRACT
Canonical interoperability contract between `rgx` parser backends (the default local PGEN backend and the recursive-descent reference/fallback backend).

## Contract metadata
- Status: active
- Version: `v0.1.11`
- Last updated: `2026-03-30`
- Owners: `rgx-core` parser/compiler maintainers

## Why this exists
- Give RGX and PGEN teams a shared, concrete parser boundary early.
- Prevent integration-time impedance mismatch by locking expected AST/error behavior.
- Keep parser backend swaps testable and low-risk.

## Public contract surface
All parser backends must satisfy the behavior of these parser-boundary surfaces:
- `RegexParser::parse_pattern(&mut self, pattern: &str) -> Result<Regex>`
- `RegexParser::parser_name(&self) -> &'static str`
- `RegexParser::capabilities(&self) -> ParserCapabilities`
- `parse_pattern(pattern: &str) -> Result<Regex>` (compile-time-selected active parser)
- `parser_name() -> &'static str`
- `parser_capabilities() -> ParserCapabilities`

`Result<Regex>` here is `std::result::Result<Regex, RgxError>`.

## AST output contract
Parser output is the canonical `Regex` AST. Backends may differ internally, but output must be contract-equivalent.

Required invariants:
- Equivalent input pattern semantics must produce equivalent AST semantics.
- Parser must not assign capture group numbers. Group nodes are emitted with `index: None`; numbering is compiler/runtime responsibility.
- Group forms map as:
  - `(...)` -> `Regex::Group { kind: Capturing, name: None, index: None }`
  - `(?:...)` -> `Regex::Group { kind: NonCapturing, ... }`
  - `(?<name>...)` -> `Regex::Group { kind: Capturing, name: Some(name), index: None }`
  - `(?>...)` -> `Regex::Group { kind: Atomic, ... }`
  - `(?|...)` -> `Regex::Group { kind: BranchReset, ... }`
- Lookaround forms map as:
  - `(?=...)` / `(?!...)` -> `Regex::Lookahead { positive: true/false, ... }`
  - `(?<=...)` / `(?<!...)` -> `Regex::Lookbehind { positive: true/false, ... }`
- Possessive quantifiers lower to canonical AST using existing shipped nodes rather than a dedicated possessive AST variant:
  - `a*+` -> `Regex::Group { kind: Atomic, expr: Regex::Quantified { expr: 'a', quantifier: ZeroOrMore { lazy: false }}}`
  - `a++` -> `Regex::Group { kind: Atomic, expr: Regex::Quantified { expr: 'a', quantifier: OneOrMore { lazy: false }}}`
  - `a?+` -> `Regex::Group { kind: Atomic, expr: Regex::Quantified { expr: 'a', quantifier: ZeroOrOne { lazy: false }}}`
  - `a{m,n}+` -> `Regex::Group { kind: Atomic, expr: Regex::Quantified { expr: 'a', quantifier: Range { min: m, max: Some(n), lazy: false }}}`
- Parsed advanced constructs with dedicated AST nodes must preserve payload content:
  - code blocks `(?{lang:code})` -> `Regex::CodeBlock { lang, code }`
  - recursion `(?R)`, `(?1)`, `(?&name)` -> `Regex::Recursion { target }`
  - backreferences like `\1` -> `Regex::Backreference(..)`
  - Unicode property classes like `\p{L}` / `\P{Greek}` -> `Regex::UnicodeClass { name, negated }`
  - Perl extended character classes like `(?[a-z])` -> `Regex::ExtendedCharClass { content }`
  - conditional (currently supported parser tests):
    - `(?(1)yes|no)` -> `Regex::Conditional { condition: GroupExists(1), ... }`
    - `(?(+1)yes|no)` -> `Regex::Conditional { condition: RelativeGroupExists(1), ... }`
    - `(?(-1)yes|no)` -> `Regex::Conditional { condition: RelativeGroupExists(-1), ... }`
    - `(?(<name>)yes|no)` -> `Regex::Conditional { condition: NamedGroupExists(name), ... }`
    - `(?(name)yes|no)` -> `Regex::Conditional { condition: NamedGroupExists(name), ... }`
    - `(?(DEFINE)yes|no)` -> `Regex::Conditional { condition: Define, ... }`
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
  - Perl extended character classes `(?[...])`
  - conditionals, including current group/named-group/lookaround forms plus relative group-exists forms such as `(?(+1)...)` and `(?(-1)...)`
  - `DEFINE` conditionals as an explicit parsed-only boundary form
  - branch-reset groups `(?|...)` as an explicit parsed-only boundary form
- Compiler/runtime status for those parser-recognized forms is:
  - recursion, backreferences, Unicode property classes, and current shipped conditional forms, including relative group-exists conditionals, are integrated on the default regex path
  - `DEFINE` conditionals are parser-recognized but compile-rejected explicitly until RGX defines downstream runtime policy
  - branch-reset groups are parser-recognized but compile-rejected explicitly until RGX defines PCRE2-compatible capture-numbering/runtime policy
  - Perl extended character classes are parser-recognized but compile-rejected explicitly until RGX defines downstream set-algebra/runtime policy
  - code blocks remain mode/language/feature gated and fail explicitly when used outside the shipped execution surface

This boundary enables parser progress without unsafe runtime behavior.

## Capability flag contract
`ParserCapabilities` values must reflect actual shipped parser behavior, not roadmap intent.

Important clarifications:
- Capability flags describe parser recognition/build behavior only.
- Capability flags do not imply runtime execution support in the VM/compiler path.

## Conformance harness
The conformance harness checks:
- Active parser output parity with recursive-descent reference fixtures.
- Group metadata invariants expected by downstream compiler/runtime.
- Error mapping invariants (`RgxError::Compile` path).
- Parse-success/compile-fail boundary for still-gated runtime features and validation cases such as mode-restricted code blocks, `DEFINE`, branch-reset groups, Perl extended character classes, and missing capture-target references.

When the default submodule-backed PGEN build is available, the harness also checks the real PGEN backend against the same reference fixtures, including wider parser-surface cases such as anchors, range quantifiers, possessive quantifiers, branch-reset groups, Perl extended character classes, code-block tags, recursion, backreferences, current conditional families (including relative group-exists transport), and Unicode property classes.

Current rollout note:
- The default `rgx-core` build now includes `pgen-parser`, so `parse_pattern(...)` uses the real PGEN AST-dump adapter unless default features are explicitly disabled.
- The local backend choice under that default PGEN-backed build is intentionally controlled by one constant (`PGEN_FEATURE_BACKEND`) so RGX can flip between the PGEN backend and the recursive-descent reference backend without rewriting call sites.
## PGEN issue reporting and upstream handoff
When RGX exercises a real PGEN-backed parser path, suspected parser misbehavior should be reported using the structured bundle described in `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`.

For parser-side release state, root cause, and fix proof, the canonical upstream ledger is `PGEN_RELEASED_PARSER_BUG_LEDGER.md`.

## Backend change policy
Any parser backend change (including PGEN rollout) must do one of:
- Preserve this contract exactly, or
- Introduce a contract version bump and update:
  - this document,
  - conformance tests,
  - the changelog,
  - and relevant roadmap/notes references.

## Suggested validation commands
- `cargo test -p rgx-core`
- `cargo test -p rgx-core --features pgen-parser`
