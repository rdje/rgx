//! Aho-Corasick dispatch for top-level literal-alternation patterns.
//!
//! Patterns of the shape `cat|dog|bird` (a top-level alternation
//! whose branches are all pure literal byte sequences) are
//! traditionally a worst case for backtracking VMs: the engine must
//! try each branch at each scan position, giving an `O(n × m)` cost
//! where `n` is input length and `m` is the number of alternatives.
//! The Aho-Corasick automaton matches all `m` alternatives against
//! all `n` input bytes in a single `O(n + m)` pass.
//!
//! These patterns are excluded from C2 dispatch by
//! [`crate::c2::program::is_c2_dispatch_eligible`] (because the
//! existing VM relies on `matched_branch_number` for top-level
//! alternation, which Pike-VM doesn't track), so without AC dispatch
//! they fall through to the backtracking VM. Adding an AC fast path
//! at the top of the dispatch chain handles them in linear time.
//!
//! # Eligibility
//!
//! A pattern qualifies for AC dispatch iff:
//! - The top-level AST (after walking past capturing/non-capturing
//!   groups; flag groups disqualify because case-folding requires
//!   different AC configuration) is `Regex::Alternation(branches)`.
//! - Every branch is a single `Regex::Char` or a
//!   `Regex::Sequence` whose children are all `Regex::Char`. No
//!   classes, no quantifiers, no nested alternations, no anchors.
//! - Every character is single-byte ASCII. Multi-byte UTF-8 codepoints
//!   would need byte-level escaping that complicates pattern-id
//!   bookkeeping for the `matched_branch_number` field.
//! - The branch list is non-empty and no branch is empty.
//!
//! Patterns that don't qualify return `None` from
//! [`extract_literal_alternation`] and continue down the regular
//! dispatch chain.
//!
//! # Match semantics
//!
//! AC is built with `MatchKind::LeftmostFirst` so the alternation
//! semantics match PCRE2: when two branches could match at the same
//! position, the first one in source order wins. The `pattern_id`
//! returned by AC is 0-based; the `matched_branch_number` field on
//! `MatchResult` is 1-based, so the dispatch wrapper adds 1.

use crate::ast::Regex;
use aho_corasick::{AhoCorasick, MatchKind};

/// Walk the top-level AST and return `Some(literal_set)` if the
/// pattern is a top-level alternation of pure ASCII literals,
/// suitable for Aho-Corasick dispatch. Returns `None` otherwise.
///
/// "Top level" means: walk through `Regex::Group { Capturing |
/// NonCapturing, .. }` wrappers. `FlagGroup` wrappers disqualify
/// (the `(?i)` flag would change AC configuration; v1 keeps the
/// extractor flag-free).
///
/// Each branch must be either a single `Regex::Char(c)` or a
/// `Regex::Sequence` of `Regex::Char` items, all single-byte ASCII.
#[must_use]
pub fn extract_literal_alternation(ast: &Regex) -> Option<Vec<Vec<u8>>> {
    let alternation_branches = top_level_alternation_branches(ast)?;
    // Reject single-branch alternations — those have no real choice
    // and the existing dispatch contract treats them with
    // `matched_branch_number = None`. Falling through preserves that.
    if alternation_branches.len() < 2 {
        return None;
    }
    let mut result = Vec::with_capacity(alternation_branches.len());
    for branch in alternation_branches {
        let bytes = pure_ascii_literal(branch)?;
        if bytes.is_empty() {
            // Reject empty branches — AC's leftmost-first semantics
            // get awkward when an empty pattern is in the set.
            return None;
        }
        result.push(bytes);
    }
    Some(result)
}

/// Return the alternation branches of `ast` if the top-level node
/// (modulo capturing/non-capturing group wrappers) is an
/// `Alternation`. `FlagGroup` wrappers disqualify.
fn top_level_alternation_branches(ast: &Regex) -> Option<&[Regex]> {
    match ast {
        Regex::Alternation(branches) => Some(branches),
        Regex::Group { kind, expr, .. } => match kind {
            crate::ast::GroupKind::Capturing | crate::ast::GroupKind::NonCapturing => {
                top_level_alternation_branches(expr)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Return the byte sequence for `ast` iff it is a pure ASCII
/// literal — a single `Char` or a `Sequence` of `Char`s, all
/// single-byte ASCII codepoints. `None` otherwise.
fn pure_ascii_literal(ast: &Regex) -> Option<Vec<u8>> {
    match ast {
        Regex::Char(c) => {
            if c.is_ascii() {
                Some(vec![*c as u8])
            } else {
                None
            }
        }
        Regex::Sequence(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Regex::Char(c) if c.is_ascii() => bytes.push(*c as u8),
                    _ => return None,
                }
            }
            Some(bytes)
        }
        _ => None,
    }
}

/// Build an Aho-Corasick automaton from the literal set, configured
/// for leftmost-first match semantics (matching PCRE2's alternation
/// rules). Returns `None` only if AC construction fails (which on
/// `aho-corasick = "1.x"` is essentially impossible for a non-empty
/// non-overlapping literal set, but the API is fallible).
#[must_use]
pub fn build_aho_corasick(literals: &[Vec<u8>]) -> Option<AhoCorasick> {
    AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostFirst)
        .build(literals)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::GroupKind;

    fn lit(c: char) -> Regex {
        Regex::Char(c)
    }

    fn seq(items: Vec<Regex>) -> Regex {
        Regex::Sequence(items)
    }

    fn alt(branches: Vec<Regex>) -> Regex {
        Regex::Alternation(branches)
    }

    #[test]
    fn extracts_three_pure_literal_branches() {
        let ast = alt(vec![
            seq(vec![lit('c'), lit('a'), lit('t')]),
            seq(vec![lit('d'), lit('o'), lit('g')]),
            seq(vec![lit('b'), lit('i'), lit('r'), lit('d')]),
        ]);
        let extracted = extract_literal_alternation(&ast);
        assert_eq!(
            extracted,
            Some(vec![b"cat".to_vec(), b"dog".to_vec(), b"bird".to_vec()])
        );
    }

    #[test]
    fn extracts_through_capturing_group() {
        let inner = alt(vec![lit('a'), lit('b')]);
        let ast = Regex::Group {
            kind: GroupKind::Capturing,
            index: Some(1),
            name: None,
            expr: Box::new(inner),
        };
        let extracted = extract_literal_alternation(&ast);
        assert_eq!(extracted, Some(vec![vec![b'a'], vec![b'b']]));
    }

    #[test]
    fn extracts_through_non_capturing_group() {
        let inner = alt(vec![lit('x'), lit('y')]);
        let ast = Regex::Group {
            kind: GroupKind::NonCapturing,
            index: None,
            name: None,
            expr: Box::new(inner),
        };
        let extracted = extract_literal_alternation(&ast);
        assert_eq!(extracted, Some(vec![vec![b'x'], vec![b'y']]));
    }

    #[test]
    fn rejects_through_flag_group() {
        // (?i)cat|dog — flag group wraps the alternation. Currently
        // disqualifies because v1 doesn't configure case-insensitive AC.
        let inner = alt(vec![lit('a'), lit('b')]);
        let ast = Regex::FlagGroup {
            flags: "i".to_string(),
            expr: Box::new(inner),
        };
        assert_eq!(extract_literal_alternation(&ast), None);
    }

    #[test]
    fn rejects_branch_with_class() {
        // `cat|do[g]` — the second branch contains a class.
        let ast = alt(vec![
            seq(vec![lit('c'), lit('a'), lit('t')]),
            seq(vec![
                lit('d'),
                lit('o'),
                Regex::CharClass(crate::ast::CharClass::Custom {
                    ranges: vec![crate::ast::CharRange {
                        start: 'g',
                        end: 'g',
                    }],
                    negated: false,
                    ci_override_ranges: None,
                }),
            ]),
        ]);
        assert_eq!(extract_literal_alternation(&ast), None);
    }

    #[test]
    fn rejects_branch_with_quantifier() {
        // `cat|dog+` — second branch has a quantifier.
        let ast = alt(vec![
            seq(vec![lit('c'), lit('a'), lit('t')]),
            seq(vec![
                lit('d'),
                lit('o'),
                Regex::Quantified {
                    expr: Box::new(lit('g')),
                    quantifier: crate::ast::Quantifier::OneOrMore { lazy: false },
                },
            ]),
        ]);
        assert_eq!(extract_literal_alternation(&ast), None);
    }

    #[test]
    fn rejects_branch_with_non_ascii_codepoint() {
        // `α|β` — multi-byte UTF-8 codepoints. v1 sticks to ASCII.
        let ast = alt(vec![lit('α'), lit('β')]);
        assert_eq!(extract_literal_alternation(&ast), None);
    }

    #[test]
    fn rejects_non_alternation_top_level() {
        let ast = seq(vec![lit('h'), lit('i')]);
        assert_eq!(extract_literal_alternation(&ast), None);
    }

    #[test]
    fn rejects_empty_alternation() {
        let ast = alt(vec![]);
        assert_eq!(extract_literal_alternation(&ast), None);
    }

    #[test]
    fn rejects_single_branch_alternation() {
        // Single-branch alternations have no real choice; the
        // existing dispatch contract returns matched_branch_number=None
        // for them, and AC's pattern_id=1 would violate that.
        let ast = alt(vec![seq(vec![lit('c'), lit('a'), lit('t')])]);
        assert_eq!(extract_literal_alternation(&ast), None);
    }

    #[test]
    fn ac_construction_succeeds_for_extracted_set() {
        let ast = alt(vec![
            seq(vec![lit('c'), lit('a'), lit('t')]),
            seq(vec![lit('d'), lit('o'), lit('g')]),
        ]);
        let literals = extract_literal_alternation(&ast).expect("eligible");
        let ac = build_aho_corasick(&literals).expect("built");
        // Sanity: AC finds matches in input.
        let m = ac.find("the dog runs").expect("match");
        assert_eq!(m.start(), 4);
        assert_eq!(m.end(), 7);
        assert_eq!(m.pattern().as_usize(), 1); // dog is the second pattern
    }

    #[test]
    fn ac_leftmost_first_semantics_match_pcre2_alternation() {
        // For `a|abc` on input "abc", PCRE2 returns "a" (first
        // alternative wins). LeftmostLongest would return "abc".
        let ast = alt(vec![lit('a'), seq(vec![lit('a'), lit('b'), lit('c')])]);
        let literals = extract_literal_alternation(&ast).expect("eligible");
        let ac = build_aho_corasick(&literals).expect("built");
        let m = ac.find("abc").expect("match");
        assert_eq!(m.start(), 0);
        assert_eq!(m.end(), 1); // "a" not "abc"
        assert_eq!(m.pattern().as_usize(), 0);
    }
}
