# COMPILE-PERF-0073: Close the PCRE2 compile-time gap to <5×

## Metadata

- Tree ID: `COMPILE-PERF-0073`
- Status: `blocked`
- Roadmap lane: `Next — Performance: close the PCRE2 compile-time gap to <5×`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Reduce the `Regex::compile` wall-clock gap to **<5×** of PCRE2 compile,
accuracy-preserving — no semantic changes, no PGEN bypass, no behavioural
drift, 100% test suite green, PCRE2 conformance ratchet unchanged.

## Non-Goals

- Replacing PGEN with a hand-written parser (violates CLAUDE.md "PGEN is the
  sole parser"). Ruled out by hard project rule.
- Any AST simplification that could change match results.

## Acceptance Criteria

- `Regex::compile` geomean within `<5×` of PCRE2-10.47-no-JIT compile on the
  8-pattern bench corpus, measured per `book/src/internals/measurement-methodology.md`.
- Full `./scripts/run-local-ci.sh` green; PCRE2 conformance ratchet unchanged.

## Task Tree

- ID: `COMPILE-PERF-0073`
  Status: `blocked`
  Goal: `Regex::compile <5× of PCRE2 compile, accuracy-preserving.`
  Children: `COMPILE-PERF-0073.1`, `COMPILE-PERF-0073.2`

- ID: `COMPILE-PERF-0073.1`
  Status: `blocked`
  Goal: `Absorb PGEN regex-parser speed releases and re-measure the compile ratio on each subs/pgen bump.`
  Acceptance: `On a faster PGEN pin, re-measured geomean recorded in Book → Performance → "Compile-time performance"; ratchet held; pin bump owned by its own leaf (subs/pgen is a code change).`
  Verification: `pending`
  Commit: `pending`

- ID: `COMPILE-PERF-0073.2`
  Status: `deferred`
  Goal: `RGX-side trivial-pattern short-circuit AFTER PGEN parses (roadmap technique #5): cheap classifier (is_pure_literal / is_single_char_class) that skips NFA/DFA/JIT downstream work for trivial shapes. PGEN still parses (sole-parser rule preserved).`
  Acceptance: `Conservative classifier (semantic flags like (?i) fall through to full pipeline); measurable compile win on trivial CLI patterns; ratchet unchanged; full gate green.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| — | `COMPILE-PERF-0073.1` | `blocked` | The dominant cost is PGEN's regex parse (≈63–86% of compile wall-clock on pin `db6f8c68`); RGX-side levers are exhausted (technique #1 lazy `Engine::new` shipped 2026-04-25; #2–#4 subsumed). No RGX fix moves the needle until PGEN's parser is faster. |
| — | `COMPILE-PERF-0073.2` | `deferred` | The only remaining RGX-side lever; deferred because PGEN-parse dominance means it cannot close the gap alone, and it carries medium correctness risk (mis-classifying a semantic flag would mis-match). Reopen if it would measurably help post-PGEN improvement. |

## Decisions

- `2026-06-15`: Tree owns the open tracker `PGEN-RGX-0073`. Per user directive
  (2026-05-19), **no new PGEN bug report** is filed — `0073` stays the open
  tracker; metrics live in the Book. Parser-speed work is PGEN-side
  (sole-parser design); RGX files no workaround (`feedback_no_pgen_workarounds`).

## Open Questions

- None blocking. The unblock is external (PGEN release cadence).

## Blockers

- **Blocker:** PGEN's regex-grammar parse is ≈63–86% of `Regex::compile`
  wall-clock (geomean ≈214× vs PCRE2-no-JIT on `db6f8c68`). **Why it blocks:**
  RGX-side downstream work targets only the 14–37% remainder; it cannot reach
  `<5×` while PGEN parse dominates. **Unblock condition:** PGEN ships a faster
  regex-grammar parser (specialised codegen / sustained parser hot-path work).
  **Run instead:** any active non-blocked tree.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | tree created; no leaf executed | `n/a` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| — | `pending` | `pending` |

## Changelog

- `2026-06-15`: Created from ROADMAP "close the PCRE2 compile-time gap to <5×"
  (leaf `TASKTREE-ADOPT.2`). Status `blocked` (PGEN-side). Records technique #1
  (lazy `Engine::new`) as already shipped 2026-04-25.
