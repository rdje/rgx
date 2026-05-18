#!/usr/bin/env bash
# Shared gate-receipt identity. Sourced by scripts/run-local-ci.sh
# (writes the receipt on a real green) and scripts/git-hooks/pre-commit
# (refuses to commit a gate-affecting tree with no fresh matching
# receipt). Single source of truth so the two can never disagree.
#
# Why this exists: the 2026-04-07 → 2026-05-18 deep-nesting failure
# was reported "green" for six weeks because the mandatory
# `cargo test -p rgx-core` gate was satisfied with targeted/`--lib`
# runs and pipe-masked exit codes. The receipt makes "commit an
# unvalidated or red gate-affecting tree" impossible without an
# explicit, loud `git commit --no-verify`.

# Receipt path is intentionally inside .git/ (never tracked, never
# part of the identity it certifies).
rgx_receipt_path() {
  printf '%s/rgx-gate-receipt' "$(git rev-parse --git-dir)"
}

# Deterministic identity of the *gate-affecting* content of the
# worktree: HEAD + tracked diff + untracked files, restricted to
# paths that can change `cargo fmt` / `cargo test` / `cargo clippy`
# outcomes. Docs (`*.md`, `book/`, `docs/`, `pgen-issues/`) are
# excluded ON PURPOSE: they cannot change a Rust gate result, and
# COMMIT.md does doc-sync AFTER the gate — hashing them would
# false-invalidate every receipt. `subs/pgen` is excluded (read-only
# from RGX). `git_message_brief.txt` is excluded (ephemeral).
rgx_gate_state_id() {
  local pathspec=(
    '--' '*.rs' '*/Cargo.toml' 'Cargo.toml' 'Cargo.lock'
    '*/build.rs' '.github/workflows/*' 'scripts/*'
    'rgx-capi/cbindgen.toml' 'rgx-capi/include/*'
    ':(exclude)subs/pgen' ':(exclude)git_message_brief.txt'
  )
  #
  # Index-independent by construction: the identity is HEAD plus the
  # *working-tree content* of every gate-affecting path (the union of
  # tracked + untracked names, content hashed from disk via
  # `git hash-object`). `git add` only moves a path between the
  # tracked/untracked name sets — the union is unchanged and the
  # on-disk content is unchanged — so the id is invariant whether a
  # (new or modified) gate file is staged or not. (The earlier
  # diff-HEAD + ls-files-others form was NOT staging-invariant: a new
  # file flipped from the untracked branch to the diff branch on
  # `git add`, changing the id and falsely staling the receipt for
  # every gate-affecting commit that adds a file.)
  {
    git rev-parse HEAD
    {
      git ls-files "${pathspec[@]}"
      git ls-files --others --exclude-standard "${pathspec[@]}"
    } \
      | LC_ALL=C sort -u \
      | while IFS= read -r f; do
          if [ -f "$f" ]; then
            printf '%s ' "$f"
            git hash-object "$f"
          else
            # Tracked gate path deleted in the worktree — a deletion
            # must still invalidate the receipt.
            printf '%s DELETED\n' "$f"
          fi
        done
  } | git hash-object --stdin
}
