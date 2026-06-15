---
id: governance-entrypoints
title: How RGX tracks work and memory — task-trees, Code-Change Doctrine, memory layers
answers:
  - "how is work tracked in RGX"
  - "where is the task tree / how do I pick the next task"
  - "can I make a code change without a task"
  - "how does memory and continuity work in RGX"
  - "what should an agent read first"
  - "what is the Code-Change Doctrine"
date: 2026-06-15
status: current
tags: [governance, task-tree, memory, bootstrap]
evidence: AGENTS.md; docs/TASK_TREE.md; MEMORY_ARCHITECTURE.md; CLAUDE.md
reverify: sed -n '1,12p' AGENTS.md
---

Start at `AGENTS.md` (tool-neutral bootstrap) → `README.md` → `MEMORY_ARCHITECTURE.md`
→ `MEMORY.md` (bounded resume pointer) → the active task-tree's frontier in
`docs/TASK_TREE.md` (board: `LIVE_ACHIEVEMENT_STATUS.md`).

**Code-Change Doctrine (binding):** no code change may be committed unless a
task-tree leaf owns it first; the leaf id goes in the commit subject. Memory is
layered: **A** `MEMORY.md` (overwrite-only pointer, ≤ cap) · **B** task-trees
(`docs/tasks/`) · **C** decision records (`docs/decisions/`) · **D** git. A
derived **Knowledge Map** (`KNOWLEDGE_MAP.md`) indexes durable facts; grep it
before re-deriving. Local hooks (`core.hooksPath = scripts/git-hooks`) + CI
enforce all of it. Canonical homes: `docs/TASK_TREE.md`, `MEMORY_ARCHITECTURE.md`,
`knowledge-map/KNOWLEDGE_MAP_ARCHITECTURE.md`.
