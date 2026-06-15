# CODEBLOCK-EXPANSION: Inline-language code-block surface maturation

## Metadata

- Tree ID: `CODEBLOCK-EXPANSION`
- Status: `active`
- Roadmap lane: `Next — Embedded code-path expansion / Multi-language code-block runtime expansion`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Mature the embedded code-block surface along the agreed product direction:
first-class inline source-body languages (Lua, JavaScript, Rhai) get the
ergonomics polish; wasm and native stay advanced reference-style backends;
Python/Julia stay deferred. Keep deterministic behavior and the safety/sandbox
guarantees explicit.

## Non-Goals

- Not making wasm the everyday inline UX target.
- Not adding Python/Julia embedding now (explicitly deferred).
- No `ExecutionMode::Pure` code-block support (rejects by design).

## Acceptance Criteria

- The shipped inline-language path (Lua/JS/Rhai) has a consistent source-body
  contract, error reporting, and helper-API parity, with feature-gated tests.
- Any backend-scope decision (native/wasm growth) is explicit and matrix-backed.
- Full gate green per touched features; Book host-integration chapters updated.

## Task Tree

- ID: `CODEBLOCK-EXPANSION`
  Status: `active`
  Goal: `Mature the inline-language code-block surface (Lua/JS/Rhai first).`
  Children: `.1` `.2` `.3` `.4`

- ID: `CODEBLOCK-EXPANSION.1`
  Status: `pending`
  Goal: `Inline-language ergonomics polish: align the Lua/JS/Rhai source-body execution contract shape, error surfacing, and rgx-helper API parity (emit_numeric/emit_replacement/steer_*) where practical; add/expand feature-gated regression coverage.`
  Acceptance: `A concrete ergonomics/parity gap is closed with tests across the relevant lua/javascript/rhai features; full gate green for those features; Book host-integration chapters reflect the behavior.`
  Verification: `pending`
  Commit: `pending`

- ID: `CODEBLOCK-EXPANSION.2`
  Status: `deferred`
  Goal: `Decide whether native registration expands beyond the current Rust-API-only surface and whether wasm grows beyond the file-backed CLI module-registration path.`
  Acceptance: `n/a (deferred).`
  Verification: `Deferred per ROADMAP "Embedded code-path expansion": revisit only after the inline-language story is mature (gated on .1).`
  Commit: `n/a`

- ID: `CODEBLOCK-EXPANSION.3`
  Status: `deferred`
  Goal: `Richer wasm ABI/result work beyond the current numeric/replacement emission slice.`
  Acceptance: `n/a (deferred).`
  Verification: `Deferred: only revisit after the preferred inline-language expansion path is clearer (ROADMAP).`
  Commit: `n/a`

- ID: `CODEBLOCK-EXPANSION.4`
  Status: `deferred`
  Goal: `Heavier embedded runtimes (Python, Julia).`
  Acceptance: `n/a (deferred).`
  Verification: `Explicitly deferred until after the Lua/JS/Rhai ergonomics/safety track (DEVELOPMENT_NOTES strategic-goal clarification).`
  Commit: `n/a`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `CODEBLOCK-EXPANSION.1` | `pending` | The only non-deferred lever; the agreed direction is Lua/JS/Rhai ergonomics before any wasm/native/Python widening. |

## Decisions

- `2026-06-15`: `.2`/`.3`/`.4` deferred per the ROADMAP/DEVELOPMENT_NOTES
  product direction (inline-language first; wasm/native advanced-reference;
  Python/Julia later). Recorded, not dropped.

## Open Questions

- `.1` scope: which specific ergonomics/parity gap is highest-value. The shipped
  surface is already broad (bare-expression + `return` bodies, `emit_*`,
  `steer_*` across all five hosts). Resolve at pick time against the current
  test coverage; if no concrete gap is found, `.1` may close as "surface
  already at parity" with that finding recorded.

## Blockers

- None for `.1`.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | tree created; no leaf executed | `n/a` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| — | `pending` | `pending` |

## Changelog

- `2026-06-15`: Created from ROADMAP "Embedded code-path expansion" +
  "Multi-language code-block runtime expansion" (leaf `TASKTREE-ADOPT.2`).
  Frontier = `.1`; `.2`/`.3`/`.4` deferred with rationale.
