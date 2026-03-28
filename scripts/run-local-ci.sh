#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"
run_step() {
  local description="$1"
  shift
  echo "[run-local-ci.sh] Running ${description}"
  "$@"
}

echo "[run-local-ci.sh] Starting local CI checks from project root"

run_step "./scripts/check-ci-paths.sh" ./scripts/check-ci-paths.sh

run_step "cargo fmt --check" cargo fmt --manifest-path Cargo.toml --all --check

run_step "cargo test --workspace" cargo test --manifest-path Cargo.toml --workspace

run_step "cargo test -p rgx-core --features pgen-parser" cargo test --manifest-path Cargo.toml -p rgx-core --features pgen-parser
run_step "cargo test -p rgx-core --features lua" cargo test --manifest-path Cargo.toml -p rgx-core --features lua
run_step "cargo test -p rgx-core --features javascript" cargo test --manifest-path Cargo.toml -p rgx-core --features javascript
run_step "cargo test -p rgx-core --features wasm" cargo test --manifest-path Cargo.toml -p rgx-core --features wasm
run_step "cargo check -p rgx-core --features all-languages" cargo check --manifest-path Cargo.toml -p rgx-core --features all-languages

run_step "cargo clippy --workspace --all-targets" cargo clippy --manifest-path Cargo.toml --workspace --all-targets

echo "[run-local-ci.sh] Local CI checks passed"
