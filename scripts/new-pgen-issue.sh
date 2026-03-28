#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/new-pgen-issue.sh --summary "short summary" [--first-seen ISO8601] [--dry-run]

Creates the next numbered git-tracked local PGEN issue stub under pgen-issues/.
EOF
}

indent_block() {
  sed 's/^/  /'
}

SUMMARY=""
FIRST_SEEN_AT=""
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --summary)
      [[ $# -ge 2 ]] || {
        printf 'missing value for --summary\n' >&2
        usage >&2
        exit 1
      }
      SUMMARY="$2"
      shift 2
      ;;
    --first-seen)
      [[ $# -ge 2 ]] || {
        printf 'missing value for --first-seen\n' >&2
        usage >&2
        exit 1
      }
      FIRST_SEEN_AT="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$SUMMARY" ]]; then
  printf 'the --summary argument is required\n' >&2
  usage >&2
  exit 1
fi

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
ISSUES_DIR="$REPO_ROOT/pgen-issues"

mkdir -p "$ISSUES_DIR"

shopt -s nullglob
ISSUE_FILES=("$ISSUES_DIR"/PGEN-RGX-*.yaml)
MAX_ID=0
for issue_path in "${ISSUE_FILES[@]}"; do
  issue_name=${issue_path##*/}
  issue_number=${issue_name#PGEN-RGX-}
  issue_number=${issue_number%.yaml}
  if [[ "$issue_number" =~ ^[0-9]{4}$ ]] && (( 10#$issue_number > MAX_ID )); then
    MAX_ID=$((10#$issue_number))
  fi
done

NEXT_ID=$((MAX_ID + 1))
ISSUE_ID=$(printf 'PGEN-RGX-%04d' "$NEXT_ID")
OUTPUT_PATH="$ISSUES_DIR/$ISSUE_ID.yaml"

NOW_UTC=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
FIRST_SEEN_VALUE=${FIRST_SEEN_AT:-$NOW_UTC}
RGX_COMMIT=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || printf 'unknown')
SUMMARY_BLOCK=$(printf '%s\n' "$SUMMARY" | indent_block)

ISSUE_CONTENT=$(cat <<EOF
id: $ISSUE_ID
summary: |
$SUMMARY_BLOCK
status: open
opened_at: $NOW_UTC
first_seen_at: $FIRST_SEEN_VALUE
last_updated_at: $NOW_UTC
parser_backend: pgen
parser_backend_version: unknown
rgx_commit: $RGX_COMMIT
upstream_report:
  reported: false
  issue_id: null
  issue_url: null
  reported_at: null
context:
  feature_flag: pgen-parser
  parser_entrypoint: rgx-core/src/parsing.rs
  command: ""
  pattern: ""
  input: ""
  notes: |
    Fill in the precise RGX-side context where the issue manifests.
expected_behavior: |
  Describe the expected parse result, AST shape, compile error, or downstream behavior.
actual_behavior: |
  Describe the observed misbehavior, wrong AST, wrong error, wrong match behavior, or crash.
reproduction: |
  Provide the smallest reproducible pattern, inputs, commands, tests, and any relevant logs.
impact: |
  Explain why this matters for RGX integration or downstream behavior.
resolution:
  status: unresolved
  fixed_in_rgx_commit: null
  verified_at: null
  verification_notes: |
    Add closing validation evidence here when the issue is resolved.
EOF
)

if (( DRY_RUN )); then
  printf 'DRY-RUN PATH: %s\n' "$OUTPUT_PATH"
  printf '%s\n' "$ISSUE_CONTENT"
  exit 0
fi

printf '%s\n' "$ISSUE_CONTENT" > "$OUTPUT_PATH"
printf 'Created %s\n' "$OUTPUT_PATH"
