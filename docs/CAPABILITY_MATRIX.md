# CAPABILITY MATRIX
Live shipped-vs-scaffolded feature status for `rgx`.

## Purpose
- Provide a single, concrete view of what is currently user-usable.
- Distinguish parser recognition from VM/runtime execution support.
- Reduce ambiguity during roadmap prioritization and PGEN integration.

## Status labels
- `shipped`: parser + compiler + VM/user API path works and is covered by tests.
- `parsed-only`: parser accepts/builds AST, but compile/runtime path is intentionally blocked with explicit unsupported errors.
- `scaffolded`: declarations/placeholders exist, but feature is not yet user-ready.

## Core regex features (user API path)
- Literals, concatenation, alternation: `shipped`
- Anchors (`^`, `$`, `\A`, `\Z`, `\z`) and word boundaries: `shipped`
- Character classes (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`, custom classes): `shipped`
- Unicode property classes (`\p{...}`, `\P{...}`): `parsed-only`
- Quantifiers (`?`, `*`, `+`, counted ranges): `shipped`
- Groups:
  - capturing/non-capturing/named groups: `shipped`
  - atomic groups `(?>...)` with no-backtracking semantics: `shipped`
- Lookarounds:
  - lookahead/lookbehind (positive and negative): `shipped`

Representative test anchors:
- `rgx-core/src/lib.rs` parser-path lookaround/atomic tests
- `rgx-core/src/vm.rs` VM execution tests

## Advanced syntax currently parsed but not runtime-integrated
- Backreferences (`\1`, ...): `parsed-only`
- Recursion (`(?R)`, `(?1)`, `(?&name)`): `parsed-only`
- Conditionals (`(?(...)yes|no)` currently supported parser condition forms): `parsed-only`
- Code blocks (`(?{lang:code})`): `parsed-only`

Behavior contract:
- These forms are accepted by parser/conformance tests where applicable.
- Compilation intentionally fails with explicit error messages until VM/runtime integration lands.

Representative test anchors:
- `rgx-core/src/lib.rs` explicit unsupported compile-boundary tests (including recursion variants and conditional condition variants)
- `rgx-core/src/parsing.rs` conformance + compile-boundary guardrail tests (active and `pgen-parser` fixture parity)

## Conditional parser condition forms (current parser coverage)
- Group-exists: `(?(1)yes|no)` (`shipped` at parser level)
- Named-group-exists: `(?(<name>)yes|no)`, `(?(name)yes|no)` (`shipped` at parser level)
- Lookaround conditions:
  - `(?(?=expr)yes|no)`
  - `(?(?!expr)yes|no)`
  - `(?(?<=expr)yes|no)`
  - `(?(?<!expr)yes|no)`
  (`shipped` at parser level)

Runtime status for all conditional forms: `parsed-only` (compile-boundary explicit unsupported error).

## Notes for roadmap usage
- This matrix is implementation-facing and must reflect verified behavior only.
- Aspirational goals (PCRE2 parity, multi-language code-block execution breadth) belong in `ROADMAP.md` and `PROJECT_VISION.md`.
- When a feature changes state, update:
  - this file,
  - relevant tests,
  - `CHANGES.md`,
  - parser contract if parser boundary semantics changed.
