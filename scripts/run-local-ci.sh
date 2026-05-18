#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

# Shared gate-receipt identity (also used by the pre-commit hook).
# shellcheck source=scripts/lib-gate-receipt.sh
. "$repo_root/scripts/lib-gate-receipt.sh"

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

run_step "./scripts/check-ci-paths.sh --allow-dirty-worktree" ./scripts/check-ci-paths.sh --allow-dirty-worktree

if [[ "$have_pgen_checkout" != "1" ]]; then
  echo "[run-local-ci.sh] Skipping cargo-based validation because the PGEN submodule is not initialized."
  echo "[run-local-ci.sh] Only path-audit checks ran in this fallback mode."
  exit 0
fi

# PGEN no longer ships its generated parsers (slice-5 pivot, pgen
# commit 0ed2b2ad — see the `project_pgen_generated_files` rule and
# README "Build note"). On a FRESH checkout (CI, or a fresh clone)
# `subs/pgen/generated/*` is absent, so pgen's
# `include!("../../generated/return_annotation_parser.rs")` fails and
# nothing downstream compiles. This was the actual cause of red CI on
# `main` (the workflow never ran the mandated bootstrap). Run the
# idempotent cold-clone bootstrap before any cargo step; skip the
# slow `make` when the artifacts already exist (the common local
# case). Generating the *untracked* `generated/` tree is the
# sanctioned PGEN workflow — it does not modify pgen's tracked
# content (subs/pgen still shows only `?`, never `M`).
pgen_generated_dir="$repo_root/subs/pgen/generated"
if [[ -f "$pgen_generated_dir/regex_parser.rs" \
      && -f "$pgen_generated_dir/return_annotation_parser.rs" ]]; then
  echo "[run-local-ci.sh] PGEN generated parsers present — skipping bootstrap."
else
  run_step "make -C subs/pgen/rust regex_parser_bootstrap (cold-clone PGEN parser gen)" \
    make -C "$repo_root/subs/pgen/rust" SHELL=/bin/bash regex_parser_bootstrap
fi

run_step "cargo fmt --check (RGX workspace packages)" cargo fmt --manifest-path Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm --check

# Keep package coverage explicit here. The submodule-backed PGEN default path has
# shown intermittent hangs under the umbrella `cargo test --workspace` command,
# while the equivalent per-package RGX coverage remains stable and precise.
run_step "cargo test -p rgx-core" cargo test --manifest-path Cargo.toml -p rgx-core
run_step "cargo test -p rgx-cli" cargo test --manifest-path Cargo.toml -p rgx-cli
run_step "cargo test -p rgx-bench" cargo test --manifest-path Cargo.toml -p rgx-bench
run_step "cargo test -p rgx-wasm" cargo test --manifest-path Cargo.toml -p rgx-wasm

run_step "cargo test -p rgx-core --features pgen-parser" cargo test --manifest-path Cargo.toml -p rgx-core --features pgen-parser
run_step "cargo test -p rgx-cli --features pgen-parser" cargo test --manifest-path Cargo.toml -p rgx-cli --features pgen-parser

run_step "cargo test -p rgx-core --features lua" cargo test --manifest-path Cargo.toml -p rgx-core --features lua
run_step "cargo test -p rgx-core --features javascript" cargo test --manifest-path Cargo.toml -p rgx-core --features javascript
run_step "cargo test -p rgx-core --features rhai" cargo test --manifest-path Cargo.toml -p rgx-core --features rhai
run_step "cargo test -p rgx-core --features wasm" cargo test --manifest-path Cargo.toml -p rgx-core --features wasm
run_step "cargo check -p rgx-core --features all-languages" cargo check --manifest-path Cargo.toml -p rgx-core --features all-languages
run_step "cargo test -p rgx-cli --features all-languages" cargo test --manifest-path Cargo.toml -p rgx-cli --features all-languages

run_step "cargo clippy --workspace --all-targets" cargo clippy --manifest-path Cargo.toml --workspace --all-targets

# Accuracy gate. The PCRE2 conformance ratchet is `#[ignore]`d so it
# is not part of the default fast gate, but it is the merge condition
# for any change touching parsing / the adapter / the VM / the
# conformance harness (see COMMIT.md step 2). Fold it in with
# RGX_RUN_CONFORMANCE=1; the harness's own `RATCHET OK` assertion
# fails the step (and thus this script, via set -e) on regression.
if [[ "${RGX_RUN_CONFORMANCE:-0}" == "1" ]]; then
  run_step "cargo test -p rgx-core --test pcre2_conformance (ratchet)" \
    cargo test --release --manifest-path Cargo.toml -p rgx-core \
      --test pcre2_conformance -- --ignored
else
  echo "[run-local-ci.sh] Skipping PCRE2 conformance ratchet (RGX_RUN_CONFORMANCE!=1)."
  echo "[run-local-ci.sh] COMMIT.md requires it for parsing/adapter/VM/conformance changes."
fi

if [[ "${RGX_SKIP_BENCH_TRENDS:-0}" == "1" ]]; then
  echo "[run-local-ci.sh] Skipping benchmark trend capture because RGX_SKIP_BENCH_TRENDS=1"
else
  run_step "./scripts/capture-benchmark-trends.sh" ./scripts/capture-benchmark-trends.sh
fi

# Reached only if every run_step above succeeded (set -euo pipefail).
# Stamp a green receipt for exactly this gate-affecting content so
# the pre-commit hook can certify the commit ran the real gate.
rgx_gate_state_id > "$(rgx_receipt_path)"
echo ""
echo "=================================================================="
echo "  ✓ run-local-ci.sh: ALL GATE STEPS PASSED"
echo "    Green receipt written: $(rgx_receipt_path)"
echo "    (covers Rust/Cargo/CI/script content of the current worktree)"
echo "=================================================================="
