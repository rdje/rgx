# TASKTREE-ADOPT: Bring RGX under task-tree governance

## Metadata

- Tree ID: `TASKTREE-ADOPT`
- Status: `active`
- Roadmap lane: `Governance / process (cross-cutting)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Adopt the repo-local task-tree tracking workflow (from PGEN/FSMGen) in RGX and
bring the project's work under it: install the workflow files and wire them
into the live docs and the Book (`.1`); decompose the live roadmap's open lanes
into task trees (`.2`); and retroactively audit shipped code changes,
annotating their outcomes back into task trees (`.3`). The end state: **no code
change happens in RGX without first being owned by a task-tree leaf**, and the
roadmap, the codebase, and the mdBook stay locked together with no drift.

## Non-Goals

- This tree does not itself implement any engine/parser/VM/CLI code change. It
  installs governance and plans the decomposition + audit. Concrete code work
  is owned by the trees that `.2`/`.3` produce.
- It does not rewrite or relitigate shipped history in `CHANGES.md`; the audit
  annotates outcomes into trees, it does not edit the historical ledger.
- It does not change the release posture (crates.io stays parked per
  `project_release_strategy`).

## Acceptance Criteria

- The workflow files exist and are internally consistent:
  `docs/TASK_TREE_README.md`, `docs/TASK_TREE.md`, `docs/tasks/TEMPLATE.md`,
  and at least one active tree file (this one).
- The Code-Change Doctrine is mirrored in `CLAUDE.md`, `COMMIT.md`,
  `DEVELOPMENT_NOTES.md`, and the Book.
- `LIVE_ACHIEVEMENT_STATUS.md` exists and links active lanes → active trees.
- `README.md` and `SESSION_BOOTSTRAP.md` route new sessions through the
  task-tree docs.
- Every open `ROADMAP.md` / `docs/BACKLOG.md` lane is either owned by a tree or
  captured in the `Proposed Task Trees` table of `docs/TASK_TREE.md` (`.2`).
- A retroactive-audit pass maps shipped roadmap/backlog work to trees and
  annotates verified outcomes (`.3`).
- Each leaf is committed through `COMMIT.md` with the leaf ID in the commit
  subject or first body line.

## Task Tree

- ID: `TASKTREE-ADOPT`
  Status: `active`
  Goal: `RGX governed by the task-tree workflow; roadmap/codebase/Book locked together.`
  Children: `TASKTREE-ADOPT.1`, `TASKTREE-ADOPT.2`, `TASKTREE-ADOPT.3`

- ID: `TASKTREE-ADOPT.1`
  Status: `done`
  Goal: `Install the task-tree workflow and wire it into RGX's live docs + Book.`
  Acceptance: `TASK_TREE_README.md + TASK_TREE.md + tasks/TEMPLATE.md + this tree exist; Code-Change Doctrine mirrored in CLAUDE.md/COMMIT.md/DEVELOPMENT_NOTES.md/Book; LIVE_ACHIEVEMENT_STATUS.md created; README.md + SESSION_BOOTSTRAP.md updated; mdbook build clean; committed.`
  Verification: `mdbook build book (clean); docs-only (non-gate-affecting) per COMMIT.md/pre-commit hook; markdown links resolve.`
  Commit: `see Commit Log (leaf TASKTREE-ADOPT.1)`

- ID: `TASKTREE-ADOPT.2`
  Status: `done`
  Goal: `Decompose the live ROADMAP / BACKLOG open lanes into task trees: promote the Proposed Task Trees in docs/TASK_TREE.md into real tree files (or revise/retire entries with reason), so every open lane is tree-owned.`
  Acceptance: `Each open roadmap/backlog lane has an owning docs/tasks/<TREE>.md (or a justified Proposed/parked/deferred entry); docs/TASK_TREE.md tables reflect reality; LIVE_ACHIEVEMENT_STATUS.md links lanes → trees.`
  Verification: `7 tree files created (COMPILE-PERF-0073, RUNTIME-REMEASURE, PERF-SOTA-GAPS, PCRE2-1047-SYNTAX, CODEBLOCK-EXPANSION, A9-BINDINGS, RELEASE-CRATESIO); docs/TASK_TREE.md Active/Blocked-Deferred-Parked tables + LIVE_ACHIEVEMENT_STATUS.md board updated; mdbook build clean; docs-only.`
  Commit: `see Commit Log (leaf TASKTREE-ADOPT.2)`

- ID: `TASKTREE-ADOPT.3`
  Status: `pending`
  Goal: `Retroactively audit shipped code changes (roadmap Done / BACKLOG shipped / PGEN-RGX ledger / engine fixes) and annotate verified outcomes back into task trees, per the user doctrine for past changes.`
  Acceptance: `A retro-audit method is recorded; shipped work is mapped to trees with verified outcomes annotated; gaps/drift between roadmap, codebase, and Book are flagged for follow-up trees.`
  Verification: `pending — likely splits per subsystem/era on pick.`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `TASKTREE-ADOPT.3` | `pending` | Retro-audit shipped history into trees now that the forward trees exist. Closes `TASKTREE-ADOPT`. |

(`TASKTREE-ADOPT.2` completed 2026-06-15 — 7 roadmap lanes decomposed into trees.)

## Decisions

- `2026-06-15`: Adopted the PGEN/FSMGen repo-local task-tree workflow in RGX
  (user directive). Installed via `docs/TASK_TREE_README.md`'s "Recommended
  Full Setup".
- `2026-06-15`: RGX has no `PGEN-<FAMILY>-<NNNN>` slice-ID scheme, so the
  **leaf ID** is the commit traceability key (subject or first body line),
  not a separate slice ID.
- `2026-06-15`: Created `LIVE_ACHIEVEMENT_STATUS.md` (RGX previously lacked
  one) as the high-level board that links roadmap lanes → active trees, rather
  than overloading `ROADMAP.md`. This also satisfies the batch-workflow
  instruction that names `LIVE_ACHIEVEMENT_STATUS.md` as a live doc to update.
- `2026-06-15`: `.1` (install) is committed as a single documentation/workflow
  slice — permitted for non-code changes by the Code-Change Doctrine — and is
  the bootstrap that makes the doctrine binding for all future code changes.

## Open Questions

- Granularity of `.2`: one tree per ROADMAP heading, or coarser per lane?
  Resolve at pick time when splitting `.2`. Does not block the frontier.
- Depth of `.3`'s retro-audit: full per-PGEN-RGX-report reconstruction vs a
  subsystem-level mapping. Resolve at pick time. Does not block the frontier.

## Blockers

- None for `.1`/`.2`. `.3` is not blocked but should follow `.2` so the trees
  it annotates into exist first.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | `TASKTREE-ADOPT.1` | `mdbook build book` (clean, exit 0); docs-only / non-gate-affecting per COMMIT.md (no Rust/Cargo/CI/scripts/subs staged); staged set verified to exclude `subs/pgen`; brief cleared + untracked post-commit | `pass` |
| `2026-06-15` | `TASKTREE-ADOPT.2` | 7 tree files created + index tables + board updated; `mdbook build book` clean; docs-only / non-gate-affecting; staged set excludes `subs/pgen` | `pass` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `TASKTREE-ADOPT.1` | `Docs: adopt task-tree tracking workflow (leaf TASKTREE-ADOPT.1)` (`d261366`) | Install + wire-up slice; 13 files; committed local-only, not pushed unless user asks. |
| `TASKTREE-ADOPT.2` | `Docs: decompose roadmap into task trees (leaf TASKTREE-ADOPT.2)` | 7 tree files + index/board; docs-only; not pushed unless user asks. |

## Changelog

- `2026-06-15`: Created task tree; `.1` install slice executed and recorded as
  `done` pending its commit; `.2` (roadmap decomposition) and `.3` (retro
  audit) placed on the frontier.
- `2026-06-15`: `.2` completed — decomposed all 7 open ROADMAP/BACKLOG lanes
  into real task trees (3 active, 2 blocked, 1 deferred, 1 parked); index
  tables + `LIVE_ACHIEVEMENT_STATUS.md` board updated. Frontier → `.3`.
