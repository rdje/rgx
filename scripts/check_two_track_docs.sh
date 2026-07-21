#!/usr/bin/env bash
# TWO-TRACK-DOCS doctrine check (structural; commit-time scope).
#
# Doctrine (`CLAUDE.md` "Two separate documentation tracks", `COMMIT.md` step 3 —
# a HARD GATE): every shipped feature/fix updates BOTH tracks —
#   Track A (the world)   : `book/src/**` — a chapter/section for any user-visible change.
#   Track B (continuity)  : `CHANGES.md` + the owning task tree + `MEMORY.md` + friends.
#
# Mechanization, and the honest split (`DOCTRINE_ENFORCEMENT.md` §6.1: a box with
# no re-runnable oracle stays advisory, never hard-gated):
#   HARD-GATED — `CHANGES.md`. `COMMIT.md` requires "a new entry for every shipped
#     feature/fix" UNCONDITIONALLY, so "code staged ⇒ CHANGES.md staged" is a real,
#     deterministic invariant.
#   ADVISORY   — the Book. Whether a change is *user-visible* is a judgement a
#     structural check cannot make without false positives (an internal refactor
#     legitimately touches no chapter). The check therefore REMINDS about track A
#     and fails only on the track-B leg it can actually prove.
#
# The owning-task-tree leg of track B is enforced separately by CODE-CHANGE-LEAF.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"

# shellcheck source=scripts/lib-code-change-scope.sh
. "$ROOT/scripts/lib-code-change-scope.sh"

staged_code="$(rgx_staged_code_files)"
if [ -z "$staged_code" ]; then
  exit 0
fi

if [ -n "$(git diff --cached --name-only -- 'CHANGES.md')" ]; then
  # Track B's provable leg holds. Nudge about track A without failing.
  if [ -z "$(git diff --cached --name-only -- 'book/src/*')" ]; then
    printf 'TWO-TRACK-DOCS: note — code staged with no book/src/** change. If this alters user-visible behaviour, the Book needs a section (CLAUDE.md track A).\n' >&2
  fi
  exit 0
fi

cat >&2 <<EOF
✗ TWO-TRACK-DOCS: a code change is staged with no CHANGES.md entry.

COMMIT.md step 3 is a hard gate: every shipped feature/fix gets a new CHANGES.md
entry (what changed, why, validation, impact). It is the chronological ledger a
future session reads when git alone is too coarse.

Staged code-change paths:
$(printf '  %s\n' $staged_code)

Fix: add the entry at the TOP of CHANGES.md (newest first) and stage it.
Also check track A while you are here: does book/src/** need a section?
(CLAUDE.md — the Book is the public face; updating live docs does NOT satisfy it.)
Last resort, justified in the commit body: git commit --no-verify
EOF
exit 1
