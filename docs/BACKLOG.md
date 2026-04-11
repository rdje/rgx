# RGX BACKLOG
Complete inventory of remaining work — roadmap items, features to port from Rust's `regex` crate, and engineering improvements. Living document.

## How to use this file
- Items are grouped by category, not priority.
- Each item has: description, effort estimate, rationale, and dependencies.
- Effort: `trivial` (<1h), `small` (1-4h), `medium` (1-3 days), `large` (1-2 weeks), `major` (weeks+).
- Move items to `CHANGES.md` when completed.

---

## A. Missing from RGX roadmap

### A1. Exponential backtracking protection
- **What**: Configurable step counter that aborts matching after N VM steps. Prevents denial-of-service on pathological patterns like `(a+)+b`.
- **Effort**: `small`
- **Rationale**: Production blocker. Any server accepting user-provided patterns will be DoS'd without this.
- **How**: Add `step_count` to `ExecContext`, increment per opcode, check against `max_steps` (configurable on `Regex`). Return `Err` or `None` when exceeded.
- **Dependencies**: None.

### A2. Memory limits
- **What**: Configurable caps on backtrack stack depth, capture trail size, and recursion depth.
- **Effort**: `small`
- **Rationale**: Defense-in-depth. Complements step limits.
- **How**: Add `MatchLimits { max_backtrack_frames, max_trail_entries, max_recursion_depth }` configurable on `Regex`.
- **Dependencies**: None.

### A3. `tail_file` — file watching/streaming
- **What**: `Regex::tail_file(path, options)` that watches a file for new content and triggers callbacks on matches.
- **Effort**: `medium`
- **Rationale**: Key use case for log monitoring. Documented in HOST_INTEGRATION_ARCHITECTURE.md Layer 6.
- **How**: Platform-specific file watching (`kqueue` on macOS, `inotify` on Linux, polling fallback). Chunked reading with overlap for cross-chunk matches.
- **Dependencies**: Layer 6 core (shipped).

### ~~A4. CLI `--follow` mode~~ ✅ Shipped
- **What**: `rgx-cli --file app.log --follow` that tails a file like `tail -f | grep`.
- **Effort**: `small` (once A3 is done)
- **Rationale**: The most common CLI use case for log monitoring.
- **Dependencies**: A3 (`tail_file`) — shipped.

### A5. CLI `--color` output
- **What**: ANSI color highlighting for matches, line numbers, filenames.
- **Effort**: `small`
- **Rationale**: All grep-like tools have color. Users expect it.
- **How**: Detect terminal via `is_terminal` crate or `std::io::IsTerminal`. Wrap match spans in `\x1b[31;1m...\x1b[0m`.
- **Dependencies**: None.

### A6. Inline-language steering
- **What**: `rgx.steer_skip(n)` / `rgx.steerSkip(n)` from Lua/JS/Rhai code blocks.
- **Effort**: `small`
- **Rationale**: Currently steering is native-callback-only. Inline languages should have the same power.
- **How**: Add `rgx.steer_*` helper functions to each language's execution context, returning special `ExecResult::Steer` values.
- **Dependencies**: Layer 3 (shipped).

### A7. Full Unicode case folding for `(?i)`
- **What**: `(?i:café)` should match `CAFÉ`. Currently only ASCII letters are folded.
- **Effort**: `medium`
- **Rationale**: Internationalized text processing. Currently a `partial` in the compatibility matrix.
- **How**: Use Unicode case folding tables (from `unicode-case-mapping` or `icu` crate) at compile time when `(?i)` is active. Expand char classes and literals to include all case variants.
- **Dependencies**: None.

### A8. Crate publishing
- **What**: Publish `rgx-core` and `rgx-cli` to crates.io.
- **Effort**: `small`
- **Rationale**: Users can't use what they can't install. Critical for adoption.
- **How**: Clean up Cargo.toml metadata, add README, `cargo publish`.
- **Dependencies**: Decide on public API stability guarantees.

### A9. Language bindings (Python, Node, C) — DEFERRED 2026-04-09
- **What**: Use rgx from Python, JavaScript/Node, and C/C++ programs.
- **Effort**: `large` per language
- **Status**: `deferred pending demand signal`. The "10x user base" rationale is generic and doesn't fit RGX specifically — RGX's killer feature is host integration (predicates, steering, events, async I/O, embedded scripting), and that surface translates poorly across FFI: Python callbacks become GIL territory, the async story assumes Rust's model, and the "embed Lua inside a regex from Python" pitch is weaker than from C/C++ because Python users already have a scripting host. Plus A9 is gated on A8 (publish, also deferred), is `large` per language, and the maintenance tail (packaging, version skew, prebuilds, per-binding CI) competes for time against engine work that strengthens the actual differentiator.
- **Reactivation criteria**: a real user or use case pulling for a specific binding. **If reactivated, start with C bindings via cbindgen** — cheapest of the three and unlocks every other FFI host (PHP, Ruby, etc.) for free.
- **How (when reactivated)**: Python via `pyo3`/`maturin`. Node via `napi-rs`. C via `cbindgen` + `extern "C"` wrapper.
- **Dependencies**: A8 (stable public API).

### A10. `\X` extended grapheme cluster
- **What**: `\X` matches a full Unicode grapheme cluster (base + combining marks).
- **Effort**: `medium`
- **Rationale**: PCRE2 parity gap. Needed for correct Unicode text processing.
- **How**: Use `unicode-segmentation` crate. Compile `\X` as a VM opcode that advances by one grapheme cluster.
- **Dependencies**: Add `unicode-segmentation` dependency.

### A11. `(*SKIP:name)` named skip
- **What**: `(*SKIP:name)` interacts with `(*MARK:name)` to skip back to a specific mark position.
- **Effort**: `small`
- **Rationale**: PCRE2 parity gap. Low usage.
- **How**: Wire mark name registry to skip position lookup.
- **Dependencies**: `(*MARK)` and `(*SKIP)` (both shipped).

### A12. Returned-capture subroutines
- **What**: `(?1(grouplist))` — PCRE2 10.47+ syntax for subroutines that return captures.
- **Effort**: `medium`
- **Rationale**: Very new PCRE2 feature with minimal adoption. Low priority.
- **Dependencies**: Subroutine infrastructure (shipped).

### A13. `(?(VERSION>=...)...)` conditionals
- **What**: Branch on engine version.
- **Effort**: `trivial`
- **Rationale**: Very rare. PCRE2-specific concept.
- **How**: Evaluate the version condition at compile time, emit only the matching branch.
- **Dependencies**: None.

### A14. Partial matching API
- **What**: `PCRE2_PARTIAL_SOFT` / `PCRE2_PARTIAL_HARD` — report when the input ends mid-potential-match.
- **Effort**: `medium`
- **Rationale**: Useful for streaming/incremental matching. Connects to `tail_file`.
- **How**: When the VM reaches end-of-input while matching could continue, return `PartialMatch` instead of failure.
- **Dependencies**: None.

---

## B. Features to port from Rust's `regex` crate

### B1. Step/time limits (like `regex`'s guaranteed linear time)
- **What**: rgx can't guarantee linear time (it's a backtracking engine), but it CAN abort after N steps.
- **Effort**: `small`
- **Rationale**: The `regex` crate's #1 advantage is "can't hang." rgx should have the next-best thing: configurable limits.
- **How**: Same as A1 above.
- **Port difficulty**: Easy — it's a counter, not an algorithm change.

### B2. `RegexSet` — match multiple patterns at once
- **What**: Compile N patterns, test an input against all of them in one pass, get which ones matched.
- **Effort**: `large`
- **Rationale**: The `regex` crate's `RegexSet` is widely used for routing, filtering, and classification. Very powerful.
- **How**: Compile each pattern to its own bytecode. Run an Aho-Corasick or NFA-union pre-filter, then confirm with individual VM executions for candidates.
- **Port difficulty**: Hard — the `regex` crate uses NFA composition, which is architecturally different from a backtracking VM. A simpler approach: run each pattern separately but share the input scan.

### B3. Compilation caching
- **What**: Cache compiled `Program` objects so recompiling the same pattern is instant.
- **Effort**: `small`
- **Rationale**: The `regex` crate caches internally. Useful for applications that compile patterns dynamically.
- **How**: `HashMap<String, Arc<Program>>` with LRU eviction. Thread-safe via `RwLock`.
- **Port difficulty**: Easy.

### B4. Configurable match semantics
- **What**: The `regex` crate supports leftmost-first and leftmost-longest semantics.
- **Effort**: `medium`
- **Rationale**: Different applications need different semantics. POSIX requires leftmost-longest.
- **How**: Add a `MatchSemantics` enum to compilation options. Modify the VM's alternation handling.
- **Port difficulty**: Medium — requires alternation changes in the VM.

### B5. `bytes::Regex` — match on `&[u8]` directly
- **What**: The `regex` crate has `Regex` (for `&str`) and `bytes::Regex` (for `&[u8]`). The bytes version doesn't require valid UTF-8.
- **Effort**: `medium`
- **Rationale**: Binary protocol parsing, log files with mixed encoding.
- **How**: rgx already operates on `&[u8]` internally. Expose a `BytesRegex` that accepts `&[u8]` input and doesn't validate UTF-8.
- **Port difficulty**: Easy — the internal machinery already works on bytes.

### B6. Replacer API with capture interpolation
- **What**: `regex` has `re.replace_all(text, "$1-$2")` with capture group interpolation in the replacement string.
- **Effort**: `small`
- **Rationale**: Very common operation. Currently rgx requires code blocks for replacement logic.
- **How**: Parse `$1`, `$2`, `$name` in the replacement string. Substitute with captured text.
- **Port difficulty**: Easy.

### B7. `CaptureMatches` / `Captures` API
- **What**: `regex` returns `Captures` objects with named group access: `caps["year"]`, `caps.name("year")`.
- **Effort**: `small`
- **Rationale**: Ergonomic capture access. rgx currently returns `groups: Vec<Option<(usize, usize)>>` which requires index arithmetic.
- **How**: Add a `Captures` struct wrapping the match result + input text, with `get(index)`, `name("group")`, `Index` impl.
- **Port difficulty**: Easy — it's a wrapper.

### B8. `split` and `splitn`
- **What**: Split a string by a regex pattern, like `str::split` but with regex.
- **Effort**: `trivial`
- **Rationale**: Very common operation. Standard in every regex library.
- **How**: Use `find_all` to get match positions, return the text between matches.
- **Port difficulty**: Trivial.

### B9. Syntax error diagnostics with spans
- **What**: The `regex` crate provides beautiful error messages with span highlighting when a pattern is invalid.
- **Effort**: `medium`
- **Rationale**: Developer experience. Helps users fix their patterns.
- **How**: Propagate PGEN's diagnostic location through to the error message. Format with caret highlighting.
- **Port difficulty**: Medium — PGEN already provides `byte_offset`/`line`/`column`, need to format nicely.

### B10. `is_match_at` / `find_at` — match from a specific position
- **What**: Start matching from byte position N instead of 0.
- **Effort**: `trivial`
- **Rationale**: Useful for parsing, tokenization, and custom scanning loops.
- **How**: Set `ExecContext.pos = start_position` before calling `execute_at`.
- **Port difficulty**: Trivial.

### B11. `RegexBuilder` — builder-pattern compilation with flag overrides
- **What**: `RegexBuilder::new(pattern).case_insensitive(true).multi_line(true).build()`.
- **Effort**: `small`
- **Rationale**: The `regex` crate's primary compilation API. Lets users set flags without embedding them in the pattern.
- **How**: Add a `RegexBuilder` struct with fields for each flag. Apply them as default inline flags before compilation.
- **Port difficulty**: Easy — rgx already supports `(?imsx)` inline; builder just sets defaults.

### B12. Iterator-based APIs — `find_iter`, `captures_iter`, lazy `split`
- **What**: All `regex` find/capture/split operations return lazy iterators instead of collecting into `Vec`.
- **Effort**: `small`
- **Rationale**: Zero-allocation iteration is idiomatic Rust. Collecting into `Vec` forces full materialization even when only the first few matches are needed.
- **How**: Add `FindIter`, `CaptureIter`, `SplitIter` structs that hold `&Regex` + `&str` + scan state.
- **Port difficulty**: Easy — the scanning logic already exists; wrap it in Iterator impl.

### B13. `Captures` wrapper — ergonomic capture access
- **What**: `caps.get(1)`, `caps.name("year")`, `caps["year"]`, `caps.extract::<N>()`, `caps.expand(template, &mut dst)`.
- **Effort**: `small`
- **Rationale**: The `regex` crate's `Captures` is the primary way to access groups. rgx currently exposes raw `Vec<Option<(usize, usize)>>`.
- **How**: Wrap `MatchResult` + `&str` + named-group map. Implement `Index<usize>`, `Index<&str>`, and helper methods.
- **Port difficulty**: Easy — it's a wrapper. Replaces B7 (partially shipped).

### B14. `Match` type — ergonomic match access
- **What**: `m.as_str()`, `m.range()`, `m.len()`, `m.is_empty()` instead of manual `&text[m.start..m.end]`.
- **Effort**: `trivial`
- **Rationale**: Every `regex` user relies on `m.as_str()`. RGX's `MatchResult` requires manual slicing.
- **How**: Either add these methods to `MatchResult`, or return a `Match<'a>` that borrows the input text.
- **Port difficulty**: Trivial.

### B15. `replacen` — replace up to N matches
- **What**: `re.replacen(text, 2, replacement)` — like `replace_all` but stops after N.
- **Effort**: `trivial`
- **Rationale**: Common operation. `regex` has it; rgx has `replace` (first) and `replace_all` but nothing in between.
- **How**: Add a `limit` parameter to the replace loop.
- **Port difficulty**: Trivial.

### B16. `Replacer` trait — custom replacement functions
- **What**: `re.replace_all(text, |caps: &Captures| { format!("{}!", caps[1]) })` — closure-based replacement.
- **Effort**: `small`
- **Rationale**: Much more powerful than string interpolation. Lets users compute replacements programmatically.
- **How**: Define a `Replacer` trait with `replace_append(&mut self, caps: &Captures, dst: &mut String)`. Implement for `&str`, `String`, `Fn`.
- **Port difficulty**: Easy.

### B17. `shortest_match` / `shortest_match_at`
- **What**: Return only the end position of the first match, not the full span. Faster because the engine can stop earlier.
- **Effort**: `small`
- **Rationale**: Performance optimization for "does it match and where does it end?" queries (tokenizers, validators).
- **How**: Early-exit VM mode that stops at the first `Match` opcode hit and returns `ctx.pos`.
- **Port difficulty**: Easy.

### B18. `escape()` — escape regex metacharacters
- **What**: `regex::escape("a.b") == "a\\.b"` — make a literal string safe for regex concatenation.
- **Effort**: `trivial`
- **Rationale**: Every regex library has this. Critical for building patterns from user input safely.
- **How**: Iterate chars, prefix metacharacters with `\`.
- **Port difficulty**: Trivial.

### B19. `captures_len` / `static_captures_len` / `capture_names` / `as_str` metadata
- **What**: Introspection: how many groups? what are their names? what was the original pattern?
- **Effort**: `trivial`
- **Rationale**: Needed for generic regex-processing code that works with any pattern.
- **How**: Expose `program.num_groups`, `program.named_groups`, and store the original pattern string.
- **Port difficulty**: Trivial.

### B20. `CaptureLocations` — reusable capture storage
- **What**: Pre-allocate a capture buffer and reuse it across matches to avoid per-match allocation.
- **Effort**: `small`
- **Rationale**: Performance-critical loops that match millions of times. Avoids `Vec` allocation per match.
- **How**: Add a `CaptureLocations` struct wrapping `Vec<Option<(usize, usize)>>`. Add `captures_read(text, &mut locs)` that fills it in-place.
- **Port difficulty**: Easy.

### B21. `Cow<str>` return for `replace` — avoid allocation when no match
- **What**: `regex`'s `replace` returns `Cow<str>`, borrowing the original text when there's no match instead of cloning.
- **Effort**: `trivial`
- **Rationale**: Avoids unnecessary allocation. RGX's `replace` currently returns `String` always.
- **How**: Return `Cow::Borrowed(text)` when no match, `Cow::Owned(result)` otherwise.
- **Port difficulty**: Trivial.

---

## C. Engineering improvements

### C1. JIT compilation — ACTIVE FOCUS 2026-04-09 (second after C2)
- **What**: Compile regex bytecode to native machine code for ~5-10x speedup.
- **Effort**: `major`
- **Status**: `planned, sequenced after C2`. C1 multiplies whatever engine is running by a constant factor; C2 changes the algorithmic class. Doing C2 first means C1's constant-factor win compounds on top of the NFA/DFA wins for the common case + the JIT'd backtracking path for everything else.
- **Rationale**: Closes the speed gap with PCRE2's JIT. Makes rgx competitive with C engines.
- **How**: Use `cranelift` (already in dependency tree via wasmtime) to translate bytecode to native code. Or `dynasm-rs` for lower-level control.
- **Dependencies**: Stable bytecode format. C2 should land first so C1 has both engines to JIT.
- **Open design questions**: binary-size impact, debug story, cross-platform validation matrix, fallback path when JIT compilation itself fails.

### C2. NFA/DFA hybrid for simple patterns — ACTIVE FOCUS 2026-04-09 (first)
- **What**: Detect patterns that don't use backtracking-only features and run them through a Thompson NFA + lazy DFA cache instead of the backtracking VM.
- **Effort**: `major`
- **Status**: `active focus`. Promoted from Tier 4 to top priority on 2026-04-09 because RGX is currently too slow on the patterns where most users live. C2 changes the algorithmic class, gives RGX the "can't hang" property the Rust `regex` crate uses as its primary differentiator, and typically delivers 10x-100x improvement on regular patterns where it applies.
- **Rationale**: Guarantees O(nm) for the common case while keeping backtracking for advanced features.
- **How**:
  1. Pattern-analysis pass at compile time: classify each compiled program as "no-backtracking subset" (no backrefs, no recursion, no lookaround, no inline code blocks, no atomic groups, no possessive quantifiers, no `\K`, no backtracking verbs) vs "needs the VM."
  2. For the no-backtracking subset, build a Thompson NFA from the AST.
  3. Run a lazy DFA cache (RE2-style) over the NFA. The DFA delivers the match span.
  4. **Capture handling**: the standard solution (per the Rust `regex` crate) is to use the DFA only for *finding* the overall match, then re-run a small bounded NFA simulation over the matched span to recover capture group positions. This avoids the full DFA-with-captures complexity.
  5. Engine dispatch in `Regex::find_first` etc. picks NFA/DFA or VM based on the compiled program's classification.
- **Dependencies**: Significant new engine code, but the existing AST is sufficient — no parser changes needed.
- **Open design questions**: DFA cache eviction policy, when to bail out of the lazy DFA back to NFA simulation, how to expose runtime stats, whether to run NFA/DFA + VM in parallel for comparison during development.

### C3. Fuzzing infrastructure
- **What**: `cargo-fuzz` integration for continuous fuzzing.
- **Effort**: `small`
- **Rationale**: Finds bugs that property tests and adversarial tests miss.
- **How**: Create `fuzz/` directory with fuzz targets for compilation, matching, and code block execution.
- **Dependencies**: None.

### C4. Benchmark CI
- **What**: Run criterion benchmarks in CI and fail on significant regressions.
- **Effort**: `small`
- **Rationale**: Performance regressions can slip in without automated detection.
- **How**: Add benchmark step to CI workflow with threshold comparison.
- **Dependencies**: None.

### C5. Remove scaffold files
- **What**: Delete `cache.rs`, `simd.rs`, `javascript.rs`, `wasm.rs` placeholder files.
- **Effort**: `trivial`
- **Rationale**: Code hygiene. These 1-line files serve no purpose.
- **Dependencies**: None.

### C6. Clean remaining clippy warnings
- **What**: Fix ~25 remaining warnings (mostly trace-gated unused variables).
- **Effort**: `trivial`
- **Rationale**: Clean CI output.
- **Dependencies**: None.

---

## Priority tiers

> **Active focus as of 2026-04-09**: C2 (NFA/DFA hybrid) first, C1 (JIT) second. RGX is currently too slow on the patterns where most users live; the strategic call is to fix the algorithmic class with C2, then add C1's constant-factor JIT win on top. A9 (language bindings) is deferred pending real demand signal — see its entry above for the full reasoning.

### Tier 0 — Active focus (perf push, started 2026-04-09)
| Item | Effort | Why | Status |
|------|--------|-----|--------|
| **C2 NFA/DFA hybrid** | `major` | Algorithmic class change. "Can't hang" guarantee for the common no-backtracking subset. 10x-100x typical speedup on regular patterns. | ✅ **SHIPPED 2026-04-11** — all 9 steps complete (0–8). Classifier (1), byte-class partitioning (2), forward + reverse NFA + `CompiledC2Program` (3), sparse-set Pike-VM with engine dispatch (4), lazy forward DFA cache + DFA dispatch for `is_match` (5), DFA dispatch for `find_first`/`find_all` (6), literal prefix integration via memchr (7), production cutover with `PrefixScanner`, nested-quantifier dispatch heuristic, pure-literal short-circuit gate, and the dedicated Book chapter (8). 902-test suite green. Benchmark wins vs the pre-C2 baseline (label `f708f7c`): `literal_simple` 38-40x faster (literal_finder gate), `email_basic` 6.1-6.6x faster (existing-VM via nested-quant gate), `capture_groups` 31-35x faster (DFA dispatch with `Digit` PrefixScanner). Vs PCRE2: `literal_simple find_all 10K` is **3.16x faster** and `capture_groups find_all 10K` is **1.96x faster**. See `book/src/internals/nfa-dfa-engine.md` for the design and the dispatch chain. |
| **C1 JIT compilation** | `major` | Constant-factor multiplier (~5-10x) on whichever engine runs. Sequenced after C2 so wins compound. | **Steps 0–2 COMPLETE 2026-04-11.** Step 0: comprehensive design proposal at `docs/C1_JIT_COMPILATION_DESIGN.md`. Step 1: standalone `rgx-core/src/c1/` module with `JitHost` Cranelift wrapper, runtime helper skeleton, opt-in `jit` Cargo feature, smoke test that JIT-compiles `extern "C" fn() -> i64 { 42 }` end-to-end. Step 2: `c1::codegen::is_jit_eligible(program: &Program) -> bool` walks compiled bytecode and decides JIT acceptance. Two-layer check: quick rejects from `ProgramFlags` (`has_backrefs` / `has_lookarounds` / `has_code_blocks`) followed by an opcode walker that recurses into optimized-quantifier inline subprograms (catches `\X+` / `(?R)?` etc.) and rejects all backtracking verbs, atomic groups, conditionals, recursion, lookaround, `\K` / `\G` / `\X`, and reserved opcodes. **Does NOT touch the engine** — pure function on `&Program`. 50 hand-curated truth-table tests cover eligible (literals, char classes, alternation, every quantifier flavour, anchors, capture groups, realistic patterns) and ineligible (every forbidden opcode family). Default build unchanged at 902 tests; with `--features jit` **955 tests pass** (53 C1 tests). Next: step 3 (codegen for the easy opcodes — Cranelift IR lowering for `Char`, `DigitAscii`, `WordAscii`, `SpaceAscii`, `Split`, `Jump`, `Match`, `SaveStart`, `SaveEnd`, `Backtrack`, `StartText`, `EndText`, `WordBoundary`, `NonWordBoundary`). Step 4 turns the differential gate active. |

### Tier 1 — Do now (production blockers + quick wins)
| Item | Effort | Why |
|------|--------|-----|
| ~~A1 Step limits~~ | `small` | ✅ Shipped — `set_max_steps` |
| ~~A2 Memory limits~~ | `small` | ✅ Shipped — `set_max_backtrack_frames` + `set_max_recursion_depth` |
| ~~B1 (= A1)~~ | `small` | ✅ Shipped |
| ~~B8 `split`/`splitn`~~ | `trivial` | ✅ Shipped |
| ~~B10 `find_at`~~ | `trivial` | ✅ Shipped |
| ~~B6 Replacer with `$1` interpolation~~ | `small` | ✅ Shipped |
| ~~B7 `Captures` API~~ | `small` | ✅ Shipped — `Captures<'t>` + `Match<'t>` + iterators |
| ~~C5 Remove scaffolds~~ | `trivial` | ✅ Shipped — 4 files deleted |
| ~~C6 Clean warnings~~ | `trivial` | ✅ Shipped — zero RGX-owned warnings |

### Tier 2 — Do soon (adoption + competitiveness)
| Item | Effort | Why |
|------|--------|-----|
| A8 Crate publishing | `small` | Users can't install without it |
| ~~A5 CLI `--color`~~ | `small` | ✅ Shipped — bold red matches, auto-detect terminal |
| ~~A6 Inline-language steering~~ | `small` | ✅ Shipped — steer_* in Lua/JS/Rhai |
| ~~B3 Compilation caching~~ | `small` | ✅ Shipped — `RegexCache` with LRU eviction |
| ~~B5 `bytes::Regex`~~ | `medium` | ✅ Shipped — `BytesRegex` matches `&[u8]` directly |
| ~~B9 Error diagnostics~~ | `medium` | ✅ Shipped — CompileError with caret highlighting |
| ~~B11 `RegexBuilder`~~ | `small` | ✅ Shipped — fluent builder with flag overrides |
| ~~B12 Iterator APIs~~ | `small` | ✅ Shipped — find_iter, captures_iter, split_iter, capture_names |
| ~~B13 `Captures` wrapper~~ | `small` | ✅ Shipped — `Captures<'t>` with index/name/expand/iter |
| ~~B14 `Match` type~~ | `trivial` | ✅ Shipped — `Match<'t>` with as_str/range/len |
| ~~B15 `replacen`~~ | `trivial` | ✅ Shipped |
| ~~B16 `Replacer` trait~~ | `small` | ✅ Shipped — Replacer trait + NoExpand + closure support |
| ~~B17 `shortest_match`~~ | `small` | ✅ Shipped — shortest_match + shortest_match_at |
| ~~B18 `escape()`~~ | `trivial` | ✅ Shipped |
| ~~B19 Metadata accessors~~ | `trivial` | ✅ Shipped — `as_str`, `captures_len` |
| ~~B20 `CaptureLocations`~~ | `small` | ✅ Shipped — captures_read + captures_read_at |
| ~~B21 `Cow<str>` replace~~ | `trivial` | ✅ Shipped |
| ~~C3 Fuzzing~~ | `small` | ✅ Shipped — 4 cargo-fuzz targets with invariant checks |
| ~~C4 Benchmark CI~~ | `small` | ✅ Shipped — criterion benchmarks in CI with artifact storage |

### Tier 3 — Do when ready (strategic)
| Item | Effort | Why |
|------|--------|-----|
| ~~A3 `tail_file`~~ | `medium` | ✅ Shipped — OS-native event-driven watching (kqueue/inotify) |
| ~~A7 Unicode case folding~~ | `medium` | ✅ Shipped — `(?i:café)` matches `CAFÉ` |
| ~~B2 `RegexSet`~~ | `large` | ✅ Shipped — multi-pattern matching with SetMatches |
| ~~B4 Match semantics~~ | `medium` | ✅ Shipped — MatchSemantics API; compiler-level alternation reorder is follow-up |

### Tier 4 — Long-term (architecture / deferred)
| Item | Effort | Why |
|------|--------|-----|
| ~~A10 `\X`~~ | `medium` | ✅ Shipped — extended grapheme cluster via unicode-segmentation |
| ~~A12 Returned-capture subroutines~~ | `medium` | ✅ Shipped — parsing + compilation; full capture-return VM semantics is follow-up |
| ~~A14 Partial matching~~ | `medium` | ✅ Shipped — PartialMatchResult with hit_end detection |
| **A9 Language bindings** | `large` per language | **Deferred 2026-04-09** — pending real demand signal. RGX's host-integration killer feature translates poorly across FFI; the maintenance tail competes with engine work. If reactivated, start with C bindings via cbindgen. See A9 entry above for the full reasoning. |
