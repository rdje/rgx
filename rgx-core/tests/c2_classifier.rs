//! Integration tests for the C2 pattern classifier.
//!
//! These tests compile real pattern strings end-to-end and verify the
//! classification stored on the resulting `Regex`. The classifier itself
//! is also unit-tested directly against synthetic ASTs in
//! `rgx-core/src/c2/classifier.rs::tests`. These integration tests
//! complement those unit tests by exercising the full compile pipeline:
//! parser → AST normalization → classifier → metadata on Program → public
//! accessor on Regex.
//!
//! See `docs/C2_NFA_DFA_DESIGN.md` §4 for the no-backtracking subset
//! definition.

use rgx_core::c2::{Classification, ExclusionReason};
use rgx_core::Regex;

fn classification_of(pattern: &str) -> Classification {
    let re = Regex::compile(pattern)
        .unwrap_or_else(|err| panic!("compile failed for {pattern:?}: {err}"));
    re.classification().clone()
}

fn assert_no_backtracking(pattern: &str) {
    let cls = classification_of(pattern);
    assert_eq!(
        cls,
        Classification::NoBacktracking,
        "expected NoBacktracking for pattern {pattern:?}, got {cls:?}"
    );
}

fn assert_needs_vm(pattern: &str, expected: ExclusionReason) {
    let cls = classification_of(pattern);
    assert_eq!(
        cls,
        Classification::NeedsVm {
            reason: expected.clone()
        },
        "expected NeedsVm({expected:?}) for pattern {pattern:?}, got {cls:?}"
    );
}

// ============================================================
// Patterns that should classify as NoBacktracking
// ============================================================

#[test]
fn literal_pattern_is_no_backtracking() {
    assert_no_backtracking("hello");
}

#[test]
fn character_class_pattern_is_no_backtracking() {
    assert_no_backtracking(r"[a-z]");
    assert_no_backtracking(r"[^0-9]");
    assert_no_backtracking(r"[a-zA-Z0-9_]");
}

#[test]
fn shorthand_class_patterns_are_no_backtracking() {
    assert_no_backtracking(r"\d");
    assert_no_backtracking(r"\D");
    assert_no_backtracking(r"\w");
    assert_no_backtracking(r"\W");
    assert_no_backtracking(r"\s");
    assert_no_backtracking(r"\S");
}

#[test]
fn dot_pattern_is_no_backtracking() {
    assert_no_backtracking(".");
}

#[test]
fn alternation_is_no_backtracking() {
    assert_no_backtracking("cat|dog|fish");
}

#[test]
fn quantifiers_are_no_backtracking() {
    assert_no_backtracking("a?");
    assert_no_backtracking("a*");
    assert_no_backtracking("a+");
    assert_no_backtracking("a{2,5}");
    assert_no_backtracking("a{3}");
    assert_no_backtracking("a{2,}");
}

#[test]
fn lazy_quantifiers_are_no_backtracking() {
    assert_no_backtracking("a??");
    assert_no_backtracking("a*?");
    assert_no_backtracking("a+?");
    assert_no_backtracking("a{2,5}?");
}

#[test]
fn capturing_groups_are_no_backtracking() {
    assert_no_backtracking("(abc)");
    assert_no_backtracking(r"(\d+)");
    assert_no_backtracking(r"(?<year>\d{4})");
}

#[test]
fn non_capturing_groups_are_no_backtracking() {
    assert_no_backtracking("(?:abc)");
    assert_no_backtracking(r"(?:\d+)");
}

#[test]
fn anchors_are_no_backtracking() {
    assert_no_backtracking(r"^abc$");
    assert_no_backtracking(r"\Aabc\z");
    assert_no_backtracking(r"\Aabc\Z");
}

#[test]
fn word_boundaries_are_no_backtracking() {
    assert_no_backtracking(r"\bword\b");
    assert_no_backtracking(r"\Bnotword\B");
}

#[test]
fn unicode_property_classes_are_no_backtracking() {
    assert_no_backtracking(r"\p{L}");
    assert_no_backtracking(r"\P{N}");
}

#[test]
fn flag_groups_with_supported_inner_are_no_backtracking() {
    assert_no_backtracking("(?i)hello");
    assert_no_backtracking("(?i:hello)");
    assert_no_backtracking("(?m)^line");
}

#[test]
fn realistic_log_pattern_is_no_backtracking() {
    // (\d{4})-(\d{2})-(\d{2}) (ERROR|WARN|INFO)
    assert_no_backtracking(r"(\d{4})-(\d{2})-(\d{2}) (ERROR|WARN|INFO)");
}

#[test]
fn realistic_email_like_pattern_is_no_backtracking() {
    // [\w.+-]+@[\w-]+\.[\w.-]+
    assert_no_backtracking(r"[\w.+-]+@[\w-]+\.[\w.-]+");
}

// ============================================================
// Patterns that should classify as NeedsVm
// ============================================================

#[test]
fn numeric_backreference_needs_vm() {
    assert_needs_vm(r"(\w+)\s+\1", ExclusionReason::Backreference);
}

#[test]
fn named_backreference_needs_vm() {
    assert_needs_vm(r"(?<word>\w+)\s+\k<word>", ExclusionReason::Backreference);
}

#[test]
fn positive_lookahead_needs_vm() {
    assert_needs_vm(r"foo(?=bar)", ExclusionReason::Lookaround);
}

#[test]
fn negative_lookahead_needs_vm() {
    assert_needs_vm(r"foo(?!bar)", ExclusionReason::Lookaround);
}

#[test]
fn positive_lookbehind_needs_vm() {
    assert_needs_vm(r"(?<=foo)bar", ExclusionReason::Lookaround);
}

#[test]
fn negative_lookbehind_needs_vm() {
    assert_needs_vm(r"(?<!foo)bar", ExclusionReason::Lookaround);
}

#[test]
fn atomic_group_needs_vm() {
    assert_needs_vm(r"(?>abc)", ExclusionReason::AtomicGroup);
}

#[test]
fn possessive_quantifier_needs_vm() {
    // Possessive quantifiers are lowered to atomic groups in the AST,
    // so they classify with the AtomicGroup reason.
    assert_needs_vm(r"a++", ExclusionReason::AtomicGroup);
    assert_needs_vm(r"a*+", ExclusionReason::AtomicGroup);
}

#[test]
fn recursion_needs_vm() {
    assert_needs_vm(r"(?R)", ExclusionReason::Recursion);
}

#[test]
fn numbered_subroutine_needs_vm() {
    assert_needs_vm(r"(\w+)(?1)", ExclusionReason::Recursion);
}

#[test]
fn conditional_needs_vm() {
    assert_needs_vm(r"(a)(?(1)b|c)", ExclusionReason::Conditional);
}
