# MEMORY — resume pointer (layer A; overwrite-only, keep ≤ ~50 lines)

This file is **layer A** of the memory architecture (`MEMORY_ARCHITECTURE.md`):
a bounded, overwrite-only pointer to *where we are now*. It is **not a log** —
durable history lives in the task-trees (layer B, `docs/tasks/`), the decision
records (layer C, `docs/decisions/`), and git (layer D, `git log`). The former
~4,785-line append-only session log is preserved in git history (read it with
`git show <old-commit>:MEMORY.md` or `git log -- MEMORY.md`); it is no longer
carried forward.

## How to resume
- Read `MEMORY_ARCHITECTURE.md` (memory system) + `README.md` (project).
- Work is tracked in task-trees under `docs/tasks/` (index: `docs/TASK_TREE.md`;
  high-level board: `LIVE_ACHIEVEMENT_STATUS.md`); follow `COMMIT.md`. Every
  code change is owned by a task-tree leaf (Code-Change Doctrine, `CLAUDE.md`).
- Durable facts/decisions: `docs/decisions/` (layer C).

## Current state (OVERWRITE this block each update — do not append)
- latest_commit: see `git log -1` — 2026-07-21 "Bound every VM sub-execution by the attempt's budget (leaf `VM-LIMITS-SUBEXEC.1`)"; tree CLOSED. Pin UNCHANGED `db6f8c68`.
- active_work_unit: none mid-flight. Safety limits are now a **whole-attempt** contract: all three VM dispatch loops (`execute_at`, `execute_subexpr_inner_full`, `execute_at_continuation`) charge one shared `ctx.step_count` (`step_budget_exhausted`), and all three `clone_exec_context` sites (`probe_subexpr`, `execute_assertion_subexpr`, `execute_lookbehind_assertion`) fold their spend back (`absorb_sub_budget`). testinput2:6509 `/(?<=(\d{1,256}))X/`: 20+ min → 37 ms. 6 regression tests in `tests/adversarial.rs::limits_bound_*` (worker-thread + `recv_timeout`, so a regression fails instead of hanging).
- next_action: **USER-DIRECTED (2026-07-21): bump the `subs/pgen` pin — upstream reports it has fixed `0089`/`0090`/`0091`.** That unblocks `COMPILE-PERF-0078` (leaf `.1`): follow the `docs/knowledge/pgen-1104-verified-blocked.md` checklist (regenerate the parser via `make -C subs/pgen/rust regex_parser_bootstrap`, absorb the documented REGEX-0098 named-reference boundary move = 4 RGX test updates, re-verify 0089's `(?[\b])` repro, full gate + ratchet). User also directed reading PGEN's regex book + handoff/integration doc + the contract as part of it. Then: `CONFORMANCE-TESTINPUT15.3` (`allusedtext` harness gap) → `.2` (adjudicate inclusion), `DOCTRINE-ADOPT.2`, `PERF-SOTA-GAPS.1`.
- enforcement ACTIVE in this clone: `core.hooksPath = scripts/git-hooks`. pre-commit = KM derive-and-stage → `scripts/check_doctrines.sh --scope hook` (**the driver**, 7 doctrines: MEMORY-ARCH, KNOWLEDGE-MAP, PGEN-READONLY, DOCTRINE-REGISTRY-SYNC, CODE-CHANGE-LEAF, TWO-TRACK-DOCS, GATE-RECEIPT); commit-msg = work-unit-id; `run-local-ci.sh` runs `--scope ci` (E4). Adding a doctrine = check script + registry line + `DOCTRINE_ENFORCEMENT.md` §10 row — never hook logic. Grep `KNOWLEDGE_MAP.md` before re-deriving a fact.
- in_flight_uncommitted: none (after this commit).
- blockers: `COMPILE-PERF-0078` blocked on the PGEN 0089 fix release. Director RULED (2026-07-21, ADR 0005): the `<5×` compile bar is KEPT — closing it is RGX-side work (`.3`/`.4`/`.2` after adoption).
- invariants: conformance ratchet 12,806/4/0/0 is the merge gate; open PGEN reports = `0089/0090/0091` (`0078` closed 2026-07-21); working tree shows only `?? subs/pgen` (regenerated `generated/*`, never staged). VM safety-limit invariant: **one match attempt = one budget** — any new execution loop or context clone must charge/absorb it (`docs/knowledge/vm-limits-whole-attempt.md`).
