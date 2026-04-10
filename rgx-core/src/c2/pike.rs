//! Sparse-set Pike-VM for the C2 NFA/DFA hybrid engine.
//!
//! Implements Russ Cox's Pike-VM with the Briggs–Torczon sparse-set
//! state container. The Pike-VM is the **permanent** NFA simulator for
//! C2: it serves three roles across the phased plan in
//! `docs/C2_NFA_DFA_DESIGN.md` §15.
//!
//! 1. **The first runnable C2 engine** (this commit, step 4a). Before
//!    the lazy DFA caches land in steps 5–6, the Pike-VM alone handles
//!    `is_match` / `find_first` / `find_all` for the no-backtracking
//!    subset. It is **not** a prototype that gets replaced — it ships
//!    production-quality and stays in production.
//! 2. **The DFA cache fallback** when the lazy DFA's state cache fills
//!    up and starts thrashing. Pike-VM is O(nm) so this is a graceful
//!    degradation, not a cliff. Lands in steps 5–6 alongside the DFA.
//! 3. **The bounded capture recovery pass** (design doc §9). Once the
//!    DFA finds a match span, the Pike-VM runs over only the matched
//!    span to recover capture group positions. Lands in step 4b.
//!
//! # Algorithm
//!
//! The classical Pike-VM with sparse-set state tracking. At each input
//! byte, the simulator maintains a **current** set of NFA states that
//! represent threads of execution. For each state, it follows the byte
//! transition for the current byte (if any) into a **next** set, then
//! swaps the two sets and advances. Epsilon edges are followed
//! immediately during state insertion (epsilon closure expansion) so
//! the active sets only contain "byte-consuming" states.
//!
//! The sparse-set design gives O(1) `add`, `contains`, and `clear` with
//! no hashing or bitmap scanning. Two arrays of size `num_states`:
//!
//! - `sparse[state] = i` records that this state is in the dense array
//!   at position `i` — but only valid if `dense[i] == state`.
//! - `dense` lists active states in insertion order.
//! - `len` is the count of active states.
//!
//! `clear` just resets `len = 0`; no memory wipe is needed because the
//! validity check uses both arrays. See Briggs and Torczon (1993),
//! "An efficient representation for sparse sets".
//!
//! # Step 4a scope
//!
//! This commit implements `pike_is_match`, `pike_find_first`, and
//! `pike_find_all` **without capture tracking**. The differential test
//! corpus in `tests/c2_pike_differential.rs` compares match SPANS
//! (start/end byte positions) against the existing backtracking VM.
//!
//! Capture tracking and engine dispatch wiring land in step 4b. The
//! lazy DFA caches that the Pike-VM will fall back from land in steps
//! 5–6.
//!
//! # Zero-width assertions
//!
//! `^`, `$`, `\A`, `\Z`, `\z`, `\b`, `\B`, and `\G` are checked during
//! epsilon closure expansion. The closure walker reads the assertion on
//! the epsilon edge, evaluates it against the current input position,
//! and follows the edge only if the assertion holds.
//!
//! # References
//!
//! - `docs/C2_NFA_DFA_DESIGN.md` §7 — design rationale and rules
//! - Russ Cox, "Regular Expression Matching: the Virtual Machine
//!   Approach" — the canonical Pike-VM article
//! - Briggs and Torczon, "An efficient representation for sparse sets"
//!   (1993) — the sparse-set data structure
//! - The Rust `regex-automata` crate's `pikevm` module

use crate::c2::byte_class::ByteClassMap;
use crate::c2::nfa::{Nfa, NfaStateId, ZeroWidthAssertion};
use crate::c2::program::CompiledC2Program;

// ============================================================
// Sparse set
// ============================================================

/// Briggs–Torczon sparse set of NFA state IDs.
///
/// O(1) insertion, lookup, and clear. Used by the Pike-VM as the active
/// thread set. Pre-allocates `sparse` and `dense` arrays sized to
/// `num_states` so there are no allocations during a match scan.
#[derive(Debug)]
struct SparseSet {
    /// `sparse[state] = position in dense` — valid only if
    /// `dense[sparse[state]] == state` and `sparse[state] < len`.
    sparse: Vec<u32>,
    /// Active state IDs in insertion order.
    dense: Vec<NfaStateId>,
    /// Number of active states (also the length of the meaningful prefix
    /// of `dense`).
    len: usize,
}

impl SparseSet {
    fn with_capacity(num_states: usize) -> Self {
        Self {
            sparse: vec![0u32; num_states],
            dense: vec![0u32; num_states],
            len: 0,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    fn contains(&self, state: NfaStateId) -> bool {
        let i = self.sparse[state as usize] as usize;
        i < self.len && self.dense[i] == state
    }

    fn insert(&mut self, state: NfaStateId) {
        if self.contains(state) {
            return;
        }
        self.sparse[state as usize] = self.len as u32;
        self.dense[self.len] = state;
        self.len += 1;
    }

    fn iter(&self) -> impl Iterator<Item = NfaStateId> + '_ {
        self.dense[..self.len].iter().copied()
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Position of `state` in the dense array, or `None` if absent.
    /// Lower positions have higher priority because epsilon closure
    /// expands edges in priority order.
    fn position_of(&self, state: NfaStateId) -> Option<usize> {
        if self.contains(state) {
            Some(self.sparse[state as usize] as usize)
        } else {
            None
        }
    }

    /// State at the given dense position. Caller must ensure
    /// `dense_pos < self.len`.
    fn state_at(&self, dense_pos: usize) -> NfaStateId {
        debug_assert!(dense_pos < self.len);
        self.dense[dense_pos]
    }
}

// ============================================================
// Zero-width assertions
// ============================================================

/// Returns `true` iff the byte at offset `pos` (or just before it for
/// look-back assertions) is a "word" character per ASCII semantics.
/// Word characters are `[A-Za-z0-9_]`.
///
/// Step 4a uses ASCII-only word semantics. Unicode word boundaries are
/// a follow-up (Q2 in `docs/C2_NFA_DFA_DESIGN.md` §16).
#[inline]
fn is_word_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

/// Evaluate a [`ZeroWidthAssertion`] at the given byte position in the
/// input. Returns `true` iff the assertion holds.
fn check_assertion(assertion: ZeroWidthAssertion, input: &[u8], pos: usize) -> bool {
    match assertion {
        ZeroWidthAssertion::StartOfText => pos == 0,
        ZeroWidthAssertion::EndOfText => pos == input.len(),
        ZeroWidthAssertion::EndOfTextOrFinalNewline => {
            pos == input.len() || (pos + 1 == input.len() && input[pos] == b'\n')
        }
        ZeroWidthAssertion::StartOfLine => pos == 0 || input[pos - 1] == b'\n',
        ZeroWidthAssertion::EndOfLine => pos == input.len() || input[pos] == b'\n',
        // \G — at step 4a there's no notion of "previous match end",
        // so this is true only at position 0 (matching the start-of-input
        // semantics for the first call). The find_all loop will need to
        // thread the previous end through to handle subsequent matches
        // correctly; deferred to step 4b.
        ZeroWidthAssertion::PreviousMatchEnd => pos == 0,
        ZeroWidthAssertion::WordBoundary => {
            let before = pos > 0 && is_word_byte(input[pos - 1]);
            let after = pos < input.len() && is_word_byte(input[pos]);
            before != after
        }
        ZeroWidthAssertion::NotWordBoundary => {
            let before = pos > 0 && is_word_byte(input[pos - 1]);
            let after = pos < input.len() && is_word_byte(input[pos]);
            before == after
        }
    }
}

// ============================================================
// Epsilon closure
// ============================================================

/// Add `state` and every state reachable from it via epsilon edges
/// (with satisfied assertions) to `set`. Recursively expands the
/// closure in priority order; the sparse-set's first-insertion-wins
/// rule encodes leftmost-first semantics.
///
/// `pos` is the input byte offset for the assertion checks. The same
/// closure is invoked at each input position during the main scan.
fn epsilon_closure(set: &mut SparseSet, nfa: &Nfa, state: NfaStateId, pos: usize, input: &[u8]) {
    if set.contains(state) {
        return;
    }
    set.insert(state);

    let state_obj = &nfa.states()[state as usize];

    // Epsilon edges are emitted in priority order during NFA construction
    // for greedy/lazy quantifiers and alternation, so iterating in slot
    // order preserves leftmost-first semantics. (Within a single state
    // the priorities are 0, 1, 2, ... in slot order.)
    for edge in &state_obj.epsilons {
        if let Some(assertion) = edge.assertion {
            if !check_assertion(assertion, input, pos) {
                continue;
            }
        }
        // Capture tags are ignored at step 4a — capture tracking lands
        // in step 4b. The bounded recovery pass (design doc §9) will
        // re-run the simulator over the matched span with capture
        // tracking enabled.
        epsilon_closure(set, nfa, edge.target, pos, input);
    }
}

// ============================================================
// Core simulation
// ============================================================

/// Run the Pike-VM over `input` starting from byte offset `start`,
/// using `nfa` (the anchored NFA) and `byte_class_map`. Returns the END
/// position of the longest match found, or `None` if no match exists at
/// `start`.
///
/// "Longest" here means the latest position at which the simulator
/// reached the accept state. Greedy quantifiers are encoded in the NFA
/// epsilon priorities, and the sparse-set first-insertion-wins rule
/// gives leftmost-first semantics, but for the **end position** of a
/// match the simulator continues extending threads as long as any are
/// alive — so the returned end is the latest accept position seen.
fn pike_match_at(
    nfa: &Nfa,
    byte_class_map: &ByteClassMap,
    input: &[u8],
    start: usize,
) -> Option<usize> {
    let num_states = nfa.num_states();
    let mut current = SparseSet::with_capacity(num_states);
    let mut next = SparseSet::with_capacity(num_states);
    let mut matched: Option<usize> = None;

    epsilon_closure(&mut current, nfa, nfa.start(), start, input);

    let mut pos = start;
    loop {
        // Where is the accept state in the current set, if at all?
        // Lower dense positions have higher priority because epsilon
        // closure expands edges in priority order.
        let accept_priority = current.position_of(nfa.accept());

        if accept_priority.is_some() {
            matched = Some(pos);
        }

        if pos >= input.len() || current.is_empty() {
            break;
        }

        let byte = input[pos];
        let cls = byte_class_map.class_of(byte);
        next.clear();

        // Only extend threads with priority >= accept's priority (i.e.,
        // dense position <= accept's dense position). Threads at higher
        // dense positions were added during epsilon closure AFTER the
        // accept edge was followed, so they have strictly lower priority
        // and cannot produce a leftmost-first-winning match. Killing
        // them at this point is what gives lazy quantifiers their
        // shortest-match semantics. For greedy patterns, accept is added
        // last during closure (the loop edge has higher priority than the
        // exit edge), so the limit equals current.len and all threads
        // are extended — which gives the longest-match behaviour.
        let limit = match accept_priority {
            Some(p) => p + 1,
            None => current.len,
        };

        for i in 0..limit {
            let state_id = current.state_at(i);
            let state_obj = &nfa.states()[state_id as usize];
            for &(transition_cls, target) in &state_obj.transitions {
                if transition_cls == cls {
                    epsilon_closure(&mut next, nfa, target, pos + 1, input);
                }
            }
        }

        std::mem::swap(&mut current, &mut next);
        pos += 1;
    }

    matched
}

// ============================================================
// Public API
// ============================================================

/// Returns `true` iff the pattern matches anywhere in `input`.
///
/// Uses the **forward unanchored** NFA from the compiled program — a
/// single scan suffices because the unanchored NFA's lazy `(?s:.)*?`
/// prefix lets the simulator start matching at any position during the
/// same scan.
#[must_use]
pub fn pike_is_match(program: &CompiledC2Program, input: &[u8]) -> bool {
    let nfa = &program.forward_unanchored;
    let bcm = &program.byte_class_map;
    pike_match_at(nfa, bcm, input, 0).is_some()
}

/// Returns the leftmost match in `input` as `(start, end)` byte
/// positions, or `None` if there is no match.
///
/// Uses the **forward anchored** NFA: the simulator runs at every scan
/// position from 0 to `input.len()` and returns the first position
/// where the anchored NFA reaches its accept state. Within a winning
/// scan position, the end is the latest accept position the simulator
/// reached, which gives greedy/longest semantics for the captured span.
#[must_use]
pub fn pike_find_first(program: &CompiledC2Program, input: &[u8]) -> Option<(usize, usize)> {
    let nfa = &program.forward_anchored;
    let bcm = &program.byte_class_map;
    for start in 0..=input.len() {
        if let Some(end) = pike_match_at(nfa, bcm, input, start) {
            return Some((start, end));
        }
    }
    None
}

/// Returns all non-overlapping matches in `input` as `(start, end)`
/// byte positions in left-to-right order.
///
/// Uses the same anchored-scan strategy as [`pike_find_first`]. The
/// advance rule matches the existing backtracking VM's `find_all`
/// behaviour:
///
/// - After a non-empty match `(s, e)`, the next scan starts at `e`.
/// - After an empty match `(s, s)`, the next scan starts at `s + 1` to
///   avoid an infinite loop on patterns like `a*` that match the empty
///   string at every position.
/// - **Empty matches immediately adjacent to a previous non-empty match
///   are dropped.** For example, `a*` on `"aaab"` returns
///   `[(0, 3), (4, 4)]` — the empty match at position 3 is skipped
///   because it sits right at the end of the non-empty match `(0, 3)`,
///   but the empty match at position 4 (after `b`) is kept. This
///   matches the convention used by the existing backtracking VM and
///   the Rust `regex` crate.
#[must_use]
pub fn pike_find_all(program: &CompiledC2Program, input: &[u8]) -> Vec<(usize, usize)> {
    let nfa = &program.forward_anchored;
    let bcm = &program.byte_class_map;
    let mut results = Vec::new();
    let mut start = 0usize;
    let mut prev_non_empty_end: Option<usize> = None;
    while start <= input.len() {
        let Some(end) = pike_match_at(nfa, bcm, input, start) else {
            start += 1;
            continue;
        };
        let is_empty = end == start;
        if is_empty && Some(start) == prev_non_empty_end {
            // Skip an empty match that sits at the same position as the
            // end of the previous non-empty match.
            start += 1;
            continue;
        }
        results.push((start, end));
        prev_non_empty_end = if is_empty { None } else { Some(end) };
        start = if is_empty { start + 1 } else { end };
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(pattern: &str) -> CompiledC2Program {
        CompiledC2Program::try_compile(pattern)
            .unwrap_or_else(|| panic!("pattern '{pattern}' is not in the C2 subset"))
    }

    // ============================================================
    // SparseSet
    // ============================================================

    #[test]
    fn sparse_set_basic_ops() {
        let mut s = SparseSet::with_capacity(8);
        assert!(s.is_empty());
        assert!(!s.contains(0));
        s.insert(3);
        s.insert(5);
        s.insert(3); // duplicate, should be a no-op
        assert!(s.contains(3));
        assert!(s.contains(5));
        assert!(!s.contains(0));
        assert!(!s.contains(4));
        let collected: Vec<u32> = s.iter().collect();
        assert_eq!(collected, vec![3, 5]);
        s.clear();
        assert!(s.is_empty());
        assert!(!s.contains(3));
    }

    // ============================================================
    // Literal patterns
    // ============================================================

    #[test]
    fn literal_match_at_start() {
        let prog = compile("hello");
        assert_eq!(pike_find_first(&prog, b"hello"), Some((0, 5)));
        assert!(pike_is_match(&prog, b"hello"));
    }

    #[test]
    fn literal_match_in_middle() {
        let prog = compile("foo");
        assert_eq!(pike_find_first(&prog, b"barfooz"), Some((3, 6)));
    }

    #[test]
    fn literal_no_match() {
        let prog = compile("xyz");
        assert_eq!(pike_find_first(&prog, b"abc"), None);
        assert!(!pike_is_match(&prog, b"abc"));
    }

    // ============================================================
    // Character classes
    // ============================================================

    #[test]
    fn ascii_char_class_matches_first_in_range() {
        let prog = compile("[a-z]");
        assert_eq!(pike_find_first(&prog, b"123abc"), Some((3, 4)));
    }

    #[test]
    fn shorthand_digit_matches_first_digit() {
        let prog = compile(r"\d");
        assert_eq!(pike_find_first(&prog, b"abc7xy"), Some((3, 4)));
    }

    #[test]
    fn shorthand_word_matches_first_word_char() {
        let prog = compile(r"\w");
        assert_eq!(pike_find_first(&prog, b"!?@_"), Some((3, 4)));
    }

    #[test]
    fn negated_class_matches_first_non_digit() {
        let prog = compile(r"[^0-9]");
        assert_eq!(pike_find_first(&prog, b"123x"), Some((3, 4)));
    }

    // ============================================================
    // Sequence and alternation
    // ============================================================

    #[test]
    fn sequence_matches_three_chars() {
        let prog = compile("abc");
        assert_eq!(pike_find_first(&prog, b"xxabcyy"), Some((2, 5)));
    }

    #[test]
    fn alternation_matches_first_branch_at_position() {
        let prog = compile("cat|dog|fish");
        assert_eq!(pike_find_first(&prog, b"i love dogs"), Some((7, 10)));
    }

    #[test]
    fn alternation_no_branch_matches() {
        let prog = compile("cat|dog");
        assert_eq!(pike_find_first(&prog, b"hello"), None);
    }

    // ============================================================
    // Quantifiers
    // ============================================================

    #[test]
    fn greedy_star_matches_longest_run() {
        let prog = compile("a*");
        // Greedy: at position 0 the longest match is "" since position 0
        // has 'b', wait - 'b' at pos 0 means a* matches empty there.
        // Actually `a*` always matches empty at any position. find_first
        // returns the first match, which is empty at position 0.
        assert_eq!(pike_find_first(&prog, b"baaab"), Some((0, 0)));
    }

    #[test]
    fn greedy_plus_requires_at_least_one() {
        let prog = compile("a+");
        assert_eq!(pike_find_first(&prog, b"baaab"), Some((1, 4)));
    }

    #[test]
    fn lazy_plus_matches_minimum() {
        let prog = compile("a+?");
        assert_eq!(pike_find_first(&prog, b"baaab"), Some((1, 2)));
    }

    #[test]
    fn optional_matches_either_zero_or_one() {
        let prog = compile("ab?c");
        assert_eq!(pike_find_first(&prog, b"ac"), Some((0, 2)));
        assert_eq!(pike_find_first(&prog, b"abc"), Some((0, 3)));
    }

    #[test]
    fn range_quantifier_matches_exact_count() {
        let prog = compile(r"\d{4}");
        assert_eq!(pike_find_first(&prog, b"year 2026 q2"), Some((5, 9)));
    }

    #[test]
    fn range_quantifier_with_max_matches_up_to_max() {
        let prog = compile(r"\d{2,4}");
        assert_eq!(pike_find_first(&prog, b"abc 12345 xyz"), Some((4, 8)));
    }

    // ============================================================
    // Anchors and assertions
    // ============================================================

    #[test]
    fn start_of_text_anchor() {
        let prog = compile(r"\Aabc");
        assert_eq!(pike_find_first(&prog, b"abc def"), Some((0, 3)));
        assert_eq!(pike_find_first(&prog, b"xx abc"), None);
    }

    #[test]
    fn end_of_text_anchor() {
        let prog = compile(r"abc\z");
        assert_eq!(pike_find_first(&prog, b"def abc"), Some((4, 7)));
        assert_eq!(pike_find_first(&prog, b"abc def"), None);
    }

    #[test]
    fn word_boundary_finds_word() {
        let prog = compile(r"\bcat\b");
        assert_eq!(pike_find_first(&prog, b"my cat is"), Some((3, 6)));
        assert_eq!(pike_find_first(&prog, b"category"), None);
    }

    #[test]
    fn line_anchors_caret_and_dollar() {
        let prog = compile(r"^abc$");
        assert_eq!(pike_find_first(&prog, b"abc"), Some((0, 3)));
        // Without multiline mode, ^ and $ only match at absolute start/end.
        assert_eq!(pike_find_first(&prog, b"x abc y"), None);
    }

    // ============================================================
    // find_all
    // ============================================================

    #[test]
    fn find_all_returns_non_overlapping_matches() {
        let prog = compile(r"\d+");
        assert_eq!(
            pike_find_all(&prog, b"a 12 b 34 c 567"),
            vec![(2, 4), (7, 9), (12, 15)]
        );
    }

    #[test]
    fn find_all_empty_input_returns_empty_vec() {
        let prog = compile("abc");
        assert_eq!(pike_find_all(&prog, b""), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn find_all_advances_past_empty_match() {
        // a* matches empty at every position; find_all should not loop.
        let prog = compile("a*");
        let result = pike_find_all(&prog, b"bbb");
        // Each position yields an empty match; the advance-by-1 rule
        // gives one match per position plus one at the end.
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], (0, 0));
        assert_eq!(result[3], (3, 3));
    }

    // ============================================================
    // Multi-byte UTF-8
    // ============================================================

    #[test]
    fn matches_two_byte_utf8_literal() {
        let prog = compile("α");
        // 'α' = U+03B1 = 0xCE 0xB1 (2 bytes).
        let input = "xαy".as_bytes();
        let pos = pike_find_first(&prog, input).expect("should match");
        assert_eq!(&input[pos.0..pos.1], "α".as_bytes());
    }

    #[test]
    fn matches_three_byte_utf8_literal() {
        let prog = compile("あ");
        // 'あ' = U+3042 = 0xE3 0x81 0x82 (3 bytes).
        let input = "xあy".as_bytes();
        let pos = pike_find_first(&prog, input).expect("should match");
        assert_eq!(&input[pos.0..pos.1], "あ".as_bytes());
    }

    // ============================================================
    // Realistic patterns
    // ============================================================

    #[test]
    fn matches_iso_date_pattern() {
        let prog = compile(r"\d{4}-\d{2}-\d{2}");
        assert_eq!(
            pike_find_first(&prog, b"today is 2026-04-10 ok"),
            Some((9, 19))
        );
    }

    #[test]
    fn matches_email_like_pattern() {
        let prog = compile(r"[\w.+-]+@[\w-]+\.[\w.-]+");
        let input = b"contact: alice+test@example.com please";
        let result = pike_find_first(&prog, input);
        assert!(result.is_some());
        let (s, e) = result.unwrap();
        assert_eq!(&input[s..e], b"alice+test@example.com");
    }

    #[test]
    fn finds_all_log_levels() {
        let prog = compile("ERROR|WARN|INFO");
        let input = b"INFO: ok\nERROR: bad\nWARN: meh\n";
        let matches = pike_find_all(&prog, input);
        assert_eq!(matches.len(), 3);
    }
}
