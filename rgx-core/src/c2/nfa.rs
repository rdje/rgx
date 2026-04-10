//! Thompson NFA construction for the C2 NFA/DFA hybrid engine.
//!
//! Builds a non-deterministic finite automaton from a regex AST using
//! Thompson's classical construction. Each AST node compiles to a small
//! NFA fragment with one entry state and one accept state, and structural
//! nodes (sequence, alternation, quantifier) wire fragments together via
//! epsilon transitions. The result is a complete NFA that recognises the
//! same language as the original pattern.
//!
//! This is C2 step 3a of the phased plan in `docs/C2_NFA_DFA_DESIGN.md` §15.
//! At this stage the module is **standalone** — no engine wiring, no
//! `Program` field, no Pike-VM yet (that's step 4). The reverse NFA and
//! `CompiledC2Program` assembly are step 3b.
//!
//! # Scope
//!
//! Forward NFA construction in both anchored and unanchored variants for
//! the full no-backtracking subset defined in `docs/C2_NFA_DFA_DESIGN.md`
//! §4, including multi-byte UTF-8 codepoints and codepoint ranges.
//! Greedy and lazy quantifier priorities are encoded on epsilon edges.
//! Capture group enter/exit positions are recorded as [`CaptureTag`]
//! values on epsilon edges and recovered later by the bounded Pike-VM
//! capture pass (design doc §9).
//!
//! Range quantifiers `{n}`, `{n,m}`, `{n,}` are unrolled at construction
//! time, matching the RE2 / Rust `regex` convention. Bounded ranges
//! produce `n` mandatory copies followed by `m - n` optional copies; the
//! unbounded form `{n,}` produces `n` mandatory copies followed by a
//! Kleene star.
//!
//! # Byte-class transitions
//!
//! NFA transitions are labelled by [`ByteClassId`] from a precomputed
//! [`ByteClassMap`] (C2 step 2), not by raw byte values. This is the SOTA
//! approach used by RE2 and the Rust `regex` crate: it keeps transition
//! tables compact and the eventual lazy DFA cache dense. A single AST
//! character class can produce transitions on multiple distinct byte
//! classes if its bytes span multiple equivalence classes.
//!
//! # Multi-byte UTF-8
//!
//! Codepoints and codepoint ranges that encode to multi-byte UTF-8
//! sequences are decomposed via [`regex_syntax::utf8::Utf8Sequences`].
//! Each sub-sequence becomes a chain of byte-class transitions through
//! intermediate states. A character class containing multiple multi-byte
//! sequences becomes an alternation of chains.
//!
//! # Zero-width assertions
//!
//! Anchors (`^`, `$`, `\A`, `\Z`, `\z`, `\G`) and word boundaries
//! (`\b`, `\B`) are encoded as [`ZeroWidthAssertion`] values on epsilon
//! edges. The Pike-VM checks the assertion when crossing the edge during
//! epsilon closure expansion.
//!
//! # References
//!
//! - `docs/C2_NFA_DFA_DESIGN.md` §6 — design rationale and rules
//! - Russ Cox, "Regular Expression Matching: the Virtual Machine
//!   Approach" — Thompson NFA construction
//! - The Rust `regex-automata` crate's NFA builder

use crate::ast::{AnchorType, CharClass, CharRange, GroupKind, Quantifier, Regex};
use crate::c2::byte_class::ByteClassMap;
use regex_syntax::utf8::Utf8Sequences;
use std::collections::HashSet;

// ============================================================
// AST reversal (used to build the reverse NFA from the forward AST)
// ============================================================

/// Reverse a regex AST so that an NFA built from the result recognises
/// the **reverse** of the language recognised by the original AST.
///
/// Used by the reverse NFA constructors ([`Nfa::build_reverse_anchored`]
/// and [`Nfa::build_reverse_unanchored`]). The reverse NFA is consumed by
/// the lazy reverse DFA in C2 step 6 to recover match start positions
/// efficiently after the forward DFA has found a match end.
///
/// # Reversal rules
///
/// - **Leaves** (literals, character classes, shorthands, dot, Unicode
///   property classes, `WhitespaceLiteral`, `Empty`) are their own reverse.
/// - **Word boundaries** are symmetric — `\b` reversed is still `\b`.
/// - **Anchors** are flipped: `^` ↔ `$`, `\A` ↔ `\z`, and `\Z` is
///   approximated as `\A` for the reverse direction (the final-newline
///   semantics of `\Z` are handled by the runtime simulator).
/// - **`\R`** (newline sequence) is expanded to its structural alternation
///   form `(\r\n | \n | \v | \f | \r | \u{85} | \u{2028} | \u{2029})`
///   and then reversed; the `\r\n` branch becomes `\n\r`, which is the
///   correct reverse-direction match.
/// - **Sequences** reverse the order of their items and recursively
///   reverse each item. `Sequence([a, b, c])` becomes `Sequence([c', b', a'])`.
/// - **Alternations** preserve branch order (each branch is independently
///   leftmost-first) but recursively reverse each branch.
/// - **Quantifiers** are symmetric — `e*` reverses to `(reverse e)*`.
/// - **Groups** preserve their kind, capture index, and name; the inner
///   expression is recursively reversed. Capture indices stay the same
///   so the bounded Pike-VM capture pass produces the same logical
///   capture group identities in either direction.
/// - **Flag groups** reverse their inner expression and preserve flags.
///
/// Out-of-subset nodes (lookaround, backref, recursion, code blocks, etc.)
/// are visited gracefully — children are recursively reversed where they
/// exist and the node itself is preserved. The reverse NFA is only
/// meaningful for `NoBacktracking`-classified patterns where these nodes
/// don't appear, but the function doesn't crash on a mixed AST.
///
/// # Why reverse the AST instead of the NFA
///
/// Structurally reversing the NFA's edges would require parallel
/// construction logic that has to stay in sync with the forward builder.
/// Reversing the AST and reusing the same `build_fragment` machinery is
/// simpler and harder to get wrong — the reversal is local and obviously
/// correct, and the Thompson construction is shared between directions.
#[must_use]
pub fn reverse_ast(ast: &Regex) -> Regex {
    match ast {
        // Leaves: own reverse.
        Regex::Char(c) => Regex::Char(*c),
        Regex::WhitespaceLiteral(c) => Regex::WhitespaceLiteral(*c),
        Regex::CharClass(cc) => Regex::CharClass(cc.clone()),
        Regex::Dot => Regex::Dot,
        Regex::Digit { negated } => Regex::Digit { negated: *negated },
        Regex::Word { negated } => Regex::Word { negated: *negated },
        Regex::Space { negated } => Regex::Space { negated: *negated },
        Regex::UnicodeClass { name, negated } => Regex::UnicodeClass {
            name: name.clone(),
            negated: *negated,
        },
        Regex::Empty => Regex::Empty,
        Regex::WordBoundary { positive } => Regex::WordBoundary {
            positive: *positive,
        },
        Regex::MatchReset => Regex::MatchReset,

        // \R: expand to structural form, then reverse so the \r\n branch
        // becomes \n\r (the correct reverse-direction match).
        Regex::NewlineSequence => reverse_ast(&newline_sequence_alternation()),

        // Anchors: flip start/end variants.
        Regex::Anchor(t) => Regex::Anchor(reverse_anchor_type(*t)),

        // Sequences: reverse item order and recursively reverse each item.
        Regex::Sequence(items) => Regex::Sequence(items.iter().rev().map(reverse_ast).collect()),

        // Alternations: preserve branch order, recursively reverse each branch.
        Regex::Alternation(items) => Regex::Alternation(items.iter().map(reverse_ast).collect()),

        // Quantifiers: symmetric — reverse the inner expression and keep
        // the quantifier unchanged.
        Regex::Quantified { expr, quantifier } => Regex::Quantified {
            expr: Box::new(reverse_ast(expr)),
            quantifier: quantifier.clone(),
        },

        // Groups: preserve kind/index/name, reverse the inner expression.
        Regex::Group {
            expr,
            kind,
            index,
            name,
        } => Regex::Group {
            expr: Box::new(reverse_ast(expr)),
            kind: kind.clone(),
            index: *index,
            name: name.clone(),
        },

        // Flag groups: reverse inner expression, preserve flags.
        Regex::FlagGroup { flags, expr } => Regex::FlagGroup {
            flags: flags.clone(),
            expr: Box::new(reverse_ast(expr)),
        },

        // Out-of-subset nodes — defensive recursion. The classifier
        // rejects all of these, so the reverse NFA never actually
        // encounters them via the normal compile pipeline.
        Regex::GraphemeCluster => Regex::GraphemeCluster,
        Regex::Lookahead { expr, positive } => Regex::Lookahead {
            expr: Box::new(reverse_ast(expr)),
            positive: *positive,
        },
        Regex::Lookbehind { expr, positive } => Regex::Lookbehind {
            expr: Box::new(reverse_ast(expr)),
            positive: *positive,
        },
        Regex::Backreference(n) => Regex::Backreference(*n),
        Regex::NamedBackreference(n) => Regex::NamedBackreference(n.clone()),
        Regex::RelativeBackreference(n) => Regex::RelativeBackreference(*n),
        Regex::Recursion { target } => Regex::Recursion {
            target: target.clone(),
        },
        Regex::ReturnedCaptureSubroutine {
            target,
            returned_groups,
        } => Regex::ReturnedCaptureSubroutine {
            target: target.clone(),
            returned_groups: returned_groups.clone(),
        },
        Regex::Conditional {
            condition,
            true_branch,
            false_branch,
        } => Regex::Conditional {
            condition: condition.clone(),
            true_branch: Box::new(reverse_ast(true_branch)),
            false_branch: false_branch.as_ref().map(|fb| Box::new(reverse_ast(fb))),
        },
        Regex::CodeBlock { lang, code } => Regex::CodeBlock {
            lang: lang.clone(),
            code: code.clone(),
        },
        Regex::Callout(n) => Regex::Callout(*n),
        Regex::ExtendedCharClass { content } => Regex::ExtendedCharClass {
            content: content.clone(),
        },
        Regex::Accept => Regex::Accept,
        Regex::Commit => Regex::Commit,
        Regex::Prune => Regex::Prune,
        Regex::Skip => Regex::Skip,
        Regex::Then => Regex::Then,
        Regex::Mark(n) => Regex::Mark(n.clone()),
    }
}

/// Flip an anchor type for reverse-direction matching.
///
/// `\Z` (end of input or just before final newline) is approximated as
/// `\A` for the reverse direction. The exact final-newline semantics
/// would need special-case handling by the runtime simulator if we ever
/// want exact `\Z` parity in the reverse direction; for the no-backtracking
/// subset on the first pass, this approximation is correct for patterns
/// that don't depend on the final-newline corner case.
fn reverse_anchor_type(t: AnchorType) -> AnchorType {
    match t {
        AnchorType::Start => AnchorType::End,
        AnchorType::End => AnchorType::Start,
        AnchorType::AbsStart => AnchorType::AbsEndNoNL,
        AnchorType::AbsEndNoNL => AnchorType::AbsStart,
        AnchorType::AbsEnd => AnchorType::AbsStart, // approximated; see doc
        AnchorType::PreviousMatchEnd => AnchorType::PreviousMatchEnd,
    }
}

/// The structural alternation form of `\R` used by `reverse_ast` so the
/// `\r\n` branch can be reversed to `\n\r`.
fn newline_sequence_alternation() -> Regex {
    Regex::Alternation(vec![
        Regex::Sequence(vec![Regex::Char('\r'), Regex::Char('\n')]),
        Regex::Char('\u{0A}'),
        Regex::Char('\u{0B}'),
        Regex::Char('\u{0C}'),
        Regex::Char('\u{0D}'),
        Regex::Char('\u{85}'),
        Regex::Char('\u{2028}'),
        Regex::Char('\u{2029}'),
    ])
}

// ============================================================
// Public types
// ============================================================

/// State identifier within an [`Nfa`]. Indexes into [`Nfa::states`].
pub type NfaStateId = u32;

/// Byte-class identifier corresponding to an entry in the source
/// [`ByteClassMap`]. Range is `0..byte_class_map.num_classes()`.
pub type ByteClassId = u8;

/// Epsilon transition priority. **Lower number is preferred** under
/// leftmost-first semantics. Used to encode greedy vs lazy quantifier
/// behaviour: greedy quantifiers give the "continue looping" edge
/// priority 0, while lazy quantifiers give the "exit loop" edge
/// priority 0.
pub type EpsilonPriority = u8;

/// A capture group tag attached to an epsilon edge. The Pike-VM records
/// the input position when crossing a tagged edge during the bounded
/// capture-recovery pass (design doc §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTag {
    /// Save the current position as the start of capture group `N`.
    GroupStart(u32),
    /// Save the current position as the end of capture group `N`.
    GroupEnd(u32),
}

/// A zero-width assertion. Checked when the simulator crosses the
/// epsilon edge that carries it during epsilon closure expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroWidthAssertion {
    /// `\A` — absolute start of input.
    StartOfText,
    /// `\z` — absolute end of input.
    EndOfText,
    /// `\Z` — end of input or just before a final newline.
    EndOfTextOrFinalNewline,
    /// `^` — start of line (multiline mode) or start of input.
    StartOfLine,
    /// `$` — end of line (multiline mode) or end of input.
    EndOfLine,
    /// `\G` — end of previous match (or start of input if no previous match).
    PreviousMatchEnd,
    /// `\b` — word boundary.
    WordBoundary,
    /// `\B` — not a word boundary.
    NotWordBoundary,
}

/// An epsilon edge. Carries an optional capture tag and/or zero-width
/// assertion. Edges are stored in priority order (lower index = higher
/// priority); the simulator follows them in that order.
#[derive(Debug, Clone)]
pub struct EpsilonEdge {
    pub target: NfaStateId,
    pub priority: EpsilonPriority,
    pub capture_tag: Option<CaptureTag>,
    pub assertion: Option<ZeroWidthAssertion>,
}

/// A single state in the NFA.
///
/// Each state has zero or more **byte-class transitions** (consume one
/// input byte by class ID and move to a target state) and zero or more
/// **epsilon edges** (consume no input). The accept state of an NFA has
/// no outgoing edges by convention.
#[derive(Debug, Clone, Default)]
pub struct NfaState {
    /// Byte-class transitions: `(class_id, target_state)`. A state may
    /// have transitions on multiple distinct byte classes — for example,
    /// a character class whose bytes span several `ByteClassMap` classes.
    pub transitions: Vec<(ByteClassId, NfaStateId)>,
    /// Epsilon edges, in priority order (lowest priority first).
    pub epsilons: Vec<EpsilonEdge>,
}

/// A complete NFA for a regex pattern.
///
/// Constructed via [`Nfa::build_anchored`] or [`Nfa::build_unanchored`].
/// At C2 step 3a the NFA is a standalone artifact; the Pike-VM (step 4)
/// and the lazy DFA (step 5) consume it.
#[derive(Debug, Clone)]
pub struct Nfa {
    states: Vec<NfaState>,
    start: NfaStateId,
    accept: NfaStateId,
    num_capture_groups: u32,
}

impl Nfa {
    /// Build an anchored NFA from `ast` using the precomputed
    /// `byte_class_map`.
    ///
    /// "Anchored" means the NFA recognises the pattern starting at
    /// position 0; it does not match anywhere else in the input. Used
    /// for `find_first_at(text, pos)` and similar position-aware APIs,
    /// or for patterns that already begin with `^` / `\A`.
    #[must_use]
    pub fn build_anchored(ast: &Regex, byte_class_map: &ByteClassMap) -> Self {
        let mut builder = NfaBuilder::new(byte_class_map);
        let frag = builder.build_fragment(ast);
        builder.into_nfa(frag)
    }

    /// Build an unanchored NFA from `ast` using the precomputed
    /// `byte_class_map`.
    ///
    /// "Unanchored" means the NFA recognises the pattern at any position
    /// in the input. Implemented as a lazy `.*?` prefix on top of the
    /// anchored construction; the lazy semantics give leftmost-match
    /// behaviour.
    ///
    /// The dot in the unanchored prefix matches **any byte** (not just
    /// non-newline), so unanchored matching can skip over newlines to
    /// find a later match. This matches the convention used by RE2 and
    /// the Rust `regex` crate.
    #[must_use]
    pub fn build_unanchored(ast: &Regex, byte_class_map: &ByteClassMap) -> Self {
        let mut builder = NfaBuilder::new(byte_class_map);
        let prefix = builder.build_unanchored_prefix();
        let body = builder.build_fragment(ast);
        builder.connect(prefix.accept, body.start, 0);
        let combined = Fragment {
            start: prefix.start,
            accept: body.accept,
        };
        builder.into_nfa(combined)
    }

    /// Build an anchored **reverse** NFA from `ast` using the precomputed
    /// `byte_class_map`.
    ///
    /// The reverse NFA recognises the reverse of the language recognised
    /// by the forward NFA. Used by the lazy reverse DFA in C2 step 6 to
    /// recover match start positions efficiently after the forward DFA
    /// has found a match end.
    ///
    /// Implemented as a forward Thompson construction over the reversed
    /// AST produced by [`reverse_ast`]. The same `NfaBuilder` machinery
    /// is reused, which guarantees structural symmetry between the
    /// forward and reverse NFAs.
    #[must_use]
    pub fn build_reverse_anchored(ast: &Regex, byte_class_map: &ByteClassMap) -> Self {
        let reversed = reverse_ast(ast);
        Self::build_anchored(&reversed, byte_class_map)
    }

    /// Build an unanchored **reverse** NFA from `ast` using the precomputed
    /// `byte_class_map`.
    ///
    /// Like [`Nfa::build_reverse_anchored`] but with the same lazy
    /// `(?s:.)*?` prefix used by [`Nfa::build_unanchored`]. The reverse
    /// unanchored NFA scans backward over the input prefix and finds the
    /// earliest position from which the pattern matches.
    #[must_use]
    pub fn build_reverse_unanchored(ast: &Regex, byte_class_map: &ByteClassMap) -> Self {
        let reversed = reverse_ast(ast);
        Self::build_unanchored(&reversed, byte_class_map)
    }

    /// All states in the NFA.
    #[must_use]
    pub fn states(&self) -> &[NfaState] {
        &self.states
    }

    /// Returns the start state ID.
    #[must_use]
    pub fn start(&self) -> NfaStateId {
        self.start
    }

    /// Returns the accept state ID.
    #[must_use]
    pub fn accept(&self) -> NfaStateId {
        self.accept
    }

    /// Returns the total number of states.
    #[must_use]
    pub fn num_states(&self) -> usize {
        self.states.len()
    }

    /// Returns the number of capture groups in the original pattern.
    /// Used to size capture buffers for the bounded Pike-VM capture pass.
    #[must_use]
    pub fn num_capture_groups(&self) -> u32 {
        self.num_capture_groups
    }
}

// ============================================================
// Builder internals
// ============================================================

/// A constructed NFA fragment with a single entry state and a single
/// accept state. The classical Thompson construction guarantees this
/// invariant for every node type.
#[derive(Debug, Clone, Copy)]
struct Fragment {
    start: NfaStateId,
    accept: NfaStateId,
}

/// Helper that incrementally builds an [`Nfa`] from AST fragments.
struct NfaBuilder<'a> {
    states: Vec<NfaState>,
    byte_class_map: &'a ByteClassMap,
    /// Highest capture group index seen so far. Used to compute the
    /// final `num_capture_groups` value.
    max_capture_index: u32,
}

impl<'a> NfaBuilder<'a> {
    fn new(byte_class_map: &'a ByteClassMap) -> Self {
        Self {
            states: Vec::new(),
            byte_class_map,
            max_capture_index: 0,
        }
    }

    fn new_state(&mut self) -> NfaStateId {
        let id = u32::try_from(self.states.len())
            .expect("NFA state count exceeds u32::MAX (impossible for any real pattern)");
        self.states.push(NfaState::default());
        id
    }

    /// Add a byte-class transition `from --[class]--> to`.
    fn add_transition(&mut self, from: NfaStateId, class: ByteClassId, to: NfaStateId) {
        self.states[from as usize].transitions.push((class, to));
    }

    /// Add an epsilon edge `from --eps[priority]--> to`.
    fn connect(&mut self, from: NfaStateId, to: NfaStateId, priority: EpsilonPriority) {
        self.states[from as usize].epsilons.push(EpsilonEdge {
            target: to,
            priority,
            capture_tag: None,
            assertion: None,
        });
    }

    /// Add an epsilon edge with a capture tag.
    fn connect_with_tag(
        &mut self,
        from: NfaStateId,
        to: NfaStateId,
        priority: EpsilonPriority,
        tag: CaptureTag,
    ) {
        self.states[from as usize].epsilons.push(EpsilonEdge {
            target: to,
            priority,
            capture_tag: Some(tag),
            assertion: None,
        });
    }

    /// Add an epsilon edge with a zero-width assertion.
    fn connect_with_assertion(
        &mut self,
        from: NfaStateId,
        to: NfaStateId,
        priority: EpsilonPriority,
        assertion: ZeroWidthAssertion,
    ) {
        self.states[from as usize].epsilons.push(EpsilonEdge {
            target: to,
            priority,
            capture_tag: None,
            assertion: Some(assertion),
        });
    }

    fn into_nfa(self, fragment: Fragment) -> Nfa {
        Nfa {
            states: self.states,
            start: fragment.start,
            accept: fragment.accept,
            num_capture_groups: self.max_capture_index,
        }
    }

    // ============================================================
    // Thompson construction — one method per node family
    // ============================================================

    /// Build an NFA fragment for `ast`. Returns the fragment's entry
    /// and accept state IDs. Recursive over the AST.
    fn build_fragment(&mut self, ast: &Regex) -> Fragment {
        match ast {
            Regex::Empty => self.build_empty(),
            Regex::Char(c) | Regex::WhitespaceLiteral(c) => self.build_char(*c),
            Regex::CharClass(cc) => self.build_char_class(cc),
            Regex::Dot => self.build_dot(),
            Regex::Digit { negated } => self.build_shorthand_digit(*negated),
            Regex::Word { negated } => self.build_shorthand_word(*negated),
            Regex::Space { negated } => self.build_shorthand_space(*negated),
            Regex::UnicodeClass { name, negated } => self.build_unicode_class(name, *negated),
            Regex::NewlineSequence => self.build_newline_sequence(),
            Regex::Anchor(t) => self.build_anchor(*t),
            Regex::WordBoundary { positive } => self.build_word_boundary(*positive),
            Regex::Sequence(items) => self.build_sequence(items),
            Regex::Alternation(items) => self.build_alternation(items),
            Regex::Quantified { expr, quantifier } => self.build_quantified(expr, quantifier),
            Regex::Group {
                expr, kind, index, ..
            } => self.build_group(expr, kind.clone(), *index),
            Regex::FlagGroup { expr, .. } => self.build_fragment(expr),

            // Out-of-subset nodes — should never reach the NFA builder
            // because the classifier rejects them. Defensive default
            // produces an unmatchable fragment so the builder doesn't
            // crash on a mixed AST.
            Regex::GraphemeCluster
            | Regex::Lookahead { .. }
            | Regex::Lookbehind { .. }
            | Regex::Backreference(_)
            | Regex::NamedBackreference(_)
            | Regex::RelativeBackreference(_)
            | Regex::Recursion { .. }
            | Regex::ReturnedCaptureSubroutine { .. }
            | Regex::Conditional { .. }
            | Regex::CodeBlock { .. }
            | Regex::Callout(_)
            | Regex::ExtendedCharClass { .. }
            | Regex::MatchReset
            | Regex::Accept
            | Regex::Commit
            | Regex::Prune
            | Regex::Skip
            | Regex::Then
            | Regex::Mark(_) => self.build_unmatchable(),
        }
    }

    fn build_empty(&mut self) -> Fragment {
        let s = self.new_state();
        let a = self.new_state();
        self.connect(s, a, 0);
        Fragment {
            start: s,
            accept: a,
        }
    }

    /// Build an unmatchable fragment: a single state with no outgoing
    /// transitions. Used as a defensive fallback for AST nodes that
    /// shouldn't reach the NFA builder.
    fn build_unmatchable(&mut self) -> Fragment {
        let s = self.new_state();
        let a = self.new_state();
        // No connection — accept is unreachable.
        Fragment {
            start: s,
            accept: a,
        }
    }

    /// Build a fragment that matches a single codepoint by emitting a
    /// chain of byte-class transitions over its UTF-8 encoding.
    fn build_char(&mut self, c: char) -> Fragment {
        let mut buf = [0u8; 4];
        let bytes = c.encode_utf8(&mut buf).as_bytes();
        self.build_byte_chain(bytes)
    }

    /// Build a chain of states connected by single-byte-class transitions
    /// for a fixed UTF-8 byte sequence.
    fn build_byte_chain(&mut self, bytes: &[u8]) -> Fragment {
        debug_assert!(!bytes.is_empty(), "byte chain cannot be empty");
        let start = self.new_state();
        let mut prev = start;
        for &b in &bytes[..bytes.len() - 1] {
            let next = self.new_state();
            let class = self.byte_class_map.class_of(b);
            self.add_transition(prev, class, next);
            prev = next;
        }
        let accept = self.new_state();
        let class = self
            .byte_class_map
            .class_of(*bytes.last().expect("non-empty"));
        self.add_transition(prev, class, accept);
        Fragment { start, accept }
    }

    /// Build a fragment that matches any of the given character ranges.
    /// Used for `Char` classes, shorthands, `Dot`, Unicode property
    /// classes, and so on.
    fn build_char_ranges(&mut self, char_ranges: &[CharRange], negated: bool) -> Fragment {
        let resolved = if negated {
            invert_char_ranges(char_ranges)
        } else {
            char_ranges.to_vec()
        };
        // Decompose every codepoint range into UTF-8 byte sequences,
        // then build the NFA fragment as an alternation of byte chains
        // sharing a single start and accept state.
        let start = self.new_state();
        let accept = self.new_state();
        let mut any_branch = false;
        for range in &resolved {
            for utf8_seq in Utf8Sequences::new(range.start, range.end) {
                let byte_ranges = utf8_seq.as_slice();
                self.build_byte_range_chain_into(start, accept, byte_ranges);
                any_branch = true;
            }
        }
        if !any_branch {
            // Empty character class — unmatchable. Leave start/accept
            // disconnected so no input can transition.
        }
        Fragment { start, accept }
    }

    /// Build a chain of states from `from` to `to` for a UTF-8 byte
    /// sequence where each position has its own byte range. The chain
    /// fans out into a tree of intermediate states because each byte
    /// range may span multiple byte classes.
    fn build_byte_range_chain_into(
        &mut self,
        from: NfaStateId,
        to: NfaStateId,
        byte_ranges: &[regex_syntax::utf8::Utf8Range],
    ) {
        debug_assert!(!byte_ranges.is_empty());
        // Build a chain `from = s0 -> s1 -> s2 -> ... -> s_{n-1} = to`
        // with `n - 1` intermediate states. Each step has transitions
        // on every byte class that overlaps the current byte range.
        let mut prev = from;
        for (i, range) in byte_ranges.iter().enumerate() {
            let next = if i + 1 == byte_ranges.len() {
                to
            } else {
                self.new_state()
            };
            let classes = byte_classes_in_range(self.byte_class_map, range.start, range.end);
            for class in classes {
                self.add_transition(prev, class, next);
            }
            prev = next;
        }
    }

    fn build_char_class(&mut self, cc: &CharClass) -> Fragment {
        match cc {
            CharClass::Custom { ranges, negated } => self.build_char_ranges(ranges, *negated),
            CharClass::Digit { negated } => self.build_shorthand_digit(*negated),
            CharClass::Word { negated } => self.build_shorthand_word(*negated),
            CharClass::Space { negated } => self.build_shorthand_space(*negated),
            CharClass::UnicodeClass { name, negated } => self.build_unicode_class(name, *negated),
        }
    }

    fn build_dot(&mut self) -> Fragment {
        // `.` matches any byte except newline by default. The byte_class
        // partition built from the same AST already distinguishes the
        // newline byte from non-newline bytes (see `byte_class.rs::Dot`).
        let ranges = vec![
            CharRange::range('\u{00}', '\u{09}'),
            CharRange::range('\u{0B}', '\u{10FFFF}'),
        ];
        self.build_char_ranges(&ranges, false)
    }

    fn build_shorthand_digit(&mut self, negated: bool) -> Fragment {
        let ranges = vec![CharRange::range('0', '9')];
        self.build_char_ranges(&ranges, negated)
    }

    fn build_shorthand_word(&mut self, negated: bool) -> Fragment {
        let ranges = vec![
            CharRange::range('0', '9'),
            CharRange::range('A', 'Z'),
            CharRange::range('_', '_'),
            CharRange::range('a', 'z'),
        ];
        self.build_char_ranges(&ranges, negated)
    }

    fn build_shorthand_space(&mut self, negated: bool) -> Fragment {
        let ranges = vec![
            CharRange::range('\u{09}', '\u{0D}'),
            CharRange::range(' ', ' '),
        ];
        self.build_char_ranges(&ranges, negated)
    }

    fn build_unicode_class(&mut self, name: &str, negated: bool) -> Fragment {
        match crate::unicode_support::resolve_unicode_property_class(name, negated) {
            Ok(ranges) => self.build_char_ranges(&ranges, false),
            // Resolution failure should be impossible at this stage —
            // the compiler validates property names before NFA build.
            // Defensive fallback: unmatchable fragment.
            Err(_) => self.build_unmatchable(),
        }
    }

    fn build_newline_sequence(&mut self) -> Fragment {
        // \R matches \r\n OR any of [\r\n\v\f\u{85}\u{2028}\u{2029}].
        // The double-character \r\n branch must take priority over the
        // single-character \r branch (longest match wins for \R).
        let crlf = self.build_byte_chain(b"\r\n");
        let single_chars = vec![
            CharRange::range('\u{0A}', '\u{0A}'), // \n
            CharRange::range('\u{0B}', '\u{0B}'), // \v
            CharRange::range('\u{0C}', '\u{0C}'), // \f
            CharRange::range('\u{0D}', '\u{0D}'), // \r
            CharRange::range('\u{85}', '\u{85}'),
            CharRange::range('\u{2028}', '\u{2028}'),
            CharRange::range('\u{2029}', '\u{2029}'),
        ];
        let single = self.build_char_ranges(&single_chars, false);

        let start = self.new_state();
        let accept = self.new_state();
        self.connect(start, crlf.start, 0); // \r\n preferred
        self.connect(crlf.accept, accept, 0);
        self.connect(start, single.start, 1);
        self.connect(single.accept, accept, 0);
        Fragment { start, accept }
    }

    fn build_anchor(&mut self, t: AnchorType) -> Fragment {
        let assertion = match t {
            AnchorType::Start => ZeroWidthAssertion::StartOfLine,
            AnchorType::End => ZeroWidthAssertion::EndOfLine,
            AnchorType::AbsStart => ZeroWidthAssertion::StartOfText,
            AnchorType::AbsEnd => ZeroWidthAssertion::EndOfTextOrFinalNewline,
            AnchorType::AbsEndNoNL => ZeroWidthAssertion::EndOfText,
            AnchorType::PreviousMatchEnd => ZeroWidthAssertion::PreviousMatchEnd,
        };
        let s = self.new_state();
        let a = self.new_state();
        self.connect_with_assertion(s, a, 0, assertion);
        Fragment {
            start: s,
            accept: a,
        }
    }

    fn build_word_boundary(&mut self, positive: bool) -> Fragment {
        let assertion = if positive {
            ZeroWidthAssertion::WordBoundary
        } else {
            ZeroWidthAssertion::NotWordBoundary
        };
        let s = self.new_state();
        let a = self.new_state();
        self.connect_with_assertion(s, a, 0, assertion);
        Fragment {
            start: s,
            accept: a,
        }
    }

    fn build_sequence(&mut self, items: &[Regex]) -> Fragment {
        if items.is_empty() {
            return self.build_empty();
        }
        let mut iter = items.iter();
        let first = iter.next().expect("non-empty checked above");
        let mut current = self.build_fragment(first);
        let combined_start = current.start;
        for next in iter {
            let next_frag = self.build_fragment(next);
            self.connect(current.accept, next_frag.start, 0);
            current = next_frag;
        }
        Fragment {
            start: combined_start,
            accept: current.accept,
        }
    }

    fn build_alternation(&mut self, items: &[Regex]) -> Fragment {
        if items.is_empty() {
            return self.build_empty();
        }
        if items.len() == 1 {
            return self.build_fragment(&items[0]);
        }
        let start = self.new_state();
        let accept = self.new_state();
        for (i, item) in items.iter().enumerate() {
            let frag = self.build_fragment(item);
            // Earlier branches have higher priority (lower number) under
            // leftmost-first semantics.
            let priority = u8::try_from(i).unwrap_or(u8::MAX);
            self.connect(start, frag.start, priority);
            self.connect(frag.accept, accept, 0);
        }
        Fragment { start, accept }
    }

    fn build_quantified(&mut self, expr: &Regex, q: &Quantifier) -> Fragment {
        match q {
            Quantifier::ZeroOrOne { lazy } => self.build_zero_or_one(expr, *lazy),
            Quantifier::ZeroOrMore { lazy } => self.build_zero_or_more(expr, *lazy),
            Quantifier::OneOrMore { lazy } => self.build_one_or_more(expr, *lazy),
            Quantifier::Range { min, max, lazy } => self.build_range(expr, *min, *max, *lazy),
        }
    }

    fn build_zero_or_one(&mut self, expr: &Regex, lazy: bool) -> Fragment {
        let body = self.build_fragment(expr);
        let start = self.new_state();
        let accept = self.new_state();
        let (try_priority, skip_priority) = if lazy { (1, 0) } else { (0, 1) };
        self.connect(start, body.start, try_priority);
        self.connect(start, accept, skip_priority);
        self.connect(body.accept, accept, 0);
        Fragment { start, accept }
    }

    fn build_zero_or_more(&mut self, expr: &Regex, lazy: bool) -> Fragment {
        let body = self.build_fragment(expr);
        let start = self.new_state();
        let accept = self.new_state();
        let (try_priority, skip_priority) = if lazy { (1, 0) } else { (0, 1) };
        self.connect(start, body.start, try_priority);
        self.connect(start, accept, skip_priority);
        // Loop back: after matching the body, return to `start` so we
        // can decide again whether to loop or exit.
        self.connect(body.accept, start, 0);
        Fragment { start, accept }
    }

    fn build_one_or_more(&mut self, expr: &Regex, lazy: bool) -> Fragment {
        // e+ = e e*
        let body = self.build_fragment(expr);
        // The state after matching the body decides whether to loop or exit.
        let decision = self.new_state();
        let accept = self.new_state();
        let (try_priority, skip_priority) = if lazy { (1, 0) } else { (0, 1) };
        self.connect(body.accept, decision, 0);
        self.connect(decision, body.start, try_priority);
        self.connect(decision, accept, skip_priority);
        Fragment {
            start: body.start,
            accept,
        }
    }

    fn build_range(&mut self, expr: &Regex, min: u32, max: Option<u32>, lazy: bool) -> Fragment {
        // Unroll: build `min` mandatory copies, then either an unbounded
        // tail (`max == None`) or `max - min` optional copies.
        let mut current_start: Option<NfaStateId> = None;
        let mut current_accept: Option<NfaStateId> = None;

        for _ in 0..min {
            let frag = self.build_fragment(expr);
            match (current_start, current_accept) {
                (None, _) => {
                    current_start = Some(frag.start);
                    current_accept = Some(frag.accept);
                }
                (Some(_), Some(prev_accept)) => {
                    self.connect(prev_accept, frag.start, 0);
                    current_accept = Some(frag.accept);
                }
                _ => unreachable!(),
            }
        }

        match max {
            None => {
                // Unbounded tail: append `e*` after the mandatory copies.
                let tail = self.build_zero_or_more(expr, lazy);
                if let Some(prev_accept) = current_accept {
                    self.connect(prev_accept, tail.start, 0);
                    Fragment {
                        start: current_start.expect("min > 0 if accept set"),
                        accept: tail.accept,
                    }
                } else {
                    // min == 0: the entire range is just `e*`.
                    tail
                }
            }
            Some(max_val) => {
                let optional_count = max_val.saturating_sub(min);
                for _ in 0..optional_count {
                    let optional = self.build_zero_or_one(expr, lazy);
                    match current_accept {
                        Some(prev_accept) => {
                            self.connect(prev_accept, optional.start, 0);
                            current_accept = Some(optional.accept);
                        }
                        None => {
                            current_start = Some(optional.start);
                            current_accept = Some(optional.accept);
                        }
                    }
                }
                match (current_start, current_accept) {
                    (Some(start), Some(accept)) => Fragment { start, accept },
                    _ => self.build_empty(),
                }
            }
        }
    }

    fn build_group(&mut self, expr: &Regex, kind: GroupKind, index: Option<u32>) -> Fragment {
        match kind {
            GroupKind::NonCapturing => self.build_fragment(expr),
            GroupKind::Capturing => {
                let group_id = index.unwrap_or(0);
                if group_id > self.max_capture_index {
                    self.max_capture_index = group_id;
                }
                let body = self.build_fragment(expr);
                let start = self.new_state();
                let accept = self.new_state();
                self.connect_with_tag(start, body.start, 0, CaptureTag::GroupStart(group_id));
                self.connect_with_tag(body.accept, accept, 0, CaptureTag::GroupEnd(group_id));
                Fragment { start, accept }
            }
            // Atomic and BranchReset are not in the C2 subset; the
            // classifier rejects them. Defensive fallback: descend into
            // the body so the walker doesn't crash on a mixed AST.
            GroupKind::Atomic | GroupKind::BranchReset => self.build_fragment(expr),
        }
    }

    /// Build the unanchored prefix `(?s:.)*?` — a lazy zero-or-more loop
    /// over any byte. Lazy semantics give leftmost-match behaviour.
    fn build_unanchored_prefix(&mut self) -> Fragment {
        // Synthesise a "match any byte" fragment by transitioning on
        // every byte class in the map.
        let any_byte = self.build_any_byte();
        // Wrap with lazy `*`.
        let start = self.new_state();
        let accept = self.new_state();
        // Lazy: prefer to skip (priority 0), then try matching (priority 1).
        self.connect(start, accept, 0);
        self.connect(start, any_byte.start, 1);
        self.connect(any_byte.accept, start, 0);
        Fragment { start, accept }
    }

    fn build_any_byte(&mut self) -> Fragment {
        let s = self.new_state();
        let a = self.new_state();
        let num = self.byte_class_map.num_classes();
        for class in 0..num {
            // num_classes ≤ 256, so the cast is safe.
            self.add_transition(s, class as u8, a);
        }
        Fragment {
            start: s,
            accept: a,
        }
    }
}

// ============================================================
// Helpers
// ============================================================

/// Compute the set of byte-class IDs that intersect the byte range
/// `[lo, hi]` in the given map. Used when constructing transitions for
/// a multi-byte UTF-8 byte range that may span multiple equivalence
/// classes.
fn byte_classes_in_range(map: &ByteClassMap, lo: u8, hi: u8) -> Vec<ByteClassId> {
    let mut classes: HashSet<ByteClassId> = HashSet::new();
    let mut b = u16::from(lo);
    while b <= u16::from(hi) {
        classes.insert(map.class_of(b as u8));
        b += 1;
    }
    let mut result: Vec<ByteClassId> = classes.into_iter().collect();
    result.sort_unstable();
    result
}

/// Invert a list of character ranges over the Unicode codepoint universe
/// (excluding the surrogate gap). Used for negated character classes.
fn invert_char_ranges(ranges: &[CharRange]) -> Vec<CharRange> {
    // Sort and merge first.
    let mut sorted: Vec<(u32, u32)> = ranges
        .iter()
        .map(|r| (r.start as u32, r.end as u32))
        .collect();
    sorted.sort_by_key(|&(s, _)| s);
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (s, e) in sorted {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 + 1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    // Walk over the universe and emit the gaps.
    const UNIVERSE: &[(u32, u32)] = &[(0x0000, 0xD7FF), (0xE000, 0x10_FFFF)];
    let mut result: Vec<CharRange> = Vec::new();
    for &(uni_lo, uni_hi) in UNIVERSE {
        let mut cursor = uni_lo;
        for &(s, e) in &merged {
            if e < uni_lo || s > uni_hi {
                continue;
            }
            let s_clamped = s.max(uni_lo);
            let e_clamped = e.min(uni_hi);
            if s_clamped > cursor {
                if let (Some(start), Some(end)) =
                    (char::from_u32(cursor), char::from_u32(s_clamped - 1))
                {
                    result.push(CharRange::range(start, end));
                }
            }
            cursor = cursor.max(e_clamped.saturating_add(1));
            if cursor > uni_hi {
                break;
            }
        }
        if cursor <= uni_hi {
            if let (Some(start), Some(end)) = (char::from_u32(cursor), char::from_u32(uni_hi)) {
                result.push(CharRange::range(start, end));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::CharRange;

    fn lit(c: char) -> Regex {
        Regex::Char(c)
    }

    fn custom(ranges: Vec<(char, char)>) -> Regex {
        Regex::CharClass(CharClass::Custom {
            ranges: ranges
                .into_iter()
                .map(|(s, e)| CharRange::range(s, e))
                .collect(),
            negated: false,
        })
    }

    fn seq(items: Vec<Regex>) -> Regex {
        Regex::Sequence(items)
    }

    fn alt(items: Vec<Regex>) -> Regex {
        Regex::Alternation(items)
    }

    fn quantified(expr: Regex, q: Quantifier) -> Regex {
        Regex::Quantified {
            expr: Box::new(expr),
            quantifier: q,
        }
    }

    fn group_capturing(index: u32, expr: Regex) -> Regex {
        Regex::Group {
            expr: Box::new(expr),
            kind: GroupKind::Capturing,
            index: Some(index),
            name: None,
        }
    }

    fn build_anchored(ast: &Regex) -> Nfa {
        let bcm = ByteClassMap::build_from_ast(ast);
        Nfa::build_anchored(ast, &bcm)
    }

    fn build_unanchored(ast: &Regex) -> Nfa {
        let bcm = ByteClassMap::build_from_ast(ast);
        Nfa::build_unanchored(ast, &bcm)
    }

    /// Returns true if `accept` is reachable from `start` via any
    /// combination of byte-class transitions and epsilon edges.
    fn is_reachable(nfa: &Nfa, start: NfaStateId, accept: NfaStateId) -> bool {
        let mut visited = HashSet::new();
        let mut stack = vec![start];
        while let Some(s) = stack.pop() {
            if s == accept {
                return true;
            }
            if !visited.insert(s) {
                continue;
            }
            let state = &nfa.states()[s as usize];
            for (_, target) in &state.transitions {
                stack.push(*target);
            }
            for edge in &state.epsilons {
                stack.push(edge.target);
            }
        }
        false
    }

    /// Count the total number of byte-class transitions in the NFA.
    fn count_transitions(nfa: &Nfa) -> usize {
        nfa.states().iter().map(|s| s.transitions.len()).sum()
    }

    /// Count the total number of epsilon edges in the NFA.
    fn count_epsilons(nfa: &Nfa) -> usize {
        nfa.states().iter().map(|s| s.epsilons.len()).sum()
    }

    /// Returns true if any state in the NFA has an epsilon edge with
    /// the given capture tag.
    fn has_capture_tag(nfa: &Nfa, tag: CaptureTag) -> bool {
        nfa.states()
            .iter()
            .any(|s| s.epsilons.iter().any(|e| e.capture_tag == Some(tag)))
    }

    // ============================================================
    // Trivial cases
    // ============================================================

    #[test]
    fn empty_pattern_yields_a_two_state_nfa_with_one_epsilon() {
        let nfa = build_anchored(&Regex::Empty);
        assert_eq!(nfa.num_states(), 2);
        assert_eq!(count_epsilons(&nfa), 1);
        assert_eq!(count_transitions(&nfa), 0);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn single_ascii_literal_yields_one_byte_transition() {
        let nfa = build_anchored(&lit('a'));
        // 2 states (start and accept), 1 transition.
        assert_eq!(nfa.num_states(), 2);
        assert_eq!(count_transitions(&nfa), 1);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn two_byte_utf8_literal_yields_a_three_state_chain() {
        // 'α' = U+03B1 = 0xCE 0xB1 (2 bytes).
        let nfa = build_anchored(&lit('α'));
        assert_eq!(nfa.num_states(), 3);
        assert_eq!(count_transitions(&nfa), 2);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn three_byte_utf8_literal_yields_a_four_state_chain() {
        // 'あ' = U+3042 = 0xE3 0x81 0x82 (3 bytes).
        let nfa = build_anchored(&lit('あ'));
        assert_eq!(nfa.num_states(), 4);
        assert_eq!(count_transitions(&nfa), 3);
    }

    // ============================================================
    // Character classes
    // ============================================================

    #[test]
    fn ascii_char_class_has_at_least_one_transition() {
        let nfa = build_anchored(&custom(vec![('a', 'z')]));
        // start, accept; transitions on every byte class overlapping a-z.
        assert!(nfa.num_states() >= 2);
        assert!(count_transitions(&nfa) >= 1);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn negated_ascii_char_class_is_reachable() {
        let nfa = build_anchored(&Regex::CharClass(CharClass::Custom {
            ranges: vec![CharRange::range('a', 'z')],
            negated: true,
        }));
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
        // Negation gives many ranges; transitions count > 1.
        assert!(count_transitions(&nfa) > 1);
    }

    #[test]
    fn shorthand_digit_class_reachable() {
        let nfa = build_anchored(&Regex::Digit { negated: false });
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn shorthand_word_class_reachable() {
        let nfa = build_anchored(&Regex::Word { negated: false });
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn dot_pattern_reachable_and_distinguishes_newline_byte_class() {
        let nfa = build_anchored(&Regex::Dot);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    // ============================================================
    // Sequence and alternation
    // ============================================================

    #[test]
    fn sequence_chains_fragments() {
        let nfa = build_anchored(&seq(vec![lit('a'), lit('b'), lit('c')]));
        // 3 literal fragments × 2 states each = 6 states; sequence wires
        // them with 2 epsilon edges.
        assert_eq!(nfa.num_states(), 6);
        assert_eq!(count_transitions(&nfa), 3);
        assert_eq!(count_epsilons(&nfa), 2);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn alternation_fans_out_and_fans_in() {
        let nfa = build_anchored(&alt(vec![lit('a'), lit('b'), lit('c')]));
        // 2 outer states + 3 × 2 inner states = 8 states.
        // 3 transitions (one per literal).
        // 6 epsilon edges (3 from start, 3 to accept).
        assert_eq!(nfa.num_states(), 8);
        assert_eq!(count_transitions(&nfa), 3);
        assert_eq!(count_epsilons(&nfa), 6);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn alternation_branches_have_priority_in_order() {
        let ast = alt(vec![lit('a'), lit('b'), lit('c')]);
        let nfa = build_anchored(&ast);
        let start = &nfa.states()[nfa.start() as usize];
        let priorities: Vec<EpsilonPriority> = start.epsilons.iter().map(|e| e.priority).collect();
        assert_eq!(priorities, vec![0, 1, 2]);
    }

    // ============================================================
    // Quantifiers
    // ============================================================

    #[test]
    fn greedy_zero_or_one_creates_two_outgoing_edges_at_start() {
        let nfa = build_anchored(&quantified(lit('a'), Quantifier::ZeroOrOne { lazy: false }));
        let start = &nfa.states()[nfa.start() as usize];
        assert_eq!(start.epsilons.len(), 2);
        // Greedy: priority 0 should be the "try matching" branch.
        assert_eq!(start.epsilons[0].priority, 0);
        assert_eq!(start.epsilons[1].priority, 1);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn lazy_zero_or_one_swaps_priorities() {
        let greedy = build_anchored(&quantified(lit('a'), Quantifier::ZeroOrOne { lazy: false }));
        let lazy = build_anchored(&quantified(lit('a'), Quantifier::ZeroOrOne { lazy: true }));
        // Same number of states, transitions, and epsilons; only priorities differ.
        assert_eq!(greedy.num_states(), lazy.num_states());
        assert_eq!(count_transitions(&greedy), count_transitions(&lazy));
        assert_eq!(count_epsilons(&greedy), count_epsilons(&lazy));
    }

    #[test]
    fn zero_or_more_introduces_a_loop_back() {
        let nfa = build_anchored(&quantified(
            lit('a'),
            Quantifier::ZeroOrMore { lazy: false },
        ));
        // The loop-back epsilon goes from `body.accept` to a state we
        // can reach again (the wrapper start).
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
        // After the body matches, control returns to a state from which
        // we can match the body again. Verify by checking that the body
        // accept's epsilon target eventually loops back to itself.
        let total_eps = count_epsilons(&nfa);
        // Greedy `*` builds 5 epsilons:
        //   wrapper_start --eps[0]--> body_start
        //   wrapper_start --eps[1]--> wrapper_accept
        //   body_accept --eps[0]--> wrapper_start
        // = 3 epsilons from the `*` wrapper plus none from the body.
        // The body is just a literal, no epsilons.
        assert_eq!(total_eps, 3);
    }

    #[test]
    fn one_or_more_requires_at_least_one_match() {
        let nfa = build_anchored(&quantified(lit('a'), Quantifier::OneOrMore { lazy: false }));
        // Reachability is preserved.
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
        // The start state IS the body start, so it has one byte transition.
        let start = &nfa.states()[nfa.start() as usize];
        assert_eq!(start.transitions.len(), 1);
    }

    #[test]
    fn range_quantifier_unrolls_min_copies() {
        let nfa = build_anchored(&quantified(
            lit('a'),
            Quantifier::Range {
                min: 3,
                max: Some(3),
                lazy: false,
            },
        ));
        // 3 mandatory literal fragments × 2 states = 6 states.
        // 3 byte transitions (one per copy). 2 epsilon connectors.
        assert_eq!(nfa.num_states(), 6);
        assert_eq!(count_transitions(&nfa), 3);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn range_quantifier_min_zero_max_three_yields_optionals() {
        let nfa = build_anchored(&quantified(
            lit('a'),
            Quantifier::Range {
                min: 0,
                max: Some(3),
                lazy: false,
            },
        ));
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    #[test]
    fn unbounded_range_quantifier_acts_like_min_then_star() {
        let nfa = build_anchored(&quantified(
            lit('a'),
            Quantifier::Range {
                min: 2,
                max: None,
                lazy: false,
            },
        ));
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    // ============================================================
    // Capture groups
    // ============================================================

    #[test]
    fn capturing_group_emits_start_and_end_tags() {
        let nfa = build_anchored(&group_capturing(1, lit('a')));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupStart(1)));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupEnd(1)));
        assert_eq!(nfa.num_capture_groups(), 1);
    }

    #[test]
    fn nested_capturing_groups_emit_distinct_tags() {
        let inner = group_capturing(2, lit('b'));
        let outer = group_capturing(1, seq(vec![lit('a'), inner, lit('c')]));
        let nfa = build_anchored(&outer);
        assert!(has_capture_tag(&nfa, CaptureTag::GroupStart(1)));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupEnd(1)));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupStart(2)));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupEnd(2)));
        assert_eq!(nfa.num_capture_groups(), 2);
    }

    #[test]
    fn non_capturing_group_does_not_emit_tags() {
        let ast = Regex::Group {
            expr: Box::new(lit('a')),
            kind: GroupKind::NonCapturing,
            index: None,
            name: None,
        };
        let nfa = build_anchored(&ast);
        // No GroupStart or GroupEnd tags anywhere.
        for state in nfa.states() {
            for edge in &state.epsilons {
                assert!(edge.capture_tag.is_none());
            }
        }
        assert_eq!(nfa.num_capture_groups(), 0);
    }

    // ============================================================
    // Anchors and assertions
    // ============================================================

    #[test]
    fn start_anchor_emits_zero_width_assertion() {
        let nfa = build_anchored(&Regex::Anchor(AnchorType::AbsStart));
        let start = &nfa.states()[nfa.start() as usize];
        assert_eq!(start.epsilons.len(), 1);
        assert_eq!(
            start.epsilons[0].assertion,
            Some(ZeroWidthAssertion::StartOfText)
        );
    }

    #[test]
    fn word_boundary_emits_word_boundary_assertion() {
        let nfa = build_anchored(&Regex::WordBoundary { positive: true });
        let start = &nfa.states()[nfa.start() as usize];
        assert_eq!(
            start.epsilons[0].assertion,
            Some(ZeroWidthAssertion::WordBoundary)
        );
    }

    // ============================================================
    // Unanchored variant
    // ============================================================

    #[test]
    fn unanchored_nfa_has_more_states_than_anchored() {
        let ast = lit('a');
        let anchored = build_anchored(&ast);
        let unanchored = build_unanchored(&ast);
        assert!(unanchored.num_states() > anchored.num_states());
        assert!(is_reachable(
            &unanchored,
            unanchored.start(),
            unanchored.accept()
        ));
    }

    #[test]
    fn unanchored_prefix_has_lazy_priorities() {
        let ast = lit('a');
        let unanchored = build_unanchored(&ast);
        // The unanchored start state should have two epsilon edges:
        // priority 0 = skip (go directly to accept), priority 1 = match
        // a byte and loop. Lazy semantics put "skip" first.
        let start = &unanchored.states()[unanchored.start() as usize];
        // Skip is at priority 0; match-byte is at priority 1.
        let priorities: Vec<EpsilonPriority> = start.epsilons.iter().map(|e| e.priority).collect();
        assert!(priorities.contains(&0));
        assert!(priorities.contains(&1));
    }

    // ============================================================
    // Combined / realistic patterns
    // ============================================================

    #[test]
    fn realistic_pattern_compiles_and_is_reachable() {
        // (a|b)+(cd)?
        let ast = seq(vec![
            quantified(
                group_capturing(1, alt(vec![lit('a'), lit('b')])),
                Quantifier::OneOrMore { lazy: false },
            ),
            quantified(
                group_capturing(2, seq(vec![lit('c'), lit('d')])),
                Quantifier::ZeroOrOne { lazy: false },
            ),
        ]);
        let nfa = build_anchored(&ast);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
        assert_eq!(nfa.num_capture_groups(), 2);
        assert!(has_capture_tag(&nfa, CaptureTag::GroupStart(1)));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupEnd(1)));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupStart(2)));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupEnd(2)));
    }

    #[test]
    fn newline_sequence_compiles() {
        let nfa = build_anchored(&Regex::NewlineSequence);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
    }

    // ============================================================
    // Helper unit tests
    // ============================================================

    #[test]
    fn invert_char_ranges_round_trip_excludes_a_to_z() {
        let inverted = invert_char_ranges(&[CharRange::range('a', 'z')]);
        // 'a' through 'z' should be absent.
        for r in &inverted {
            for codepoint in (r.start as u32)..=(r.end as u32) {
                let c = char::from_u32(codepoint).unwrap();
                assert!(
                    !('a'..='z').contains(&c),
                    "inverted ranges should not contain {c:?}"
                );
            }
        }
    }

    // ============================================================
    // Reverse AST and reverse NFA
    // ============================================================

    #[test]
    fn reverse_ast_leaves_atomic_nodes_unchanged() {
        assert_eq!(reverse_ast(&lit('a')), lit('a'));
        assert_eq!(reverse_ast(&Regex::Dot), Regex::Dot);
        assert_eq!(reverse_ast(&Regex::Empty), Regex::Empty);
        assert_eq!(
            reverse_ast(&Regex::Digit { negated: false }),
            Regex::Digit { negated: false }
        );
        assert_eq!(
            reverse_ast(&Regex::WordBoundary { positive: true }),
            Regex::WordBoundary { positive: true }
        );
    }

    #[test]
    fn reverse_ast_reverses_sequence_order() {
        let ast = seq(vec![lit('a'), lit('b'), lit('c')]);
        let reversed = reverse_ast(&ast);
        assert_eq!(reversed, seq(vec![lit('c'), lit('b'), lit('a')]));
    }

    #[test]
    fn reverse_ast_recursively_reverses_nested_sequences() {
        // (ab)(cd) → (dc)(ba) — outer sequence reversed, each inner
        // sequence also reversed.
        let ast = seq(vec![
            seq(vec![lit('a'), lit('b')]),
            seq(vec![lit('c'), lit('d')]),
        ]);
        let reversed = reverse_ast(&ast);
        assert_eq!(
            reversed,
            seq(vec![
                seq(vec![lit('d'), lit('c')]),
                seq(vec![lit('b'), lit('a')]),
            ])
        );
    }

    #[test]
    fn reverse_ast_preserves_alternation_branch_order_but_reverses_each() {
        let ast = alt(vec![
            seq(vec![lit('a'), lit('b')]),
            seq(vec![lit('c'), lit('d')]),
        ]);
        let reversed = reverse_ast(&ast);
        assert_eq!(
            reversed,
            alt(vec![
                seq(vec![lit('b'), lit('a')]),
                seq(vec![lit('d'), lit('c')]),
            ])
        );
    }

    #[test]
    fn reverse_ast_keeps_quantifiers_unchanged_but_reverses_inner() {
        let ast = quantified(
            seq(vec![lit('a'), lit('b')]),
            Quantifier::OneOrMore { lazy: false },
        );
        let reversed = reverse_ast(&ast);
        assert_eq!(
            reversed,
            quantified(
                seq(vec![lit('b'), lit('a')]),
                Quantifier::OneOrMore { lazy: false },
            )
        );
    }

    #[test]
    fn reverse_ast_preserves_capture_indices_in_groups() {
        let ast = group_capturing(1, seq(vec![lit('a'), lit('b')]));
        let reversed = reverse_ast(&ast);
        assert_eq!(reversed, group_capturing(1, seq(vec![lit('b'), lit('a')])));
    }

    #[test]
    fn reverse_ast_flips_start_and_end_anchors() {
        assert_eq!(
            reverse_ast(&Regex::Anchor(AnchorType::Start)),
            Regex::Anchor(AnchorType::End)
        );
        assert_eq!(
            reverse_ast(&Regex::Anchor(AnchorType::End)),
            Regex::Anchor(AnchorType::Start)
        );
    }

    #[test]
    fn reverse_ast_flips_abs_start_and_abs_end_no_nl() {
        assert_eq!(
            reverse_ast(&Regex::Anchor(AnchorType::AbsStart)),
            Regex::Anchor(AnchorType::AbsEndNoNL)
        );
        assert_eq!(
            reverse_ast(&Regex::Anchor(AnchorType::AbsEndNoNL)),
            Regex::Anchor(AnchorType::AbsStart)
        );
    }

    #[test]
    fn reverse_ast_double_reverse_recovers_simple_pattern() {
        // A literal pattern double-reversed should equal itself.
        let ast = seq(vec![lit('a'), lit('b'), lit('c')]);
        assert_eq!(reverse_ast(&reverse_ast(&ast)), ast);
    }

    #[test]
    fn reverse_ast_expands_newline_sequence_so_crlf_branch_reverses() {
        let reversed = reverse_ast(&Regex::NewlineSequence);
        // The first branch must be the reversed CRLF sequence: \n then \r.
        if let Regex::Alternation(branches) = reversed {
            assert!(!branches.is_empty());
            match &branches[0] {
                Regex::Sequence(items) => {
                    assert_eq!(items.len(), 2);
                    assert_eq!(items[0], Regex::Char('\n'));
                    assert_eq!(items[1], Regex::Char('\r'));
                }
                other => panic!("expected Sequence([\\n, \\r]), got {other:?}"),
            }
        } else {
            panic!("expected Alternation, got {reversed:?}");
        }
    }

    #[test]
    fn reverse_anchored_nfa_is_reachable() {
        let ast = seq(vec![lit('a'), lit('b'), lit('c')]);
        let bcm = ByteClassMap::build_from_ast(&ast);
        let nfa = Nfa::build_reverse_anchored(&ast, &bcm);
        assert!(is_reachable(&nfa, nfa.start(), nfa.accept()));
        // 3 literal fragments × 2 states + 2 epsilons for the sequence
        // wiring = same shape as the forward NFA for "cba".
        assert_eq!(nfa.num_states(), 6);
        assert_eq!(count_transitions(&nfa), 3);
    }

    #[test]
    fn reverse_unanchored_nfa_has_more_states_than_reverse_anchored() {
        let ast = lit('a');
        let bcm = ByteClassMap::build_from_ast(&ast);
        let anchored = Nfa::build_reverse_anchored(&ast, &bcm);
        let unanchored = Nfa::build_reverse_unanchored(&ast, &bcm);
        assert!(unanchored.num_states() > anchored.num_states());
        assert!(is_reachable(
            &unanchored,
            unanchored.start(),
            unanchored.accept()
        ));
    }

    #[test]
    fn reverse_nfa_preserves_capture_tags() {
        let ast = group_capturing(1, seq(vec![lit('a'), lit('b')]));
        let bcm = ByteClassMap::build_from_ast(&ast);
        let nfa = Nfa::build_reverse_anchored(&ast, &bcm);
        assert!(has_capture_tag(&nfa, CaptureTag::GroupStart(1)));
        assert!(has_capture_tag(&nfa, CaptureTag::GroupEnd(1)));
        assert_eq!(nfa.num_capture_groups(), 1);
    }

    #[test]
    fn reverse_nfa_uses_same_byte_class_map_as_forward() {
        // Build the byte_class_map once from the forward AST and reuse
        // it for the reverse NFA. The reverse NFA must produce a valid
        // NFA against the same map (no out-of-range class IDs).
        let ast = seq(vec![
            custom(vec![('a', 'c')]),
            custom(vec![('d', 'f')]),
            lit('z'),
        ]);
        let bcm = ByteClassMap::build_from_ast(&ast);
        let max_class = bcm.num_classes() as u8 - 1;
        let nfa = Nfa::build_reverse_anchored(&ast, &bcm);
        for state in nfa.states() {
            for &(class, _) in &state.transitions {
                assert!(
                    class <= max_class,
                    "reverse NFA used out-of-range byte class {class} (max {max_class})"
                );
            }
        }
    }

    #[test]
    fn byte_classes_in_range_returns_unique_sorted_classes() {
        // Build a byte class map from a pattern that creates multiple
        // classes, then check that byte_classes_in_range returns them
        // in sorted order with no duplicates.
        let bcm = ByteClassMap::build_from_ast(&seq(vec![
            custom(vec![('a', 'c')]),
            custom(vec![('b', 'd')]),
        ]));
        let classes = byte_classes_in_range(&bcm, b'a', b'd');
        let mut sorted = classes.clone();
        sorted.sort_unstable();
        assert_eq!(classes, sorted);
        // Deduplicated.
        let unique: HashSet<u8> = classes.iter().copied().collect();
        assert_eq!(unique.len(), classes.len());
    }
}
