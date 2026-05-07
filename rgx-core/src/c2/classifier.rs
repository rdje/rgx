//! Pattern classifier for the C2 NFA/DFA hybrid engine.
//!
//! Walks a regex AST and decides whether the pattern can be handled by the
//! C2 NFA/DFA engine ([`Classification::NoBacktracking`]) or whether it must
//! fall back to the existing backtracking VM ([`Classification::NeedsVm`])
//! along with a precise reason for the exclusion.
//!
//! See `docs/C2_NFA_DFA_DESIGN.md` §4 for the full subset definition and
//! the design rationale for each excluded construct.
//!
//! # Conservative classifier
//!
//! The classifier is **conservative**. Any AST node it isn't certain about
//! returns [`Classification::NeedsVm`]. False negatives (a pattern that
//! *could* run on C2 but classifies as `NeedsVm`) are a perf miss but never
//! a correctness risk. False positives (a pattern that classifies as
//! `NoBacktracking` but C2 cannot actually handle it) are a correctness bug
//! and are forbidden by the differential test suite that lands in step 4
//! of the C2 implementation plan.
//!
//! # Status
//!
//! This is C2 step 1 of the phased plan in `docs/C2_NFA_DFA_DESIGN.md` §15.
//! At this stage, classification is **metadata only** — every compiled
//! `Program` carries a `Classification` field, but no runtime dispatch
//! reads it yet. Runtime dispatch lands in step 4 (Pike-VM) once the
//! NFA/DFA engine itself exists.

use crate::ast::{GroupKind, Regex};

/// Classification of a compiled regex pattern.
///
/// Decides which engine handles the pattern at runtime once the C2 dispatch
/// boundary is wired in (C2 step 4+). At step 1, this is metadata only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// The pattern uses only constructs the C2 NFA/DFA engine can handle.
    ///
    /// Once C2 step 4 lands, patterns with this classification will dispatch
    /// to the sparse-set Pike-VM (and later, the lazy DFA cache). For now,
    /// they continue to run on the existing backtracking VM.
    NoBacktracking,

    /// The pattern uses one or more constructs that require the existing
    /// backtracking VM. The first encountered exclusion reason is recorded
    /// for diagnostics.
    NeedsVm {
        /// Why the pattern was excluded from the C2 path.
        reason: ExclusionReason,
    },
}

impl Default for Classification {
    /// The default is the conservative one: `NeedsVm` with the
    /// [`ExclusionReason::NotYetClassified`] sentinel reason. This guarantees
    /// that any code path which constructs a `Program` without explicitly
    /// running the classifier still routes to the existing backtracking VM —
    /// no correctness risk, only a perf miss until the field is overwritten.
    fn default() -> Self {
        Self::NeedsVm {
            reason: ExclusionReason::NotYetClassified,
        }
    }
}

/// Reason a pattern was excluded from the C2 NFA/DFA path.
///
/// The classifier short-circuits on the first encountered exclusion, so the
/// reported reason corresponds to the first excluded AST node found in a
/// pre-order walk. This is intentional — the goal is diagnostics, not an
/// exhaustive list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExclusionReason {
    /// Sentinel: the classifier has not been run on this `Program` yet.
    ///
    /// This appears only as the result of `Classification::default()`. It
    /// should never appear on a `Program` returned from the normal
    /// compilation pipeline because the compiler always runs the classifier
    /// after VM compilation. If it does appear there, it indicates a bug
    /// in the compile pipeline wiring.
    NotYetClassified,

    /// Pattern contains a numeric, named, or relative backreference.
    Backreference,

    /// Pattern contains a recursion or subroutine call (`(?R)`, `(?1)`, `(?&name)`).
    Recursion,

    /// Pattern contains a returned-capture subroutine call (`(?N(grouplist))`).
    ReturnedCaptureSubroutine,

    /// Pattern contains a lookaround assertion (positive or negative,
    /// ahead or behind).
    Lookaround,

    /// Pattern contains a conditional construct (`(?(cond)yes|no)`).
    Conditional,

    /// Pattern contains an atomic group (`(?>...)`).
    ///
    /// Possessive quantifiers also reach this branch because the parser
    /// lowers `*+`, `++`, `?+`, and `{n,m}+` into atomic-group AST nodes.
    AtomicGroup,

    /// Pattern contains a branch-reset group (`(?|...)`).
    BranchReset,

    /// Pattern contains a Perl extended character class (`(?[...])`).
    PerlExtendedClass,

    /// Pattern contains `\K` (match-start reset).
    KeepOut,

    /// Pattern contains a backtracking verb: `(*ACCEPT)`, `(*COMMIT)`,
    /// `(*PRUNE)`, `(*SKIP)`, `(*THEN)`, or `(*MARK:name)`.
    BacktrackingVerb,

    /// Pattern contains an inline code block (`(?{lang:code})`).
    InlineCodeBlock,

    /// Pattern contains a `(?C)` callout.
    Callout,

    /// Pattern contains `\X` (extended grapheme cluster).
    ///
    /// Matching a grapheme cluster requires Unicode-aware traversal of
    /// base codepoint plus combining marks, which doesn't fit cleanly into
    /// a Thompson NFA without significant additional machinery. Excluded
    /// from the C2 subset on the first pass; can be added later if
    /// profiling shows it's worth the engineering effort. Patterns with
    /// `\X` continue to run on the existing backtracking VM (which has
    /// full `\X` support).
    GraphemeCluster,
}

/// Classify a regex AST against the C2 no-backtracking subset.
///
/// Returns [`Classification::NoBacktracking`] if every node in the AST
/// belongs to the C2 subset, or [`Classification::NeedsVm`] with the first
/// encountered exclusion reason otherwise.
///
/// This is a single linear-time pre-order walk of the AST. Cost is bounded
/// by the AST size and is computed once per pattern at compile time.
#[must_use]
pub fn classify(ast: &Regex) -> Classification {
    match first_exclusion(ast) {
        Some(reason) => Classification::NeedsVm { reason },
        None => Classification::NoBacktracking,
    }
}

/// Walk the AST in pre-order and return the first encountered exclusion
/// reason, or `None` if every node is supported.
fn first_exclusion(ast: &Regex) -> Option<ExclusionReason> {
    match ast {
        // ============================================================
        // Supported leaves — these never produce an exclusion.
        // ============================================================
        Regex::Char(_)
        | Regex::CharClass(_)
        | Regex::Dot
        | Regex::Digit { .. }
        | Regex::Word { .. }
        | Regex::Space { .. }
        | Regex::UnicodeClass { .. }
        | Regex::Anchor(_)
        | Regex::WordBoundary { .. }
        | Regex::NewlineSequence
        | Regex::Empty
        | Regex::WhitespaceLiteral(_) => None,

        // \X is excluded from the C2 subset on the first pass — see the
        // doc comment on `ExclusionReason::GraphemeCluster` for the
        // rationale. Falls back to the existing backtracking VM.
        Regex::GraphemeCluster => Some(ExclusionReason::GraphemeCluster),

        // ============================================================
        // Supported recursive constructs — descend into children.
        // ============================================================
        Regex::Sequence(items) | Regex::Alternation(items) => {
            items.iter().find_map(first_exclusion)
        }
        Regex::Quantified {
            expr,
            quantifier: _,
        } => {
            // Possessive quantifiers (`*+`, `++`, `?+`, `{n,m}+`) are
            // lowered into Group { kind: Atomic, ... } by the parser, so
            // they're caught by the atomic-group exclusion below and we
            // don't need to inspect the quantifier itself here.
            first_exclusion(expr)
        }
        Regex::Group { expr, kind, .. } => match kind {
            GroupKind::Capturing | GroupKind::NonCapturing => first_exclusion(expr),
            GroupKind::Atomic => Some(ExclusionReason::AtomicGroup),
            GroupKind::BranchReset => Some(ExclusionReason::BranchReset),
        },
        Regex::FlagGroup { expr, .. } => first_exclusion(expr),

        // ============================================================
        // Excluded constructs — short-circuit with the reason.
        // ============================================================
        Regex::Lookahead { .. } | Regex::Lookbehind { .. } => Some(ExclusionReason::Lookaround),
        Regex::Backreference(_)
        | Regex::NamedBackreference(_)
        | Regex::RelativeBackreference(_) => Some(ExclusionReason::Backreference),
        Regex::Conditional { .. } => Some(ExclusionReason::Conditional),
        Regex::Recursion { .. } => Some(ExclusionReason::Recursion),
        Regex::ReturnedCaptureSubroutine { .. } => Some(ExclusionReason::ReturnedCaptureSubroutine),
        Regex::CodeBlock { .. } => Some(ExclusionReason::InlineCodeBlock),
        Regex::Callout(_) => Some(ExclusionReason::Callout),
        Regex::ExtendedCharClass { .. } => Some(ExclusionReason::PerlExtendedClass),
        Regex::MatchReset => Some(ExclusionReason::KeepOut),
        Regex::Accept
        | Regex::Commit
        | Regex::Prune
        | Regex::Skip(_)
        | Regex::Then
        | Regex::Mark(_) => Some(ExclusionReason::BacktrackingVerb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        AnchorType, CharClass, CharRange, ConditionalTest, Quantifier, RecursionTarget,
    };

    fn lit(c: char) -> Regex {
        Regex::Char(c)
    }

    fn seq(items: Vec<Regex>) -> Regex {
        Regex::Sequence(items)
    }

    fn alt(items: Vec<Regex>) -> Regex {
        Regex::Alternation(items)
    }

    fn group(kind: GroupKind, expr: Regex) -> Regex {
        Regex::Group {
            expr: Box::new(expr),
            kind,
            index: None,
            name: None,
        }
    }

    fn quantified(expr: Regex, q: Quantifier) -> Regex {
        Regex::Quantified {
            expr: Box::new(expr),
            quantifier: q,
        }
    }

    fn assert_no_backtracking(ast: &Regex) {
        assert_eq!(
            classify(ast),
            Classification::NoBacktracking,
            "expected NoBacktracking for AST: {ast:?}"
        );
    }

    fn assert_needs_vm(ast: &Regex, expected: ExclusionReason) {
        assert_eq!(
            classify(ast),
            Classification::NeedsVm {
                reason: expected.clone()
            },
            "expected NeedsVm({expected:?}) for AST: {ast:?}"
        );
    }

    // ============================================================
    // Default behaviour
    // ============================================================

    #[test]
    fn default_classification_is_not_yet_classified() {
        assert_eq!(
            Classification::default(),
            Classification::NeedsVm {
                reason: ExclusionReason::NotYetClassified,
            }
        );
    }

    // ============================================================
    // Supported leaves
    // ============================================================

    #[test]
    fn classifies_single_literal_as_no_backtracking() {
        assert_no_backtracking(&lit('a'));
    }

    #[test]
    fn classifies_dot_as_no_backtracking() {
        assert_no_backtracking(&Regex::Dot);
    }

    #[test]
    fn classifies_empty_as_no_backtracking() {
        assert_no_backtracking(&Regex::Empty);
    }

    #[test]
    fn classifies_shorthand_classes_as_no_backtracking() {
        assert_no_backtracking(&Regex::Digit { negated: false });
        assert_no_backtracking(&Regex::Digit { negated: true });
        assert_no_backtracking(&Regex::Word { negated: false });
        assert_no_backtracking(&Regex::Word { negated: true });
        assert_no_backtracking(&Regex::Space { negated: false });
        assert_no_backtracking(&Regex::Space { negated: true });
    }

    #[test]
    fn classifies_unicode_class_as_no_backtracking() {
        assert_no_backtracking(&Regex::UnicodeClass {
            name: "L".to_string(),
            negated: false,
        });
    }

    #[test]
    fn classifies_anchors_as_no_backtracking() {
        for anchor in [
            AnchorType::Start,
            AnchorType::End,
            AnchorType::AbsStart,
            AnchorType::AbsEnd,
            AnchorType::AbsEndNoNL,
            AnchorType::PreviousMatchEnd,
        ] {
            assert_no_backtracking(&Regex::Anchor(anchor));
        }
    }

    #[test]
    fn classifies_word_boundary_as_no_backtracking() {
        assert_no_backtracking(&Regex::WordBoundary { positive: true });
        assert_no_backtracking(&Regex::WordBoundary { positive: false });
    }

    #[test]
    fn classifies_newline_sequence_as_no_backtracking() {
        assert_no_backtracking(&Regex::NewlineSequence);
    }

    #[test]
    fn excludes_grapheme_cluster_from_c2_subset() {
        // \X is excluded on the first pass — Thompson NFA doesn't model
        // base codepoint + combining marks cleanly. Falls back to VM.
        assert_needs_vm(&Regex::GraphemeCluster, ExclusionReason::GraphemeCluster);
    }

    #[test]
    fn classifies_custom_char_class_as_no_backtracking() {
        let cc = Regex::CharClass(CharClass::Custom {
            ranges: vec![CharRange::range('a', 'z'), CharRange::range('0', '9')],
            negated: false,
            ci_override_ranges: None,
        });
        assert_no_backtracking(&cc);
    }

    // ============================================================
    // Supported recursive constructs
    // ============================================================

    #[test]
    fn classifies_concatenation_as_no_backtracking() {
        assert_no_backtracking(&seq(vec![lit('a'), lit('b'), lit('c')]));
    }

    #[test]
    fn classifies_alternation_as_no_backtracking() {
        assert_no_backtracking(&alt(vec![lit('a'), lit('b'), lit('c')]));
    }

    #[test]
    fn classifies_capturing_group_as_no_backtracking() {
        assert_no_backtracking(&group(GroupKind::Capturing, lit('a')));
    }

    #[test]
    fn classifies_non_capturing_group_as_no_backtracking() {
        assert_no_backtracking(&group(GroupKind::NonCapturing, lit('a')));
    }

    #[test]
    fn classifies_greedy_and_lazy_quantifiers_as_no_backtracking() {
        for lazy in [false, true] {
            assert_no_backtracking(&quantified(lit('a'), Quantifier::ZeroOrOne { lazy }));
            assert_no_backtracking(&quantified(lit('a'), Quantifier::ZeroOrMore { lazy }));
            assert_no_backtracking(&quantified(lit('a'), Quantifier::OneOrMore { lazy }));
            assert_no_backtracking(&quantified(
                lit('a'),
                Quantifier::Range {
                    min: 2,
                    max: Some(5),
                    lazy,
                },
            ));
            assert_no_backtracking(&quantified(
                lit('a'),
                Quantifier::Range {
                    min: 2,
                    max: None,
                    lazy,
                },
            ));
        }
    }

    #[test]
    fn classifies_flag_group_as_no_backtracking_when_inner_is_supported() {
        let inner = seq(vec![lit('a'), lit('b')]);
        let flag_group = Regex::FlagGroup {
            flags: "i".to_string(),
            expr: Box::new(inner),
        };
        assert_no_backtracking(&flag_group);
    }

    // ============================================================
    // Excluded constructs — leaves
    // ============================================================

    #[test]
    fn excludes_numeric_backreference() {
        assert_needs_vm(&Regex::Backreference(1), ExclusionReason::Backreference);
    }

    #[test]
    fn excludes_named_backreference() {
        assert_needs_vm(
            &Regex::NamedBackreference("year".to_string()),
            ExclusionReason::Backreference,
        );
    }

    #[test]
    fn excludes_relative_backreference() {
        assert_needs_vm(
            &Regex::RelativeBackreference(-1),
            ExclusionReason::Backreference,
        );
    }

    #[test]
    fn excludes_recursion_entire() {
        assert_needs_vm(
            &Regex::Recursion {
                target: RecursionTarget::Entire,
            },
            ExclusionReason::Recursion,
        );
    }

    #[test]
    fn excludes_recursion_group() {
        assert_needs_vm(
            &Regex::Recursion {
                target: RecursionTarget::Group(1),
            },
            ExclusionReason::Recursion,
        );
    }

    #[test]
    fn excludes_recursion_named() {
        assert_needs_vm(
            &Regex::Recursion {
                target: RecursionTarget::NamedGroup("rec".to_string()),
            },
            ExclusionReason::Recursion,
        );
    }

    #[test]
    fn excludes_returned_capture_subroutine() {
        assert_needs_vm(
            &Regex::ReturnedCaptureSubroutine {
                target: RecursionTarget::Group(1),
                returned_groups: vec![RecursionTarget::Group(1)],
            },
            ExclusionReason::ReturnedCaptureSubroutine,
        );
    }

    #[test]
    fn excludes_lookahead() {
        assert_needs_vm(
            &Regex::Lookahead {
                expr: Box::new(lit('a')),
                positive: true, non_atomic: false,
            },
            ExclusionReason::Lookaround,
        );
    }

    #[test]
    fn excludes_negative_lookahead() {
        assert_needs_vm(
            &Regex::Lookahead {
                expr: Box::new(lit('a')),
                positive: false, non_atomic: false,
            },
            ExclusionReason::Lookaround,
        );
    }

    #[test]
    fn excludes_lookbehind() {
        assert_needs_vm(
            &Regex::Lookbehind {
                expr: Box::new(lit('a')),
                positive: true, non_atomic: false,
            },
            ExclusionReason::Lookaround,
        );
    }

    #[test]
    fn excludes_negative_lookbehind() {
        assert_needs_vm(
            &Regex::Lookbehind {
                expr: Box::new(lit('a')),
                positive: false, non_atomic: false,
            },
            ExclusionReason::Lookaround,
        );
    }

    #[test]
    fn excludes_conditional() {
        assert_needs_vm(
            &Regex::Conditional {
                condition: ConditionalTest::GroupExists(1),
                true_branch: Box::new(lit('a')),
                false_branch: Some(Box::new(lit('b'))),
            },
            ExclusionReason::Conditional,
        );
    }

    #[test]
    fn excludes_atomic_group() {
        assert_needs_vm(
            &group(GroupKind::Atomic, lit('a')),
            ExclusionReason::AtomicGroup,
        );
    }

    #[test]
    fn excludes_branch_reset_group() {
        assert_needs_vm(
            &group(GroupKind::BranchReset, alt(vec![lit('a'), lit('b')])),
            ExclusionReason::BranchReset,
        );
    }

    #[test]
    fn excludes_perl_extended_char_class() {
        assert_needs_vm(
            &Regex::ExtendedCharClass {
                content: "[a-z]&[^aeiou]".to_string(),
            },
            ExclusionReason::PerlExtendedClass,
        );
    }

    #[test]
    fn excludes_match_reset() {
        assert_needs_vm(&Regex::MatchReset, ExclusionReason::KeepOut);
    }

    #[test]
    fn excludes_inline_code_block() {
        assert_needs_vm(
            &Regex::CodeBlock {
                lang: "lua".to_string(),
                code: "return true".to_string(),
            },
            ExclusionReason::InlineCodeBlock,
        );
    }

    #[test]
    fn excludes_callout() {
        assert_needs_vm(&Regex::Callout(0), ExclusionReason::Callout);
    }

    #[test]
    fn excludes_backtracking_verbs() {
        assert_needs_vm(&Regex::Accept, ExclusionReason::BacktrackingVerb);
        assert_needs_vm(&Regex::Commit, ExclusionReason::BacktrackingVerb);
        assert_needs_vm(&Regex::Prune, ExclusionReason::BacktrackingVerb);
        assert_needs_vm(&Regex::Skip(None), ExclusionReason::BacktrackingVerb);
        assert_needs_vm(
            &Regex::Skip(Some("foo".to_string())),
            ExclusionReason::BacktrackingVerb,
        );
        assert_needs_vm(&Regex::Then, ExclusionReason::BacktrackingVerb);
        assert_needs_vm(
            &Regex::Mark("name".to_string()),
            ExclusionReason::BacktrackingVerb,
        );
    }

    // ============================================================
    // Exclusions reached through recursion
    // ============================================================

    #[test]
    fn excludes_when_excluded_node_is_inside_sequence() {
        let ast = seq(vec![lit('a'), Regex::Backreference(1), lit('b')]);
        assert_needs_vm(&ast, ExclusionReason::Backreference);
    }

    #[test]
    fn excludes_when_excluded_node_is_inside_alternation() {
        let ast = alt(vec![lit('a'), Regex::MatchReset, lit('b')]);
        assert_needs_vm(&ast, ExclusionReason::KeepOut);
    }

    #[test]
    fn excludes_when_excluded_node_is_inside_quantifier() {
        let ast = quantified(
            Regex::Lookahead {
                expr: Box::new(lit('a')),
                positive: true, non_atomic: false,
            },
            Quantifier::ZeroOrMore { lazy: false },
        );
        assert_needs_vm(&ast, ExclusionReason::Lookaround);
    }

    #[test]
    fn excludes_when_excluded_node_is_inside_capturing_group() {
        let ast = group(
            GroupKind::Capturing,
            Regex::Recursion {
                target: RecursionTarget::Entire,
            },
        );
        assert_needs_vm(&ast, ExclusionReason::Recursion);
    }

    #[test]
    fn excludes_when_excluded_node_is_inside_flag_group() {
        let ast = Regex::FlagGroup {
            flags: "i".to_string(),
            expr: Box::new(Regex::CodeBlock {
                lang: "lua".to_string(),
                code: "return true".to_string(),
            }),
        };
        assert_needs_vm(&ast, ExclusionReason::InlineCodeBlock);
    }

    // ============================================================
    // First-encountered semantics
    // ============================================================

    #[test]
    fn reports_first_encountered_exclusion_in_sequence() {
        // Lookahead comes before Backreference in pre-order; the classifier
        // should report the lookahead, not the backreference.
        let ast = seq(vec![
            Regex::Lookahead {
                expr: Box::new(lit('a')),
                positive: true, non_atomic: false,
            },
            Regex::Backreference(1),
        ]);
        assert_needs_vm(&ast, ExclusionReason::Lookaround);
    }

    // ============================================================
    // Complex realistic patterns (built by hand, not parsed)
    // ============================================================

    #[test]
    fn realistic_alternation_with_quantified_groups_is_no_backtracking() {
        // (cat|dog)\s+(?:[A-Z][a-z]+)+
        let ast = seq(vec![
            group(
                GroupKind::Capturing,
                alt(vec![
                    seq(vec![lit('c'), lit('a'), lit('t')]),
                    seq(vec![lit('d'), lit('o'), lit('g')]),
                ]),
            ),
            quantified(
                Regex::Space { negated: false },
                Quantifier::OneOrMore { lazy: false },
            ),
            group(
                GroupKind::NonCapturing,
                quantified(
                    seq(vec![
                        Regex::CharClass(CharClass::Custom {
                            ranges: vec![CharRange::range('A', 'Z')],
                            negated: false,
                            ci_override_ranges: None,
                        }),
                        quantified(
                            Regex::CharClass(CharClass::Custom {
                                ranges: vec![CharRange::range('a', 'z')],
                                negated: false,
                                ci_override_ranges: None,
                            }),
                            Quantifier::OneOrMore { lazy: false },
                        ),
                    ]),
                    Quantifier::OneOrMore { lazy: false },
                ),
            ),
        ]);
        assert_no_backtracking(&ast);
    }

    #[test]
    fn realistic_backref_pattern_needs_vm() {
        // (\w+)\s+\1
        let ast = seq(vec![
            group(
                GroupKind::Capturing,
                quantified(
                    Regex::Word { negated: false },
                    Quantifier::OneOrMore { lazy: false },
                ),
            ),
            quantified(
                Regex::Space { negated: false },
                Quantifier::OneOrMore { lazy: false },
            ),
            Regex::Backreference(1),
        ]);
        assert_needs_vm(&ast, ExclusionReason::Backreference);
    }
}
