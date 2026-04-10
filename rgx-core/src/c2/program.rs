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

    /// C2 step 7: literal prefix byte for memchr-based scan acceleration.
    ///
    /// If the pattern's match must start with a specific byte (e.g.,
    /// `abc` starts with `b'a'`, `ERROR.*` starts with `b'E'`), this
    /// field holds that byte. Engine dispatch (`try_dfa_find_*` and
    /// the Pike-VM `pike_captures*` family) uses [`memchr::memchr`] to
    /// jump to the next candidate position rather than scanning every
    /// byte 0..len. Pure-prefix patterns get the largest speedup.
    ///
    /// `None` when the pattern's leading element is a character class,
    /// quantifier with min=0, alternation, or any other construct
    /// where the first byte isn't fixed. The dispatch falls through
    /// to the regular per-position scan in that case.
    ///
    /// Computed at construction time by [`first_literal_byte`].
    pub c2_prefix_byte: Option<u8>,

    /// Pike-VM dispatch heuristic: `true` iff the pattern contains a
    /// quantifier whose subtree itself contains another quantifier
    /// (e.g., `(a+)+`, `(\w+\s+)+`). Computed once at construction
    /// time by [`has_nested_quantifier`].
    ///
    /// The engine dispatch layer uses this to decide whether Pike-VM
    /// is worth invoking on a Pike-VM-eligible-but-DFA-ineligible
    /// pattern: classifier-positive patterns without nested
    /// quantifiers run efficiently on the existing backtracking VM
    /// (no risk of exponential blow-up by construction), so Pike-VM
    /// dispatch would be a measurable regression. Patterns with
    /// nested quantifiers benefit from Pike-VM's O(nm) bound and
    /// the per-trial overhead is justified.
    pub c2_has_nested_quantifier: bool,
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

        // C2 step 7: precompute the literal prefix byte for memchr-based
        // scan acceleration in dispatch.
        let c2_prefix_byte = first_literal_byte(ast);

        // C2 step 8: precompute the nested-quantifier dispatch
        // heuristic. Patterns with nested quantifiers route through
        // Pike-VM (DFA-ineligible case) because their backtracking
        // worst case is exponential. Patterns without nested
        // quantifiers stay on the existing backtracking VM.
        let c2_has_nested_quantifier = has_nested_quantifier(ast);

        Self {
            byte_class_map,
            forward_anchored,
            forward_unanchored,
            reverse_anchored,
            reverse_unanchored,
            num_capture_groups,
            c2_prefix_byte,
            c2_has_nested_quantifier,
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

/// Returns `true` iff the AST is eligible for C2 **DFA** dispatch
/// (Pike-VM dispatch is governed by [`is_c2_dispatch_eligible`]).
///
/// DFA eligibility is **stricter** than Pike-VM eligibility because
/// the lazy DFA's subset construction can't express two regex
/// semantics that Pike-VM handles correctly:
///
/// - **Zero-width assertions** (`\A`, `\z`, `\Z`, `^`, `$`, `\b`, `\B`,
///   `\G`): subset construction has no notion of "context" between
///   transitions. The DFA could be extended to track look-behind bytes
///   per state, but that's a significant refactor not yet done.
/// - **Lazy quantifiers** (`a*?`, `a+?`, `a??`, `{n,m}?`): the DFA is
///   leftmost-longest by construction; it cannot express the priority
///   order Pike-VM uses for lazy semantics. For `a+?` on `"baaab"` the
///   DFA returns end=4 but PCRE2/Pike-VM return end=2.
///
/// Patterns excluded from DFA dispatch continue to run on the Pike-VM
/// (which handles both assertions and lazy quantifiers correctly).
/// As the DFA gains support for each excluded feature, the
/// corresponding check can be removed.
pub fn is_c2_dfa_eligible(ast: &Regex) -> bool {
    is_c2_dispatch_eligible(ast)
        && !contains_zero_width_assertion(ast)
        && !contains_lazy_quantifier(ast)
}

/// Recursively walks the AST and returns `true` if any node is a
/// zero-width assertion: `Regex::Anchor` (any kind), `Regex::WordBoundary`,
/// or `\G` (`AnchorType::PreviousMatchEnd`, already excluded by
/// `is_c2_dispatch_eligible` but included here for completeness so
/// the check is self-contained).
fn contains_zero_width_assertion(ast: &Regex) -> bool {
    match ast {
        Regex::Anchor(_) | Regex::WordBoundary { .. } => true,
        Regex::Sequence(items) | Regex::Alternation(items) => {
            items.iter().any(contains_zero_width_assertion)
        }
        Regex::Quantified { expr, .. } => contains_zero_width_assertion(expr),
        Regex::Group { expr, .. } => contains_zero_width_assertion(expr),
        Regex::FlagGroup { expr, .. } => contains_zero_width_assertion(expr),
        Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => {
            contains_zero_width_assertion(expr)
        }
        Regex::Conditional {
            true_branch,
            false_branch,
            ..
        } => {
            contains_zero_width_assertion(true_branch)
                || false_branch
                    .as_ref()
                    .is_some_and(|fb| contains_zero_width_assertion(fb))
        }
        _ => false,
    }
}

/// Recursively walks the AST and returns `true` if any
/// `Regex::Quantified` node has its `lazy` flag set. The DFA's subset
/// construction can't express lazy semantics, so any pattern containing
/// a lazy quantifier must route through the Pike-VM.
/// Returns `true` if the AST contains a quantifier whose subtree
/// itself contains another quantifier — i.e., a structurally nested
/// quantifier like `(a+)+`, `(\w+\s+)+`, or `(?:foo|bar+)+`.
///
/// This is the Pike-VM dispatch heuristic: classifier-positive patterns
/// without nested quantifiers run efficiently on the existing
/// backtracking VM (no risk of exponential blow-up by construction
/// — there's no nesting that can interleave alternative paths). For
/// those, the existing VM's per-trial cost is lower than Pike-VM's,
/// so dispatching to Pike-VM would be a measurable regression on
/// common patterns like `\b\w+@\w+\.\w+\b`.
///
/// Patterns with nested quantifiers are at risk of catastrophic
/// backtracking on some inputs, and Pike-VM's O(nm) bound becomes
/// strictly better than the existing VM's exponential worst case.
/// Those are the patterns that benefit from Pike-VM dispatch.
///
/// This is a **structural** property of the AST, not a runtime
/// determination — it's evaluated once at compile time.
#[must_use]
pub fn has_nested_quantifier(ast: &Regex) -> bool {
    match ast {
        Regex::Quantified { expr, .. } => contains_quantifier(expr),
        Regex::Sequence(items) | Regex::Alternation(items) => {
            items.iter().any(has_nested_quantifier)
        }
        Regex::Group { expr, .. } | Regex::FlagGroup { expr, .. } => has_nested_quantifier(expr),
        Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => {
            has_nested_quantifier(expr)
        }
        Regex::Conditional {
            true_branch,
            false_branch,
            ..
        } => {
            has_nested_quantifier(true_branch)
                || false_branch
                    .as_ref()
                    .is_some_and(|fb| has_nested_quantifier(fb))
        }
        _ => false,
    }
}

/// Returns `true` if the AST contains any quantified node anywhere in
/// its subtree. Helper for [`has_nested_quantifier`].
fn contains_quantifier(ast: &Regex) -> bool {
    match ast {
        Regex::Quantified { .. } => true,
        Regex::Sequence(items) | Regex::Alternation(items) => items.iter().any(contains_quantifier),
        Regex::Group { expr, .. } | Regex::FlagGroup { expr, .. } => contains_quantifier(expr),
        Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => contains_quantifier(expr),
        Regex::Conditional {
            true_branch,
            false_branch,
            ..
        } => {
            contains_quantifier(true_branch)
                || false_branch
                    .as_ref()
                    .is_some_and(|fb| contains_quantifier(fb))
        }
        _ => false,
    }
}

fn contains_lazy_quantifier(ast: &Regex) -> bool {
    match ast {
        Regex::Quantified { quantifier, expr } => {
            let lazy = matches!(
                quantifier,
                crate::ast::Quantifier::ZeroOrOne { lazy: true }
                    | crate::ast::Quantifier::ZeroOrMore { lazy: true }
                    | crate::ast::Quantifier::OneOrMore { lazy: true }
                    | crate::ast::Quantifier::Range { lazy: true, .. }
            );
            lazy || contains_lazy_quantifier(expr)
        }
        Regex::Sequence(items) | Regex::Alternation(items) => {
            items.iter().any(contains_lazy_quantifier)
        }
        Regex::Group { expr, .. } => contains_lazy_quantifier(expr),
        Regex::FlagGroup { expr, .. } => contains_lazy_quantifier(expr),
        Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => {
            contains_lazy_quantifier(expr)
        }
        Regex::Conditional {
            true_branch,
            false_branch,
            ..
        } => {
            contains_lazy_quantifier(true_branch)
                || false_branch
                    .as_ref()
                    .is_some_and(|fb| contains_lazy_quantifier(fb))
        }
        _ => false,
    }
}

/// Returns the first byte that any match of `ast` MUST begin with, or
/// `None` if the pattern doesn't have a fixed first byte.
///
/// Used by C2 step 7 dispatch to accelerate per-position scans via
/// [`memchr::memchr`]: instead of trying every position 0..=len, the
/// dispatch jumps to the next position where the prefix byte appears.
///
/// Detected cases (return `Some(byte)`):
/// - `Regex::Char(c)` where `c` is any codepoint — first byte of the
///   UTF-8 encoding (so non-ASCII literals like `α` and `🎉` benefit too)
/// - `Regex::WhitespaceLiteral(c)` — same
/// - `Regex::Sequence([first, ...])` where `first` (after walking
///   through any leading zero-width assertions like `\A`, `^`, `\b`)
///   has a fixed first literal byte
/// - `Regex::Group { kind, expr, .. }` for `Capturing` and
///   `NonCapturing` — recurses into `expr`
/// - `Regex::FlagGroup { expr, .. }` — recurses into `expr`
///
/// Non-detected cases (return `None`):
/// - Character classes (`[a-z]`, `\d`, `\w`, `\p{L}`, `Dot`)
/// - Alternations (different branches may start with different bytes)
/// - Quantifiers with `min == 0` (the leading element may be skipped)
/// - Backreferences, recursion, lookaround, etc. (not in C2 subset)
///
/// The detection is **conservative**: any case it isn't sure about
/// returns `None`. False negatives (missing an optimization
/// opportunity) are a perf miss but never a correctness risk. False
/// positives (claiming a fixed first byte that doesn't actually
/// constrain matches) would silently drop matches and are forbidden.
#[must_use]
pub fn first_literal_byte(ast: &Regex) -> Option<u8> {
    match ast {
        Regex::Char(c) | Regex::WhitespaceLiteral(c) => {
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf);
            bytes.as_bytes().first().copied()
        }
        Regex::Sequence(items) => {
            // Walk through leading zero-width nodes (anchors, word
            // boundaries) until we find a real literal or run out.
            for item in items {
                if let Some(b) = first_literal_byte(item) {
                    return Some(b);
                }
                if !is_zero_width_node(item) {
                    return None;
                }
            }
            None
        }
        Regex::Group { kind, expr, .. } => match kind {
            crate::ast::GroupKind::Capturing | crate::ast::GroupKind::NonCapturing => {
                first_literal_byte(expr)
            }
            // Atomic and BranchReset aren't in the C2 subset (the
            // classifier rejects them), so this branch is defensive.
            _ => None,
        },
        Regex::FlagGroup { expr, .. } => first_literal_byte(expr),
        // Quantifier with min >= 1: the leading element MUST appear,
        // so its first literal byte (if any) is the prefix. min == 0
        // means the leading element might be skipped — no fixed prefix.
        Regex::Quantified { expr, quantifier } => {
            let min = match quantifier {
                crate::ast::Quantifier::OneOrMore { .. } => 1,
                crate::ast::Quantifier::ZeroOrOne { .. }
                | crate::ast::Quantifier::ZeroOrMore { .. } => 0,
                crate::ast::Quantifier::Range { min, .. } => *min,
            };
            if min >= 1 {
                first_literal_byte(expr)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Returns `true` if `ast` is a zero-width node — consumes no input
/// bytes during matching. Used by [`first_literal_byte`] to walk past
/// leading anchors and word boundaries when looking for the first
/// literal byte in a sequence.
fn is_zero_width_node(ast: &Regex) -> bool {
    matches!(
        ast,
        Regex::Anchor(_) | Regex::WordBoundary { .. } | Regex::Empty
    )
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
    use crate::ast::{CharRange, GroupKind, Quantifier, Regex};

    fn lit(c: char) -> Regex {
        Regex::Char(c)
    }

    fn alt(items: Vec<Regex>) -> Regex {
        Regex::Alternation(items)
    }

    fn group_capturing_idx(idx: u32, expr: Regex) -> Regex {
        Regex::Group {
            expr: Box::new(expr),
            kind: GroupKind::Capturing,
            index: Some(idx),
            name: None,
        }
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

    // ============================================================
    // first_literal_byte (C2 step 7)
    // ============================================================

    #[test]
    fn first_literal_byte_for_ascii_char() {
        assert_eq!(first_literal_byte(&lit('a')), Some(b'a'));
        assert_eq!(first_literal_byte(&lit('z')), Some(b'z'));
        assert_eq!(first_literal_byte(&lit('0')), Some(b'0'));
    }

    #[test]
    fn first_literal_byte_for_non_ascii_char_returns_first_utf8_byte() {
        // 'α' = U+03B1 = 0xCE 0xB1 in UTF-8.
        assert_eq!(first_literal_byte(&lit('α')), Some(0xCE));
        // '🎉' = U+1F389 = 0xF0 0x9F 0x8E 0x89 in UTF-8.
        assert_eq!(first_literal_byte(&lit('🎉')), Some(0xF0));
    }

    #[test]
    fn first_literal_byte_for_sequence_of_literals() {
        let ast = Regex::Sequence(vec![lit('h'), lit('i')]);
        assert_eq!(first_literal_byte(&ast), Some(b'h'));
    }

    #[test]
    fn first_literal_byte_for_capturing_group_wrapping_literal() {
        let ast = group_capturing_idx(1, lit('q'));
        assert_eq!(first_literal_byte(&ast), Some(b'q'));
    }

    #[test]
    fn first_literal_byte_skips_leading_zero_width_anchor() {
        // \Aabc — the AbsStart anchor is zero-width, the next item is
        // the literal 'a'.
        let ast = Regex::Sequence(vec![
            Regex::Anchor(crate::ast::AnchorType::AbsStart),
            lit('a'),
            lit('b'),
            lit('c'),
        ]);
        assert_eq!(first_literal_byte(&ast), Some(b'a'));
    }

    #[test]
    fn first_literal_byte_skips_leading_word_boundary() {
        let ast = Regex::Sequence(vec![Regex::WordBoundary { positive: true }, lit('w')]);
        assert_eq!(first_literal_byte(&ast), Some(b'w'));
    }

    #[test]
    fn first_literal_byte_for_alternation_returns_none() {
        let ast = alt(vec![lit('a'), lit('b')]);
        assert_eq!(first_literal_byte(&ast), None);
    }

    #[test]
    fn first_literal_byte_for_quantifier_with_min_zero_returns_none() {
        // a*b — leading 'a*' could be skipped, so no fixed first byte.
        let ast = Regex::Sequence(vec![
            Regex::Quantified {
                expr: Box::new(lit('a')),
                quantifier: Quantifier::ZeroOrMore { lazy: false },
            },
            lit('b'),
        ]);
        assert_eq!(first_literal_byte(&ast), None);
    }

    #[test]
    fn first_literal_byte_for_quantifier_with_min_one_returns_inner() {
        // a+ — leading 'a' is mandatory.
        let ast = Regex::Quantified {
            expr: Box::new(lit('a')),
            quantifier: Quantifier::OneOrMore { lazy: false },
        };
        assert_eq!(first_literal_byte(&ast), Some(b'a'));
    }

    #[test]
    fn first_literal_byte_for_range_with_min_zero_returns_none() {
        // a{0,3}b
        let ast = Regex::Sequence(vec![
            Regex::Quantified {
                expr: Box::new(lit('a')),
                quantifier: Quantifier::Range {
                    min: 0,
                    max: Some(3),
                    lazy: false,
                },
            },
            lit('b'),
        ]);
        assert_eq!(first_literal_byte(&ast), None);
    }

    #[test]
    fn first_literal_byte_for_range_with_min_one_returns_inner() {
        // a{1,3}b
        let ast = Regex::Quantified {
            expr: Box::new(lit('a')),
            quantifier: Quantifier::Range {
                min: 1,
                max: Some(3),
                lazy: false,
            },
        };
        assert_eq!(first_literal_byte(&ast), Some(b'a'));
    }

    #[test]
    fn first_literal_byte_for_char_class_returns_none() {
        let ast = Regex::CharClass(crate::ast::CharClass::Custom {
            ranges: vec![CharRange::range('a', 'z')],
            negated: false,
        });
        assert_eq!(first_literal_byte(&ast), None);
    }

    #[test]
    fn first_literal_byte_for_dot_returns_none() {
        assert_eq!(first_literal_byte(&Regex::Dot), None);
    }

    #[test]
    fn first_literal_byte_for_realistic_log_pattern() {
        // ERROR — five literals
        let ast = Regex::Sequence(vec![lit('E'), lit('R'), lit('R'), lit('O'), lit('R')]);
        assert_eq!(first_literal_byte(&ast), Some(b'E'));
    }

    // ============================================================
    // C2 step 8: nested-quantifier dispatch heuristic
    // ============================================================

    fn one_or_more(expr: Regex) -> Regex {
        Regex::Quantified {
            quantifier: Quantifier::OneOrMore { lazy: false },
            expr: Box::new(expr),
        }
    }

    fn zero_or_more(expr: Regex) -> Regex {
        Regex::Quantified {
            quantifier: Quantifier::ZeroOrMore { lazy: false },
            expr: Box::new(expr),
        }
    }

    #[test]
    fn nested_quantifier_detected_for_classic_pathological_pattern() {
        // (a+)+ — the classic exponential-blowup pattern
        let inner = one_or_more(lit('a'));
        let outer = one_or_more(group_capturing(inner));
        assert!(has_nested_quantifier(&outer));
    }

    #[test]
    fn nested_quantifier_detected_through_sequence() {
        // ((a+)b)+ — nested via a sequence inside the group
        let inner = Regex::Sequence(vec![one_or_more(lit('a')), lit('b')]);
        let outer = one_or_more(group_capturing(inner));
        assert!(has_nested_quantifier(&outer));
    }

    #[test]
    fn nested_quantifier_detected_through_alternation() {
        // (a|b+)+ — quantifier inside an alternation branch
        let inner = alt(vec![lit('a'), one_or_more(lit('b'))]);
        let outer = one_or_more(group_capturing(inner));
        assert!(has_nested_quantifier(&outer));
    }

    #[test]
    fn nested_quantifier_detected_for_zero_or_more_outer() {
        // (a+)* — outer star, inner plus
        let inner = one_or_more(lit('a'));
        let outer = zero_or_more(group_capturing(inner));
        assert!(has_nested_quantifier(&outer));
    }

    #[test]
    fn no_nested_quantifier_for_flat_email_like_pattern() {
        // \w+@\w+.\w+ — three quantifiers but none nested inside
        // another quantifier. This is the email-style pattern that
        // should NOT route through Pike-VM (the existing VM is faster).
        let word = Regex::Word { negated: false };
        let dot = Regex::Char('.');
        let at = Regex::Char('@');
        let ast = Regex::Sequence(vec![
            one_or_more(word.clone()),
            at,
            one_or_more(word.clone()),
            dot,
            one_or_more(word),
        ]);
        assert!(!has_nested_quantifier(&ast));
    }

    #[test]
    fn no_nested_quantifier_for_flat_date_like_pattern() {
        // (\d){4}-(\d){2}-(\d){2}-style pattern: each capturing group
        // has a quantifier but the groups themselves aren't quantified.
        // Should NOT be routed to Pike-VM.
        let digit = Regex::Digit { negated: false };
        let ast = Regex::Sequence(vec![
            group_capturing_idx(1, one_or_more(digit.clone())),
            lit('-'),
            group_capturing_idx(2, one_or_more(digit.clone())),
            lit('-'),
            group_capturing_idx(3, one_or_more(digit)),
        ]);
        assert!(!has_nested_quantifier(&ast));
    }

    #[test]
    fn no_nested_quantifier_for_simple_literal_or_alternation() {
        assert!(!has_nested_quantifier(&lit('a')));
        assert!(!has_nested_quantifier(&alt(vec![lit('a'), lit('b')])));
    }

    #[test]
    fn nested_quantifier_recorded_on_compiled_program() {
        // Sanity-check the field is computed at construction time. Use
        // try_compile so the full compile pipeline runs (including the
        // capture-index assignment).
        let nested = CompiledC2Program::try_compile("(a+)+").expect("nested compiles");
        assert!(nested.c2_has_nested_quantifier);

        let flat = CompiledC2Program::try_compile("a+b+").expect("flat compiles");
        assert!(!flat.c2_has_nested_quantifier);
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
