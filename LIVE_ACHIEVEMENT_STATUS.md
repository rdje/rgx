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

Every open roadmap lane is now tree-owned (`TASKTREE-ADOPT.2`, 2026-06-15).

| Roadmap lane | Owning tree | Status | Current frontier |
| --- | --- | --- | --- |
| Governance / process | [`TASKTREE-ADOPT`](docs/tasks/TASKTREE-ADOPT.md) | `active` | `TASKTREE-ADOPT.3` — retro-audit shipped work |
| Next — SOTA algorithmic perf gaps | [`PERF-SOTA-GAPS`](docs/tasks/PERF-SOTA-GAPS.md) | `active` | `.1` — inner-literal prefilter |
| Next — PCRE2 10.47+ syntax alignment | [`PCRE2-1047-SYNTAX`](docs/tasks/PCRE2-1047-SYNTAX.md) | `active` | `.1` — A12 capture-return VM semantics |
| Next — code-block expansion | [`CODEBLOCK-EXPANSION`](docs/tasks/CODEBLOCK-EXPANSION.md) | `active` | `.1` — inline-language ergonomics |
| Next — compile-time `<5×` | [`COMPILE-PERF-0073`](docs/tasks/COMPILE-PERF-0073.md) | `blocked` | PGEN-side (`PGEN-RGX-0073`) |
| Next — perf validation loop | [`RUNTIME-REMEASURE`](docs/tasks/RUNTIME-REMEASURE.md) | `blocked` | needs a quiescent machine (task #57) |
| Later — language bindings (A9) | [`A9-BINDINGS`](docs/tasks/A9-BINDINGS.md) | `deferred` | pending a demand signal |
| Later — crates.io release | [`RELEASE-CRATESIO`](docs/tasks/RELEASE-CRATESIO.md) | `parked` | user is the trigger |

Shipped lanes remain recorded in `CHANGES.md` / `RUST_CODEBASE_ANALYSIS.md`;
their retroactive mapping into trees is owned by `TASKTREE-ADOPT.3`.

## Latest Completed Slice

- `2026-06-15` — `TASKTREE-ADOPT.2`: decomposed all 7 open ROADMAP/BACKLOG
  lanes into real task trees (3 active, 2 blocked, 1 deferred, 1 parked).
- `2026-06-15` — `TASKTREE-ADOPT.1`: installed the repo-local task-tree
  tracking workflow and wired it into RGX's live docs and the Book.
