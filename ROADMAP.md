# ROADMAP
Live forward-looking tracker for rgx.

## Purpose
- Track what we are actively building, what is next, and what is deferred.
- Keep planning separate from shipped history (`CHANGES.md`).
- Reduce handoff friction across sessions.

## How to maintain this file
- Update at least when scope or priorities materially change.
- Keep entries concrete and implementation-oriented.
- Move items across sections (`Now`, `Next`, `Later`, `Done`) as status changes.
- Link validation and shipped details in `CHANGES.md` once completed.

## Status legend
- `planned`: scoped but not started
- `in-progress`: active implementation
- `blocked`: cannot proceed yet
- `done`: completed and validated (then move to `CHANGES.md`)

## Now (active)
### Tier-2 perf headroom + parity polish (post-C1/C2 cleanup)
- Status: `in-progress` (started 2026-04-12, after the C1 production cutover)
- Context: the C2 NFA/DFA hybrid (shipped 2026-04-11) and C1 JIT compilation (shipped 2026-04-12) closed the major-perf track. Both are default-on. The 4-tier dispatch chain (`DFA → Pike-VM → JIT → backtracking VM`) is in production and exercised by every test in the suite.
- Goal: pick off the smaller follow-up wins now that the major arcs are done. Three concrete focuses for this session:
  - **Reverse-DFA pipeline (C2 follow-up).** Foundation shipped 2026-04-12; `is_match` single-pass fast path shipped 2026-04-13; `find_first` onto pipeline shipped 2026-04-24 (morning); **`find_all` onto pipeline shipped 2026-04-24 (afternoon)**. The unanchored NFA tags its lazy-prefix states + body entry point, `LazyDfa` subset construction re-runs the epsilon closure excluding those tagged states once accept is in the set, `LazyDfa::find_first_accept_at` delivers the stop-at-first-accept contract, and `LazyDfa::find_match_start_at_reverse_bounded(end, min_start)` bounds the reverse walk so find_all can prevent iteration N+1 from locating a start inside iteration N's span. The 3-pass pipeline (forward-unanchored first-accept → reverse-anchored leftmost start → forward-anchored greedy end → Pike-VM captures) replaces the per-position scan **only** when the pattern has no prefix hint (`c2_prefix_byte.is_none()` and `PrefixFilter::None`). Prefix-rich patterns stay on the per-position scan with memchr / byte-class skip. Track closed.
  - **DFA negated-char-class semantics fix.** ✅ Shipped 2026-04-12 (`7d195a4`) — UTF-8 byte-category boundary oracles in `byte_class.rs` partition continuation / leading bytes into their own classes.
  - **A8 crate publishing prep.** Metadata + per-crate READMEs shipped 2026-04-13. `cargo publish --dry-run` on rgx-core now passes the metadata checks and surfaces the real blocker: `pgen` is a private-submodule path dep, not on crates.io. User decision pending on the pgen strategy (publish pgen to crates.io; vendor pgen's generated code; or make pgen-parser truly optional). **Binary rename (`rgx-cli` → `rgx`) shipped 2026-04-24** — `rgx-cli/Cargo.toml` carries a `[[bin]] name = "rgx"` section, so the crate still publishes as `rgx-cli` but installs as the binary `rgx`. User-facing doc examples in `README.md`, `docs/CLI_GUIDE.md`, `docs/USER_GUIDE.md`, `rgx-cli/README.md`, and `WARP.md` updated; historical validation logs in `CHANGES.md` / `MEMORY.md` left as-is.
- Validation: existing benchmark trend infrastructure (label-paired quick/full captures under `target/benchmark-trends/`) for the perf items; the differential gate for the correctness fix; `cargo publish --dry-run` for the publishing prep.

### PCRE2 parity program (features, speed, accuracy)
- Status: `in-progress`
- Goal: converge toward practical parity with PCRE2 in capabilities and runtime behavior.
- Scope:
  - maintain a compatibility matrix against PCRE2 feature areas
  - use differential tests to catch semantic mismatches
  - track benchmark parity trends in `rgx-bench`
  - baseline established: `docs/PCRE2_COMPATIBILITY_MATRIX.md` + `rgx-bench/tests/pcre2_parity.rs`
### Parser-independent engine maturity
- Status: `in-progress`
- Goal: continue delivering advanced regex semantics through AST-first paths while parser syntax catches up.
- Scope:
  - extend assertion/group behavior in VM/compiler
  - add API-level tests for behavior guarantees

### Parser completeness path (toward PGEN integration)
- Status: `in-progress`
- Goal: support advanced group/assertion syntax in parser path to match AST-first capabilities.
- Scope:
  - align parser tokenization/AST output with VM-supported constructs
  - keep parser behavior consistent with API tests
  - incrementally close remaining syntax gaps (numeric backreferences, conditionals, Unicode property classes, possessive quantifiers, and current recursion forms are now shipped)

### Parser interoperability contract and conformance harness
- Status: `in-progress`
- Goal: define and enforce a stable parser boundary so PGEN integration is seamless.
- Scope:
  - maintain a versioned parser interoperability contract
  - keep fixture-based parser conformance tests around the active parser boundary
  - enforce parse-success/compile-unsupported boundary checks for unintegrated runtime features
  - keep downstream integration guidance aligned to `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` and `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`

### Capability matrix hardening
- Status: `in-progress`
- Goal: document and test exactly what is shipped vs scaffolded.
- Scope:
  - maintain `docs/CAPABILITY_MATRIX.md` as source of truth for shipped-vs-scaffolded status
  - expand integration tests for user-facing APIs
  - keep docs synchronized with verified behavior

## Next (near-term)
### PCRE2 10.47+ downstream syntax alignment
- Status: `planned`
- Goal: prepare RGX for newer PCRE2 syntax that may arrive through the default PGEN parser path.
- Scope:
  - define RGX AST/interoperability handling for returned-capture subroutine forms such as `(?R(grouplist))`, `(?n(grouplist))`, `(?+n(grouplist))`, `(?-n(grouplist))`, `(?&name(grouplist))`, and `(?P>name(grouplist))`
  - decide explicit compile-boundary or runtime behavior for newer conditional forms such as `(?(VERSION[...])...)`, now that current recursion-condition variants `(?(R)...)` / `(?(Rn)...)` / `(?(R&name)...)` are shipped
  - extend Perl extended character classes `(?[...])` beyond the newly shipped grouped bracket/property/nested-ordinary/POSIX/shorthand/escaped-term subset, which now also includes nested ordinary bracket terms such as `[\dA-F]`, `[[:graph:]]`, and `[\p{L}]`, current control-literal escapes such as `\a`, `\b`, `\e`, `\f`, and control/octal/codepoint atoms such as `\cA`, `\040`, `\o{101}`, and `\x{41}`, with explicit runtime policy for wider set-expression forms and any further bare-term families
  - expand `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, and differential tests to reflect whichever boundary or support level is chosen

### Performance validation loop
- Status: `in-progress`
- Goal: tighten benchmark-driven optimization workflow.
- Scope:
  - run and track `rgx-bench` baselines against recent changes
  - keep the default local validation loop emitting a low-overhead quick trend summary under `target/benchmark-trends/`
  - preserve mode-scoped latest snapshots plus timestamped local history and surface delta summaries against the most recent prior archived capture from the same benchmark mode
  - keep mode-scoped rolling history summaries so the recent longitudinal ratio story is visible without manually opening individual archived captures
  - keep archived captures revision-aware with optional labels so longitudinal reports can be tied back to specific RGX commits or release candidates
  - keep label-paired quick/full summaries so the same RGX revision can be compared across low-overhead and bench-profile captures without manual report stitching
  - keep rolling label-pair history so shared-label quick/full comparisons can also be tracked across successive revisions without manual report stitching
  - preserve a higher-fidelity `full` mode for slower bench-profile captures when deeper measurement is needed
  - prioritize optimizations with measurable impact

### Embedded code-path expansion beyond phase 1
- Status: `planned`
- Goal: refine the post-phase-1 code-block surface so first-class inline languages and advanced reference-style backends are treated differently.
- Scope:
  - keep Lua and JavaScript as the primary shipped inline source-body languages
  - treat wasm as an advanced registered-module/reference-style path rather than the main everyday inline code-block UX target
  - decide later whether native should expand beyond the current Rust-API-only surface and whether wasm should grow beyond the new file-backed CLI module-registration path once the inline-language story is mature
  - only revisit richer wasm ABI/result work after the preferred inline-language expansion path is clearer

### Multi-language code-block runtime expansion
- Status: `in-progress`
- Goal: extend code-block runtime support beyond initial languages while preserving deterministic behavior and safety guarantees.
- Scope:
  - build on the now-shipped `lua`, `js` / `javascript`, and `rhai` inline/source-body slice
  - keep `lua`, `js`, `javascript`, and `rhai` aligned around the same source-body execution contract shape where practical
  - explicitly defer heavier embedded runtimes such as Python and Julia until after the current ergonomics/safety work on the shipped inline-language track
  - treat wasm and native as advanced reference-style backends rather than the primary model for new inline language growth
  - shared execution contracts, resource limits, and sandbox controls

## Next (near-term) — continued

### Performance: close the PCRE2 compile-time gap to <5x
- Status: `planned 2026-04-25`
- Goal: reduce the `Regex::compile` gap from ~1000-2000x slower to **<5x** of PCRE2 (PCRE2 baseline ~300-700ns; RGX target ~1.5-3.5µs from current ~1-2ms). Accuracy-preserving — no semantic changes, no PGEN bypass, no behavioural drift. The shipped fix-set must keep 100% of the test suite green and the PCRE2 conformance ratchet unchanged.
- Context: every technique below is mechanical / refactoring, not algorithmic. The eager construction work in `Engine::new` (4 NFAs, byte-class map, classifier, 3 DFA caches, optional JIT codegen) is the dominant cost — most patterns use only a subset of those artifacts on any given match. Source: surveyed 2026-04-25; the "within 5x" target was confirmed acceptable by the user as a realistic intermediate milestone before any compile-time-driven UX changes.
- Bench reference: see `target/benchmark-trends/latest.md` — `compile literal_simple` 1083x slower, `compile email_basic` 1956x slower, `compile capture_groups` 1971x slower. PCRE2 is the column to close against.

#### Techniques in priority order

1. **Lazy artifact construction in `Engine::new`** — ✅ **SHIPPED 2026-04-25**.
   - **What landed**: `c2_dfa`, `c2_forward_unanchored_dfa`, `c2_reverse_dfa`, and `jit_program` are now `OnceLock<Option<Mutex<...>>>` wrappers. `Engine::new` returns immediately after cloning the AST + program; each artifact is built on first access from its dispatch helper (`should_dispatch_to_dfa`, `should_dispatch_to_forward_unanchored_dfa`, `should_dispatch_to_reverse_dfa`, `should_use_jit`). Construction code unchanged — same `build_*_if_eligible` helpers, just wrapped behind `get_or_init`.
   - **Measured win**: median compile time reduced **27.6% on JIT-eligible patterns** (range 15.3-32.7%) per `compile_phase_split.rs` re-run 2026-04-25. Engine::new share of compile time dropped from 17-33% to 0.0-0.2%. Two non-eligible patterns (`alternation`, `anchor_complex`) showed no change (within bench noise) because `Engine::new` was already negligible for them. 1077 lib + 30 cli tests green; PCRE2 conformance ratchet preserved at 12,709 / 101.
   - **Combined with PGEN bottleneck**: total RGX-vs-PCRE2 compile gap on the bench corpus moved from ~1083-1971x slower to ~750-1450x slower — a real but bounded improvement (PGEN parse still 96-100% of the new compile budget). Confirms the PGEN-side work in PGEN-RGX-0073 remains the dominant remaining lever.

2. **Skip artifacts the dispatch path provably can't use** — `speed`, paired with #1.
   - **Win in what sense**: 1.5-3x further reduction on top of #1 for patterns that exit the C2/JIT eligibility check. Today `Engine::new` runs `is_c2_dfa_eligible(ast)` and `has_top_level_alternation(ast)` checks but still builds the byte-class map and all four NFAs. For patterns that fail these checks (top-level alternation, lookaround, captures with backrefs, lazy quantifiers, etc.) all of that work is dead code — those patterns will run on the backtracking VM regardless.
   - **How**: short-circuit `Engine::new` early when `is_c2_dispatch_eligible(ast)` returns false. Build only the bytecode `Program` and the existing `RegexVM`. Skip the byte-class map, NFAs, DFAs, JIT entirely. Already covered structurally by #1's lazy approach but worth an explicit early-exit so the `OnceLock`s never even fire.

3. **Defer JIT to second match call** — `speed`, targets one-shot CLI workloads.
   - **Win in what sense**: variable but big for `rgx "pat" "txt"` style invocations. Cranelift codegen takes meaningful time (a few hundred µs to a few ms depending on pattern complexity); for one-shot matches the codegen runs once and the JIT'd code runs once, so the codegen is pure overhead. Long-running services that match the same pattern repeatedly recoup the codegen on the first or second call and are unaffected after.
   - **How**: gate JIT codegen behind a `match_count >= JIT_WARMUP_THRESHOLD` (likely 1 — codegen on second call). Until then, use the existing C2/Pike-VM/backtracking-VM dispatch. The JIT'd path is already a 4-tier fallback so removing it from the first-call hot path is cosmetic.
   - **Note**: this could be ungated by an opt-in `RegexBuilder::eager_jit()` for callers who explicitly want zero per-call latency variance.

4. **Allocation cleanup in `compiler.rs` + `vm.rs` compile path** — `speed`, long-tail.
   - **Win in what sense**: estimated 1.2-2x compile-time reduction depending on how aggressive the cleanup. RGX's compile pipeline allocates heavily: many `Vec::push` loops without `with_capacity`, `HashMap` instead of small fixed maps, `String` where `&str` would suffice, `Box::new` on hot paths. Each allocation is a malloc; on micro-benchmarks of compile, they accumulate.
   - **How**: profile-guided. `cargo flamegraph -p rgx-bench --bench throughput -- compile_*` to identify the hottest allocation sites. Replace `Vec` with `SmallVec` where the upper bound is small (e.g., capture-group counts), pre-size `Vec::with_capacity` where the count is known, intern repeated `String`s, use arena allocators (`bumpalo` is in the dep tree) for the AST construction phase if it's a hot site.
   - **Risk**: low — pure refactoring, covered by the existing 1,077 lib tests + PCRE2 conformance ratchet.

5. **Trivial-pattern short-circuit AFTER PGEN parses** — `speed`, targets the very-simplest patterns.
   - **Win in what sense**: small absolute time savings on patterns like `"hello"`, `\d`, `\w+` — the kind of patterns that motivate the `literal_finder` fast path on the existing VM. Not a big win in median, but closes the worst case for one-line CLI usage.
   - **How**: after PGEN produces the AST, run a cheap classifier (`is_pure_literal`, `is_single_char_class`, etc.). For trivially-recognisable shapes, skip NFA/DFA/JIT construction entirely and stash a compact representation that only holds what `find_first_via_literal` / `find_first_via_char_class` need. **Crucially**: PGEN still parses; this only short-circuits *downstream* work. CLAUDE.md's "PGEN is the sole parser" rule is preserved.
   - **Risk**: medium — needs careful enumeration of "trivial" so an exotic semantic flag (e.g., `(?i)` case-insensitive) doesn't slip through and produce wrong matches. The trivial-classifier MUST be conservative; when in doubt, fall through to the full pipeline.

6. **Already shipped — `RegexCache` (B3)** — bookkeeping note, not new work.
   - The LRU compilation cache means repeated `Regex::compile(same_pattern)` calls return a cached `Engine` after the first hit. First compile still pays the cost; second-and-subsequent are O(1). Worth confirming via the cache hit/miss telemetry that real workloads benefit. Not on this critical path because the goal here is to reduce the *first* compile, not amortise.

#### Validation plan

- Bench delta: re-run `cargo bench -p rgx-bench --bench throughput -- compile` after each technique lands; record in `target/benchmark-trends/`.
- Test gate: 1,077 lib tests, 30 cli tests, PCRE2 conformance ratchet (`pass=12709`, `fail=101`).
- Regression check: ensure no `find_first` / `find_all` benchmark regresses by more than 5% against the current label (the lazy-init techniques can add first-call latency that shows up as a runtime regression in some benchmarks if measured carelessly).

#### Out of scope for this entry

- **Replacing PGEN with a hand-written regex parser** — would violate CLAUDE.md's "PGEN is the sole parser. No builtin parser fallback." policy. Ruled out by hard project rule.
- **Altering AST simplification semantics** — any AST rewrite that could change match results is out, even if it would speed compile up.

### Performance: close the PCRE2 gap to <10x
- Status: `planned`
- Goal: reduce the matching speed gap from ~20-60x to <10x for common patterns.
- Scope:
  - eliminate `ExecContext.text` Vec copy (switch to borrowed `&[u8]`)
  - pre-allocate and reuse capture and backtrack structures across match attempts
  - compile-time eliminate trace macros behind `#[cfg(feature = "trace")]`
  - opcode fusion for common sequences (Char+Char → string compare)
  - reduce per-opcode bounds checking overhead
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Performance target section

### SOTA algorithmic gaps not on the original C1/C2 roadmap
- Status: `surveyed 2026-04-25, not yet planned`
- Context: as of 2026-04-24 every algorithmic improvement that was on the planned C1+C2 roadmap is shipped (NFA/DFA hybrid, JIT, Pike-VM, prefix scanner for byte classes, reverse-DFA pipeline for find_first/find_all, DFA negated-class boundary fix). The remaining benchmark gaps vs PCRE2 are not closable by constant-factor cleanup alone — they reflect SOTA algorithmic techniques that exist in RE2 / Hyperscan / PCRE2-JIT and aren't yet in RGX. This section captures those gaps so they're visible and rankable.
- "Win in what sense" — each item below is graded by what it actually buys, measured against the current bench numbers in `target/benchmark-trends/latest.md` (RGX vs PCRE2 on `find_first` 10K: literal_simple 6.26x slower, email_basic 3.73x slower, capture_groups 1.92x faster). "Speed win" = expected reduction in median wall-clock latency on a target pattern class. "Memory win" = reduction in DFA cache pressure / steady-state RSS. None of these add features — they're pure perf.

#### Likely-big wins (order-of-magnitude on a target pattern class)

- **Inner-literal prefilter (RE2/Hyperscan style)** — `speed`, targets `find_first email_basic` (currently 3.73x slower) and similar.
  - **Win in what sense**: median latency on patterns with required interior literals. For `\b\w+@\w+\.\w+\b` the `@` is a required byte anywhere in any match. Today RGX runs the DFA over the entire input. With this, RGX would `memchr('@')` and only run the DFA at those candidate positions — one DFA walk per `@` instead of one per byte. Expected 3-10x speedup on email-style patterns; closes most of the email_basic gap.
  - **How**: extract required-literal sets from the AST during compile; pick the rarest as the prefilter (rarity estimated from byte-class entropy or measured). Reference: RE2's `Prog::PrefixAccel`, `regex-automata`'s prefilter API.
- **Aho-Corasick for top-level literal alternation** — `speed`, targets patterns like `cat|dog|bird|fish`.
  - **Win in what sense**: throughput on multi-literal alternations against large inputs. Today these patterns hit `has_top_level_alternation` and fall out of C2 dispatch entirely, landing on the backtracking VM at O(n × m) (n=input, m=alternatives). AC-automaton matches all m literals in a single O(n) sweep. For log-grep-style workloads (`ERROR|WARN|FATAL` over GB of logs) this is order-of-magnitude.
  - **How**: detect "top-level alternation of pure literals" at compile, build AC automaton, dispatch through it instead of falling back. Existing crate `aho-corasick` is a known good fit and already in the ecosystem.
- **SIMD-vectorized byte-class lookup** — `speed`, generic across all DFA-eligible patterns.
  - **Win in what sense**: throughput on the DFA hot loop and the PrefixScanner. SIMD compares 16-64 bytes per cycle vs RGX's scalar byte-by-byte. PCRE2-JIT uses NEON on aarch64 / SSE/AVX on x86 in exactly these spots. Expected 2-4x on DFA-bound workloads. No semantic change.
  - **How**: vectorize `PrefixFilter::Digit/Word/Space` and the byte-class lookup in `LazyDfa::transition`'s hot path. Use `std::simd` (stable in 1.95) or `wide` (already in the dep tree).

#### Medium wins (steady-state speedup, not order-of-magnitude)

- **Tagged DFA (Laurikari TDFA)** — `speed`, targets capture-heavy patterns.
  - **Win in what sense**: eliminates the Pike-VM second pass. Today the reverse-DFA pipeline finds the span in three DFA walks then runs Pike-VM bounded over `[start, end]` to recover captures. A tagged DFA tracks capture positions inline during the forward walk — no second pass. Expected 1.5-2x on capturing patterns. Note: RE2 uses TDFAs partially; full TDFA support is non-trivial to retrofit onto an existing lazy DFA.
- **Multi-byte literal prefilter via `memmem`** — `speed`, targets `find_first` on patterns with multi-byte literal prefixes.
  - **Win in what sense**: candidate density. `c2_prefix_byte` is one byte; for `https://` we currently `memchr('h')` over the input. Switching to `memmem("https://")` (Boyer-Moore-Horspool internally) drops candidate count from "every 'h'" to "every actual literal occurrence" — typically 10-100x fewer candidates on real inputs. Could explain a sizeable chunk of the `literal_simple find_first` 6.26x gap.
  - **How**: extend `c2_prefix_byte: Option<u8>` to `c2_prefix_literal: Option<Vec<u8>>`; pick `memmem::Finder` over `memchr` when length > 1. The VM already has `literal_finder: Option<memmem::Finder>` for the existing-VM path; the work is plumbing the same hint into the C2 dispatch.
- **DFA minimization** — `memory + speed (cache pressure)`, targets complex patterns.
  - **Win in what sense**: smaller cache, fewer cache-exhaustion fallbacks to Pike-VM, less RAM. RGX's lazy DFA caches every reached subset; equivalent states are not merged. Hopcroft minimization (or partition refinement on the lazy cache) would shrink state count, often 2-5x for medium-complexity patterns.
- **Materialized DFA for small patterns** — `speed`, targets steady-state for small DFAs.
  - **Win in what sense**: eliminates lazy-allocation overhead. Below a threshold (say, 64 states), build the full DFA upfront so the steady-state walk has zero allocation and zero locking overhead. The current lazy cache uses `Mutex<LazyDfa>` and per-byte cache lookup; a fully-materialized small DFA can be a flat lock-free array.

#### Smaller / unclear wins

- **Glushkov position-automaton** instead of Thompson — alternative NFA construction with smaller state count for some patterns. Whether this matters depends on input pattern shapes; needs measurement.
- **Anchored fast-paths** — when `^pattern$` is detected, skip the unanchored-prefix construction entirely. Need to verify whether RGX already does this; if not, mechanical fix.
- **Suffix-anchored backward scanning** — for `pattern\z`, match backward from end of input. Niche but easy when reverse-anchored DFA already exists.

- Source: surveyed 2026-04-25 in conversation with the user after the reverse-DFA pipeline track closed; conversation context noted that without these, the published benchmark gaps (especially `email_basic` and `literal_simple find_first`) cannot be closed by code-tidying alone.

### Host integration Layer 6: File-Backed Matching
- Status: `done` (core API); `tail_file` and CLI integration planned as follow-up
- Shipped: `match_file`, `match_file_lines`, `scan_file`, `scan_file_lines` with `FileMatch` struct; 5 tests.
- Remaining: `tail_file` (streaming/watching), mmap for large files, CLI `--file` / `--line-mode` flags.
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 6

### Host integration Layer 3: Match Steering
- Status: `done`
- Shipped: `SteerResult` enum with `Continue`, `Fail`, `Accept`, `Skip(usize)`, `Abort`; `ExecResult::Steer` variant; VM dispatch for all actions; native callback API support; 5 tests.
- Inline-language steering (Lua/JS/Rhai helpers) planned as follow-up.
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 3

### Host integration Layer 4: Structured Events
- Status: `done`
- Shipped: `MatchEvent` enum with 6 variants; `Regex::on_event(observer)` API; zero overhead when no observer; instrumented all scanning strategies, `SetAlternative`, `SaveEnd`, `try_backtrack`, `execute_inline_code_block`.
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 4

### Host integration Layer 5: Async/External I/O
- Status: `done`
- Shipped: continuation-passing with `MatchOutcome`, `MatchContinuation` (Send+Sync, owns all data), `ExecResult::Suspend`, `find_first_suspendable`, `resume`, `find_first_async`; zero sync overhead; correct under backtracking/recursion; 12 tests.
- Design: `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 5

## Later (strategic)

### GitHub Pages for The RGX Book
- Priority: `medium`
- Status: `blocked` (Pages on private repos requires GitHub Pro)
- Scope:
  - User plans to subscribe to GitHub Pro soon, which unlocks Pages on private repos.
  - **Re-add `.github/workflows/book.yml`** (was deleted to stop CI failures — git history has the working version, see commit that removed it).
  - Enable Pages in repo settings → Source: GitHub Actions.
  - Book will publish to `https://rdje.github.io/rgx`.

### Public release preparation
- Priority: `high` once codebase is ready
- Status: `planned`
- Scope:
  - **Publish `rgx-core` and `rgx-cli` to crates.io** (backlog item A8). Requires API stability decision and final review.
  - Open-source license + contribution guidelines.
  - Public README polish targeting first-time visitors.
  - Tag v0.1.0 release.

### Remaining PCRE2 feature gaps
The compatibility matrix is now at ~99% parity. JIT compilation has shipped as of the C1 production cutover (2026-04-12, default-on). A11 `(*SKIP:name)` shipped 2026-04-12. A13 VERSION conditionals shipped end-to-end 2026-04-13 (RGX-side + PGEN 1.1.10). No hard gaps remain; residual work is in the newer PCRE2 10.47+ advanced surface captured under "Next".

### Binding/runtime expansion (A9)
- Status: `deferred` (deprioritized 2026-04-09)
- Scope: production-ready external bindings and runtime targets (Python/Node/C) after core stability gates.
- Why deferred: the conventional argument for A9 ("10x user base because most regex users aren't Rust devs") is generic and doesn't fit RGX specifically. RGX's killer feature is host integration (predicates, steering, events, async I/O, embedded scripting), and that surface is hardest to translate cleanly across FFI — Python callbacks become GIL territory, the async story assumes Rust's model, and the "embed Lua inside a regex from Python" pitch is weaker than from C/C++ because Python users already have a scripting host. Meanwhile A9 is gated on A8 (publish), is `large` per language, and the maintenance tail (packaging, version skew, prebuilds, separate CI per binding) competes for time against engine work that strengthens the actual differentiator.
- Reactivation criteria: a real user or use case pulling for a specific binding. Without a demand signal, this is speculative work. If it does reactivate, **C bindings via cbindgen first** (cheapest, unlocks every other FFI host for free) — not Python.

## Done recently (snapshot)
- **Reverse-DFA pipeline: `is_match` single-pass fast path** (2026-04-13). `Engine::try_dfa_is_match` now tries the forward-unanchored `LazyDfa` first — one O(n) walk answers the boolean is_match query in a single pass instead of the per-position anchored loop. New `c2_forward_unanchored_dfa` field on `Engine` alongside the existing `c2_dfa` (anchored) and `c2_reverse_dfa`, same eligibility gate. Falls back to the per-position anchored scan on cache exhaustion. `find_first` / `find_all` stay on the per-position anchored path — the forward-unanchored DFA's leftmost-LONGEST subset-construction semantics diverge from leftmost-first for multi-match patterns, which would regress `a` on `"xaxa"` from `(1,2)` to `(3,4)`. A leftmost-first-aware unanchored NFA construction is the next step to extend the pipeline there.
- **PGEN 1.1.10 bump — A13 VERSION conditionals closed end-to-end** (2026-04-13). Submodule pointer moved from `ac2acb3` (1.1.9) to `8783757` (1.1.10). PGEN now accepts `(?(VERSION op X.Y)...)` as a conditional with a bare-text condition body; the RGX-side `parse_version_conditional` short-circuit (already shipped 2026-04-12) now runs for real. The three previously-`#[ignore]`'d integration tests in `parsing::tests::version_conditional_*` pass unmodified. `pgen-issues/PGEN-RGX-0016.yaml` marked closed.
- **A11 `(*SKIP:name)` named skip verb** (2026-04-12). `Regex::Skip` became `Skip(Option<String>)`, new `VerbSkipNamed = 0xA5` opcode with length-prefixed name operand, per-attempt mark registry on `ExecContext`, forward-progress guards at all 12 scan-loop sites. Completes the backtracking verb surface.
- **A13 VERSION conditionals — RGX-side** (2026-04-12). New `RGX_PCRE2_COMPAT_VERSION = (10, 47)` constant, `parse_version_conditional` helper in `parsing.rs`, compile-time short-circuit in `convert_conditional`. Integration tests gated on PGEN 1.1.10 (closed 2026-04-13, above).
- **C1 JIT compilation production cutover** (2026-04-12). All 9 steps (0–8) of the design doc plan complete. The `jit` Cargo feature is now default-on; existing users get the JIT for free at the next `cargo update`. The 4-tier dispatch chain (`DFA → Pike-VM → JIT → backtracking VM`) is in production and exercised by every test in the suite. Public design lives in `book/src/internals/jit-compiler.md` (new chapter, ~250 lines). With the new default, `cargo test -p rgx-core` runs 957 lib tests (= 695 baseline + 262 C1) — up from 695 baseline. Opt-out via `--no-default-features --features pgen-parser` still works (drops Cranelift entirely from the dependency closure, runs 695 baseline tests). See `CHANGES.md` 2026-04-12 entry for the full step-by-step history.
- **C2 NFA/DFA hybrid production cutover** (2026-04-11). All 9 steps (0–8) complete. Sparse-set Pike-VM, lazy DFA cache, byte-class equivalence partitioning, two-pass capture recovery, and the 3-tier dispatch chain wired into `Engine::find_first` / `find_all` / `is_match`. Patterns the DFA can handle run ~1.9x faster than PCRE2; pure-literal patterns ~3.2x faster. Public design lives in `book/src/internals/nfa-dfa-engine.md`.
- Extended Perl extended character classes again so nested ordinary bracket terms inside `(?[...])` now accept the current ordinary char-class atom subset, including representative shorthand/range, POSIX, and Unicode-property forms such as `(?[[\dA-F]])`, `(?[[[:graph:]]])`, and `(?[[\p{L}] - [\p{Lu}]])`, with parser-path, parser-contract, compiler/unit, and differential parity coverage.
- Extended Perl extended character classes again so the default path now also supports bare escaped literal/codepoint terms such as `\n`, `\t`, `\r`, `\x{41}`, `\x41`, and escaped operators like `\-` inside the shipped `(?[...])` subset, including differential parity coverage for hex/control-escape cases while still keeping broader remaining forms behind an explicit compile boundary.
- Extended Perl extended character classes again so the default path now also supports horizontal/vertical whitespace shorthands (`\h`, `\H`, `\v`, `\V`) inside the shipped `(?[...])` subset, including parser-path and differential parity coverage while still keeping broader remaining forms behind an explicit compile boundary.
- Extended Perl extended character classes again so the default path now also supports bare ASCII POSIX class terms such as `[:graph:]`, complemented `[:alpha:]`, and POSIX-class algebra cases like `(?[ [:alpha:] & [a-z\t] ])` inside the shipped `(?[...])` subset, including parser-path and differential parity coverage while still keeping broader remaining forms behind an explicit compile boundary.
- Extended Perl extended character classes again so the default path now also supports bare control escapes like `\cA` and bare octal escapes like `\040` / `\o{101}` inside the shipped `(?[...])` subset, including parser-path and differential parity coverage while deliberately keeping `\N` out because upstream PCRE2 rejects it inside extended classes.
- Extended Perl extended character classes again so the default path now also supports the current control-literal escape family inside the shipped `(?[...])` subset, explicitly including `\b` alongside `\a`, `\e`, and `\f`, with parser-path, compiler/unit, parser-contract, and PCRE2 differential coverage.
- Extended Perl extended character classes again so the default path now also supports bare shorthand terms (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`) inside the shipped `(?[...])` subset, including differential parity coverage for digit/word and negated-shorthand cases while still keeping broader remaining forms behind an explicit compile boundary.
- Extended Perl extended character classes again so the default path now also supports bare negated ASCII POSIX class terms such as `[:^alpha:]` inside the shipped `(?[...])` subset, including parser-path, parser-contract, compiler/unit, and differential parity coverage.
- Extended Perl extended character classes again so the default path now also supports same-level multi-operator algebra with `&` binding tighter than `|`, `+`, `-`, and `^`, while still keeping additional bare-term families and wider set-expression forms behind an explicit compile boundary.
- Consolidated the benchmark trend capture internals so artifact path planning, file writes, and summary logging now run through one shared path in `trend_capture.rs`, reducing duplication without changing the external report set.
- Extended Perl extended character classes again so the default path now also supports unary complement, symmetric difference, and grouped subexpressions over the existing bracket/property subset, laying the groundwork for the later same-level multi-operator precedence widening.
- Tightened benchmark trend capture again so `overview.*` now surfaces the newest shared-label quick/full pair alongside the latest per-mode quick/full state, making the cross-mode landing artifact more release-oriented.
- Tightened benchmark trend capture again so rolling `profile-history.*` summaries now call out the latest shared-label quick/full pair's biggest improvements and regressions instead of only exposing the raw pair-over-pair table.
- Deepened benchmark trend capture again so shared-label quick/full pairs now also produce rolling `profile-history.*` summaries, making pair-over-pair revision deltas visible alongside the existing latest-pair snapshot.
- Deepened benchmark trend capture again so shared labels now also produce `profile-pairs.*` summaries pairing the latest quick/full captures for the same revision, including aggregate medians plus full-vs-quick deltas per benchmark kind.
- Deepened benchmark trend capture again so every run now also rewrites a compact cross-mode `overview.*` artifact summarizing the latest quick/full history state in one place, including latest labels, aggregate medians, and delta-vs-previous for each mode.
- Deepened benchmark trend capture again so explicit same-mode baseline selection can now target either a unix timestamp or a capture label via `label:<text>`, with newest-match resolution when multiple archived captures reuse the same label.
- Deepened benchmark trend capture again so each archived quick/full run can now carry an explicit label, the wrapper defaults that label from the current git revision, and the rolling history summaries surface those labels alongside timestamped ratio/delta rows.
- Extended benchmark trend capture again so each quick/full run now also writes mode-scoped rolling history summaries (`history-quick.*` / `history-full.*`) with aggregate median ratios and delta-vs-previous columns, not just one latest snapshot plus one comparison baseline.
- Shipped current recursion-condition conditionals on the default regex path by teaching both parser backends plus the compiler/VM to preserve `(?(R)...)` / `(?(Rn)...)`, honor PCRE2's `R` / `Rn` named-group ambiguity rule, and execute those conditionals against the active recursion level with explicit missing-group validation.
- Tightened the shipped inline-language result contract again by adding explicit emitted-result helpers to Lua/JavaScript/Rhai statement bodies, so Lua/JavaScript now expose `rgx.emit_numeric(...)` / `rgx.emit_replacement(...)`, Rhai exposes `emit_numeric(...)` / `emit_replacement(...)`, and winning-path richer-result emission no longer depends only on direct return values.
- Shipped branch-reset groups on the default regex path by assigning shared capture numbers across the branch-reset group's top-level alternatives, carrying that numbering through later backreferences/conditionals, and adding PCRE2 differential coverage.
- Stabilized the shared local/GitHub validation loop by replacing the flaky umbrella `cargo test --workspace` step with explicit RGX package tests (`rgx-core`, `rgx-cli`, `rgx-bench`, `rgx-wasm`) while keeping the existing feature-matrix coverage intact.
- Hardened Perl extended character classes as an explicit parser boundary so `(?[...])` now round-trips through both parser backends and compile-rejects cleanly instead of remaining an ambiguous parser gap.
- Shipped the first real Perl extended character class runtime slice on the default regex path by lowering simple nested bracket-equivalent literal/range content such as `(?[[a-z]])` and `(?[[^0-9]])` into the existing char-class engine, while keeping broader algebraic forms explicitly gated.
- Hardened branch-reset groups as an explicit parser boundary so `(?|...)` now round-trips through both parser backends and compile-rejects cleanly instead of remaining an ambiguous parser gap.
- Shipped single-branch `DEFINE` conditionals on the default regex path by treating `DEFINE` as always false while keeping its branch available for numbered and named subroutine definitions, with explicit compile-time rejection for invalid false-branch forms.
- Hardened the shipped Rhai source-body contract so explicit `return ...` bodies are now locked in alongside final-expression authoring, with regression coverage and docs aligned to the actual runtime behavior.
- Separated benchmark trend artifacts into mode-scoped latest snapshots and history directories so auto-selected comparison baselines no longer mix quick-profile and full-profile captures, while still preserving explicit archived-baseline selection.
- Added file-backed CLI wasm module registration through repeatable `--wasm-module NAME=PATH`, so `(?{wasm:module:function})` no longer requires Rust glue just to exercise registered modules from the command line.
- Shipped relative conditional group references on the default regex path by resolving `(?(+1)...)` / `(?(-1)...)` to absolute conditional-group checks at compile time, with API, parser-contract, and PCRE2 differential coverage.
- Tightened the shipped inline-language CLI path by adding repeatable `--var NAME=VALUE`, optional `--show-details` match rendering, and single-pass match collection so CLI code blocks are not pre-executed twice before output.
- Stabilized relative conditional group references on both parser backends first by transporting `(?(+1)...)` and `(?(-1)...)` as dedicated AST before the later default-path runtime integration landed.
- Deepened the quick benchmark-trend loop again so each capture can now compare against either the most recent prior archived snapshot or an explicit archived baseline while still preserving timestamped history under `target/benchmark-trends/history/`.
- Tightened the shipped inline-language ergonomics again so Lua now accepts bare expression bodies as well as explicit `return ...`, matching the JavaScript/Rhai source-body direction more closely.
- Added automated quick benchmark-trend capture to the default local validation loop via `scripts/capture-benchmark-trends.sh` and `rgx-bench/src/bin/trend_capture.rs`.
- Hardened the shipped inline-language contract so JavaScript bare-expression bodies now drive predicate/result behavior instead of silently falling through, and added helper-API regression coverage across Lua/JavaScript/Rhai.
- Shipped Rhai code blocks on the default execution path in `ExecutionMode::Safe` / `ExecutionMode::Full`, including feature-gated runtime tests, parser-contract coverage, and CI/doc refreshes.
- Expanded code-block execution contexts with current match metadata (`match_start`, `match_end`, `match_length`, top-level `branch_number`) across native/Lua/JavaScript plus new wasm host imports.
- Shipped possessive quantifiers on the default compiler/VM path by lowering `*+`, `++`, `?+`, and counted possessive forms through atomic-group semantics, including parser-path regressions and PCRE2 differential coverage.
- Shipped Unicode property classes on the default compiler/VM path, including invalid-property compile errors, parser-path and AST-first regressions, and representative PCRE2 differential coverage.
- Switched the default RGX build over to the real submodule-backed PGEN parser so normal workspace builds now exercise PGEN by default.
- Shipped conditional runtime support on the default compiler/VM path, including group-exists, named-group-exists, and lookaround condition forms, missing-group compile errors, and PCRE2 differential coverage.
- Shipped numeric backreferences on the default compiler/VM path, including backtracking-safe runtime matching, missing-group compile errors, and PCRE2 differential coverage.
- Extended wasm code blocks with winning-path `Numeric` / `Replacement` result emission through `rgx.emit_numeric(...)` and `rgx.emit_replacement(...)`, including last-emitted-wins and invalid-payload failure coverage.
- Extended the shared local/GitHub CI path so `./scripts/run-local-ci.sh` now covers the `rgx-core` feature matrix (`pgen-parser`, `lua`, `javascript`, `wasm`, and `all-languages`) instead of leaving those checks manual.
- Added first dedicated numeric-result Rust APIs for code-block results by shipping `Regex::find_first_numeric_with_code(...)` / `Regex::find_all_numeric_with_code(...)` on top of winning-path `Numeric(f64)` payloads.
- Added the first replacement-oriented Rust APIs for code-block results by shipping `Regex::replace_first_with_code(...)` / `Regex::replace_all_with_code(...)` on top of winning-path `Replacement(String)` payloads.
- Added the first richer non-boolean code-block result slice by surfacing winning-path numeric/replacement values through `MatchResult.code_result`, now across Lua/JavaScript/native/wasm.
- Added host-provided execution variables to the shipped code-block slice, including `Regex::set_variable(...)`, cross-backend variable bindings, and wasm variable imports.
- Expanded the wasm ABI with `rgx` host imports for named captures, including deterministic named-capture ordering and regression coverage for name/value reads.
- Expanded the wasm ABI with `rgx` host imports for current position, full input text, and numbered captures, including safe guest-memory failure handling and regression coverage.
- Rust-API wasm module registration and dispatch for `(?{wasm:module:function})` in `ExecutionMode::Safe` / `ExecutionMode::Full`, including runtime wiring, tests, and doc refreshes.
- Rust-API native callback registration for `(?{native:...})` in `ExecutionMode::Full`, including runtime wiring, tests, and doc refreshes.
- Phase-1 embedded code-block execution for `(?{lua:...})` and `(?{js:...})` / `(?{javascript:...})` in `ExecutionMode::Safe` / `ExecutionMode::Full` with feature-gated validation.
- Built-in top-level branch reporting with user-facing 1-based branch number.
- AST-first lookahead support in compiler/VM and API tests.
- AST-first lookbehind support in compiler/VM and API tests.

Detailed implementation history and validation remain in `CHANGES.md`.
