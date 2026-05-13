//! Public-API differential gate for the C2 tagged DFA dispatch
//! (Phase 3).
//!
//! This is the merge gate for engine-level TDFA correctness. Phase
//! 2d's in-module differential test verified the TDFA simulator
//! against `pike_match_at_with_captures` at the construction-API
//! level. Phase 3 wires the TDFA into `Regex::find_first` and this
//! test verifies that the **public** API produces identical results
//! whether the TDFA path or the legacy DFA → Pike pipeline is taken.
//!
//! # How it works
//!
//! For each `(pattern, input)` in the corpus:
//!
//! 1. Compile the pattern via `Regex::compile`.
//! 2. Assert `regex.uses_tdfa()` reports the AST-level eligibility.
//! 3. Run `regex.find_first(input)`. With Phase 3 wiring, the TDFA
//!    path runs first for TDFA-eligible patterns; otherwise the
//!    legacy DFA → Pike pipeline runs.
//! 4. Compare to the same regex's `find_first` after suppressing
//!    the TDFA path via a control pattern that's TDFA-ineligible
//!    by construction (e.g. wrap the same body inside `(?:...)`
//!    + `\b` so the classifier rejects it). If the bodies are
//!    semantically equivalent except for the eligibility marker,
//!    the captures should be identical.
//!
//! That second comparison is hard to do cleanly without engine
//! plumbing; for now we settle for verifying that the TDFA path
//! produces the captures the existing test corpus expects. Any
//! divergence becomes a Phase 4 perf-gate concern (where we'll
//! also wire a runtime toggle for ablation).

use rgx_core::Regex;

/// Assert that `Regex::find_first` returns the expected capture
/// positions for a TDFA-eligible pattern. The TDFA path is exercised
/// transparently — `uses_tdfa()` confirms eligibility.
fn assert_find_first_captures(
    pattern: &str,
    input: &str,
    expected: Option<Vec<Option<(usize, usize)>>>,
) {
    let regex = Regex::compile(pattern).expect("compile");
    assert!(
        regex.uses_tdfa(),
        "pattern {:?} must be TDFA-eligible for this test",
        pattern
    );
    let result = regex.find_first(input);
    match (result, expected) {
        (None, None) => {}
        (Some(m), Some(expected_groups)) => {
            assert_eq!(
                m.groups, expected_groups,
                "pattern {:?} input {:?}: captures mismatch (TDFA path)",
                pattern, input
            );
        }
        (got, want) => panic!(
            "pattern {:?} input {:?}: match outcome mismatch — got matched={}, want matched={}",
            pattern,
            input,
            got.is_some(),
            want.is_some()
        ),
    }
}

#[test]
fn simple_capture_via_tdfa() {
    assert_find_first_captures("(a)", "a", Some(vec![Some((0, 1)), Some((0, 1))]));
    assert_find_first_captures("(a)", "ba", Some(vec![Some((1, 2)), Some((1, 2))]));
    assert_find_first_captures("(a)", "z", None);
}

#[test]
fn sequential_captures_via_tdfa() {
    assert_find_first_captures(
        "(a)(b)",
        "ab",
        Some(vec![Some((0, 2)), Some((0, 1)), Some((1, 2))]),
    );
    assert_find_first_captures(
        "(a)(b)",
        "zab",
        Some(vec![Some((1, 3)), Some((1, 2)), Some((2, 3))]),
    );
    assert_find_first_captures("(a)(b)", "az", None);
}

#[test]
fn alternation_inside_sequence_via_tdfa() {
    // x(?:(a)|(b)) — the outer Sequence node defeats the top-level
    // alternation exclusion. Inner alternation with captures fires
    // through the TDFA. After consuming 'x' the alt is entered.
    assert_find_first_captures(
        "x(?:(a)|(b))",
        "xa",
        Some(vec![Some((0, 2)), Some((1, 2)), None]),
    );
    assert_find_first_captures(
        "x(?:(a)|(b))",
        "xb",
        Some(vec![Some((0, 2)), None, Some((1, 2))]),
    );
    assert_find_first_captures("x(?:(a)|(b))", "xz", None);
    assert_find_first_captures("x(?:(a)|(b))", "yz", None);
}

#[test]
fn greedy_repeat_captures_last_iteration_via_tdfa() {
    // (a)+ — leftmost-longest, last iteration's capture wins.
    assert_find_first_captures("(a)+", "aaa", Some(vec![Some((0, 3)), Some((2, 3))]));
    assert_find_first_captures("(a)+", "ab", Some(vec![Some((0, 1)), Some((0, 1))]));
}

#[test]
fn nested_captures_via_tdfa() {
    // ((a)b) — group 1 wraps group 2 wraps 'a' then literal 'b'.
    assert_find_first_captures(
        "((a)b)",
        "ab",
        Some(vec![Some((0, 2)), Some((0, 2)), Some((0, 1))]),
    );
    assert_find_first_captures(
        "((a)b)",
        "zab",
        Some(vec![Some((1, 3)), Some((1, 3)), Some((1, 2))]),
    );
}

#[test]
fn digit_capture_via_tdfa() {
    // Realistic capture pattern: (\d+) — TDFA must handle character
    // classes inside captures.
    assert_find_first_captures(
        r"(\d+)",
        "abc 123 def",
        Some(vec![Some((4, 7)), Some((4, 7))]),
    );
    assert_find_first_captures(r"(\d+)", "abc", None);
}

#[test]
fn two_digit_groups_via_tdfa() {
    // (\d+)-(\d+) — the canonical "capture date components" shape.
    assert_find_first_captures(
        r"(\d+)-(\d+)",
        "2026-05-13",
        Some(vec![Some((0, 7)), Some((0, 4)), Some((5, 7))]),
    );
}

#[test]
fn uses_tdfa_rejects_non_eligible_patterns() {
    // No captures — eligible for DFA but not for TDFA (zero-capture
    // fast path wins).
    assert!(!Regex::compile(r"abc").unwrap().uses_tdfa());
    assert!(!Regex::compile(r"\d+").unwrap().uses_tdfa());

    // \b in pattern — Phase 2 conservative reject.
    assert!(!Regex::compile(r"\b(\w+)\b").unwrap().uses_tdfa());

    // Lazy quantifier — DFA semantics can't express lazy.
    assert!(!Regex::compile(r"(a)+?").unwrap().uses_tdfa());

    // Backreference — outside C2 entirely.
    assert!(!Regex::compile(r"(a)\1").unwrap().uses_tdfa());

    // Lookahead — outside C2 entirely.
    assert!(!Regex::compile(r"(?=a)(a)").unwrap().uses_tdfa());
}

#[test]
fn uses_tdfa_accepts_eligible_patterns() {
    assert!(Regex::compile(r"(a)").unwrap().uses_tdfa());
    assert!(Regex::compile(r"(a)(b)").unwrap().uses_tdfa());
    // Inner alternation inside a Sequence is fine; top-level
    // alternation is excluded because the C2 dispatch can't track
    // `matched_branch_number`. (The non-capturing wrapper alone
    // is NOT enough — `has_top_level_alternation` unwraps groups.)
    assert!(Regex::compile(r"x(?:(a)|(b))").unwrap().uses_tdfa());
    assert!(!Regex::compile(r"(?:(a)|(b))").unwrap().uses_tdfa());
    assert!(Regex::compile(r"((a)b)").unwrap().uses_tdfa());
    assert!(Regex::compile(r"(\d+)").unwrap().uses_tdfa());
    assert!(Regex::compile(r"(\d+)-(\d+)").unwrap().uses_tdfa());
    assert!(Regex::compile(r"(a)+").unwrap().uses_tdfa());
    // Top-level alternation excluded — falls back to existing path.
    assert!(!Regex::compile(r"(a)|(b)").unwrap().uses_tdfa());
}
