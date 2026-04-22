//! Byte-class equivalence partitioning for the C2 NFA/DFA hybrid engine.
//!
//! Computes a compact equivalence relation over the 256 possible byte
//! values such that two bytes are in the same class iff every character
//! class and literal in the pattern treats them identically. NFA and DFA
//! transitions then index by class ID rather than by raw byte value, which
//! keeps state transition tables small and the lazy DFA cache dense.
//!
//! This is C2 step 2 of the phased plan in `docs/C2_NFA_DFA_DESIGN.md` §15.
//! At this stage the module is **standalone** — no engine wiring, no
//! `Program` field. The NFA construction in C2 step 3 will consume this
//! map; until then it can be tested in isolation.
//!
//! # Algorithm
//!
//! Each character class / literal / shorthand / `Dot` / property class in
//! the pattern is one **membership oracle**: a set of byte ranges (with
//! UTF-8 multi-byte sequences decomposed into per-position byte ranges).
//! Two bytes are in the same equivalence class iff every membership oracle
//! gives the same answer for both bytes.
//!
//! The boundary-points algorithm computes this efficiently:
//!
//! 1. Walk the AST in pre-order and collect each pattern construct's byte
//!    ranges into its own membership oracle.
//! 2. Collect all `(start, end+1)` boundary points across every oracle.
//! 3. Sort and deduplicate the boundary points. Each maximal interval
//!    between consecutive boundaries is a candidate byte class.
//! 4. For each candidate interval, compute its **membership signature** —
//!    a `Vec<bool>` recording whether the interval is contained in each
//!    oracle. Intervals with identical signatures share the same class ID.
//! 5. Fill the lookup table.
//!
//! Cost: linear in the AST size for collection, O(n log n) for boundary
//! sort, O(n × k) for signature computation where n is the number of
//! intervals (≤ 257) and k is the number of oracles. Computed once per
//! pattern at compile time.
//!
//! # Conservative over-approximation
//!
//! The map is computed from the AST, before the NFA is built. This is a
//! conservative over-approximation: extra distinctions may be introduced
//! that the NFA wouldn't actually need. Extra classes never affect
//! correctness — only the compactness of the resulting DFA. The NFA
//! construction in step 3 may refine the map further if profiling shows
//! it matters.
//!
//! # UTF-8 handling
//!
//! Multi-byte UTF-8 codepoint ranges are decomposed via
//! [`regex_syntax::utf8::Utf8Sequences`] into per-position byte ranges.
//! Each per-position byte range is added to the corresponding oracle.
//! This is the same approach used by RE2 and the Rust `regex` crate.
//!
//! # References
//!
//! - `docs/C2_NFA_DFA_DESIGN.md` §5 — design rationale and algorithm
//! - Russ Cox, "Regular Expression Matching in the Wild" — RE2 byte classes
//! - The Rust `regex-automata` crate's `ByteClasses` type

use crate::ast::{CharClass, Regex};
use regex_syntax::utf8::Utf8Sequences;
use std::collections::HashMap;

/// Byte-class equivalence map for a compiled pattern.
///
/// Maps each of the 256 possible bytes to a class ID such that two bytes
/// in the same class are treated identically by every character class and
/// literal in the pattern. NFA and DFA transitions index by class ID
/// instead of raw byte value.
///
/// See module documentation for the algorithm and the
/// `docs/C2_NFA_DFA_DESIGN.md` §5 for the full design rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteClassMap {
    /// `byte → class id`. Class IDs are dense, starting from 0.
    table: [u8; 256],
    /// Number of distinct classes (1..=256).
    ///
    /// `u16` because the count can be 256 (one class per byte) which
    /// doesn't fit in `u8`. Class IDs themselves are always in `0..256`
    /// so they fit in `u8` regardless.
    num_classes: u16,
}

impl ByteClassMap {
    /// Build a byte-class map from a regex AST.
    ///
    /// Walks the AST in pre-order, collects each construct's membership
    /// oracle, and partitions the 256-byte alphabet via the boundary-
    /// points algorithm described in the module docs.
    ///
    /// Cost: linear in the AST size for collection, plus O(n log n) where
    /// n is the number of distinct boundary points. Computed once per
    /// pattern at compile time.
    #[must_use]
    pub fn build_from_ast(ast: &Regex) -> Self {
        let mut oracles: Vec<Vec<(u8, u8)>> = Vec::new();
        collect_oracles(ast, &mut oracles);
        // Force the partition to distinguish UTF-8 byte categories
        // (continuation, 2/3/4-byte leading) from ASCII bytes when
        // the AST contains a construct that produces multi-byte
        // chains in the NFA. Without this, a negated character class
        // like `[^0-9]` produces an NFA with multi-byte chains
        // (because the negated range spans non-ASCII Unicode), but
        // the byte_class_map only knows about the positive ASCII
        // range — so the chains' transitions on "non-digit" fire on
        // ANY non-digit byte, including ASCII bytes that are not
        // valid UTF-8 continuation/leading bytes. The result is
        // that `pike_match_at` walks past the leftmost single-char
        // accept and records subsequent "longer" matches that don't
        // correspond to valid UTF-8 characters in the input.
        //
        // Adding the UTF-8 boundary oracles forces the partition to
        // separate ASCII bytes from continuation bytes, leading
        // bytes, etc. The chains' transitions on the leading-byte
        // ranges then ONLY fire for actual leading bytes, which
        // means the multi-byte chains correctly die when they
        // encounter ASCII input.
        //
        // We add the oracles unconditionally — the partition cost
        // is at most 5 extra equivalence classes for every pattern,
        // which is negligible given that DFA states are sparse
        // arrays indexed by class. See `byte_class.rs::tests::partition_distinguishes_utf8_byte_categories`
        // for the partition shape.
        push_utf8_byte_boundary_oracles(&mut oracles);
        Self::from_oracles(&oracles)
    }

    /// Build a byte-class map directly from a list of membership oracles.
    ///
    /// Each oracle is a set of byte ranges (inclusive on both ends). Used
    /// internally by [`build_from_ast`] after walking the AST and by
    /// direct unit tests of the partition algorithm.
    ///
    /// [`build_from_ast`]: Self::build_from_ast
    fn from_oracles(oracles: &[Vec<(u8, u8)>]) -> Self {
        // Boundary-points algorithm.
        //
        // Step 1: collect all (start, end+1) values from every oracle.
        // The 0 and 256 sentinels frame the universe so the windows
        // iteration covers every byte exactly once.
        //
        // We use u16 because end+1 can equal 256, which doesn't fit in u8.
        let mut boundaries: Vec<u16> =
            Vec::with_capacity(2 + oracles.iter().map(Vec::len).sum::<usize>() * 2);
        boundaries.push(0);
        boundaries.push(256);
        for oracle in oracles {
            for &(start, end) in oracle {
                boundaries.push(u16::from(start));
                boundaries.push(u16::from(end) + 1);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        // Step 2: walk consecutive boundary pairs. Each pair defines a
        // maximal interval `[lo, hi]` (both inclusive) where membership in
        // every oracle is uniform. Compute the signature, deduplicate via
        // a HashMap, and assign a class ID.
        let mut signature_to_class: HashMap<Vec<bool>, u8> = HashMap::new();
        let mut table = [0u8; 256];
        let mut num_classes: u16 = 0;

        for window in boundaries.windows(2) {
            // After dedup, the last boundary is always 256 and every
            // earlier boundary is < 256, so `window[0] < 256` for every
            // pair. `hi` is `window[1] - 1` which is always ≤ 255.
            let lo = window[0] as u8;
            let hi = (window[1] - 1) as u8;

            let signature: Vec<bool> = oracles
                .iter()
                .map(|oracle| oracle.iter().any(|&(s, e)| lo >= s && lo <= e))
                .collect();

            let class_id = match signature_to_class.get(&signature).copied() {
                Some(id) => id,
                None => {
                    debug_assert!(
                        num_classes < 256,
                        "byte class count overflowed u8 table value range"
                    );
                    let id = num_classes as u8;
                    signature_to_class.insert(signature, id);
                    num_classes += 1;
                    id
                }
            };

            for b in lo..=hi {
                table[b as usize] = class_id;
            }
        }

        Self { table, num_classes }
    }

    /// Returns the class ID assigned to the given byte.
    #[must_use]
    pub fn class_of(&self, byte: u8) -> u8 {
        self.table[byte as usize]
    }

    /// Returns the total number of distinct classes (1..=256).
    #[must_use]
    pub fn num_classes(&self) -> u16 {
        self.num_classes
    }
}

/// Walk the AST in pre-order and collect each construct's membership
/// oracle (a set of byte ranges) into the output list.
///
/// Each character class / literal / shorthand / `Dot` / property class /
/// `\R` / `\X` contributes exactly one oracle. Structural nodes (sequence,
/// alternation, quantified, group, flag group) descend into their children.
/// Zero-width assertions (anchors, word boundaries, `Empty`) contribute
/// nothing.
///
/// Non-supported nodes (lookaround, backref, recursion, code blocks, etc.)
/// are visited gracefully — the walker descends into any children where
/// they exist and contributes nothing for the node itself. The byte-class
/// map is only meaningful for `NoBacktracking`-classified patterns where
/// these nodes don't appear, but the walker doesn't crash if called on a
/// mixed AST.
fn collect_oracles(ast: &Regex, oracles: &mut Vec<Vec<(u8, u8)>>) {
    match ast {
        // ============================================================
        // Constructs that contribute one oracle each.
        // ============================================================
        Regex::Char(c) | Regex::WhitespaceLiteral(c) => {
            let mut set = Vec::new();
            push_char_range_as_byte_ranges(*c, *c, &mut set);
            oracles.push(set);
        }
        Regex::CharClass(cc) => {
            let mut set = Vec::new();
            collect_char_class_into(cc, &mut set);
            oracles.push(set);
        }
        Regex::Dot => {
            // Default `.` matches any byte except newline. With `(?s)`
            // it matches every byte. Adding both intervals around 0x0A
            // keeps the newline byte distinguishable in the partition,
            // which is conservative and correct under both modes.
            oracles.push(vec![(0x00, 0x09), (0x0B, 0xFF)]);
        }
        Regex::Digit { .. } => {
            oracles.push(vec![(b'0', b'9')]);
        }
        Regex::Word { .. } => {
            oracles.push(vec![(b'0', b'9'), (b'A', b'Z'), (b'_', b'_'), (b'a', b'z')]);
        }
        Regex::Space { .. } => {
            oracles.push(vec![(0x09, 0x0D), (b' ', b' ')]);
        }
        Regex::UnicodeClass { name, negated } => {
            // Resolve via the existing unicode_support bridge so we get
            // the same character ranges the VM uses for matching. Each
            // codepoint range becomes a series of UTF-8 byte ranges.
            //
            // For byte-class purposes, the positive and negative
            // resolutions of a property name produce the same set of
            // *interesting* byte boundaries (one is the complement of
            // the other within the universe), so we always pass the
            // pattern's `negated` flag through to keep the byte ranges
            // aligned with what the runtime will actually test against.
            if let Ok(char_ranges) =
                crate::unicode_support::resolve_unicode_property_class(name, *negated)
            {
                let mut set = Vec::new();
                for char_range in char_ranges {
                    push_char_range_as_byte_ranges(char_range.start, char_range.end, &mut set);
                }
                oracles.push(set);
            }
            // Invalid property names are caught by the compiler before
            // the byte class is built; nothing to contribute here.
        }
        Regex::NewlineSequence => {
            // \R matches the line-terminator set. ASCII-range bytes are
            // single-byte; the multi-byte ones get UTF-8-decomposed.
            let mut set = vec![(0x0A, 0x0A), (0x0B, 0x0B), (0x0C, 0x0C), (0x0D, 0x0D)];
            push_char_range_as_byte_ranges('\u{0085}', '\u{0085}', &mut set);
            push_char_range_as_byte_ranges('\u{2028}', '\u{2028}', &mut set);
            push_char_range_as_byte_ranges('\u{2029}', '\u{2029}', &mut set);
            oracles.push(set);
        }
        Regex::GraphemeCluster => {
            // \X matches any grapheme cluster, which can start with any
            // byte. Step 3 (NFA construction) handles the actual cluster
            // traversal; for byte-class purposes, every byte may
            // participate, so we contribute the universe as one oracle.
            oracles.push(vec![(0x00, 0xFF)]);
        }

        // ============================================================
        // Zero-width assertions — no byte contribution.
        // ============================================================
        Regex::Anchor(_) | Regex::WordBoundary { .. } | Regex::Empty | Regex::MatchReset => {}

        // ============================================================
        // Structural nodes — descend into children.
        // ============================================================
        Regex::Sequence(items) | Regex::Alternation(items) => {
            for item in items {
                collect_oracles(item, oracles);
            }
        }
        Regex::Quantified { expr, .. }
        | Regex::Group { expr, .. }
        | Regex::FlagGroup { expr, .. } => collect_oracles(expr, oracles),

        // ============================================================
        // Non-supported nodes (classifier flags as NeedsVm). The walker
        // descends gracefully where children exist and contributes
        // nothing for the node itself. Only relevant when called on a
        // mixed AST; the byte-class map is only used for NoBacktracking
        // patterns where these nodes don't appear.
        // ============================================================
        Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => {
            collect_oracles(expr, oracles);
        }
        Regex::Conditional {
            true_branch,
            false_branch,
            ..
        } => {
            collect_oracles(true_branch, oracles);
            if let Some(fb) = false_branch {
                collect_oracles(fb, oracles);
            }
        }
        Regex::Recursion { .. }
        | Regex::ReturnedCaptureSubroutine { .. }
        | Regex::Backreference(_)
        | Regex::NamedBackreference(_)
        | Regex::RelativeBackreference(_)
        | Regex::CodeBlock { .. }
        | Regex::Callout(_)
        | Regex::ExtendedCharClass { .. }
        | Regex::Accept
        | Regex::Commit
        | Regex::Prune
        | Regex::Skip(_)
        | Regex::Then
        | Regex::Mark(_) => {
            // No children to descend into.
        }
    }
}

/// Append a [`CharClass`]'s byte ranges to the given oracle set.
fn collect_char_class_into(cc: &CharClass, set: &mut Vec<(u8, u8)>) {
    match cc {
        CharClass::Digit { .. } => {
            set.push((b'0', b'9'));
        }
        CharClass::Word { .. } => {
            set.push((b'0', b'9'));
            set.push((b'A', b'Z'));
            set.push((b'_', b'_'));
            set.push((b'a', b'z'));
        }
        CharClass::Space { .. } => {
            set.push((0x09, 0x0D));
            set.push((b' ', b' '));
        }
        CharClass::UnicodeClass { name, negated } => {
            if let Ok(char_ranges) =
                crate::unicode_support::resolve_unicode_property_class(name, *negated)
            {
                for char_range in char_ranges {
                    push_char_range_as_byte_ranges(char_range.start, char_range.end, set);
                }
            }
        }
        CharClass::Custom {
            ranges: char_ranges,
            ..
        } => {
            for char_range in char_ranges {
                push_char_range_as_byte_ranges(char_range.start, char_range.end, set);
            }
        }
    }
}

/// Append UTF-8 byte-category boundary oracles to the oracle list.
///
/// The four UTF-8 byte categories that need to be distinguishable
/// in the byte-class partition are:
///   - 0x80-0xBF: continuation bytes (any multi-byte char)
///   - 0xC0-0xDF: 2-byte UTF-8 leading bytes
///   - 0xE0-0xEF: 3-byte UTF-8 leading bytes
///   - 0xF0-0xF7: 4-byte UTF-8 leading bytes
///
/// Each category becomes its own oracle so the partition algorithm
/// assigns each category a distinct equivalence class. ASCII bytes
/// (0x00-0x7F) and invalid bytes (0xF8-0xFF) share a "no UTF-8
/// category" signature; this is fine because invalid bytes can
/// never appear in valid UTF-8 input.
///
/// Without these oracles, a pattern like `[^0-9]` partitions the
/// byte alphabet into just two classes (digit / non-digit), which
/// causes the NFA's multi-byte chains for the negated range to
/// fire on ASCII bytes that aren't valid UTF-8 continuation /
/// leading bytes. See [`ByteClassMap::build_from_ast`] for the
/// full rationale and the bug history (the C1 step 6 differential
/// gate exposed this).
fn push_utf8_byte_boundary_oracles(oracles: &mut Vec<Vec<(u8, u8)>>) {
    oracles.push(vec![(0x80, 0xBF)]); // continuation
    oracles.push(vec![(0xC0, 0xDF)]); // 2-byte leading
    oracles.push(vec![(0xE0, 0xEF)]); // 3-byte leading
    oracles.push(vec![(0xF0, 0xF7)]); // 4-byte leading
}

/// Decompose a `[start, end]` codepoint range into per-position UTF-8
/// byte ranges and append them to `set`.
///
/// Uses [`regex_syntax::utf8::Utf8Sequences`] which produces, for each
/// codepoint sub-range with a uniform UTF-8 byte length, a sequence of
/// 1–4 byte ranges (one per byte position). Every byte range from every
/// position is appended to the same oracle, because the byte-class map
/// is position-independent — a single byte may appear at any position in
/// any UTF-8 sequence.
fn push_char_range_as_byte_ranges(start: char, end: char, set: &mut Vec<(u8, u8)>) {
    for utf8_seq in Utf8Sequences::new(start, end) {
        for byte_range in utf8_seq.as_slice() {
            set.push((byte_range.start, byte_range.end));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{CharRange, GroupKind, Quantifier};

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
            ci_override_ranges: None,
        })
    }

    fn negated_custom(ranges: Vec<(char, char)>) -> Regex {
        Regex::CharClass(CharClass::Custom {
            ranges: ranges
                .into_iter()
                .map(|(s, e)| CharRange::range(s, e))
                .collect(),
            negated: true,
            ci_override_ranges: None,
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

    fn group(kind: GroupKind, expr: Regex) -> Regex {
        Regex::Group {
            expr: Box::new(expr),
            kind,
            index: None,
            name: None,
        }
    }

    /// Build a `ByteClassMap` from a list of `(start, end)` ranges treated
    /// as a single oracle. Convenience for testing the partition directly.
    fn map_from_single_oracle(ranges: Vec<(u8, u8)>) -> ByteClassMap {
        ByteClassMap::from_oracles(&[ranges])
    }

    // ============================================================
    // Empty / trivial inputs
    // ============================================================

    #[test]
    fn empty_ast_yields_utf8_category_classes() {
        // Even with no pattern oracles, the partition is forced to
        // distinguish UTF-8 byte categories (continuation, 2/3/4-byte
        // leading) from ASCII bytes by `push_utf8_byte_boundary_oracles`.
        // See `ByteClassMap::build_from_ast` for the rationale (the C1
        // step 6 negated-char-class fix). Five categories: ASCII (incl.
        // invalid bytes 0xF8-0xFF), continuation, 2-byte leading,
        // 3-byte leading, 4-byte leading.
        let map = ByteClassMap::build_from_ast(&Regex::Empty);
        assert_eq!(map.num_classes(), 5);
        // ASCII bytes share class 0 (and so do invalid bytes 0xF8-0xFF).
        let ascii_class = map.class_of(0x00);
        assert_eq!(map.class_of(0x7F), ascii_class);
        assert_eq!(map.class_of(0xFF), ascii_class);
        // The four UTF-8 categories each get their own distinct class.
        assert_ne!(map.class_of(0x80), ascii_class);
        assert_ne!(map.class_of(0xC0), ascii_class);
        assert_ne!(map.class_of(0xE0), ascii_class);
        assert_ne!(map.class_of(0xF0), ascii_class);
    }

    #[test]
    fn anchor_only_pattern_yields_utf8_category_classes() {
        // ^$ — both anchors are zero-width, contribute no byte ranges.
        // Same five UTF-8-category-driven classes as the empty AST.
        let ast = seq(vec![
            Regex::Anchor(crate::ast::AnchorType::Start),
            Regex::Anchor(crate::ast::AnchorType::End),
        ]);
        let map = ByteClassMap::build_from_ast(&ast);
        assert_eq!(map.num_classes(), 5);
    }

    #[test]
    fn from_oracles_with_empty_input_yields_one_class() {
        let map = ByteClassMap::from_oracles(&[]);
        assert_eq!(map.num_classes(), 1);
        for b in 0..=255u8 {
            assert_eq!(map.class_of(b), 0);
        }
    }

    // ============================================================
    // Single ASCII literal
    // ============================================================

    #[test]
    fn single_ascii_literal_yields_six_classes() {
        // 'a' adds one oracle (0x61, 0x61). Combined with the four
        // UTF-8 byte-category oracles, the partition has 6 classes:
        //   class 0: ASCII non-'a' + invalid bytes (0xF8-0xFF)
        //   class 1: 'a' (0x61)
        //   class 2: continuation bytes (0x80-0xBF)
        //   class 3: 2-byte leading (0xC0-0xDF)
        //   class 4: 3-byte leading (0xE0-0xEF)
        //   class 5: 4-byte leading (0xF0-0xF7)
        // (Class IDs are assigned by first-encounter order so the
        // exact numbering may differ; the count is what matters.)
        let map = ByteClassMap::build_from_ast(&lit('a'));
        assert_eq!(map.num_classes(), 6);
        let class_a = map.class_of(b'a');
        // 'a' is in its own class — distinct from every other byte.
        for b in 0..=255u8 {
            if b == b'a' {
                assert_eq!(map.class_of(b), class_a);
            } else {
                assert_ne!(map.class_of(b), class_a);
            }
        }
    }

    #[test]
    fn single_byte_oracle_partitions_into_two_classes() {
        // {a} only — bytes 'a' and "everything else".
        let map = map_from_single_oracle(vec![(b'a', b'a')]);
        assert_eq!(map.num_classes(), 2);
        assert_ne!(map.class_of(b'a'), map.class_of(b'b'));
        assert_eq!(map.class_of(b'\0'), map.class_of(b'b'));
        assert_eq!(map.class_of(0xFF), map.class_of(b'b'));
    }

    // ============================================================
    // Custom character classes — bytes within one class are equivalent
    // ============================================================

    #[test]
    fn class_abc_groups_a_b_c_into_one_class() {
        // [abc] — bytes 'a', 'b', 'c' are all in the same class because
        // they appear identically in the only pattern oracle. Combined
        // with the four UTF-8 byte-category oracles the total is 6
        // classes: ASCII non-{a,b,c}, {a,b,c}, continuation, 2/3/4-byte
        // leading.
        let map = ByteClassMap::build_from_ast(&custom(vec![('a', 'a'), ('b', 'b'), ('c', 'c')]));
        assert_eq!(map.num_classes(), 6);
        let abc_class = map.class_of(b'a');
        assert_eq!(map.class_of(b'b'), abc_class);
        assert_eq!(map.class_of(b'c'), abc_class);
        assert_ne!(map.class_of(b'd'), abc_class);
        assert_ne!(map.class_of(b'@'), abc_class);
    }

    #[test]
    fn class_a_to_z_groups_all_lowercase_into_one_class() {
        // [a-z] — same shape as [abc], 6 classes total: ASCII non-letter,
        // 'a-z', continuation, 2/3/4-byte leading.
        let map = ByteClassMap::build_from_ast(&custom(vec![('a', 'z')]));
        assert_eq!(map.num_classes(), 6);
        let lower_class = map.class_of(b'a');
        for b in b'a'..=b'z' {
            assert_eq!(map.class_of(b), lower_class);
        }
        assert_ne!(map.class_of(b'A'), lower_class);
        assert_ne!(map.class_of(b'0'), lower_class);
    }

    #[test]
    fn negated_char_class_yields_same_partition_as_positive() {
        // [^a-z] and [a-z] partition the bytes the same way — only the
        // membership semantics differ at runtime, not the byte classes.
        let positive = ByteClassMap::build_from_ast(&custom(vec![('a', 'z')]));
        let negative = ByteClassMap::build_from_ast(&negated_custom(vec![('a', 'z')]));
        assert_eq!(positive, negative);
    }

    #[test]
    fn two_disjoint_classes_partition_into_seven_classes() {
        // [a-c][d-f] — alternation creates two pattern oracles. Combined
        // with the four UTF-8 byte-category oracles, the partition has
        // 7 classes: ASCII non-pattern, {a-c}, {d-f}, continuation,
        // 2-byte leading, 3-byte leading, 4-byte leading.
        let ast = seq(vec![custom(vec![('a', 'c')]), custom(vec![('d', 'f')])]);
        let map = ByteClassMap::build_from_ast(&ast);
        assert_eq!(map.num_classes(), 7);
        let abc = map.class_of(b'a');
        assert_eq!(map.class_of(b'b'), abc);
        assert_eq!(map.class_of(b'c'), abc);
        let def = map.class_of(b'd');
        assert_eq!(map.class_of(b'e'), def);
        assert_eq!(map.class_of(b'f'), def);
        assert_ne!(abc, def);
        assert_ne!(map.class_of(b'g'), abc);
        assert_ne!(map.class_of(b'g'), def);
    }

    #[test]
    fn two_overlapping_classes_distinguish_overlap_from_unique_parts() {
        // [a-c][b-d] — bytes 'a', 'b', 'c', 'd' must all be distinguished:
        //   'a' is only in oracle 0
        //   'b' and 'c' are in BOTH oracles (so they share a class)
        //   'd' is only in oracle 1
        // Combined with the four UTF-8 byte-category oracles, the
        // partition has 8 classes: ASCII non-pattern, 'a', {'b','c'},
        // 'd', continuation, 2/3/4-byte leading.
        let ast = seq(vec![custom(vec![('a', 'c')]), custom(vec![('b', 'd')])]);
        let map = ByteClassMap::build_from_ast(&ast);
        assert_eq!(map.num_classes(), 8);
        // 'b' and 'c' have identical membership and must share a class.
        assert_eq!(map.class_of(b'b'), map.class_of(b'c'));
        // 'a' and 'd' must each have their own class distinct from 'b'/'c'.
        assert_ne!(map.class_of(b'a'), map.class_of(b'b'));
        assert_ne!(map.class_of(b'd'), map.class_of(b'b'));
        assert_ne!(map.class_of(b'a'), map.class_of(b'd'));
    }

    // ============================================================
    // Shorthand classes
    // ============================================================

    #[test]
    fn digit_class_distinguishes_digits_from_others() {
        // \d — single pattern oracle (0x30, 0x39). With the four UTF-8
        // byte-category oracles the total is 6 classes: ASCII
        // non-digit, digit, continuation, 2/3/4-byte leading.
        let map = ByteClassMap::build_from_ast(&Regex::Digit { negated: false });
        assert_eq!(map.num_classes(), 6);
        let digit_class = map.class_of(b'0');
        for b in b'0'..=b'9' {
            assert_eq!(map.class_of(b), digit_class);
        }
        assert_ne!(map.class_of(b'a'), digit_class);
    }

    #[test]
    fn word_class_keeps_word_components_in_one_class() {
        let map = ByteClassMap::build_from_ast(&Regex::Word { negated: false });
        // The Word oracle is the union of (0-9) (A-Z) (_) (a-z), but since
        // they're all in one oracle, every byte in any of those ranges
        // shares the same membership signature → same class.
        let word_class = map.class_of(b'a');
        for b in b'a'..=b'z' {
            assert_eq!(map.class_of(b), word_class);
        }
        for b in b'A'..=b'Z' {
            assert_eq!(map.class_of(b), word_class);
        }
        for b in b'0'..=b'9' {
            assert_eq!(map.class_of(b), word_class);
        }
        assert_eq!(map.class_of(b'_'), word_class);
        // A non-word byte must be in a different class.
        assert_ne!(map.class_of(b' '), word_class);
        assert_ne!(map.class_of(b'#'), word_class);
    }

    #[test]
    fn space_class_groups_ascii_whitespace() {
        let map = ByteClassMap::build_from_ast(&Regex::Space { negated: false });
        let space_class = map.class_of(b' ');
        for b in 0x09..=0x0D {
            assert_eq!(map.class_of(b), space_class);
        }
        assert_eq!(map.class_of(b' '), space_class);
        assert_ne!(map.class_of(b'a'), space_class);
    }

    // ============================================================
    // Dot
    // ============================================================

    #[test]
    fn dot_distinguishes_newline_from_other_bytes() {
        let map = ByteClassMap::build_from_ast(&Regex::Dot);
        // Dot's oracle is `[0x00..=0x09] ∪ [0x0B..=0xFF]`. Byte 0x0A
        // is outside the dot oracle. Combined with the four UTF-8
        // byte-category oracles the partition has 6 classes:
        //   class 0: ASCII non-newline (0x00-0x09, 0x0B-0x7F) + invalid (0xF8-0xFF)
        //   class 1: newline (0x0A)
        //   class 2: continuation (0x80-0xBF)
        //   class 3: 2-byte leading (0xC0-0xDF)
        //   class 4: 3-byte leading (0xE0-0xEF)
        //   class 5: 4-byte leading (0xF0-0xF7)
        assert_eq!(map.num_classes(), 6);
        // Newline is in its own class, distinct from other ASCII.
        assert_ne!(map.class_of(0x0A), map.class_of(0x09));
        assert_eq!(map.class_of(0x09), map.class_of(0x0B));
        // 0xFF (invalid byte) shares the ASCII non-newline class
        // because both have signature (1, 0, 0, 0, 0).
        assert_eq!(map.class_of(0x09), map.class_of(0xFF));
    }

    // ============================================================
    // Multi-byte UTF-8 contents
    // ============================================================

    #[test]
    fn non_ascii_literal_decomposes_into_byte_ranges() {
        // 'α' = U+03B1 = 0xCE 0xB1 in UTF-8.
        let map = ByteClassMap::build_from_ast(&lit('α'));
        // Bytes 0xCE and 0xB1 should be distinguished from each other and
        // from "everything else" because they participate in the UTF-8
        // sequence at different positions.
        assert!(map.num_classes() >= 2);
        // 0xCE is in the oracle (as the leading byte) → some class.
        // 0xB1 is in the oracle (as the trailing byte) → some class.
        // 0x00 is in neither → "everything else" class.
        // Check that bytes 0xCE and 0xB1 are in distinct ranges from
        // unrelated bytes.
        let other = map.class_of(0x00);
        assert_ne!(map.class_of(0xCE), other);
        assert_ne!(map.class_of(0xB1), other);
    }

    #[test]
    fn non_ascii_char_range_partition_distinguishes_relevant_bytes() {
        // [α-ω] = U+03B1 .. U+03C9. The UTF-8 leading byte is 0xCE for
        // U+0380..U+03BF and 0xCF for U+03C0..U+03FF, so the leading byte
        // is in {0xCE, 0xCF}. The trailing bytes form ranges in 0x80..0xBF.
        let map = ByteClassMap::build_from_ast(&custom(vec![('α', 'ω')]));
        let other = map.class_of(0x00);
        assert_ne!(map.class_of(0xCE), other);
        assert_ne!(map.class_of(0xCF), other);
    }

    // ============================================================
    // Nested structural nodes
    // ============================================================

    #[test]
    fn quantified_node_descends_into_inner_expression() {
        // a* — the partition is the same as a single 'a' literal:
        // 6 classes (ASCII non-'a', 'a', plus four UTF-8 byte categories).
        let q = quantified(lit('a'), Quantifier::ZeroOrMore { lazy: false });
        let map = ByteClassMap::build_from_ast(&q);
        assert_eq!(map.num_classes(), 6);
        assert_ne!(map.class_of(b'a'), map.class_of(b'b'));
    }

    #[test]
    fn capturing_group_descends_into_inner_expression() {
        let g = group(GroupKind::Capturing, custom(vec![('a', 'z')]));
        let map = ByteClassMap::build_from_ast(&g);
        let lower_class = map.class_of(b'a');
        for b in b'a'..=b'z' {
            assert_eq!(map.class_of(b), lower_class);
        }
    }

    #[test]
    fn alternation_combines_oracles_from_each_branch() {
        // a|b|c — three oracles, each contributing one byte. Bytes 'a',
        // 'b', 'c' must each be in their own class.
        let ast = alt(vec![lit('a'), lit('b'), lit('c')]);
        let map = ByteClassMap::build_from_ast(&ast);
        // 'a', 'b', 'c' must each be in distinct classes.
        let a = map.class_of(b'a');
        let b = map.class_of(b'b');
        let c = map.class_of(b'c');
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        // Bytes outside all three oracles share one class.
        assert_eq!(map.class_of(b'x'), map.class_of(b'y'));
    }

    #[test]
    fn realistic_log_pattern_partitions_into_expected_set() {
        // (\d{4}-\d{2}-\d{2})\s+(ERROR|WARN) — every digit byte should
        // share the digit class, every word byte should share, etc.
        let ast = seq(vec![
            group(
                GroupKind::Capturing,
                seq(vec![
                    quantified(
                        Regex::Digit { negated: false },
                        Quantifier::Range {
                            min: 4,
                            max: Some(4),
                            lazy: false,
                        },
                    ),
                    lit('-'),
                    quantified(
                        Regex::Digit { negated: false },
                        Quantifier::Range {
                            min: 2,
                            max: Some(2),
                            lazy: false,
                        },
                    ),
                    lit('-'),
                    quantified(
                        Regex::Digit { negated: false },
                        Quantifier::Range {
                            min: 2,
                            max: Some(2),
                            lazy: false,
                        },
                    ),
                ]),
            ),
            quantified(
                Regex::Space { negated: false },
                Quantifier::OneOrMore { lazy: false },
            ),
            group(
                GroupKind::Capturing,
                alt(vec![
                    seq(vec![lit('E'), lit('R'), lit('R'), lit('O'), lit('R')]),
                    seq(vec![lit('W'), lit('A'), lit('R'), lit('N')]),
                ]),
            ),
        ]);
        let map = ByteClassMap::build_from_ast(&ast);
        // The structure is too complex to predict the exact class count,
        // but every digit byte must share the same class with the others
        // because they only appear via the Digit oracle.
        let digit_class = map.class_of(b'0');
        for b in b'1'..=b'9' {
            assert_eq!(
                map.class_of(b),
                digit_class,
                "digit byte 0x{b:02X} ({}) must share digit class",
                b as char
            );
        }
        // 'R' is the only character that appears in BOTH ERROR and WARN.
        // 'E', 'O', 'W', 'A', 'N' each appear in exactly one of the two
        // alternatives. So 'R' has signature (true, true) while 'E', 'O',
        // 'W', 'A', 'N' have differing signatures. Verify 'R' is distinct
        // from at least one other letter.
        assert_ne!(map.class_of(b'R'), map.class_of(b'E'));
    }

    // ============================================================
    // Boundary correctness
    // ============================================================

    #[test]
    fn class_ids_are_dense_and_start_at_zero() {
        // Build a few maps and verify that class IDs in the table never
        // exceed `num_classes - 1`.
        let inputs: Vec<Regex> = vec![
            Regex::Empty,
            lit('a'),
            custom(vec![('a', 'z')]),
            seq(vec![custom(vec![('a', 'c')]), custom(vec![('d', 'f')])]),
            Regex::Word { negated: false },
        ];
        for ast in inputs {
            let map = ByteClassMap::build_from_ast(&ast);
            let max_id = (map.num_classes() - 1) as u8;
            for b in 0..=255u8 {
                assert!(
                    map.class_of(b) <= max_id,
                    "class id {} > max {} for byte 0x{b:02X}",
                    map.class_of(b),
                    max_id
                );
            }
        }
    }

    #[test]
    fn duplicate_byte_ranges_in_oracles_do_not_change_partition() {
        let single = ByteClassMap::from_oracles(&[vec![(b'a', b'z')]]);
        let duplicated =
            ByteClassMap::from_oracles(&[vec![(b'a', b'z'), (b'a', b'z'), (b'a', b'z')]]);
        assert_eq!(single, duplicated);
    }

    #[test]
    fn full_universe_oracle_yields_one_class() {
        let map = map_from_single_oracle(vec![(0x00, 0xFF)]);
        assert_eq!(map.num_classes(), 1);
    }

    #[test]
    fn adjacent_ranges_in_one_oracle_are_treated_as_one_set() {
        // Oracle = {a..c, d..f}. Bytes 'a'..'f' should all be in the same
        // class (one membership signature: "in oracle").
        let map = map_from_single_oracle(vec![(b'a', b'c'), (b'd', b'f')]);
        assert_eq!(map.num_classes(), 2);
        let in_class = map.class_of(b'a');
        for b in b'a'..=b'f' {
            assert_eq!(map.class_of(b), in_class);
        }
        assert_ne!(map.class_of(b'g'), in_class);
    }

    #[test]
    fn boundary_byte_zero_and_byte_ff_are_handled_correctly() {
        // Single-oracle range that touches the universe boundaries.
        let map = map_from_single_oracle(vec![(0x00, 0x00)]);
        assert_eq!(map.num_classes(), 2);
        assert_ne!(map.class_of(0x00), map.class_of(0x01));

        let map = map_from_single_oracle(vec![(0xFF, 0xFF)]);
        assert_eq!(map.num_classes(), 2);
        assert_ne!(map.class_of(0xFF), map.class_of(0xFE));
    }
}
