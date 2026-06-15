# LEDGER-HYGIENE: Reconcile the PGEN-RGX ledger to the maintainer state

## Metadata

- Tree ID: `LEDGER-HYGIENE`
- Status: `done`
- Roadmap lane: `Governance / process (ledger hygiene)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Bring the on-disk `pgen-issues/` ledger into agreement with the maintainer's
authoritative state (2026-06-15): **`PGEN-RGX-0078` is the sole active/open PGEN
bug — it replaces `PGEN-RGX-0073`, and every other report has been addressed.**
Resolve drift D1 raised by `RETRO-AUDIT`.

## Non-Goals

- No code change (this is a `pgen-issues/` tracker + live-doc reconciliation).
- Not rewriting the historical narrative in `CHANGES.md`/`MEMORY.md`.

## Acceptance Criteria

- `PGEN-RGX-0073.yaml` top-level `status:` reads `closed` (superseded by `0078`).
- `PGEN-RGX-0078.yaml` remains the only top-level `status: open` report.
- `RETRO-AUDIT` drift D1's overcount (loose-grep estimate) is corrected to the
  accurate finding.
- Live docs already say "0078 sole open, replaces 0073" (done in
  `TASKTREE-ADOPT.3`); confirm no residual stale claim.

## Finding correction (accurate ledger state)

`RETRO-AUDIT` D1 estimated "16 `status: open` matches / ~15 stale files" using a
**loose** `grep -l "status: open"` that matched the string anywhere in a file
(nested fix-attempt/history/resolution blocks). The accurate **top-level**
check (`^status: open`) shows only **two** reports open on disk: `0073` and
`0078`. The 13 files D1 listed (`0021/0022/0023/0027/0028/0033–0039/0053`) are
**already** `status: closed` at top level — they were never stale. So the real
remediation is a single flip: `0073 → closed` (`0078` stays open). Recorded
honestly: D1's count was an artifact of the loose grep, not a real 15-file
backlog.

## Task Tree

- ID: `LEDGER-HYGIENE`
  Status: `done`
  Goal: `Ledger agrees with the maintainer: 0078 sole open, 0073 superseded.`
  Children: `LEDGER-HYGIENE.1` (`done`)

- ID: `LEDGER-HYGIENE.1`
  Status: `done`
  Goal: `Flip PGEN-RGX-0073 to closed (superseded by 0078); confirm 0078 is the only top-level open report; correct RETRO-AUDIT D1's overcount.`
  Acceptance: `0073.yaml status: closed + resolution superseded_by 0078; ^status: open matches exactly {0078}; D1 corrected.`
  Verification: `PASS — grep '^status: open' over pgen-issues/PGEN-RGX-*.yaml returns exactly PGEN-RGX-0078; 0073.yaml status: closed (line 21) + closed_at + resolution.status: superseded / superseded_by: PGEN-RGX-0078; RETRO-AUDIT D1 corrected; mdbook clean.`
  Commit: `Docs: close PGEN-RGX-0073 (superseded by 0078) — ledger hygiene (leaf LEDGER-HYGIENE.1)`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| — | — | — | `.1` complete in this slice. `LEDGER-HYGIENE` closes. |

## Decisions

- `2026-06-15`: Flip `0073` to `closed` with `resolution.status: superseded` /
  `superseded_by: PGEN-RGX-0078` (rather than `fixed`) — the underlying
  compile-time perf issue is NOT fixed; it is tracked by the still-open `0078`.
  This honors both maintainer statements ("0078 replace 0073" + "every other
  report addressed").

## Open Questions

- None.

## Blockers

- None.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | `LEDGER-HYGIENE.1` | `^status: open` over `pgen-issues/PGEN-RGX-*.yaml` returns exactly `0078`; `0073.yaml` shows `status: closed` + `superseded_by: PGEN-RGX-0078`; docs-only / non-gate-affecting; `mdbook build book` clean | `pass` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `LEDGER-HYGIENE.1` | `Docs: close PGEN-RGX-0073 (superseded by 0078) — ledger hygiene (leaf LEDGER-HYGIENE.1)` | tracker + doc reconcile; not pushed unless user asks. |

## Changelog

- `2026-06-15`: Created + executed from `RETRO-AUDIT` drift D1 (leaf
  `TASKTREE-ADOPT.3` follow-up). Single-leaf tree; `0073` closed/superseded.
