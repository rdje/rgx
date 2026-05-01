// Standalone PCRE2 compile-time baseline WITH JIT — measures total
// time for pcre2_compile() PLUS pcre2_jit_compile(PCRE2_JIT_COMPLETE)
// over an N-compile batch, sub-µs resolution. Companion to
// pcre2_compile_baseline.c (which measures pcre2_compile() alone, no
// JIT). Together they bracket PCRE2's compile-time spectrum:
//   - pcre2_compile_baseline.c       : parse + bytecode codegen + meta
//   - pcre2_compile_jit_baseline.c   : the above + JIT codegen (full)
//
// Build:
//   cc -O2 -lpcre2-8 -o pcre2_compile_jit_baseline pcre2_compile_jit_baseline.c
//
// Run:
//   ./pcre2_compile_jit_baseline
//
// Both baselines together let PGEN make a fair "compile-time" claim
// against either PCRE2 compile mode. The PGEN-RGX-0078 report's
// PRIMARY closure target is the non-JIT comparison (PGEN parse-only
// vs PCRE2 parse+codegen+meta) since neither side involves JIT and
// the work is structurally analogous. The JIT-enabled column is
// supplied for transparency — PGEN parse is still much slower than
// PCRE2 even with JIT included.
#define PCRE2_CODE_UNIT_WIDTH 8
#include <pcre2.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define BATCH 10000

static void bench_one(const char *name, const char *pat) {
    int errcode;
    PCRE2_SIZE erroffset;
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (int i = 0; i < BATCH; i++) {
        pcre2_code *re = pcre2_compile(
            (PCRE2_SPTR)pat, strlen(pat), 0, &errcode, &erroffset, NULL
        );
        if (re) {
            // PCRE2_JIT_COMPLETE — full JIT codegen for find_first +
            // find_all match modes. Mirrors what RGX measures against
            // when the C1 JIT path is enabled.
            (void)pcre2_jit_compile(re, PCRE2_JIT_COMPLETE);
            pcre2_code_free(re);
        }
    }
    clock_gettime(CLOCK_MONOTONIC, &t1);
    long long ns = (t1.tv_sec - t0.tv_sec) * 1000000000LL + (t1.tv_nsec - t0.tv_nsec);
    long long avg = ns / BATCH;
    printf("%-22s %10lld ns/compile+jit  (%d-batch took %lld ns)\n", name, avg, BATCH, ns);
}

int main(void) {
    bench_one("literal_simple", "test");
    bench_one("digit_sequence", "\\d{3}-\\d{2}-\\d{4}");
    bench_one("character_class", "[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}");
    bench_one("alternation", "cat|dog|bird");
    bench_one("capture_groups", "(\\d{4})-(\\d{2})-(\\d{2})");
    bench_one("url_simple", "https?://\\S+");
    bench_one("email_basic", "\\b\\w+@\\w+\\.\\w+\\b");
    bench_one("anchor_complex", "^(\\d+)\\s+(?P<word>\\w+)\\s+(?:foo|bar)$");
    return 0;
}
