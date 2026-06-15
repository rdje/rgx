# Task-Tree Tracking Setup Guide (RGX)

This guide explains the repo-local task-tree tracking workflow used by RGX.
RGX adopted it from PGEN (which adopted it from FSMGen); this document is the
portable installation reference, kept here so the workflow is self-describing
and any spin-off can reuse it.

Use this document when a project already has, or wants to add, a roadmap and a
live roadmap-status surface, but also needs a precise way to track task
decomposition over time without losing subtasks, blockers, decisions, or
completion evidence.

## What The Task Tree Is For

The roadmap (RGX's is captured across `ROADMAP.md`,
`RUST_CODEBASE_ANALYSIS.md`, `docs/BACKLOG.md`, and the high-level board
`LIVE_ACHIEVEMENT_STATUS.md`) answers:

- What broad lanes exist?
- Which lane is active?
- What is done, in progress, left, or deferred at the workstream level?

The task tree answers:

- Which exact top-level task is being decomposed?
- Which subtasks and sub-subtasks exist?
- Which leaf is eligible to be picked next?
- What decisions, blockers, and open questions belong to this task?
- What validation and commit evidence closed each executable leaf?

The task tree is therefore a companion to the roadmap, the contracts, the
live-doc surface, and the RGX Book. It does not replace them.

## Files To Add

Minimum required files:

```text
docs/TASK_TREE.md
docs/tasks/TEMPLATE.md
docs/tasks/<FIRST-TREE>.md
```

Project integration files already present in RGX:

```text
README.md
ROADMAP.md
RUST_CODEBASE_ANALYSIS.md
docs/BACKLOG.md
LIVE_ACHIEVEMENT_STATUS.md
COMMIT.md
CLAUDE.md
SESSION_BOOTSTRAP.md
MEMORY.md
CHANGES.md
DEVELOPMENT_NOTES.md
book/src/**
```

## File Roles

| File | Role |
| --- | --- |
| `docs/TASK_TREE_README.md` | Setup guide for installing/understanding this workflow. |
| `docs/TASK_TREE.md` | Local operating spec, active task-tree index, and PNT selection rules. |
| `docs/tasks/TEMPLATE.md` | Copyable skeleton for each new top-level task tree. |
| `docs/tasks/<TREE>.md` | One task tree for one top-level task. |
| `LIVE_ACHIEVEMENT_STATUS.md` | High-level status board; links active roadmap lanes to active task trees. |
| `ROADMAP.md` | Forward-looking lane tracker (`Now`/`Next`/`Later`/`Done`). |
| `RUST_CODEBASE_ANALYSIS.md` | Live roadmap-grounded analysis of the Rust workspace. |
| `docs/BACKLOG.md` | Complete inventory of remaining work. |
| `COMMIT.md` | Commit workflow; requires task-file updates and leaf-ID traceability. |
| `CLAUDE.md` | Non-negotiable AI rules; mirrors the Code-Change Doctrine. |
| `README.md` | Project entry point; links the task-tree docs. |
| `SESSION_BOOTSTRAP.md` | Session startup ritual; tells agents to read active task trees. |
| `MEMORY.md` / `CHANGES.md` / `DEVELOPMENT_NOTES.md` | Recovery and rationale logs; summarize task-tree state changes without duplicating the tree. |
| `book/src/internals/contributing.md` | User/contributor-facing description of the governance model. |

## Recommended Full Setup

Use this for a project where agents need reliable crash recovery, handoff
continuity, and PNT-style execution. (This is the setup RGX adopted.)

1. Add `docs/TASK_TREE_README.md`.
2. Add `docs/TASK_TREE.md`.
3. Add `docs/tasks/TEMPLATE.md`.
4. Create `docs/tasks/<FIRST-TREE>.md` from the template.
5. Add the first tree to the `Active Task Trees` table in `docs/TASK_TREE.md`.
6. Update `README.md`:
   - Add `docs/TASK_TREE_README.md` and `docs/TASK_TREE.md` to the documentation index.
   - Add `docs/TASK_TREE.md` to the fast ramp-up order.
   - Add `docs/tasks/TEMPLATE.md` to the documentation index.
7. Update `SESSION_BOOTSTRAP.md`:
   - Read `README.md`, `COMMIT.md`, `MEMORY.md`.
   - Read `LIVE_ACHIEVEMENT_STATUS.md`.
   - Read `docs/TASK_TREE.md`.
   - Read active task files listed in `docs/TASK_TREE.md`.
   - Pick work from the current frontier when the user asks for PNT.
8. Update `LIVE_ACHIEVEMENT_STATUS.md`:
   - Keep roadmap lanes high-level.
   - For each active lane with task-tree-managed work, link the owning task
     file and name the current frontier leaf.
   - Do not copy the whole task tree into the board.
9. Update `COMMIT.md`:
   - Require task-tree files to be updated when node status, frontier,
     blockers, decisions, validation, or completion evidence changes.
   - Require the commit subject or first body line to include the leaf ID for
     task-tree-managed work.
   - Require one commit per completed leaf before selecting another leaf.
10. Update `CLAUDE.md` to mirror the binding Code-Change Doctrine.
11. Update continuity/history docs:
    - `MEMORY.md`: record the current active tree and frontier for recovery.
    - `CHANGES.md`: log creation of the workflow and any task-tree status
      transition that changes project state.
    - `DEVELOPMENT_NOTES.md`: record rationale and policy decisions.
12. Update the RGX Book (`book/src/internals/contributing.md`) so the
    governance model is documented on the user-facing track too.
13. Commit the setup as one documentation/workflow slice.

## Adapting `docs/TASK_TREE.md`

Keep these sections:

- Purpose
- Code-Change Doctrine
- Active Task Trees
- Directory Layout
- Definitions
- ID Rules
- Status Vocabulary
- Required Task File Sections
- Node Rules
- Current Frontier Rules
- PNT Selection Rules
- Splitting Rules
- Completion Rules
- Blocker Rules
- Relationship To Live Docs
- Leaf ID Commit Convention

Customize these parts:

- Project name.
- Roadmap lane names.
- Live-doc filenames if the project uses different names.
- Commit-message policy. (RGX uses brief commit messages and a transient
  `git_message_brief.txt` buffer; it does **not** use PGEN's
  `PGEN-<FAMILY>-<NNNN>` slice-ID scheme, so in RGX the leaf ID is the
  traceability key in the commit subject or first body line.)
- Any project-specific default rule.

Remove project-specific sections that do not apply.

## Operating Rules

- PNT selects the first eligible leaf from the active tree's current frontier.
- Implement only one leaf at a time.
- Do not implement container nodes.
- If a leaf is too large, split it into child leaves before implementation.
- Keep node IDs stable forever. Do not renumber closed nodes.
- Record blockers with unblock conditions.
- Record decisions where they are made.
- Record validation in the owning task file.
- Update live docs only with summaries and links, not a duplicate of the whole
  task tree.
- Commit every completed leaf before selecting another leaf.

## Completion Evidence

A completed leaf should leave these traces:

- The task node status is `done`.
- The verification log names the checks run.
- The commit log names the commit subject or reference.
- The commit subject or first body line contains the leaf ID.
- Live docs summarize any project-state change.
- The live-status board reflects any active-lane, done, left, or frontier
  change.

Commit hashes do not have to be written into the same task-file update. The
hash is only known after commit. The reliable join key is the leaf ID in the
task file and commit message. Hashes can be backfilled later if wanted.

## What Not To Do

- Do not use the roadmap as the detailed task ledger.
- Do not put broad container tasks in the current frontier.
- Do not create vague children that cannot be verified.
- Do not duplicate the whole task tree into `LIVE_ACHIEVEMENT_STATUS.md`.
- Do not leave completed leaves uncommitted.
- Do not silently continue when a discovered subtask changes the scope; split
  the node and update the frontier.
- Do not renumber nodes after they have been referenced by commits or live
  docs.

## Setup Checklist

```text
[ ] docs/TASK_TREE_README.md exists.
[ ] docs/TASK_TREE.md exists and is customized for the project.
[ ] docs/tasks/TEMPLATE.md exists.
[ ] docs/tasks/<FIRST-TREE>.md exists.
[ ] docs/TASK_TREE.md lists the first active tree.
[ ] README.md links docs/TASK_TREE_README.md and docs/TASK_TREE.md.
[ ] LIVE_ACHIEVEMENT_STATUS.md links active roadmap lane(s) to active task tree(s).
[ ] COMMIT.md requires task-file updates and leaf-ID commit traceability.
[ ] CLAUDE.md mirrors the Code-Change Doctrine.
[ ] SESSION_BOOTSTRAP.md reads docs/TASK_TREE.md and active task files.
[ ] Continuity/history docs summarize the setup.
[ ] The RGX Book documents the governance model.
[ ] The setup is committed as one documentation/workflow slice.
```
