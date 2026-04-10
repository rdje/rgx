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
    /// pipeline (`Regex::compile`) doesn't go through this method —
    /// engine dispatch is wired in at C2 step 4c.
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
