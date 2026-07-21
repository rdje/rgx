#!/usr/bin/env bash
# scripts/check_doctrines.sh — THE GENERAL DOCTRINE ENFORCER (registry + driver).
#
# RGX's instance of the Doctrine Enforcement Architecture (`DOCTRINE_ENFORCEMENT.md`,
# devised by the sibling PGEN project and adopted here 2026-07-21 via task-tree leaf
# `DOCTRINE-ADOPT.1`). One driver runs EVERY mechanizable doctrine check, reports
# per-doctrine PASS/FAIL/SKIP, and exits NONZERO on any breach.
#
# Thesis: a doctrine that is not mechanically checked is not enforced — it is a
# suggestion. So every doctrine is paired with a deterministic check
# (`DOCTRINE_ENFORCEMENT.md` §4 contract), all checks run from this one registry,
# and the gates run this driver.
#
# Enforcement layering (defense in depth — `DOCTRINE_ENFORCEMENT.md` §7):
#   E1 discovery  : CLAUDE.md / AGENTS.md / README.md / docs/decisions/ name the doctrines.
#   E2 self-check : THIS driver + each registered `check_*.sh` (single source of truth).
#   E3 git hook   : `scripts/git-hooks/pre-commit` calls this (activate: ./scripts/setup-hooks.sh).
#   E4 CI         : `scripts/run-local-ci.sh` calls this, and hosted CI runs that script —
#                   server-side, so `--no-verify` cannot reach it. THIS is the backstop.
#
# Proof strength, stated honestly (the three archetypes, §3):
#   - STRUCTURAL  — the check re-derives the invariant from the tree; the verdict is a
#                   fact about the files and cannot be faked.
#   - ORACLE      — the check re-EXECUTES a deterministic tool and asserts the result;
#                   a fabricated claim does not reproduce. RGX's heavyweight oracles (the
#                   full test suite, clippy, the PCRE2 conformance ratchet) run inside
#                   `run-local-ci.sh` and are bound to the commit by the GATE-RECEIPT
#                   doctrine below.
#   - EVIDENCE    — requires a re-checkable artifact for a process that leaves no other
#                   trace. Queued as `DOCTRINE-ADOPT.2`; not yet registered.
#
# SCOPE. Some doctrines are meaningful only at commit time: the gate receipt does not
# exist server-side, and CI has no staged set. Those are marked `hook` and are SKIPPED
# (reported, not silently passed) under `--scope ci`. Everything else is `always`.
#
# Usage:  scripts/check_doctrines.sh [--scope hook|ci]     (default: hook)
#
# The registry below is the source of truth for "which doctrines are enforced by what";
# `DOCTRINE_ENFORCEMENT.md` §10 is its human-readable mirror, and the two are kept in
# lockstep BY A CHECK (`DOCTRINE-REGISTRY-SYNC`), not by discipline.
set -uo pipefail   # deliberately NOT `-e`: run ALL checks, collect every result, then report.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"

scope="hook"
while [ $# -gt 0 ]; do
  case "$1" in
    --scope) scope="${2:-}"; shift 2 ;;
    --scope=*) scope="${1#*=}"; shift ;;
    -h|--help) sed -n '1,40p' "$0"; exit 0 ;;
    *) printf 'check_doctrines.sh: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done
case "$scope" in
  hook|ci) ;;
  *) printf 'check_doctrines.sh: --scope must be "hook" or "ci" (got: %s)\n' "$scope" >&2; exit 2 ;;
esac

# Each entry: "ID|scope|what it proves|relative/path/to/check.sh"
#   scope=always → runs everywhere (structural invariants over the tree)
#   scope=hook   → commit-time only (staged-set or local-receipt semantics)
#
# Adding a doctrine = write a `check_*.sh` obeying the §4 contract, add ONE line here,
# and add its row to `DOCTRINE_ENFORCEMENT.md` §10. The meta-check below asserts every
# registered enforcer exists and is executable, so a registry line can never be a
# dangling promise.
DOCTRINES=(
  "MEMORY-ARCH|always|the durable 4-layer memory architecture invariants (MEMORY_ARCHITECTURE.md §9)|scripts/check_memory_architecture.sh"
  "KNOWLEDGE-MAP|always|the derived Knowledge Map is in sync with its fact sources|knowledge-map/scripts/check_knowledge_map.sh"
  "PGEN-READONLY|always|subs/pgen carries no modified tracked content (read-only from RGX; ADR 0002)|scripts/check_pgen_submodule_readonly.sh"
  "DOCTRINE-REGISTRY-SYNC|always|this registry and DOCTRINE_ENFORCEMENT.md §10 agree, id for id|scripts/check_doctrine_registry_sync.sh"
  "CODE-CHANGE-LEAF|hook|every staged code change is owned by a task-tree leaf whose tree file is updated in the same commit (Code-Change Doctrine)|scripts/check_code_change_leaf.sh"
  "TWO-TRACK-DOCS|hook|a staged code change carries its CHANGES.md ledger entry (COMMIT.md step 3, track B)|scripts/check_two_track_docs.sh"
  "GATE-RECEIPT|hook|a gate-affecting commit is certified by a fresh green ./scripts/run-local-ci.sh receipt for exactly this content|scripts/check_gate_receipt.sh"
)

fail=0
ran=0
skipped=0
declare -a report=()

for entry in "${DOCTRINES[@]}"; do
  IFS='|' read -r id dscope proves script <<< "$entry"

  # Meta-check first: a registered enforcer must exist and be executable,
  # regardless of scope — a dangling registry entry is itself a breach.
  if [ ! -x "$ROOT/$script" ]; then
    report+=("✗ FAIL  ${id} — registered enforcer missing or not executable: ${script}")
    fail=1
    continue
  fi

  if [ "$dscope" = "hook" ] && [ "$scope" = "ci" ]; then
    report+=("• SKIP  ${id} — commit-time doctrine (no staged set / no local receipt in CI)")
    skipped=$((skipped + 1))
    continue
  fi

  if out="$("$ROOT/$script" 2>&1)"; then
    report+=("✓ PASS  ${id} — ${proves}")
  else
    report+=("✗ FAIL  ${id} — ${proves}")
    printf '%s\n' "$out" >&2
    fail=1
  fi
  ran=$((ran + 1))
done

printf '\n================ RGX DOCTRINE ENFORCEMENT REPORT ================\n' >&2
for line in "${report[@]}"; do printf '  %s\n' "$line" >&2; done
printf '=================================================================\n' >&2
if [ "$fail" -eq 0 ]; then
  printf 'doctrines: ALL %d checked doctrines PASS (scope=%s; %d skipped).\n' \
    "$ran" "$scope" "$skipped" >&2
else
  printf 'doctrines: ✗ one or more doctrines FAILED — commit/merge blocked. Fix above, do not bypass.\n' >&2
fi
exit "$fail"
