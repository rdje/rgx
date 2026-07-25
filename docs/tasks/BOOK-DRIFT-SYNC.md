# BOOK-DRIFT-SYNC: close the measured mdBook↔codebase drift

## Metadata

- Tree ID: `BOOK-DRIFT-SYNC`
- Status: `active`
- Roadmap lane: `Governance / process — two-track documentation (the Book is the
  director's only window into the project; drift is a first-class defect)`
- Created: `2026-07-25`
- Last updated: `2026-07-25`
- Owner: repo-local workflow

## Goal

Bring the RGX Book's **architecture-level factual claims** back into lockstep
with what `rgx-core` actually does, and record how the drift escaped so the
same class of staleness is caught next time rather than re-discovered by
reading.

The Book is the project's public face **and** the director's review surface
(session directive §7: "The mdBook is my only window into the project — I
review the book, not the code. So it must always reflect what the codebase
actually does."). A chapter that misstates the parser pin by 96 releases or
the dispatch architecture by two tiers is therefore a defect, not cosmetics.

## Non-Goals

- Not a Book rewrite. Only claims **verified false against the code** are in
  scope; prose, tone, and structure stay as-is.
- Not a re-measurement of any performance or conformance number. The perf
  tables in `internals/performance.md` and the ratchet figures in the
  conformance chapters were re-measured on 2026-07-21 and are current.
- Not a fix for chapter-local narrative that is correct *in its own context*
  (e.g. `jit-compiler.md` describing the pre-JIT chain as
  "DFA → Pike-VM → backtracking VM" when explaining what the JIT falls back
  to). Only the claims that describe the **public API's dispatch chain** are
  in scope.
- Not a change to any live continuity doc's numbers — `README.md`,
  `RUST_CODEBASE_ANALYSIS.md`, and `LIVE_ACHIEVEMENT_STATUS.md` were all
  verified current for these facts during the 2026-07-25 ramp.

## Acceptance Criteria

- Every drifted claim listed in "Measured drift" below is corrected against
  the code, with the source line that proves the correction cited in this
  tree's Verification Log.
- No new claim is introduced that is not verifiable from the code.
- `mdbook build book` succeeds (the Book still renders).
- `scripts/check-book-examples.sh` still passes (no example ratchet
  regression).
- The two-track doctrine is satisfied: `CHANGES.md` entry + this tree updated.
- A durable prevention mechanism is decided in `.2` — either a registered
  doctrine check or an explicit, recorded decision that this class stays
  human-reviewed.

## Measured drift (verified 2026-07-25 against the working tree)

All six items below were confirmed by reading the code, not inferred from
other docs. `subs/pgen` pin at the time of measurement: `d9d41c28`
(release 1.1.106 / integration contract 1.1.109).

| # | File:line | Book claims | Code says | Proof |
| --- | --- | --- | --- | --- |
| D1 | `book/src/internals/architecture.md:101` | PGEN submodule "currently pinned to **1.1.10**" | pin is **1.1.106** / contract **1.1.109** at `d9d41c28` | `git submodule status`; `book/src/internals/pgen-integration.md:13` already states it correctly — the two chapters contradict each other |
| D2 | `book/src/internals/architecture.md:77` | Engine dispatch = "DFA → Pike-VM → backtracking VM" | **5-tier: AC → DFA → Pike-VM → JIT → interpreter** | `rgx-core/src/lib.rs:1406` (the comment states "5-tier dispatch" verbatim) and the `if/else` chain at `lib.rs:1411–1421` |
| D3 | `book/src/internals/architecture.md:52` + its crate table | "a Cargo workspace with **four** crates" (`rgx-core`, `rgx-cli`, `rgx-bench`, `rgx-wasm`) | **five** members — `rgx-capi` is missing | `Cargo.toml` `[workspace] members`. `rgx-capi` has its own Book chapter (`host-integration/c-abi.md`), so the Book documents a crate its own architecture table omits |
| D4 | `book/src/internals/project-status.md:40` | "workspace with **four** crates" | five (same as D3) | `Cargo.toml` |
| D5 | `book/src/internals/nfa-dfa-engine.md:149` | "The public `Regex::is_match`, `find_first`, `find_all` go through a **3-tier dispatch chain**" | the public chain is 5-tier; 3-tier was the pre-C1 state | `rgx-core/src/lib.rs:1406` |
| D6 | `book/src/internals/jit-compiler.md:209` | same sentence, "**4-tier dispatch chain**" | 5-tier since AC literal-alternation dispatch shipped 2026-04-25 | `rgx-core/src/lib.rs:1406`; `rgx-core/src/ac.rs` |

Minor, same family, lower confidence of being worth changing (decide in `.1`):

- `README.md:63` says the Book is "45 chapters"; `book/src/SUMMARY.md` lists
  **49** numbered chapters + 2 prefaces (52 markdown files under `book/src`).

### Why this matters more than the individual facts

D2/D5/D6 are the same fact wrong in three chapters, and D3/D4 are the same
fact wrong in two. That is the signature of **narrative that was true once and
was never re-derived** — the AC tier (2026-04-25) and `rgx-capi`
(2026-05-13) both shipped with Book updates *in their own chapters* while the
architecture-level summaries that also describe them went unvisited. The
per-feature Book gate ("does this feature need a chapter?") does not catch a
feature that invalidates a *different* chapter's summary.

## Task Tree

- ID: `BOOK-DRIFT-SYNC`
  Status: `active`
  Goal: `The Book's architecture-level claims match the code, and the drift class has a recorded prevention decision.`
  Children: `BOOK-DRIFT-SYNC.1`, `BOOK-DRIFT-SYNC.2`

- ID: `BOOK-DRIFT-SYNC.1`
  Status: `pending`
  Goal: `Correct D1–D6 (and adjudicate the README chapter-count minor) against the code, citing the proving source line for each in the Verification Log.`
  Acceptance: `Each of D1–D6 corrected; no unverifiable claim introduced; mdbook build book succeeds; scripts/check-book-examples.sh passes; CHANGES.md entry written.`
  Verification: `pending`
  Commit: `pending`

- ID: `BOOK-DRIFT-SYNC.2`
  Status: `pending`
  Goal: `Decide and record how this drift class is prevented: either a registered doctrine check (candidate: BOOK-FACT-SYNC — assert the Book's stated PGEN pin equals git submodule status, and that its stated workspace-crate count equals Cargo.toml members) or an explicit decision that it stays human-reviewed, with the reason.`
  Acceptance: `Either a check_*.sh obeying the DOCTRINE_ENFORCEMENT.md §4 contract + a registry line + a §10 row (with a negative test proving it bites), or a docs/decisions/ record explaining why this class is not mechanizable. Not left implicit.`
  Verification: `pending`
  Commit: `pending`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `BOOK-DRIFT-SYNC.1` | `pending` | The facts are already measured and the proving source lines are already cited — this is a bounded, low-risk documentation slice with no code blast radius. |
| 2 | `BOOK-DRIFT-SYNC.2` | `pending` | Prevention is worth more than the one-time fix, but it should be designed after `.1` has shown exactly which claims are mechanically checkable (a pin string and a crate count are; "is the dispatch chain described correctly" is not). |

## Decisions

- `2026-07-25`: **The drift was recorded as a tree before any file was
  changed**, per the session directive "Nothing changes without a task-tree
  first" — which is stricter than `CLAUDE.md`'s pure-documentation exemption.
  The exemption would have allowed fixing the Book directly as a docs slice;
  the directive does not, so the tree comes first.
- `2026-07-25`: Scope limited to claims **proven false against the code**.
  Chapter-local narrative that is correct in its own frame (the JIT chapter
  describing what the JIT falls back to) is explicitly out of scope, so the
  fix cannot turn into an unbounded prose edit.

## Open Questions

- Should `nfa-dfa-engine.md` / `jit-compiler.md` state the full 5-tier chain,
  or state their own tier's position and link to one canonical description in
  `architecture.md`? The second is more drift-resistant (one place to update)
  but changes chapter structure. Resolve in `.1`; does not block picking it.
- Is the `README.md` "45 chapters" figure worth carrying at all, given it goes
  stale on every chapter added? Candidate: drop the count rather than correct
  it. Resolve in `.1`.

## Blockers

- None. `.1` is eligible immediately and touches no code.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-07-25` | — | Drift discovered during the mandated session-ramp read of the mdBook. Each claim cross-checked against source rather than against another doc: `Cargo.toml` `[workspace] members` (5), `rgx-core/src/lib.rs:1406` + `:1411–1421` (5-tier AC → DFA → Pike-VM → JIT → interpreter), `git submodule status` (`d9d41c28`). `book/src/internals/pgen-integration.md` verified **already correct** (1.1.106 / 1.1.109) — the drift is localized to the architecture-level summaries, not the Book as a whole | `tree opened; 6 drifted claims measured` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| — | `Docs: tree the measured mdBook↔codebase drift (BOOK-DRIFT-SYNC)` | Tree creation only — no Book file changed yet, so the drift is recorded before it is touched. |
| `.1` | `pending` | `pending` |
| `.2` | `pending` | `pending` |

## Changelog

- `2026-07-25`: Created during the session ramp. Six architecture-level Book
  claims measured false against the code (D1–D6). No Book file changed yet —
  `.1` owns the correction, `.2` owns the prevention decision.
