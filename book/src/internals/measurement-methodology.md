# Performance Measurement Methodology

This chapter is the **complete, reproducible specification** of how RGX
measures performance — both the compile-time numbers (which are dominated by
PGEN's regex parse) and the runtime match-speed numbers. It is written so that
**PGEN can replicate the exact methodology** and produce numbers that line up
with RGX's, without depending on RGX. Every constant, entry point, statistic,
and environment control below is taken verbatim from the committed measurement
tooling in this repository.

If you only read one section, read [Environment controls](#environment-controls)
and [Why RGX measures ~214× where an earlier note said ~80×](#reconciling-214-vs-80)
— allocator choice and machine quiescence are the two things that move the
number the most.

## Why this exists

RGX does not own a parser; **PGEN is the sole parser**. The cost of
`Regex::compile` is therefore dominated by PGEN's regex parse, and the
RGX-vs-PCRE2 compile-time gap cannot be closed RGX-side. To let PGEN iterate
on parser speed independently, the methodology must be (a) precisely
specified, (b) reproducible on a machine with no RGX checkout, and (c)
identical on both sides so the ratio is meaningful. This chapter is that
specification; the vendorable bundle that implements it is described in
[The vendorable PGEN bundle](#the-vendorable-pgen-bundle).

## The benchmark corpus

All compile-time measurements use one fixed 8-pattern corpus, stable across
`PGEN-RGX-0073` and `PGEN-RGX-0078`. It is the single source of truth; do not
substitute patterns when comparing across runs or across projects.

| Name | Pattern |
|---|---|
| `literal_simple` | `test` |
| `digit_sequence` | `\d{3}-\d{2}-\d{4}` |
| `character_class` | `[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}` |
| `alternation` | `cat\|dog\|bird` |
| `capture_groups` | `(\d{4})-(\d{2})-(\d{2})` |
| `url_simple` | `https?://\S+` |
| `email_basic` | `\b\w+@\w+\.\w+\b` |
| `anchor_complex` | `^(\d+)\s+(?P<word>\w+)\s+(?:foo\|bar)$` |

The corpus is also persisted as `patterns.tsv` next to every measurement
bundle so a consuming project never has to transcribe it.

## Axis 1 — PGEN regex parse time (the "regex compile time")

This is the headline number. "Regex compile time" in RGX's tracking docs means
**the time PGEN takes to parse one pattern through the embedding API**, because
that is 63–86 % of `Regex::compile` wall-clock (see Axis 2).

- **Tool:** `rgx-core/examples/pgen_compile_perf_dump.rs`
- **Entry point timed:** `pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern)` — the *exact* call RGX's adapter makes. Not a PGEN-internal micro-path; the integration-relevant boundary.
- **Iterations:** `5000` measured samples per pattern.
- **Warm-up:** `200` samples run and discarded before measurement (JIT/cache/branch-predictor warm-up; allocator arena warm-up).
- **Clock:** `std::time::Instant::now()` immediately before the call, `t0.elapsed().as_nanos()` immediately after. One call per timed sample (not batched) so per-call latency is what is recorded.
- **Statistics reported:** the sample vector is sorted; **p50** = `samples[len/2]` (median), plus `min`, `mean`, `p99`, `max`. p50 is the headline because parse latency has a right-skewed tail (allocator, page faults) and the median is the stable central estimate.
- **Output:** `pgen-issues/artifacts/PGEN-RGX-0078/measurements/pgen_parse_p50.txt`, tab-separated `name<TAB>p50<TAB>min<TAB>mean<TAB>p99<TAB>max` in nanoseconds, with a methodology header line. The same run also persists per-pattern parse outcomes, AST dumps, the `parser_embedding_api_contract()` snapshot, and `patterns.tsv` so the measurement is fully self-describing.

## Axis 2 — compile phase split (where the time goes)

Establishes that PGEN parse dominates `Regex::compile`, so the gap is
PGEN-bound.

- **Tool:** `rgx-core/examples/compile_phase_split.rs`
- **Iterations / warm-up:** `1000` / `50` per pattern.
- **Phases, each timed with its own `Instant` clock:**
  1. **PGEN parse** — `rgx_core::parsing::parse_pattern(pattern)`
  2. **AST → bytecode + C2 build** — `Compiler::new().compile_ast(ast)`
  3. **Engine construction** — `Engine::new(&compiled)` (DFA caches + JIT)
  - plus an **independent** end-to-end `Regex::compile` measurement as a control (the phase-by-phase sum carries extra timer overhead, so the control is the honest total).
- **Statistics:** median over the 1000 samples, plus mean and percentiles; each phase is reported as a percentage of the independent full-compile control.
- **Caveat (stated in the tool itself):** this is a phase-*split* decision tool, not an absolute-perf claim. Absolute compile/parse numbers come from Axis 1; absolute runtime numbers come from Axis 4 (criterion).

## Axis 3 — the PCRE2 baseline (the denominator)

The compile-time gap is always expressed as a ratio against PCRE2 10.47. Two
PCRE2 baselines are measured so the comparison is honest about JIT.

- **Tools:** `pgen_iteration_flow/pcre2_compile_baseline.c` (no JIT) and `pgen_iteration_flow/pcre2_compile_jit_baseline.c` (`pcre2_compile()` + `pcre2_jit_compile(PCRE2_JIT_COMPLETE)`).
- **Dependency:** `libpcre2-8` from the system package manager (`brew install pcre2` on macOS) — PCRE2 **10.47**, the same release tracked by the `subs/pcre2` submodule.
- **Method:** `clock_gettime(CLOCK_MONOTONIC)` around a tight loop of `BATCH = 10000` `pcre2_compile()` calls; reported value = `total_ns / BATCH` (batch **mean**), printed as `ns/compile`.
- **Cross-check:** `pgen_iteration_flow/pgen_pcre2_compile_ratio.rs` is a self-contained Rust microbench that times the [`pcre2` crate](https://crates.io/crates/pcre2) compile and PGEN parse in one process and prints the ratio table directly. The C and Rust PCRE2 numbers must agree within ±10 %; a larger divergence flags Rust-binding overhead and is called out.

### A deliberate statistical asymmetry, and how to neutralize it

The PGEN side reports **p50 of 5000 single-call samples**; the PCRE2 C side
reports the **mean of a 10000-call batch**. This is acceptable because PCRE2
compile is low-variance (p50 ≈ mean to within noise), but it is *not* strictly
apples-to-apples. The bundled Rust microbench removes the asymmetry by
computing **p50 on both sides in the same process** — that is the number to
cite when the asymmetry could matter. Always state which estimator produced a
quoted ratio.

## Axis 4 — runtime match speed (post-compile throughput)

A separate axis from compile time, and **independent of the PGEN pin** (matching
runs on RGX's own VM/DFA/JIT engine, not PGEN).

- **Tool:** the `rgx-bench` crate (Criterion) via `scripts/capture-benchmark-trends.sh` → `rgx-bench/src/bin/trend_capture.rs`.
- **Comparator:** the `pcre2` crate (`pcre2::bytes::Regex`) compiled and matched in the same harness, so RGX and PCRE2 run identical inputs.
- **Kinds:** `find_first`, `find_all`, and `compile`, over the corpus at 1 K and 10 K input sizes.
- **Modes:** `quick` (default; low-overhead, used in every local CI pass) and `full` (`--profile bench`; higher-fidelity Criterion sampling for release-grade numbers). Use `full` for any number that goes in the book.
- **Artifacts:** `target/benchmark-trends/latest.md` + label-tagged rolling history under `target/benchmark-trends/history/`; each capture records the git label and the comparison baseline so longitudinal deltas are reproducible. (`target/` is git-ignored — runtime captures are local; only curated summaries land in the book.)

## The ratio computation

- Per pattern: `ratio = pgen_parse_p50 / pcre2_compile` (separately for no-JIT and +JIT PCRE2).
- Headline: the **geometric mean** of the per-pattern ratios (geomean, not arithmetic mean — ratios compose multiplicatively and the corpus spans a wide latency range).
- Every quoted figure is tagged with: the `subs/pgen` commit, PGEN release + integration-contract version (`parser_embedding_api_contract()`), host CPU, allocator, and date. A ratio without that provenance is not citable.

## Environment controls

These are mandatory. Changing any of them changes the number materially.

- **Build profile:** `--release`. Never measure a debug build.
- **Allocator:** the **default system allocator** (macOS `libmalloc`). RGX does **not** measure with `mimalloc`. This is the single biggest methodology lever (see below).
- **Single measurement process, single thread.** No parallel benchmarking; the per-call clock is sensitive to scheduler contention.
- **Machine quiescence — required.** The host must be otherwise idle. Benchmarks run under CPU contention are invalid, not merely noisy (see [Known failure mode](#known-failure-mode-load-contamination)).
- **Provenance recorded** with every run (pin, versions, CPU, allocator, date) — emitted automatically by `pgen_compile_perf_dump.rs` into the bundle.

### Reconciling ~214× vs ~80×

Older notes quoted "~80× slower than PCRE2-no-JIT". RGX's standard
measurement on PGEN 1.1.81 (pin `db6f8c68`, 2026-05-19) is **≈214×**. Both can
be correct *for their methodology*:

- The "~80×" came from **PGEN's own benchmark harness with mimalloc** and PGEN's internal sampling. mimalloc materially speeds up an allocation-heavy parser.
- The "≈214×" is RGX's standard measurement: **default system allocator**, the embedding-API entry point, p50 of 5000 samples, same-session PCRE2 C baseline.

Neither is "wrong"; they measure different configurations. **The actionable
ask for PGEN:** publish the integration-side number under **both** allocators
(default *and* mimalloc) using the corpus and entry point above, so the
RGX-integration ratio (the one the `<5×` ROADMAP target is defined against) is
unambiguous. The integration closure criterion is defined on the
default-allocator number, because that is what RGX ships.

## Known failure mode: load contamination

On 2026-05-19 a quick runtime capture ran while the host was under heavy
unrelated load (load average ≈5.6; long-running `cargo-mutants` and
multi-day batch jobs). Symptoms of an invalid run, all of which were present:

- Absolute timings inflated **non-uniformly**: RGX `find_all` literal 1 K went 1,516 ns → 18,620 ns (~12×) while PCRE2 went 4,855 ns → 8,228 ns (~1.7×).
- The ratio swung wildly ("624 % regression") with **no corresponding code change**.
- The PCRE2 baseline — which is version- and hardware-stable — itself degraded, the tell-tale sign that the measurement environment, not the engine, moved.

That capture was **discarded, not published**. Rule: if the PCRE2 baseline
for a known pattern differs from a quiet-machine baseline by more than ~20 %,
the whole run is contaminated — discard it and re-measure on an idle host.
Prefer `full` mode for anything destined for the book.

## Reproduce it yourself

RGX side, from the repo root:

```text
# Axis 1 — PGEN parse p50 (5000 samples) + self-describing bundle
cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser

# Axis 2 — compile phase split (PGEN parse share of Regex::compile)
cargo run --release -p rgx-core --example compile_phase_split --features pgen-parser

# Axis 3 — PCRE2 10.47 baselines (no-JIT and +JIT)
P=$(brew --prefix pcre2)
cc -O2 -I"$P/include" pgen-issues/artifacts/PGEN-RGX-0078/pgen_iteration_flow/pcre2_compile_baseline.c     -o /tmp/pcre2_nojit -L"$P/lib" -lpcre2-8 && /tmp/pcre2_nojit
cc -O2 -I"$P/include" pgen-issues/artifacts/PGEN-RGX-0078/pgen_iteration_flow/pcre2_compile_jit_baseline.c -o /tmp/pcre2_jit   -L"$P/lib" -lpcre2-8 && /tmp/pcre2_jit

# Axis 4 — runtime match speed (use full mode for book-grade numbers)
RGX_BENCHMARK_TREND_MODE=full ./scripts/capture-benchmark-trends.sh
```

## The vendorable PGEN bundle

`pgen-issues/artifacts/PGEN-RGX-0078/pgen_iteration_flow/` is a self-contained
copy of this methodology with **no RGX dependency**, designed to be vendored
into PGEN's tree and wired as a standard release gate:

| File | Role |
|---|---|
| `patterns.tsv` | the fixed 8-pattern corpus |
| `pcre2_compile_baseline.c` | PCRE2 no-JIT compile baseline (Axis 3) |
| `pcre2_compile_jit_baseline.c` | PCRE2 +JIT compile baseline (Axis 3) |
| `pgen_pcre2_compile_ratio.rs` | self-contained Rust microbench: PGEN parse p50 vs `pcre2`-crate compile p50, prints the ratio table + the `<5×` closure-status line |
| `run_perf_gate.sh` | one-shot driver: builds + runs all three and prints a unified ratio table; runs from inside PGEN's `rust/` with only `libpcre2-8` + the `pcre2` crate as external deps |
| `Cargo.toml.snippet` / `Makefile.snippet` | exactly what to add to PGEN's `rust/Cargo.toml` and `rust/Makefile` to expose the gate as a make target |
| `README.md` | the end-to-end recipe and the two integration options (Rust-only, or Rust + C cross-validated) |

PGEN can adopt this so the PCRE2-relative compile-time number is produced on
every PGEN release with the *same corpus, entry point, statistic, and
environment controls* RGX uses — which is the entire point: the ratio is only
meaningful if both sides measure identically. The integration-side closure
criterion (`PGEN-RGX-0078`) is **geomean PGEN-parse / PCRE2-no-JIT-compile
< 5×**, default allocator.

## Where the numbers live

- Current measured numbers: [Performance → Compile-time performance](./performance.md#compile-time-performance) (kept current; the canonical table).
- Tracking + history: `docs/BACKLOG.md` (C10, PGEN-RGX-0073), `pgen-issues/PGEN-RGX-0078.yaml` (`measurement_refresh` block), `RUST_CODEBASE_ANALYSIS.md`, `ROADMAP.md` (`<5×` compile entry).
- This chapter is the methodology of record; the numbers chapters cite it.
