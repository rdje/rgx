#!/usr/bin/env bash
# PGEN compile-time perf gate driver — runs the full PGEN-vs-PCRE2
# compile-time comparison on the current host, printing a unified
# ratio table to stdout.
#
# Adapted from RGX's PGEN-RGX-0078 bundle. No RGX dependency; the
# only external dependencies are:
#   - PGEN (the parser being measured) — assumed to be `cargo run`able
#     from a checkout. By default the script assumes the current
#     working directory is PGEN's `rust/`.
#   - libpcre2-8 — for both C baselines. From the system package
#     manager. macOS: `brew install pcre2`.
#   - The `pcre2` Rust crate (in dev-dependencies of PGEN's Cargo.toml,
#     per Cargo.toml.snippet) — for the Rust microbench.
#
# Usage (from inside PGEN's rust/ directory):
#   bash perf/run_perf_gate.sh                           # default
#   PCRE2_PREFIX=/opt/homebrew/opt/pcre2 bash perf/run_perf_gate.sh
#
# Output: combined ratio table + closure-status line.

set -euo pipefail

PCRE2_PREFIX="${PCRE2_PREFIX:-$(brew --prefix pcre2 2>/dev/null || echo /usr/local)}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK_DIR="${WORK_DIR:-$SCRIPT_DIR/../target/perf_gate}"
mkdir -p "$WORK_DIR"

echo "# PGEN regex parse vs PCRE2 compile — perf-gate run"
echo "# Host: $(uname -srm)"
echo "# Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "# PCRE2 prefix: $PCRE2_PREFIX"
echo

# 1. Build + run the standalone C baselines.
echo "## 1/3 — Building PCRE2 no-JIT baseline (C)"
cc -O2 \
    -I"$PCRE2_PREFIX/include" -L"$PCRE2_PREFIX/lib" \
    -o "$WORK_DIR/pcre2_compile_baseline" \
    "$SCRIPT_DIR/pcre2_compile_baseline.c" -lpcre2-8

echo "## 2/3 — Building PCRE2 with-JIT baseline (C)"
cc -O2 \
    -I"$PCRE2_PREFIX/include" -L"$PCRE2_PREFIX/lib" \
    -o "$WORK_DIR/pcre2_compile_jit_baseline" \
    "$SCRIPT_DIR/pcre2_compile_jit_baseline.c" -lpcre2-8

echo "## 3/3 — Running both C baselines"
"$WORK_DIR/pcre2_compile_baseline" > "$WORK_DIR/pcre2_no_jit.txt"
"$WORK_DIR/pcre2_compile_jit_baseline" > "$WORK_DIR/pcre2_with_jit.txt"

echo
echo "=== PCRE2 compile (no JIT) ==="
cat "$WORK_DIR/pcre2_no_jit.txt"
echo
echo "=== PCRE2 compile + JIT ==="
cat "$WORK_DIR/pcre2_with_jit.txt"
echo

# 2. Run the Rust microbench (which itself prints the combined PGEN
#    parse + PCRE2 ratio table).
echo "## Running PGEN parse + PCRE2-via-Rust microbench"
echo "(this is the canonical measurement; the C baselines above are"
echo "for cross-validation only.)"
echo
cargo run --release --features generated_parsers \
    --example pgen_pcre2_compile_ratio
