# RUNTIME-REMEASURE: Clean-machine runtime benchmark re-measure + Book refresh

## Metadata

- Tree ID: `RUNTIME-REMEASURE`
- Status: `blocked`
- Roadmap lane: `Next — Performance validation loop (task #57)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Produce a trustworthy runtime (post-compile, match-speed) benchmark snapshot on
a quiescent machine and refresh the Book's "Honest numbers" table so the
user-facing runtime figures are fresh, not the known-stale pre-TDFA
`c2-step8-final` numbers currently carrying a freshness banner.

## Non-Goals

- No engine change. This is measurement + documentation only.
- No new perf optimization (those are owned by `PERF-SOTA-GAPS`).

## Acceptance Criteria

- A full-mode benchmark capture runs on a quiescent machine (load avg low),
  producing stable ratios (not contention artifacts).
- `book/src/internals/performance.md` "Honest numbers" table refreshed with the
  fresh numbers; the stale/freshness banner removed once the data is real.
- Numbers stated only as verifiable facts (per `feedback_publish_readiness_bar`
  and the 2026-05-19 no-BS pass).

## Task Tree

- ID: `RUNTIME-REMEASURE`
  Status: `blocked`
  Goal: `Fresh, trustworthy runtime numbers in the Book.`
  Children: `RUNTIME-REMEASURE.1`, `RUNTIME-REMEASURE.2`

- ID: `RUNTIME-REMEASURE.1`
  Status: `blocked`
  Goal: `Run RGX_BENCHMARK_TREND_MODE=full ./scripts/capture-benchmark-trends.sh on a quiescent machine; capture RGX-vs-PCRE2 ratios for the headline benches.`
  Acceptance: `Capture taken under low machine load; ratios stable across two runs; raw artifacts retained (target/benchmark-trends/ is gitignored — record numbers in the commit/Book, not the artifacts).`
  Verification: `pending`
  Commit: `pending`

- ID: `RUNTIME-REMEASURE.2`
  Status: `pending`
  Goal: `Refresh book/src/internals/performance.md "Honest numbers" with the fresh ratios; drop the stale-pre-TDFA freshness banner; keep figures verifiable (operation/input/build stated).`
  Acceptance: `Book table updated; no overclaim; mdbook build clean; doctest ratchet unchanged.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| — | `RUNTIME-REMEASURE.1` | `blocked` | Needs a quiescent machine. |
| 2 | `RUNTIME-REMEASURE.2` | `pending` | Depends on `.1`'s fresh numbers. |

## Decisions

- `2026-06-15`: Tree owns task #57. The 2026-05-19 attempt (MEMORY f/u 23) ran
  under load avg ~5.6 (concurrent `cargo-mutants` + `specforge` jobs) and
  produced contention artifacts (e.g. a spurious "624% find_all regression"),
  so it was discarded and the Book table left labelled stale-pending-remeasure.

## Open Questions

- None blocking.

## Blockers

- **Blocker:** machine contention inflates absolute timings non-uniformly,
  making ratios meaningless. **Why it blocks:** publication-readiness bar
  forbids presenting contended numbers as fresh. **Unblock condition:** a
  quiescent machine (low load avg, no competing long-running jobs).
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

- `2026-06-15`: Created from ROADMAP "Performance validation loop" / task #57
  (leaf `TASKTREE-ADOPT.2`). Status `blocked` (machine quiescence).
