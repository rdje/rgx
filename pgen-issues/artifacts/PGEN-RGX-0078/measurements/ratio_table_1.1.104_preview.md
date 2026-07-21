# PGEN-RGX-0078 verification measurement — PREVIEW pin 960dddaa (PGEN release 1.1.105 / contract 1.1.108; embedding constants report 1.1.104/1.1.106 — see PGEN-RGX-0089/0090/0091)
# Date: 2026-07-21  Host: Darwin arm64 Apple Silicon  default allocator, standard release profile
# NOT the shipped pin: RGX remains pinned at db6f8c68 (rel 1.1.81) — adoption blocked by PGEN-RGX-0089 (+0090/0091).
# PGEN parse: rgx-core/examples/pgen_compile_perf_dump.rs (5000 samples / 200 warmup, p50), raw embedding parse.
# PCRE2: pgen_iteration_flow C baselines vs homebrew libpcre2-8 10.47 (10000-compile batch mean), same session.
#
pattern              PGENp50_ns  PCRE2nojit_ns  PCRE2jit_ns  x_nojit  x_jit  parse_speedup_vs_1.1.81
literal_simple             1083            139         1561     7.8x   0.7x    22.3x
digit_sequence             2125            273         2158     7.8x   1.0x    28.4x
character_class           10333            602         2806    17.2x   3.7x     9.1x
alternation                2000            288         1977     6.9x   1.0x    29.9x
capture_groups             4708            437         2869    10.8x   1.6x    26.4x
url_simple                 1625            263         2056     6.2x   0.8x    35.4x
email_basic                1625            279         2627     5.8x   0.6x    50.5x
anchor_complex             5500            596         3092     9.2x   1.8x    34.1x

GEOMEAN raw PGEN parse = 2,748 ns (was ~74,000 ns geomean on db6f8c68 → 26.9x faster)
GEOMEAN PGEN/PCRE2-no-JIT = 8.44x   (was ≈214x)     — the <5x line is NOT met on this instrument
GEOMEAN PGEN/PCRE2+JIT    = 1.17x   (was ≈32x)      — parity with PCRE2's JIT-enabled compile

Cross-reference: PGEN's own release-day run of the vendored gate (their host) reads 6.6x on the
embedding-path instrument and ≈3.9x on the direct parse path under fat-LTO + mimalloc (their
closure configuration); PGEN closed the 0078 campaign against a director-re-based ABSOLUTE bar
(sub-1µs corpus geomean, met at 1,004.4 ns) rather than the <5x relative line. See
PGEN_RELEASED_PARSER_BUG_LEDGER.md row REGEX-0078 and the contract's 2026-07-21 maintenance entries.

# Compile-phase inversion (phase_split_1.1.104_preview.txt, same session):
# full Regex::compile geomean ≈ 42.4 µs → ≈130x PCRE2-no-JIT. Raw PGEN parse is now only ~4-18%
# of full compile. The RGX-side adapter boundary (AST-dump JSON serialize + serde deserialize +
# typed-walker conversion, ~10-25 µs/pattern — the "pgen phase" measures 16-35 µs vs 1-10 µs raw
# parse) and the eager C2 program build inside compile (~12-36 µs, "ast phase") are now the
# dominant costs. The old deferral rationale for RGX-side compile work (PGEN-parse dominance)
# INVERTS once this pin (or a fixed successor) is adopted.
