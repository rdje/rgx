/*
 * rgx-capi Phase 1 smoke test.
 *
 * Exercises the Phase 1 API surface end-to-end from C:
 *   - compile / free
 *   - retain
 *   - is_match
 *   - find_first
 *   - error reporting on a bad pattern
 *
 * Compiled and linked by the harness in tests/c_smoke_test.rs.
 * Exits 0 on success; non-zero on failure with a diagnostic on
 * stderr.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "rgx.h"

#define ASSERT(cond, msg)                                                      \
    do {                                                                       \
        if (!(cond)) {                                                         \
            fprintf(stderr, "FAIL [%s:%d]: %s\n", __FILE__, __LINE__, msg);    \
            return 1;                                                          \
        }                                                                      \
    } while (0)

int test_version(void) {
    /* Just check the version functions are callable and return
     * non-zero (we don't pin specific version values — those come
     * from Cargo.toml). */
    uint32_t maj = rgx_runtime_version_major();
    uint32_t min = rgx_runtime_version_minor();
    uint32_t pat = rgx_runtime_version_patch();
    /* Use the values to keep the compiler from warning. */
    (void)maj; (void)min; (void)pat;
    return 0;
}

int test_compile_and_free(void) {
    RgxRegex* re = NULL;
    const char* pat = "\\d+";
    int32_t rc = rgx_compile((const uint8_t*)pat, strlen(pat), &re);
    ASSERT(rc == RGX_OK, "compile of \\d+ should succeed");
    ASSERT(re != NULL, "compile must populate out_regex");
    rgx_regex_free(re);
    return 0;
}

int test_compile_invalid_pattern(void) {
    RgxRegex* re = NULL;
    const char* bad = "(\\d+"; /* unbalanced paren */
    int32_t rc = rgx_compile((const uint8_t*)bad, strlen(bad), &re);
    ASSERT(rc == RGX_ERR_INVALID_PATTERN,
           "compile of unbalanced paren should fail with INVALID_PATTERN");
    ASSERT(re == NULL, "failed compile must NOT populate out_regex");
    const char* err = rgx_last_error();
    ASSERT(err != NULL, "rgx_last_error must return non-null");
    ASSERT(strlen(err) > 0, "error message should be non-empty after failure");
    return 0;
}

int test_is_match(void) {
    RgxRegex* re = NULL;
    const char* pat = "\\d+";
    int32_t rc = rgx_compile((const uint8_t*)pat, strlen(pat), &re);
    ASSERT(rc == RGX_OK, "compile must succeed");

    /* Match present. */
    const char* text1 = "abc 123 def";
    int32_t matched = 0;
    rc = rgx_is_match(re, (const uint8_t*)text1, strlen(text1), &matched);
    ASSERT(rc == RGX_OK, "is_match must succeed");
    ASSERT(matched == 1, "expected match in 'abc 123 def'");

    /* No match. */
    const char* text2 = "abc def";
    matched = 99;
    rc = rgx_is_match(re, (const uint8_t*)text2, strlen(text2), &matched);
    ASSERT(rc == RGX_OK, "is_match (no match) must succeed");
    ASSERT(matched == 0, "expected no match in 'abc def'");

    rgx_regex_free(re);
    return 0;
}

int test_find_first(void) {
    RgxRegex* re = NULL;
    const char* pat = "\\d+";
    int32_t rc = rgx_compile((const uint8_t*)pat, strlen(pat), &re);
    ASSERT(rc == RGX_OK, "compile must succeed");

    const char* text = "abc 123 def";
    int32_t matched = 0;
    size_t start = 99, end = 99;
    rc = rgx_find_first(re, (const uint8_t*)text, strlen(text),
                        &matched, &start, &end);
    ASSERT(rc == RGX_OK, "find_first must succeed");
    ASSERT(matched == 1, "expected match");
    ASSERT(start == 4, "expected start == 4");
    ASSERT(end == 7, "expected end == 7");

    rgx_regex_free(re);
    return 0;
}

int test_retain_creates_independent_handle(void) {
    RgxRegex* re = NULL;
    const char* pat = "[a-z]+";
    int32_t rc = rgx_compile((const uint8_t*)pat, strlen(pat), &re);
    ASSERT(rc == RGX_OK, "compile must succeed");

    RgxRegex* retained = rgx_regex_retain(re);
    ASSERT(retained != NULL, "retain must return non-null");
    ASSERT(retained != re, "retain must return a fresh handle");

    /* Both handles must be usable. */
    int32_t m1 = 0, m2 = 0;
    const char* text = "hello";
    rc = rgx_is_match(re, (const uint8_t*)text, strlen(text), &m1);
    ASSERT(rc == RGX_OK && m1 == 1, "original handle works");
    rc = rgx_is_match(retained, (const uint8_t*)text, strlen(text), &m2);
    ASSERT(rc == RGX_OK && m2 == 1, "retained handle works");

    /* Free both independently. */
    rgx_regex_free(re);
    rgx_regex_free(retained);
    return 0;
}

int test_free_null_is_noop(void) {
    /* Should not crash. */
    rgx_regex_free(NULL);
    return 0;
}

int test_null_arguments_rejected(void) {
    RgxRegex* re = NULL;

    /* null pattern. */
    int32_t rc = rgx_compile(NULL, 0, &re);
    ASSERT(rc == RGX_ERR_NULL_POINTER, "null pattern must be rejected");

    /* null out_regex. */
    const char* pat = "\\d+";
    rc = rgx_compile((const uint8_t*)pat, strlen(pat), NULL);
    ASSERT(rc == RGX_ERR_NULL_POINTER, "null out_regex must be rejected");

    return 0;
}

int main(void) {
    int failures = 0;
    int (*tests[])(void) = {
        test_version,
        test_compile_and_free,
        test_compile_invalid_pattern,
        test_is_match,
        test_find_first,
        test_retain_creates_independent_handle,
        test_free_null_is_noop,
        test_null_arguments_rejected,
    };
    size_t n = sizeof(tests) / sizeof(tests[0]);
    for (size_t i = 0; i < n; ++i) {
        if (tests[i]() != 0) {
            ++failures;
        }
    }
    if (failures) {
        fprintf(stderr, "%d/%zu test(s) failed\n", failures, n);
        return 1;
    }
    fprintf(stdout, "all %zu rgx-capi C smoke tests passed\n", n);
    return 0;
}
