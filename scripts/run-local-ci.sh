#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

echo "[run-local-ci.sh] Starting local CI checks from project root"

./scripts/check-ci-paths.sh

echo "[run-local-ci.sh] Running cargo fmt --check"
cargo fmt --manifest-path Cargo.toml --all --check

echo "[run-local-ci.sh] Running cargo test --workspace"
cargo test --manifest-path Cargo.toml --workspace

echo "[run-local-ci.sh] Running cargo clippy --workspace --all-targets"
cargo clippy --manifest-path Cargo.toml --workspace --all-targets

echo "[run-local-ci.sh] Local CI checks passed"
