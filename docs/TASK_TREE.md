# Repo-Local Task Tree Workflow (RGX)

This document defines the repo-local task-tree workflow used by RGX.
For a step-by-step setup guide reusable by another project, read
[docs/TASK_TREE_README.md](TASK_TREE_README.md).

## Purpose

Use a task tree when a top-level task is too broad to finish safely as one
signoff-level slice, or when a task is expected to discover subtasks and
sub-subtasks over time.

The goal is not to create a second roadmap. The roadmap (split across
`ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `docs/BACKLOG.md`, and the
high-level board `LIVE_ACHIEVEMENT_STATUS.md`) states the high-level
workstream direction. A task tree owns the recursive breakdown, current
frontier, acceptance criteria, blockers, decisions, validation, and
completion evidence for one top-level task.

RGX uses brief commit messages (subject + 2–5 line body; gory detail goes to
`CHANGES.md`). RGX does **not** use PGEN's `PGEN-<FAMILY>-<NNNN>` slice-ID
scheme. Therefore, in RGX, the **leaf ID** (e.g. `TASKTREE-ADOPT.2`) is the
unit of commit traceability and must appear in the commit subject or first
body line whenever a commit completes a task-tree leaf.

## Code-Change Doctrine (binding, non-negotiable — adopted 2026-06-15)

**It is strictly forbidden to make any code change unless that change is
first tracked by, or owned by, a task-tree leaf.** This is the standing
doctrine going forward — no compromise, non-negotiable.

- "Code change" means any edit to: Rust sources (`rgx-core/`, `rgx-cli/`,
  `rgx-bench/`, `rgx-capi/`, `rgx-wasm/`, `rgx-bindings/`, `fuzz/`,
  `examples/`), `Cargo.toml` / `Cargo.lock`, build scripts, generated
  artifacts, CI (`.github/workflows/*`), `scripts/*`, or anything that
  alters engine / parser-adapter / compiler / VM / CLI / build behavior.
- The `subs/pgen` submodule is **read-only from RGX** (see
  `feedback_pgen_submodule_readonly`); a submodule **pin bump** is a code
  change and must be owned by a task-tree leaf.
- Before touching code, a task-tree leaf must exist that owns the change
  (create/extend a tree, or add a leaf to an active one). The leaf — its
  goal, acceptance, verification, and commit — is the unit of review. Then
  implement only that leaf and run the full `COMMIT.md` workflow.
- **Rationale (user, 2026-06-15):** task-tree ownership improves code review
  and code quality. The tree's explicit goal/acceptance/verification/blocker
  structure forces the change to be scoped, justified, independently
  verified, and lock-stepped with docs (and the Book) before it lands.
- Pure non-code changes (live docs, the RGX Book, `pgen-issues/` tracker
  files, this workflow doc itself) may still be committed as a single
  documentation slice without a task tree — the doctrine governs **code**
  changes specifically. When in doubt (a change touches both), treat it as a
  code change and require a task-tree leaf.

This doctrine is mirrored in `CLAUDE.md`, `COMMIT.md`, `DEVELOPMENT_NOTES.md`,
the RGX Book (`book/src/internals/contributing.md`), and the auto-memory
(`feedback_task_tree_workflow`).

## Active Task Trees

Trees that are open with at least one eligible (pickable) frontier leaf. PNT
selects the first eligible leaf of the first active tree unless the user names
another tree or `LIVE_ACHIEVEMENT_STATUS.md` names a different immediate lane.

| Tree | File | Status | Current frontier | Roadmap lane |
| --- | --- | --- | --- | --- |
| `KNOWLEDGE-MAP-DOC` | [tasks/KNOWLEDGE-MAP-DOC.md](tasks/KNOWLEDGE-MAP-DOC.md) | `active` | `KNOWLEDGE-MAP-DOC.1` (copy bundle) | Governance / process (fact retrieval) |
| `PERF-SOTA-GAPS` | [tasks/PERF-SOTA-GAPS.md](tasks/PERF-SOTA-GAPS.md) | `active` | `PERF-SOTA-GAPS.1` (inner-literal prefilter) | Next — SOTA algorithmic gaps |
| `PCRE2-1047-SYNTAX` | [tasks/PCRE2-1047-SYNTAX.md](tasks/PCRE2-1047-SYNTAX.md) | `active` | `PCRE2-1047-SYNTAX.1` (A12 capture-return VM) | Next — PCRE2 10.47+ syntax |
| `CODEBLOCK-EXPANSION` | [tasks/CODEBLOCK-EXPANSION.md](tasks/CODEBLOCK-EXPANSION.md) | `active` | `CODEBLOCK-EXPANSION.1` (inline-lang ergonomics) | Next — code-block expansion |

## Blocked / Deferred / Parked Task Trees

Captured and tree-owned, but not PNT-eligible until their condition clears.

| Tree | File | Status | Condition to re-activate |
| --- | --- | --- | --- |
| `COMPILE-PERF-0078` | [tasks/COMPILE-PERF-0078.md](tasks/COMPILE-PERF-0078.md) | `blocked` | PGEN addresses `PGEN-RGX-0078` (sole open bug; replaces `0073`) — faster regex-grammar parser; sole-parser design, no RGX fix |
| `RUNTIME-REMEASURE` | [tasks/RUNTIME-REMEASURE.md](tasks/RUNTIME-REMEASURE.md) | `blocked` | A quiescent machine for a trustworthy benchmark capture (task #57) |
| `A9-BINDINGS` | [tasks/A9-BINDINGS.md](tasks/A9-BINDINGS.md) | `deferred` | A real user/use-case pulling for a specific binding (Phase 0/1 shipped) |
| `RELEASE-CRATESIO` | [tasks/RELEASE-CRATESIO.md](tasks/RELEASE-CRATESIO.md) | `parked` | Explicit user authorization (`project_release_strategy`) |

## Proposed Task Trees

None outstanding. The 7 roadmap-derived proposals were promoted into real tree
files by `TASKTREE-ADOPT.2`, and `LEDGER-HYGIENE` (from `RETRO-AUDIT` drift D1)
was executed + completed on `2026-06-15`. New proposals are added here before
activation.

## Completed Task Trees

| Tree | File | Closed | Outcome |
| --- | --- | --- | --- |
| `TASKTREE-ADOPT` | [tasks/TASKTREE-ADOPT.md](tasks/TASKTREE-ADOPT.md) | `2026-06-15` | Task-tree workflow installed (`.1`), roadmap decomposed into 7 trees (`.2`), shipped code retro-audited (`.3`). |
| `RETRO-AUDIT` | [tasks/RETRO-AUDIT.md](tasks/RETRO-AUDIT.md) | `2026-06-15` | Evidence-based audit of all major shipped subsystems (C1/C2/TDFA/AC, host-integration, A/B API, A9 capi, conformance); all verified present; drift D1 (PGEN-RGX ledger) flagged → `LEDGER-HYGIENE`. |
| `LEDGER-HYGIENE` | [tasks/LEDGER-HYGIENE.md](tasks/LEDGER-HYGIENE.md) | `2026-06-15` | Closed `PGEN-RGX-0073` (superseded by `0078`); confirmed `0078` is the sole top-level open report; corrected D1's loose-grep overcount (only 0073 was actually open, not 15 files). |
| `MEMORY-ARCHITECTURE-DOC` | [tasks/MEMORY-ARCHITECTURE-DOC.md](tasks/MEMORY-ARCHITECTURE-DOC.md) | `2026-06-15` | Adopted the durable memory architecture: standard authored; layer C `docs/decisions/` + 4 ADRs; MEMORY.md demoted 4785→25 lines + doctrine flipped; E1–E4 enforcement installed + active (full run-local-ci.sh green; gates proven to bite). |

## Coverage Note

RGX is a mature codebase: most `ROADMAP.md` lanes are shipped and recorded in
`CHANGES.md` and the Book. The task-tree system was adopted on `2026-06-15`,
after most of that history landed. The retroactive audit of past code changes
(annotating shipped work back into trees) is owned by `TASKTREE-ADOPT.3`.
Until that audit runs, shipped history remains authoritative in `CHANGES.md`
and `RUST_CODEBASE_ANALYSIS.md`; the trees own present and future work.

## Directory Layout

```text
docs/TASK_TREE_README.md
docs/TASK_TREE.md
docs/tasks/
  TEMPLATE.md
  <TREE>.md
```

`docs/TASK_TREE.md` is the workflow and active-tree index.
Each top-level task owns one file in `docs/tasks/`.
`docs/tasks/TEMPLATE.md` is copied when creating a new top-level tree.

## Definitions

- Task tree: the recursive decomposition of one top-level task.
- Node: one item in that tree.
- Container node: a node with children. It is not directly executable.
- Leaf node: a node with no children. It is the only unit PNT may implement.
- Current frontier: the ordered set of leaf nodes that are eligible to be
  picked next.
- Slice: one completed leaf task plus its tests, docs, live-doc updates, Book
  updates, and commit workflow.
- Evidence: the validation output, changed-doc summary, and git commit subject
  that prove a leaf was completed.

## ID Rules

Each task tree has a stable top-level ID.

```text
<TREE>
<TREE>.1
<TREE>.1.1
<TREE>.1.1.1
```

Rules:

- `<TREE>` uses uppercase letters, digits, and hyphens.
- Child IDs append dot-separated positive integers.
- IDs are permanent once published.
- Never renumber closed nodes.
- If a new ordering is needed, add new IDs and mark old nodes `superseded` or
  `deferred` with a reason.
- A commit that completes a task-tree leaf must identify the leaf ID in the
  commit subject or in the first body line.

## Status Vocabulary

Use only these statuses.

| Status | Meaning |
| --- | --- |
| `proposed` | Captured but not yet accepted into the active tree. |
| `active` | The top-level tree is open, or a container has unfinished children. |
| `pending` | Ready to be selected once it reaches the current frontier. |
| `in_progress` | Currently being implemented in the worktree. |
| `blocked` | Cannot proceed without a named blocker and unblock condition. |
| `done` | Completed, validated, documented, and committed. |
| `deferred` | Deliberately postponed with an explicit consequence. |
| `parked` | Captured and intentionally not PNT-eligible; awaits a trigger. |
| `superseded` | Replaced by another node, with the replacement ID named. |

## Required Task File Sections

Every top-level task file must contain:

- Metadata: tree ID, status, roadmap lane, created date, last updated date.
- Goal: the user-visible or project-visible outcome.
- Non-goals: what this tree deliberately does not try to solve.
- Acceptance criteria: concrete conditions that close the top-level task.
- Task tree: all known nodes, with status and short result intent.
- Current frontier: ordered leaf nodes that PNT may select next.
- Decisions: accepted technical decisions and their rationale.
- Open questions: unresolved questions that do not block the whole tree yet.
- Blockers: blockers with unblock conditions.
- Verification log: checks run for completed leaves.
- Commit log: leaf IDs mapped to completion commit subjects.
- Changelog: dated edits to the tree itself.

## Node Rules

Every node must be one of these two shapes.

Container node:

```text
- ID: <TREE>.<n>
  Status: active
  Goal: ...
  Children: <TREE>.<n>.1, <TREE>.<n>.2
```

Leaf node:

```text
- ID: <TREE>.<n>
  Status: pending
  Goal: ...
  Acceptance: ...
  Verification: pending
  Commit: pending
```

A node with children must not be marked `done` until every child is `done`,
`deferred`, or `superseded`, and every non-`done` child has a recorded reason.

## Current Frontier Rules

The current frontier is the only list PNT uses when selecting work from a task
tree.

Rules:

- The frontier contains only leaf nodes.
- The frontier is ordered by intended priority.
- A container never appears in the frontier.
- A blocked node stays out of the frontier until unblocked.
- When a leaf is split, remove that leaf from the frontier, mark it `active`,
  add children, and place the first executable child or children in the
  frontier.
- When a leaf completes, remove it from the frontier and add the next eligible
  leaf or leaves.

## PNT Selection Rules

When PNT is asked to continue and at least one active task tree exists:

1. Read `docs/TASK_TREE.md`.
2. Read the active task file named in the `Active Task Trees` table.
3. Pick the first eligible leaf in that file's `Current Frontier`.
4. Implement only that leaf.
5. If the leaf is too broad, split it before implementation and commit the
   tree update as the leaf's honest outcome.
6. Run the required validation for the leaf (focused checks always; the full
   `./scripts/run-local-ci.sh` gate when the change is gate-affecting, plus
   the PCRE2 conformance ratchet when the change touches parsing / the PGEN
   adapter / the VM / compiler / the conformance harness — see `COMMIT.md`).
7. Update the task file, live docs, the Book, and the roadmap if status
   changed.
8. Run the full commit workflow before selecting another leaf.

If several active trees exist, choose the first active tree in the table unless
the user names another tree or `LIVE_ACHIEVEMENT_STATUS.md` names a different
immediate lane.

Slice-level mechanical work that fits as a single non-code documentation slice
does not have to be promoted into a task tree. Any **code** change must be
owned by a leaf (Code-Change Doctrine).

## Splitting Rules

Split a node when any of these are true:

- It cannot be completed to signoff quality in one slice.
- It mixes design, implementation, diagnostics, tests, and docs in ways that
  can be reviewed independently.
- It hides an unresolved policy choice behind implementation wording.
- It would require touching unrelated ownership areas in one commit.
- It discovers a lower-level dependency that should be solved first.

Do not split merely to create vague placeholders. Every child must have a
clear goal and a way to verify completion.

## Completion Rules

A leaf is complete only when all of the following are true:

- Implementation or documentation work for that leaf is finished.
- Focused checks passed, and broader checks ran when warranted.
- The owning task file records the result, validation, and commit subject.
- `MEMORY.md`, `CHANGES.md`, `DEVELOPMENT_NOTES.md`, and
  `LIVE_ACHIEVEMENT_STATUS.md` are updated when the leaf changes project
  state.
- The RGX Book is updated when the leaf changes anything user-facing.
- The commit workflow in `COMMIT.md` has completed.
- `git_message_brief.txt` has been cleared after commit.

Commit hashes are intentionally not required inside the same task-file update:
the final hash cannot be known until after the commit exists. The stable join
key is the leaf ID in the commit subject or first body line. Later status
refreshes may backfill hashes if useful.

## Blocker Rules

A blocked node must record:

- the exact blocker,
- why it blocks the node,
- the unblock condition,
- and the next task that should run instead, if any.

Do not leave a node as `blocked` only because it is large or unclear. Large or
unclear work should be split until a real blocker is visible.

## Relationship To Live Docs

The task tree is the detailed execution ledger.

- `LIVE_ACHIEVEMENT_STATUS.md` remains the canonical high-level workstream
  status board (links lanes → active trees).
- `ROADMAP.md` remains the forward-looking lane tracker.
- `RUST_CODEBASE_ANALYSIS.md` remains the roadmap-grounded workspace analysis.
- `docs/BACKLOG.md` remains the complete inventory of remaining work.
- `MEMORY.md` remains the recovery/handoff continuity log.
- `CHANGES.md` remains the chronological technical history.
- `DEVELOPMENT_NOTES.md` remains design rationale.
- The RGX Book under `book/src/` remains the user-facing reference.

Do not duplicate the whole task tree into those files. Link to the task tree
and summarize only the part that changes live project state.

## Leaf ID Commit Convention

Commits associated with task-tree leaves follow this form:

```text
<short subject> (leaf <TREE>.<path>)

<2–5 line body explaining the why; validation summary goes here or in CHANGES.md>
```

The leaf ID is the join key between the task tree and the commit. Keep the
subject brief (≤70 chars) per `COMMIT.md`; the leaf ID may sit in the subject
or the first body line.
