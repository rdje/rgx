#!/usr/bin/env bash
#
# check_memory_architecture.sh — the single source of truth for the
# memory-architecture invariants (layer E2 of MEMORY_ARCHITECTURE.md). Exits
# NONZERO on any breach. Called by the git pre-commit hook
# (scripts/git-hooks/pre-commit) and by CI (scripts/run-local-ci.sh).
#
# Adapt only the knobs below to port to another repo. (RGX adaptations vs the
# portable §9.1 kit: hooks live in scripts/git-hooks/ — already RGX's
# core.hooksPath — not .githooks/; tasks-dir is docs/tasks; commit-workflow doc
# is COMMIT.md.)
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

# -- Knobs (everything else is project-neutral) ------------------------------
MEMORY_POINTER_LINE_CAP="${MEMORY_POINTER_LINE_CAP:-60}"   # layer-A resume-pointer cap
TASKS_DIR="docs/tasks"                                      # task-trees (layer B)
DECISIONS_DIR="docs/decisions"                              # decision records (layer C)
BOOTSTRAP_FILES=("AGENTS.md" "CLAUDE.md")                   # tool-neutral entrypoints (E1)

fail=0
note() { printf '[memory-arch] FAIL: %s\n' "$1" >&2; fail=1; }
ok()   { printf '[memory-arch] ok:   %s\n' "$1"; }

# 1) The standard itself must be present (it documents the whole system).
if [[ -f MEMORY_ARCHITECTURE.md ]]; then ok "MEMORY_ARCHITECTURE.md present"
else note "MEMORY_ARCHITECTURE.md is missing (the memory standard)"; fi

# 2) MEMORY.md must exist and stay a bounded resume pointer (layer A).
if [[ -f MEMORY.md ]]; then
  lines="$(wc -l < MEMORY.md | tr -d ' ')"
  if [[ "${lines}" -le "${MEMORY_POINTER_LINE_CAP}" ]]; then
    ok "MEMORY.md is ${lines} lines (<= cap ${MEMORY_POINTER_LINE_CAP})"
  else
    note "MEMORY.md is ${lines} lines (> cap ${MEMORY_POINTER_LINE_CAP}) — it is the resume pointer; move content to ${TASKS_DIR}/ or ${DECISIONS_DIR}/"
  fi
else
  note "MEMORY.md (the resume pointer) is missing"
fi

# 3) Tool-neutral bootstrap pointers must exist and route to the standard (E1).
for f in "${BOOTSTRAP_FILES[@]}"; do
  if [[ -f "${f}" ]]; then
    if grep -q "MEMORY_ARCHITECTURE.md" "${f}"; then ok "${f} points at MEMORY_ARCHITECTURE.md"
    else note "${f} exists but does not point at MEMORY_ARCHITECTURE.md"; fi
  else
    note "${f} bootstrap pointer is missing"
  fi
done

# 4) Layer-C decision store must exist with an index.
if [[ -d "${DECISIONS_DIR}" && -f "${DECISIONS_DIR}/INDEX.md" ]]; then
  ok "${DECISIONS_DIR}/ + INDEX.md present"
else
  note "${DECISIONS_DIR}/INDEX.md is missing (layer C — durable decision records)"
fi

# 5) Layer-B task-trees must exist with an index.
if [[ -d "${TASKS_DIR}" && -f "docs/TASK_TREE.md" ]]; then
  ok "${TASKS_DIR}/ + docs/TASK_TREE.md present"
else
  note "${TASKS_DIR}/ or docs/TASK_TREE.md is missing (layer B — task-trees)"
fi

if [[ "${fail}" -ne 0 ]]; then
  printf '[memory-arch] memory-architecture check FAILED — see MEMORY_ARCHITECTURE.md\n' >&2
  exit 1
fi
printf '[memory-arch] all memory-architecture invariants hold\n'
