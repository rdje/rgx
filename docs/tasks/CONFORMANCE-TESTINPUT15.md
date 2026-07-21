# CONFORMANCE-TESTINPUT15: re-test the excluded match-limiting corpus file

## Metadata

- Tree ID: `CONFORMANCE-TESTINPUT15`
- Status: `active`
- Roadmap lane: `PCRE2 parity program (accuracy) — conformance corpus coverage`
- Created: `2026-07-21`
- Last updated: `2026-07-21`
- Owner: repo-local workflow

## Goal

Decide, with evidence, whether PCRE2's `testinput15` can be returned to the
differential conformance corpus now that its stated blocker is gone.

`testinput15` is the only corpus file excluded from
`rgx-core/tests/pcre2_conformance.rs` (`TESTINPUT_FILES`). The exclusion
comment gives one reason, verbatim: *"Several of them hang RGX even with a 1M
step cap because the hot compile/exec path doesn't honor the cap for every
case."* That is exactly the defect `VM-LIMITS-SUBEXEC.1` fixed on 2026-07-21 —
sub-executions ran off-budget, so the cap was only ever a top-level-loop cap.

Since the harness is the project's ground-truth accuracy oracle, an exclusion
whose justification has expired is a coverage hole that should be re-tested,
not inherited.

## Non-Goals

- Not a commitment to include the file. If cases still hang, or if the file's
  `(*LIMIT_MATCH=…)` / `(*LIMIT_DEPTH=…)` / `(*LIMIT_HEAP=…)` directives are
  architecturally untestable through this harness, the honest outcome is a
  documented exclusion with a *current* reason.
- Not a change to the limit semantics themselves (owned by `VM-LIMITS-SUBEXEC`,
  closed).
- Not an attempt to make the pattern-level `(*LIMIT_*)` verbs functional in RGX
  — that is a feature question, separable from corpus inclusion.

## Acceptance Criteria

- `testinput15` is run through the harness out-of-tree and the outcome is
  measured: cases parsed, pass / fail / panic / skip, and total wall-time.
- A decision is recorded with evidence: include (ratchet baselines bumped in the
  same commit, per the ratchet idiom) or keep excluded (comment rewritten to
  state the *real, current* reason).
- If included: full gate + conformance ratchet green at the new baselines, and
  the Book's PCRE2 conformance chapters reflect the corpus change.
- `docs/BACKLOG.md` "Excluded files" note is updated to match the outcome.

## Task Tree

- ID: `CONFORMANCE-TESTINPUT15`
  Status: `active`
  Goal: `Re-test and adjudicate the testinput15 exclusion.`
  Children: `.1` `.2`

- ID: `CONFORMANCE-TESTINPUT15.1`
  Status: `pending`
  Goal: `Measure: run the harness over testinput15 with the current limits and record cases/pass/fail/panic/skip + wall-time, without changing TESTINPUT_FILES.`
  Acceptance: `A measurement recorded in this tree's Verification Log, including whether any case still fails to terminate under the harness caps.`
  Verification: `pending`
  Commit: `pending`

- ID: `CONFORMANCE-TESTINPUT15.2`
  Status: `pending`
  Goal: `Adjudicate: either include the file (bump PASS/FAIL/PANIC/SKIP baselines in the same commit) or rewrite the exclusion comment with the current reason.`
  Acceptance: `Decision recorded here + in the harness comment; gate + ratchet green; BACKLOG and Book synced.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `CONFORMANCE-TESTINPUT15.1` | `pending` | Measurement is cheap, is a prerequisite for the decision, and cannot move the ratchet. |
| 2 | `CONFORMANCE-TESTINPUT15.2` | `pending` | Needs `.1`'s numbers to be an evidence-based decision rather than a guess. |

## Decisions

- `2026-07-21`: Split measurement (`.1`) from adjudication (`.2`) so the
  ratchet-moving change is reviewed on its own, with numbers already in hand.

## Open Questions

- The file is "entirely dedicated to the `(*LIMIT_MATCH=…)` / `(*LIMIT_DEPTH=…)`
  / `(*LIMIT_HEAP=…)` directives" per the harness comment. RGX does not
  implement those pattern-level verbs, so many cases may be untestable for a
  reason unrelated to hangs. `.1` must separate "still hangs" from "semantically
  untestable" — they lead to different outcomes.
- Roughly 41 cases are at stake (BACKLOG estimate, unverified). `.1` confirms.

## Blockers

- None. (`VM-LIMITS-SUBEXEC.1`, the precondition, closed 2026-07-21.)

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-07-21` | — | Exclusion rationale traced to `rgx-core/tests/pcre2_conformance.rs` `TESTINPUT_FILES` comment; its named cause fixed by `VM-LIMITS-SUBEXEC.1` the same day | `tree opened` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| — | `pending` | — |

## Changelog

- `2026-07-21`: Created — surfaced while closing `VM-LIMITS-SUBEXEC.1`, whose
  fix removes the stated cause of the exclusion.
