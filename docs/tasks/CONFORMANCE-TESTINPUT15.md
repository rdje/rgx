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
  Status: `done`
  Goal: `Measure: run the harness over testinput15 with the current limits and record cases/pass/fail/panic/skip + wall-time, without changing TESTINPUT_FILES.`
  Acceptance: `A measurement recorded in this tree's Verification Log, including whether any case still fails to terminate under the harness caps.`
  Verification: `2026-07-21 — see Verification Log. The file NO LONGER HANGS: 80 cases, 68 pass / 12 fail / 0 panic / 0 skip, +65.6s sweep cost. All 12 failures triaged.`
  Commit: `CONFORMANCE-TESTINPUT15.1 — measure the excluded testinput15 corpus file`

- ID: `CONFORMANCE-TESTINPUT15.2`
  Status: `pending`
  Goal: `Adjudicate: include the file (bumping PASS/FAIL baselines in the same commit), keep it excluded with a rewritten reason, or first close the allusedtext harness gap that accounts for 10 of its 12 failures.`
  Acceptance: `Decision recorded here + in the harness comment; gate + ratchet green; BACKLOG and Book synced.`
  Verification: `pending`
  Commit: `pending`

- ID: `CONFORMANCE-TESTINPUT15.3`
  Status: `pending`
  Goal: `Teach the harness pcre2test's allusedtext output shape (the printed span includes lookaround-consulted text, so the parsed "expected match" is wider than the real match).`
  Acceptance: `The 10 allusedtext-affected testinput15 cases pair correctly (or are honestly classified as harness-untestable); no change to any currently-counted case; ratchet green.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `CONFORMANCE-TESTINPUT15.3` | `pending` | `.1` showed 10 of the 12 failures are one harness gap, not RGX defects. Closing it first means `.2` adjudicates a file with ~2 real residuals instead of 12 mixed ones — and it is a self-contained harness change that cannot regress a counted case. |
| 2 | `CONFORMANCE-TESTINPUT15.2` | `pending` | The inclusion decision; cleanest once `.3` has removed the noise. Can still be taken without `.3` if the +65.6s sweep cost is judged unacceptable. |
| — | `CONFORMANCE-TESTINPUT15.1` | `done` | Measured 2026-07-21. |

## Decisions

- `2026-07-21`: Split measurement (`.1`) from adjudication (`.2`) so the
  ratchet-moving change is reviewed on its own, with numbers already in hand.
- `2026-07-21` (`.1`): Measured with a **temporary, reverted** local edit rather
  than shipping an env-var escape hatch into the harness. The measurement leaf
  therefore commits no code and needs no gate receipt; the numbers live here.
- `2026-07-21` (`.1`): Added `.3` (the `allusedtext` harness gap) and put it
  **ahead of** `.2` in the frontier. 10 of the 12 failures are that one gap;
  adjudicating inclusion while they are miscounted as RGX failures would be
  deciding on bad data.

## Open Questions

- ~~The file is "entirely dedicated to the `(*LIMIT_MATCH=…)` /
  `(*LIMIT_DEPTH=…)` / `(*LIMIT_HEAP=…)` directives"; many cases may be
  untestable for a reason unrelated to hangs.~~ **Answered `.1`:** the framing
  was too pessimistic. 68 of 80 cases pass as-is; only 2 failures trace to
  unimplemented `(*LIMIT_*)` / recursion-loop semantics.
- ~~Roughly 41 cases are at stake (BACKLOG estimate, unverified).~~ **Answered
  `.1`: 80 cases**, not 41 — the estimate was low by ~2×.
- **Is +65.6s of sweep time worth 68 more verified cases?** The sweep runs on
  every `RGX_RUN_CONFORMANCE=1` gate. This is the real trade in `.2`, and it is
  a judgement about gate ergonomics, not correctness.
- Should the 2 RGX-side gaps (`:246`, `:254`) be tracked as their own defect —
  pattern-level `(*LIMIT_*)` verbs and infinite-recursion detection are genuine
  PCRE2 features RGX lacks — or accepted as documented residuals? `.2` decides;
  if "defect", it spawns a tree rather than living here.

## Blockers

- None. (`VM-LIMITS-SUBEXEC.1`, the precondition, closed 2026-07-21.)

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-07-21` | — | Exclusion rationale traced to `rgx-core/tests/pcre2_conformance.rs` `TESTINPUT_FILES` comment; its named cause fixed by `VM-LIMITS-SUBEXEC.1` the same day | `tree opened` |
| `2026-07-21` | `.1` | Temporarily added `"testinput15"` to `TESTINPUT_FILES` (local edit, reverted — this leaf commits no code), release build, full sweep: **80 parsed cases → 68 pass / 12 fail / 0 panic / 0 skip (85.0%)**. **The stated blocker is gone: nothing hangs.** Corpus total with it included would be 12,874 pass / 16 fail | `PASS — file is runnable` |
| `2026-07-21` | `.1` | Wall-time cost: full sweep **55.99s without** testinput15 → **121.60s with** it (**+65.6s**, roughly doubling the sweep). The file is 80 cases out of 12,890 — it is deliberately expensive (catastrophic-backtracking stress patterns now running to their honest 64M-step ceiling instead of hanging) | `measured — real cost to weigh in .2` |
| `2026-07-21` | `.1` | Failure triage (`RGX_CONFORMANCE_DUMP_CAT=""`), all 12 classified: **10 = one harness gap, not RGX defects** — under pcre2test's `allusedtext`, the printed span includes lookaround-consulted text, so the harness parses a wider "expected match" than the real match (`:115` `/abc(?=xyz)/` PCRE2 prints `abcxyz` vs RGX `abc`; same shape at `:119 :133 :138 :143 :171 :174 :177 :180 :246`). The harness already treats `allusedtext` as `ModifierAction::Ignore` for the *input* side but never adjusts the *expected-output* side. **2 = RGX-side semantic gaps**: `:246` `/b(?<!ax)(?!cx)/` and `:254` `/(a(?1)z\|\|(?1)++)$/` — PCRE2 reports no-match (pattern-level `(*LIMIT_*)` verbs / infinite-recursion detection, neither implemented in RGX), RGX matches | `triaged` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.1` | `CONFORMANCE-TESTINPUT15.1 — measure the excluded testinput15 corpus file` | Docs-only: the measuring edit was temporary and reverted. |

## Changelog

- `2026-07-21`: Created — surfaced while closing `VM-LIMITS-SUBEXEC.1`, whose
  fix removes the stated cause of the exclusion.
- `2026-07-21`: `.1` done. The exclusion's stated cause is confirmed gone (no
  hangs; 68/80 pass). Added `.3` for the `allusedtext` harness gap and moved it
  ahead of `.2` in the frontier.
