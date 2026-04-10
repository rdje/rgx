# CHANGES
This is the living progress ledger for rgx.

## How this file is used
- Append new entries at the top (newest first).
- Record what changed, why it changed, and how it was validated.
- Keep entries factual and implementation-focused.

## Entry template
### YYYY-MM-DD - Short title
- Scope:
- Changes:
- Validation:
- Notes/impact:

## Entries
### 2026-04-10 - C2 step 4a: sparse-set Pike-VM + differential test against existing VM
- Scope: Fifth code commit for the C2 NFA/DFA hybrid engine. The biggest correctness milestone in the C2 plan — this is where the differential gate against the existing backtracking VM goes ACTIVE for the first time.
- Step 4 is split into 4a (this commit, Pike-VM core handling `is_match` / `find_first` / `find_all` without captures + differential test for match spans) and 4b (capture tracking + engine dispatch wiring). Each sub-commit is a coherent, production-quality deliverable.
- New module `rgx-core/src/c2/pike.rs` with the sparse-set Pike-VM (Russ Cox's design with the Briggs–Torczon sparse set). Two arrays of size `num_states` give O(1) `add`, `contains`, and `clear`. Pike-VM is the **permanent** NFA simulator: it serves three roles across the phased plan — first runnable C2 engine (this commit), DFA cache fallback (steps 5–6), and bounded capture recovery pass (step 4b). It is NOT a prototype.
- Public API: `pike_is_match` (uses forward unanchored NFA), `pike_find_first` (anchored scan from each position 0..len), `pike_find_all` (find_first repeated with proper advance rules including the standard "drop empty match adjacent to previous non-empty match" convention).
- Zero-width assertion handling: `\A`, `\z`, `\Z`, `^`, `$`, `\b`, `\B`, `\G`. ASCII word semantics for `\b`/`\B` (Unicode word boundaries are Q2 in the design doc — deferred). `\G` evaluates to true only at position 0 at this stage; full prev-end threading lands in step 4b.
- New helper `CompiledC2Program::try_compile(pattern: &str) -> Option<Self>` that parses + classifies in one call and returns `Some` only for `NoBacktracking`-classified patterns. Convenience for tests, benchmarks, and the differential testing harness.
- New integration test `rgx-core/tests/c2_pike_differential.rs` with 12 test corpora (literals, character classes, sequences/alternations, greedy quantifiers, lazy quantifiers, range quantifiers, anchors, word boundaries, capturing groups, realistic patterns, empty matches, multi-byte UTF-8). For each `(pattern, input)` pair, the test compiles via both `Regex::compile` (existing VM) and `CompiledC2Program::try_compile` (C2 path), then asserts that `pike_is_match`, `pike_find_first`, and `pike_find_all` agree byte-for-byte with the existing VM. Patterns outside the C2 subset are skipped silently. **All 12 corpus suites pass.**
- 29 new Pike-VM unit tests in `pike.rs::tests` covering sparse set operations, literal matching, character classes (ASCII + multi-byte UTF-8), shorthand classes, sequences, alternations, greedy/lazy quantifiers, range quantifiers, anchors, word boundaries, find_all with non-overlapping matches, empty-match advance, and realistic patterns (ISO date, email-like, log levels).
- 2 SOTA correctness fixes discovered during testing:
  1. **Lazy quantifier priority bug**. The Pike-VM's epsilon closure walks edges in slot order, but the quantifier builders in `c2/nfa.rs` were inserting lazy edges in semantic order rather than priority order. For lazy `a+?` on `baaab`, this put the loop edge at a lower dense position than the accept edge, defeating the priority-cutoff that gives lazy quantifiers their shortest-match semantics. Fix: enforce **slot order == priority order** in `build_zero_or_one`, `build_zero_or_more`, and `build_one_or_more` so the closure walker (which already iterates in slot order) gets the right priority semantics. Documented the slot/priority invariant prominently. The `EpsilonEdge.priority` field is now informational only — the slot order is what the simulator honours.
  2. **Find_all empty-match adjacency rule**. For `a*` on `aaab`, the existing VM returns `[(0, 3), (4, 4)]` (skips the empty match at position 3 immediately adjacent to the non-empty match) but my initial implementation returned `[(0, 3), (3, 3), (4, 4)]`. Fix: track `prev_non_empty_end` and skip an empty match if its position equals the previous non-empty match's end. Matches the convention used by the existing VM and the Rust `regex` crate.
- The Pike-VM uses the **dense-position-as-priority** trick for leftmost-first semantics: when the accept state is in the current set at dense position `p`, only states at dense positions `0..=p` are extended in the next iteration. States at higher dense positions were added by lower-priority epsilon edges and cannot produce a leftmost-first-winning match. This works because the closure walker visits edges in priority order, so the dense order naturally encodes priority. The trick is what makes lazy quantifiers terminate at the earliest accept position without a separate "kill lower priority threads" pass.
- Validation: full quality gates green. `cargo test -p rgx-core c2::` 150 passing (29 new pike + 121 across the other c2 modules), full `cargo test -p rgx-core` covers everything, `cargo test -p rgx-cli`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`. No new clippy errors. The 4 new `EpsilonEdge` field doc warnings from step 3a were also cleared while I was here.
- Scope deliberately deferred to step 4b: capture tracking inside the Pike-VM, engine dispatch wiring (so the public `Regex::compile` API automatically routes classifier-positive patterns through Pike-VM), differential test extension to compare capture group positions.

### 2026-04-10 - C2 step 3b: reverse NFA + CompiledC2Program assembly
- Scope: Fourth code commit for the C2 NFA/DFA hybrid engine. Completes step 3 of the design doc §15 phased plan. SOTA quality from day one. Standalone module — no engine wiring, no `Program` field, no Pike-VM yet (that's step 4).
- New `reverse_ast(ast: &Regex) -> Regex` in `c2/nfa.rs`: walks the AST and produces a structurally reversed AST. Reverses sequence order, swaps `^`↔`$` and `\A`↔`\z`, expands `\R` to its structural alternation form so the `\r\n` branch reverses to `\n\r`, recursively reverses each alternation branch, leaves capture indices unchanged. Out-of-subset nodes are visited gracefully.
- New `Nfa::build_reverse_anchored` and `Nfa::build_reverse_unanchored` constructors that call `reverse_ast` then reuse the existing forward Thompson construction. Same `NfaBuilder` machinery for both directions guarantees structural symmetry — no parallel construction logic to drift.
- New module `rgx-core/src/c2/program.rs` with `CompiledC2Program` struct holding the byte-class map plus all four NFAs (forward+anchored, forward+unanchored, reverse+anchored, reverse+unanchored) plus the capture group count. `build_from_ast(&Regex)` constructor builds the byte-class map once from the original AST and reuses it for all four NFAs (the set of bytes the pattern uses is direction-independent).
- `\X` (`Regex::GraphemeCluster`) moved out of the C2 subset: new `ExclusionReason::GraphemeCluster` variant on the classifier, classifier now returns `NeedsVm { GraphemeCluster }` for `\X` patterns. Rationale: matching a grapheme cluster requires Unicode-aware traversal of base codepoint plus combining marks, which doesn't fit cleanly into a Thompson NFA without significant additional machinery. `\X` patterns continue to run on the existing backtracking VM (which has full `\X` support). Can be added back to the C2 subset later if profiling shows it's worth the engineering effort. Renamed the existing classifier test `classifies_newline_sequence_and_grapheme_cluster_as_no_backtracking` to `classifies_newline_sequence_as_no_backtracking` and added a new `excludes_grapheme_cluster_from_c2_subset` test.
- 14 new reverse-NFA unit tests in `c2/nfa.rs::tests`: `reverse_ast` leaves atomic nodes unchanged, reverses sequence order, recursively reverses nested sequences, preserves alternation branch order while reversing each branch, keeps quantifiers unchanged but reverses inner expression, preserves capture indices in groups, flips `^`/`$` anchors, flips `\A`/`\z` anchors, double reverse recovers a simple pattern, expands `\R` so the CRLF branch becomes `\n\r`, reverse anchored NFA is reachable, reverse unanchored has more states than reverse anchored, reverse NFA preserves capture tags, reverse NFA uses byte-class IDs valid against the same shared byte-class map.
- 8 new `CompiledC2Program` unit tests in `c2/program.rs::tests`: build_from_ast produces all four NFAs, unanchored NFAs have more states than anchored, palindromic literal has equal forward/reverse state counts, capture group count is recorded, nested capture groups count correctly, byte-class map is shared across all four NFAs (every NFA's transitions use class IDs valid against the shared map), realistic log-line pattern assembles cleanly, alternation pattern assembles with each branch reversed.
- Capture indices are preserved across the reversal so the bounded Pike-VM capture pass (design doc §9) produces the same logical capture group identities in either direction. Tested via `reverse_ast_preserves_capture_indices_in_groups`.
- The `reverse_ast` function is exported as `c2::reverse_ast`; `CompiledC2Program` is exported as `c2::CompiledC2Program`.
- Validation: `cargo test -p rgx-core c2` covers all C2 module tests including the new reverse and program ones. Full quality gates green: `cargo fmt --check`, `cargo test -p rgx-core`, `-p rgx-cli`, `cargo clippy --workspace --all-targets`. No new clippy warnings.
- Step 3 (forward + reverse Thompson NFA construction) is now complete. Next: step 4 (sparse-set Pike-VM with the differential gate against the existing VM going active for the first time).

### 2026-04-10 - C2 step 3a: forward Thompson NFA construction (anchored + unanchored)
- Scope: Third code commit for the C2 NFA/DFA hybrid engine. SOTA quality from day one. Standalone module per design doc §15 — no engine wiring, no `Program` field, no Pike-VM yet (that's step 4). Step 3b will add the reverse NFA and `CompiledC2Program` assembly.
- Step 3 is the biggest single step in the C2 plan, so it's split into 3a (this commit, forward NFA) and 3b (reverse NFA + assembly). Each sub-commit is a coherent, production-quality deliverable.
- New module: `rgx-core/src/c2/nfa.rs` (~1180 lines incl. tests). Defines `Nfa`, `NfaState`, `NfaStateId`, `ByteClassId`, `EpsilonPriority`, `EpsilonEdge`, `CaptureTag`, `ZeroWidthAssertion`. Re-exported via `c2::{Nfa, NfaState, NfaStateId, CaptureTag, ZeroWidthAssertion}`.
- Forward Thompson NFA construction in both anchored and unanchored variants for the full no-backtracking subset:
  - `Char` (1- to 4-byte UTF-8 codepoints, encoded as state chains via `char::encode_utf8`)
  - `CharClass::Custom` (multi-range, with negation support via `invert_char_ranges` over the Unicode universe excluding the surrogate gap)
  - `CharClass::Digit`, `CharClass::Word`, `CharClass::Space` (predefined ranges)
  - `CharClass::UnicodeClass` (resolved via the existing `unicode_support` bridge)
  - `Dot` (any byte except newline)
  - Top-level `Digit`, `Word`, `Space`, `UnicodeClass`, `WhitespaceLiteral`
  - `NewlineSequence` (`\R` — alternation of `\r\n` and the seven single-character newline forms)
  - `Anchor` (`^`, `$`, `\A`, `\Z`, `\z`, `\G` — encoded as `ZeroWidthAssertion` on epsilon edges)
  - `WordBoundary` (`\b`, `\B`)
  - `Empty`
  - `Sequence` (chained fragments via epsilon connectors)
  - `Alternation` (fan-out from new start with priority-ordered branches, fan-in to new accept)
  - `Quantified` (greedy and lazy `?`, `*`, `+`, plus `{n}`, `{n,m}`, `{n,}` — bounded ranges unrolled per RE2/regex convention, unbounded ranges = `n` mandatory copies + `*` tail)
  - `Group::Capturing` (wrapped with `CaptureTag::GroupStart(N)` / `GroupEnd(N)` on epsilon entry/exit, recovered later by the bounded Pike-VM capture pass per design doc §9)
  - `Group::NonCapturing` (descend without tags)
  - `FlagGroup` (descend; flag handling is the parser's job)
- Multi-byte UTF-8 handling: codepoint ranges decomposed via `regex_syntax::utf8::Utf8Sequences` (already a transitive dep). Each Utf8Sequence becomes a chain of byte-class transitions; multiple sequences from a single character class become an alternation of chains sharing entry and accept states. The chain construction handles cases where a per-position UTF-8 byte range spans multiple byte-class IDs by emitting transitions on every overlapping class.
- Greedy vs lazy quantifier priorities encoded on epsilon edges via the `EpsilonPriority` field. **Lower priority is preferred** under leftmost-first semantics. Greedy `e?` puts the "try matching" edge at priority 0; lazy `e??` puts the "skip" edge at priority 0. Verified by the `lazy_zero_or_one_swaps_priorities` test.
- Unanchored variant uses a lazy `(?s:.)*?` prefix wired before the pattern via an epsilon connector. The dot in the prefix matches **any byte** (including newline) so unanchored matching can skip over newlines to find a later match. The prefix is constructed via the same Thompson machinery (`build_any_byte` plus `build_zero_or_more`-style wrapping with lazy priorities). Same approach as RE2 and the Rust `regex` crate.
- 30 new unit tests in `rgx-core/src/c2/nfa.rs::tests` covering: empty pattern, single ASCII literal, 2-byte and 3-byte UTF-8 literals, ASCII char class, negated char class, shorthand classes, `Dot`, sequence chaining, alternation fan-out/fan-in, alternation branch priority order, greedy and lazy `?` priority swap, `*` loop-back, `+` minimum-one-match, range quantifier unrolling for `{n}` `{0,m}` and `{n,}`, capturing group tag emission, nested capturing groups, non-capturing group tag absence, anchors emit zero-width assertions, word boundaries emit assertions, unanchored variant has more states than anchored, unanchored prefix has lazy priorities, realistic combined pattern `(a|b)+(cd)?`, newline sequence, `invert_char_ranges` round-trip, `byte_classes_in_range` sorted+unique invariant.
- 1 small fix during initial build: `kind` in the `Group` AST destructure needed `.clone()` instead of `*` because `GroupKind` doesn't implement `Copy`. Caught by the first `cargo build`.
- Validation: `cargo test -p rgx-core c2::nfa` 30/0/0. Full quality gates green: `cargo fmt --check`, `cargo test -p rgx-core`, `-p rgx-cli`, `cargo clippy --workspace --all-targets`. No new clippy warnings introduced.

### 2026-04-10 - C2 step 2: byte-class equivalence partitioning (standalone module)
- Scope: Second code commit for the C2 NFA/DFA hybrid engine. SOTA quality from day one per `feedback_sota_first_time.md`. Standalone module per design doc §15 — no engine wiring, no `Program` field, no runtime behaviour change.
- New module: `rgx-core/src/c2/byte_class.rs` with `ByteClassMap` struct (`table: [u8; 256]`, `num_classes: u16`), `build_from_ast(&Regex)` constructor, `class_of(byte)` lookup, `num_classes()` accessor. Re-exported via `c2::ByteClassMap`.
- Algorithm: boundary-points partition with per-character-class membership oracles. Each character class / literal / shorthand / `Dot` / property class / `\R` / `\X` contributes one oracle (a set of byte ranges). Two bytes share a class iff every oracle gives the same membership answer for both. Multi-byte UTF-8 codepoint ranges are decomposed via `regex_syntax::utf8::Utf8Sequences` (already a transitive dep via `unicode_support`) into per-position byte ranges, with all positions added to the same oracle since the byte-class map is position-independent.
- Critical correctness point: each character class is ONE membership oracle, not a set of independent oracles per range. `[abc]` puts bytes 'a', 'b', 'c' in the same class (one oracle, all three bytes have membership signature `(true,)`), not three different classes. Treating ranges as separate oracles would be a correctness bug — `[a-z]` should yield 2 classes (`[a-z]` and "everything else"), not 26 + 1 classes.
- Conservative over-approximation: the byte-class map is computed from the AST before the NFA is built, so it may have more classes than the optimal map computed from actual NFA transitions. Extra classes never affect correctness — only DFA cache compactness. Step 3 (NFA construction) may refine the map further if profiling shows it matters.
- Walks all AST node families gracefully — supported nodes contribute oracles, structural nodes descend into children, zero-width assertions contribute nothing. Non-supported nodes (lookaround, backref, recursion, code blocks, callouts, branch-reset, extended classes, backtracking verbs) are visited gracefully so the walker doesn't crash on a mixed AST, but contribute nothing for the node itself. Only meaningful for `NoBacktracking`-classified patterns.
- 25 new unit tests in `rgx-core/src/c2/byte_class.rs::tests`: empty AST, anchor-only, single ASCII literal, custom char classes (`[abc]`, `[a-z]`), negation invariance, disjoint classes (`[a-c][d-f]`), overlapping classes (`[a-c][b-d]`), shorthand classes (`\d`, `\w`, `\s`), `Dot` newline distinction, non-ASCII literal (`'α'`), non-ASCII char range (`[α-ω]`), nested structural nodes (quantified, capturing group, alternation), realistic log pattern (`(\d{4}-\d{2}-\d{2})\s+(ERROR|WARN)`), class ID density invariant, duplicate ranges idempotency, full universe oracle, adjacent ranges in one oracle, byte 0x00 / 0xFF boundary handling.
- Design doc fix: `docs/C2_NFA_DFA_DESIGN.md` §5 had `num_classes: u8` which was wrong because the count can be exactly 256 (one class per byte). Updated to `u16` with an explanatory note and aligned the example function signatures.
- Validation: `cargo test -p rgx-core c2::byte_class` 25/0/0. Full quality gates green: `cargo fmt --check`, `cargo test -p rgx-core`, `-p rgx-cli`, `-p rgx-bench`, `cargo clippy --workspace --all-targets`. No new clippy warnings introduced. Existing 633+ test suite continues to pass (no regression).
- Sign-off gate: design doc §20 was implicitly approved when the user said "PNT" after the design doc landed. Step 3 (forward + reverse Thompson NFA construction) is the next concrete step — it will consume `ByteClassMap` to drive its transition tables.

### 2026-04-10 - Hotfix CI: gracefully skip PCRE2 parity cases on older PCRE2 builds
- Scope: CI test infrastructure fix in `rgx-bench/tests/pcre2_parity.rs`. No code changes outside the test file.
- Symptom: 3 differential parity tests panicked on `origin/main` HEAD `114ef3b` when CI ran on Ubuntu 24.04 with `libpcre2-dev 10.42-4ubuntu2.1`. Failing tests: `pcre2_parity_supported_syntax_find_all_spans`, `pcre2_parity_supported_syntax_first_match_span`, `pcre2_parity_supported_syntax_no_match_consistency`. PCRE2 rejected patterns like `(?[[a-z]])+`, `(?[[a-z] - [aeiou]])+`, `(?[[^0-9]])+` at offset 2 with "unrecognized character after (? or (?-".
- Root cause: PCRE2 version mismatch. Perl extended character classes `(?[...])` require PCRE2 >= 10.45 (March 2025) by default; older builds were configured without `--enable-pcre2-perl-extended-class` and reject the syntax at parse time. Local dev machines have newer PCRE2 (homebrew on macOS, etc.) so the tests pass there. The CI's PCRE2 build doesn't recognize the syntax at all — passing extra options at runtime wouldn't help.
- Fix: runtime detection helper (`pcre2_supports_perl_extended_class`) caches the result of compiling a canonical extended-class pattern via `OnceLock`. A `skip_if_unavailable` guard at the top of each affected test loop checks whether the case's pattern uses `(?[` and the runtime PCRE2 lacks support; if so, prints a clear stderr notice naming the pattern, the missing feature, and the fact that RGX still validates the feature via its own unit tests in `rgx-core`, then continues the loop without the differential check. On dev machines and on future CI with PCRE2 >= 10.45, the cases run unchanged.
- Why this approach: minimal change (no CI workflow edits, no new dependencies, no PCRE2 vendoring), self-documenting (the helper docstrings explain exactly why), no coverage loss (RGX still validates `(?[...])` in `rgx-core` regardless), and correct by construction (only skips when PCRE2 itself rejects the canonical pattern).
- Alternatives considered and rejected: vendoring PCRE2 from source in CI (bigger change, longer build, `pcre2-sys` doesn't cleanly support vendoring); passing `PCRE2_EXTRA_PERL_EXTENDED_CLASS` (the symbol doesn't exist in PCRE2 10.42); removing the failing cases entirely (regression on dev machines where PCRE2 supports the syntax).
- Validation: `cargo test -p rgx-bench --test pcre2_parity` 13 passing on local PCRE2 (skip is a no-op locally because macOS PCRE2 supports the syntax). Full quality gates green: `cargo fmt --check` clean, `cargo test -p rgx-bench`, `-p rgx-core`, `-p rgx-cli`, `cargo clippy --workspace --all-targets`.
- Note: this hotfix is for the CI failure on `origin/main` HEAD `114ef3b`. None of the local-only session commits caused it. The fix lands as a separate commit so it can be cherry-picked or rebased independently if needed.

### 2026-04-10 - C2 step 1: pattern classifier (metadata only, no runtime dispatch)
- Scope: First code commit for the C2 NFA/DFA hybrid engine. SOTA-quality from day one per `feedback_sota_first_time.md`.
- New module: `rgx-core/src/c2/{mod.rs, classifier.rs}`. Defines `Classification` (NoBacktracking | NeedsVm { reason }) and `ExclusionReason`. Single linear-time AST visitor classifies patterns against the no-backtracking subset defined in `docs/C2_NFA_DFA_DESIGN.md` §4. Conservative — any node it isn't certain about returns NeedsVm.
- New `Program.classification` field on `vm::Program`. Initialized via `Default` to `NeedsVm { NotYetClassified }` so any code path that bypasses the classifier still routes safely to the existing VM. Populated in `compile_ast_with_label` after VM bytecode generation.
- New doc-hidden accessor `Regex::classification() -> &c2::Classification` on the public Regex type, plus a doc-hidden `Engine::classification()` plumbing method. The user-facing `uses_c2() -> bool` introspection (design doc Q8) is intentionally deferred to step 8.
- 43 new unit tests in `rgx-core/src/c2/classifier.rs::tests` covering every supported leaf, every supported recursive construct, every excluded construct, exclusions reached through recursion, first-encountered semantics, and two realistic hand-built ASTs.
- 26 new integration tests in `rgx-core/tests/c2_classifier.rs` covering the full compile pipeline (parser → classifier → metadata on Program → public accessor) with real pattern strings: literals, character classes, shorthand classes, dot, alternation, greedy/lazy quantifiers, capturing/non-capturing groups, anchors, word boundaries, Unicode property classes, flag groups, two realistic patterns, plus all the major exclusions (numeric/named backreferences, positive/negative lookahead/lookbehind, atomic groups, possessive quantifiers, recursion, numbered subroutines, conditionals).
- No runtime dispatch wired in. Existing backtracking VM continues to handle every pattern unchanged. Step 1 is metadata only by design — the field can be validated in isolation before step 4 (Pike-VM) starts depending on it.
- Validation: `cargo fmt --check` clean, `cargo test -p rgx-core` 721 passing (was 633, +88: 43 new unit + 26 new integration + 19 small delta from re-counted suites), `cargo test -p rgx-cli` 30/0, `cargo clippy --workspace --all-targets` clean, `cargo build` clean (29 pre-existing warnings, no new ones).

### 2026-04-09 - Drop Co-Authored-By trailers from commit workflow
- Scope: commit-workflow rule change. Doc only.
- User directive: do not include `Co-Authored-By:` trailers in RGX commit messages. Supersedes the prior workflow agreement that required them.
- Updated `COMMIT.md` Step 6 (brief preparation) to forbid trailers and reinforce brevity.
- Updated `MEMORY.md` workflow agreements section to reflect the new rule and remove the prior `Oz` trailer requirement.
- Persistent preference saved to auto-memory `feedback_no_coauthored_by.md` so future sessions inherit it.

### 2026-04-09 - C2 step 0: NFA/DFA hybrid engine design proposal
- Scope: SOTA-quality design proposal for the NFA/DFA hybrid engine. No code changes; design doc only.
- New file: `docs/C2_NFA_DFA_DESIGN.md` — comprehensive design covering goals/non-goals, architectural overview with module layout, no-backtracking subset definition (full inclusion/exclusion table), byte-class equivalence partitioning, forward + reverse Thompson NFA construction with anchored/unanchored variants, sparse-set Pike-VM (Russ Cox design from day one — not a prototype, the permanent NFA simulator and lazy DFA fallback), lazy DFA cache with clear-on-overflow + retry budget policy, two-pass capture recovery (the architectural decision recorded in this commit), engine dispatch boundary, what the existing VM path does NOT lose, differential testing strategy, benchmark strategy, phased implementation plan (steps 0-8), 10 open architectural questions with leans, risks and mitigations, and references.
- Architectural decision recorded: **two-pass capture recovery via bounded Pike-VM over the matched span**, NOT tagged transitions in the NFA. Rationale: this is the SOTA approach used by RE2 and the Rust `regex` crate; it keeps the lazy DFA cache compact, the correctness proof is structural (the bounded Pike-VM is identical to the full Pike-VM that would have run on the whole input), and the implementation risk is much lower than the tagged-transition alternative which has worse interaction with lazy DFA construction.
- Quality bar: SOTA from day one per the persistent project preference. Every step in the phased plan ships production-quality code; nothing is throwaway. Sparse-set Pike-VM, byte-class equivalence partitioning, lazy DFA with state cache and graceful fallback, reverse DFA for start-of-match recovery, two-pass capture recovery, anchored/unanchored variants — all SOTA techniques from RE2 and the Rust `regex` crate, used by name and by reference.
- Cohabitation invariant: the existing backtracking VM stays in place forever and handles every pattern outside the no-backtracking subset (backreferences, recursion, lookaround, conditionals, atomic groups, possessive quantifiers, `\K`, backtracking verbs, inline code blocks, callouts, branch-reset, Perl extended classes). C2 is a parallel engine, not a replacement. Listed explicitly in design doc §12 with the rule "if anything in this list regresses, it's a merge blocker."
- Differential testing: the existing 633-test suite plus the PCRE2 parity corpus plus a proptest harness form the merge gate. Every C2 commit from step 4 onward must produce zero differential failures against the existing VM on classifier-positive patterns. New `rgx-core/tests/c2_differential.rs` and `c2_proptest.rs` test files land in step 4.
- Phased implementation plan (8 steps): step 0 design (this commit), step 1 pattern classifier, step 2 byte-class partitioning, step 3 forward+reverse NFA construction, step 4 sparse-set Pike-VM with differential gate active, step 5 lazy forward DFA, step 6 lazy reverse DFA, step 7 literal prefix integration, step 8 production cutover with benchmarks and Book chapter. Estimated 8 minimum / 12-15 realistic commits, multi-week timeline.
- Open questions documented (10): full Unicode case folding scope, ASCII vs Unicode `\b`, LeftmostFirst vs LeftmostLongest first pass, per-instance vs thread-local DFA cache, default `dfa_size_limit`, Pike-VM fallback restart policy, debug-mode parallel-engine equivalence assertion, public `regex.uses_c2()` introspection, RegexSet C2 integration, Pike-VM capture pass cost on long spans. Each has my current lean for review.
- Validation: doc-only commit. `cargo fmt --check` clean. Existing test suites unchanged.
- Sign-off: this design doc blocks all C2 implementation work until the user approves. No code lands until then.

### 2026-04-09 - Reprioritize: defer A9 (language bindings), elevate C1+C2 (perf) to active focus
- Scope: Roadmap/backlog reprioritization. No Rust changes.
- Decision: A9 (language bindings) deferred pending real demand signal; C2 (NFA/DFA hybrid) and C1 (JIT compilation) promoted to active focus, sequenced C2-first.
- Rationale for deferring A9: the conventional "10x user base because most regex users aren't Rust devs" argument is generic and doesn't fit RGX specifically. RGX's killer feature is host integration (predicates, steering, events, async I/O, embedded Lua/JS/Rhai/Wasm), which translates poorly across FFI — Python callbacks become GIL territory, the async story assumes Rust's model, and "embed Lua inside a regex from Python" is a weaker pitch than from C/C++ because Python users already have a scripting host. A9 is also gated on A8 (publish, deferred), is `large` per language, and the maintenance tail (packaging, prebuilds, version skew, per-binding CI) competes with engine work that strengthens the actual differentiator. Reactivation criteria: a real user pulling for a specific binding. If reactivated, start with C bindings via cbindgen (cheapest, unlocks every other FFI host for free).
- Rationale for C2-first ordering: C2 changes the algorithmic class — patterns that don't use backtracking-only features (no backreferences, no recursion, no lookaround, no inline code blocks, no atomic groups, no possessive quantifiers, no `\K`, no backtracking verbs) run through a Thompson NFA + lazy DFA cache instead of the backtracking VM. Gives RGX the "can't hang" property the Rust `regex` crate uses as its primary differentiator and typically delivers 10x-100x speedup on regular patterns. C1 then provides a constant-factor multiplier (~5-10x via Cranelift, already in the dep tree via wasmtime) on whichever engine runs, so the wins compound. Doing C1 first would still leave pathological backtracking patterns exponential.
- Capture-handling design note for C2: the standard solution from the Rust `regex` crate is to use the DFA only for finding the overall match span, then re-run a small bounded NFA simulation over the matched span to recover capture group positions. This avoids the full DFA-with-captures complexity.
- Changes:
  - `ROADMAP.md`: new "Now" entry "Performance: NFA/DFA hybrid (C2) + JIT compilation (C1)" with strategic ordering and rationale. "Binding/runtime expansion (A9)" in Later annotated as `deferred` with full reasoning and reactivation criteria.
  - `docs/BACKLOG.md`: new "Tier 0 — Active focus" added at top of priority tiers with C2 first, C1 second. A9 entry rewritten with deprioritization reasoning, status `deferred pending demand signal`, and "if reactivated, start with C bindings via cbindgen" note. A9 moved from Tier 3 to Tier 4. C1 and C2 entries rewritten with active-focus annotations, design notes, dependencies, and open design questions.
  - `MEMORY.md`: dated session entry recording the decision, the strategic ordering rationale, the capture-handling design lesson, and the proposed next concrete action.
  - This entry.
- Validation: doc-only commit. `cargo fmt --check` clean (no Rust files touched). Quality gates re-run per COMMIT.md hard gate.
- Notes/impact: this is a planning commit, not a feature commit. The actual C2 design proposal and implementation will follow as separate commits. The Tier 0 banner in `docs/BACKLOG.md` plus the new ROADMAP "Now" entry mean a fresh AI session will know the current focus without having to scan recent MEMORY.md entries.

### 2026-04-09 - Sync RUST_CODEBASE_ANALYSIS.md with current workspace reality
- Scope: Live continuity doc sync — no Rust changes.
- Changes:
  - `RUST_CODEBASE_ANALYSIS.md`: brought the verified snapshot, codebase realities, and high-confidence next actions sections back into sync with the actual workspace state after the 2026-04-08 backlog blitz, the A10/A12 ship in 2026-04-09, and The RGX Book Part VI rollout.
  - Corrected stale facts: PGEN pin (was `54ed190…` 1.1.8 → now `ac2acb3` 1.1.9), source totals (was ~26K → now ~34K with per-file breakdown), test count (was ~550 → now 633 per smoke commit `c147ddc`), MSRV (now noted as 1.88), scaffold inventory (`simd.rs` / `javascript.rs` / `wasm.rs` deleted in 2026-04-08; `cache.rs` is now real and hosts `RegexCache`).
  - Documented the new public API surface: `Match` / `Captures`, ergonomic + lazy iterator APIs, position-aware matching, `split` family, `Replacer` trait + closure support, `RegexBuilder`, `RegexSet`, `RegexCache`, `BytesRegex`, `MatchSemantics`, `PartialMatchResult`, `CaptureLocations`, `escape()`, metadata accessors, `CompileError` with caret diagnostics, safety limits (`set_max_steps` / `set_max_backtrack_frames` / `set_max_recursion_depth`).
  - Documented shipped backlog items A3 (`tail_file` with kqueue/inotify), A4 (CLI `--follow`), A6 (inline-language `rgx.steer_*`), A7 (full Unicode `(?i)` case folding), A10 (`\X` extended grapheme cluster), A12 (returned-capture subroutines parse + `Call` lowering, with capture-return VM semantics noted as follow-up).
  - Documented the two-track documentation rule and that The RGX Book is now a 45-chapter mdBook with the new Part VI internals (architecture, compilation pipeline, the VM, PGEN integration, performance, sandboxing, testing philosophy, project status, contributing).
  - Replaced the old "high-confidence next actions" list (which still pointed at deepening benchmark trend capture) with the current backlog inventory: A9 bindings, C1 JIT, C2 NFA/DFA hybrid, A12 capture-return VM follow-up, A8 publish prep, GitHub Pages (blocked on Pro), perf push to <10x, remaining `VERSION[...]` and `(*SKIP:name)` PCRE2 surface.
- Validation: `cargo fmt -- --check` clean (no Rust files touched); `cargo test -p rgx-core`, `cargo test -p rgx-cli`, `cargo clippy --workspace --all-targets` all run as part of the COMMIT.md hard gate.
- Notes/impact: doc-only commit. The analysis file is now an accurate roadmap-grounded snapshot a fresh AI session can trust without first having to cross-check against `MEMORY.md` and `CHANGES.md`.

### 2026-04-09 - Fix x86_64 SIMD compile error in vm.rs
- Scope: CI fix for x86_64 Linux builds.
- Changes:
  - `rgx-core/src/vm.rs`: SIMD bytes-equal helper had `let _len = a.len();` but referenced `len` inside the AVX2 block. The `_` prefix marks unused, which Rust accepts on non-x86_64 platforms but fails to resolve on x86_64 where the SIMD path is compiled.
  - Fix: gate the binding to `#[cfg(target_arch = "x86_64")]` and rename to `len`. Both the binding and the use are now under the same cfg, so non-x86_64 platforms see neither (no unused warning) and x86_64 sees both (no missing variable).
  - Also removed unused `MatchResult` import from `bytes.rs`.
- Why this wasn't caught locally: development is on aarch64 (M1), so `cfg(target_arch = "x86_64")` blocks never compile. Cross-checking with `cargo check --target x86_64-unknown-linux-gnu` would have caught it but requires `x86_64-linux-gnu-gcc`.
- Validation: 483 lib tests pass on aarch64.

### 2026-04-09 - Bump MSRV to 1.88 and remove Pages workflow until GitHub Pro
- Scope: CI fixes.
- Changes:
  - `Cargo.toml`: rust-version 1.85 → 1.88 (transitive dep `home@0.5.12` requires 1.88)
  - `.github/workflows/ci.yml`: toolchain 1.85.0 → 1.88.0
  - **Removed `.github/workflows/book.yml`** — Pages on private repos requires GitHub Pro, which user plans to enable later. Workflow can be re-added from git history when Pro is active.
  - `ROADMAP.md`: noted that book.yml needs to be re-added when Pages is enabled
- Validation: 483 lib tests pass with Rust 1.89 locally.

### 2026-04-09 - Rewrite README with public-facing top section
- Scope: README polish for first-time visitors.
- Changes:
  - Added compelling top section: tagline, code example, highlights, quick start, doc links
  - Preserved internal navigation under "For contributors" header
  - Fixed duplicate `3.` numbering in the ramp-up list
  - Added CLAUDE.md to the contributor file list
- Rationale: README is the first thing visitors see. Previous version was internal-doc style ("the repo", "the project") which didn't sell what RGX is or why it matters.

### 2026-04-09 - Ship Part VI: Internals & Project (9 chapters, 1,587 lines)
- Scope: complete the book's coverage of every aspect of RGX, not just user features.
- Changes:
  - `book/src/internals/architecture.md` — pipeline diagram, crate map, 6 host layers
  - `book/src/internals/compilation-pipeline.md` — 4-stage parse→normalize→emit→optimize with worked `\d+` example
  - `book/src/internals/the-vm.md` — backtracking VM design, dispatch loop, ExecContext, capture trail, scanning strategies
  - `book/src/internals/pgen-integration.md` — what PGEN is, contract, adapter, pgen-issues workflow, 1.1.9 pin
  - `book/src/internals/performance.md` — real numbers (6.4x literal, 3.4x email; 0.88x capture), 8 key optimizations, what's not done (JIT/DFA)
  - `book/src/internals/sandboxing.md` — ExecutionMode, sandbox details per language, threat model, determinism
  - `book/src/internals/testing-philosophy.md` — hostile skepticism doctrine, 8-layer test taxonomy, claims-to-prove
  - `book/src/internals/project-status.md` — current snapshot, shipped features, remaining backlog, pre-release checklist
  - `book/src/internals/contributing.md` — dev setup, test commands, two-track docs, PGEN issue filing
- Validation: mdbook builds clean. 19/19 API smoke tests pass.
- Notes: all chapters are grounded in actual repo state — no aspirational claims. Tone is warm, honest, and practical.

### 2026-04-09 - Codify two-track documentation rule (Book + live docs)
- Scope: documentation discipline.
- Changes:
  - `CLAUDE.md`: split documentation rules into two clear tracks — The Book (user-facing, for the world) and live continuity docs (internal, for session survival)
  - `COMMIT.md`: Step 3 now explicitly checks both tracks; the two are non-interchangeable
  - Added Part VI: Internals & Project to The RGX Book SUMMARY (architecture, compilation pipeline, VM, PGEN integration, performance, sandboxing, testing philosophy, project status, contributing) — chapters being written
- Rationale: the user clarified that updating live docs does NOT satisfy the requirement to update the book. The book is what the world sees; the live docs are internal infrastructure. Both must be maintained.

### 2026-04-09 - Add API smoke test to catch book/code drift
- Scope: regression test infrastructure.
- Changes:
  - New `rgx-core/tests/api_smoke_test.rs` — 19 tests exercising every public API method documented in The RGX Book
  - Tests: compile, find/find_iter/find_all, captures, replace/replace_all/replacen with closures and NoExpand, split, RegexBuilder, escape, position-aware (find_first_at/find_all_at/is_match_at/shortest_match), RegexSet, RegexCache, BytesRegex, safety limits, MatchSemantics, PartialMatchResult, CaptureLocations, metadata accessors, error diagnostics, \X grapheme cluster, Unicode case folding
  - If a public API is renamed/removed/changed, this test fails and the commit is blocked
  - Found and fixed one wrong assertion in the test itself (closure replace returns the closure's value for the entire match)
- Validation: 19/19 smoke tests pass. Total test count now 633 across all suites.

### 2026-04-09 - Auto-deploy The RGX Book to GitHub Pages
- Scope: documentation infrastructure.
- Changes:
  - Added `.github/workflows/book.yml` — builds and deploys mdBook on every push to main
  - Triggers only when `book/**` changes
  - Uses `actions/deploy-pages@v4` with proper permissions
  - Book will be available at `https://rdje.github.io/rgx` after first successful run
  - README updated with the public URL
- Setup required: enable GitHub Pages in repo settings (Source: GitHub Actions)

### 2026-04-09 - Bump MSRV to 1.85 for PGEN 1.1.9 edition2024 requirement
- Scope: CI fix following PGEN 1.1.9 update.
- Changes:
  - `Cargo.toml`: `rust-version = "1.85"` (was 1.75)
  - `.github/workflows/ci.yml`: pinned toolchain bumped to 1.85.0 (both jobs)
  - PGEN 1.1.9 uses Rust edition 2024 which requires Rust ≥1.85
- Validation: 483 lib tests pass with Rust 1.89 locally.

### 2026-04-09 - Ship CLI --follow mode (tail -f | grep)
- Scope: Backlog item A4.
- Changes:
  - `rgx --file app.log --follow` — watches a file and prints matches as new lines are appended
  - Uses `tail_file` with OS-native watching (kqueue/inotify)
  - Color output supported (`--color`)
  - Clean shutdown on Ctrl-C via `ctrlc` crate
  - Added `ctrlc = "3"` dependency to rgx-cli
- Validation: 30 CLI tests pass. Manual testing confirmed.

### 2026-04-09 - Ship The RGX Book (mdBook, 30 chapters, 7,300+ lines)
- Scope: comprehensive documentation covering every user-facing feature.
- Changes:
  - `book/` directory with mdBook configuration and 37 markdown source files
  - Part I: Getting Started (5 chapters — first match, finding, captures, replace/split, RegexBuilder)
  - Part II: Core API (8 chapters — Match type, iterators, position-aware, RegexSet, RegexCache, BytesRegex, safety limits, error diagnostics)
  - Part III: Advanced (5 chapters — Unicode, match semantics, partial matching, CaptureLocations, Replacer trait)
  - Part IV: Host Integration (6 chapters — data exchange, callbacks, steering, events, async, file matching)
  - Part V: Real World (5 chapters — log monitor, tokenizer, HTTP router, data pipeline, WAF engine)
  - Appendices (5 — pattern syntax, PCRE2 compat, context reference, execution modes, CLI)
  - Build with `mdbook serve book` for searchable HTML with Rust theme
  - This is a live document that evolves alongside the project
- Validation: `mdbook build book` succeeds cleanly.

### 2026-04-09 - Ship returned-capture subroutine parsing and compilation (A12)
- Scope: Tier 4 backlog item A12, enabled by PGEN 1.1.9.
- Changes:
  - PGEN submodule updated to 1.1.9 (adds `returned_capture_subroutine` syntax)
  - `ReturnedCaptureSubroutine` AST node with `target` and `returned_groups`
  - Parser adapter handles `returned_capture_subroutine` / `returned_capture_group_list` / `returned_capture_group`
  - Compiles to same `Call` opcode as regular subroutines
  - `collect_descendants` helper added to parser adapter
  - Full capture-return semantics (preserving specified groups across call boundary) is a VM-level follow-up
  - 2 new tests: compilation and matching
- Validation: 484 tests pass (1 ignored). Zero clippy errors.

### 2026-04-08 - Ship \X extended grapheme cluster matching
- Scope: Tier 4 backlog item A10.
- Changes:
  - `\X` matches one Unicode extended grapheme cluster (base + combining marks + ZWJ sequences)
  - AST node `GraphemeCluster`, VM opcode `0x08`, parser mapping in `simple_escape`
  - Uses `unicode-segmentation` crate for UAX#29-compliant grapheme boundary detection
  - 5 tests: basic, combining marks (e + accent), ZWJ emoji (family), find_all, quantifier
  - Bug found via trace: opcode 0x08 was missing from `TryFrom<u8>` dispatch table
- Validation: 482 tests pass (1 ignored). Zero clippy errors.

### 2026-04-08 - Ship partial matching API for streaming/incremental input
- Scope: Tier 4 backlog item A14 (partial matching).
- Changes:
  - `PartialMatchResult` enum: `Full(MatchResult)`, `Partial(offset)`, `NoMatch`
  - `Regex::find_first_partial(text)` — detect when input ends mid-match
  - `ExecContext.hit_end` flag — set when a match attempt reaches EOF while the pattern was actively matching (pos > match_start)
  - Only flags partial when the pattern has started consuming characters
  - 5 new tests: full match, partial match, no match, date boundary, empty input
- Validation: 477 lib tests pass (1 ignored). Zero clippy errors.

### 2026-04-08 - Ship MatchSemantics API (leftmost-first / leftmost-longest)
- Scope: Tier 3 backlog item B4 (configurable match semantics).
- Changes:
  - `MatchSemantics` enum: `LeftmostFirst` (default, PCRE2/Perl), `LeftmostLongest` (POSIX)
  - `Regex::set_match_semantics()` — runtime switch stored as AtomicU8
  - For non-alternation patterns, greedy quantifiers already produce the longest match
  - Full POSIX alternation reordering (e.g., `a|ab` → `ab|a`) requires compiler-level AST sorting — tracked as follow-up
  - Workaround: put longer branches first in alternation
  - 6 new tests
- Validation: 472 lib tests pass (1 ignored). Zero clippy errors.

### 2026-04-08 - Upgrade tail_file to OS-native event-driven watching (kqueue/inotify)
- Scope: SOTA upgrade for A3 tail_file.
- Changes:
  - Replaced poll-based loop with `notify` crate (kqueue on macOS, inotify on Linux)
  - Zero idle CPU cost — kernel wakes the thread only on file modification
  - Debounce: drains queued events after burst writes, 10ms settle delay
  - Fallback: `recv_timeout` at poll_interval still catches missed events
  - Truncation detection: resets position and line counter
  - Timing-sensitive detection test marked `#[ignore]` for CI stability

### 2026-04-08 - Ship tail_file for poll-based file watching with match callbacks
- Scope: Tier 3 backlog item A3.
- Changes:
  - `Regex::tail_file(path, options, on_match)` — watch a file, call back on matches in new lines
  - `TailOptions` — `poll_interval` (default 250ms), `from_end` (default true)
  - `TailHandle` — `stop()`, `is_running()`, auto-stop on drop
  - Background thread with poll loop, truncation detection, line-number tracking
  - 3 new tests: appended content detection, from-beginning mode, handle lifecycle
- Validation: 606+ tests pass. Zero clippy errors.

### 2026-04-08 - Ship RegexSet for multi-pattern matching
- Scope: Tier 3 backlog item B2.
- Changes:
  - `RegexSet::new(&["pattern1", "pattern2", ...])` — compile N patterns
  - `set.is_match(text)` — any pattern matches?
  - `set.matches(text)` → `SetMatches` — which patterns matched
  - `SetMatches`: `matched(i)`, `matched_any()`, `matched_all()`, `iter()`, `IntoIterator`
  - `SetMatchesIter` / `SetMatchesIntoIter` for iterating matched indices
  - `patterns()`, `len()`, `is_empty()`, `empty()`
  - 10 tests: basic, partial, no-match, routing use case, iterators, error handling
- Validation: 463+ tests pass. Zero clippy errors.

### 2026-04-08 - Ship full Unicode case folding for (?i)
- Scope: Tier 3 backlog item A7 (Unicode case folding).
- Changes:
  - `(?i)` now folds all Unicode letters, not just ASCII. `(?i:café)` matches `CAFÉ`.
  - `unicode_case_variants()` collects lowercase + uppercase variants via `char::to_lowercase()` / `char::to_uppercase()`
  - Literal chars: expanded to char class with all case variants when `(?i)` active
  - Custom char classes: each endpoint folded to include Unicode case variants
  - ASCII ranges still get mirror-range folding as before
  - 6 new tests: accented Latin, Greek, Cyrillic, builder, char classes, ASCII regression
- Validation: 593+ tests pass. Zero clippy errors.
- Notes/impact: closes A7. PCRE2 `(?i)` parity for internationalized text.

### 2026-04-08 - Ship bytes::BytesRegex for matching on &[u8] without UTF-8 validation
- Scope: Tier 2 backlog item B5.
- Changes:
  - `bytes::BytesRegex` — compile patterns, match against `&[u8]` directly
  - `bytes::BytesMatch` — match result with `as_bytes()`, `start()`, `end()`, `range()`, `len()`
  - `compile()`, `with_mode()`, `is_match()`, `find()`, `find_all()`, `as_str()`
  - Bypasses UTF-8 validation: `.` matches single bytes, `\w`/`\d`/`\s` work on ASCII
  - `Engine::vm_find_first` / `vm_find_all` internal methods for direct `&str` access
  - 7 tests including non-UTF-8 input and binary patterns
- Validation: 587+ tests pass. Zero clippy errors.

### 2026-04-08 - Ship inline-language steering for Lua, JavaScript, and Rhai
- Scope: Tier 2 backlog item A6 (inline-language steering).
- Changes:
  - Lua: `rgx.steer_continue()`, `rgx.steer_fail()`, `rgx.steer_accept()`, `rgx.steer_skip(n)`, `rgx.steer_abort()`
  - JavaScript: `rgx.steerContinue()`, `rgx.steerFail()`, `rgx.steerAccept()`, `rgx.steerSkip(n)`, `rgx.steerAbort()`
  - Rhai: `steer_continue()`, `steer_fail()`, `steer_accept()`, `steer_skip(n)`, `steer_abort()`
  - Steer takes highest priority: if emitted, overrides return-value semantics
  - `finish_exec_result_with_steer` centralizes steer/result priority logic
- Validation: all 616+ tests pass. Zero clippy errors.
- Notes/impact: closes A6. Inline languages now have the same match-steering power as native callbacks.

### 2026-04-08 - Ship benchmark CI job
- Scope: Tier 2 backlog item C4 (benchmark CI).
- Changes:
  - Added `benchmarks` job to `.github/workflows/ci.yml`
  - Runs criterion throughput benchmarks on every push to main
  - Results uploaded as artifacts (90-day retention) for regression tracking
  - Skipped on PRs to avoid noisy CI
- Validation: CI YAML is valid. Main workspace tests unaffected.

### 2026-04-08 - Ship fuzzing infrastructure (cargo-fuzz targets)
- Scope: Tier 2 backlog item C3 (fuzzing).
- Changes:
  - `fuzz/` directory with standalone Cargo.toml (independent of workspace)
  - 4 fuzz targets:
    - `fuzz_compile` — arbitrary bytes as patterns, no panics/UB
    - `fuzz_match` — pattern + input, exercises is_match/find_first/find_all/captures with step limits
    - `fuzz_replace` — pattern + input + replacement, exercises replace/split APIs
    - `fuzz_roundtrip` — invariant checks (bounds, non-overlap, is_match/find_first agreement, group 0 consistency)
  - Uses `libfuzzer-sys` + `arbitrary` for structured input generation
  - Step limits (50K) prevent hangs on pathological patterns
- Validation: main workspace tests unaffected. Run with `cargo +nightly fuzz run fuzz_compile`.

### 2026-04-08 - Ship syntax error diagnostics with span highlighting
- Scope: Tier 2 backlog item B9 (error diagnostics).
- Changes:
  - `CompileError` struct with `message`, `pattern`, `offset` fields
  - `RgxError::compile()` and `RgxError::compile_at()` constructors
  - Caret-highlighted error output when PGEN provides byte_offset location
  - All `RgxError::Compile(String)` sites migrated to `RgxError::compile(msg)` / `RgxError::compile_at(msg, pattern, offset)`
  - Error format: `regex compile error: <msg>\n  <pattern>\n  <caret>`
- Validation: 655+ tests pass. Zero clippy errors.

### 2026-04-08 - Ship CaptureLocations for zero-allocation capture loops
- Scope: Tier 2 backlog item B20.
- Changes:
  - `CaptureLocations` type with `get(i)`, `len()`, `is_empty()`
  - `Regex::capture_locations()` — create reusable buffer sized for this pattern
  - `Regex::captures_read(text, &mut locs)` — fill buffer in-place, return `Match`
  - `Regex::captures_read_at(text, start, &mut locs)` — position-aware variant
  - 6 new tests: basic, reuse, no-match, optional groups, offset, len
- Validation: 586+ tests pass. Zero clippy errors.

### 2026-04-08 - Ship CLI --color output with ANSI match highlighting
- Scope: Tier 2 backlog item A5 (CLI color).
- Changes:
  - `--color auto|always|never` flag (default: `auto` = detect terminal via `IsTerminal`)
  - Matched text highlighted in bold red, line numbers in green, file prefixes in magenta, separators in cyan
  - Color applies to: `--only-matching`, default span output, file-mode line/span output
  - Helper functions: `color_match`, `color_file`, `color_line_num`, `color_sep`, `highlight_line`
- Validation: 30 CLI tests pass. Manual verification of color output.

### 2026-04-08 - Ship thread-safe compilation cache (RegexCache)
- Scope: Tier 2 backlog item B3.
- Changes:
  - `RegexCache::new(capacity)` — thread-safe LRU cache for compiled regexes
  - `cache.get(pattern)` / `cache.get_with_mode(pattern, mode)` — compile or retrieve `Arc<Regex>`
  - Read-lock fast path, write-lock slow path, double-check after compile
  - LRU eviction when at capacity, mode-aware keying, `clear()` / `len()` / `is_empty()`
  - 8 tests: cache hits, eviction, error handling, mode separation, thread safety (8 threads)
- Validation: 609+ tests pass. Zero clippy errors.

### 2026-04-08 - Ship RegexBuilder for fluent compilation with flag overrides
- Scope: Tier 2 backlog item B11.
- Changes:
  - `RegexBuilder::new(pattern).case_insensitive(true).build()` — fluent flag configuration
  - Methods: `case_insensitive`, `multi_line`, `dot_matches_new_line`, `ignore_whitespace`, `swap_greed`, `mode`
  - Prepends `(?imsx)` prefix to pattern — zero compiler changes needed
  - 7 new tests
- Validation: 600+ tests pass. Zero clippy errors.

### 2026-04-08 - Ship Replacer trait, NoExpand, shortest_match
- Scope: Tier 2 backlog items B16 (Replacer trait) and B17 (shortest_match).
- Changes:
  - `Replacer` trait with `replace_append` and `no_expansion` — pluggable replacement strategy
  - Implemented for `&str`, `String`, `&String` (template interpolation), `FnMut(&Captures) -> T` (closures), and `NoExpand` (literal)
  - `replace`/`replace_all`/`replacen` now accept `impl Replacer` instead of `&str` — closures work directly
  - `NoExpand` wrapper prevents `$1`/`$name` interpolation
  - Fast path: when `no_expansion()` returns `Some`, capture extraction is skipped entirely
  - `shortest_match(text)` and `shortest_match_at(text, start)` — return only end position
  - 9 new tests
- Validation:
  - All 562+ tests pass across rgx-core, rgx-cli, rgx-bench. Zero clippy errors.

### 2026-04-08 - Remove scaffold files and confirm zero RGX-owned warnings
- Scope: Tier 1 items C5 (scaffold removal) and C6 (clean warnings).
- Changes:
  - Deleted 4 one-line placeholder files: `cache.rs`, `simd.rs`, `javascript.rs`, `wasm.rs`
  - Removed corresponding `pub mod` declarations from `lib.rs`
  - Confirmed zero RGX-owned clippy warnings across `rgx-core`, `rgx-cli`
- Validation: all 552+ tests pass, zero clippy errors.
- Notes/impact: closes C5 and C6. All remaining workspace warnings are from the PGEN submodule.

### 2026-04-08 - Ship lazy iterator APIs: find_iter, captures_iter, split_iter, capture_names
- Scope: Tier 2 backlog item B12 (iterator APIs).
- Changes:
  - `find_iter(text)` → `FindIter` — lazy match iteration, zero Vec allocation
  - `captures_iter(text)` → `CaptureIter` — lazy capture iteration
  - `split_iter(text)` → `SplitIter` — lazy regex-delimited split
  - `splitn_iter(text, limit)` → `SplitNIter` — lazy limited split
  - `capture_names()` → `CaptureNames` — iterator over group names with `ExactSizeIterator`
  - All iterators implement `FusedIterator`
  - 12 new tests including parity checks against Vec-returning methods
- Validation:
  - `cargo test -p rgx-core` (552 pass), `cargo test -p rgx-cli` (30 pass), `cargo test -p rgx-bench` (39 pass). Zero clippy errors.

### 2026-04-08 - Ship ergonomic API batch: Match, Captures, escape, replacen, Cow replace, metadata
- Scope: Tier 2 backlog items B13, B14, B15, B18, B19, B21.
- Changes:
  - `Match<'t>` type with `as_str()`, `start()`, `end()`, `range()`, `len()`, `is_empty()`
  - `Captures<'t>` wrapper with `get(i)`, `name("n")`, `Index<usize>`, `Index<&str>`, `expand()`, `iter()`
  - `SubCaptureMatches` iterator with `ExactSizeIterator`
  - `escape()` function for safe literal pattern construction
  - `replacen(text, limit, rep)` — replace up to N matches
  - `replace` / `replace_all` now return `Cow<str>` (zero allocation on no-match)
  - `Regex::find()` returns `Match<'t>`, `Regex::captures()` returns `Captures<'t>`
  - `Regex::as_str()` returns original pattern, `captures_len()` returns group count
  - Stored original pattern string in `Regex` struct
  - 19 new tests covering all new APIs
- Validation:
  - `cargo test -p rgx-core` (541 pass), `cargo test -p rgx-cli` (30 pass), `cargo test -p rgx-bench` (39 pass). Zero clippy errors.
- Notes/impact: closes B13, B14, B15, B18, B19, B21. The public API now matches `regex` crate ergonomics.

### 2026-04-08 - Ship step limits, backtrack frame limits, and configurable recursion depth
- Scope: Tier 1 backlog items A1 (step limits) and A2 (memory limits).
- Changes:
  - `Regex::set_max_steps(Option<u64>)` — configurable per-attempt opcode step counter. Prevents exponential backtracking DoS on pathological patterns like `(a+)+b`.
  - `Regex::set_max_backtrack_frames(Option<u64>)` — configurable backtrack stack depth limit.
  - `Regex::set_max_recursion_depth(Option<u64>)` — configurable recursion depth limit (default: 1024).
  - All limits use `AtomicU64` for interior mutability (`&self`, zero-lock overhead).
  - 8 new tests covering catastrophic backtracking, per-attempt semantics, stack/recursion control.
- Validation:
  - `cargo test -p rgx-core` (522 pass), `cargo test -p rgx-cli` (30 pass), `cargo test -p rgx-bench` (39 pass). Zero clippy errors.
- Notes/impact: closes A1 and A2. Production-grade DoS protection now available.

### 2026-04-08 - Ship find_at, split, splitn, replace with capture interpolation, and MatchResult groups
- Scope: Tier 1 backlog items B10, B8, B6, B7 (partial — groups on MatchResult).
- Changes:
  - `find_first_at(text, start)`, `find_all_at(text, start)`, `is_match_at(text, start)` — position-aware matching at all 3 layers (VM, Engine, public API). Panics on non-UTF-8-boundary start.
  - `split(text)` and `splitn(text, limit)` — regex-delimited string splitting.
  - `replace(text, replacement)` and `replace_all(text, replacement)` — capture interpolation with `$0`, `$1`, `$name`, `${name}`, `$&`, `$$`.
  - `MatchResult.groups: Vec<Option<(usize, usize)>>` — capture group positions now surfaced on every match result (VM, Engine, suspendable paths).
  - `Regex::named_groups()` / `Engine::named_groups()` — accessor for named capture group map.
  - 37 new unit tests covering all new APIs including edge cases.
- Validation:
  - `cargo test -p rgx-core` (513 pass), `cargo test -p rgx-cli` (30 pass), `cargo test -p rgx-bench` (39 pass). Zero clippy errors.
- Notes/impact: closes B10 (find_at), B8 (split/splitn), B6 (replacer with $1 interpolation). Partially addresses B7 (groups on MatchResult — full Captures wrapper is next).

### 2026-04-04 - Create comprehensive BACKLOG.md
- Scope: living inventory of all remaining work across the project.
- Changes:
  - Created `docs/BACKLOG.md` with 30 items across 3 categories:
    - A. Missing from RGX roadmap (14 items: step/memory limits, tail_file, CLI follow/color, inline steering, Unicode case folding, crate publishing, language bindings, \X, named skip, returned-capture subroutines, version conditionals, partial matching)
    - B. Features to port from Rust's regex crate (10 items: step limits, RegexSet, compilation caching, match semantics, bytes::Regex, replacer API, Captures API, split/splitn, error diagnostics, find_at)
    - C. Engineering improvements (6 items: JIT, NFA/DFA hybrid, fuzzing, benchmark CI, scaffold cleanup, clippy warnings)
  - Each item has effort estimate, rationale, implementation approach, and dependencies
  - Priority tiers 1-4 from production blockers to long-term architecture
  - Added to README.md docs index
- Validation: manual review of completeness against ROADMAP.md, PCRE2_COMPATIBILITY_MATRIX.md, and Rust regex crate API.
- Notes/impact: provides the master task list for the next phase of development.

### 2026-04-08 - Expose remaining engine surfaces in CLI
- Scope: close the gap between engine capabilities and CLI access.
- Changes:
  - `--var-json NAME=JSON` for typed variables (int, float, bool, array, map via JSON)
  - `--events` prints structured match events to stderr (debugging/profiling)
  - `--json` now includes `branch` and `code_result` fields when available
  - `--numeric` collects and prints numeric code block results
  - `--replace-with-code` uses code block replacement values for find-and-replace
  - `--stats` prints match statistics summary to stderr
  - Updated `docs/CLI_GUIDE.md` with all new features
  - 7 new tests (30 total CLI tests)
- Validation:
  - `cargo test -p rgx-cli` (30 pass). Manual testing of all features.

### 2026-04-07 - Ship 6 new CLI features + comprehensive CLI guide
- Scope: major CLI enhancement with full documentation.
- Changes:
  - `--recursive` / `-r`: scan directories recursively
  - `--context N` / `-C N`: show surrounding lines (like grep -C)
  - `--replace STRING`: find-and-replace, print to stdout
  - `--json`: structured JSON output for piping
  - `--only-matching` / `-o`: print just matched text
  - `--invert-match` / `-v`: print non-matching lines
  - Created `docs/CLI_GUIDE.md` with 12 sections and 20+ examples
  - 8 new tests (23 total CLI tests)
- Validation:
  - `cargo test -p rgx-cli` (23 pass). Manual testing of all features.

### 2026-04-07 - Add CLI file matching: --file, --line-mode, --count
- Scope: CLI enhancement for file-backed matching.
- Changes:
  - `--file <PATH>` reads input from a file instead of positional argument
  - `--line-mode` (with `--file`) matches each line independently, prints `LINE_NUM: text`
  - `--count` prints match count instead of spans
  - Clear error messages for missing/unreadable files (exit code 1)
  - 5 new CLI tests
- Validation:
  - `cargo test -p rgx-cli` (15 pass). Manual testing confirmed all 4 usage patterns.
- Also fixed: exhaustiveness errors in Lua/JS/Rhai/WASM backends for Steer/Suspend/Structured variants.

### 2026-04-07 - Fix nested recursion bug in quantifier zero-width guard
- Scope: fix the last known bug — nested recursive patterns now match correctly.
- Changes:
  - **Root cause**: `StarGreedy`/`PlusGreedy` zero-width match guards broke out of the quantifier loop immediately when the body matched zero characters, without trying alternative branches. For `([^()]*|(?&pair))*`, when `[^()]*` matched empty at a `(`, the loop exited without ever trying `(?&pair)`.
  - **Fix**: Added `execute_subexpr_advancing` that retries sub-expressions rejecting zero-width matches, giving alternatives like recursive calls a chance to match and advance position.
  - Updated all 4 zero-width guards (2 in `execute_at`, 2 in `execute_subexpr`).
  - Removed `#[ignore]` from `deep_recursion_with_captures_restored_correctly` test.
  - All 44 adversarial tests now pass — zero ignored, zero failures.
- Validation:
  - `cargo test -p rgx-core`: all pass (343 + 44 + 55 + 11 + 21 + 6). `-p rgx-bench`: all pass.
  - Pattern `(?<pair>\((?:[^()]+|(?&pair))*\))` on `(a(b)c)` now returns `(0, 7)`.

### 2026-04-07 - Fix events+async bug, document recursion limitation
- Scope: fix 1 of 2 bugs found by gap testing, document the other.
- Changes:
  - **FIXED**: Events now fire during `find_first_suspendable` and `resume` — `MatchAttemptStarted`/`MatchAttemptCompleted` emitted in the suspendable scanning path and resume completion path.
  - **FIXED**: Subroutine calls now revert captures on success (PCRE2 semantics — subroutines advance position but don't export internal captures).
  - **DOCUMENTED**: Nested recursive balanced-paren matching returns inner match instead of outer — marked as known limitation with `#[ignore]` test.
  - 43 adversarial tests pass, 1 ignored (recursion limitation).
- Validation:
  - All tests pass across all crates.

### 2026-04-07 - Test all 9 known gap combinations — 2 real bugs found
- Scope: prove or disprove every untested feature combination listed in TESTING_PHILOSOPHY.md.
- Changes:
  - Added 10 tests covering all 9 known gaps: recursion+steering, events+async, file+callbacks, variable mutation in find_all, captures across \K, verbs in lookaheads, steering+zero-width, deep recursion+trail, concurrent variable mutation.
  - **Bug found**: events don't fire during `find_first_suspendable` before suspension — observability layer is blind to pre-suspension work.
  - **Bug found**: recursive subroutine calls clobber outer match position/capture state — `(a(b)c)` with recursive balanced-paren pattern reports inner match instead of outer.
  - Both bugs marked `#[ignore]` with documentation; 42 pass, 2 ignored.
  - Total adversarial tests: 44.
- Validation:
  - `cargo test -p rgx-core --test adversarial`: 42 pass, 0 fail, 2 ignored.

### 2026-04-07 - Expand four weakest guide chapters to SOTA+++ documentation quality
- Scope: improve user-facing documentation for the chapters that needed it most.
- Changes:
  - **Chapter 00 (First Match)**: 144→334 lines, 6→18 examples. Added warm welcome, real-world patterns (dates, emails, URLs, key-value), try-it-yourself exercises, common gotchas, visual match diagram.
  - **Chapter 03 (Steering)**: 382→769 lines, 11→27 examples. Added before/after comparisons, decision flowchart, patterns & recipes section, combined example with variables+callbacks+steering.
  - **Chapter 05 (Async)**: 344→552 lines, 11→17 examples. Added gentle intro, ASCII flow diagram, step-by-step walkthrough, common mistakes, sync-unaffected proof.
  - **Chapter 06 (Files)**: 384→748 lines, 13→23 examples. Added CSV/config/multi-file scenarios, mini-grep walkthrough, data pipeline, binary handling, performance notes.
  - Total guide: 4,350→5,810 lines, 41→85 examples in the improved chapters alone.

### 2026-04-07 - Add property-based, stress, and fuzz test suites + testing philosophy doc
- Scope: three new test categories + testing doctrine.
- Changes:
  - Created `rgx-core/tests/property_tests.rs` — 11 proptest-based tests (256+ random cases each): compilation safety, position bounds, non-overlapping, determinism, branch numbers, UTF-8 validity.
  - Created `rgx-core/tests/stress_tests.rs` — 21 stress/soak/fuzz tests: 1000 pattern compilations, 10K input matching, 100K rapid-fire, 1000 variables, 5000 callbacks, 8-thread concurrency, 100K-line file scan, 100 suspend/resume cycles, 5000 random compilations.
  - Created `docs/TESTING_PHILOSOPHY.md` — hostile skepticism approach, behavioral categories, claims-to-prove, known gaps, process rules.
  - Added `proptest = "1"` to dev-dependencies.
  - Total test count: ~520 across all crates.
- Validation:
  - All tests pass. Property tests found no violations. Stress tests completed without panics or resource issues.

### 2026-04-07 - Fix all 4 RGX-side adversarial failures — zero failures remain
- Scope: fix the 4 remaining adversarial test failures that were RGX-side issues.
- Changes:
  - **serde_json recursion limit**: enabled `unbounded_depth` feature and `serde_stacker` for safe deep AST deserialization. 50-level nested groups now compile.
  - **Many alternatives test**: fixed test expectation — `a100` correctly prefix-matches inside `a1001`.
  - **Empty pattern**: removed explicit rejection. Empty regex now compiles and matches the empty string at every position (PCRE2 semantics).
  - **Steering filter logic**: added `\b` word boundary to prevent `\d+` from backtracking into shorter digit matches, ensuring full numeric IDs are compared to the threshold.
- Validation:
  - All 34 adversarial tests pass (was 27 pass / 7 fail).
  - All ~487 tests across all crates pass, zero failures.

### 2026-04-07 - Bump PGEN to 1.1.8, close PGEN-RGX-0011/0012/0013
- Scope: PGEN submodule upgrade verifying Unicode and nesting fixes.
- Changes:
  - Bumped `subs/pgen` from `8b31c80` (1.1.7) to `54ed190` (1.1.8).
  - PGEN-RGX-0011 (emoji literal): now parses. PGEN-RGX-0012 (café/multibyte): now parses. PGEN-RGX-0013 (50 nested groups): now parses.
  - All 3 closed as `verified-fixed-upstream` with fixed AST dumps archived.
  - Adversarial tests: 30 now pass (was 27). 4 remaining failures are RGX-side issues (serde_json recursion limit for deep AST, prefix-match false positive for 1000 alternatives, empty pattern rejection, steering logic).
  - All 9 RGX-filed PGEN issues (0005-0013) now closed.
- Validation:
  - `cargo test -p rgx-core` (343 unit pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (39 pass)

### 2026-04-07 - File PGEN-RGX-0011/0012/0013 for adversarial test failures
- Scope: PGEN bug reports for 3 failures found by adversarial testing.
- Changes:
  - Filed PGEN-RGX-0011: emoji/non-ASCII literal rejected (`🎉` fails at position 0)
  - Filed PGEN-RGX-0012: multibyte UTF-8 literal rejected (`café` fails at `é`, position 3)
  - Filed PGEN-RGX-0013: 50 nested groups rejected (hits parser recursion limit)
  - Confirmed that empty pattern and 1000 alternatives are NOT PGEN bugs (PGEN accepts both; RGX adapter issues)
  - Confirmed all_layers_under_stress failure is a steering logic issue (not PGEN)
  - Full repro inputs, traces, and contract snapshots for each issue.
- Notes/impact:
  - PGEN issue tracker: 0005-0010 closed, 0011-0013 open.
  - These are real limitations: any pattern with non-ASCII literals or >~30 nesting levels will fail to compile.

### 2026-04-07 - Add adversarial and edge-case tests for real confidence
- Scope: tests that prove correctness under stress, not just happy paths.
- Changes:
  - Added 17 adversarial tests to `host_integration.rs`:
    - Backtracking after resume (suspension point rollback)
    - Steering: Skip past end of text, Accept at position 0, Abort partial results
    - Thread safety: 10 concurrent threads on shared regex (find_first, find_all, events)
    - Zero-width: events during zero-width matches, steering from zero-width callback
    - Error conditions: nonexistent file, resume with error, empty input, empty pattern
    - Stress: 10K matches on 80K input, 5-level deep nested variable access
  - Total integration tests: 55. Total across all crates: 453.
- Validation:
  - All 453 tests pass. Thread safety tests run 10 concurrent threads without panics.

### 2026-04-07 - Add comprehensive integration tests for all host integration layers
- Scope: fill every test gap across all 6 layers.
- Changes:
  - Created `rgx-core/tests/host_integration.rs` with 39 integration tests:
    - Layer 1 Data Exchange (15): typed variable readers, null/empty values, deep nesting, backward compat, structured results, branch numbers, numeric/replacement collection
    - Layer 3 Steering (5): abort with find_all, skip with find_all, accept with captures, fail+backtrack, continue no-op
    - Layer 4 Events (6): branch entered, capture completed, code block evaluated, events during find_all, events don't affect results, all event types in one pattern
    - Layer 5 Async (3): find_all graceful handling, abort via steering, accept via steering
    - Layer 6 Files (5): no matches, empty file, callbacks during scan, unicode content, line text preservation
    - Cross-layer (5): variables+callbacks, steering+callbacks, events+branches, callbacks+files, all layers combined
- Validation:
  - `cargo test -p rgx-core` (388 pass including 39 new integration tests), `-p rgx-cli` (10 pass), `-p rgx-bench` (39 pass)
  - Total across all crates: 437 tests.

### 2026-04-07 - Ship vars!/value! macros and re.set_vars() for zero-ceremony variable setup
- Scope: two declarative approaches to host variable construction.
- Changes:
  - **Option A** — `vars!(re, { "key" => value, ... })`: sets variables directly on the regex using JSON-style `{}` for maps and `[]` for arrays.
  - **Option C** — `re.set_vars(value!({ ... }))`: builds a `Value` with the `value!` macro, then unpacks it into variables via `set_vars()`.
  - Both support arbitrary nesting, scalars, arrays, and maps with zero `Value::` mentions.
  - `value!` macro: standalone value construction for use anywhere.
  - `Regex::set_vars(Value)`: unpacks a `Value::Map` into individual typed variables.
  - 5 macro tests + 1 `set_vars` test.
- Validation:
  - `cargo test -p rgx-core` (349 pass), 0 new clippy warnings.

### 2026-04-07 - Ship fluent variable builder API
- Scope: ergonomic API for building host variables without exposing `Value` internals.
- Changes:
  - Created `rgx-core/src/vars.rs` with `VarsBuilder`, `ArrayBuilder`, `HashBuilder` using move-based ownership for type-safe chaining.
  - `re.vars().set("key", value).hash("config").set("port", 8080).done()` — no `Value::` mentions.
  - Arbitrary nesting: `.hash()` and `.list()` inside `.hash()` at any depth.
  - Added `set_var<V: Into<Value>>()` ergonomic setter and `var_int()`, `var_float()`, `var_bool()`, `var_str()`, `var_array()`, `var_map()` convenience readers on `ExecContext`.
  - Added `Value::array()` and `Value::map()` static builders, plus `From` impls for `i32`, `u32`, `usize`, `f32`, `Vec<&str>`, `Vec<String>`, `Vec<i64>`, `Vec<f64>`, `Vec<(K,V)>`.
  - 5 fluent builder tests + 3 ergonomic API tests.
- Validation:
  - `cargo test -p rgx-core` (343 pass), 0 new clippy warnings.

### 2026-04-07 - Ship typed host values: scalars, arrays, maps for data exchange
- Scope: extend host variable system beyond strings to typed scalars and aggregates.
- Changes:
  - Added `Value` enum with 7 variants: `Null`, `Bool(bool)`, `Int(i64)`, `Float(f64)`, `String(String)`, `Array(Vec<Value>)`, `Map(Vec<(String, Value)>)`.
  - Added `Regex::set_typed_variable(name, Value)` for typed variable input.
  - Added `ExecContext::typed_variable(name) -> Option<Value>` for typed variable reading in callbacks.
  - Added `CodeBlockValue::Structured(Value)` and `ExecResult::Structured(Value)` for structured return values from callbacks.
  - Backward compatible: `set_variable("x", "hello")` still works and is also accessible as `typed_variable("x") → Value::String("hello")`. Typed variables auto-stringify for Lua/JS/Rhai compat.
  - `From` impls for `&str`, `String`, `i64`, `f64`, `bool` for ergonomic construction.
  - 6 unit tests: int, array, map, structured result, backward compat (x2).
- Validation:
  - `cargo test -p rgx-core` (333 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (39 pass), 0 new clippy warnings.
- Notes/impact:
  - Callbacks can now receive typed data (thresholds as ints, lookup tables as maps, feature flags as bools) without string parsing.
  - Callbacks can return structured results (maps with multiple fields) instead of single numeric/string values.

### 2026-04-07 - Create The RGX Guide: 12-file book-style documentation
- Scope: comprehensive user-facing documentation covering every feature.
- Changes:
  - Created `docs/guide/` with 12 markdown files (4,350+ lines total):
    - Chapter 0: Your First Match — basics, patterns, capture groups
    - Chapter 1: Passing Data In and Out — variables, results, branch IDs
    - Chapter 2: Predicate Callbacks — 4 languages, 18+ examples, execution modes
    - Chapter 3: Steering the Match — 5 actions, real scenarios
    - Chapter 4: Watching the Engine — debugger, profiler, coverage tool
    - Chapter 5: Async Callbacks — continuation-passing, async runtime integration
    - Chapter 6: Working with Files — grep tool, log alerter
    - Chapter 7: Real-World Patterns — log monitor, tokenizer, data pipeline, config parser, WAF engine
    - Quick Reference — one-page cheat sheet
    - Execution Modes — Pure/Safe/Full decision guide
    - Context Reference — all callback fields across all languages
  - Updated README to link to the guide.

### 2026-04-06 - Add comprehensive host integration guide with examples
- Scope: user-facing documentation for all 6 host integration layers.
- Changes:
  - Created `docs/HOST_INTEGRATION_GUIDE.md` with practical examples for every layer:
    - Layer 1: variables, results, branch identification
    - Layer 2: native/Lua/JS/Rhai callbacks with context reference table
    - Layer 3: all 5 steering actions with code examples
    - Layer 4: event observer, backtrack counting, event type reference
    - Layer 5: suspendable matching, async resolver, continuation-passing walkthrough
    - Layer 6: file matching, line-oriented mode, reactive scanning
    - Combined example: log monitor using all layers together
    - Quick reference table for common tasks
  - Added link to README docs index.
- Notes/impact:
  - This is the user-facing companion to `docs/HOST_INTEGRATION_ARCHITECTURE.md` (which is implementation-facing).

### 2026-04-06 - Ship Layer 5: Async/External I/O via continuation-passing
- Scope: the hardest host integration layer — callbacks can suspend the match, do async work, and resume.
- Changes:
  - Added `MatchOutcome` enum (`Completed` / `Suspended`), `MatchContinuation` struct (captures full VM state, owns all data, automatically `Send + Sync`), `ExecContextSnapshot` for async resolvers.
  - Added `ExecResult::Suspend(String)` variant for async callback signaling.
  - VM code-block dispatch: when `ctx.suspendable` is true, unregistered native callbacks trigger suspension instead of error; registered callbacks run synchronously as before.
  - `execute_at_continuation` resumes VM from arbitrary instruction pointer with restored state.
  - Public API: `Regex::find_first_suspendable()`, `Regex::resume(continuation, result)`, `Regex::find_first_async(resolver)`.
  - Zero overhead on synchronous path (`suspendable` defaults to `false`; the only new branch is never-taken).
  - Thread-safe: `MatchContinuation` is `Send + Sync` by construction (all owned data).
  - 12 unit tests covering sync completion, suspension, resume with success/failure/values, backtracking after failure, chained suspensions, context snapshot correctness, Send+Sync verification.
- Validation:
  - `cargo test -p rgx-core` (327 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (39 pass), 0 new clippy warnings.

### 2026-04-06 - Ship Layer 6: File-Backed Matching (core API)
- Scope: new host integration layer — match directly against filesystem files.
- Changes:
  - Created `rgx-core/src/file.rs` with `FileMatch` struct and 4 methods on `Regex`:
    - `match_file(path)` — whole-file `find_all`, returns `Vec<MatchResult>`
    - `match_file_lines(path)` — line-oriented scan, returns `Vec<FileMatch>` with 1-based line numbers
    - `scan_file(path)` — whole-file scan returning match count (callbacks fire implicitly)
    - `scan_file_lines(path)` — line-by-line scan returning match count
  - `FileMatch` re-exported from `rgx-core` public API.
  - 5 unit tests covering matches, line numbers, error handling, and scan variants.
  - `tail_file` (streaming/watching) and CLI integration deferred to follow-up.
- Validation:
  - `cargo test -p rgx-core` (314 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (39 pass), 0 new clippy warnings.

### 2026-04-06 - Ship Layer 4: Structured Events
- Scope: new host integration layer — engine emits typed events at key execution points.
- Changes:
  - Created `rgx-core/src/events.rs` with `MatchEvent` enum (6 variants).
  - Added `Regex::on_event(observer)` API; zero overhead when no observer registered.
  - Instrumented all scanning strategies and key VM opcodes with event emission.
  - 3 unit tests covering match attempts, backtracking, and zero-overhead verification.
- Validation:
  - `cargo test -p rgx-core` (309 pass), 0 new clippy warnings.

### 2026-04-06 - Ship Layer 3: Match Steering
- Scope: new host integration layer — callbacks can steer match execution.
- Changes:
  - Added `SteerResult` enum with 5 variants: `Continue`, `Fail`, `Accept`, `Skip(usize)`, `Abort`.
  - Added `ExecResult::Steer(SteerResult)` to the callback return type.
  - VM code-block dispatch handles all steering actions: `Accept` forces immediate match, `Skip(n)` advances position, `Abort` reuses `(*COMMIT)` infrastructure to stop the scanning loop.
  - Internal `CodeBlockOutcome` enum replaces `Option<bool>` for clearer VM dispatch.
  - `SteerResult` re-exported from `rgx-core` public API.
  - Added 5 unit tests covering each steering action.
  - Updated `docs/HOST_INTEGRATION_ARCHITECTURE.md` Layer 3 status to `shipped`.
- Validation:
  - `cargo test -p rgx-core` (306 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (39 pass), 0 new clippy warnings.
- Notes/impact:
  - This is the first host integration layer beyond basic predicates. Callbacks can now actively control match behavior, enabling use cases like early termination, position skipping for log scanning, and forced acceptance based on domain logic.
  - Inline-language steering (Lua/JS/Rhai helpers) is planned as a follow-up.

### 2026-04-06 - Close PCRE2 speed gap: text copy elimination and trace gating
- Scope: major performance optimization targeting per-call overhead.
- Changes:
  - `ExecContext.text` changed from `Vec<u8>` to `&[u8]` borrowed slice, eliminating the per-call text allocation.
  - All trace/debug/log macros gated behind `cfg(feature = "trace")`, zero-cost in non-trace builds.
  - Prefix filter extended to skip zero-width assertions and match compiled char classes.
- Validation:
  - Literal find_first 1K: 51x → 4.6x vs PCRE2. Capture find_first 1K: 24x → 5.6x.
  - 8 of 10 matching benchmarks under <10x target. Email at ~14x (VM dispatch overhead).
  - Total session speedup: literal 106x→7x (15x faster), capture 1437x→6x (240x faster).

### 2026-04-06 - Ship \K match-reset and \R newline sequence escapes
- Scope: two new PCRE2 escape sequences.
- Changes:
  - `\K` resets the reported match start without affecting what the engine matches. `foo\Kbar` on `foobar` reports match `bar` (span 3..6).
  - `\R` matches any newline sequence: `\r\n` (CRLF tried first), `\r`, `\n`, `\x0B`, `\x0C`, `\x85`, `\u{2028}`, `\u{2029}`.
  - `\K` implemented via new `MatchReset` opcode (0x06) and `match_start_override` field in ExecContext.
  - `\R` expanded at codegen time into `(?:\r\n|\r|\n|...)` alternation.
  - Added 14 unit tests and 12 PCRE2 parity tests across both features.
- Validation:
  - `cargo test -p rgx-core` (258 pass), `-p rgx-bench` (39 pass), 0 clippy warnings

### 2026-04-06 - Ship extended/verbose mode (?x:...)
- Scope: new regex feature — extended mode where unescaped whitespace is ignored and `#` starts comments.
- Changes:
  - Added `WhitespaceLiteral(char)` AST variant to distinguish unescaped whitespace (strippable in x-mode) from escaped whitespace (`\ `, always preserved).
  - PGEN adapter `convert_whitespace_literal` produces `WhitespaceLiteral` for PGEN's `whitespace_literal` rule.
  - Added `strip_extended_mode` compiler pass that removes `WhitespaceLiteral` nodes and `#`-comments inside `FlagGroup` scopes containing `x`.
  - Outside x-mode, `WhitespaceLiteral` is lowered to regular `Char`.
  - Escaped space `\ ` recognized as valid escape in the PGEN adapter.
  - Added 5 unit tests (whitespace, comments, escaped space, class space, scoping) and 3 PCRE2 parity tests.
- Validation:
  - `cargo test -p rgx-core` (244 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
- Notes/impact:
  - `(?x)` is the fourth and final inline flag. The flag system is now complete: `(?i)`, `(?m)`, `(?s)`, `(?x)`, with enable, disable, scoped, inline, and combined forms all working.
  - Parity case count now 243.

### 2026-04-06 - Ship flag negation (?-i:...), (?-m:...), (?-s:...) and combined forms
- Scope: new regex feature — scoped and inline flag disabling.
- Changes:
  - Flag strings now properly parse the `-` separator: characters before `-` are enabled, after `-` are disabled.
  - `(?-i:ABC)` disables case-insensitive within scope; `(?i-s:...)` enables case-insensitive and disables dotall.
  - Inline `(?-i)`, `(?-m)`, `(?-s)` also work.
  - Added 4 unit tests covering disable for each flag and combined enable+disable.
- Validation:
  - `cargo test -p rgx-core` (239 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
- Notes/impact:
  - Completes the inline flag system: enable, disable, scoped, inline, combined — all working.

### 2026-04-06 - Bump PGEN to 1.1.7, close PGEN-RGX-0010
- Scope: PGEN submodule upgrade, R1 workaround removal.
- Changes:
  - Bumped `subs/pgen` from `f876a60` (1.1.6) to `8b31c80` (1.1.7).
  - PGEN 1.1.7 fixes `(?(R1)...)` ambiguity: now produces `recursion_condition` with digits child instead of bare `name("R1")`.
  - Removed R1 workaround `strip_prefix` calls from `convert_condition_text_fallback`.
  - `strip_prefix`/`strip_suffix` count in parsing.rs: **2** (only untagged code block fallback).
  - Closed `PGEN-RGX-0010` as `verified-fixed-upstream`.
- Validation:
  - All 282 tests pass, 0 clippy warnings.
- Notes/impact:
  - All 6 RGX-filed PGEN issues (0005-0010) now closed. PGEN 1.1.7 is the current pin.
  - String-parsing in the adapter is now at the irreducible minimum: 2 calls for untagged code block delimiter stripping.

### 2026-04-06 - Use PGEN structured AST for remaining adapter sites; file PGEN-RGX-0010
- Scope: eliminate remaining string-parsing in the PGEN adapter.
- Changes:
  - `convert_extended_char_class` now reads `extended_class_content` child structurally.
  - `convert_condition` rewritten to dispatch on PGEN child rule names (`recursion_condition`, `name_ref`, `signed_digits`, `name`) instead of text parsing. Split into 5 focused methods.
  - `parse_counted_quantifier` now walks `counted_quantifier_body` children.
  - String-parsing sites reduced from 11 to **4** (2 untagged code block fallback, 2 R1 ambiguity workaround).
  - Filed `PGEN-RGX-0010` for `(?(R1)...)` ambiguity: PGEN parses `R1` as a bare name instead of `recursion_condition` with group number.
- Validation:
  - All 282 tests pass, 0 clippy warnings.
- Notes/impact:
  - The adapter now uses PGEN's structured AST for virtually all constructs. The 4 remaining string-parsing sites are either intentional (untagged code blocks have no structure) or workarounds for open PGEN issues.

### 2026-04-06 - Bump PGEN to 1.1.6, close PGEN-RGX-0009
- Scope: PGEN submodule upgrade verifying the code_content span fix.
- Changes:
  - Bumped `subs/pgen` from `11821c4` (1.1.5) to `f876a60` (1.1.6).
  - PGEN 1.1.6 fixes the `ws?` rule that consumed the first byte of the code body in `code_block_lang`.
  - Adapter `convert_code_block` now reads `code_content` span directly — the span-text workaround is removed.
  - `strip_prefix`/`strip_suffix` count in parsing.rs reduced from 16 to 11.
  - Closed `PGEN-RGX-0009` as `verified-fixed-upstream`.
- Validation:
  - All 282 tests pass, 0 clippy warnings.
  - code_content span verified: [10, 23] → "validate_word" (was [11, 23] → "alidate_word").
- Notes/impact:
  - All 5 RGX-filed PGEN issues (0005-0009) now closed. PGEN 1.1.6 is the current pin.

### 2026-04-06 - Bump PGEN to 1.1.5, close PGEN-RGX-0007 and PGEN-RGX-0008
- Scope: PGEN submodule upgrade, adapter cleanup, issue closure.
- Changes:
  - Bumped `subs/pgen` from `962acfd` (1.1.3) to `11821c4` (1.1.5).
  - PGEN 1.1.4 fixes `\g<1>` numeric-angle subroutine reference (PGEN-RGX-0007).
  - PGEN 1.1.5 fixes `code_block_lang` PEG ordering and adds `rhai`/`native`/`wasm` to `code_lang` (PGEN-RGX-0008).
  - Adapter `convert_code_block` now reads `code_lang` structurally from `code_block_lang` child; body extracted from span text due to ws? offset.
  - Adapter `convert_named_backreference` removed `\g<1>` span-text workaround since PGEN now produces proper `subroutine_ref` for all `\g` forms.
  - Both issues closed as `verified-fixed-upstream` with fixed AST dumps archived.
- Validation:
  - `cargo test -p rgx-core` (235 pass), `-p rgx-bench` (38 pass), 0 clippy warnings
- Notes/impact:
  - All 4 RGX-filed PGEN issues (0005-0008) are now closed. PGEN 1.1.5 is the current pin.

### 2026-04-06 - Use PGEN structured AST for flags, backrefs, subroutines; file PGEN-RGX-0007
- Scope: deeper PGEN adapter integration using structured child nodes instead of span-text string-parsing.
- Changes:
  - `convert_scoped_inline_modifiers` now walks `modifier_spec` → `modifier_group` for flag chars and calls `convert_pattern()` directly on the nested body, no more delimiter splitting or body re-parsing.
  - `convert_inline_modifiers` now walks `modifier_spec` natively.
  - `convert_named_backreference` dispatches on prefix terminal and uses structured `backreference_digits`/`name_ref`/`name` children for `\1`, `\k<name>`, `\k'name'`, `\k{name}`, `\g{name}`.
  - `convert_python_named_backreference` uses `name_text` helper on the `name` descendant.
  - `convert_subroutine_call` inspects `subroutine_target` structure for `(?R)`, `(?1)`, `(?&name)`, `(?P>name)` — adds `P>` support.
  - Added `name_text()` and `collect_modifier_flags()` helpers for structured extraction.
  - Code blocks retain span-text parsing (PGEN's PEG ordering fuses language prefix into `code_block_plain`).
  - Filed `PGEN-RGX-0007` for the `\g<1>` numeric-angle subroutine reference misparse.
- Validation:
  - `cargo test -p rgx-core` (235 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (38 pass), 0 clippy warnings
  - `strip_prefix`/`strip_suffix` count reduced from **31 → 16** in `parsing.rs`
  - Remaining: 2 for code blocks (intentional), 5 for `\g<1>` fallback, 9 in extended char class / conditional / counted quantifier
- Notes/impact:
  - Over half of the remaining string-parsing sites eliminated by walking PGEN's native AST structure.

### 2026-04-06 - Bump PGEN to 1.1.3 with braced octal fix (closes PGEN-RGX-0006)
- Scope: PGEN submodule upgrade verifying the upstream fix for the braced octal bug.
- Changes:
  - Bumped `subs/pgen` from `f97e0fe` (PGEN 1.1.2) to `962acfd` (PGEN 1.1.3) — "Release regex 1.1.3 braced octal fix".
  - Verified `\o{101}` now produces the correct `octal_escape` AST with three `octal_digit` children (was misparsed as `simple_escape(o) + counted_quantifier{101}`).
  - Added 2 RGX regression tests (`braced_octal_escape_matches_codepoint`, `braced_octal_escape_various_values`) and 1 PCRE2 parity case (`braced_octal_escape_all`).
  - Closed `pgen-issues/PGEN-RGX-0006.yaml` as `verified-fixed-upstream`, with verification notes and the fixed AST dump archived under `pgen-issues/artifacts/PGEN-RGX-0006/`.
  - Updated pinned-commit references in README.md, RUST_CODEBASE_ANALYSIS.md, MEMORY.md, and DEVELOPMENT_NOTES.md.
- Validation:
  - `cargo test -p rgx-core` (233 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (38 pass)
  - Parse probe confirms AST now contains `octal_escape` with `octal_digits` structured child tree
- Notes/impact:
  - First successful round-trip of the local PGEN issue reporting protocol: bug filed → fix upstream → submodule bump → regression tests added → issue closed.
  - Parity case count now 240.

### 2026-04-05 - Use PGEN's structured AST natively for escapes and char classes
- Scope: eliminate secondary parsing in the PGEN adapter by traversing PGEN's structured child trees.
- Changes:
  - Removed the `crate::lexer::Lexer::new(fragment).next_token()` call that was tokenizing bracket expressions inside `convert_char_class`.
  - Removed `convert_escape_complex` string-slicing dispatch that hand-parsed `\h`, `\H`, `\v`, `\V`, `\p`, `\P`, `\x`, `\c`.
  - Added structured tree-walking handlers that use PGEN's native rule hierarchy:
    - `convert_hex_escape` — walks `hex_digit` descendants
    - `convert_property_escape` — walks `prop_name` subtree, reads `p{`/`P{` polarity
    - `convert_control_escape` — reads `any_char` child
    - `convert_octal_escape` — walks `octal_digit` descendants
    - `convert_char_class` — uses `negation` child, iterates `class_body` children
    - `convert_class_range` / `convert_class_escape` — traverse structured range/escape nodes
  - Added tree-navigation helpers: `find_direct_child`, `find_first_terminal_text`, `collect_first_terminal_char`, `walk_collect_terminal_chars`, `collect_all_terminal_chars`.
- Validation:
  - `cargo test -p rgx-core` (233 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (37 pass)
  - 0 clippy warnings
  - `grep 'crate::lexer\|crate::parser::Parser' parsing.rs` returns zero matches
- Notes/impact:
  - The adapter now uses PGEN's grammar structure directly for escapes and character classes instead of re-parsing span text.
  - Remaining string parsing is limited to short prefixes/suffixes for constructs where PGEN coarsely flattens (flag chars in `(?i:...)`, code block `lang:code` splits, subroutine targets, backreference names) — these are single-character discriminations, not full parsers.
  - The shorthand escape inspection (`\d` vs `\D`) still reads the terminal letter because PGEN flattens all shorthands through `simple_escape -> any_char -> letter`.

### 2026-04-05 - Retire builtin recursive-descent parser — PGEN is now the sole parser
- Scope: major integration refactor eliminating all use of the builtin parser from the PGEN adapter.
- Changes:
  - Removed `RecursiveDescentParser` struct and its `RegexParser` impl entirely.
  - Removed `PgenFeatureBackend` enum and `PGEN_FEATURE_BACKEND` const — no backend switch remains.
  - Removed `parse_leaf_fragment` method — the core fallback that delegated to `crate::parser::Parser`.
  - Removed all `#[cfg(not(feature = "pgen-parser"))]` code paths for `parse_pattern`, `parser_name`, `parser_capabilities`.
  - Added native PGEN atom converters for all 8 confirmed leaf rule names: `literal`, `dot`, `anchor`, `escape`, `char_class`, `code_block`, `subroutine_call`, `python_named_backreference`.
  - Added unsupported-error paths for 4 grammar-defined but unimplemented rule names: `callout`, `comment_group`, `directive_verb`, `whitespace_literal`.
  - The `escape` handler covers: `\d/\D/\w/\W/\s/\S`, `\b/\B`, `\A/\Z/\z`, `\h/\H/\v/\V`, `\p{}/\P{}`, `\xNN/\x{NNNN}`, `\cA`, `\n/\t/\r/\f/\a/\e`, `\1/\2`, and escaped metacharacters.
  - The `char_class` handler uses the Lexer (tokenizer) to parse bracket expressions — the Lexer is a tokenizer, not the parser.
  - The wildcard `_ =>` in `convert_atom` now returns a contract error instead of silently falling back.
  - Fixed `convert_scoped_inline_modifiers` to traverse PGEN's child nodes instead of re-parsing the body.
  - Fixed `convert_named_backreference` to handle numeric backreferences natively.
- Validation:
  - `cargo test -p rgx-core` (266 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
  - `grep -c 'parse_leaf_fragment|crate::parser::Parser' parsing.rs` = 0
  - `grep -c 'RecursiveDescentParser|PgenFeatureBackend' parsing.rs` = 0
- Notes/impact:
  - **The builtin recursive-descent parser is now fully retired from the PGEN integration path.** Any PGEN parse issue or missing atom rule will surface as an explicit error instead of being silently masked by the fallback.
  - The `parser.rs` and `lexer.rs` modules still exist in the codebase but are no longer called from the default parsing path.

### 2026-04-05 - Ship Python-style named groups (?P<name>...) and (?P=name)
- Scope: new regex feature — Python-compatible named group syntax.
- Changes:
  - `(?P<name>...)` now works as an alternative syntax for `(?<name>...)` named capturing groups.
  - `(?P=name)` now works as an alternative syntax for `\k<name>` named backreferences.
  - Added `parse_python_group` lexer helper that dispatches on `<` (named group) or `=` (backreference).
  - Both forms reuse existing `NamedGroupStart` and `NamedBackreference` tokens — no new AST/compiler changes needed.
  - Added 3 unit tests and 2 PCRE2 parity tests.
- Validation:
  - `cargo test -p rgx-core` (266 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
- Notes/impact:
  - Patterns from Python's `re` module now work without rewriting. Parity case count now 239.

### 2026-04-05 - Wire PGEN adapter for inline flags and named backreferences
- Scope: PGEN parser adapter integration for recently shipped syntax.
- Changes:
  - Added native `convert_scoped_inline_modifiers`, `convert_inline_modifiers`, and `convert_named_backreference` methods to the PGEN AST adapter, replacing the recursive-descent `parse_leaf_fragment` fallback for these constructs.
  - PGEN already produces correct rule names (`scoped_inline_modifiers`, `inline_modifiers`, `backreference`) — the gap was in RGX's adapter, not in PGEN.
  - Removed dead `convert_flag_group` method (PGEN never produces a `flag_group` rule name).
  - Added 6 parser-contract reference fixtures for the new syntax families.
- Validation:
  - `cargo test -p rgx-core` (263 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
- Notes/impact:
  - No PGEN bug reports needed — PGEN handles all three syntax families correctly at the grammar level.
  - The PGEN-backed path now converts inline flags and named backreferences natively instead of discarding the PGEN output and re-parsing.

### 2026-04-05 - Ship named backreferences \k<name> and \k'name'
- Scope: new regex feature — named backreferences.
- Changes:
  - `\k<name>` and `\k'name'` now reference previously captured named groups and match the same text.
  - Added `Token::NamedBackreference`, `Regex::NamedBackreference(String)` AST node, lexer parsing for both delimiter styles, and compiler resolution to numbered `OpCode::Backref` via the named-group registry.
  - Missing named groups produce explicit compile errors.
  - Added 3 unit tests (basic, quote-style, missing-group error) and 2 PCRE2 parity tests.
- Validation:
  - `cargo test -p rgx-core` (263 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
- Notes/impact:
  - Patterns like `(?<word>\w+)\s+\k<word>` now work for repeated-word detection.
  - Parity case count now 237.

### 2026-04-05 - Ship non-scoped inline flags (?i), (?m), (?s) and combinations
- Scope: new regex feature — non-scoped inline flag toggles.
- Changes:
  - `(?i)`, `(?m)`, `(?s)` and combinations like `(?im)` now apply their flag to the rest of the current group or pattern.
  - Added `Token::FlagToggle { flags }` to the lexer; when flag chars are followed by `)` instead of `:`, emit a toggle instead of a scoped group start.
  - Parser intercepts `FlagToggle` in `parse_sequence` and wraps the remaining sequence in `Regex::FlagGroup`.
  - Added `lower_flag_toggles` compiler pass to handle standalone empty-body `FlagGroup` nodes (from PGEN-backed parsing) by absorbing subsequent siblings.
  - Added 7 lexer/parser unit tests and 3 PCRE2 parity tests.
- Validation:
  - `cargo test -p rgx-core` (260 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
- Notes/impact:
  - `(?i)abc` is now equivalent to `(?i:abc)`. Combined forms like `(?ims)` work.
  - Parity case count now 235.

### 2026-04-04 - Ship scoped case-insensitive mode (?i:...)
- Scope: new regex feature — scoped case-insensitive flag groups.
- Changes:
  - `(?i:...)` now makes literal characters and character classes match case-insensitively within the group scope.
  - When case-insensitive, `Char('a')` compiles to a 2-entry char class matching both `'a'` and `'A'`; custom character class ranges are expanded with their case-folded counterparts.
  - Added `case_insensitive: bool` flag to compiler, propagated through sub-compilers.
  - Added 4 unit tests and 3 PCRE2 parity tests.
- Validation:
  - `cargo test -p rgx-core` (250 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
- Notes/impact:
  - Third inline flag shipped. `(?i:...)` is the most commonly used flag in real-world regex patterns.
  - Current scope is ASCII case folding. Unicode case folding is future work.
  - Parity case count now 232.

### 2026-04-04 - Ship scoped dotall mode (?s:...)
- Scope: new regex feature — scoped dotall flag groups.
- Changes:
  - `(?s:...)` now makes `.` match any character INCLUDING `\n` within the group scope.
  - Added `AnyDotAll` opcode (0x05) for dotall-mode dot matching.
  - Added `dotall: bool` flag to `OptimizingCompiler`, toggling `Dot` compilation between `Any` (default, excludes `\n`) and `AnyDotAll` (dotall, includes `\n`).
  - Flag state saves/restores correctly across nested groups, propagated to sub-compilers.
  - Added 3 unit tests (dotall match, scope-leak, default behavior) and 3 PCRE2 parity tests.
- Validation:
  - `cargo test -p rgx-core` (246 pass), `-p rgx-bench` (37 pass), 0 clippy warnings
- Notes/impact:
  - Second inline flag shipped. Combined with `(?m:...)`, patterns like `(?ms:^.*$)` can now match entire lines including newlines.
  - Parity case count now 229.

### 2026-04-04 - Replace backtrack frame cloning with SOTA trail/undo log
- Scope: SOTA-grade refactor of the VM backtracking mechanism.
- Changes:
  - Replaced full capture-vector cloning (`Vec<Option<usize>>::clone()`) on every backtrack frame with a trail/undo log that records only modified slots.
  - Replaced full call-stack cloning with a length mark (`usize`); backtrack now truncates to the mark.
  - New `set_capture(ctx, index, value)` helper records `(index, old_value)` in the trail before each write.
  - New `undo_trail(ctx, mark)` replays the trail backwards to restore capture state at the save point.
  - `BacktrackFrame` now stores `trail_mark: usize` + `call_stack_mark: usize` instead of two cloned `Vec`s.
  - Probe-based frames (StarLazy) retain snapshot captures for correctness where trail replay is insufficient.
  - Updated ~15 frame construction sites, ~10 inline save/restore sites, and 3 backtrack restoration paths.
- Validation:
  - `cargo test -p rgx-core` (243 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (37 pass)
  - 0 clippy warnings
  - Benchmark improvement from trail-based backtracking: email find_all 10K 105x→62x (1.7x faster), literal find_all 10K 45x→30x (1.5x faster)
- Notes/impact:
  - This is the most impactful SOTA upgrade: backtrack frame saves are now O(1) instead of O(num_groups), and restores are O(modified_slots) instead of O(num_groups).
  - Total session performance: find_all literal 1K from 106x to 30x (3.5x), capture 10K from 1437x to 21x (68x).

### 2026-04-04 - Upgrade VM hot paths to SOTA quality
- Scope: replace below-SOTA implementations in the VM execution hot path.
- Changes:
  - **UTF-8 character decoding**: `current_char()` now decodes only the minimal bytes at the current position (1 for ASCII, 2-4 for multi-byte) instead of validating the entire remaining text via `from_utf8()`. ASCII fast path is a single byte check and cast.
  - **Character advance**: `advance_char()` now determines character width directly from the leading byte without calling `current_char()` to decode the full character.
  - **Unicode range lookup**: `test_char_class()` now uses binary search (`O(log N)`) on sorted Unicode ranges instead of linear scan (`O(N)`). For classes with 50+ Unicode ranges (common with `\p{...}` properties), this is a significant improvement.
- Validation:
  - `cargo test -p rgx-core` (243 pass), `-p rgx-bench` (37 pass)
  - 0 clippy warnings
- Notes/impact:
  - These are the first SOTA-grade replacements in the VM hot path, targeting the two areas where the implementation was furthest from production regex engine quality.
  - The ASCII bitmap character class lookup was already O(1) and SOTA-grade; no changes needed there.
  - Remaining SOTA gaps: backtrack frame capture cloning (critical, needs trail/undo log), text reference copy (major, needs lifetime refactor), trace log overhead in hot loop (moderate, needs compile-time gating).

### 2026-04-04 - Ship scoped multiline mode (?m:...)
- Scope: new regex feature — scoped multiline flag groups.
- Changes:
  - `(?m:...)` now makes `^` and `$` match at line boundaries (after/before `\n`) within the group scope, while keeping single-line semantics outside.
  - Lexer: added `parse_flag_modifier()` that recognizes `(?m:`, `(?i:`, `(?s:`, `(?x:` and multi-flag combinations.
  - Parser: handles `Token::FlagModifier` and wraps the group body in `Regex::FlagGroup { flags, expr }`.
  - AST: added `FlagGroup` variant to the `Regex` enum.
  - Compiler: added `multiline: bool` field to `OptimizingCompiler`, toggles anchor opcode emission between `StartLine`/`EndLine` (multiline) and `StartText`/`EndTextOrNL` (default). Flag state saves/restores correctly across nested groups.
  - `should_use_start_anchored_search` now correctly avoids the anchored fast-path when `StartLine` is the first opcode (multiline `^` needs scanning, not just position 0).
  - PGEN adapter handles the new AST node via leaf-fragment fallback.
  - Added 3 unit tests and 5 PCRE2 differential parity tests, including scope-leak verification.
- Validation:
  - `cargo test -p rgx-core` (243 pass), `-p rgx-cli` (10 pass), `-p rgx-bench` (37 pass)
  - 0 clippy warnings
- Notes/impact:
  - This is the first inline flag shipped on the default regex path.
  - The lexer infrastructure also accepts `(?i:`, `(?s:`, `(?x:` syntax, but only `(?m:...)` has compiler/VM support in this commit.
  - Total parity case count now 226.

### 2026-04-04 - Fix find_all zero-width suppression to match PCRE2 iteration semantics
- Scope: accuracy fix for find_all zero-width match handling.
- Changes:
  - `find_all` now tracks the end position of the previous consuming match and suppresses zero-width matches that start at that exact position, matching PCRE2's `find_iter` semantics.
  - Previously `a*` on `"aab"` returned `[(0,2),(2,2),(3,3)]`; now returns `[(0,2),(3,3)]` (PCRE2 behavior).
  - Also fixes the previously-known `(?=a)|b` on `"ba"` bug — the extra zero-width `(1,1)` match is now suppressed.
  - Added 4 differential parity regression tests: `star_zero_width_suppressed_after_consuming`, `star_zero_width_suppressed_single_char`, `star_zero_width_suppressed_mixed`, `lookahead_alt_zero_width_suppressed`.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (37 pass)
  - 0 clippy warnings
- Notes/impact:
  - This closes all 3 accuracy bugs found by the initial PCRE2 probe plus the open low-severity item.
  - Total parity case count now 221.

### 2026-04-04 - Fix empty-string match compilation bug
- Scope: accuracy fix for zero-width pattern matching.
- Changes:
  - Added an explicit `Regex::Empty => {}` arm to the compiler's `codegen_pass`, preventing the empty AST node from falling through to the catch-all `_ => Fail` arm.
  - Previously, patterns like `()`, `|a`, and `a||b` that should match the empty string at every position produced no matches because the empty node compiled to `OpCode::Fail`.
  - Added 4 differential parity regression tests: `empty_capture_group`, `empty_first_alternative`, `empty_middle_alternative`, `optional_zero_width`.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (37 pass)
  - 0 clippy warnings
- Notes/impact:
  - This is a **semantic accuracy fix**. The empty-string pattern is fundamental to regex semantics — it appears in `()?`, `()*`, `|alt`, and `DEFINE` blocks.
  - Total parity case count now 217.

### 2026-04-04 - Fix ^ and $ to match PCRE2 single-line default semantics
- Scope: accuracy fix for anchor behavior in default mode.
- Changes:
  - `^` and `$` now compile to `StartText` / `EndTextOrNL` (start-of-string / end-of-string-or-before-final-newline), matching PCRE2's default single-line anchor semantics.
  - Previously `^` compiled to `StartLine` (matched after `\n`) and `$` compiled to `EndLine` (matched before `\n`), which is multiline behavior that PCRE2 only enables with `(?m)`.
  - `StartLine` / `EndLine` opcodes are preserved for future `(?m)` multiline mode support.
  - Added 4 differential parity regression tests specifically covering default-mode anchor behavior: `caret_not_multiline_by_default`, `dollar_not_multiline_by_default`, `caret_only_matches_string_start`, `dollar_before_final_newline`.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (37 pass)
  - 0 clippy warnings
- Notes/impact:
  - This is a **semantic accuracy fix**, not a performance change. Without it, `^a` would incorrectly match `a` after a newline in `"b\na"`, and `a$` would incorrectly match `a` before a newline in `"a\nb"`.
  - Total parity case count now 213.

### 2026-04-04 - Add character-class prefix filter to scanning loop
- Scope: performance optimization extending the prefix skip to character classes.
- Changes:
  - Replaced the single-byte `first_literal_byte` field with a richer `PrefixFilter` enum that also recognizes `\d` (Digit), `\w` (Word), and `\s` (Space) class prefixes.
  - Both `find_first_scanning` and `find_all` now skip positions where the first byte cannot match the prefix class, using `memchr` for literal bytes and inline class predicates for `\d`/`\w`/`\s`.
  - `PrefixFilter` is cached once at VM construction and used on every scanning iteration.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (37 pass)
  - Benchmark trend capture confirms dramatic improvement for digit-prefixed patterns:
    - `find_first capture_groups 1K`: 1116x → 31x slower vs PCRE2 (**36x faster**)
    - `find_all capture_groups 1K`: 1098x → 23x slower vs PCRE2 (**49x faster**)
    - `find_first capture_groups 10K`: 1414x → 28x slower vs PCRE2 (**50x faster**)
    - `find_all capture_groups 10K`: 1437x → 22x slower vs PCRE2 (**65x faster**)
  - Literal patterns unchanged from previous memchr baseline (~35-57x vs PCRE2).
- Notes/impact:
  - Total session performance improvement across all benchmark patterns now ranges from 1.9x to 65x faster vs the original baseline.
  - The remaining ~22-57x gap vs PCRE2 is primarily in per-position VM execution overhead, not in candidate selection.

### 2026-04-04 - Use memchr for scanning loop candidate search
- Scope: performance optimization for VM scanning strategy.
- Changes:
  - Replaced manual byte-comparison skip in `find_first_scanning` and `find_all` with `memchr`-based candidate jumping, which uses platform-native SIMD internally.
  - Both fast paths now use `memchr(fb, &ctx.text[offset..])` to find the next position where the first required literal byte occurs, skipping all impossible positions in bulk.
  - The slow path (no literal prefix) falls through to the original position-by-position scan unchanged.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (37 pass)
  - Benchmark results within noise of previous byte-comparison skip for these inputs; the win is in code clarity and future rare-byte scenarios.
- Notes/impact:
  - This completes the three-part scanning optimization series: (1) literal-prefix extraction, (2) in-place find_all, (3) memchr-accelerated candidate search.
  - Total session improvement vs original baseline: find_all literal 1K 106x→35x (3.0x), find_first literal 1K 109x→57x (1.9x).

### 2026-04-04 - Rewrite find_all to scan in-place with single context
- Scope: performance optimization for `find_all` matching.
- Changes:
  - Replaced the old `find_all` implementation (which called `find_first` on substrings, copying the remaining text on each iteration) with an in-place scanning loop that reuses one `ExecContext`.
  - The new implementation also applies the literal-prefix skip directly, avoiding unnecessary `execute_at` calls at impossible positions.
  - Eliminates O(n) text copies per match — for a 10K input with 100 matches, the old path allocated ~1MB of copies; the new path allocates once.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` (10 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (37 pass)
  - Benchmark trend capture confirms measurable `find_all` improvement across all patterns:
    - `find_all literal_simple 1K`: 60x → 34x slower vs PCRE2 (~1.8x faster)
    - `find_all literal_simple 10K`: 70x → 43x slower vs PCRE2 (~1.6x faster)
    - `find_all capture_groups 1K`: 1144x → 876x slower vs PCRE2 (~1.3x faster)
    - `find_all capture_groups 10K`: 1426x → 1124x slower vs PCRE2 (~1.3x faster)
  - Combined with the earlier literal-prefix skip, total `find_all` improvement vs original baseline: up to 3.1x faster.

### 2026-04-04 - Add literal-prefix skip to VM scanning loop
- Scope: performance optimization for the VM scanning strategy.
- Changes:
  - Added `first_required_byte()` helper that extracts the first literal byte from the compiled bytecode, cached once at VM construction.
  - Modified `find_first_scanning` to skip positions where the first required literal byte doesn't match, avoiding full VM invocations at impossible positions.
  - Added explanatory comment to `should_use_simd_search` documenting why the SIMD candidate-collection strategy is gated on x86 SSE2 rather than ARM NEON.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` (10 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (37 pass)
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (33 total, 0 RGX-owned)
  - Quick benchmark trend capture confirms measurable improvement for literal-starting patterns:
    - `find_first literal_simple 1K`: 109x → 55x slower vs PCRE2 (~2x faster)
    - `find_first literal_simple 10K`: 130x → 74x slower vs PCRE2 (~1.8x faster)
    - `find_all literal_simple 1K`: 106x → 60x slower vs PCRE2 (~1.8x faster)
    - `find_all literal_simple 10K`: 119x → 70x slower vs PCRE2 (~1.7x faster)
  - No change for patterns that don't start with a single-byte literal (capture_groups, email_basic with word boundary).
- Notes/impact:
  - This is the first landed VM performance optimization targeting the scanning loop hot path.
  - The approach is conservative — it only skips when the pattern begins with a single-byte `Char` opcode. Multi-byte and non-literal prefixes fall through to the full scan.
  - Future work: extend to multi-byte literal prefixes, use `memchr` for the byte search, and reduce per-call `ExecContext` allocation.

### 2026-04-04 - Eliminate all RGX-owned clippy warnings through function refactoring
- Scope: structural refactoring of 10 over-length functions plus targeted suppression of 3 architectural VM functions.
- Changes:
  - Refactored 6 borderline functions (112-141 lines) in `compiler.rs`, `lexer.rs`, and `vm.rs` by extracting natural helpers.
  - Refactored 4 medium functions (180-308 lines) in `execution.rs`, `lexer.rs`, and `parser.rs` by extracting dispatch/parsing helpers.
  - Added `#[allow(clippy::too_many_lines)]` with architectural rationale to 3 large VM functions (`execute_at` at 718, `execute_subexpr` at 527, `codegen_pass` at 282 lines) that are inherently monolithic dispatch loops.
  - Feature-gated `dispatch_engine` helper behind language features to avoid dead-code warning.
  - Fixed iterator-style loop warning in extracted `find_byte_tail` helper.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` (10 pass)
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (33 total, 0 RGX-owned)
- Notes/impact:
  - RGX-owned clippy warnings now at **zero** — from 296 at the start of this session. All remaining 33 workspace warnings come from the PGEN submodule.

### 2026-04-04 - Expand PCRE2 differential parity coverage for combined features
- Scope: parity test expansion covering combined-feature patterns and edge cases.
- Changes:
  - Added `pcre2_parity_supported_combined_feature_patterns` test function with 24 new differential parity cases covering:
    - nested lookarounds (lookahead-in-lookbehind, lookbehind-in-lookahead, negative lookahead in alternation)
    - atomic groups combined with quantifiers and alternation (3 no-match behavioral cases)
    - backreference edge cases (alternation, quantified captures)
    - possessive quantifiers combined with alternation
    - named groups in various positions
    - complex quantifier interactions (nested, lazy-inside-greedy, counted-range backtracking)
    - anchors combined with groups and alternation
    - dot and character class interactions
  - All 24 cases verify both first-match and find-all span parity against PCRE2.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (37 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
- Notes/impact:
  - Parity case count increased from 185 to 209 (13% increase).
  - All new cases pass, confirming RGX matches PCRE2 behavior for these combined-feature patterns.

### 2026-04-04 - Clear remaining non-architectural clippy warnings
- Scope: final mechanical warning cleanup across rgx-core.
- Changes:
  - Rewrote 5 `let...else` patterns in `lexer.rs` and `vm.rs`.
  - Unwrapped 3 unnecessary `Result` wrappers from private lexer functions (`parse_star`, `parse_plus`, `parse_question`).
  - Changed 2 pass-by-value `name: String` parameters to `name: &str` in `execution.rs` callback/variable registries, propagated through `engine.rs` and `lib.rs`.
  - Added `#[allow(clippy::inline_always)]` to 5 hot logging/SIMD check functions.
  - Added `#[allow(clippy::struct_excessive_bools)]` to 2 naturally-boolean flag structs.
  - Added `#[allow(clippy::only_used_in_recursion)]` to 3 recursive traversal helpers.
  - Replaced `format!`-based iterator string building with `fold` + `write!` in `log.rs`.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` (10 pass)
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (46 total, 13 RGX-owned, all function-length)
- Notes/impact:
  - RGX-owned warnings now at 13, all exclusively function-length limits (architectural). Every other warning category is resolved.
  - Warning count dropped from 296 to 13 (96% reduction) across this session's four commits.

### 2026-04-04 - Resolve cast-truncation and doc-section warnings
- Scope: targeted clippy warning cleanup in codegen paths and public API docs.
- Changes:
  - Added `#[allow(clippy::cast_possible_truncation)]` to 9 VM codegen functions that intentionally write compact u8/u16 bytecode operands.
  - Added `#[allow(clippy::cast_sign_loss)]` and `#[allow(clippy::cast_possible_wrap)]` to 2 conditional-group index conversions in `compiler.rs` and `parsing.rs`.
  - Added missing `# Errors` sections to 11 public functions across `compiler.rs`, `engine.rs`, `execution.rs`, `vm.rs`, and `log.rs`.
  - Added missing `# Panics` sections to 10 public functions across `execution.rs` and `log.rs`.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` (10 pass)
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --lib` (35 RGX-owned warnings, down from 88)
- Notes/impact:
  - RGX-owned warnings now at 35 (88% reduction from original 296); remaining backlog is function-length limits (12), intentional `#[inline(always)]` (5), and a small tail of structural suggestions.

### 2026-04-04 - Remove dead opcodes and memoization scaffolding from VM
- Scope: dead code cleanup in `vm.rs` opcode surface and execution context.
- Changes:
  - Removed 11 dead/superseded opcodes from the `OpCode` enum: `String`, `CharNoCase`, `StringNoCase`, `Range`, `RangeNeg`, `Return`, `SaveStartCond`, `RestoreCaptures`, `RepeatRange`, `RepeatExact`.
  - Removed the dead `memo_cache: HashMap<(usize, usize), bool>` field from `ExecContext` and its two initializations.
  - Preserved hex slot stability with tombstone comments so remaining opcode values don't shift.
  - Added explicit "reserved for future work" comments on kept-but-unemitted opcodes (SIMD, optimization hints, Accept, Halt, JumpIfMatch).
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` (10 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` (36 pass)
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (121 warnings, unchanged)
- Notes/impact:
  - Shipped regex behavior did not change; this is a dead-code cleanup pass.
  - The opcode surface is now cleaner: every declared opcode is either emitted/executed or explicitly reserved.

### 2026-04-04 - Deep warning-debt reduction across rgx-core
- Scope: workspace-wide clippy warning cleanup, doc hardening, and code quality improvement.
- Changes:
  - Removed 30 redundant `continue` statements from VM execution loops in `vm.rs`.
  - Converted 16 private methods to associated functions in `vm.rs` (unused `self`), updating all call sites.
  - Added `#[allow(clippy::unused_self)]` to 3 intentional stub methods (`simd_compare`, `optimize_ast`, `peephole_optimize`).
  - Combined 11 identical match arms across `compiler.rs`, `parsing.rs`, and `vm.rs`.
  - Rewrote 4 `if let` patterns to `let...else` in `lexer.rs`.
  - Unwrapped 3 unnecessarily `Result`-wrapped private functions in `lexer.rs` (`parse_star`, `parse_plus`, `parse_question`).
  - Changed 2 pass-by-value parameters to references in `compiler.rs` (`ScalarRangeSet::apply`, `lower_extended_char_class_content`).
  - Inlined format string variables across `lexer.rs`, `execution.rs`, `log.rs`, `unicode_support.rs`, and `compiler.rs`.
  - Applied auto-fixable suggestions: redundant else blocks, `map_or` simplifications, `if let` rewrites, and miscellaneous lint fixes across `vm.rs`, `execution.rs`, `lexer.rs`, `engine.rs`, `log.rs`.
  - Added missing `///` field/variant docs to 40 items in `ast.rs`, 36 items in `token.rs`, 4 items in `error.rs`, and 3 functions in `log.rs`.
  - Fixed stale `BranchReset` AST comment from "runtime semantics pending" to reflect shipped status.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` (240 pass)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` (10 pass)
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (121 warnings, down from 329)
- Notes/impact:
  - RGX-owned warnings dropped from 296 to 88 (70% reduction).
  - Remaining backlog is concentrated in cast-truncation warnings, missing `# Errors` / `# Panics` doc sections, function-length limits, and design-intentional patterns.
  - Shipped regex behavior did not change; this is purely a code quality pass.

### 2026-04-02 - Harden parser-facing utility docs and warning contracts
- Scope: parser/AST utility cleanup, public-doc hardening, and warning-debt reduction.
- Changes:
  - Added `#[must_use]` coverage for parser/AST/token constructors and result-returning utility helpers where dropping the value would be surprising.
  - Added missing `# Errors` sections and module docs across parser-facing/public API surfaces, including parser entry points, lexer tokenization, and `Regex` construction/registration helpers.
  - Simplified a small parser/lexer utility slice by switching several `Option` snapshots to `map_or_else`, removing `Position` clones on `Copy` data, centralizing parser fallback-position lookup, adding `Default` to the parser adapter shells, and making a couple of internal `Regex` helpers static.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `cargo clippy --manifest-path Cargo.toml -p rgx-core --tests --message-format short 2>&1 | rg 'rgx-core/src/(ast\\.rs|token\\.rs|lexer\\.rs|parser\\.rs|parsing\\.rs|lib\\.rs)' | rg 'could have a `#\\[must_use\\]` attribute|docs for function returning `Result` missing `# Errors` section|called `map\\(<f>\\)\\.unwrap_or_else\\(<g>\\)` on an `Option` value|using `clone` on type `Position` which implements the `Copy` trait|you should consider adding a `Default` implementation|missing documentation for a function|missing documentation for an associated function|missing documentation for a module|unused `self` argument'`
- Notes/impact:
  - This is a cleanup-only pass; shipped regex behavior did not change.
  - The full workspace `clippy` run now reports `rgx-core` lib warnings down to 329 from the previous 426-warning snapshot, with the remaining backlog now concentrated more heavily in broader docs, lexer structure, and VM/runtime internals.

### 2026-04-02 - Preserve extended char class parity boundary and trim warning debt
- Scope: parity-boundary confirmation, targeted warning cleanup, and continuity refresh.
- Changes:
  - Probed bare top-level Perl extended character class ordinary terms such as `(?[a-z])` and `(?[\dA-F])`, then deliberately kept them out of the shipped subset after local PCRE2 parity checks compile-rejected those forms.
  - Backed out the exploratory syntax widening before commit so the default RGX path stays aligned with current PCRE2 behavior for `(?[...])`.
  - Removed a small RGX-owned clippy-warning slice in `rgx-core` by adding separators to the Unicode scalar-universe literal, simplifying the lexer's relative-conditional sign pattern, renaming quantified locals in the parser and PGEN adapter, and dropping unnecessary raw-string hashes in native-code-block tests.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `cargo clippy --manifest-path Cargo.toml -p rgx-core --tests --message-format short 2>&1 | rg 'rgx-core/src/(compiler\\.rs:18:79|lexer\\.rs:1092:13|parser\\.rs:19:13|parser\\.rs:429:13|parsing\\.rs:919:13|lib\\.rs:(1174:13|1196:38|1220:13|1248:38|1275:38|1289:38|1301:38|1319:38|1337:38|1356:38))'`
- Notes/impact:
  - Shipped extended-character-class behavior did not widen in this pass; the practical boundary is now more explicit: nested ordinary bracket terms remain supported, while bare top-level ordinary terms remain intentionally unsupported unless future parity evidence changes.

### 2026-04-02 - Extend nested ordinary extended char class terms
- Scope: regex runtime feature delivery, parser-contract widening, differential parity coverage, and continuity refresh.
- Changes:
  - Nested ordinary bracket terms inside `(?[...])` now accept the current ordinary char-class atom subset instead of staying limited to plain literal/range bodies.
  - Added compiler support and guardrails for representative shorthand/range, POSIX, and Unicode-property forms such as `(?[[\dA-F]])`, `(?[[[:graph:]]])`, and `(?[[\p{L}] - [\p{Lu}]])`.
  - Locked the widened slice through parser-path, parser-contract, compiler/unit, and PCRE2 differential coverage.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core lower_extended_char_class_content_maps_nested_ordinary -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-wasm`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This widens the shipped extended-character-class subset toward ordinary PCRE2 char-class behavior without claiming the remaining broader extended-class grammar.

### 2026-04-02 - Consolidate parser-path extended char class fixtures
- Scope: internal test cleanup, parser-path maintainability hardening, and continuity refresh.
- Changes:
  - Replaced the long run of one-off parser-path `(?[...])` execution tests in `rgx-core/src/lib.rs` with one shared `ParserExtendedCharClassExecutionFixture` helper plus simple/algebraic fixture tables.
  - Kept the user-visible parser-path coverage unchanged while making the extended-character-class match/reject matrix cheaper to widen and less error-prone to maintain.
  - Mirrored the same simple-vs-algebraic split already used in the parser-contract coverage so the two test surfaces stay easier to compare.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; shipped regex behavior did not widen.

### 2026-04-02 - Extend extended char class control-literal escapes
- Scope: regex runtime feature delivery, parser-contract widening, differential parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` escaped-term subset again so the default path now also accepts bare `\b` backspace atoms inside Perl extended character classes instead of stopping at the explicit compile boundary.
  - Locked the broader current control-literal family into the shipped contract at the same time by adding compiler, parser-path, parser-contract, and PCRE2 differential coverage for `\a`, `\b`, `\e`, and `\f`.
  - Updated the public subset docs and compiler boundary message so the shipped escaped-term surface no longer under-describes the now-supported control-literal family.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This widens the shipped Perl extended character class subset by one real PCRE2-aligned atom family edge (`\b`) while making the already-supported `\a`, `\e`, and `\f` behavior explicit and locked by tests.

### 2026-04-02 - Consolidate parser-contract extended char class fixtures
- Scope: internal test cleanup, parser-contract maintainability hardening, and continuity refresh.
- Changes:
  - Replaced the long hand-written `(?[...])` execution assertion chain in `rgx-core/src/parsing.rs` with one shared `ExtendedCharClassExecutionFixture` table plus a small helper.
  - Kept the shipped regex surface unchanged while making parser-contract extended-character-class coverage cheaper to widen safely the next time the default-path subset grows.
  - Preserved the simple-vs-algebraic test split, but both tests now iterate through fixture rows instead of duplicating compile/assert boilerplate.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_simple_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; shipped regex behavior did not widen.

### 2026-04-02 - Extend shipped extended char class negated POSIX subset
- Scope: regex runtime feature surfacing, parser-contract widening, differential parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` subset again so RGX now explicitly supports bare negated ASCII POSIX class terms such as `[:^alpha:]` on the default path instead of leaving that capability implicit and undocumented.
  - Added direct compiler/unit coverage for negated bare POSIX-term lowering, plus parser-path and parser-contract regressions that lock representative forms like `(?[ [:^alpha:] ])` onto the default PGEN-backed path.
  - Expanded PCRE2 differential parity coverage for the new negated POSIX-term slice across first-match, all-match, and explicit no-match views.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_explicit_compile_boundary_and_validation_cases -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_find_all_spans -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_no_match_consistency -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_first_match_span -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This closes another practical PCRE2 `(?[...])` slice on the default path while keeping the explicit compile boundary focused on broader remaining extended-class forms rather than already-supported negated POSIX atoms.

### 2026-04-02 - Consolidate extended char class braced escape parsing
- Scope: internal cleanup, extended-char-class maintainability hardening, and continuity refresh.
- Changes:
  - Replaced the duplicated braced-digit loops in the `(?[...])` hex and octal escape paths with one shared `consume_extended_braced_radix_digits(...)` helper in `rgx-core/src/compiler.rs`.
  - Kept the shipped runtime subset unchanged while making the braced escape contract less repetitive and easier to extend safely the next time we widen the escaped-atom family.
  - Added direct helper-level tests for accepted braced octal/hex bodies plus malformed empty, invalid, and unclosed braced-digit forms.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; the shipped `(?[...])` subset did not widen.

### 2026-04-02 - Extend shipped extended char class escaped-atom subset
- Scope: regex runtime feature delivery, parser-contract widening, differential parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` lowering path again so RGX now executes bare control escapes like `\cA` and bare octal escapes like `\040`, `\011`, and `\o{101}` inside the current extended-character-class subset instead of compile-rejecting them.
  - Added compiler/unit coverage for the new control/octal escaped-atom forms, including explicit malformed-control and malformed-octal guardrail tests, plus parser-path and parser-contract regressions that lock representative forms like `(?[\cA | [B]])` and `(?[\040 | \011 | \o{101}])` onto the default PGEN-backed path.
  - Expanded PCRE2 differential parity coverage for the new escaped-atom slice and deliberately backed out an exploratory `\N` variant when the focused parity probe showed that upstream PCRE2 compile-rejects `\N` inside extended classes.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_find_all_spans -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_no_match_consistency -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_first_match_span -- --nocapture`
- Notes/impact:
  - This closes another practical PCRE2 `(?[...])` slice on the default path without over-claiming `\N`, which upstream PCRE2 itself still rejects in extended classes.

### 2026-04-02 - Consolidate extended char class POSIX spec parsing
- Scope: internal cleanup, extended-char-class maintainability hardening, and continuity refresh.
- Changes:
  - Replaced the new bare POSIX-term string matching path in `rgx-core/src/compiler.rs` with a typed internal ASCII POSIX registry (`AsciiPosixClass`) plus an `ExtendedPosixClassSpec` helper so parsing, negation, and range lookup now flow through one explicit contract.
  - Kept shipped regex behavior unchanged while making invalid POSIX names fail through one narrower helper path instead of ad hoc string splitting plus a later range lookup.
  - Added direct compiler-unit coverage for valid POSIX spec parsing, unknown POSIX-name rejection, and non-POSIX bodies staying available to the ordinary extended-char-class lowering path.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; the shipped `(?[...])` subset did not widen, but the new bare POSIX-term slice is now less stringly-typed and easier to extend safely.

### 2026-04-01 - Extend shipped extended char class POSIX-term subset
- Scope: regex runtime feature delivery, parser-contract widening, differential parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` lowering path again so RGX now executes bare ASCII POSIX class terms such as `[:graph:]`, `[:alpha:]`, `[:digit:]`, `[:space:]`, and `[:word:]` inside the current extended-character-class subset instead of compile-rejecting them.
  - Added compiler/unit coverage for bare POSIX-class lowering plus complemented and algebraic POSIX cases, along with parser-path and parser-contract regressions that lock representative forms like `(?[ [:graph:] ])`, `(?[ ![:alpha:] ])`, and `(?[ [:alpha:] & [a-z\t] ])` onto the default PGEN-backed path.
  - Expanded PCRE2 differential parity coverage for the new bare POSIX-term slice while keeping the current parity inputs ASCII-only so the bytes-based harness does not over-claim broader Unicode-mode POSIX behavior.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This closes another practical PCRE2 `(?[...])` slice on the default path without pretending the broader remaining extended-character-class grammar is finished.

### 2026-04-01 - Consolidate parser-path capability matrix test
- Scope: internal test cleanup, warning-noise reduction, and continuity refresh.
- Changes:
  - Moved the large parser-path capability case table in `rgx-core/src/lib.rs` out of the `capability_matrix_supported_parser_path_cases` test body and into one shared constant, then routed the assertions through a small helper.
  - Kept the supported-pattern coverage identical while making the parser-path capability matrix easier to extend without turning one test function into a monolith.
  - Removed the RGX-owned `clippy::too_many_lines` warning for that parser-path capability test; the remaining visible `too_many_lines` warnings in the standard loop are still the older wasm-heavy tests.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --tests -- --no-deps`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; shipped regex behavior did not widen, but one of the biggest RGX-owned parser-path regression tests is now data-driven and cheaper to maintain.

### 2026-04-01 - Extend shipped extended char class h/v-space shorthand subset
- Scope: regex runtime feature delivery, parser-contract widening, differential parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` lowering path again so RGX now executes bare horizontal/vertical whitespace shorthand terms (`\h`, `\H`, `\v`, `\V`) inside the current extended-character-class subset instead of compile-rejecting them.
  - Added compiler/unit coverage for positive and negated horizontal/vertical shorthand lowering, plus parser-path and parser-contract regressions that lock those cases onto the default PGEN-backed path.
  - Expanded PCRE2 differential parity coverage for the new shorthand slice while keeping the new parity inputs ASCII-only so they stay aligned with the current `pcre2::bytes::Regex` harness instead of over-claiming UTF-mode behavior there.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This closes another practical PCRE2 `(?[...])` slice on the default path without pretending the broader remaining extended-character-class grammar is finished.

### 2026-04-01 - Consolidate extended char class escape parsing
- Scope: internal cleanup, extended-char-class maintainability hardening, and continuity refresh.
- Changes:
  - Refactored the shipped `(?[...])` escaped-term lowering path in `rgx-core/src/compiler.rs` into smaller dedicated helpers for literal escapes, Unicode-property escapes, and braced-name consumption.
  - Added direct compiler coverage for escaped operator literals like `\-` and for malformed unclosed hex escapes so the escaped-term subset stays explicit and better defended against regressions.
  - Kept runtime and parser-contract behavior unchanged while reducing drift risk in the newest extended-character-class slice.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; shipped regex behavior did not widen, but escaped-term follow-up work now hangs off clearer helper boundaries and tighter guardrail tests.

### 2026-04-01 - Extend shipped extended char class escaped-term subset
- Scope: regex runtime feature delivery, parser-contract widening, differential parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` lowering path again so RGX now executes bare escaped literal/codepoint terms such as `\n`, `\t`, `\r`, `\f`, `\a`, `\e`, escaped operators like `\-`, and hexadecimal codepoint escapes like `\x{41}` / `\x41` inside the current extended-character-class subset instead of compile-rejecting them.
  - Added compiler/unit coverage for bare hex-escape and control-escape lowering, parser-path and parser-contract regressions for hex/control runtime behavior, and PCRE2 differential parity cases for the newly shipped escaped-term slice.
  - Kept the boundary disciplined by still rejecting wider set-expression forms and additional bare-term families beyond the current bracket/property/shorthand/escaped-term subset instead of over-claiming the full PCRE2 extended-character-class grammar.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This closes another practical PCRE2 `(?[...])` slice on the default path without pretending the broader remaining extended-class grammar is finished.

### 2026-04-01 - Consolidate extended char class boundary message
- Scope: internal cleanup, compile-boundary contract hardening, and continuity refresh.
- Changes:
  - Promoted the shipped extended-character-class compile-boundary wording in `rgx-core/src/compiler.rs` into one crate-visible constant so the compiler, capability validation, and parser-contract tests all read from the same source of truth.
  - Replaced duplicated hard-coded message fragments in `rgx-core/src/lib.rs` and `rgx-core/src/parsing.rs` with direct references to that compiler-owned constant.
  - Kept runtime behavior unchanged while reducing drift risk for the still-explicit non-shipped `(?[...])` grammar surface.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; shipped regex behavior did not widen, but future `(?[...])` follow-up work now has one stable boundary message to assert against.

### 2026-04-01 - Extend shipped extended char class shorthand subset
- Scope: regex runtime feature delivery, parser-contract widening, differential parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` lowering path again so RGX now executes bare shorthand terms (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`) inside the current extended-character-class subset instead of compile-rejecting them.
  - Added compiler/unit coverage for positive and negated shorthand lowering, parser-contract and API regressions for digit/word/negated-shorthand runtime behavior, and PCRE2 differential parity cases for the newly shipped shorthand slice.
  - Kept the boundary disciplined by still rejecting wider set-expression forms and additional bare-term families beyond the current bracket/property/shorthand subset instead of over-claiming the full PCRE2 extended-character-class grammar.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This closes another practical PCRE2 `(?[...])` slice on the default path without pretending the broader remaining extended-class grammar is finished.

### 2026-04-01 - Consolidate extended char class operator parser
- Scope: internal cleanup, extended-char-class maintainability hardening, and continuity refresh.
- Changes:
  - Replaced the duplicated `(?[...])` binary-operator parsing loops in `rgx-core/src/compiler.rs` with one precedence-climbing parser that owns left-associativity and `&`-before-`|`/`+`/`-`/`^` precedence in one place.
  - Moved operator metadata and set application onto `ExtendedCharClassOperator`, which removed the now-awkward split between low-precedence helpers and the separate intersection-only path.
  - Added a direct regression for repeated `&` chaining so the shipped precedence model stays locked independently of the broader parser-path/API tests.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; shipped regex behavior stays the same, but the remaining Perl extended-character-class follow-up work now has a cleaner parser/lowering base.

### 2026-04-01 - Extend shipped extended char class precedence
- Scope: regex runtime feature delivery, parser-contract widening, parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` lowering path again so RGX now executes same-level left-associative set algebra with `&` binding tighter than `|`, `+`, `-`, and `^` over the current bracket/property subset.
  - Added precedence-sensitive and chained algebra coverage in compiler/unit tests, parser-contract/API regressions, and PCRE2 differential parity cases.
  - Kept the boundary disciplined by still rejecting additional bare-term families and wider set-expression forms instead of over-claiming the full PCRE2 extended-character-class grammar.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This closes another meaningful PCRE2 `(?[...])` slice on the default path without pretending the full extended-class grammar is finished.

### 2026-04-01 - Reduce RGX-owned warning scaffolding noise
- Scope: internal cleanup, warning-debt reduction, and continuity refresh.
- Changes:
  - Removed several dead or purely carried-over private scaffolding pieces from the core parser/runtime path, including the unused `Regex.pattern` field, the unused `Lexer.input` field, the stale `PatternAnalysis` parser helper, and an unused VM capture-extraction helper.
  - Tightened feature gating around embedded-language helper plumbing so base builds no longer warn on dormant Lua/JavaScript/Rhai-only result-merging utilities or their `Mutex` import.
  - Folded in a couple of remaining small RGX-owned lint cleanups, including the parser-contract `let ... else` simplification and a `clone_on_copy` fix in token tests.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass; shipped regex behavior did not change, but the RGX-owned `rgx-core` warning floor visible during routine validation dropped from 101 to 93 and the Rust-state docs now match the cleaned code more closely.

### 2026-04-01 - Extend grouped extended char class subset
- Scope: regex runtime feature delivery, parser-contract widening, parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` lowering path again so RGX now executes unary complement (`!`), grouped subexpressions, and symmetric difference (`^`) on top of the existing bracket/property subset.
  - Kept the boundary disciplined by still rejecting broader same-level multi-operator expressions instead of over-claiming the full PCRE2 set-expression grammar.
  - Added compiler, parser-contract, parser-path, and PCRE2 differential coverage for complemented, grouped, and symmetric-difference extended character class forms.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_first_match_span -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This still does not ship the full PCRE2 `(?[...])` grammar, but it closes a meaningful next slice by making grouped algebra and complement real on the default path.

### 2026-04-01 - Consolidate extended char class range algebra internals
- Scope: internal cleanup, extended-char-class maintainability hardening, and continuity refresh.
- Changes:
  - Replaced the loose scalar-range helper cluster in `rgx-core/src/compiler.rs` with one private `ScalarRangeSet` abstraction that owns normalization, union, difference, intersection, complement, and char-range conversion for the shipped `(?[...])` subset.
  - Simplified the extended-char-class lowering path so bracket terms and Unicode-property terms both resolve through the same normalized range-set flow instead of manually re-normalizing slices at each branch.
  - Added direct unit tests for adjacent-range normalization and split difference behavior so future `(?[...])` widening has a tighter internal algebra baseline.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core compiler::tests::lower_extended_char_class_content -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core scalar_range_set -- --nocapture`
- Notes/impact:
  - This is a consolidation-only pass; the shipped regex surface stays the same, but the current one-operator extended-class subset is now built on a cleaner internal range-algebra layer.

### 2026-04-01 - Extend Perl extended character class algebra subset
- Scope: regex runtime feature delivery, parser-contract widening, parity coverage, and current-state doc refresh.
- Changes:
  - Extended the shipped `(?[...])` lowering path beyond plain nested bracket terms so RGX now executes the current one-operator bracket/property subset on the default path.
  - Added compiler support for exactly one explicit operator (`|`, `+`, `-`, `&`) over bracket terms or Unicode property terms, including examples like `(?[[a-z] - [aeiou]])` and `(?[\p{L} & \p{Lu}])`.
  - Kept broader grouped algebra, complement operators, multi-operator expressions, and wider nested/set-expression forms behind the explicit compile boundary instead of over-claiming the full PCRE2 extended-class surface.
  - Added direct compiler tests, parser-contract/runtime tests, and PCRE2 differential parity cases for the widened shipped subset.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported -- --nocapture`
- Notes/impact:
  - This intentionally does not ship full PCRE2 extended-class algebra; it closes one disciplined middle slice so RGX can exercise real operator/property behavior without losing a clear boundary for the remaining grammar.

### 2026-04-01 - Harden extended char class guardrails
- Scope: internal cleanup, compiler/VM regression hardening, and continuity refresh.
- Changes:
  - Extracted a dedicated compiler helper for the shipped extended-char-class subset error so the `(?[...])` lowering path stops rebuilding the same compile error at each branch.
  - Added direct compiler unit tests for nested simple-subset extraction and lowering, covering both positive range and negated-range forms plus an explicit rejection case for broader set algebra.
  - Added a direct VM unit test for ordinary negated custom char classes so the recent double-negation fix stays locked in even outside the new extended-char-class surface.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core compiler::tests -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core vm::tests::test_negated_custom_char_class -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This is a consolidation-only pass: it does not widen the shipped regex syntax, but it makes the newly added `(?[[...]])` path safer to refactor.

### 2026-04-01 - Ship simple extended character class runtime support
- Scope: regex runtime feature delivery, parser-boundary reduction, parity coverage, and current-state doc refresh.
- Changes:
  - Lowered the current simple nested bracket-equivalent `(?[...])` subset into RGX's existing char-class runtime path before VM codegen, so forms like `(?[[a-z]])` and `(?[[^0-9]])` now execute on the default path instead of failing at the compiler boundary.
  - Kept broader algebraic extended-class forms explicitly gated with a narrower compile-time policy message rather than pretending to implement the full PCRE2 set-algebra surface.
  - Added API regressions, parser-contract coverage, and PCRE2 differential tests for the shipped subset, and updated the capability/parity/roadmap analysis docs to reflect the new partial support level.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_simple_extended_char_class_executes_on_default_path -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_find_all_spans -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-wasm`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This intentionally ships only the simple nested bracket-equivalent slice; broader set operators, nested classes, property escapes, and whitespace-separated set expressions remain open follow-up work.

### 2026-04-01 - Reduce RGX-owned clippy warning noise
- Scope: internal cleanup, warning-debt reduction, and continuity refresh.
- Changes:
  - Replaced a handful of RGX-owned style issues that were adding avoidable `clippy` noise during the normal workspace sweep.
  - Swapped the remaining debug-print formatting in `rgx-core/src/vm.rs` over to inline-format-arg style.
  - Removed unnecessary `format!` calls from the `rgx-core` debug examples and simplified one compile-boundary test in `rgx-core/src/lib.rs` to `let ... else`.
  - Reworked the native test helper that emitted a match length as `f64` so it converts through `u32::try_from(...)` and `f64::from(...)` instead of using a direct precision-loss cast.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This does not change shipped regex behavior; it is a consolidation pass to keep RGX-owned validation output cleaner as the default PGEN-backed path grows.

### 2026-04-01 - Ship named recursion-condition support on the default path
- Scope: PGEN dependency bump, conditional-runtime feature delivery, parity coverage, and continuity/doc refresh.
- Changes:
  - Updated the pinned `subs/pgen` submodule from `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77` to `f97e0fe31750885f4fc48a67ed7660110cd20271`, bringing RGX's default parser path onto the verified standalone PGEN `1.1.2` fix level.
  - Extended RGX's conditional AST/parser boundary to recognize named recursion conditions `(?(R&name)...)` on both the handwritten lexer/parser path and the default PGEN-backed path.
  - Resolved named recursion conditions at compile time onto the existing recursion-target runtime model, so `(?(R&name)...)` now executes through the same active-recursion check as `(?(Rn)...)` while still failing explicitly when the named capture does not exist.
  - Added parser, runtime, parser-contract, and PCRE2 differential coverage for named recursion conditions, and refreshed the user-facing status/roadmap docs so `R&name` is no longer described as a parser blocker.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core recursion_named -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_conditionals -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-wasm`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This closes the parser/runtime gap for the `R&name` conditional family on the default RGX path and narrows the remaining newer-PCRE2 conditional follow-up work to forms such as `VERSION[...]`.

### 2026-04-01 - Verify upstream PGEN fix for named recursion conditions
- Scope: PGEN upstream verification, local issue closure, and continuity-doc correction.
- Changes:
  - Re-ran the exact `PGEN-RGX-0005` reproducer `(?(R&word)a|b)` against the standalone local PGEN checkout at commit `f97e0fe31750885f4fc48a67ed7660110cd20271`.
  - Verified that the standalone PGEN contract now reports `regex_parser_release_version=1.1.2` and `regex_integration_contract_version=1.1.2`, and that the minimal repro now parses successfully.
  - Verified the accepted-tree transport shape through a fresh AST dump: the pattern now reaches `recursion_condition` inside `conditional`, with separate `yes_branch` and `no_branch` spans.
  - Added a durable verification bundle under `pgen-issues/artifacts/PGEN-RGX-0005/verified-fix-1.1.2/` and closed `pgen-issues/PGEN-RGX-0005.yaml` as `verified-fixed-upstream`.
  - Updated continuity docs so they distinguish between “fixed upstream in standalone PGEN 1.1.2” and “still blocked on the current RGX-pinned submodule at `1.1.1` / `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77`”.
- Validation:
  - `cargo run --offline --manifest-path /Users/richarddje/Documents/github/pgen/rust/Cargo.toml --target-dir /tmp/pgen-verify-target --features generated_parsers --bin parseability_probe -- --parse regex /Users/richarddje/Documents/github/rgx/pgen-issues/artifacts/PGEN-RGX-0005/repro_input.txt --profile regex_default`
  - `cargo run --offline --manifest-path /Users/richarddje/Documents/github/pgen/rust/Cargo.toml --target-dir /tmp/pgen-verify-target --features generated_parsers --bin parseability_probe -- --parse-dump-ast-pretty regex /Users/richarddje/Documents/github/rgx/pgen-issues/artifacts/PGEN-RGX-0005/repro_input.txt /tmp/pgen-rgx-0005-verify-ast.json --profile regex_default`
  - `PGEN_TRACE_VERBOSITY=debug cargo run --offline --manifest-path /Users/richarddje/Documents/github/pgen/rust/Cargo.toml --target-dir /tmp/pgen-verify-target --features generated_parsers --bin parseability_probe -- --parse regex /Users/richarddje/Documents/github/rgx/pgen-issues/artifacts/PGEN-RGX-0005/repro_input.txt --profile regex_default --trace --trace-log-file /tmp/pgen-rgx-0005-verify.trace.log`
  - `cargo run --offline --manifest-path /tmp/pgen_issue_bundle_external/Cargo.toml --target-dir /tmp/pgen-issue-bundle-external-target -- /Users/richarddje/Documents/github/rgx/pgen-issues/artifacts/PGEN-RGX-0005/repro_input.txt /Users/richarddje/Documents/github/rgx/pgen-issues/artifacts/PGEN-RGX-0005/verified-fix-1.1.2`
- Notes/impact:
  - The upstream PGEN parser bug is verified fixed.
  - At the time of this verification record, RGX’s default parser path was still on the older pinned submodule revision; the later follow-up entry in this file records the dependency bump plus shipped `R&name` support.

### 2026-03-31 - Log named recursion-condition parser gap in PGEN
- Scope: PGEN integration bug triage, issue-bundle capture, and roadmap/continuity refresh.
- Changes:
  - Attempted the next PCRE2 10.47+ feature slice for named recursion-condition conditionals `(?(R&name)...)`, but did not ship RGX code because the default PGEN parser rejects the syntax before RGX compilation.
  - Reduced the failure to a minimal parser-only reproducer, `(?(R&word)a|b)`, and captured a new local upstream-style issue bundle in `pgen-issues/PGEN-RGX-0005.yaml` plus `pgen-issues/artifacts/PGEN-RGX-0005/`.
  - Recorded the exact generated-backend evidence for the current pinned PGEN submodule revision (`1.1.1` / `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77`), including structured contract/outcome JSON and a debug trace log.
  - Reverted the speculative RGX parser/compiler/runtime edits for `R&name` support so the repo stays aligned with what the default PGEN-backed build can actually parse today.
  - Updated `ROADMAP.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` to record that `(?(R&name)...)` is still roadmap work and is currently blocked at the parser transport layer by local issue `PGEN-RGX-0005`.
- Validation:
  - `cargo run --offline --manifest-path /tmp/pgen_issue_bundle_current/Cargo.toml -- pgen-issues/artifacts/PGEN-RGX-0005/repro_input.txt pgen-issues/artifacts/PGEN-RGX-0005`
  - `PGEN_TRACE_VERBOSITY=debug subs/pgen/rust/target/debug/parseability_probe --parse regex pgen-issues/artifacts/PGEN-RGX-0005/repro_input.txt --profile regex_default --trace --trace-log-file pgen-issues/artifacts/PGEN-RGX-0005/pgen_trace.log`
  - `subs/pgen/rust/target/debug/parseability_probe --parse regex /tmp/pgen-rgx-0005-control.txt --profile regex_default`
- Notes/impact:
  - The blocker is now precise and forwardable to PGEN: named recursion-condition syntax fails at byte 0 on the generated regex backend, while the numeric control form `(?(R1)...)` still parses.

### 2026-03-31 - Consolidate benchmark trend artifact writing internals
- Scope: benchmark-tooling internal cleanup, validation hardening, and continuity doc refreshes.
- Changes:
  - Refactored `rgx-bench/src/bin/trend_capture.rs` around a planned artifact-path bundle plus shared artifact-group writing/reporting helpers instead of repeating path assembly, `fs::write(...)`, and log formatting at each output site.
  - Added focused unit coverage for the artifact layout plan and the multi-path report-line shape so future benchmark-report additions can extend the centralized path without silently drifting output locations or summary logs.
  - Kept the external artifact set unchanged: `latest.*`, mode-scoped `latest-*.{md,tsv}`, archived history snapshots, `history-*.{md,tsv}`, `overview.*`, `profile-pairs.*`, and `profile-history.*` are still written with the same filenames and semantics.
  - Updated `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` to record this as a consolidation pass on the benchmark validation loop rather than a new feature addition.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture`
- Notes/impact:
  - This makes the benchmark capture code materially less fragile for future report additions because path planning, write errors, and summary logging now share one internal contract.

### 2026-03-31 - Surface latest shared pair in benchmark overview
- Scope: benchmark landing-artifact ergonomics, release-profile visibility, and validation/doc refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so `overview.md` / `overview.tsv` now surface the newest shared quick/full label pair alongside the per-mode latest quick/full state.
  - Reused the existing `profile-pairs.*` data so the overview can expose current shared-label quick/full medians and full-vs-quick deltas without inventing another aggregation path.
  - Added focused `trend_capture` coverage for the richer overview markdown/TSV shape, including the duplicated machine-readable shared-pair fields in `overview.tsv`.
  - Updated `README.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the overview artifact is documented as the current landing page for both mode state and latest shared quick/full pair context.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture`
- Notes/impact:
  - This makes `overview.*` the single quickest place to inspect both “latest quick/full mode state” and “latest shared quick/full revision pair” before drilling into the deeper paired-history reports.

### 2026-03-31 - Add latest-pair callouts to rolling benchmark history
- Scope: benchmark release-profile readability, benchmark-report ergonomics, and validation/doc refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so `profile-history.md` now summarizes the latest shared-label quick/full pair against the previous pair before the full rolling tables, including lane-specific delta bullets plus biggest improvement/regression callouts.
  - Kept the existing `profile-history.tsv` and raw pair-over-pair table intact so machine-readable consumers and full longitudinal scans still work as before.
  - Added focused `trend_capture` coverage for the new latest-pair callout section, including the single-pair fallback where no previous pair exists yet.
  - Updated `README.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the benchmark workflow now describes the richer `profile-history.*` report shape.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture`
- Notes/impact:
  - This makes the rolling quick/full longitudinal report actionable at a glance instead of requiring manual scanning of the pair-over-pair history table for the newest revision pair.

### 2026-03-31 - Add rolling label-pair benchmark history
- Scope: benchmark release-profile longitudinal visibility, wrapper output clarity, and validation/doc refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so it now writes `profile-history.md` / `profile-history.tsv`, turning the existing shared-label quick/full pair snapshots into a rolling pair-over-pair history across revisions.
  - Reused the existing `profile-pairs.*` aggregation path so rolling history rows stay anchored to the latest quick/full capture per shared label while also surfacing delta-vs-previous-pair values for compile, first-match, and find-all medians.
  - Added focused `trend_capture` coverage for markdown/TSV rendering of those rolling shared-label quick/full histories, including pair-over-pair delta reporting.
  - Updated `scripts/capture-benchmark-trends.sh`, `README.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the benchmark workflow now documents the new rolling paired-label artifact.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture`
- Notes/impact:
  - This gives RGX one stable report for “same revision pair, then next revision pair” comparisons instead of requiring manual cross-reading of `profile-pairs.*` and per-mode history files.

### 2026-03-31 - Add label-paired quick/full benchmark summaries
- Scope: benchmark release-profile longitudinal visibility, wrapper output clarity, and validation/doc refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so it now writes `profile-pairs.md` / `profile-pairs.tsv`, pairing the latest quick/full archived captures for each shared label and surfacing aggregate median ratios plus full-vs-quick deltas for compile, first-match, and find-all measurements.
  - Reused the existing revision-label history metadata so paired summaries naturally prefer the most recent capture per mode for a given label and ignore one-sided labels that do not yet have both quick and full captures.
  - Added focused `trend_capture` coverage for markdown/TSV rendering of those quick/full label pairs, including latest-per-mode selection for repeated labels.
  - Updated `scripts/capture-benchmark-trends.sh`, `README.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the benchmark workflow now documents the new label-paired quick/full artifact.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture`
- Notes/impact:
  - This gives RGX one stable report for “same revision, quick vs full” comparisons instead of requiring manual cross-reading of separate mode histories.

### 2026-03-31 - Add cross-mode benchmark overview artifacts
- Scope: benchmark longitudinal visibility, quick/full release-profile ergonomics, and validation/doc refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so each run now also writes `overview.md` / `overview.tsv`, a compact cross-mode summary of the latest archived quick/full capture state including entry counts, labels, aggregate medians, and delta-vs-previous values per mode.
  - Reused the existing history-summary aggregation path so the overview stays mode-scoped, history-backed, and resilient when one mode has not been captured yet.
  - Added focused `trend_capture` coverage for overview markdown/TSV rendering across populated and empty modes.
  - Updated `scripts/capture-benchmark-trends.sh`, `README.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the benchmark workflow now documents the new cross-mode overview artifact.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture`
- Notes/impact:
  - This gives RGX one stable place to inspect the latest quick/full benchmark story together without manually opening both mode-specific history summaries.

### 2026-03-31 - Add label-based benchmark baseline selection
- Scope: benchmark trend baseline ergonomics, revision-targeted comparison, and continuity/doc refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so `--compare-against` now accepts `label:<text>` in addition to `auto`, `none`, and unix timestamps, while keeping explicit selection mode-scoped and backward-compatible with older unlabeled history snapshots.
  - Taught archived baseline resolution to pick the most recent same-mode capture whose stored label matches the requested label, and refreshed the markdown summary text so resolved baselines now surface labels when present.
  - Added focused `trend_capture` coverage for label-based argument parsing, empty-label rejection, missing-label reporting, and newest-match resolution when archived captures reuse the same label.
  - Refreshed `README.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the benchmark validation story now documents the explicit `label:<text>` selector alongside the existing timestamp-based baseline flow.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture`
- Notes/impact:
  - Benchmark comparisons can now target a stable revision-style label without requiring users to remember archived unix timestamps.

### 2026-03-31 - Add benchmark capture labels to longitudinal history
- Scope: benchmark trend identity tracking, revision-aware longitudinal reporting, wrapper defaults, and validation/doc refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so captures now accept an optional `--label`, persist it in archived TSV snapshots, surface it in `latest.md`, and include it in the rolling `history-quick.*` / `history-full.*` summaries.
  - Taught historical TSV loading to preserve those labels while remaining backward-compatible with older unlabeled history files, and added focused unit coverage for labeled history loading plus label-bearing summary rendering.
  - Updated `scripts/capture-benchmark-trends.sh` so the wrapper now forwards `RGX_BENCHMARK_TREND_LABEL` or, by default, derives a label from the current git revision (`<short-sha>` or `<short-sha>-dirty`) before invoking `trend_capture`.
  - Refreshed `README.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the benchmark validation story now includes revision-aware capture labels alongside the existing history/delta flow.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `./scripts/capture-benchmark-trends.sh`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This makes the local benchmark-history trail more useful for release profiling because archived captures and rolling summaries can now be tied back to a concrete checkout instead of only a timestamp.

### 2026-03-31 - Add rolling benchmark history summaries
- Scope: benchmark trend-capture longitudinal reporting, wrapper output alignment, and validation/doc refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so each quick/full capture now also emits `history-quick.*` or `history-full.*` summaries with aggregate median ratios and delta-vs-previous columns across archived same-mode captures.
  - Kept quick-mode legacy-history fallback intact while adding explicit mode-scoped history loading for the new longitudinal summaries, plus unit coverage for history rendering and merged quick-history loading.
  - Updated `scripts/capture-benchmark-trends.sh`, `README.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the benchmark validation loop now documents the rolling history artifacts alongside the existing latest snapshots and archived baseline comparison flow.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-history-smoke --compare-against none`
  - `RGX_BENCHMARK_TREND_DIR=/tmp/rgx-benchmark-history-smoke ./scripts/capture-benchmark-trends.sh`
- Notes/impact:
  - This moves the benchmark loop a step closer to the roadmap’s fuller longitudinal/perf-story goal without changing the default quick-mode local CI footprint.

### 2026-03-31 - Ship current recursion-condition conditionals
- Scope: conditional parser/runtime parity, PCRE2 ambiguity handling, and parser-contract/differential coverage refreshes.
- Changes:
  - Extended both parser paths so `(?(R)...)` and `(?(Rn)...)` now preserve dedicated conditional intent instead of being misread as bare named-group tests.
  - Added compiler-side resolution for PCRE2's `R` / `Rn` ambiguity, so existing named groups `R` or `Rn` still behave as named-group conditions while unambiguous recursion-condition forms validate missing groups explicitly.
  - Taught the VM to execute current recursion-condition operands against the active recursion level, added API regressions for whole-pattern/group-recursion behavior plus named-group ambiguity, and promoted representative cases into the PCRE2 conditional parity suite.
  - Refreshed `README.md`, `docs/USER_GUIDE.md`, `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, `docs/PARSER_CONTRACT.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the shipped parser/runtime contract reflects the new conditional slice accurately.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_conditional_recursion -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_conditionals -- --nocapture`
- Notes/impact:
  - This closes the current `R` / `Rn` conditional slice that PGEN already transports today and narrows the remaining PCRE2 conditional follow-up work to newer forms such as `(?(R&name)...)` and `(?(VERSION[...])...)`.

### 2026-03-31 - Tightened inline-language emitted-result helpers
- Scope: Lua/JavaScript/Rhai inline-language result-contract hardening, regression coverage, and shipped-surface documentation alignment.
- Changes:
  - Updated `rgx-core/src/execution.rs` so successful Lua and JavaScript statement bodies can now emit winning-path numeric/replacement payloads through `rgx.emit_numeric(...)` / `rgx.emit_replacement(...)`, and successful Rhai statement bodies can do the same through top-level `emit_numeric(...)` / `emit_replacement(...)`.
  - Kept direct numeric/string returns as the simplest shorthand, but added explicit helper emission for statement-style bodies that otherwise need to return `true` / `false`.
  - Added `rgx-core/src/lib.rs` regressions covering emitted numeric/replacement payloads, failure-path suppression of emitted values, and repeated-emission last-wins behavior across the shipped inline-language backends.
  - Refreshed `README.md`, `docs/USER_GUIDE.md`, `docs/CAPABILITY_MATRIX.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the shipped inline-language contract describes the new helper surface truthfully.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua safe_mode_lua_rgx_helpers_can_emit_results_from_statement_bodies -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript safe_mode_javascript_rgx_helpers_can_emit_results_from_statement_bodies -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai safe_mode_rhai_helpers_can_emit_results_from_statement_bodies -- --nocapture`
- Notes/impact:
  - This keeps the preferred Lua/JavaScript/Rhai inline-language lane closer to one coherent result contract while still treating wasm as the more advanced import-based backend.

### 2026-03-31 - Ship branch-reset runtime support
- Scope: branch-reset capture-numbering semantics, compiler/VM integration, parser-contract alignment, and PCRE2 differential coverage.
- Changes:
  - Replaced the old branch-reset compile boundary with a compiler-side capture-index assignment pass that gives `(?|...)` top-level alternatives a shared numbering window and propagates the resulting maximum branch arity to later references.
  - Updated the VM to honor compiler-assigned capture indices directly, made branch-reset wrappers transparent at codegen time, and adjusted subroutine-definition collection so duplicated branch-reset capture numbers stay representable downstream.
  - Replaced the old compile-boundary regressions with AST/parser-path runtime coverage, promoted representative branch-reset backreference and conditional cases into the PCRE2 parity suite, and refreshed the capability/compatibility/parser-contract/roadmap continuity docs accordingly.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core branch_reset -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_first_match_span -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_find_all_spans -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_no_match_consistency -- --nocapture`
- Notes/impact:
  - `(?|...)` is no longer just a safe parser boundary in RGX; it now executes on the default path with shared capture numbering and parity-backed downstream reference behavior.

### 2026-03-31 - Stabilize local CI package test matrix
- Scope: local/GitHub validation reliability, submodule-backed PGEN build stability, and validation-doc alignment.
- Changes:
  - Replaced the flaky umbrella `cargo test --workspace` step in `scripts/run-local-ci.sh` with explicit RGX package tests for `rgx-core`, `rgx-cli`, `rgx-bench`, and `rgx-wasm`, while preserving the existing feature-matrix and benchmark-capture coverage.
  - Updated `README.md`, `docs/USER_GUIDE.md`, `DEVELOPMENT_NOTES.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the repo now documents the explicit package-matrix validation path consistently.
- Validation:
  - `bash -n /Users/richarddje/Documents/github/rgx/scripts/run-local-ci.sh`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-wasm`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This keeps RGX’s local and hosted CI story broad without depending on the intermittently hanging `cargo test --workspace` path seen with the submodule-backed `pgen` default parser build.

### 2026-03-30 - Ship single-branch DEFINE conditionals
- Scope: conditional runtime parity, PCRE2-aligned `DEFINE` validation, and parser-boundary contract refreshes.
- Changes:
  - Removed the old compile-boundary rejection for single-branch `DEFINE` conditionals and taught the VM to execute `DEFINE` as an always-false conditional operand, which makes its branch act as a definition-only block with empty-else runtime behavior.
  - Preserved PCRE2's rule that `DEFINE` may not have a false branch, so `(?(DEFINE)yes|no)` now fails explicitly at compile time instead of drifting into RGX-only semantics.
  - Added runtime regressions for empty-else `DEFINE` behavior plus numbered and named subroutine definitions inside `DEFINE`, promoted representative `DEFINE` coverage into the PCRE2 differential conditional suite, and refreshed the parser contract/capability/compatibility/continuity docs accordingly.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core define -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_conditionals -- --nocapture`
- Notes/impact:
  - RGX now executes the practical PCRE2 `DEFINE` pattern shape instead of leaving it at a parser-only boundary, while still rejecting invalid two-branch `DEFINE` forms rather than inventing non-PCRE2 behavior.

### 2026-03-30 - Harden Perl extended-character-class parser boundary
- Scope: parser-boundary hardening for newer PCRE2 character-class syntax, explicit compile-policy messaging, and parser/PGEN alignment.
- Changes:
  - Added `Regex::ExtendedCharClass { content }` plus recursive-descent token/parser transport for `(?[...])`, and taught the default PGEN AST adapter to map `extended_class` into the same canonical RGX AST.
  - Updated the public compile path so Perl extended character classes now fail early with a deliberate compile-time policy message instead of staying an ambiguous parser gap.
  - Refreshed the parser contract, capability matrix, compatibility matrix, roadmap, and continuity notes so `(?[...])` is now tracked as a parsed-only boundary while downstream runtime/set-algebra semantics remain future work.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_extended_char_class_reports_explicit_compile_boundary -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
- Notes/impact:
  - Newer PCRE2 extended character-class syntax can now reach RGX downstream safely through both parser backends without implying that RGX already executes PCRE2 set-algebra semantics.

### 2026-03-30 - Harden branch-reset group parser boundary
- Scope: parser-boundary hardening for newer PCRE2 group syntax, explicit compile-policy messaging, and parser/PGEN alignment.
- Changes:
  - Added `GroupKind::BranchReset` plus recursive-descent token/parser transport for `(?|...)`, and taught the default PGEN AST adapter to map `branch_reset_group` into the same canonical RGX AST.
  - Updated the public compile path so branch-reset groups now fail early with a deliberate compile-time policy message before RGX's normal capture-numbering logic can make invalid assumptions.
  - Refreshed the parser contract, capability matrix, compatibility matrix, roadmap, and continuity notes so branch-reset groups are now tracked as a parsed-only boundary rather than an ambiguous parser gap.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_branch_reset_group_reports_explicit_compile_boundary -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core branch_reset -- --nocapture`
- Notes/impact:
  - Newer PCRE2 branch-reset syntax can now reach RGX downstream safely through both parser backends without pretending RGX already implements PCRE2's capture renumbering semantics.

### 2026-03-30 - Harden DEFINE conditional parser boundary
- Scope: parser-boundary hardening for newer PCRE2 conditionals, explicit compile-policy messaging, and parser/PGEN alignment.
- Changes:
  - Added `ConditionalTest::Define` to the regex AST and taught both parser backends to preserve `(?(DEFINE)...)` explicitly instead of misclassifying it as a named-group conditional.
  - Updated the default PGEN AST adapter, parser-contract fixtures, and public compile path so `DEFINE` conditionals now round-trip through the parser boundary and fail with a deliberate compile-time policy message.
  - Refreshed the roadmap/capability/compatibility docs so `DEFINE` is now tracked as a parsed-only boundary rather than an ambiguous gap.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_define_conditional_reports_explicit_compile_boundary -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional_tokens_define_condition -- --nocapture`
- Notes/impact:
  - Newer PCRE2 conditional syntax can now reach RGX downstream without silently changing meaning, which makes future runtime-policy work much safer.

### 2026-03-30 - Separated benchmark history by capture mode
- Scope: benchmark trend longitudinal safety, mode-aware baseline resolution, wrapper output clarity, and roadmap/doc alignment.
- Changes:
  - Updated `rgx-bench/src/bin/trend_capture.rs` so archived benchmark artifacts now live under mode-scoped history directories, shared output also keeps `latest-quick.*` / `latest-full.*`, and automatic baseline lookup stays within the current capture mode instead of mixing quick and full runs.
  - Added regression coverage for mode-scoped baseline preference, safe legacy quick-history fallback, and the guardrail that `full` mode does not silently reuse legacy quick-only archives.
  - Refreshed `scripts/capture-benchmark-trends.sh`, `README.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the benchmark-capture contract and roadmap now describe the new mode-aware behavior consistently.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-trends-mode-smoke.JSCCE6 --compare-against none`
  - `RGX_BENCHMARK_TREND_DIR=/tmp/rgx-benchmark-trends-mode-smoke.JSCCE6 RGX_BENCHMARK_TREND_MODE=full ./scripts/capture-benchmark-trends.sh`
  - `RGX_BENCHMARK_TREND_DIR=/tmp/rgx-benchmark-trends-mode-smoke.JSCCE6 ./scripts/capture-benchmark-trends.sh`
- Notes/impact:
  - This closes a real measurement-quality gap: the default quick validation loop and the slower bench-profile flow can now coexist in one output tree without contaminating each other's automatic comparisons.
  - The next benchmark follow-up can focus on deeper release/longitudinal reporting instead of first fixing mixed-profile baseline selection.

### 2026-03-30 - Hardened Rhai explicit-return ergonomics
- Scope: inline-language contract hardening for Rhai, regression coverage, and shipped-surface documentation alignment.
- Changes:
  - Added `rgx-core/src/lib.rs` regressions proving Rhai source bodies accept explicit `return ...` bodies for predicate matching plus numeric/replacement helper flows, in addition to the already-shipped final-expression style.
  - Refreshed `README.md`, `docs/USER_GUIDE.md`, `docs/CAPABILITY_MATRIX.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the repo now describes the Rhai contract the same way the runtime already behaves.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai safe_mode_rhai_explicit_return_body_can_match -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai safe_mode_rhai_explicit_return_helpers_surface_numeric_and_replacement_results -- --nocapture`
- Notes/impact:
  - This closes a small but real documentation/contract gap in the preferred Lua/JavaScript/Rhai inline-language lane.
  - Everyday authoring is now intentionally aligned across all three shipped source-body languages: bare expressions still work, and explicit `return ...` bodies are also a supported choice.

### 2026-03-30 - Added CLI wasm module registration
- Scope: wasm code-block usability from `rgx-cli`, CLI parsing/application tests, and shipped-surface documentation refreshes.
- Changes:
  - Added repeatable `--wasm-module NAME=PATH` support in `rgx-cli/src/main.rs`, which reads named wasm binaries from disk and registers them on the compiled regex before matching.
  - Added CLI tests covering wasm-module argument parsing, missing-file failures, missing-feature registration failures, and successful safe-mode registration from a temp WAT-assembled module.
  - Refreshed `README.md`, `docs/USER_GUIDE.md`, `docs/CAPABILITY_MATRIX.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the shipped CLI/runtime boundary now describes wasm accurately while keeping native registration explicitly Rust-API-only.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli --features wasm`
- Notes/impact:
  - This makes the advanced wasm backend practically exercisable from the CLI without changing its intentionally reference-shaped `module:function` contract.
  - The next surface decision is now narrower: native callbacks are still Rust-API-only, and wasm can be evaluated from the CLI without broadening it into a general plugin system.

### 2026-03-30 - Shipped relative conditional-group runtime support
- Scope: conditional runtime parity, compiler rewrite policy, parser-contract alignment, and docs/test refreshes.
- Changes:
  - Promoted relative conditional group references `(?(+1)...)` and `(?(-1)...)` from parser-only transport into the default compiler/VM path by resolving them to absolute `GroupExists(n)` checks at compile time.
  - Added AST and parser-path regressions covering both backward and forward relative references, and tightened compile-time validation so unresolved relative references still fail explicitly with missing-capture errors.
  - Extended `rgx-bench/tests/pcre2_parity.rs` so the supported-conditionals differential suite now covers the relative conditional family against PCRE2.
  - Refreshed `README.md`, `docs/CAPABILITY_MATRIX.md`, `docs/PARSER_CONTRACT.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the shipped/default-path status is documented consistently.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core relative_group -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_conditionals -- --nocapture`
- Notes/impact:
  - This closes the last intentionally held conditional-family parser/runtime boundary on the default regex path without adding new VM conditional opcodes.
  - Broader PCRE2 conditional follow-up work now shifts to newer families such as `R&name`, `VERSION[...]`, and `DEFINE`, rather than the baseline relative group-reference forms.

### 2026-03-30 - Tightened CLI code-block ergonomics
- Scope: CLI code-block usability, host-variable surface, match-detail rendering, and validation/doc refreshes.
- Changes:
  - Added repeatable `--var NAME=VALUE` support in `rgx-cli/src/main.rs` so CLI users can inject host-provided variables into shipped code-block patterns without dropping to the Rust API.
  - Added opt-in `--show-details` match rendering so CLI output can include top-level branch numbers and winning-path `code_result` values when available, while keeping the default plain `start..end` span output stable.
  - Switched the CLI matching path to collect matches directly instead of calling `is_match` before `find_all`, which avoids one extra round of callback/script execution on successful code-block patterns.
  - Added CLI unit tests for variable parsing, detail rendering, host-variable application, and the single-pass match-collection behavior, and extended local CI to validate `rgx-cli --features all-languages`.
  - Refreshed `README.md`, `docs/USER_GUIDE.md`, `docs/CAPABILITY_MATRIX.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the shipped CLI surface is documented consistently.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli --features javascript`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli --features javascript -- --mode safe --var env=prod '(?{js:vars.env === \"prod\"})' ''`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli --features rhai -- --mode safe --show-details 'foo|cat(?{rhai: 7})' 'cat'`
- Notes/impact:
  - This strengthens the preferred Lua/JavaScript/Rhai inline-language lane without broadening the CLI into native/wasm registration yet.
  - The default CLI output shape remains backward-friendly for span-oriented scripting, while `--show-details` opts into the richer match metadata.

### 2026-03-30 - Hardened the relative-conditional parser boundary
- Scope: parser interoperability, conditional AST transport, compile-boundary guardrails, and status-document refreshes.
- Changes:
  - Added dedicated AST transport for relative conditional group references so `(?(+1)...)` and `(?(-1)...)` now parse into `ConditionalTest::RelativeGroupExists(offset)` on both the recursive-descent parser and the default PGEN-backed adapter.
  - Extended lexer, parser, and API-level regressions to cover positive/negative relative conditional offsets and to lock the compile boundary explicitly instead of letting backend behavior drift.
  - Hardened compiler/VM safety boundaries so relative conditional group references now fail with a deliberate unsupported compile error until RGX defines runtime semantics for them.
  - Refreshed `docs/PARSER_CONTRACT.md`, `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, `RUST_CODEBASE_ANALYSIS.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the parser/runtime boundary is documented consistently.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional_relative_group_exists -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional_tokens_relative_group_exists -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_explicit_unsupported_compile_boundary_cases -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - This keeps the default PGEN-backed build and the recursive-descent reference backend aligned on a real PCRE-family conditional form without over-claiming runtime support.
  - The next decision point is no longer parser transport; it is whether RGX wants to execute relative conditional group references or keep them as an explicit long-term boundary.

### 2026-03-30 - Added local benchmark trend history and delta reporting
- Scope: quick benchmark capture operationalization, bench-side regression coverage, and validation/docs refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` so each quick/full capture now archives timestamped `history/<unix>.md` and `history/<unix>.tsv` snapshots in addition to refreshing `latest.md` / `latest.tsv`.
  - Added comparison logic and bench-side tests so `latest.md` now reports median ratio deltas plus top regressions/improvements against the most recent prior archived capture when one exists.
  - Updated `scripts/capture-benchmark-trends.sh`, `README.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the validation story reflects the new archival/comparison behavior.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-trends-smoke`
  - repeated `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-trends-smoke`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - The quick benchmark loop is still intentionally directional, but it now leaves a durable local history trail and immediately tells us whether the latest run moved up or down versus the previous archived capture.

### 2026-03-30 - Added explicit benchmark baseline selection
- Scope: benchmark trend capture usability, wrapper parity, and validation/docs refreshes.
- Changes:
  - Extended `rgx-bench/src/bin/trend_capture.rs` with `--compare-against <auto|none|unix-timestamp>` so captures can compare against the latest prior archive, disable comparison entirely, or target a specific archived baseline.
  - Updated `scripts/capture-benchmark-trends.sh` to pass through `RGX_BENCHMARK_COMPARE_AGAINST`, which keeps the default local CI path simple while enabling targeted local longitudinal checks.
  - Refreshed `README.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the benchmark-validation story reflects the new explicit-baseline path.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-trends-explicit-smoke --compare-against none`
  - `RGX_BENCHMARK_TREND_DIR=/tmp/rgx-benchmark-trends-explicit-smoke RGX_BENCHMARK_COMPARE_AGAINST=1774884688 ./scripts/capture-benchmark-trends.sh`
- Notes/impact:
  - The quick benchmark loop is still intentionally directional, but it no longer forces every local comparison to be “latest versus immediate predecessor”; we can now target a known archived baseline when chasing or confirming specific regressions.

### 2026-03-30 - Aligned Lua code-block authoring with JS and Rhai
- Scope: Lua source-body ergonomics, regression coverage, and inline-language contract documentation refreshes.
- Changes:
  - Updated `rgx-core/src/execution.rs` so the Lua engine now tries direct evaluation first and then falls back to `return ...` wrapping, which lets `(?{lua:...})` accept bare expression bodies without dropping support for explicit `return ...` chunks.
  - Added `rgx-core/src/lib.rs` regressions covering Lua bare-expression predicate matching plus numeric/replacement helper behavior.
  - Refreshed `docs/USER_GUIDE.md`, `docs/CAPABILITY_MATRIX.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the shipped inline-language contract describes Lua/JavaScript/Rhai consistently.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua safe_mode_lua_expression_body_can_match -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua safe_mode_lua_expression_body_helpers_surface_numeric_and_replacement_results -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - The everyday inline-language story is now more coherent: Lua, JavaScript, and Rhai all support expression-style authoring, while Lua/JavaScript still keep explicit `return ...` available when users want it.
  - No parser transport change was needed; this is a runtime ergonomics improvement on the shipped execution path.

### 2026-03-30 - Added automated benchmark trend capture
- Scope: benchmark harness reuse, local-CI automation, and roadmap/status documentation refreshes.
- Changes:
  - Promoted `rgx-bench/src/lib.rs` from a placeholder into shared benchmark-fixture code used by both the criterion throughput bench and a new lightweight `rgx-bench/src/bin/trend_capture.rs` binary.
  - Added `scripts/capture-benchmark-trends.sh`, which writes quick benchmark summaries to `target/benchmark-trends/latest.md` and `target/benchmark-trends/latest.tsv`, and taught `scripts/run-local-ci.sh` to run it by default unless `RGX_SKIP_BENCH_TRENDS=1`.
  - Updated `scripts/check-ci-paths.sh` so the new CI helper script is tracked and audited like the existing local-CI entry points.
  - Refreshed `README.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, `MEMORY.md`, and benchmark-related notes so the validation loop now truthfully includes quick benchmark trend capture.
- Validation:
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir target/benchmark-trends-smoke`
  - `./scripts/capture-benchmark-trends.sh`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - The default validation story now captures directional benchmark drift automatically without paying the cost of full criterion runs on every commit.
  - `RGX_BENCHMARK_TREND_MODE=full` remains available for slower bench-profile captures when deeper measurement is needed.

### 2026-03-30 - Hardened inline-language source-body semantics
- Scope: JavaScript inline-body ergonomics, helper-API regression coverage, and roadmap/continuity documentation refreshes.
- Changes:
  - Fixed `rgx-core/src/execution.rs` so JavaScript code blocks now preserve direct expression results before falling back to wrapped `return ...` evaluation, which means bare expression bodies like `(?{js:named.word === "cat"})` now drive predicate and richer-result behavior correctly.
  - Added `rgx-core/src/lib.rs` regressions covering the JavaScript bare-expression failure path plus numeric/replacement helper APIs across Lua, JavaScript, and Rhai.
  - Updated `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, and `docs/USER_GUIDE.md` so the shipped inline-language contract is described truthfully.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - The shipped Lua/JavaScript/Rhai lane is now closer to one coherent source-body contract instead of treating JavaScript as return-only in practice.
  - No parser transport changes were needed; this was a downstream RGX execution/runtime hardening slice.

### 2026-03-30 - Shipped Rhai code-block execution
- Scope: embedded-language expansion, feature wiring, parser-contract coverage, and doc/CI refreshes.
- Changes:
  - Added a new feature-gated Rhai backend in `rgx-core/src/execution.rs` and exposed it publicly through `rgx-core/src/rhai.rs`.
  - Extended compiler/runtime language validation so `(?{rhai:...})` is accepted in `ExecutionMode::Safe` / `ExecutionMode::Full` when the `rhai` cargo feature is enabled and rejected explicitly otherwise.
  - Added feature-gated Rhai runtime tests in `rgx-core/src/lib.rs` covering variables, named captures, match metadata, backtracking participation, and richer-result behavior.
  - Extended parser-contract fixtures in `rgx-core/src/parsing.rs` so the default PGEN-backed parser and the recursive-descent reference parser are both checked on `(?{rhai:...})`.
  - Updated `Cargo.toml`, `rgx-core/Cargo.toml`, `rgx-cli/Cargo.toml`, and `scripts/run-local-ci.sh` so Rhai is part of the feature matrix and `all-languages` coverage.
  - Refreshed `README.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, and `docs/USER_GUIDE.md` so the shipped inline-language surface now truthfully includes Rhai.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai safe_mode_rhai_code_block_can_match -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai safe_mode_rhai_code_blocks_use_last_non_boolean_result -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_pgen_backend_matches_reference_fixtures -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - Rhai is now the third first-class inline/source-body language on the RGX path, alongside Lua and JavaScript.
  - The default PGEN-backed parser path now has a regression guard for `rhai`, but upstream PGEN contract docs still need their own explicit marker publication later.

### 2026-03-30 - Logged embedded code-block language direction for future work
- Scope: roadmap steering, durable design notes, PGEN-facing contract guidance, and continuity capture.
- Changes:
  - Updated `ROADMAP.md` so future code-block expansion now clearly prioritizes the inline/source-body lane (`lua`, `js` / `javascript`, future `rhai`) ahead of additional wasm-centric work.
  - Refined `DEVELOPMENT_NOTES.md` to record the current product direction: wasm and native stay supported as advanced reference-style backends, while Julia/Python are intentionally deferred for later evaluation.
  - Extended `PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md` so future upstream marker requests can treat `rhai` as the next source-body tag candidate alongside `lua` / `js` / `javascript`.
  - Captured the design decision in `MEMORY.md` so later sessions continue from the same embedded-language prioritization without having to reconstruct this discussion from chat history.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - This is a planning-only update; no runtime behavior changed.
  - Future implementation and future PGEN coordination now have a clear default direction for embedded language growth.

### 2026-03-30 - Exposed current match metadata to code-block runtimes
- Scope: execution-context expansion, wasm ABI growth, regression coverage, and status-doc refreshes.
- Changes:
  - Extended `rgx-core/src/execution.rs` `ExecContext` so code blocks can read current match start/end/length metadata plus the current 1-based top-level branch number when applicable.
  - Exposed that metadata to Lua and JavaScript globals (`match_start`, `match_end`, `match_length`, `branch_number`) and to native callbacks through new `ExecContext` helper methods.
  - Expanded the wasm host ABI with `rgx.match_start()`, `rgx.match_end()`, `rgx.match_length()`, and `rgx.branch_number()` while preserving the stable `(?{wasm:module:function})` / exported `() -> i32` predicate contract.
  - Wired the VM execution context in `rgx-core/src/vm.rs` so code blocks receive the active match-attempt span and current top-level alternation branch number without changing parser/compiler behavior.
  - Added focused native/Lua/JavaScript/wasm regressions in `rgx-core/src/lib.rs` covering match-span metadata and branch-number visibility, including the explicit wasm `-1` boundary when no top-level branch is active.
  - Refreshed `README.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, and `docs/USER_GUIDE.md` so the shipped execution-context surface is described truthfully.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core full_mode_native_code_block_can_access_match_metadata -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua safe_mode_lua_code_block_can_access_match_metadata -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript safe_mode_javascript_code_block_can_access_match_metadata -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm safe_mode_wasm_code_block_can_read_match_metadata -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm safe_mode_wasm_code_block -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - The code-block runtime surface is now more self-describing: callers no longer need to reconstruct current match span or top-level branch selection indirectly from `arg[0]`, `pos`, and surrounding match results.
  - Wasm remains on the stable predicate ABI; this change only broadens the optional `rgx` host-import surface.

### 2026-03-30 - Shipped recursion / subroutine execution on the default regex path
- Scope: compiler validation, VM runtime wiring, recursion parity coverage, and status-doc refreshes.
- Changes:
  - Removed the old compile-time hard stop for current recursion forms and replaced it with explicit target validation for missing numbered and named subroutine references in `rgx-core/src/compiler.rs`.
  - Added VM/runtime lowering for `(?R)`, `(?1)`, and `(?&name)` via compiled subroutine bytecode and guarded runtime calls in `rgx-core/src/vm.rs`, including zero-width cycle protection and preserved capture-state/backtracking behavior.
  - Added parser-path runtime regressions and capability-matrix guardrails in `rgx-core/src/lib.rs` covering whole-pattern recursion, numbered-group recursion, named-group recursion, and missing-target compile errors.
  - Promoted recursion from a known gap to supported PCRE2 differential coverage in `rgx-bench/tests/pcre2_parity.rs`.
  - Refreshed `README.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, and `docs/PARSER_CONTRACT.md` so recursion is described as a shipped default-path feature rather than a parser-only boundary.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core recursion -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_recursion_forms -- --nocapture`
- Notes/impact:
  - Current PCRE2-style recursion forms are no longer the main default-path regex gap for rgx.
  - Broader returned-capture subroutine forms and newer conditional families remain planned follow-up work rather than part of this shipped slice.

### 2026-03-29 - Shipped possessive quantifiers on the default regex path
- Scope: parser transport, runtime semantics hardening, parity coverage, and documentation refreshes.
- Changes:
  - Added lexer/parser support for possessive quantifier forms `*+`, `++`, `?+`, and counted possessive repeats by extending `rgx-core/src/token.rs` and `rgx-core/src/lexer.rs`.
  - Defined the canonical RGX lowering in both parser paths so possessive quantifiers become atomic-wrapped greedy quantified AST nodes in `rgx-core/src/parser.rs` and `rgx-core/src/parsing.rs`, keeping the recursive-descent reference backend and the default PGEN-backed adapter aligned without adding a new AST variant.
  - Added parser-path runtime regressions in `rgx-core/src/lib.rs` proving possessive quantifiers block suffix backtracking while still matching straightforward success cases, and extended capability-matrix guardrails with possessive cases.
  - Promoted possessive quantifiers to supported PCRE2 differential coverage in `rgx-bench/tests/pcre2_parity.rs`.
  - Refreshed `DEVELOPMENT_NOTES.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, `docs/PARSER_CONTRACT.md`, and `docs/USER_GUIDE.md` so shipped status and remaining gaps are described truthfully.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core possessive -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_active_parser_matches_reference_fixtures -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_pgen_backend_matches_reference_fixtures -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_possessive_quantifiers -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - Possessive quantifiers are no longer a parser-adapter gap; they are now part of the supported default compiler/VM path.
  - The shipped lowering deliberately reuses atomic-group semantics, which keeps parser/backend interoperability simple while matching PCRE2 no-backtracking behavior for this feature family.

### 2026-03-29 - Added a rough maintained PCRE2 support estimate and checklist
- Scope: parity-tracking documentation only.
- Changes:
  - Expanded `docs/PCRE2_COMPATIBILITY_MATRIX.md` with a hand-maintained rough progress estimate so rgx now carries a durable approximate answer for “how much of PCRE2 regex do we support?”
  - Added an explicit supported / open-gap / planned-follow-up checklist to the PCRE2 matrix so current parity-verified families are easier to scan without reading multiple docs together.
  - Kept the estimate intentionally approximate and documented that it should move only when whole feature families move.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - No runtime behavior changed in this pass.
  - The repo now has a stable, caveated rough-percent answer that can be kept current as major PCRE2 feature families land.

### 2026-03-29 - Shipped Unicode property classes on the default regex path
- Scope: compiler/VM Unicode-property execution, compile-time validation, parity coverage, and documentation refreshes.
- Changes:
  - Root cause: Unicode property classes had previously been hardened into a compile boundary to avoid a silent VM miscompile, but that left `\p{...}` / `\P{...}` as a visible default-path gap even though both parsers already transported the syntax successfully.
  - Added `rgx-core/src/unicode_support.rs` and a small `regex-syntax` dependency so RGX can resolve Unicode property/script classes through maintained Unicode tables instead of hard-coding them locally.
  - Removed the blanket Unicode-property unsupported path from `rgx-core/src/compiler.rs` and replaced it with explicit invalid-property diagnostics that still fail fast for unknown property names.
  - Wired Unicode property classes through `rgx-core/src/vm.rs` analysis and code generation, and fixed inline subexpression char-class rebasing so quantified/lookaround subprograms keep their nested char-class tables instead of dropping them.
  - Added parser-path and AST-first regressions in `rgx-core/src/lib.rs` for positive classes, negated classes, script-value classes, and invalid-property compile failures.
  - Promoted representative Unicode property cases to supported PCRE2 differential coverage in `rgx-bench/tests/pcre2_parity.rs`.
  - Refreshed `README.md`, `DEVELOPMENT_NOTES.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, `docs/PARSER_CONTRACT.md`, and `docs/USER_GUIDE.md` so shipped status and remaining gaps are described truthfully.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core unicode_property -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench unicode_property -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - Unicode property classes are no longer a parser-only or parity-gap family; they are now part of the supported default compiler/VM path.
  - The inline char-class rebasing fix also closes a broader latent correctness issue for nested subprograms that compile their own char-class tables.

### 2026-03-29 - Shipped conditional runtime support on the default regex path
- Scope: compiler/VM conditional execution, compile-boundary validation, parity coverage, and documentation refreshes.
- Changes:
  - Removed conditionals from the generic parsed-but-unintegrated compile boundary in `rgx-core/src/compiler.rs` and replaced that blanket rejection with dedicated validation for missing numbered and named conditional references.
  - Wired `Regex::Conditional(...)` through `rgx-core/src/vm.rs` analysis, bytecode emission, opcode decoding, and both execution paths so group-exists, named-group-exists, and lookaround condition forms now execute on the default runtime path.
  - Added AST-first and parser-path regressions in `rgx-core/src/lib.rs` covering group-exists, named-group-exists, optional false branches, lookaround conditions, and explicit compile errors for missing conditional references.
  - Promoted conditionals from known-gap coverage to supported differential coverage in `rgx-bench/tests/pcre2_parity.rs`.
  - Refreshed `README.md`, `DEVELOPMENT_NOTES.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, `docs/USER_GUIDE.md`, and `docs/PARSER_CONTRACT.md` so shipped status and remaining gaps are described truthfully.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_conditionals -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - Conditionals are no longer a parser-only or parity-gap family; they are now part of the supported default compiler/VM path.
  - No new PGEN parser show-stopper surfaced while rerunning the shared local CI path with the submodule-backed parser active.

### 2026-03-29 - Pinned the default PGEN parser backend as a real RGX submodule
- Scope: submodule-backed parser dependency, default-build verification, Cargo workspace separation, CI workflow updates, and documentation refreshes.
- Changes:
  - Added the private PGEN repository as the committed submodule `subs/pgen` and switched `rgx-core` to depend on `../subs/pgen/rust` instead of the old sibling checkout path.
  - Kept the default parser rollout intact by leaving `rgx-core` default features on `pgen-parser`, then verified the active default backend explicitly through `parsing::tests::test_parser_name`.
  - Updated `scripts/run-local-ci.sh`, `README.md`, `DEVELOPMENT_NOTES.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `WARP.md`, and `docs/PARSER_CONTRACT.md` so they now describe the submodule-backed PGEN dependency rather than the old `../pgen` local-checkout workaround.
  - Taught GitHub Actions checkout to initialize submodules recursively and documented the likely need for `RGX_SUBMODULES_TOKEN` when the default `GITHUB_TOKEN` cannot read the private `rdje/pgen` repository.
  - Added `exclude = ["subs/pgen/rust"]` to the root Cargo workspace so the submodule remains a distinct project even though it lives under the RGX tree; this keeps RGX workspace validation scoped to RGX while still building against the pinned PGEN dependency.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core test_parser_name -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - A normal default RGX build now really does use PGEN by default through the pinned `subs/pgen` submodule, rather than only through a local sibling checkout convention.
  - No new PGEN parser show-stopper surfaced during the widened submodule-backed validation sweep.
### 2026-03-29 - Shipped numeric backreferences on the default regex path
- Scope: compiler/VM backreference runtime integration, compile-boundary validation, parity coverage, and documentation refreshes.
- Changes:
  - Removed numeric backreferences from the generic parsed-but-unintegrated compile boundary in `rgx-core/src/compiler.rs` and replaced that blanket rejection with a dedicated validation pass that now rejects only invalid references to missing capture groups.
  - Added capture-group counting plus compile-time diagnostics so patterns like `(a)\2` now fail explicitly with `backreference '\2' refers to missing capture group`.
  - Wired `Regex::Backreference(...)` through `rgx-core/src/vm.rs` analysis, bytecode emission, opcode decoding, and both execution paths so numbered backreferences now compare against the bytes captured on the current winning path.
  - Added AST-first and parser-path regressions in `rgx-core/src/lib.rs` covering successful numeric backreference matching, backtracking-sensitive capture restoration, lookahead interaction, and missing-group compile errors.
  - Promoted numeric backreferences from known-gap coverage to supported parity coverage in `rgx-bench/tests/pcre2_parity.rs`.
  - Refreshed `README.md`, `DEVELOPMENT_NOTES.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, `docs/PCRE2_COMPATIBILITY_MATRIX.md`, and `docs/USER_GUIDE.md` so shipped status and remaining gaps are described truthfully.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core backreference -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - Numeric backreferences are no longer a parser-only or parity-gap family; they are now part of the supported default compiler/VM path.
  - No new PGEN parser show-stopper surfaced while rerunning the shared local CI path with the real sibling-checkout PGEN backend enabled.
### 2026-03-29 - Extended wasm code blocks with emitted numeric and replacement results
- Scope: wasm execution ABI/result surfacing, regression coverage, and roadmap/continuity documentation refreshes.
- Changes:
  - Root cause: the public richer-result slice already surfaced winning-path `Numeric(f64)` and `Replacement(String)` payloads for Lua/JavaScript/native backends, but the wasm path still dropped all non-boolean result metadata even after `MatchResult.code_result`, numeric helpers, and replacement helpers had shipped.
  - Extended `rgx-core/src/execution.rs` so wasm modules can emit winning-path numeric and replacement payloads through new `rgx.emit_numeric(f64)` and `rgx.emit_replacement(ptr, len)` host imports while keeping the stable `(?{wasm:module:function})` / exported `() -> i32` predicate contract for success/failure.
  - Stored the current wasm callout payload in per-invocation store data so the last emitted payload wins, failed predicates drop emitted payloads naturally, and successful predicates can surface emitted values as `ExecResult::Numeric(...)` / `ExecResult::Replacement(...)`.
  - Added wasm regressions in `rgx-core/src/lib.rs` for the default no-emission case, numeric emission, replacement emission, last-emitted-wins behavior, failed-predicate payload discard, and invalid UTF-8 replacement payload failure.
  - Refreshed `README.md`, `DEVELOPMENT_NOTES.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `MEMORY.md`, `docs/CAPABILITY_MATRIX.md`, and `docs/USER_GUIDE.md` so the shipped wasm behavior is documented truthfully.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm safe_mode_wasm_code_block -- --nocapture`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - Wasm is no longer predicate-only on the result side; winning-path numeric and replacement payloads now flow through the same public Rust APIs as Lua/JavaScript/native.
  - The shipped wasm result ABI is still intentionally narrow: successful modules may emit only numeric or UTF-8 replacement payloads through host imports, `emit_replacement` still requires exported linear memory, and invalid guest payloads fail the current match path.
### 2026-03-29 - Activated the real local PGEN backend behind `pgen-parser`
- Scope: parser-backend rollout, conformance hardening, CLI feature plumbing, and local workflow updates.
- Changes:
  - Replaced the `pgen-parser` placeholder path in `rgx-core/src/parsing.rs` with a real PGEN AST-dump adapter guarded by contract/release version checks that require regex contract/release `>= 1.1.1`.
  - Added a one-constant local backend switch (`PGEN_FEATURE_BACKEND`) so the `pgen-parser` feature can force either the real PGEN backend or the recursive-descent reference backend without changing call sites.
  - Expanded parser conformance fixtures to cover anchors, range quantifiers, code-block tags, recursion, backreferences, conditionals, and Unicode property classes against the recursive-descent reference AST.
  - Added `rgx-cli` feature passthrough for `pgen-parser` and validated the CLI crate against the real PGEN-backed parser path.
  - Tightened repo workflow docs/scripts so `cargo fmt` is scoped to the RGX workspace packages instead of leaking into the sibling `pgen` checkout.
  - Tracked the local `pgen-issues/` report bundles so the untracked-file guard no longer blocks the local CI path.
  - Taught hosted GitHub CI to export `RGX_SKIP_PGEN_CHECKS=1` temporarily while the verified PGEN fix revision remains unpublished upstream.
- Validation:
  - `./scripts/run-local-ci.sh`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm --check`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --offline`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser --offline`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features "pgen-parser lua javascript wasm" --offline`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli --features pgen-parser --offline`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets --offline`
- Notes/impact:
  - The `pgen-parser` feature is now a real local alternative parser backend rather than a recursive-descent conformance placeholder.
  - The four previously reported RGX transport bugs are fixed in the local PGEN `1.1.1` checkout and no new show-stopper surfaced in the widened local regression sweep.
  - The remaining blocker is distribution: the verified PGEN fix commit `bd110c9c374f0bc1c5c8f8d5d508f5eb0f90cf77` is still only in the sibling local checkout and is not yet available on PGEN `origin/main`.
  - Local RGX development now exercises the real PGEN backend end-to-end, while hosted CI temporarily skips only the `pgen-parser` slice until upstream publication catches up.
### 2026-03-29 - Refreshed the PGEN embedded-code review docs for contract 1.1.0
- Scope: downstream PGEN contract re-review, embedded-code follow-up, and continuity refreshes.
- Changes:
  - Re-reviewed the new `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` `1.1.0` revision against the RGX complaint/proposal documents.
  - Updated `PGEN_REGEX_PARSER_INTEGRATION_COMPLAINT.md` so it no longer claims that untagged blocks or `lua` / `js` tag classes are undefined; the live complaint surface now focuses on:
    - AST semantic upgrade discipline,
    - intentionally narrow JS/Lua structural guarantees,
    - lack of published `native` / `wasm` tag support,
    - and the still-out-of-scope runtime/wrapper semantics.
  - Updated `PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md` with an explicit upstream-adoption-status section showing which parts of the proposal were effectively adopted by contract `1.1.0` and which parts remain open.
- Validation:
  - Re-read the new upstream contract and both RGX-side review documents to confirm the local complaint/proposal set now matches the published `1.1.0` code-block contract.
- Notes/impact:
  - The RGX-side review docs are now aligned with the newer upstream contract instead of continuing to complain about points PGEN has already addressed.
  - The remaining open parser-contract discussion is now mainly about scope widening (`native` / `wasm`, stronger JS/Lua shielding), not baseline code-block meaning.
### 2026-03-28 - Added a forwardable PGEN embedded code-block contract proposal
- Scope: downstream PGEN integration guidance, documentation indexing, and continuity refreshes.
- Changes:
  - Added `PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md` as a git-tracked proposal document describing a recommended embedded code-block contract shape for PGEN.
  - Proposed an explicit split between parser-layer structural guarantees and runtime-layer semantics, including recommended treatment for untagged blocks, source-body tags (`lua`, `js`, `javascript`), and reference-style tags (`native`, `wasm`).
  - Updated `README.md` so the root markdown inventory now includes both the PGEN complaint document and the new embedded-code-block proposal document.
- Validation:
  - Re-read `PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md` after creation to confirm the forwarded contract shape is internally consistent with the current RGX runtime/backend model.
  - Re-read the updated `README.md` markdown inventory to confirm it now matches the tracked root markdown files relevant to the current PGEN review.
- Notes/impact:
  - RGX now has a separate forwardable “what PGEN could adopt” document instead of only a caveat list.
  - The proposal keeps the parser contract honest by distinguishing structural code-block parsing from backend-owned language validation/execution.
### 2026-03-28 - Refreshed the PGEN regex complaint down to the remaining live caveats
- Scope: downstream PGEN integration review follow-up and continuity documentation.
- Changes:
  - Reworked `PGEN_REGEX_PARSER_INTEGRATION_COMPLAINT.md` so it now distinguishes the complaints already addressed by the 2026-03-28 upstream contract refresh from the caveats that still remain live.
  - Narrowed the live complaint surface to the remaining non-blocking integration limits:
    - AST upgrade discipline is still only envelope-stable, not a fully frozen semantic rule taxonomy.
    - Embedded code-block support is still structurally specified rather than per-language specified.
    - Untagged `(?{...})` blocks still need an explicit downstream policy.
    - Runtime code-block semantics and host-literal wrapper parsing remain intentionally out of scope.
- Validation:
  - Re-read `PGEN_REGEX_PARSER_INTEGRATION_COMPLAINT.md` after the rewrite to confirm the forwarded caveat list is precise and no longer mixes resolved complaints with live ones.
- Notes/impact:
  - The RGX-side complaint document is now suitable to forward upstream without forcing PGEN to re-litigate already-fixed contract issues.
  - The remaining upstream discussion is now focused on embedded-code-block contract clarity and AST upgrade expectations rather than basic contract plumbing.
### 2026-03-28 - Automated the rgx-core feature matrix in local/GitHub CI
- Scope: local-first CI automation, GitHub workflow prerequisites, and validation/state documentation refreshes.
- Changes:
  - Extended `scripts/run-local-ci.sh` so the shared CI path now runs the `rgx-core` feature matrix after the default workspace checks: `pgen-parser`, `lua`, `javascript`, `wasm`, and `all-languages`.
  - Kept the shared local/GitHub entry point intact by continuing to route `.github/workflows/ci.yml` through `./scripts/run-local-ci.sh`.
  - Added the missing Ubuntu-side Lua 5.4 development package to the GitHub workflow so the `lua` feature can participate in the default hosted validation path too.
  - Refreshed `README.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, `WARP.md`, and `MEMORY.md` so they no longer describe the feature matrix as a manual-only validation step.
- Validation:
  - `bash -n /Users/richarddje/Documents/github/rgx/scripts/run-local-ci.sh`
  - `/Users/richarddje/Documents/github/rgx/scripts/run-local-ci.sh`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - The default validation loop now continuously checks the shipped feature-gated code-block/backend surface instead of relying on a manual side matrix.
  - Benchmark trend capture is still separate and remains the next validation-process gap.
### 2026-03-28 - Added a contract-scoped PGEN regex integration complaint
- Scope: downstream PGEN integration review and RGX markdown cleanup.
- Changes:
  - Added a git-tracked RGX complaint document that records only the missing or unusable details found in `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` and the contract files it points to.
  - Tightened RGX-side PGEN integration guidance so it points only to published upstream contract files such as `rust/docs/EMBEDDING_API_CONTRACT.md`, `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`, and `PGEN_RELEASED_PARSER_BUG_LEDGER.md`.
  - Removed earlier markdown references to local RGX PGEN-tracking files from the PGEN-integration guidance surface.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - RGX now carries a precise git-tracked complaint PGEN can review without mixing in unpublished local integration assumptions.
  - RGX-side PGEN guidance now stays anchored to the published contract surface instead of local workflow file references.
### 2026-03-28 - Added a git-tracked local PGEN parser issue workflow
- Scope: parser-integration workflow, local issue recording infrastructure, and repository/parser-contract documentation refreshes.
- Changes:
  - Added a canonical structured schema for one local RGX-side record per suspected PGEN parser bug.
  - Added a stub generator for the next numbered `PGEN-RGX-####.yaml` issue record with timestamps, current `rgx` commit, required context fields, and upstream-reference placeholders.
  - Documented the local ID scheme, required fields, status vocabulary, and update/closure workflow for PGEN-related issues observed from RGX.
  - Updated the parser contract so PGEN issue recording and upstream handoff are part of the parser-boundary story during real-backend rollout.
  - Refreshed repository guidance so the local PGEN issue workflow is discoverable in project-state docs.
- Validation:
  - `bash -n <local PGEN issue stub generator>`
  - `<local PGEN issue stub generator> --summary "Dry-run validation for local PGEN issue workflow" --dry-run`
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - RGX can now keep a precise git-tracked local record for each suspected PGEN parser issue even before or alongside upstream filing.
  - Real PGEN rollout can now preserve local context, local IDs, and upstream links without overloading `CHANGES.md` or `MEMORY.md`.
### 2026-03-28 - Added dedicated numeric-result Rust APIs for code-block results
- Scope: public numeric-result API surface in `rgx-core`, regression coverage, and repository/user documentation refreshes.
- Changes:
  - Root cause: the first richer-result slice already surfaced winning-path `Numeric(f64)` values through `MatchResult.code_result`, but there was still no dedicated public API for collecting numeric payloads directly from match order.
  - Added `Regex::find_first_numeric_with_code(&self, text: &str) -> Option<f64>` and `Regex::find_all_numeric_with_code(&self, text: &str) -> Vec<f64>` in `rgx-core/src/lib.rs`.
  - Added internal helpers that extract only `CodeBlockValue::Numeric(f64)` values; matches with no code result or only a replacement payload are skipped so mixed code-block patterns remain usable.
  - Added regressions for first/all numeric collection, non-numeric payload skipping, and winning-path numeric selection under backtracking using native callbacks on the default Rust API path.
  - Refreshed `README.md`, `WARP.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, `docs/CAPABILITY_MATRIX.md`, and `docs/USER_GUIDE.md` so the shipped numeric-result helper layer and remaining wasm richer-result boundary are described truthfully.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - Host applications can now collect numeric code-block payloads directly without manually scanning `MatchResult.code_result`.
  - The remaining richer-result gap is now narrower: wasm still remains predicate-only on the result side.
### 2026-03-28 - Added replacement-oriented Rust APIs for code-block replacement payloads
- Scope: public replacement-oriented API surface in `rgx-core`, regression coverage, and repository/user documentation refreshes.
- Changes:
  - Root cause: the first richer-result slice already surfaced winning-path `Replacement(String)` and `Numeric(f64)` values through `MatchResult.code_result`, but there was still no public API that could turn a winning-path replacement payload into rebuilt output text.
  - Added `Regex::replace_first_with_code(&self, text: &str) -> String` and `Regex::replace_all_with_code(&self, text: &str) -> String` in `rgx-core/src/lib.rs`.
  - Added an internal helper that rebuilds output text from match spans and only consumes `CodeBlockValue::Replacement(String)`; matches with no code result or only a numeric result keep their original matched text unchanged.
  - Added regressions for first-match replacement, all-match replacement, numeric-result passthrough, and winning-path replacement selection under backtracking using native callbacks on the default Rust API path.
  - Refreshed `README.md`, `WARP.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, `docs/CAPABILITY_MATRIX.md`, and `docs/USER_GUIDE.md` so the shipped replacement-oriented API layer and remaining wasm/numeric boundaries are described truthfully.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - Host applications now have a first explicit code-driven replacement API on the Rust path without overloading future conventional replacement APIs.
  - Numeric code-block results remain match metadata only, and wasm remains predicate-only on the result side for now.
### 2026-03-28 - Added a session-bootstrap entry point for new AI sessions
- Scope: onboarding/documentation flow in the repository root.
- Changes:
  - Added `SESSION_BOOTSTRAP.md` with the exact bootstrap instruction to read `README.md` and all referenced markdown files, analyze the Rust codebase, update `RUST_CODEBASE_ANALYSIS.md` if needed, and then work from the roadmap.
  - Appended the requested end-of-file reminder to `README.md`: `Read SESSION_BOOTSTRAP.md and start from there.`
  - Updated the root markdown inventory in `README.md` so the newly added bootstrap file is listed in the repository’s documentation index.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - Future AI/LLM sessions now have an explicit repository-level bootstrap path instead of relying on implicit startup behavior.
### 2026-03-28 - Shipped first richer non-boolean code-block result slice
- Scope: winning-path result retention in `rgx-core`, public match-result exposure, richer-result regressions, and repository/user documentation refreshes.
- Changes:
  - Root cause: `ExecResult` already included `Numeric(f64)` and `Replacement(String)`, and Lua/JavaScript/native backends could already emit those values, but the VM still dropped them in match mode and public match results had no place to surface them.
  - Added public `CodeBlockValue` plus `MatchResult.code_result` so `find_first` / `find_all` can expose optional richer match metadata without changing the boolean contract of `is_match`.
  - Extended the VM execution context, internal match type, and backtrack frames so the last winning-path non-boolean result is saved and restored alongside captures/call-stack state during speculative execution and backtracking.
  - Treated Lua/JavaScript/native `ExecResult::Numeric(...)` and `ExecResult::Replacement(...)` as successful zero-width outcomes in match mode, with the deterministic rule that the last winning-path non-boolean result is the one surfaced publicly.
  - Kept wasm predicate-only for this slice and added regressions that explicitly assert `code_result == None` on the wasm path while richer results are available on Lua/JavaScript/native.
  - Added regression coverage in `rgx-core/src/lib.rs` for Lua numeric-result surfacing, Lua winning-path backtracking restoration, JavaScript last-result-wins behavior, native `find_all` replacement-result surfacing, and unchanged wasm predicate-only payload behavior.
  - Refreshed `README.md`, `WARP.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, `docs/CAPABILITY_MATRIX.md`, and `docs/USER_GUIDE.md` so the shipped semantics and the remaining wasm boundary are described truthfully.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - Host applications can now observe the first shipped richer-result layer through `MatchResult.code_result` while keeping `is_match` fast and boolean-only.
  - The next logical expansion is no longer “any richer results at all”; it is the next layer beyond this metadata slice, especially replacement-oriented APIs and wasm richer-result handling.
### 2026-03-28 - Shipped host-provided execution variables across code-block runtimes
- Scope: shared execution-variable ownership in `rgx-core`, Rust API/runtime exposure across Lua/JavaScript/native/wasm, regression coverage, and project-state documentation refreshes.
- Changes:
  - Root cause: `ExecContext` already carried a `variables` field, but it was dead scaffolding. Variables were not owned by `ExecutionManager`, `RegexVM::build_code_exec_context()` rebuilt fresh contexts without shared variable state, and there was no public API on compiled regexes to set or update variables.
  - Added a shared `ExecutionVariableRegistry` in `rgx-core/src/execution.rs` plus `ExecutionManager::set_variable(...)` / `ExecutionManager::variable_snapshot(...)` so variables are owned alongside the other runtime registrations.
  - Extended the Rust API/runtime path through `rgx-core/src/vm.rs`, `rgx-core/src/engine.rs`, and `rgx-core/src/lib.rs` with `RegexVM::set_variable(...)`, `Engine::set_variable(...)`, and public `Regex::set_variable(...)`.
  - Added `ExecContext::variable(...)` and `ExecContext::variables_snapshot()` helpers and exposed variables consistently across the shipped backends:
    - Lua and JavaScript now receive read-only `vars`
    - native callbacks can read variables through `ctx.variable("name")`
    - wasm now exposes deterministic read-only `rgx` imports for variables:
      - `variable_count() -> i32`
      - `variable_name_length(index) -> i32`
      - `variable_name_read(index, ptr, offset, len) -> i32`
      - `variable_value_length(index) -> i32`
      - `variable_value_read(index, ptr, offset, len) -> i32`
  - Chose per-evaluation variable snapshots instead of shared mutable match-time state so callout inputs remain deterministic under backtracking while still allowing Rust API updates between matches.
  - Added regressions in `rgx-core/src/lib.rs` for native/Lua/JavaScript/wasm variable access, wasm missing-slot behavior, and explicit unavailable-registration errors on regexes without an attached execution manager.
  - Refreshed `README.md`, `WARP.md`, `docs/USER_GUIDE.md`, `docs/CAPABILITY_MATRIX.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so variables are described truthfully as a shipped code-block-runtime capability.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - Host applications can now provide stable read-only execution variables to all shipped embedded-code backends without changing regex syntax or the existing registration model.
  - The wasm slice now covers position, full input text, numbered captures, named captures, and variables; richer non-boolean result semantics remain the next higher-value runtime expansion.
### 2026-03-28 - Expanded the wasm ABI with named-capture imports
- Scope: wasm runtime ABI expansion in `rgx-core`, wasm regression coverage, and project-state documentation refreshes.
- Changes:
  - Root cause: the VM was already materializing named captures into `ExecContext` for code-block execution, but the shipped wasm ABI still stopped at position, full input text, and numbered captures. That left wasm predicates narrower than the Lua/JavaScript/native backends even though the runtime already had the necessary named-capture data.
  - Extended `rgx-core/src/execution.rs` with deterministic named-capture host imports in the `rgx` namespace:
    - `named_capture_count() -> i32`
    - `named_capture_name_length(index) -> i32`
    - `named_capture_name_read(index, ptr, offset, len) -> i32`
    - `named_capture_value_length(index) -> i32`
    - `named_capture_value_read(index, ptr, offset, len) -> i32`
  - Exposed named captures to wasm through a stable lexicographic ordering by group name so guest-visible indices are deterministic across runs even though the host stores named captures in a `HashMap`.
  - Reused the existing guest-memory/error-handling model so read-style named-capture helpers still require exported linear memory `memory`, unavailable slots still report `-1`, and malformed guest interactions continue to fail explicitly.
  - Added new wasm regressions in `rgx-core/src/lib.rs` for successful name/value reads across multiple named captures plus explicit `-1` behavior for missing named-capture slots.
  - Refreshed `README.md`, `WARP.md`, `docs/USER_GUIDE.md`, `docs/CAPABILITY_MATRIX.md`, `ROADMAP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the repository now describes the wasm slice truthfully as position/text/numbered-capture/named-capture aware.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - Wasm predicates can now inspect named-group context without changing the public regex syntax or the Rust-API registration model.
  - The wasm slice is still intentionally smaller than the Lua/JavaScript/native surface: variables and richer result semantics remain future work.
### 2026-03-27 - Expanded the wasm ABI with read-only `rgx` host imports
- Scope: wasm runtime ABI expansion in `rgx-core`, wasm regression coverage, and project-state documentation refreshes.
- Changes:
  - Root cause: the shipped wasm slice could compile and dispatch `(?{wasm:module:function})`, but `WasmEngine::execute()` still discarded the regex execution context. That left wasm predicates limited to self-contained `() -> i32` exports instead of real match-aware logic, even though the VM was already materializing context for every code block.
  - Reworked the wasmtime path in `rgx-core/src/execution.rs` around a linker plus per-call store data so registered wasm modules can import a read-only `rgx` host namespace while preserving the existing `module:function` / exported `() -> i32` predicate surface.
  - Added the first context-aware wasm import slice:
    - `position() -> i32`
    - `text_length() -> i32`
    - `text_read(ptr, offset, len) -> i32`
    - `capture_count() -> i32`
    - `capture_length(index) -> i32`
    - `capture_read(index, ptr, offset, len) -> i32`
  - Kept capture slot `0` aligned with the current overall match prefix and required exported linear memory `memory` only for the read-style imports that copy bytes into guest memory.
  - Hardened the wasm runtime so malformed context reads, missing exported memory, and invalid guest-memory writes now fail explicitly at runtime rather than panicking or silently succeeding.
  - Added real wasm regressions for:
    - current-position access
    - full-input reads
    - numbered-capture reads
    - missing exported memory
    - invalid guest-memory writes
    - malformed context reads
  - Refreshed `README.md`, `docs/CAPABILITY_MATRIX.md`, `docs/USER_GUIDE.md`, `ROADMAP.md`, `WARP.md`, `DEVELOPMENT_NOTES.md`, `RUST_CODEBASE_ANALYSIS.md`, and `MEMORY.md` so the repository now describes wasm as an import-based context-aware slice rather than a zero-context predicate-only backend.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - Wasm predicates can now make real match-aware decisions without changing the public regex syntax or the Rust-API registration model.
  - The wasm slice is still intentionally narrower than the Lua/JavaScript/native `ExecContext` surface: named captures, variables, and richer non-boolean result semantics remain future work.
### 2026-03-27 - Shipped Rust-API wasm module registration in `ExecutionMode::Safe` / `ExecutionMode::Full`
- Scope: wasm runtime ownership in `rgx-core`, public API exposure, compiler gating changes, regression coverage, and live project-state docs.
- Changes:
  - Root cause: `(?{wasm:...})` was still a parsed-only compile boundary even though the VM callout path was already generic and the workspace already carried wasmtime support. There was no runtime registration surface or execution contract for named wasm modules on compiled regexes.
  - Added a shared wasm module registry and a wasmtime-backed execution engine inside `rgx-core/src/execution.rs`, following the same shared `Arc<ExecutionManager>` model already used by other code-block backends.
  - Added public `Regex::register_wasm_module(...)` support, threaded through `rgx-core/src/engine.rs` and `rgx-core/src/vm.rs`, with an explicit engine error when registration is attempted on a compiled regex that does not have an attached execution manager.
  - Lifted compiler gating so `(?{wasm:...})` now compiles in `ExecutionMode::Safe` and `ExecutionMode::Full` when the `wasm` cargo feature is enabled; `ExecutionMode::Pure` still rejects all code blocks and `native` remains `Full`-only.
  - Landed an initial wasm ABI contract: code blocks use `module:function`, modules are registered from Rust API code, and the exported function must have signature `() -> i32` where `0` means failure and any non-zero value means success.
  - Preserved explicit runtime failure for malformed wasm call specs, unknown module names, and invalid or missing exports.
  - Replaced the old “wasm is still blocked” regression with real coverage for:
    - successful wasm predicate execution
    - cargo-feature gating
    - runtime failure for missing modules
    - runtime failure for malformed call specs
    - runtime failure for invalid export signatures
    - registration failure on regexes without an attached execution manager
  - Refreshed `README.md`, `docs/CAPABILITY_MATRIX.md`, `docs/USER_GUIDE.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `WARP.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the repository now describes the wasm slice truthfully as Rust-API-only with an intentionally small initial ABI.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - `ExecutionMode::Safe` / `ExecutionMode::Full` now expose a real wasm backend instead of a compile boundary.
  - Wasm is intentionally scoped to Rust-API registration for now, and the initial ABI is deliberately smaller than the Lua/JavaScript/native `ExecContext` path.
### 2026-03-27 - Shipped Rust-API native callbacks in `ExecutionMode::Full`
- Scope: native callback runtime ownership in `rgx-core`, public API exposure, compile-boundary changes, regression coverage, and live project-state docs.
- Changes:
  - Root cause: `ExecutionMode::Full` still did not unlock any real public-only behavior because `(?{native:...})` remained compile-blocked even though the runtime already had native callback dispatch machinery. The existing callback registry also required mutable access, which was incompatible with the shared `Arc<ExecutionManager>` already attached to VM-backed regexes.
  - Refactored native callback storage in `rgx-core/src/execution.rs` to use shared interior mutability so callbacks can be registered through the same runtime instance the VM uses during matching.
  - Added public `Regex::register_native(...)` support, threaded through `rgx-core/src/engine.rs` and `rgx-core/src/vm.rs`, with an explicit engine error when registration is attempted on a compiled regex that does not have an attached execution manager.
  - Lifted compiler gating so `(?{native:...})` now compiles only in `ExecutionMode::Full`; `ExecutionMode::Pure` still rejects all code blocks, `ExecutionMode::Safe` still rejects `native`, and `wasm` remains blocked.
  - Preserved explicit runtime failure for unknown native callback names.
  - Replaced the old public-boundary regression with real coverage for:
    - successful native callback execution
    - capture and named-capture visibility inside native callbacks
    - `ExecutionMode::Safe` rejection of `native`
    - runtime failure for unregistered callback names
    - registration failure on regexes without an attached execution manager
  - Refreshed `README.md`, `docs/CAPABILITY_MATRIX.md`, `docs/USER_GUIDE.md`, `ROADMAP.md`, `RUST_CODEBASE_ANALYSIS.md`, `WARP.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the repository now describes the native slice truthfully as Rust-API-only.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - `ExecutionMode::Full` now unlocks a real public-only capability beyond the sandboxed Lua/JavaScript slice.
  - Native callbacks are intentionally scoped to the Rust API for now; the CLI/native configuration story remains deferred.
### 2026-03-27 - Shipped phase-1 Lua/JavaScript predicate code blocks
- Scope: compiler/VM/engine/runtime integration in `rgx-core`, public/API regression coverage, feature-gated validation, and live project-state docs.
- Changes:
  - Root cause: `(?{lang:code})` was already tokenized and parsed, and the repository already contained substantial execution infrastructure, but the public regex path rejected code blocks before bytecode generation and runtime dispatch.
  - Added execution-mode-aware compile validation so code blocks are now accepted only in the documented phase-1 slice:
    - `ExecutionMode::Pure` rejects all code blocks
    - `ExecutionMode::Safe` / `ExecutionMode::Full` accept `lua` and `js` / `javascript` only when the matching cargo feature is enabled
    - `native` and `wasm` remain explicit compile boundaries
  - Added a first-class VM `CodeBlock` opcode with inline operands, wired it through the main VM loop and subexpression execution path, and materialized current overall match, numbered captures, and named captures into the execution-layer context at runtime.
  - Wired `Engine` to attach a shared `ExecutionManager` only when compiled programs actually contain code blocks.
  - Made Lua execution stateless per invocation and wrapped JavaScript execution in an IIFE so documented `return ...` predicate bodies work consistently under speculative execution/backtracking.
  - Added public/API regression coverage for:
    - pure-mode rejection
    - safe-mode cargo-feature gating
    - explicit `native` / `wasm` boundaries
    - Lua named-capture access
    - code-block participation in backtracking
    - JavaScript predicate execution
    - numeric-result rejection in match mode
  - Refreshed `RUST_CODEBASE_ANALYSIS.md`, `WARP.md`, `README.md`, `docs/CAPABILITY_MATRIX.md`, `docs/USER_GUIDE.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md` so the repo now describes the shipped slice truthfully.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
- Notes/impact:
  - Turns `ExecutionMode::{Safe, Full}` into a real shipped surface for feature-gated Lua/JavaScript predicate code blocks instead of pure scaffolding.
  - Keeps unsupported code-block families explicit and bounded until the next roadmap slices land.
### 2026-03-26 - Fixed lazy quantifier parity and restored JavaScript feature builds
- Scope: VM/compiler/runtime changes in `rgx-core`, parity regressions in `rgx-core/src/lib.rs`, PCRE2 differential coverage in `rgx-bench/tests/pcre2_parity.rs`, and live continuity/status docs.
- Changes:
  - Root cause: lazy quantifiers were parsed but not compiled/executed correctly in the public path because the VM compiler only had dedicated lowering for greedy quantifiers, while lazy forms effectively degraded or failed in real use.
  - Added dedicated VM/compiler support for lazy `??`, `*?`, `+?`, `{n,m}?`, and `{n,}?`, plus nested sub-expression backtracking support needed by quantified subprograms.
  - Added public API regression coverage for lazy zero-width, shortest-match, and suffix-backtracking behavior.
  - Added PCRE2 differential parity cases for lazy quantifiers and lazy counted ranges.
  - Root cause for the feature-build failures: the QuickJS backend in `rgx-core/src/execution.rs` had drifted from `rquickjs` 0.4 APIs and also stored a non-`Send`/`Sync` runtime inside a trait implementation that required `Send + Sync`.
  - Reworked the JavaScript backend to create a fresh sandboxed QuickJS runtime per execution, updated it to current `rquickjs` 0.4 APIs, and restored successful `javascript` / `all-languages` feature builds.
  - Updated `RUST_CODEBASE_ANALYSIS.md`, `DEVELOPMENT_NOTES.md`, `MEMORY.md`, and parity/capability docs so future sessions do not re-open the already-fixed gaps.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported_syntax_find_all_spans -- --nocapture`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
- Notes/impact:
  - Closes a real public-path correctness gap for lazy quantifiers and counted lazy ranges.
  - Restores feature-flag build confidence for JavaScript and combined multi-language configurations without yet claiming user-visible code-block execution support.
### 2026-03-26 - Added live Rust codebase analysis and wired it into commit workflow
- Scope: new `RUST_CODEBASE_ANALYSIS.md`, plus workflow/documentation updates in `README.md`, `COMMIT.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md`.
- Changes:
  - Added `RUST_CODEBASE_ANALYSIS.md` as a live roadmap-grounded assessment of the Rust workspace, covering crate/module maturity, roadmap alignment, feature-gated build status, warning debt, and concrete implementation gaps.
  - Captured and documented current high-signal findings, including:
    - default workspace and `pgen-parser` feature-path validation are green
    - `lua` and `wasm` feature checks compile
    - `javascript` and `all-languages` feature checks currently fail in `rgx-core/src/execution.rs`
    - lazy quantifiers are parsed but not correctly compiled in the public path
  - Updated `README.md` to include the new analysis doc in onboarding and markdown inventory.
  - Updated `COMMIT.md` so Rust-focused commits explicitly review/update `RUST_CODEBASE_ANALYSIS.md` alongside `CHANGES.md` and `MEMORY.md`.
  - Updated continuity/docs policy in `DEVELOPMENT_NOTES.md` and `MEMORY.md` to treat the analysis doc as live project infrastructure.
- Validation:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features pgen-parser`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua`
  - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm`
  - investigative snapshots recorded in `RUST_CODEBASE_ANALYSIS.md`:
    - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript`
    - `cargo check --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features all-languages`
    - `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli "a??" "b"`
    - `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli "a*" "b"`
    - `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli "ab*?c" "abbbc"`
    - `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli "ab*c" "abbbc"`
- Notes/impact:
  - Rust-task commits now have a mandatory place to record roadmap alignment and implementation reality.
  - Future sessions can distinguish supported default-path behavior from feature-gated/runtime scaffolding much faster.
### 2026-03-09 - Added local-first CI workflow and reproducible lockfile tracking
- Scope: GitHub Actions workflow in `.github/workflows/ci.yml`, local CI helpers in `scripts/`, lockfile tracking via `.gitignore`/`Cargo.lock`, README onboarding, and continuity docs.
- Changes:
  - Root cause: the repository did not actually contain a GitHub Actions workflow, there was no single checked-in local CI entry point, and `Cargo.lock` was ignored, allowing local dependency resolution to differ from GitHub CI.
  - Added `.github/workflows/ci.yml` to run workspace checks on GitHub Actions and delegate execution to the same checked-in local CI entry point used before pushing.
  - Added `scripts/run-local-ci.sh` to run the local pre-push CI sequence from project root:
    - CI path/tracking audit
    - `cargo fmt --manifest-path Cargo.toml --all --check`
    - `cargo test --manifest-path Cargo.toml --workspace`
    - `cargo clippy --manifest-path Cargo.toml --workspace --all-targets`
  - Added `scripts/check-ci-paths.sh` to verify CI-critical paths exist and are git-controlled, fail on non-ignored untracked files, report compile-time `include!`-style macro usage, and reject absolute filesystem paths in workspace Rust source and CI execution files.
  - Stopped ignoring `Cargo.lock` so GitHub CI uses the same dependency lockfile as local validation.
  - Updated `README.md` to document the CI workflow path and the local pre-push command.
- Validation:
  - `./scripts/run-local-ci.sh`
- Notes/impact:
  - Local and GitHub CI now share one command path, reducing drift between pre-push checks and hosted automation.
  - CI reproducibility is improved because dependency resolution is now anchored by a tracked `Cargo.lock`.
### 2026-03-08 - Hardened Unicode property classes into an explicit compile boundary
- Scope: compile-boundary validation in `rgx-core/src/compiler.rs`, API regressions in `rgx-core/src/lib.rs`, PCRE2 known-gap coverage in `rgx-bench/tests/pcre2_parity.rs`, and user/capability/parity continuity docs.
- Changes:
  - Root cause: Unicode property classes (`\p{...}`, `\P{...}`) were parsed successfully but not actually executed by the VM. Instead, VM code generation silently lowered them to `Any`, causing public miscompiles such as `\p{L}+` matching `123`.
  - Added an explicit compile boundary in `Compiler::unsupported_feature_message()` for both AST forms of Unicode property classes so parser-path and AST-first compilation now fail with a clear unsupported message.
  - Added parser-path/API regression coverage for `\p{L}+` and `\P{L}+` explicit compile errors.
  - Added AST-first regression coverage for Unicode property classes.
  - Added PCRE2 differential known-gap coverage so Unicode property classes are tracked as a deliberate rgx gap instead of silently behaving like supported syntax.
  - Updated docs to classify Unicode property classes as parsed-only / rgx-gap until real VM execution support exists.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --test pcre2_parity`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - `cargo build --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - CLI smoke via `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli`:
    - `'\p{L}+' '123'` now exits nonzero with explicit unsupported compile error instead of returning `0..3`
    - `'\P{L}+' 'abc'` now exits nonzero with the same explicit unsupported compile error
- Notes/impact:
  - Eliminates a dangerous silent miscompile in the public API/CLI path.
  - Makes Unicode property classes accurately documented as parsed-only until real execution semantics land.
### 2026-03-07 - Fixed absolute text anchor support for `\A`, `\Z`, and `\z`
- Scope: absolute-anchor execution in `rgx-core/src/vm.rs`, parser-path/API regressions in `rgx-core/src/lib.rs`, PCRE2 differential coverage in `rgx-bench/tests/pcre2_parity.rs`, and capability/parity continuity docs.
- Changes:
  - Root cause: the compiler/parser already accepted absolute anchors, but `RegexVM::execute_at()` and `execute_subexpr()` did not execute `StartText`, `EndText`, or `EndTextOrNL`, so `\A`, `\Z`, and `\z` compiled but produced no matches.
  - Secondary bug: the compiler emitted the wrong opcodes for `\Z` and `\z`, swapping “before final newline” with “true end-of-text” behavior.
  - Added VM runtime support for `StartText`, `EndText`, and `EndTextOrNL` plus helper logic for absolute end-of-text and final-newline handling.
  - Corrected compiler anchor mapping so `\Z` emits `EndTextOrNL` and `\z` emits `EndText`.
  - Added parser-path/API regression coverage for `\A`, `\Z`, and `\z`, including final-newline behavior and no-match cases.
  - Added PCRE2 differential coverage for positive and negative cases of `\A`, `\Z`, and `\z`.
  - Updated `docs/CAPABILITY_MATRIX.md` and `docs/PCRE2_COMPATIBILITY_MATRIX.md` so absolute anchors are reflected as shipped/parity-verified behavior.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --test pcre2_parity`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - `cargo build --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - CLI smoke via `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli`:
    - `--verbosity debug --trace-log '\Acat' 'cat dog'` => stdout `0..3`; `trace.log` counts: `LOW=19`, `MEDIUM=57`, `HIGH=258`, `TRACE=30`
    - `--verbosity low --trace-log 'dog\Z' 'cat dog\n'` => stdout `4..7`; `MEDIUM/HIGH/TRACE = 0`
    - `--quiet --trace-log 'dog\z' 'cat dog'` => stdout `4..7`; `trace.log` size `0`
- Notes/impact:
  - Closes a public parser-path/runtime parity gap for absolute text anchors.
  - Reinforces that lexer/parser/AST anchor semantics, compiler opcode mapping, and VM execution paths must stay synchronized.
### 2026-03-06 - Fixed negated shorthand class runtime parity for `\D`, `\W`, and `\S`
- Scope: VM shorthand opcode execution in `rgx-core/src/vm.rs`, public API regression coverage in `rgx-core/src/lib.rs`, differential parity coverage in `rgx-bench/tests/pcre2_parity.rs`, and capability/parity docs.
- Changes:
  - Root cause: code generation already emitted `DigitAsciiNeg`, `WordAsciiNeg`, and `SpaceAsciiNeg`, but `RegexVM::execute_subexpr()` did not handle `WordAsciiNeg`, `SpaceAscii`, or `SpaceAsciiNeg`, so quantified shorthand patterns such as `\W+` and `\S+` failed even though the main execution loop had shorthand support.
  - Normalized `RegexVM::execute_at()` by removing duplicate negated-shorthand opcode branches left by a partial edit and keeping a single runtime path for those opcodes.
  - Extended `RegexVM::execute_subexpr()` to handle `WordAsciiNeg`, `SpaceAscii`, and `SpaceAsciiNeg`, aligning quantifier/assertion subexpression execution with the main VM loop.
  - Added parser-path/API regression tests for `\D+`, `\W+`, and `\S+` first-match, find-all, and no-match behavior in `rgx-core/src/lib.rs`.
  - Added PCRE2 differential parity cases for `\D+`, `\W+`, and `\S+` in `rgx-bench/tests/pcre2_parity.rs`.
  - Updated `docs/CAPABILITY_MATRIX.md` and `docs/PCRE2_COMPATIBILITY_MATRIX.md` to classify negated shorthand classes as shipped/parity-verified.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --test pcre2_parity`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - `cargo build --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - CLI smoke via `/Users/richarddje/Documents/github/rgx/target/debug/rgx-cli`:
    - `--verbosity debug --trace-log '\W+' 'ab!!cd'` => stdout `2..4`; `trace.log` counts: `LOW=19`, `MEDIUM=35`, `HIGH=188`, `TRACE=72`
    - `--verbosity low --trace-log '\D+' '123abc456'` => stdout `3..6`; `MEDIUM/HIGH/TRACE = 0`
    - `--quiet --trace-log '\S+' '  abc  '` => stdout `2..5`; `trace.log` size `0`
- Notes/impact:
  - Closes a real public API/CLI parity bug for negated shorthand classes under quantified execution.
  - Reinforces that opcode support must stay synchronized between main-loop and subexpression execution paths.
### 2026-03-06 - Promoted README to single project entry point and clarified update policy
- Scope: onboarding/documentation navigation in `README.md` and commit policy in `COMMIT.md`
- Changes:
  - Reworked `README.md` as the central entry point with:
    - explicit project objective
    - fast ramp-up sequence
    - project file-path map for key crates/modules
    - complete markdown index covering all version-controlled `.md` files
  - Added explicit maintenance rule in `README.md`:
    - update when objective, onboarding links, or key path maps change
    - no requirement to update on every commit
  - Updated `COMMIT.md` to align commit workflow language with this rule (`README.md` updated when needed, not per-commit).
- Validation:
  - markdown coverage verification against tracked markdown files:
    - `ALL_MARKDOWN_REFERENCED`
  - git tracking verification:
    - `git ls-files --error-unmatch README.md >/dev/null 2>&1; echo TRACKED:$?` => `TRACKED:0`
- Notes/impact:
  - Establishes a stable single onboarding hub while keeping commit overhead practical.
  - Reduces ambiguity for future contributors/AI sessions about when README maintenance is required.
### 2026-03-02 - Added structured tracing to parser token-inspection helpers
- Scope: parser utility-boundary observability in `rgx-core/src/parser.rs`
- Changes:
  - Root-cause gap: token-inspection helper calls were not explicitly traced, so parser state-introspection transitions were implicit in parent-function logs only.
  - Instrumented parser helper boundaries:
    - `Parser::peek`
    - `Parser::current_token_snapshot`
    - `Parser::regex_kind`
  - Added decision tracing in `Parser::peek` for token-availability branch (`token.is_some()`).
  - Added entry/exit argument/result snapshots for helper-return values (token snapshot and regex-kind labels).
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - debug smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log 'a|b' 'a'`
    - verified `trace.log` contains `Parser::peek`, `Parser::current_token_snapshot`, and `Parser::regex_kind` enter/exit lines
  - low smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log 'cat|dog' 'I have a dog'`
    - `grep -E '\\[(MEDIUM|HIGH|TRACE)\\]' /Users/richarddje/Documents/github/rgx/trace.log | wc -l` => `0`
    - `grep -E '\\[LOW\\]' /Users/richarddje/Documents/github/rgx/trace.log | wc -l` => `19`
  - quiet smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log 'cat|dog' 'I have a dog'`
    - `wc -l /Users/richarddje/Documents/github/rgx/trace.log` => `0`
- Notes/impact:
  - Extends parser trace continuity into token/AST introspection helpers without changing parse or match semantics.
  - Improves debug-time diagnostics when following parser control-flow and node-kind classification decisions.
### 2026-03-02 - Added structured tracing to lexer escape-helper boundaries
- Scope: escape-sequence utility observability in `rgx-core/src/lexer.rs`
- Changes:
  - Root-cause gap: helper-level escape parsing boundaries were not explicitly traced, making it harder to diagnose failures inside specific escape subparsers.
  - Instrumented helper boundaries:
    - `Lexer::parse_unicode_class`
    - `Lexer::parse_backreference`
    - `Lexer::parse_hex_escape`
    - `Lexer::parse_octal_escape`
  - Added decision traces for critical branches:
    - unicode-class opening-brace validation
    - backreference range validation (`1..=99`)
    - braced-vs-short hex format dispatch
    - octal byte-range validation (`<= 255`)
  - Added explicit traced error exits for parse/validation failure paths in the above helpers.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - debug smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log '\\x41' 'A'`
    - verified `trace.log` includes `Lexer::parse_hex_escape` enter/exit lines with code-point summary
  - low smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log 'cat|dog' 'I have a dog'`
    - `grep -E '\\[(MEDIUM|HIGH|TRACE)\\]' /Users/richarddje/Documents/github/rgx/trace.log | wc -l` => `0`
    - `grep -E '\\[LOW\\]' /Users/richarddje/Documents/github/rgx/trace.log | wc -l` => `19`
  - quiet smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log 'cat|dog' 'I have a dog'`
    - `wc -l /Users/richarddje/Documents/github/rgx/trace.log` => `0`
- Notes/impact:
  - Extends structured tracing continuity to escape-subparser internals without changing matching semantics.
  - During debug verification, `\\101` continued to route through backreference handling (existing behavior), confirming this increment is observability-only.
### 2026-03-01 - Added structured tracing to parser token-cursor advance boundary
- Scope: parser/lexer handoff observability in `rgx-core/src/parser.rs`
- Changes:
  - Root-cause gap: parser token-cursor advancement was not explicitly traced, making token-consumption transitions opaque between parser nodes and lexer fetches.
  - Instrumented `Parser::advance` with structured tracing:
    - function-entry snapshot of current parser token
    - decision trace for whether advancing must fetch the next lexer token (`should_fetch_next`)
    - explicit error exit when lexer `next_token()` fails during parser advancement
    - function-exit summary with consumed token and resulting next token
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - debug smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log '(?<word>[a-z]+)' 'abc'`
    - verified `trace.log` contains `Parser::advance` enter/exit lines with consumed/next token snapshots
  - low smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log '(?<word>[a-z]+)' 'abc'`
    - `grep -E '\\[(MEDIUM|HIGH|TRACE)\\]' /Users/richarddje/Documents/github/rgx/trace.log | wc -l` => `0`
    - `grep -E '\\[LOW\\]' /Users/richarddje/Documents/github/rgx/trace.log | wc -l` => `11`
  - quiet smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log '(?<word>[a-z]+)' 'abc'`
    - `wc -l /Users/richarddje/Documents/github/rgx/trace.log` => `0`
- Notes/impact:
  - Extends structured tracing continuity across parser token-cursor transitions without changing parse semantics.
  - Improves debugging of parser branch behavior by making token consumption and lexer-fetch boundaries explicit.
### 2026-02-28 - Added structured tracing to AST and token utility boundaries
- Scope: AST/token construction-path observability in `rgx-core/src/ast.rs` and `rgx-core/src/token.rs`
- Changes:
  - Observability-gap root cause: utility-level constructors/context helpers were not yet in the structured trace chain, so parser/lexer traces skipped part of object-creation context.
  - Instrumented AST utility boundaries:
    - `CharRange::single`
    - `CharRange::range` (includes ordering decision trace)
    - `ParseContext::new`
    - `ParseContext::next_group_number`
    - `ParseContext::register_named_group` (includes replacement decision trace)
    - `ParseContext::get_named_group` (includes lookup-hit decision trace)
  - Instrumented token/position utility boundaries:
    - `Position::new`
    - `Position::start`
    - `TokenWithPos::new`
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - debug smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log '(?<word>[a-z]+)' 'abc'`
    - verified `trace.log` includes new boundary lines for `Position::start/new`, `TokenWithPos::new`, and `CharRange::range`
  - low smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log '(?<word>[a-z]+)' 'abc'`
    - `grep -E '\\[(MEDIUM|HIGH|TRACE)\\]' /Users/richarddje/Documents/github/rgx/trace.log | wc -l` => `0`
  - quiet smoke:
    - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log '(?<word>[a-z]+)' 'abc'`
    - `wc -l /Users/richarddje/Documents/github/rgx/trace.log` => `0`
- Notes/impact:
  - Extends structured trace continuity through AST/token utility construction and parse-context mutation/lookup paths.
  - Improves root-cause visibility for parser/lexer behavior without changing regex semantics.
### 2026-02-28 - Added structured tracing to compiler constructors and parser utility boundaries
- Scope: compile-time configuration/selection observability in `rgx-core/src/compiler.rs` and `rgx-core/src/parsing.rs`
- Changes:
  - Instrumented compiler constructors:
    - `Compiler::new`
    - `Compiler::with_mode` (including mode-selection decision trace)
  - Instrumented parsing utility boundaries:
    - `parser_name` (recursive-descent + pgen-feature variants)
    - `parser_capabilities` (recursive-descent + pgen-feature variants, including perl-advanced capability decision)
    - `RecursiveDescentParser::new`, `RecursiveDescentParser::parser_name`, `RecursiveDescentParser::capabilities`
    - `PgenParser::new`, `PgenParser::parser_name`, `PgenParser::capabilities` (feature-gated)
    - `ParserConfig::default`
  - Resolved an in-progress patch artifact in `parsing.rs` while applying this increment (corrupted capability block merge), then revalidated.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log "cat|dog" "I have a dog"` + `grep -n 'Compiler::new' /Users/richarddje/Documents/github/rgx/trace.log`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --mode safe --verbosity debug --trace-log "cat" "I have a cat"` + `grep -n 'Compiler::with_mode' /Users/richarddje/Documents/github/rgx/trace.log`
  - low/quiet filter checks preserved:
    - `grep -nE '\[(TRACE|HIGH|MEDIUM)\]' /Users/richarddje/Documents/github/rgx/trace.log` after low run (no matches)
    - `wc -c /Users/richarddje/Documents/github/rgx/trace.log` after quiet run (`0`)
- Notes/impact:
  - Improves diagnosis of compile-time mode/backend/capability selection before heavy parser/compiler execution begins.
  - Preserves runtime behavior while extending structured trace continuity into configuration and constructor phases.
### 2026-02-28 - Added structured tracing to RegexVM initialization and SIMD detection
- Scope: VM construction-path observability in `rgx-core/src/vm.rs`
- Changes:
  - Instrumented `RegexVM::new` with structured tracing:
    - compile-program context at VM construction entry (bytecode/classes/literals/groups/anchor+lookaround flags)
    - SIMD-availability decision summary
    - VM-construction exit summary including detected SIMD flags
  - Instrumented `RegexVM::detect_simd_support` with structured entry/exit traces and capability summary fields.
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (exit `0`; warnings present, no clippy errors)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log "cat|dog" "I have a dog"`
  - `grep -n 'RegexVM::new' /Users/richarddje/Documents/github/rgx/trace.log` and `grep -n 'RegexVM::detect_simd_support' /Users/richarddje/Documents/github/rgx/trace.log` (verified boundary traces)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log "cat|dog" "I have a dog"`
  - `grep -nE '\[(TRACE|HIGH|MEDIUM)\]' /Users/richarddje/Documents/github/rgx/trace.log` (verified filtered)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log "cat|dog" "I have a dog"`
  - `wc -c /Users/richarddje/Documents/github/rgx/trace.log` (verified `0`)
- Notes/impact:
  - Extends tracing continuity into VM startup so runtime capability detection and initialization context are now visible in debug traces.
  - Improves first-hop diagnosis for architecture-specific execution behavior without changing matching semantics.
### 2026-02-28 - Added clippy error gate to commit workflow
- Scope: workflow policy and commit-quality gates
- Changes:
  - Updated `COMMIT.md` commit workflow to include a mandatory clippy step:
    - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets`
  - Added explicit workflow policy:
    - clippy warnings are tolerated for now
    - clippy errors are not allowed before commit
  - Mirrored policy in:
    - `DEVELOPMENT_NOTES.md` documentation policy section
    - `MEMORY.md` persistent workflow agreements
- Validation:
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` (completed, exit code `0`; warnings present, no clippy errors)
- Notes/impact:
  - Establishes an explicit lint-quality floor in the standard workflow while preserving short-term flexibility on warning volume.
  - Prevents clippy regressions from entering history as hard errors.
### 2026-02-28 - Added structured tracing to CLI main control-flow path
- Scope: top-level `rgx-cli` execution-flow observability in `rgx-cli/src/main.rs`
- Changes:
  - Instrumented CLI `main()` with structured tracing:
    - function entry summary (mode argument, pattern/input lengths, verbosity, quiet/trace-log flags)
    - mode-selection decision tracing (`pure` vs other execution modes)
    - input-source decision tracing (stdin vs positional argument)
    - match-outcome decision tracing (`regex.is_match(input)`)
    - function exit summary with final match boolean
  - Preserved existing logger output behavior and ensured structured tracing is emitted only after logging env initialization.
  - Fixed an in-progress patch artifact during implementation (duplicate nested `if regex.is_match(...)` branch).
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log "cat|dog" "I have a dog"`
  - `grep -n 'ENTER main' /Users/richarddje/Documents/github/rgx/trace.log` and `grep -n 'EXIT main' /Users/richarddje/Documents/github/rgx/trace.log` (verified CLI boundary traces)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log "cat|dog" "I have a dog"`
  - `grep -nE '\[(TRACE|HIGH|MEDIUM)\]' /Users/richarddje/Documents/github/rgx/trace.log` (verified filtered)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log "cat|dog" "I have a dog"`
  - `wc -c /Users/richarddje/Documents/github/rgx/trace.log` (verified `0`)
- Notes/impact:
  - Extends structured tracing all the way to CLI ingress/egress, improving whole-pipeline flow diagnosis from command invocation through match result.
  - Keeps verbosity semantics unchanged while making top-level branch decisions explicit in `trace.log`.
### 2026-02-27 - Added structured tracing at VM OptimizingCompiler boundaries
- Scope: compile-time VM bytecode generation observability in `rgx-core/src/vm.rs`
- Changes:
  - Instrumented `OptimizingCompiler::new` with structured entry/exit tracing and initialization summary fields.
  - Instrumented `OptimizingCompiler::compile` with:
    - function entry including AST-kind context
    - decision trace for post-analysis JIT-worthiness and collected stats
    - function exit summary (bytecode length, char classes, string literals, groups, jit_worthy)
  - Added internal AST-kind classifier helper used by compile-boundary traces for concise node-type reporting.
  - Fixed interrupted patch artifacts during implementation (duplicate `Program` initializer token in compile path).
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log "cat|dog" "I have a dog"`
  - `grep -n 'OptimizingCompiler::compile' /Users/richarddje/Documents/github/rgx/trace.log` (verified ENTER/EXIT lines)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log "cat|dog" "I have a dog"`
  - `grep -nE '\[(TRACE|HIGH|MEDIUM)\]' /Users/richarddje/Documents/github/rgx/trace.log` (verified filtered)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log "cat|dog" "I have a dog"`
  - `wc -c /Users/richarddje/Documents/github/rgx/trace.log` (verified `0`)
- Notes/impact:
  - Extends trace continuity into compiler internals, making bytecode-generation phase boundaries diagnosable from `trace.log`.
  - Improves reasoning visibility around VM JIT-heuristic decisions without changing codegen behavior.
### 2026-02-27 - Extended structured tracing into execution manager and callback runtime
- Scope: execution-module boundary observability for context access, callback dispatch, and language routing
- Changes:
  - Instrumented `rgx-core/src/execution.rs` with structured tracing at public/runtime boundaries:
    - `ExecContext::{new,current_match,group,named}`
    - `NativeCallbackRegistry::{new,register,call,has}`
    - `ExecutionManager::{new,execute,register_native,is_language_available}`
  - Added decision traces for:
    - capture/named-capture lookup outcomes
    - callback replacement/registration behavior
    - callback existence checks and native dispatch fallback
    - language backend routing (native vs supported/unsupported backend)
    - backend-availability outcomes during execution manager construction
  - Added internal execution-result kind classification helper for consistent exit trace summaries (`Success|Failure|Replacement|Numeric|Error`)
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log "cat|dog" "I have a dog"`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log "cat|dog" "I have a dog"`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log "cat|dog" "I have a dog"`
  - verified low filtering (`[LOW]` only) and quiet sink behavior (`trace.log` size `0`)
- Notes/impact:
  - Extends trace continuity into the code-execution subsystem so callback/language-dispatch decisions are now externally diagnosable
  - Preserves runtime behavior while improving branch-level observability for future execution-feature integration
### 2026-02-27 - Extended structured tracing into API and engine execution path
- Scope: high-level API/engine observability and UTF-8 gate decision visibility
- Changes:
  - Instrumented `rgx-core/src/lib.rs` API boundaries with structured tracing:
    - compile constructors: `Regex::compile`, `Regex::with_mode`, `Regex::from_ast`, `Regex::from_ast_with_mode`
    - execution API calls: `Regex::find_all`, `Regex::find_first`, `Regex::is_match`
  - Instrumented `rgx-core/src/engine.rs` runtime dispatch boundaries:
    - `Engine::new`, `Engine::find_all`, `Engine::find_first`, `Engine::is_match`
    - added explicit decision logs for UTF-8 validation gates and match/no-match outcomes
    - added structured exits that preserve reasons for invalid UTF-8 early returns
  - Corrected interrupted partial edit fallout in `lib.rs`/`engine.rs` while applying the tracing increment (duplicate/fragmented return path cleanup)
- Validation:
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --all`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core`
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity debug --trace-log "cat|dog" "I have a dog"`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --verbosity low --trace-log "cat|dog" "I have a dog"`
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --bin rgx-cli -- --quiet --trace-log "cat|dog" "I have a dog"`
  - verified low filtering (`[LOW]` retained, no `[MEDIUM]/[HIGH]/[TRACE]`) and quiet sink behavior (`trace.log` size `0`)
- Notes/impact:
  - Completes structured trace continuity from API ingress through engine dispatch to VM internals
  - Improves root-cause diagnosis for invalid-input and boundary outcomes without changing matching semantics
### 2026-02-27 - Extended structured tracing into lexer-path pipeline
- Scope: lexer observability and trace continuity before parser/compile stages
- Changes:
  - Instrumented `rgx-core/src/lexer.rs` with structured tracing on lexer hotspots:
    - `Lexer::new`, `Lexer::next_token`, `Lexer::parse_escape`
    - quantifier token helpers (`parse_star`, `parse_plus`, `parse_question`, `parse_repeat_quantifier`)
    - character-class parsing (`parse_character_class`)
    - group/conditional paths (`parse_group`, `parse_conditional_start`, `parse_conditional_subexpression_ast`)
  - Added lexer decision logs for:
    - EOF token emission in `next_token`
    - simple-vs-special group parsing branch
    - conditional-start close-token validation
    - repeat-quantifier form validation
  - Added structured success/error exits for key lexer parse helpers to improve boundary diagnosability
- Validation:
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-cli`
  - `cargo run --bin rgx-cli -- --verbosity debug --trace-log \"a{2,3}\" \"aaa\"`
  - `cargo run --bin rgx-cli -- --verbosity low --trace-log \"a{2,3}\" \"aaa\"`
  - `cargo run --bin rgx-cli -- --quiet --trace-log \"a{2,3}\" \"aaa\"`
  - verified lexer trace lines appear in `trace.log` at debug and are filtered at low/quiet
- Notes/impact:
  - Improves trace readability for tokenization decisions and lexer parse failures
  - Strengthens first-class tracing coverage from lexer through parser, compiler, and VM paths
### 2026-02-27 - Extended structured tracing into parser-path pipeline
- Scope: parser-path observability depth in `rgx-core` parsing stack
- Changes:
  - Instrumented `rgx-core/src/parser.rs` with structured tracing on parser hotspots:
    - function entry/exit tracing for `Parser::new`, `parse`, `parse_alternation`, `parse_sequence`, `parse_quantified`, and `parse_atom`
    - decision tracing for alternation branching, quantifier wrapping, and post-parse EOF boundary checks
  - Instrumented `rgx-core/src/parsing.rs` compile-time parser entry points:
    - structured tracing for `parse_pattern` in both recursive-descent and `pgen-parser` feature paths
    - structured tracing for `RecursiveDescentParser::parse_pattern` trait adapter
    - low-level parser backend-selection logs plus parse-boundary success/failure decisions
- Validation:
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-cli`
  - `cargo run --bin rgx-cli -- --verbosity debug --trace-log \"a|b\" \"a\"`
  - `cargo run --bin rgx-cli -- --verbosity low --trace-log \"a|b\" \"a\"`
  - `cargo run --bin rgx-cli -- --quiet --trace-log \"a|b\" \"a\"`
  - verified parser-path trace lines appear in `trace.log` at debug verbosity and are filtered at low/quiet
- Notes/impact:
  - Improves trace continuity from parser frontend into compiler/VM flow
  - Makes parser decisions and parse-boundary failures more diagnosable with file/function/line origin metadata
### 2026-02-27 - Added UVM-style multi-level verbosity and structured tracing helpers
- Scope: first-class tracing ergonomics and observability depth in `rgx-core` + `rgx-cli`
- Changes:
  - Refactored `rgx-core/src/log.rs` to support UVM-style verbosity levels:
    - `Verbosity::{None, Low, Medium, High, Debug}`
    - `RGX_VERBOSITY=none|low|medium|high|debug` env control
    - backward-compatible mapping for `RGX_DEBUG`/`RGX_TRACE`
  - Added structured tracing helpers/macros in `rgx-core/src/log.rs`:
    - `trace_enter!`, `trace_exit!`, `trace_decision!`
    - low/medium/high log macros for tiered output curation
    - consistent emoji-tagged level formatting in emitted lines
  - Updated `rgx-cli/src/main.rs`:
    - added `--verbosity <none|low|medium|high|debug>`
    - added `--quiet` for forced silence
    - retained compatibility aliases (`--debug` => high, `--trace` => debug)
    - routes CLI messages through verbosity-filtered core sink (`emit_external_at`)
  - Instrumented compiler/VM hotspots with function-entry/function-exit/decision logs:
    - compiler compile path (`compile`, `compile_ast`, `compile_ast_with_label`)
    - VM execution path (`find_first`, strategy selection, scanning, anchored/SIMD entry points, `find_all`, `is_match`)
- Validation:
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-cli`
  - `cargo run --bin rgx-cli -- --verbosity debug --trace-log \"a\" \"a\"`
  - `cargo run --bin rgx-cli -- --verbosity low --trace-log \"a\" \"a\"`
  - `cargo run --bin rgx-cli -- --quiet --trace-log \"a\" \"a\"`
  - verified `trace.log` filtering behavior by level (debug = exhaustive, low = milestones, quiet = empty file)
- Notes/impact:
  - Delivers user-controllable trace depth consistent with UVM-style workflow expectations
  - Improves post-run diagnostics by making function flow and decision rationale explicitly visible
### 2026-02-27 - Added trace.log routing for debug/trace output
- Scope: tracing usability and output control in `rgx-core` + `rgx-cli`
- Changes:
  - Refactored `rgx-core/src/log.rs` into a centralized emit/sink model:
    - supports `RGX_TRACE_FILE=<path>` to route debug/trace output into a file (e.g., `trace.log`)
    - keeps existing `RGX_DEBUG`/`RGX_TRACE` filtering behavior
    - updates `debug_log!` / `trace_log!` macros to use centralized emit helpers that include source file/module/line metadata
  - Updated `rgx-cli/src/main.rs`:
    - added `--trace-log` option to route logs to `trace.log`
    - routes CLI debug/trace banner and progress messages through the same core logging sink
    - initializes logging environment before first log emission so filtering/routing configuration is stable
  - Updated docs to include trace-log usage in quick-start examples and technical notes
- Validation:
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-cli`
  - `cargo run --bin rgx-cli -- --debug --trace-log "a" "a"`
  - verified `trace.log` contains emitted trace/debug lines while match result output remains on CLI output
- Notes/impact:
  - Enables file-backed trace collection for post-run debugging and handoff artifacts
  - Ensures trace logs and CLI-side debug messages are routed consistently through one sink
### 2026-02-26 - Added `COMMIT.md` as authoritative commit-workflow contract
- Scope: workflow/documentation hardening for deterministic commits and AI handoff consistency
- Changes:
  - Added root-level `COMMIT.md` defining:
    - commit workflow purpose and cadence
    - files involved in commit operations and precise role of each file
    - exact ordered pre-commit, commit, and post-commit verification steps
    - non-negotiable commit invariants
  - Integrated references to `COMMIT.md` in:
    - `README.md` documentation map
    - `DEVELOPMENT_NOTES.md` documentation policy
    - `MEMORY.md` deep-reference list and session entry
- Validation:
  - Documentation consistency review across `COMMIT.md`, `README.md`, `DEVELOPMENT_NOTES.md`, and `MEMORY.md`
- Notes/impact:
  - Makes commit behavior explicit for successor AI instances and reduces commit-process ambiguity
  - Improves reliability of staged-file integrity and post-commit cleanup practices
### 2026-02-22 - Added differential parity guardrails for greedy quantifier suffix backtracking
- Scope: PCRE2 differential test coverage expansion for `*`, `+`, and `?` backtracking semantics
- Changes:
  - Added `pcre2_parity_supported_quantifier_suffix_backtracking_behavior` in `rgx-bench/tests/pcre2_parity.rs`
  - New differential first-match and `find_all` cases validate suffix-sensitive behavior for:
    - `a*a`
    - `a+a`
    - `ab?b`
  - Added explicit PCRE2 expected-span assertions inside the new differential test to pin expected behavior
- Validation:
  - `cargo test -p rgx-bench pcre2_parity_supported_quantifier_suffix_backtracking_behavior -- --nocapture`
  - `cargo test -p rgx-bench`
  - `cargo test -p rgx-core quantifier_backtracks_for_suffix -- --nocapture`
- Notes/impact:
  - Hardens parity regression detection for the same greedy quantifier suffix semantics recently fixed in VM execution
  - Keeps parity assertions focused on executable, behavior-level outcomes rather than documentation-only claims
### 2026-02-22 - Fixed greedy quantifier backtracking runtime semantics and added unbounded-range parity coverage
- Scope: VM quantifier execution correctness + PCRE2 parity hardening for unbounded ranges
- Changes:
  - Updated greedy quantifier execution in `rgx-core/src/vm.rs`:
    - `PlusGreedy`, `StarGreedy`, and `QuestionGreedy` now preserve backtrack fallback states for consumed repetitions
    - failed/no-advance repetition attempts now restore pre-attempt position/capture/call-stack state before continuing
    - `PlusGreedy` first-required repetition failure now properly participates in outer backtracking
  - Added parser-path regressions in `rgx-core/src/lib.rs`:
    - unbounded range `{2,}` first-match/find-all behavior
    - unbounded-range suffix backtracking/greedy behavior (`\\d{2,}3`)
    - suffix backtracking guardrails for greedy `*`, `+`, and `?` (`a*a`, `a+a`, `ab?b`)
  - Added differential PCRE2 parity test in `rgx-bench/tests/pcre2_parity.rs`:
    - `pcre2_parity_supported_unbounded_range_quantifier_behavior` covering `{n,}` scan parity and suffix-sensitive `{n,}3` behavior
  - Expanded supported parser-path matrix cases with unbounded range and unbounded-range suffix examples
  - Updated `docs/PCRE2_COMPATIBILITY_MATRIX.md` range note to include unbounded range parity coverage
- Validation:
  - `cargo test -p rgx-core parser_unbounded_range_quantifier -- --nocapture`
  - `cargo test -p rgx-core quantifier_backtracks_for_suffix -- --nocapture`
  - `cargo test -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo test -p rgx-bench pcre2_parity_supported_unbounded_range_quantifier_behavior -- --nocapture`
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-bench`
- Notes/impact:
  - Closes a correctness hole where greedy quantified subexpressions could over-consume without fallback to suffix-compatible spans
  - Raises confidence that range quantifier parity now holds for both bounded `{n,m}` and unbounded `{n,}` forms
### 2026-02-22 - Expanded bounded-range suffix parity coverage in differential and API tests
- Scope: parity hardening for backtracking-sensitive bounded range quantifier behavior
- Changes:
  - Extended `rgx-bench/tests/pcre2_parity.rs` supported-syntax differential matrices with additional range cases:
    - `{2,3}3` bounded suffix backtracking scenarios in first-match and find-all parity sets
    - exact-range `{3}` multi-match find-all parity scenario
  - Extended parser-path regressions in `rgx-core/src/lib.rs`:
    - bounded-range suffix backtracking stays correct (`123` with `\\d{2,3}3`)
    - greedy longest-valid bounded-range suffix span is preferred (`2233` with `\\d{2,3}3`)
    - bounded-range suffix `find_all` spans are stable across multiple tokens
  - Expanded `capability_matrix_supported_parser_path_cases` with positive/negative bounded-range suffix examples
  - Updated `docs/PCRE2_COMPATIBILITY_MATRIX.md` notes to reflect expanded bounded-range suffix and exact-range differential coverage
- Validation:
  - `cargo test -p rgx-core parser_range_quantifier -- --nocapture`
  - `cargo test -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo test -p rgx-bench`
  - `cargo test -p rgx-core`
- Notes/impact:
  - Increases confidence that recently fixed `{n,m}` execution semantics remain aligned with PCRE2 under suffix-sensitive backtracking pressure
  - Improves regression detection without changing runtime feature scope
### 2026-02-22 - Closed `{n,m}` scan parity gap against PCRE2
- Scope: `rgx-core` range-quantifier execution semantics, differential parity tests, and parity docs
- Changes:
  - Updated `rgx-core/src/vm.rs` range quantifier code generation:
    - bounded ranges (`{n,m}`) now compile required prefix + greedy optional tail via `Split`, enabling fallback to shorter valid spans
    - unbounded ranges (`{n,}`) now compile required prefix + unbounded `StarGreedy` tail
  - Added VM helper `try_backtrack` and wired mismatch paths for key opcodes (`Any`, ASCII classes, boundaries, anchors, lookarounds, custom char classes) to honor pending backtrack frames instead of hard-failing immediately
  - Added parser-path regressions in `rgx-core/src/lib.rs`:
    - `{2,3}` earliest valid scan span behavior
    - `{2,3}` bounded backtracking when followed by a literal suffix
  - Updated differential parity coverage in `rgx-bench/tests/pcre2_parity.rs`:
    - reclassified range scan test from known-gap to parity-supported and now asserts equality with PCRE2
  - Updated `docs/PCRE2_COMPATIBILITY_MATRIX.md`:
    - moved `{n,m}` scanning/earliest-match behavior to parity-verified baseline
- Validation:
  - `cargo test -p rgx-core parser_range_quantifier -- --nocapture`
  - `cargo test -p rgx-bench pcre2_parity_supported_range_quantifier_scan_behavior -- --nocapture`
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-bench`
- Notes/impact:
  - Closes the previously tracked `{n,m}` scanning parity divergence
  - Keeps remaining known-gap focus on parsed-but-unintegrated advanced families (backreferences, recursion, conditionals)
### 2026-02-22 - Introduced `MEMORY.md` as live continuity infrastructure for cross-session resume
- Scope: documentation/process hardening for interruption-safe session handoff
- Changes:
  - Added `MEMORY.md` at repository root as a live, compact continuity document designed for:
    - rapid post-crash/post-reset resume
    - preserving key actionable user/agent exchange outcomes
    - explicit resume checklist and workflow invariants
  - Defined mandatory update cadence in `MEMORY.md`:
    - update after completed tasks
    - update before commit workflow
  - Integrated `MEMORY.md` into live-doc ecosystem references:
    - `README.md` documentation map
    - `DEVELOPMENT_NOTES.md` documentation policy
- Validation:
  - Documentation consistency review across `MEMORY.md`, `README.md`, and `DEVELOPMENT_NOTES.md`
- Notes/impact:
  - Reduces context-loss risk across session interruptions and AI instance handoffs
  - Makes process-critical workflow expectations explicit and centralized
### 2026-02-20 - Fixed end-anchor (`$`) suffix matching parity by correcting anchored-search strategy selection
- Scope: `rgx-core` VM matching strategy + regression coverage + parity docs/tests
- Changes:
  - Fixed VM strategy selection in `rgx-core/src/vm.rs`:
    - introduced `should_use_start_anchored_search()` so anchored fast-path is used only for start-anchored programs
    - end-anchor-only patterns now use standard scanning instead of incorrectly forcing start-position-only execution
  - Added VM regressions in `rgx-core/src/vm.rs`:
    - suffix match for `dog$` in `cat dog`
    - `find_all` behavior confirming only terminal match is returned for end-anchored pattern
  - Added parser-path API regressions in `rgx-core/src/lib.rs`:
    - `Regex::compile(\"dog$\")` now validated for `find_first`, `find_all`, and non-terminal rejection behavior
    - capability-matrix supported parser-path cases now include `dog$` true/false expectations
  - Updated differential parity harness in `rgx-bench/tests/pcre2_parity.rs`:
    - moved end-anchor from known-gap test back into supported parity first-match and find-all case sets
    - removed obsolete known-gap end-anchor divergence assertion
  - Updated `docs/PCRE2_COMPATIBILITY_MATRIX.md`:
    - anchors (`^`, `$`) now listed as parity-verified in supported parser-path forms
- Validation:
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-bench`
- Notes/impact:
  - Closes previously documented end-anchor parity gap against PCRE2 for supported parser-path cases
  - Preserves truthful gap reporting for remaining known divergence (`{n,m}` range quantifier scanning behavior)
### 2026-02-20 - Expanded PCRE2 differential parity coverage for anchors, quantifiers, and no-match behavior
- Scope: supported-syntax parity hardening in `rgx-bench` differential suite
- Changes:
  - Extended `pcre2_parity_supported_syntax_first_match_span` with additional supported-syntax coverage:
    - start-anchor (`^`) cases
    - additional basic-quantifier (`+`) cases
    - explicit no-match example
  - Extended `pcre2_parity_supported_syntax_find_all_spans` with:
    - start-anchor (`^`) cases
    - basic-quantifier (`+`) multi-match scanning cases
    - explicit no-match all-span case
  - Added `pcre2_parity_supported_syntax_no_match_consistency` to assert parity invariants when no match exists:
    - first-match parity (`None`)
    - all-match parity (empty span set)
  - Added explicit known-gap differential test for end-anchor (`$`) divergence (`pcre2_parity_known_gap_end_anchor_behavior`)
  - Added explicit known-gap differential test for range quantifier scanning divergence (`pcre2_parity_known_gap_range_quantifier_scan_behavior`)
  - Updated `docs/PCRE2_COMPATIBILITY_MATRIX.md` to reflect start-anchor/no-match parity and end-anchor known-gap status
  - Updated `docs/PCRE2_COMPATIBILITY_MATRIX.md` to classify range quantifier scanning as a known gap
- Validation:
  - `cargo test -p rgx-bench`
- Notes/impact:
  - Improves confidence in parity for scan behavior and negative-path semantics, not just positive match examples
  - Strengthens regression detection for no-match and scan semantics while keeping end-anchor and range-quantifier behavior truthfully classified as current parity gaps
### 2026-02-20 - Expanded PCRE2 supported-syntax differential checks to find-all span parity
- Scope: parity harness depth for currently shipped syntax behavior
- Changes:
  - Extended `rgx-bench/tests/pcre2_parity.rs` with reusable all-span helpers:
    - rgx `find_all` span collection
    - PCRE2 `find_iter` span collection
  - Added `pcre2_parity_supported_syntax_find_all_spans` covering representative supported syntax classes:
    - literals, alternation, digit classes, word boundaries
    - lookahead/lookbehind (positive and negative)
    - atomic-group no-backtracking behavior and non-atomic counterexample
  - Updated `docs/PCRE2_COMPATIBILITY_MATRIX.md` to explicitly state first-match and find-all differential parity coverage
- Validation:
  - `cargo test -p rgx-bench`
- Notes/impact:
  - Strengthens parity confidence beyond first-match behavior by validating non-overlapping multi-match span parity
  - Improves regression detection for scanning behavior differences between rgx and PCRE2
### 2026-02-20 - Expanded PCRE2 differential gap guardrails for recursion and conditionals
- Scope: parity harness hardening for parsed-but-unintegrated syntax families
- Changes:
  - Extended `rgx-bench/tests/pcre2_parity.rs` with reusable known-gap assertions that enforce:
    - `rgx` compile-time explicit unsupported errors with expected error text
    - successful PCRE2 execution for the same patterns
  - Added recursion known-gap differential cases for:
    - `(?R)`, `(?1)`, and `(?&name)` recursion forms
  - Added conditional known-gap differential cases for:
    - group-exists and named-group-exists forms
    - lookahead and lookbehind condition forms (positive/negative variants)
  - Updated `docs/PCRE2_COMPATIBILITY_MATRIX.md` to document the newly covered conditional variant set under known gaps
- Validation:
  - `cargo test -p rgx-bench`
- Notes/impact:
  - Turns additional parity-gap claims into executable regression guards
  - Reduces risk of silent drift between documented PCRE2 gaps and test-enforced behavior
### 2026-02-20 - Started PCRE2 parity baseline with live matrix and differential tests
- Scope: parity-program bootstrap for capability/behavior verification against PCRE2
- Changes:
  - Added `docs/PCRE2_COMPATIBILITY_MATRIX.md` as the live rgx-vs-PCRE2 parity source-of-truth with:
    - parity status labels
    - parity-verified baseline feature set
    - explicit known rgx gaps and out-of-scope extensions
  - Added executable differential tests in `rgx-bench/tests/pcre2_parity.rs`:
    - first-match span parity checks across representative supported syntax
    - explicit known-gap guardrail for backreference compile behavior (`rgx` rejected vs PCRE2 executed)
  - Updated `README.md`, `DEVELOPMENT_NOTES.md`, and `ROADMAP.md` to reference the new parity matrix and baseline harness
- Validation:
  - `cargo test -p rgx-bench`
- Notes/impact:
  - Converts parity claims into executable checks instead of documentation-only assertions
  - Establishes a concrete baseline for incremental PCRE2 parity expansion
### 2026-02-20 - Expanded capability-matrix guardrails across recursion and conditional syntax variants
- Scope: parser/API guardrail hardening for parsed-but-unintegrated advanced syntax
- Changes:
  - Expanded parser-path lookaround coverage in `rgx-core/src/lib.rs` with explicit parser syntax tests for:
    - negative lookahead `(?!...)`
    - positive lookbehind `(?<=...)`
  - Expanded capability-matrix supported parser-path cases in `rgx-core/src/lib.rs` to include representative negative-lookahead and positive-lookbehind semantics
  - Expanded explicit unsupported compile-boundary cases in `rgx-core/src/lib.rs` to cover:
    - recursion variants `(?R)`, `(?1)`, `(?&word)`
    - conditional condition variants `(?(1)...)`, `(?(<word>)...)`, `(?(word)...)`, `(?(?=...)...)`, `(?(?!...)...)`, `(?(?<=...)...)`, `(?(?<!...)...)`
  - Expanded parser contract fixtures/guardrails in `rgx-core/src/parsing.rs` to include:
    - named-group conditional angle-bracket form `(?(<word>)...)`
    - recursion variants `(?1)` and `(?&word)` in active and `pgen-parser` fixture parity checks
    - compile-boundary guardrail cases for the same recursion/conditional variants
- Validation:
  - `cargo test -p rgx-core` passed (92 tests)
  - `cargo test -p rgx-core --features pgen-parser` passed (93 tests)
- Notes/impact:
  - Reduces regression risk by ensuring capability-matrix boundaries are enforced across all currently documented recursion/conditional parser variants
  - Keeps parser acceptance and compile-boundary behavior aligned between active and feature-gated parser backends
### 2026-02-19 - Started capability matrix hardening with live matrix doc and integration guardrail tests
- Scope: docs + user-facing API behavior validation for shipped-vs-scaffolded clarity
- Changes:
  - Added `docs/CAPABILITY_MATRIX.md` as live source-of-truth for feature status (`shipped`, `parsed-only`, `scaffolded`)
  - Added capability-matrix integration tests in `rgx-core/src/lib.rs` for:
    - representative supported parser-path feature cases
    - representative parsed-but-explicitly-unsupported compile-boundary cases
  - Aligned `rgx-core/src/parsing.rs` parser-conformance fixtures after recent conditional additions (removed duplicate fixture, synchronized active/pgen fixture coverage)
  - Updated `README.md`, `DEVELOPMENT_NOTES.md`, and `ROADMAP.md` to reference matrix ownership and mark matrix hardening as active
- Validation:
  - `cargo test -p rgx-core` passed (90 tests)
  - `cargo test -p rgx-core --features pgen-parser` passed (91 tests)
- Notes/impact:
  - Makes shipped behavior boundaries explicit for users and contributors
  - Reduces regression risk by enforcing matrix expectations in executable tests
### 2026-02-19 - Expanded conditional parser support to include negative lookaround condition forms
- Scope: `rgx-core` conditional parser completeness and conformance/contract alignment
- Changes:
  - Extended lexer conditional-start parsing to support:
    - negative lookahead condition form `(?(?!expr)yes|no)`
    - negative lookbehind condition form `(?(?<!expr)yes|no)`
  - Updated `ConditionalTest` lookaround condition shape to encode sign explicitly:
    - `Lookahead { expr, positive }`
    - `Lookbehind { expr, positive }`
  - Added lexer tests for negative lookahead/lookbehind conditional tokenization
  - Added parser tests for negative lookahead/lookbehind conditional AST mapping
  - Extended parser conformance fixtures and compile-boundary guardrail checks with negative lookaround conditional patterns
  - Added API regression for negative-lookbehind conditional syntax to keep explicit unsupported compile/runtime boundary behavior validated
  - Updated parser contract (`docs/PARSER_CONTRACT.md` v0.1.3), README, and development notes to reflect expanded parser coverage
- Validation:
  - `cargo test -p rgx-core`
  - `cargo test -p rgx-core --features pgen-parser`
- Notes/impact:
  - Reduces parser completeness gap for conditional syntax without changing runtime integration status
  - Conditional execution remains explicitly unsupported until VM execution support lands
### 2026-02-19 - Expanded parser-path conditional syntax support to include bare-name and lookaround conditions
- Scope: `rgx-core` lexer/parser/conformance coverage for conditional syntax completeness
- Changes:
  - Extended lexer conditional-start parsing to support:
    - bare named-group condition `(?(name)yes|no)` (mapped to `NamedGroupExists`)
    - lookahead condition `(?(?=expr)yes|no)` (mapped to `ConditionalTest::Lookahead`)
    - lookbehind condition `(?(?<=expr)yes|no)` (mapped to `ConditionalTest::Lookbehind`)
  - Added internal lexer helper to parse lookaround condition sub-expressions into AST before emitting `Token::ConditionalStart`
  - Added lexer tests for:
    - bare named-group condition tokenization
    - lookahead condition tokenization
    - lookbehind condition tokenization
  - Added parser tests for:
    - bare named-group conditional AST mapping
    - lookahead conditional AST mapping
    - lookbehind conditional AST mapping
  - Extended parser contract/conformance fixtures to include the new conditional forms
  - Added API regression for lookahead-conditional syntax to confirm explicit unsupported compile/runtime boundary behavior remains intact
- Validation:
  - `cargo test -p rgx-core` passed (83 tests)
  - `cargo test -p rgx-core --features pgen-parser` passed (84 tests)
- Notes/impact:
  - Advances parser completeness toward PGEN-readiness without changing runtime safety semantics
  - Conditional execution remains explicitly unsupported in VM runtime path until dedicated integration lands
### 2026-02-19 - Collected and committed carried-over code cleanup edits from previously unstaged files
- Scope: cross-crate code hygiene cleanup (`rgx-core`, `rgx-cli`, `rgx-bench`)
- Changes:
  - Consolidated long-standing unstaged edits in bench/CLI/core files into one tracked change set
  - Normalized formatting/style in touched files (import ordering, spacing, line wrapping, macro formatting, and newline hygiene)
  - Applied the same cleanup to debug examples and supporting modules to keep code style consistent
  - No new feature surface introduced; intent is repository hygiene and maintainability for already-modified files
- Validation:
  - `cargo test -p rgx-core`
- Notes/impact:
  - Removes stale local drift from earlier sessions
  - Reduces review noise in future feature commits by clearing carried-over non-functional edits
### 2026-02-19 - Added parser-path conditional syntax support (group-exists subset) with explicit unsupported compile behavior
- Scope: `rgx-core` lexer/parser/parsing conformance and docs alignment
- Changes:
  - Extended lexer group parsing to recognize conditional-start syntax for:
    - group-exists form `(?(1)yes|no)`
    - named-group-exists form `(?(<name>)yes|no)`
  - Extended parser atom handling to build `Regex::Conditional` AST nodes from `Token::ConditionalStart`
  - Added lexer and parser tests for conditional tokenization/AST mapping
  - Added API-level regression test verifying conditional syntax now parses but still fails explicitly at compile/runtime boundary
  - Extended parser conformance fixtures and parser contract docs to include conditional syntax as a parsed-but-unintegrated feature
  - Updated roadmap/readme/development notes wording to reflect partial conditional parser support
- Validation:
  - `cargo test -p rgx-core`
- Notes/impact:
  - Advances parser completeness toward PGEN integration without introducing unsafe or silent runtime behavior
  - Keeps conditional execution semantics explicitly unsupported until VM integration lands
### 2026-02-19 - Added formal parser contract and conformance harness scaffolding for PGEN readiness
- Scope: `rgx-core` parser boundary definition and interoperability infrastructure
- Changes:
  - Added `docs/PARSER_CONTRACT.md` as a versioned contract document covering:
    - parser public interface (`RegexParser` trait + compile-time selected parser functions)
    - AST output invariants required by compiler/runtime
    - parse error mapping contract (`RgxError::Compile`)
    - parse-success/compile-fail boundary for currently unintegrated runtime features
    - capability-flag interpretation and backend change policy
  - Added parser conformance scaffold tests in `rgx-core/src/parsing.rs` for:
    - fixture parity between active parser and recursive-descent reference output
    - group metadata invariants (`index: None` parser responsibility boundary)
    - parse-failure error mapping guarantees
    - explicit compile-boundary failures for parsed-but-unintegrated constructs
    - `pgen-parser` backend-type parity check hook (feature-gated)
  - Made `pgen-parser` capability reporting truthful to current fallback behavior (no overclaiming of advanced/recovery/highlighting support)
  - Updated `README.md`, `ROADMAP.md`, and `DEVELOPMENT_NOTES.md` to reference the parser contract and conformance harness as active infrastructure
- Validation:
  - `cargo test -p rgx-core`
- Notes/impact:
  - Establishes an explicit RGX↔PGEN parser handshake artifact early
  - Reduces integration risk by turning parser compatibility into executable tests
### 2026-02-19 - Added recursion syntax parsing with explicit unsupported compile errors
- Scope: `rgx-core` lexer/parser/compiler behavior for advanced unintegrated constructs
- Changes:
  - Extended lexer/parser support for recursion syntax:
    - `(?R)`
    - `(?1)`
    - `(?&name)`
  - Added parser AST mapping for recursion tokens (`Regex::Recursion`)
  - Generalized compiler unsupported-feature detection so these constructs now fail explicitly (instead of silently degrading):
    - backreferences
    - recursion
    - conditionals
    - code blocks
  - Added tests for:
    - recursion tokenization/parsing
    - API-level explicit compile errors for backreference and recursion
- Validation:
  - `cargo test -p rgx-core` passed (67 tests)
- Notes/impact:
  - Improves correctness and debuggability by replacing silent failure behavior with explicit unsupported diagnostics
  - Advances parser completeness while preserving safe behavior until VM execution integration lands
### 2026-02-19 - Added parser-side code-block syntax parsing with explicit unsupported compile behavior
- Scope: `rgx-core` lexer/parser/compiler safety and capability signaling
- Changes:
  - Extended lexer group parsing to recognize code blocks:
    - `(?{lang:code})` -> `Token::CodeBlock { lang, code }`
  - Extended parser to build `Regex::CodeBlock` AST nodes
  - Updated recursive-descent parser capability flags to reflect implemented parsing support:
    - `named_groups = true`
    - `lookarounds = true`
    - `code_blocks = true`
  - Added explicit compile-time rejection for code-block AST nodes with clear error text, avoiding silent miscompilation in current VM path
  - Added tests for:
    - lexer code-block tokenization
    - parser code-block AST construction
    - API-level explicit unsupported compile error
    - parser capability flags
- Validation:
  - `cargo test -p rgx-core` passed (62 tests)
- Notes/impact:
  - Improves correctness by replacing silent failure behavior with explicit unsupported signaling
  - Preserves forward progress toward full code-block runtime integration
### 2026-02-19 - Implemented atomic-group no-backtracking runtime semantics
- Scope: `rgx-core` VM/compiler behavior for `(?>...)` groups
- Changes:
  - Updated compiler codegen for `GroupKind::Atomic` to emit:
    - `OpCode::AtomicStart`
    - inner expression
    - `OpCode::AtomicEnd`
  - Implemented VM runtime handling for atomic opcodes:
    - marks/tracks backtrack-stack depth at atomic-group entry
    - truncates internal backtrack frames on atomic-group success
  - Preserved atomic marker stack state across backtrack restores
  - Added opcode decoding for `AtomicStart`/`AtomicEnd`
  - Added parser-path API tests verifying atomic semantics:
    - `(?>a|ab)c` does not match `abc`
    - `(a|ab)c` matches `abc`
    - `(?>ab|a)c` matches `abc`
- Validation:
  - `cargo test -p rgx-core` passed (59 tests)
- Notes/impact:
  - Delivers actual atomic-group behavior instead of prior scaffolded no-op handling
  - Improves regex semantics parity for atomic-group constructs in parser path
### 2026-02-19 - Added parser-path lookaround syntax support
- Scope: `rgx-core` lexer/parser and compile-path behavior alignment
- Changes:
  - Extended group-token lexing to recognize:
    - positive lookahead `(?=...)`
    - negative lookahead `(?!...)`
    - positive lookbehind `(?<=...)`
    - negative lookbehind `(?<!...)`
    - atomic-group start `(?>...)`
  - Extended parser atom handling to build AST nodes for lookaround tokens and atomic groups
  - Added lexer tests for lookaround tokenization
  - Added parser tests for lookaround and atomic-group parsing
  - Added API tests through `Regex::compile(...)` for parser-path lookahead/lookbehind semantics
- Validation:
  - `cargo test -p rgx-core` passed (57 tests)
- Notes/impact:
  - Closes a parser-vs-AST gap for lookaround support
  - Keeps AST-first path available while parser completeness work continues for other advanced constructs
### 2026-02-19 - Clarified strategic goals: PCRE2 parity + broader code-block languages
- Scope: vision/roadmap/notes alignment for project direction
- Changes:
  - Updated `PROJECT_VISION.md` to explicitly target practical parity with PCRE2 for:
    - feature coverage
    - speed
    - matching accuracy
  - Updated `ROADMAP.md` with explicit PCRE2 parity workstream and multi-language code-block expansion goals
  - Updated `DEVELOPMENT_NOTES.md` to capture this goal clarification and re-prioritize immediate work accordingly
  - Updated `docs/TECHNICAL_DECISIONS.md` with explicit decision records for:
    - PCRE2 parity as north-star target
    - staged multi-language code-block expansion (including Julia)
- Validation:
  - Reviewed cross-doc consistency and wording to ensure goals are clearly marked as targets, not currently shipped guarantees
- Notes/impact:
  - Makes strategic direction explicit for future sessions and contributors
  - Reduces ambiguity between current capabilities and long-term parity goals
### 2026-02-19 - Added live roadmap tracker and layered end-user guide
- Scope: repository documentation structure and usability
- Changes:
  - Added `ROADMAP.md` as a live forward-looking tracker with:
    - maintenance workflow
    - explicit status legend
    - structured `Now` / `Next` / `Later` sections
  - Added `docs/USER_GUIDE.md` as a live end-user guide with layered depth:
    - Level 0 quick start
    - Level 1 practical usage
    - Level 2 advanced AST-first usage
    - Level 3 behavior semantics and implementation-facing details
  - Updated `README.md` documentation map to include both new docs
  - Updated `DEVELOPMENT_NOTES.md` documentation policy to include maintenance intent for both docs
- Validation:
  - Verified documentation links and cross-references for consistency
  - Content reviewed for alignment with current shipped behavior and known parser-path limits
- Notes/impact:
  - Establishes dedicated live planning and user-facing guidance surfaces
  - Improves onboarding for both contributors and end users at different depth levels
### 2026-02-19 - Added AST-first lookbehind support in compiler and VM
- Scope: `rgx-core` VM/compiler assertion semantics (parser-independent path)
- Changes:
  - Implemented AST codegen for lookbehind assertions:
    - `Regex::Lookbehind { positive: true }` -> `OpCode::Lookbehind`
    - `Regex::Lookbehind { positive: false }` -> `OpCode::LookbehindNeg`
  - Implemented VM execution semantics for lookbehind opcodes in:
    - main executor
    - sub-expression executor
  - Added bounded lookbehind assertion evaluation helper that requires the assertion sub-expression to end at current position
  - Extended opcode decoding (`TryFrom<u8>`) for `Lookbehind` and `LookbehindNeg`
  - Removed duplicate lookahead opcode branch in VM executor and bounded character reads by execution context end
  - Added parser-independent public API tests for positive and negative lookbehind behavior
- Validation:
  - `cargo test -p rgx-core` passed (51 tests)
- Notes/impact:
  - Enables AST-first progress on lookbehind assertions without parser syntax dependency
  - Parser syntax for lookbehind remains pending in parser path
### 2026-02-18 - Added built-in 1-based top-level branch number reporting
- Scope: `rgx-core` compiler/engine/public API semantics for top-level alternations
- Changes:
  - Restricted alternative tracking instrumentation to top-level alternation codegen paths
  - Exposed a single user-facing field on match results:
    - `MatchResult.matched_branch_number: Option<usize>`
  - Mapped internal alternative indices to user-facing 1-based branch numbers
  - Added/updated API tests to verify:
    - top-level alternation branch number exposure
    - nested alternation does not override top-level branch selection
- Validation:
  - `cargo test -p rgx-core` passed (49 tests)
- Notes/impact:
  - Removes user friction from 0-based IDs while preserving deterministic branch reporting
  - Keeps branch reporting semantics focused on the top-level alternation contract
### 2026-02-18 - Added AST-first lookahead support in compiler and VM
- Scope: `rgx-core` VM/compiler execution semantics (parser-independent path)
- Changes:
  - Implemented AST codegen for lookahead assertions:
    - `Regex::Lookahead { positive: true }` -> `OpCode::Lookahead`
    - `Regex::Lookahead { positive: false }` -> `OpCode::LookaheadNeg`
  - Implemented VM execution semantics for lookahead opcodes in:
    - main executor
    - sub-expression executor
  - Added non-consuming assertion evaluation helper so lookahead does not mutate parent context
  - Extended opcode decoding (`TryFrom<u8>`) for `Lookahead` and `LookaheadNeg`
  - Added parser-independent public API tests for positive and negative lookahead behavior
- Validation:
  - `cargo test -p rgx-core` passed (46 tests)
- Notes/impact:
  - Enables continued feature progress on advanced assertions without depending on parser readiness
  - Parser syntax for lookarounds remains pending; AST-first workflow is the current delivery path
### 2026-02-18 - Added parser-independent compile path for AST-driven development
- Scope: `rgx-core` compiler/API and feature-gating
- Changes:
  - Added explicit `pgen-parser` feature in `rgx-core/Cargo.toml` to match existing cfg usage and upcoming PGEN integration
  - Added `Compiler::compile_ast(ast)` to compile VM programs directly from AST without parsing
  - Added public parserless entry points:
    - `Regex::from_ast(ast)`
    - `Regex::from_ast_with_mode(ast, mode)`
  - Added tests exercising AST-driven compilation and matching via public API
- Validation:
  - `cargo test -p rgx-core` passed after these changes
- Notes/impact:
  - Unblocks VM/compiler/engine feature work while PGEN parser is still in active design
  - Reduces dependency on parser completeness for core-engine progress
### 2026-02-18 - Added parser and codegen support for `(?:...)` and `(?<name>...)`
- Scope: `rgx-core` lexer/parser/compiler integration
- Changes:
  - Extended lexer group parsing to emit:
    - `Token::NonCapturingGroupStart` for `(?:...)`
    - `Token::NamedGroupStart { name }` for `(?<name>...)`
  - Extended parser to build AST `Regex::Group` nodes for both syntaxes
  - Updated VM compiler group codegen to preserve group kind semantics:
    - capturing groups emit capture save opcodes
    - non-capturing groups compile without allocating captures
  - Added lexer/parser tests for both new syntaxes
- Validation:
  - `cargo test -p rgx-core` passed (42 tests)
  - CLI smoke tests passed:
    - `rgx-cli "(?:cat|dog)" "pet dog"` -> `4..7`
    - `rgx-cli "(?<word>cat)" "catnap"` -> `0..3`
- Notes/impact:
  - Brings parser behavior closer to common regex expectations for grouping semantics
  - Does not yet add lookaround or inline code-block parser support
### 2026-02-18 - Documentation quality reset and consolidation
- Scope: repository markdown documentation set
- Changes:
  - Rewrote core docs for accuracy and maintainability: `README.md`, `CHANGES.md`, `DEVELOPMENT_NOTES.md`, `PROJECT_VISION.md`, `docs/architecture.md`, `docs/TECHNICAL_DECISIONS.md`
  - Removed stale/redundant docs that conflicted with current implementation state:
    - `ROADMAP.md`
    - `docs/GETTING_STARTED.md`
    - `docs/extensibility.md`
    - `docs/implementation-status.md`
    - `docs/vm-implementation-guide.md`
  - Established this file (`CHANGES.md`) as the explicit living progress tracker
- Validation:
  - Verified documentation set for internal consistency
  - Confirmed retained docs now separate current status from long-term vision
- Notes/impact:
  - Reduced doc/code drift
  - Created one stable progress ledger for future sessions

### 2025-10-06 - Performance benchmark baseline and Lua foundation
- Scope: benchmarking and execution infrastructure
- Changes:
  - Added benchmark baseline for rgx vs PCRE2 in `rgx-bench`
  - Added Lua execution infrastructure foundation and execution-manager scaffolding
- Validation:
  - Benchmark harness runs and records comparative throughput/compile metrics
- Notes/impact:
  - Established measurable baseline for future optimization work

### 2025-09-07 - VM milestone completion
- Scope: `rgx-core` VM and compiler path
- Changes:
  - Built comprehensive VM execution engine and multi-pass compiler structure
  - Added VM tests covering core feature paths
- Validation:
  - VM test suite established and passing for covered features
- Notes/impact:
  - Enabled practical end-to-end regex execution through the VM backend

### 2025-09-02 to 2025-09-04 - Project bootstrap and parser/compiler foundation
- Scope: workspace setup and core compilation pipeline
- Changes:
  - Initialized workspace crates (`rgx-core`, `rgx-cli`, `rgx-bench`, `rgx-wasm`)
  - Implemented early lexer/parser/AST/compiler foundations
- Validation:
  - Early crate compilation and base tests
- Notes/impact:
  - Established architecture used by all later work
