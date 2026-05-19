# PGEN-RGX-0078 fresh measurement — adopted pin db6f8c68 (PGEN release 1.1.81 / contract 1.1.83)
# Date: 2026-05-19  Host: Darwin arm64  Apple Silicon  default allocator
# PGEN parse: rgx-core/examples/pgen_compile_perf_dump.rs (5000 samples / 200 warmup, p50)
# PCRE2: pgen_iteration_flow/pcre2_compile_baseline.c + _jit_baseline.c vs homebrew libpcre2-8 10.47 (10000-compile batch mean)
#
pattern              PGENp50_ns  PCRE2nojit_ns   PCRE2jit_ns   x_nojit    x_jit
literal_simple            24125            182          1590      133x    15.2x
digit_sequence            60375            286          2149      211x    28.1x
character_class           94417            623          2779      152x    34.0x
alternation               59750            300          1926      199x    31.0x
capture_groups           124083            446          2797      278x    44.4x
url_simple                57583            273          2206      211x    26.1x
email_basic               82041            285          2531      288x    32.4x
anchor_complex           187625            608          3124      309x    60.1x

GEOMEAN PGEN/PCRE2-no-JIT = 214x   PGEN/PCRE2+JIT = 31.7x
Prior (PGEN 1.1.40, 0078 filing): ~360x no-JIT / ~85x +JIT. Raw PGEN parse p50 now ~2.0-3.8x faster than the 1.1.40-era committed baseline.
