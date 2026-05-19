# Performance

RGX is fast. It is competitive with PCRE2 and Rust's `regex` crate on a wide range of patterns, and on several benchmarks it is actually faster than PCRE2. The honest summary: RGX is fast **enough** to be practical, sometimes the fastest, sometimes second.

This chapter is about what "fast enough" means, how we measured it, and what we did (and did not do) to get here.

## Honest numbers

Here is where RGX sits against PCRE2 on the current benchmark suite (after the C2 NFA/DFA hybrid and C1 JIT production cutovers), measured by `rgx-bench` against PCRE2 10.x:

| Benchmark | RGX vs PCRE2 | Interpretation |
|-----------|--------------|----------------|
| `find_all` literal 1K | **~3.20x faster** | memmem fast path; preserved by C2 dispatch gates |
| `find_all` literal 10K | **~3.16x faster** | same fast path scales linearly |
| `find_first` capture 1K | **~1.91x faster** | DFA dispatch via the C2 hybrid |
| `find_all` capture 1K | **~1.93x faster** | DFA scan + Pike-VM capture recovery |
| `find_all` capture 10K | **~1.96x faster** | DFA dispatch dominates |
| `find_first` email 1K | ~3.09x slower | C1 JIT path (has `\b`, ineligible for C2) |
| `find_all` email 1K | ~2.63x slower | same; the JIT eats the per-opcode dispatch cost but pays for word-boundary helper calls |

These are **wins** in most cases. On patterns with strong literal content the memmem fast path is so effective that RGX outruns PCRE2 by several times. On patterns the C2 lazy DFA can handle (no zero-width assertions, no lazy quantifiers), the dispatch chain delivers another factor-of-two over PCRE2 — see [The NFA/DFA Hybrid Engine](./nfa-dfa-engine.md) for the dispatch design. On patterns the JIT can handle but C2 can't (anchors, word boundaries, lazy quantifiers), the C1 JIT delivers a constant-factor speedup over the existing VM — see [The JIT Compiler](./jit-compiler.md) for the codegen design.

Older versions of this chapter would have shown a very different story. Before the optimizations described below, RGX was roughly **50-70x slower** than PCRE2 on the same suite. The path from "50x slower" to "3x faster" is the subject of the rest of this chapter, and the most recent stretch of that path is the C2 production cutover described in the dedicated chapter.

## Methodology: rgx-bench

`rgx-bench` is a workspace crate whose job is honest measurement. It contains:

- A set of fixture patterns and inputs that match real-world use cases (literals, email addresses, URLs, log lines, Unicode identifiers).
- Criterion-based benchmark harnesses that measure `find_first`, `find_all`, captures, and replacement throughput.
- A differential parity suite (`rgx-bench/tests/pcre2_parity.rs`) that runs roughly 250 pattern+input combinations through both RGX and PCRE2 and asserts the results match.
- A trend capture tool (`rgx-bench/src/bin/trend_capture.rs`) that archives historical benchmark runs and lets us see regressions between commits.

The trend capture writes artifacts to `target/benchmark-trends/` including mode-scoped latest snapshots, rolling history, cross-mode overviews, and label-paired quick/full summaries. Each run is tagged with the git revision, so when we see a regression we can bisect to the commit that caused it.

There are two measurement modes:

- **Quick** — runs in seconds, good for the inner loop of local development. The default for `./scripts/run-local-ci.sh`.
- **Full** — uses criterion's full statistical sampling, slower but higher fidelity. Used for release validation.

The quick/full split means developers can get feedback on their changes immediately, and we still have a high-confidence measurement when it matters.

## Key optimizations

The big wins came from a handful of engineering changes, in rough order of impact.

### 1. Borrowed `&[u8]` text (no copy)

The single biggest win. Early RGX copied the input into an owned `Vec<u8>` at the start of every `find_first` call. On short inputs (1K benchmark text), that allocation dominated runtime — we were allocating 1K per match attempt, which at 10 million matches/sec is 10 GB/sec of allocator pressure.

Switching `ExecContext.text` from `Vec<u8>` to `&[u8]` eliminated the copy. The VM never modifies the input, so borrowing is safe; the only reason it was owned in the first place was legacy code. This single change moved several benchmarks from "50x slower" to "under 10x slower."

### 2. Trace gating behind `#[cfg(feature = "trace")]`

The VM's dispatch loop was full of calls like `trace!("entering opcode {:?}", op)`. In release builds the `trace!` macro expands to nothing, but "nothing" still includes formatting the arguments — and the arguments referenced `ctx.pc`, `ctx.pos`, and `op`, which caused LLVM to keep them in registers it would otherwise have freed.

Gating the trace macros behind a compile-time feature flag means release builds have literally zero trace code. The branch predictor and register allocator immediately got happier. This was a smaller absolute win than the text copy fix but was critical for the per-opcode overhead story.

### 3. memmem fast path

When the compiler detects a pattern is a pure literal with no metacharacters, it marks the program as literal-only and the scanning loop delegates to `memchr::memmem::find`. `memmem` uses SIMD-accelerated searching — the same primitives that make ripgrep fast. RGX gets that performance for free.

This is why literal benchmarks are **6.4x faster** than PCRE2: PCRE2's interpreter has per-position overhead, and our implementation is a direct call into a vectorized primitive.

### 4. Trail-based backtracking

Before this optimization, every backtrack frame copied the entire capture array. For a pattern with 20 capture groups and 100 backtrack points, that is 2000 integer copies per match — significant on tight loops.

The trail approach records only the writes that happened on a branch, and on backtrack it undoes exactly those writes. For most patterns the number of writes per branch is tiny (often zero for the non-winning branches), so the work scales with actual capture activity rather than program size.

### 5. Binary search Unicode property tables

RGX supports Unicode property classes like `\p{L}` and `\p{Greek}`. Each property is represented as a sorted list of codepoint ranges. The old code walked the list linearly; for "is this codepoint a letter?" that meant up to several hundred range comparisons per codepoint.

Switching to binary search made Unicode-heavy patterns an order of magnitude faster. This was a small code change with a big impact.

### 6. Prefix filter

For patterns that are not pure literals but have a useful prefix (a literal, a character class, or an anchor), the compiler extracts the prefix and the scanning loop uses it to skip positions that cannot possibly match.

`\b\w+@\w+` has a `@` somewhere in every match, so the scanning loop can use `memchr::memchr(b'@', text)` to jump directly to candidate positions. The VM then runs from the candidate position to verify. This is what gets the email benchmark into "faster than PCRE2" territory.

### 7. Zero-allocation iterators

The `find_iter`, `captures_iter`, and `split_iter` APIs are lazy — they allocate nothing beyond their initial construction. Each `next()` call reuses the previous match buffers. For code that processes millions of matches and only needs the first few, this is the difference between "collects everything up front" and "stops when you stop asking."

### 8. `Cow<str>` replace

`replace` and `replace_all` used to always return `String`, allocating even when there was nothing to replace. They now return `Cow<'_, str>`: if the input had no matches, the return value borrows the original text with zero allocation. If the input had matches, the return is an owned `String` as before.

This is a small ergonomic improvement on the API and a measurable speedup on the common case of "try to replace, usually nothing matches."

## Three execution tiers

As of the C1 production cutover, RGX runs three execution tiers in parallel:

1. **C2 — NFA/DFA hybrid** ([nfa-dfa-engine.md](./nfa-dfa-engine.md)). Lazy DFA cache + sparse-set Pike-VM. Guaranteed linear time for the no-backtracking subset. Preferred whenever the pattern is C2-eligible — this is what gives RGX the "can't hang" property the Rust `regex` crate uses as its primary differentiator.
2. **C1 — JIT compiler** ([jit-compiler.md](./jit-compiler.md)). Cranelift-based native codegen for the JIT-eligible subset. Eliminates per-opcode dispatch overhead from the existing VM and runs at native speed. On by default as of the production cutover. JIT is sequenced after C2 in the dispatch chain so its constant-factor win compounds on top of C2's algorithmic-class improvement.
3. **The backtracking VM** ([the-vm.md](./the-vm.md)). Always available, handles every PCRE2 feature including the ones C1 and C2 can't lower (backreferences, lookaround, recursion, code blocks, atomic groups, backtracking verbs, `\K`).

The dispatch chain (DFA → Pike-VM → JIT → backtracking VM) is described in detail in the C2 and C1 chapters. Each tier handles the patterns it's best at and falls through to the next tier when ineligible.

## Compile-time performance

Everything above is *match* throughput. Compile time — the cost of turning a
pattern string into a runnable `Regex` — is a separate story, and an honest one.

RGX does not have its own parser: **PGEN is the sole parser** (see
[PGEN Integration](./pgen-integration.md)). So `Regex::compile` time is
dominated by PGEN's regex parse, and RGX's downstream phases (AST → bytecode +
C2 build, engine construction) are a comparatively thin slice. Phase-splitting
`Regex::compile` over the standard 8-pattern bench corpus
(`rgx-core/examples/compile_phase_split.rs`) shows PGEN parse is
**≈ 63–86 %** of total compile wall-clock; lazy engine-artifact construction
already drove the `Engine::new` share down to ~0–1 %.

Latest measurement — **PGEN `db6f8c68` (release 1.1.81 / contract 1.1.83),
2026-05-19, Apple Silicon, default allocator**, PGEN parse p50 over 5000
samples vs PCRE2 10.47 `pcre2_compile()` (10 000-compile batch mean):

| Pattern | PGEN parse p50 | vs PCRE2 (no JIT) | vs PCRE2 (+JIT) |
|---|--:|--:|--:|
| `test` | 24 µs | 133× | 15× |
| `\d{3}-\d{2}-\d{4}` | 60 µs | 211× | 28× |
| `[a-zA-Z0-9._%+-]+@…` | 94 µs | 152× | 34× |
| `cat\|dog\|bird` | 60 µs | 199× | 31× |
| `(\d{4})-(\d{2})-(\d{2})` | 124 µs | 278× | 44× |
| `https?://\S+` | 58 µs | 211× | 26× |
| `\b\w+@\w+\.\w+\b` | 82 µs | 288× | 32× |
| `^(\d+)\s+(?P<word>\w+)\s+(?:foo\|bar)$` | 188 µs | 309× | 60× |
| **geomean** | | **≈ 214×** | **≈ 32×** |

This is the known structural gap, tracked PGEN-side as `PGEN-RGX-0073`. It is
**not** an RGX-side defect and there is no RGX-side fix: per the
PGEN-is-the-sole-parser design, parser speed work lands in PGEN, not in an RGX
workaround. The trend is real — raw PGEN parse p50 is **~2–3.8× faster** than
the PGEN-1.1.40-era baseline this corpus was first measured against, and the
geomean-vs-PCRE2-no-JIT ratio has moved from ~360× (at the original filing) to
**~214×** today. (An earlier "~80×" figure quoted in some notes came from
PGEN's own benchmark methodology — mimalloc, its internal harness — and is not
reproduced by RGX's standard default-allocator measurement; the numbers above
are the RGX-side ground truth.) The ROADMAP target is **< 5× of PCRE2
compile**; closing the remaining gap is sustained PGEN parser work and is the
precondition for that target.

Reproduce both measurements yourself from the repo root:

```text
cargo run --release -p rgx-core --example compile_phase_split --features pgen-parser
cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser
```

The second persists a full bundle (inputs, parse outcomes, AST dumps, p50s)
under `pgen-issues/artifacts/PGEN-RGX-0078/`; the companion PCRE2 C baselines
live in that bundle's `pgen_iteration_flow/`.

The complete, reproducible specification of every constant, entry point,
statistic, and environment control behind these numbers — written so PGEN can
replicate the exact methodology on its side from a self-contained, RGX-free
bundle — is [Performance Measurement Methodology](./measurement-methodology.md).

## What is NOT optimized yet

Being honest about what we have not done is part of the deal.

### Reverse-DFA pipeline (C2 follow-up)

The C2 hybrid currently uses per-position anchored scans for `find_first` / `find_all`. A faster approach would build the match span via a forward-then-reverse DFA pipeline (forward DFA finds the match end → reverse DFA finds the match start → bounded Pike-VM recovers captures). The reverse NFAs are already built and stored on `CompiledC2Program`, but the dispatch path doesn't use them yet. This is a future C2 optimization.

### JIT-ahead-of-Pike-VM dispatch (C1 follow-up)

C1 v1 ships with the JIT after Pike-VM in the dispatch chain. The original design doc sketched JIT before Pike-VM, but Pike-VM is the safety net for nested-quantifier patterns where the JIT could blow up exponentially. Re-ordering is a future optimization that requires benchmark evidence the JIT consistently beats Pike-VM on the disputed pattern shapes.

### Opcode fusion

Some common opcode sequences could be fused into a single faster opcode. For example, `Char 'a'; Char 'b'; Char 'c'` could become `LitString "abc"` with a single multi-byte compare instead of three single-byte checks. This is small compared to JIT but would shave measurable time off the VM path.

### Capture/backtrack preallocation

Each `find_first` still allocates a few small vectors for captures and the backtrack stack. For hot paths that match millions of times, preallocating these once per regex and reusing them (as `CaptureLocations` already does for captures) would reduce allocator pressure. This is a targeted fix we will ship when the benchmarks tell us it matters.

## How to benchmark RGX yourself

If you are contributing performance work, the workflow is:

1. Run the baseline:
   ```bash
   ./scripts/capture-benchmark-trends.sh
   ```
   This writes a snapshot to `target/benchmark-trends/` tagged with the current git revision.

2. Make your change.

3. Run again. The trend capture automatically compares against the most recent prior run from the same mode and prints a delta summary.

4. Look at `target/benchmark-trends/latest.md` for the high-level summary and `target/benchmark-trends/overview.md` for the cross-mode picture.

For deeper analysis, run criterion directly:

```bash
cargo bench -p rgx-bench
```

Criterion writes HTML reports to `target/criterion/` with throughput graphs, distribution plots, and historical comparisons. When a change has ambiguous impact, the criterion report usually makes it obvious.

The parity suite is separate:

```bash
cargo test -p rgx-bench --test pcre2_parity
```

This is **correctness**, not performance — it asserts that for each of ~250 pattern+input fixtures, RGX produces the same match result as PCRE2. Performance work must not break parity.

## The philosophy

Performance in RGX is not a slogan; it is a continuous validation loop. We do not trust claims like "this should be faster" — we measure. The quick benchmark capture runs on every local CI pass. The trend history lets us see regressions when they happen, not weeks later. The parity suite keeps us honest about correctness.

We also try to be honest about tradeoffs. "Fast" in the abstract is meaningless; fast on which workload, on which input, with which features enabled? The benchmark suite covers literals, captures, Unicode, and complex patterns specifically so we know when a change helps one case at the expense of another.

RGX will probably never be the fastest regex engine in every dimension. It is fast where it matters, it is measurable, and it gives you deep host integration and programmability that the fastest engines do not. That is the tradeoff we picked, and the performance story is strong enough that we can explain it without apologizing.

## Next: safety

Performance is one pillar. Safety is the other. When you run untrusted patterns or host untrusted code blocks, the story is not just "how fast" but "what can go wrong." Head to [Sandboxing & Security](./sandboxing.md) next.
