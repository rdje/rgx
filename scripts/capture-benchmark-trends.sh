#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

output_dir="${RGX_BENCHMARK_TREND_DIR:-target/benchmark-trends}"
mode="${RGX_BENCHMARK_TREND_MODE:-quick}"
compare_against="${RGX_BENCHMARK_COMPARE_AGAINST:-auto}"
profile_args=()

if [[ "$mode" == "full" ]]; then
  profile_args=(--profile bench)
fi

echo "[capture-benchmark-trends.sh] Capturing ${mode} benchmark trends into ${output_dir}"
echo "[capture-benchmark-trends.sh] Comparison baseline request: ${compare_against}"

cargo run "${profile_args[@]}" --manifest-path Cargo.toml -p rgx-bench --bin trend_capture -- --mode "$mode" --output-dir "$output_dir" --compare-against "$compare_against"

echo "[capture-benchmark-trends.sh] Benchmark trend summary available at ${output_dir}/latest.md and ${output_dir}/latest-${mode}.md"
echo "[capture-benchmark-trends.sh] Archived ${mode} history available under ${output_dir}/history/${mode}/"
