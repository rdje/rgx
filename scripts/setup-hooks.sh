#!/usr/bin/env bash
# One-time per clone: activate the tracked git hooks (the
# gate-receipt pre-commit guard). `core.hooksPath` is local config
# (not committed), so each clone runs this once.
#
#   ./scripts/setup-hooks.sh
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

# Belt-and-braces: git already tracks the executable bit (mode 100755) for all
# of these, so a fresh clone gets them runnable. The chmod is here for trees
# where the bit was lost (a zip export, a permissive umask copy, a Windows
# checkout).
chmod +x scripts/git-hooks/pre-commit scripts/git-hooks/commit-msg \
  scripts/setup-hooks.sh scripts/lib-gate-receipt.sh \
  scripts/lib-code-change-scope.sh \
  scripts/check_doctrines.sh \
  scripts/check_memory_architecture.sh \
  scripts/check_code_change_leaf.sh \
  scripts/check_two_track_docs.sh \
  scripts/check_pgen_submodule_readonly.sh \
  scripts/check_gate_receipt.sh \
  scripts/check_doctrine_registry_sync.sh \
  knowledge-map/scripts/gen_knowledge_map.sh \
  knowledge-map/scripts/check_knowledge_map.sh 2>/dev/null || true
git config core.hooksPath scripts/git-hooks

echo "[setup-hooks.sh] core.hooksPath -> scripts/git-hooks"
echo "[setup-hooks.sh] pre-commit  = Knowledge Map derive+stage, then the DOCTRINE DRIVER"
echo "[setup-hooks.sh]               (scripts/check_doctrines.sh --scope hook) — active."
echo "[setup-hooks.sh] commit-msg  = work-unit-id subject convention (active)."
echo "[setup-hooks.sh] Enforced doctrines: see DOCTRINE_ENFORCEMENT.md §10, or run"
echo "[setup-hooks.sh]   ./scripts/check_doctrines.sh"
echo "[setup-hooks.sh] Run ./scripts/run-local-ci.sh before committing gate-affecting changes"
echo "[setup-hooks.sh]   (it stamps the green receipt the GATE-RECEIPT doctrine requires)."
