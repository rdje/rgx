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
- **PGEN pin:** `subs/pgen` at **`d9d41c28`** (regex release **1.1.106** /
  integration contract **1.1.109**), adopted `2026-07-21` — a 24-release jump.
  Raw PGEN parse geomean **2.81µs = 26.3× faster** than the former `db6f8c68`
  pin; **8.64× vs PCRE2-no-JIT**, **1.20× vs PCRE2+JIT** (3/8 patterns parse
  faster than PCRE2 compiles with JIT). **RGX has ZERO open PGEN-RGX reports** —
  all 91 (`0001`–`0091`) closed, including the three adoption blockers
  (`0089` rejects-valid, `0090` cold-clone bootstrap, `0091` version drift),
  each re-verified on the adopted pin. Ratchet held 12,806/4/0/0 across the
  bump; AST schema unchanged (`1`). **Compile bottleneck inverted:** raw parse
  is ~4% of `Regex::compile`; the adapter boundary + eager C2 build dominate —
  remaining `<5×` work is RGX-side (`COMPILE-PERF-0078.3`/`.4`).
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
| Now — production safety (limits DoS gap, found + fixed 2026-07-21) | [`VM-LIMITS-SUBEXEC`](docs/tasks/VM-LIMITS-SUBEXEC.md) | `done` | — (closed; limits now bound every execution path) |
| Now — PCRE2 parity (accuracy): excluded corpus file | [`CONFORMANCE-TESTINPUT15`](docs/tasks/CONFORMANCE-TESTINPUT15.md) | `active` | `.3` — close the `allusedtext` harness gap, then `.2` adjudicate (`.1` measured: 68/80 pass, **no hangs**) |
| Next — SOTA algorithmic perf gaps | [`PERF-SOTA-GAPS`](docs/tasks/PERF-SOTA-GAPS.md) | `active` | `.1` — inner-literal prefilter |
| Next — PCRE2 10.47+ syntax alignment | [`PCRE2-1047-SYNTAX`](docs/tasks/PCRE2-1047-SYNTAX.md) | `active` | `.1` — A12 capture-return VM semantics |
| Next — code-block expansion | [`CODEBLOCK-EXPANSION`](docs/tasks/CODEBLOCK-EXPANSION.md) | `active` | `.1` — inline-language ergonomics |
| Next — compile-time `<5×` | [`COMPILE-PERF-0078`](docs/tasks/COMPILE-PERF-0078.md) | `active` | `.3` — adapter-boundary fast path (pin ADOPTED: parse 26.3× faster; bottleneck now RGX-side) |
| Next — perf validation loop | [`RUNTIME-REMEASURE`](docs/tasks/RUNTIME-REMEASURE.md) | `blocked` | needs a quiescent machine (task #57) |
| Later — language bindings (A9) | [`A9-BINDINGS`](docs/tasks/A9-BINDINGS.md) | `deferred` | pending a demand signal |
| Later — crates.io release | [`RELEASE-CRATESIO`](docs/tasks/RELEASE-CRATESIO.md) | `parked` | user is the trigger |
| Governance — doctrine enforcement (4th portable architecture) | [`DOCTRINE-ADOPT`](docs/tasks/DOCTRINE-ADOPT.md) | `active` | `.2` — the evidence archetype (`.1` shipped: driver + 7 doctrines gated at E3+E4) |
| Governance / process | [`TASKTREE-ADOPT`](docs/tasks/TASKTREE-ADOPT.md) · [`RETRO-AUDIT`](docs/tasks/RETRO-AUDIT.md) · [`LEDGER-HYGIENE`](docs/tasks/LEDGER-HYGIENE.md) · [`MEMORY-ARCHITECTURE-DOC`](docs/tasks/MEMORY-ARCHITECTURE-DOC.md) · [`KNOWLEDGE-MAP-DOC`](docs/tasks/KNOWLEDGE-MAP-DOC.md) | `done` | — (complete; see Completed Task Trees in `docs/TASK_TREE.md`) |

Ledger status (2026-07-21): **no open PGEN-RGX reports** — all 91 closed.
Shipped lanes remain recorded in `CHANGES.md` / `RUST_CODEBASE_ANALYSIS.md`.

## Latest Completed Slice

- `2026-07-21` — `COMPILE-PERF-0078.1`: **the fast PGEN parser is ADOPTED**
  (user-directed once upstream fixed the blockers). Pin `db6f8c68` (1.1.81) →
  **`d9d41c28` (1.1.106 / contract 1.1.109)**, 24 releases. Raw parse is
  **26.3× faster** (geomean 2.81µs; 8.64× vs PCRE2-no-JIT, 1.20× vs +JIT),
  reproducing the pre-adoption preview within +2.4%. All three blockers
  re-verified rather than trusted: `(?[\b])`/`(?[[\b]])` match U+0008 again,
  the cold-clone bootstrap completes with no workaround, and the version
  constants match the contract identity — **RGX now has zero open PGEN
  reports**. Absorbed PGEN's REGEX-0098 boundary move (unknown named references
  reject at parse time; tests now match the diagnostic *code*, not prose) and
  added a 9-form must-reject fixture family. Lib 1203/0; **ratchet held
  12,806/4/0/0**; AST dumps byte-identical apart from `api_version`. The
  compile bottleneck is now provably RGX-side, unblocking `.3`/`.4`.
- `2026-07-21` — `CONFORMANCE-TESTINPUT15.1` (measurement, docs-only): the one
  PCRE2 corpus file RGX excludes, `testinput15`, **no longer hangs** — its
  exclusion's stated cause was the limits bug fixed hours earlier. Measured
  **80 cases: 68 pass / 12 fail / 0 panic**, at a cost of +65.6s sweep time.
  Triage: **10 of the 12 failures are a single harness gap** (pcre2test's
  `allusedtext` prints a span including lookaround-consulted text; the harness
  never adjusts the expected side), 2 are real RGX gaps (pattern-level
  `(*LIMIT_*)` verbs, infinite-recursion detection). Added `.3` to close the
  harness gap *before* `.2` adjudicates inclusion. Also corrected a
  transparency defect this surfaced: README claimed "no silent skips, no hidden
  asterisks" while silently excluding the file — now declared there and
  documented in the Book's Testing Philosophy.
- `2026-07-21` — `VM-LIMITS-SUBEXEC.1` (tree closed): **safety limits now bound
  every VM execution path.** Only the top-level dispatch loop had been counting
  steps, and speculative sub-contexts silently restarted the budget — so a
  variable-length lookbehind ran 20+ minutes on an 8-byte subject with a
  1,000,000-step cap set (PCRE2 `testinput2:6509`). All three dispatch loops now
  charge one shared per-attempt budget and all three cloned-context sites fold
  their spend back; the case completes in **37 ms**. 6 regression tests (each
  timeout-guarded on a worker thread so a regression fails rather than hangs);
  full gate + conformance ratchet green. Book: Safety Limits' "Known gap"
  section replaced by the positive guarantee; VM chapter documents the
  three-loop / one-counter invariant.
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
