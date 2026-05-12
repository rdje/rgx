# C2 TDFA: Laurikari Tagged DFA for Capture-Bearing Patterns — Design Proposal

> **Status**: design proposal, awaiting sign-off. **No production code lands until this document is approved.**
>
> **Authors**: Richard DJE, with Claude (Opus 4.7 1M ctx) as collaborator
>
> **Date**: 2026-05-08
>
> **Supersedes / extends**: `docs/C2_NFA_DFA_DESIGN.md` §9 — the present document supersedes the two-pass capture-recovery design that ships today (DFA finds the span, Pike-VM recovers captures over the span). The two-pass approach stays as the fallback path for patterns we don't (yet) compile to a TDFA; this document specifies the tagged-DFA path that replaces it where applicable.
>
> **Blocks**: TDFA Phase 1 (NFA tag helpers), Phase 2 (tagged subset construction), Phase 3 (engine dispatch). Each phase is a separate commit gated on this doc.
>
> **Quality bar**: SOTA from the first commit, per the persistent project preference. Reference implementations: Laurikari's TRE (2001 paper and 2004 implementation), the Rust `regex-automata` crate's `dfa` module with explicit tag support, RE2 (which does NOT do tagged DFA — captures recovered via NFA second pass — so RE2 is *not* a reference here, RGX is going further than RE2 on this axis), and `libtre`. No invented techniques where proven ones exist.

---

## 1. Goals and non-goals

### Goals

1. **Eliminate the Pike-VM capture-recovery pass on hot capture-bearing patterns.** The current C2 pipeline runs the DFA to find `(start, end)`, then re-runs the Pike-VM from `start` to recover capture positions. On patterns with non-trivial capture trees and long match spans, the second pass dominates wall time — samply consistently attributes 30-60% of `Regex::find_all` self-time to `pike_match_at_with_captures` on `email_basic` / `capture_groups`. The TDFA tracks capture positions inline with the byte scan, eliminating the second pass entirely.

2. **Preserve 100% capture-semantics equivalence with the Pike-VM** on every pattern the TDFA accepts. Differential testing against `pike_match_at_with_captures` is the merge gate (see §13). One discrepancy on a single input is a merge blocker.

3. **Cohabit with the existing C2 dispatch.** TDFA is a new path inside `c2/`, not a replacement of the lazy DFA. Patterns that compile to a TDFA route through it; patterns that don't fall back to the existing DFA-then-Pike pipeline. Compile-time classification decides; runtime is one branch on a stored enum.

4. **Match SOTA design choices** from Laurikari TRE / `regex-automata`. Specifically:
   - **Posix-style tag registers** with copy semantics (not position lists, not history queues).
   - **Tag priority resolution at determinization time**, not at runtime — leftmost-first semantics are baked into the merge order during subset construction.
   - **Bounded register allocation** per Laurikari's theorem 4.5: the number of registers per TDFA state is ≤ 2 · |captures| · |Q|, but in practice ≪ that bound. We allocate registers lazily and reuse aggressively.
   - **Register-update instructions on transitions, not states.** Each TDFA transition carries a (potentially empty) list of register copy / save operations that fire when the transition is taken. This is the Laurikari "ɛ-closure with tags" lowering.

### Non-goals

- **Replacing the Pike-VM.** The Pike-VM stays as:
  1. The fallback for patterns the TDFA classifier rejects (§4).
  2. The fallback for TDFA cache exhaustion (§11).
  3. The verification engine in differential testing (§13).
- **Capture support for backtracking-only constructs.** Backreferences, lookaround, recursion, `\K`, atomic groups, possessive quantifiers, inline code, callouts, backtracking verbs — these route to the existing backtracking VM and are entirely outside the C2 subset. The TDFA inherits the §4 subset from the lazy DFA and refines it slightly (see §4 below).
- **POSIX leftmost-longest captures.** RGX has `MatchSemantics::LeftmostLongest` as a runtime switch. POSIX captures are a *different* algorithm (longest match per group, ties broken by leftmost) and would need a different determinization order. First-pass TDFA supports only `LeftmostFirst` (Perl/PCRE2 default). LeftmostLongest patterns fall back to the existing engine. Lifting this is a future commit, not this design.
- **JIT compilation of TDFA transitions.** That's a C1 follow-on; this doc covers the interpreted TDFA. The data structures will be designed to be JIT-friendly (register-update instructions are an SSA-ish IR already) but the JIT itself is out of scope here.
- **Submatching POSIX longest-leftmost.** Same reason as above.
- **Replacing the lazy DFA for zero-capture patterns.** Patterns with no capture groups have nothing to recover; the existing zero-capture fast path in `engine.rs:742` already short-circuits the Pike-VM pass. The TDFA brings no benefit there — the fast path stays.

---

## 2. Architectural overview

```
                          ┌─────────────────────────────────────┐
                          │            AST (existing)           │
                          └──────────────┬──────────────────────┘
                                         │
                                         ▼
                          ┌─────────────────────────────────────┐
                          │   Pattern classifier (extended, §4) │
                          │   NoBacktracking | NeedsVm          │
                          │   + IsTdfaEligible: bool            │
                          └──────────────┬──────────────────────┘
                                         │
                       ┌─────────────────┴─────────────────┐
                       │                                   │
              NoBacktracking                            NeedsVm
                       │                                   │
                       ▼                                   ▼
   ┌────────────────────────────────┐    ┌──────────────────────────────┐
   │   Compile to C2 program        │    │   Compile to backtracking VM │
   │   (NFA + reverse NFA + classes)│    │   bytecode (unchanged path)  │
   │   + tagged forward NFA  ← NEW  │    │                              │
   └──────────────┬─────────────────┘    └──────────────┬───────────────┘
                  │                                     │
                  ▼                                     ▼
   ┌────────────────────────────────┐    ┌──────────────────────────────┐
   │   C2 runtime                   │    │   Backtracking VM (existing) │
   │     • Lazy forward DFA         │    │                              │
   │     • Lazy reverse DFA         │    │                              │
   │     • Sparse-set Pike-VM       │    │                              │
   │     • Tagged DFA  ← NEW        │    │                              │
   └──────────────┬─────────────────┘    └──────────────┬───────────────┘
                  │                                     │
                  └─────────────────┬───────────────────┘
                                    ▼
                          ┌─────────────────────┐
                          │   MatchResult /     │
                          │   Captures<'t>      │
                          │   (existing types)  │
                          └─────────────────────┘
```

Dispatch order inside the C2 runtime (highest priority first), once Phase 3 lands:

1. **Tagged DFA path** — if `tdfa_eligible` and the TDFA finishes within its state cache, the TDFA produces `(start, end, captures)` in a single forward scan. No Pike-VM pass needed.
2. **Reverse-DFA + forward-DFA + Pike-VM capture pass** — current path. Used when the pattern is C2-eligible but not TDFA-eligible, or when the TDFA exhausts its cache.
3. **Pike-VM only** — fallback when both DFAs are unavailable (zero-width assertions other than `\b`, very large state spaces).
4. **Backtracking VM** — fallback for everything else (the `NeedsVm` set).

### Key invariants

- **Compile-time TDFA eligibility decision.** Stored as a `Classification` extension on the compiled `Program`. Runtime is one branch.
- **Identical `MatchResult` and `Captures<'t>` shape** between the TDFA path and the Pike-VM path. The public API surface does not change.
- **TDFA correctness is provable by differential test.** Every TDFA-eligible pattern is run on BOTH the TDFA and the Pike-VM during the differential test suite, and any disagreement on `(start, end, groups[..])` is a merge blocker.
- **TDFA accepts a strict subset of the lazy DFA's eligibility set.** Anything the lazy DFA can't handle, the TDFA can't either. The opposite is allowed — there are lazy-DFA-eligible patterns the TDFA can't handle (e.g., patterns whose captures Laurikari's algorithm can't determinize cleanly; see §4).

---

## 3. Module layout

New module under `rgx-core/src/c2/`:

```
rgx-core/src/c2/
├── ...                 # existing modules unchanged
├── tdfa.rs             # NEW — tagged DFA construction + simulation
└── tdfa/
    ├── tags.rs         # tag inventory + tag identity
    ├── registers.rs    # register allocation + register-set merging
    └── construction.rs # tagged subset construction algorithm
```

Whether the submodule structure or a single 1500-line `tdfa.rs` ships first is a tactical decision. The lazy DFA started as a single file and has stayed clean at 1647 lines. The TDFA will likely cross 2000 lines; the submodule split is the SOTA choice. Decided: submodule, starting at Phase 2.

Touched (extended, not rewritten):

- `c2/nfa.rs` — add tag-edge enumeration helpers and a tag inventory accessor. The NFA already carries `CaptureTag` on epsilon edges (lines 292, 337); we add the read-side API the TDFA needs. **No semantic change to the NFA.** Phase 1.
- `c2/program.rs` — add `tdfa_eligible: bool` field on `CompiledC2Program`. Set by a new classifier visitor that runs alongside the existing C2 classification.
- `c2/mod.rs` — `pub mod tdfa;`.
- `engine.rs` — add `c2_tdfa: OnceLock<Option<TdfaCell>>` field on `Regex`, parallel to `c2_dfa: OnceLock<Option<DfaCell>>`. Dispatch sites (`try_dfa_find_first`, `try_dfa_find_all`, `try_pipeline_find_first`, `try_pipeline_find_all`) extended with a tdfa-first branch.
- `lib.rs` — extend `Regex::classification()` to report TDFA eligibility. Add `Regex::uses_tdfa() -> bool` analogous to `uses_c2()`.

---

## 4. TDFA eligibility (a subset of C2 eligibility)

### Definition (what's in)

A pattern is **TDFA-eligible** if and only if **all** of the following hold:

1. It is C2-eligible per `docs/C2_NFA_DFA_DESIGN.md` §4 (the no-backtracking subset).
2. It contains at least one capture group. (Zero-capture patterns have nothing to recover; the existing fast path already wins.)
3. It does not contain a lazy quantifier whose body contains a capture group. (Lazy capture interaction: lazy semantics need the Pike-VM's priority-ordered closure; the TDFA's leftmost-first tag resolution gives the wrong end position for `(a)+?` patterns. The existing lazy DFA already excludes lazy quantifiers from DFA dispatch; the TDFA inherits this and tightens it: lazy *outside* a capture is fine if the capture itself is non-lazy.)
4. Each capture group is reachable via at most one tag-edge per NFA state under the closure expansion. (Multiple tag edges into the same NFA state from the same source — a structure produced by certain alternation+capture interactions — is the construct that defeats Laurikari's determinization; we conservatively reject patterns where the classifier sees this risk. See §15 for the exact static test.)

### Definition (what's out — falls back to existing path)

| Construct | Why excluded |
|---|---|
| Zero capture groups | Existing zero-capture fast path wins |
| Lazy quantifier wrapping a capture | Priority-ordered captures need Pike-VM |
| LeftmostLongest semantics | Different determinization order (POSIX TDFA — future work) |
| Word-boundary inside a capture's epsilon closure | First-pass conservatism; the interaction is subtle. Lifting this is Phase 4 work, not first-pass. |

The classifier visits the AST once and emits `tdfa_eligible: bool`. The implementation lives in `c2/classifier.rs` extension; the algorithm is straightforward (single walk with three boolean accumulators).

False negatives (TDFA-eligible patterns the classifier conservatively rejects) are a perf miss but never a correctness risk. False positives (classifier says yes but TDFA can't actually handle it correctly) are a correctness bug — the differential test suite catches them.

---

## 5. Background: Laurikari's tagged DFA in 200 lines

The canonical reference is Laurikari (2001), "NFAs with Tagged Transitions, Their Conversion to Deterministic Automata and Application to Regular Expressions." Below is the part RGX uses.

### 5.1 Tags

A **tag** is a label attached to an NFA epsilon transition. For a regex with `g` capture groups, we have `2g` tags: `t_{2i}` marks the start of group `i`, `t_{2i+1}` marks the end. (Same as the existing `CaptureTag::GroupStart(i)` / `CaptureTag::GroupEnd(i)` in `c2/nfa.rs:292` — the tag numbering scheme is identical to what RGX already emits.)

When the NFA simulator crosses a tagged edge, it "fires" the tag: it records the current input position as the value of that tag.

### 5.2 Untagged DFA — what it loses

Standard subset construction produces a DFA state = a set of NFA states. Determinization loses tag information: two NFA configurations reached via different tag firings are merged into the same DFA state set, and the simulator can't tell them apart at match time. This is why the current C2 lazy DFA needs the Pike-VM second pass.

### 5.3 Tagged DFA — what it adds

A **tagged DFA state** is a set of (NFA state, *register map*) pairs. A register map is an injection from tags to a finite set of *registers* (small integers). A register holds a position (`Option<usize>`).

When the simulator transitions from a tagged DFA state `S` to a new tagged DFA state `S'` on byte class `c`:

- For each NFA state `n ∈ S` and each byte-class-`c` transition `n → m` in the NFA, the target NFA state `m` becomes part of the new set.
- The epsilon closure from `m` runs as usual, BUT each tagged epsilon edge crossed during the closure means "fire the corresponding tag" — and the tagged DFA transition records this as a **register write**: "at the position *after consuming the byte that triggered this transition*, write the position into the register that holds tag `t`."
- If two NFA states in `S` lead to the *same* NFA state `m` in `S'` but with different register maps, the determinizer either (a) reuses a register if it can prove the values are always equal at this state (the trivial case), or (b) emits a **register copy instruction** on the transition: "copy register A to register B."

So each tagged DFA transition carries an ordered list of **register operations**:

```rust
enum RegOp {
    /// Copy the value of `src` register into `dst` register.
    Copy { src: u16, dst: u16 },
    /// Save the current input position (the position after consuming
    /// the transition's byte) into `dst` register.
    Save { dst: u16 },
}
```

Order matters: copies must run before saves that read from the destination. The Laurikari paper specifies the dependency ordering rule; we'll follow it directly.

### 5.4 Match readout

At match time:

1. The simulator runs the byte scan as usual. On each transition taken, it executes the transition's `[RegOp]` list against the live register array.
2. When the simulator enters an accept state, it reads from the registers corresponding to that state's register map:
   - Group 0 start: `registers[start_register_for_group_0]`
   - Group 0 end: `registers[end_register_for_group_0]`
   - Group `i` start: `registers[start_register_for_group_i]`
   - ...

The accept state stores its own "final register map" — which register holds which group's start/end at this state. There is no second pass.

### 5.5 Leftmost-first vs leftmost-longest

The choice of which register map to keep when merging two configurations is what encodes the matching semantics:

- **Leftmost-first** (Perl/PCRE2 default, RGX default): prefer the register map from the configuration whose path through the NFA followed lower-priority epsilon edges first. This is exactly the priority order RGX's NFA already encodes via `EpsilonPriority` slot ordering (`c2/nfa.rs:286`). We follow slot order during ɛ-closure during determinization; the first map to reach a target state wins.
- **Leftmost-longest** (POSIX): prefer the register map that yields the longest match for the *outermost* group, with ties broken by leftmost. Different algorithm. Not in first-pass scope.

So: tag priority resolution happens at *determinization time* by following the existing epsilon slot order. The runtime is just walking register-op lists. This is the key insight — it means the runtime hot loop is dead simple (read transition, walk register ops, advance state) and all the cleverness is in the offline construction.

---

## 6. Data structures

### 6.1 Tag inventory (on the NFA — Phase 1)

`c2/nfa.rs` extension:

```rust
impl Nfa {
    /// Returns true iff the NFA has any tagged epsilon edges.
    /// Used by the TDFA classifier to short-circuit on capture-free
    /// patterns.
    pub fn has_capture_tags(&self) -> bool { /* scan once at build time, cache */ }

    /// Returns the number of distinct tags = 2 * num_capture_groups.
    /// Tags are numbered 0..2g where tag 2i is GroupStart(i) and
    /// tag 2i+1 is GroupEnd(i). This is the canonical numbering the
    /// TDFA uses for register allocation.
    pub fn num_tags(&self) -> u32 { 2 * self.num_capture_groups() }

    /// Returns an iterator over (target, tag) for every tagged
    /// epsilon edge originating at `state`. Untagged edges are not
    /// reported. The order matches the epsilon slot order, which is
    /// the leftmost-first priority order. Critical: the TDFA
    /// determinizer iterates in this order to encode priority
    /// resolution.
    pub fn tagged_epsilons(&self, state: NfaStateId)
        -> impl Iterator<Item = (NfaStateId, Tag)> + '_
    { /* ... */ }
}

/// Canonical tag identifier. Wraps a u32 to keep the API typed.
/// `Tag::start_of(g)` = `2g`, `Tag::end_of(g)` = `2g + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tag(u32);
```

No semantic change to the NFA. New accessors only. Phase 1 commit.

### 6.2 Tagged DFA state

```rust
/// A tagged DFA state. Index into a `Vec<TaggedDfaState>`.
pub type TaggedDfaStateId = u32;

#[derive(Debug, Clone)]
struct TaggedDfaState {
    /// Sorted, deduplicated NFA state IDs. Same shape as the lazy
    /// DFA's nfa_states.
    nfa_states: Vec<NfaStateId>,

    /// Per (nfa_state_id_index) → per-tag register assignment. Element
    /// at flat index (i * num_tags + t) gives the register holding
    /// tag t's position for NFA state `nfa_states[i]`. A value of
    /// REGISTER_NONE means the tag was never fired for this thread.
    ///
    /// Stored as a flat Vec<u16> for cache locality. The same layout
    /// works because nfa_states is sorted; (i, t) indexing is stable.
    register_map: Vec<u16>,

    /// True iff the NFA's accept state is in nfa_states.
    is_accept: bool,

    /// If is_accept, the per-tag register that holds the final value
    /// of each tag at this state. Indexed by tag number (0..num_tags).
    /// Length `num_tags` when accept; empty otherwise.
    accept_register_map: Vec<u16>,
}
```

The `register_map` per-(state, tag) pair is essential for determinization but is not read at runtime. We could drop it after construction to save memory; the cache will hold it through Phase 3 and we'll measure before deciding.

### 6.3 Tagged DFA transition

```rust
/// A transition entry in the flat transition table.
#[derive(Debug, Clone, Copy)]
struct TaggedTransition {
    /// Target state. DEAD_STATE / UNCACHED sentinels follow the
    /// lazy DFA's convention.
    target: TaggedDfaStateId,
    /// Index into the TaggedDfa's `reg_op_pool` for the start of this
    /// transition's RegOp slice. `len_or_inline` distinguishes inline
    /// (0-2 ops, fits in the cap field) from pooled.
    reg_op_idx: u32,
    /// 0 means no ops. 1-N means N ops in the pool starting at
    /// reg_op_idx. We can pack inline-1-op transitions tighter later
    /// if profiling shows it matters.
    reg_op_len: u16,
}
```

The transition table is flat: `transitions[state * num_classes + cls]: TaggedTransition`. Same layout as the lazy DFA's flat `Vec<DfaStateId>`, except each cell is wider (now 10 bytes: 4 + 4 + 2 = 10, padded to 12 or 16 depending on alignment).

The `reg_op_pool` is a global `Vec<RegOp>` on the TaggedDfa; transitions index into it. This avoids per-transition `Vec<RegOp>` allocations (which would tank cache locality).

### 6.4 Register operation

```rust
/// A single register update. Executed in order when a transition is
/// taken. The "current position" referenced by `Save` is the byte
/// position AFTER consuming the byte that triggered the transition —
/// matching Laurikari's "position after edge" semantic.
#[derive(Debug, Clone, Copy)]
enum RegOp {
    /// `registers[dst] = registers[src]`
    Copy { src: u16, dst: u16 },
    /// `registers[dst] = Some(current_position)`
    Save { dst: u16 },
}
```

### 6.5 The TDFA itself

```rust
pub struct TaggedDfa {
    nfa: Arc<Nfa>,
    byte_class_map: Arc<ByteClassMap>,
    num_tags: u32,
    num_classes: usize,

    /// Per-state metadata.
    states: Vec<TaggedDfaState>,

    /// Flat transition table. Length = states.len() * num_classes.
    transitions: Vec<TaggedTransition>,

    /// Pool of RegOps indexed by transitions.reg_op_idx.
    reg_op_pool: Vec<RegOp>,

    /// Cache key → state id. Same role as the lazy DFA's cache.
    cache: HashMap<TaggedDfaStateKey, TaggedDfaStateId>,

    state_limit: usize,

    /// Total registers allocated. Used to size the register array
    /// at simulation start.
    num_registers: u32,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct TaggedDfaStateKey {
    nfa_states: Vec<NfaStateId>,
    /// Canonicalised register assignment. Two states with the
    /// same nfa_states but different register_maps are different
    /// keys — but if a permutation of registers makes two register_maps
    /// equal, they ARE the same key (after canonicalisation). This is
    /// Laurikari's "reorder" step.
    canonical_register_map: Vec<u16>,
}
```

### 6.6 The simulator

```rust
pub struct TdfaSimulator<'a> {
    tdfa: &'a TaggedDfa,
    registers: Vec<Option<usize>>,
    // Scratch for the (write-to-temp, copy-back) pattern when a
    // transition's RegOps create a cycle. Sized to num_registers.
    scratch: Vec<Option<usize>>,
}
```

The hot loop:

```rust
pub fn find_match_at(&mut self, input: &[u8], start: usize)
    -> Option<(usize, Vec<Option<usize>>)>
{
    let mut state = self.tdfa.start_state();
    let mut matched_end_and_regs = None;
    self.reset_registers();
    // Fire start-of-match tags for the start state's epsilon closure.
    self.apply_start_state_ops();

    for (i, &b) in input[start..].iter().enumerate() {
        let cls = self.tdfa.byte_class_map.class_of(b);
        let trans = self.tdfa.transition(state, cls);
        if trans.target == DEAD_STATE { break; }
        self.apply_reg_ops(trans, start + i + 1);
        state = trans.target;
        if self.tdfa.states[state].is_accept {
            matched_end_and_regs = Some((start + i + 1, self.registers.clone()));
        }
    }
    matched_end_and_regs.map(|(end, regs)| (end, regs))
}
```

The per-byte cost is roughly: byte-class lookup (existing), transition lookup (one array index), RegOp slice walk (1-3 entries average per Laurikari section 3.4 — small constant), state advance. No HashMap, no clone-per-byte, no Pike-VM. This is the perf win.

---

## 7. Tagged subset construction algorithm

This section is the heart of the work. It is a careful adaptation of Laurikari §3.3 ("Determinization of TNFAs") with our slot-ordered priority semantics.

### 7.1 Start state

1. Initial NFA state set: epsilon closure of `nfa.start()`. Same closure walker as the lazy DFA, but extended to track tags fired during closure.
2. While walking the closure in slot order, when a tagged edge is crossed:
   - Allocate a register `r_t` for tag `t` if the current configuration doesn't have one yet.
   - Set `register_map[target_nfa_state, t] = r_t`.
   - Record a start-state initialization op `Save { dst: r_t }` (these run before the first byte).
3. After the closure completes, canonicalise the register map (renumber registers so the same (state set, conceptual register map) always produces the same key) and store the state.

### 7.2 Transition from state `S` on byte class `c`

1. For each NFA state `n ∈ S.nfa_states`, walk its byte-class-`c` transitions. For each `n → m`:
   - Inherit the register assignment from `n` for every tag.
2. From each `m`, run the tagged epsilon closure in slot order:
   - When crossing a tagged epsilon edge `m → m'` with tag `t`:
     - If `m'` is reached for the first time in this closure, allocate a fresh register `r_new` for tag `t` (or reuse an existing register if Laurikari's "reorder" rule says they're equivalent).
     - Emit `Save { dst: r_new }`. The save runs at the position *after* consuming the byte.
   - When reaching the same `m'` via a higher-priority path (a path that fired fewer tags or that came through a lower epsilon slot), the existing assignment wins — leftmost-first.
3. The new NFA state set is the union of all reached `m'`.
4. Canonicalise the new state set's register map. If a permutation of registers makes the new set's map equal to a previously cached state's map (modulo reordering), reuse that state and emit `Copy` ops on the transition to reshuffle live registers into the canonical layout.
5. Store the transition's `[RegOp]` list in the pool; record the transition.

The "reorder" / canonicalisation step is what makes Laurikari's algorithm terminate. Without it, every distinct register permutation would create a distinct DFA state, and the state space would blow up exponentially. With it, the state space is bounded by `2^O(|Q| · g)` — exponential in the worst case but vastly smaller in practice (Laurikari measured ~3x untagged DFA on real workloads).

### 7.3 Dependency-ordered RegOp emission

When a transition's RegOps include both copies and saves, the order matters. Example: if we need both `Copy { src: 1, dst: 2 }` and `Save { dst: 1 }`, doing the save first overwrites register 1 before the copy reads it.

Algorithm: emit a dependency graph (edges: copy reads from src, save writes to dst), topologically sort. Cycles in copies (mutual exchange) need a scratch register. Sufficient scratch is allocated as part of `num_registers` — Laurikari shows one global scratch suffices, but we allocate per-cycle for clarity (will tighten if it costs anything measurable).

### 7.4 Accept state register map

When the NFA's accept state is in `S.nfa_states`, the TDFA state is an accept state. The `accept_register_map` is the register assignment for the accept state at this point in the determinization: `accept_register_map[t] = register holding tag t's position when this state is reached`.

At match time, when the simulator hits an accept state, it reads the registers indexed by this map directly. No per-tag lookup; it's a contiguous read.

### 7.5 Termination and bounding

Laurikari theorem 4.5: the determinization terminates with at most `2^O(|Q| · log |Q|)` states. In practice we cap the cache at `TDFA_STATE_LIMIT` (proposed: 4096, double the lazy DFA's default) and fall back to the lazy DFA + Pike-VM path on exhaustion.

The cache eviction policy is **bounded re-run** — if we exhaust mid-construction, abandon the partially-built TDFA and dispatch the match attempt through the existing two-pass path. The TDFA is "lazy" in the same sense as the existing lazy DFA: states are constructed on demand and accumulate across `find_first` / `find_all` calls.

---

## 8. The hot loop in detail

```rust
#[inline]
fn apply_reg_ops(&mut self, trans: TaggedTransition, pos: usize) {
    let ops = &self.tdfa.reg_op_pool[
        trans.reg_op_idx as usize
            ..(trans.reg_op_idx as usize + trans.reg_op_len as usize)
    ];
    if ops.is_empty() { return; } // common case: no caps to update
    // Copies are emitted before saves by construction (§7.3).
    for op in ops {
        match *op {
            RegOp::Copy { src, dst } => {
                self.registers[dst as usize] = self.registers[src as usize];
            }
            RegOp::Save { dst } => {
                self.registers[dst as usize] = Some(pos);
            }
        }
    }
}
```

Per-byte cost target: ≤ 2 ns on M-series Apple Silicon, ≤ 3 ns on x86-64. Memory: registers vector is `num_registers × 16 bytes` (Option<usize>); for typical 4-5 capture group patterns this is 8-10 registers, 128-160 bytes, stays in L1.

---

## 9. Engine dispatch integration

`engine.rs` field:

```rust
pub(crate) enum TdfaCell {
    Materialized(Arc<TaggedDfa>),
    Lazy(parking_lot::Mutex<TaggedDfa>),
}

pub struct Regex {
    // ... existing fields
    c2_tdfa: OnceLock<Option<TdfaCell>>,
}
```

Dispatch site (illustrative — actual integration point inside `try_dfa_find_first`):

```rust
// New top-of-function branch, before the existing DFA dispatch.
if let Some(tdfa_cell) = self.should_dispatch_to_tdfa() {
    match tdfa_cell {
        TdfaCell::Materialized(tdfa) => {
            if let Some((end, regs)) = tdfa.find_match_at_immut(input, start) {
                let start_pos = regs[0].unwrap_or(start);
                return Some(self.build_match_from_tdfa_registers(start_pos, end, &regs, c2));
            }
            // TDFA found no match — return None (TDFA is exhaustive for
            // its eligible subset; no fall-through to DFA).
            return None;
        }
        TdfaCell::Lazy(mutex) => {
            let mut tdfa = mutex.lock();
            match tdfa.find_match_at(input, start) {
                TdfaOutcome::Match(end, regs) => {
                    let start_pos = regs[0].unwrap_or(start);
                    return Some(self.build_match_from_tdfa_registers(start_pos, end, &regs, c2));
                }
                TdfaOutcome::NoMatch => return None,
                TdfaOutcome::Exhausted => {
                    // Cache exhausted mid-scan. Fall through to the
                    // existing DFA + Pike pipeline for this call.
                }
            }
        }
    }
}
// existing DFA dispatch unchanged
```

Notes:
- The TDFA path is *exhaustive* for its eligible subset when the cache holds — if it returns NoMatch, that's the final answer. Only `Exhausted` falls through.
- `Materialized` (fully eager-filled) skips the mutex entirely, matching the lazy-DFA materialised optimisation that landed earlier this session.
- `should_dispatch_to_tdfa` honours the same runtime gates as `should_dispatch_to_dfa` (no event observer, no match limits, no literal finder).

---

## 10. Phased implementation

Each phase is its own commit. The previous phase must land and pass the differential gate before the next phase starts.

### Phase 0 — Design doc (this document)

- Land this document.
- Update `MEMORY.md`, `CHANGES.md`, `BACKLOG.md`, book TOC.

### Phase 1 — NFA tag helpers (≈ 1 day)

- Add `Tag` newtype to `c2/nfa.rs`.
- Add `has_capture_tags()`, `num_tags()`, `tagged_epsilons()` accessors.
- Cache the `has_capture_tags` flag at NFA build time (one bool field).
- Add unit tests that verify slot-order iteration over tagged epsilons for representative patterns: `(a)`, `(a)(b)`, `(a|b)(c)`, `((a)b)`.
- **No behavioural change.** Existing code compiles and tests pass.

### Phase 2 — Tagged subset construction (≈ 4-7 days)

- New module `c2/tdfa/` with `tags.rs`, `registers.rs`, `construction.rs`, and a top-level `c2/tdfa.rs` that re-exports.
- Implement `TaggedDfa::try_build(nfa, byte_class_map, state_limit) -> Option<TaggedDfa>`.
- Implement the determinization with leftmost-first slot priority.
- Implement register allocation, canonicalisation, dependency-ordered RegOp emission.
- Unit tests in module: build TDFAs for `(a)`, `(a)(b)`, `(\d+)-(\d+)`, `([a-z]+)@([a-z]+)`. Assert state count is within a sanity bound; assert the start state's register map exists for every tag.
- **Not wired to the engine yet.** The classifier doesn't refer to `TaggedDfa` yet either.

### Phase 3 — Simulator + dispatch (≈ 3-5 days)

- Implement `TdfaSimulator` and `find_match_at` / `find_match_at_immut` (mirror the lazy DFA's mut/immut split).
- Add `tdfa_eligible: bool` to `CompiledC2Program`; add the classifier visitor.
- Add `c2_tdfa: OnceLock<Option<TdfaCell>>` to `Regex`; add `should_dispatch_to_tdfa()`.
- Wire dispatch sites: `try_dfa_find_first`, `try_dfa_find_all`, `try_pipeline_find_first`, `try_pipeline_find_all`. TDFA is tried *before* the lazy DFA.
- Add `Regex::uses_tdfa() -> bool`.
- **Differential gate: all 856 differential tests pass on TDFA-eligible patterns.**

### Phase 4 — Performance + perf gate (≈ 2-3 days)

- Run `regression_check`. Expect ≥ 1.3x on `email_basic.find_all`, `capture_groups.find_all`, `url_simple.find_all`.
- If a TDFA-eligible bench regresses, profile and fix before the commit lands.
- Update `book/src/internals/c2-tagged-dfa.md` chapter (NEW chapter) with the rationale, the algorithm, and the perf numbers.

### Phase 5 — Eligibility broadening (open-ended)

- Lift the "no `\b` inside captures" restriction.
- Profile-guided: which TDFA-ineligible patterns appear in real workloads? Extend the classifier where the algorithm permits.
- POSIX leftmost-longest TDFA construction (separate algorithm; future doc).

---

## 11. Cache eviction policy

The TDFA cache uses the same lazy-build + bounded-state-cap discipline as the existing lazy DFA. The proposed `TDFA_STATE_LIMIT` is 4096 (double the lazy DFA's 2048) because:

- Tagged determinization adds state count by a factor of 2-3x in typical patterns (per Laurikari §5).
- The hot loop's cache-friendly hit rate matters more on the TDFA than on the lazy DFA because of the per-transition RegOp pool reads.
- 4096 states × ~10 RegOps average × 4 bytes = ~160 KB. Fits comfortably in L2.

On exhaustion, the simulator returns `Exhausted` and the engine falls back to the existing DFA+Pike path for this *call*. The TDFA cache is *not* cleared — subsequent calls may proceed if the existing states cover their walk. A future commit can add an eviction strategy if cache thrashing becomes an issue.

`try_materialize` (analogous to the lazy DFA's eager fill) is a Phase 4 follow-on: if `try_build` succeeds and the resulting TDFA has ≤ `TDFA_MATERIALIZE_LIMIT` (proposed: 64, same as the lazy DFA) states, eager-fill the entire transition table at compile time and store as `Materialized`. The materialised path skips the mutex on every call, which is the perf cliff we paid down in the lazy DFA materialisation commit and want to preserve here.

---

## 12. Open questions

The following are unresolved at design time and need a decision before or during the relevant phase.

1. **Inline vs pooled RegOps.** The 0-2-ops common case could be stored inline in `TaggedTransition` (12 bytes), with a separate "spilled to pool" representation for longer lists. Tighter cache locality but more complex hot loop. Decision deferred to Phase 4 profiling.

2. **Should TDFA support `(?i)` case-insensitivity directly?** The lazy DFA already handles `(?i)` via byte-class equivalence; the TDFA inherits this for free for ASCII. Full Unicode case folding interacts with multi-byte transitions and is the same question §15 of `C2_NFA_DFA_DESIGN.md` raised. **Phase 3 punts** by inheriting whatever the C2 classifier decides.

3. **Reverse TDFA?** The reverse-DFA pipeline exists in the lazy DFA. A reverse TDFA would let us recover captures during the *reverse* scan and skip both the forward DFA's second pass AND the Pike-VM second pass. Probably overkill; the forward TDFA alone gives us 80% of the win.

4. **Materialised cap.** 64 states is a guess inherited from the lazy DFA. The TDFA's per-state size is larger (the `register_map` field). May need to tune the cap differently. Profile.

5. **What about find_all with overlapping match attempts?** The TDFA register array needs to reset between match attempts. The simulator does this (`reset_registers()`). Does this defeat any compiler optimization in the hot loop? Measure in Phase 4.

6. **Lazy-prefix interaction.** The lazy DFA prunes lazy-prefix states from accept-containing state sets to preserve leftmost-first on unanchored matches (`c2/dfa.rs:895` `compute_start_set`). The TDFA must do the same. This adds an extra closure walk at each accept state — manageable but worth flagging.

---

## 13. Correctness — the differential test gate

This is the merge gate for every TDFA commit from Phase 2 onward. No clippy warnings, no test failures, no skipped tests. **The same standard the rest of C2 has been held to.**

### 13.1 Test corpus

The C2 differential corpus already exists at `rgx-core/tests/c2_pike_differential.rs`. Extend it with a TDFA differential suite:

```rust
fn assert_tdfa_matches_pike(pattern: &str, input: &str) {
    let regex = Regex::new(pattern).unwrap();
    if !regex.uses_tdfa() { return; }  // skip non-eligible patterns
    let tdfa_match = regex.find_first_via_tdfa(input);
    let pike_match = regex.find_first_via_pike(input);
    assert_eq!(tdfa_match, pike_match,
        "TDFA/Pike disagree on pattern={:?} input={:?}", pattern, input);
}
```

The corpus is the union of:
- Hand-curated patterns: every capture pattern in `rgx-core/tests/`.
- PCRE2 conformance suite filtered by TDFA eligibility: anything passing `regex.uses_tdfa()` is auto-included.
- Random patterns generated by the existing fuzz harness, filtered by TDFA eligibility.

### 13.2 Property-based testing

A new property test in `rgx-core/tests/c2_tdfa_property.rs`:

```rust
proptest! {
    #[test]
    fn tdfa_matches_pike_on_capture_patterns(
        pattern in capture_pattern_strategy(),
        input in arb_input(),
    ) {
        let r = Regex::new(&pattern)?;
        if !r.uses_tdfa() { return Ok(()); }
        prop_assert_eq!(
            r.find_first_via_tdfa(&input),
            r.find_first_via_pike(&input)
        );
    }
}
```

### 13.3 Benchmark regression gate

The existing `regression_check` infrastructure (`rgx-bench/baselines/main.toml`) is the perf gate. New TDFA-eligible benchmarks land in Phase 4. Tolerance: ≤ 20% regression vs. the pre-TDFA baseline on *any* bench. Net wins on capture-bearing benches expected ≥ 1.3x.

---

## 14. Performance targets

Per-bench expectations after Phase 4 (assuming the differential gate passes):

| Benchmark | Pre-TDFA ratio (vs PCRE2) | Target post-TDFA ratio | Notes |
|---|---|---|---|
| `email_basic.find_all` | 2.27x faster | ≥ 3.0x faster | Has 3 capture groups; Pike-VM second pass dominates today |
| `capture_groups.find_all` | 46.9x faster | ≥ 50x faster | Already DFA-dominated; modest win expected |
| `url_simple.find_all` | 1.22x faster | ≥ 2.0x faster | Has 5 capture groups; biggest expected win |
| `literal_simple.find_all` | 1.97x faster | unchanged | Zero captures; not TDFA-eligible |
| `character_class.find_all` | 2.58x faster | unchanged | Zero captures; not TDFA-eligible |
| `alternation.find_all` | 13.6x faster | unchanged | Zero captures; not TDFA-eligible |
| `digit_sequence.find_all` | 1.08x faster | unchanged | Zero captures; not TDFA-eligible |

These are *targets*, not guarantees. The differential gate is the merge condition; perf targets are nice-to-have. If a TDFA-eligible pattern *regresses*, the commit doesn't land.

---

## 15. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Tagged determinization state explosion on alternation-heavy patterns | Cache cap + graceful fallback to existing path. Classifier rejects patterns whose untagged DFA already approaches the cap. |
| Subtle leftmost-first vs Pike-VM semantics divergence | Differential gate. Hand-curated tests for known-tricky patterns: `(a|ab)`, `((a*)*)`, `(a*?)b`, `(a)(b)*(c)`. |
| Register allocation bug producing wrong captures | Differential gate. Plus: register-state inspector for debug builds that dumps the register map at every transition for a given input. |
| RegOp dependency ordering bug (copies and saves interleaved wrong) | Unit tests for the topological sort. Property test: random RegOp lists, verify execution against a reference simulator. |
| First-pass eligibility too narrow → no perf win | Acceptable for first commit. Phase 5 broadens incrementally based on workload analysis. |
| Hot loop regresses due to extra per-transition work | Phase 4 perf gate. Materialised path skips the mutex; samply attribution at every commit. |
| Submodule split creates churn on later refactors | Single-file start at Phase 2 is the safer call. Reconsidered: ship Phase 2 as a single `c2/tdfa.rs` and split later if it crosses 2000 lines. |
| The "register canonicalisation makes states equal" rule produces non-deterministic state IDs across runs | Sort the canonicalised map by tag number, deterministic by construction. Add a unit test that builds the same TDFA twice and asserts identical state count + transition map. |

---

## 16. References

- **Ville Laurikari (2001)**, "NFAs with Tagged Transitions, Their Conversion to Deterministic Automata and Application to Regular Expressions." *SPIRE 2000.* The canonical reference for this entire document.
- **Ville Laurikari (2004)**, *TRE* — the reference C implementation. `https://github.com/laurikari/tre`. Useful for the reorder-and-canonicalise step, which the paper sketches but the code implements concretely.
- **Russ Cox**, "Regular Expression Matching in the Wild." RE2's design rationale; informs the lazy-construction discipline.
- **The Rust `regex-automata` crate**, `dfa::dense::DFA` and the deprecated tagged-DFA branch (search the issue tracker for "Laurikari"). Useful for the data layout decisions in §6.
- **`docs/C2_NFA_DFA_DESIGN.md`**, §9. The two-pass capture recovery this design supersedes.
- **`docs/C1_JIT_COMPILATION_DESIGN.md`**. The JIT will eventually target both the lazy DFA and the TDFA; data-structure choices here keep that path open.

---

## 17. Acceptance criteria for this document

This document is approved for implementation when:

- [ ] The user has read it end-to-end.
- [ ] The phase boundaries (§10) are agreed.
- [ ] The differential-gate plan (§13) is agreed.
- [ ] The perf targets (§14) are accepted as goals (not contractual).
- [ ] The "first-pass eligibility is narrow on purpose" framing (§4) is agreed.

Until then, no production code lands in `c2/tdfa.rs`. The doc itself is the Phase 0 deliverable.
