# LIVE ACHIEVEMENT STATUS

High-level workstream status board for RGX. This file links active roadmap
lanes to their owning task trees and names the current frontier leaf. It is
deliberately high-level — the detailed execution ledger lives in the task
trees under `docs/tasks/`, the chronological history in `CHANGES.md`, and the
forward plan in `ROADMAP.md`.

- **Task-tree index:** [`docs/TASK_TREE.md`](docs/TASK_TREE.md)
- **Workflow guide:** [`docs/TASK_TREE_README.md`](docs/TASK_TREE_README.md)
- **Last updated:** `2026-07-21`

## Snapshot

- **Correctness (gate-enforced):** PCRE2 10.47 `testinput1..29` conformance
  **12,806 pass / 4 fail / 0 panic / 0 skip**. The 4 residuals are by-design /
  won't-fix (Unicode/8-bit engine-model adjudications). Ratchet-gated in
  `rgx-core/tests/pcre2_conformance.rs`; any regression fails CI.
- **PGEN pin:** `subs/pgen` at `db6f8c68` (regex release 1.1.81 / integration
  contract 1.1.83). **PGEN's `0078` speed campaign is CLOSED upstream and
  RGX-verified (2026-07-21, preview pin `960dddaa` / rel 1.1.105):** raw parse
  **26.9× faster** (geomean 2.75µs), **8.44× vs PCRE2-no-JIT / 1.17× vs
  PCRE2+JIT** on RGX's standard instrument — but the fast pin is
  **unadoptable**: `PGEN-RGX-0089` (`(?[\b])` rejects-valid regression,
  oracle-proven) + `0090` (cold-clone bootstrap broken) + `0091`
  (version-constant drift) filed with full repro bundles. `0078` is closed;
  **0089/0090/0091 are the open PGEN reports.** Adoption also absorbs the
  documented REGEX-0098 named-reference boundary move (4 RGX test updates).
  Compile-bottleneck inversion on the fast pin: raw parse ~4–18% of
  `Regex::compile` — remaining compile-time work is RGX-side
  (`COMPILE-PERF-0078.3`/`.4`).
- **Feature parity:** ~98% PCRE2 feature-family coverage (explicitly
  hand-maintained estimate, not a measured number — see the Book).
- **Engine:** 4-tier dispatch (DFA → Pike-VM → JIT → backtracking VM) plus AC
  literal-alternation and TDFA capture recovery, all default-on. C1 (JIT) and
  C2 (NFA/DFA hybrid) shipped.
- **Process — 4 portable architectures.** Task-trees (work memory) + the durable
  memory architecture (layers A `MEMORY.md` / B task-trees / C `docs/decisions/`
  / D git) + the Knowledge Map (derived `KNOWLEDGE_MAP.md` over
  `docs/knowledge/` fact cards), all adopted `2026-06-15`; and since
  `2026-07-21` the **Doctrine Enforcement Architecture**
  (`DOCTRINE_ENFORCEMENT.md`, adopted from PGEN, ADR 0006) — every mechanizable
  doctrine is a deterministic check run from one registry+driver
  (`scripts/check_doctrines.sh`), gated at E3 (pre-commit hook) and E4
  (`run-local-ci.sh`, which CI runs). **7 doctrines enforced:** `MEMORY-ARCH`,
  `KNOWLEDGE-MAP`, `PGEN-READONLY`, `DOCTRINE-REGISTRY-SYNC`,
  `CODE-CHANGE-LEAF`, `TWO-TRACK-DOCS`, `GATE-RECEIPT`. The Code-Change
  Doctrine is therefore mechanically binding, not advisory. **Grep
  `KNOWLEDGE_MAP.md` before re-deriving any logged fact.**

## Active Lanes → Task Trees

Every open roadmap lane is now tree-owned (`TASKTREE-ADOPT.2`, 2026-06-15).

| Roadmap lane | Owning tree | Status | Current frontier |
| --- | --- | --- | --- |
| Now — production safety (limits DoS gap, found 2026-07-21) | [`VM-LIMITS-SUBEXEC`](docs/tasks/VM-LIMITS-SUBEXEC.md) | `active` | `.1` — limits must bound lookbehind sub-execution |
| Next — SOTA algorithmic perf gaps | [`PERF-SOTA-GAPS`](docs/tasks/PERF-SOTA-GAPS.md) | `active` | `.1` — inner-literal prefilter |
| Next — PCRE2 10.47+ syntax alignment | [`PCRE2-1047-SYNTAX`](docs/tasks/PCRE2-1047-SYNTAX.md) | `active` | `.1` — A12 capture-return VM semantics |
| Next — code-block expansion | [`CODEBLOCK-EXPANSION`](docs/tasks/CODEBLOCK-EXPANSION.md) | `active` | `.1` — inline-language ergonomics |
| Next — compile-time `<5×` | [`COMPILE-PERF-0078`](docs/tasks/COMPILE-PERF-0078.md) | `blocked` | speed verified upstream; adoption blocked on `PGEN-RGX-0089` fix release (then `.1` absorb + `.3`/`.4` RGX-side levers) |
| Next — perf validation loop | [`RUNTIME-REMEASURE`](docs/tasks/RUNTIME-REMEASURE.md) | `blocked` | needs a quiescent machine (task #57) |
| Later — language bindings (A9) | [`A9-BINDINGS`](docs/tasks/A9-BINDINGS.md) | `deferred` | pending a demand signal |
| Later — crates.io release | [`RELEASE-CRATESIO`](docs/tasks/RELEASE-CRATESIO.md) | `parked` | user is the trigger |
| Governance — doctrine enforcement (4th portable architecture) | [`DOCTRINE-ADOPT`](docs/tasks/DOCTRINE-ADOPT.md) | `active` | `.2` — the evidence archetype (`.1` shipped: driver + 7 doctrines gated at E3+E4) |
| Governance / process | [`TASKTREE-ADOPT`](docs/tasks/TASKTREE-ADOPT.md) · [`RETRO-AUDIT`](docs/tasks/RETRO-AUDIT.md) · [`LEDGER-HYGIENE`](docs/tasks/LEDGER-HYGIENE.md) · [`MEMORY-ARCHITECTURE-DOC`](docs/tasks/MEMORY-ARCHITECTURE-DOC.md) · [`KNOWLEDGE-MAP-DOC`](docs/tasks/KNOWLEDGE-MAP-DOC.md) | `done` | — (complete; see Completed Task Trees in `docs/TASK_TREE.md`) |

Ledger reconciled (`LEDGER-HYGIENE`, 2026-06-15): `PGEN-RGX-0078` is the sole
top-level open report (`0073` closed/superseded). Shipped lanes remain recorded
in `CHANGES.md` / `RUST_CODEBASE_ANALYSIS.md`.

## Latest Completed Slice

- `2026-07-21` — `DOCTRINE-ADOPT.1`: adopted the **Doctrine Enforcement
  Architecture** (RGX's 4th portable architecture, from PGEN). One registry +
  driver (`scripts/check_doctrines.sh`) now runs 7 doctrine checks and gates
  both the pre-commit hook (E3) and `run-local-ci.sh`/CI (E4); new checks
  `PGEN-READONLY`, `DOCTRINE-REGISTRY-SYNC`, `CODE-CHANGE-LEAF`,
  `TWO-TRACK-DOCS` + `GATE-RECEIPT` extracted from the hook. Every check
  negative-tested to prove it bites; full gate green. ADR 0006.
- `2026-07-21` — `COMPILE-PERF-0078.1` execution (adoption attempt +
  verification; docs-only outcome): PGEN's 0078 speed-campaign closure
  verified on preview pin `960dddaa` — parse 26.9× faster, 8.44× vs
  PCRE2-no-JIT / 1.17× vs +JIT (RGX instrument; bundle in
  `pgen-issues/artifacts/PGEN-RGX-0078/measurements/`); adoption blocked →
  `PGEN-RGX-0089` (`(?[\b])` rejects-valid) / `0090` (cold-clone bootstrap) /
  `0091` (version-constant drift) filed with protocol-grade artifacts;
  submodule restored to `db6f8c68` (lib re-verified 1202/0). Discovered +
  tree'd the pin-independent `VM-LIMITS-SUBEXEC` limits gap (lookbehind
  sub-execution escapes `set_max_steps`; testinput2:6509). Book performance +
  safety-limits chapters updated; `PGEN-RGX-0078` closed.
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
