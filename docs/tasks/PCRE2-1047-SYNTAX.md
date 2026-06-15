# PCRE2-1047-SYNTAX: PCRE2 10.47+ downstream syntax alignment

## Metadata

- Tree ID: `PCRE2-1047-SYNTAX`
- Status: `active`
- Roadmap lane: `Next — PCRE2 10.47+ downstream syntax alignment`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Define RGX AST/runtime handling for newer PCRE2 advanced syntax arriving through
the default PGEN parser path, and close the residual advanced-form gaps:
returned-capture subroutine capture-return VM semantics, bracketed `VERSION[...]`
conditionals, and wider Perl extended-character-class `(?[...])` set-expression
forms — each with capability/compatibility-matrix and differential coverage.

## Non-Goals

- No PGEN grammar work in RGX (PGEN owns grammar; file reports per protocol if a
  form is mis-parsed — verify with `parseability_probe` first).
- Not re-opening already-shipped forms (relative conditionals, recursion
  conditions, `DEFINE`, branch-reset, the shipped `(?[...])` subset).

## Acceptance Criteria

- Each leaf either ships runtime support with API + parser-contract + PCRE2
  differential coverage, or sets an explicit compile-boundary policy with a
  clear error and a matrix entry.
- `docs/CAPABILITY_MATRIX.md` + `docs/PCRE2_COMPATIBILITY_MATRIX.md` updated.
- PCRE2 conformance ratchet held or improved; full gate green; Book updated.

## Task Tree

- ID: `PCRE2-1047-SYNTAX`
  Status: `active`
  Goal: `Align RGX with PCRE2 10.47+ advanced syntax via PGEN.`
  Children: `.1` `.2` `.3` `.4`

- ID: `PCRE2-1047-SYNTAX.1`
  Status: `pending`
  Goal: `A12 returned-capture subroutine FULL capture-return VM semantics: preserve the specified groups across the recursive call. (Parse + Call/CallReturning lowering already shipped 2026-05-07; this is the capture-return runtime follow-up.)`
  Acceptance: `(?N(grouplist)) returns the specified captures across the call; differential cases at the testinput2:8067–8168 family verified; ratchet held/improved; full gate green.`
  Verification: `pending`
  Commit: `pending`

- ID: `PCRE2-1047-SYNTAX.2`
  Status: `pending`
  Goal: `Bracketed VERSION conditional (?(VERSION[...])...): decide compile-boundary or runtime behavior (the operator form (?(VERSION op X.Y)...) is shipped — A13).`
  Acceptance: `Explicit policy implemented (runtime or clean compile-boundary error); matrix + differential coverage; ratchet held; gate green.`
  Verification: `pending`
  Commit: `pending`

- ID: `PCRE2-1047-SYNTAX.3`
  Status: `pending`
  Goal: `Widen (?[...]) extended character classes beyond the shipped grouped bracket/property/nested-ordinary/POSIX/shorthand/escaped-term subset, per an explicit runtime policy for wider set-expression forms and remaining bare-term families.`
  Acceptance: `Next set-expression family ships on the default path OR stays a clean compile-boundary with a matrix entry and PCRE2 parity probe; ratchet held; gate green.`
  Verification: `pending`
  Commit: `pending`

- ID: `PCRE2-1047-SYNTAX.4`
  Status: `pending`
  Goal: `Returned-capture subroutine grouplists for the remaining call forms: (?R(grouplist)), (?+n(grouplist)), (?-n(grouplist)), (?&name(grouplist)), (?P>name(grouplist)) — AST/interoperability handling + matrix + differential tests.`
  Acceptance: `Each form parses + lowers consistently with .1's capture-return semantics (or a documented boundary); matrix + differential coverage; ratchet held; gate green.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `PCRE2-1047-SYNTAX.1` | `pending` | Capture-return VM semantics is the most-load-bearing residual (A12 follow-up; unblocks `.4`'s grouplist forms). |
| 2 | `PCRE2-1047-SYNTAX.4` | `pending` | Builds on `.1`'s capture-return semantics. |
| 3 | `PCRE2-1047-SYNTAX.2` | `pending` | Self-contained conditional-form policy. |
| 4 | `PCRE2-1047-SYNTAX.3` | `pending` | Incremental extended-class widening. |

## Decisions

- `2026-06-15`: Order `.1` before `.4` because the grouplist call forms depend
  on the capture-return VM semantics landing first.

## Open Questions

- `.3`: which extended-class family is the right next increment (the shipped
  subset is already broad). Resolve at pick time against a PCRE2 parity probe.

## Blockers

- None. `.1` is eligible. Any leaf touching the PGEN adapter must
  `parseability_probe` first and file a PGEN report (not an RGX workaround) if
  PGEN mis-parses (`feedback_no_pgen_workarounds`).

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | tree created; no leaf executed | `n/a` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| — | `pending` | `pending` |

## Changelog

- `2026-06-15`: Created from ROADMAP "PCRE2 10.47+ downstream syntax alignment"
  (leaf `TASKTREE-ADOPT.2`). Frontier ordered `.1`→`.4`→`.2`→`.3`.
