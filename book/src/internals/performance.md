# Performance

RGX is fast. It is not the fastest regex engine in the world — PCRE2 with JIT enabled still wins on raw pattern matching, and Rust's `regex` crate wins on patterns that fit its NFA/DFA model. But RGX is fast **enough** to be practical, and on several benchmarks it is actually faster than PCRE2.

This chapter is about what "fast enough" means, how we measured it, and what we did (and did not do) to get here.

## Honest numbers

Here is where RGX sits against PCRE2 on the current benchmark suite, measured by `rgx-bench` against PCRE2 10.x:

| Benchmark | RGX vs PCRE2 | Interpretation |
|-----------|--------------|----------------|
| `find_first` literal 1K | **~6.4x faster** | memmem fast path beats PCRE2's interpreter |
| `find_all` literal 1K | **~3.4x faster** | scanning loop is tight, memmem dominates |
| `find_first` email 1K | **~3.4x faster** | strong literal prefix (`\b\w+@`) |
| `find_first` capture 1K | **~0.88x** | 12% slower — VM-heavy, no literal shortcut |
| `find_all` capture 1K | competitive | in-place scanning helps |

These are **wins** in several cases. On patterns with strong literal content — which is most real-world patterns — the memmem fast path is so effective that RGX outruns PCRE2 by several times. On patterns dominated by VM interpretation, RGX is within 15% of PCRE2 despite having no JIT.

Older versions of this chapter would have shown a very different story. Before the optimizations described below, RGX was roughly **50-70x slower** than PCRE2 on the same suite. The path from "50x slower" to "3x faster" is the subject of the rest of this chapter.

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

## What is NOT optimized yet

Being honest about what we have not done is part of the deal.

### JIT compilation (backlog C1)

RGX has no JIT. PCRE2's JIT translates bytecode into native machine code and gets roughly 5-10x speedup on top of its interpreter. RGX's roadmap position is: **engineering optimizations first, JIT later**. The interpreter is fast, and most of the low-hanging fruit is in better bytecode and smarter scanning rather than native codegen.

If we do build a JIT, the most likely vehicle is Cranelift, which is already in our dependency graph via wasmtime. `dynasm-rs` is also on the table for lower-level control. This is a "weeks of work" project and we have not decided when it is worth doing.

### NFA/DFA hybrid (backlog C2)

For patterns that do not use backreferences, lookaround, or recursion, a Thompson NFA gives guaranteed linear time. This is what Rust's `regex` crate does, and it is why the `regex` crate cannot hang on pathological input.

RGX does not have this. Every pattern runs through the backtracking VM, and pathological patterns can be catastrophic without explicit limits. We protect against that with `set_max_steps`, `set_max_backtrack_frames`, and `set_max_recursion_depth`, which is the best we can do without the hybrid architecture.

Building an NFA/DFA hybrid is a major project — probably the largest single item in the backlog. It requires a separate compiler path, a separate runtime, and a pattern analyzer that decides which engine to use. We will get there, but not soon.

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
