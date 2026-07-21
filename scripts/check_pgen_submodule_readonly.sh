#!/usr/bin/env bash
# PGEN-READONLY doctrine check (structural; runs everywhere).
#
# Doctrine (`docs/decisions/0002-pgen-submodule-readonly-regenerate.md`,
# `CLAUDE.md`, the `feedback_pgen_submodule_readonly` rule): `subs/pgen` is
# READ-ONLY from RGX. Parser defects are fixed upstream in PGEN and adopted via
# a pin bump — never by editing the submodule's tracked content here (which
# would silently fork the parser and make the pin a lie).
#
# The sanctioned exception is the regenerated, UNTRACKED `generated/` tree
# (`make -C subs/pgen/rust regex_parser_bootstrap`), which PGEN stopped
# tracking at commit 0ed2b2ad. So the invariant is exactly:
#
#   `git -C subs/pgen status --porcelain` shows ONLY untracked (`??`) entries —
#   never ` M`/`M `/`A`/`D`/`R` on tracked content.
#
# This is the mechanized form of the long-standing pre-commit habit "verify
# subs/pgen shows only `?`, not `M`". A pin bump itself (the superproject's
# gitlink moving) is NOT a breach — that is the adoption path, governed by
# CODE-CHANGE-LEAF like any other code change.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"

sub="$ROOT/subs/pgen"

if [ ! -e "$sub/.git" ]; then
  # Submodule not initialized (fresh clone / docs-only CI job). Nothing to
  # verify, and absence is not a breach — `run-local-ci.sh` has its own
  # "missing submodule" handling for the build steps.
  printf 'PGEN-READONLY: subs/pgen not initialized — nothing to check.\n' >&2
  exit 0
fi

# --porcelain=v1: XY <path>. Untracked is exactly "?? ". Anything else on a
# tracked path is a modification of read-only content.
dirty="$(git -C "$sub" status --porcelain=v1 2>/dev/null | grep -v '^?? ' || true)"

if [ -z "$dirty" ]; then
  exit 0
fi

cat >&2 <<EOF
✗ PGEN-READONLY: subs/pgen has modified TRACKED content.

subs/pgen is read-only from RGX (docs/decisions/0002). Parser fixes land
upstream in PGEN and arrive here as a pin bump; editing the submodule's tracked
files forks the parser and makes the recorded pin a lie. Only the regenerated,
UNTRACKED generated/ tree may differ.

Offending entries (git -C subs/pgen status --porcelain):
$(printf '%s\n' "$dirty" | sed 's/^/  /')

Fix:
  git -C subs/pgen checkout -- <paths>     # discard the local edits
  # or, if the change is genuinely needed: make it upstream in PGEN, release it,
  # then bump the pin here under an owning task-tree leaf.
Last resort, justified in the commit body: git commit --no-verify
EOF
exit 1
