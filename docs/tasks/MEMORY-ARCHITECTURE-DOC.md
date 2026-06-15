# MEMORY-ARCHITECTURE-DOC: Adopt the durable memory-architecture standard

## Metadata

- Tree ID: `MEMORY-ARCHITECTURE-DOC`
- Status: `active`
- Roadmap lane: `Governance / process (memory durability)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Adopt the portable, harness-agnostic **Durable Agent Memory Architecture**
standard (from `specforge/MEMORY_ARCHITECTURE.md`) in RGX: the 4 durability
properties; the 4 lifecycle layers (A resume-pointer · B task-trees · C
decision records · D git history); the write/read paths; and the §9 E1–E4
enforcement that makes non-compliance hard. RGX already has layer B (task-trees,
adopted earlier 2026-06-15); this adds A, C, D-wiring, and the mechanical gates.

## Non-Goals

- Not adopting the separate **Knowledge Map** system (`KNOWLEDGE_MAP_ARCHITECTURE.md`)
  — specforge's pre-commit also runs that; RGX's `.githooks/pre-commit` will run
  only the memory-architecture check (+ RGX's existing gate-receipt guard).
- Not deleting MEMORY.md history — it stays in git; MEMORY.md is demoted to the
  bounded layer-A pointer.
- No engine/parser/VM behavior change.

## Acceptance Criteria

- `MEMORY_ARCHITECTURE.md` present at repo root; README routes to it.
- `docs/decisions/` (layer C) with `INDEX.md` + migrated decision records.
- `MEMORY.md` demoted to the bounded resume pointer (≤ cap); doctrine docs
  (CLAUDE.md/COMMIT.md/SESSION_BOOTSTRAP.md) updated from "append forever" to the
  layered model.
- Enforcement: `scripts/check_memory_architecture.sh` (E2) wired into
  `run-local-ci.sh`/CI (E4); `.githooks/` `pre-commit`+`commit-msg` via
  `core.hooksPath` (E3), composing RGX's existing gate-receipt guard; bootstrap
  pointers `AGENTS.md`/`CLAUDE.md`/`.cursorrules`/`.github/copilot-instructions.md` (E1).
- Full `./scripts/run-local-ci.sh` green (memory-arch check + Rust suite); the
  gates demonstrably bite; live docs synced; tree closed.

## Task Tree

- ID: `MEMORY-ARCHITECTURE-DOC`
  Status: `active`
  Goal: `RGX governed by the durable memory architecture (layers A/B/C/D + E1–E4).`
  Children: `.1` `.2` `.3` `.4` `.5`

- ID: `MEMORY-ARCHITECTURE-DOC.1`
  Status: `done`
  Goal: `Author/copy the project-agnostic MEMORY_ARCHITECTURE.md at repo root + add the README doc-map pointer.`
  Acceptance: `MEMORY_ARCHITECTURE.md present (adapted knobs: line cap, tasks-dir=docs/tasks, commit-workflow-doc=COMMIT.md, leaf-id scheme); README documentation index + ramp-up reference it.`
  Verification: `MEMORY_ARCHITECTURE.md copied verbatim (project-agnostic standard; knobs are adapted in the check script + bootstrap pointers at .4, not in the portable doc); README root-md index + ramp-up order reference it; mdbook unaffected.`
  Commit: `see Commit Log (leaf MEMORY-ARCHITECTURE-DOC.1)`

- ID: `MEMORY-ARCHITECTURE-DOC.2`
  Status: `done`
  Goal: `Create docs/decisions/ (layer C) + INDEX.md; migrate key durable facts out of harness-only ~/.claude auto-memory into tracked ADR-style decision records.`
  Acceptance: `docs/decisions/INDEX.md + >=3 migrated decision records (Context→Decision→Consequences), indexed; cross-linked from relevant task-trees where useful.`
  Verification: `docs/decisions/INDEX.md + 4 ADRs (0001 PGEN sole parser/no workarounds; 0002 subs/pgen read-only + regenerate; 0003 release on hold; 0004 accuracy-first ratchet) — each Context→Decision→Consequences, indexed, marked as superseding the harness-only ~/.claude notes. Cross-linked (0001↔0002, 0003↔0004).`
  Commit: `see Commit Log (leaf MEMORY-ARCHITECTURE-DOC.2)`

- ID: `MEMORY-ARCHITECTURE-DOC.3`
  Status: `pending`
  Goal: `Demote MEMORY.md (4760+ lines) to the bounded layer-A resume pointer (history stays in git); update CLAUDE.md/COMMIT.md/SESSION_BOOTSTRAP.md doctrine from "append-forever/never-delete" to the layered model.`
  Acceptance: `MEMORY.md <= cap, resume-pointer template (current state/frontier/next action); doctrine docs updated + internally consistent; the pre-demotion MEMORY.md remains in git history.`
  Verification: `pending`
  Commit: `pending`

- ID: `MEMORY-ARCHITECTURE-DOC.4`
  Status: `pending`
  Goal: `Install enforcement E1–E4: scripts/check_memory_architecture.sh; .githooks/{pre-commit,commit-msg} (pre-commit composes RGX's gate-receipt guard; commit-msg regex accepts RGX styles) via core.hooksPath; wire the check into run-local-ci.sh + check-ci-paths.sh (CI E4); bootstrap pointers AGENTS.md/.cursorrules/.github/copilot-instructions.md (+ CLAUDE.md pointer line).`
  Acceptance: `check script exits nonzero on a planted breach + zero when clean; commit-msg rejects a bad subject + accepts RGX styles; core.hooksPath set; run-local-ci.sh runs the check first; check-ci-paths.sh registers the new paths. GATE-AFFECTING → run-local-ci.sh green receipt required.`
  Verification: `pending`
  Commit: `pending`

- ID: `MEMORY-ARCHITECTURE-DOC.5`
  Status: `pending`
  Goal: `Verify end-to-end (full run-local-ci.sh green incl. memory-arch check; prove the gates bite) + sync live docs (CHANGES/LIVE_ACHIEVEMENT_STATUS/TASK_TREE) + close the tree.`
  Acceptance: `Full gate green by receipt+banner; demonstrated hook rejection/acceptance; live docs synced; tree → done/Completed.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `MEMORY-ARCHITECTURE-DOC.3` | `pending` | Demote MEMORY.md before installing the check (else the cap check fails on commit). |
| 2 | `MEMORY-ARCHITECTURE-DOC.4` | `pending` | Install enforcement once A/B/C exist and MEMORY.md is within cap. |
| 3 | `MEMORY-ARCHITECTURE-DOC.5` | `pending` | Verify + close. |

(`.1` completed 2026-06-15 — standard authored + README routed. `.2` completed
2026-06-15 — layer C `docs/decisions/` + INDEX + 4 ADRs.)

## Decisions

- `2026-06-15`: Adopt order mirrors specforge (.1 standard → .2 decisions → .3
  demote → .4 enforce → .5 verify). Order is load-bearing: the cap check in `.4`
  fails unless MEMORY.md is demoted in `.3` first.
- `2026-06-15`: `.4` composes RGX's existing gate-receipt `pre-commit` guard
  inside `.githooks/pre-commit` so switching to `core.hooksPath .githooks` does
  not silently disable the COMMIT.md gate-receipt contract.
- `2026-06-15`: `commit-msg` regex knob is widened to accept RGX's commit styles
  (unit-id-first like `TREE.N — …`; typed prefixes `Docs:`/`Book:`; a `(leaf
  TREE.path)` token; `PGEN-RGX-NNNN:`), not just specforge's.
- `2026-06-15`: Knowledge Map is explicitly out of scope (non-goal).

## Open Questions

- `.2`: how many ~/.claude memories to migrate. Plan: migrate the durable
  cross-cutting *project* facts + key doctrines; leave purely-stylistic feedback
  in harness memory (it is also mirrored in CLAUDE.md). Resolve at pick time.
- `.4`: does the full Rust gate build cleanly + quickly enough in this
  environment? If the gate is broken/too-heavy for environmental reasons, that
  is a real blocker for `.4`/`.5` (record it; `.1`–`.3` still land as docs).

## Blockers

- None for `.1`–`.3` (docs-only). `.4`/`.5` need a working `run-local-ci.sh`.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | tree created | `n/a` |
| `2026-06-15` | `.1` | MEMORY_ARCHITECTURE.md present at root (verbatim); README references it; docs-only / non-gate-affecting | `pass` |
| `2026-06-15` | `.2` | docs/decisions/INDEX.md + 4 ADRs present + indexed; docs-only / non-gate-affecting | `pass` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.1` | `MEMORY-ARCHITECTURE-DOC.1 — author standard + README pointer` (`1976563`) | docs-only; not pushed unless user asks. |
| `.2` | `MEMORY-ARCHITECTURE-DOC.2 — layer C docs/decisions + 4 migrated ADRs` | docs-only; not pushed unless user asks. |

## Changelog

- `2026-06-15`: Created from the user directive to adopt the durable
  memory-architecture standard (leaf owns the adoption per the Code-Change
  Doctrine — it touches `scripts/`, `.githooks/`, CI).
