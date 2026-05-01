// Batched PCRE2 compile bench — measures total time for N compiles
// to get sub-µs resolution.
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
        pcre2_code *re = pcre2_compile((PCRE2_SPTR)pat, strlen(pat), 0, &errcode, &erroffset, NULL);
        if (re) pcre2_code_free(re);
    }
    clock_gettime(CLOCK_MONOTONIC, &t1);
    long long ns = (t1.tv_sec - t0.tv_sec) * 1000000000LL + (t1.tv_nsec - t0.tv_nsec);
    long long avg = ns / BATCH;
    printf("%-22s %10lld ns/compile  (%d-compile batch took %lld ns)\n", name, avg, BATCH, ns);
}

int main() {
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
