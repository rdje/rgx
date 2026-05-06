# PCRE2 Conformance Fix Audit

> **Living document.** First published 2026-05-05 covering 2026-04-13 → 2026-05-05. Updated whenever a conformance-touching commit lands or a residual cluster opens/closes. The fix inventory (§2), theme map (§3), whack-a-mole flags (§4), and residual cross-reference (§7) move with the codebase; the systemic-gap analysis (§5) and recommendations (§6) are revised each time a recommendation lands or a new gap is identified.
>
> **Update protocol.** When a conformance commit ships: (a) add a row to §2's relevant subtable with date / commit / Δ pass / classification; (b) if it closes or weakens a §5 systemic gap, edit that gap entry and timestamp the change; (c) if it lands a §6 recommendation, mark the recommendation done and remove it; (d) if it surfaces a new whack-a-mole signature, append it to §4. The first paragraph of §1 must always cite the current `PASS_BASELINE` / `FAIL_BASELINE`.

This chapter audits every PCRE2-conformance change shipped to RGX since the differential conformance harness was introduced on 2026-04-13. The goal is to surface **whack-a-mole patterns** (per-case fixes that don't generalize) versus **principled engine changes** (fixes that ratify a general rule), and to give future contributors a basis for deciding which clusters in the residual catalogue are safe to attack one case at a time and which need a unified model first.

Companion chapter: [PCRE2 Conformance Residual](./pcre2-conformance-residual.md). Where this audit looks backward at what landed, the residual chapter looks forward at what remains.

---

## 1. Executive summary

The conformance ratchet is currently locked at **12,720 pass / 90 fail / 0 panic / 0 skip** against the full PCRE2 10.47 `testinput1..29` corpus — **~99.3%**. That number is the result of roughly 240 commits over 23 days, of which about 110 directly touched conformance behaviour. Pass-count progression visible from `CHANGES.md`: 1,061 (initial harness, testinput1 only) → 3,613 (full-corpus expansion) → 12,562 (~98% after engine fix #9) → 12,719 (2026-05-05) → **12,720** (2026-05-06, per-verb effects Phase 2 closes Cluster 1D testinput1:5457). The work decomposes into three mostly disjoint tracks running in parallel:

- A **harness track** (≈90 commits): reclassify untestable PCRE2 features, decode subject-line escapes correctly, gate test-only modifiers, recognise pcre2test directive blocks. Most of the early absolute-volume wins (the +961, +409, +234, +161 single-commit bumps in April 2026) came from this track. None of it changes engine behaviour; it changes which divergences the harness counts.
- A **PGEN-as-parser track** (≈40 commits + 82 issue reports): each parser-side bug or AST-shape gap in the failing patterns gets a `PGEN-RGX-NNNN` report filed against the `subs/pgen` submodule per the cluster-first protocol; closure arrives via a submodule bump and an adapter migration in `rgx-core/src/parsing.rs`. **Zero RGX-side parser workarounds** exist — the no-PGEN-workarounds rule held throughout. The cost is a tightly-coupled walker that has been rewritten three times to absorb PGEN's typed-shape campaign (slices 1–42 across PGEN releases 1.1.10 through 1.1.75).
- An **engine track** (38 numbered fixes plus ~10 unnumbered) — overwhelmingly localised in `rgx-core/src/vm.rs`. This is where the audit's central tension lives.

The central tension is that the engine track is **conformance-driven** rather than **spec-driven**. Each numbered engine fix recovered a small cluster of failing cases (median ~3 cases, range 1–18); each was proximally caused by a specific test in the corpus rather than by a code review of pcre2pattern(3) against the implementation. The result is a working engine that passes 99.3% of PCRE2's tests via a stack of targeted patches whose boundaries are defined more by which testinput cases ran red than by any intrinsic factoring of the PCRE2 semantic. Some of those patches generalise — engine fix #9's `(*THEN)` semantics, #16's `/i` char-class case-closure, the `AltScopeBegin`/`AltScopeEnd` opcode pair — but several do not. Backtracking-verb interaction in particular is now a layered set of point fixes (engine fixes #9, #17, #18, #19, #21, #23, #24, #25, #27, #28, #33, #34, #35, #36, #37 plus the 2026-05-05 SKIP-overrides-COMMIT scanning-loop fix and the in-progress `commit_saved_alt` proposal) where each closes a specific verb interaction and the next interaction is found by running the corpus and looking for what's still red. PCRE2 patterns can mix arbitrarily many backtracking verbs in a single branch (`(*MARK:m)(*COMMIT)(*PRUNE)(*SKIP:m)(*THEN)` is legal); §5.1 below presents a per-verb effects model that scales to N verbs by composition rather than the per-pair patches that produced the layered point-fix history.

**Headline finding**: the cleanest mechanical changes in the inventory are the ones grounded in *spec reading* (the unicode-property fixes, the `\h`/`\v`/U+180E corrections, the case-fold reduction to UCD simple-fold, the `(*CRLF)` pair-anchor compile rewrite, the `\K` backtrack-unwind). The least clean are the ones grounded in *test-case observation*: backtracking-verb interactions (15+ commits, no unifying state machine documented), capture-state propagation across atomic/lookaround/subroutine boundaries (engine fixes #18, #25, #28, #29, #30, #31, #32, #37 and the 2026-04-24 negative-vs-positive tightening), and the literal-prefix/scanner start-optimization parity work (engine fixes #21, #27, #28, residual Cluster 1D). The clean fixes do not return as later regressions; the test-case-driven fixes generate follow-ups because the model behind them was never written down.

**Re-audit refinement (§9, 2026-05-06).** A focused re-audit of every fix §2 originally classified *targeted* (27 fixes) found that **23 of 27 are principled in disguise** (category A in §9.C) — the *targeted* label was assigned defensively at ship time, but the underlying fix ratifies a documented spec rule (or, in the U+180E and `[:print:]` cases, PCRE2's source-level behaviour where the man page is under-specified). The original *"engine track is conformance-driven rather than spec-driven"* framing is correct as a process observation but overstates the per-fix story: most fixes were correct readings of pcre2pattern(3) that went in under conservative labels. The single category-B finding — engine #13's `CharClass::Custom::ci_override_ranges` for `\P{Lu/Ll/Lt}/i` — was **closed 2026-05-06** by a family-aware case-fold-closure refactor (`unicode_support::case_fold_property_closure` is now the single source of truth for the case-distinguished property family — Lu/Ll/Lt/L&/Lc/Cased_Letter/Upper/Lower/Cased and aliases — across both polarities and both standalone/in-class contexts; see §9.B B1). **3 fixes** are members of the §5.1 verb-effects family (#24, #36, the 2026-05-05 SKIP-overrides-COMMIT) and collapse into rows of the per-verb `apply` table that §6.2.1 scopes; Phase 1 of that refactor landed the same day. **1 fix** (#6 case-fold ASCII ranges) is already subsumed by later commits. Final tally: A: 23 / B: 0 / C: 1 / D: 3, with only the verb-effects-Phase-2 deferred-stack work remaining as a known carry-over.

**Per-verb effects refactor — Phases 1 & 2 shipped 2026-05-06.** The §5.1 / §6.2.1 model is landed. Each backtracking verb has a single `verb_apply_*` associated function on `RegexVM` (`rgx-core/src/vm.rs:2200-`); all three dispatch sites (top-level, continuation, subexpr) call the same functions. Last-verb-wins precedence is encoded inside each apply function. **Phase 2** defers the stack-clear effect of `(*COMMIT)` (non-atomic branch) so a following `(*THEN)` can reach the alt-fallback frame; `try_backtrack` honors `committed` and `OpCode::Char`/`OpCode::Fail` are routed through it; `ThenOutcome` is now trichotomous (`Redirected` / `ScopeExhausted` / `FullyDegraded`) using `alt_scope_marks` for the lexical-scope distinction. Conformance ratchet **12,720 / 90** (+1 pass — closes residual Cluster 1D testinput1:5457 by construction). `(*SKIP)` keeps eager stack-clear (deferring would regress SKIP-alone semantics); SKIP+THEN compositions stay as in baseline (testinput1:5447 remains span-mismatch). A Phase 3 with a `pending_alt_revival` side-slot consumed by THEN would generalize the closure to SKIP+THEN — tracked at `docs/BACKLOG.md` C8.2.1.

The next sections enumerate every fix that landed, group them by theme, and identify where the layered-patches-without-a-model pattern is most likely to spawn more whack-a-mole if approached one case at a time.

---

## 2. Fix inventory

The table below covers every conformance-relevant commit between 2026-04-13 (harness landed) and 2026-05-05 (HEAD). Performance commits and pure documentation commits are excluded. "Δ pass" is the conformance-test pass-count delta as reported in the commit's CHANGES.md entry; values prefixed with **+** are absolute, not relative to the next commit.

The classification column uses three labels:

- **principled** — the fix changes a general rule and would have been the right implementation choice given the relevant PCRE2 spec text, irrespective of which specific test case surfaced it.
- **targeted** — the fix recovered a small set of related cases by altering one dispatch site or AST shape; the change is correct as far as it goes but is shaped by the failing examples rather than by an underlying invariant the engine intends to maintain.
- **harness-only** — no engine semantic change; either reclassifies a PCRE2-only feature as untestable, fixes a harness parsing/decoding bug, or threads a per-subject pcre2test modifier through the harness.

### 2.1 Engine fixes (numbered)

The 38 numbered engine fixes — explicit "engine fix #N" tags in `CHANGES.md` and `MEMORY.md`. Each touches `rgx-core/src/vm.rs` directly except where noted.

| # | Date | Subject | Δ pass | Theme | Classification |
|---:|---|---|---:|---|---|
| 1 | 2026-04-13 | `{0}`/`{0,0}` capture compile crash; AST-observed group-count sizing | crash fix | quantifier codegen | principled |
| 2 | 2026-04-13 | Char-class operand overflow on high-min counted quantifier; CompiledCharClass dedup | crash fix | char-class compile | principled |
| 3 | 2026-04-13 | `\0` parsed as NUL byte, not Backreference(0) | +3 (testinput1) | parser/lexer | principled |
| 4 | 2026-04-13 | `\NNN` octal fallback when group N doesn't exist | +1 (testinput1) | parser/lexer | principled |
| 5 | 2026-04-13 | Lower extended-char-class inside FlagGroup (panic→0) | crash fix | compile pipeline | principled |
| 6 | 2026-04-14 | Case-fold ASCII ranges spanning both cases | targeted | case-fold | targeted (later subsumed by #14/#16) |
| 7 | 2026-04-21 | `Custom{negated:true}` honoured for UCP `\W`/`\D`/`\S` in class | +17 | UCP class | principled |
| 8 | (subsumed by #14) — case-insensitive backref UCD simple-fold | +6 | case-fold | principled |
| 9 | 2026-04-22 | Full alternation-aware `(*THEN)` semantics; introduces `alt_boundaries` stack | +18 | verb dispatch | **principled** |
| 10 | 2026-04-22 | UCP `\w` includes M + Pc | +2 | UCP class | principled |
| 11 | 2026-04-22 | `.`/`\N` under `(*CRLF)` compiles to `(?!\r\n)<any>` | +2 | newline | principled |
| 12 | 2026-04-22 | `\b`/`\B` UCP word-char aligned with expanded `\w` | +2 | UCP class | principled |
| 13 | 2026-04-22 | `CharClass::Custom::ci_override_ranges` for `\P{Lu/Ll/Lt}` in `[…]` | 0 (lifts gates) | case-fold | targeted (per-item provenance band-aid) |
| 14 | 2026-04-22 | `/i` case-variants use UCD simple-fold only | +5 | case-fold | principled |
| 15 | 2026-04-22 | `X+` codegen switches to Split-based inlining when body has alt/inner-quant | +3 | quantifier codegen | principled |
| 16 | 2026-04-22 | `/i` char-class range folding via full case closure | +8 | case-fold | principled |
| 17 | 2026-04-22 | `(*COMMIT)` clears the backtrack stack, not just the abort flag | +3 | verb dispatch | principled |
| 18 | 2026-04-22 | `(*ACCEPT)` emits dedicated opcode `0xF2`; bubbles through subexpr/probe | +5 | verb dispatch | principled |
| 19 | 2026-04-22 | `(*COMMIT)` inside atomic group uses `COMMIT_SENTINEL_IP` | +3 | verb dispatch | principled |
| 20 | 2026-04-22 | Branch-reset subroutine calls resolve to leftmost group | +4 | subroutine | targeted (single-rule fix) |
| 21 | 2026-04-22 | Literal-prefix scan skips past leading verbs | +1 | scan loop | targeted |
| 22 | 2026-04-22 | `X*` greedy switches to Split-based inlining | +5 | quantifier codegen | principled (mirror of #15) |
| 23 | 2026-04-22 | Subexpr `(*PRUNE)`/`(*THEN)` with no enclosing alt propagate to outer | +3 | verb dispatch | targeted |
| 24 | 2026-04-23 | `(*PRUNE)` clears any pending `(*SKIP)` mark | +2 | verb interaction | targeted |
| 25 | 2026-04-23 | Subroutine calls rewrap body in enclosing FlagGroup scope | +11 | subroutine | principled |
| 26 | 2026-04-23 | Atomic-group codegen suppresses `(?U)` swap_greed | +2 | quantifier flag | targeted |
| 27 | 2026-04-23 | Subexpr `(*COMMIT)` doesn't clear local stack | +2 | verb dispatch | targeted |
| 28 | 2026-04-23 | `(*COMMIT)` propagates on assertion failure; `try_backtrack` honours `committed` | +2 | verb propagation | principled (gated on positive-only by 2026-04-24 cleanup) |
| 29 | 2026-04-23 | `Call` opcode pushes empty-match retry frame | +7 | subroutine | principled |
| 30 | 2026-04-24 | Subexpr/continuation `Call` also push retry frame | +3 | subroutine | targeted (completes #29 across all dispatch sites) |
| 31 | 2026-04-24 | Lookbehind body keeps full subject visible | +2 | lookaround | principled |
| 32 | 2026-04-24 | Lookbehind body honours must-end-at target for greedy backtrack | +2 | lookaround | principled |
| 33 | 2026-04-24 | Subexpr `(*THEN)` uses local alt-boundary stack | +3 | verb dispatch | targeted |
| 34 | 2026-04-24 | New `AltScopeBegin`/`AltScopeEnd` opcodes for `(*THEN)` lexical scope | +3 | verb dispatch | **principled** |
| 35 | 2026-04-24 | `StarLazy`/`PlusLazy` propagate `(*ACCEPT)` from probed body | +2 | verb propagation | targeted (parallel of #18 in lazy quantifiers) |
| 36 | 2026-04-24 | `(*PRUNE)` clears pending `(*COMMIT)` abort | +1 | verb interaction | targeted |
| 37 | 2026-04-24 | `(*SKIP)` inside failing lookahead propagates to outer | +1 | verb propagation | targeted (mirror of #28 for SKIP) |
| 38 | 2026-04-24 | Dupnames conditional checks ANY instance; new `CONDITIONAL_KIND_NAMED_GROUP_EXISTS_ANY` | +3 | dupnames | **principled** |

### 2.2 Engine fixes (unnumbered, still `vm.rs` / compiler / engine semantic)

| Date | Subject | Δ pass | Theme | Classification |
|---|---|---:|---|---|
| 2026-04-17 | Octal-then-literal fallback for multi-digit numeric backrefs | targeted | parser/lexer | principled |
| 2026-04-17 | `\c<char>` control escape: PCRE2 XOR 0x40 rule | targeted | parser/lexer | principled |
| 2026-04-17 | Scoped flag-disable `(?-x:...)` correctly disables x-mode | targeted | flag scope | targeted |
| 2026-04-17 | Case-insensitive numbered backref via new `BackrefCaseInsensitive` opcode | targeted | case-fold | principled |
| 2026-04-17 | Positive lookaround captures propagate to outer scope | targeted | lookaround | principled |
| 2026-04-17 | Unscoped `(?flags)` toggle propagates across alternation branches | targeted | flag scope | targeted |
| 2026-04-18 | Zero-width quantifier iteration terminates loop | +6 | quantifier loop | principled |
| 2026-04-18 | Unicode simple-fold under `/i` | +161 | case-fold | **principled** (largest single-commit win) |
| 2026-04-18 | VT in `\s`, `{,N}` bare upper-bound | +26 | parser semantic | principled |
| 2026-04-18 | Class-context escape semantics + runtime-policy verb no-ops | +19 | parser/verb | targeted |
| 2026-04-18 | PCRE2_UCP: Unicode-aware `\d`/`\w`/`\s` and POSIX classes | +31 | UCP class | principled |
| 2026-04-18 | QuestionGreedy zero-width preserves captures | +1 | quantifier | targeted |
| 2026-04-18 | Substitute: PCRE2 backslash escapes + case-change | +7 | substitute | targeted |
| 2026-04-19 | `\g<...>` / `\g'...'` as subroutine call | +21 | subroutine | principled |
| 2026-04-19 | UCP `[:graph:]` / `[:print:]` include Cf + Co | +29 | UCP class | principled |
| 2026-04-19 | `\h` U+180E + Xsp/Xps/Xwd Unicode expansion | +59 | UCP class | principled |
| 2026-04-19 | Quantifier retargets across transparent atoms | +10 | quantifier | targeted |
| 2026-04-19 | `\p{Lu}/i` etc. expand to `\p{L&}` | +8 | case-fold | targeted (later partially subsumed by #13) |
| 2026-04-19 | `(?U)` / `/ungreedy` swaps quantifier greediness | +4 | flag scope | principled |
| 2026-04-20 | `(*BSR_…)` pragmas restrict `\R` | +20 | newline | principled |
| 2026-04-20 | `(*CR)`/`(*LF)`/`(*CRLF)`/`(*ANYCRLF)`/`(*ANY)`/`(*NUL)` change `.`/`\N` | +40 | newline | principled |
| 2026-04-20 | Newline pragmas govern `^`/`$` line anchors under `/m` | +20 | newline | principled |
| 2026-04-20 | `\K` reset unwinds on backtrack (`saved_match_start_override` field) | +3 | match-reset | **principled** |
| 2026-04-21 | `(*CRLF)` + `(*ANY)` line anchors treat `\r\n` as one unit | +8 | newline | principled |
| 2026-04-21 | `\b`/`\B` honour PCRE2_UCP | +13 | UCP class | principled |
| 2026-04-21 | `OpCode::GraphemeCluster` dispatch in `execute_subexpr_inner` | +35 | dispatch parity | targeted |
| 2026-04-21 | `[:print:]` UCP includes U+180E | +2 | UCP class | targeted |
| 2026-04-21 | UCP `[:xdigit:]` adds fullwidth; `[:graph:]`/`[:print:]` drop bidi-format | +17 | UCP class | principled |
| 2026-04-21 | `X?` codegen switches to Split-based | +5 | quantifier codegen | principled (mirror of #15/#22) |
| 2026-04-22 | `[:blank:]` UCP includes U+180E | +1 | UCP class | targeted |
| 2026-04-22 | `[:word:]` UCP aligned with `\w` | +1 | UCP class | targeted |
| 2026-04-23 | `\N{U+HEX}` pre-transform to `\x{HEX}` | +3 | parser/lexer | principled |
| 2026-04-23 | UCP `[:punct:]` excludes generic S*, keeps ASCII-punct-symbols | +1 | UCP class | targeted |
| 2026-04-24 | `(*CRLF)` `.` rejects BOTH ends of `\r\n` pair | +2 | newline | principled (refines #11) |
| 2026-04-24 | API substitute `$name` interpolation respects dupnames | +1 | substitute | targeted (companion of #38) |
| 2026-04-24 | Tighten assertion verb propagation to positive-only | 0 (semantic cleanup) | verb propagation | **principled** (eliminates latent #28/#37 asymmetry) |
| 2026-05-03 | Widen lookaround body length prefix u8 → u16 LE | +2 | bytecode format | principled |
| 2026-05-03 | U+180E recognised as `\s`/`[:space:]` under `/ucp` | +1 | UCP class | targeted |
| 2026-05-04 | `\p{<script>}` defaults to Script_Extensions | +2 | UCP class | principled |
| 2026-05-04 | Pattern_White_Space ignorable under `(?x,utf)` | +1 | parser semantic | principled |
| 2026-05-05 | PCRE2 quoted-run-as-range-start `[\Qabc\E-z]` | +1 | parser/adapter | targeted |
| 2026-05-05 | `(*SKIP)` overrides `(*COMMIT)` in scanning loop | +3 | verb interaction | targeted |

### 2.3 Parser / PGEN-adapter fixes

These are RGX-side adapter changes to keep up with PGEN's evolving AST shapes (slices 1–42), or to handle a parser-emitted shape that the walker had silently dropped. None of them inserts a workaround against PGEN; every parser bug PGEN owns has a corresponding `pgen-issues/PGEN-RGX-NNNN.yaml` report and is closed by a submodule bump rather than a local fix.

| Date | Subject | Δ pass | Notes |
|---|---|---:|---|
| 2026-04-13 → 2026-04-18 | PGEN 1.1.10 → 1.1.29 cycle (nine bumps absorbing PGEN-RGX-0016/0030/0050…0072) | several hundred cumulative | each bump closes a cluster of `should_parse_but_fails` reports |
| 2026-05-01 | Typed-shape walker rewrite for PGEN 1.1.30 → 1.1.40 | regressed −16 (recovered later) | major adapter refactor; new `convert_typed_regex` dispatch tree |
| 2026-05-05 | PGEN bump 1.1.40 → 1.1.75 + walker migration | +2 | absorbs 0078–0082, ~600 lines new dispatch |
| 2026-05-05 | Walker: accept boolean `initial_close: true` for leading-`]` char_class | +5 | silent typed-shape gap |
| 2026-05-05 | Walker: typed `class_quoted_literal` in quoted-run-as-range-start | +1 | silent typed-shape gap |
| 2026-05-05 | Walker: `quoted_literal` body flattens sub-array elements | +1 | silent typed-shape gap |
| 2026-05-05 | Walker: `class_quoted_literal` body flattens sub-array elements | +2 | silent typed-shape gap |

### 2.4 Harness-only fixes (sample of the largest by Δ pass)

The harness track has ~90 commits; the table below lists only the ones that moved the ratchet by ≥30 cases. None of these changes engine semantics; they correct the harness's classification of pcre2test syntax that RGX has no need to model.

| Date | Subject | Δ pass | What it actually was |
|---|---|---:|---|
| 2026-04-20 | Truncate subject at pcre2test `\=` modifier separator | **+961** | the harness was matching against the literal `\=g` suffix |
| 2026-04-20 | Per-subject untestable-modifier detection | +409 | per-subject pcre2test modifiers (`\=allcaptures`, `\=mark`, etc.) |
| 2026-04-21 | Harness: `alt_extended_class` / `allow_empty_class` / `callout_none` untestable | +234 | PCRE2-only modifiers RGX doesn't model |
| 2026-04-21 | Pattern-body gate for ASCII/caseless_restrict + script_run | +125 | PCRE2-only verb families |
| 2026-04-19 | `/g` first-match anchor for comparison | +120 | `/g` global-match output formatting |
| 2026-04-21 | Scan every line of directive block for `#subject dfa` + `(*NOTEMPTY)` body gate | +100 | pcre2test directive-block parsing |
| 2026-04-20 | Recognise `Partial match:` as `Expected::PartialMatch` pass-through | +98 | partial-match output format |
| 2026-04-20 | Subject-level `Failed:` → `NoMatch`, not compile error | +84 | pcre2test "Failed:" line format |
| 2026-04-19 | UTF-8 encode `\x{NN}` under `/utf` | +80 | subject-line escape decoding |
| 2026-04-20 | Widen untestable set to ovector/callout/diagnostic modifiers | +60 | pcre2test diagnostic modifiers |
| 2026-04-20 | Add `ps`/`ph`/`partial_soft`/`partial_hard` to untestable set | +42 | partial-match modifiers |
| 2026-04-21 | `is_subject_echo` accepts 3–7 space indents | +35 | pcre2test echo-line formats |
| 2026-04-21 | `OpCode::GraphemeCluster` dispatch in subexpr | +35 | dispatch-site parity (this one IS engine, listed for completeness) |
| 2026-04-21 | 2-space subject echoes close subject block | +24 | pcre2test block-boundary parsing |
| 2026-04-20 | Skip `/B` bytecode blocks in preamble | +30 | pcre2test diagnostic preamble |
| 2026-04-20 | Pattern-level untestable-modifier gate | +30 | shared classifier with per-subject path |
| 2026-04-21 | `alt_bsux` / `extra_alt_bsux` / `allow_lookaround_bsk` modifiers untestable | +29 | PCRE2-only mode flags |
| 2026-04-20 | Turkish/ASCII-restricted modifier families untestable | +76 | `(*TURKISH_CASING)` / `(*CASELESS_RESTRICT)` |

The harness-only track was the largest absolute-volume contributor to the ratchet but contributed nothing to engine quality. Its purpose is to keep the harness from counting RGX as failing on tests that probe surfaces RGX explicitly does not model (locale, EBCDIC, JIT info dumps, /B bytecode echoes, etc.).

### 2.5 Per-fix PGEN-issue absorption

Of the 82 `PGEN-RGX-NNNN` reports filed (`pgen-issues/PGEN-RGX-0001.yaml` through `PGEN-RGX-0082.yaml`), 81 are now closed and 1 (`PGEN-RGX-0073` — PGEN parse compile-time perf) remains open as a non-conformance issue. Two reports (`PGEN-RGX-0014` is a renumbering, not present on disk; the report-numbering campaign began with `PGEN-RGX-0015`) and several mid-campaign reports were retroactively flipped to closed during the 2026-04-24 user directive that declared the report-tracking surface complete. None of these reports introduced an RGX-side workaround; every one closed via a submodule bump and an adapter walker update.

---

## 3. Theme map

Every fix from §2 falls into one of nine themes. Counts here include numbered + unnumbered engine fixes; harness-only changes are excluded.

| Theme | Fix count | Delta-pass total (visible) | Convergent? |
|---|---:|---:|---|
| **Backtracking-verb dispatch** (COMMIT/PRUNE/SKIP/THEN/MARK/ACCEPT/FAIL) | 18 | ~63 | **No** — see §4.1 |
| **Capture-state propagation** (atomic / lookaround / subroutine boundaries) | 8 | ~25 | Partial — model exists for atomic, missing for napla and recursive subroutines |
| **Quantifier codegen** (Split-based inline for `?`/`+`/`*`; preserve nested backtracks) | 4 (#15, #22, the `X?` fix, #1 sizing) | ~14 | Yes — three commits ratify the same inlining rule, complete by #22 |
| **UCP / Unicode property classes** (`\w`, `\s`, `\d`, `[:posix:]`, `\p{X}`) | 14 | ~150 | Yes — driven by spec reading of UTS#18; refresh-on-Unicode-version is the only open question |
| **Case-fold under `/i`** | 5 (#6, #8, #13, #14, #16; +161 unnumbered) | ~190 | Yes — converged on UCD simple-fold + `\p{L&}` expansion. #13 `ci_override_ranges` is a band-aid carry-over |
| **Newline pragmas** (`(*CR)`, `(*LF)`, `(*CRLF)`, `(*BSR_*)`, `\R`, `.`) | 7 | ~111 | Mostly — single open issue: `(*NUL)` + `/s` interaction (Cluster 2D) |
| **Substitute-template escapes** (`\N`, `\0NN`, `${*MARK}`, dupnames) | 5 | ~17 | Partial — `(?J)` dupnames and CRLF substitution still residual (Bucket 4) |
| **Subroutine / recursion semantics** | 6 (#20, #25, #29, #30, `\g<…>`, retry-empty) | ~50 | **No** — Cluster 1A (recursive captures) and Cluster 1B (returned-capture VM semantics) are explicitly architectural follow-ups |
| **Parser/lexer semantics** (`\0`, `\NNN`, `\c<x>`, `\N{U+HEX}`, octal/named/numeric backrefs) | 8 | ~10 (most are single-case) | Yes — driven by reading pcre2pattern(3) §"Escape sequences" |

The themes that **converged on a model** are the spec-driven ones: Unicode property classes, case-fold, newline pragmas, parser/lexer escapes. In each, the relevant section of pcre2pattern(3) defines the rule, the fix encodes the rule once, and follow-ups are limited to refining table data (e.g. U+180E) or fixing one missed dispatch site.

The themes where **fixes accreted without converging on a model** are backtracking-verb dispatch, capture-state propagation, and subroutine semantics. These three themes account for 32 of the 38 numbered engine fixes, all of the in-progress work, and all of the architectural items in the residual catalogue. They are the proper subject of §4 and §5.

---

## 4. Whack-a-mole flagging

This section names the per-case fixes that did not generalize, the coupled mechanisms where a fix to A required a follow-up to B, and the dispatch sites that were retroactively shaped by failing test cases rather than by a written semantic.

### 4.1 Backtracking-verb dispatch — the deepest accretion

The verb-dispatch theme has 18 fixes. They form three sub-clusters with very different shapes:

**Sub-cluster A: single-verb base semantics (mostly principled)**

- **Engine fix #9 (`(*THEN)` alternation-aware semantics)** — introduced the `alt_boundaries: Vec<usize>` stack on `ExecContext` (declared at `rgx-core/src/vm.rs:725`), opcode `AltSplit = 0x47` (`vm.rs:143`), and `try_backtrack`'s alt-boundary popping logic (`vm.rs:2275-2278`). This is the principled core of `(*THEN)` dispatch: every alternation pushes the next-alternative frame's index on `alt_boundaries`, every `try_backtrack` pop synchronises the stack. **+18 cases**.
- **Engine fix #17 (`(*COMMIT)` clears the backtrack stack)** — corrected the original implementation that set only `ctx.committed`. Pure spec read.
- **Engine fix #18 (`(*ACCEPT)` dedicated opcode `0xF2`)** — turned `(*ACCEPT)` from a flag into a real terminator that bubbles through subexpr probes. Principled: ACCEPT's PCRE2 contract is "force success at this point", which is materially different from any other verb.
- **Engine fix #19 (`COMMIT_SENTINEL_IP`)** — introduced sentinel-frame escalation for `(*COMMIT)` inside atomic groups (`vm.rs:338` constant; `vm.rs:2274-2278` escalation in `try_backtrack`). This is principled: pcre2pattern(3) explicitly limits `(*COMMIT)`'s scope to "an enclosing atomic group", and the sentinel is a clean encoding.
- **Engine fix #34 (`AltScopeBegin`/`AltScopeEnd`)** — added opcodes `0x48` and `0x49` plus the `alt_scope_marks: Vec<usize>` field (`vm.rs:732`) so `(*THEN)` resolves to its lexically-enclosing alternation, not whatever alt-frame happened to still be on the stack. The closure of an inner alternation now truncates `alt_boundaries` cleanly. Principled — fixes a class of bug, not one case.

**Sub-cluster B: verb-pair interactions (where the whack-a-mole lives)**

The remaining fixes are pairwise:

- **#24** — `(*PRUNE)` clears any pending `(*SKIP)` mark
- **#36** — `(*PRUNE)` clears any pending `(*COMMIT)` abort
- **2026-05-05 commit `4fb3980`** — `(*SKIP)` overrides `(*COMMIT)` in scanning loop

These three are siblings: each closes one ordered pair from the family of "what wins when verbs A and B both fired". The catalogue's Cluster 1D entry for `(*COMMIT)(*THEN)` (testinput1:5457) is the next member of the same family. The "live work in progress" `commit_saved_alt: Option<BacktrackFrame>` proposal that the user is currently iterating on closes that fourth pair specifically — and the proposal's current shape is a side-slot that stashes the alt-fallback frame at COMMIT time so a following THEN can re-enter it. The design choice is reasonable as a local fix; the diagnostic question is whether the underlying pattern signals that *every* verb pair needs a slot, not just COMMIT+THEN. With six independent verbs (COMMIT, PRUNE, SKIP, THEN, ACCEPT, FAIL/`(*F)`) and six locations (top-level, inside alternation, inside atomic, inside positive lookaround, inside negative lookaround, inside subroutine), the matrix has up to ~36 ordered pairs × 6 contexts = ~216 cells; the corpus has only exercised perhaps 25 of them.

The whack-a-mole signature in this sub-cluster is exactly:

```
fix N    : (*X)(*Y) in context C → recovers M cases
fix N+k  : (*Y)(*Z) in context C → recovers M' cases  (because the N fix didn't model Y at all)
fix N+2k : (*X)(*Y) in context D → recovers M'' cases (because nothing said the C fix should generalize to D)
```

Each of #24, #36, the SKIP-overrides-COMMIT scanning-loop fix, and the in-progress `commit_saved_alt` proposal fits this signature. Concrete code reference: `rgx-core/src/vm.rs:2862-2864` shows `(*PRUNE)` clearing both `skip_position` and `committed`:

```rust
ctx.backtrack_stack.clear();
ctx.skip_position = None;
ctx.committed = false;
```

…with a 7-line comment explaining "PCRE2's documented behaviour is that PRUNE's normal scanner-advance supersedes SKIP's advance-to-mark and COMMIT's don't-advance-at-all". That comment would be a verb-precedence rule if it were stated for every pair; today it's stated only for the pairs the corpus exercised. The "PCRE2 semantic for verb interactions" comment in the 2026-05-05 SKIP-overrides-COMMIT fix at `rgx-core/src/vm.rs:1879-1885` is in the same shape — local, specific, prose-only, no machine-readable model.

**Sub-cluster C: verb-propagation across boundaries (the latent-asymmetry pattern)**

This is the most instructive whack-a-mole instance because it self-disclosed:

- **Engine fix #28 (2026-04-23)** — `(*COMMIT)` propagates on assertion failure. Code added at `execute_assertion_subexpr`: `if !body_matched && assertion_ctx.committed { ctx.committed = true; }`.
- **Engine fix #37 (2026-04-24)** — `(*SKIP)` inside failing lookahead propagates. Code added immediately after #28's block: `if !body_matched { if let Some(skip_pos) = assertion_ctx.skip_position { ctx.skip_position = Some(skip_pos); ctx.committed = true; } }`. CHANGES.md note: *"Mirrors engine fix #28's COMMIT-on-assertion-failure rule exactly."*
- **2026-04-24 cleanup commit (no `Δ pass`)** — the cleanup commit's CHANGES.md entry says it best: *"engine fix #37 shipped unconditional `skip_position` / `committed` propagation on `!body_matched`. That's correct for positive assertions … but wrong for negative assertions. The pre-existing engine-fix-#28 `committed` propagation had the same latent asymmetry. No test in the current corpus exercises the divergence, but a negative lookahead like `(?!b(*SKIP)a)bnn` on `bnn` would have leaked SKIP and aborted the outer match incorrectly."* The fix combined both blocks under a single `propagate_captures && !body_matched` gate (current code at `rgx-core/src/vm.rs:6551-6559`):

```rust
if propagate_captures && !body_matched {
    if assertion_ctx.committed {
        ctx.committed = true;
    }
    if let Some(skip_pos) = assertion_ctx.skip_position {
        ctx.skip_position = Some(skip_pos);
        ctx.committed = true;
    }
}
```

This is the cleanest example in the inventory of the conformance-driven design pattern: fix #28 was correct for the case the corpus exercised (a positive lookahead with COMMIT), fix #37 mirrored it for SKIP (also correct for a positive lookahead), and only the symmetry-checking commit caught that **both** had been wrong for negative lookaheads — and the commit's own note acknowledges no corpus case exercised the divergence. The commit shipped as a no-pass-delta semantic correction. That is exactly the evidence the conformance corpus is shaping the implementation: the bug existed for ≥ 24 hours past fix #28, the corpus had no test for it, and only an explicit symmetry audit found it.

The same audit lens applied to the rest of the verb sub-cluster B fixes would likely surface several more latent asymmetries.

### 4.2 Capture-state propagation across boundaries

Eight fixes touched capture-state propagation:

- **Engine fix #18 (`(*ACCEPT)`)** carries `ctx.accept_forced` through subexpr layers; the saved-and-restored pair pattern at `invoke_subroutine` (`rgx-core/src/vm.rs:2368-2369`) is the documented invariant that ACCEPT inside a subroutine is scoped to that subroutine.
- **Engine fix #25 (subroutine flag-scope rewrap)** — subroutine calls preserve enclosing `(?i:)` / `(?s:)` scope. Compile-time fix.
- **Engine fix #28 + #37 + 2026-04-24 cleanup** — already covered in §4.1.
- **Engine fix #29 (Call empty-match retry frame)** — top-level only.
- **Engine fix #30** — same fix replicated at the subexpr `Call` dispatch site and the continuation `Call` dispatch site. CHANGES.md: *"Engine fix #29 added the empty-match retry backtrack frame only to the top-level `OpCode::Call` dispatch. Patterns where `(?1)` calls are reached from inside a subexpression went through the subexpr `Call` dispatch and missed the retry."*
- **Engine fix #31, #32** — lookbehind body keeps full subject visible / honours must-end-at target.
- **`\K` backtrack-unwind (2026-04-20)** — adds `saved_match_start_override: Option<usize>` to `BacktrackFrame` (`vm.rs:772`). Principled.

The pattern at #29 → #30 is a textbook **dispatch-site fan-out**: the same fix applied separately at three places because there are three places. The `Call` opcode is dispatched from `execute_at`, `execute_subexpr_inner`, and the continuation path; each was a separate code change with the same conceptual shape. The current code has effectively three copies of the empty-match retry frame logic, each maintained independently. A future fix that, say, added a third dispatch consideration (e.g. tail-call elimination, or per-call capture-isolation as Cluster 1A demands) would have to update all three sites again.

The same fan-out shows up for `try_backtrack` calls: a quick count in `rgx-core/src/vm.rs` shows **34 distinct call sites** of `self.try_backtrack(...)` across the dispatch loop, each followed by similar but not identical post-conditions. Refactoring that into a single dispatcher with explicit failure modes is one of the recommendations in §6.

### 4.3 Conformance-driven design — explicit examples

These are the places where the implementation was retroactively shaped by which testinput cases ran red:

- **Engine fix #13 `ci_override_ranges`** — the CHANGES.md entry frames this as a temporary band-aid: *"Proper engine fix for the `\p{Lu/Ll/Lt}/i` case-fold expansion gap that commit `509744f` harness-gated as a temporary measure."* The fix carries per-item provenance through `CharClass::Custom`, which was specifically introduced because the previous `complement(Lu)` resolution at parse time interacted badly with `/i` case-fold expansion, which in turn was fixed because the corpus had `\P{Lu}/i` cases. The proper engine fix per the CHANGES.md text would be to thread `\p{X}/i` case-fold expansion through class-item provenance generally; the `ci_override_ranges` field is explicitly a "specifically substitute the `complement(L&)` expansion for those items" hack.
- **Engine fix #21 (literal-prefix scan skips past verbs)** — added because `(*COMMIT)ABC` on `"DEFABC"` failed, which it did because the literal-prefix optimization assumed no instruction *before* the literal could change scanner semantics. The fix lets the scan skip past verbs at the head of the program. The fix is shaped exactly to the case: it doesn't generalize to "the scan ought to know about every operation that could change it", it adds a verb skip. CHANGES.md identifies the limitation in the same paragraph: *"PCRE2's start-optimization skips the COMMIT-bearing pos 0 entirely — RGX's literal-prefix scan would need to look past a leading `a?` quantifier to achieve the same."* That follow-up case is now Cluster 1D in the residual catalogue.
- **Engine fix #27 (subexpr `(*COMMIT)` no longer clears local stack; harness widens `no_start_optimize` gate)** — the *and* in the title is significant: half the fix was VM, half was harness gating. The VM half was correct; the harness half was acknowledging that two further cases (testinput2:6610, 6613) couldn't be fixed without changing how the literal-prefix scan composes with `a?`-prefixed COMMITs, so they were marked untestable. The semantic gap is real and is present in the residual catalogue at Cluster 1D as start-optimization-past-`a?`.
- **Engine fix #20 (branch-reset subroutine resolves to leftmost group)** — recovered four cases. PCRE2's behaviour here is documented in pcre2pattern(3) §"Branch reset"; the fix is principled but the *spec* part of the work — checking what *all* the branch-reset cross-features do — wasn't done. Whether `(?|...)` interacts correctly with `(*ACCEPT)`, `\K`, or named conditional checks is currently unvalidated.

### 4.4 Coupled-mechanics signatures in the inventory

Several places where a fix to mechanism A required a follow-up fix to mechanism B because the engine model was incomplete:

- **#11 `(*CRLF)` `.` → `(?!\r\n)<any>`** required follow-up **2026-04-24** `(*CRLF)` `.` rejects BOTH ends of `\r\n` pair. The first fix protected the start of the CR/LF pair; the second protected the end via `(?!\r\n|(?<=\r)\n)<any>`. The inner lookbehind in the second alternative scopes the prev-`\r` check to `\n`-only positions, so bare `\r` followed by non-`\n` still matches.
- **#15 `X+` Split-based inlining** required follow-up **#22 for `X*`**, then a separate fix for `X?` (committed `d6cfa5f`). All three apply the same rule (switch to Split-based inlining when the body has alternation or inner quantifier) but each got its own commit because the codegen for each quantifier is a different switch arm.
- **#28 + #37 + 2026-04-24 cleanup** — described in §4.1.
- **#18 `(*ACCEPT)` opcode** required harness fix `endanchored` no-match branch post-checks match end (CHANGES.md 2026-04-22): *"`(*ACCEPT)` now bubbles through the enclosing `\z` (per engine fix #18), so the wrap no longer enforces end-of-subject."* The harness wrap that guards `endanchored` was correct *before* fix #18 because `(*ACCEPT)` didn't bubble; the engine fix made the harness wrap incorrect. This is a coupled-mechanism signature pointing in the opposite direction — fixing the engine broke a harness assumption.
- **PGEN bumps + walker migrations** form a continuous coupled-mechanism stream. Every PGEN typed-shape change requires a walker update, and four post-PGEN-1.1.75-bump silent-shape sweeps closed previously-passing cases that had silently regressed. The CHANGES.md entries call this "silent typed-shape gap" — a structurally honest name for "the walker ignored a PGEN field it didn't know about and a test silently went from passing-by-coincidence to failing-by-coincidence". The 2026-05-05 sweep itself contains four such fixes (`initial_close: true`, `class_quoted_literal` typed shape, `quoted_literal` body sub-array, `class_quoted_literal` body sub-array), each recovering 1–5 cases, none of them surfaced by anything other than running the conformance corpus.

---

## 5. Systemic gaps

What unified models are missing? Five gaps, in roughly decreasing order of impact:

### 5.1 Backtracking-verb dispatch — per-verb effects model (scales to N verbs in a branch)

The backtracking verbs `(*COMMIT)`, `(*PRUNE)`, `(*SKIP)`, `(*SKIP:name)`, `(*THEN)`, `(*ACCEPT)`, `(*FAIL)` / `(*F)`, `(*MARK:name)` can appear in arbitrary numbers and orderings inside a single alternation branch — there is no syntactic limit. A pattern like `(*MARK:m)(*COMMIT)(*PRUNE)(*SKIP:m)(*THEN)` is legal PCRE2 and the engine must produce the right behaviour from the verb sequence as a whole, not from any particular pair. The original framing of this gap as a *precedence matrix of ordered pairs* — used in earlier drafts of this audit and in the per-pair fix history (engine fixes #9, #17, #19, #21, #23, #24, #25, #27, #28, #33, #34, #35, #36, #37, plus the 2026-05-05 SKIP-overrides-COMMIT scanning-loop fix) — is therefore **insufficient**: it answers what `(*A)(*B)` means but not what `(*A)(*B)(*C)…` means.

The principled model is **per-verb effects on engine state**. Each verb is a pure transition function on the failure-relevant subset of `ExecContext`:

```rust
struct VerbState {
    committed:        bool,                    // (*COMMIT) sets; later verbs may clear
    skip_position:    Option<usize>,           // (*SKIP) / (*SKIP:name) sets
    then_pending:     bool,                    // (*THEN) sets
    accept_forced:    bool,                    // (*ACCEPT) sets
    fail_forced:      bool,                    // (*FAIL) sets
    mark_trail:       Vec<(String, usize)>,    // (*MARK:name) appends
    backtrack_policy: BacktrackPolicy,         // unrestricted / cleared-by-PRUNE / cleared-by-COMMIT / etc.
}

trait VerbEffect {
    fn apply(&self, state: &mut VerbState, ctx_pos: usize);
}
```

A branch's verb sequence is then `state = verbs.iter().fold(state, |s, v| v.apply(s, pos))` — N verbs apply by composition with no special-cased pair logic. Verbs that "override" earlier ones do so because their `apply` clears the earlier verb's flag (e.g. `(*PRUNE)` zeros `committed` and `skip_position`; `(*SKIP)` zeros `committed`). The per-pair rules in today's code are special cases of this composition; with the effects model they collapse into single-verb effect definitions.

Concrete worked example for `(*MARK:m)(*COMMIT)(*PRUNE)(*SKIP:m)(*THEN)`:

| Step | Verb | State after `apply` |
|------|------|---------------------|
| 0 | (initial)        | `{ committed=F, skip=None, then=F, mark_trail=[] }` |
| 1 | `(*MARK:m)`      | `{ committed=F, skip=None, then=F, mark_trail=[("m", pos)] }` |
| 2 | `(*COMMIT)`      | `{ committed=T, skip=None, then=F, mark_trail=[("m", pos)] }` |
| 3 | `(*PRUNE)`       | `{ committed=F, skip=None, then=F, mark_trail=[("m", pos)] }`  *(PRUNE clears `committed`)* |
| 4 | `(*SKIP:m)`      | `{ committed=F, skip=Some(mark_pos("m")), then=F, mark_trail=[…] }` |
| 5 | `(*THEN)`        | `{ committed=F, skip=Some(…), then=T, mark_trail=[…] }` |

Engine failure-handling reads the final state once: if `accept_forced`, succeed; else if `then_pending` and an enclosing alternation, redirect to next alt at same position; else if `skip` is Some, scanner advances to that position; else if `committed`, scanner aborts; else default-advance. **No pair lookup is ever required** because the state already encodes the cumulative effect.

A principled implementation has three layers:

1. **`VerbEffect` table** — one entry per verb opcode (COMMIT, PRUNE, SKIP, SKIP:name, THEN, ACCEPT, FAIL, MARK:name). Each entry defines a deterministic `apply` function on `VerbState`. The table is the spec: pcre2pattern(3) prose maps to one row each.
2. **VM dispatch composes effects** — every `OpCode::*` arm for a verb calls `verb_table[op].apply(&mut ctx.verb_state, ctx.pos)` and continues. No arm contains "if a previous verb was X, override Y" logic; the override is encoded in the apply function itself.
3. **Failure-handling reads final state** — exactly one site (the scanner-loop tail / assertion-propagation site / `try_backtrack` failure return) inspects `verb_state` and decides scanner-advance, alt-redirect, or abort. Today this logic is duplicated across 8+ scanning-loop sites, the SIMD path, and the assertion-subexpr propagation; the effects model unifies it.

Properties of the effects model:
- **Scales by construction to N verbs** in a branch (the user's directive). The `apply` chain is associative if each verb's effect is monotone; for verbs whose effects clear earlier flags (PRUNE), order matters but is handled by sequential `apply` over the textual order — which is exactly what the engine sees.
- **No combinatorial explosion**: 8 verbs × 1 effect each = 8 definitions. Pair behaviours emerge from composition, triple/quadruple/N-tuple behaviours emerge from the same composition for free.
- **Test coverage requirement is linear**: one spec test per verb (does its effect match PCRE2's?) plus a small handful of composition tests (does N-verb composition produce PCRE2's output?). The pair-matrix's quadratic test obligation collapses.
- **Existing per-pair patches absorb naturally**: engine fix #24 (PRUNE clears pending SKIP) and #36 (PRUNE clears pending COMMIT) become two lines of the PRUNE `apply` function. The 2026-05-05 SKIP-overrides-COMMIT scanning-loop fix (commit `4fb3980`, 8 sites) becomes one line of the SKIP `apply` (`state.committed = false`) plus deletion of the precedence checks at every scanning-loop site.

Scope estimate: **medium** (4-7 days). Breakdown: (a) write the `VerbEffect` table from a reading of pcre2pattern(3) §"Backtracking control" — 0.5 day, (b) write per-verb spec tests against `pcre2test` to confirm each effect — 1 day, (c) refactor VM dispatch to use the table — 1-2 days, (d) collapse the failure-handling sites — 1 day, (e) write composition tests for representative N-verb sequences — 0.5-1 day, (f) delete the now-redundant ad-hoc precedence checks at each call site — 0.5 day.

### 5.2 Capture-state propagation contract across atomic / lookaround / subroutine / napla boundaries

`execute_assertion_subexpr` propagates `committed` and `skip_position` from a clone back into the caller (engine fix #28+#37+cleanup); positive lookaheads also propagate `captures` and `capture_trail` (engine fix from 2026-04-17); negative lookaheads suppress capture propagation. Subroutine calls preserve and restore `accept_forced` (`rgx-core/src/vm.rs:2368-2369`). Atomic groups push `COMMIT_SENTINEL_IP`. None of these is wrong, but the rule for "what state propagates across this boundary" is nine separate cases scattered across `clone_exec_context`, `execute_assertion_subexpr`, `execute_lookbehind_assertion`, `invoke_subroutine`, `OpCode::AtomicStart` / `AtomicEnd` dispatch, and `OpCode::Commit` (at `rgx-core/src/vm.rs:2830-2843` which has the call_stack-vs-empty branch).

A unified contract would be a single `BoundaryPolicy` struct/enum carrying:

```rust
struct BoundaryPolicy {
    propagate_captures_on_success: bool,   // positive vs negative
    propagate_captures_on_failure: bool,   // always false today
    propagate_committed: ScopeRule,        // outer / scoped-to-this-boundary / never
    propagate_skip_position: ScopeRule,    // ditto
    propagate_accept: ScopeRule,           // ditto
    inherit_match_start_override: bool,    // \K isolation policy
    // ...
}
```

…with one `BoundaryPolicy` value per boundary kind (positive_lookahead, negative_lookahead, positive_lookbehind, negative_lookbehind, atomic_group, subroutine_call, napla, naplb). The current dispatch sites would consult the policy instead of having local logic for each propagation flag.

The big payoff is the residual Cluster 1C (`(*napla:...)`): the cluster description in the residual catalogue says napla is currently routed through ordinary positive-lookahead code, but napla's defining property is that it's **non-atomic** — backtracking from outside can re-enter the lookahead body, which means `propagate_captures_on_failure = true` and the lookahead's frames need to live on the outer stack. With a `BoundaryPolicy` framework, napla is one new value with a different choice for two flags. Without it, napla is going to be ~5-10 commits of the same kind of per-case-fix work that backtracking-verb dispatch produced.

Scope estimate: **medium-large** (5-10 days). The state diagram can be derived from pcre2pattern(3) §"Lookaround assertions" and §"Atomic grouping and possessive quantifiers" plus the napla extension in PCRE2 10.34. Implementation is a struct + 9 constants + a dispatch refactor; testing is the heavier half.

### 5.3 `try_backtrack` auto-cleanup contract

There are 34 call sites of `self.try_backtrack(ctx, &mut ip)` in `vm.rs`. The function (defined at `rgx-core/src/vm.rs:2254`) does several things:

1. Honours `ctx.committed` (clears stack and returns false; `vm.rs:2262-2266`)
2. Pops a frame; escalates on `COMMIT_SENTINEL_IP` (`vm.rs:2274-2279`)
3. Restores the frame's state via `restore_frame` (which itself unwinds the trail and `match_start_override`)
4. **Synchronises `ctx.alt_boundaries` with the new stack length** (`vm.rs:2288-2291`, `while ctx.alt_boundaries.last().map_or(false, |&b| b >= new_len) { ctx.alt_boundaries.pop(); }`)

The synchronisation in step 4 is a defensive cleanup added with engine fix #9 and reaffirmed by #34. Some manual call sites also clean up `alt_boundaries` directly (the `OpCode::Then` dispatch at `vm.rs:2891-2893` does its own pop-loop). The dual cleanup paths are correct today but easy to break — if a future opcode introduces a new per-frame side state, every manual cleanup site needs to update too.

A principled fix is to make `try_backtrack` the single place that knows about cross-stack invariants, and to forbid manual `alt_boundaries.truncate()` calls outside of opcode arms that explicitly need to reshape it (`AltScopeEnd`, `(*THEN)`'s redirect, the COMMIT-clears-everything path). Today there are ~15 places that touch `alt_boundaries` directly; an audit would converge them.

Scope estimate: **small** (1-3 days). Mostly a code-organization commit with no semantic change; risk is mostly in the alt-scope marks for `(*THEN)` lexical scope, which interact with `alt_boundaries` non-locally.

### 5.4 `COMMIT_SENTINEL_IP` routing rules

The sentinel is consumed in `try_backtrack` (`vm.rs:2274-2278`) and pushed in two places: `OpCode::Commit` when `ctx.call_stack` is non-empty (`vm.rs:2830-2843`) and `OpCode::Commit` in `execute_subexpr_inner` (around line 5482). The decision for "is this an atomic group context?" is currently approximated by `ctx.call_stack.is_empty()`, which is true at top-level and false inside a subroutine call — but the actual question is "are we inside an atomic group?", not "are we inside any function call". The two coincide *for the cases the corpus exercises* (no test combines `(*COMMIT)` inside a subroutine that isn't itself inside an atomic group), but they are not the same predicate.

A correct model would carry an explicit `atomic_depth: u32` counter on `ExecContext` incremented by `OpCode::AtomicStart` and decremented by `OpCode::AtomicEnd`. `OpCode::Commit` would test `atomic_depth > 0` instead of `!call_stack.is_empty()`. This is a one-field change and a one-line predicate flip; the only reason it hasn't shipped is that no test in the corpus runs the divergent case.

Scope estimate: **trivial** (≤ 1 day). The risk is non-corpus regressions; the value is closing a known semantic gap.

### 5.5 Pike-VM vs backtracking-VM dispatch gating

The 2026-05-05 root-cause analysis on testinput2:6244/6249 (in `book/src/internals/pcre2-conformance-residual.md` Cluster 1G) pinned the divergence to a contract documented in `Engine::should_dispatch_to_c2` (engine.rs:1770): Pike-VM is gated off when ANY runtime match limit is set, because Pike-VM doesn't (yet) honour those limits. This is a documented choice — the contract says *"patterns relying on [limits] continue to run on the existing backtracking VM"* — but it has the consequence that *correctness-preserving* dispatch (Pike-VM, linear time, can match `\A\s*(a|...)/I` on `"a"` instantly) is sacrificed for *limit-honouring* dispatch (backtracking VM, fails at 1M steps).

The systemic gap here is that Pike-VM's defining property — guaranteed linear time — is exactly the property that makes per-step limits **less necessary** than they are for backtracking. A principled resolution:

- **Option A**: thread `max_steps` through Pike-VM as a state-transition counter (Pike-VM increments per state visit; the count is bounded by `O(n × pattern_size)`).
- **Option B**: remove the limit gate from `should_dispatch_to_c2` for limits whose purpose is catastrophic-backtracking protection; document that limits are advisory for Pike-VM-eligible patterns.

Either is a deliberate engine-session call. The current state — leaving the gate in place — is documented but produces test-corpus divergences (testinput2:6244/6249) that look like bugs to anyone reading the failure list cold.

Scope estimate: **medium** (2-5 days for either option), plus a contract-change discussion before implementation.

---

## 6. Recommendations

Prioritized. Each item names what to do, why, scope, and what it would unlock.

### 6.1 Now — small, high-leverage cleanups

**1. Document the per-verb effects table.** Write the eight `apply` functions for COMMIT, PRUNE, SKIP, SKIP:name, THEN, ACCEPT, FAIL, MARK:name as Rust pseudocode in this audit, citing the pcre2pattern(3) §"Backtracking control" lines that justify each effect and the current commit that implements (or fails to implement) it. The act of writing it is what surfaces missing or asymmetric effects — including the existing #24 / #36 / #34 / 2026-05-05 patches that are special cases of the table. **Scope: 0.5 day. Unlocks: §6.2 item 1.**

**2. Replace `!ctx.call_stack.is_empty()` with `ctx.atomic_depth > 0` in `OpCode::Commit`.** Add the field, increment/decrement at `AtomicStart`/`AtomicEnd`, flip the predicate. **Scope: 0.5 day. Unlocks: a hidden-but-known correctness gap.**

**3. Audit the 8 manual `alt_boundaries` truncation sites for consistency with `try_backtrack`'s post-pop cleanup.** Either convert each manual pop into a call into a shared helper, or document why each manual site is correct against the auto-cleanup. **Scope: 1 day. Unlocks: confidence that future opcodes won't silently break the invariant.**

**4. Write a one-page "boundary-policy" doc** describing the 8 boundary kinds (positive_lookahead, negative_lookahead, positive_lookbehind, negative_lookbehind, atomic_group, subroutine_call, napla, naplb) and the propagation rules for each of `committed`, `skip_position`, `accept_forced`, captures, `match_start_override`. The doc is the model `execute_assertion_subexpr` etc. should converge on. **Scope: 1 day. Unlocks: §6.2 item 2 and Cluster 1C closure.**

### 6.2 Next — medium-term audits to land before more whack-a-mole accretes

**1. Per-verb effects refactor (full sweep).** With the effects table from §6.1.1 in hand, implement the `VerbState` struct and the `apply` table; rewrite each verb's `OpCode::*` arm to call `verb_table[op].apply(&mut ctx.verb_state, ctx.pos)` and remove the local precedence logic; collapse the failure-handling sites (8 scanning-loop sites + SIMD path + assertion-subexpr propagation) into one consumer that reads `verb_state` and decides. Write a focused test file with one spec test per verb (verb-vs-PCRE2 behaviour match) plus ~10 composition tests covering 3-verb, 4-verb, 5-verb sequences. Per-pair patches (#24, #36, the 2026-05-05 SKIP-overrides-COMMIT, the held `commit_saved_alt`) collapse into rows of the apply table. **Scope: 4-7 days. Unlocks: residual Cluster 1D (testinput1:5457 and family) by construction, scales to any verb count in the same branch, prevents future verb-pair commits — there are no future verb-pair commits because there are no pairs.**

**2. Boundary-policy refactor.** Convert the 8 boundary kinds into `BoundaryPolicy` const values; replace the per-kind ad-hoc propagation logic with policy lookups. The minimum viable version doesn't need to redesign anything: it just collects the existing dispatch into one place. The maximum version adds napla as a new policy and ships Cluster 1C. **Scope: 5-10 days. Unlocks: Cluster 1C (5 cases), prevents future propagation-asymmetry latent bugs.**

**3. Pike-VM step-limit threading.** Pick option A or B from §5.5 and ship it. Without this, testinput2:6244/6249 stay red, and the residual catalogue has to carry a paragraph about a known dispatch-chain trade-off. **Scope: 2-5 days. Unlocks: 2 cases plus removes a documented contract divergence.**

**4. PGEN walker silent-shape audit.** The 2026-05-05 commits show the walker silently dropping fields it doesn't understand. Audit every typed-shape arm in `rgx-core/src/parsing.rs::convert_typed_*` for the pattern `if let Some(s) = elem.as_str()` (which silently drops sub-array elements) and replace with `walk_json_terminal_chars` per element. This is preventive work — nothing is currently red, but the post-bump silent-shape gaps in May 2026 (`initial_close`, `class_quoted_literal`, `quoted_literal` body sub-array) all had this signature. **Scope: 2-3 days. Unlocks: resilience to PGEN typed-shape changes.**

### 6.3 Later — speculative larger redesigns

**1. Subroutine-stack reification (residual Cluster 1A + 1B + 2A + 2G).** The architectural capstone in the residual catalogue. Recursive captures across quantifier iterations need a "previous iteration's completed capture" read-only slot, returned-capture subroutines need explicit capture-merge semantics on success. Together these are 16+13+8+2 = 39 cases — the largest single architectural payoff in the residual. **Scope: weeks. Unlocks: ~39 cases.**

**2. Compile-time `(*NUL)`/`(*CRLF)` newline-mode threading (residual Cluster 2D's `(*NUL)` case).** Today the parser rewrites `.` under `(*CRLF)` etc. into a CharClass / Lookahead at parse time, before `/s` flag context is known. Defer the rewrite to compile time. **Scope: 2-3 days. Unlocks: ~3 cases, removes a structural awkwardness.**

**3. `\K` propagation from inside lookarounds (residual Cluster 2C).** Non-local engine change. The 2026-05-03 analysis correction pinned the diagnosis but explicitly deferred the fix. **Scope: 5-10 days. Unlocks: 3 cases, lookbehind variants need same care.**

**4. Reverse-DFA pipeline unanchored extension.** Currently `is_match` uses the forward-unanchored DFA but `find_first` / `find_all` don't, due to leftmost-LONGEST vs leftmost-first semantics. Extending requires a leftmost-first-aware unanchored NFA construction. Not a conformance issue but a perf-headroom item that would compound with conformance work in the engine. **Scope: medium. Unlocks: matches PCRE2's start-optimization on more patterns.**

---

## 7. Appendix — residual cluster status (cross-reference)

This section lists each residual cluster from `book/src/internals/pcre2-conformance-residual.md` against the audit's classification of how prone it is to whack-a-mole if attacked one case at a time.

| Cluster | Cases | Status | Whack-a-mole risk if approached per-case |
|---|---:|---|---|
| **1A — Recursive / self-referencing captures** | 16 | OPEN, architectural | **High**. Each case has a different surface (`^(a\1?){4}$`, `^((\1+)\|\d)+133X$`, palindrome family). Per-case patches will accrete the way verb-dispatch did. Tackle via subroutine-stack reification (§6.3.1). |
| **1B — Returned-capture subroutines** | 13 | OPEN, A12 follow-up | **Medium**. The surface is uniform (return-capture-list at call site), and the parser already handles it; the VM ignores the return list. One coherent fix. |
| **1C — Non-atomic positive lookahead `(*napla:...)`** | 5 (+1 FP +1 SM) | OPEN, compiler-level | **Medium**. The entire cluster shares one dispatch decision: napla compiles as ordinary `(?=...)` instead of as a re-enterable variant. Fits into the boundary-policy refactor (§6.2.2). |
| **1D — Complex backtracking-verb interactions** | 7 | OPEN, partial | **Very high**. Each open case (`COMMIT+THEN`, `(*:N)`+`(*SKIP:N)`+atomic, start-optimization-past-`a?`) is a different verb pair × context cell. The verb-precedence audit (§6.2.1) is the targeted intervention. |
| **1E — Conditional lookahead in repeated alt** | 3 | OPEN, dispatch | **Low-medium**. The 3 cases share one shape; one targeted dispatch investigation. |
| **1F — `(?J)` dupnames** | 1 (substitute case) | 3 closed via #38 | **Low**. The remaining substitute case is the same root cause as the closed ones; one parallel fix. |
| **1G — Misc FN edges** | varied | mixed (closed: 5, open: ~10) | **Low**. Each is its own investigation, but the cluster doesn't share a root. Tractable per-case. |
| **2A — Balanced-bracket greedy recursion** | 8 | OPEN, entangled with 1A | **High**. Same architectural prerequisite as 1A. |
| **2B — Empty-alternative lazy-quantifier** | 4 | OPEN, semantic | **Medium**. Single `OpCode::StarLazy`/`PlusLazy` codegen change. |
| **2C — `\K` inside `{0}`** | 3 | OPEN, deferred | **High** if treated as compile-time bypass (the original wrong prescription); **medium** as the actual `\K`-from-lookaround propagation. |
| **2D — Backtracking-verb span divergences** | 7 | OPEN | **Very high**. Same family as Cluster 1D. |
| **2E — `(?0)` self-pattern recursion** | 3 | OPEN | **Medium**. Bounded compiler-level fix. |
| **2F — `\Q…\E` inside char-class range** | 0 | CLOSED 2026-05-05 | n/a |
| **2G — Returned-capture subroutine balanced-paren** | 2 | OPEN, same as 1B | **Low** (closes with 1B). |
| **2H — Lookahead-as-alternative in greedy star** | 1 | OPEN, single-case | **Low**. |
| **2I — Conditional over empty capture** | 1 | OPEN, single-case | **Low**. |
| **3A — `(*SKIP)` inside failing lookbehind** | 1 | OPEN, attempted | **Medium**. Mirrors fix #37 but the lookbehind variant regressed 3 other cases on the first attempt. Needs per-iteration disambiguation. |
| **3B — `.+` under `/newline=...`** | 0 | CLOSED 2026-04-24 | n/a |
| **3C — `\K` inside `{0}` (FP)** | 0 | reclassified into 2C | n/a |
| **3D — napla + COMMIT + backref** | 1 | OPEN, same as 1C | **Low** (closes with 1C). |
| **Bucket 4 — substitute output divergence** | 4 (1 closed) | mixed | **Low-medium** per case. |
| **Bucket 5 — RGX too permissive** | 4 | OPEN | **Very low**. Each is a one-line compile-time rejection. |

The clusters with **very high or high** whack-a-mole risk if approached per-case (1A, 1D, 2A, 2C, 2D) account for ~40 of the 91 remaining failures. They are all in §5's systemic-gap list and §6's recommendations. The clusters with **low** risk (1F, 1G individual cases, 2H, 2I, 3D, Bucket 5) account for ~12 cases and can be picked off opportunistically without architectural work.

---

## 8. Cross-references

- **Companion residual chapter**: [PCRE2 Conformance Residual](./pcre2-conformance-residual.md). Per-case map of the 91 remaining failures.
- **Conformance harness source**: [`rgx-core/tests/pcre2_conformance.rs`](https://github.com/Raycast-Lab/rgx/blob/main/rgx-core/tests/pcre2_conformance.rs). `PASS_BASELINE` / `FAIL_BASELINE` constants are the ratchet gate.
- **VM dispatch core**: [`rgx-core/src/vm.rs`](https://github.com/Raycast-Lab/rgx/blob/main/rgx-core/src/vm.rs). Verb opcode arms at lines 2805-2897; assertion propagation at 6521-6561; `try_backtrack` at 2254-2300.
- **Engine-fix history**: `CHANGES.md` entries dated 2026-04-13 → 2026-05-05.
- **Session narrative**: `MEMORY.md` dated entries; sessions 2026-04-22 / 2026-04-23 / 2026-04-24 cover engine fixes #9 → #38.
- **PGEN-issue tracker**: `pgen-issues/PGEN-RGX-NNNN.yaml`. 81 of 82 closed; only `PGEN-RGX-0073` (compile-time perf) remains open and is non-conformance.
- **PCRE2 source of truth**: `subs/pcre2` submodule pinned to 10.47 (commit `f454e231`); `subs/pgen` submodule pinned to PGEN 1.1.75 (commit `08593d05`).

When this audit goes stale (a recommendation lands; a new fix accretes; the matrix gets written), update the inventory in §2, the systemic-gap status in §5, and the cluster cross-reference table in §7. The thematic and whack-a-mole-flagging sections (§3, §4) should age slowly because they capture the *patterns* of the work, not the specific commits.

---

## 9. Targeted-fix re-audit (per-fix principled-vs-hardcoded review)

The fix-classification labels in §2 were assigned at the time each row landed, in the heat of shipping. This section re-audits every row that §2 marked **targeted**, with the question: *given the spec text in `subs/pcre2/doc/pcre2pattern.3` and the actual code change*, is the fix shaped by an underlying PCRE2 rule (which would make it principled and the right implementation choice independent of any specific test case), or by which testinput cases happened to be red?

The re-audit uses four labels:

- **A — Principled in disguise.** §2's targeted classification was too pessimistic. The fix ratifies a documented spec rule and would have been the right implementation choice independent of which testinput case surfaced it. The §2 row should flip to *principled*.
- **B — Genuinely conformance-hardcoded.** The fix is shaped by the failing test pattern, not the spec. A general implementation looks materially different. Listed in §9.B with a concrete proposal.
- **C — Subsumed.** No longer load-bearing because a later commit replaced its mechanism with a general one.
- **D — Member of the verb-effects family** (§5.1). The fix is one of the per-pair / per-context verb-interaction patches that the per-verb effects refactor (§6.2.1) collapses into rows of an `apply` table. No individual analysis needed; the family-level remediation is already documented.

### 9.A Per-fix table

| # / Date | Commit | Subject | Re-class | Citation | Justification |
|---|---|---|---|---|---|
| #6 (2026-04-14) | `1e18cef` | Case-fold ASCII ranges spanning both cases (`[W-c]/i`) | **C** | Subsumed by `c051eb3` (engine #16) and `4879c73` (#14) | The `case_fold_ranges` per-codepoint iteration shipped here is the right algorithm for ASCII range case-closure, but #14 (UCD simple-fold) and #16 (full case-closure for class ranges) replaced this code path entirely. `case_fold_ranges` no longer exists in the form #6 introduced. Already labelled *targeted (later subsumed by #14/#16)* in §2.1; re-audit confirms C. |
| #13 (2026-04-22) | `d434229` | `CharClass::Custom::ci_override_ranges` for `\P{Lu/Ll/Lt}` in `[…]` | **B** | Carries per-item provenance through `CharClass::Custom` | The fix's own CHANGES.md text frames it as a band-aid: "7 of 14 harness-gated cases now real engine coverage. Remaining 7 positive `\p{Lu/Ll/Lt}/i` need case-fold table refactor; harness gate restored for those." The `ci_override_ranges: Option<Vec<CharRange>>` field threads a parallel ranges vector specifically because the `\P{Lu}/i` items need to expand to `complement(L&)` while the rest of the class folds normally. A general fix expands `\p{X}/i` to its case-closed property at parse time *uniformly*, threading provenance through every class item rather than only the few that need an override. See §9.B. |
| #20 (2026-04-22) | `bf6a1da` | Branch-reset subroutine calls resolve to leftmost group | **A** | pcre2pattern(3) lines 1985-1992: *"a subroutine call to a capture group always refers to the first one in the pattern with the given number"* | The fix flips `collect_capturing_group_defs` from `Alternation(group_defs)` to `group_defs[0]` for branch-reset (`(?\|...)`) groups. The spec's "first one in the pattern with the given number" is exactly the semantic the fix encodes. The original *targeted* label was assigned because only four corpus cases exercised it; the spec is unambiguous and the fix is the only correct interpretation. Flip §2.1 row to **principled**. |
| #21 (2026-04-22) | `6726e88` | Literal-prefix scan skips past leading verbs | **A** | pcre2pattern(3) lines 3870-3880 + 4040-4050: backtracking verbs are zero-width control directives | `extract_prefix_filter` (`vm.rs:1683`) walks past zero-width opcodes (anchors, word boundaries, etc.) to find the first consuming literal. The fix appends `Commit`/`Prune`/`Then`/`VerbSkip`/`Accept` to the same skip-list and handles `Mark`/`VerbSkipNamed`'s name-operand. The verbs *are* zero-width by spec, so skipping them is the same general rule. The follow-up gap (skipping past `a?`-style optional quantifiers) is a *different* general rule and is correctly tracked separately in residual Cluster 1D. Flip §2.1 row to **principled (partial — does not generalize to optional quantifier prefix; that's a separate spec rule)**. |
| #23 (2026-04-22) | `c756eb3` | Subexpr `(*PRUNE)`/`(*THEN)` with no enclosing alt propagate to outer | **A** | pcre2pattern(3) lines 4127-4140: assertion-verb propagation rules | When `(*THEN)` fires inside a subexpr with no enclosing alternation, PCRE2 degrades it to `(*PRUNE)` (per the spec's "the next innermost alternative" semantic — if no alternative exists, fall back to PRUNE's next-position advance). The fix clears `ctx.backtrack_stack` when `ctx.alt_boundaries` is empty, which is the correct encoding of "no alternative ⇒ behave as PRUNE". Spec-grounded; flip to **principled**. |
| #24 (2026-04-23) | `ca95b58` | `(*PRUNE)` clears any pending `(*SKIP)` mark | **D** | pcre2pattern(3) lines 4051-4072: *"if two or more backtracking verbs appear in succession, all but the last of them has no effect"* | The spec literally documents this as the general rule: a later verb supersedes an earlier one. The fix is the SKIP→PRUNE direction of that rule; #36 is COMMIT→PRUNE; the 2026-05-05 SKIP-overrides-COMMIT fix is COMMIT→SKIP. All three collapse into rows of the §5.1 / §6.2.1 verb-effects `apply` table. Family-level remediation already scoped. |
| #26 (2026-04-23) | `528ee0a` | Atomic-group codegen suppresses `(?U)` swap_greed | **A** | pcre2pattern(3) lines 2377-2378: *"Possessive quantifiers are always greedy; the setting of the PCRE2_UNGREEDY option is ignored"* | Possessive quantifiers lower to atomic groups in RGX's compiler, so the fix saves/restores `swap_greed` around the atomic-group inner codegen — directly ratifies the spec's "ignored" rule. Flip §2.1 row to **principled**. |
| #27 (2026-04-23) | `52783af` | Subexpr `(*COMMIT)` doesn't clear local stack | **A** | pcre2pattern(3) lines 4108-4112: *"The remaining verbs act only when a later failure causes a backtrack to reach them. … their effect is confined to the assertion, because Perl lookaround assertions are atomic."* | `(*COMMIT)` inside an assertion body is absorbed by the assertion's atomic boundary per spec; clearing the local stack inside the subexpr would break that absorption. The fix removes the clear and keeps `ctx.committed` for the post-assertion propagation path (engine fix #28's territory). Flip §2.1 row to **principled**. (The harness widening of `no_start_optimize` mentioned in the same commit *is* harness-only and is correctly classified there.) |
| #30 (2026-04-24) | `e4c90dd` | Subexpr/continuation `Call` also push retry frame | **A** | Same spec rule as #29; pcre2pattern(3) §"Recursive groups" empty-match-retry rule | The Call empty-match retry frame is a single general rule (Call may match empty; on empty must allow retry). Engine fix #29 implemented it at the top-level dispatch; #30 mirrors it at the two other dispatch sites because RGX has three Call dispatch paths. The §4.2 "dispatch-site fan-out" diagnosis is correct (the fan-out is a code-organization weakness), but each individual fix encodes the same spec rule. Flip §2.1 row to **principled (completes #29 across dispatch fan-out)**. The fan-out itself is a §6.1 cleanup target. |
| #33 (2026-04-24) | `f7c236c` | Subexpr `(*THEN)` uses local alt-boundary stack | **A** | pcre2pattern(3) line 4131: *"The effect of (*THEN) is not allowed to escape beyond an assertion"* | `(*THEN)` inside an assertion body must redirect to alternations inside that body, not outer ones. The fix threads `local_alt_boundaries: Vec<usize>` through `execute_subexpr_inner_full` so subexpr `AltSplit` pushes locally and `OpCode::Then` consults the local stack first. Direct ratification of the "not allowed to escape" rule. Flip §2.1 row to **principled**. |
| #35 (2026-04-24) | `3f60ee2` | `StarLazy`/`PlusLazy` propagate `(*ACCEPT)` from probed body | **A** | pcre2pattern(3) §"Backtracking verbs in subroutines" + engine fix #18's spec citation: ACCEPT forces success at this point and bubbles through enclosing constructs | The fix is the same general rule as #18 (ACCEPT bubbles through subexpr probes), applied to the lazy-quantifier probe sites. Lazy quantifiers `execute_subexpr_inner` for the body, so they need to inspect `probe_ctx.accept_forced` and propagate. Same spec rule, different opcode. Flip §2.1 row to **principled (mirror of #18 in lazy quantifier dispatch)**. |
| #36 (2026-04-24) | `6a56509` | `(*PRUNE)` clears pending `(*COMMIT)` abort | **D** | Same as #24 — pcre2pattern(3) lines 4067-4072 explicitly: *"...(*COMMIT)(*PRUNE)... If there is a matching failure to the right, backtracking onto (*PRUNE) causes it to be triggered, and its action is taken. There can never be a backtrack onto (*COMMIT)."* | Spec literally uses `(*COMMIT)(*PRUNE)` as the worked example of the "later verb wins" rule. Member of the §5.1 verb-effects family; collapses to one line of PRUNE's `apply` function (`state.committed = false`). |
| #37 (2026-04-24) | `6d124a2` | `(*SKIP)` inside failing lookahead propagates to outer | **A** | pcre2pattern(3) lines 4136-4141: *"In a conditional positive assertion, backtracking (from within the assertion) into (*COMMIT), (*SKIP), or (*PRUNE) causes the condition to be false. However, for both standalone and conditional negative assertions, backtracking into (*COMMIT), (*SKIP), or (*PRUNE) causes the assertion to be true, without considering any further alternative branches."* | The fix mirrors engine fix #28's COMMIT propagation for SKIP. The 2026-04-24 cleanup commit (already classified principled in §2.2) further tightened the propagation gate to positive-only. With that cleanup applied, #37's mechanism is the spec rule for verb propagation across positive-assertion boundaries. Flip §2.1 row to **principled (mirror of #28 for SKIP, gated to positive-only by 2026-04-24 cleanup)**. |
| 2026-04-17 (`372a66e`) | Scoped flag-disable `(?-x:...)` correctly disables x-mode | **A** | pcre2pattern(3) lines 1825-1828: *"unset these options by preceding the relevant letters with a hyphen, for example (?-im)"* | `strip_extended_inner` was using `flags.contains('x')` which fires for both `"x"` and `"-x"`; the fix parses at the `-` boundary the same way the VM codegen already does for i/m/s. Strict bug-fix to a parser that misread the documented flag-disable syntax. Flip §2.2 row to **principled**. |
| 2026-04-17 (`736306a`) | Unscoped `(?flags)` propagation across alternation | **A** | pcre2pattern(3) lines 1869-1878: *"Any changes made in one alternative do carry on into subsequent branches within the same group. For example, (a(?i)b\|c) matches 'ab', 'aB', 'c', and 'C' …"* | The spec uses literally the same shape as the failing test (`(a(?i)bc\|BB)x`) as its worked example. The fix detects trailing `FG(_, Empty)` toggles in `convert_alternation` and wraps subsequent branches in the carried `FlagGroup`. Direct ratification. Flip §2.2 row to **principled**. |
| 2026-04-18 (`5ea6ee1`) | Class-context escape semantics + runtime-policy verb no-ops | **A** (mixed) | pcre2pattern(3) lines 1483-1487: class-context `\b` is backspace; unrecognized class escapes are an error. pcre2syntax(3): runtime-policy verbs are advisory directives | Two of the three sub-changes are spec-direct: `\E`-as-no-op-outside-`\Q\E` (well-formed empty sequence per spec), class-context `\b`=`0x08` (spec line 1484). The third sub-change (accept runtime-policy verbs `(*NOTEMPTY)` etc. as no-ops) is principled-by-design — RGX explicitly does not model these runtime policies and accepting them as no-ops is the only sensible compile-time behaviour. The "unrecognized alphanumeric escapes fall back to literal" sub-rule is *softer* than the strict spec ("cause an error"), but RGX makes that softening explicitly under a documented permissive policy and the corpus-level effect is to keep the harness running rather than to mask a real spec violation. Flip §2.2 row to **principled**. |
| 2026-04-18 (`938916f`) | QuestionGreedy zero-width preserves captures | **A** | pcre2pattern(3) lines 2225-2230: empty-match iterations don't roll back capture state; the spec's empty-match termination rule does not say capture state is undone | `OpCode::QuestionGreedy` was undoing the capture trail when the body matched zero-width. The spec says zero-width iterations move on to the next pattern item; it does not say captures are rolled back. The fix only undoes the trail on `!matched`, mirroring the equivalent fix to `Star/PlusGreedy` from `871c8fd` (which §2.2 already classifies principled). Same spec rule, different opcode. Flip §2.2 row to **principled**. |
| 2026-04-18 (`00d0b11`) | Substitute: PCRE2 backslash escapes + case-change | **A** | pcre2api(3) lines 4055-4080: *"There are also four escape sequences for forcing the case of inserted letters … \eU and \eL change to upper or lower case forcing, respectively, and \eE … reverts to no case forcing. The sequences \eu and \el force the next character …"* | Replacement-string backslash escapes (`\\`, `\$`, `\n`, `\r`, `\t`, `\NNN`, `\xHH`, `\u`, `\l`, `\U`, `\L`, `\E`) are explicitly documented in pcre2api(3) under PCRE2_SUBSTITUTE_EXTENDED. The fix implements the documented set in `Regex::interpolate_replacement`. Flip §2.2 row to **principled**. |
| 2026-04-19 (`c6fa93e`) | Quantifier retargets across transparent atoms | **A** | pcre2pattern(3) §"Comments" + §"PCRE2_EXTENDED" (lines around 1570-1600 for the comment shape, around 1800 for `/x` whitespace): comments and `/x`-whitespace are transparent for quantifier attachment | PCRE2 documents both `(?#...)` comments and `/x` whitespace as transparent at the *parse* level — the quantifier should attach to the next real atom. PGEN attaches `{N}` literally to the immediately preceding token, so RGX needs a compiler pass that retargets across `Empty`/`WhitespaceLiteral`. `retarget_quantifiers_on_transparent` is exactly that: walks Sequence/Alternation/Quantified/Group/Look bodies, drops bare `Empty`s, transfers `Quantified(Empty\|Whitespace, q)` to the previous real atom. Same general rule as PCRE2's documented transparency. Flip §2.2 row to **principled**. |
| 2026-04-19 (`bdf910c`) | `\p{Lu}/i` etc. expand to `\p{L&}` | **A** | pcre2pattern(3) lines 980-985: *"From release 10.45 of PCRE2 the properties Lu, Ll, and Lt are all treated as Lc when case-independent matching is set by the PCRE2_CASELESS option or (?i) within the pattern."* (PCRE2 calls the merged property Lc; L& is the Perl alias) | The spec text is explicit: under `/i`, Lu/Ll/Lt all become Lc=L&. The fix's VM codegen remaps under `case_insensitive`. Direct ratification. Note that #13 was the *parse-time* counterpart of the same rule for `\P{Lu/Ll/Lt}/i` inside character classes — the bare-property variant here is principled, the in-class-with-other-items variant (#13) needs the broader refactor in §9.B. Flip §2.2 row to **principled**. |
| 2026-04-21 (`2278678`) | `OpCode::GraphemeCluster` dispatch in `execute_subexpr_inner` | **A** | dispatch-parity invariant; not a spec citation but a code-correctness rule | Quantified `\X` lowers to a sub-program dispatched through `execute_subexpr_inner`, which lacked the `GraphemeCluster` arm. The fix mirrors the main-loop handler. This is a *dispatch-parity* fix: every opcode reachable from the main loop should be reachable from every sub-program loop. The §4.2 "34 distinct call sites of `try_backtrack`" pattern is the larger systemic version. The individual fix is correct and general — flip §2.2 row to **principled (dispatch-parity)** — but it surfaces the same fan-out issue §6.1.3 targets. |
| 2026-04-21 (`108dcec`) | `[:print:]` UCP includes U+180E | **A** | PCRE2 source `subs/pcre2/src/pcre2_xclass.c:213-220`: *"Printable character: same as graphic, with the addition of Zs, i.e. not Zl and not Zp, and U+180E"* (explicit U+180E exception in the source's `PT_PXPRINT` arm) | The pcre2pattern(3) man page lists U+180E only under `[:graph:]`'s exclusion list, but the actual PCRE2 implementation explicitly re-adds U+180E to `[:print:]` (see pcre2_xclass.c line 220 `c != 0x180e` is *absent* from the print case, intentionally). RGX is conforming against PCRE2's behaviour, not against an under-specified man page. Flip §2.2 row to **principled (matches PCRE2 source PT_PXPRINT)**. |
| 2026-04-22 (`4f78658`) | `[:blank:]` UCP includes U+180E | **A** | pcre2pattern(3) lines 1712 + 754-758: *"[:blank:] becomes \\h"* and `\h`'s explicit list includes U+180E | `[:blank:]` lowers to `\h` per spec, and `\h` includes U+180E in its explicit horizontal-space list. The earlier omission from `[:blank:]` was an internal divergence between `\h` and `[:blank:]` — closing it is direct ratification. Flip §2.2 row to **principled**. |
| 2026-04-22 (`087d101`) | `[:word:]` UCP aligned with `\w` | **A** (with caveat) | pcre2pattern(3) line 1718: *"[:word:] becomes \ep{Xwd}"* — Xwd = L+N+Mn+Pc per pcre2pattern(3) lines 1118-1125 | The fix aligns `[:word:]` with RGX's existing `\w` UCP set (L+N+M+Pc). PCRE2 source uses Mn (`PT_WORD` in pcre2_xclass.c:158-163: `chartype == ucp_Mn || chartype == ucp_Pc`); RGX's `\w` uses full M (broader than spec). The alignment fix is *internally* principled — `[:word:]` should equal `\w` per spec — but RGX's `\w` itself is slightly broader than PCRE2's. That broadening is a separate question that should be tracked. Flip §2.2 row to **principled (alignment with internal `\w`)**, and add follow-up: tighten `ucp_word_ranges` from M to Mn. |
| 2026-04-23 (`b60e350`) | UCP `[:punct:]` excludes generic S*, keeps ASCII-punct-symbols | **A** | pcre2pattern(3) lines 1739-1742: *"[:punct:] matches all characters that have the Unicode P (punctuation) property, plus those characters with code points less than 256 that have the S (Symbol) property"* | The spec is exact: P* (all Unicode P*) plus ASCII-only S (sub-256). The fix narrows from `merge(&["P","S"])` to P* + the ASCII-symbol whitelist. Direct spec ratification. Flip §2.2 row to **principled**. |
| 2026-04-24 (`130a283`) | API substitute `$name` interpolation respects dupnames | **A** | pcre2pattern(3) lines 2108-2111: *"the groups to which the name refers are checked in the order in which they appear in the overall pattern. The first one that is set is used"* | Engine fix #38 implemented this exact rule for the conditional path; the substitute follow-up extends the same rule to template interpolation. Both are direct ratifications of the documented dupnames lookup. §2.2 already pairs this with #38; flip the row to **principled (companion of #38, same spec rule)**. |
| 2026-05-03 (`1ae4484`) | U+180E recognised as `\s`/`[:space:]` under `/ucp` | **A** | PCRE2 source `subs/pcre2/src/pcre2_internal.h:379` (HSPACE list) + `pcre2_xclass.c:143-156` (`PT_PXSPACE` falls through to `HSPACE_CASES`/`VSPACE_CASES` first) | PCRE2's source explicitly retains U+180E in the horizontal-space list (`HSPACE_BYTE_CASES`) and uses that list for both `\s` and `[:space:]` under `PT_PXSPACE`. The pcre2pattern(3) man page documents this as a compat retention from pre-Unicode-6.3. RGX was driving `\s` straight from the current `White_Space` property (which excludes it post-6.3). The fix unions U+180E into RGX's UCP space ranges. Direct conformance to PCRE2's documented compat behaviour. Flip §2.2 row to **principled (matches PCRE2 source HSPACE_CASES + documented compat)**. |
| 2026-05-05 (`acb4a50`) | PCRE2 quoted-run-as-range-start `[\Qabc\E-z]` | **A** | pcre2syntax(3) §"Quoted strings" + pcre2pattern(3) char-class range semantics: a `\Q…\E` quoted run inside `[…]` produces a sequence of literals, the *last* of which can be the start of a range | PCRE2 reads `[\Qabc\E-z]` as `[ab(c-z)]` because the last character of the quoted run is exposed to the range parser. RGX's adapter was processing each `class_item` independently, missing the range start. The fix peeks ahead in `convert_typed_char_class` for `[<quoted_run>, "-", <atom>]` and splits accordingly. The §2.2 row labelled this *parser/adapter targeted* because the surface looks like an adapter quirk, but the underlying rule (last char of a quoted run is range-start eligible) is the general PCRE2 behaviour. Flip §2.2 row to **principled (PCRE2 quoted-run-as-range-start rule)**. |
| 2026-05-05 (`4fb3980`) | `(*SKIP)` overrides `(*COMMIT)` in scanning loop | **D** | pcre2pattern(3) lines 4060-4072: *"if two or more backtracking verbs appear in succession, all but the last of them has no effect"* — explicitly | Same family as #24 / #36: a later verb's effect supersedes an earlier verb's pending effect. The 8 scanning-loop sites that the fix touched are exactly the §6.2.1 verb-effects "failure-handling reads final state" consumer that needs to collapse into one site. Member of the §5.1 family. |

### 9.B Genuinely conformance-hardcoded fixes — proposed general alternatives

Of 27 targeted fixes re-audited, **1** fell into category B (genuinely hardcoded). It is now **CLOSED** as of 2026-05-06 — see B1 below for the landed family-aware fix.

#### B1. Engine fix #13 — `CharClass::Custom::ci_override_ranges` for `\P{Lu/Ll/Lt}/i` (CLOSED 2026-05-06)

Commit: `d434229` (2026-04-22).

**Why hardcoded.** The fix introduces a *side-channel* on `CharClass::Custom`: a `ci_override_ranges: Option<Vec<CharRange>>` field that the parser populates only for the specific items `\p{Lu}`, `\p{Ll}`, `\p{Lt}`, `\P{Lu}`, `\P{Ll}`, `\P{Lt}` so that under `/i` codegen, those items expand to `complement(L&)` (or `L&`) instead of folding their literal range. Every other class item still uses the literal `ranges` field. The fix's own CHANGES.md text acknowledges the asymmetry: *"7 of 14 harness-gated cases now real engine coverage. Remaining 7 positive `\p{Lu/Ll/Lt}/i` need case-fold table refactor."* The data structure encodes the failing test cases (the few class items that need an L&/complement-L& expansion) rather than the underlying spec rule.

**The underlying spec rule.** pcre2pattern(3) lines 980-985: under `/i`, `\p{Lu}`, `\p{Ll}`, and `\p{Lt}` all collapse to the merged `L&`/`Lc` property. This is a *uniform* rule on Unicode property items, and it composes with character-class union/intersection/complement the same way every other class element composes.

**Landed fix (2026-05-06).** A new helper `unicode_support::case_fold_property_closure(name) -> Option<&'static str>` is the single source of truth for the case-distinguished property family. It returns:
- `"L&"` for the general-category letter triple {Lu, Ll, Lt} and the merged-property aliases {L&, Lc, Cased_Letter, Uppercase_Letter, Lowercase_Letter, Titlecase_Letter}
- `"Cased"` for the boolean case-distinction triple {Upper/Uppercase, Lower/Lowercase, Cased}
- `None` for case-invariant properties (Lo, Lm, Mn, Nd, scripts, blocks, etc.)

Three call sites consult the same helper:
1. **Standalone `Regex::UnicodeClass` codegen in `vm.rs`** — replaced the hardcoded `matches!(name, "Lu" | "Ll" | "Lt")` predicate with `case_fold_property_closure(name).unwrap_or(name)`.
2. **In-class untyped walker `parsing.rs::convert_char_class`** — `case_fold_property_class_item_ranges` (formerly `negated_letter_property_ci_ranges`, now polarity- and family-general) populates `ci_ranges` for any case-distinguished item.
3. **In-class typed walker `parsing.rs::convert_typed_char_class_object`** — new `case_fold_property_typed_class_item_ranges` recognises the typed-shape `{kind: "property", name, negated, type: "escape"}` and applies the same closure rule. Previously this walker always set `ci_override_ranges = None`, missing the case-distinguished family for the modern PGEN path.

The `CharClass::Custom::ci_override_ranges` field was retained as a side-channel; eliminating it requires storing classes as item-list-with-provenance (a larger refactor that is now optional rather than load-bearing). Its **contents** are now principled: any case-distinguished item populates the override correctly.

Correctness gains beyond the original engine #13:
- `\p{Lu}/i` standalone now matches Lt characters (e.g. `Dz` U+01F2). Previously `case_fold_ranges` expanded Lu's literal range to Lu ∪ Ll, missing Lt.
- `\p{Upper}/i` / `\p{Lower}/i` / `\p{Cased}/i` and their `\P` complements all resolve correctly.
- `(?i)[\P{Lu}]` on lowercase letters correctly returns no-match — the typed walker previously fell back to `case_fold_ranges(complement(Lu))` which incorrectly added 'a' via case-fold expansion.

Test coverage in `lib.rs::case_distinguished_property_expands_under_i` covers the full family across `\p` / `\P` × standalone / in-class. Conformance ratchet preserved.

### 9.C Summary count

| Category | Count | Fixes |
|---|---:|---|
| **A — Principled in disguise** | 23 | #20, #21, #23, #26, #27, #30, #33, #35, #37, **#13 (closed 2026-05-06, see §9.B B1)**; 2026-04-17 scoped flag-disable; 2026-04-17 unscoped flag propagation; 2026-04-18 class-context escapes (mixed); 2026-04-18 QuestionGreedy zero-width; 2026-04-18 substitute backslash escapes; 2026-04-19 quantifier-retargets; 2026-04-19 `\p{Lu}/i`→`L&`; 2026-04-21 GraphemeCluster dispatch; 2026-04-21 `[:print:]` U+180E; 2026-04-22 `[:blank:]` U+180E; 2026-04-22 `[:word:]` (with caveat); 2026-04-23 `[:punct:]`; 2026-04-24 API substitute dupnames; 2026-05-03 U+180E `\s`/`[:space:]`; 2026-05-05 quoted-run-as-range-start |
| **B — Genuinely conformance-hardcoded** | 0 | (formerly #13 `ci_override_ranges`, closed 2026-05-06) |
| **C — Subsumed** | 1 | #6 case-fold ASCII ranges (already labelled in §2.1) |
| **D — Verb-effects family** | 3 | #24 PRUNE-clears-SKIP, #36 PRUNE-clears-COMMIT, 2026-05-05 SKIP-overrides-COMMIT |
| **Total re-audited** | 27 | |

The dominant finding is **A** by a wide margin: 22 of 27 targeted fixes ratify a documented PCRE2 spec rule (or, in the U+180E and `[:print:]` cases, ratify PCRE2's *source-level* behaviour where the man page is under-specified). The original §2 *targeted* labels were assigned defensively at ship time — many of them were principled fixes that surfaced via a single failing test and got the conservative label. The audit's headline framing in §1 (*"the engine track is conformance-driven rather than spec-driven"*) is correct as a process observation but overstates the underlying-rule-fidelity of the individual fixes; in practice most fixes were correct readings of pcre2pattern(3) that went in under defensive labels.

The category **B** finding (#13 `ci_override_ranges`) was closed 2026-05-06 by the family-aware case-fold-closure refactor (see §9.B B1 above). The category **D** count of 3 is exactly the verb-effects family scoped in §5.1 and §6.2.1; Phase 1 of that refactor landed 2026-05-06 (centralized `verb_apply_*` dispatch), Phase 2 (deferred stack effects to close residual Cluster 1D testinput1:5457 / 5447) is pending.

As of 2026-05-06 (post-§9.B B1 family-aware fix and Phase 1 of §6.2.1 verb-effects refactor) this section's count is **A: 23, B: 0, C: 1, D: 3** — the only remaining audit-flagged carry-over is the verb-effects family Phase 2 (deferred stack effects). The audit's headline framing now matches the data: the substantive work was *spec-driven the first time*, the *labels* were conformance-driven.
