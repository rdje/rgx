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
- latest_commit: see `git log -1` — 2026-07-21 "COMPILE-PERF-0078.1 — adopt PGEN 1.1.106; parse 26.3x faster". **Pin is now `d9d41c28` (rel 1.1.106 / contract 1.1.109)**, NOT `db6f8c68`.
- active_work_unit: none mid-flight. Today shipped, in order: `VM-LIMITS-SUBEXEC.1` (safety limits bound every VM dispatch loop; tree closed), `CONFORMANCE-TESTINPUT15.1` (measured the excluded corpus file: 68/80, no hangs), `COMPILE-PERF-0078.1` (PGEN 1.1.106 adopted: parse 26.3× faster / 8.64× vs PCRE2-no-JIT; REGEX-0098 absorbed; **all 91 PGEN-RGX reports now closed — zero open**).
- next_action: PNT — `COMPILE-PERF-0078.3` (adapter-boundary fast path: RGX spends ~10× the parse cost ingesting the AST-dump JSON; contract sanctions consuming `ParseContent::Shaped` natively or `to_json_value()` at the boundary), then `.4` (lazy C2 build). Also open: `CONFORMANCE-TESTINPUT15.3` (`allusedtext` harness gap → then `.2` adjudicate inclusion), `DOCTRINE-ADOPT.2`, `PERF-SOTA-GAPS.1`.
- enforcement ACTIVE in this clone: `core.hooksPath = scripts/git-hooks`. pre-commit = KM derive-and-stage → `scripts/check_doctrines.sh --scope hook` (**the driver**, 7 doctrines: MEMORY-ARCH, KNOWLEDGE-MAP, PGEN-READONLY, DOCTRINE-REGISTRY-SYNC, CODE-CHANGE-LEAF, TWO-TRACK-DOCS, GATE-RECEIPT); commit-msg = work-unit-id; `run-local-ci.sh` runs `--scope ci` (E4). Adding a doctrine = check script + registry line + `DOCTRINE_ENFORCEMENT.md` §10 row — never hook logic. Grep `KNOWLEDGE_MAP.md` before re-deriving a fact.
- in_flight_uncommitted: none (after this commit).
- blockers: none. `COMPILE-PERF-0078` is UNBLOCKED (pin adopted); per ADR 0005 the `<5×` bar is KEPT and is now RGX-side work (`.3`/`.4`/`.2`).
- invariants: conformance ratchet 12,806/4/0/0 is the merge gate; **zero open PGEN reports** (all 91 closed); the working tree is now **fully clean** — on pin `d9d41c28` the submodule gitignores `generated/`, so the long-standing `?? subs/pgen` marker is GONE (the regenerated parser still exists; its absence from `git status` is expected, not a missing bootstrap). VM safety-limit invariant: **one match attempt = one budget** — any new execution loop or context clone must charge/absorb it (`docs/knowledge/vm-limits-whole-attempt.md`).
