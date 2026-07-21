# 0006 — the Doctrine Enforcement Architecture is adopted; every mechanizable doctrine gets a check

- Status: accepted
- Date: 2026-07-21
- Topic: governance / enforcement
- Origin: devised by the sibling **PGEN** project (`pgen/DOCTRINE_ENFORCEMENT.md`)
- Directive: director, 2026-07-21 — "adopt that new doctrine enforcement"

## Context

RGX already ran three portable architectures: task-trees (per-unit work memory),
the memory architecture (durable 4-layer agent memory), and the Knowledge Map
(retrieval over fact cards). Its *doctrines*, though — the Code-Change Doctrine,
the two-track documentation gate, `subs/pgen` read-only, the mandatory-gate
rule — lived as prose in `CLAUDE.md` / `COMMIT.md` / `docs/TASK_TREE.md`, backed
by an ad-hoc stack of checks hard-coded inside the pre-commit hook. Prose is
discoverable but not enforceable: compliance was, for several doctrines, a
"trust me" claim, and the 2026-04-07 → 2026-05-18 six-week false-green gate
incident is the standing proof that a rule nothing re-derives will erode.

PGEN generalized the same E1→E4 defense-in-depth that `MEMORY_ARCHITECTURE.md`
applies to *memory* into a standard that applies it to **every** doctrine.

## Decision

**Adopt it.** `DOCTRINE_ENFORCEMENT.md` is now an RGX standard (§10 = RGX's own
registry). Concretely:

- **doctrine = a rule + a deterministic check that exits nonzero on breach.**
  Every mechanizable RGX doctrine gets a `scripts/check_<id>.sh` obeying the
  standard's §4 contract.
- **One registry + driver** (`scripts/check_doctrines.sh`) owns the list, runs
  every check, reports per-doctrine PASS/FAIL/SKIP, and meta-checks that each
  registered enforcer exists.
- **Both gates run the driver**: `scripts/git-hooks/pre-commit` (E3) and
  `scripts/run-local-ci.sh` (E4, which hosted CI executes).
- **Adding a doctrine** = write the check + add one registry line + add one §10
  row. The `DOCTRINE-REGISTRY-SYNC` check enforces that last pair mechanically.
- RGX extends the standard with a per-entry **scope** (`always` / `hook`):
  doctrines whose semantics exist only at commit time report `SKIP` in CI rather
  than passing vacuously. An honest SKIP beats a fake green.

## Consequences

- A code change cannot land without: an owning task-tree file in the same
  commit, a `CHANGES.md` entry, a clean `subs/pgen`, and a fresh green
  `run-local-ci.sh` receipt for exactly that content — all mechanically, at
  every active gate.
- The pre-commit hook is no longer where enforcement logic accretes; it derives
  the Knowledge Map, then delegates. Doctrine changes touch the registry, not
  the hook.
- **Honest limits (the standard's §9, restated so they are not over-claimed):**
  local hooks are bypassable (`--no-verify`, unset `hooksPath`) — CI is the real
  backstop and is only as strong as its next run; structural checks prove
  artifacts moved, not that they are semantically right; the Book leg of the
  two-track doctrine stays advisory because user-visibility is a judgement.
  The goal is non-compliance that is *expensive and visible*, not literally
  impossible.
- Queued: the evidence archetype (`DOCTRINE-ADOPT.2`) — requiring a code
  change's leaf to carry a tool-backed WHY+WHERE diagnosis plus a measured
  before→after, with the cited oracle re-run rather than merely grepped.
