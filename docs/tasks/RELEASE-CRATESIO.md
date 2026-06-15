# RELEASE-CRATESIO: crates.io publication readiness

## Metadata

- Tree ID: `RELEASE-CRATESIO`
- Status: `parked`
- Roadmap lane: `Later — Public release preparation (A8)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Reach crates.io publication readiness for `rgx-core` / `rgx-cli` and the PGEN
regex parser, per the publication-readiness bar — without actually publishing
until the user triggers it.

## Non-Goals

- Do NOT publish. Publication is an outward-facing, hard-to-reverse act that
  requires explicit user authorization (`feedback_no_auto_push` spirit +
  `project_release_strategy`).
- Not lowering the publication bar to "release-blockers cleared"
  (`feedback_publish_readiness_bar`).

## Acceptance Criteria

- The full deliverables list in `docs/PUBLISH_READINESS.md` is met (API
  contract stable, Book in sync, no showstopper bugs, ...).
- The `pgen` path-dependency publication strategy is decided and executed.
- A v0.1.0 tag is prepared (not pushed) pending the user trigger.

## Task Tree

- ID: `RELEASE-CRATESIO`
  Status: `parked`
  Goal: `crates.io readiness for rgx-core/rgx-cli + PGEN parser; publish only on user trigger.`
  Children: `.1` `.2` `.3` `.4`

- ID: `RELEASE-CRATESIO.1`
  Status: `parked`
  Goal: `Decide + execute the pgen path-dependency strategy (publish pgen to crates.io / vendor pgen's generated code / make pgen-parser truly optional) — the one hard cargo-publish blocker.`
  Acceptance: `cargo publish --dry-run on rgx-core passes the dependency check; strategy recorded.`
  Verification: `pending`
  Commit: `pending`

- ID: `RELEASE-CRATESIO.2`
  Status: `parked`
  Goal: `API stability sign-off for the public surface.`
  Acceptance: `Public API reviewed + frozen for 0.1.0; api_smoke_test guards it.`
  Verification: `pending`
  Commit: `pending`

- ID: `RELEASE-CRATESIO.3`
  Status: `parked`
  Goal: `Final review against docs/PUBLISH_READINESS.md (book in sync, no showstopper bugs, license, README polish).`
  Acceptance: `All publication-readiness gates closed.`
  Verification: `pending`
  Commit: `pending`

- ID: `RELEASE-CRATESIO.4`
  Status: `parked`
  Goal: `Prepare the v0.1.0 tag (do not push) and the publish runbook.`
  Acceptance: `Tag + runbook ready; publish awaits explicit user authorization.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| — | `RELEASE-CRATESIO.1` | `parked` | Whole tree parked; the user is the trigger. |

## Decisions

- `2026-06-15`: Parked per `project_release_strategy` (crates.io for both
  rgx-core/rgx-cli AND the PGEN regex parser is a near-future intent, ON HOLD;
  user is the trigger). Binary rename (`rgx`), per-crate metadata, READMEs, and
  Apache-2.0 LICENSE already shipped (BACKLOG A8). The PGEN regex parser is the
  eventual release vehicle, gated on PGEN compile-time work (`COMPILE-PERF-0078`).

## Open Questions

- pgen path-dependency strategy (publish vs vendor vs optional) — decided in
  `.1` when the tree reactivates.

## Blockers

- **Blocker:** parked pending user trigger. **Why it blocks:** publication is
  outward-facing and irreversible; the user owns the go/no-go. **Unblock
  condition:** explicit user authorization to pursue release readiness.
  **Run instead:** any active non-parked tree.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | tree created; no leaf executed | `n/a` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| — | `pending` | A8 metadata/rename/LICENSE historical — see CHANGES |

## Changelog

- `2026-06-15`: Created from ROADMAP "Public release preparation" + BACKLOG A8
  (leaf `TASKTREE-ADOPT.2`). Status `parked`; user is the trigger.
