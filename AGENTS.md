# Agent bootstrap (read this first, whatever AI / harness you are)

This is the tool-neutral entrypoint. Every harness auto-reads a different file
(`CLAUDE.md`, `.cursorrules`, `.github/copilot-instructions.md`, …); they all
point here so discovery is solved once.

1. Read `README.md` (project objective, layout, build/test commands).
2. Read `MEMORY_ARCHITECTURE.md` (how memory + continuity work here — **MANDATORY**).
3. Resume from `MEMORY.md` — the bounded **layer-A resume pointer** (current
   state + next action) → then the active task-tree's frontier
   (`docs/TASK_TREE.md`; high-level board `LIVE_ACHIEVEMENT_STATUS.md`).
4. Track **all** work in task-trees under `docs/tasks/`; record durable facts in
   `docs/decisions/` (layer C); commit per `COMMIT.md` with the work-unit (leaf)
   id in the subject. **Every code change must be owned by a task-tree leaf**
   (the Code-Change Doctrine — `CLAUDE.md` / `docs/TASK_TREE.md`).
5. Before committing run `scripts/check_memory_architecture.sh`; for
   gate-affecting (Rust / Cargo / CI / scripts) changes run
   `./scripts/run-local-ci.sh` — CI runs both too. Activate the local hooks once
   per clone: `./scripts/setup-hooks.sh`.

Memory layers: **A** `MEMORY.md` (resume pointer) · **B** task-trees
(`docs/tasks/`) · **C** decision records (`docs/decisions/`) · **D** git history.
Nothing important may live only in this conversation — route it to a layer and
commit before the turn ends.
