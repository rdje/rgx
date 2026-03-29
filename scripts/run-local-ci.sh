#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

pgen_checkout="$repo_root/subs/pgen/rust/Cargo.toml"
skip_pgen_checks="${RGX_SKIP_PGEN_CHECKS:-0}"
have_pgen_checkout=0

if [[ ! -f "$pgen_checkout" ]]; then
  echo "[run-local-ci.sh] Missing initialized PGEN submodule at subs/pgen (expected $pgen_checkout)"
  echo "[run-local-ci.sh] Run: git submodule update --init --recursive"
  if [[ "$skip_pgen_checks" == "1" ]]; then
    echo "[run-local-ci.sh] RGX_SKIP_PGEN_CHECKS=1, so cargo-based checks will be skipped."
  else
    echo "[run-local-ci.sh] RGX now defaults to the submodule-backed PGEN parser and needs that checkout."
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

if [[ "$have_pgen_checkout" != "1" ]]; then
  echo "[run-local-ci.sh] Skipping cargo-based validation because the PGEN submodule is not initialized."
  echo "[run-local-ci.sh] Only path-audit checks ran in this fallback mode."
  exit 0
fi

run_step "cargo fmt --check (RGX workspace packages)" cargo fmt --manifest-path Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm --check

run_step "cargo test --workspace" cargo test --manifest-path Cargo.toml --workspace

run_step "cargo test -p rgx-core --features pgen-parser" cargo test --manifest-path Cargo.toml -p rgx-core --features pgen-parser
run_step "cargo test -p rgx-cli --features pgen-parser" cargo test --manifest-path Cargo.toml -p rgx-cli --features pgen-parser

run_step "cargo test -p rgx-core --features lua" cargo test --manifest-path Cargo.toml -p rgx-core --features lua
run_step "cargo test -p rgx-core --features javascript" cargo test --manifest-path Cargo.toml -p rgx-core --features javascript
run_step "cargo test -p rgx-core --features wasm" cargo test --manifest-path Cargo.toml -p rgx-core --features wasm
run_step "cargo check -p rgx-core --features all-languages" cargo check --manifest-path Cargo.toml -p rgx-core --features all-languages

run_step "cargo clippy --workspace --all-targets" cargo clippy --manifest-path Cargo.toml --workspace --all-targets

echo "[run-local-ci.sh] Local CI checks passed"
