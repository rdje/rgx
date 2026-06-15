# LIVE ACHIEVEMENT STATUS

High-level workstream status board for RGX. This file links active roadmap
lanes to their owning task trees and names the current frontier leaf. It is
deliberately high-level — the detailed execution ledger lives in the task
trees under `docs/tasks/`, the chronological history in `CHANGES.md`, and the
forward plan in `ROADMAP.md`.

- **Task-tree index:** [`docs/TASK_TREE.md`](docs/TASK_TREE.md)
- **Workflow guide:** [`docs/TASK_TREE_README.md`](docs/TASK_TREE_README.md)
- **Last updated:** `2026-06-15`

## Snapshot

- **Correctness (gate-enforced):** PCRE2 10.47 `testinput1..29` conformance
  **12,806 pass / 4 fail / 0 panic / 0 skip**. The 4 residuals are by-design /
  won't-fix (Unicode/8-bit engine-model adjudications). Ratchet-gated in
  `rgx-core/tests/pcre2_conformance.rs`; any regression fails CI.
- **PGEN pin:** `subs/pgen` at `db6f8c68` (regex release 1.1.81 / integration
  contract 1.1.83). 88 PGEN-RGX reports filed; 87 closed; the octal-vs-backref
  cluster (0084/0085/0086/0087/0088) and the typed-shape cluster
  (0079/0080/0081/0082) are fully resolved. Sole substantive open report:
  `PGEN-RGX-0073` (compile-time perf; PGEN-side).
- **Feature parity:** ~98% PCRE2 feature-family coverage (explicitly
  hand-maintained estimate, not a measured number — see the Book).
- **Engine:** 4-tier dispatch (DFA → Pike-VM → JIT → backtracking VM) plus AC
  literal-alternation and TDFA capture recovery, all default-on. C1 (JIT) and
  C2 (NFA/DFA hybrid) shipped.
- **Process:** task-tree governance adopted `2026-06-15`. The Code-Change
  Doctrine is now binding — no code change without an owning task-tree leaf.

## Active Lanes → Task Trees

| Roadmap lane | Owning tree | Status | Current frontier |
| --- | --- | --- | --- |
| Governance / process | [`TASKTREE-ADOPT`](docs/tasks/TASKTREE-ADOPT.md) | `active` | `TASKTREE-ADOPT.2` — decompose the roadmap into trees |

All other roadmap lanes are either shipped (recorded in `CHANGES.md` /
`RUST_CODEBASE_ANALYSIS.md`) or captured as `Proposed Task Trees` in
[`docs/TASK_TREE.md`](docs/TASK_TREE.md) pending decomposition by
`TASKTREE-ADOPT.2`.

## Open Lanes Not Yet Tree-Owned (captured; awaiting `TASKTREE-ADOPT.2`)

| Item | Disposition | Source |
| --- | --- | --- |
| Compile-time gap to `<5×` PCRE2 (`PGEN-RGX-0073`) | `blocked` — PGEN-side, sole-parser design | ROADMAP "Next — perf" |
| Runtime (match-speed) clean re-measure + Book "Honest numbers" refresh | `pending` — needs a quiescent machine (task #57) | ROADMAP "Next — perf validation" |
| SOTA algorithmic perf gaps (inner-literal prefilter, SIMD byte-class, …) | `proposed` | ROADMAP "Next — surveyed" |
| PCRE2 10.47+ downstream syntax alignment | `proposed` | ROADMAP "Next" |
| Inline-language (Lua/JS/Rhai) ergonomics expansion | `proposed` | ROADMAP "Next" |
| A9 language bindings Phases 2–7 (over shipped `rgx-capi`) | `deferred` — pending demand signal | ROADMAP "Later" |
| crates.io publication readiness | `parked` — user is the trigger | ROADMAP "Later" / `project_release_strategy` |

## Latest Completed Slice

- `2026-06-15` — `TASKTREE-ADOPT.1`: installed the repo-local task-tree
  tracking workflow and wired it into RGX's live docs and the Book.
