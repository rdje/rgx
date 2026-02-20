use pcre2::bytes::Regex as PcreRegex;
use rgx_core::Regex as RgxRegex;

struct ParityCase {
    name: &'static str,
    pattern: &'static str,
    input: &'static str,
}

struct KnownGapCase {
    name: &'static str,
    pattern: &'static str,
    input: &'static str,
    expected_rgx_error: &'static str,
}

fn rgx_first_span(pattern: &str, input: &str) -> Result<Option<(usize, usize)>, String> {
    let regex =
        RgxRegex::compile(pattern).map_err(|e| format!("rgx compile failed for '{pattern}': {e}"))?;
    Ok(regex.find_first(input).map(|m| (m.start, m.end)))
}

fn pcre2_first_span(pattern: &str, input: &str) -> Result<Option<(usize, usize)>, String> {
    let regex = PcreRegex::new(pattern)
        .map_err(|e| format!("pcre2 compile failed for '{pattern}': {e}"))?;
    let found = regex
        .find(input.as_bytes())
        .map_err(|e| format!("pcre2 find failed for '{pattern}': {e}"))?;
    Ok(found.map(|m| (m.start(), m.end())))
}

fn assert_known_gap_case(case: &KnownGapCase) {
    let rgx_err = match RgxRegex::compile(case.pattern) {
        Ok(_) => panic!(
            "[{}] rgx unexpectedly compiled known-gap pattern '{}'",
            case.name, case.pattern
        ),
        Err(err) => err,
    };
    assert!(
        rgx_err.to_string().contains(case.expected_rgx_error),
        "[{}] rgx error mismatch for pattern '{}': {}",
        case.name,
        case.pattern,
        rgx_err
    );

    let pcre2 = PcreRegex::new(case.pattern)
        .unwrap_or_else(|e| panic!("[{}] pcre2 compile failed: {e}", case.name));
    let matched = pcre2
        .find(case.input.as_bytes())
        .unwrap_or_else(|e| panic!("[{}] pcre2 execution failed: {e}", case.name));
    assert!(
        matched.is_some(),
        "[{}] pcre2 should execute known-gap pattern '{}' on input '{}'",
        case.name,
        case.pattern,
        case.input
    );
}

#[test]
fn pcre2_parity_supported_syntax_first_match_span() {
    let cases = [
        ParityCase {
            name: "literal",
            pattern: "cat",
            input: "xxcatyy",
        },
        ParityCase {
            name: "alternation",
            pattern: "cat|dog",
            input: "pet dog",
        },
        ParityCase {
            name: "digit_range",
            pattern: r"\d{2,3}",
            input: "id 1234",
        },
        ParityCase {
            name: "word_boundary",
            pattern: r"\bcat\b",
            input: "a cat nap",
        },
        ParityCase {
            name: "positive_lookahead",
            pattern: "(?=cat)c",
            input: "xxcat",
        },
        ParityCase {
            name: "negative_lookahead",
            pattern: "(?!cat)c",
            input: "cat",
        },
        ParityCase {
            name: "positive_lookbehind",
            pattern: "(?<=x)a",
            input: "xa",
        },
        ParityCase {
            name: "negative_lookbehind",
            pattern: "(?<!x)a",
            input: "ba",
        },
        ParityCase {
            name: "atomic_group_no_backtrack",
            pattern: "(?>a|ab)c",
            input: "abc",
        },
        ParityCase {
            name: "non_atomic_counterexample",
            pattern: "(a|ab)c",
            input: "abc",
        },
    ];

    for case in cases {
        let rgx = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx error: {e}", case.name));
        let pcre2 = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 error: {e}", case.name));
        assert_eq!(
            rgx, pcre2,
            "span mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }
}

#[test]
fn pcre2_parity_known_gap_backreference_compile_behavior() {
    assert_known_gap_case(&KnownGapCase {
        name: "backreference_basic",
        pattern: r"(a)\1",
        input: "aa",
        expected_rgx_error: "backreferences are parsed but not yet integrated into VM execution",
    });
}

#[test]
fn pcre2_parity_known_gap_recursion_compile_behavior() {
    let cases = [
        KnownGapCase {
            name: "recursion_entire_pattern",
            pattern: "a(?R)?b",
            input: "ab",
            expected_rgx_error: "recursion syntax is parsed but not yet integrated into VM execution",
        },
        KnownGapCase {
            name: "recursion_group_number",
            pattern: "(a(?1)?b)",
            input: "ab",
            expected_rgx_error: "recursion syntax is parsed but not yet integrated into VM execution",
        },
        KnownGapCase {
            name: "recursion_named_group",
            pattern: "(?<word>a(?&word)?b)",
            input: "ab",
            expected_rgx_error: "recursion syntax is parsed but not yet integrated into VM execution",
        },
    ];

    for case in cases {
        assert_known_gap_case(&case);
    }
}

#[test]
fn pcre2_parity_known_gap_conditional_compile_behavior() {
    let cases = [
        KnownGapCase {
            name: "conditional_group_exists",
            pattern: "(a)?(?(1)b|c)",
            input: "ab",
            expected_rgx_error: "conditional syntax is parsed but not yet integrated into VM execution",
        },
        KnownGapCase {
            name: "conditional_named_group_exists_angle_bracket",
            pattern: "(?<g>a)?(?(<g>)b|c)",
            input: "ab",
            expected_rgx_error: "conditional syntax is parsed but not yet integrated into VM execution",
        },
        KnownGapCase {
            name: "conditional_named_group_exists_bare",
            pattern: "(?<g>a)?(?(g)b|c)",
            input: "ab",
            expected_rgx_error: "conditional syntax is parsed but not yet integrated into VM execution",
        },
        KnownGapCase {
            name: "conditional_lookahead",
            pattern: "(?(?=ab)a|z)b",
            input: "ab",
            expected_rgx_error: "conditional syntax is parsed but not yet integrated into VM execution",
        },
        KnownGapCase {
            name: "conditional_negative_lookahead",
            pattern: "(?(?!ab)z|a)b",
            input: "ab",
            expected_rgx_error: "conditional syntax is parsed but not yet integrated into VM execution",
        },
        KnownGapCase {
            name: "conditional_lookbehind",
            pattern: "(?(?<=x)a|b)",
            input: "b",
            expected_rgx_error: "conditional syntax is parsed but not yet integrated into VM execution",
        },
        KnownGapCase {
            name: "conditional_negative_lookbehind",
            pattern: "(?(?<!x)b|a)",
            input: "b",
            expected_rgx_error: "conditional syntax is parsed but not yet integrated into VM execution",
        },
    ];

    for case in cases {
        assert_known_gap_case(&case);
    }
}
