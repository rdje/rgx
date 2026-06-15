# A9-BINDINGS: Language bindings Phases 2–7 over rgx-capi

## Metadata

- Tree ID: `A9-BINDINGS`
- Status: `deferred`
- Roadmap lane: `Later — Binding/runtime expansion (A9)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Complete the language-bindings programme on top of the shipped `rgx-capi` C ABI
(Phase 0 design + Phase 1 scaffolding/basic-matching landed 2026-05-13):
captures+iterators, safety limits+replace, `tail_file`, observers, embedded
scripting pass-through, and per-language wrappers.

## Non-Goals

- Per-language idiomatic wrappers are SEPARATE projects shipped on demand; this
  tree owns the C ABI foundation phases, not the wrapper crates.
- crates.io publication (owned by `RELEASE-CRATESIO`).

## Acceptance Criteria

- Each phase extends `rgx-capi` with cbindgen-generated header parity (the
  header-drift CI gate in `scripts/check-capi-abi.sh`), Rust-side unit tests, a
  C-side smoke test, and the ABI-stability discipline from
  `docs/A9_LANGUAGE_BINDINGS_DESIGN.md`.
- Full gate green per phase.

## Task Tree

- ID: `A9-BINDINGS`
  Status: `deferred`
  Goal: `Complete A9 Phases 2–7 over rgx-capi.`
  Children: `.1` `.2` `.3` `.4` `.5` `.6`

- ID: `A9-BINDINGS.1`
  Status: `deferred`
  Goal: `Phase 2 — captures + iterators across the C ABI.`
  Acceptance: `C ABI exposes captures + iteration; header regenerates byte-identical; tests + smoke; gate green.`
  Verification: `pending`
  Commit: `pending`

- ID: `A9-BINDINGS.2`
  Status: `deferred`
  Goal: `Phase 3 — safety limits + replace across the C ABI.`
  Acceptance: `set_max_* + replace exposed; tests + smoke; gate green.`
  Verification: `pending`
  Commit: `pending`

- ID: `A9-BINDINGS.3`
  Status: `deferred`
  Goal: `Phase 4 — tail_file across the C ABI.`
  Acceptance: `tail_file exposed with a C-friendly callback model; tests + smoke; gate green.`
  Verification: `pending`
  Commit: `pending`

- ID: `A9-BINDINGS.4`
  Status: `deferred`
  Goal: `Phase 5 — observers (structured events) across the C ABI.`
  Acceptance: `event observer exposed; tests + smoke; gate green.`
  Verification: `pending`
  Commit: `pending`

- ID: `A9-BINDINGS.5`
  Status: `deferred`
  Goal: `Phase 6 — embedded scripting pass-through across the C ABI.`
  Acceptance: `code-block registration/execution reachable from C where feasible; tests + smoke; gate green.`
  Verification: `pending`
  Commit: `pending`

- ID: `A9-BINDINGS.6`
  Status: `deferred`
  Goal: `Phase 7 — per-language wrappers (priority Go → Python → Julia → Zig → Ruby/PHP), as separate projects.`
  Acceptance: `n/a until a wrapper is pulled by demand.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| — | `A9-BINDINGS.1` | `deferred` | Whole tree deferred — no demand signal. |

## Decisions

- `2026-06-15`: Deferred per ROADMAP. The generic "10× user base" argument
  doesn't fit RGX: its differentiator is host integration, the hardest surface
  to translate across FFI. Reactivate on a real user/use-case pull; if so, **C
  bindings via cbindgen first** (already the Phase-1 foundation), not Python.

## Open Questions

- Which binding gets the first demand signal? Unknown until a user asks.

## Blockers

- **Blocker:** no demand signal. **Why it blocks:** speculative binding work
  competes with engine work that strengthens the actual differentiator.
  **Unblock condition:** a real user/use-case pulling for a specific binding.
  **Run instead:** any active non-deferred tree.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | tree created; Phase 0/1 noted shipped 2026-05-13 | `n/a` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| — | `pending` | Phase 0/1 historical — see CHANGES 2026-05-13 |

## Changelog

- `2026-06-15`: Created from ROADMAP "Binding/runtime expansion (A9)" + BACKLOG
  A9 (leaf `TASKTREE-ADOPT.2`). Phase 0/1 shipped; Phases 2–7 deferred.
