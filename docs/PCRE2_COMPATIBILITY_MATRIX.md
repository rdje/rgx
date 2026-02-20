# PCRE2 COMPATIBILITY MATRIX
Live compatibility tracker for `rgx` against PCRE2.

## Purpose
- Keep parity claims concrete and verifiable.
- Separate verified behavior from aspirational parity targets.
- Track known divergences explicitly until they are closed.

## Status labels
- `parity-verified`: behavior is checked via executable differential tests.
- `rgx-gap`: PCRE2 supports behavior that rgx does not yet execute.
- `out-of-scope`: behavior is not a parity target for PCRE2 comparison.

## Parity-verified baseline
Backed by `rgx-bench/tests/pcre2_parity.rs`.

- Literals and concatenation: `parity-verified`
- Alternation: `parity-verified`
- Basic quantifiers/ranges: `parity-verified`
- Character-class shorthand (`\d`) and word boundaries: `parity-verified`
- Lookarounds:
  - positive/negative lookahead: `parity-verified`
  - positive/negative lookbehind: `parity-verified`
- Atomic-group no-backtracking semantics: `parity-verified`

## Known rgx gaps relative to PCRE2
- Backreferences (`\1`, etc.): `rgx-gap`
  - rgx currently parses and returns explicit compile-time unsupported errors.
  - PCRE2 executes these forms.
- Recursion (`(?R)`, `(?1)`, `(?&name)`): `rgx-gap`
  - rgx currently parses and returns explicit compile-time unsupported errors.
- Conditionals (`(?(...)yes|no)`): `rgx-gap`
  - rgx currently parses and returns explicit compile-time unsupported errors.

## Out of scope for PCRE2 parity
- rgx inline code blocks (`(?{lang:code})`): `out-of-scope`
  - This is rgx extension behavior rather than a direct PCRE2 parity target.

## Maintenance workflow
- Keep this matrix synchronized with:
  - `rgx-bench/tests/pcre2_parity.rs`
  - `docs/CAPABILITY_MATRIX.md`
  - `CHANGES.md`
- When moving an item from `rgx-gap` to `parity-verified`, require:
  - differential test coverage proving match behavior parity
  - API-level guardrails for rgx user-facing behavior
  - a changelog entry with validation commands/results
