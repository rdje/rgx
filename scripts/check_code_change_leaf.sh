#!/usr/bin/env bash
# CODE-CHANGE-LEAF doctrine check (structural; commit-time scope).
#
# Doctrine (binding, `CLAUDE.md` / `COMMIT.md` / `docs/TASK_TREE.md`):
#   "No code change may happen unless it is first owned by a task-tree leaf",
#   and: "Never finalize a task-tree leaf without updating its
#   `docs/tasks/<TREE>.md` file."
#
# Mechanization: a commit that stages any code-change-governed path must ALSO
# stage at least one `docs/tasks/<TREE>.md` file — the owning leaf's tree file,
# whose status/verification/commit rows the doctrine requires to move with the
# change. A pure non-code commit (live docs, the Book, `pgen-issues/`, workflow
# docs) is explicitly exempt and passes untouched.
#
# What this does and does NOT prove (be honest — `DOCTRINE_ENFORCEMENT.md` §9):
#   PROVES  — a code change cannot land without a task-tree file moving in the
#             same commit; the tree file is the reviewable unit and the join key.
#   DOES NOT PROVE — that the updated tree is the *right* tree, or that the leaf
#             genuinely covers this change. The `commit-msg` hook's work-unit-id
#             requirement carries the leaf ID for greppability; a human/agent
#             reviewer owns semantic correctness. Strengthening this toward the
#             evidence archetype is `DOCTRINE-ADOPT.2`.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"

# shellcheck source=scripts/lib-code-change-scope.sh
. "$ROOT/scripts/lib-code-change-scope.sh"

staged_code="$(rgx_staged_code_files)"
if [ -z "$staged_code" ]; then
  # Pure non-code commit — the doctrine governs CODE changes.
  exit 0
fi

staged_trees="$(git diff --cached --name-only -- 'docs/tasks/*.md')"
if [ -n "$staged_trees" ]; then
  exit 0
fi

cat >&2 <<EOF
✗ CODE-CHANGE-LEAF: a code change is staged with no owning task-tree file.

The Code-Change Doctrine (CLAUDE.md, COMMIT.md, docs/TASK_TREE.md) is binding:
no code change may land unless a task-tree leaf owns it, and the owning
docs/tasks/<TREE>.md must be updated in the same commit (node status, frontier,
verification, commit log).

Staged code-change paths:
$(printf '  %s\n' $staged_code)

Fix one of:
  - create/extend a tree so a leaf owns this change, update
    docs/tasks/<TREE>.md, and stage it;
  - or, if this is genuinely a pure non-code slice, unstage the code paths
    above (docs / the Book / pgen-issues / workflow docs are exempt).

Put the leaf ID in the commit subject or first body line, e.g.
  "... (leaf <TREE>.<path>)".
Last resort, justified in the commit body: git commit --no-verify
EOF
exit 1
