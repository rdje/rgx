# KNOWLEDGE-MAP-DOC: Adopt the Knowledge Map retrieval layer

## Metadata

- Tree ID: `KNOWLEDGE-MAP-DOC`
- Status: `active`
- Roadmap lane: `Governance / process (fact retrieval)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Adopt the portable Knowledge Map (KM) bundle (from `pgen/knowledge-map/`) in RGX
so that any fact logged once is **findable in one lookup** — eliminating
archaeology (re-deriving an already-established structural/causal fact from code
or runtime). The KM is **additive**: it indexes nothing destructively, converts
nothing, and composes with the task-trees + `MEMORY_ARCHITECTURE.md` already in
place. `KNOWLEDGE_MAP.md` is a **derived** artifact (question-keyed index) over
`answers:`-front-mattered fact cards under `docs/knowledge/` (+ optionally
`docs/decisions/`), kept in sync by a derive-and-diff gate.

## Non-Goals

- No migration sweep — cards are written **lazily**, one per durable fact
  established or archaeology caught. Not converting the book/prose/task-trees.
- Not carding volatile metrics (card the durable conclusion, not the reading).
- No engine/parser/VM behavior change.

## Acceptance Criteria

- `knowledge-map/` bundle present at repo root; `KNOWLEDGE_MAP.md` generated
  (derived, deterministic); `docs/knowledge/` exists.
- A handful of seed fact cards for durable facts established this session;
  `KNOWLEDGE_MAP.md` regenerated from them and in sync.
- Enforcement wired: the KM gen+stage+check folded into
  `scripts/git-hooks/pre-commit` (composing the existing memory-arch + gate-receipt
  steps); `check_knowledge_map.sh` run by `run-local-ci.sh` (CI/E4);
  `check-ci-paths.sh` registers the new paths; README + bootstrap pointers route to it.
- Full `./scripts/run-local-ci.sh` green; derive-and-diff gate demonstrably bites;
  live docs synced; tree closed.

## Task Tree

- ID: `KNOWLEDGE-MAP-DOC`
  Status: `active`
  Goal: `RGX has the KM retrieval layer (derived map + fact cards + derive-and-diff enforcement).`
  Children: `.1` `.2` `.3` `.4`

- ID: `KNOWLEDGE-MAP-DOC.1`
  Status: `done`
  Goal: `Copy the knowledge-map/ bundle into RGX root; run install (create docs/knowledge/, generate the first KNOWLEDGE_MAP.md); add README doc-map pointer + AGENTS.md "grep the map before re-deriving" read-path line.`
  Acceptance: `knowledge-map/ present; KNOWLEDGE_MAP.md generated (0 facts initially OK); docs/knowledge/ created; README + AGENTS reference the KM; bundle scripts confirmed NOT to trip the top-level scripts/* gate pathspec (else fold into .3).`
  Verification: `cp -r bundle → knowledge-map/ (10 files); bash knowledge-map/install.sh → docs/knowledge/ created + KNOWLEDGE_MAP.md generated (0 facts, check OK). Empirically confirmed knowledge-map/scripts/* does NOT match the gate pathspec scripts/* (git anchors at top-level), so .1/.2 are docs-only / non-gate-affecting. README root-md index + AGENTS read-path updated. Committed through the active hooks (memory-arch ok; no gate-affecting staged → gate-receipt skipped).`
  Commit: `see Commit Log (leaf KNOWLEDGE-MAP-DOC.1)`

- ID: `KNOWLEDGE-MAP-DOC.2`
  Status: `pending`
  Goal: `Seed initial fact cards under docs/knowledge/ for durable structural facts established this session (e.g. conformance-ratchet location/value; sole-open-PGEN-bug; subs/pgen regenerate-after-bump; the two governance standards' entrypoints). Regenerate the map.`
  Acceptance: `>=3 valid fact cards (required fields + answers:); KNOWLEDGE_MAP.md regenerated + in sync; check_knowledge_map.sh green.`
  Verification: `pending`
  Commit: `pending`

- ID: `KNOWLEDGE-MAP-DOC.3`
  Status: `pending`
  Goal: `Wire enforcement: fold the KM gen+stage+check into scripts/git-hooks/pre-commit (after the memory-arch check, before/with the gate-receipt guard); add check_knowledge_map.sh as a run-local-ci.sh step (E4); register the new required paths in check-ci-paths.sh; setup-hooks.sh chmods the KM scripts.`
  Acceptance: `pre-commit runs the KM gate; run-local-ci.sh runs the KM check; check-ci-paths green with new paths. GATE-AFFECTING → run-local-ci.sh green receipt required.`
  Verification: `pending`
  Commit: `pending`

- ID: `KNOWLEDGE-MAP-DOC.4`
  Status: `pending`
  Goal: `Verify end-to-end (full run-local-ci.sh green; derive-and-diff gate bites on a planted edit) + sync live docs (CHANGES/LIVE_ACHIEVEMENT_STATUS/TASK_TREE/MEMORY) + close the tree.`
  Acceptance: `Full gate green; demonstrated KM gate rejection of a hand-edited/out-of-sync map; live docs synced; tree → done/Completed.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `KNOWLEDGE-MAP-DOC.2` | `pending` | Seed cards now the bundle + map exist. |
| 2 | `KNOWLEDGE-MAP-DOC.3` | `pending` | Wire enforcement once cards exist (so the gate runs against real facts). GATE-AFFECTING. |
| 3 | `KNOWLEDGE-MAP-DOC.4` | `pending` | Verify + close. |

(`.1` completed 2026-06-15 — bundle copied + installed; first map generated.)

## Decisions

- `2026-06-15`: Copy the bundle verbatim (`cp -r`) — it is project-agnostic and
  meant to be copied whole. RGX-specific wiring lives in `.3` (hooks + CI),
  not in the bundle.
- `2026-06-15`: Fold the KM gate into RGX's existing `scripts/git-hooks/pre-commit`
  (already `core.hooksPath`) rather than a separate `.githooks/`, composing with
  the memory-arch check + gate-receipt guard (same adaptation as MEMORY-ARCHITECTURE-DOC).
- `2026-06-15`: KM scans `docs/knowledge/` (new) + `docs/decisions/` (existing
  layer C). The current ADRs have no `answers:` front-matter, so they are not
  indexed unless/until folded in — no duplication.

## Open Questions

- Whether to fold any `docs/decisions/` ADR into the map via `answers:`
  front-matter. Default: no (keep ADRs as-is); seed `docs/knowledge/` cards
  instead. Revisit lazily.

## Blockers

- None for `.1`/`.2`. `.3`/`.4` need a working `./scripts/run-local-ci.sh`.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | tree created | `n/a` |
| `2026-06-15` | `.1` | bundle copied; install ok (0-fact map, check OK); pathspec test = not gate-affecting; README+AGENTS routed; docs-only | `pass` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.1` | `KNOWLEDGE-MAP-DOC.1 — copy KM bundle + install + route docs` | docs-only; not pushed unless user asks. |

## Changelog

- `2026-06-15`: Created from the user directive to adopt the Knowledge Map
  architecture (leaf owns it per the Code-Change Doctrine — it touches
  `knowledge-map/scripts/`, `scripts/`, hooks, CI).
