//! Differential test harness for the C2 Pike-VM against the existing
//! backtracking VM.
//!
//! This is the merge gate that the C2 phased plan in
//! `docs/C2_NFA_DFA_DESIGN.md` §15 promises starting at step 4: "every
//! classifier-positive pattern in the existing test suite must produce
//! identical results on Pike-VM and the existing backtracking VM."
//!
//! For C2 step 4a the harness compares **match spans only** (start and
//! end byte positions). Capture group recovery lands in step 4b along
//! with engine dispatch wiring; the test corpus here will be extended
//! to compare capture positions at that point.
//!
//! # How it works
//!
//! For each `(pattern, input)` pair in the corpus:
//!
//! 1. Compile the pattern via `Regex::compile(...)` (the existing
//!    backtracking VM path).
//! 2. Try to compile the same pattern via
//!    `CompiledC2Program::try_compile(...)` (the C2 path). If the
//!    pattern is outside the C2 no-backtracking subset, the harness
//!    skips it — those patterns continue to run on the existing VM and
//!    don't reach the Pike-VM.
//! 3. Run both engines on the input.
//! 4. Assert that `pike_find_first` returns the same `(start, end)` as
//!    the existing VM's `find_first`, and that `pike_find_all` returns
//!    the same list of spans as the existing VM's `find_all`.
//! 5. Assert that `pike_is_match` agrees with the existing VM's `is_match`.
//!
//! Any disagreement is a Pike-VM correctness bug.

use rgx_core::c2::{
    pike_captures, pike_captures_all, pike_find_all, pike_find_first, pike_is_match,
    CompiledC2Program,
};
use rgx_core::Regex as RgxRegex;

/// One differential test case: a pattern, an input string, and a short
/// human-readable name for diagnostics.
struct Case {
    name: &'static str,
    pattern: &'static str,
    input: &'static str,
}

/// Compile the pattern with both engines and assert that Pike-VM and
/// the existing VM agree on `is_match`, `find_first`, and `find_all`.
///
/// Patterns outside the C2 subset are skipped silently — those route to
/// the existing VM exclusively and don't exercise the Pike-VM at all.
fn assert_pike_matches_vm(case: &Case) {
    let vm_regex = RgxRegex::compile(case.pattern)
        .unwrap_or_else(|err| panic!("[{}] vm compile failed: {err}", case.name));

    let Some(c2_program) = CompiledC2Program::try_compile(case.pattern) else {
        // Outside the C2 subset — nothing to compare. Real-world dispatch
        // routes these to the existing VM unchanged.
        return;
    };

    let input_bytes = case.input.as_bytes();

    // is_match agreement
    let vm_is_match = vm_regex.is_match(case.input);
    let pike_is = pike_is_match(&c2_program, input_bytes);
    assert_eq!(
        vm_is_match, pike_is,
        "[{}] is_match disagreement: vm={vm_is_match}, pike={pike_is} for pattern '{}' on '{}'",
        case.name, case.pattern, case.input
    );

    // find_first agreement
    let vm_first = vm_regex.find_first(case.input).map(|m| (m.start, m.end));
    let pike_first = pike_find_first(&c2_program, input_bytes);
    assert_eq!(
        vm_first, pike_first,
        "[{}] find_first disagreement: vm={vm_first:?}, pike={pike_first:?} \
         for pattern '{}' on '{}'",
        case.name, case.pattern, case.input
    );

    // find_all agreement
    let vm_all: Vec<(usize, usize)> = vm_regex
        .find_all(case.input)
        .into_iter()
        .map(|m| (m.start, m.end))
        .collect();
    let pike_all = pike_find_all(&c2_program, input_bytes);
    assert_eq!(
        vm_all, pike_all,
        "[{}] find_all disagreement: vm={vm_all:?}, pike={pike_all:?} \
         for pattern '{}' on '{}'",
        case.name, case.pattern, case.input
    );

    // captures agreement (first match): both engines should agree on the
    // overall span AND on each capture group's span (or `None` for
    // groups that didn't participate).
    let vm_captures_first = vm_regex
        .find_first(case.input)
        .map(|m| (m.start, m.end, m.groups.clone()));
    let pike_captures_first =
        pike_captures(&c2_program, input_bytes).map(|m| (m.start, m.end, m.groups));
    assert_eq!(
        vm_captures_first, pike_captures_first,
        "[{}] captures(first) disagreement: vm={vm_captures_first:?}, pike={pike_captures_first:?} \
         for pattern '{}' on '{}'",
        case.name, case.pattern, case.input
    );

    // captures agreement (all matches): same comparison applied across
    // every non-overlapping match.
    let vm_captures_all_vec: Vec<(usize, usize, Vec<Option<(usize, usize)>>)> = vm_regex
        .find_all(case.input)
        .into_iter()
        .map(|m| (m.start, m.end, m.groups))
        .collect();
    let pike_captures_all_vec: Vec<(usize, usize, Vec<Option<(usize, usize)>>)> =
        pike_captures_all(&c2_program, input_bytes)
            .into_iter()
            .map(|m| (m.start, m.end, m.groups))
            .collect();
    assert_eq!(
        vm_captures_all_vec, pike_captures_all_vec,
        "[{}] captures(all) disagreement: vm={vm_captures_all_vec:?}, pike={pike_captures_all_vec:?} \
         for pattern '{}' on '{}'",
        case.name, case.pattern, case.input
    );
}

fn run_corpus(cases: &[Case]) {
    let mut tested = 0usize;
    let mut skipped = 0usize;
    for case in cases {
        // Mirror try_compile's "in subset?" check so we can count.
        if CompiledC2Program::try_compile(case.pattern).is_some() {
            tested += 1;
        } else {
            skipped += 1;
            continue;
        }
        assert_pike_matches_vm(case);
    }
    eprintln!(
        "[c2_pike_differential] tested {tested} cases, skipped {skipped} (outside C2 subset)"
    );
    assert!(
        tested > 0,
        "no cases reached the differential check; corpus is empty or entirely out-of-subset"
    );
}

// ============================================================
// Corpus
// ============================================================

#[test]
fn literals_match_identically() {
    run_corpus(&[
        Case {
            name: "literal_match_at_start",
            pattern: "hello",
            input: "hello world",
        },
        Case {
            name: "literal_match_in_middle",
            pattern: "world",
            input: "hello world",
        },
        Case {
            name: "literal_no_match",
            pattern: "xyz",
            input: "hello world",
        },
        Case {
            name: "single_char_literal",
            pattern: "a",
            input: "banana",
        },
        Case {
            name: "empty_input",
            pattern: "abc",
            input: "",
        },
    ]);
}

#[test]
fn character_classes_match_identically() {
    run_corpus(&[
        Case {
            name: "lowercase_class",
            pattern: r"[a-z]",
            input: "ABC123abc",
        },
        Case {
            name: "digit_shorthand",
            pattern: r"\d",
            input: "abc123def",
        },
        Case {
            name: "word_shorthand",
            pattern: r"\w",
            input: "  _abc",
        },
        Case {
            name: "space_shorthand",
            pattern: r"\s",
            input: "no_spaces_yet here",
        },
        Case {
            name: "negated_digit",
            pattern: r"\D",
            input: "1234x",
        },
        Case {
            name: "negated_class",
            pattern: r"[^aeiou]",
            input: "aeixo",
        },
    ]);
}

#[test]
fn sequences_and_alternations_match_identically() {
    run_corpus(&[
        Case {
            name: "three_letter_sequence",
            pattern: "abc",
            input: "xxabcyy",
        },
        Case {
            name: "alternation_first_branch",
            pattern: "cat|dog|fish",
            input: "i love cats",
        },
        Case {
            name: "alternation_third_branch",
            pattern: "cat|dog|fish",
            input: "school of fish",
        },
        Case {
            name: "alternation_no_match",
            pattern: "cat|dog",
            input: "no animals here",
        },
        Case {
            name: "longer_alternation",
            pattern: "ERROR|WARN|INFO|DEBUG",
            input: "INFO: started; WARN: slow; ERROR: fail",
        },
    ]);
}

#[test]
fn greedy_quantifiers_match_identically() {
    run_corpus(&[
        Case {
            name: "star_runs_to_end",
            pattern: "a*",
            input: "aaab",
        },
        Case {
            name: "plus_run",
            pattern: "a+",
            input: "baaab",
        },
        Case {
            name: "optional_present",
            pattern: "ab?c",
            input: "abc",
        },
        Case {
            name: "optional_absent",
            pattern: "ab?c",
            input: "ac",
        },
        Case {
            name: "digit_run",
            pattern: r"\d+",
            input: "abc 12345 def",
        },
    ]);
}

#[test]
fn lazy_quantifiers_match_identically() {
    run_corpus(&[
        Case {
            name: "lazy_star",
            pattern: "a*?",
            input: "aaab",
        },
        Case {
            name: "lazy_plus",
            pattern: "a+?",
            input: "baaab",
        },
        Case {
            name: "lazy_optional",
            pattern: "ab??c",
            input: "abc",
        },
    ]);
}

#[test]
fn range_quantifiers_match_identically() {
    run_corpus(&[
        Case {
            name: "exact_count",
            pattern: r"\d{4}",
            input: "year 2026 and 12345",
        },
        Case {
            name: "min_max",
            pattern: r"\d{2,4}",
            input: "abc 1 22 333 4444 55555 def",
        },
        Case {
            name: "min_only",
            pattern: r"\d{3,}",
            input: "1 22 333 4444 55555",
        },
    ]);
}

#[test]
fn anchors_match_identically() {
    run_corpus(&[
        Case {
            name: "abs_start",
            pattern: r"\Aabc",
            input: "abc def",
        },
        Case {
            name: "abs_start_no_match",
            pattern: r"\Aabc",
            input: "xx abc",
        },
        Case {
            name: "abs_end_no_nl",
            pattern: r"abc\z",
            input: "def abc",
        },
        Case {
            name: "abs_end_no_match",
            pattern: r"abc\z",
            input: "abc def",
        },
        Case {
            name: "anchored_full",
            pattern: r"\Ahello\z",
            input: "hello",
        },
    ]);
}

#[test]
fn word_boundaries_match_identically() {
    run_corpus(&[
        Case {
            name: "word_boundary_match",
            pattern: r"\bcat\b",
            input: "the cat sat",
        },
        Case {
            name: "word_boundary_no_match_substring",
            pattern: r"\bcat\b",
            input: "category",
        },
        Case {
            name: "word_boundary_no_match_prefix",
            pattern: r"\bcat\b",
            input: "scattered",
        },
    ]);
}

#[test]
fn capturing_groups_match_identically() {
    // At step 4a, we only compare match SPANS — capture group positions
    // are deferred to step 4b. These cases verify that capturing-group
    // patterns produce the same overall match span on both engines even
    // though Pike-VM doesn't track captures yet.
    run_corpus(&[
        Case {
            name: "single_group",
            pattern: r"(\w+)",
            input: "  hello world",
        },
        Case {
            name: "multiple_groups",
            pattern: r"(\d+)-(\d+)",
            input: "year 12-34 day",
        },
        Case {
            name: "named_group",
            pattern: r"(?<word>\w+)",
            input: "hello",
        },
        Case {
            name: "non_capturing_group",
            pattern: r"(?:abc){2}",
            input: "abcabcdef",
        },
    ]);
}

#[test]
fn realistic_patterns_match_identically() {
    run_corpus(&[
        Case {
            name: "iso_date",
            pattern: r"\d{4}-\d{2}-\d{2}",
            input: "today is 2026-04-10 ok",
        },
        Case {
            name: "phone_number",
            pattern: r"\d{3}-\d{3}-\d{4}",
            input: "call 555-867-5309 maybe",
        },
        Case {
            name: "log_line_prefix",
            pattern: r"\[(ERROR|WARN|INFO)\]",
            input: "[INFO] started\n[WARN] slow\n[ERROR] failed\n",
        },
        Case {
            name: "ipv4_like",
            pattern: r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}",
            input: "host 192.168.1.42 ok",
        },
    ]);
}

#[test]
fn empty_match_patterns_match_identically() {
    run_corpus(&[
        Case {
            name: "empty_alt_branch",
            pattern: "a|",
            input: "bcd",
        },
        Case {
            name: "star_only",
            pattern: "a*",
            input: "bbb",
        },
    ]);
}

#[test]
fn multi_byte_utf8_patterns_match_identically() {
    run_corpus(&[
        Case {
            name: "two_byte_literal",
            pattern: "α",
            input: "x α y",
        },
        Case {
            name: "three_byte_literal",
            pattern: "あ",
            input: "x あ y",
        },
        Case {
            name: "two_byte_class",
            pattern: r"[α-ω]",
            input: "abc α def β",
        },
    ]);
}
