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
- latest_commit: `8b0ae4a` — "MEMORY-ARCHITECTURE-DOC.5 …" (the KNOWLEDGE-MAP-DOC.1 commit lands on top; run `git log -1` for the true head; ahead of origin: ~12; push only on user request, `feedback_no_auto_push`).
- active_work_unit: `BUILD-FLOW` → frontier leaf: `.2` (pending) — fix the `--no-default-features` (pgen-free) build.
- next_action: `BUILD-FLOW.2` — fix the pgen-free reference build (36 errors at HEAD: B1 `CharRange` import gated on pgen-parser but `posix_class_ranges()` (parsing.rs:7293) uses it ungated; B2 non-exhaustive `ast::Regex` match in parser.rs; B3 residual edition/`_`/unreachable once B1/B2 clear). Default path unchanged. **GATE-AFFECTING → full run-local-ci.sh + PCRE2 conformance ratchet** (touches parsing). Then `.3` verify + KM card + downstream response + close. (`.1` done: root `make` build entrypoint hides the PGEN bootstrap — fixes LINKEDSPEC Path A.) Then the active CODE trees (`PERF-SOTA-GAPS.1` etc.).
- enforcement ACTIVE in this clone: `core.hooksPath = scripts/git-hooks`. pre-commit = memory-arch check + KM gen/stage/check + gate-receipt; commit-msg = work-unit-id. Layers A/B/C/D + E1–E4 + the Knowledge Map all live. Grep `KNOWLEDGE_MAP.md` before re-deriving a fact.
- other active trees (pick after MEMORY-ARCHITECTURE-DOC closes): `PERF-SOTA-GAPS.1` (inner-literal prefilter), `PCRE2-1047-SYNTAX.1` (A12 capture-return VM semantics), `CODEBLOCK-EXPANSION.1` (inline-lang ergonomics; preliminary read = likely at-parity). Blocked/deferred/parked: `COMPILE-PERF-0078`, `RUNTIME-REMEASURE`, `A9-BINDINGS`, `RELEASE-CRATESIO`.
- in_flight_uncommitted: none.
- blockers: none. (`.4`/`.5` require a working `./scripts/run-local-ci.sh` build in this environment — unverified here.)
- invariants: conformance ratchet 12,806/4/0/0 is the merge gate; the sole open PGEN bug is `PGEN-RGX-0078`; working tree shows only `?? subs/pgen` (regenerated `generated/*`, never staged).
