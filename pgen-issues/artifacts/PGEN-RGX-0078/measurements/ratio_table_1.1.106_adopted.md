# PGEN 1.1.106 (ADOPTED pin) — regex parse vs PCRE2 10.47 compile

Measured 2026-07-21 on the adopted pin (release 1.1.106 / contract 1.1.109).
RGX standard instrument: `cargo run --release -p rgx-core --example pgen_compile_perf_dump`,
default allocator, p50 of 5000 samples, 200 warmup discarded. PCRE2 baselines are the
same-session C measurements in `pcre2_compile_p50_2026-07-21_session.txt` / `..._jit_...`.

| pattern | PGEN parse p50 | PCRE2 compile (no JIT) | ratio | PCRE2 compile+JIT | ratio |
|---|---|---|---|---|---|
| `literal_simple` | 1083 ns | 139 ns | 7.79x | 1561 ns | 0.69x |
| `digit_sequence` | 2167 ns | 273 ns | 7.94x | 2158 ns | 1.00x |
| `character_class` | 10750 ns | 602 ns | 17.86x | 2806 ns | 3.83x |
| `alternation` | 2042 ns | 288 ns | 7.09x | 1977 ns | 1.03x |
| `capture_groups` | 4833 ns | 437 ns | 11.06x | 2869 ns | 1.68x |
| `url_simple` | 1666 ns | 263 ns | 6.33x | 2056 ns | 0.81x |
| `email_basic` | 1666 ns | 279 ns | 5.97x | 2627 ns | 0.63x |
| `anchor_complex` | 5667 ns | 596 ns | 9.51x | 3092 ns | 1.83x |
| **geomean** | **2813 ns** | 325 ns | **8.64x** | 2339 ns | **1.20x** |

- vs the previously-shipped pin 1.1.81 (~74,000 ns geomean): **26.3x faster**.
- Reproduces the 1.1.104 preview measurement (2,748 ns geomean) within +2.4%.
- 3/8 patterns parse faster than PCRE2 *compiles with JIT*.
- The ROADMAP's `<5x`-vs-PCRE2-no-JIT bar is still NOT met at the raw-parse layer (8.64x),
  and `Regex::compile` as a whole is now dominated by RGX-side phases — see
  `COMPILE-PERF-0078.3` (adapter boundary) and `.4` (eager C2 build).
