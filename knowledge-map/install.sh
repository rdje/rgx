#!/usr/bin/env bash
# install.sh — one-shot adoption helper for the Knowledge Map (KM) bundle.
#
# Idempotent. Run from any repo that has copied the knowledge-map/ bundle into its root:
#   bash knowledge-map/install.sh
#
# It: creates the fact dir, generates the first map, and PRINTS the hook + CI wiring for you
# to apply (it does NOT silently rewrite your .githooks/pre-commit or CI — those are yours).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

[ -f "$ROOT/.knowledge_map.conf" ] && . "$ROOT/.knowledge_map.conf"
[ -f "$SCRIPT_DIR/scripts/knowledge_map.conf" ] && . "$SCRIPT_DIR/scripts/knowledge_map.conf"
: "${KM_SCAN_DIRS:=docs/knowledge docs/decisions}"

chmod +x "$SCRIPT_DIR/scripts/gen_knowledge_map.sh" "$SCRIPT_DIR/scripts/check_knowledge_map.sh" 2>/dev/null || true

# 1) ensure at least the first scan dir exists, with a pointer to the template
first_dir="${KM_SCAN_DIRS%% *}"
if [ ! -d "$first_dir" ]; then
  mkdir -p "$first_dir"
  printf 'Fact files live here. Copy knowledge-map/templates/FACT_TEMPLATE.md and rename it <id>.md.\n' \
    > "$first_dir/README.md"
  printf 'install: created %s/\n' "$first_dir"
fi

# 2) generate the first map
bash "$SCRIPT_DIR/scripts/gen_knowledge_map.sh"

# 3) validate
bash "$SCRIPT_DIR/scripts/check_knowledge_map.sh" || true

# 4) print the wiring for the user to apply
cat <<EOF

================================ KM adoption — next steps ================================
Hooks are local; CI is the backstop. Apply both:

1) Activate hooks (one-time per clone):
     git config core.hooksPath .githooks

2) Add the KM gate to .githooks/pre-commit (see knowledge-map/hooks/pre-commit.snippet):
$(sed 's/^/     /' "$SCRIPT_DIR/hooks/pre-commit.snippet")

3) Add the CI gate (knowledge-map/ci/knowledge-map-gate.yml) to your pipeline, or run:
     bash knowledge-map/scripts/check_knowledge_map.sh

4) Write your first facts: copy knowledge-map/templates/FACT_TEMPLATE.md into $first_dir/.
   Read knowledge-map/FAQ.md and knowledge-map/KNOWLEDGE_MAP_ARCHITECTURE.md first.
=========================================================================================
EOF
