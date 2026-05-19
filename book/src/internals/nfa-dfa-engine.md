# The NFA/DFA Hybrid Engine (C2)

The previous chapter introduced the backtracking VM — the engine that has been the heart of RGX since day one. This chapter is about its companion: a **Thompson NFA + lazy DFA hybrid** that ships in parallel with the VM and takes over for the patterns where it can deliver a strict speedup.

The hybrid is internally referred to as **C2** — the second engineering improvement track on the RGX roadmap. It is fully shipped and exercised by the entire test suite. This chapter explains what it is, why it exists, and how the dispatch decision is made.

## Why have two engines

Backtracking VMs and DFAs have complementary strengths.

A backtracking VM is the only model that can express the full PCRE2 feature surface — backreferences, lookaround, recursion, embedded code blocks, atomic groups, possessive quantifiers, and the family of backtracking verbs (`(*COMMIT)`, `(*SKIP)`, `(*PRUNE)`, …). These features fundamentally need a thread of control that can rewind. The cost is that pathological patterns can backtrack exponentially: `(a+)+b` against `"aaaaaaaaaaaaa"` is the textbook example, and any backtracking engine that doesn't enforce a step limit will hang on inputs like that.

A DFA gives the opposite trade. It guarantees linear time on every input and never backtracks, but it cannot express any of the features above. It can only handle the regular subset — what an undergraduate compiler textbook calls a regular language. For that subset it is faster than any backtracking implementation, often by an order of magnitude on inputs where the match is sparse.

The Rust `regex` crate, RE2, and Hyperscan all use this same idea: pick the engine that can handle the pattern, and prefer the DFA whenever it's available. RGX does the same — but RGX also keeps the backtracking VM permanently in place because the features above are part of the value proposition. The hybrid is **additive**, not a replacement.

## What "C2" is

C2 is the cluster of code under `rgx-core/src/c2/`:

```text
rgx-core/src/c2/
├── classifier.rs       Pattern → "in the no-backtracking subset?" decision
├── byte_class.rs       Byte equivalence partitioning shared by NFA + DFA
├── nfa.rs              Forward + reverse Thompson NFA construction
├── pike.rs             Sparse-set Pike-VM with capture tracking
├── dfa.rs              Lazy DFA cache (subset construction)
└── program.rs          CompiledC2Program — the assembled artifact
```

It exports two execution engines and one classifier:

| Component | What it does | When it runs |
|-----------|--------------|--------------|
| **Classifier** | Tags an AST as `NoBacktracking` (regular subset) or `NeedsVm { reason }`. | Compile time, every pattern. |
| **Pike-VM** | Sparse-set NFA simulator. O(nm) bound, no backtracking. | Run time, for nested-quantifier patterns. |
| **Lazy DFA** | Subset construction over the Thompson NFA, transitions cached on demand. | Run time, for patterns without zero-width assertions or lazy quantifiers. |

The pieces are wired into the public `Regex` API through a 3-tier dispatch chain (described below). Patterns that fall outside the no-backtracking subset never reach C2 at all — they continue to run on the backtracking VM unchanged.

## The no-backtracking subset

The classifier (`c2/classifier.rs`) walks the AST once and decides whether C2 can handle the pattern. The exclusions are all features that need a backtracking thread of control or runtime context the NFA can't track:

- **Backreferences** (`\1`, `\k<name>`, `\k'name'`, relative `\k<-1>`) — a DFA cannot remember a captured substring and compare against it later.
- **Recursion and subroutines** (`(?R)`, `(?1)`, `(?&name)`, `(?P>name)`) — needs a call stack.
- **Lookaround** (`(?=…)`, `(?!…)`, `(?<=…)`, `(?<!…)`) — context-dependent.
- **Atomic groups** (`(?>…)`) and **possessive quantifiers** (`a*+`, `a++`, `a?+`) — these are *defined* in terms of backtracking suppression, so they only have meaning in a backtracking engine.
- **Inline code blocks** (`(?{lua:…})`, `(?{js:…})`, `(?{native:…})`) — they invoke host code mid-match.
- **Backtracking verbs** (`(*COMMIT)`, `(*SKIP)`, `(*SKIP:name)`, `(*PRUNE)`, `(*THEN)`, `(*ACCEPT)`, `(*MARK:name)`) — semantics are defined in terms of backtracking interactions.
- **`\K`** — moves the match start retroactively, which the NFA cannot model.

If none of those appear in the AST, the classifier returns `NoBacktracking` and the compile pipeline builds a `CompiledC2Program` alongside the bytecode for the existing VM.

## The CompiledC2Program

`c2/program.rs::CompiledC2Program` is the C2 artifact for a single pattern. It holds **four Thompson NFAs**, one shared byte-class equivalence map, and a small handful of dispatch hints:

```text
pub struct CompiledC2Program {
    pub byte_class_map: ByteClassMap,
    pub forward_anchored: Nfa,
    pub forward_unanchored: Nfa,
    pub reverse_anchored: Nfa,
    pub reverse_unanchored: Nfa,
    pub num_capture_groups: u32,
    pub c2_prefix_byte: Option<u8>,
    pub c2_has_nested_quantifier: bool,
}
```

The four NFA variants exist because different dispatch paths need different things:

- The **forward anchored** NFA is used at every scan position by both Pike-VM and DFA `find_first` / `find_all`.
- The **forward unanchored** NFA wraps the pattern with a lazy `(?s:.)*?` prefix. Pike-VM `is_match` uses it so a single pass over the input can answer the boolean question; the DFA-tier `is_match` also uses it now — a `LazyDfa` built over this NFA is walked once per call to answer `is_match` in O(n) instead of O(n × candidate_positions).
- The **reverse anchored** NFA backs the reverse half of the reverse-DFA pipeline via `LazyDfa::find_match_start_at_reverse` (and its bounded variant `find_match_start_at_reverse_bounded`). Both `find_first` and `find_all` now run on the pipeline: **forward-unanchored DFA (stop at first accept) → reverse-anchored DFA (leftmost start) → forward-anchored DFA (greedy end) → Pike-VM captures**. The third pass is what lets greedy tails like `a+` on `"baaab"` extend the match beyond the first-accept position (end=2) out to the greedy end (end=4); without it, the pipeline would truncate greedy matches. For `find_all`, each iteration seeds the forward walk at the current `pos` and bounds the reverse walk at `min_start = pos` so the second-and-later iterations can't re-locate the start inside a previously-consumed span. Dispatch only prefers the pipeline when no prefix hint exists (`c2_prefix_byte == None` and `PrefixFilter::None`) — otherwise the per-position anchored scan with `memchr` / byte-class skip acceleration wins on prefix-rich patterns because it jumps directly to candidate positions. The **reverse unanchored** NFA is reserved for follow-up work.

The byte-class map is the most subtle piece. The naive transition table for a Thompson NFA uses 256 entries per state (one per possible byte), which blows up memory on patterns with hundreds of states. RGX partitions all 256 byte values into a small number of *equivalence classes* — bytes that the pattern treats identically — and indexes the transition table by class instead. For `[a-z]+`, the partition is just two classes: "lowercase letter" and "everything else". For a pattern that uses `\d`, `\w`, and `[A-F]`, the partition might have ten classes. The map is stored once per pattern and shared by all four NFAs and any DFA that derives from them.

`c2_prefix_byte` is the optional first literal byte the match must start with. When present, the dispatch loop uses `memchr::memchr` to jump directly to the next candidate position instead of trying every offset. `c2_has_nested_quantifier` is the Pike-VM dispatch heuristic — see "Dispatch decisions" below.

## The sparse-set Pike-VM

`c2/pike.rs` implements the simulator. The data structure is a **sparse set** in the Briggs-Torczon style: O(1) insert, O(1) membership test, O(1) clear. Two sets — `current` and `next` — hold the threads alive at the current and next byte position.

The simulation loop is the textbook Pike-VM:

```text
1. Epsilon-close the start state into `current`.
2. For each input byte:
   a. Look up the byte's equivalence class.
   b. For each thread in `current`, follow byte-class-matching transitions
      and epsilon-close the targets into `next`.
   c. Swap `current` and `next`; clear `next`.
3. If the accept state was ever in `current`, the pattern matched.
```

The crucial detail is that **slot order in the sparse set encodes priority order**. Epsilon-closure walks edges in priority order, so threads added first are leftmost-first winners. When the accept state appears in `current`, only threads at slot positions ≤ accept's position survive — threads at higher positions were added later (during a closure that happened after the accept edge was walked) and have strictly lower priority. Killing them is what gives lazy quantifiers their shortest-match semantics. The same trick gives greedy quantifiers leftmost-longest semantics: in the greedy NFA the loop edge has higher priority than the exit edge, so the accept appears last in the closure and all threads survive.

The Pike-VM also tracks captures. Each thread carries a capture buffer alongside its state ID. Tagged epsilon edges (`save_start(g)` / `save_end(g)`) clone the buffer, write the current position into the appropriate slot, and recurse with the modified copy. The common case (untagged edges) passes the buffer through by reference with no allocation.

## The lazy DFA

`c2/dfa.rs` is the subset construction layer. A DFA state is a *set* of NFA states; transitioning the DFA by one byte means transitioning every NFA state in the set and unioning the results, then epsilon-closing.

Computing every reachable DFA state ahead of time is wasteful — most patterns visit only a tiny fraction of their state space on any given input. The lazy DFA computes states **on demand** and caches them in a hash table:

```text
LazyDfa {
    nfa: Arc<Nfa>,
    bcm: Arc<ByteClassMap>,
    states: Vec<DfaState>,                   // index → state metadata
    cache: HashMap<DfaStateKey, DfaStateId>, // NFA state set → DFA index
    state_limit: usize,                      // 2048 by default
    num_classes: u16,
}

DfaState {
    transitions: Vec<DfaStateId>, // length = num_classes
    is_accept: bool,
    nfa_states: Vec<NfaStateId>,
}
```

The simulation hot path is two array lookups per input byte: one to get the byte's equivalence class, one to follow the cached transition. When the transition is missing, the simulator does the subset construction (epsilon-close the union of NFA target states), interns the resulting state in the hash table, and stores the new transition. Subsequent passes over the same DFA state pay only the array-lookup cost.

When the cache exceeds `state_limit` the DFA can't extend further. It signals exhaustion to the dispatch layer, which falls through to Pike-VM (or the existing backtracking VM, depending on the call). The default limit of 2048 is the same order of magnitude the Rust `regex` crate uses; in practice it covers nearly every realistic pattern.

The DFA has two architectural restrictions worth knowing:

1. **No zero-width assertions** (`^`, `$`, `\A`, `\z`, `\Z`, `\b`, `\B`, `\G`). Subset construction has no way to track context like "previous byte was a word character" inside a single DFA state. Patterns containing assertions are routed to Pike-VM instead.
2. **No lazy quantifiers**. Subset construction is leftmost-longest by nature — it has no priority order — so it cannot express `a+?` semantics. For `a+?` on `"baaab"` the DFA returns the full `aaa` whereas Pike-VM (and PCRE2) return just `a`. Patterns containing lazy quantifiers route to Pike-VM.

Both restrictions are checked at compile time by `is_c2_dfa_eligible` and the `c2_dfa` field is left empty when they fail. The DFA never produces an answer it can't justify.

## Captures: the two-pass trick

The DFA tells you *whether* the pattern matched and *where* it ended. It doesn't tell you what the capture groups were, because tracking captures inside the subset construction would multiply the state space by the number of distinct capture combinations and ruin the DFA's compactness.

The standard solution (used by the Rust `regex` crate, RE2, and now RGX) is **two-pass capture recovery**:

1. The DFA scans the input forward, finds the match end, and confirms a match exists at some scan position `start`.
2. The bounded Pike-VM is then run **at exactly that start position** via `pike_captures_at`. It re-runs the same NFA but tracks captures, and because the start is known the simulation is bounded by the match length, not the full input.

The cost is small in practice: for sparse-match patterns the DFA does the heavy lifting on the long input, and the Pike-VM only runs on the few positions where a match was confirmed. The capture cost is amortized over the DFA's per-byte savings.

## Dispatch decisions

The public `Regex::is_match`, `Regex::find_first`, and `Regex::find_all` go through a **3-tier dispatch chain**:

```text
                ┌──────────────────────┐
                │  Regex API call       │
                └──────────┬───────────┘
                           │
                           ▼
              ┌────────────────────────────┐
              │ Engine::should_dispatch_   │
              │       to_dfa()?            │
              └─────────┬──────────────────┘
                        │ yes
                        ▼
              ┌────────────────────────────┐
              │  Lazy DFA scan              │
              │  (PrefixScanner accelerated)│
              └─────────┬──────────────────┘
                        │ exhausted or ineligible
                        ▼
              ┌────────────────────────────┐
              │ Engine::should_dispatch_   │
              │       to_c2()? (Pike-VM)    │
              └─────────┬──────────────────┘
                        │ yes (nested quantifier)
                        ▼
              ┌────────────────────────────┐
              │  Sparse-set Pike-VM scan    │
              │  (PrefixScanner accelerated)│
              └─────────┬──────────────────┘
                        │ ineligible
                        ▼
              ┌────────────────────────────┐
              │   Existing backtracking VM  │
              └────────────────────────────┘
```

The DFA tier is preferred whenever the pattern is DFA-eligible. The DFA's per-byte cost is two array lookups; the existing VM's per-byte cost is the bytecode interpreter loop. For the patterns the DFA can handle, the DFA is strictly faster.

The Pike-VM tier is **conservative**. It only fires for patterns with **structurally nested quantifiers** like `(a+)+`, `(\w+\s+)+`, or `(?:foo|bar+)+`. Those are the patterns where the existing backtracking VM can blow up exponentially and where Pike-VM's O(nm) bound provides a strict improvement. Classifier-positive patterns *without* nested quantifiers — `\b\w+@\w+\.\w+\b`, `\d{3}-\d{2}-\d{4}`, `https?://\S+` — run efficiently on the existing VM by construction (no exponential risk) and the existing VM's per-trial cost is lower than Pike-VM's. Routing those through Pike-VM would be a measurable regression.

A few additional gates short-circuit the dispatch:

- If the existing VM has a `memchr::memmem::Finder` for a pure-literal pattern, neither the DFA nor Pike-VM can beat it. Both gates return immediately.
- If the user has set `set_max_steps` / `set_max_backtrack_frames` / `set_max_recursion_depth` on the `Regex`, both C2 paths are skipped — Pike-VM doesn't enforce those limits and the user explicitly asked for them.
- If the user has registered a `MatchEvent` observer, both C2 paths are skipped because the C2 engines don't emit structured events.

These checks are read on every API call, so toggling features after `Regex::compile` takes effect immediately.

## The PrefixScanner

Both C2 dispatch loops use a shared `PrefixScanner` to skip non-candidate scan positions. The scanner consults the existing VM's compile-time `PrefixFilter` and resolves it through one of five strategies:

| Filter | Skip strategy |
|--------|----------------|
| `Byte(b)` | `memchr::memchr(b, …)` — SIMD-accelerated |
| `Digit` | tight scalar loop testing `is_ascii_digit` |
| `Word` | tight scalar loop testing word characters |
| `Space` | tight scalar loop testing `is_ascii_whitespace` |
| `CharClass(id)` | tight scalar loop calling the program's class table |
| `None` | identity (every position is a candidate) |

The scanner is the reason `(\d{4})-(\d{2})-(\d{2})` running through the DFA dispatch is **31x faster than the pre-C2 baseline** and, in the dated `c2-step8-final` capture, **1.9x faster than PCRE2** — the DFA simulator only runs at byte positions that begin with a digit, instead of every position. (That 1.9× is pre-TDFA and stale; post-TDFA the same pattern's `find_all` is much faster — see [Measured impact](#measured-impact) and the freshness caveat in [Performance](./performance.md).)

## Differential testing

The 12-suite corpus in `rgx-core/tests/c2_pike_differential.rs` runs every C2-eligible pattern through both the C2 dispatch path and the existing backtracking VM and asserts byte-for-byte equivalence on `is_match`, `find_first`, `find_all`, and the capture-tracking variants of each. The corpus covers literals, sequences, alternations, quantifiers (greedy and lazy), anchors, capture groups, realistic patterns (dates, ISO timestamps, identifiers), and a small set of edge cases that caught real bugs during development.

The differential gate is **active across the entire 902-test rgx-core suite**. Every classifier-positive pattern in every test gets exercised through C2 dispatch first, then through the existing VM as a cross-check. Any divergence is a hard failure. The whole suite has been green at every C2 step since dispatch wiring landed.

## Performance impact

The numbers in [Performance](./performance.md) are kept up to date with each benchmark trend capture. As of the production cutover (label `c2-step8-final`), comparing absolute RGX `ns/iter` against the pre-C2 baseline (label `f708f7c`) on the standard benchmark corpus:

| Pattern | Pre-C2 | C2 step 8 | Speedup |
|---------|--------|-----------|---------|
| `test` (literal_simple) `find_all` 10K | 617902 | 16085 | **38x** |
| `\b\w+@\w+\.\w+\b` (email_basic) `find_all` 10K | 1471331 | 222342 | **6.6x** |
| `(\d{4})-(\d{2})-(\d{2})` (capture_groups) `find_all` 10K | 90738 | 2532 | **35x** |

And vs PCRE2 (10.45):

| Pattern | RGX vs PCRE2 (find_all 10K) |
|---------|-----------------------------|
| `test` | **3.16x faster** |
| `\b\w+@\w+\.\w+\b` | 2.59x slower |
| `(\d{4})-(\d{2})-(\d{2})` | **1.96x faster** |

The capture-groups win is pure DFA dispatch. The literal_simple win is the existing VM's `memmem::Finder` fast path being preserved by the dispatch gates. The email_basic improvement comes from the existing VM running unchanged plus the trend capture's natural variance — `\b\w+@\w+\.\w+\b` uses the existing backtracking VM by construction (no nested quantifier).

## The Tagged DFA (TDFA): capture recovery without a second pass

The two-pass capture-recovery trick described above — DFA locates the span, Pike-VM recovers captures over that span — is correct but pays for it. On capture-heavy patterns (`capture_groups`, `url_simple`) the Pike-VM second pass dominated wall time before the TDFA shipped: samply consistently attributed 30-60% of `find_all` self-time to `pike_match_at_with_captures` even with the DFA fully materialised.

The **Laurikari tagged DFA** propagates capture-position information through the DFA subset construction itself. Instead of two passes, the simulator walks the input once and reads capture positions directly out of per-state **tag registers** at match end. RGX's TDFA shipped on 2026-05-13 in `rgx-core/src/c2/tdfa.rs`. Design rationale in `docs/C2_TDFA_DESIGN.md`.

### How it works

- Capture group boundaries are emitted as tagged epsilon edges in the NFA — `CaptureTag::GroupStart(n)` / `CaptureTag::GroupEnd(n)` (see `c2/nfa.rs:292`). The Pike-VM already consumed them; the TDFA extends the same machinery to subset construction.
- Each TDFA state carries a register assignment per (NFA-state, tag) pair; each transition carries a list of `Copy { src, dst }` and `Save { dst }` register operations that fire when the transition is taken.
- Leftmost-first priority is preserved by following the NFA's existing epsilon slot order during determinization. No runtime priority comparison; the priority order is baked into the offline construction.
- The **Laurikari reorder rule** collapses equivalent register configurations into one state — patterns like `(a)+` produce a 3-state TDFA that stabilises after the second byte. Cache hits emit `Copy` RegOps to reshuffle freshly-allocated registers into the existing state's physical layout. Cycles in the copy graph (mutual register swaps) are broken via a fresh scratch register.

### Eligibility

The TDFA accepts a strict subset of the lazy DFA's eligibility set. `Regex::uses_tdfa()` reports the AST-level decision:

| Constraint | Rationale |
|---|---|
| Pattern is C2-eligible | No backreferences, lookaround, atomic groups, possessive quantifiers, etc. |
| At least one capture group | Zero-capture patterns hit the existing fast path in `recover_match_for_dfa_span` which is strictly faster |
| No lazy quantifier | DFA semantics can't express lazy capture priority |
| No `\b` / `\B` anywhere | Phase 2 first-pass conservatism; `TaggedDfa::try_build` rejects all assertions. Future phase lifts via the same `prev_byte_was_word` state extension the DFA uses |
| LeftmostFirst semantics only | POSIX leftmost-longest captures use a different determinization order (future work) |

For patterns where any of these conditions fail, dispatch falls through transparently to the existing DFA → Pike-VM two-pass path. The TDFA is purely additive.

### Dispatch

`Engine::try_dfa_find_first` and `try_dfa_find_all` try the TDFA path FIRST for capture-bearing C2 patterns. On success the TDFA returns the match plus captures in a single forward scan, skipping the Pike-VM second pass entirely. On TDFA failure (state cache overflow during construction, ineligible pattern) the function falls through to the existing pipeline.

The TDFA call is gated on `c2.num_capture_groups > 0` at the dispatch site — capture-free patterns skip the call entirely, keeping the hot path for benches like `url_simple` (no captures) unchanged.

### Measured impact

On the benchmark corpus (10 000 iterations × 11 samples, median ns/op, 1 KB inputs):

| Pattern | rgx (ns) | PCRE2 (ns) | rgx vs PCRE2 |
|---|---:|---:|---:|
| `find_all/capture_groups` (`(\d{4})-(\d{2})-(\d{2})`) | 12 | 561 | **47× faster** |
| `find_first/capture_groups` | 12 | 557 | 46× faster |
| `find_all/url_simple` (no captures, control) | 25 | 35 | 1.4× faster |
| `find_first/url_simple` (no captures, control) | 27 | 34 | 1.3× faster |

The TDFA delivers nearly all of its win on `find_all` — `find_first` was already so fast (matched at position 0, no scan needed) that the Pike-VM second pass overhead was below the measurement floor. `find_all` runs many matches across the full 1 KB input, accumulating Pike-VM overhead the TDFA eliminates per-match.

`url_simple` is included as a no-capture control: it's NOT TDFA-eligible (no captures), and the TDFA dispatch step is gated out for it. Its perf is unchanged from the pre-TDFA baseline, verifying the gate works.

## What's not in C2 yet

Three things on the C2 roadmap are deliberately deferred:

- **Reverse-DFA pipeline** — both `find_first` and `find_all` shipped on the pipeline. `Engine` builds a **forward-unanchored** and a **reverse-anchored** `LazyDfa` alongside the existing forward-anchored DFA. One O(n) forward-unanchored walk stops at the first accept (leftmost-first end hint), the reverse-anchored DFA walks backward to locate the leftmost start, and a final forward-anchored pass from that start captures the greedy extension (e.g. `a+` on `"baaab"` takes end from 2 out to 4). Pike-VM then recovers captures bounded to the known span, and its end offset cross-checks the pipeline's greedy end before the result is returned. The forward-unanchored NFA carries `lazy_prefix_states` / `body_entry` metadata so DFA subset construction re-runs the epsilon closure excluding lazy-prefix states once accept is in the set — the leftmost-first signal at the state-set level. `LazyDfa::find_first_accept_at` delivers the stop-at-first-accept contract; `LazyDfa::find_match_start_at_reverse_bounded(end, min_start)` lets `find_all` bound the reverse walk at the previous scan position so iteration N+1 can't locate a start inside iteration N's consumed span. Dispatch prefers the pipeline only when no prefix hint is available (`c2_prefix_byte.is_none()` and `PrefixFilter::None`) so prefix-rich patterns keep the faster memchr / byte-class skip scan. The **reverse unanchored** NFA is still reserved for follow-up work (unclear it buys anything on top of the current pipeline).
- **Multi-byte literal prefix (memmem)** — the `c2_prefix_byte` field is a single byte. Patterns like `https://` could in principle do a full `memmem::Finder` lookup, but the existing VM's `literal_finder` already handles pure literals and the dispatch gate routes them through the existing VM.
- **Smarter Pike-VM dispatch heuristic** — the nested-quantifier check is conservative. Pike-VM could in principle dispatch for some flat patterns where the existing VM has hidden weaknesses, but no benchmark currently demonstrates a pattern shape where it wins.

Each is tracked in `docs/BACKLOG.md`. C1 (JIT compilation) was the original "next major push" — that's now shipped (see [The JIT Compiler](./jit-compiler.md)) and the C2 follow-ups above are sequenced after it.

## Next: the JIT compiler

C2 is the second of three execution tiers RGX ships. The third — and the next chapter — is the **JIT compiler** (C1), which translates RGX bytecode into native machine code via Cranelift. Head to [The JIT Compiler](./jit-compiler.md) to see how RGX gets a constant-factor speedup on top of C2's algorithmic-class improvement.
