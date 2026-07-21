#!/usr/bin/env bash
# Shared definition of "what counts as a CODE CHANGE" in RGX.
#
# Single source of truth for the doctrine checks that are scoped to code
# changes (`check_code_change_leaf.sh`, `check_two_track_docs.sh`), so the two
# can never disagree about what they govern.
#
# The list mirrors the Code-Change Doctrine as written in `CLAUDE.md`,
# `COMMIT.md`, and `docs/TASK_TREE.md`:
#
#   "Code change" = any edit to Rust sources, `Cargo.toml`/`Cargo.lock`, build
#   scripts, generated artifacts, `.github/workflows/*`, `scripts/*`, or a
#   `subs/pgen` pin bump.
#
# NOTE — deliberately NOT the same list as `lib-gate-receipt.sh`'s
# `rgx_gate_state_id` pathspec. That one answers "can this change alter a
# `cargo fmt`/`test`/`clippy` result?" and therefore EXCLUDES `subs/pgen`
# (read-only from RGX, its content is not RGX's gate). This one answers "is
# this change governed by the Code-Change Doctrine?" and therefore INCLUDES the
# `subs/pgen` gitlink, because a pin bump is a code change that must be owned by
# a task-tree leaf.
#
# PORTABILITY (deliberate): the pathspec is written inline as literal arguments
# rather than built into an array via `mapfile`/`readarray`. Those are bash-4+
# builtins, and stock macOS ships bash 3.2 as `/bin/bash` — where `mapfile`
# fails, leaving an EMPTY pathspec, which makes `git diff --cached -- ` match
# EVERY staged file. That silently widens the doctrine (a docs-only commit would
# be treated as a code change) instead of failing loudly. Keep this function
# free of bash-4-only builtins.

# Staged files (relative to the repo root) that the Code-Change Doctrine
# governs. Empty output ⇒ this is a pure non-code commit (docs, the Book,
# `pgen-issues/`, workflow docs), which the doctrine explicitly exempts.
rgx_staged_code_files() {
  git diff --cached --name-only -- \
    '*.rs' \
    'Cargo.toml' '*/Cargo.toml' 'Cargo.lock' \
    '*/build.rs' \
    '.github/workflows/*' \
    'scripts/*' \
    'knowledge-map/scripts/*' \
    'rgx-capi/cbindgen.toml' 'rgx-capi/include/*' \
    'Makefile' \
    'subs/pgen' \
    ':(exclude)git_message_brief.txt'
}
