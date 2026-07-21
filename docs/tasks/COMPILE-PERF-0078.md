# COMPILE-PERF-0078: Close the PCRE2 compile-time gap to <5×

## Metadata

- Tree ID: `COMPILE-PERF-0078`
- Status: `active` (UNBLOCKED 2026-07-21 — the fast pin is adopted; all three
  blockers fixed upstream. Remaining work is RGX-side: `.3` / `.4` / `.2`)
- Roadmap lane: `Next — Performance: close the PCRE2 compile-time gap to <5×`
- Created: `2026-06-15`
- Last updated: `2026-07-21`
- Owner: repo-local workflow

## Goal

Reduce the `Regex::compile` wall-clock gap to **<5×** of PCRE2 compile,
accuracy-preserving — no semantic changes, no PGEN bypass, no behavioural
drift, 100% test suite green, PCRE2 conformance ratchet unchanged.

## Non-Goals

- Replacing PGEN with a hand-written parser (violates CLAUDE.md "PGEN is the
  sole parser"). Ruled out by hard project rule.
- Any AST simplification that could change match results.

## Acceptance Criteria

- `Regex::compile` geomean within `<5×` of PCRE2-10.47-no-JIT compile on the
  8-pattern bench corpus, measured per `book/src/internals/measurement-methodology.md`.
- Full `./scripts/run-local-ci.sh` green; PCRE2 conformance ratchet unchanged.

## Task Tree

- ID: `COMPILE-PERF-0078`
  Status: `active`
  Goal: `Regex::compile <5× of PCRE2 compile, accuracy-preserving.`
  Children: `COMPILE-PERF-0078.1`, `COMPILE-PERF-0078.2`, `COMPILE-PERF-0078.3`, `COMPILE-PERF-0078.4`

- ID: `COMPILE-PERF-0078.1`
  Status: `done`
  Goal: `Absorb PGEN regex-parser speed releases and re-measure the compile ratio on each subs/pgen bump.`
  Acceptance: `On a faster PGEN pin, re-measured geomean recorded in Book → Performance → "Compile-time performance"; ratchet held; pin bump owned by its own leaf (subs/pgen is a code change).`
  Verification: `2026-07-21 ADOPTED d9d41c28 (rel 1.1.106 / contract 1.1.109). Parser regenerated cold; REGEX-0098 absorbed; lib 1203/0; ratchet held 12,806/4/0/0; parse geomean 2,813 ns = 26.3x faster (8.64x vs PCRE2-no-JIT, 1.20x vs +JIT). See Verification Log.`
  Commit: `COMPILE-PERF-0078.1 — adopt PGEN 1.1.106; parse 26.3x faster`

- ID: `COMPILE-PERF-0078.2`
  Status: `deferred`
  Goal: `RGX-side trivial-pattern short-circuit AFTER PGEN parses (roadmap technique #5): cheap classifier (is_pure_literal / is_single_char_class) that skips NFA/DFA/JIT downstream work for trivial shapes. PGEN still parses (sole-parser rule preserved).`
  Acceptance: `Conservative classifier (semantic flags like (?i) fall through to full pipeline); measurable compile win on trivial CLI patterns; ratchet unchanged; full gate green.`
  Verification: `pending — deferral rationale UPDATED 2026-07-21: the old reason (PGEN-parse dominance) inverts on the fast pin (raw parse ~4–18% of compile). Re-rank .2/.3/.4 by phase-split measurement once the fast pin is adopted.`
  Commit: `pending`

- ID: `COMPILE-PERF-0078.3`
  Status: `pending`
  Goal: `Adapter-boundary fast path: eliminate the AST-dump JSON round-trip (PGEN serialize → serde deserialize → collapse walker, measured ~10–25µs/pattern on the fast pin — larger than the raw parse itself). Contract-sanctioned options: consume ParseContent::Shaped natively or parse_full_regex()?.content.to_json_value() at the boundary (see the contract's -0105/-0212 Rust-embedding notes).`
  Acceptance: `Measured reduction of the pgen-phase share in compile_phase_split; zero AST semantic drift (parser-contract fixtures + conformance ratchet unchanged); full gate green.`
  Verification: `pending — UNBLOCKED 2026-07-21 (fast pin adopted). Measured on the adopted pin, the adapter boundary is the largest single compile phase.`
  Commit: `pending`

- ID: `COMPILE-PERF-0078.4`
  Status: `pending`
  Goal: `Lazy/skippable C2 program construction inside compile (the "ast" phase, ~12–36µs/pattern on the fast pin): build CompiledC2Program on first dispatch-eligible use, mirroring the 2026-04-25 lazy Engine::new artifacts (technique #2's explicit early-exit).`
  Acceptance: `Compile-time reduction on C2-ineligible and one-shot patterns without regressing first-match latency beyond the agreed threshold (5%); ratchet unchanged; full gate green.`
  Verification: `pending — UNBLOCKED 2026-07-21 (fast pin adopted); per-phase numbers now available in the Verification Log.`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `COMPILE-PERF-0078.3` | `pending` | Now the single largest compile phase. On the adopted pin the AST-dump adapter boundary costs ~25–45µs/pattern against a ~2.8µs raw parse — RGX pays roughly 10× the parser's own cost to import its output. Contract-sanctioned fix (consume `Shaped` natively / `to_json_value()` at the boundary). |
| 2 | `COMPILE-PERF-0078.4` | `pending` | The other dominant phase (eager C2 program build). Mirrors the proven 2026-04-25 lazy-`Engine::new` technique, so the risk profile is known. |
| 3 | `COMPILE-PERF-0078.2` | `deferred` | Trivial-pattern short-circuit. Still last: medium correctness risk (mis-classifying a semantic flag would mis-match), and `.3`/`.4` should be measured first — they may make it unnecessary. |
| — | `COMPILE-PERF-0078.1` | `done` | Adopted 2026-07-21. |

## Decisions

- `2026-06-15`: **`PGEN-RGX-0078` replaces `PGEN-RGX-0073`** per the maintainer
  (PGEN authority). `0078` is the **sole active, still-open PGEN bug** for this
  lane — PGEN has not yet had time to address it. **Every other PGEN-RGX report
  has been addressed**, including `0073` (now superseded by `0078`). This tree
  was **renamed `COMPILE-PERF-0073` → `COMPILE-PERF-0078`** to track the live
  bug number (the prior ID was published 2026-06-15 in commit `7a3ec39`; this
  rename is recorded for traceability). Per user directive (2026-05-19), **no
  new PGEN bug report** is filed — metrics live in the Book. Parser-speed work
  is PGEN-side (sole-parser design); RGX files no workaround
  (`feedback_no_pgen_workarounds`).

## Open Questions

- None blocking. The unblock is external (PGEN release cadence).

## Blockers

- **Blocker (superseded 2026-07-21):** ~~PGEN's regex-grammar parse is ≈63–86%
  of `Regex::compile` wall-clock (geomean ≈214× vs PCRE2-no-JIT on
  `db6f8c68`), tracked by the still-open `PGEN-RGX-0078`.~~ **CLEARED** —
  PGEN's 0078 speed campaign closed upstream (head `960dddaa`, rel 1.1.104 /
  contract 1.1.106); RGX-side verification measurements in the Verification
  Log.
- **Blocker (CLEARED 2026-07-21):** ~~the fast pin `960dddaa` cannot be adopted
  — it carries a **rejects-valid parser regression** (`(?[\b])` /
  `(?[[\b]])` backspace inside Perl extended classes; PCRE2-oracle-verified;
  filed **`PGEN-RGX-0089`**) that turns 2 shipped RGX default-path tests red,
  plus a **broken cold-clone bootstrap** (`regex_parser_bootstrap` seed step
  refused + exit-0-on-refusal; filed **`PGEN-RGX-0090`**). A further 4 red
  tests are the documented, PCRE2-convergent REGEX-0098 named-reference
  validation boundary move — legitimate RGX absorption work owned by leaf
  `.1` when adoption unblocks. **Why it blocks:** the gate cannot be
  committed red, and RGX ships no parser workarounds
  (`feedback_no_pgen_workarounds`). **Unblock condition:** a PGEN release
  fixing 0089 (0090 strongly desired for downstream builds). **Run instead:**
  `PERF-SOTA-GAPS.1`.~~ **CLEARED** — PGEN fixed all three (release 1.1.106 /
  contract 1.1.109); RGX adopted `d9d41c28` on 2026-07-21 and verified each fix
  (see the Verification Log and the closed `pgen-issues/PGEN-RGX-0089/0090/0091`).
  **The tree has no blockers: every remaining leaf is RGX-side work.**

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | — | tree created; no leaf executed | `n/a` |
| `2026-07-21` | `.1` | **RGX-side verification of PGEN's 0078 closure** on preview pin `960dddaa` (rel 1.1.105 per contract doc / constants report 1.1.104 — PGEN-RGX-0091), standard methodology (default allocator, release, p50/5000, same-session PCRE2 baselines): raw parse geomean **2,748 ns** (was ~74µs → **26.9× faster**); **8.44× vs PCRE2-no-JIT** (was ≈214×), **1.17× vs PCRE2+JIT** (was ≈32×; 5/8 patterns faster than PCRE2+JIT). Phase split: raw parse now ~4–18% of `Regex::compile` (full compile geomean ≈42µs ⇒ ≈130× no-JIT) — the bottleneck is now RGX-side (adapter boundary ~10–25µs + eager C2 build ~12–36µs). Bundle: `pgen-issues/artifacts/PGEN-RGX-0078/measurements/*_1.1.104_preview*`. Adoption gate: rgx-core lib on the preview pin = 1196 pass / 6 fail (2 = PGEN-RGX-0089 regression; 4 = documented REGEX-0098 boundary absorption). Pin NOT adopted; restored `db6f8c68` and re-verified lib green (1202/0). | `measured; adoption blocked` |

| `2026-07-21` | `.1` | **ADOPTION (user-directed).** Pin `db6f8c68` (rel 1.1.81 / contract 1.1.83) → **`d9d41c28` (rel 1.1.106 / contract 1.1.109)** — 24 releases. Read the PGEN regex book, the embedding/handoff doc and the integration contract first (user directive). **Blocker verification:** `0089` — `(?[\b])` AND `(?[[\b]])` both match U+0008 again (CLI `1..2`), both extended-class fixtures green; `0090` — deleted `subs/pgen/generated/` outright (cold-clone repro) and the canonical `regex_parser_bootstrap` target completed with NO workaround; `0091` — `parser_embedding_api_contract()` reports `1.1.106`/`1.1.109`, matching the contract identity. All three reports closed. | `PASS` |
| `2026-07-21` | `.1` | **REGEX-0098 absorption** (unknown named references now reject at parse time, PCRE2 err 115): 4 red surfaces fixed — the `(?&word)` reference fixture became well-formed `(?<word>a)(?&word)`, a new `parser_contract_must_reject_fixtures()` + `parser_contract_unknown_named_reference_fixtures_reject` locks the 9-form reject family (`(?&word)` `(?P>word)` `\k<word>` `\k'word'` `\k{word}` `\g{word}` `\g<word>` `\g'word'` `(?P=word)`), and the 2 message-text assertions now match the contract-sanctioned diagnostic CODE (`E_PARSE_FAILURE`) instead of prose. **rgx-core lib 1203 pass / 0 fail** | `PASS` |
| `2026-07-21` | `.1` | **Accuracy gate: PCRE2 conformance ratchet HELD at 12,806 / 4 / 0 / 0** across a 24-release parser bump (55.15s). No regression and no improvement — consistent with PGEN's changelog, which marks most of those releases corpus-invisible validator→grammar migrations. | `PASS` |
| `2026-07-21` | `.1` | **Speed (the point of the bump), RGX standard instrument on the adopted pin:** raw parse geomean **2,813 ns** vs ~74,000 ns on `db6f8c68` = **26.3× faster**; **8.64× vs PCRE2-10.47-no-JIT compile**, **1.20× vs PCRE2+JIT**; 3/8 patterns parse faster than PCRE2 compiles WITH JIT. Reproduces the 1.1.104 preview (2,748 ns) within +2.4%, confirming the preview measurement was sound. Bundle: `measurements/ratio_table_1.1.106_adopted.md` + `pgen_parse_p50_1.1.106_adopted.txt`. | `PASS` |
| `2026-07-21` | `.1` | **Phase split on the adopted pin — the inversion is now shipped fact, not projection:** raw parse ≈4% of `Regex::compile`; the adapter boundary (`pgen` phase minus raw parse) runs ~25–45µs/pattern and the eager C2 build ~25–45µs, against a ~55–78µs full compile. NOTE: captured on a machine that had been building continuously, so treat the absolute full-compile figures as indicative; the raw-parse p50s reproduce the preview and ARE trustworthy. `.3`/`.4` own the two dominant phases. | `measured` |
| `2026-07-21` | `.1` | **AST shape stability:** `regex_ast_dump_schema_version` stays `1`; the 8 persisted AST dumps are byte-identical except `api_version` `1.2.0`→`1.3.0`. RGX consumes only the serialized `parse_regex_default_ast_dump` path, so PGEN's Rust-level `ParseNode` slimming (72→48B) and `ParseContent::Json`→`Shaped(PgenValue)` carrier change do not reach RGX. No walker change. | `PASS` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.1` | `COMPILE-PERF-0078.1 — adopt PGEN 1.1.106; parse 26.3x faster` | Pin bump + REGEX-0098 absorption + 3 reports closed. |

## Changelog

- `2026-06-15`: Created from ROADMAP "close the PCRE2 compile-time gap to <5×"
  (leaf `TASKTREE-ADOPT.2`), originally as `COMPILE-PERF-0073`. Status
  `blocked` (PGEN-side). Records technique #1 (lazy `Engine::new`) as already
  shipped 2026-04-25.
- `2026-06-15`: Renamed `COMPILE-PERF-0073` → `COMPILE-PERF-0078` per maintainer
  ("0078 replace 0073"); `PGEN-RGX-0078` is the sole open PGEN bug, all others
  addressed. Done as part of leaf `TASKTREE-ADOPT.3` (ledger reconciliation).
- `2026-07-21`: `.1` executed against PGEN's 0078-closure release (user
  directive): speed verified (26.9× parse; 8.44×/1.17× ratios), adoption
  blocked by PGEN-RGX-0089/0090/0091 → filed; submodule restored to
  `db6f8c68`. `PGEN-RGX-0078` marked closed (upstream closure adjudicated
  against PGEN's re-based absolute sub-1µs bar; RGX numbers recorded).
  Added proposed RGX-side leaves `.3` (adapter-boundary fast path) and `.4`
  (lazy C2 build) from the phase-inversion finding; `.2`'s deferral rationale
  refreshed. **OPEN DIRECTOR QUESTION: keep the `<5×`-vs-PCRE2-no-JIT
  acceptance bar (still unmet at 8.44×, now mostly RGX-side), or re-base it
  (PGEN retired its own relative bar)?** The tree keeps the `<5×` bar until
  the director rules otherwise.
- `2026-07-21`: **`.1` DONE — the fast pin is ADOPTED** (`d9d41c28`, rel 1.1.106
  / contract 1.1.109), user-directed after PGEN fixed `0089`/`0090`/`0091`.
  Parse is 26.3× faster; ratchet held; REGEX-0098 absorbed. The tree is
  UNBLOCKED and its remaining work (`.3` adapter boundary, `.4` lazy C2 build,
  `.2` trivial short-circuit) is entirely RGX-side — exactly as ADR 0005
  anticipated when the director kept the `<5×` bar.
- `2026-07-21` (later same day): **DIRECTOR RULING — the `<5×` bar is KEPT;
  "the ball is now in RGX's yard."** Recorded as decision record
  `docs/decisions/0005-compile-perf-5x-bar-kept-rgx-side.md`. Acceptance
  criteria unchanged; the path of record to `<5×` is the RGX-side leaves
  `.3`/`.4`/`.2` once the fast pin is adopted (post-`PGEN-RGX-0089` fix).
