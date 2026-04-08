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

### A4. CLI `--follow` mode
- **What**: `rgx-cli --file app.log --follow` that tails a file like `tail -f | grep`.
- **Effort**: `small` (once A3 is done)
- **Rationale**: The most common CLI use case for log monitoring.
- **Dependencies**: A3 (`tail_file`).

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

### A9. Language bindings (Python, Node, C)
- **What**: Use rgx from Python, JavaScript/Node, and C/C++ programs.
- **Effort**: `large` per language
- **Rationale**: 10x the user base. Most regex users aren't Rust developers.
- **How**: Python via `pyo3`/`maturin`. Node via `napi-rs`. C via `cbindgen` + `extern "C"` wrapper.
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

### C1. JIT compilation
- **What**: Compile regex bytecode to native machine code for ~5-10x speedup.
- **Effort**: `major`
- **Rationale**: Closes the speed gap with PCRE2's JIT. Makes rgx competitive with C engines.
- **How**: Use `cranelift` (already in dependency tree via wasmtime) to translate bytecode to native code. Or `dynasm-rs` for lower-level control.
- **Dependencies**: Stable bytecode format.

### C2. NFA/DFA hybrid for simple patterns
- **What**: Detect patterns that don't use backtracking features and run them through a Thompson NFA.
- **Effort**: `major`
- **Rationale**: Guarantees O(nm) for the common case while keeping backtracking for advanced features.
- **How**: Pattern analysis at compile time. If no backreferences/lookaround/recursion, use NFA path.
- **Dependencies**: Significant new engine code.

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
| A9 Language bindings | `large` | 10x user base |
| ~~B2 `RegexSet`~~ | `large` | ✅ Shipped — multi-pattern matching with SetMatches |
| ~~B4 Match semantics~~ | `medium` | ✅ Shipped — MatchSemantics API; compiler-level alternation reorder is follow-up |

### Tier 4 — Long-term (architecture)
| Item | Effort | Why |
|------|--------|-----|
| C1 JIT | `major` | Ultimate speed |
| C2 NFA/DFA hybrid | `major` | Guaranteed linear time for simple patterns |
| A10 `\X` | `medium` | Niche Unicode feature |
| A12 Returned-capture subroutines | `medium` | Bleeding-edge PCRE2 |
| A14 Partial matching | `medium` | Streaming |
