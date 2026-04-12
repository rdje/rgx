# Project Status & Roadmap

This chapter is a snapshot of where RGX actually stands today. Other parts of this book describe the engine as if it were finished; this chapter is where we tell the truth about what is shipped, what is in flight, and what is still a plan.

The numbers and status claims here are tied to what is in the repository as of this writing. Every item is either shipped (with a test proving it), in flight (with explicit scope), or explicitly deferred. Nothing is aspirational.

## Where RGX is today

| Dimension | Status |
|-----------|--------|
| PCRE2 feature parity | **~98%** of tracked feature families |
| Real-world pattern coverage | **~95%** of PCRE2 patterns expected to work |
| Host integration layers | **6 of 6** shipped (Data, Callbacks, Steering, Events, Async, File) |
| Public API stability | Pre-release — API surface settled but not tagged 1.0 |
| Test count | **~633** tests on default paths (unit, integration, adversarial, stress, parity, smoke, CLI) |
| Property test cases | **11 properties × 256+ cases** per run |
| Fuzz targets | **4** (cargo-fuzz) |
| Clippy errors | **Zero** on RGX-owned code |
| PGEN dependency | Pinned to **1.1.9**, commit `ac2acb3`, via `subs/pgen` submodule |
| Crates.io publication | **Not yet** — pending public release prep |

The parity number is worth elaborating. "98%" is a rough hand-maintained estimate of tracked feature families in `docs/PCRE2_COMPATIBILITY_MATRIX.md`. The remaining 2% is concentrated in a handful of low-priority edge cases plus the performance story (JIT). The matrix has individual line items for every feature — if you want the details, read the matrix directly.

## What's actually shipped

If you were to use RGX today, you would get:

**Core regex features** — literals, classes, escapes, quantifiers (greedy/lazy/possessive/counted), alternation, capture groups (numbered, named, Python-style `(?P<name>...)`), backreferences (numeric and named), anchors, boundaries, lookarounds (positive/negative, lookahead/lookbehind), atomic groups, branch-reset groups, duplicate-name groups, comments, inline flag toggles `(?imsx)`, conditional groups, recursive subroutines, named subroutines, Unicode property classes `\p{...}`, POSIX classes, newline sequence `\R`, non-newline `\N`, extended grapheme cluster `\X`, Perl extended char classes `(?[...])` with set algebra, and backtracking control verbs (`(*COMMIT)`, `(*FAIL)`, `(*MARK:name)`, `(*PRUNE)`, `(*SKIP)`, `(*SKIP:name)`, `(*THEN)`, `(*ACCEPT)`).

**Public API** — `Regex`, `RegexBuilder` with flag overrides, `Captures` with indexed/named/expand access, `Match` with `as_str`/`range`/`len`, lazy iterators (`find_iter`, `captures_iter`, `split_iter`), `replace`/`replacen`/`replace_all` with `$1`-style interpolation, `Replacer` trait for closure-based replacement, `escape`, `shortest_match`/`shortest_match_at`, `is_match_at`/`find_at`, `capture_names`, `captures_len`, `as_str`, `Cow<str>` returns from replace, `CaptureLocations` for reusable capture storage.

**Advanced API** — `BytesRegex` for `&[u8]` matching without UTF-8 validation, `RegexSet` for multi-pattern simultaneous matching, `RegexCache` with LRU eviction for compiled-pattern reuse, `MatchSemantics` for leftmost-first/leftmost-longest configuration, partial matching (`PartialMatchResult` with `hit_end`), full Unicode case folding for `(?i:café)` matching `CAFÉ`, error diagnostics with caret highlighting.

**Safety limits** — `set_max_steps`, `set_max_backtrack_frames`, `set_max_recursion_depth`. All shipped. All tested against pathological patterns.

**Host integration (all 6 layers)** — Data exchange (`set_variable`, `code_result`, numeric/replacement helpers). Predicate callbacks for Lua, JavaScript, Rhai, native Rust closures, and WebAssembly modules. Match steering via `SteerResult` (Continue/Fail/Accept/Skip/Abort) with native and inline-language support. Structured events via `MatchEvent` with `on_event` observer API. Async I/O via continuation-passing with `MatchContinuation`, `find_first_suspendable`, `resume`, and `find_first_async`. File-backed matching with `match_file`, `scan_file`, `tail_file` (including OS-native event-driven watching via kqueue/inotify/polling), `match_file_lines`, `scan_file_lines`.

**CLI** — full regex, host variables (`--var`), file mode (`--file` with `--follow`), ANSI color (`--color`, auto-detect), WASM module registration (`--wasm-module NAME=PATH`), rich match details (`--show-details`), verbosity control, tracing.

**Infrastructure** — workspace with four crates, local CI script, GitHub Actions CI, benchmark trend capture with mode-scoped history, differential PCRE2 parity suite, cargo-fuzz targets, PGEN submodule with pinned release.

Every item in this list is shipped with tests. There is no "planned" in this list — planned items live in the next section.

## What's NOT yet shipped

These are the remaining items, grouped by priority.

### Tier 1: Adoption blockers

**A8 — Crate publishing.** The crates are ready to publish to crates.io, but we have not pulled the trigger. The blocker is a final API stability review and a license/contribution checklist. Once those land, `rgx-core` and `rgx-cli` go public. This is what stands between RGX and a wider user base.

**A9 — Language bindings.** RGX is a Rust crate. Making it available from Python, Node.js, and C would 10x the potential user base. The approach: `pyo3`/`maturin` for Python, `napi-rs` for Node, `cbindgen` plus a C ABI wrapper for C/C++. Each binding is a large effort (~1-2 weeks each) and they all depend on **A8**. Python first, probably.

### Tier 2: Performance headroom

**C1 — JIT compilation.** ✅ Shipped. The C1 production cutover landed a Cranelift-based JIT that translates RGX bytecode into native machine code for the JIT-eligible subset (literals, char classes, anchors, word boundaries, all six optimized quantifiers, capture groups 1..=16, runtime safety limits). The JIT'd function is called via a stable C ABI signature; the engine layer dispatches to it as the third tier in the 4-tier `DFA → Pike-VM → JIT → backtracking VM` chain. The `jit` Cargo feature is now default-on; users who don't want Cranelift in their dependency tree can opt out via `default-features = false`. See [The JIT Compiler](./jit-compiler.md) for the design.

**C2 — NFA/DFA hybrid.** ✅ Shipped. The C2 production cutover landed the full sparse-set Pike-VM, lazy DFA cache, byte-class equivalence partitioning, two-pass capture recovery, and a 3-tier dispatch chain (DFA → Pike-VM → existing backtracking VM) that automatically routes each pattern through the engine that handles it best. Patterns the DFA can handle now run **~1.9x faster than PCRE2**; pure-literal patterns **~3.2x faster than PCRE2**; pathological backtracking patterns gain the O(nm) Pike-VM bound. See [The NFA/DFA Hybrid Engine](./nfa-dfa-engine.md) for the design.

### Tier 3: Parity edge cases

**A11 — `(*SKIP:name)` named skip.** ✅ Shipped. The named form `(*SKIP:name)` now interacts with `(*MARK:name)` via a per-attempt mark registry on `ExecContext`. When a match attempt fails past `(*SKIP:name)`, the scan loop advances to the position of the most recent matching mark instead of the position where `(*SKIP)` was encountered. If no matching mark exists, the verb is a no-op (PCRE2 fallback semantics). New `VerbSkipNamed` opcode with length-prefixed name operand. Forward-progress guards at all 12 scan-loop sites prevent infinite loops when the mark position is behind the current scan start.

**A13 — `(?(VERSION>=...)...)` conditionals.** Branch on engine version. Trivial to implement, almost never used in real patterns. Deferred on priority, not complexity.

**A12 — Returned-capture subroutines.** `(?1(grouplist))` — PCRE2 10.47+ syntax. Parsing and compilation are shipped, but the full capture-return VM semantics are still a follow-up. This is one of the few features where RGX has explicit "shipped partial" status.

### Tier 4: Nice-to-haves

**Opcode fusion.** Combining common sequences like `Char + Char → LitString` for faster dispatch. Smaller win than JIT, easier to implement.

**Capture and backtrack preallocation.** `CaptureLocations` already does this for captures; extending to backtrack frames would shave allocator pressure on hot loops.

**Benchmark CI regression gates.** Benchmarks run in CI today, but we do not fail the build on regressions beyond a threshold. Adding that is straightforward once we pick a threshold.

## What recently shipped

RGX went through a significant sprint in the current session, with **42+ items** landing. Highlights:

- Perl extended character classes `(?[...])` with a broad shipped subset: grouped brackets, set algebra (intersection, union, difference, symmetric difference, complement), nested ordinary bracket terms, POSIX class terms (including negated), shorthand classes (`\d`, `\w`, `\s`, and negated forms), horizontal/vertical whitespace (`\h`, `\H`, `\v`, `\V`), Unicode property terms (`\p{L}`), control-literal escapes (`\a`, `\b`, `\e`, `\f`), control/octal/codepoint atoms (`\cA`, `\040`, `\o{101}`, `\x{41}`), and same-level multi-operator precedence.
- Recursion-condition conditionals `(?(R)...)`, `(?(Rn)...)`, `(?(R&name)...)` on the default path with capture-resolution and differential coverage.
- Single-branch `DEFINE` conditionals treating DEFINE as always-false while preserving its branch for subroutine definitions.
- Relative conditional group references like `(?(+1)...)` and `(?(-1)...)` with absolute resolution at compile time.
- Branch-reset groups `(?|...)` with shared capture numbering across alternatives.
- Possessive quantifiers (`*+`, `++`, `?+`, counted forms) via atomic-group lowering.
- Unicode property classes `\p{L}`, `\P{Greek}` with invalid-property compile-time errors.
- Inline-language emission helpers: `rgx.emit_numeric` / `rgx.emit_replacement` in Lua/JS, `emit_numeric`/`emit_replacement` in Rhai, WASM `rgx.emit_numeric`/`rgx.emit_replacement` imports.
- Rhai code blocks on the default path with explicit `return ...` support alongside final-expression bodies.
- CLI additions: `--var NAME=VALUE`, `--show-details`, `--wasm-module NAME=PATH`, `--color` auto-detect, `--file`, `--follow`.
- Benchmark trend capture: mode-scoped latest snapshots, rolling history, cross-mode overview, label-paired quick/full summaries, git-derived capture labels, explicit baseline selection.
- Stabilized local and GitHub CI around explicit RGX package tests with feature-matrix coverage.
- Switched the default build to the submodule-backed PGEN 1.1.9 parser.
- Capture API rebuild: `Captures<'t>`, `Match<'t>`, `find_iter`, `captures_iter`, `split_iter`, `capture_names`, `captures_read`, `captures_read_at`, `replacen`, `shortest_match`, `escape`, `Cow<str>` for replace.
- Safety limits: `set_max_steps`, `set_max_backtrack_frames`, `set_max_recursion_depth`.
- Compilation cache (`RegexCache`) with LRU eviction.
- `BytesRegex` for non-UTF-8 byte matching.
- `RegexSet` multi-pattern matching.
- Full Unicode case folding for `(?i)`.
- `PartialMatchResult` with `hit_end` detection.
- Error diagnostics with caret highlighting.
- `RegexBuilder` fluent API.
- `Replacer` trait with closure support.
- Extended grapheme cluster `\X` via `unicode-segmentation`.
- OS-native file watching for `tail_file` (kqueue/inotify/polling).
- Cargo-fuzz integration with four fuzz targets.

If you want the exhaustive list, `CHANGES.md` is the authoritative ledger.

## Pre-public-release checklist

Before RGX tags v0.1.0 and publishes to crates.io, a handful of things need to happen:

- [ ] Final API review — lock down method signatures and return types for 1.0 stability promises.
- [ ] License audit — ensure Apache-2.0 is correctly attributed in every crate.
- [ ] Contribution guidelines — `CONTRIBUTING.md` with PR process, test requirements, and style rules.
- [ ] Public README polish — the current README is internal-facing; the public version should be user-facing.
- [ ] Crates.io metadata — description, keywords, categories, README paths in every `Cargo.toml`.
- [ ] Documentation publication — `cargo doc` for rgx-core on docs.rs, plus the mdBook on GitHub Pages (currently blocked by Pro-subscription requirement for private repos).
- [ ] Changelog cleanup — `CHANGES.md` entries reorganized into a release-oriented format.
- [ ] Security disclosure policy — `SECURITY.md` explaining how to report vulnerabilities.
- [ ] Tag v0.1.0 — once everything above is green.

None of these are blockers for using the code. They are blockers for calling it "publicly released."

## The PGEN dependency

Every RGX build depends on PGEN. The current state:

- **Version:** 1.1.9
- **Commit:** `ac2acb3`
- **Integration:** `subs/pgen` git submodule
- **Workflow:** `git submodule update --init --recursive` on fresh clones, or `git clone --recurse-submodules`

PGEN is a private repository. Hosted CI that cannot read it needs a `RGX_SUBMODULES_TOKEN` secret. The default `GITHUB_TOKEN` works for the main repository but does not automatically have read access to PGEN.

When PGEN ships a new release, the process is:

1. Bump the submodule pointer to the new commit.
2. Run the full test suite — any PGEN behavior change will show up as parity-test or unit-test failures.
3. Either fix the RGX adapter (`parsing.rs`) to handle the new behavior, or file a bug back to PGEN via the `pgen-issues/` workflow.
4. Commit the new pin with a changelog entry.

PGEN upgrades are never automatic. They are always intentional.

## The forward story

Zooming out: RGX's next year is shaped by four themes.

1. **Make it available.** Publish to crates.io (A8). Tag v0.1.0. Enable GitHub Pages for the book once Pro is active.
2. **Make it reachable.** Language bindings (A9). Python first, then Node, then C. This is what turns RGX from "a Rust crate" into "a regex engine."
3. **Make it faster.** Both major performance pushes (C2 NFA/DFA hybrid and C1 JIT) have now shipped. Remaining performance work: opcode fusion, multi-byte literal prefix in C2 dispatch, smarter Pike-VM heuristics, JIT-ahead-of-Pike-VM dispatch ordering, the reverse-DFA pipeline. Each benchmarked against the parity suite.
4. **Keep the parity number honest.** PCRE2 keeps shipping. We track new syntax as it appears, update the matrix, and add fixtures. "98%" should stay roughly true over time.

These themes are tracked concretely in `ROADMAP.md` and `docs/BACKLOG.md`. This chapter is a human-readable overview; those documents are the definitive planning sources.

## Next: how to contribute

If you want to work on any of the above, the next chapter tells you exactly how to get set up. Head to [Contributing](./contributing.md).
