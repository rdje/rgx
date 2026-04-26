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
use crate::c2::nfa::{CaptureTag, Nfa, NfaStateId, ZeroWidthAssertion};
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

/// Returns `true` iff the pattern matches at the **specific** scan
/// position `start` in `input`.
///
/// Uses the **forward anchored** NFA — the simulator runs at exactly
/// one position rather than scanning the whole input. Used by the
/// engine dispatch layer's `try_pike_is_match` together with the
/// `PrefixScanner` to skip non-candidate scan positions.
#[must_use]
pub fn pike_is_match_at(program: &CompiledC2Program, input: &[u8], start: usize) -> bool {
    let nfa = &program.forward_anchored;
    let bcm = &program.byte_class_map;
    pike_match_at(nfa, bcm, input, start).is_some()
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

// ============================================================
// Capture-tracking simulation (step 4b)
// ============================================================
//
// The capture-tracking path is a parallel implementation of the
// no-captures path above. The simulation loop is structurally
// identical, but each thread carries its own capture buffer alongside
// its state ID. The two paths are kept separate so the no-captures
// fast path doesn't pay the cost of capture-buffer allocation, copies,
// or per-edge tag application.
//
// # Slot layout
//
// Capture buffers are flat slices of `2 * (num_capture_groups + 1)`
// `Option<usize>` slots. Slot indexing:
//
// - `slots[0]` = overall match start (group 0 start)
// - `slots[1]` = overall match end (group 0 end)
// - `slots[2k]` = group `k` start (for `k >= 1`)
// - `slots[2k+1]` = group `k` end
//
// Slots 0 and 1 (overall match) are populated by the caller from the
// scan position and the simulator's matched end position — the NFA
// builder doesn't emit `CaptureTag::GroupStart(0)` / `GroupEnd(0)` for
// the overall match.

/// A match span with capture group positions.
///
/// Returned by [`pike_captures`] and [`pike_captures_all`]. The
/// `groups` vector is indexed the same way as the existing
/// `MatchResult.groups`: index 0 is the overall match span, indices
/// `1..=N` are the explicit capture groups (`None` for groups that
/// didn't participate in the match).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PikeMatch {
    /// Overall match start (byte offset).
    pub start: usize,
    /// Overall match end (byte offset).
    pub end: usize,
    /// Capture group spans. Index 0 is the overall match, indices
    /// 1..=N are explicit capture groups. `None` means the group did
    /// not participate in the match.
    pub groups: Vec<Option<(usize, usize)>>,
}

/// Sparse-set thread container that carries a capture buffer per
/// active state. Used by the capture-tracking simulation path.
///
/// Structurally a parallel implementation of [`SparseSet`] with an
/// extra `dense_captures` array indexed by dense position. Each
/// capture buffer is a fixed-length slice of `Option<usize>` slots
/// pre-allocated at construction time so the simulation loop never
/// allocates.
#[derive(Debug)]
struct ThreadSet {
    sparse: Vec<u32>,
    dense_states: Vec<NfaStateId>,
    /// Per-thread capture buffers, parallel to `dense_states`. Each
    /// inner slice has length `num_slots`.
    dense_captures: Vec<Vec<Option<usize>>>,
    len: usize,
    num_slots: usize,
}

impl ThreadSet {
    fn new(num_states: usize, num_slots: usize) -> Self {
        Self {
            sparse: vec![0u32; num_states],
            dense_states: vec![0u32; num_states],
            dense_captures: vec![vec![None; num_slots]; num_states],
            len: 0,
            num_slots,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    fn contains(&self, state: NfaStateId) -> bool {
        let i = self.sparse[state as usize] as usize;
        i < self.len && self.dense_states[i] == state
    }

    /// Add `state` with `captures` to the set if not already present.
    /// First-insertion-wins, mirroring [`SparseSet::insert`].
    fn add(&mut self, state: NfaStateId, captures: &[Option<usize>]) {
        debug_assert_eq!(captures.len(), self.num_slots);
        if self.contains(state) {
            return;
        }
        self.sparse[state as usize] = self.len as u32;
        self.dense_states[self.len] = state;
        self.dense_captures[self.len].copy_from_slice(captures);
        self.len += 1;
    }

    fn position_of(&self, state: NfaStateId) -> Option<usize> {
        if self.contains(state) {
            Some(self.sparse[state as usize] as usize)
        } else {
            None
        }
    }

    fn state_at(&self, dense_pos: usize) -> NfaStateId {
        debug_assert!(dense_pos < self.len);
        self.dense_states[dense_pos]
    }

    fn captures_at(&self, dense_pos: usize) -> &[Option<usize>] {
        debug_assert!(dense_pos < self.len);
        &self.dense_captures[dense_pos]
    }
}

/// Reusable scratch buffers for the capture-tracking Pike-VM
/// simulator. Allocated once per `CompiledC2Program` and reset
/// between calls so the simulator never allocates `ThreadSet`s on
/// the hot path.
///
/// Profiles taken 2026-04-26 (`scripts/run-samply.sh
/// digit_sequence.find_first` etc.) showed `ThreadSet::new` at
/// 13-24% inclusive across non-literal patterns — Pike-VM was
/// allocating a fresh sparse-set scratch buffer at every
/// `PrefixScanner` candidate position. Caching the buffer at the
/// engine level and `clear()`ing it between calls eliminates that
/// alloc cycle without touching the algorithm.
///
/// Keeps the `dense_captures` row Vecs allocated (capacity =
/// `num_states`); only `len` is reset, mirroring [`ThreadSet::clear`].
#[derive(Debug)]
pub struct PikeScratch {
    current: ThreadSet,
    next: ThreadSet,
    /// Initial capture buffer (all `None`) reused across calls.
    initial_captures: Vec<Option<usize>>,
    num_states: usize,
    num_slots: usize,
}

impl PikeScratch {
    /// Allocate scratch sized for `program`'s capture-tracking
    /// simulation. Sized once; reused indefinitely.
    #[must_use]
    pub fn new(program: &CompiledC2Program) -> Self {
        let num_states = program.forward_anchored.num_states();
        let num_slots = slot_count(program);
        Self {
            current: ThreadSet::new(num_states, num_slots),
            next: ThreadSet::new(num_states, num_slots),
            initial_captures: vec![None; num_slots],
            num_states,
            num_slots,
        }
    }

    /// Reset for a fresh match attempt. Keeps allocated buffers; just
    /// rewinds the lengths.
    fn reset(&mut self) {
        self.current.clear();
        self.next.clear();
        // initial_captures must always read as all-None for the next
        // call. Reset to match. This is a single contiguous write
        // and stays in cache.
        for slot in &mut self.initial_captures {
            *slot = None;
        }
    }
}

/// Apply a capture tag to a buffer at the given position.
///
/// `slot_for_tag` maps a capture group number to its start/end slot
/// indices according to the layout in the module-level comment above.
fn apply_capture_tag(captures: &mut [Option<usize>], tag: CaptureTag, pos: usize) {
    match tag {
        CaptureTag::GroupStart(n) => {
            let slot = 2 * (n as usize);
            if slot < captures.len() {
                captures[slot] = Some(pos);
            }
        }
        CaptureTag::GroupEnd(n) => {
            let slot = 2 * (n as usize) + 1;
            if slot < captures.len() {
                captures[slot] = Some(pos);
            }
        }
    }
}

/// Capture-aware epsilon closure.
///
/// Mirrors [`epsilon_closure`] but threads a capture buffer through
/// the recursion. When an epsilon edge carries a capture tag, the
/// buffer is cloned and the tag is applied; the modified buffer is
/// then passed to the recursive call. Edges without tags pass the
/// buffer through unchanged, which avoids the per-edge clone on the
/// common case (most epsilon edges don't carry tags).
fn epsilon_closure_with_captures(
    set: &mut ThreadSet,
    nfa: &Nfa,
    state: NfaStateId,
    captures: &[Option<usize>],
    pos: usize,
    input: &[u8],
) {
    if set.contains(state) {
        return;
    }
    set.add(state, captures);

    let state_obj = &nfa.states()[state as usize];

    for edge in &state_obj.epsilons {
        if let Some(assertion) = edge.assertion {
            if !check_assertion(assertion, input, pos) {
                continue;
            }
        }
        if let Some(tag) = edge.capture_tag {
            // Tagged edge: clone the buffer, apply the tag, recurse
            // with the modified buffer. The clone is unavoidable
            // because the tagged target needs different captures than
            // any sibling target reached through a non-tagged edge.
            let mut new_captures = captures.to_vec();
            apply_capture_tag(&mut new_captures, tag, pos);
            epsilon_closure_with_captures(set, nfa, edge.target, &new_captures, pos, input);
        } else {
            // No tag: pass the buffer through unchanged. Same buffer
            // contents reach the target, no allocation.
            epsilon_closure_with_captures(set, nfa, edge.target, captures, pos, input);
        }
    }
}

/// Capture-aware single-position match.
///
/// Mirrors [`pike_match_at`] but tracks capture group positions per
/// thread and returns the winning capture buffer alongside the match
/// end position. Slots 0 and 1 (the overall match span) are populated
/// by the caller from `start` and the returned end position.
fn pike_match_at_with_captures(
    nfa: &Nfa,
    byte_class_map: &ByteClassMap,
    input: &[u8],
    start: usize,
    num_slots: usize,
    scratch: &mut PikeScratch,
) -> Option<(usize, Vec<Option<usize>>)> {
    debug_assert_eq!(scratch.num_states, nfa.num_states());
    debug_assert_eq!(scratch.num_slots, num_slots);
    scratch.reset();
    // Disjoint field borrows: `current` and `next` are independent
    // `ThreadSet`s on the `scratch` struct; `initial_captures` is a
    // shared read of a third field. Rust's split-borrow rule allows
    // this because no two borrows alias.
    let current: &mut ThreadSet = &mut scratch.current;
    let next: &mut ThreadSet = &mut scratch.next;
    let initial_captures: &[Option<usize>] = &scratch.initial_captures;
    let mut matched: Option<(usize, Vec<Option<usize>>)> = None;

    epsilon_closure_with_captures(current, nfa, nfa.start(), initial_captures, start, input);

    let mut pos = start;
    loop {
        let accept_priority = current.position_of(nfa.accept());

        if let Some(p) = accept_priority {
            // Snapshot the captures from the highest-priority thread
            // that reached accept. The dense order encodes priority,
            // so the captures at position `p` are the winning ones.
            matched = Some((pos, current.captures_at(p).to_vec()));
        }

        if pos >= input.len() || current.len == 0 {
            break;
        }

        let byte = input[pos];
        let cls = byte_class_map.class_of(byte);
        next.clear();

        // Same priority-cutoff trick as the no-captures path: only
        // extend threads at dense positions ≤ accept's position.
        let limit = match accept_priority {
            Some(p) => p + 1,
            None => current.len,
        };

        for i in 0..limit {
            let state_id = current.state_at(i);
            // Borrow the captures buffer directly from `current` —
            // `epsilon_closure_with_captures` reads it as `&[…]` and
            // its own `set.add` already does `copy_from_slice` into
            // `next`'s storage. The previous `.to_vec()` here was a
            // redundant per-thread heap allocation: in profiles
            // taken 2026-04-26 (`pike_captures_at` 90-96% inclusive
            // on non-literal patterns) it accounted for a
            // measurable share of the libsystem_malloc time. The
            // borrow checker accepts this because `current` and
            // `next` are distinct `ThreadSet`s; the only mutation
            // path goes through `&mut next`.
            let state_captures = current.captures_at(i);
            let state_obj = &nfa.states()[state_id as usize];
            for &(transition_cls, target) in &state_obj.transitions {
                if transition_cls == cls {
                    epsilon_closure_with_captures(
                        next,
                        nfa,
                        target,
                        state_captures,
                        pos + 1,
                        input,
                    );
                }
            }
        }

        // Swap the `ThreadSet`s held behind the two mutable
        // references. `std::mem::swap` rotates the values in place;
        // the underlying allocations stay attached to `scratch` and
        // are reused for the next iteration.
        std::mem::swap(current, next);
        pos += 1;
    }

    matched
}

/// Compute the slot count for a compiled program: two slots per
/// group, plus two slots for the overall match (group 0).
fn slot_count(program: &CompiledC2Program) -> usize {
    2 * (program.num_capture_groups as usize + 1)
}

/// Convert a flat capture buffer into a `Vec<Option<(usize, usize)>>`
/// for the [`PikeMatch.groups`] field. Pairs adjacent slots into
/// `(start, end)` tuples; both slots must be `Some` for the group to
/// be reported as having participated in the match.
fn captures_to_groups(captures: &[Option<usize>]) -> Vec<Option<(usize, usize)>> {
    captures
        .chunks(2)
        .map(|pair| match (pair[0], pair[1]) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        })
        .collect()
}

/// Returns the leftmost match in `input` with capture group
/// positions, or `None` if there is no match.
///
/// This is the capture-tracking variant of [`pike_find_first`]. It
/// runs the capture-aware simulator at every scan position from 0 to
/// `input.len()` and returns the first position where the anchored
/// NFA reaches its accept state, along with the winning thread's
/// capture buffer.
///
/// Slots 0 and 1 of the returned `PikeMatch.groups` (the overall
/// match span) are populated from the scan position and the
/// simulator's matched end. Capture groups 1..=N are populated from
/// the winning thread's capture buffer.
#[must_use]
pub fn pike_captures(program: &CompiledC2Program, input: &[u8]) -> Option<PikeMatch> {
    let nfa = &program.forward_anchored;
    let bcm = &program.byte_class_map;
    let num_slots = slot_count(program);
    let prefix = program.c2_prefix_byte;
    let mut scratch = PikeScratch::new(program);
    let mut start = 0usize;
    while start <= input.len() {
        // C2 step 7: literal prefix skip via memchr.
        if let Some(byte) = prefix {
            if start >= input.len() {
                break;
            }
            match memchr::memchr(byte, &input[start..]) {
                Some(offset) => start += offset,
                None => return None,
            }
        }
        if let Some((end, mut caps)) =
            pike_match_at_with_captures(nfa, bcm, input, start, num_slots, &mut scratch)
        {
            // Populate the overall match span (group 0) from the
            // scan position and the simulator's matched end.
            caps[0] = Some(start);
            caps[1] = Some(end);
            return Some(PikeMatch {
                start,
                end,
                groups: captures_to_groups(&caps),
            });
        }
        start += 1;
    }
    None
}

/// Returns the match at a **specific** scan position with capture
/// group positions, or `None` if the pattern doesn't match starting at
/// `start`.
///
/// Mirrors [`pike_captures`] but runs the simulator at exactly one scan
/// position rather than scanning every position from 0 to `input.len()`.
/// Used by C2 step 6 engine dispatch to recover capture positions
/// after the lazy DFA has confirmed a match exists at a specific scan
/// position. Avoids the wasted scan that calling `pike_captures` would
/// do for the same caller.
///
/// This is the bounded-Pike-VM-pass building block from
/// `docs/C2_NFA_DFA_DESIGN.md` §9 with the START position known.
#[must_use]
pub fn pike_captures_at(
    program: &CompiledC2Program,
    input: &[u8],
    start: usize,
) -> Option<PikeMatch> {
    let mut scratch = PikeScratch::new(program);
    pike_captures_at_with_scratch(program, input, start, &mut scratch)
}

/// Variant of [`pike_captures_at`] that takes a caller-owned
/// [`PikeScratch`] so the same buffers are reused across many
/// calls. Engine dispatch holds a cached scratch and passes it here
/// for every `try_pike_find_first` / `try_pike_find_all` call,
/// eliminating per-candidate `ThreadSet` allocation that profiling
/// (see CHANGES.md 2026-04-26) showed was 13-24% inclusive on
/// non-literal patterns.
#[must_use]
pub fn pike_captures_at_with_scratch(
    program: &CompiledC2Program,
    input: &[u8],
    start: usize,
    scratch: &mut PikeScratch,
) -> Option<PikeMatch> {
    let nfa = &program.forward_anchored;
    let bcm = &program.byte_class_map;
    let num_slots = slot_count(program);
    let (end, mut caps) = pike_match_at_with_captures(nfa, bcm, input, start, num_slots, scratch)?;
    caps[0] = Some(start);
    caps[1] = Some(end);
    Some(PikeMatch {
        start,
        end,
        groups: captures_to_groups(&caps),
    })
}

/// Returns all non-overlapping matches in `input` with capture group
/// positions in left-to-right order.
///
/// Capture-tracking variant of [`pike_find_all`]. Same advance rules:
/// after a non-empty match the next scan starts at the match end;
/// after an empty match the next scan starts one byte later; an empty
/// match immediately adjacent to a previous non-empty match is
/// dropped.
#[must_use]
pub fn pike_captures_all(program: &CompiledC2Program, input: &[u8]) -> Vec<PikeMatch> {
    let mut scratch = PikeScratch::new(program);
    pike_captures_all_with_scratch(program, input, &mut scratch)
}

/// Variant of [`pike_captures_all`] that takes a caller-owned
/// [`PikeScratch`] so the simulator never allocates fresh
/// [`ThreadSet`]s. Used by [`crate::engine::Engine::try_pike_find_all`]
/// with an engine-cached scratch buffer.
#[must_use]
pub fn pike_captures_all_with_scratch(
    program: &CompiledC2Program,
    input: &[u8],
    scratch: &mut PikeScratch,
) -> Vec<PikeMatch> {
    let nfa = &program.forward_anchored;
    let bcm = &program.byte_class_map;
    let num_slots = slot_count(program);
    let prefix = program.c2_prefix_byte;
    let mut results = Vec::new();
    let mut start = 0usize;
    let mut prev_non_empty_end: Option<usize> = None;
    while start <= input.len() {
        // C2 step 7: literal prefix skip via memchr.
        if let Some(byte) = prefix {
            if start >= input.len() {
                break;
            }
            match memchr::memchr(byte, &input[start..]) {
                Some(offset) => start += offset,
                None => break,
            }
        }
        let Some((end, mut caps)) =
            pike_match_at_with_captures(nfa, bcm, input, start, num_slots, scratch)
        else {
            start += 1;
            continue;
        };
        let is_empty = end == start;
        if is_empty && Some(start) == prev_non_empty_end {
            start += 1;
            continue;
        }
        caps[0] = Some(start);
        caps[1] = Some(end);
        results.push(PikeMatch {
            start,
            end,
            groups: captures_to_groups(&caps),
        });
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

    #[test]
    fn negated_class_matches_first_non_digit_with_run_of_non_digits() {
        // Regression test for the C1 step 6 bug: `[^0-9]` against
        // "123abc" used to return (3, 6) — the entire run of
        // non-digits — because the byte_class_map didn't distinguish
        // ASCII bytes from UTF-8 continuation/leading bytes, which
        // allowed the NFA's multi-byte chains for the negated range
        // to fire on ASCII input. The fix in c2/byte_class.rs adds
        // UTF-8 byte-category boundary oracles unconditionally,
        // forcing a finer partition that correctly separates ASCII
        // from non-ASCII byte ranges.
        let prog = compile(r"[^0-9]");
        // Both pike_find_first AND pike_captures_at must return
        // (3, 4) — one non-digit character, not the whole run.
        assert_eq!(pike_find_first(&prog, b"123abc"), Some((3, 4)));
        let caps = pike_captures_at(&prog, b"123abc", 3).expect("must match");
        assert_eq!((caps.start, caps.end), (3, 4));
    }

    #[test]
    fn negated_class_correctly_consumes_multibyte_unicode_char() {
        // Verify the fix doesn't break valid multi-byte UTF-8 matching.
        // `[^0-9]` against "1café" (where 'é' is 0xC3 0xA9, 2 bytes):
        //   - pos 0,1,2,3: '1' is a digit, no match.
        //   - pos 1: 'c' is non-digit ASCII → match (1,2). ✓
        //   - At pos 4: 'é' is the start of a 2-byte UTF-8 char →
        //     match (4, 6) consuming both bytes. ✓
        let prog = compile(r"[^0-9]");
        let text = "1café".as_bytes();
        // First match starts at pos 1 ('c'), span (1, 2).
        assert_eq!(pike_find_first(&prog, text), Some((1, 2)));
        // Verify we can match the multi-byte 'é' at pos 4 (after
        // "1caf"). The 'é' is bytes [0xC3, 0xA9] at positions 4..=5.
        let caps_at_e = pike_captures_at(&prog, text, 4).expect("must match");
        assert_eq!((caps_at_e.start, caps_at_e.end), (4, 6));
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

    // ============================================================
    // Capture tracking
    // ============================================================

    #[test]
    fn captures_zero_groups_returns_overall_match_only() {
        // Pattern with no capture groups: groups[0] = overall match,
        // and the result has length 1 (only group 0).
        let prog = compile("hello");
        let m = pike_captures(&prog, b"hello world").expect("match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
        assert_eq!(m.groups, vec![Some((0, 5))]);
    }

    #[test]
    fn captures_one_group_returns_two_entries() {
        // (\w+) — one capture group. groups[0] = overall, groups[1] = group 1.
        let prog = compile(r"(\w+)");
        let m = pike_captures(&prog, b"  hello").expect("match");
        assert_eq!(m.start, 2);
        assert_eq!(m.end, 7);
        assert_eq!(m.groups, vec![Some((2, 7)), Some((2, 7))]);
    }

    #[test]
    fn captures_multiple_groups() {
        // (\d+)-(\d+) — two capture groups.
        let prog = compile(r"(\d+)-(\d+)");
        let m = pike_captures(&prog, b"year 12-345 day").expect("match");
        assert_eq!(m.start, 5);
        assert_eq!(m.end, 11);
        assert_eq!(m.groups, vec![Some((5, 11)), Some((5, 7)), Some((8, 11))]);
    }

    #[test]
    fn captures_nested_groups() {
        // ((\w+)-(\w+)) — nested groups. group 1 = whole, group 2 = first
        // word, group 3 = second word.
        let prog = compile(r"((\w+)-(\w+))");
        let m = pike_captures(&prog, b"foo-bar baz").expect("match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 7);
        assert_eq!(
            m.groups,
            vec![
                Some((0, 7)), // overall
                Some((0, 7)), // outer group 1
                Some((0, 3)), // inner group 2 = "foo"
                Some((4, 7)), // inner group 3 = "bar"
            ]
        );
    }

    #[test]
    fn captures_optional_group_unmatched() {
        // a(b)?c — group 1 is optional. When the input is "ac", group 1
        // does not participate.
        let prog = compile("a(b)?c");
        let m = pike_captures(&prog, b"ac").expect("match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 2);
        // group 0 = overall, group 1 = None (didn't participate)
        assert_eq!(m.groups, vec![Some((0, 2)), None]);
    }

    #[test]
    fn captures_optional_group_matched() {
        let prog = compile("a(b)?c");
        let m = pike_captures(&prog, b"abc").expect("match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
        assert_eq!(m.groups, vec![Some((0, 3)), Some((1, 2))]);
    }

    #[test]
    fn captures_no_match_returns_none() {
        let prog = compile(r"(\d+)");
        assert_eq!(pike_captures(&prog, b"no digits"), None);
    }

    #[test]
    fn captures_alternation_picks_winning_branch() {
        // (cat|dog) — only one branch participates per match.
        let prog = compile("(cat|dog)");
        let m = pike_captures(&prog, b"the dog runs").expect("match");
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert_eq!(m.groups, vec![Some((4, 7)), Some((4, 7))]);
    }

    #[test]
    fn captures_all_returns_all_matches_with_groups() {
        // (\w+) — find all words and capture each.
        let prog = compile(r"(\w+)");
        let matches = pike_captures_all(&prog, b"foo bar baz");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].groups[1], Some((0, 3)));
        assert_eq!(matches[1].groups[1], Some((4, 7)));
        assert_eq!(matches[2].groups[1], Some((8, 11)));
    }

    #[test]
    fn captures_quantified_group_keeps_last_iteration() {
        // (\w)+ — group 1 captures the last iteration of \w. Standard
        // PCRE2/Perl semantics.
        let prog = compile(r"(\w)+");
        let m = pike_captures(&prog, b"abc").expect("match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
        // group 1 = last single-char iteration = 'c' at (2, 3)
        assert_eq!(m.groups[1], Some((2, 3)));
    }

    #[test]
    fn captures_iso_date_with_three_groups() {
        let prog = compile(r"(\d{4})-(\d{2})-(\d{2})");
        let m = pike_captures(&prog, b"today is 2026-04-10 ok").expect("match");
        assert_eq!(m.start, 9);
        assert_eq!(m.end, 19);
        assert_eq!(
            m.groups,
            vec![
                Some((9, 19)),  // overall
                Some((9, 13)),  // year
                Some((14, 16)), // month
                Some((17, 19)), // day
            ]
        );
    }
}
