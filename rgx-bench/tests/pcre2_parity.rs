use pcre2::bytes::Regex as PcreRegex;
use rgx_core::Regex as RgxRegex;

struct ParityCase {
    name: &'static str,
    pattern: &'static str,
    input: &'static str,
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
    let rgx_result = RgxRegex::compile(r"(a)\1");
    assert!(
        rgx_result.is_err(),
        "rgx should explicitly reject backreference runtime execution for now"
    );

    let pcre2 = PcreRegex::new(r"(a)\1").expect("pcre2 should compile backreference pattern");
    let matched = pcre2
        .find(b"aa")
        .expect("pcre2 backreference search should not error");
    assert!(
        matched.is_some(),
        "pcre2 should execute backreference pattern on 'aa'"
    );
}
