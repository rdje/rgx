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
            name: "digit_class_neg_all",
            pattern: r"\D+",
            input: "123abc45!!",
        },
        ParityCase {
            name: "word_class_neg_all",
            pattern: r"\W+",
            input: "ab!!cd??",
        },
        ParityCase {
            name: "space_class_neg_all",
            pattern: r"\S+",
            input: "  ab\tcd  ",
        },
        ParityCase {
            name: "extended_class_simple_range_all",
            pattern: r"(?[[a-z]])+",
            input: "abc 123 xy",
        },
        ParityCase {
            name: "extended_class_simple_negated_all",
            pattern: r"(?[[^0-9]])+",
            input: "123abc45!!",
        },
        ParityCase {
            name: "extended_class_nested_ordinary_shorthand_range_all",
            pattern: r"(?[[\dA-F]])+",
            input: "zz FACE204 xx 19B yy",
        },
        ParityCase {
            name: "extended_class_nested_ordinary_posix_all",
            pattern: r"(?[[[:graph:]]])+",
            input: "\x01AZ9! \tB",
        },
        ParityCase {
            name: "extended_class_nested_ordinary_property_difference_all",
            pattern: r"(?[[\p{L}] - [\p{Lu}]])+",
            input: "AZ facet qQ XYZ",
        },
        ParityCase {
            name: "extended_class_difference_all",
            pattern: r"(?[[a-z] - [aeiou]])+",
            input: "aei bcdf xyz ou",
        },
        ParityCase {
            name: "extended_class_digit_shorthand_difference_all",
            pattern: r"(?[\d - [3]])+",
            input: "ab20479 333 55",
        },
        ParityCase {
            name: "extended_class_word_shorthand_intersection_all",
            pattern: r"(?[\w & [a-z]])+",
            input: "ABC facet_ xyz 123",
        },
        ParityCase {
            name: "extended_class_posix_graph_all",
            pattern: r"(?[ [:graph:] ])+",
            input: "\x01AZ9! \tB",
        },
        ParityCase {
            name: "extended_class_negated_posix_alpha_all",
            pattern: r"(?[ [:^alpha:] ])+",
            input: "AZ 19?! \nB",
        },
        ParityCase {
            name: "extended_class_posix_alpha_algebra_all",
            pattern: r"(?[ [:alpha:] & [a-z\t] ])+",
            input: "ABC facet\t xyz 123",
        },
        ParityCase {
            name: "extended_class_horizontal_shorthand_all",
            pattern: r"(?[\h])+",
            input: "A \tB\n",
        },
        ParityCase {
            name: "extended_class_vertical_shorthand_all",
            pattern: r"(?[\v])+",
            input: "A\n\u{000B}\u{000C}\rB \t",
        },
        ParityCase {
            name: "extended_class_hex_escape_difference_all",
            pattern: r"(?[\x{41} - [B]])+",
            input: "B AA C A",
        },
        ParityCase {
            name: "extended_class_control_escape_union_all",
            pattern: "(?[\\n | \\t])+",
            input: "x\n\t\n y\t",
        },
        ParityCase {
            name: "extended_class_control_literal_escape_union_all",
            pattern: r"(?[\a | \b | \e | \f])+",
            input: "x\u{07}\u{08}\u{1B}\u{0C} y\u{08}\u{07}",
        },
        ParityCase {
            name: "extended_class_control_letter_escape_union_all",
            pattern: r"(?[\cA | [B]])+",
            input: "x\u{0001}BB y B",
        },
        ParityCase {
            name: "extended_class_octal_escape_union_all",
            pattern: r"(?[\040 | \011 | \o{101}])+",
            input: "Z \tA\t Q A",
        },
        ParityCase {
            name: "extended_class_property_intersection_all",
            pattern: r"(?[\p{L} & \p{Lu}])+",
            input: "abc XYZ q M",
        },
        ParityCase {
            name: "extended_class_complement_all",
            pattern: r"(?[ ![0-9] ])+",
            input: "123abc!!45Z",
        },
        ParityCase {
            name: "extended_class_grouped_algebra_all",
            pattern: r"(?[ ([a-z] - [aeiou]) & [b-d] ])+",
            input: "ae bcd xyz bc",
        },
        ParityCase {
            name: "extended_class_symmetric_difference_all",
            pattern: r"(?[ [AC] ^ [BC] ])+",
            input: "CCABBAAC",
        },
        ParityCase {
            name: "extended_class_same_level_precedence_all",
            pattern: r"(?[ [a-f] | [d-z] & [m-p] ])+",
            input: "abc mnop xyz def",
        },
        ParityCase {
            name: "extended_class_low_precedence_chain_all",
            pattern: r"(?[ [a-z] - [aeiou] + [0-9] - [5] ])+",
            input: "aei bcdf0249 555 xyz",
        },
        ParityCase {
            name: "word_boundary_all",
            pattern: r"\bcat\b",
            input: "cat scat cat",
        },
        ParityCase {
            name: "backreference_all",
            pattern: r"(ab)\1",
            input: "abab xx ababab yy abab",
        },
        ParityCase {
            name: "branch_reset_backreference_all",
            pattern: r"(?|(a)|(b))\1",
            input: "aa bb ab ba",
        },
        ParityCase {
            name: "branch_reset_conditional_all",
            pattern: r"(?|(a)(b)|c)(?(2)d|e)",
            input: "abd xx ce yy abe",
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
            name: "anchor_abs_start_all",
            pattern: r"\Acat",
            input: "cat dog",
        },
        ParityCase {
            name: "anchor_end_all",
            pattern: "dog$",
            input: "cat dog",
        },
        ParityCase {
            name: "anchor_abs_end_all",
            pattern: r"dog\z",
            input: "cat dog",
        },
        ParityCase {
            name: "anchor_abs_end_or_newline_all",
            pattern: r"dog\Z",
            input: "cat dog\n",
        },
        ParityCase {
            name: "quantifier_plus_all",
            pattern: "ab+",
            input: "ab abb abbb a",
        },
        ParityCase {
            name: "quantifier_question_lazy_all",
            pattern: "a??",
            input: "ba",
        },
        ParityCase {
            name: "quantifier_star_lazy_all",
            pattern: "ab*?",
            input: "abbb ab a",
        },
        ParityCase {
            name: "range_bounded_lazy_suffix_all",
            pattern: r"\d{2,3}?3",
            input: "123 2233 993 4443",
        },
        ParityCase {
            name: "range_unbounded_lazy_suffix_all",
            pattern: r"\d{2,}?3",
            input: "123 2233 993 4443",
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
            name: "no_match_anchor_abs_start",
            pattern: r"\Acat",
            input: "xxcat",
        },
        ParityCase {
            name: "no_match_anchor_abs_end",
            pattern: r"dog\z",
            input: "cat dog\n",
        },
        ParityCase {
            name: "no_match_anchor_abs_end_or_newline",
            pattern: r"dog\Z",
            input: "cat dog\nx",
        },
        ParityCase {
            name: "no_match_digit_class_neg",
            pattern: r"\D+",
            input: "12345",
        },
        ParityCase {
            name: "no_match_word_class_neg",
            pattern: r"\W+",
            input: "abc_123",
        },
        ParityCase {
            name: "no_match_space_class_neg",
            pattern: r"\S+",
            input: " \t\n",
        },
        ParityCase {
            name: "no_match_extended_class_negated",
            pattern: r"(?[[^0-9]])+",
            input: "12345",
        },
        ParityCase {
            name: "no_match_extended_class_nested_ordinary_shorthand_range",
            pattern: r"(?[[\dA-F]])+",
            input: "xyz_",
        },
        ParityCase {
            name: "no_match_extended_class_nested_ordinary_posix",
            pattern: r"(?[[[:graph:]]])+",
            input: " \t\n",
        },
        ParityCase {
            name: "no_match_extended_class_nested_ordinary_property_difference",
            pattern: r"(?[[\p{L}] - [\p{Lu}]])+",
            input: "FACE",
        },
        ParityCase {
            name: "no_match_extended_class_difference",
            pattern: r"(?[[a-z] - [aeiou]])+",
            input: "aeiou",
        },
        ParityCase {
            name: "no_match_extended_class_digit_shorthand_difference",
            pattern: r"(?[\d - [3]])+",
            input: "3333",
        },
        ParityCase {
            name: "no_match_extended_class_negated_shorthand_intersection",
            pattern: r"(?[\D & [A-F]])+",
            input: "1237",
        },
        ParityCase {
            name: "no_match_extended_class_posix_graph",
            pattern: r"(?[ [:graph:] ])+",
            input: " \t\n",
        },
        ParityCase {
            name: "no_match_extended_class_negated_posix_alpha",
            pattern: r"(?[ [:^alpha:] ])+",
            input: "ABCxyz",
        },
        ParityCase {
            name: "no_match_extended_class_complemented_posix_alpha",
            pattern: r"(?[ ![:alpha:] ])+",
            input: "ABCxyz",
        },
        ParityCase {
            name: "no_match_extended_class_posix_alpha_algebra",
            pattern: r"(?[ [:alpha:] & [a-z\t] ])+",
            input: "FACE\t",
        },
        ParityCase {
            name: "no_match_extended_class_horizontal_shorthand",
            pattern: r"(?[\h])+",
            input: "\n\r",
        },
        ParityCase {
            name: "no_match_extended_class_vertical_shorthand",
            pattern: r"(?[\v])+",
            input: " \t",
        },
        ParityCase {
            name: "no_match_extended_class_negated_horizontal_shorthand",
            pattern: r"(?[\H])+",
            input: " \t",
        },
        ParityCase {
            name: "no_match_extended_class_negated_vertical_shorthand",
            pattern: r"(?[\V])+",
            input: "\n\u{000B}\u{000C}\r",
        },
        ParityCase {
            name: "no_match_extended_class_hex_escape_difference",
            pattern: r"(?[\x{41} - [B]])+",
            input: "BBBB",
        },
        ParityCase {
            name: "no_match_extended_class_control_escape_union",
            pattern: "(?[\\n | \\t])+",
            input: "    ",
        },
        ParityCase {
            name: "no_match_extended_class_control_literal_escape_union",
            pattern: r"(?[\a | \b | \e | \f])+",
            input: "A\t ",
        },
        ParityCase {
            name: "no_match_extended_class_control_letter_escape_union",
            pattern: r"(?[\cA | [B]])+",
            input: "AAC",
        },
        ParityCase {
            name: "no_match_extended_class_octal_escape_union",
            pattern: r"(?[\040 | \011 | \o{101}])+",
            input: "ZZ\n",
        },
        ParityCase {
            name: "no_match_extended_class_property_intersection",
            pattern: r"(?[\p{L} & \p{Lu}])+",
            input: "abc xyz",
        },
        ParityCase {
            name: "no_match_extended_class_complement",
            pattern: r"(?[ ![0-9] ])+",
            input: "12345",
        },
        ParityCase {
            name: "no_match_extended_class_grouped_algebra",
            pattern: r"(?[ ([a-z] - [aeiou]) & [b-d] ])+",
            input: "aeiou",
        },
        ParityCase {
            name: "no_match_extended_class_symmetric_difference",
            pattern: r"(?[ [AC] ^ [BC] ])+",
            input: "CCCC",
        },
        ParityCase {
            name: "no_match_extended_class_same_level_precedence",
            pattern: r"(?[ [a-f] | [d-z] & [m-p] ])+",
            input: "xyz",
        },
        ParityCase {
            name: "no_match_extended_class_low_precedence_chain",
            pattern: r"(?[ [a-z] - [aeiou] + [0-9] - [5] ])+",
            input: "aeiou5",
        },
        ParityCase {
            name: "no_match_backreference",
            pattern: r"(a)\1",
            input: "ab",
        },
        ParityCase {
            name: "no_match_branch_reset_backreference",
            pattern: r"(?|(a)|(b))\1",
            input: "ab",
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

fn rgx_first_span(pattern: &str, input: &str) -> Result<Option<(usize, usize)>, String> {
    let regex = RgxRegex::compile(pattern)
        .map_err(|e| format!("rgx compile failed for '{pattern}': {e}"))?;
    Ok(regex.find_first(input).map(|m| (m.start, m.end)))
}

fn rgx_all_spans(pattern: &str, input: &str) -> Result<Vec<(usize, usize)>, String> {
    let regex = RgxRegex::compile(pattern)
        .map_err(|e| format!("rgx compile failed for '{pattern}': {e}"))?;
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
            name: "digit_class_neg",
            pattern: r"\D+",
            input: "123abc45",
        },
        ParityCase {
            name: "word_class_neg",
            pattern: r"\W+",
            input: "ab!!cd",
        },
        ParityCase {
            name: "space_class_neg",
            pattern: r"\S+",
            input: "  ab  ",
        },
        ParityCase {
            name: "word_boundary",
            pattern: r"\bcat\b",
            input: "a cat nap",
        },
        ParityCase {
            name: "extended_class_difference",
            pattern: r"(?[[a-z] - [aeiou]])+",
            input: "aei bcdf xyz",
        },
        ParityCase {
            name: "extended_class_nested_ordinary_shorthand_range",
            pattern: r"(?[[\dA-F]])+",
            input: "xxFACE204yy",
        },
        ParityCase {
            name: "extended_class_nested_ordinary_posix",
            pattern: r"(?[[[:graph:]]])+",
            input: " \t!A9 ",
        },
        ParityCase {
            name: "extended_class_nested_ordinary_property_difference",
            pattern: r"(?[[\p{L}] - [\p{Lu}]])+",
            input: "AZfacet",
        },
        ParityCase {
            name: "extended_class_digit_shorthand_difference",
            pattern: r"(?[\d - [3]])+",
            input: "id 20479",
        },
        ParityCase {
            name: "extended_class_negated_shorthand_intersection",
            pattern: r"(?[\D & [A-F]])+",
            input: "99FACE77",
        },
        ParityCase {
            name: "extended_class_posix_graph",
            pattern: r"(?[ [:graph:] ])+",
            input: " \t!A9 ",
        },
        ParityCase {
            name: "extended_class_negated_posix_alpha",
            pattern: r"(?[ [:^alpha:] ])+",
            input: "ab19?!\n",
        },
        ParityCase {
            name: "extended_class_complemented_posix_alpha",
            pattern: r"(?[ ![:alpha:] ])+",
            input: "AZ19!\n",
        },
        ParityCase {
            name: "extended_class_posix_alpha_algebra",
            pattern: r"(?[ [:alpha:] & [a-z\t] ])+",
            input: "AZfacet\t",
        },
        ParityCase {
            name: "extended_class_horizontal_shorthand",
            pattern: r"(?[\h])+",
            input: "xx \tyy",
        },
        ParityCase {
            name: "extended_class_negated_horizontal_shorthand",
            pattern: r"(?[\H])+",
            input: " \nAZ\t",
        },
        ParityCase {
            name: "extended_class_vertical_shorthand",
            pattern: r"(?[\v])+",
            input: "xx\n\u{000B}\u{000C}yy",
        },
        ParityCase {
            name: "extended_class_negated_vertical_shorthand",
            pattern: r"(?[\V])+",
            input: "\nA \t\r",
        },
        ParityCase {
            name: "extended_class_hex_escape_difference",
            pattern: r"(?[\x{41} - [B]])+",
            input: "zzAAByy",
        },
        ParityCase {
            name: "extended_class_control_escape_union",
            pattern: "(?[\\n | \\t])+",
            input: "xx\n\tzz",
        },
        ParityCase {
            name: "extended_class_control_literal_escape_union",
            pattern: r"(?[\a | \b | \e | \f])+",
            input: "xx\u{07}\u{08}\u{1B}\u{0C}zz",
        },
        ParityCase {
            name: "extended_class_control_letter_escape_union",
            pattern: r"(?[\cA | [B]])+",
            input: "xx\u{0001}BBzz",
        },
        ParityCase {
            name: "extended_class_octal_escape_union",
            pattern: r"(?[\040 | \011 | \o{101}])+",
            input: "xx \tA\tzz",
        },
        ParityCase {
            name: "extended_class_property_intersection",
            pattern: r"(?[\p{L} & \p{Lu}])+",
            input: "abc XYZ q",
        },
        ParityCase {
            name: "extended_class_complement",
            pattern: r"(?[ ![0-9] ])+",
            input: "123abc!!",
        },
        ParityCase {
            name: "extended_class_grouped_algebra",
            pattern: r"(?[ ([a-z] - [aeiou]) & [b-d] ])+",
            input: "ae bcd xyz",
        },
        ParityCase {
            name: "extended_class_symmetric_difference",
            pattern: r"(?[ [AC] ^ [BC] ])+",
            input: "xxABCC",
        },
        ParityCase {
            name: "extended_class_same_level_precedence",
            pattern: r"(?[ [a-f] | [d-z] & [m-p] ])+",
            input: "xxabcmnop",
        },
        ParityCase {
            name: "extended_class_low_precedence_chain",
            pattern: r"(?[ [a-z] - [aeiou] + [0-9] - [5] ])+",
            input: "xxbcdf0249",
        },
        ParityCase {
            name: "backreference",
            pattern: r"(a|ab)\1",
            input: "zzababxx",
        },
        ParityCase {
            name: "branch_reset_backreference",
            pattern: r"(?|(a)|(b))\1",
            input: "xxbbzz",
        },
        ParityCase {
            name: "branch_reset_conditional",
            pattern: r"(?|(a)(b)|c)(?(2)d|e)",
            input: "xxceyy",
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
            name: "anchor_abs_start",
            pattern: r"\Acat",
            input: "cat dog",
        },
        ParityCase {
            name: "anchor_end",
            pattern: "dog$",
            input: "cat dog",
        },
        ParityCase {
            name: "anchor_abs_end",
            pattern: r"dog\z",
            input: "cat dog",
        },
        ParityCase {
            name: "anchor_abs_end_or_newline",
            pattern: r"dog\Z",
            input: "cat dog\n",
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
fn pcre2_parity_supported_unicode_property_classes() {
    let cases = [
        ParityCase {
            name: "unicode_property_letters",
            pattern: r"\p{L}+",
            input: "123 abc XYZ !",
        },
        ParityCase {
            name: "unicode_property_negated_letters",
            pattern: r"\P{L}+",
            input: "abc 123 XYZ !",
        },
        ParityCase {
            name: "unicode_property_hex_digit",
            pattern: r"\p{ASCII_Hex_Digit}+",
            input: "zz 12AF xx G",
        },
    ];

    for case in cases {
        let rgx_first = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx first error: {e}", case.name));
        let pcre2_first = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 first error: {e}", case.name));
        assert_eq!(
            rgx_first, pcre2_first,
            "unicode-property first-match mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );

        let rgx_all = rgx_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx all error: {e}", case.name));
        let pcre2_all = pcre2_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 all error: {e}", case.name));
        assert_eq!(
            rgx_all, pcre2_all,
            "unicode-property find_all mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }
}

#[test]
fn pcre2_parity_supported_recursion_forms() {
    let first_cases = [
        ParityCase {
            name: "recursion_entire_pattern",
            pattern: "a(?R)?b",
            input: "xxaaabbbzz",
        },
        ParityCase {
            name: "recursion_group_number",
            pattern: "(a(?1)?b)",
            input: "xxaaabbbzz",
        },
        ParityCase {
            name: "recursion_named_group",
            pattern: "(?<word>a(?&word)?b)",
            input: "xxaaabbbzz",
        },
    ];

    for case in first_cases {
        let rgx = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx error: {e}", case.name));
        let pcre2 = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 error: {e}", case.name));
        assert_eq!(
            rgx, pcre2,
            "recursion first-match mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }

    let no_match_cases = [
        ParityCase {
            name: "recursion_entire_pattern_no_match",
            pattern: "a(?R)?b",
            input: "xxaabbbzz",
        },
        ParityCase {
            name: "recursion_group_number_no_match",
            pattern: "(a(?1)?b)",
            input: "xxaabbbzz",
        },
        ParityCase {
            name: "recursion_named_group_no_match",
            pattern: "(?<word>a(?&word)?b)",
            input: "xxaabbbzz",
        },
    ];

    for case in no_match_cases {
        let rgx = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx error: {e}", case.name));
        let pcre2 = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 error: {e}", case.name));
        assert_eq!(
            rgx, pcre2,
            "recursion no-match mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
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
    let pcre2_all = pcre2_all_spans(pattern, all_input)
        .unwrap_or_else(|e| panic!("[unbounded_range_quantifier_supported] pcre2 all error: {e}"));
    assert_eq!(pcre2_all, vec![(4, 6), (8, 11), (13, 17)]);
    assert_eq!(rgx_all, pcre2_all);

    let suffix_pattern = r"\d{2,}3";
    let suffix_first_input = "x123 y2233";
    let rgx_suffix_first = rgx_first_span(suffix_pattern, suffix_first_input)
        .unwrap_or_else(|e| panic!("[unbounded_range_suffix_supported] rgx first error: {e}"));
    let pcre2_suffix_first = pcre2_first_span(suffix_pattern, suffix_first_input)
        .unwrap_or_else(|e| panic!("[unbounded_range_suffix_supported] pcre2 first error: {e}"));
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
fn pcre2_parity_supported_possessive_quantifiers() {
    let first_cases = [
        ParityCase {
            name: "possessive_star_success_first",
            pattern: "a*+b",
            input: "aaab aab b",
        },
        ParityCase {
            name: "possessive_plus_success_first",
            pattern: "a++b",
            input: "aaab aab b",
        },
        ParityCase {
            name: "possessive_range_success_first",
            pattern: "a{2,3}+b",
            input: "aaab aab ab",
        },
        ParityCase {
            name: "possessive_star_suffix_no_backtrack_first",
            pattern: r"\Aa*+a\z",
            input: "aaaa",
        },
        ParityCase {
            name: "possessive_plus_suffix_no_backtrack_first",
            pattern: r"\Aa++a\z",
            input: "aaaa",
        },
        ParityCase {
            name: "possessive_question_suffix_no_backtrack_first",
            pattern: r"\Aa?+a\z",
            input: "a",
        },
        ParityCase {
            name: "possessive_range_suffix_no_backtrack_first",
            pattern: r"\A\d{2,3}+3\z",
            input: "123",
        },
    ];

    for case in first_cases {
        let rgx = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx first error: {e}", case.name));
        let pcre2 = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 first error: {e}", case.name));
        assert_eq!(
            rgx, pcre2,
            "possessive-quantifier first-match mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }

    assert_eq!(
        pcre2_first_span("a*+b", "aaab aab b")
            .unwrap_or_else(|e| panic!("[possessive_star_success_first] pcre2 error: {e}")),
        Some((0, 4))
    );
    assert_eq!(
        pcre2_first_span("a++b", "aaab aab b")
            .unwrap_or_else(|e| panic!("[possessive_plus_success_first] pcre2 error: {e}")),
        Some((0, 4))
    );
    assert_eq!(
        pcre2_first_span("a{2,3}+b", "aaab aab ab")
            .unwrap_or_else(|e| panic!("[possessive_range_success_first] pcre2 error: {e}")),
        Some((0, 4))
    );
    assert_eq!(
        pcre2_first_span(r"\Aa*+a\z", "aaaa").unwrap_or_else(|e| {
            panic!("[possessive_star_suffix_no_backtrack_first] pcre2 error: {e}")
        }),
        None
    );
    assert_eq!(
        pcre2_first_span(r"\A\d{2,3}+3\z", "123").unwrap_or_else(|e| {
            panic!("[possessive_range_suffix_no_backtrack_first] pcre2 error: {e}")
        }),
        None
    );

    let all_cases = [
        ParityCase {
            name: "possessive_star_success_all",
            pattern: "a*+b",
            input: "aaab aab b",
        },
        ParityCase {
            name: "possessive_plus_success_all",
            pattern: "a++b",
            input: "aaab aab b",
        },
        ParityCase {
            name: "possessive_range_success_all",
            pattern: "a{2,3}+b",
            input: "aaab aab ab",
        },
        ParityCase {
            name: "possessive_star_suffix_no_backtrack_all",
            pattern: r"\Aa*+a\z",
            input: "aaaa",
        },
        ParityCase {
            name: "possessive_plus_suffix_no_backtrack_all",
            pattern: r"\Aa++a\z",
            input: "aaaa",
        },
        ParityCase {
            name: "possessive_question_suffix_no_backtrack_all",
            pattern: r"\Aa?+a\z",
            input: "a",
        },
        ParityCase {
            name: "possessive_range_suffix_no_backtrack_all",
            pattern: r"\A\d{2,3}+3\z",
            input: "123",
        },
    ];

    for case in all_cases {
        let rgx = rgx_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx all error: {e}", case.name));
        let pcre2 = pcre2_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 all error: {e}", case.name));
        assert_eq!(
            rgx, pcre2,
            "possessive-quantifier find_all mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }

    assert_eq!(
        pcre2_all_spans("a*+b", "aaab aab b")
            .unwrap_or_else(|e| panic!("[possessive_star_success_all] pcre2 error: {e}")),
        vec![(0, 4), (5, 8), (9, 10)]
    );
    assert_eq!(
        pcre2_all_spans("a++b", "aaab aab b")
            .unwrap_or_else(|e| panic!("[possessive_plus_success_all] pcre2 error: {e}")),
        vec![(0, 4), (5, 8)]
    );
    assert_eq!(
        pcre2_all_spans("a{2,3}+b", "aaab aab ab")
            .unwrap_or_else(|e| panic!("[possessive_range_success_all] pcre2 error: {e}")),
        vec![(0, 4), (5, 8)]
    );
    assert_eq!(
        pcre2_all_spans(r"\Aa++a\z", "aaaa").unwrap_or_else(|e| {
            panic!("[possessive_plus_suffix_no_backtrack_all] pcre2 error: {e}")
        }),
        vec![]
    );
    assert_eq!(
        pcre2_all_spans(r"\Aa?+a\z", "a").unwrap_or_else(|e| {
            panic!("[possessive_question_suffix_no_backtrack_all] pcre2 error: {e}")
        }),
        vec![]
    );
}

#[test]
fn pcre2_parity_supported_conditionals() {
    let cases = [
        ParityCase {
            name: "conditional_group_exists",
            pattern: "(a)?(?(1)b|c)",
            input: "ab c ac cab",
        },
        ParityCase {
            name: "conditional_named_group_exists_angle_bracket",
            pattern: "(?<g>a)?(?(<g>)b|c)",
            input: "ab c ac cab",
        },
        ParityCase {
            name: "conditional_named_group_exists_bare",
            pattern: "(?<g>a)?(?(g)b|c)",
            input: "ab c ac cab",
        },
        ParityCase {
            name: "conditional_recursion_any",
            pattern: "a(?(R)b|c)(?R)?d",
            input: "acd xx acabdd yy abd",
        },
        ParityCase {
            name: "conditional_recursion_group",
            pattern: "(a(?(R1)b|c)(?1)?d)",
            input: "acd xx acabdd yy abd",
        },
        ParityCase {
            name: "conditional_recursion_named",
            pattern: "(?<word>a(?(R&word)b|c)(?&word)?d)",
            input: "acd xx acabdd yy abd",
        },
        ParityCase {
            name: "conditional_define_named_subroutine",
            pattern: r"\A(?(DEFINE)(?<word>a+))(?&word)\z",
            input: "aaa",
        },
        ParityCase {
            name: "conditional_relative_group_exists_backward",
            pattern: "(a)?(?(-1)b|c)",
            input: "ab c ac cab",
        },
        ParityCase {
            name: "conditional_relative_group_exists_forward",
            pattern: "(?(+1)a|b)(a)",
            input: "ba aa aba",
        },
        ParityCase {
            name: "conditional_lookahead",
            pattern: "(?(?=ab)a|z)b",
            input: "ab zb xb",
        },
        ParityCase {
            name: "conditional_negative_lookahead",
            pattern: "(?(?!ab)z|a)b",
            input: "ab zb xb",
        },
        ParityCase {
            name: "conditional_lookbehind",
            pattern: "(?(?<=x)a|b)",
            input: "xa b a",
        },
        ParityCase {
            name: "conditional_negative_lookbehind",
            pattern: "(?(?<!x)b|a)",
            input: "xa b a",
        },
    ];

    for case in cases {
        let rgx_first = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx first error: {e}", case.name));
        let pcre2_first = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 first error: {e}", case.name));
        assert_eq!(
            rgx_first, pcre2_first,
            "conditional first-match mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );

        let rgx_all = rgx_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx all error: {e}", case.name));
        let pcre2_all = pcre2_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 all error: {e}", case.name));
        assert_eq!(
            rgx_all, pcre2_all,
            "conditional find_all mismatch for case '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }
}

#[test]
fn pcre2_parity_supported_combined_feature_patterns() {
    let cases = [
        // Nested lookarounds
        ParityCase {
            name: "lookahead_inside_lookbehind",
            pattern: "(?<=(?=a).)b",
            input: "ab xb",
        },
        ParityCase {
            name: "lookbehind_inside_lookahead",
            pattern: "(?=b(?<=ab))ab",
            input: "ab xb cab",
        },
        ParityCase {
            name: "nested_lookahead",
            pattern: "(?=a(?=ab))a",
            input: "aab xaa",
        },
        ParityCase {
            name: "negative_lookahead_in_alternation",
            pattern: "(?!cat)c\\w+|dog",
            input: "cat car cup dog",
        },
        // Atomic groups combined with quantifiers
        ParityCase {
            name: "atomic_greedy_star_no_match",
            pattern: "(?>a*)ab",
            input: "aab aaab",
        },
        ParityCase {
            name: "atomic_alternation_prefix_no_match",
            pattern: "(?>cat|ca)t",
            input: "cat",
        },
        ParityCase {
            name: "atomic_nested_group_no_match",
            pattern: "(?>(?:ab)+)ab",
            input: "ababab abab",
        },
        // Backreference edge cases
        ParityCase {
            name: "backreference_in_alternation",
            pattern: "(a)\\1|bb",
            input: "aa bb ab ba",
        },
        ParityCase {
            name: "backreference_with_quantifier",
            pattern: "(a+)b\\1",
            input: "aba aabaa aabaaa",
        },
        // Possessive combined with alternation
        ParityCase {
            name: "possessive_star_in_alternation",
            pattern: "a*+b|ac",
            input: "ac aab aaac",
        },
        ParityCase {
            name: "possessive_plus_suffix_no_match",
            pattern: "a++ab",
            input: "aaab",
        },
        // Named groups in various positions
        ParityCase {
            name: "named_group_basic",
            pattern: "(?<year>\\d{4})-(?<month>\\d{2})",
            input: "date: 2026-04 end",
        },
        ParityCase {
            name: "named_group_with_alternation",
            pattern: "(?<t>cat|dog)s",
            input: "cats dogs birds",
        },
        // Complex quantifier interactions
        ParityCase {
            name: "nested_quantifiers",
            pattern: "(?:ab?)+c",
            input: "abc aac ababc c",
        },
        ParityCase {
            name: "lazy_inside_greedy",
            pattern: "a(?:b+?c)+",
            input: "abc abbc abbbc",
        },
        ParityCase {
            name: "counted_range_backtracking",
            pattern: "a{2,4}b",
            input: "ab aab aaab aaaab aaaaab",
        },
        // Anchors with groups
        ParityCase {
            name: "anchor_start_group",
            pattern: "^(a|b)c",
            input: "ac bc cc",
        },
        ParityCase {
            name: "anchor_end_alternation",
            pattern: "cat$|dog$",
            input: "the cat",
        },
        ParityCase {
            name: "word_boundary_in_group",
            pattern: "(\\bcat\\b)s?",
            input: "cats cat scat",
        },
        // Dot and character class interactions
        ParityCase {
            name: "dot_with_alternation",
            pattern: ".at|.ot",
            input: "cat hot sit",
        },
        ParityCase {
            name: "char_class_with_quantifier_greedy",
            pattern: "[aeiou]+",
            input: "aei bbb ooo",
        },
        ParityCase {
            name: "nested_char_class_ranges",
            pattern: "[a-zA-Z0-9_]+",
            input: "foo_BAR!! baz123",
        },
        // Anchor default-mode regressions: ^ and $ must NOT be multiline without (?m)
        ParityCase {
            name: "caret_not_multiline_by_default",
            pattern: "^a",
            input: "b\na",
        },
        ParityCase {
            name: "dollar_not_multiline_by_default",
            pattern: "a$",
            input: "a\nb",
        },
        ParityCase {
            name: "caret_only_matches_string_start",
            pattern: "^.",
            input: "a\nb\nc",
        },
        ParityCase {
            name: "dollar_before_final_newline",
            pattern: "a$",
            input: "a\n",
        },
        // Empty-match regressions: patterns that match the empty string
        ParityCase {
            name: "empty_capture_group",
            pattern: "()",
            input: "ab",
        },
        ParityCase {
            name: "empty_first_alternative",
            pattern: "|a",
            input: "b",
        },
        ParityCase {
            name: "empty_middle_alternative",
            pattern: "a||b",
            input: "c",
        },
        ParityCase {
            name: "optional_zero_width",
            pattern: "a?",
            input: "bbb",
        },
        // Zero-width suppression after consuming match (PCRE2 find_all semantics)
        ParityCase {
            name: "star_zero_width_suppressed_after_consuming",
            pattern: "a*",
            input: "aab",
        },
        ParityCase {
            name: "star_zero_width_suppressed_single_char",
            pattern: "a*",
            input: "a",
        },
        ParityCase {
            name: "star_zero_width_suppressed_mixed",
            pattern: "a*",
            input: "bab",
        },
        ParityCase {
            name: "lookahead_alt_zero_width_suppressed",
            pattern: "(?=a)|b",
            input: "ba",
        },
        // Scoped multiline mode (?m:...)
        ParityCase {
            name: "multiline_caret_after_newline",
            pattern: "(?m:^a)",
            input: "b\na",
        },
        ParityCase {
            name: "multiline_dollar_before_newline",
            pattern: "(?m:a$)",
            input: "a\nb",
        },
        ParityCase {
            name: "multiline_caret_at_start",
            pattern: "(?m:^a)",
            input: "abc",
        },
        ParityCase {
            name: "multiline_dollar_at_end",
            pattern: "(?m:a$)",
            input: "ba",
        },
        ParityCase {
            name: "multiline_scoped_no_leak",
            pattern: "(?m:^a)|^b",
            input: "x\nb",
        },
        ParityCase {
            name: "dotall_matches_newline",
            pattern: "(?s:a.b)",
            input: "a\nb axb",
        },
        ParityCase {
            name: "dotall_scoped_no_leak",
            pattern: "(?s:a.b)c.d",
            input: "a\nbcxd",
        },
        ParityCase {
            name: "default_dot_no_newline",
            pattern: "a.b",
            input: "a\nb axb",
        },
    ];

    for case in cases {
        let rgx_first = rgx_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx first error: {e}", case.name));
        let pcre2_first = pcre2_first_span(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 first error: {e}", case.name));
        assert_eq!(
            rgx_first, pcre2_first,
            "combined first-match mismatch for '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );

        let rgx_all = rgx_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] rgx all error: {e}", case.name));
        let pcre2_all = pcre2_all_spans(case.pattern, case.input)
            .unwrap_or_else(|e| panic!("[{}] pcre2 all error: {e}", case.name));
        assert_eq!(
            rgx_all, pcre2_all,
            "combined find_all mismatch for '{}' (pattern '{}', input '{}')",
            case.name, case.pattern, case.input
        );
    }
}
