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
  {
    git rev-parse HEAD
    # Tracked modifications (staged + unstaged) to gate-affecting paths.
    git -c core.autocrlf=false diff HEAD "${pathspec[@]}"
    # New untracked gate-affecting files: name + content hash, sorted
    # for determinism.
    git ls-files --others --exclude-standard "${pathspec[@]}" \
      | LC_ALL=C sort \
      | while IFS= read -r f; do
          [ -f "$f" ] || continue
          printf '%s ' "$f"
          git hash-object "$f"
        done
  } | git hash-object --stdin
}
