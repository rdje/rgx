//! Lazy forward DFA cache for the C2 NFA/DFA hybrid engine.
//!
//! Implements the SOTA "lazy DFA" approach used by RE2 and the Rust
//! `regex` crate: DFA states are constructed on demand from the source
//! NFA via subset construction, cached in a `HashMap`, and reused on
//! repeated transitions. The result is a deterministic, table-driven
//! engine whose hot path is two array lookups per input byte (a byte-
//! class lookup and a transition table lookup) and an integer compare
//! against a sentinel "dead state" value.
//!
//! This is the lazy-DFA tier of the C2 chain (see
//! `docs/C2_NFA_DFA_DESIGN.md` §15 steps 5/6 for the phased plan). It
//! is wired into engine dispatch via `try_dfa_*` in `engine.rs` and
//! handles patterns containing `\b` / `\B` word boundaries (since
//! 2026-05-12) by extending state IDs with a `prev_byte_was_word`
//! flag and pre-computing per-state acceptance flags for both word-
//! boundary contexts. Positional anchors (`\A`, `\z`, `\Z`, `^`, `$`)
//! and `\G` are still DFA-ineligible — those patterns route to the
//! Pike-VM tier.
//!
//! # DFA semantic limitations
//!
//! Subset construction is **leftmost-longest by nature**. The DFA
//! cannot directly express the leftmost-first / lazy semantics that
//! the Pike-VM honours via its priority cutoff. For patterns whose
//! semantics depend on priority order:
//!
//! - **Lazy quantifiers** (`a*?`, `a+?`, `a??`, `{n,m}?`): the DFA
//!   gives the longest match where the Pike-VM would give the shortest.
//!   For `a+?` on `"baaab"`, the DFA returns end=4 but the Pike-VM
//!   (and PCRE2/Perl) return end=2. Step 5b excludes patterns
//!   containing lazy quantifiers from DFA dispatch.
//!
//! - **Top-level alternation with priority semantics**: already excluded
//!   from C2 dispatch entirely (because Pike-VM doesn't track
//!   `matched_branch_number`).
//!
//! These exclusions are SOTA-correct: routed patterns continue to run
//! on the Pike-VM which honours the leftmost-first priority order.
//!
//! # Subset construction (the basics)
//!
//! A DFA state corresponds to a set of NFA states the simulator could
//! be in simultaneously. Each transition over a byte class advances the
//! whole set: for each NFA state in the source set, follow byte
//! transitions matching the class, then epsilon-close the resulting
//! targets. The new set is looked up in the cache; if it's already a
//! known DFA state, reuse the existing ID. Otherwise allocate a fresh
//! state and store it. Either way, the source state's transition table
//! is updated with the target ID so future lookups are O(1).
//!
//! # Byte-class compression
//!
//! Transitions are indexed by **byte class** (from the precomputed
//! [`ByteClassMap`]) rather than by raw byte. The transition table per
//! state has `num_byte_classes` entries instead of 256. For typical
//! patterns this is a 5-10x compression on transition table memory.
//!
//! # Dead states and the cache
//!
//! Transitions that lead to an empty NFA state set are recorded as
//! `DEAD_STATE` (the sentinel). The DFA simulator stops as soon as it
//! enters a dead state — no more matches can be found from there.
//!
//! When the cache exceeds `state_limit`, `transition` returns `None`
//! to signal cache exhaustion. The caller (eventually engine dispatch
//! in step 5b) is expected to fall back to the Pike-VM at that point.
//!
//! # Differential testing
//!
//! Step 5a includes a small set of unit tests that compile real
//! patterns through `CompiledC2Program::try_compile`, build a `LazyDfa`
//! from the resulting forward anchored NFA, and assert the DFA's
//! `find_match_at` produces the same results as the Pike-VM's
//! `pike_find_first` on a corpus of inputs. This is the in-module
//! sanity check before step 5b plugs the DFA into the broader 856-test
//! differential gate via engine dispatch.
//!
//! # References
//!
//! - `docs/C2_NFA_DFA_DESIGN.md` §8 — design rationale and the cache
//!   eviction policy
//! - Russ Cox, "Regular Expression Matching in the Wild" — RE2's lazy
//!   DFA construction and byte-class compression
//! - The Rust `regex-automata` crate's `dfa::dense` and `hybrid::dfa`
//!   modules

use crate::c2::byte_class::ByteClassMap;
use crate::c2::nfa::{Nfa, NfaStateId};
use std::collections::HashMap;
use std::sync::Arc;

/// State identifier in the lazy DFA. The start state is always `0`.
pub type DfaStateId = u32;

/// Sentinel for "computed dead transition" in the cached transitions
/// table — a `(state, byte_class)` pair that was looked up and found
/// to have no NFA-reachable target. The simulator stops on entry to
/// the dead state. `transition()` returns `TransitionResult::Dead` in
/// this case **without** re-running `compute_transition_set`.
const DEAD_STATE: DfaStateId = u32::MAX - 1;

/// Sentinel for "uncached transition slot" — the initial value for
/// every cell in `LazyDfa.transitions` until that `(state, byte_class)`
/// pair is first looked up. Distinct from `DEAD_STATE` so cached-dead
/// lookups can short-circuit without re-running the cold computation.
///
/// History: 2026-04-27 instrumentation showed `compute_transition_set`
/// firing ~6,000 times per `capture_groups.find_all` call because the
/// transitions table used a single sentinel for both "uncached" and
/// "computed dead". Every dead-transition lookup hit the cold path. The
/// two-sentinel design is the SOTA fix; matches `regex-automata::dfa`.
const UNCACHED: DfaStateId = u32::MAX;

/// Outcome of a single-position DFA match attempt.
///
/// Distinguishes "definitively no match" (`NoMatch`) from "couldn't
/// finish — fall back to a slower engine" (`Exhausted`). Engine
/// dispatch in C2 step 5b uses this to decide whether to return the
/// answer or fall back to the Pike-VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DfaSearchOutcome {
    /// A match was found ending at the given byte position.
    Match(usize),
    /// No match exists at this scan position. The simulator ran to
    /// completion (either the input was exhausted or it entered a
    /// dead state).
    NoMatch,
    /// The DFA cache filled up before the simulator could finish.
    /// The caller should fall back to a slower engine (Pike-VM) for
    /// this match attempt.
    Exhausted,
}

/// Outcome of a single transition lookup. Internal to the DFA — the
/// public `find_match_at` API surfaces this as `DfaSearchOutcome`.
#[derive(Debug, Clone, Copy)]
enum TransitionResult {
    /// Successfully transitioned to the given DFA state.
    Next(DfaStateId),
    /// No transition exists for this byte class from the source state.
    /// The simulator stops here (current `matched_end` is the answer).
    Dead,
    /// The DFA cache is full and a new state would have been allocated.
    /// The simulator stops and the caller should fall back to Pike-VM.
    Exhausted,
}

/// A single DFA state's metadata. Transitions live on `LazyDfa` in a
/// single flat `Vec<DfaStateId>` so the hot scan loop walks one Vec
/// indirection per byte instead of two. samply 2026-04-26 attributed
/// 25-31% self-time on `capture_groups.find_first` / `find_all` to
/// `LazyDfa::transition`; the previous two-level layout
/// (`Vec<DfaState> { transitions: Vec<DfaStateId> }`) cost an extra
/// pointer chase + bounds-check per byte. Compare with the equivalent
/// design in `regex-automata::dfa::dense`. The `nfa_states` field is
/// kept on the state itself rather than only in the cache so the
/// transition computation can read it without a reverse cache lookup.
#[derive(Debug, Clone)]
struct DfaState {
    /// True iff the NFA's accept state is in `nfa_states` *without*
    /// any word-boundary epsilon expansion. For patterns with no
    /// `\b` / `\B` edges this is the unconditional accept indicator;
    /// the simulator just reads this and treats it as "the position
    /// after the transition into this state is a valid match end".
    /// For patterns *with* `\b` / `\B` the accept may also be
    /// reachable via a satisfied `WordBoundary` epsilon — see the
    /// precomputed [`Self::accept_when_fire_wb`] /
    /// [`Self::accept_when_not_fire_wb`] flags below.
    is_accept: bool,
    /// Pre-computed acceptance flag for "current position is a word
    /// boundary" context (`fire_wb = true`). Set at allocation by
    /// running an epsilon closure that traverses `WordBoundary`
    /// edges (and skips `NotWordBoundary`). Always `is_accept` is a
    /// subset of this. The runtime accept-check is a flag lookup
    /// instead of a per-byte closure expansion — critical for
    /// throughput on `\b`-heavy patterns scanning long inputs.
    accept_when_fire_wb: bool,
    /// Pre-computed acceptance flag for "current position is NOT a
    /// word boundary" context (`fire_wb = false`). Mirror of
    /// [`Self::accept_when_fire_wb`] for the complementary
    /// `NotWordBoundary` direction.
    accept_when_not_fire_wb: bool,
    /// The NFA state set this DFA state represents. Sorted, deduplicated.
    /// Stored WITHOUT `WordBoundary` / `NotWordBoundary` epsilon
    /// expansion — those edges are re-evaluated on demand at each
    /// transition (and at each accept check) with the
    /// (prev-byte-was-word, current-byte-is-word) context. See
    /// [`LazyDfa::compute_transition_set`].
    nfa_states: Vec<NfaStateId>,
    /// Whether the byte that put us into this state was an ASCII word
    /// byte. The start state has `prev_byte_was_word = false` (start
    /// of input acts as a non-word "byte" for `\b` evaluation). For
    /// NFAs without word-boundary assertions this field is always
    /// `false` and never read — the fast path in
    /// [`LazyDfa::compute_transition_set`] short-circuits.
    prev_byte_was_word: bool,
}

/// Cache key for the (NFA state set, prev-byte-was-word) → DFA state
/// lookup. Two semantically different DFA states can share the same
/// NFA state set but have different `prev_byte_was_word` flags — they
/// behave differently when evaluating `\b` / `\B` at the next
/// transition, so they're cached separately.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct DfaStateKey {
    nfa_states: Vec<NfaStateId>,
    prev_byte_was_word: bool,
}

/// A lazy forward DFA built on demand from a Thompson NFA.
///
/// Construct via [`LazyDfa::new`], then call [`LazyDfa::find_match_at`]
/// to run the simulator over an input slice starting at a given byte
/// position. The simulator returns the END byte offset of the longest
/// match found at that scan position, or `None` if no match exists or
/// the cache exhausted before a match could be confirmed.
///
/// The DFA owns its `Arc<Nfa>` and `Arc<ByteClassMap>`, so multiple
/// DFAs can share the same NFA cheaply (the eventual lazy reverse DFA
/// in step 6 will share the byte-class map with the forward DFA).
#[derive(Debug)]
pub struct LazyDfa {
    nfa: Arc<Nfa>,
    byte_class_map: Arc<ByteClassMap>,
    /// All allocated DFA states' metadata (accept flag + NFA state set).
    /// Index is the `DfaStateId`. The start state is always at index 0.
    /// Transitions live in `transitions` to keep the hot scan loop a
    /// single Vec dereference; this Vec only carries the cold metadata.
    states: Vec<DfaState>,
    /// Flat transition table: `transitions[state * num_classes + cls]`
    /// is the target `DfaStateId` for `(state, byte_class)`, or
    /// `DEAD_STATE` if no transition has been computed yet (lazy fill)
    /// or the transition is genuinely dead (cached). Length is always
    /// `states.len() * num_classes`. Allocating a new state grows this
    /// by `num_classes` `DEAD_STATE` entries.
    transitions: Vec<DfaStateId>,
    /// Maps NFA state sets to allocated DFA state IDs. Used by
    /// `transition` to deduplicate state allocation.
    cache: HashMap<DfaStateKey, DfaStateId>,
    /// Maximum number of DFA states before the cache "overflows" and
    /// `transition` starts returning `None` to signal fallback. The
    /// cache eviction policy (clear-and-retry) lands in step 5b.
    state_limit: usize,
    /// Number of distinct byte classes in `byte_class_map`. Cached so
    /// transition table allocation doesn't need to read the map.
    num_classes: usize,
}

impl LazyDfa {
    /// Default state cache limit: 2048 DFA states. Mirrors the order of
    /// magnitude used by the Rust `regex` crate. Tunable per
    /// construction call.
    pub const DEFAULT_STATE_LIMIT: usize = 2048;

    /// Build a lazy DFA from a forward NFA and a byte-class map.
    ///
    /// At C2 step 5a the DFA does **not** support zero-width assertions.
    /// If the NFA contains any `\A` / `\z` / `\Z` / `^` / `$` / `\b` /
    /// `\B` / `\G` epsilon edge, this method returns an `Err`. The
    /// caller is expected to fall back to the Pike-VM in that case.
    ///
    /// The start DFA state is constructed eagerly via the epsilon
    /// closure of the NFA's start state, so a freshly-built DFA always
    /// has at least one state at index `0`.
    ///
    /// # Errors
    ///
    /// Returns `Err(...)` if the NFA contains zero-width assertions.
    /// Step 5b will lift this restriction.
    pub fn new(
        nfa: Arc<Nfa>,
        byte_class_map: Arc<ByteClassMap>,
        state_limit: usize,
    ) -> Result<Self, &'static str> {
        // The DFA handles `\b` / `\B` (word-boundary assertions) by
        // extending state IDs with a `prev_byte_was_word` flag and
        // evaluating word-boundary edges on demand during transition
        // and accept-check. Other zero-width assertions (`\A`, `\z`,
        // `\Z`, `^`, `$`, `\G`) remain DFA-ineligible and route to
        // Pike-VM.
        if nfa.has_non_word_boundary_assertions() {
            return Err(
                "LazyDfa does not support patterns with anchor or \\G zero-width assertions; \
                 patterns containing \\A, \\z, \\Z, ^, $, or \\G must run on Pike-VM",
            );
        }
        let num_classes = byte_class_map.num_classes() as usize;
        let mut dfa = Self {
            nfa,
            byte_class_map,
            states: Vec::new(),
            transitions: Vec::new(),
            cache: HashMap::new(),
            state_limit,
            num_classes,
        };
        // Construct two start states — one for "previous byte was
        // non-word" (state 0, used at position 0 of input or after a
        // non-word byte) and one for "previous byte was word"
        // (state 1, used after a word byte). They share the same
        // stored NFA set (the start closure without word-boundary
        // expansion) but differ in `prev_byte_was_word`. The DFA
        // simulator (`find_match_at` etc.) picks between them based
        // on the byte at `input[start - 1]`.
        //
        // For NFAs without word-boundary edges, the two start states
        // behave identically (the WB-expansion is a no-op) and the
        // cache deduplicates them via `DfaStateKey`. But because pw
        // is part of the key, both are still allocated — a small
        // constant overhead per pattern.
        let start_set = dfa.compute_start_set();
        let start_id_non_word = dfa.allocate_state(start_set.clone(), false);
        let start_id_after_word = dfa.allocate_state(start_set, true);
        debug_assert_eq!(start_id_non_word, 0);
        debug_assert_eq!(start_id_after_word, 1);
        Ok(dfa)
    }

    /// The DFA's start state ID for "previous byte was non-word"
    /// context (always `0`). At position 0 of input the previous
    /// byte doesn't exist; for `\b` purposes that's equivalent to a
    /// non-word byte (PCRE2 / Rust regex semantics).
    #[must_use]
    pub fn start_state(&self) -> DfaStateId {
        0
    }

    /// Pick the appropriate **forward-walk** start state given the
    /// byte immediately before `start` in `input`. Returns state 0
    /// (pw=false) if `start == 0` or `input[start-1]` is a non-word
    /// byte, state 1 (pw=true) otherwise.
    ///
    /// For NFAs without word-boundary edges the two start states are
    /// behaviourally identical; the choice doesn't affect the match
    /// outcome. For `\b` / `\B`-bearing patterns this is the
    /// distinction that makes per-position scans evaluate `\b` at
    /// the correct boundary.
    #[inline]
    fn start_state_for(&self, input: &[u8], start: usize) -> DfaStateId {
        if start == 0 {
            0
        } else {
            u32::from(Self::word_ness_at(input, start - 1))
        }
    }

    /// Pick the **reverse-walk** start state given the byte at
    /// `end` in `input`. In reverse-walk semantics the state's
    /// `prev_byte_was_word` represents the byte just consumed in
    /// walk direction — which at the start of a reverse walk is
    /// the byte to the *right* of position `end` (i.e. `input[end]`),
    /// or "no byte" (treated as non-word) when `end >= input.len()`.
    ///
    /// The fire-wb formula `pw != cw` is symmetric between forward
    /// and reverse walks (the operands swap roles), so the
    /// `is_accept_with_word_boundary_context` and
    /// `epsilon_close_with_word_boundary` helpers don't need to
    /// know which direction they're called from — only the initial
    /// pw and per-position cw differ.
    #[inline]
    fn start_state_for_reverse(&self, input: &[u8], end: usize) -> DfaStateId {
        if end >= input.len() {
            0
        } else {
            u32::from(Self::word_ness_at(input, end))
        }
    }

    /// Returns `true` if the given DFA state is an accept state.
    /// Returns `false` for `DEAD_STATE` and for any state whose NFA
    /// state set doesn't contain the NFA accept state.
    #[must_use]
    pub fn is_accept(&self, state: DfaStateId) -> bool {
        state != DEAD_STATE && self.states[state as usize].is_accept
    }

    /// Returns the total number of DFA states currently allocated.
    /// Used by tests and benchmarks; should never exceed `state_limit`
    /// unless the limit was set to `usize::MAX`.
    #[must_use]
    pub fn num_states(&self) -> usize {
        self.states.len()
    }

    /// Compute the transition `(state, byte_class)`, lazily allocating
    /// a new DFA state if needed. Returns one of three outcomes:
    ///
    /// - [`TransitionResult::Next`] for a successful transition (cached
    ///   or freshly allocated)
    /// - [`TransitionResult::Dead`] if no transition exists from the
    ///   source state for this byte class (the simulator stops here)
    /// - [`TransitionResult::Exhausted`] if a new state would have to
    ///   be allocated but the cache is full (the simulator stops and
    ///   the caller falls back to Pike-VM)
    /// Pre-fill the transition cache via BFS from the start states,
    /// stopping early if more than `state_limit` states would be
    /// allocated. Returns `true` if the entire reachable DFA fits
    /// within the limit and is now fully materialised — every
    /// possible transition is either cached as `Next(target)` or
    /// recorded as `DEAD_STATE`. Once this returns `true`, future
    /// calls to [`Self::find_match_at`] / [`Self::find_first_accept_at`]
    /// / [`Self::find_match_start_at_reverse_bounded`] can use the
    /// immutable `_immut` variants instead, which skip the Mutex
    /// acquisition on the dispatch hot path.
    ///
    /// Returns `false` if the limit was hit before BFS completed.
    /// The cache is left in a valid partial state (callers can keep
    /// using the lazy mutable variants).
    pub fn try_materialize(&mut self, state_limit: usize) -> bool {
        let mut queue: std::collections::VecDeque<DfaStateId> = std::collections::VecDeque::new();
        queue.push_back(0);
        if self.states.len() > 1 {
            queue.push_back(1);
        }
        let num_classes = self.num_classes;
        while let Some(state) = queue.pop_front() {
            for cls in 0..num_classes {
                let prev_state_count = self.states.len();
                match self.transition(state, cls as u8) {
                    TransitionResult::Next(target) => {
                        if self.states.len() > prev_state_count && self.states.len() <= state_limit
                        {
                            queue.push_back(target);
                        }
                    }
                    TransitionResult::Dead => {}
                    TransitionResult::Exhausted => return false,
                }
            }
            if self.states.len() > state_limit {
                return false;
            }
        }
        true
    }

    /// Minimize a fully-materialised DFA via Moore's partition
    /// refinement algorithm.
    ///
    /// Two DFA states are equivalent iff they have the same accept-
    /// flag triple (`is_accept`, `accept_when_fire_wb`,
    /// `accept_when_not_fire_wb`) AND for every byte class their
    /// transition targets are equivalent. Moore's algorithm
    /// iteratively refines a partition starting from the accept-
    /// flag-tuple grouping; each round computes a per-state
    /// signature `(current_partition, targets_for_class_0..N)`,
    /// and states with identical signatures collapse into the same
    /// new partition. Converges in at most `n` iterations.
    ///
    /// Should only be called on a fully-materialised DFA (every
    /// reachable transition cached as `Next` or `Dead`, no
    /// `UNCACHED` entries). Defensive: a stray `UNCACHED` slot is
    /// preserved verbatim and excluded from minimisation. After
    /// successful minimisation:
    ///
    /// - `self.states` shrinks to one entry per equivalence class.
    /// - `self.transitions` is rebuilt with mapped targets.
    /// - The start state (originally ID `0`) remains ID `0`.
    /// - `self.cache` is cleared (stale post-merge); the
    ///   materialised flat table is now the source of truth.
    ///
    /// Use cases where minimisation pays back its construction cost:
    /// - Patterns whose materialisation is just below the state
    ///   limit cap — minimisation may bring them under by enough
    ///   margin that they fit comfortably in L1 / L2.
    /// - Patterns with redundant alternation branches that the
    ///   forward subset construction can't pre-collapse.
    ///
    /// For patterns whose materialised DFA is already small (under
    /// ~30 states), minimisation typically removes 1-5 states and
    /// the runtime impact is below measurement noise. Called
    /// unconditionally because the construction cost is one-time
    /// and small.
    pub fn minimize(&mut self) {
        let n = self.states.len();
        if n <= 1 {
            return;
        }
        let num_classes = self.num_classes;

        // Sentinel partition ID for the virtual "dead state".
        // Distinct from any real partition; chosen as u32::MAX so it
        // sorts predictably in signature comparisons.
        const DEAD_PARTITION: u32 = u32::MAX;

        // Initial partition by accept-flag triple. States with
        // different combos can never be equivalent — they observe
        // different accept behaviour under different word-boundary
        // contexts.
        let mut partition: Vec<u32> = vec![0; n];
        {
            let mut sig_to_id: HashMap<(bool, bool, bool), u32> = HashMap::new();
            for s in 0..n {
                let state = &self.states[s];
                let key = (
                    state.is_accept,
                    state.accept_when_fire_wb,
                    state.accept_when_not_fire_wb,
                );
                let next_id = sig_to_id.len() as u32;
                let id = *sig_to_id.entry(key).or_insert(next_id);
                partition[s] = id;
            }
        }

        // Moore iteration: refine the partition until stable. Each
        // iteration computes, for each state, a signature
        // (current_partition, transition_targets_in_partitions). States
        // with the same signature stay together; differing signatures
        // split their parent partition.
        loop {
            let mut new_partition: Vec<u32> = vec![0; n];
            let mut sig_to_new_id: HashMap<Vec<u32>, u32> = HashMap::new();
            for s in 0..n {
                let mut sig: Vec<u32> = Vec::with_capacity(num_classes + 1);
                sig.push(partition[s]);
                for cls in 0..num_classes {
                    let trans_idx = s * num_classes + cls;
                    let target = self.transitions[trans_idx];
                    let target_partition = if target == DEAD_STATE || target == UNCACHED {
                        DEAD_PARTITION
                    } else {
                        partition[target as usize]
                    };
                    sig.push(target_partition);
                }
                let next_id = sig_to_new_id.len() as u32;
                let id = *sig_to_new_id.entry(sig).or_insert(next_id);
                new_partition[s] = id;
            }
            if new_partition == partition {
                break;
            }
            partition = new_partition;
        }

        // Critical invariant: states 0 and 1 are both start states
        // (state 0 for pw=false context, state 1 for pw=true context).
        // The simulator's `start_state_for(input, start)` returns
        // either 0 or 1 unconditionally, so both slots MUST exist in
        // the minimised DFA even when behaviourally equivalent
        // (which is the case for every non-WB pattern — both states
        // have the same accept flags, same transitions, and end up
        // in the same Moore partition).
        //
        // Solution: always preserve two start-state slots verbatim,
        // and let the encounter-order loop fill in the rest.
        let final_count = {
            let distinct_partitions = (partition.iter().max().copied().unwrap_or(0) as usize) + 1;
            // We always allocate at least `min(n, 2)` start-state
            // slots. The remaining partitions add their own slots
            // only if they're not already pinned to slot 0 or 1.
            let pinned = n.min(2);
            let mut counted_partitions: std::collections::HashSet<u32> =
                std::collections::HashSet::new();
            counted_partitions.insert(partition[0]);
            if n >= 2 {
                counted_partitions.insert(partition[1]);
            }
            let extra_partitions = distinct_partitions.saturating_sub(counted_partitions.len());
            pinned + extra_partitions
        };
        if final_count >= n {
            return;
        }

        // Build the partition → new-state-ID mapping. Slots 0 and 1
        // are pinned to the original start states (regardless of
        // their partition). Other partitions populate in encounter
        // order from slot 2 upward.
        let mut partition_to_new_id: HashMap<u32, DfaStateId> = HashMap::new();
        let mut new_states: Vec<DfaState> = Vec::with_capacity(final_count);

        // Slot 0: state 0 (pw=false start).
        let start_partition = partition[0];
        partition_to_new_id.insert(start_partition, 0);
        new_states.push(self.states[0].clone());

        // Slot 1: state 1 (pw=true start), if it exists. Note we
        // ONLY add a partition_to_new_id entry for state 1's
        // partition if it differs from state 0's — otherwise the
        // partition is already mapped to slot 0 and we just need
        // the state slot to exist for start_state_for's contract.
        if n >= 2 {
            let state1_partition = partition[1];
            new_states.push(self.states[1].clone());
            if state1_partition != start_partition {
                partition_to_new_id.insert(state1_partition, 1);
            }
        }

        // Remaining partitions get IDs in encounter order.
        for s in 0..n {
            let p = partition[s];
            if let std::collections::hash_map::Entry::Vacant(e) = partition_to_new_id.entry(p) {
                let new_id = new_states.len() as DfaStateId;
                e.insert(new_id);
                new_states.push(self.states[s].clone());
            }
        }

        // Rebuild transitions. For old state 0 → new slot 0. For
        // old state 1 → new slot 1 (always, even if partition[1] ==
        // partition[0] — preserving the start-state contract). All
        // other old states map via partition_to_new_id.
        let actual_final_count = new_states.len();
        let mut new_transitions: Vec<DfaStateId> =
            vec![DEAD_STATE; actual_final_count * num_classes];
        for old_s in 0..n {
            let new_s: DfaStateId = if old_s == 0 {
                0
            } else if old_s == 1 {
                1
            } else {
                partition_to_new_id[&partition[old_s]]
            };
            for cls in 0..num_classes {
                let old_idx = old_s * num_classes + cls;
                let old_target = self.transitions[old_idx];
                let new_target = if old_target == DEAD_STATE {
                    DEAD_STATE
                } else if old_target == UNCACHED {
                    UNCACHED
                } else {
                    partition_to_new_id[&partition[old_target as usize]]
                };
                let new_idx = (new_s as usize) * num_classes + cls;
                new_transitions[new_idx] = new_target;
            }
        }

        self.states = new_states;
        self.transitions = new_transitions;
        // Cache holds state IDs from the pre-minimisation DFA;
        // discard. Post-materialisation, the cache is unused — the
        // flat transition table is the source of truth.
        self.cache.clear();
    }

    /// Immutable companion to [`Self::transition`]. Reads only the
    /// pre-filled cache; returns `Exhausted` if it hits an UNCACHED
    /// slot (which means the caller needs the mutable variant to
    /// allocate fresh state). Designed for use after a successful
    /// [`Self::try_materialize`] where every reachable transition
    /// is guaranteed to be cached.
    #[inline]
    fn transition_immut(&self, state: DfaStateId, cls: u8) -> TransitionResult {
        if state == DEAD_STATE {
            return TransitionResult::Dead;
        }
        let trans_idx = (state as usize) * self.num_classes + (cls as usize);
        let cached = self.transitions[trans_idx];
        if cached < DEAD_STATE {
            return TransitionResult::Next(cached);
        }
        if cached == DEAD_STATE {
            return TransitionResult::Dead;
        }
        // UNCACHED — fall back to mutable path.
        TransitionResult::Exhausted
    }

    fn transition(&mut self, state: DfaStateId, cls: u8) -> TransitionResult {
        if state == DEAD_STATE {
            return TransitionResult::Dead;
        }
        let trans_idx = (state as usize) * self.num_classes + (cls as usize);
        let cached = self.transitions[trans_idx];
        // Real DFA state IDs are 0..states.len() — strictly less than
        // both sentinels (`DEAD_STATE = u32::MAX - 1`, `UNCACHED = u32::MAX`).
        // The fast path is one compare + branch, identical in cost to the
        // previous shape; the difference is what falls through.
        if cached < DEAD_STATE {
            return TransitionResult::Next(cached);
        }
        if cached == DEAD_STATE {
            // Cached-dead — return immediately without recomputing.
            // Without this check, dead transitions hit `compute_transition_set`
            // on every lookup; instrumentation 2026-04-27 measured ~6K
            // recomputations per `capture_groups.find_all` call.
            return TransitionResult::Dead;
        }
        debug_assert_eq!(cached, UNCACHED);
        // Uncached. Compute the next NFA state set by following byte
        // transitions for `cls` from every NFA state in the source
        // DFA state, then epsilon-closing the targets.
        let next_set = self.compute_transition_set(state, cls);
        if next_set.is_empty() {
            // Genuinely dead — record DEAD_STATE in the transition
            // table so future lookups short-circuit at the dead-cache
            // check above instead of recomputing.
            self.transitions[trans_idx] = DEAD_STATE;
            return TransitionResult::Dead;
        }
        // Look up or allocate the target DFA state. The target's
        // `prev_byte_was_word` is determined by the byte class we
        // just transitioned on — every byte in the class shares the
        // same word-ness because `Regex::WordBoundary` contributes
        // the word-byte oracle in `byte_class.rs`, ensuring the
        // partition never mixes word and non-word bytes within one
        // class.
        let target_pw = self.byte_class_map.class_is_word(cls);
        let key = DfaStateKey {
            nfa_states: next_set.clone(),
            prev_byte_was_word: target_pw,
        };
        let target_id = if let Some(&id) = self.cache.get(&key) {
            id
        } else {
            if self.states.len() >= self.state_limit {
                // Cache full. Don't allocate, signal fallback.
                return TransitionResult::Exhausted;
            }
            self.allocate_state(next_set, target_pw)
        };
        self.transitions[trans_idx] = target_id;
        TransitionResult::Next(target_id)
    }

    /// Run the DFA simulator BACKWARD over `input` starting just
    /// before byte position `end` and walking toward byte 0.
    ///
    /// Used by the **reverse-DFA pipeline** (the C2 follow-up
    /// optimization sketched in the C2 chapter): once the forward
    /// DFA has found the END position of a match, this method walks
    /// the reverse-anchored DFA backward from that end to find the
    /// START position of the match. The combined forward + reverse
    /// pass replaces the per-position scan loop with a single
    /// O(n) sweep.
    ///
    /// The DFA must be built from a **reverse-anchored** NFA — i.e.,
    /// constructed via `LazyDfa::new(Arc::new(c2.reverse_anchored.clone()), ...)`.
    /// Calling this method on a forward DFA produces meaningless
    /// results (the byte order assumption is wrong). The caller is
    /// responsible for using the right DFA.
    ///
    /// On `Match(start)`, `start` is the START byte position of the
    /// LEFTMOST match in the forward direction. The reverse DFA
    /// records the latest accept seen during the backward walk;
    /// because the input bytes are consumed in reverse order, the
    /// "latest accept" corresponds to the smallest forward index,
    /// which is the leftmost match start.
    ///
    /// On `NoMatch`, no leftmost-start was found in the input prefix
    /// (which would indicate a bug — the forward DFA should not have
    /// signaled a match if the reverse DFA can't find the start).
    ///
    /// On `Exhausted`, the cache filled up before the walk completed
    /// and the caller should fall back to the Pike-VM.
    ///
    /// **Status:** foundation only. The dispatch path that consumes
    /// this method lands in a follow-up commit per the
    /// `docs/BACKLOG.md` "C2 follow-up: reverse-DFA pipeline" entry.
    pub fn find_match_start_at_reverse(&mut self, input: &[u8], end: usize) -> DfaSearchOutcome {
        self.find_match_start_at_reverse_bounded(input, end, 0)
    }

    /// Bounded variant of [`Self::find_match_start_at_reverse`].
    ///
    /// Walks the reverse-anchored DFA backward from `end`, but stops
    /// when `pos == min_start`. The returned `Match(start)` therefore
    /// satisfies `start >= min_start`.
    ///
    /// Used by the reverse-DFA pipeline's `find_all` driver: after a
    /// previous match ends at `prev_end`, the next leftmost match
    /// must start at `>= prev_end` (non-overlapping). Passing
    /// `min_start = prev_end` prevents the backward walk from
    /// reporting a start that overlaps the already-consumed span.
    ///
    /// `min_start = 0` recovers the original contract.
    pub fn find_match_start_at_reverse_bounded(
        &mut self,
        input: &[u8],
        end: usize,
        min_start: usize,
    ) -> DfaSearchOutcome {
        debug_assert!(end <= input.len(), "end out of bounds for input");
        debug_assert!(min_start <= end, "min_start must not exceed end");
        // Reverse walk: state.pw = is_word(byte just consumed in
        // walk direction) = is_word(input[pos]) when at position pos.
        // At the start of the walk no byte has been consumed; the
        // implicit "previous byte in walk direction" is `input[end]`
        // (the byte to the right of the start position textually).
        let mut state = self.start_state_for_reverse(input, end);
        // The DFA could already accept the empty span at the end
        // position. Use the context-aware accept check so
        // `\b`-bearing patterns whose accept is only reachable via
        // a satisfied WordBoundary epsilon are recognised here.
        // At position `end`, the boundary check needs:
        //   pw = state.pw  (set above from input[end])
        //   cw = is_word(input[end - 1])  (byte to the LEFT — the
        //                                  next byte in reverse walk)
        // or false at start-of-input.
        let cw_at_end = if end == 0 {
            false
        } else {
            Self::word_ness_at(input, end - 1)
        };
        let mut leftmost_start = if end >= min_start
            && self.is_accept_with_word_boundary_context(
                state,
                self.states[state as usize].prev_byte_was_word,
                cw_at_end,
            ) {
            Some(end)
        } else {
            None
        };
        let mut pos = end;
        while pos > min_start {
            pos -= 1;
            let byte = input[pos];
            let cls = self.byte_class_map.class_of(byte);
            match self.transition(state, cls) {
                TransitionResult::Next(next_state) => {
                    state = next_state;
                    // Post-transition we're at position `pos`. For
                    // the boundary at this textual position the
                    // operands are:
                    //   pw = state.pw = is_word(input[pos])  (byte
                    //        just consumed in walk direction — set
                    //        by `transition` via class_is_word(cls))
                    //   cw = is_word(input[pos - 1])  (next byte in
                    //        walk direction; non-word at pos == 0)
                    let pw = self.states[state as usize].prev_byte_was_word;
                    let cw = if pos == 0 {
                        false
                    } else {
                        Self::word_ness_at(input, pos - 1)
                    };
                    if self.is_accept_with_word_boundary_context(state, pw, cw) {
                        leftmost_start = Some(pos);
                    }
                }
                TransitionResult::Dead => break,
                TransitionResult::Exhausted => {
                    return DfaSearchOutcome::Exhausted;
                }
            }
        }
        match leftmost_start {
            Some(start) => DfaSearchOutcome::Match(start),
            None => DfaSearchOutcome::NoMatch,
        }
    }

    /// Run the DFA simulator over `input` starting at byte position
    /// `start` and return **at the first accept state reached**.
    ///
    /// Used by the reverse-DFA pipeline for `find_first`: when built
    /// over the forward-unanchored NFA, the earliest accept during
    /// the forward walk corresponds to the END of the leftmost
    /// match (modulo the pending greedy-extension pass via the
    /// forward-anchored DFA). This contract is narrower than
    /// [`Self::find_match_at`], which walks to exhaustion to compute
    /// leftmost-longest semantics.
    ///
    /// Distinctions vs `find_match_at`:
    ///
    /// - Returns the END byte position of the **first** accept, not
    ///   the **latest** accept. For patterns like `\w\w` on `"abc"`
    ///   this is end=2 (first match "ab") rather than end=3 (extended
    ///   to "bc").
    /// - If the DFA's start state is already accepting (pattern
    ///   accepts empty), returns `Match(start)` immediately without
    ///   consuming any input.
    ///
    /// `NoMatch` and `Exhausted` semantics match `find_match_at`.
    pub fn find_first_accept_at(&mut self, input: &[u8], start: usize) -> DfaSearchOutcome {
        let mut state = self.start_state_for(input, start);
        let start_pw = self.states[state as usize].prev_byte_was_word;
        if self.is_accept_with_word_boundary_context(
            state,
            start_pw,
            Self::word_ness_at(input, start),
        ) {
            return DfaSearchOutcome::Match(start);
        }
        let mut pos = start;
        while pos < input.len() {
            let byte = input[pos];
            let cls = self.byte_class_map.class_of(byte);
            match self.transition(state, cls) {
                TransitionResult::Next(next_state) => {
                    state = next_state;
                    pos += 1;
                    let pw = self.states[state as usize].prev_byte_was_word;
                    let cw = Self::word_ness_at(input, pos);
                    if self.is_accept_with_word_boundary_context(state, pw, cw) {
                        return DfaSearchOutcome::Match(pos);
                    }
                }
                TransitionResult::Dead => break,
                TransitionResult::Exhausted => {
                    return DfaSearchOutcome::Exhausted;
                }
            }
        }
        DfaSearchOutcome::NoMatch
    }

    /// Run the DFA simulator over `input` starting at byte position
    /// `start`. Returns a [`DfaSearchOutcome`] distinguishing match,
    /// no-match, and cache-exhausted cases.
    ///
    /// On `Match(end)`, `end` is the END byte position of the longest
    /// match the simulator found at `start`. On `Exhausted`, the
    /// caller (engine dispatch) should fall back to the Pike-VM for
    /// this match attempt — the DFA can't give a definitive answer.
    ///
    /// Mirrors the contract of `c2::pike::pike_match_at` plus the
    /// exhaustion signal.
    pub fn find_match_at(&mut self, input: &[u8], start: usize) -> DfaSearchOutcome {
        // Pick the start state that matches the previous-byte context
        // at `start` — state 0 (pw=false) at position 0 of input or
        // after a non-word byte, state 1 (pw=true) after a word byte.
        // The choice determines how `\b` evaluates at this position.
        let mut state = self.start_state_for(input, start);
        // The DFA could already accept the empty string at `start`
        // (e.g., for patterns like `a*`). Use the context-aware
        // accept check so `\b`-bearing patterns whose accept is only
        // reachable via a satisfied WordBoundary epsilon are
        // recognised here too.
        let start_pw = self.states[state as usize].prev_byte_was_word;
        let mut matched_end = if self.is_accept_with_word_boundary_context(
            state,
            start_pw,
            Self::word_ness_at(input, start),
        ) {
            Some(start)
        } else {
            None
        };
        let mut pos = start;
        while pos < input.len() {
            let byte = input[pos];
            let cls = self.byte_class_map.class_of(byte);
            match self.transition(state, cls) {
                TransitionResult::Next(next_state) => {
                    state = next_state;
                    pos += 1;
                    let pw = self.states[state as usize].prev_byte_was_word;
                    let cw = Self::word_ness_at(input, pos);
                    if self.is_accept_with_word_boundary_context(state, pw, cw) {
                        matched_end = Some(pos);
                    }
                }
                TransitionResult::Dead => break,
                TransitionResult::Exhausted => {
                    return DfaSearchOutcome::Exhausted;
                }
            }
        }
        match matched_end {
            Some(end) => DfaSearchOutcome::Match(end),
            None => DfaSearchOutcome::NoMatch,
        }
    }

    /// Immutable companion to [`Self::find_match_at`]. Uses
    /// [`Self::transition_immut`] so a `&self` reference suffices
    /// (no Mutex acquisition required at the call site). Returns
    /// `Exhausted` if it encounters an uncached transition — the
    /// caller should then fall back to the `&mut` variant.
    ///
    /// Intended for use after a successful [`Self::try_materialize`]
    /// where every reachable transition is guaranteed to be cached;
    /// the `Exhausted` return is then a defensive guard rather than
    /// the expected outcome.
    #[must_use]
    pub fn find_match_at_immut(&self, input: &[u8], start: usize) -> DfaSearchOutcome {
        let mut state = self.start_state_for(input, start);
        let start_pw = self.states[state as usize].prev_byte_was_word;
        let mut matched_end = if self.is_accept_with_word_boundary_context(
            state,
            start_pw,
            Self::word_ness_at(input, start),
        ) {
            Some(start)
        } else {
            None
        };
        let mut pos = start;
        while pos < input.len() {
            let byte = input[pos];
            let cls = self.byte_class_map.class_of(byte);
            match self.transition_immut(state, cls) {
                TransitionResult::Next(next_state) => {
                    state = next_state;
                    pos += 1;
                    let pw = self.states[state as usize].prev_byte_was_word;
                    let cw = Self::word_ness_at(input, pos);
                    if self.is_accept_with_word_boundary_context(state, pw, cw) {
                        matched_end = Some(pos);
                    }
                }
                TransitionResult::Dead => break,
                TransitionResult::Exhausted => return DfaSearchOutcome::Exhausted,
            }
        }
        match matched_end {
            Some(end) => DfaSearchOutcome::Match(end),
            None => DfaSearchOutcome::NoMatch,
        }
    }

    // ============================================================
    // Internals
    // ============================================================

    /// Allocate a fresh DFA state for the given NFA state set and
    /// register it in the cache. Returns the new state's ID. Grows the
    /// flat `transitions` table by `num_classes` `DEAD_STATE` slots so
    /// the new state's row is fully addressable.
    fn allocate_state(
        &mut self,
        nfa_states: Vec<NfaStateId>,
        prev_byte_was_word: bool,
    ) -> DfaStateId {
        let id = self.states.len() as DfaStateId;
        let accept = self.nfa.accept();
        let is_accept = nfa_states.contains(&accept);
        // Pre-compute acceptance for both word-boundary contexts so
        // the simulator's hot loop doesn't pay per-byte epsilon
        // closure cost on `\b` patterns. For NFAs without word-
        // boundary edges this is a no-op (the closure ignores all
        // assertion edges and the bool collapses to `is_accept`).
        let (accept_when_fire_wb, accept_when_not_fire_wb) = if self
            .nfa
            .has_word_boundary_assertions()
        {
            let mut accept_fire = is_accept;
            let mut accept_not_fire = is_accept;
            if !is_accept {
                // Re-expand once per direction, short-circuiting on
                // first acceptance hit.
                let mut visited = vec![false; self.nfa.num_states()];
                let mut expanded = Vec::new();
                for &n in &nfa_states {
                    self.epsilon_close_with_word_boundary(&mut expanded, &mut visited, n, true);
                    if expanded.contains(&accept) {
                        accept_fire = true;
                        break;
                    }
                }
                let mut visited = vec![false; self.nfa.num_states()];
                let mut expanded = Vec::new();
                for &n in &nfa_states {
                    self.epsilon_close_with_word_boundary(&mut expanded, &mut visited, n, false);
                    if expanded.contains(&accept) {
                        accept_not_fire = true;
                        break;
                    }
                }
            }
            (accept_fire, accept_not_fire)
        } else {
            (is_accept, is_accept)
        };
        self.states.push(DfaState {
            is_accept,
            accept_when_fire_wb,
            accept_when_not_fire_wb,
            nfa_states: nfa_states.clone(),
            prev_byte_was_word,
        });
        // Append a fresh row of UNCACHED entries for this state's
        // outgoing transitions. The flat-table invariant is
        // `transitions.len() == states.len() * num_classes`. UNCACHED
        // (distinct from DEAD_STATE) lets `transition()` distinguish
        // "never looked up" from "computed dead and cached" so dead
        // lookups don't recompute every call.
        self.transitions
            .resize(self.transitions.len() + self.num_classes, UNCACHED);
        let key = DfaStateKey {
            nfa_states,
            prev_byte_was_word,
        };
        self.cache.insert(key, id);
        id
    }

    /// Compute the start NFA state set: epsilon closure of the NFA's
    /// start state.
    ///
    /// For unanchored NFAs whose initial closure already contains the
    /// accept state (pattern accepts empty at position 0, e.g. `a*`),
    /// the closure is re-run from `nfa.body_entry()` with the lazy-
    /// prefix states excluded from traversal. This gives the body-only
    /// state set — the state the DFA should be in immediately after
    /// "finding" a zero-width leftmost match at position 0, ready to
    /// extend greedily if input allows.
    fn compute_start_set(&self) -> Vec<NfaStateId> {
        let mut set = Vec::new();
        let mut visited = vec![false; self.nfa.num_states()];
        self.epsilon_close(&mut set, &mut visited, self.nfa.start());
        set.sort_unstable();

        if self.nfa.lazy_prefix_states().is_empty() || !set.contains(&self.nfa.accept()) {
            return set;
        }
        let Some(body_entry) = self.nfa.body_entry() else {
            return set;
        };
        self.closure_excluding_lazy_prefix(&[body_entry])
    }

    /// Compute the next NFA state set for `(state, cls)`: for each NFA
    /// state in the source DFA state, follow byte transitions matching
    /// `cls`, then epsilon-close every reached target.
    ///
    /// If the first-pass closure contains the accept state (meaning
    /// the set includes a match that has just completed), the closure
    /// is re-computed from the byte-transition targets with the lazy-
    /// prefix states excluded from traversal. This removes any body
    /// states that were only reachable via the prefix's "spawn a new
    /// match attempt" edge (like `body_start` for `\d`, whose body
    /// has no internal back-edge); body states genuinely reachable via
    /// body-internal epsilon loops (like `body_start` in `a+`, reached
    /// via `body_mid → body_start` greedy loop) survive and let the
    /// DFA greedily extend the leftmost match. The net effect is
    /// leftmost-first-aware subset construction on the unanchored DFA.
    fn compute_transition_set(&self, state: DfaStateId, cls: u8) -> Vec<NfaStateId> {
        // For patterns containing `\b` / `\B`, the source state's
        // stored `nfa_states` is the closure WITHOUT word-boundary
        // expansion. We re-expand here with the current
        // (prev-byte-was-word, current-byte-is-word) context so any
        // states reachable only across a satisfied word boundary
        // become byte-transition candidates. For patterns with no
        // word-boundary edges, the re-expansion is a no-op — the
        // fast path uses the stored set directly.
        let needs_wb_expansion = self.nfa.has_word_boundary_assertions();
        let pw = self.states[state as usize].prev_byte_was_word;
        let cw = self.byte_class_map.class_is_word(cls);
        let fire_wb = pw != cw;

        let expanded_owned: Vec<NfaStateId>;
        let expanded_source: &[NfaStateId] = if needs_wb_expansion {
            let mut expanded = Vec::new();
            let mut visited = vec![false; self.nfa.num_states()];
            let snapshot = self.states[state as usize].nfa_states.clone();
            for s in snapshot {
                self.epsilon_close_with_word_boundary(&mut expanded, &mut visited, s, fire_wb);
            }
            expanded.sort_unstable();
            expanded_owned = expanded;
            &expanded_owned
        } else {
            &self.states[state as usize].nfa_states
        };

        let mut targets: Vec<NfaStateId> = Vec::new();
        for &nfa_state in expanded_source {
            let state_obj = &self.nfa.states()[nfa_state as usize];
            for &(transition_cls, target) in &state_obj.transitions {
                if transition_cls == cls {
                    targets.push(target);
                }
            }
        }
        // Target closure WITHOUT word-boundary expansion — the next
        // transition (or the runtime accept-check) re-expands based
        // on its own context.
        let mut next = Vec::new();
        let mut visited = vec![false; self.nfa.num_states()];
        for &t in &targets {
            self.epsilon_close(&mut next, &mut visited, t);
        }
        next.sort_unstable();

        if self.nfa.lazy_prefix_states().is_empty() || !next.contains(&self.nfa.accept()) {
            return next;
        }
        self.closure_excluding_lazy_prefix(&targets)
    }

    /// Leftmost-first helper: re-run the epsilon closure starting from
    /// `entries`, but refuse to traverse into or through any lazy-
    /// prefix state. Entries that are themselves lazy-prefix states
    /// are skipped. The result is the body-reachable subset of the
    /// standard closure — states that exist in the set for a reason
    /// other than "the lazy prefix spawned another match attempt".
    fn closure_excluding_lazy_prefix(&self, entries: &[NfaStateId]) -> Vec<NfaStateId> {
        let mut set = Vec::new();
        let mut visited = vec![false; self.nfa.num_states()];
        for &lp in self.nfa.lazy_prefix_states() {
            visited[lp as usize] = true;
        }
        for &entry in entries {
            if self.nfa.lazy_prefix_states().contains(&entry) {
                continue;
            }
            self.epsilon_close(&mut set, &mut visited, entry);
        }
        set.sort_unstable();
        set
    }

    /// Recursive epsilon closure starting from `state`. Adds every
    /// reachable NFA state (via non-WB epsilon edges) to `set` and
    /// marks them in `visited`. Capture tags are ignored.
    ///
    /// `WordBoundary` / `NotWordBoundary` epsilon edges are **deferred**
    /// — their traversal depends on the (prev-byte-was-word,
    /// current-byte-is-word) context which is only known at the
    /// transition / accept-check site. The companion
    /// [`Self::epsilon_close_with_word_boundary`] handles those edges
    /// with the context supplied. `LazyDfa::new` rejects NFAs with
    /// non-word-boundary assertions, so any non-WB assertion edge
    /// encountered here is a bug.
    fn epsilon_close(&self, set: &mut Vec<NfaStateId>, visited: &mut [bool], state: NfaStateId) {
        if visited[state as usize] {
            return;
        }
        visited[state as usize] = true;
        set.push(state);
        let state_obj = &self.nfa.states()[state as usize];
        for edge in &state_obj.epsilons {
            match edge.assertion {
                None => self.epsilon_close(set, visited, edge.target),
                Some(
                    crate::c2::nfa::ZeroWidthAssertion::WordBoundary
                    | crate::c2::nfa::ZeroWidthAssertion::NotWordBoundary,
                ) => {
                    // Deferred — evaluated at transition / accept-check time.
                }
                Some(_) => {
                    debug_assert!(
                        false,
                        "DFA construction rejects anchor / \\G assertions in LazyDfa::new"
                    );
                }
            }
        }
    }

    /// Word-boundary-aware variant of [`Self::epsilon_close`]. Adds
    /// every reachable NFA state to `set`, traversing `WordBoundary`
    /// edges iff `fire_wb` is `true` and `NotWordBoundary` edges iff
    /// `fire_wb` is `false`. Non-assertion epsilon edges are always
    /// traversed.
    ///
    /// `fire_wb = (prev_byte_was_word != current_byte_is_word)` — i.e.,
    /// `true` iff the current position is a word boundary.
    fn epsilon_close_with_word_boundary(
        &self,
        set: &mut Vec<NfaStateId>,
        visited: &mut [bool],
        state: NfaStateId,
        fire_wb: bool,
    ) {
        if visited[state as usize] {
            return;
        }
        visited[state as usize] = true;
        set.push(state);
        let state_obj = &self.nfa.states()[state as usize];
        for edge in &state_obj.epsilons {
            let should_follow = match edge.assertion {
                None => true,
                Some(crate::c2::nfa::ZeroWidthAssertion::WordBoundary) => fire_wb,
                Some(crate::c2::nfa::ZeroWidthAssertion::NotWordBoundary) => !fire_wb,
                Some(_) => false,
            };
            if should_follow {
                self.epsilon_close_with_word_boundary(set, visited, edge.target, fire_wb);
            }
        }
    }

    /// Context-aware accept check. Returns `true` if either the
    /// stored `is_accept` flag fires (accept reachable without any
    /// `WordBoundary` epsilon) or the accept is reachable through a
    /// satisfied `WordBoundary` edge given the current position
    /// context.
    ///
    /// `prev_byte_was_word` is the state's stored flag (set to the
    /// word-ness of the byte that put us into this state, or `false`
    /// for the start state). `current_byte_is_word` is the word-ness
    /// of the byte at the current position — i.e., the byte we're
    /// ABOUT to read, or `false` at end-of-input (end-of-input acts
    /// as a non-word byte for `\b` evaluation).
    ///
    /// For patterns with no word-boundary edges (the common case),
    /// the fast path returns the stored `is_accept` directly. Only
    /// `\b`-bearing patterns pay the per-position closure
    /// re-expansion.
    #[inline]
    fn is_accept_with_word_boundary_context(
        &self,
        state: DfaStateId,
        prev_byte_was_word: bool,
        current_byte_is_word: bool,
    ) -> bool {
        if state == DEAD_STATE {
            return false;
        }
        let s = &self.states[state as usize];
        let fire_wb = prev_byte_was_word != current_byte_is_word;
        // Both `accept_when_*` flags were precomputed at
        // `allocate_state` time. For NFAs without word-boundary
        // edges both flags equal `is_accept`, so this collapses to
        // the unconditional accept check. For `\b`-bearing patterns
        // the flag lookup replaces a per-byte epsilon closure
        // expansion (~10× speedup on `email_basic`-style workloads).
        if fire_wb {
            s.accept_when_fire_wb
        } else {
            s.accept_when_not_fire_wb
        }
    }

    /// Returns the word-ness of the byte at `pos` in `input`, or
    /// `false` at end-of-input (which acts as a non-word "byte" for
    /// `\b` evaluation per PCRE2 / Rust regex semantics).
    #[inline]
    fn word_ness_at(input: &[u8], pos: usize) -> bool {
        input
            .get(pos)
            .copied()
            .is_some_and(crate::c2::byte_class::is_ascii_word_byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c2::pike::pike_find_first;
    use crate::c2::program::CompiledC2Program;

    /// Build a `LazyDfa` from a pattern's forward anchored NFA. Skips
    /// patterns that contain assertions or that classify outside the
    /// C2 subset.
    fn try_build_dfa(pattern: &str) -> Option<(LazyDfa, CompiledC2Program)> {
        let prog = CompiledC2Program::try_compile(pattern)?;
        if prog.forward_anchored.has_assertions() {
            return None;
        }
        let nfa = Arc::new(prog.forward_anchored.clone());
        let bcm = Arc::new(prog.byte_class_map.clone());
        let dfa = LazyDfa::new(nfa, bcm, LazyDfa::DEFAULT_STATE_LIMIT).ok()?;
        Some((dfa, prog))
    }

    /// Run the DFA at every scan position in the input and return the
    /// leftmost match span, mirroring `pike_find_first`'s contract.
    /// Returns `None` for both no-match and exhaustion (tests use a
    /// large enough state limit that exhaustion shouldn't happen).
    fn dfa_find_first(dfa: &mut LazyDfa, input: &[u8]) -> Option<(usize, usize)> {
        for start in 0..=input.len() {
            match dfa.find_match_at(input, start) {
                DfaSearchOutcome::Match(end) => return Some((start, end)),
                DfaSearchOutcome::NoMatch => continue,
                DfaSearchOutcome::Exhausted => return None,
            }
        }
        None
    }

    /// Compare DFA result against Pike-VM result for a single
    /// `(pattern, input)` pair.
    fn assert_dfa_matches_pike(pattern: &str, input: &str) {
        let Some((mut dfa, prog)) = try_build_dfa(pattern) else {
            // Pattern outside C2 subset or contains assertions; nothing
            // to compare for this step.
            return;
        };
        let pike_result = pike_find_first(&prog, input.as_bytes());
        let dfa_result = dfa_find_first(&mut dfa, input.as_bytes());
        assert_eq!(
            dfa_result, pike_result,
            "DFA disagrees with Pike-VM on pattern '{pattern}' / input '{input}'"
        );
    }

    // ============================================================
    // Construction
    // ============================================================

    #[test]
    fn build_dfa_from_literal_pattern() {
        let (dfa, _) = try_build_dfa("a").expect("buildable");
        // Start state plus at least the accept state.
        assert!(dfa.num_states() >= 1);
    }

    #[test]
    fn refuses_construction_for_pattern_with_anchor() {
        let prog = CompiledC2Program::try_compile(r"\Aabc").expect("compilable");
        let nfa = Arc::new(prog.forward_anchored.clone());
        let bcm = Arc::new(prog.byte_class_map.clone());
        let result = LazyDfa::new(nfa, bcm, LazyDfa::DEFAULT_STATE_LIMIT);
        assert!(result.is_err(), "expected error for pattern with assertion");
    }

    // ============================================================
    // Basic matching
    // ============================================================

    #[test]
    fn literal_matches_at_start() {
        assert_dfa_matches_pike("hello", "hello");
    }

    #[test]
    fn literal_matches_in_middle() {
        assert_dfa_matches_pike("foo", "barfooz");
    }

    #[test]
    fn literal_no_match() {
        assert_dfa_matches_pike("xyz", "abc");
    }

    #[test]
    fn ascii_char_class_matches() {
        assert_dfa_matches_pike("[a-z]", "ABC123abc");
    }

    #[test]
    fn shorthand_digit_matches() {
        assert_dfa_matches_pike(r"\d", "abc7xy");
    }

    #[test]
    fn shorthand_word_matches() {
        assert_dfa_matches_pike(r"\w", "!?@_");
    }

    #[test]
    fn negated_class_matches() {
        assert_dfa_matches_pike(r"[^0-9]", "123x");
    }

    // ============================================================
    // Sequence and alternation
    // ============================================================

    #[test]
    fn sequence_matches() {
        assert_dfa_matches_pike("abc", "xxabcyy");
    }

    #[test]
    fn alternation_in_group_matches() {
        // Top-level alternation routes to VM (matched_branch_number),
        // so use a wrapping group to keep the alternation non-top-level.
        assert_dfa_matches_pike(r"(?:cat|dog|fish)x", "i love dogx");
    }

    // ============================================================
    // Quantifiers
    // ============================================================

    #[test]
    fn greedy_star_matches() {
        assert_dfa_matches_pike("a*", "baaab");
    }

    #[test]
    fn greedy_plus_matches() {
        assert_dfa_matches_pike("a+", "baaab");
    }

    #[test]
    fn lazy_quantifier_diverges_from_pike_by_design() {
        // The DFA's subset construction gives longest-match semantics
        // by construction; it cannot directly express the leftmost-
        // first / lazy semantics the Pike-VM honours via its priority
        // cutoff. For `a+?` on "baaab" the Pike-VM returns end=2
        // (lazy: shortest one-or-more match) but the DFA returns
        // end=4 (longest).
        //
        // C2 step 5b excludes lazy-quantifier patterns from DFA
        // dispatch via the eligibility check, so the divergence
        // never reaches the public `Regex` API. This test pins the
        // current DFA semantics so any future change shows up loudly.
        let (mut dfa, _) = try_build_dfa("a+?").expect("buildable");
        let dfa_result = dfa_find_first(&mut dfa, b"baaab");
        assert_eq!(
            dfa_result,
            Some((1, 4)),
            "DFA gives longest match for `a+?` (subset construction has no priority)"
        );
    }

    #[test]
    fn optional_matches() {
        assert_dfa_matches_pike("ab?c", "ac");
    }

    #[test]
    fn range_quantifier_exact() {
        assert_dfa_matches_pike(r"\d{4}", "year 2026 q2");
    }

    #[test]
    fn range_quantifier_with_max() {
        assert_dfa_matches_pike(r"\d{2,4}", "abc 12345 xyz");
    }

    // ============================================================
    // Realistic patterns
    // ============================================================

    #[test]
    fn iso_date_pattern() {
        assert_dfa_matches_pike(r"\d{4}-\d{2}-\d{2}", "today is 2026-04-10 ok");
    }

    #[test]
    fn email_like_pattern() {
        assert_dfa_matches_pike(
            r"[\w.+-]+@[\w-]+\.[\w.-]+",
            "contact: alice+test@example.com please",
        );
    }

    // ============================================================
    // Cache behavior
    // ============================================================

    #[test]
    fn transitions_are_cached_on_repeated_lookup() {
        let (mut dfa, _) = try_build_dfa("a").expect("buildable");
        // Run once to populate the cache.
        let _ = dfa.find_match_at(b"a", 0);
        let after_first = dfa.num_states();
        // Run again on the same input — no new states should be allocated.
        let _ = dfa.find_match_at(b"a", 0);
        let after_second = dfa.num_states();
        assert_eq!(
            after_first, after_second,
            "second run on same input shouldn't allocate new states"
        );
    }

    #[test]
    fn try_materialize_succeeds_for_small_pattern() {
        let (mut dfa, _) = try_build_dfa(r"\d{3}-\d{2}-\d{4}").expect("buildable");
        assert!(dfa.try_materialize(64));
        // After materialisation the find_match_at_immut variant
        // should produce identical results to the mutable version.
        let input = b"123-45-6789";
        let mut mutable = try_build_dfa(r"\d{3}-\d{2}-\d{4}").unwrap().0;
        let _ = mutable.try_materialize(64);
        assert_eq!(
            dfa.find_match_at_immut(input, 0),
            mutable.find_match_at(input, 0)
        );
    }

    #[test]
    fn try_materialize_reports_failure_under_state_limit() {
        let (mut dfa, _) = try_build_dfa(r"\d{3}-\d{2}-\d{4}").expect("buildable");
        // 2 is far below the DFA's actual reachable-state count, so
        // BFS hits the limit before completion.
        assert!(!dfa.try_materialize(2));
    }

    #[test]
    fn find_match_at_immut_returns_exhausted_on_uncached() {
        // Without materialising first, the cache is empty (only the
        // start state populated). The first transition the immut
        // walker tries hits UNCACHED and returns Exhausted.
        let (dfa, _) = try_build_dfa(r"\d{3}-\d{2}-\d{4}").expect("buildable");
        let result = dfa.find_match_at_immut(b"123-45-6789", 0);
        assert_eq!(result, DfaSearchOutcome::Exhausted);
    }

    #[test]
    fn dfa_search_outcome_match_variant() {
        let (mut dfa, _) = try_build_dfa("ab").expect("buildable");
        let result = dfa.find_match_at(b"ab", 0);
        assert_eq!(result, DfaSearchOutcome::Match(2));
    }

    #[test]
    fn dfa_search_outcome_no_match_variant() {
        let (mut dfa, _) = try_build_dfa("ab").expect("buildable");
        let result = dfa.find_match_at(b"xy", 0);
        assert_eq!(result, DfaSearchOutcome::NoMatch);
    }

    #[test]
    fn cache_exhaustion_signals_fallback() {
        // Build a DFA with state_limit=1 so any new state allocation
        // beyond the start state fails.
        let prog = CompiledC2Program::try_compile("abc").expect("compilable");
        if prog.forward_anchored.has_assertions() {
            return;
        }
        let nfa = Arc::new(prog.forward_anchored.clone());
        let bcm = Arc::new(prog.byte_class_map.clone());
        let mut dfa = LazyDfa::new(nfa, bcm, 1).expect("buildable");
        // After construction, only the start state exists. Trying to
        // run the simulator on input "abc" should hit cache exhaustion
        // immediately when the first byte transition tries to allocate
        // a new state. The simulator returns Exhausted, signalling the
        // caller (engine dispatch in step 5b) to fall back to Pike-VM.
        let result = dfa.find_match_at(b"abc", 0);
        assert_eq!(result, DfaSearchOutcome::Exhausted);
    }

    // ============================================================
    // Find-first via scan
    // ============================================================

    #[test]
    fn find_first_via_scan_matches_pike() {
        assert_dfa_matches_pike(r"\d+", "abc 12345 xyz");
        assert_dfa_matches_pike(r"[a-z]+", "ABC abc XYZ");
    }

    // ============================================================
    // Reverse-DFA pipeline foundation (C2 follow-up)
    // ============================================================
    //
    // The reverse-DFA pipeline replaces the per-position scan loop
    // with a single forward-then-reverse sweep:
    //   1. forward DFA finds the END of the leftmost match
    //   2. reverse-anchored DFA walks backward from that end and
    //      finds the START of the leftmost match
    //   3. Pike-VM is then run bounded over [start, end] to recover
    //      capture groups
    //
    // The methods on `LazyDfa` (`find_match_at` for the forward
    // walk, `find_match_start_at_reverse` for the backward walk)
    // are the foundation. The dispatch wiring in `engine.rs` lands
    // in a follow-up commit per `docs/BACKLOG.md`.
    //
    // These tests pin the contract of `find_match_start_at_reverse`:
    // given a forward end position, it returns the leftmost start
    // such that the reverse-anchored DFA accepts the slice walked
    // backward.

    /// Build a reverse-anchored DFA from `pattern`. Returns `None`
    /// if the pattern is outside the C2 subset or if the reverse
    /// NFA contains assertions (the existing eligibility check).
    fn try_build_reverse_dfa(pattern: &str) -> Option<(LazyDfa, CompiledC2Program)> {
        let prog = CompiledC2Program::try_compile(pattern)?;
        if prog.reverse_anchored.has_assertions() {
            return None;
        }
        let nfa = Arc::new(prog.reverse_anchored.clone());
        let bcm = Arc::new(prog.byte_class_map.clone());
        let dfa = LazyDfa::new(nfa, bcm, LazyDfa::DEFAULT_STATE_LIMIT).ok()?;
        Some((dfa, prog))
    }

    #[test]
    fn reverse_dfa_builds_for_literal_pattern() {
        let (dfa, _) = try_build_reverse_dfa("abc").expect("buildable");
        assert!(dfa.num_states() >= 1);
    }

    #[test]
    fn reverse_dfa_finds_start_of_literal_match() {
        // Pattern "abc" against "xyzabc" — forward end is 6, reverse
        // walk should find the start at 3.
        let (mut dfa, _) = try_build_reverse_dfa("abc").expect("buildable");
        let outcome = dfa.find_match_start_at_reverse(b"xyzabc", 6);
        assert_eq!(outcome, DfaSearchOutcome::Match(3));
    }

    #[test]
    fn reverse_dfa_finds_start_of_match_at_input_start() {
        // Pattern "abc" against "abcdef" — forward end is 3, reverse
        // walk should find the start at 0.
        let (mut dfa, _) = try_build_reverse_dfa("abc").expect("buildable");
        let outcome = dfa.find_match_start_at_reverse(b"abcdef", 3);
        assert_eq!(outcome, DfaSearchOutcome::Match(0));
    }

    #[test]
    fn reverse_dfa_finds_start_of_char_class_match() {
        // Pattern "[a-z]+" against "ABC123abcXYZ" — forward end is 9
        // (after "abc"), reverse walk should find the start at 6.
        let (mut dfa, _) = try_build_reverse_dfa(r"[a-z]+").expect("buildable");
        let outcome = dfa.find_match_start_at_reverse(b"ABC123abcXYZ", 9);
        assert_eq!(outcome, DfaSearchOutcome::Match(6));
    }

    #[test]
    fn reverse_dfa_finds_leftmost_start_for_repeated_pattern() {
        // Pattern "a+" against "bbaaa" — forward end is 5 (the full
        // run "aaa"), reverse walk should find the start at 2.
        let (mut dfa, _) = try_build_reverse_dfa(r"a+").expect("buildable");
        let outcome = dfa.find_match_start_at_reverse(b"bbaaa", 5);
        assert_eq!(outcome, DfaSearchOutcome::Match(2));
    }

    #[test]
    fn reverse_dfa_handles_full_input_match() {
        // Pattern "[a-z]+" against "abcdef" — forward end is 6,
        // reverse walk should find the start at 0.
        let (mut dfa, _) = try_build_reverse_dfa(r"[a-z]+").expect("buildable");
        let outcome = dfa.find_match_start_at_reverse(b"abcdef", 6);
        assert_eq!(outcome, DfaSearchOutcome::Match(0));
    }

    #[test]
    fn reverse_dfa_no_match_when_no_pattern_in_prefix() {
        // Pattern "abc" walked backward from end=2 in input "abcdef"
        // — only the prefix "ab" is visible, the reverse pattern
        // (which is also "cba" in NFA terms) cannot find an accept.
        let (mut dfa, _) = try_build_reverse_dfa("abc").expect("buildable");
        let outcome = dfa.find_match_start_at_reverse(b"abcdef", 2);
        assert_eq!(outcome, DfaSearchOutcome::NoMatch);
    }

    #[test]
    fn reverse_dfa_finds_start_for_quantified_class_pattern() {
        // Pattern "\d+" against "abc12345xyz" — forward end is 8,
        // reverse walk should find the start at 3.
        let (mut dfa, _) = try_build_reverse_dfa(r"\d+").expect("buildable");
        let outcome = dfa.find_match_start_at_reverse(b"abc12345xyz", 8);
        assert_eq!(outcome, DfaSearchOutcome::Match(3));
    }

    // ============================================================
    // Forward-unanchored DFA: leftmost-first subset construction
    // ============================================================
    //
    // These tests pin the behaviour unlocked by tagging the
    // unanchoring lazy prefix `(?s:.)*?` on the NFA and pruning
    // those states from any DFA state set that also contains the
    // accept state. Without the prune, `find_match_at(input, 0)`
    // on the forward-unanchored DFA returns the END of the LAST
    // accept reached during the walk (leftmost-longest). With it,
    // the walk stops spawning new match attempts after the first
    // accept, so it returns the leftmost-first end — the
    // precondition for wiring `find_first` / `find_all` onto the
    // reverse-DFA pipeline.

    fn try_build_forward_unanchored_dfa(pattern: &str) -> Option<(LazyDfa, CompiledC2Program)> {
        let prog = CompiledC2Program::try_compile(pattern)?;
        if prog.forward_unanchored.has_assertions() {
            return None;
        }
        let nfa = Arc::new(prog.forward_unanchored.clone());
        let bcm = Arc::new(prog.byte_class_map.clone());
        let dfa = LazyDfa::new(nfa, bcm, LazyDfa::DEFAULT_STATE_LIMIT).ok()?;
        Some((dfa, prog))
    }

    #[test]
    fn forward_unanchored_dfa_returns_leftmost_first_end_for_repeated_literal() {
        // Pattern `a` against "xaxa" — the pre-prune DFA returns
        // end=4 (the LAST 'a'); leftmost-first says end=2.
        let (mut dfa, _) = try_build_forward_unanchored_dfa("a").expect("buildable");
        let outcome = dfa.find_match_at(b"xaxa", 0);
        assert_eq!(outcome, DfaSearchOutcome::Match(2));
    }

    #[test]
    fn forward_unanchored_dfa_returns_end_of_first_match_for_digits() {
        // Pattern `\d+` against "abc12xy45" — leftmost-first is
        // end=5 (after the first run "12"), not end=9.
        let (mut dfa, _) = try_build_forward_unanchored_dfa(r"\d+").expect("buildable");
        let outcome = dfa.find_match_at(b"abc12xy45", 0);
        assert_eq!(outcome, DfaSearchOutcome::Match(5));
    }

    #[test]
    fn forward_unanchored_dfa_greedy_star_reports_extended_end() {
        // Pattern `a*` against "xaaax" — leftmost-first start is
        // position 0 with the empty match, so end=0. The DFA is
        // accepting at the start state (a* accepts empty), and
        // pruning the lazy prefix means the first byte 'x' kills
        // the walk without advancing. end=0 is correct.
        let (mut dfa, _) = try_build_forward_unanchored_dfa(r"a*").expect("buildable");
        let outcome = dfa.find_match_at(b"xaaax", 0);
        assert_eq!(outcome, DfaSearchOutcome::Match(0));
    }

    #[test]
    fn forward_unanchored_dfa_empty_input_with_optional_pattern() {
        // Pattern `a?` against "" — empty match at position 0.
        let (mut dfa, _) = try_build_forward_unanchored_dfa(r"a?").expect("buildable");
        let outcome = dfa.find_match_at(b"", 0);
        assert_eq!(outcome, DfaSearchOutcome::Match(0));
    }

    #[test]
    fn forward_unanchored_dfa_no_match_returns_no_match() {
        let (mut dfa, _) = try_build_forward_unanchored_dfa("abc").expect("buildable");
        let outcome = dfa.find_match_at(b"xyz", 0);
        assert_eq!(outcome, DfaSearchOutcome::NoMatch);
    }

    // ============================================================
    // find_first_accept_at — leftmost-first end for the pipeline
    // ============================================================

    #[test]
    fn find_first_accept_at_returns_end_of_leftmost_match() {
        // `\w\w` on "abc": first accept after "ab" at end=2. The
        // broader `find_match_at` would extend to end=3 because body
        // states that still match word chars survive in the DFA state
        // set; `find_first_accept_at` cuts off the walk as soon as any
        // accept is seen.
        let (mut dfa, _) = try_build_forward_unanchored_dfa(r"\w\w").expect("buildable");
        let outcome = dfa.find_first_accept_at(b"abc", 0);
        assert_eq!(outcome, DfaSearchOutcome::Match(2));
    }

    #[test]
    fn find_first_accept_at_returns_start_for_empty_matcher() {
        // `a?` accepts empty at position 0.
        let (mut dfa, _) = try_build_forward_unanchored_dfa(r"a?").expect("buildable");
        let outcome = dfa.find_first_accept_at(b"xyz", 0);
        assert_eq!(outcome, DfaSearchOutcome::Match(0));
    }

    #[test]
    fn find_first_accept_at_returns_first_position_not_greedy_end() {
        // `a+` on "baaab": first accept after single 'a' at end=2.
        // The greedy extension to end=4 is the anchored DFA's job in
        // step 3 of the pipeline.
        let (mut dfa, _) = try_build_forward_unanchored_dfa(r"a+").expect("buildable");
        let outcome = dfa.find_first_accept_at(b"baaab", 0);
        assert_eq!(outcome, DfaSearchOutcome::Match(2));
    }

    #[test]
    fn find_first_accept_at_handles_no_match() {
        let (mut dfa, _) = try_build_forward_unanchored_dfa(r"\d").expect("buildable");
        let outcome = dfa.find_first_accept_at(b"abc", 0);
        assert_eq!(outcome, DfaSearchOutcome::NoMatch);
    }

    #[test]
    fn reverse_dfa_finds_zero_width_match_at_end() {
        // Pattern "a*" against "bbb" — the empty match span at
        // any position is valid. From end=3 the reverse walk
        // should find the leftmost-acceptable start, which for
        // "a*" against "bbb" is... well, walking backward from
        // pos 3, the DFA accepts the empty string immediately
        // (zero a's), and any 'b' byte transitions to dead. So
        // leftmost start = 3 (the zero-width match at end).
        let (mut dfa, _) = try_build_reverse_dfa(r"a*").expect("buildable");
        let outcome = dfa.find_match_start_at_reverse(b"bbb", 3);
        assert_eq!(outcome, DfaSearchOutcome::Match(3));
    }
}
