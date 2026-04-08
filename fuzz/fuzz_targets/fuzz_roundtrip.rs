//! Fuzz target: invariant checks on match results.
//!
//! Goal: verify that match positions are valid, non-overlapping, and within bounds.
//! Any violation is a bug.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    pattern: String,
    text: String,
}

fuzz_target!(|input: FuzzInput| {
    let Ok(re) = rgx_core::Regex::compile(&input.pattern) else {
        return;
    };
    re.set_max_steps(Some(50_000));

    let matches = re.find_all(&input.text);

    // Invariant 1: all match positions are within bounds.
    for m in &matches {
        assert!(m.start <= m.end, "start > end: {} > {}", m.start, m.end);
        assert!(
            m.end <= input.text.len(),
            "end out of bounds: {} > {}",
            m.end,
            input.text.len()
        );
    }

    // Invariant 2: matches are non-overlapping and ordered.
    for pair in matches.windows(2) {
        assert!(
            pair[0].end <= pair[1].start,
            "overlapping matches: {}..{} and {}..{}",
            pair[0].start,
            pair[0].end,
            pair[1].start,
            pair[1].end
        );
    }

    // Invariant 3: is_match agrees with find_first.
    let has_match = re.is_match(&input.text);
    let has_first = re.find_first(&input.text).is_some();
    assert_eq!(
        has_match, has_first,
        "is_match ({has_match}) disagrees with find_first ({has_first})"
    );

    // Invariant 4: split produces valid UTF-8 slices that cover the input.
    let parts = re.split(&input.text);
    if matches.is_empty() {
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], input.text.as_str());
    }

    // Invariant 5: capture group 0 matches the overall match span.
    if let Some(m) = re.find_first(&input.text) {
        if !m.groups.is_empty() {
            if let Some((s, e)) = m.groups[0] {
                assert_eq!(s, m.start);
                assert_eq!(e, m.end);
            }
        }
    }
});
