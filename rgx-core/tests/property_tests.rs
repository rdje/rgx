//! Property-based tests for the rgx regex engine.
//!
//! Uses `proptest` to generate random inputs and verify invariants hold for
//! ALL generated cases.  Each test runs 256+ cases by default.

use proptest::prelude::*;
use rgx_core::Regex;

// =========================================================================
// INVARIANT: Compilation never panics -- returns Ok or Err
// =========================================================================

proptest! {
    #[test]
    fn compile_never_panics_ascii(pattern in "[a-zA-Z0-9.*+?|()\\[\\]{}^$\\\\]{0,50}") {
        let _ = Regex::compile(&pattern);
        // No panic = pass
    }

    #[test]
    fn compile_never_panics_random_bytes(pattern in prop::collection::vec(any::<u8>(), 0..100)) {
        if let Ok(s) = String::from_utf8(pattern) {
            let _ = Regex::compile(&s);
        }
    }
}

// =========================================================================
// INVARIANT: find_first positions are within bounds
// =========================================================================

proptest! {
    #[test]
    fn find_first_positions_within_bounds(
        pattern in "(a|b|c|\\d|\\w|.){1,5}",
        input in "[a-zA-Z0-9 ]{0,100}"
    ) {
        if let Ok(re) = Regex::compile(&pattern) {
            if let Some(m) = re.find_first(&input) {
                prop_assert!(m.start <= m.end,
                    "start {} must be <= end {}", m.start, m.end);
                prop_assert!(m.end <= input.len(),
                    "end {} must be <= input len {}", m.end, input.len());
                prop_assert!(input.is_char_boundary(m.start),
                    "start {} must be a char boundary", m.start);
                prop_assert!(input.is_char_boundary(m.end),
                    "end {} must be a char boundary", m.end);
            }
        }
    }
}

// =========================================================================
// INVARIANT: find_all results are non-overlapping and sorted
// =========================================================================

proptest! {
    #[test]
    fn find_all_non_overlapping_sorted(
        pattern in "(a|b|\\d|\\w|.){1,3}",
        input in "[a-zA-Z0-9 ]{0,200}"
    ) {
        if let Ok(re) = Regex::compile(&pattern) {
            let matches = re.find_all(&input);
            for i in 1..matches.len() {
                prop_assert!(matches[i].start >= matches[i-1].end,
                    "overlapping matches at index {}: prev.end={} >= curr.start={}",
                    i, matches[i-1].end, matches[i].start);
            }
            for m in &matches {
                prop_assert!(m.start <= m.end);
                prop_assert!(m.end <= input.len());
            }
        }
    }
}

// =========================================================================
// INVARIANT: is_match agrees with find_first
// =========================================================================

proptest! {
    #[test]
    fn is_match_agrees_with_find_first(
        pattern in "(a|b|c|\\d|.){1,5}",
        input in "[a-zA-Z0-9]{0,50}"
    ) {
        if let Ok(re) = Regex::compile(&pattern) {
            let has_match = re.find_first(&input).is_some();
            prop_assert_eq!(re.is_match(&input), has_match);
        }
    }
}

// =========================================================================
// INVARIANT: find_all with same input produces same results (determinism)
// =========================================================================

proptest! {
    #[test]
    fn find_all_is_deterministic(
        pattern in "[a-z.]+",
        input in "[a-z ]{0,100}"
    ) {
        if let Ok(re) = Regex::compile(&pattern) {
            let r1: Vec<_> = re.find_all(&input).iter().map(|m| (m.start, m.end)).collect();
            let r2: Vec<_> = re.find_all(&input).iter().map(|m| (m.start, m.end)).collect();
            prop_assert_eq!(r1, r2, "find_all should be deterministic");
        }
    }
}

// =========================================================================
// INVARIANT: matched text is valid UTF-8 substring of input
// =========================================================================

proptest! {
    #[test]
    fn matched_text_is_valid_substring(
        pattern in "[a-z]{1,3}",
        input in "[a-zA-Z0-9 ]{0,100}"
    ) {
        if let Ok(re) = Regex::compile(&pattern) {
            for m in re.find_all(&input) {
                let slice = &input[m.start..m.end];
                // Verify it's a valid substring (no broken UTF-8)
                let _ = slice.to_string();
            }
        }
    }
}

// =========================================================================
// INVARIANT: branch_number is within range when present
// =========================================================================

proptest! {
    #[test]
    fn branch_number_within_range(
        n_branches in 2..10usize,
        input in "[a-z]{0,20}"
    ) {
        let branches: Vec<String> = (0..n_branches).map(|i| {
            ((b'a' + (i as u8 % 26)) as char).to_string()
        }).collect();
        let pattern = branches.join("|");
        if let Ok(re) = Regex::compile(&pattern) {
            if let Some(m) = re.find_first(&input) {
                if let Some(bn) = m.matched_branch_number {
                    prop_assert!(bn >= 1 && bn <= n_branches,
                        "branch {} out of range 1..={}", bn, n_branches);
                }
            }
        }
    }
}

// =========================================================================
// INVARIANT: find_all on empty input returns no matches (for non-empty patterns)
// =========================================================================

proptest! {
    #[test]
    fn find_all_empty_input_no_matches(
        pattern in "[a-z]{1,5}"
    ) {
        if let Ok(re) = Regex::compile(&pattern) {
            let matches = re.find_all("");
            prop_assert!(matches.is_empty(),
                "non-empty pattern '{}' should not match empty input", pattern);
        }
    }
}

// =========================================================================
// INVARIANT: find_first match is the earliest possible
// =========================================================================

proptest! {
    #[test]
    fn find_first_is_earliest_match(
        pattern in "[a-z]{1,3}",
        input in "[a-zA-Z0-9 ]{0,100}"
    ) {
        if let Ok(re) = Regex::compile(&pattern) {
            if let Some(first) = re.find_first(&input) {
                let all = re.find_all(&input);
                if let Some(earliest) = all.first() {
                    prop_assert_eq!(first.start, earliest.start,
                        "find_first should return the earliest match");
                }
            }
        }
    }
}

// =========================================================================
// INVARIANT: find_all matches are non-empty for non-zero-width patterns
// =========================================================================

proptest! {
    #[test]
    fn find_all_matches_non_empty_for_fixed_patterns(
        pattern in "[a-z]{1,4}",
        input in "[a-zA-Z0-9 ]{0,100}"
    ) {
        if let Ok(re) = Regex::compile(&pattern) {
            for m in re.find_all(&input) {
                prop_assert!(m.start < m.end,
                    "character-class pattern should produce non-zero-width matches, got start={} end={}", m.start, m.end);
            }
        }
    }
}
