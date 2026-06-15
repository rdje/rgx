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
  contract 1.1.83). Per the maintainer (2026-06-15), **`PGEN-RGX-0078` is the
  sole active/open PGEN bug** (compile-time perf, PCRE2-relative; PGEN has not
  yet had time to address it) — it **replaces `PGEN-RGX-0073`**, and **every
  other PGEN-RGX report has been addressed**. (The on-disk YAML `status:` fields
  are being reconciled — see `RETRO-AUDIT` D1 / the `LEDGER-HYGIENE` follow-up.)
- **Feature parity:** ~98% PCRE2 feature-family coverage (explicitly
  hand-maintained estimate, not a measured number — see the Book).
- **Engine:** 4-tier dispatch (DFA → Pike-VM → JIT → backtracking VM) plus AC
  literal-alternation and TDFA capture recovery, all default-on. C1 (JIT) and
  C2 (NFA/DFA hybrid) shipped.
- **Process:** task-tree governance + the durable memory architecture (layers
  A `MEMORY.md` / B task-trees / C `docs/decisions/` / D git, E1–E4) + the
  Knowledge Map retrieval layer (derived `KNOWLEDGE_MAP.md` over
  `docs/knowledge/` fact cards) all adopted `2026-06-15`. The Code-Change
  Doctrine is binding — no code change without an owning task-tree leaf;
  `core.hooksPath` enforces the memory-arch check + KM gen/stage/check +
  gate-receipt + work-unit-id commit subjects locally, and `run-local-ci.sh`/CI
  enforces server-side. **Grep `KNOWLEDGE_MAP.md` before re-deriving any logged fact.**

## Active Lanes → Task Trees

Every open roadmap lane is now tree-owned (`TASKTREE-ADOPT.2`, 2026-06-15).

| Roadmap lane | Owning tree | Status | Current frontier |
| --- | --- | --- | --- |
| Next — SOTA algorithmic perf gaps | [`PERF-SOTA-GAPS`](docs/tasks/PERF-SOTA-GAPS.md) | `active` | `.1` — inner-literal prefilter |
| Next — PCRE2 10.47+ syntax alignment | [`PCRE2-1047-SYNTAX`](docs/tasks/PCRE2-1047-SYNTAX.md) | `active` | `.1` — A12 capture-return VM semantics |
| Next — code-block expansion | [`CODEBLOCK-EXPANSION`](docs/tasks/CODEBLOCK-EXPANSION.md) | `active` | `.1` — inline-language ergonomics |
| Next — compile-time `<5×` | [`COMPILE-PERF-0078`](docs/tasks/COMPILE-PERF-0078.md) | `blocked` | PGEN-side (`PGEN-RGX-0078`, sole open bug) |
| Next — perf validation loop | [`RUNTIME-REMEASURE`](docs/tasks/RUNTIME-REMEASURE.md) | `blocked` | needs a quiescent machine (task #57) |
| Later — language bindings (A9) | [`A9-BINDINGS`](docs/tasks/A9-BINDINGS.md) | `deferred` | pending a demand signal |
| Later — crates.io release | [`RELEASE-CRATESIO`](docs/tasks/RELEASE-CRATESIO.md) | `parked` | user is the trigger |
| Governance / process | [`TASKTREE-ADOPT`](docs/tasks/TASKTREE-ADOPT.md) · [`RETRO-AUDIT`](docs/tasks/RETRO-AUDIT.md) · [`LEDGER-HYGIENE`](docs/tasks/LEDGER-HYGIENE.md) · [`MEMORY-ARCHITECTURE-DOC`](docs/tasks/MEMORY-ARCHITECTURE-DOC.md) · [`KNOWLEDGE-MAP-DOC`](docs/tasks/KNOWLEDGE-MAP-DOC.md) | `done` | — (complete; see Completed Task Trees in `docs/TASK_TREE.md`) |

Ledger reconciled (`LEDGER-HYGIENE`, 2026-06-15): `PGEN-RGX-0078` is the sole
top-level open report (`0073` closed/superseded). Shipped lanes remain recorded
in `CHANGES.md` / `RUST_CODEBASE_ANALYSIS.md`.

## Latest Completed Slice

- `2026-06-15` — `BUILD-FLOW` (`.1`–`.4`): zero-friction build for downstream /
  submodule consumers (from LINKEDSPEC's repro). Root `make` hides the PGEN
  bootstrap (Path A); the pgen-free `--no-default-features` build is restored
  (36→0 errors) and CI-guarded (Path B); `docs/INTEGRATION.md` is the precise
  downstream handoff guide (`.4`). Full gate + PCRE2 conformance GREEN.
- `2026-06-15` — `KNOWLEDGE-MAP-DOC` (`.1`–`.4`): adopted the Knowledge Map
  retrieval layer — `knowledge-map/` bundle + 4 seed fact cards → derived
  `KNOWLEDGE_MAP.md` (22 question keys) + pre-commit/CI derive-and-diff gate
  (proven to bite). Archaeology for logged structural facts is now one lookup.
- `2026-06-15` — `MEMORY-ARCHITECTURE-DOC` (`.1`–`.5`): adopted the durable
  memory architecture — standard + layer-C `docs/decisions/` (4 ADRs) + MEMORY.md
  demoted to a 25-line resume pointer + E1–E4 enforcement (active, full
  run-local-ci.sh green, gates proven to bite).
- `2026-06-15` — `LEDGER-HYGIENE.1`: closed `PGEN-RGX-0073` (superseded by
  `0078`); confirmed `0078` is the sole top-level open report; corrected
  `RETRO-AUDIT` D1's loose-grep overcount.
- `2026-06-15` — `TASKTREE-ADOPT.3`: retro-audited shipped code into the
  `RETRO-AUDIT` tree (all major subsystems verified present); flagged the
  PGEN-RGX ledger drift (D1) and folded in the maintainer correction
  (`0078` replaces `0073`; `0078` sole open bug). `TASKTREE-ADOPT` complete.
- `2026-06-15` — `TASKTREE-ADOPT.2`: decomposed all 7 open ROADMAP/BACKLOG
  lanes into real task trees (3 active, 2 blocked, 1 deferred, 1 parked).
- `2026-06-15` — `TASKTREE-ADOPT.1`: installed the repo-local task-tree
  tracking workflow and wired it into RGX's live docs and the Book.
