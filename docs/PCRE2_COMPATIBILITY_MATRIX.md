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

## Rough progress estimate
- Tracked parity estimate: about `90%`
  - Rationale: among the major regex feature families that this matrix currently tracks explicitly, only recursion remains in the `rgx-gap` bucket.
- Broader PCRE2 regex estimate: about `70%`
  - Rationale: rgx now covers most day-to-day PCRE2-style regex usage on the default path, but PCRE2 still has a meaningful long tail of advanced families that are either only planned, not yet parity-targeted, or still intentionally unsupported.
- These percentages are intentionally rough and hand-maintained.
  - They are not derived from a formal PCRE2 feature census.
  - Update them only when whole feature families move, not for tiny edge-case wins.

## Parity-verified baseline
Backed by `rgx-bench/tests/pcre2_parity.rs`.
- Differential assertions currently verify both:
  - first-match span parity (`find_first` equivalent)
  - all-match non-overlapping span parity (`find_all` vs `find_iter`)

- Literals and concatenation: `parity-verified`
- Alternation: `parity-verified`
- Basic quantifiers (`*`, `+`, `?`, `*?`, `+?`, `??`): `parity-verified`
  - differential coverage includes suffix-sensitive backtracking scenarios (e.g., `a*a`, `a+a`, `ab?b`) and lazy shortest-match cases
- Range quantifier (`{n,m}`) scanning/earliest-match behavior: `parity-verified`
  - differential coverage includes bounded-range suffix backtracking scenarios (e.g., `{2,3}3`, `{2,3}?3`), exact-range `{n}` find-all behavior, and unbounded-range `{n,}` / `{n,}?` scan and suffix-sensitive cases
- Anchors (`^`, `$`, `\A`, `\Z`, `\z`) in supported parser-path forms: `parity-verified`
- Character-class shorthand (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`) and word boundaries: `parity-verified`
- Unicode property classes (`\p{...}`, `\P{...}`) in current covered forms: `parity-verified`
- Numeric backreferences (`\1`, `\2`, ...): `parity-verified`
  - differential coverage includes successful backreference matching, explicit no-match behavior, and alternation/lookahead cases that depend on capture restoration under backtracking
- Conditionals (`(?(...)yes|no)` current supported parser forms): `parity-verified`
  - differential coverage includes group-exists, named-group-exists, and lookaround conditions for both first-match and all-match span parity
- Lookarounds:
  - positive/negative lookahead: `parity-verified`
  - positive/negative lookbehind: `parity-verified`
- Atomic-group no-backtracking semantics: `parity-verified`
- Explicit no-match parity checks (first-match = `None`, all-match = empty): `parity-verified`

## Known rgx gaps relative to PCRE2
- Recursion (`(?R)`, `(?1)`, `(?&name)`): `rgx-gap`
  - rgx currently parses and returns explicit compile-time unsupported errors.

## Supported / Gap / Planned checklist
### Supported today on the default regex path
- Literals, concatenation, and alternation
- Capturing, non-capturing, and named groups
- Atomic groups
- Anchors and word boundaries
- Shorthand character classes and custom classes
- Unicode property classes in the currently covered forms
- Greedy/lazy quantifiers and counted ranges
- Numeric backreferences
- Current shipped conditional forms
  - group-exists
  - named-group-exists
  - lookaround conditions
- Positive and negative lookahead/lookbehind

### Explicitly unsupported or still open
- Recursion / subroutine recursion forms currently used by rgx (`(?R)`, `(?1)`, `(?&name)`)
- Possessive quantifiers in the current rgx parser adapter
  - PGEN may transport them, but rgx does not yet represent them in its parser AST / runtime path

### Planned next or broader PCRE2 follow-up
- Returned-capture subroutine forms such as `(?R(grouplist))`, `(?n(grouplist))`, `(?+n(grouplist))`, `(?-n(grouplist))`, `(?&name(grouplist))`, and `(?P>name(grouplist))`
- Newer conditional forms such as `(?(R&name)...)` and `(?(VERSION[...])...)`
- Branch-reset groups
- `DEFINE` conditionals
- Perl extended character classes `(?[...])`
These broader families are tracked in `ROADMAP.md` as follow-up work rather than as parity-verified support today.

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
