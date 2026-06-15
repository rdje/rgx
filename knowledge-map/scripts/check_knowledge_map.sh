#!/usr/bin/env bash
# check_knowledge_map.sh — validate Knowledge Map fact files and assert the map is in sync.
#
# Part of the Knowledge Map (KM) bundle. See ../KNOWLEDGE_MAP_ARCHITECTURE.md.
# Exits NONZERO on any violation (wire it into pre-commit + CI). Checks:
#   1. every KM fact (front-matter has `answers:`) carries the required fields
#      (id, title, date, and at least one of evidence/reverify);
#   2. fact ids are unique;
#   3. the committed map equals a fresh regeneration (derive-and-diff — no drift possible).
# Portable: POSIX shell + awk (BSD or GNU).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null || pwd)"

[ -f "$ROOT/.knowledge_map.conf" ] && . "$ROOT/.knowledge_map.conf"
[ -f "$SCRIPT_DIR/knowledge_map.conf" ] && . "$SCRIPT_DIR/knowledge_map.conf"
: "${KM_SCAN_DIRS:=docs/knowledge docs/decisions}"
: "${KM_OUTPUT:=KNOWLEDGE_MAP.md}"

cd "$ROOT"
fail=0
note() { printf 'knowledge-map: %s\n' "$1" >&2; fail=1; }
warn() { printf 'knowledge-map: WARNING: %s\n' "$1" >&2; }

ids_file=""; tmpmap=""
cleanup() { [ -n "$ids_file" ] && rm -f "$ids_file"; [ -n "$tmpmap" ] && rm -f "$tmpmap"; }
trap cleanup EXIT
ids_file="$(mktemp)"; tmpmap="$(mktemp)"

# --- 1 + 2: per-file required-field validation; collect ids ---
for d in $KM_SCAN_DIRS; do
  [ -d "$d" ] || continue
  while IFS= read -r f; do
    res="$(awk '
      BEGIN { infm=0; started=0; hasans=0; curkey="" }
      NR==1 { if ($0 ~ /^---[ \t]*$/) { infm=1; started=1; next } }
      infm==1 && $0 ~ /^---[ \t]*$/ { infm=0; next }
      infm==1 {
        if ($0 ~ /^[ \t]+-[ \t]+/) { if (curkey=="answers") hasans=1; next }
        if (match($0, /^[A-Za-z_][A-Za-z0-9_]*:/)) {
          key=substr($0,1,RLENGTH-1); rest=substr($0,RLENGTH+1)
          gsub(/^[ \t]+/,"",rest); gsub(/[ \t]+$/,"",rest); curkey=key
          if (key=="answers" && rest ~ /\[.*[A-Za-z0-9].*\]/) hasans=1
          if (key=="id")       { gsub(/^"|"$/,"",rest); vid=rest }
          if (key=="title")    okt=1
          if (key=="date")     okd=1
          if (key=="evidence") oke=1
          if (key=="reverify") okr=1
        }
        next
      }
      END {
        if (!started || !hasans) { print "NOTFACT"; exit }
        miss=""
        if (vid=="") miss=miss " id"
        if (!okt)    miss=miss " title"
        if (!okd)    miss=miss " date"
        if (!oke && !okr) miss=miss " evidence-or-reverify"
        print "FACT\t" vid "\t" miss
      }
    ' "$f")"
    case "$res" in
      FACT*)
        vid="$(printf '%s' "$res" | cut -f2)"
        miss="$(printf '%s' "$res" | cut -f3)"
        [ -n "$vid" ] && printf '%s\t%s\n' "$vid" "$f" >> "$ids_file"
        [ -n "$(printf '%s' "$miss" | tr -d ' ')" ] && note "$f: KM fact missing required front-matter:$miss"
        ;;
      *) : ;;  # NOTFACT or empty — ignore non-fact markdown
    esac
  done < <(find "$d" -type f -name '*.md' 2>/dev/null | LC_ALL=C sort)
done

if [ -s "$ids_file" ]; then
  dups="$(cut -f1 "$ids_file" | LC_ALL=C sort | uniq -d || true)"
  [ -n "$dups" ] && printf '%s\n' "$dups" | while IFS= read -r dd; do
    [ -n "$dd" ] && note "duplicate fact id: '$dd' (ids must be unique across all scanned dirs)"
  done
  # surviving fail-state from the subshell above is lost; re-detect:
  [ -n "$dups" ] && fail=1
fi

# --- 3: map-in-sync (derive-and-diff) ---
if KM_OUTPUT="$tmpmap" bash "$SCRIPT_DIR/gen_knowledge_map.sh" >/dev/null 2>&1; then
  if [ -f "$KM_OUTPUT" ]; then
    if ! diff -q "$KM_OUTPUT" "$tmpmap" >/dev/null 2>&1; then
      note "$KM_OUTPUT is OUT OF SYNC with the fact files — run knowledge-map/scripts/gen_knowledge_map.sh and commit the result"
    fi
  else
    note "$KM_OUTPUT does not exist — run knowledge-map/scripts/gen_knowledge_map.sh and commit it"
  fi
else
  note "gen_knowledge_map.sh failed to run"
fi

[ "$fail" -eq 0 ] && printf 'knowledge-map: OK (facts valid, ids unique, map in sync)\n' >&2
exit "$fail"
