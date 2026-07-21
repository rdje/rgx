# PERF-SOTA-GAPS: SOTA algorithmic perf gaps vs PCRE2

## Metadata

- Tree ID: `PERF-SOTA-GAPS`
- Status: `active`
- Roadmap lane: `Next — SOTA algorithmic gaps not on the original C1/C2 roadmap`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Close the remaining benchmark gaps vs PCRE2 that constant-factor cleanup cannot
reach — the SOTA algorithmic techniques (RE2 / Hyperscan / PCRE2-JIT) not yet in
RGX. Pure performance; no feature or semantic change; conformance ratchet held
on every leaf.

## Non-Goals

- No new features or syntax. No accuracy change.
- Not re-doing constant-factor micro-optimization (the 2026-04-25→27 sprint
  already harvested those).

## Acceptance Criteria

- Each shipped leaf shows a measured win on a target pattern class (recorded in
  `target/benchmark-trends/` and the relevant Book chapter) with the PCRE2
  conformance ratchet unchanged and the full gate green.
- Deferred leaves carry an explicit, evidence-grounded deferral rationale.

## Task Tree

- ID: `PERF-SOTA-GAPS`
  Status: `active`
  Goal: `Close SOTA algorithmic perf gaps, accuracy-preserving.`
  Children: `.1` `.2` `.3` `.4` `.5` `.6`

- ID: `PERF-SOTA-GAPS.1`
  Status: `pending`
  Goal: `Inner-literal prefilter (RE2/Hyperscan style): extract required-interior-literal sets from the AST at compile time, pick the rarest, memchr/memmem to seed DFA candidate positions instead of walking the DFA over every byte. Targets email-shaped patterns (e.g. \b\w+@\w+\.\w+\b — the '@' is required).`
  Acceptance: `Measured 3–10× on email-style patterns; closes most of the email_basic gap; ratchet held; full gate green; Book performance chapter updated.`
  Verification: `pending`
  Commit: `pending`

- ID: `PERF-SOTA-GAPS.2`
  Status: `deferred`
  Goal: `SIMD-vectorized byte-class lookup in the DFA hot loop.`
  Acceptance: `n/a (deferred).`
  Verification: `Investigated 2026-05-13 (BACKLOG C2): DFA inner loop is inherently sequential; table lookup is ~couple ns of a 12–30 ns call; the 2–4× target is not achievable on the current bench corpus. Reopen only if a workload generates very large DFAs.`
  Commit: `n/a`

- ID: `PERF-SOTA-GAPS.3`
  Status: `deferred`
  Goal: `TDFA broadening to \b-in-capture patterns (e.g. \b(\w+)\b).`
  Acceptance: `n/a (deferred).`
  Verification: `Investigated 2026-05-13 (BACKLOG C2): architectural conflict — WB-gated tagged ε-edges change the register map per WB context; clean fix needs a closure-walker rework carrying per-WB-context register maps. Existing DFA→Pike pipeline handles these correctly today. Reopen on a real workload pull or a cleaner design.`
  Commit: `n/a`

- ID: `PERF-SOTA-GAPS.4`
  Status: `done`
  Goal: `DFA minimization (Hopcroft/Moore partition refinement).`
  Acceptance: `Shipped 2026-05-13 — c2/dfa.rs::LazyDfa::minimize, runs after try_materialize. Within-noise on current small DFAs; win materialises on larger patterns. No regressions; differential + conformance gates passed.`
  Verification: `Recorded in BACKLOG C2 / CHANGES.`
  Commit: `(historical — pre-task-tree; see CHANGES 2026-05-13)`

- ID: `PERF-SOTA-GAPS.5`
  Status: `done`
  Goal: `Materialized DFA for small patterns (lock-free flat array below ~64 states).`
  Acceptance: `Shipped (BACKLOG C2 — materialised-DFA Mutex-skip in the 2026-05-12 push).`
  Verification: `Recorded in CHANGES / RUST_CODEBASE_ANALYSIS perf-sprint notes.`
  Commit: `(historical — pre-task-tree; see CHANGES)`

- ID: `PERF-SOTA-GAPS.6`
  Status: `proposed`
  Goal: `Smaller/unclear wins: Glushkov position-automaton (vs Thompson), anchored fast-paths for ^pat$, suffix-anchored backward scanning for pat\z. Each needs measurement before promotion.`
  Acceptance: `Per-item measurement shows a real win before any is implemented; otherwise leave proposed.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `PERF-SOTA-GAPS.1` | `pending` | The largest remaining measurable win (email-shaped patterns); the only big lever not shipped or deferred. |
| 2 | `PERF-SOTA-GAPS.6` | `proposed` | Needs measurement first; promote per-item only if a win is shown. |

## Decisions

- `2026-06-15`: `.2` and `.3` are deferred with the 2026-05-13 investigation
  findings recorded (not silently dropped). `.4`/`.5` are recorded as already
  shipped (pre-task-tree; outcomes annotated by `TASKTREE-ADOPT.3`).

## Open Questions

- `.1` prefilter rarity estimation: byte-class entropy vs measured frequency.
  Resolve during `.1` design. Does not block picking `.1`.

## Design notes — `.1` inner-literal prefilter (session ramp-up, 2026-07-21)

Recorded so the analysis survives the session (the leaf was ramped-up but not
started; the session pivoted to `COMPILE-PERF-0078` on a user directive).

- **What exists already** (do not duplicate): `c2_required_inner_byte` /
  `c2_required_inner_literal` + finders (`rgx-core/src/c2/program.rs`) are a
  whole-input **fast-fail** only (`memchr`/`memmem` miss ⇒ no match anywhere).
  The leaf's target is **candidate seeding**: run the engine only near literal
  occurrences instead of per-position scanning.
- **Algorithm (find_first)**: pattern splits at the top-level `Sequence` as
  `P · lit · S`. Loop: (1) memchr/memmem next occurrence `p` of `lit`;
  (2) reverse-anchored DFA of **P only** walks back from `p` to the *leftmost*
  prefix start `s(p)` (reuse `Nfa::build_reverse_anchored` on the P sub-AST +
  `LazyDfa::find_match_start_at_reverse_bounded(end=p, min_start=floor)`, same
  full-AST `ByteClassMap` — a finer partition is always sound); (3) forward
  full-pattern anchored DFA verifies at `s(p)`; hit ⇒ recover via the existing
  `recover_match_for_dfa_span`, miss ⇒ next occurrence. Any `Exhausted` ⇒
  return `None` (fall through to today's paths).
- **Correctness condition (byte-disjointness)**: eligible only when **no byte
  of `lit` is in P's possible-byte set** (conservative AST walk; unknown node ⇒
  ineligible). Then: prefix spans cannot contain a lit occurrence ⇒ the
  leftmost match's minimal split `m₀` is the *first* occurrence ≥ its start
  `s*`; `s(m₀) = s*` (a strictly-earlier prefix start would combine with the
  suffix-match — suffix independence — to contradict leftmostness); all
  occurrences processed before `m₀` sit strictly left of `s*`, and a
  verification success there would contradict leftmostness, so the first
  verification success is exactly the leftmost match. **Without disjointness
  the seeding is UNSOUND** — counterexample worked out: `P=(?:bXa)?`,
  `lit=X`, `S=(?:aXc|c)` on input `bXaXc` returns [1,5) instead of the true
  leftmost [0,5). Do not ship a variant that drops the disjointness gate.
- **Linearity guard**: after processing occurrence `p`, raise the reverse-scan
  floor to `p + lit.len()` clamped to the next occurrence position (prefix
  spans cannot cross an occurrence under disjointness) ⇒ total reverse work is
  O(n) per `find_first`, no quadratic re-scan. Forward verification retains
  the same O(n·m) worst case as today's per-position scan — not a regression.
- **Dispatch gate (v1)**: fire only when `c2_prefix_byte` and
  `c2_prefix_literal` are both `None` (no leading-literal path) — class-based
  `PrefixFilter` (e.g. `Word` for `\b\w+@…`) is allowed and expected: the
  seed byte (`@`) is rarer than the prefix class. Targets `email_basic`
  (currently ~3.7× slower than PCRE2) exactly.
- **Verify before building**: reverse-DFA handling of `\b` at sub-pattern
  boundaries (P = `\b\w+` reverse-anchored mid-input) — the machinery exists
  (`is_accept_with_word_boundary_context` in `c2/dfa.rs`) but the
  `\b`-leading reverse path may be under-exercised (the pipeline gate
  `pipeline_dispatch_preferred` excludes prefix-hinted patterns). Check
  `should_dispatch_to_reverse_dfa` eligibility for WB patterns first.
- **Extractor**: new top-level-`Sequence` walk (distinct from
  `required_inner_substring`, which tracks runs anywhere): find maximal
  ASCII `Char` runs that are direct sequence items, keep P/S split indices,
  choose the rarest eligible run (prefer non-alphanumeric, longer), require
  P byte-disjointness; zero-width items adjacent to the run stay in P/S.

## Blockers

- None for `.1`.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | `.4`/`.5` | recorded as historically shipped (BACKLOG C2 / CHANGES) | `done (historical)` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.4`/`.5` | `(historical — see CHANGES 2026-05-12/13)` | pre-task-tree shipped work |

## Changelog

- `2026-06-15`: Created from ROADMAP "SOTA algorithmic gaps" (leaf
  `TASKTREE-ADOPT.2`). Frontier = `.1` inner-literal prefilter. `.2`/`.3`
  deferred with rationale; `.4`/`.5` recorded shipped; `.6` proposed pending
  measurement.
