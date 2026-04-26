#!/usr/bin/env bash
# Systematic samply-based profiling runner for RGX hot paths.
#
# Builds rgx-core's `perf_profile_targets` example with the
# `profiling` Cargo profile (release-fast + symbols + debuginfo so
# samply can resolve function names), then records a samply trace
# per (pattern, method) target into `target/samply-profiles/`.
#
# Usage:
#   ./scripts/run-samply.sh                          # run all default targets
#   ./scripts/run-samply.sh email_basic.find_first   # just one target
#   RGX_PROFILE_DURATION_MS=5000 ./scripts/run-samply.sh   # longer recording window
#
# Output: `target/samply-profiles/<target>.json.gz` per target.
# Open with `samply load <file>` (spawns Firefox Profiler UI on
# localhost) or upload to https://profiler.firefox.com.
#
# Why `--save-only`: keeps the script non-interactive. The user
# inspects results explicitly via `samply load`.
#
# Why `iteration-count=3` per target: samply's first iteration of a
# fresh process has cold-cache noise. Three iterations + averaging
# in the UI gives steadier hot-path attribution. The driver itself
# loops for 3 seconds inside the process; iteration-count gates how
# many times samply spawns the binary.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

OUT_DIR="target/samply-profiles"
mkdir -p "$OUT_DIR"

# Default target list — covers the bench corpus + the slow ones we
# specifically want to characterise. Override by passing targets as
# arguments.
DEFAULT_TARGETS=(
    "literal_simple.find_first"
    "literal_simple.find_all"
    "digit_sequence.find_first"
    "digit_sequence.find_all"
    "email_basic.find_first"
    "email_basic.find_all"
    "email_basic.is_match"
    "alternation.find_first"
    "capture_groups.find_first"
    "url_simple.find_first"
    "character_class.find_first"
)

if [[ $# -gt 0 ]]; then
    TARGETS=("$@")
else
    TARGETS=("${DEFAULT_TARGETS[@]}")
fi

DURATION_MS="${RGX_PROFILE_DURATION_MS:-3000}"

echo "[run-samply.sh] Building perf_profile_targets with profiling profile"
cargo build --profile profiling -p rgx-core --example perf_profile_targets >&2

BIN="target/profiling/examples/perf_profile_targets"
if [[ ! -x "$BIN" ]]; then
    echo "[run-samply.sh] expected binary not found: $BIN" >&2
    exit 1
fi

echo "[run-samply.sh] Running ${#TARGETS[@]} targets, ${DURATION_MS}ms each"
for target in "${TARGETS[@]}"; do
    out="$OUT_DIR/${target//\//_}.json.gz"
    echo ""
    echo "[run-samply.sh] === $target ==="
    RGX_PROFILE_TARGET="$target" \
    RGX_PROFILE_DURATION_MS="$DURATION_MS" \
        samply record \
            --save-only \
            --rate 1000 \
            --output "$out" \
            "$BIN"
    echo "[run-samply.sh] wrote $out"
done

echo ""
echo "[run-samply.sh] Done. Inspect any profile with:"
echo "    samply load <profile.json.gz>"
echo ""
echo "[run-samply.sh] Profiles in $OUT_DIR:"
ls -lh "$OUT_DIR" | awk 'NR > 1 { printf "  %s  %s\n", $5, $9 }'
