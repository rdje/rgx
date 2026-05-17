#!/usr/bin/env bash
# One-time per clone: activate the tracked git hooks (the
# gate-receipt pre-commit guard). `core.hooksPath` is local config
# (not committed), so each clone runs this once.
#
#   ./scripts/setup-hooks.sh
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

chmod +x scripts/git-hooks/pre-commit scripts/setup-hooks.sh scripts/lib-gate-receipt.sh 2>/dev/null || true
git config core.hooksPath scripts/git-hooks

echo "[setup-hooks.sh] core.hooksPath -> scripts/git-hooks"
echo "[setup-hooks.sh] pre-commit gate-receipt guard is now active."
echo "[setup-hooks.sh] Run ./scripts/run-local-ci.sh before committing gate-affecting changes."
