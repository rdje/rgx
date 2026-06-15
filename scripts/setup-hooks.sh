#!/usr/bin/env bash
# One-time per clone: activate the tracked git hooks (the
# gate-receipt pre-commit guard). `core.hooksPath` is local config
# (not committed), so each clone runs this once.
#
#   ./scripts/setup-hooks.sh
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

chmod +x scripts/git-hooks/pre-commit scripts/git-hooks/commit-msg \
  scripts/setup-hooks.sh scripts/lib-gate-receipt.sh \
  scripts/check_memory_architecture.sh \
  knowledge-map/scripts/gen_knowledge_map.sh \
  knowledge-map/scripts/check_knowledge_map.sh 2>/dev/null || true
git config core.hooksPath scripts/git-hooks

echo "[setup-hooks.sh] core.hooksPath -> scripts/git-hooks"
echo "[setup-hooks.sh] pre-commit  = memory-architecture check + gate-receipt guard (active)."
echo "[setup-hooks.sh] commit-msg  = work-unit-id subject convention (active)."
echo "[setup-hooks.sh] Run ./scripts/run-local-ci.sh before committing gate-affecting changes."
