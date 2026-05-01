# Host Call Site — PGEN-RGX-0078

This note documents how RGX measured PGEN's regex-parser parse time
against PCRE2's full compile time for PGEN-RGX-0078, and points
PGEN at the self-contained iteration flow under
`pgen_iteration_flow/` so PGEN can run the same comparison without
checking out RGX.

## How RGX measured PGEN parse time

`rgx-core/examples/pgen_compile_perf_dump.rs` is the in-tree helper.
It calls `pgen::embedding_api::parse_grammar_profile_named("regex",
"regex_default", pat)` in a tight `Instant::now()`-around-call loop:
5000 samples per pattern, 200-sample warmup discarded. The PGEN-side
work timed by this loop is **parse only** — `parse_grammar_profile_named`
returns a `ParseOutcome` containing the AST envelope; it does not
codegen, build any matcher state, or invoke any JIT. So the PGEN
column in the comparison table is pure parse-time.

```rust
// In rgx-core/examples/pgen_compile_perf_dump.rs:
let t0 = Instant::now();
let _ = pgen::embedding_api::parse_grammar_profile_named(
    "regex", "regex_default", pattern,
);
samples.push(t0.elapsed().as_nanos());
```

Output: `pgen-issues/artifacts/PGEN-RGX-0078/measurements/pgen_parse_p50.txt`
plus per-pattern parse outcomes and AST dumps (proves valid parse,
not error-path timings).

## How RGX measured PCRE2 compile time

Two standalone C bench programs under
`pgen_iteration_flow/`:

- `pcre2_compile_baseline.c` — calls `pcre2_compile()` only. **No JIT.**
  Structurally analogous to PGEN parse + bytecode codegen + match
  metadata layout. **This is the primary comparison.**
- `pcre2_compile_jit_baseline.c` — calls `pcre2_compile()` followed
  by `pcre2_jit_compile(re, PCRE2_JIT_COMPLETE)`. PCRE2 with JIT
  enabled. Mirrors what RGX measures against when RGX's C1 JIT path
  is enabled.

Both bench programs depend only on `libpcre2-8` from the system
package manager. Built with `cc -O2 -lpcre2-8`. 10000-compile batch
each, total wall-clock divided by batch size for sub-µs resolution.

## Why the comparison is "compile-time" on both sides

- **PCRE2 side**: `pcre2_compile()` does parse + bytecode codegen +
  match metadata layout. That's PCRE2's full "compile" step,
  pre-JIT. PCRE2's JIT prep is a separate `pcre2_jit_compile()`
  call, captured separately in the `_jit_baseline.c` variant.

- **PGEN side**: `parse_grammar_profile_named` does parse only. PGEN
  does NOT have a "PGEN compile" step that produces a matcher; the
  matcher is RGX's responsibility (codegen + Engine state in
  `rgx-core`). So the closest structural analogue from PGEN's side
  is parse-only, which is strictly LESS work than PCRE2's compile.

- **Therefore**: the gap measured here is a lower bound. PGEN parse
  alone is ~360x slower than PCRE2's full compile (no JIT) at PGEN
  pin `056f6784`. RGX adds its own codegen and Engine::new on top;
  the full end-to-end RGX `Regex::compile` vs PCRE2 `pcre2_compile()`
  ratio is even larger (RGX-side work adds 4-14% extra per
  `compile_phase_split.rs`, so end-to-end ratio rises to ~400-450x
  geomean). The closure target ("<5x of PCRE2 compile") therefore
  requires PGEN parse alone to drop to under 5x of PCRE2's compile,
  not RGX's portion.

## How PGEN can iterate WITHOUT RGX

Vendor `pgen_iteration_flow/` into PGEN's tree (suggested location:
`pgen/perf/`). Add `Cargo.toml.snippet` entries to PGEN's
`rust/Cargo.toml` and the `Makefile.snippet` recipe to PGEN's
`rust/Makefile`. Then:

```bash
make -C rust regex_pcre2_compile_perf_gate
```

Output: per-pattern + geomean ratio table. CI can fail the gate when
geomean exceeds the closure threshold (currently <5x; raise as PGEN
makes progress).

`pgen_iteration_flow/README.md` documents the full integration
recipe end-to-end with two integration depths (Rust microbench only
vs Rust + C cross-validation).

## Cross-validation between Rust crate and C baseline

The `pcre2` Rust crate (`pgen_pcre2_compile_ratio.rs` uses it)
is a thin Rust binding around libpcre2-8. Its compile timings
should agree with the C baseline (`pcre2_compile_baseline.c`)
within the noise floor — typically <5% delta. If the divergence
is larger, that flags a Rust-binding overhead concern (the crate
allocates a Rust `Vec` for the captures buffer, plus a small Rust
struct around the compiled regex). PGEN's release-note p50s should
cite the C baseline number when the divergence matters; otherwise
the Rust-microbench number is preferred for ergonomic reasons.

This bundle's `measurements/` directory contains both for the
2026-05-01 reference run on this host:

- `pcre2_compile_p50.txt` — both `pcre2_compile_baseline.c` (no JIT)
  and `pcre2_compile_jit_baseline.c` (with JIT) outputs
- `pgen_parse_p50.txt` — PGEN parse p50 from the in-tree
  `pgen_compile_perf_dump.rs` helper
- `ratio_table.md` — combined comparison + analysis + closure status
- `host_metadata.txt` — host CPU/OS, allocator, sample counts,
  trace status, slowdown character (per protocol §D)
