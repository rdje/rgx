# 0002 — `subs/pgen` is read-only; regenerate `generated/*` after every PGEN bump

- Status: accepted
- Date: 2026-06-15
- Topic: build / PGEN integration
- Supersedes harness-only memory: `feedback_pgen_submodule_readonly`, `project_pgen_generated_files`

## Context

PGEN is vendored as the `subs/pgen` git submodule. Two recurring hazards:

1. RGX tooling run unscoped (`cargo fmt`, `cargo clippy` without `-p`) can touch
   submodule content, polluting the submodule worktree.
2. Since PGEN commit `0ed2b2ad`, PGEN no longer tracks its generated parser
   artifacts (`subs/pgen/generated/*`) — they grew too large to vendor. A fresh
   clone or a submodule pin bump therefore leaves `generated/*` missing, and
   `cargo build -p rgx-core` fails until they are regenerated. A stale
   `target/` binary built against an old parser also produces false verdicts.

## Decision

- **Treat `subs/pgen` as read-only from RGX.** Never modify submodule content.
  Scope all tooling to RGX packages: `cargo fmt -p rgx-core …`, never bare
  `cargo fmt`. Before committing, verify `git -C subs/pgen status` shows only
  untracked (`??`) `generated/*`, never modified (`M`) tracked files.
- **After every `subs/pgen` pin bump (and on fresh clone), regenerate the
  parser** before building: `make -C subs/pgen/rust SHELL=/bin/bash
  regex_parser_bootstrap` (the idempotent cold-clone bootstrap; see README for
  the live command). Then rebuild binaries — do not trust a stale `target/`.
- A submodule **pin bump is a code change** → task-tree-owned, full gate.

## Consequences

- The expected clean working-tree state shows `?? subs/pgen` (regenerated
  untracked `generated/*`); this is benign and must NOT be staged.
- Conformance verdicts are only trusted from a fresh build after a bump
  (cf. the false "still broken" reversal in the PGEN-RGX-0079..0082 episode).
