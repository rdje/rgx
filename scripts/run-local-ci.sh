#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

pgen_checkout="$repo_root/../pgen/rust/Cargo.toml"
skip_pgen_checks="${RGX_SKIP_PGEN_CHECKS:-0}"
have_pgen_checkout=0

if [[ ! -f "$pgen_checkout" ]]; then
  echo "[run-local-ci.sh] Missing sibling PGEN checkout at ../pgen (expected $pgen_checkout)"
  if [[ "$skip_pgen_checks" == "1" ]]; then
    echo "[run-local-ci.sh] RGX_SKIP_PGEN_CHECKS=1, so pgen-specific checks will be skipped."
  else
    echo "[run-local-ci.sh] The current local pgen-parser integration depends on that checkout."
    exit 1
  fi
else
  have_pgen_checkout=1
fi

run_step() {
  local description="$1"
  shift
  echo "[run-local-ci.sh] Running ${description}"
  "$@"
}

echo "[run-local-ci.sh] Starting local CI checks from project root"

run_step "./scripts/check-ci-paths.sh" ./scripts/check-ci-paths.sh

run_step "cargo fmt --check (RGX workspace packages)" cargo fmt --manifest-path Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm --check

run_step "cargo test --workspace" cargo test --manifest-path Cargo.toml --workspace

if [[ "$have_pgen_checkout" == "1" ]]; then
  run_step "cargo test -p rgx-core --features pgen-parser" cargo test --manifest-path Cargo.toml -p rgx-core --features pgen-parser
  run_step "cargo test -p rgx-cli --features pgen-parser" cargo test --manifest-path Cargo.toml -p rgx-cli --features pgen-parser
else
  echo "[run-local-ci.sh] Skipping pgen-parser feature checks because sibling PGEN checkout is unavailable."
fi

run_step "cargo test -p rgx-core --features lua" cargo test --manifest-path Cargo.toml -p rgx-core --features lua
run_step "cargo test -p rgx-core --features javascript" cargo test --manifest-path Cargo.toml -p rgx-core --features javascript
run_step "cargo test -p rgx-core --features wasm" cargo test --manifest-path Cargo.toml -p rgx-core --features wasm
run_step "cargo check -p rgx-core --features all-languages" cargo check --manifest-path Cargo.toml -p rgx-core --features all-languages

run_step "cargo clippy --workspace --all-targets" cargo clippy --manifest-path Cargo.toml --workspace --all-targets

echo "[run-local-ci.sh] Local CI checks passed"
