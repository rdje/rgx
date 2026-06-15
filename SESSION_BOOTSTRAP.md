# SESSION BOOTSTRAP

New-session startup ritual for AI/LLM handoff. Read these in order before doing
any work.

1. `README.md` — the navigation map; read it and the referenced `.md` files.
2. `CLAUDE.md` — non-negotiable project rules (loaded automatically).
3. `COMMIT.md` — the commit workflow contract.
4. `MEMORY_ARCHITECTURE.md` — the memory system (4 layers + enforcement).
5. `MEMORY.md` — the bounded **layer-A resume pointer**; read it for current
   state + the active frontier (durable history is in `docs/tasks/`,
   `docs/decisions/`, and git — not in `MEMORY.md` anymore).
6. `LIVE_ACHIEVEMENT_STATUS.md` — high-level status board (lanes → trees).
7. `docs/TASK_TREE.md` — the task-tree workflow, active-tree index, PNT
   selection rules, and the **binding Code-Change Doctrine**.
8. The active task files named in `docs/TASK_TREE.md` (under `docs/tasks/`).

Then, thoroughly and precisely analyze the Rust codebase as needed for the task
at hand, and update `RUST_CODEBASE_ANALYSIS.md` if it has drifted.

## Doctrine reminder (binding)

**No code change may happen unless it is first owned by a task-tree leaf**
(`docs/TASK_TREE.md` → Code-Change Doctrine). Before touching code: create or
extend a tree so a leaf owns the change, implement only that leaf, then run the
full `COMMIT.md` workflow with the leaf ID in the commit subject or first body
line.

When the user asks for PNT ("pick the next task"), select the first eligible
leaf from the active tree's current frontier and roll.
