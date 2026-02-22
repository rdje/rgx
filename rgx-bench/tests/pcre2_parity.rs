use pcre2::bytes::Regex as PcreRegex;
use rgx_core::Regex as RgxRegex;

struct ParityCase {
    name: &'static str,
    pattern: &'static str,
    input: &'static str,
}

#[test]
fn pcre2_parity_supported_syntax_find_all_spans() {
    let cases = [
        ParityCase {
            name: "literal_all",
            pattern: "cat",
            input: "cat xx cat yy cat",
        },
        ParityCase {
            name: "alternation_all",
            pattern: "cat|dog",
            input: "dog xx cat yy dog",
        },
        ParityCase {
            name: "digit_class_all",
            pattern: r"\d+",
            input: "a1 bb22 c333",
        },
        ParityCase {
            name: "word_boundary_all",
            pattern: r"\bcat\b",
            input: "cat scat cat",
        },
        ParityCase {
            name: "positive_lookahead_all",
            pattern: "(?=ab)a",
            input: "abxxab",
        },
        ParityCase {
            name: "negative_lookahead_all",
            pattern: "(?!cat)c",
            input: "car cat cup",
        },
        ParityCase {
            name: "positive_lookbehind_all",
            pattern: "(?<=x)a",
            input: "xaxxa",
        },
        ParityCase {
            name: "negative_lookbehind_all",
            pattern: "(?<!x)a",
            input: "a xa ba",
        },
        ParityCase {
            name: "atomic_group_no_backtrack_all",
            pattern: "(?>a|ab)c",
            input: "ac abc ac",
        },
        ParityCase {
            name: "non_atomic_counterexample_all",
            pattern: "(a|ab)c",
            input: "abc ac",
        },
        ParityCase {
            name: "anchor_start_all",
            pattern: "^cat",
            input: "cat dog cat",
        },
        ParityCase {
            name: "anchor_end_all",
            pattern: "dog$",
            input: "cat dog",
        },
        ParityCase {
            name: "quantifier_plus_all",
            pattern: "ab+",
            input: "ab abb abbb a",
        },
        ParityCase {
            name: "range_bounded_suffix_backtrack_all",
            pattern: r"\d{2,3}3",
            input: "123 2233 993 4443",
        },
        ParityCase {
            name: "range_exact_all",
            pattern: r"\d{3}",
            input: "12 123 4567 890",
        },
        ParityCase {
            name: "no_match_all",
            pattern: "cat",
            input: "dog bird",
        },
    ];

    for case in cases {
        let rgx = rgx_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx error: {e}", case.name));
        let pcre2 = pcre2_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 error: {e}", case.name));
        assert_eq!(
            rgx, pcre2,
            "find_all span mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }
}

#[test]
fn pcre2_parity_supported_syntax_no_match_consistency() {
    let cases = [
        ParityCase {
            name: "no_match_literal",
            pattern: "cat",
            input: "bird",
        },
        ParityCase {
            name: "no_match_alternation",
            pattern: "cat|dog",
            input: "bird",
        },
        ParityCase {
            name: "no_match_anchor",
            pattern: "^cat$",
            input: "xcat",
        },
        ParityCase {
            name: "no_match_lookbehind",
            pattern: "(?<=x)a",
            input: "ba",
        },
        ParityCase {
            name: "no_match_atomic",
            pattern: "(?>a|ab)c",
            input: "abc",
        },
    ];

    for case in cases {
        let rgx_first = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx first error: {e}", case.name));
        let pcre2_first = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 first error: {e}", case.name));
        assert_eq!(
            rgx_first, pcre2_first,
            "first-match no-match mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
        assert!(
            rgx_first.is_none(),
            "expected no first match for case '{}' in rgx",
            case.name
        );

        let rgx_all = rgx_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx all error: {e}", case.name));
        let pcre2_all = pcre2_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 all error: {e}", case.name));
        assert_eq!(
            rgx_all, pcre2_all,
            "find_all no-match mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
        assert!(
            rgx_all.is_empty(),
            "expected no find_all spans for case '{}' in rgx",
            case.name
        );
    }
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

fn rgx_all_spans(pattern: &str, input: &str) -> Result<Vec<(usize, usize)>, String> {
    let regex =
        RgxRegex::compile(pattern).map_err(|e| format!("rgx compile failed for '{pattern}': {e}"))?;
    Ok(regex
        .find_all(input)
        .into_iter()
        .map(|m| (m.start, m.end))
        .collect())
}

fn pcre2_first_span(pattern: &str, input: &str) -> Result<Option<(usize, usize)>, String> {
    let regex = PcreRegex::new(pattern)
        .map_err(|e| format!("pcre2 compile failed for '{pattern}': {e}"))?;
    let found = regex
        .find(input.as_bytes())
        .map_err(|e| format!("pcre2 find failed for '{pattern}': {e}"))?;
    Ok(found.map(|m| (m.start(), m.end())))
}

fn pcre2_all_spans(pattern: &str, input: &str) -> Result<Vec<(usize, usize)>, String> {
    let regex = PcreRegex::new(pattern)
        .map_err(|e| format!("pcre2 compile failed for '{pattern}': {e}"))?;
    let mut spans = Vec::new();
    for next in regex.find_iter(input.as_bytes()) {
        let found = next.map_err(|e| format!("pcre2 find_iter failed for '{pattern}': {e}"))?;
        spans.push((found.start(), found.end()));
    }
    Ok(spans)
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
        ParityCase {
            name: "anchor_start",
            pattern: "^cat",
            input: "cat dog",
        },
        ParityCase {
            name: "anchor_end",
            pattern: "dog$",
            input: "cat dog",
        },
        ParityCase {
            name: "quantifier_plus",
            pattern: "ab+",
            input: "xxabbbzz",
        },
        ParityCase {
            name: "range_bounded_suffix_backtrack",
            pattern: r"\d{2,3}3",
            input: "x123y",
        },
        ParityCase {
            name: "range_bounded_suffix_greedy",
            pattern: r"\d{2,3}3",
            input: "x2233y",
        },
        ParityCase {
            name: "no_match",
            pattern: "cat",
            input: "dog",
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
fn pcre2_parity_supported_range_quantifier_scan_behavior() {
    let pattern = r"\d{2,3}";

    let first_input = "x1y22z333";
    let rgx_first = rgx_first_span(pattern, first_input)
        .unwrap_or_else(|e| panic!("[range_quantifier_supported] rgx first error: {e}"));
    let pcre2_first = pcre2_first_span(pattern, first_input)
        .unwrap_or_else(|e| panic!("[range_quantifier_supported] pcre2 first error: {e}"));
    assert_eq!(pcre2_first, Some((3, 5)));
    assert_eq!(rgx_first, pcre2_first);

    let all_input = "x1 y22 z333 w4444";
    let rgx_all = rgx_all_spans(pattern, all_input)
        .unwrap_or_else(|e| panic!("[range_quantifier_supported] rgx all error: {e}"));
    let pcre2_all = pcre2_all_spans(pattern, all_input)
        .unwrap_or_else(|e| panic!("[range_quantifier_supported] pcre2 all error: {e}"));
    assert_eq!(pcre2_all, vec![(4, 6), (8, 11), (13, 16)]);
    assert_eq!(rgx_all, pcre2_all);
}

#[test]
fn pcre2_parity_supported_unbounded_range_quantifier_behavior() {
    let pattern = r"\d{2,}";

    let first_input = "x1y22z333";
    let rgx_first = rgx_first_span(pattern, first_input)
        .unwrap_or_else(|e| panic!("[unbounded_range_quantifier_supported] rgx first error: {e}"));
    let pcre2_first = pcre2_first_span(pattern, first_input).unwrap_or_else(|e| {
        panic!("[unbounded_range_quantifier_supported] pcre2 first error: {e}")
    });
    assert_eq!(pcre2_first, Some((3, 5)));
    assert_eq!(rgx_first, pcre2_first);

    let all_input = "x1 y22 z333 w4444";
    let rgx_all = rgx_all_spans(pattern, all_input)
        .unwrap_or_else(|e| panic!("[unbounded_range_quantifier_supported] rgx all error: {e}"));
    let pcre2_all = pcre2_all_spans(pattern, all_input).unwrap_or_else(|e| {
        panic!("[unbounded_range_quantifier_supported] pcre2 all error: {e}")
    });
    assert_eq!(pcre2_all, vec![(4, 6), (8, 11), (13, 17)]);
    assert_eq!(rgx_all, pcre2_all);

    let suffix_pattern = r"\d{2,}3";
    let suffix_first_input = "x123 y2233";
    let rgx_suffix_first = rgx_first_span(suffix_pattern, suffix_first_input).unwrap_or_else(|e| {
        panic!("[unbounded_range_suffix_supported] rgx first error: {e}")
    });
    let pcre2_suffix_first =
        pcre2_first_span(suffix_pattern, suffix_first_input).unwrap_or_else(|e| {
            panic!("[unbounded_range_suffix_supported] pcre2 first error: {e}")
        });
    assert_eq!(pcre2_suffix_first, Some((1, 4)));
    assert_eq!(rgx_suffix_first, pcre2_suffix_first);

    let suffix_all_input = "123 2233 993 4443";
    let rgx_suffix_all = rgx_all_spans(suffix_pattern, suffix_all_input)
        .unwrap_or_else(|e| panic!("[unbounded_range_suffix_supported] rgx all error: {e}"));
    let pcre2_suffix_all = pcre2_all_spans(suffix_pattern, suffix_all_input)
        .unwrap_or_else(|e| panic!("[unbounded_range_suffix_supported] pcre2 all error: {e}"));
    assert_eq!(pcre2_suffix_all, vec![(0, 3), (4, 8), (9, 12), (13, 17)]);
    assert_eq!(rgx_suffix_all, pcre2_suffix_all);
}

#[test]
fn pcre2_parity_supported_quantifier_suffix_backtracking_behavior() {
    let first_cases = [
        ParityCase {
            name: "star_suffix_backtrack_first",
            pattern: "a*a",
            input: "a",
        },
        ParityCase {
            name: "plus_suffix_backtrack_first",
            pattern: "a+a",
            input: "aa",
        },
        ParityCase {
            name: "question_suffix_backtrack_first",
            pattern: "ab?b",
            input: "ab",
        },
    ];

    for case in first_cases {
        let rgx = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx first error: {e}", case.name));
        let pcre2 = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 first error: {e}", case.name));
        assert_eq!(
            rgx, pcre2,
            "quantifier suffix first-match mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }

    assert_eq!(
        pcre2_first_span("a*a", "a")
            .unwrap_or_else(|e| panic!("[star_suffix_backtrack_first] pcre2 error: {e}")),
        Some((0, 1))
    );
    assert_eq!(
        pcre2_first_span("a+a", "aa")
            .unwrap_or_else(|e| panic!("[plus_suffix_backtrack_first] pcre2 error: {e}")),
        Some((0, 2))
    );
    assert_eq!(
        pcre2_first_span("ab?b", "ab")
            .unwrap_or_else(|e| panic!("[question_suffix_backtrack_first] pcre2 error: {e}")),
        Some((0, 2))
    );

    let all_cases = [
        ParityCase {
            name: "star_suffix_backtrack_all",
            pattern: "a*a",
            input: "a a a",
        },
        ParityCase {
            name: "plus_suffix_backtrack_all",
            pattern: "a+a",
            input: "aa aaaa",
        },
        ParityCase {
            name: "question_suffix_backtrack_all",
            pattern: "ab?b",
            input: "ab abb abbb",
        },
    ];

    for case in all_cases {
        let rgx = rgx_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx all error: {e}", case.name));
        let pcre2 = pcre2_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 all error: {e}", case.name));
        assert_eq!(
            rgx, pcre2,
            "quantifier suffix find_all mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }

    assert_eq!(
        pcre2_all_spans("a*a", "a a a")
            .unwrap_or_else(|e| panic!("[star_suffix_backtrack_all] pcre2 error: {e}")),
        vec![(0, 1), (2, 3), (4, 5)]
    );
    assert_eq!(
        pcre2_all_spans("a+a", "aa aaaa")
            .unwrap_or_else(|e| panic!("[plus_suffix_backtrack_all] pcre2 error: {e}")),
        vec![(0, 2), (3, 7)]
    );
    assert_eq!(
        pcre2_all_spans("ab?b", "ab abb abbb")
            .unwrap_or_else(|e| panic!("[question_suffix_backtrack_all] pcre2 error: {e}")),
        vec![(0, 2), (3, 6), (7, 10)]
    );
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
