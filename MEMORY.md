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
- latest_commit: `e5d352a` — "BUILD-FLOW.4 — add docs/INTEGRATION.md downstream handoff guide". **Pushed to origin/main** 2026-06-15 (`b771c7b..e5d352a`); the local `main` was in sync at push (0 ahead). This pointer-refresh commit lands on top — `git log -1` / `git status -sb` for the true head/ahead-count.
- active_work_unit: none in progress. All five governance trees (TASKTREE-ADOPT, RETRO-AUDIT, LEDGER-HYGIENE, MEMORY-ARCHITECTURE-DOC, KNOWLEDGE-MAP-DOC) + the downstream `BUILD-FLOW` fix are done + pushed.
- next_action: PNT into the active CODE trees — `PERF-SOTA-GAPS.1` (inner-literal prefilter) → `PCRE2-1047-SYNTAX.1` (A12 capture-return VM) → `CODEBLOCK-EXPANSION.1` (inline-lang ergonomics; likely at-parity). Each = real engine change → full `./scripts/run-local-ci.sh` + PCRE2 conformance ratchet per leaf; one leaf at a time. Blocked/deferred/parked: `COMPILE-PERF-0078`, `RUNTIME-REMEASURE`, `A9-BINDINGS`, `RELEASE-CRATESIO`.
- enforcement ACTIVE in this clone: `core.hooksPath = scripts/git-hooks`. pre-commit = memory-arch check + KM gen/stage/check + gate-receipt; commit-msg = work-unit-id. Memory layers A/B/C/D + E1–E4 + the Knowledge Map all live. Grep `KNOWLEDGE_MAP.md` before re-deriving a fact. Downstream build answer: `docs/INTEGRATION.md` + `docs/tasks/BUILD-FLOW.md`.
- in_flight_uncommitted: none.
- blockers: none.
- invariants: conformance ratchet 12,806/4/0/0 is the merge gate; the sole open PGEN bug is `PGEN-RGX-0078`; working tree shows only `?? subs/pgen` (regenerated `generated/*`, never staged).
