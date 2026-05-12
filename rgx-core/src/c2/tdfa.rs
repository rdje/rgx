//! Tagged DFA (TDFA) for capture-bearing patterns.
//!
//! Implements the **Laurikari tagged DFA** described in
//! `docs/C2_TDFA_DESIGN.md`. Replaces the two-pass capture recovery
//! (DFA finds the span → Pike-VM recovers captures over the span)
//! with a single forward scan that tracks capture positions inline
//! via per-state **tag registers**.
//!
//! # Phase 2a scope (this commit)
//!
//! This commit lands the foundational data types and the start-state
//! construction with tag firing:
//!
//! - [`RegOp`] — register update instruction on a transition.
//! - [`TaggedTransition`] — a target state plus a `[RegOp]` slice
//!   reference.
//! - [`TaggedDfaState`] — a DFA state with its NFA state set,
//!   per-(NFA-state, tag) register assignment, and accept-state
//!   register map.
//! - [`TaggedDfa`] — the construction-time container. Carries the
//!   per-state metadata, the flat transition table, the RegOp pool,
//!   and the cache.
//! - [`TaggedDfa::try_build`] — builds the start state. Byte
//!   transitions land in Phase 2b.
//!
//! The simulator and engine dispatch land in Phase 2d / Phase 3
//! respectively. Until then, this module is dead code from the
//! engine's perspective; only the unit tests exercise it.
//!
//! # Algorithm in 200 lines
//!
//! See `docs/C2_TDFA_DESIGN.md` §5 for the full Laurikari TDFA
//! summary. The short version:
//!
//! 1. The NFA has tagged epsilon edges (`CaptureTag::GroupStart(g)` /
//!    `GroupEnd(g)`). These already exist (`c2/nfa.rs:292`).
//! 2. A TDFA state is **NOT** just a set of NFA states; it's a set
//!    of (NFA state, register map) pairs. The register map says,
//!    for this NFA state in this DFA state, which register holds
//!    each tag's position.
//! 3. Transitions carry [`RegOp`] sequences: `Copy { src, dst }`
//!    and `Save { dst }`. The simulator runs these in order when
//!    taking the transition.
//! 4. At an accept state, captures are read directly from the
//!    registers indicated by the accept state's
//!    [`TaggedDfaState::accept_register_map`]. No second pass.
//!
//! # Register numbering convention (matches `c2/pike.rs`)
//!
//! Tag `Tag(2g)` is group `g`'s start; `Tag(2g + 1)` is group `g`'s
//! end. Slot 0/1 are reserved for the whole-match span (group 0);
//! the simulator fills those in from the scan start/end positions
//! rather than via register firing. Same convention as the Pike-VM's
//! capture buffer.

use crate::c2::byte_class::ByteClassMap;
use crate::c2::nfa::{Nfa, NfaStateId, Tag};
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================
// Public types
// ============================================================

/// State identifier in the TDFA. The start state is always `0`.
pub type TaggedDfaStateId = u32;

/// Sentinel register value meaning "this tag has not been fired
/// for this (state, tag) pair." The simulator initialises every
/// register slot to `None`; this sentinel is the in-DFA marker that
/// the register *index* is unassigned.
const REGISTER_NONE: u16 = u16::MAX;

/// A single register update on a TDFA transition. Executed in order
/// when the transition is taken. The "current position" referenced
/// by [`RegOp::Save`] is the byte position **after** consuming the
/// byte that triggered the transition — matching Laurikari's
/// "position after edge" convention and the Pike-VM's existing
/// capture-tag firing point.
///
/// Dependency rule: when a transition's RegOps include both copies
/// and saves, the construction emits copies in topological order
/// before saves that read from the destination register. Cycles in
/// the copy graph (mutual exchange) need a scratch register; this
/// is handled at construction time, not at simulation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegOp {
    /// `registers[dst] = registers[src]`.
    ///
    /// Used when register canonicalisation collapses two register
    /// configurations into one — the live values are reshuffled to
    /// match the canonical layout before the transition completes.
    Copy { src: u16, dst: u16 },
    /// `registers[dst] = Some(current_position)`.
    ///
    /// Used when a tagged epsilon edge is crossed during the
    /// tagged closure expansion — the tag's value is the position
    /// after the transition's byte.
    Save { dst: u16 },
}

/// A single TDFA transition. Lives in the flat transition table at
/// index `state * num_classes + cls`.
///
/// `reg_op_len == 0` means no register updates run on this transition
/// — the common case for transitions inside a deeply nested capture
/// where neither bracket boundary fires.
#[derive(Debug, Clone, Copy)]
pub struct TaggedTransition {
    /// Target state. The same sentinels as the lazy DFA apply
    /// (`DEAD_STATE`, `UNCACHED`); see `c2/dfa.rs` for the rationale.
    /// Phase 2a only builds the start state; the dead/uncached
    /// sentinels formalise in Phase 2b when transitions land.
    pub target: TaggedDfaStateId,
    /// Index into [`TaggedDfa::reg_op_pool`] for the start of this
    /// transition's RegOp slice.
    pub reg_op_idx: u32,
    /// Number of RegOps starting at `reg_op_idx`. Zero is the common
    /// case (no captures cross this transition).
    pub reg_op_len: u16,
}

/// Sentinel for "computed dead transition" — no NFA-reachable target
/// for `(state, byte_class)`. Mirror of `c2/dfa.rs::DEAD_STATE`.
/// Lands in Phase 2b transitions.
const DEAD_STATE: TaggedDfaStateId = u32::MAX - 1;

/// Sentinel for "uncached transition slot." Distinct from
/// [`DEAD_STATE`] so cached-dead lookups can short-circuit without
/// recomputing — same two-sentinel design as `c2/dfa.rs`.
/// Lands in Phase 2b transitions.
const UNCACHED: TaggedDfaStateId = u32::MAX;

/// A single TDFA state.
///
/// Compared to a lazy DFA state (`c2/dfa.rs::DfaState`), the TDFA
/// state additionally carries a **per-(NFA-state, tag) register
/// assignment** in [`Self::register_map`] and an accept-state-only
/// **accept_register_map** that the simulator reads when this state
/// is the accept terminus.
///
/// `register_map` is indexed as a flat `Vec<u16>` of length
/// `nfa_states.len() * num_tags`. The (i, t) entry is the register
/// holding tag t's position for `nfa_states[i]`. `REGISTER_NONE`
/// means the tag has not been fired for that NFA-state thread.
#[derive(Debug, Clone)]
pub struct TaggedDfaState {
    /// Sorted, deduplicated NFA state IDs. Same shape as the lazy
    /// DFA's `nfa_states` field.
    pub nfa_states: Vec<NfaStateId>,

    /// Per-(NFA-state-index-in-`nfa_states`, tag) → register id.
    /// Length `nfa_states.len() * num_tags`. Indexed as
    /// `i * num_tags + tag.index()`. Values in `0..num_registers`
    /// or `REGISTER_NONE`.
    ///
    /// Stored flat for cache locality.
    pub register_map: Vec<u16>,

    /// True iff the NFA's accept state is in `nfa_states`.
    pub is_accept: bool,

    /// Per-tag register holding the final value at this accept
    /// state. Length `num_tags` when `is_accept`; empty otherwise.
    /// Indexed by `tag.index()` directly.
    ///
    /// Reading captures at match time is one flat read of this
    /// slice followed by a dereference of each register — no
    /// per-state closure expansion, no Pike-VM second pass.
    pub accept_register_map: Vec<u16>,
}

/// Cache key for the (NFA state set + register configuration) → DFA
/// state lookup.
///
/// Two TDFA states with the same NFA state set *and* the same
/// canonicalised register configuration are the same state. The
/// canonicalisation step (Phase 2c) ensures equivalent register
/// permutations don't create distinct states — without it, the
/// state space would blow up exponentially. With it, Laurikari's
/// algorithm terminates.
///
/// Phase 2a uses the trivial canonicalisation (compare the
/// register map verbatim). Phase 2c upgrades to the Laurikari
/// reorder rule.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct TaggedDfaStateKey {
    nfa_states: Vec<NfaStateId>,
    register_map: Vec<u16>,
}

/// A tagged DFA built from a Thompson NFA and a byte-class map.
///
/// Construct via [`TaggedDfa::try_build`]. Phase 2a builds only the
/// start state; Phase 2b adds byte transitions; Phase 2c adds
/// canonicalisation; Phase 2d adds the simulator.
///
/// The TDFA owns its `Arc<Nfa>` and `Arc<ByteClassMap>`, so multiple
/// TDFAs (or a TDFA and the existing lazy DFA) can share the same
/// NFA cheaply.
#[derive(Debug)]
pub struct TaggedDfa {
    nfa: Arc<Nfa>,
    byte_class_map: Arc<ByteClassMap>,

    /// Cached `num_tags()` from the NFA. Used to size register maps.
    num_tags: usize,
    /// Cached `num_classes()` from the byte-class map.
    num_classes: usize,

    /// Per-state metadata. Index is `TaggedDfaStateId`. State 0 is
    /// the start state.
    states: Vec<TaggedDfaState>,

    /// Flat transition table. Length `states.len() * num_classes`.
    /// Phase 2a leaves this empty (the start state has no outgoing
    /// transitions yet); Phase 2b populates it.
    transitions: Vec<TaggedTransition>,

    /// RegOp pool. [`TaggedTransition::reg_op_idx`] indexes into
    /// this. Phase 2a uses this only for start-state initialisation
    /// operations (see [`Self::start_reg_ops`]).
    reg_op_pool: Vec<RegOp>,

    /// Cache: (NFA state set, register config) → TaggedDfaStateId.
    /// Phase 2a uses verbatim register-map comparison; Phase 2c
    /// upgrades to canonicalised comparison.
    cache: HashMap<TaggedDfaStateKey, TaggedDfaStateId>,

    /// Maximum number of TDFA states allowed before construction
    /// gives up. The TDFA classifier (Phase 3) falls back to the
    /// existing DFA + Pike pipeline if `try_build` returns `None`.
    state_limit: usize,

    /// Total registers allocated during construction. The simulator
    /// allocates a `Vec<Option<usize>>` of this length per match
    /// attempt.
    num_registers: u32,

    /// RegOps that run **before the first byte** to initialise
    /// registers for tags fired during the start-state ε-closure.
    /// In Laurikari's formulation these are part of the "ε-prefix"
    /// firing; in our register layout we materialise them as a
    /// distinguished `Vec<RegOp>` the simulator runs once at
    /// `find_match_at` entry.
    start_reg_ops: Vec<RegOp>,
}

// ============================================================
// Construction errors
// ============================================================

/// Why a TDFA construction attempt was abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TdfaBuildError {
    /// The NFA contains a zero-width assertion that the TDFA cannot
    /// yet handle. First-pass TDFA accepts only assertion-free NFAs
    /// (and `\b` / `\B`, which the existing DFA already handles).
    /// Future commits may lift this restriction.
    UnsupportedAssertion,
    /// The NFA has no capture tags. The zero-capture fast path
    /// already wins; the TDFA is redundant for this pattern.
    NoCaptureTags,
    /// Construction's state cache exhausted before completion.
    /// Caller should fall back to the existing two-pass path for
    /// this pattern.
    StateLimit,
}

// ============================================================
// Implementation
// ============================================================

impl TaggedDfa {
    /// Default state cache limit. Per `docs/C2_TDFA_DESIGN.md` §11
    /// this is double the lazy DFA's `DEFAULT_STATE_LIMIT` because
    /// tagged determinization typically expands the state count by
    /// a factor of 2-3x on real patterns.
    pub const DEFAULT_STATE_LIMIT: usize = 4096;

    /// Build a TDFA from a forward NFA and a byte-class map.
    ///
    /// Phase 2a builds only the start state. Phase 2b adds byte
    /// transitions; until then the TDFA has exactly one state and
    /// no transitions, but the start-state register map and
    /// initialisation RegOps are fully constructed.
    ///
    /// # Errors
    ///
    /// - [`TdfaBuildError::UnsupportedAssertion`] if the NFA
    ///   contains a non-`\b` zero-width assertion. (Phase 2a is
    ///   conservative; word boundaries inside captures will be
    ///   admitted in a later phase.)
    /// - [`TdfaBuildError::NoCaptureTags`] if the NFA has no
    ///   capture groups (the existing zero-capture fast path is
    ///   strictly better).
    /// - [`TdfaBuildError::StateLimit`] if the state cache fills.
    pub fn try_build(
        nfa: Arc<Nfa>,
        byte_class_map: Arc<ByteClassMap>,
        state_limit: usize,
    ) -> Result<Self, TdfaBuildError> {
        if !nfa.has_capture_tags() {
            return Err(TdfaBuildError::NoCaptureTags);
        }
        // Phase 2a: reject patterns with any zero-width assertions.
        // The DFA handles `\b` separately; the TDFA inherits the
        // lazy DFA's exclusion list initially and tightens it
        // further (`\b` inside a capture's ε-closure is
        // conservatively excluded in first-pass). Future phases
        // can lift these one at a time.
        if nfa.has_assertions() {
            return Err(TdfaBuildError::UnsupportedAssertion);
        }

        let num_tags = nfa.num_tags() as usize;
        let num_classes = byte_class_map.num_classes() as usize;

        let mut tdfa = TaggedDfa {
            nfa: Arc::clone(&nfa),
            byte_class_map: Arc::clone(&byte_class_map),
            num_tags,
            num_classes,
            states: Vec::new(),
            transitions: Vec::new(),
            reg_op_pool: Vec::new(),
            cache: HashMap::new(),
            state_limit,
            num_registers: 0,
            start_reg_ops: Vec::new(),
        };

        tdfa.build_start_state()?;
        Ok(tdfa)
    }

    /// Number of TDFA states allocated. Always at least 1 after
    /// successful construction.
    #[must_use]
    pub fn num_states(&self) -> usize {
        self.states.len()
    }

    /// Number of registers allocated across all states. The
    /// simulator allocates a `Vec<Option<usize>>` of this length
    /// per match attempt.
    #[must_use]
    pub fn num_registers(&self) -> u32 {
        self.num_registers
    }

    /// RegOps that run before the first byte to initialise
    /// registers for the start-state's tag firings.
    #[must_use]
    pub fn start_reg_ops(&self) -> &[RegOp] {
        &self.start_reg_ops
    }

    /// Returns the start state ID, which is always `0`.
    #[must_use]
    pub fn start_state(&self) -> TaggedDfaStateId {
        0
    }

    /// Borrow a TDFA state by ID. Panics on out-of-bounds.
    #[must_use]
    pub fn state(&self, id: TaggedDfaStateId) -> &TaggedDfaState {
        &self.states[id as usize]
    }

    /// Number of distinct tags the TDFA tracks. Equal to
    /// `nfa.num_tags()`.
    #[must_use]
    pub fn num_tags(&self) -> usize {
        self.num_tags
    }

    /// Number of byte classes. Equal to `byte_class_map.num_classes()`.
    #[must_use]
    pub fn num_classes(&self) -> usize {
        self.num_classes
    }

    // ----------------------------------------------------------
    // Construction internals
    // ----------------------------------------------------------

    /// Allocate a fresh register for a tag firing.
    ///
    /// Phase 2a uses monotonic register allocation: every tag
    /// firing gets a new register. Phase 2c adds the Laurikari
    /// reorder rule that collapses equivalent register
    /// configurations and reuses registers across states. Without
    /// canonicalisation the register count can grow linearly with
    /// state count; with it, it's bounded by `O(|tags|^2)` per
    /// Laurikari §4.5.
    fn allocate_register(&mut self) -> u16 {
        let id = self.num_registers;
        self.num_registers = self.num_registers.saturating_add(1);
        u16::try_from(id).expect("TDFA register count exceeded u16::MAX")
    }

    /// Build the start state.
    ///
    /// Walks the NFA's start-state ε-closure in epsilon-slot order
    /// (leftmost-first priority). When a tagged ε-edge is crossed,
    /// fires the tag: allocates a fresh register and records the
    /// firing in `start_reg_ops` (which the simulator runs once at
    /// the beginning of every match attempt) and in the target
    /// NFA state's register map.
    fn build_start_state(&mut self) -> Result<(), TdfaBuildError> {
        let mut nfa_states_in_order: Vec<NfaStateId> = Vec::new();
        // Per-NFA-state register map for this DFA state. Built up
        // during the closure walk; collapsed into the flat
        // `register_map` storage at the end.
        let mut per_state_register_map: HashMap<NfaStateId, Vec<u16>> = HashMap::new();

        let start = self.nfa.start();
        self.tagged_epsilon_closure(
            start,
            &mut nfa_states_in_order,
            &mut per_state_register_map,
            None, // no inherited register map for the start state
            /* is_start_state */ true,
        );

        let mut nfa_states_sorted = nfa_states_in_order.clone();
        nfa_states_sorted.sort_unstable();
        nfa_states_sorted.dedup();

        let is_accept = nfa_states_sorted.contains(&self.nfa.accept());
        let register_map_flat =
            self.flatten_register_map(&nfa_states_sorted, &per_state_register_map);

        let accept_register_map = if is_accept {
            self.compute_accept_register_map(&nfa_states_sorted, &per_state_register_map)
        } else {
            Vec::new()
        };

        let state = TaggedDfaState {
            nfa_states: nfa_states_sorted.clone(),
            register_map: register_map_flat.clone(),
            is_accept,
            accept_register_map,
        };

        let id = self.allocate_state_in_cache(state, &nfa_states_sorted, &register_map_flat)?;
        debug_assert_eq!(id, 0, "start state must always be allocated at index 0");
        Ok(())
    }

    /// Run an ε-closure from `seed` in epsilon-slot order, firing
    /// tags along tagged ε-edges.
    ///
    /// `nfa_states_in_order` accumulates the closure's NFA states
    /// in visit order (the canonical slot-priority traversal). The
    /// sorted, deduped version is what lands on the TDFA state.
    ///
    /// `per_state_register_map` accumulates, for each NFA state
    /// reached, that state's per-tag register assignment at this
    /// point in the closure. The closure inherits from the
    /// "predecessor" (the state that brought us to the current
    /// state) and updates on every crossed tagged ε-edge.
    ///
    /// `inherited` is the predecessor's register map (or `None`
    /// for the seed state — which gets an all-`REGISTER_NONE`
    /// starting map).
    ///
    /// `is_start_state` distinguishes the start-state firing
    /// context (RegOps go in `self.start_reg_ops`) from a
    /// transition firing context (RegOps go on the transition,
    /// added in Phase 2b).
    fn tagged_epsilon_closure(
        &mut self,
        seed: NfaStateId,
        nfa_states_in_order: &mut Vec<NfaStateId>,
        per_state_register_map: &mut HashMap<NfaStateId, Vec<u16>>,
        inherited: Option<&[u16]>,
        is_start_state: bool,
    ) {
        // Iterative DFS preserves slot order (lowest-slot edge
        // pushed last so it pops first).
        let mut stack: Vec<(NfaStateId, Vec<u16>)> = Vec::new();
        let seed_map = inherited
            .map(<[u16]>::to_vec)
            .unwrap_or_else(|| vec![REGISTER_NONE; self.num_tags]);
        stack.push((seed, seed_map));

        while let Some((state, regs)) = stack.pop() {
            // Higher-priority paths to the same state win. If the
            // state already has a register map, we keep the existing
            // one (it came from a lower-slot path) and don't
            // overwrite. This is the leftmost-first encoding.
            if per_state_register_map.contains_key(&state) {
                continue;
            }
            per_state_register_map.insert(state, regs.clone());
            nfa_states_in_order.push(state);

            // Snapshot the outgoing epsilon edges so we can mutate
            // self (allocate_register / push to start_reg_ops)
            // inside the loop without conflicting borrows. Per-state
            // epsilon count is small (typically 1-3) so the clone is
            // cheap. Assertion edges are screened out by try_build's
            // `has_assertions()` gate; every edge here is either
            // tagged or plain untagged ε.
            let edges: Vec<_> = self.nfa.states()[state as usize]
                .epsilons
                .iter()
                .map(|e| (e.target, e.capture_tag))
                .collect();

            // Push edges in REVERSE slot order so they pop in slot
            // order. Slot 0 (highest priority) is pushed last.
            for (target, capture_tag) in edges.into_iter().rev() {
                if per_state_register_map.contains_key(&target) {
                    continue;
                }
                let mut child_regs = regs.clone();
                if let Some(tag) = capture_tag {
                    let tag = Tag::from(tag);
                    let r = self.allocate_register();
                    let tag_idx = tag.index() as usize;
                    if tag_idx < child_regs.len() {
                        child_regs[tag_idx] = r;
                    }
                    if is_start_state {
                        self.start_reg_ops.push(RegOp::Save { dst: r });
                    }
                    // If !is_start_state, the firing should land on
                    // the in-flight transition's RegOp list. Phase
                    // 2b adds the transition-RegOp accumulator.
                }
                stack.push((target, child_regs));
            }
        }
    }

    /// Flatten the per-NFA-state register map into a single
    /// `Vec<u16>` indexed as `i * num_tags + tag.index()` where
    /// `i` is the position of `nfa_states[i]` in the sorted set.
    fn flatten_register_map(
        &self,
        nfa_states_sorted: &[NfaStateId],
        per_state: &HashMap<NfaStateId, Vec<u16>>,
    ) -> Vec<u16> {
        let mut flat = Vec::with_capacity(nfa_states_sorted.len() * self.num_tags);
        for &s in nfa_states_sorted {
            let regs = per_state
                .get(&s)
                .expect("per-state register map missing entry for NFA state in sorted set");
            debug_assert_eq!(regs.len(), self.num_tags);
            flat.extend_from_slice(regs);
        }
        flat
    }

    /// Compute the accept-state register map: for the accept NFA
    /// state, the registers holding each tag's final value.
    fn compute_accept_register_map(
        &self,
        nfa_states_sorted: &[NfaStateId],
        per_state: &HashMap<NfaStateId, Vec<u16>>,
    ) -> Vec<u16> {
        let accept = self.nfa.accept();
        debug_assert!(nfa_states_sorted.contains(&accept));
        per_state
            .get(&accept)
            .cloned()
            .expect("accept NFA state must have a register map entry")
    }

    /// Allocate a state in the cache. Returns the new state ID, or
    /// [`TdfaBuildError::StateLimit`] if the cache is full.
    fn allocate_state_in_cache(
        &mut self,
        state: TaggedDfaState,
        nfa_states: &[NfaStateId],
        register_map: &[u16],
    ) -> Result<TaggedDfaStateId, TdfaBuildError> {
        if self.states.len() >= self.state_limit {
            return Err(TdfaBuildError::StateLimit);
        }
        let id = u32::try_from(self.states.len())
            .expect("TDFA state count exceeded u32::MAX (impossible in practice)");
        self.states.push(state);
        // Pre-allocate the transition row with UNCACHED sentinels;
        // Phase 2b populates these as transitions are computed.
        // The pre-allocation keeps the flat-table invariant
        // `transitions.len() == states.len() * num_classes`.
        for _ in 0..self.num_classes {
            self.transitions.push(TaggedTransition {
                target: UNCACHED,
                reg_op_idx: 0,
                reg_op_len: 0,
            });
        }
        let key = TaggedDfaStateKey {
            nfa_states: nfa_states.to_vec(),
            register_map: register_map.to_vec(),
        };
        self.cache.insert(key, id);
        Ok(id)
    }
}

// Suppress unused-symbol warnings during the phased rollout. The
// transition sentinels and cache types are wired into the public
// surface in Phase 2b / 2c; this `_` re-export keeps the compiler
// from emitting noise without making the symbols ambiguously
// re-exported.
#[allow(dead_code)]
const _UNUSED: (TaggedDfaStateId, TaggedDfaStateId) = (DEAD_STATE, UNCACHED);

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AnchorType, GroupKind, Regex};
    use crate::c2::byte_class::ByteClassMap;
    use crate::c2::nfa::{Nfa, Tag};

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

    /// Build an anchored NFA + byte class map directly from an AST.
    /// Avoids the full `Regex::compile` engine path so tests stay
    /// scoped to the TDFA construction.
    fn build_components(ast: &Regex) -> (Arc<Nfa>, Arc<ByteClassMap>) {
        let byte_class_map = ByteClassMap::build_from_ast(ast);
        let nfa = Nfa::build_anchored(ast, &byte_class_map);
        (Arc::new(nfa), Arc::new(byte_class_map))
    }

    #[test]
    fn try_build_rejects_no_capture_pattern() {
        // abc — no captures, fast path wins.
        let (nfa, bcm) = build_components(&seq(vec![lit('a'), lit('b'), lit('c')]));
        let result = TaggedDfa::try_build(nfa, bcm, TaggedDfa::DEFAULT_STATE_LIMIT);
        assert_eq!(result.err(), Some(TdfaBuildError::NoCaptureTags));
    }

    #[test]
    fn try_build_rejects_assertion_bearing_pattern() {
        // ^(a) — anchored start assertion, conservative reject.
        let pattern = seq(vec![
            Regex::Anchor(AnchorType::AbsStart),
            group_capturing(1, lit('a')),
        ]);
        let (nfa, bcm) = build_components(&pattern);
        let result = TaggedDfa::try_build(nfa, bcm, TaggedDfa::DEFAULT_STATE_LIMIT);
        assert_eq!(result.err(), Some(TdfaBuildError::UnsupportedAssertion));
    }

    #[test]
    fn try_build_accepts_simple_capture() {
        let (nfa, bcm) = build_components(&group_capturing(1, lit('a')));
        let tdfa = TaggedDfa::try_build(nfa, bcm, TaggedDfa::DEFAULT_STATE_LIMIT)
            .expect("build tdfa for (a)");
        assert_eq!(tdfa.num_states(), 1);
        assert!(
            tdfa.num_registers() >= 1,
            "expected at least one register fired in start ε-closure (GroupStart(1)), got {}",
            tdfa.num_registers()
        );
        // Start RegOps must contain at least one Save for the
        // GroupStart(1) tag fired during the start-state closure.
        assert!(
            !tdfa.start_reg_ops().is_empty(),
            "start ε-closure crossed GroupStart(1); start_reg_ops must contain a Save"
        );
        assert!(
            tdfa.start_reg_ops()
                .iter()
                .all(|op| matches!(op, RegOp::Save { .. })),
            "start RegOps must all be Saves (no Copies before the first byte)"
        );
    }

    #[test]
    fn start_state_register_map_has_group_start_for_a() {
        // (a) at the start state: ε-closure enters the body via
        // GroupStart(1). The body-entry NFA state must have
        // tag 2 (= start_of group 1) mapped to a register; tag 3
        // (= end_of group 1) is still unfired.
        let (nfa, bcm) = build_components(&group_capturing(1, lit('a')));
        let nfa_ref = Arc::clone(&nfa);
        let tdfa = TaggedDfa::try_build(nfa, bcm, TaggedDfa::DEFAULT_STATE_LIMIT)
            .expect("build tdfa for (a)");

        let start_state = tdfa.state(tdfa.start_state());
        let num_tags = tdfa.num_tags();
        assert_eq!(num_tags, nfa_ref.num_tags() as usize);

        // For each NFA state in the start-state set, find at least
        // one entry where tag 2 (GroupStart(1)) is fired (non-NONE).
        let tag_start_1 = Tag::start_of(1).index() as usize;
        let mut any_fired = false;
        for i in 0..start_state.nfa_states.len() {
            let base = i * num_tags;
            if start_state.register_map[base + tag_start_1] != REGISTER_NONE {
                any_fired = true;
                break;
            }
        }
        assert!(
            any_fired,
            "at least one NFA state in start-state set must have GroupStart(1) fired"
        );
    }

    #[test]
    fn start_state_for_anchored_alternation_fires_both_branches() {
        // (a)|(b) — both branches' GroupStart tags fire during the
        // start ε-closure since both branches are reachable via
        // priority-ordered ε-edges from the alt-start.
        let (nfa, bcm) = build_components(&alt(vec![
            group_capturing(1, lit('a')),
            group_capturing(2, lit('b')),
        ]));
        let tdfa = TaggedDfa::try_build(nfa, bcm, TaggedDfa::DEFAULT_STATE_LIMIT)
            .expect("build tdfa for (a)|(b)");

        let start_state = tdfa.state(tdfa.start_state());
        let num_tags = tdfa.num_tags();
        let tag_start_1 = Tag::start_of(1).index() as usize;
        let tag_start_2 = Tag::start_of(2).index() as usize;

        let mut fired_1 = false;
        let mut fired_2 = false;
        for i in 0..start_state.nfa_states.len() {
            let base = i * num_tags;
            if start_state.register_map[base + tag_start_1] != REGISTER_NONE {
                fired_1 = true;
            }
            if start_state.register_map[base + tag_start_2] != REGISTER_NONE {
                fired_2 = true;
            }
        }
        assert!(fired_1, "GroupStart(1) must be fired in start state");
        assert!(fired_2, "GroupStart(2) must be fired in start state");
        // Both branches fire — at least two Saves in the start ops.
        assert!(tdfa.start_reg_ops().len() >= 2);
    }

    #[test]
    fn nested_captures_fire_both_starts_in_order() {
        // ((a)) — group 1 wraps group 2 wraps 'a'. Both GroupStart
        // tags fire during the start ε-closure. Slot order means
        // outer (1) fires before inner (2).
        let (nfa, bcm) = build_components(&group_capturing(1, group_capturing(2, lit('a'))));
        let tdfa = TaggedDfa::try_build(nfa, bcm, TaggedDfa::DEFAULT_STATE_LIMIT)
            .expect("build tdfa for ((a))");

        let start_state = tdfa.state(tdfa.start_state());
        let num_tags = tdfa.num_tags();
        let tag_start_1 = Tag::start_of(1).index() as usize;
        let tag_start_2 = Tag::start_of(2).index() as usize;

        let mut fired_1 = false;
        let mut fired_2 = false;
        for i in 0..start_state.nfa_states.len() {
            let base = i * num_tags;
            if start_state.register_map[base + tag_start_1] != REGISTER_NONE {
                fired_1 = true;
            }
            if start_state.register_map[base + tag_start_2] != REGISTER_NONE {
                fired_2 = true;
            }
        }
        assert!(fired_1, "GroupStart(1) must fire in nested capture start");
        assert!(fired_2, "GroupStart(2) must fire in nested capture start");
        // Two starts → at least two Saves.
        assert!(tdfa.start_reg_ops().len() >= 2);
    }

    #[test]
    fn num_registers_reflects_tag_firings() {
        // Each tag firing allocates one register in Phase 2a's
        // monotonic allocator.
        //
        // For (a)(b) the start ε-closure crosses GroupStart(1) but
        // STOPS at the byte-'a' transition — GroupStart(2) fires
        // only after the 'a' is consumed (Phase 2b territory). So
        // exactly one register is allocated and the start ops
        // contain exactly one Save.
        let (nfa, bcm) = build_components(&seq(vec![
            group_capturing(1, lit('a')),
            group_capturing(2, lit('b')),
        ]));
        let tdfa = TaggedDfa::try_build(nfa, bcm, TaggedDfa::DEFAULT_STATE_LIMIT)
            .expect("build tdfa for (a)(b)");
        assert_eq!(tdfa.num_registers(), 1);
        assert_eq!(tdfa.start_reg_ops().len(), 1);
        assert!(matches!(tdfa.start_reg_ops()[0], RegOp::Save { .. }));
    }

    #[test]
    fn start_state_includes_accept_register_map_when_pattern_empty() {
        // (()) — outer group wraps inner empty group. The entire
        // pattern matches the empty string, so the start state IS
        // the accept state. accept_register_map must be populated.
        let (nfa, bcm) = build_components(&group_capturing(1, group_capturing(2, Regex::Empty)));
        let tdfa = TaggedDfa::try_build(nfa, bcm, TaggedDfa::DEFAULT_STATE_LIMIT)
            .expect("build tdfa for (())");
        let start = tdfa.state(tdfa.start_state());
        assert!(
            start.is_accept,
            "(()) matches empty; start state must be accept"
        );
        assert_eq!(
            start.accept_register_map.len(),
            tdfa.num_tags(),
            "accept register map must have one entry per tag"
        );
        // Group 1 start and group 2 start must both be fired at
        // the accept state (we crossed both GroupStart edges in
        // the start ε-closure on the way to the accept).
        let tag_start_1 = Tag::start_of(1).index() as usize;
        let tag_start_2 = Tag::start_of(2).index() as usize;
        assert_ne!(start.accept_register_map[tag_start_1], REGISTER_NONE);
        assert_ne!(start.accept_register_map[tag_start_2], REGISTER_NONE);
    }

    #[test]
    fn state_limit_rejection_path() {
        // Pass an unrealistically low state limit (0) and verify
        // the build refuses. This exercises the StateLimit error
        // branch even though Phase 2a only allocates one state.
        let (nfa, bcm) = build_components(&group_capturing(1, lit('a')));
        let result = TaggedDfa::try_build(nfa, bcm, 0);
        assert_eq!(result.err(), Some(TdfaBuildError::StateLimit));
    }
}
