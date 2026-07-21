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
- latest_commit: see `git log -1` — 2026-07-21 docs slice "PGEN 1.1.105 speed verified 26.9×; adoption blocked (0089/0090/0091); VM-LIMITS-SUBEXEC opened" (leaf `COMPILE-PERF-0078.1` execution; pin UNCHANGED `db6f8c68`).
- active_work_unit: none mid-flight. 2026-07-21 session: PGEN's 0078 speed closure verified on preview `960dddaa` (parse 26.9× faster; 8.44×/1.17× vs PCRE2; compile bottleneck now RGX-side); 3 PGEN reports filed (`0089` `(?[\b])` rejects-valid = adoption blocker, `0090` cold-clone bootstrap, `0091` version consts); submodule restored, lib 1202/0 green.
- next_action: PNT — `VM-LIMITS-SUBEXEC.1` (limits must bound lookbehind sub-execution; proven DoS gap, testinput2:6509) is the top frontier, then `DOCTRINE-ADOPT.2` (evidence archetype) / `PERF-SOTA-GAPS.1` (inner-literal prefilter; design notes preserved in the tree file). On a PGEN release fixing 0089: re-run `COMPILE-PERF-0078.1` (bump + REGEX-0098 absorption per `docs/knowledge/pgen-1104-verified-blocked.md` checklist).
- enforcement ACTIVE in this clone: `core.hooksPath = scripts/git-hooks`. pre-commit = KM derive-and-stage → `scripts/check_doctrines.sh --scope hook` (**the driver**, 7 doctrines: MEMORY-ARCH, KNOWLEDGE-MAP, PGEN-READONLY, DOCTRINE-REGISTRY-SYNC, CODE-CHANGE-LEAF, TWO-TRACK-DOCS, GATE-RECEIPT); commit-msg = work-unit-id; `run-local-ci.sh` runs `--scope ci` (E4). Adding a doctrine = check script + registry line + `DOCTRINE_ENFORCEMENT.md` §10 row — never hook logic. Grep `KNOWLEDGE_MAP.md` before re-deriving a fact.
- in_flight_uncommitted: none (after this commit).
- blockers: `COMPILE-PERF-0078` blocked on the PGEN 0089 fix release. Director RULED (2026-07-21, ADR 0005): the `<5×` compile bar is KEPT — closing it is RGX-side work (`.3`/`.4`/`.2` after adoption).
- invariants: conformance ratchet 12,806/4/0/0 is the merge gate; open PGEN reports = `0089/0090/0091` (`0078` closed 2026-07-21); working tree shows only `?? subs/pgen` (regenerated `generated/*`, never staged). NOTE: conformance testinput2 wall-time is dominated by the un-bounded lookbehind case until `VM-LIMITS-SUBEXEC.1` lands.
