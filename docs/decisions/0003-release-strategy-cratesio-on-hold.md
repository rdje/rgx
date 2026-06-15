# 0003 — crates.io publication is on hold; PGEN regex parser is the eventual vehicle; user is the trigger

- Status: accepted
- Date: 2026-06-15
- Topic: release strategy
- Supersedes harness-only memory: `project_release_strategy`

## Context

Publishing `rgx-core` / `rgx-cli` (and the PGEN regex parser) to crates.io is a
genuine near-future intent, not a "never." It was reopened after the repo went
fully public (2026-05-18), then explicitly deferred by the user. The hard
cargo-publish blocker is that `pgen` is a private-submodule path dependency not
on crates.io; the publication-readiness bar (`docs/PUBLISH_READINESS.md`) is also
not fully met. Compile-time perf (`PGEN-RGX-0078`, formerly 0073) gates the PGEN
parser as a release vehicle.

## Decision

- **crates.io publication is ON HOLD.** Not "never," not "now."
- The **PGEN regex parser is the eventual release vehicle**, gated on PGEN
  compile-time work (`COMPILE-PERF-0078`).
- **The user is the sole trigger.** Publication is outward-facing and
  irreversible; do not publish, tag a release, or push without explicit user
  authorization. Never frame closing items as "release-blockers cleared."

## Consequences

- The `RELEASE-CRATESIO` task-tree is `parked`; its leaves re-activate only on
  user authorization.
- A8 groundwork already shipped (binary rename to `rgx`, per-crate metadata +
  READMEs, Apache-2.0 LICENSE) but does not imply intent to publish now.
- Relates to ADR [0004](0004-accuracy-first-conformance-ratchet-is-the-gate.md)
  (the publication bar is quality-first, not date-driven).
