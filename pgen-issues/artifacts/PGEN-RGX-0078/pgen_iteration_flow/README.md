# PGEN-RGX-0078 iteration flow — PGEN-only compile-time perf gate

This directory contains everything PGEN needs to **iterate on the
compile-time gap independently of RGX**. Vendor these files into
PGEN's tree, wire them into the existing Make + Cargo machinery, and
the regression-gate runs as a standard PGEN release-gate target.
No RGX dependency.

## What's in this directory

| File | Purpose |
|---|---|
| `pcre2_compile_baseline.c` | Standalone C bench: pure `pcre2_compile()` p50, no JIT. Depends only on libpcre2-8. |
| `pcre2_compile_jit_baseline.c` | Standalone C bench: `pcre2_compile()` + `pcre2_jit_compile(PCRE2_JIT_COMPLETE)` p50. Same dependency. |
| `pgen_pcre2_compile_ratio.rs` | Self-contained Rust microbench: PGEN parse p50 vs `pcre2` Rust crate compile p50, prints the ratio table directly. |
| `patterns.tsv` | 8-pattern corpus, stable across PGEN-RGX-0073/0078. |
| `Cargo.toml.snippet` | What to add to PGEN's `rust/Cargo.toml` to enable the Rust microbench. |
| `Makefile.snippet` | What to add to PGEN's `rust/Makefile` to expose `regex_pcre2_compile_perf_gate` as a standard make target. |
| `run_perf_gate.sh` | One-shot bash driver: builds + runs both baselines + Rust microbench, writes a combined ratio table. |
| `README.md` | This file. |

## Two ways PGEN can integrate this

### Option A — minimal (just the Rust microbench)

Drop `pgen_pcre2_compile_ratio.rs` into `pgen/rust/examples/`. Add
the `[[example]]` + `[dev-dependencies] pcre2 = "0.2"` entries from
`Cargo.toml.snippet` to `pgen/rust/Cargo.toml`. Then:

```bash
cargo run --release --features generated_parsers \
    --example pgen_pcre2_compile_ratio
```

Output is a per-pattern ratio table + geomean. Threshold check at the
end ("ROADMAP target: <5x") is hard-coded; flip to a CI-failing exit
status if you want CI integration.

This option uses the [`pcre2` Rust crate](https://crates.io/crates/pcre2)
as the PCRE2 binding. The crate is widely deployed, depends only on
libpcre2-8 from the system package manager, and produces numbers that
agree with the C baseline within noise (<5% delta in our cross-check).
**No new C build infrastructure required.**

### Option B — most-honest (Rust + C side-by-side cross-validation)

Keep both the Rust microbench (Option A) AND the standalone C baselines
(`pcre2_compile_baseline.c` + `pcre2_compile_jit_baseline.c`). The two
should agree within ±10% on PCRE2 timings; if they diverge significantly,
that flags a Rust-binding overhead concern (the `pcre2` crate adds a
thin Rust wrapper, allocates a `Vec` for captures, etc.). PGEN's
release-notes p50s should cite both numbers when the divergence
matters.

`run_perf_gate.sh` builds + runs all three (C-no-JIT + C-JIT + Rust-microbench)
and writes a combined ratio table. Simplest CI form:

```bash
./pgen_iteration_flow/run_perf_gate.sh > release_perf_gate.txt
```

## Cargo.toml.snippet

```toml
# Add to pgen/rust/Cargo.toml after the existing `[[example]]` blocks.

[[example]]
name = "pgen_pcre2_compile_ratio"
path = "examples/pgen_pcre2_compile_ratio.rs"
required-features = ["generated_parsers"]

[dev-dependencies]
# Adds the libpcre2-8 Rust binding for the compile-ratio bench. Used
# only by the bench example; production code is unaffected. The crate
# transitively links libpcre2-8 from the system package manager (brew,
# apt, dnf etc.), so build hosts need libpcre2-8 installed.
pcre2 = "0.2"
```

## Makefile.snippet

```makefile
# Add to pgen/rust/Makefile.

PCRE2_PREFIX ?= $(shell brew --prefix pcre2 2>/dev/null || echo /usr/local)

.PHONY: regex_pcre2_compile_perf_gate
regex_pcre2_compile_perf_gate:
	@echo "🧱 Running regex compile-time perf gate (PGEN parse vs PCRE2 compile)..."
	cd $(RUST_DIR) && cargo run --release \
	    --features generated_parsers \
	    --example pgen_pcre2_compile_ratio
	@echo "✅ regex compile-time perf gate completed."

# Optional: cross-validate against standalone C baselines.
.PHONY: regex_pcre2_compile_perf_gate_cross_check
regex_pcre2_compile_perf_gate_cross_check:
	@echo "🧪 Building + running C-side PCRE2 baselines (cross-check)..."
	cc -O2 -I$(PCRE2_PREFIX)/include -L$(PCRE2_PREFIX)/lib \
	    -o $(RUST_DIR)/target/pcre2_compile_baseline \
	    perf/pcre2_compile_baseline.c -lpcre2-8
	cc -O2 -I$(PCRE2_PREFIX)/include -L$(PCRE2_PREFIX)/lib \
	    -o $(RUST_DIR)/target/pcre2_compile_jit_baseline \
	    perf/pcre2_compile_jit_baseline.c -lpcre2-8
	@echo "=== PCRE2 compile (no JIT) ==="
	@$(RUST_DIR)/target/pcre2_compile_baseline
	@echo "=== PCRE2 compile + JIT ==="
	@$(RUST_DIR)/target/pcre2_compile_jit_baseline

.PHONY: regex_pcre2_compile_perf_gate_full
regex_pcre2_compile_perf_gate_full: regex_pcre2_compile_perf_gate regex_pcre2_compile_perf_gate_cross_check
```

(Adapt `RUST_DIR` and the `perf/` source path to match PGEN's tree
layout. Suggested vendoring: drop the `.c` files into `pgen/perf/`.)

## run_perf_gate.sh

A driver bash script is included alongside this README. It builds
both C baselines + the Rust microbench, runs them, and writes a
combined ratio table to stdout (and `release_perf_gate.txt` if
redirected). Single command:

```bash
bash pgen_iteration_flow/run_perf_gate.sh
```

## Methodology — closure criteria for PGEN-RGX-0078

Per the YAML's resolution block, closure requires geomean PGEN/PCRE2
ratio under 5x, measured on the same host with the same build profile
on both sides. With current `1.1.40` numbers (geomean ~360x), closure
is far away; the path forward likely involves specialised codegen for
the regex grammar (PGEN's tracker mentions this as "long-term") rather
than constant-factor wins on the general-purpose EBNF-driven codegen.

PGEN can publish its release-note p50s against the **same metric** the
RGX integration measures against — geomean PGEN/PCRE2 ratio — by
running this gate at every release. That replaces the current
"PRIMARY <50µs" target which is internal to PGEN and doesn't reflect
the integration-side reality.

## Cross-validation reference numbers

These are the live numbers from the RGX-side measurement (PGEN pin
`056f6784`, Apple M-series, macOS, system allocator, `cargo run --release`,
5000 samples / 200 warmup) that PGEN can use to verify a fresh
PGEN-side run reproduces:

```
                           PGEN parse p50  PCRE2 no-JIT p50  PCRE2 +JIT p50
literal_simple                  92,042 ns           346 ns       2,305 ns
digit_sequence                 216,833 ns           682 ns       2,483 ns
character_class                266,584 ns         1,156 ns       2,799 ns
alternation                    119,709 ns           463 ns       1,896 ns
capture_groups                 265,000 ns           685 ns       2,468 ns
url_simple                     196,292 ns           370 ns       1,776 ns
email_basic                    213,292 ns           389 ns       2,372 ns
anchor_complex                 377,209 ns           766 ns       3,079 ns

Geomean PGEN/PCRE2-noJIT: ~360x
Geomean PGEN/PCRE2+JIT:   ~85x
```

A PGEN-side run on a different host with `mimalloc` enabled will
likely show smaller absolute numbers (PGEN's `1.1.30` release-note
table claimed 13µs-76µs PGEN parse), but **the ratio against PCRE2
compiled on the same host with the same allocator should be the
robust metric**. That's what closure is measured against.
