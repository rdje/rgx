#!/usr/bin/env bash

set -euo pipefail

allow_dirty_worktree=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --allow-dirty-worktree)
      allow_dirty_worktree=1
      shift
      ;;
    *)
      echo "[check-ci-paths.sh] Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

echo "[check-ci-paths.sh] Verifying required CI paths exist and are tracked by git"

required_paths=(
  ".github/workflows/ci.yml"
  ".github/workflows/book.yml"
  "scripts/check-ci-paths.sh"
  "scripts/capture-benchmark-trends.sh"
  "scripts/run-local-ci.sh"
  "Cargo.toml"
  "Cargo.lock"
  "rgx-core/Cargo.toml"
  "rgx-cli/Cargo.toml"
  "rgx-bench/Cargo.toml"
  "rgx-wasm/Cargo.toml"
)

for path in "${required_paths[@]}"; do
  if [[ ! -e "$path" ]]; then
    echo "[check-ci-paths.sh] Missing required path: $path" >&2
    exit 1
  fi

  if ! git ls-files --error-unmatch "$path" >/dev/null 2>&1; then
    if [[ "$allow_dirty_worktree" == "1" ]]; then
      echo "[check-ci-paths.sh] Allowing required path pending first commit: $path"
    else
      echo "[check-ci-paths.sh] Required path is not tracked by git: $path" >&2
      exit 1
    fi
  fi
done

if [[ "$allow_dirty_worktree" == "1" ]]; then
  echo "[check-ci-paths.sh] Skipping untracked-file audit in dirty-worktree mode"
else
  echo "[check-ci-paths.sh] Checking for non-ignored untracked files"

  if untracked_files="$(git ls-files --others --exclude-standard)" && [[ -n "$untracked_files" ]]; then
    echo "[check-ci-paths.sh] Found non-ignored untracked files:" >&2
    printf '%s\n' "$untracked_files" >&2
    exit 1
  fi
fi

# Hardcoded developer-machine absolute paths must not enter source.
# Unix dev paths (`/Users/…`, `/home/…`) are the common, important
# case. The Windows-drive alternative requires TWO literal backslashes
# (`[A-Za-z]:\\\\` → on-disk `X:\\`): a real Windows path embedded in
# a Rust string is escaped (`"C:\\Users\\…"`), whereas the previous
# one-backslash form (`[A-Za-z]:\\`) spuriously matched the ubiquitous
# `<letter>:\n` / `:\x` / `:\t` in `println!`/format literals and doc
# comments — which had silently kept this audit (and therefore the
# whole `run-local-ci.sh` gate) red on benign pre-existing source.
absolute_path_pattern='(/Users/|/home/|[A-Za-z]:\\\\)'

rust_path_report="$(mktemp)"
ci_path_report="$(mktemp)"
include_report="$(mktemp)"
trap 'rm -f "$rust_path_report" "$ci_path_report" "$include_report"' EXIT

echo "[check-ci-paths.sh] Auditing Rust source for compile-time include macros"

if grep -RInE --include='*.rs' 'include(_str|_bytes)?!\(' rgx-core rgx-cli rgx-bench rgx-wasm >"$include_report"; then
  echo "[check-ci-paths.sh] Found include-style macros:"
  cat "$include_report"
else
  echo "[check-ci-paths.sh] No include-style macros found in workspace source"
fi

echo "[check-ci-paths.sh] Checking Rust source for absolute filesystem paths"

if grep -RInE --include='*.rs' "$absolute_path_pattern" rgx-core rgx-cli rgx-bench rgx-wasm >"$rust_path_report"; then
  echo "[check-ci-paths.sh] Absolute filesystem paths are not allowed in Rust source files:" >&2
  cat "$rust_path_report" >&2
  exit 1
fi

echo "[check-ci-paths.sh] Checking CI workflow and helper scripts for absolute filesystem paths"
if grep -InE "$absolute_path_pattern" .github/workflows/ci.yml scripts/run-local-ci.sh scripts/capture-benchmark-trends.sh >"$ci_path_report"; then
  echo "[check-ci-paths.sh] Absolute filesystem paths are not allowed in CI workflow/script files:" >&2
  cat "$ci_path_report" >&2
  exit 1
fi

echo "[check-ci-paths.sh] CI path audit passed"
