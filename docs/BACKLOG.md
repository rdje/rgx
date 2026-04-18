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

### ~~A7. Full Unicode case folding for `(?i)`~~ ✅ Shipped
- **What**: `(?i:café)` matches `CAFÉ`. Full simple-fold equivalences (ſ↔s, K↔K(Kelvin), Σ↔σ↔ς) now match under `/i`.
- **Effort**: `medium`
- **Shipped**: 2026-04-16. `rgx-core/src/vm.rs` `unicode_case_variants` consults `regex_syntax::hir::ClassUnicode::try_case_fold_simple` alongside `char::to_lowercase` / `char::to_uppercase`, giving full UCD simple-fold equivalence classes.
- **Impact**: PCRE2 conformance +161 passes in one commit (8,988 → 9,149).

### A8. Crate publishing
- **What**: Publish `rgx-core` and `rgx-cli` to crates.io.
- **Effort**: `small` (metadata+docs) + `medium` (pgen-publish strategy decision)
- **Status**: **Metadata + READMEs ready (2026-04-13).** Both crates have `description`, `readme`, `documentation`, `homepage`, `keywords`, `categories`, `repository`, `license` populated; per-crate READMEs written for crates.io display; `rgx-cli` now specifies a version on the `rgx-core` path dep; LICENSE (Apache-2.0) is in place at repo root. `cargo publish --dry-run` on rgx-core surfaces **one hard blocker**:
  ```
  error: all dependencies must have a version specified when publishing.
  dependency `pgen` does not specify a version
  ```
  The `pgen` crate lives in `subs/pgen/rust` (private submodule) and is not on crates.io. Three paths forward, user decision:
  1. Publish `pgen` (and its dependency chain) to crates.io first, then bump rgx-core's dep to `pgen = "1.1.10"`.
  2. Vendor pgen's generated Rust code into rgx-core so the dependency disappears.
  3. Make `pgen` an optional dep so `rgx-core` can publish without it, with the caveat that `pgen-parser` feature is only usable from git.
- **Binary rename decision pending**: the CLI binary is currently named `rgx-cli` (package default). The README advertises `rgx foo bar` but `cargo install rgx-cli` will install `rgx-cli` unless an explicit `[[bin]] name = "rgx"` is added. Touches 461 references across docs and scripts — a coordinated follow-up commit.
- **Rationale**: Users can't use what they can't install. Critical for adoption.
- **Dependencies**: pgen-publish strategy (above) + API stability decision + explicit user authorization to actually publish.

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

### A11. `(*SKIP:name)` named skip ✅ DONE (2026-04-12)
- **What**: `(*SKIP:name)` interacts with `(*MARK:name)` to skip back to a specific mark position.
- **Shipped**: New `VerbSkipNamed` opcode, per-attempt mark registry on `ExecContext`, forward-progress guards at all scan-loop sites. See `CHANGES.md` entry for details.

### A12. Returned-capture subroutines
- **What**: `(?1(grouplist))` — PCRE2 10.47+ syntax for subroutines that return captures.
- **Effort**: `medium`
- **Rationale**: Very new PCRE2 feature with minimal adoption. Low priority.
- **Dependencies**: Subroutine infrastructure (shipped).

### A13. `(?(VERSION>=...)...)` conditionals ✅ DONE (2026-04-13)
- **What**: Branch on engine version.
- **Shipped**: RGX-side parser-level short-circuit landed 2026-04-12; PGEN 1.1.10 shipped the grammar recognition on 2026-04-13, closing `PGEN-RGX-0016`. Submodule bumped from `ac2acb3` (1.1.9) to `8783757` (1.1.10), the three integration tests in `parsing::tests::version_conditional_*` now run unmodified.

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

### C7. PCRE2 10.47 differential conformance — bug triage
- **What**: Triage the bugs uncovered by the `rgx-core/tests/pcre2_conformance.rs` differential harness (introduced 2026-04-13).
- **Effort**: `medium` (each bug class is its own investigation)
- **Status**: harness shipped and expanded to all 23 paired testinput files; **crash-class bugs fixed**; harness-side false positives cleaned up; first three real RGX parse bugs fixed (`\0`, `\NNN`-octal-fallback, `{0,0}`-with-captures); semantic-class failures still in progress.
- **Timeline of pass-rate** (testinput1 only early, full corpus later):
  - 2026-04-13 commit 1 (harness, testinput1): 1061 pass / 1616 fail / 12 panic / 182 skip / 2871 parsed / 39.6% ran-pass-rate
  - 2026-04-13 commit 2 (crash fixes, testinput1): 1063 / 1626 / **0 panic** / 182 / 39.5%
  - 2026-04-13 commit 3 (harness refactor + `\0` fix, testinput1): 1952 / 429 / 0 / 139 / 2520 / 82.0%
  - 2026-04-13 commit 4 (`\NNN` octal fallback, testinput1): 1957 / 424 / 0 / 139 / 82.2%
  - 2026-04-13 commit 5 (full corpus expansion, 23 files): 3613 / 1018 / 9 panic / 6576 / 11216 / 78.0%
  - 2026-04-13 commit 6 (FlagGroup lowering fix): 3618 / 1022 / **0 panic** / 6576 / 78.0%
  - 2026-04-14 commit 7 (case-fold ASCII ranges spanning both cases): 3624 / 1016 / 0 / 6576 / 78.1%
  - 2026-04-14 commit 8 (PGEN 1.1.19 bump — 25 reports closed + 13 partial): 3661 / 979 / 0 / 6576 / 11216 / 78.9%
  - 2026-04-14 commit 9 (PGEN 1.1.21 audit pre-adapter-catch-up — interim regression): 3599 / 1042 / 0 / 6575 / 77.5%
  - **2026-04-14 commit 10 (PGEN 1.1.21 + adapter catch-up — 0054 closed, `\K`/`\R`/`\N`/`\X` and `modifier_item` handled): 3670 pass / 971 fail / 0 panic / 6575 skip / 11216 parsed / 79.1%**
- **Fixed bugs**:
  1. ✅ **`{0,0}` / `{0}` quantifier with captures** — sized `subroutines` in `compile_subroutines` via AST-observed max group id. 5 regression tests.
  2. ✅ **Char class operand overflow on `{0,N}` with large N** — deduplicated identical `CompiledCharClass` entries during sub-compiler merge via remap-table rewrite. 1 regression test.
  3. ✅ **`\0` treated as `Regex::Backreference(0)` instead of NUL byte** — `convert_simple_escape` now handles `'0'` explicitly before the `is_ascii_digit()` backref arm. Group 0 is the overall match and is never a valid backref target. 3 regression tests.
- **Aggregate failure categories across all 23 files (1018 total, after commit 5)** — sorted by count:
  - 245 PGEN parse failures — `/([[:]+)/`, `\Q...\E`, `(?(*pla:...))`, etc.
  - 200 false negatives — RGX misses matches PCRE2 finds (case-insensitive char-class ranges, `\s` semantics, etc.)
  - 200 false positives — RGX matches where PCRE2 doesn't (anchor/whitespace interactions)
  - 173 span mismatches — semantic divergences on specific patterns
  - 78 PGEN rejects simple escape — `\"`, `\/` literal escapes
  - 62 class_escape unsupported variant — `[\b]`, `[\c]` in char classes (RGX adapter gap)
  - 42 other compile errors — `(*pla:foo)` backtracking-verb aliases RGX doesn't know, etc.
  - 16 PGEN AST contract mismatch (other) — POSIX classes inside char classes (`[[:space:]]+`)
  - 2 unterminated char class — `\c[` control-char escape parsing
- ✅ **9 panics fixed (2026-04-13)**. Root cause was `Compiler::lower_extended_char_classes` not recursing through `FlagGroup`, so `(?i)(?[...])` left the `ExtendedCharClass` node unlowered under the FlagGroup wrapper. 4-line fix + 2 regression tests. Full-corpus panic count now 0/11,216.
- **Excluded files** (see harness comments for details):
  - `testinput15` — match-limiting stress file with catastrophic-backtracking patterns (`(a+)*zz`). Some cases don't honor the harness's `max_steps=1M` cap and hang indefinitely. BACKLOG follow-up: audit every RGX hot path to ensure it checks `max_steps`.
- **Per-file pass rates** (for reference; see the harness output for the full table):
  - testinput10, testinput13, testinput18: 100%
  - testinput28: 97.6%
  - testinput6 (DFA): 88.9%
  - testinput4 (UTF): 86.3%
  - testinput1 (core Perl-compatible): 81.9%
  - testinput17 (JIT): 76.5%
  - testinput7 (UTF DFA): 58.8%
  - testinput2 (PCRE2-specific API + Python/.NET syntax): 28.3%
  - testinput5 (UTF API internals): 20.0%
  - testinput24 (pattern conversion API): 12.2%
  - testinput3 (fr_FR locale): 0.0% (all skipped — locale not applicable)
  - testinput26 / testinput27 (UCP-generated): 0% ran, 100% skipped (all use modifiers our harness doesn't parse yet)
- **Next bugs to investigate** (prioritized by count + value):
  - ✅ The `\123` → octal fallback when group 123 doesn't exist (shipped 2026-04-13, see entry above)
  - Case-insensitive char-class range handling (`[W-c]/i`)
  - `[\b]` backspace literal inside char class
- **PGEN-side reports filed (2026-04-13)**: 37 unique PGEN-RGX-NNNN reports (`PGEN-RGX-0017` through `PGEN-RGX-0053`) covering every PGEN-related failing pattern from testinput1. Each carries the full bundle per `subs/pgen/docs/contracts/PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`: yaml metadata, `repro_input.txt`, `pgen_contract.json`, `pgen_parse_outcome.json`. Generated by the new internal tool `cargo run -p rgx-core --bin file_pgen_issues --features pgen-parser` which is reusable for future PCRE2 testfiles. Bug-class breakdown:
  - 32 `should_parse_but_fails` — PGEN rejects patterns PCRE2 accepts (POSIX class delimiters in unusual positions, `\Q...\E` literal-quoting, `(*PRUNE:m(...))` mark-name with parens, `(?(*pla:...))` callout-style lookarounds, malformed-quantifier-falls-back-to-literal cases like `X{`, etc.)
  - 5 `parses_but_returns_wrong_ast` — PGEN parses but emits a `class_item` node shape RGX's adapter doesn't have a case for (POSIX classes inside char classes like `[[:space:]]+`)
- **NOT filed as PGEN bugs (RGX-side adapter gaps)**:
  - 40 `simple_escape` cases (`\"`, `\/`, etc) — PGEN parses correctly; RGX's `convert_simple_escape` has no fallback case for "unknown escape character → literal char". Should add one.
  - 42 `class_escape unsupported variant` cases (`[\b]`, `[\c]`) — PGEN routes these to `class_escape` variants RGX doesn't lower. Should expand RGX's class_escape converter.
- **Dependencies**: the harness is in place and gated `#[ignore]` so it doesn't run on `cargo test` by default.

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
| **C1 JIT compilation** | `major` | Constant-factor multiplier (~5-10x) on whichever engine runs. Sequenced after C2 so wins compound. | ✅ **SHIPPED 2026-04-12.** All 9 steps (0–8) of the design doc plan complete. The `jit` Cargo feature is **default-on** as of step 8. With the new default, `cargo test -p rgx-core` runs 957 lib tests (= 695 baseline + 262 C1) — every existing test exercises the JIT path for JIT-eligible patterns. Opt-out via `default-features = false` still works (drops Cranelift entirely from the dependency closure, runs 695 baseline tests). Public design lives in `book/src/internals/jit-compiler.md` (new chapter, ~250 lines). Steps 0–7 history below. Step 0: design proposal. Step 1: standalone `c1/` module. Step 2: eligibility check. Steps 3a–3e: literal/char-class/anchor/word-boundary/control-flow/all-six-quantifier codegen via decoder unfolding. Step 4a: corpus-based differential test harness (27 tests, zero divergences). Step 5: engine dispatch wiring (`Regex::find_first` / `find_all` / `is_match` route through the JIT for JIT-eligible patterns via the 4-tier DFA → Pike-VM → JIT → interpreter dispatch chain). **Step 4b (this commit)**: capture trail in JIT'd code. The JIT'd function signature was extended from `(text, text_len, pos) -> isize` to `(text, text_len, pos, captures_ptr) -> isize`. Per-frame **capture snapshot**: each backtrack frame in the stack-allocated `bt_stack` carries a snapshot of the captures buffer at the moment of the matching `Split` / `SplitLazy` push, and on backtrack-pop the snapshot is restored back into the buffer in one shot. Per-frame size grows from 16 bytes (steps 3a–4a) to `16 + 16 * (num_groups + 1)` bytes; eligibility caps user groups at `C1_MAX_USER_GROUPS = 16` so the per-function stack budget stays bounded (~72 KiB at the cap). Decoder accepts `SaveStart(g)` / `SaveEnd(g)` for any group id (previously only `g == 0`). New `JitOp::Save { group, which }` replaces the step-3a `JitOp::SaveGroupZero { which }`. Engine `try_jit_*` methods allocate a captures buffer of size `2 * (num_groups + 1)`, reset it between calls, and read it back into `MatchResult.groups` after a successful match. **14 new step-4b tests** in `c1::codegen::tests::step4b_*` covering single/multi-capture patterns, capture-with-backtrack (`(a+)b`), lazy capture quantifiers (`(a+?)b`), anchored captures (`\A(\w+)\z`), nested alternation in captures (`(a\|b)c`), three-way captures (`(\w+)@(\w+)\.(\w+)`), and the eligibility cap. **Step 6 (this commit)**: `CharClass(id)` and multi-byte literal codegen. New runtime helper `rgx_runtime_char_class_match_at` (replaces step-1 stub) handles UTF-8 decode + char-class lookup + width-aware return. New `JitOp::CharBytes` variant for multi-byte literals (lengths 2..=4) lowered as inline byte comparisons. New `JitOp::CharClass` variant for custom char classes lowered as indirect call to the runtime helper. Function signature extended to 6 args by adding `char_classes_ptr` + `char_classes_len`. **Differential gate switched to compare against the raw `RegexVM::find_first` interpreter** instead of the public `Regex::find_first` API — the public API's C2 DFA path implements leftmost-LONGEST for negated char classes which conflicts with the JIT/VM's leftmost-FIRST single-char semantics. **19 new step-6 tests** covering `[abc]`, `[a-z]`, `[^0-9]`, `[a-z]+`, `([a-z]+)`, `[a-z][0-9]`, `é` (2-byte), `日` (3-byte), `🦀` (4-byte), `é+`, `(é)`, `日本`, ASCII classes against Unicode text, `[а-я]` Cyrillic Unicode range, plus 4 eligibility tests. **Step 7 (this commit)**: runtime safety helpers (`max_steps` + `max_backtrack_frames`) inlined as Cranelift branches. JIT'd function signature extended to 8 args by adding `max_steps: u64` + `max_bt_frames: u64`. New `emit_step_limit_check` helper called at the start of every JitOp's emit (mirrors the interpreter's main-loop check). New `JIT_LIMIT_EXCEEDED_SENTINEL = -2` distinct from `-1` (no match) so the engine can stop scanning entirely on limit overflow. `emit_backtrack_push` extended with a user-frame-limit check. **Removed `has_runtime_match_limits` exclusion** from `Engine::should_use_jit` — patterns with safety limits set are now JIT-eligible. **13 new step-7 tests**: 5 max_steps codegen, 4 max_bt_frames codegen, 4 engine-integration via the public API. Default build 902 baseline tests unchanged; with `--features jit` **957 lib tests pass** (695 baseline + 262 C1, +13 from step 7). Patterns like `[abc]`, `[a-z]`, `[^0-9]`, `[а-я]`, `é`, `日本`, `🦀` are now JIT-eligible. Next: step 8 (production cutover, benchmarks, Book chapter expanded to its full form — flips the `jit` feature to default-on). |

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
