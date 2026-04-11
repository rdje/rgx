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
//! This is C2 step 5a of the phased plan in `docs/C2_NFA_DFA_DESIGN.md`
//! §15. At this stage the module is **standalone** — no engine wiring,
//! no integration with `Regex::compile`, and **no support for zero-width
//! assertions**. NFAs containing `\A`, `\z`, `\Z`, `^`, `$`, `\b`, `\B`,
//! or `\G` are rejected at construction time. Patterns with assertions
//! continue to run on the Pike-VM via the existing dispatch path.
//!
//! C2 step 5b will wire the lazy DFA into engine dispatch (preferring
//! DFA over Pike-VM when available), implement the cache-exhaustion
//! fallback to the Pike-VM, and add the structural exclusions for
//! features the DFA cannot express (assertions and lazy quantifiers).
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

/// Sentinel value for "no transition" / "dead state" in DFA transition
/// tables. The simulator stops on entry to the dead state.
const DEAD_STATE: DfaStateId = u32::MAX;

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

/// A single DFA state. Stores its transition table indexed by byte
/// class plus a precomputed `is_accept` flag for the simulation hot
/// path. The `nfa_states` field is the NFA state set this DFA state
/// represents (sorted, deduplicated); kept on the state itself rather
/// than only in the cache so the transition computation can read it
/// without a reverse cache lookup.
#[derive(Debug, Clone)]
struct DfaState {
    /// `transitions[byte_class] = next DFA state ID`, or `DEAD_STATE`
    /// for "no transition / dead". Length is `LazyDfa.num_classes`.
    transitions: Vec<DfaStateId>,
    /// True iff the NFA's accept state is in `nfa_states`. Cached so
    /// the simulation loop doesn't have to scan the set on every step.
    is_accept: bool,
    /// The NFA state set this DFA state represents. Sorted, deduplicated.
    nfa_states: Vec<NfaStateId>,
}

/// Cache key for the NFA-state-set → DFA-state-id lookup. Wraps a
/// sorted, deduplicated `Vec<NfaStateId>`. Two DFA states are the same
/// iff their NFA state sets are equal.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct DfaStateKey {
    nfa_states: Vec<NfaStateId>,
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
    /// All allocated DFA states. Index is the `DfaStateId`. The start
    /// state is always at index 0.
    states: Vec<DfaState>,
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
        if nfa.has_assertions() {
            return Err(
                "LazyDfa step 5a does not support patterns with zero-width assertions; \
                 patterns containing \\A, \\z, \\Z, ^, $, \\b, \\B, or \\G must run on Pike-VM",
            );
        }
        let num_classes = byte_class_map.num_classes() as usize;
        let mut dfa = Self {
            nfa,
            byte_class_map,
            states: Vec::new(),
            cache: HashMap::new(),
            state_limit,
            num_classes,
        };
        // Construct the start state from the NFA's start.
        let start_set = dfa.compute_start_set();
        let start_id = dfa.allocate_state(start_set);
        debug_assert_eq!(start_id, 0);
        Ok(dfa)
    }

    /// The DFA's start state ID. Always `0`.
    #[must_use]
    pub fn start_state(&self) -> DfaStateId {
        0
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
    fn transition(&mut self, state: DfaStateId, cls: u8) -> TransitionResult {
        if state == DEAD_STATE {
            return TransitionResult::Dead;
        }
        let cached = self.states[state as usize].transitions[cls as usize];
        if cached != DEAD_STATE {
            return TransitionResult::Next(cached);
        }
        // No cached transition. Compute the next NFA state set by
        // following byte transitions for `cls` from every NFA state in
        // the source DFA state, then epsilon-closing the targets.
        let next_set = self.compute_transition_set(state, cls);
        if next_set.is_empty() {
            // Genuinely dead — record DEAD_STATE in the transition
            // table so future lookups skip the recomputation.
            self.states[state as usize].transitions[cls as usize] = DEAD_STATE;
            return TransitionResult::Dead;
        }
        // Look up or allocate the target DFA state.
        let key = DfaStateKey {
            nfa_states: next_set.clone(),
        };
        let target_id = if let Some(&id) = self.cache.get(&key) {
            id
        } else {
            if self.states.len() >= self.state_limit {
                // Cache full. Don't allocate, signal fallback.
                return TransitionResult::Exhausted;
            }
            self.allocate_state(next_set)
        };
        self.states[state as usize].transitions[cls as usize] = target_id;
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
        // The reverse-anchored DFA accepts at the START of the
        // reversed pattern, which is the END of the forward pattern.
        // Walking backward from `end` consumes input bytes from
        // index `end - 1` downward. The latest accept seen during
        // this walk is at the smallest forward index, which is the
        // leftmost forward start.
        debug_assert!(end <= input.len(), "end out of bounds for input");
        let mut state = self.start_state();
        // The DFA could already accept the empty string at the end
        // position (e.g., for patterns like `a*` matching the empty
        // span at any position). Record that as a tentative match.
        let mut leftmost_start = if self.is_accept(state) {
            Some(end)
        } else {
            None
        };
        let mut pos = end;
        while pos > 0 {
            pos -= 1;
            let byte = input[pos];
            let cls = self.byte_class_map.class_of(byte);
            match self.transition(state, cls) {
                TransitionResult::Next(next_state) => {
                    state = next_state;
                    if self.is_accept(state) {
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
        let mut state = self.start_state();
        // The DFA could already accept the empty string at the start
        // position (e.g., for patterns like `a*`). Record that as a
        // tentative match before the loop runs.
        let mut matched_end = if self.is_accept(state) {
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
                    if self.is_accept(state) {
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

    // ============================================================
    // Internals
    // ============================================================

    /// Allocate a fresh DFA state for the given NFA state set and
    /// register it in the cache. Returns the new state's ID.
    fn allocate_state(&mut self, nfa_states: Vec<NfaStateId>) -> DfaStateId {
        let id = self.states.len() as DfaStateId;
        let is_accept = nfa_states.contains(&self.nfa.accept());
        self.states.push(DfaState {
            transitions: vec![DEAD_STATE; self.num_classes],
            is_accept,
            nfa_states: nfa_states.clone(),
        });
        let key = DfaStateKey { nfa_states };
        self.cache.insert(key, id);
        id
    }

    /// Compute the start NFA state set: epsilon closure of the NFA's
    /// start state. Sorted and deduplicated.
    fn compute_start_set(&self) -> Vec<NfaStateId> {
        let mut set = Vec::new();
        let mut visited = vec![false; self.nfa.num_states()];
        self.epsilon_close(&mut set, &mut visited, self.nfa.start());
        set.sort_unstable();
        set
    }

    /// Compute the next NFA state set for `(state, cls)`: for each NFA
    /// state in the source DFA state, follow byte transitions matching
    /// `cls`, then epsilon-close every reached target. Returned set is
    /// sorted and deduplicated.
    fn compute_transition_set(&self, state: DfaStateId, cls: u8) -> Vec<NfaStateId> {
        let nfa_states = &self.states[state as usize].nfa_states;
        let mut next = Vec::new();
        let mut visited = vec![false; self.nfa.num_states()];
        for &nfa_state in nfa_states {
            let state_obj = &self.nfa.states()[nfa_state as usize];
            for &(transition_cls, target) in &state_obj.transitions {
                if transition_cls == cls {
                    self.epsilon_close(&mut next, &mut visited, target);
                }
            }
        }
        next.sort_unstable();
        next
    }

    /// Recursive epsilon closure starting from `state`. Adds every
    /// reachable NFA state (via epsilon edges) to `set` and marks them
    /// in `visited`. Capture tags are ignored — the DFA doesn't track
    /// captures (those are recovered via the bounded Pike-VM pass per
    /// design doc §9). Assertions are debug-asserted absent because
    /// the constructor refused to build the DFA if any were present.
    fn epsilon_close(&self, set: &mut Vec<NfaStateId>, visited: &mut [bool], state: NfaStateId) {
        if visited[state as usize] {
            return;
        }
        visited[state as usize] = true;
        set.push(state);
        let state_obj = &self.nfa.states()[state as usize];
        for edge in &state_obj.epsilons {
            debug_assert!(
                edge.assertion.is_none(),
                "DFA construction expects assertion-free NFAs (checked in LazyDfa::new)"
            );
            self.epsilon_close(set, visited, edge.target);
        }
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
