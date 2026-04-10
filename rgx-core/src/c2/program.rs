//! `CompiledC2Program` — the assembled C2 artifact for a single regex.
//!
//! This is the top-level data structure produced by the C2 compile path
//! once all the building blocks from steps 1–3 are in place. It holds
//! the four Thompson NFAs needed by the eventual lazy DFA pipeline (step
//! 5+) and the byte-class equivalence map shared across them. It does
//! NOT hold any DFA cache yet — the lazy DFAs are constructed at match
//! time from the NFAs and live on the runtime engine, not on the
//! compiled program.
//!
//! This is C2 step 3b of the phased plan in `docs/C2_NFA_DFA_DESIGN.md`
//! §15. At this stage the module is **standalone** — no engine wiring,
//! no `Program` field, no runtime dispatch. Step 4 (sparse-set Pike-VM)
//! consumes `CompiledC2Program` to drive its simulation.
//!
//! # Cohabitation invariant
//!
//! `CompiledC2Program` is built only for patterns that the C2 classifier
//! (step 1) tags as `NoBacktracking`. Patterns outside the subset never
//! reach this module — they continue to run on the existing backtracking
//! VM unchanged. The cohabitation rule from design doc §12 is enforced
//! at the dispatch boundary in step 4+.
//!
//! See `docs/C2_NFA_DFA_DESIGN.md` §6 for the design rationale.

use crate::ast::Regex;
use crate::c2::byte_class::ByteClassMap;
use crate::c2::nfa::Nfa;

/// The complete C2-compiled artifact for a single regex pattern.
///
/// Holds the byte-class equivalence map, all four Thompson NFAs (forward
/// + reverse, each in anchored and unanchored variants), and the capture
/// group metadata needed by the bounded Pike-VM capture pass (design doc §9).
#[derive(Debug, Clone)]
pub struct CompiledC2Program {
    /// Byte-class equivalence map shared by all four NFAs.
    ///
    /// Built once from the original (un-reversed) AST. The set of bytes
    /// the pattern uses is direction-independent, so the same map is
    /// valid for both the forward and reverse NFAs.
    pub byte_class_map: ByteClassMap,

    /// Forward NFA in anchored mode. Used for `find_first_at(text, pos)`
    /// and similar position-aware APIs, and for patterns that already
    /// begin with `^` / `\A`.
    pub forward_anchored: Nfa,

    /// Forward NFA in unanchored mode. Used for `find_first(text)`,
    /// `find_all`, and other scanning entry points. Wraps the pattern
    /// with a lazy `(?s:.)*?` prefix.
    pub forward_unanchored: Nfa,

    /// Reverse NFA in anchored mode. Used by the lazy reverse DFA in
    /// step 6 to recover match start positions when the forward DFA has
    /// found a match end at a known position.
    pub reverse_anchored: Nfa,

    /// Reverse NFA in unanchored mode. Used by the lazy reverse DFA when
    /// the start position is unknown.
    pub reverse_unanchored: Nfa,

    /// Number of capture groups in the original pattern. Used to size
    /// capture buffers for the bounded Pike-VM capture pass.
    pub num_capture_groups: u32,
}

impl CompiledC2Program {
    /// Build a complete C2 program from a regex AST.
    ///
    /// Computes the byte-class map once from the original AST, then
    /// builds all four NFAs against the same map. The reverse NFAs are
    /// produced by [`crate::c2::nfa::reverse_ast`] followed by the same
    /// forward Thompson construction — see the `reverse_ast` doc comment
    /// for the reversal rules.
    ///
    /// The caller is responsible for ensuring the AST has been classified
    /// as `NoBacktracking` by [`crate::c2::classifier::classify`]; calling
    /// this on a `NeedsVm` pattern will produce an NFA where unsupported
    /// nodes degrade to unmatchable fragments (defensive fallback) and
    /// the result is unlikely to recognise the intended language.
    #[must_use]
    pub fn build_from_ast(ast: &Regex) -> Self {
        let byte_class_map = ByteClassMap::build_from_ast(ast);
        let forward_anchored = Nfa::build_anchored(ast, &byte_class_map);
        let forward_unanchored = Nfa::build_unanchored(ast, &byte_class_map);
        let reverse_anchored = Nfa::build_reverse_anchored(ast, &byte_class_map);
        let reverse_unanchored = Nfa::build_reverse_unanchored(ast, &byte_class_map);

        // The forward and reverse NFAs visit the same capture groups, so
        // any of them can supply the canonical group count. Use the
        // forward anchored NFA as the source of truth.
        let num_capture_groups = forward_anchored.num_capture_groups();

        Self {
            byte_class_map,
            forward_anchored,
            forward_unanchored,
            reverse_anchored,
            reverse_unanchored,
            num_capture_groups,
        }
    }

    /// Returns the number of distinct byte classes in the byte-class map.
    /// Convenience accessor for tests and benchmarks.
    #[must_use]
    pub fn num_byte_classes(&self) -> u16 {
        self.byte_class_map.num_classes()
    }

    /// Compile a pattern string into a `CompiledC2Program` if (and only if)
    /// the pattern lies inside the no-backtracking subset that C2 can
    /// handle. Returns `None` for patterns that the classifier tags as
    /// `NeedsVm` (those continue to run on the existing backtracking VM
    /// via the normal `Regex::compile` path).
    ///
    /// Convenience for tests, benchmarks, and the differential testing
    /// harness in `tests/c2_pike_differential.rs`. The normal compile
    /// pipeline goes through `Compiler::compile_ast_with_label` which
    /// builds the C2 program automatically when the pattern is C2-
    /// dispatch-eligible (see [`is_c2_dispatch_eligible`]).
    ///
    /// # Capture index assignment
    ///
    /// The PGEN parser emits capture groups with `index: None`; capture
    /// indices are assigned later in the compile pipeline by
    /// `Compiler::assign_capture_indices`. This method runs the same
    /// assignment pass before classification and NFA construction so
    /// the resulting `CompiledC2Program` has correct group numbering
    /// for the bounded Pike-VM capture pass.
    ///
    /// # Errors
    ///
    /// Returns `None` if the pattern fails to parse or fails to classify
    /// as `NoBacktracking`. Both cases mean the pattern can't be handled
    /// by the C2 engine.
    #[must_use]
    pub fn try_compile(pattern: &str) -> Option<Self> {
        let ast = crate::parsing::parse_pattern(pattern).ok()?;
        let ast = crate::compiler::Compiler::assign_capture_indices(ast);
        match crate::c2::classify(&ast) {
            crate::c2::Classification::NoBacktracking => Some(Self::build_from_ast(&ast)),
            crate::c2::Classification::NeedsVm { .. } => None,
        }
    }
}

/// Returns `true` iff the AST is eligible for engine dispatch through
/// the C2 Pike-VM via the public `Regex` API.
///
/// At C2 step 4c the eligibility check is **stricter than classification**
/// because the Pike-VM doesn't yet track every metadata field that
/// `MatchResult` carries and doesn't yet handle every regex semantic.
/// The check excludes:
///
/// - **Top-level alternation**: patterns whose AST root (after
///   unwrapping single capturing / non-capturing / flag groups) is
///   `Regex::Alternation(_)`. These patterns set
///   `MatchResult.matched_branch_number` on the existing backtracking
///   VM, but the Pike-VM doesn't track which branch matched. Lift by
///   adding branch tracking to the Pike-VM.
/// - **Flag groups**: any pattern containing `Regex::FlagGroup { ... }`
///   anywhere in its AST. The flags `(?i)` (case-insensitive),
///   `(?s)` (dot-all), `(?m)` (multiline), and `(?x)` (extended
///   whitespace) require runtime semantic adjustments the Pike-VM
///   doesn't apply yet. Lift by extending the NFA construction to
///   honour the flag context (case-folded char classes, dot-newline,
///   per-line anchor semantics).
/// - **`\G` anchor (`PreviousMatchEnd`)**: the Pike-VM's `\G` check
///   only fires at byte position 0; it doesn't thread the previous
///   match end through `find_all`. Lift by passing the previous end
///   into the simulator and updating `check_assertion`.
/// - **Non-ASCII character classes**: `Regex::UnicodeClass { ... }` at
///   any position, `CharClass::UnicodeClass`, and `CharClass::Custom`
///   with any non-ASCII codepoint range. The Pike-VM's byte-class
///   partition (built from the AST in `c2/byte_class.rs`) collapses
///   all byte ranges from a multi-range character class into a single
///   oracle, which is too coarse to distinguish per-position byte
///   constraints across UTF-8 sequences. For `\P{L}` this manifests
///   as false positives like `is_match("β")` returning true (β is a
///   Greek LETTER but its second byte 0xB2 also appears as a second
///   byte of `\xC2\xB2 = ²` which is a non-letter, so the coarse
///   partition collapses them). Lift by refactoring `byte_class.rs`
///   to emit per-Utf8Sequence-per-position oracles, or by computing
///   the byte-class partition from the NFA transitions instead of
///   from the AST.
///
/// Single literal non-ASCII characters (`Regex::Char(c)` where `c >
/// 0x7F`) are still dispatchable because they produce a single
/// Utf8Sequence with no inter-sequence overlap, so the coarse oracle
/// is precise enough.
///
/// The classifier's own check (`Classification::NoBacktracking`) is a
/// necessary condition that the caller must verify separately. This
/// function only adds the structural eligibility checks on top.
///
/// The exclusions here are SOTA-correct: they preserve every existing
/// test behaviour by routing affected patterns through the existing
/// backtracking VM. As Pike-VM gains support for each excluded
/// feature, the corresponding check can be removed.
pub fn is_c2_dispatch_eligible(ast: &Regex) -> bool {
    !has_top_level_alternation(ast)
        && !contains_flag_group(ast)
        && !contains_previous_match_end_anchor(ast)
        && !contains_multi_byte_char_class(ast)
}

/// Returns `true` if the "top level" of the AST is an alternation node.
///
/// "Top level" means: walk through any number of capturing /
/// non-capturing / flag-group wrappers and see if the unwrapped node
/// is `Alternation`. Used by [`is_c2_dispatch_eligible`] to detect
/// patterns whose `matched_branch_number` field would be lost on
/// engine dispatch.
fn has_top_level_alternation(ast: &Regex) -> bool {
    match ast {
        Regex::Alternation(_) => true,
        Regex::Group { expr, .. } => has_top_level_alternation(expr),
        Regex::FlagGroup { expr, .. } => has_top_level_alternation(expr),
        _ => false,
    }
}

/// Recursively walks the AST and returns `true` if any node is a
/// `Regex::FlagGroup`. The Pike-VM doesn't apply flag semantics
/// (case-insensitive, dot-all, multiline, extended) yet, so any
/// pattern containing one must route through the existing VM.
fn contains_flag_group(ast: &Regex) -> bool {
    match ast {
        Regex::FlagGroup { .. } => true,
        Regex::Sequence(items) | Regex::Alternation(items) => items.iter().any(contains_flag_group),
        Regex::Quantified { expr, .. } => contains_flag_group(expr),
        Regex::Group { expr, .. } => contains_flag_group(expr),
        Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => contains_flag_group(expr),
        Regex::Conditional {
            true_branch,
            false_branch,
            ..
        } => {
            contains_flag_group(true_branch)
                || false_branch
                    .as_ref()
                    .is_some_and(|fb| contains_flag_group(fb))
        }
        _ => false,
    }
}

/// Recursively walks the AST and returns `true` if any node is a
/// character class that involves multi-byte UTF-8 contents — either a
/// `Regex::UnicodeClass` / `CharClass::UnicodeClass`, or a
/// `CharClass::Custom` with at least one non-ASCII codepoint range.
/// See [`is_c2_dispatch_eligible`] for the rationale.
///
/// Single literal non-ASCII characters (`Regex::Char(c)` where `c` is
/// non-ASCII) are NOT excluded — they produce one Utf8Sequence with
/// non-overlapping byte ranges, which the coarse oracle handles
/// correctly.
fn contains_multi_byte_char_class(ast: &Regex) -> bool {
    match ast {
        Regex::UnicodeClass { .. } => true,
        Regex::CharClass(cc) => char_class_is_multi_byte(cc),
        Regex::Sequence(items) | Regex::Alternation(items) => {
            items.iter().any(contains_multi_byte_char_class)
        }
        Regex::Quantified { expr, .. } => contains_multi_byte_char_class(expr),
        Regex::Group { expr, .. } => contains_multi_byte_char_class(expr),
        Regex::FlagGroup { expr, .. } => contains_multi_byte_char_class(expr),
        Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => {
            contains_multi_byte_char_class(expr)
        }
        Regex::Conditional {
            true_branch,
            false_branch,
            ..
        } => {
            contains_multi_byte_char_class(true_branch)
                || false_branch
                    .as_ref()
                    .is_some_and(|fb| contains_multi_byte_char_class(fb))
        }
        _ => false,
    }
}

/// Returns `true` if a [`crate::ast::CharClass`] involves multi-byte
/// UTF-8 contents — Unicode property class or any non-ASCII codepoint
/// range.
fn char_class_is_multi_byte(cc: &crate::ast::CharClass) -> bool {
    match cc {
        crate::ast::CharClass::UnicodeClass { .. } => true,
        crate::ast::CharClass::Custom { ranges, .. } => ranges
            .iter()
            .any(|r| !r.start.is_ascii() || !r.end.is_ascii()),
        crate::ast::CharClass::Digit { .. }
        | crate::ast::CharClass::Word { .. }
        | crate::ast::CharClass::Space { .. } => false,
    }
}

/// Recursively walks the AST and returns `true` if any node is the
/// `\G` anchor (`Regex::Anchor(AnchorType::PreviousMatchEnd)`). The
/// Pike-VM's `\G` check only fires at position 0, so any pattern
/// using `\G` for `find_all`-style continuation matching must route
/// through the existing VM.
fn contains_previous_match_end_anchor(ast: &Regex) -> bool {
    match ast {
        Regex::Anchor(crate::ast::AnchorType::PreviousMatchEnd) => true,
        Regex::Sequence(items) | Regex::Alternation(items) => {
            items.iter().any(contains_previous_match_end_anchor)
        }
        Regex::Quantified { expr, .. } => contains_previous_match_end_anchor(expr),
        Regex::Group { expr, .. } => contains_previous_match_end_anchor(expr),
        Regex::FlagGroup { expr, .. } => contains_previous_match_end_anchor(expr),
        Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => {
            contains_previous_match_end_anchor(expr)
        }
        Regex::Conditional {
            true_branch,
            false_branch,
            ..
        } => {
            contains_previous_match_end_anchor(true_branch)
                || false_branch
                    .as_ref()
                    .is_some_and(|fb| contains_previous_match_end_anchor(fb))
        }
        _ => false,
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::ast::{GroupKind, Regex};

    fn lit(c: char) -> Regex {
        Regex::Char(c)
    }

    fn alt(items: Vec<Regex>) -> Regex {
        Regex::Alternation(items)
    }

    fn group_capturing(expr: Regex) -> Regex {
        Regex::Group {
            expr: Box::new(expr),
            kind: GroupKind::Capturing,
            index: Some(1),
            name: None,
        }
    }

    #[test]
    fn dispatch_eligible_for_simple_literal() {
        assert!(is_c2_dispatch_eligible(&lit('a')));
    }

    #[test]
    fn dispatch_ineligible_for_top_level_alternation() {
        assert!(!is_c2_dispatch_eligible(&alt(vec![lit('a'), lit('b')])));
    }

    #[test]
    fn dispatch_ineligible_for_alternation_wrapped_in_capturing_group() {
        let inner = alt(vec![lit('a'), lit('b')]);
        let outer = group_capturing(inner);
        assert!(!is_c2_dispatch_eligible(&outer));
    }

    #[test]
    fn dispatch_ineligible_for_alternation_wrapped_in_flag_group() {
        let inner = alt(vec![lit('a'), lit('b')]);
        let outer = Regex::FlagGroup {
            flags: "i".to_string(),
            expr: Box::new(inner),
        };
        assert!(!is_c2_dispatch_eligible(&outer));
    }

    #[test]
    fn dispatch_eligible_for_sequence_containing_alternation() {
        // Alternation is NOT at the top level here — it's wrapped by
        // a sequence with anchors. matched_branch_number is None on
        // the existing VM for this shape, so dispatch is safe.
        let inner = group_capturing(alt(vec![lit('a'), lit('b')]));
        let seq = Regex::Sequence(vec![
            Regex::Anchor(crate::ast::AnchorType::AbsStart),
            inner,
            Regex::Anchor(crate::ast::AnchorType::AbsEndNoNL),
        ]);
        assert!(is_c2_dispatch_eligible(&seq));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{CharRange, GroupKind, Quantifier, Regex};

    fn lit(c: char) -> Regex {
        Regex::Char(c)
    }

    fn seq(items: Vec<Regex>) -> Regex {
        Regex::Sequence(items)
    }

    fn alt(items: Vec<Regex>) -> Regex {
        Regex::Alternation(items)
    }

    fn group_capturing(index: u32, expr: Regex) -> Regex {
        Regex::Group {
            expr: Box::new(expr),
            kind: GroupKind::Capturing,
            index: Some(index),
            name: None,
        }
    }

    fn quantified(expr: Regex, q: Quantifier) -> Regex {
        Regex::Quantified {
            expr: Box::new(expr),
            quantifier: q,
        }
    }

    fn custom(ranges: Vec<(char, char)>) -> Regex {
        use crate::ast::CharClass;
        Regex::CharClass(CharClass::Custom {
            ranges: ranges
                .into_iter()
                .map(|(s, e)| CharRange::range(s, e))
                .collect(),
            negated: false,
        })
    }

    #[test]
    fn build_from_ast_produces_all_four_nfas() {
        let ast = lit('a');
        let prog = CompiledC2Program::build_from_ast(&ast);
        assert!(prog.forward_anchored.num_states() > 0);
        assert!(prog.forward_unanchored.num_states() > 0);
        assert!(prog.reverse_anchored.num_states() > 0);
        assert!(prog.reverse_unanchored.num_states() > 0);
    }

    #[test]
    fn unanchored_nfas_have_more_states_than_anchored() {
        let ast = lit('a');
        let prog = CompiledC2Program::build_from_ast(&ast);
        assert!(
            prog.forward_unanchored.num_states() > prog.forward_anchored.num_states(),
            "forward unanchored should be larger than forward anchored"
        );
        assert!(
            prog.reverse_unanchored.num_states() > prog.reverse_anchored.num_states(),
            "reverse unanchored should be larger than reverse anchored"
        );
    }

    #[test]
    fn forward_and_reverse_anchored_have_same_state_count_for_palindromic_pattern() {
        // The Thompson construction is structural — for a single literal,
        // forward and reverse produce the same shape because the literal
        // is its own reverse.
        let ast = lit('a');
        let prog = CompiledC2Program::build_from_ast(&ast);
        assert_eq!(
            prog.forward_anchored.num_states(),
            prog.reverse_anchored.num_states()
        );
    }

    #[test]
    fn capture_group_count_is_recorded() {
        let ast = group_capturing(1, seq(vec![lit('a'), lit('b')]));
        let prog = CompiledC2Program::build_from_ast(&ast);
        assert_eq!(prog.num_capture_groups, 1);
    }

    #[test]
    fn nested_capture_groups_count_correctly() {
        let inner = group_capturing(2, lit('b'));
        let outer = group_capturing(1, seq(vec![lit('a'), inner, lit('c')]));
        let prog = CompiledC2Program::build_from_ast(&outer);
        assert_eq!(prog.num_capture_groups, 2);
    }

    #[test]
    fn byte_class_map_is_shared_across_all_nfas() {
        // Every NFA in the compiled program should use byte-class IDs
        // that are valid against `prog.byte_class_map`.
        let ast = seq(vec![
            custom(vec![('a', 'c')]),
            custom(vec![('d', 'f')]),
            lit('z'),
        ]);
        let prog = CompiledC2Program::build_from_ast(&ast);
        let max_class = prog.num_byte_classes() as u8 - 1;
        for nfa in [
            &prog.forward_anchored,
            &prog.forward_unanchored,
            &prog.reverse_anchored,
            &prog.reverse_unanchored,
        ] {
            for state in nfa.states() {
                for &(class, _) in &state.transitions {
                    assert!(
                        class <= max_class,
                        "NFA used out-of-range byte class {class} (max {max_class})"
                    );
                }
            }
        }
    }

    #[test]
    fn realistic_pattern_assembles_cleanly() {
        // (\d{4})-(\d{2})-(\d{2}) ERROR
        let ast = seq(vec![
            group_capturing(
                1,
                quantified(
                    Regex::Digit { negated: false },
                    Quantifier::Range {
                        min: 4,
                        max: Some(4),
                        lazy: false,
                    },
                ),
            ),
            lit('-'),
            group_capturing(
                2,
                quantified(
                    Regex::Digit { negated: false },
                    Quantifier::Range {
                        min: 2,
                        max: Some(2),
                        lazy: false,
                    },
                ),
            ),
            lit('-'),
            group_capturing(
                3,
                quantified(
                    Regex::Digit { negated: false },
                    Quantifier::Range {
                        min: 2,
                        max: Some(2),
                        lazy: false,
                    },
                ),
            ),
            lit(' '),
            seq(vec![lit('E'), lit('R'), lit('R'), lit('O'), lit('R')]),
        ]);
        let prog = CompiledC2Program::build_from_ast(&ast);
        assert_eq!(prog.num_capture_groups, 3);
        assert!(prog.forward_anchored.num_states() > 0);
        assert!(prog.reverse_anchored.num_states() > 0);
    }

    #[test]
    fn alternation_pattern_assembles_with_each_branch_reversed() {
        // (cat|dog) — the reverse should match (tac|god). We can't run
        // the NFA at step 3b (no Pike-VM yet), but we can check that
        // assembly succeeds and that the byte-class map is consistent.
        let ast = group_capturing(
            1,
            alt(vec![
                seq(vec![lit('c'), lit('a'), lit('t')]),
                seq(vec![lit('d'), lit('o'), lit('g')]),
            ]),
        );
        let prog = CompiledC2Program::build_from_ast(&ast);
        assert_eq!(prog.num_capture_groups, 1);
        // Bytes c, a, t, d, o, g all participate; the byte-class map
        // should distinguish them from non-pattern bytes.
        assert!(prog.num_byte_classes() >= 2);
    }
}
