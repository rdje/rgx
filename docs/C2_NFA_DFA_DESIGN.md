# C2: NFA/DFA Hybrid Engine — Design Proposal

> **Status**: design proposal, awaiting sign-off. **No code lands until this document is approved.**
>
> **Authors**: Richard DJE, with Claude (Opus 4.6 1M ctx) as collaborator
>
> **Date**: 2026-04-09 (decision session)
>
> **Supersedes**: nothing — this is the first design pass for C2.
>
> **Blocks**: all C2 implementation steps (1 through 8 in the phased plan, see §15).
>
> **Quality bar**: SOTA from the first commit, per the persistent project preference. No prototype work, no "simplest possible" framing, no "ship it and improve later." Each piece that lands is the right thing the first time. Reference implementations: RE2 and the Rust `regex` crate, both of which have spent years on the problems described here.

---

## 1. Goals and non-goals

### Goals

1. **Change RGX's algorithmic class** on the patterns where most users live. The existing backtracking VM is bounded by O(2ⁿ) on pathological patterns and by VM dispatch overhead on common patterns. C2 will deliver:
   - O(nm) worst-case for the no-backtracking subset (n = input length, m = pattern size)
   - 10x–100x typical speedup on regular patterns where the lazy DFA fits in cache
   - The "can't hang" property the Rust `regex` crate uses as its primary differentiator over PCRE2
2. **Preserve full semantic equivalence** with the existing backtracking VM on every pattern in the no-backtracking subset. Differential testing against the existing engine is the merge gate (see §13).
3. **Cohabit cleanly with the existing engine.** The backtracking VM stays in place forever and handles every pattern outside the subset (backreferences, recursion, lookaround, inline code blocks, atomic groups, possessive quantifiers, `\K`, backtracking verbs). C2 is a parallel engine, not a replacement.
4. **Match SOTA design choices** from RE2 and the Rust `regex` crate. Sparse-set Pike-VM, byte-class equivalence partitioning, lazy DFA with state cache and graceful fallback, reverse DFA for start-of-match recovery, two-pass capture recovery. No invented techniques where proven ones exist.

### Non-goals

- **Replacing the backtracking VM.** It stays. C2 is dispatched-to, not a rewrite.
- **Supporting backreferences, recursion, lookaround, inline code blocks, atomic groups, possessive quantifiers, `\K`, or backtracking verbs in C2.** These features are fundamentally backtracking-only or stateful in ways the NFA/DFA model can't express. Patterns using them route to the existing VM.
- **JIT compilation.** That's C1 and is sequenced after C2 so the JIT can target both engines.
- **Supporting POSIX leftmost-longest semantics in C2 on the first pass.** RGX has `MatchSemantics::LeftmostLongest` as a runtime switch already. Whether C2 supports both semantics or only LeftmostFirst initially is an open question (§15).
- **Capture group support inside the DFA itself.** Captures are recovered via a small bounded NFA pass over the matched span. The DFA never tracks captures. This is the §9 decision.

---

## 2. Architectural overview

```
                          ┌─────────────────────────────────────┐
                          │            AST (existing)           │
                          └──────────────┬──────────────────────┘
                                         │
                                         ▼
                          ┌─────────────────────────────────────┐
                          │   Pattern classifier (new, §4)      │
                          │   NoBacktracking | NeedsVm          │
                          └──────────────┬──────────────────────┘
                                         │
                       ┌─────────────────┴─────────────────┐
                       │                                   │
              NoBacktracking                            NeedsVm
                       │                                   │
                       ▼                                   ▼
   ┌────────────────────────────────┐    ┌──────────────────────────────┐
   │   Compile to C2 program        │    │   Compile to existing VM     │
   │   (NFA + reverse NFA + classes)│    │   bytecode (unchanged path)  │
   └──────────────┬─────────────────┘    └──────────────┬───────────────┘
                  │                                     │
                  ▼                                     ▼
   ┌────────────────────────────────┐    ┌──────────────────────────────┐
   │   C2 runtime (new, §§7-10)     │    │   Backtracking VM (existing) │
   │     • Lazy forward DFA         │    │                              │
   │     • Lazy reverse DFA         │    │                              │
   │     • Sparse-set Pike-VM       │    │                              │
   │       (fallback + capture pass)│    │                              │
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

### Key invariants

- **Compile-time classification.** The decision of which engine handles a pattern is made once, at compile time, and stored as metadata on the compiled `Program`. Runtime dispatch is one branch on a stored enum, not a re-analysis.
- **Both engines produce identical `MatchResult` and `Captures<'t>`.** The public API surface is unchanged. Users do not see two engines; they see one regex engine that's faster on the patterns where C2 applies.
- **Differential equivalence is provable.** Every pattern that classifies as `NoBacktracking` is run on BOTH engines during testing, and any disagreement is a merge blocker (§13).

---

## 3. Module layout

New modules under `rgx-core/src/c2/`:

```
rgx-core/src/c2/
├── mod.rs              # public surface within the crate; re-exports
├── classifier.rs       # AST → Classification (NoBacktracking | NeedsVm)
├── byte_class.rs       # byte-class equivalence partitioning
├── nfa.rs              # Thompson NFA construction (forward + reverse)
├── pike.rs             # sparse-set Pike-VM
├── dfa.rs              # lazy DFA cache (used by both forward and reverse)
├── dispatch.rs         # engine selection at runtime; the cohabitation seam
└── tests/              # C2-internal unit tests (in addition to differential)
```

The existing `rgx-core/src/{compiler.rs, vm.rs, engine.rs, lib.rs}` get small additions to:
- store classification metadata on `Program`
- route classified-positive patterns through `c2::dispatch` instead of straight to the VM
- nothing else changes in the existing path

This module layout is non-negotiable for the design — it keeps C2 cleanly bounded so it can't accidentally entangle with the backtracking VM internals.

---

## 4. The no-backtracking subset

### Definition (what's in)

A pattern is **classified as `NoBacktracking`** if and only if its AST contains *only* nodes from this exact set:

| Construct | Notes |
|---|---|
| Literal characters and strings | UTF-8 aware |
| Character classes (`[abc]`, `[^a-z]`, `\d`, `\D`, `\w`, `\W`, `\s`, `\S`, `\h`, `\H`, `\v`, `\V`) | Including negated forms |
| Unicode property classes (`\p{L}`, `\P{Greek}`, etc.) | Resolved via the existing `unicode_support.rs` bridge |
| ASCII case folding under `(?i)` | Full Unicode case folding under `(?i)` is an open question — see §15 |
| Alternation (`a|b|c`) | |
| Concatenation | |
| Greedy quantifiers (`?`, `*`, `+`, `{n,m}`, `{n,}`, `{n}`) | |
| Lazy quantifiers (`??`, `*?`, `+?`, `{n,m}?`, `{n,}?`) | |
| Non-capturing groups `(?:...)` | |
| Capturing groups `(...)`, named groups `(?<name>...)`, `(?P<name>...)` | Captures are recovered via the §9 two-pass approach |
| Anchors `^`, `$`, `\A`, `\Z`, `\z` | Multiline-aware via the existing flag plumbing |
| Word boundaries `\b`, `\B` | ASCII initially; Unicode word boundaries are an open question — see §15 |
| Inline flag groups `(?i)`, `(?m)`, `(?s)`, `(?x)` and their scoped forms | Flags are baked into the NFA at construction time |

### Definition (what's out)

A pattern is **classified as `NeedsVm`** if its AST contains *any* of:

| Construct | Why excluded |
|---|---|
| Backreferences `\1`, `\k<name>`, `\k'name'`, `(?P=name)` | Not regular; require state the NFA can't express |
| Recursion / subroutine calls `(?R)`, `(?1)`, `(?&name)`, `(?P>name)`, returned-capture `(?N(grouplist))` | Not regular; same reason |
| Lookahead `(?=...)`, `(?!...)` | Could be supported via product construction but explicitly excluded on the first pass — too much state-space risk |
| Lookbehind `(?<=...)`, `(?<!...)` | Same |
| Conditionals `(?(1)...)`, `(?(R)...)`, `(?(<name>)...)`, `(?(?=...)...)`, `(?(DEFINE)...)`, etc. | Carry state across positions |
| Atomic groups `(?>...)` | Backtracking-only semantics |
| Possessive quantifiers `*+`, `++`, `?+`, `{n,m}+` | Same |
| `\K` (keep-out) | Repositions match start mid-execution |
| Backtracking verbs `(*COMMIT)`, `(*PRUNE)`, `(*SKIP)`, `(*FAIL)`, `(*ACCEPT)`, `(*MARK:name)` | Backtracking-only semantics |
| Inline code blocks `(?{lua:...})`, `(?{js:...})`, `(?{rhai:...})`, `(?{native:...})`, `(?{wasm:...})` | Side-effecting; require the execution layer |
| `(?C)` callouts | Side-effecting |
| Branch-reset groups `(?|...)` | Could be supported but excluded on the first pass — capture numbering interaction is subtle |
| Perl extended character classes `(?[...])` | Excluded on the first pass — the class algebra is implemented as VM-side bytecode and would need re-lowering for C2 |

The classifier is **conservative**: anything it isn't sure about classifies as `NeedsVm`. False negatives (a pattern that *could* run on C2 but classifies to VM) are a perf miss but never a correctness risk. False positives (a pattern that classifies to C2 but C2 can't actually handle) are a correctness bug and are forbidden by the differential test suite.

### Classification algorithm

A single AST visitor in `classifier.rs`. Walks the AST once, accumulating a `Classification` enum. The first encountered out-of-subset construct sets the result to `NeedsVm` and short-circuits. Stored as `program.classification: Classification` after compilation.

```rust
enum Classification {
    NoBacktracking,
    NeedsVm { reason: ExclusionReason },
}

enum ExclusionReason {
    Backreference,
    Recursion,
    Lookaround,
    Conditional,
    AtomicGroup,
    PossessiveQuantifier,
    KeepOut,
    BacktrackingVerb,
    InlineCodeBlock,
    Callout,
    BranchReset,
    PerlExtendedClass,
    UnsupportedFlag,
    // open: any others discovered during implementation
}
```

The `reason` field exists for debuggability (`--show-details` could expose it; the trace layer could log it) and for the benchmark suite (which can report C2 hit rate and the most common exclusion reasons across a corpus).

---

## 5. Byte-class equivalence partitioning

### What it is

The 256-byte alphabet is collapsed into equivalence classes computed at compile time. Two bytes belong to the same equivalence class iff every character class and literal in the pattern treats them identically. Then NFA transitions and DFA transitions index by equivalence class number, not by byte value.

### Why it matters

- **DFA cache density.** A naive DFA stores 256 transitions per state. With `n` equivalence classes (typically `n ≤ 32` for most patterns), each state stores `n` transitions. The cache fits more states for the same memory budget.
- **Transition table size.** Same reason. Smaller tables, better cache locality.
- **NFA simulation speed.** The Pike-VM's transition lookup is by equivalence class, not by byte. One indirection saved per character.

### Algorithm

1. Collect every character class and literal in the AST as a set of `CharRange`.
2. Sort all range endpoints.
3. Walk the sorted endpoints and assign an equivalence class ID to each maximal interval where all character classes agree.
4. Build a 256-element lookup table `byte → class_id`.
5. Store the table on the compiled program.

This is computed once at compile time. The lookup table is `[u8; 256]`. For UTF-8 patterns with multi-byte content, this works on bytes — the NFA construction handles UTF-8 decoding by emitting byte-level state sequences for multi-byte characters (the existing `lexer.rs` and `compiler.rs` already do this; C2 inherits the convention).

### Module: `c2/byte_class.rs`

```rust
pub struct ByteClassMap {
    /// byte → class id
    table: [u8; 256],
    /// number of distinct classes (1..=256)
    num_classes: u16,
}

impl ByteClassMap {
    pub fn build_from_ast(ast: &Regex) -> Self { /* ... */ }
    pub fn class_of(&self, byte: u8) -> u8 { self.table[byte as usize] }
    pub fn num_classes(&self) -> u16 { self.num_classes }
}
```

> **Note**: `num_classes` is `u16` rather than `u8` because the count can be exactly 256 (one class per byte) which doesn't fit in `u8`. Class IDs themselves are always in `0..256` and fit in `u8`, which is why the table values stay `u8`. (Corrected from the original sketch in C2 step 2.)

---

## 6. Forward and reverse NFA construction

### Forward NFA

Standard Thompson construction from the AST. Each AST node compiles to a small NFA fragment:

| AST node | NFA fragment |
|---|---|
| Literal byte `b` | `start --[b]--> accept` |
| Character class `[abc]` | `start --[class]--> accept` (transitions are by byte-class id) |
| Concatenation `e₁e₂` | NFA(e₁) followed by NFA(e₂) via epsilon |
| Alternation `e₁|e₂` | New start with epsilons to NFA(e₁).start and NFA(e₂).start; both accepts join via epsilon to a new accept |
| `e?` (greedy) | New start with epsilons: priority 0 to NFA(e).start, priority 1 to new accept |
| `e?` (lazy) | Same but with priorities swapped |
| `e*` (greedy) | New start with epsilons: priority 0 to NFA(e).start, priority 1 to new accept; NFA(e).accept loops back via epsilon to new start |
| `e*` (lazy) | Same but with priorities swapped |
| `e+` | NFA(e) followed by an `e*` |
| `e{n}`, `e{n,m}` | Unrolled per RE2 / regex convention; bounded ranges expand at compile time |
| Anchor `^`, `$`, `\A`, etc. | Special "zero-width assertion" transitions; consumed during simulation, not by byte input |
| `\b`, `\B` | Same |
| Capture group entry/exit | Marked as **tags** on the corresponding epsilon transitions (used only by the bounded Pike-VM capture pass — see §9) |

### Reverse NFA

The reverse NFA matches the **reverse** of the original language. It's used by the reverse DFA (§8) to recover match start positions efficiently.

Construction: walk the AST and reverse the order of every concatenation; reverse anchors (`^` ↔ `$`, `\A` ↔ `\z`); leave alternation, quantifiers, and character classes alone (they're symmetric).

This is **not** an arbitrary transformation — it's structural and provably equivalent to running the original NFA backwards over the byte sequence. Both RE2 and the Rust `regex` crate use this approach.

The reverse NFA is a separate `Nfa` value, not a flag on the forward NFA. The lazy DFA construction in §8 builds two DFAs from the two NFAs.

### Anchored vs unanchored variants

For each NFA (forward and reverse), we compile **two variants**:

- **Anchored**: the NFA's start state requires `^` or beginning-of-input.
- **Unanchored**: the NFA's start state has an implicit `.*?` prefix that allows match starts at any position.

Why both: anchored matching is the path for `find_first_at(text, pos)` and similar position-aware APIs. Unanchored is the path for `find_first(text)` scanning. They have different DFA shapes — the unanchored variant has the prefix `.*?` baked in, which means more states but only one scan needed.

So at compile time, for a `NoBacktracking`-classified pattern, we build **four NFAs**:

1. Forward anchored
2. Forward unanchored
3. Reverse anchored
4. Reverse unanchored

Memory cost is small (NFAs are compact). Compile-time cost is also small (Thompson construction is linear in pattern size).

### Module: `c2/nfa.rs`

```rust
pub struct Nfa {
    states: Vec<NfaState>,
    start: NfaStateId,
    accept: NfaStateId,
    /// per-state transitions; indexed by ByteClassMap class id
    transitions: Vec<Vec<(ByteClassId, NfaStateId)>>,
    /// epsilon transitions with priority (for greedy/lazy)
    epsilons: Vec<Vec<(NfaStateId, EpsilonPriority)>>,
    /// capture group tags on transitions (used only by the capture pass)
    capture_tags: Vec<Vec<CaptureTag>>,
}

pub struct CompiledC2Program {
    pub byte_class_map: ByteClassMap,
    pub forward_anchored: Nfa,
    pub forward_unanchored: Nfa,
    pub reverse_anchored: Nfa,
    pub reverse_unanchored: Nfa,
    pub num_capture_groups: usize,
    pub named_groups: Vec<(String, usize)>,
}
```

---

## 7. Sparse-set Pike-VM

### Why sparse-set

The naive Pike-VM tracks "currently active NFA states" as a hash set or bitmap. Hash set is O(1) amortized but has terrible constants. Bitmap is O(n) per character to scan. Russ Cox's sparse-set design (originally from Briggs and Torczon, 1993) gives O(1) **worst-case** for both `add(state)` and `clear()`, with no hashing and no bitmap scanning. It's the right structure for NFA simulation.

### How it works

Two arrays of size `num_states`:

```
sparse: [u32; num_states]  // sparse[state] = position in dense (only valid if dense[sparse[state]] == state)
dense:  [u32; num_states]  // dense[i] = state at position i; valid for i in 0..len
len:    usize
```

`add(state)`: check `sparse[state] < len && dense[sparse[state]] == state`. If true, already present, do nothing. Else write `dense[len] = state; sparse[state] = len; len += 1`.

`clear()`: `len = 0`. (No memory wipe needed because the validity check uses both arrays.)

`contains(state)`: same as the add check.

`iter()`: walks `dense[0..len]`.

This is the foundation. Every NFA state set in the Pike-VM is a sparse set. Cache-friendly, allocation-free per character, and O(1) for the operations that matter.

### Pike-VM execution loop (forward, no captures)

```
function pike_vm_match(nfa, byte_class_map, input):
    current_set = SparseSet::new(nfa.num_states)
    next_set = SparseSet::new(nfa.num_states)
    add_with_epsilons(current_set, nfa.start)
    matched = false

    for byte in input:
        cls = byte_class_map.class_of(byte)
        for state in current_set.iter():
            for (transition_cls, target) in nfa.transitions[state]:
                if transition_cls == cls:
                    add_with_epsilons(next_set, target)
            if state == nfa.accept:
                matched = true
        swap(current_set, next_set)
        next_set.clear()

    for state in current_set.iter():
        if state == nfa.accept:
            matched = true

    return matched
```

`add_with_epsilons` adds a state and recursively adds all states reachable from it via epsilon transitions, respecting greedy/lazy priority. This is the only place priority matters — at NFA entry to a state, the order of epsilon-following determines whether we end up at the greedy or lazy accept first.

### Role in C2

The Pike-VM is the **production NFA simulator**. It's not a prototype, it's not a stepping stone, it's not "simplest possible" — it's the permanent algorithm RE2 and the Rust `regex` crate use as their NFA execution engine. It serves three roles in C2:

1. **The fallback engine** when the lazy DFA cache exhausts and can't construct a new state without evicting too much. Pike-VM is still O(nm) so this is a graceful degradation, not a cliff.
2. **The capture recovery pass** (§9). When the DFA finds a match span, a Pike-VM run over just that span (with capture tags enabled) recovers the capture positions.
3. **The first runnable C2 engine** (in the phased plan, §15) — before the lazy DFA lands, the Pike-VM alone is wired into dispatch. This is not a prototype; it ships production-quality and stays in production after the DFA lands as the fallback.

### Module: `c2/pike.rs`

```rust
pub struct PikeVm {
    nfa: Arc<Nfa>,
}

impl PikeVm {
    pub fn new(nfa: Arc<Nfa>) -> Self { /* ... */ }

    /// Returns true if the input matches.
    pub fn is_match(&self, input: &[u8]) -> bool { /* ... */ }

    /// Returns the leftmost match span, or None.
    pub fn find_first(&self, input: &[u8]) -> Option<(usize, usize)> { /* ... */ }

    /// Returns all non-overlapping match spans.
    pub fn find_all(&self, input: &[u8]) -> Vec<(usize, usize)> { /* ... */ }

    /// Capture recovery: given a known match span, returns capture group positions.
    /// Used by the two-pass DFA path (§9). Bounded to the matched span.
    pub fn recover_captures(
        &self,
        input: &[u8],
        match_start: usize,
        match_end: usize,
    ) -> Vec<Option<(usize, usize)>> { /* ... */ }
}
```

Sparse sets are owned by the `PikeVm` and reused across calls (allocation-free per match, allocation-once per `PikeVm` instance). This is `&self` not `&mut self` because the sparse sets live in a `RefCell` or are constructed per-call from a pool — the design choice between these is an implementation detail of step 4, but the public API is `&self`.

---

## 8. Lazy DFA cache

### What

A DFA where states are constructed **lazily**, on demand, from the NFA. The first time the simulator needs a transition `(state, byte_class) → ?`, it computes the next NFA state set, looks it up in the cache, returns the cached DFA state if present, or constructs a new one and inserts it.

Two DFAs are built per pattern:

- **Forward DFA** from the forward NFA (anchored and unanchored variants).
- **Reverse DFA** from the reverse NFA (anchored and unanchored variants).

So a `NoBacktracking` pattern has up to **four** lazy DFAs in memory after compilation, sharing the byte-class map and dispatching off the NFA structure built in §6.

### State representation

A DFA state is a sorted set of NFA state IDs (the "subset construction" classic). For lookup we need a hashable representation:

```rust
struct DfaStateKey {
    /// sorted, deduplicated NFA state IDs (the closure under epsilons)
    nfa_states: SmallVec<[NfaStateId; 8]>,
}
```

The DFA state itself stores its transition table:

```rust
struct DfaState {
    /// transitions[byte_class] = next DFA state ID (or NONE)
    transitions: Vec<DfaStateId>,
    /// is this an accept state?
    is_accept: bool,
}
```

Transitions are indexed by byte-class id, so the table size per state is `num_classes`, not 256.

### Cache structure

```rust
pub struct LazyDfa {
    nfa: Arc<Nfa>,
    byte_class_map: Arc<ByteClassMap>,

    /// All known DFA states.
    states: Vec<DfaState>,

    /// Lookup from NFA-state-set to DFA state ID.
    cache: HashMap<DfaStateKey, DfaStateId>,

    /// State count limit before fallback.
    state_limit: usize,

    /// Has the cache been cleared at least once during this run?
    /// (used to decide when to give up and fall back to Pike-VM)
    cleared_count: u32,
}
```

### Cache eviction policy

The decision: **clear-on-overflow with retry budget**, mirroring the Rust `regex` crate.

When the cache hits `state_limit`:
1. Clear the entire cache (`states.clear(); cache.clear()`).
2. Increment `cleared_count`.
3. If `cleared_count > MAX_CLEARS_PER_RUN` (e.g., 3), permanently fall back to the Pike-VM for the rest of this match operation.
4. Otherwise, restart the DFA simulation from the NFA start state and resume from the current input position.

Why this policy:
- LRU is more memory-overhead than the bound is worth. The cache is small.
- "Clear and retry" is what RE2 and the regex crate do.
- The retry budget prevents thrashing on adversarial inputs that would force constant clearing.
- When fallback happens, Pike-VM still gives O(nm) — never O(2ⁿ). Graceful.

### Default state limit

Initial value: `2 * 1024` DFA states. This is the regex crate's default order of magnitude. Tunable per `Regex` via the existing builder pattern (e.g., `RegexBuilder::dfa_size_limit(bytes)`).

### Lazy DFA simulation loop (forward, no captures)

```
function lazy_dfa_match(dfa, input):
    state = dfa.start_state()
    matched_end = NONE
    pos = 0

    while pos < input.len():
        byte = input[pos]
        cls = dfa.byte_class_map.class_of(byte)
        next_state = dfa.transition(state, cls)  // may construct new state lazily

        if next_state == NONE:
            // dead state, no further match possible
            break

        if dfa.cache_overflow_pending():
            return fall_back_to_pike_vm(input, pos)

        state = next_state
        if dfa.is_accept(state):
            matched_end = pos + 1
        pos += 1

    return matched_end
```

The actual simulation is a tight inner loop. The byte-class lookup is one indexed load. The transition lookup is one indexed load (transition table indexed by class id). The accept check is one boolean load. This is the SOTA hot path.

### Module: `c2/dfa.rs`

```rust
pub struct LazyDfa {
    /* fields from above */
}

impl LazyDfa {
    pub fn new(nfa: Arc<Nfa>, byte_class_map: Arc<ByteClassMap>, state_limit: usize) -> Self;

    /// Returns the end position of the leftmost match, or None.
    pub fn find_match_end(&mut self, input: &[u8], start: usize) -> Option<usize>;

    /// Used by the reverse DFA: returns the *start* position given a known end.
    pub fn find_match_start(&mut self, input: &[u8], end: usize) -> Option<usize>;
}
```

The forward DFA returns end positions; the reverse DFA returns start positions. The dispatch layer (§11) combines them.

Note: `&mut self` here because the cache mutates. In production this lives behind a `Mutex` or thread-local pool. The choice between mutex and pool is an implementation detail and doesn't affect the public regex API (`Regex::find_first` etc. remain `&self`).

---

## 9. Two-pass capture recovery — DECIDED

This is the architectural decision recorded in this document. **The DFA never tracks capture group positions.** Captures are recovered after-the-fact via a small bounded Pike-VM pass over only the matched span.

### Why this approach (and why not tagged transitions)

**Two-pass advantages:**
- The DFA stays small. State space is bounded by the NFA's reachability, not multiplied by the number of capture groups.
- The cache stays compact. More patterns benefit.
- Capture recovery is **provably correct** by construction: it's the same Pike-VM that would have run on the whole input, just bounded to the matched span. Differential testing against the existing backtracking VM is straightforward.
- It's the SOTA approach: RE2 and the Rust `regex` crate both do this. Years of production validation.
- The cost of the second pass is bounded by the matched span, not the input size. For a typical match where the span is short relative to the input, this is negligible.

**Tagged transitions disadvantages:**
- The DFA state space grows because each state must distinguish "I got here via path P with captures (a, b)" vs "I got here via path P′ with captures (c, d)". This blows up the cache.
- Lazy DFA construction with tags is significantly harder to get right. The Rust `regex` crate explicitly avoided this for the lazy DFA path.
- Higher correctness risk on a first-pass implementation. Doesn't fit the SOTA-from-day-one bar.

### Algorithm

```
function find_with_captures(c2_program, input):
    // Pass 1: forward DFA finds the match end
    end = c2_program.forward_dfa.find_match_end(input, 0)
    if end == NONE: return None

    // Pass 2: reverse DFA finds the match start
    // (The reverse DFA scans backward over input[0..end] and returns the
    //  earliest position s such that input[s..end] matches the reverse pattern.)
    start = c2_program.reverse_dfa.find_match_start(input, end)
    assert(start != NONE)  // forward found a match, reverse must succeed

    // Pass 3 (only if captures requested): bounded Pike-VM over [start..end]
    captures = c2_program.pike_vm.recover_captures(input, start, end)

    return Match { start, end, captures }
```

Three passes total when captures are needed; two passes when they aren't. For `is_match` we stop after pass 1. For `find_first` (no captures) we run passes 1 and 2. For `captures` / `find_iter` we run all three.

### Why a reverse DFA at all (vs scanning forward from byte 0 with the Pike-VM)

The reverse DFA gives O(n) for start-of-match recovery. Without it, we'd need to either:
- Scan forward from byte 0 with the Pike-VM until we find a match — O(nm) and slower in practice
- Track all possible start positions in the forward DFA — blows up the state space

The reverse DFA is a one-time compile cost (small) plus a lazy state cache (also small) and gives the fastest possible start-of-match recovery. RE2 and the regex crate both do this.

### When the Pike-VM capture pass is too slow

Edge case: extremely long matched spans (think kilobytes of matched text). The Pike-VM pass over the matched span is O(matched_span_length × num_states). For most patterns this is fine. For pathological patterns it might not be. The fallback is the same as the DFA fallback: the existing backtracking VM. The dispatch layer will detect this and route accordingly.

This is an open question for the implementation phase, not the design phase: when do we decide a matched span is too long for the Pike-VM capture pass and route to the VM instead? Initial answer: never. The Pike-VM is bounded so it's always correct. Optimization can come later if benchmarks show it matters.

---

## 10. Anchored vs unanchored variants

Already described in §6. The compile-time output is four NFAs and four lazy DFAs:

| Variant | Used by |
|---|---|
| Forward anchored | `find_first_at(text, pos)`, `is_match_at`, anchored regexes (`^...$`) |
| Forward unanchored | `find_first(text)`, `find_all`, scanning patterns |
| Reverse anchored | Start-of-match recovery for anchored matches |
| Reverse unanchored | Start-of-match recovery for unanchored matches |

The dispatch layer picks which variant based on the API entry point.

Memory cost: small. Each NFA is compact and the DFAs are lazy (only states that get hit are constructed).

Compile-time cost: also small. Thompson construction is linear in pattern size; we do it four times for the four variants.

---

## 11. Engine dispatch boundary

### The seam

The dispatch decision lives in `rgx-core/src/c2/dispatch.rs`. The existing public API (`Regex::find_first`, `find_all`, `captures`, `is_match`, etc. in `lib.rs`) gets a one-line addition at the top of each method:

```rust
pub fn find_first(&self, text: &str) -> Result<MatchResult, RgxError> {
    // existing entry logic...
    if let Classification::NoBacktracking = self.program.classification {
        return c2::dispatch::find_first(&self.program.c2, text.as_bytes());
    }
    // existing VM path unchanged
    self.engine.find_first(text)
}
```

That's it. One conditional dispatch per API method. No other changes to the existing engine path.

### Why this seam

- **Backtracking VM path is unchanged.** Zero risk of regression on patterns that route to it.
- **Dispatch is a single branch on a stored enum.** No re-analysis at runtime.
- **C2 has its own internal API surface** (`c2::dispatch::find_first`, `find_all`, `is_match`, `captures`) that mirrors the public API but takes `&[u8]` and returns the same `MatchResult` / `Captures` types.
- **Cohabitation is provable.** Both engines produce the same types. The differential test suite (§13) verifies they produce the same values.

### Module: `c2/dispatch.rs`

```rust
pub fn is_match(c2: &CompiledC2Program, input: &[u8]) -> bool { /* ... */ }
pub fn find_first(c2: &CompiledC2Program, input: &[u8]) -> Option<MatchResult> { /* ... */ }
pub fn find_all(c2: &CompiledC2Program, input: &[u8]) -> Vec<MatchResult> { /* ... */ }
pub fn captures(c2: &CompiledC2Program, input: &[u8]) -> Option<Captures<'_>> { /* ... */ }
```

Each function decides internally:
- Whether to try the lazy DFA first or go straight to the Pike-VM (default: try DFA, fall back).
- How many passes to run (one for `is_match`, two for `find_first` no captures, three for `captures`).
- Whether to use anchored or unanchored variants (based on the entry point and any anchors in the pattern).

---

## 12. What the existing VM path does NOT lose

Listing this explicitly because the SOTA-first preference means we do not regress anything to ship C2:

- ✅ Backreferences, recursion, lookaround, conditionals, atomic groups, possessive quantifiers, `\K`, backtracking verbs, inline code blocks (Lua/JS/Rhai/native/wasm), `(?C)` callouts, branch-reset groups, Perl extended character classes, returned-capture subroutines — all stay on the existing VM path with zero behavior changes.
- ✅ Host integration layers (predicates, steering, events, async I/O, file-backed matching, `tail_file`) stay on the VM path. They're tied to the execution layer which C2 doesn't touch.
- ✅ All public API: `Match`, `Captures`, `RegexBuilder`, `RegexSet`, `RegexCache`, `BytesRegex`, safety limits (`set_max_steps`, etc.), `MatchSemantics`, `PartialMatchResult`, `CaptureLocations`, `escape()`, metadata accessors — unchanged. C2 plugs in below this surface.
- ✅ `MatchResult.code_result`, `find_first_numeric_with_code`, `replace_first_with_code`, etc. — unchanged. These are inline-code-block features and route to the VM.
- ✅ All existing 633 tests continue to pass without modification.

If anything in this list regresses, it's a merge blocker and the change is reverted. No exceptions.

---

## 13. Differential testing strategy

### The corpus

Three sources:

1. **The existing test suite.** Every pattern in `rgx-core/tests/`, `rgx-cli/tests/`, the doc tests, and the parity suites (`rgx-bench/tests/pcre2_parity.rs`). Roughly 633 tests today; that's the baseline.
2. **The PCRE2 parity corpus.** The `pcre2_parity_supported_*` tests in `rgx-bench` already cover representative supported patterns. Every classifier-positive pattern in this corpus must produce identical results on C2 and the existing VM.
3. **Random pattern generation.** A `proptest`-style harness that generates random patterns from the no-backtracking subset's grammar and runs both engines on random inputs. Targets correctness on edge cases the static corpus doesn't cover.

### The harness

A new `rgx-core/tests/c2_differential.rs` test file that, for every pattern in the corpus:

1. Compiles the pattern to a `Regex`.
2. If `program.classification == NoBacktracking`, runs the pattern on a fixed set of test inputs through:
   - The existing backtracking VM (forced via a debug-only escape hatch)
   - The C2 engine
3. Asserts the results are identical: same `is_match`, same `find_first` span, same `find_all` spans (in same order), same `Captures` (every group, including unmatched groups, produces the same `Option<(start, end)>`).
4. If results differ, the test fails with a precise diff.

### The merge gate

**Every C2 implementation commit must produce zero differential failures.** This is non-negotiable. If a commit reduces failures from N to 0, it's allowed. If it introduces any failure, it's reverted and reworked.

### Property tests

A separate file `rgx-core/tests/c2_proptest.rs` that uses `proptest` to generate:
- Random patterns from the no-backtracking grammar
- Random inputs

And asserts that VM and C2 produce identical results. Initial run budget: 256 cases per property (matches the existing property test budget). Increased to 4096+ for the final cutover.

### Continuous validation

The differential test runs as part of `cargo test -p rgx-core` and `./scripts/run-local-ci.sh`. There is no "C2 testing mode" that can be skipped. The differential corpus is the ground truth.

---

## 14. Benchmark strategy

### Existing infrastructure

The `target/benchmark-trends/` infrastructure already supports mode-scoped quick/full captures, label pairing, rolling history, and same-mode delta reporting. C2 plugs into this directly.

### New benchmark workloads

Add to `rgx-bench/benches/throughput.rs`:

| Workload | Why |
|---|---|
| Literal match (`hello`) | C2 should be at least as fast as existing memmem fast path |
| Alternation (`cat|dog|fish`) | DFA strength |
| Character class scan (`\d+`) | DFA strength |
| Capture-heavy (`(\w+) (\w+) (\w+)`) | Tests the two-pass capture recovery overhead |
| Long input small pattern (1MB log file, `ERROR`) | Tests cache locality |
| Pathological backtracking (`(a+)+b` on `aaaa…aaab`) | C2 should be O(n) where the VM is O(2ⁿ); huge win |
| PCRE2-comparable patterns from the parity suite | For the headline numbers |

### Reporting

Two captures per benchmark run, label-paired per the existing infrastructure:
1. **VM-only baseline** (force VM dispatch via the debug escape hatch)
2. **C2 hybrid** (default dispatch)

The delta report shows:
- Speedup factor on each workload
- DFA cache hit rate
- Pike-VM fallback rate
- Memory overhead per compiled pattern

### Success criteria for the C2 cutover commit

- Zero differential failures across the test corpus and the proptest harness
- Equal-or-faster on every workload above except pathological backtracking, where C2 must be **dramatically** faster (orders of magnitude)
- Memory overhead per compiled pattern ≤ 2× the existing VM bytecode (this is generous; in practice it should be smaller)

If these criteria aren't met, the cutover commit doesn't land. The C2 path stays opt-in until they are.

---

## 15. Phased implementation plan

Each step is its own commit. Each step is gated on differential tests passing. Each step ships production-quality code per the SOTA-first preference.

| Step | Module(s) added or modified | Differential gate |
|---|---|---|
| **0. This design proposal** | `docs/C2_NFA_DFA_DESIGN.md` (this file), CHANGES, MEMORY, BACKLOG, README index | N/A — doc only |
| **1. Pattern classifier** | `c2/mod.rs`, `c2/classifier.rs`, classification metadata on `Program`, classifier unit tests against the existing 633-test suite | Classifier output is verified against a hand-curated truth table for representative patterns. No runtime dispatch yet — the field is metadata-only at this stage so it can be validated in isolation. |
| **2. Byte-class equivalence partitioning** | `c2/byte_class.rs`, unit tests on representative patterns | Standalone correctness tests |
| **3. Forward + reverse Thompson NFA construction** | `c2/nfa.rs`, anchored and unanchored variants for both directions, unit tests on NFA structure | NFA structural correctness; epsilon closure correctness; capture tag placement correctness |
| **4. Sparse-set Pike-VM** | `c2/pike.rs`, `c2/dispatch.rs` initial wiring (Pike-VM path only, no DFA yet), `is_match` / `find_first` / `find_all` / `captures` all routed through Pike-VM for `NoBacktracking` patterns | **Differential gate active from this step onward.** Every classifier-positive pattern in the existing test suite must produce identical results on Pike-VM and the existing backtracking VM. New `c2_differential.rs` test file lands here. |
| **5. Lazy DFA cache (forward)** | `c2/dfa.rs` forward path, dispatch layer prefers DFA when available, falls back to Pike-VM on cache exhaustion or unsupported patterns | Differential gate; benchmark capture |
| **6. Lazy DFA cache (reverse)** | `c2/dfa.rs` reverse path, dispatch layer uses reverse DFA for start-of-match recovery, Pike-VM still used for capture pass | Differential gate; benchmark capture |
| **7. Literal prefix integration with C2 dispatch** | The existing memmem literal-prefix scan now feeds into C2's DFA dispatch the same way it feeds into the VM | Differential gate; benchmark capture |
| **8. Production cutover, benchmarks, Book chapter** | Final dispatch wiring, full benchmark sweep with label-paired captures, new `book/src/internals/nfa-dfa-engine.md` chapter documenting the design (per the two-track docs rule), `RUST_CODEBASE_ANALYSIS.md` updated to reflect C2 as a shipped engine | Differential gate; benchmark targets met (§14); zero regressions on the existing 633 tests |

**Estimated commit count**: 8 commits minimum for the happy path. Realistic: 12–15 commits including small fixes, doc touch-ups, and any architectural adjustments discovered during implementation.

**Estimated timeline**: multi-week. RE2 and the Rust `regex` crate took years cumulative; we're matching their *design* on the first pass, not their cumulative testing budget. The differential testing harness against the existing VM is what makes this feasible — every step is provably equivalent or it doesn't land.

---

## 16. Open architectural questions

These are decisions the design doc does NOT make. They're flagged here for resolution either before step 1 starts or during the relevant implementation step. None are blockers for steps 1–3, but some need answers before step 4.

| Question | When it needs an answer | My current lean |
|---|---|---|
| **Q1.** Should C2 support full Unicode case folding (`(?i)` over `café` matches `CAFÉ`) on the first pass, or only ASCII? | Step 3 (NFA construction) | Full Unicode. The existing VM already supports it (A7 shipped). C2 should match. Cost is encoding the case-fold table into NFA construction, which is mechanical. |
| **Q2.** Should `\b` and `\B` use Unicode word semantics or ASCII-only on the first pass? | Step 3 | ASCII-only first, Unicode follow-up. The Rust `regex` crate took this path. Unicode word boundaries are a separate compile-time pass and add complexity. ASCII covers the vast majority of patterns. |
| **Q3.** Should C2 support both `LeftmostFirst` and `LeftmostLongest` semantics, or only `LeftmostFirst` on the first pass? | Step 3 | LeftmostFirst only on first pass. This matches RE2's default and the regex crate's default. LeftmostLongest patterns route to the VM until C2 step N+1. |
| **Q4.** Should the lazy DFA cache live on the `Regex` (per-instance) or in a thread-local pool (shared across regexes)? | Step 5 | Per-instance, behind a `Mutex`. Simpler ownership. The regex crate uses a per-Regex cache. Thread-local pools are an optimization for high-concurrency scenarios; can come later if benchmarks show contention. |
| **Q5.** Default value for `dfa_size_limit`? | Step 5 | 2 MiB per direction (forward + reverse) per Regex, configurable via `RegexBuilder::dfa_size_limit(bytes)`. Matches the regex crate's default order of magnitude. |
| **Q6.** When the Pike-VM fallback is triggered mid-match, do we restart the entire match or resume from the current position? | Step 5 | Restart the entire match. Resume-mid-match is fragile because the DFA state and Pike-VM state aren't trivially convertible. Restart is correct by construction. The cost is one extra scan of the input, which is bounded. |
| **Q7.** Should we run BOTH engines in parallel during development (one as truth, the other as test) and assert equivalence at runtime, behind a debug feature flag? | Step 4 | Yes. Debug builds with a `debug-c2-equiv` feature flag run both engines and assert equivalence. Catches drift early. Disabled in release. |
| **Q8.** Should the classifier be exposed publicly as a `Regex` introspection method (`regex.uses_c2() -> bool`)? | Step 8 | Yes. Useful for users and for benchmarks. One-line addition to the public API. |
| **Q9.** Should `RegexSet` use C2 internally for its individual patterns? | Step 8 | Yes, but as a follow-up commit after the core C2 cutover. RegexSet currently runs each pattern as a separate Regex; the dispatch will pick C2 automatically if classification allows. No special integration needed. |
| **Q10.** Long matched spans and Pike-VM capture pass cost — should we have a fallback to VM for capture recovery on long spans? | Step 4 (when capture pass lands) | No on the first pass. The Pike-VM is bounded so it's always correct. Optimization comes later if benchmarks show it matters. |

---

## 17. Risks and mitigations

| Risk | Mitigation |
|---|---|
| **Subtle correctness bug in NFA construction or Pike-VM** | Differential test suite against the existing VM is the merge gate. Every step ≥ 4 is gated on zero differential failures. |
| **DFA cache thrashing on adversarial inputs** | "Clear and retry" fallback policy with a retry budget; permanent fallback to Pike-VM after the budget is exhausted. Pike-VM is O(nm) so worst case is still bounded. |
| **Capture recovery is wrong for unmatched groups** | Differential test specifically checks `Option<(start, end)>` for *every* group, including unmatched ones. The bounded Pike-VM pass is structurally identical to the full Pike-VM that would have run on the whole input, so correctness is by construction. |
| **C2 is slower than the existing VM on some workload we didn't anticipate** | Benchmark capture in step 8 includes label-paired comparisons with the VM-only baseline. If C2 is slower on any workload, the cutover doesn't land for that workload — the dispatch layer can have per-pattern-shape policies. |
| **Step 4 (Pike-VM) is hard and takes longer than expected** | Sparse-set Pike-VM is well-documented (Russ Cox's articles, RE2 source, regex crate source). The implementation is bounded. If it overruns, the SOTA-first principle still applies — we don't ship a worse Pike-VM to save time. |
| **Lazy DFA construction is hard and takes longer than expected** | Same. Well-documented. |
| **Reverse NFA / reverse DFA correctness** | Differential testing covers this. The reverse DFA is logically just the forward DFA running on a structurally-reversed NFA over the input read backward. |
| **Adding C2 modules increases compile time noticeably** | Acceptable up to a point. Mitigation: keep C2 modules small and feature-gated if necessary. The goal is fast runtime, not fast compile time. |
| **Memory overhead per compiled pattern grows beyond 2× existing** | Benchmark gate in step 8 catches this. If overhead is too high, the lazy DFA's state limit is reduced or the cache is shared across regexes (Q4). |

---

## 18. Out of scope for this document (and this project phase)

These are explicitly NOT addressed here:
- **C1 (JIT compilation).** Comes after C2. C2 makes C1 more valuable because then JIT has two engines to target.
- **Multi-pattern compilation in one DFA** (the regex crate's `RegexSet` internal optimization). Possibly later, after C2 cutover.
- **DFA serialization** (compile DFA once, save to disk, load on startup). The regex crate has this; it could come much later.
- **GPU offload, SIMD-accelerated DFA dispatch, multi-character batching.** Speculative.
- **Anchored/unanchored DFA sharing of state subgraphs.** Memory optimization for later if needed.

---

## 19. References

- Russ Cox, "Regular Expression Matching Can Be Simple And Fast" (2007) — the foundational article that introduced the modern thinking. https://swtch.com/~rsc/regexp/regexp1.html
- Russ Cox, "Regular Expression Matching: the Virtual Machine Approach" (2009) — the Pike-VM design. https://swtch.com/~rsc/regexp/regexp2.html
- Russ Cox, "Regular Expression Matching in the Wild" (2010) — RE2's lazy DFA, capture handling, byte-class equivalence. https://swtch.com/~rsc/regexp/regexp3.html
- The Rust `regex` crate source, particularly the `regex-automata` sub-crate, which contains the production implementation of every technique described here. https://github.com/rust-lang/regex
- Andrew W. Appel, "Modern Compiler Implementation in ML" — Thompson construction reference.
- Briggs and Torczon, "An efficient representation for sparse sets" (1993) — the sparse-set data structure used by the Pike-VM.
- The existing RGX backtracking VM in `rgx-core/src/vm.rs` — the source of truth for differential testing.

---

## 20. Sign-off

This document blocks all C2 implementation work until the user signs off.

**Reviewer**: Richard DJE

**Sign-off**: ☐ Approved as-is &nbsp;&nbsp; ☐ Approved with the following changes &nbsp;&nbsp; ☐ Needs revision

**Notes**:

(reviewer fills in)
