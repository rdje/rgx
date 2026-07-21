#!/usr/bin/env bash
# DOCTRINE-REGISTRY-SYNC doctrine check (structural; runs everywhere).
#
# Doctrine (`DOCTRINE_ENFORCEMENT.md` §5): the driver's `DOCTRINES` array is the
# source of truth for "which doctrines are enforced by what", and §10 is its
# human-readable mirror — "kept in lockstep". A mirror kept in lockstep by
# discipline drifts; this check makes the lockstep mechanical.
#
# It asserts, id for id, that:
#   - every doctrine registered in scripts/check_doctrines.sh has a §10 row, and
#   - every §10 row corresponds to a registered doctrine,
# so the published manifest can never promise an enforcement that does not exist
# (the §11 anti-pattern "a registry entry pointing at a check that does not
# exist" — the driver's own meta-check covers the script side; this covers the
# documentation side).
#
# Deliberately compares IDs only, not prose: the wording of "what it proves" is
# allowed to differ in register between a shell comment and a doc table.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"

driver="$ROOT/scripts/check_doctrines.sh"
manifest="$ROOT/DOCTRINE_ENFORCEMENT.md"

for f in "$driver" "$manifest"; do
  if [ ! -f "$f" ]; then
    printf '✗ DOCTRINE-REGISTRY-SYNC: missing %s\n' "${f#"$ROOT"/}" >&2
    exit 1
  fi
done

# Registry ids: the quoted "ID|scope|..." lines inside the DOCTRINES=( ... ) array.
registry_ids="$(
  awk '/^DOCTRINES=\(/{inside=1; next} inside && /^\)/{inside=0} inside' "$driver" \
    | sed -n 's/^[[:space:]]*"\([A-Z0-9+-]*\)|.*/\1/p' \
    | LC_ALL=C sort -u
)"

# Manifest ids: §10 table rows of the form  | `ID` | archetype | check | proves |
manifest_ids="$(
  sed -n 's/^|[[:space:]]*`\([A-Z0-9+-]*\)`[[:space:]]*|.*|.*|.*/\1/p' "$manifest" \
    | LC_ALL=C sort -u
)"

if [ -z "$registry_ids" ]; then
  printf '✗ DOCTRINE-REGISTRY-SYNC: could not parse any doctrine id from the driver registry.\n' >&2
  exit 1
fi
if [ -z "$manifest_ids" ]; then
  printf '✗ DOCTRINE-REGISTRY-SYNC: could not parse any doctrine row from DOCTRINE_ENFORCEMENT.md §10.\n' >&2
  exit 1
fi

missing_in_manifest="$(LC_ALL=C comm -23 <(printf '%s\n' "$registry_ids") <(printf '%s\n' "$manifest_ids"))"
missing_in_registry="$(LC_ALL=C comm -13 <(printf '%s\n' "$registry_ids") <(printf '%s\n' "$manifest_ids"))"

if [ -z "$missing_in_manifest" ] && [ -z "$missing_in_registry" ]; then
  exit 0
fi

{
  printf '✗ DOCTRINE-REGISTRY-SYNC: the driver registry and DOCTRINE_ENFORCEMENT.md §10 disagree.\n\n'
  if [ -n "$missing_in_manifest" ]; then
    printf 'Registered in scripts/check_doctrines.sh but ABSENT from §10 (undocumented enforcement):\n'
    printf '%s\n' "$missing_in_manifest" | sed 's/^/  /'
  fi
  if [ -n "$missing_in_registry" ]; then
    printf 'Listed in §10 but NOT registered in the driver (a dangling promise):\n'
    printf '%s\n' "$missing_in_registry" | sed 's/^/  /'
  fi
  printf '\nFix: adding a doctrine means BOTH — one DOCTRINES=() line and one §10 row.\n'
} >&2
exit 1
