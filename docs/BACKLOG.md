# RGX BACKLOG
Complete inventory of remaining work — roadmap items, features to port from Rust's `regex` crate, and engineering improvements. Living document.

## How to use this file
- Items are grouped by category, not priority.
- Each item has: description, effort estimate, rationale, and dependencies.
- Effort: `trivial` (<1h), `small` (1-4h), `medium` (1-3 days), `large` (1-2 weeks), `major` (weeks+).
- Move items to `CHANGES.md` when completed.

---

## A. Missing from RGX roadmap

### A1. Exponential backtracking protection ✅ Shipped
- **Status**: `Regex::set_max_steps(Some(limit))` at `rgx-core/src/lib.rs:2040`. VM accumulates a step counter per opcode in `ExecContext`; exceeding the limit causes the match attempt to fail (returns `None`), while the scanning loop is free to try other start positions. Doc-comment example uses the canonical `(a+)+b` pathological pattern. Tests at `lib.rs:8160-8192` cover four cases: pathological-input-aborts, valid-match-not-blocked, `None`-is-unlimited, and per-attempt application (a low limit blocks every start position). User-facing documentation in `book/src/core-api/safety-limits.md`.
- **Production gate**: A1 is the production blocker for any server accepting user-supplied regex patterns. Shipping `set_max_steps` closes the DoS surface. Defaults to `None` (unbounded) so existing users see no behaviour change; servers MUST set a limit explicitly.

### A2. Memory limits ✅ Shipped
- **Status**: all three limits shipped.
  - `Regex::set_max_backtrack_frames(Some(n))` at `lib.rs:2048`. Tests at `lib.rs:8197+`.
  - `Regex::set_max_recursion_depth(Some(n))` at `lib.rs:2056`. Tests at `lib.rs:8215+`. Default hard ceiling of 1024 even when `None`.
  - `Regex::set_max_trail_entries(Some(n))` at `lib.rs:2068`. Tests at `lib.rs:8245+`. Caps the capture-trail length so a single backtrack frame can't grow an unbounded undo log on pathological patterns (e.g. `(.)*` on long input).
- **Production gate**: A1 (`set_max_steps`) + A2's three limits cover every resource axis the backtracking VM can blow up on — CPU time (steps), state count (frames), recursion depth, and per-state memory (trail). Defaults are `None` (unbounded) so existing users see no behaviour change; server deployments accepting user-supplied patterns MUST set limits explicitly. User-facing documentation in `book/src/core-api/safety-limits.md`.

### A3. `tail_file` — file watching/streaming ✅ DONE
- **Status**: shipped. `Regex::tail_file(path, options, on_match)` lives in `rgx-core/src/file.rs` with `TailHandle` / `TailOptions` types and integration tests (`tail_file_detects_appended_content`, `tail_file_from_beginning`).

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
- **What**: `rgx.steer_skip(n)` / `rgx.steerSkip(n)` from Lua / JS / Rhai / WASM code blocks. (Native Rust callbacks already have steering — they return `ExecResult::Steer` directly.)
- **Effort**: `small`
- **Rationale**: Currently steering is native-callback-only. The four *embedded* scripting hosts should have the same power.
- **How**: Add `rgx.steer_*` helper functions to each embedded host's execution context, returning special `ExecResult::Steer` values.
- **Why this set and not C / Python / Julia**: A6 is about *embedded* hosts — languages rgx runs *inside* the regex pattern. The embedded set (Lua, JS, Rhai, WASM, native) was chosen for sandboxability + low embed cost. C lacks a sandboxable runtime; CPython is ~10MB + GIL + not safely sandboxable; libjulia is ~100MB + JIT-heavy. WASM is the back door for anyone wanting C/Go/AssemblyScript inline: compile to WASM, use `(?{wasm:...})`. Calling rgx *from* C/Python/Julia is the FFI direction (A9), a different axis.
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

### A10. `\X` extended grapheme cluster ✅ DONE
- **Status**: shipped. `OpCode::GraphemeCluster = 0x08` emitted by the compiler from `RegexAst::GraphemeCluster`; VM dispatch uses `unicode-segmentation`'s `graphemes(true)` to advance by one cluster per `\X`. Verified end-to-end on ASCII, accented (`é`), single-codepoint emoji (`🦀`), ZWJ family emoji (`👨‍👩‍👧‍👦`, 25 bytes one cluster), and combining marks (`e\u{301}`, 3 bytes one cluster).

### A11. `(*SKIP:name)` named skip ✅ DONE (2026-04-12)
- **What**: `(*SKIP:name)` interacts with `(*MARK:name)` to skip back to a specific mark position.
- **Shipped**: New `VerbSkipNamed` opcode, per-attempt mark registry on `ExecContext`, forward-progress guards at all scan-loop sites. See `CHANGES.md` entry for details.

### A12. Returned-capture subroutines ✅ DONE (2026-05-07)
- **What**: `(?1(grouplist))` — PCRE2 10.47+ syntax for subroutines that return captures.
- **Status**: shipped. `parsing.rs::convert_typed_subroutine_call_object` walks `target.captures`, populates `Regex::ReturnedCaptureSubroutine { target, returned_groups }`. The compiler emits `OpCode::CallReturning = 0x46`; VM dispatches at three sites (main, `execute_at_continuation`, `execute_subexpr_inner_full`). Closed cluster-1B (13 cases at testinput2:8067–8168 family) + cluster-2G (2 cases at testinput2:8109 nested-bracket subjects). Verified part of the 12,806/4 ratchet.

### A13. `(?(VERSION>=...)...)` conditionals ✅ DONE (2026-04-13)
- **What**: Branch on engine version.
- **Shipped**: RGX-side parser-level short-circuit landed 2026-04-12; PGEN 1.1.10 shipped the grammar recognition on 2026-04-13, closing `PGEN-RGX-0016`. Submodule bumped from `ac2acb3` (1.1.9) to `8783757` (1.1.10), the three integration tests in `parsing::tests::version_conditional_*` now run unmodified.

### A14. Partial matching API ✅ DONE
- **Status**: shipped. `Regex::find_first_partial(text) -> PartialMatchResult` lives in `rgx-core/src/lib.rs` (line 2049) with `Complete` / `Partial` / `NoMatch` variants and unit tests at the bottom of the file (`partial_match_full`, `partial_match_partial`, `partial_match_no_match`).

---

## B. Features to port from Rust's `regex` crate

> **Section status (2026-05-13)**: every B-item has shipped. Code locations are listed per entry below. The shipping cadence was incremental over the C2 / TDFA period, but the section was never audited as a batch — this audit closes it. New `regex`-crate-style API gaps belong in a new section, not as additions to B.

### B1. Step/time limits ✅ Shipped
- **Status**: `Regex::set_max_steps(Some(limit))` at `rgx-core/src/lib.rs:2040`. Engine accumulates a step counter per opcode; exceeding the limit returns no match (`None`) instead of looping. Tests at `lib.rs:8164+`. Same machinery satisfies A1 (production safety) and B1 (port from `regex`).

### B2. `RegexSet` — match multiple patterns at once ✅ Shipped
- **Status**: `pub struct RegexSet` in `rgx-core/src/regex_set.rs`. `RegexSet::new(&[...])` / `set.matches(text)` API with `SetMatches` result. Book chapter `book/src/core-api/regex-set.md`.

### B3. Compilation caching ✅ Shipped
- **Status**: `pub struct RegexCache` in `rgx-core/src/cache.rs`. Thread-safe LRU via `RwLock<HashMap<String, Arc<Regex>>>`. Book chapter `book/src/core-api/regex-cache.md`.

### B4. Configurable match semantics ✅ Shipped
- **Status**: `MatchSemantics::{LeftmostFirst, LeftmostLongest}` enum in `engine.rs`. `Regex::set_match_semantics(MatchSemantics::LeftmostLongest)` at `lib.rs:2091`. Tests at `lib.rs:8780+`. Book chapter `book/src/advanced/match-semantics.md`.

### B5. `bytes::Regex` — match on `&[u8]` directly ✅ Shipped
- **Status**: `pub struct BytesRegex` in `rgx-core/src/bytes.rs`. Accepts `&[u8]` without UTF-8 validation. Book chapter `book/src/core-api/bytes-regex.md`.

### B6. Replacer API with capture interpolation ✅ Shipped
- **Status**: `Regex::interpolate_replacement_ext` at `lib.rs:2359` parses `$1`, `${name}`, `$&` in replacement strings. Reused by both `replace` and `replace_all` paths.

### B7. `CaptureMatches` / `Captures` API ✅ Shipped (folded into B13 implementation)
- **Status**: `pub struct Captures<'t>` at `lib.rs:253` with `name()`, `Index<usize>`, `Index<&str>`. Iterator form via `captures_iter` (B12).

### B8. `split` and `splitn` ✅ Shipped
- **Status**: `Regex::split(text)` at `lib.rs:1697`, `Regex::splitn(text, limit)` at `lib.rs:1724`. Lazy variants `split_iter` / `splitn_iter` at `lib.rs:1999/2011`.

### B9. Syntax error diagnostics with spans ✅ Shipped
- **Status**: `CompileError` struct at `rgx-core/src/error.rs:40` with caret-position formatting. Book chapter `book/src/core-api/error-diagnostics.md`.

### B10. `is_match_at` / `find_at` ✅ Shipped
- **Status**: `Regex::is_match_at(text, start)` at `lib.rs:1680`, `Regex::find_first_at(text, start)` at `lib.rs:1658`. Names differ from `regex`'s `find_at`/`is_match_at` to match rgx's `find_first` convention; semantics are identical. Tests at `lib.rs:7916+`. Book chapter `book/src/core-api/position-aware.md`.

### B11. `RegexBuilder` ✅ Shipped
- **Status**: `pub struct RegexBuilder` at `lib.rs:763`. Chainable `case_insensitive()`, `multi_line()`, `dot_matches_new_line()`, etc. Book chapter `book/src/getting-started/regex-builder.md`.

### B12. Iterator-based APIs ✅ Shipped
- **Status**: `FindIter` / `CaptureIter` / `SplitIter` / `SplitNIter` at `lib.rs:1975`/`1988`/`1999`/`2011`. All implement `Iterator`. Book chapter `book/src/core-api/iterators.md`.

### B13. `Captures` wrapper ✅ Shipped
- **Status**: `Captures<'t>` at `lib.rs:253`. Methods: `get(idx)`, `name(name)`, `expand(template, dst)`, `Index<usize>`, `Index<&str>`. Tests at `lib.rs:6221+`.

### B14. `Match` type ✅ Shipped
- **Status**: `pub struct Match<'t>` at `lib.rs:200` with `as_str()`, `range()`, `start()`, `end()`, `len()`, `is_empty()`. Book chapter `book/src/core-api/match-type.md`.

### B15. `replacen` ✅ Shipped
- **Status**: `Regex::replacen(text, limit, replacer)` at `lib.rs:1803` returns `Cow<str>`.

### B16. `Replacer` trait ✅ Shipped
- **Status**: `pub trait Replacer` at `lib.rs:438`. Blanket impls for `&str`, `String`, `Fn(&Captures) -> String`. Book chapter `book/src/advanced/replacer-trait.md`.

### B17. `shortest_match` / `shortest_match_at` ✅ Shipped
- **Status**: `Regex::shortest_match(text)` at `lib.rs:1875`, `Regex::shortest_match_at(text, start)` at `lib.rs:1884`. Returns `Option<usize>` of the match-end byte position.

### B18. `escape()` ✅ Shipped
- **Status**: `pub fn escape(text: &str) -> String` at `lib.rs:177`. Escapes the standard PCRE2 metacharacter set.

### B19. Introspection metadata ✅ Shipped
- **Status**: `Regex::captures_len()` (lib.rs:1898), `Regex::capture_names()` (lib.rs:1963), `Regex::as_str()` (lib.rs:1892). `CaptureNames` iterator at lib.rs:713. `Regex::named_groups()` accessor at lib.rs:2717.

### B20. `CaptureLocations` ✅ Shipped
- **Status**: `pub struct CaptureLocations` at `lib.rs:397`. Reusable across matches. Book chapter `book/src/advanced/capture-locations.md`.

### B21. `Cow<str>` return for `replace` ✅ Shipped
- **Status**: `Regex::replace` / `replace_all` / `replacen` all return `Cow<'t, str>` (lib.rs:1767/1794/1803). No allocation when no match occurs.

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

### C2. NFA/DFA hybrid for simple patterns — ✅ SHIPPED 2026-04-11, Step 8 finalised 2026-05-11
- **What**: Detect patterns that don't use backtracking-only features and run them through a Thompson NFA + lazy DFA cache instead of the backtracking VM.
- **Effort**: `major`
- **Status**: Steps 0–8 complete. Public introspection `Regex::uses_c2()` / `Regex::classification()` promoted from doc-hidden in `f8dda9e` (2026-05-11). Multi-byte memmem inner-literal prefilter (two-stage memchr → memmem) shipped in `fd50b63` (2026-05-11). Conformance ratchet holds at 12,806 / 4 throughout the C2 work. Book chapter at `book/src/internals/nfa-dfa-engine.md` documents dispatch.
- **Rationale**: Guarantees O(nm) for the common case while keeping backtracking for advanced features.
- **Open C2 perf levers (future sessions)**:
  - **DFA `\b` / `\B` word-boundary support** — ✅ SHIPPED 2026-05-12. Forward DFA tier now handles `\b` / `\B` via `DfaStateKey::prev_byte_was_word`, deferred WordBoundary epsilon expansion, and precomputed `accept_when_fire_wb` / `accept_when_not_fire_wb` per state (option (b) from the Phase 2 finding — flag lookup beats option (a)'s per-byte closure re-expansion by ~7×). Two start states (pw=false / pw=true), `start_state_for(input, start)` selects per-call. Phase 1 prep in `26c4953`; Phase 2 + 3 land together in the headline commit alongside the perf result. **`email_basic` find_first: 159 ns (rgx) vs 236 ns (pcre2) = 1.49× faster than PCRE2** (was 3.7× slower — a 5.5× turnaround). Reverse DFA still rejects `\b` patterns (walk-order semantics differ; pipeline shortcut deferred); per-position forward anchored scan handles them instead.
  - **Tagged DFA (Laurikari TDFA) for captures** — current pipeline runs DFA for the match span then re-runs Pike-VM for capture recovery (samply attributes 30–60% of `email_basic.find_all` / `capture_groups.find_all` self-time to `pike_match_at_with_captures`). A tagged DFA recovers captures in one pass. **Phase 0 (design doc) landed 2026-05-08** at `docs/C2_TDFA_DESIGN.md` — covers Laurikari semantics, the tagged subset-construction algorithm, the register-update IR, the 4-phase staging plan, and the differential gate. **Phase 1 (NFA tag inventory helpers) landed 2026-05-08** — `Tag` newtype + `has_capture_tags()` / `num_tags()` / `tagged_epsilons(state)` accessors on `Nfa`. **Phase 2a (TDFA data types + start-state construction) landed 2026-05-08** at `rgx-core/src/c2/tdfa.rs` — `RegOp`, `TaggedTransition`, `TaggedDfaState`, `TaggedDfa`, `TaggedDfa::try_build` with start-state tag firing in epsilon-slot order. **Phase 2b (byte transitions with tag propagation) landed 2026-05-13** — `TaggedDfa::transition(state, cls)` lazy lookup, `compute_transition` with per-source-NFA-state register-map inheritance, RegOp pool, dead/uncached two-sentinel discipline. **Phase 2c (register canonicalisation + dep-ordered Copy ops) landed 2026-05-13** — `canonicalise_register_map`, `build_copy_ops`, `topologically_sort_copies` with scratch-register cycle breaking. **Phase 2d (simulator + differential gate) landed 2026-05-13** — `find_match_at`, lazy-register growth, snapshot-on-accept hot loop, capture readout. **Phase 3 (engine dispatch + Pike-VM bypass) landed 2026-05-13** — TDFA-first dispatch in `try_dfa_find_first`, public `Regex::uses_tdfa()`. **Phase 4 (find_all wiring + perf gate + baseline + book chapter) landed 2026-05-13** — `try_tdfa_find_all` helper, `regression_check` extended with `find_all` benches (14-entry baseline), capture-group gate at dispatch sites (fixes url_simple +43% regression caught by perf gate), measured **find_all/capture_groups = 47× faster than PCRE2** (12 ns rgx vs 561 ns pcre2). Book chapter `book/src/internals/nfa-dfa-engine.md` updated to document the shipped TDFA. **TDFA project complete: 8 commits across Phases 0-4 in one day. Conformance held at 12,806/4 through every commit.**
  - **SIMD byte-class lookup in DFA hot loop** — the inner `transitions[state * num_classes + cls]` lookup is scalar; widening it to SIMD-gather or 64-byte vectorised lookup could give 2-4× on DFA-bound workloads. Effort: `small-medium`. Reference: `regex-automata::dfa::dense` uses similar tricks.
  - **DFA minimization (Hopcroft)** — smaller state count reduces cache pressure. Effort: `medium`. Currently the lazy DFA grows on demand without minimization.
  - **Materialized DFA for small patterns** — when the full DFA fits in <64 states, flatten into a lock-free array instead of the Mutex-protected lazy cache. Effort: `small`. Removes the Mutex lock on the hot path for short patterns.
- **Dependencies**: Significant new engine code, but the existing AST is sufficient — no parser changes needed.

### C7. PCRE2 10.47 differential conformance — bug triage
- **What**: Triage the bugs uncovered by the `rgx-core/tests/pcre2_conformance.rs` differential harness (introduced 2026-04-13).
- **Effort**: `medium` (each bug class is its own investigation)
- **Status as of 2026-05-11, head `0ba42b1`**: ratchet locked at **12,806 pass / 4 fail / 0 panic / 0 skip (~99.97%)** against the full `testinput1..29` corpus. Cumulative progression from 2026-05-05 to 2026-05-11: 12,716 → **12,806** (+90 passes; 94 → 4 fail). The residual **4** failures are at the engine frontier and fall into two cohorts:
  - **PGEN-blocked (1)**: `testinput1:3910` — `()()()()()()()()()(?:(?(10)\10a|b)(X|Y))+`. PGEN parses `\10` as a backref to group 10 when only 9 groups have been seen at the parse position; PCRE2's "longest digit run / count groups seen so far" rule says it should be the octal escape `\010` (U+0008). Filed as **PGEN-RGX-0084** with full artifact bundle. Ratchet ticks +1 when PGEN ships the fix; **no RGX-side workaround per the no-PGEN-workarounds doctrine**.
  - **Engine-frontier (3)**: `testinput2:6592` (complex multi-iter lookahead + backref `\G(?:(?=(\1.|)(.))){1,13}?(?!.*\2.*\2)\1\K\2`), `testinput2:6595` (`|(?0).` /endanchored), `testinput2:6601` (`(?:|(?0).)(?(R)|\z)`). All three require the engine to backtrack from an outer-failure INTO a subroutine call's body to explore deeper-recursion / alternate paths. The 2026-05-11 `SubroutineRetryMode::{Shorter,Different}` mechanism handles "subroutine made progress, wrong end position"; these cases need "subroutine matched empty, caller needs progress" — a different family requiring subroutine-internal alt-frame reification (cross-subexpr alt-frame promotion). 6595 additionally needs an engine `ANCHORED_END` option since the harness's `\z` wrap propagates incorrectly into recursive `(?0)` / `(?R)` calls. Substantial new engine work; deferred to a future session.
- **Engine-fix family-tree (2026-05-07 → 2026-05-11)**: Cluster 1C napla (+6) → CallReturning subexpr dispatch (testinput2:8092, +1) → ANYCRLF treats CRLF as single unit (+1) → assertion-scoped COMMIT/SKIP (+) → empty-alt lazy quantifier (+4) → StarGreedyBlock symmetric extension (+4) → conditional lookahead in repeated alt (+3) → typed walker for `target.captures` (+) → substitute empty-match retry at same pos / NOTEMPTY_ATSTART (+) → ACCEPT scoping inside napla (+) → SKIP:NAME with atomic-MARK (+3) → too-permissive validation (+4) → suppress `\K` in lookarounds/subroutines (+) → AltSplitLong/JumpLong for >64KB alt bodies → lookbehind body codepoint-length narrowing + SKIP propagation (+1) → `StarGreedy(Call)` retry-shorter (+2) → narrow retry-different on `Call`-followed-by-backref (+1) → `SubroutineRetryMode` Shorter|Different split (+4) → scope `(*THEN)` FullyDegraded to subroutine call (+1).
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

### C8. PCRE2 conformance audit — recommendations (2026-05-05)
The conformance fix audit at [`book/src/internals/pcre2-conformance-audit.md`](../book/src/internals/pcre2-conformance-audit.md) §6 enumerates prioritized items. Living document — sync this entry with the audit when the audit changes.

**Tier 0 — Now (small, high-leverage cleanups)**
- **C8.1.1 Per-verb effects table (doc-only)** — write the eight `apply` functions for `(*COMMIT)` / `(*PRUNE)` / `(*SKIP)` / `(*SKIP:name)` / `(*THEN)` / `(*ACCEPT)` / `(*FAIL)` / `(*MARK:name)` as Rust pseudocode, citing the pcre2pattern(3) §"Backtracking control" lines that justify each effect and the current commit that handles it. Place in the audit's §5.1. The effects model scales to **any number of verbs in a branch** by sequential composition — there are no pair lookups, no triple lookups, no N-tuple lookups. **Effort**: `trivial` (0.5 day). **Unlocks**: C8.2.1.
- ~~**C8.1.2 `atomic_depth` field replacing `!ctx.call_stack.is_empty()` predicate**~~ ✅ shipped 2026-05-06. New `atomic_depth: u32` on `ExecContext`, bumped at `OpCode::AtomicStart` (3 sites), decremented at `AtomicEnd`. The `(*COMMIT)` predicate now tests `ctx.atomic_depth > 0`. Latent semantic gap closed; corpus unaffected.
- ~~**C8.1.3 `alt_boundaries` manual-truncation audit**~~ ✅ shipped 2026-05-06 (audit §5.3). Phase-2 verb-effects refactor (`efb69b3`, `ad49523`) gave parallel pop sites in `try_backtrack` and `local_backtrack_or_return_false!` that share the cleanup contract. The remaining manual truncations are intentional bodies of `verb_apply_then`/`verb_apply_prune`/`AltScopeEnd` and don't need refactoring.
- **C8.1.4 Boundary-policy doc (no code)** — write the propagation rules (`committed`, `skip_position`, `accept_forced`, captures, `match_start_override`) for each of the 8 boundary kinds (positive_lookahead, negative_lookahead, positive_lookbehind, negative_lookbehind, atomic_group, subroutine_call, napla, naplb). **Effort**: `small` (1 day). **Unlocks**: C8.2.2 and Cluster 1C closure.

**Tier 1 — Next (medium-term audits before more whack-a-mole accretes)**
- **C8.2.1 Per-verb effects refactor (replaces per-pair patches and scales to N verbs)** —
  - **Phase 1 ✅ shipped 2026-05-06 (efb69b3)**: Centralized `verb_apply_*` associated functions for all 6 verbs ((*COMMIT), (*PRUNE), (*SKIP), (*SKIP:name), (*THEN), (*MARK:name)) at `rgx-core/src/vm.rs:2200-`; all 3 dispatch sites (top-level, continuation, subexpr) call the same functions. Last-verb-wins precedence encoded inside the apply functions. Conformance ratchet unchanged at 12,719/91 — semantic refactor.
  - **Phase 2 ✅ shipped 2026-05-06 (next commit)**: Deferred stack-clear for (*COMMIT) (non-atomic). `try_backtrack` honors `committed` and clears at failure-time; `OpCode::Char` and `OpCode::Fail` routed through `try_backtrack`. `ThenOutcome` enum split into `Redirected` / `ScopeExhausted` / `FullyDegraded` (last uses `alt_scope_marks` for lexical-scope detection). Closes Cluster 1D testinput1:5457. Conformance ratchet: **12,720 / 90** (+1 pass).
  - **Phase 3 ✅ shipped 2026-05-06 (+2 passes)**: `pending_alt_revival` slot bridges SKIP/PRUNE eager-clear to a following THEN. Closes testinput1:5447 (SKIP+THEN) and testinput1:5452 (PRUNE+THEN). All 3 dispatch sites plumbed. Verb-effects family fully closed for the corpus.
- **C8.2.2 Boundary-policy refactor** — convert the 8 boundary kinds into `BoundaryPolicy` const values; replace per-kind ad-hoc propagation logic with policy lookups. Minimum viable: collect existing dispatch into one place. Maximum: add napla as a new policy and ship Cluster 1C. **Effort**: `medium` (5-10 days). **Unlocks**: Cluster 1C (5 cases), prevents future propagation-asymmetry latent bugs.
- **C8.2.3 Pike-VM step-limit threading** — pick option A (thread `max_steps` through Pike-VM as a state-transition counter) or option B (remove the limit gate from `Engine::should_dispatch_to_c2` for limits whose purpose is catastrophic-backtracking protection). **Effort**: `medium` (2-5 days). **Unlocks**: testinput2:6244/6249 (2 cases) plus removes a documented contract divergence.
- **C8.2.4 PGEN walker silent-shape audit** — every typed-shape arm in `rgx-core/src/parsing.rs::convert_typed_*` that has the pattern `if let Some(s) = elem.as_str()` should be replaced with `walk_json_terminal_chars` per element. Preventive — nothing is currently red, but four post-PGEN-1.1.75-bump silent-shape gaps in May 2026 had this signature. **Effort**: `small-medium` (2-3 days). **Unlocks**: resilience to PGEN typed-shape changes.

**Tier 2 — Later (speculative larger redesigns)**
- **C8.3.1 Subroutine-stack reification** — recursive captures across quantifier iterations need a "previous iteration's completed capture" read-only slot (Cluster 1A polish landed 2026-05-06 via doubled capture vector + prev-iter slot; 11/16 cases closed). Cluster 1B + 2G (returned-capture subroutines) is now an RGX-only typed-walker change in `parsing.rs::convert_typed_subroutine_call_object` reading `target.captures` — see A12. Together with Cluster 2A balanced-bracket recursion: residual ≈ 24 cases. **Effort**: 1B+2G `small` (parser walker only, half day); 1A residual + 2A `major` (weeks).
- **C8.3.2 Compile-time `(*NUL)`/`(*CRLF)` newline-mode threading** — defer `.` rewrite under `(*CRLF)` etc. to compile time so `/s` flag context is known. **Effort**: `medium` (2-3 days). **Unlocks**: ~3 cases.
- **C8.3.3 `\K` propagation from inside lookarounds** — non-local engine change for residual Cluster 2C. **Effort**: `medium` (5-10 days). **Unlocks**: 3 cases plus lookbehind variants.
- **C8.3.4 Reverse-DFA pipeline unanchored extension** — `find_first` / `find_all` don't currently use the forward-unanchored DFA due to leftmost-LONGEST vs leftmost-first semantics. Not a conformance issue, a perf-headroom item. **Effort**: `medium`.

**Dependencies between items**: C8.1.1 → C8.2.1; C8.1.4 → C8.2.2; C8.2.1 supersedes any future per-verb-pair fix proposal (including the held `commit_saved_alt` work for testinput1:5457).

### C3. Fuzzing infrastructure ✅ DONE
- **Status**: shipped. `fuzz/` directory with 4 cargo-fuzz targets — `fuzz_compile`, `fuzz_match`, `fuzz_replace`, `fuzz_roundtrip` — each runs through libfuzzer-sys + arbitrary. The BACKLOG entry was stale.
- **Follow-up**: a future task could wire one of the fuzz targets into CI on a short-budget basis (e.g., `cargo fuzz run fuzz_compile -- -max_total_time=60`) to catch regressions on every PR. Not urgent; the local-run path is enabled.

### C4. Benchmark CI ✅ DONE (2026-05-12 in `5273de1`)
- **Status**: shipped. New `rgx-bench/src/bin/regression_check` binary times find_first on the 7 shared PATTERNS, computes the rgx-vs-PCRE2 ratio, compares vs `rgx-bench/baselines/main.toml`, exits 1 if any ratio regressed >20%. New CI job `benchmark-regression-check` runs on every PR + push to main. Update procedure: `cargo run --release -p rgx-bench --bin regression_check -- --update-baseline` then commit the new baseline alongside the intentional perf change. The criterion bench job (push-to-main only, artifact upload) stays for historical capture; the regression gate is the merge condition.

### C5. Remove scaffold files ✅ DONE (2026-04 sometime)
- **What**: Originally tracked deletion of `cache.rs`, `simd.rs`, `javascript.rs`, `wasm.rs` placeholders. All scaffold files now either deleted or grown into real modules: `cache.rs` is the working 231-line `RegexCache`; `lua.rs`/`rhai.rs` are 21-24 line feature-gated re-exports (type alias to `RgxError` when feature is off, real engine when on); `simd.rs`/`javascript.rs`/`wasm.rs` no longer exist as separate files (SIMD lives inline in hot paths, JS lowered to JIT codegen, wasm lives in its own `rgx-wasm` workspace crate).
- **Status**: closed. Entry retained as a forward-search anchor.

### C6. Clean remaining clippy warnings
- **What**: Fix the ~479 remaining lint warnings in `rgx-core` (most are doc-string nits, trace-gated unused variables, and `clippy::pedantic` opinions that don't affect correctness). Audit the lint surface and either fix or `#[allow]` with rationale.
- **Effort**: `small` (1-2 days for the lint pass plus a follow-up commit to refresh CI baselines).
- **Rationale**: Clean CI output and reduce the noise floor when reviewing diffs. Original BACKLOG entry claimed ~25 warnings; the lint cliff has grown since the C2 sprint (multi-thousand-line files mean more pedantic hits per file) and the count now reads ~479. Most are repetitive (missing `# Errors` doc on internal helpers, `must_use` on builder methods); a single pass cleans the bulk.
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
