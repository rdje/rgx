# <TREE-ID>: <Task Title>

## Metadata

- Tree ID: `<TREE-ID>`
- Status: `proposed`
- Roadmap lane: `<roadmap lane name>`
- Created: `YYYY-MM-DD`
- Last updated: `YYYY-MM-DD`
- Owner: repo-local workflow

## Goal

State the exact outcome this top-level task must deliver.

## Non-Goals

- State what this task deliberately does not solve.

## Acceptance Criteria

- The behavior, documentation, or infrastructure outcome is implemented.
- Focused validation passes.
- Broader validation runs when the blast radius warrants it (the full
  `./scripts/run-local-ci.sh` gate when gate-affecting; the PCRE2 conformance
  ratchet when parsing / PGEN adapter / VM / compiler / harness is touched).
- Live docs, the roadmap board, and the RGX Book are updated where project
  state changed.
- Each completed leaf is committed through `COMMIT.md`.

## Task Tree

- ID: `<TREE-ID>`
  Status: `active`
  Goal: `<top-level goal>`
  Children: `<TREE-ID>.1`

- ID: `<TREE-ID>.1`
  Status: `pending`
  Goal: `<first executable leaf>`
  Acceptance: `<what proves this leaf is done>`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `<TREE-ID>.1` | `pending` | `<reason>` |

## Decisions

- `YYYY-MM-DD`: `<decision and rationale>`

## Open Questions

- `<question, owner, and why it does or does not block the frontier>`

## Blockers

- None.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `YYYY-MM-DD` | `<TREE-ID>.1` | `pending` | `pending` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `<TREE-ID>.1` | `pending` | `pending` |

## Changelog

- `YYYY-MM-DD`: Created task tree.
