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
- Tracked parity estimate: about `97%`
  - Rationale: the major regex feature families tracked explicitly in this matrix are now almost entirely in the `parity-verified` bucket, including recursion, relative conditional-group references, and branch-reset numbering semantics, while some uncovered differential edge space and newer advanced families still remain.
- Broader PCRE2 regex estimate: about `77%`
  - Rationale: rgx now covers most day-to-day PCRE2-style regex usage on the default path, including recursion, possessive quantifiers, branch-reset groups, conditionals, and Unicode properties, but PCRE2 still has a meaningful long tail of advanced families that are either only planned, not yet parity-targeted, or intentionally outside the current shipped target surface.
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
- Possessive quantifiers (`*+`, `++`, `?+`, `{n,m}+`): `parity-verified`
  - differential coverage includes both straightforward success cases and suffix-sensitive no-backtracking behavior (e.g., `\Aa*+a\z`, `\Aa++a\z`, `\A\d{2,3}+3\z`)
- Range quantifier (`{n,m}`) scanning/earliest-match behavior: `parity-verified`
  - differential coverage includes bounded-range suffix backtracking scenarios (e.g., `{2,3}3`, `{2,3}?3`), exact-range `{n}` find-all behavior, and unbounded-range `{n,}` / `{n,}?` scan and suffix-sensitive cases
- Anchors (`^`, `$`, `\A`, `\Z`, `\z`) in supported parser-path forms: `parity-verified`
- Character-class shorthand (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`) and word boundaries: `parity-verified`
- Unicode property classes (`\p{...}`, `\P{...}`) in current covered forms: `parity-verified`
- Recursion / subroutine calls (`(?R)`, `(?1)`, `(?&name)`): `parity-verified`
  - differential coverage includes whole-pattern recursion plus numbered-group and named-group subroutine recursion forms
- Numeric backreferences (`\1`, `\2`, ...): `parity-verified`
  - differential coverage includes successful backreference matching, explicit no-match behavior, and alternation/lookahead cases that depend on capture restoration under backtracking
- Branch-reset groups (`(?|...)`): `parity-verified`
  - differential coverage includes shared capture-slot backreferences plus max-branch-arity conditional numbering after the branch-reset group
- Conditionals (`(?(...)yes|no)` current supported parser forms): `parity-verified`
  - differential coverage includes group-exists, named-group-exists, single-branch `DEFINE` definition blocks, and lookaround conditions for both first-match and all-match span parity
- Lookarounds:
  - positive/negative lookahead: `parity-verified`
  - positive/negative lookbehind: `parity-verified`
- Atomic-group no-backtracking semantics: `parity-verified`
- Explicit no-match parity checks (first-match = `None`, all-match = empty): `parity-verified`

## Known rgx gaps relative to PCRE2
- Remaining PCRE2 follow-up work is concentrated in newer or broader advanced forms listed below rather than in the currently parity-verified baseline families.

## Supported / Gap / Planned checklist
### Supported today on the default regex path
- Literals, concatenation, and alternation
- Capturing, non-capturing, and named groups
- Atomic groups
- Anchors and word boundaries
- Shorthand character classes and custom classes
- Unicode property classes in the currently covered forms
- Greedy/lazy/possessive quantifiers and counted ranges
- Current recursion / subroutine recursion forms used by rgx (`(?R)`, `(?1)`, `(?&name)`)
- Numeric backreferences
- Branch-reset groups
- Current shipped conditional forms
  - group-exists
  - relative-group-exists
  - named-group-exists
  - current recursion conditions `(?(R)...)` and `(?(Rn)...)`
  - single-branch `DEFINE` definition blocks
  - lookaround conditions
- Positive and negative lookahead/lookbehind

### Explicitly unsupported or still open
- Returned-capture subroutine forms such as `(?R(grouplist))`, `(?n(grouplist))`, `(?+n(grouplist))`, `(?-n(grouplist))`, `(?&name(grouplist))`, and `(?P>name(grouplist))`
- Newer conditional forms such as `(?(R&name)...)` and `(?(VERSION[...])...)`
  - current pinned PGEN parser blocker: `(?(R&word)a|b)` is rejected at byte 0 on the default generated backend; tracked in `pgen-issues/PGEN-RGX-0005.yaml`
- Perl extended character classes `(?[...])`
  - current RGX boundary is parser-recognized but compile-rejected explicitly; downstream set-algebra/runtime behavior is still open

### Planned next or broader PCRE2 follow-up
- Drive the broader advanced families above through parser, compiler, runtime-policy, and parity decisions without regressing the now-shipped baseline recursion and conditional forms.
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
