# PGEN regex parse vs PCRE2 compile — ratio table

## Headline numbers

**PGEN parse alone** (no codegen, no JIT) is **~360x slower than PCRE2's no-JIT
full compile** (parse + bytecode codegen + match metadata) and **~85x slower
than PCRE2's full JIT-enabled compile** (parse + bytecode codegen + meta +
JIT codegen for match modes), geomean across the 8-pattern bench corpus.

This is the comparison `PGEN-RGX-0078` was filed for. The closure target per
RGX's ROADMAP is **<5x of PCRE2 compile**; the live measurement is 50-100x
above that target, both with and without PCRE2 JIT inclusion.

## Methodology — both sides

- Host: Apple M-series, macOS Darwin Kernel 25.4.0 (arm64).
- Build profile: `--release` with default Cargo settings (no LTO override
  beyond Cargo's release default; no thin-LTO / fat-LTO; no
  `mimalloc`/`jemalloc` override; system allocator = macOS libmalloc).
- PGEN parse measurement: in-process `Instant::now()` deltas around
  `pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pat)`,
  5000 samples per pattern, 200-sample warmup discarded. Ran via the
  in-tree helper `rgx-core/examples/pgen_compile_perf_dump.rs`.
- PCRE2 compile measurement: standalone C bench
  `pgen_iteration_flow/pcre2_compile_baseline.c` linked against
  Homebrew `libpcre2-8` (PCRE2 10.47-derived release). 10000-compile
  batch, total wall-clock divided by batch size for sub-µs resolution.
- PCRE2 compile+JIT measurement: standalone C bench
  `pgen_iteration_flow/pcre2_compile_jit_baseline.c`, identical batch
  shape, calls `pcre2_compile()` followed by
  `pcre2_jit_compile(re, PCRE2_JIT_COMPLETE)` inside the timed loop.
- PGEN side does NOT involve any JIT or codegen — `parse_grammar_profile_named`
  returns a `ParseOutcome` containing the AST envelope; nothing more.
- PCRE2 side's "no JIT" column is `pcre2_compile()` alone — that's the
  closest structural analogue to PGEN parse (parse + bytecode codegen +
  match metadata, no JIT codegen). The "JIT" column adds
  `pcre2_jit_compile()` and is supplied for transparency; PGEN parse is
  still much slower than PCRE2-with-JIT.

## Comparison table — PGEN pin `056f6784` (1.1.40), 2026-05-01

| Pattern | PGEN parse p50 | PCRE2 no-JIT p50 | PCRE2 +JIT p50 | PGEN / no-JIT | PGEN / +JIT |
|--------------------|---------------:|-----------------:|---------------:|--------------:|------------:|
| literal_simple     |      92,042 ns |          346 ns  |     2,305 ns   |       **266x** |      **40x** |
| digit_sequence     |     216,833 ns |          682 ns  |     2,483 ns   |       **318x** |      **87x** |
| character_class    |     266,584 ns |        1,156 ns  |     2,799 ns   |       **231x** |      **95x** |
| alternation        |     119,709 ns |          463 ns  |     1,896 ns   |       **259x** |      **63x** |
| capture_groups     |     265,000 ns |          685 ns  |     2,468 ns   |       **387x** |     **107x** |
| url_simple         |     196,292 ns |          370 ns  |     1,776 ns   |       **530x** |     **111x** |
| email_basic        |     213,292 ns |          389 ns  |     2,372 ns   |       **548x** |      **90x** |
| anchor_complex     |     377,209 ns |          766 ns  |     3,079 ns   |       **492x** |     **123x** |
| **geomean**        |   ~218,000 ns  |    ~605 ns       |    ~2,400 ns   |       **~360x** |      **~85x** |

## Reading the table

1. **PGEN parse is the slow path on both sides of the comparison.** Even
   compared to PCRE2 with JIT codegen INCLUDED (which is strictly more work
   than PGEN parse), PGEN parse is still 40-123x slower per pattern (geomean
   ~85x). When isolated to a parse-only-vs-compile-only comparison
   (no JIT on either side, structurally analogous), the gap is 231-548x
   (geomean ~360x).

2. **The ROADMAP closure target ("<5x of PCRE2 compile")** is far out of
   reach with the current PGEN architecture. Even at PGEN's claimed `1.1.30`
   release-note p50s (literal_simple 13µs through anchor_complex 76µs),
   the gap would still be 30-200x against PCRE2-no-JIT, never under 5x.

3. **The methodology gap between PGEN's release notes and these numbers**:
   PGEN's `1.1.30` release notes claimed 13-76µs p50 with mimalloc + 5000
   samples + 200 warmup. This bench uses default allocator + 5000 samples
   + 200 warmup. The 4-7x slowdown observed here vs PGEN's release-note
   p50s is almost certainly the allocator gap. Adopting `mimalloc` on both
   sides would close that — but **even with that closed, geomean would
   remain ~50x against PCRE2-no-JIT**, still 10x over the closure target.

4. **The bottleneck is structural, not constant-factor.** PCRE2 compiles
   faster than PGEN parses despite doing strictly more work (parsing AND
   bytecode codegen AND metadata layout). PGEN's general-purpose
   EBNF-driven codegen has overhead that the regex grammar exposes
   acutely. The path forward is either specialised codegen for the regex
   grammar (which PGEN's tracker mentions as "long-term") or a complete
   re-architecting of the PGEN parse path for grammars where parse-time
   matters.

## Variance / determinism note (per protocol §D)

- Slowdown is **corpus-wide**: every pattern in the 8-pattern corpus
  shows the same regime (231-548x non-JIT, 40-123x JIT-included).
- Slowdown is **deterministic**: re-running the bench reproduces p50s
  within ±10% across runs (typical for in-process Instant timing on
  macOS without TSC pinning).
- Slowdown is **size-correlated within the corpus** but only weakly
  — `literal_simple` (4 chars) and `anchor_complex` (~40 chars) span
  roughly 4x in PGEN parse time, while their PCRE2 compile times span
  only 2x. So PGEN parse-time grows faster with pattern complexity than
  PCRE2 compile time does.
- Single-sample variance is meaningfully higher on PGEN than on PCRE2
  due to the latter's smaller absolute time (allocator and timer noise
  dominates at sub-µs scales). PGEN's p50 / max ratios are on the order
  of 2-3x (occasional 1ms+ outliers in the long tail) whereas PCRE2's
  are tighter around 1.2-1.5x.

## Reproducing these numbers

End-to-end recipe:

```bash
# 1. Build + run PGEN-side parse measurement (writes
#    measurements/pgen_parse_p50.txt + per-pattern parse outcomes).
cargo run --release -p rgx-core --example pgen_compile_perf_dump

# 2. Build + run PCRE2-no-JIT baseline.
PCRE2_PREFIX="$(brew --prefix pcre2)"
cc -O2 -I"$PCRE2_PREFIX/include" -L"$PCRE2_PREFIX/lib" \
   -o /tmp/pcre2_compile_baseline \
   pgen-issues/artifacts/PGEN-RGX-0078/pgen_iteration_flow/pcre2_compile_baseline.c \
   -lpcre2-8
/tmp/pcre2_compile_baseline

# 3. Build + run PCRE2-JIT baseline.
cc -O2 -I"$PCRE2_PREFIX/include" -L"$PCRE2_PREFIX/lib" \
   -o /tmp/pcre2_compile_jit_baseline \
   pgen-issues/artifacts/PGEN-RGX-0078/pgen_iteration_flow/pcre2_compile_jit_baseline.c \
   -lpcre2-8
/tmp/pcre2_compile_jit_baseline
```

A self-contained PGEN-side equivalent (no RGX dependency) is documented
in `pgen_iteration_flow/README.md`.
