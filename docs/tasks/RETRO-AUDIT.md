# RETRO-AUDIT: Retroactive audit of shipped code into task trees

## Metadata

- Tree ID: `RETRO-AUDIT`
- Status: `done`
- Roadmap lane: `Governance / process (retroactive)`
- Created: `2026-06-15`
- Last updated: `2026-06-15`
- Owner: repo-local workflow

## Goal

Honor the doctrine for **past** code changes: perform a thorough, accurate,
meticulous audit of the shipped RGX codebase (which predates the task-tree
system, adopted 2026-06-15) and annotate the verified outcomes back into task
trees, so the roadmap, the codebase, and the mdBook are confirmed locked
together and any drift is flagged. Created by `TASKTREE-ADOPT.3`.

## Non-Goals

- Not a line-by-line re-verification of the ~70K-LOC workspace. The audit
  altitude is **subsystem-level claim verification** (the module/opcode/method/
  baseline/Book-chapter exists as documented) **cross-referenced** with
  `CHANGES.md`, `RUST_CODEBASE_ANALYSIS.md`, the conformance ratchet, and the
  Book — not a fresh correctness re-proof of already-gated code.
- Not editing the historical `CHANGES.md` ledger (it stays authoritative).
- Not fixing flagged drift here — drift is recorded and dispatched to a
  follow-up tree.

## Acceptance Criteria

- Each major shipped subsystem is audited with concrete evidence (file/line or
  symbol) and a verified/`drift` verdict.
- Drift between roadmap/codebase/Book is recorded with a disposition.
- The audit method is recorded so future audits are repeatable.

## Method (leaf `.1`)

Evidence-based verification against the working tree at pin `db6f8c68`:
`grep`/`ls` for the claimed module, opcode, public method, or constant; confirm
the conformance baselines in source; confirm the Book chapter exists; compare
against the documented claim in `CHANGES.md` / `RUST_CODEBASE_ANALYSIS.md` /
`docs/BACKLOG.md`. A claim is `verified` when the artifact is present as
documented; `drift` when the codebase and the docs disagree.

## Task Tree

- ID: `RETRO-AUDIT`
  Status: `done`
  Goal: `Audit shipped code; annotate verified outcomes; flag drift.`
  Children: `.1` `.2` `.3` `.4` `.5` `.6` `.7` `.8` (all `done`)

- ID: `RETRO-AUDIT.1`
  Status: `done`
  Goal: `Record the audit method + scope/altitude (above).`
  Acceptance: `Method section present and repeatable.`
  Verification: `This file's "Method" section.`
  Commit: `leaf RETRO-AUDIT.1 (this slice)`

- ID: `RETRO-AUDIT.2`
  Status: `done`
  Goal: `Audit C1 (JIT/Cranelift) subsystem.`
  Acceptance: `Subsystem present as documented.`
  Verification: `VERIFIED — rgx-core/src/c1/{codegen,jit,runtime,mod}.rs present; Regex::uses_jit() at lib.rs:1222; OpCode dispatch tier confirmed. Matches CHANGES/BACKLOG C1 (shipped 2026-04-11, Step 8 2026-05-13) and Book internals/jit-compiler.md.`
  Commit: `leaf RETRO-AUDIT.2 (this slice)`

- ID: `RETRO-AUDIT.3`
  Status: `done`
  Goal: `Audit C2 (NFA/DFA hybrid), TDFA, AC, and the SIMD prefix scanner.`
  Acceptance: `Subsystems present as documented.`
  Verification: `VERIFIED — rgx-core/src/c2/{nfa,pike,program,byte_class,dfa,classifier,tdfa,simd_scan,mod}.rs present; Regex::uses_c2() at lib.rs:1146, uses_tdfa() at lib.rs:1178; rgx-core/src/ac.rs present. Matches BACKLOG C2 + CHANGES (C2 2026-04-11, TDFA 2026-05-13, AC 2026-04-25). Note: c2/simd_scan.rs exists (the prefix scanner) — this is distinct from the DFA-hot-loop SIMD byte-class lookup that PERF-SOTA-GAPS.2 records as deferred; the two are not the same lever (recorded for accuracy).`
  Commit: `leaf RETRO-AUDIT.3 (this slice)`

- ID: `RETRO-AUDIT.4`
  Status: `done`
  Goal: `Audit the 6 host-integration layers + tail_file + CLI.`
  Acceptance: `Layers present as documented.`
  Verification: `VERIFIED — tail_file at file.rs:201 (Layer 6); SteerResult at execution.rs:587 (Layer 3); MatchEvent at events.rs:9 (Layer 4); MatchContinuation at execution.rs:3083 (Layer 5). Matches ROADMAP "Host integration Layers 3/4/5/6 = done" + DEVELOPMENT_NOTES + Book host-integration/*.`
  Commit: `leaf RETRO-AUDIT.4 (this slice)`

- ID: `RETRO-AUDIT.5`
  Status: `done`
  Goal: `Audit the A-series (A1–A14) and B-series (B1–B21) backlog — public API + safety limits.`
  Acceptance: `Representative items present as documented.`
  Verification: `VERIFIED (spot-checked) — 4 safety-limit setters (set_max_steps/backtrack_frames/recursion_depth/trail_entries) in lib.rs (A1/A2/B1); RegexSet (regex_set.rs), RegexCache (cache.rs), BytesRegex (bytes.rs) present (B2/B3/B5); A10 GraphemeCluster=0x08 + A11 VerbSkipNamed opcodes in vm.rs; A12 below (.7 of PCRE2-1047-SYNTAX owns the follow-up). lib.rs carries 449 #[test] blocks (consistent with the ~1,197 lib-test claim). BACKLOG section B is marked all-shipped with per-item lib.rs line refs; audit confirms the type/method anchors exist.`
  Commit: `leaf RETRO-AUDIT.5 (this slice)`

- ID: `RETRO-AUDIT.6`
  Status: `done`
  Goal: `Audit the PCRE2 conformance program + the PGEN-RGX ledger.`
  Acceptance: `Ratchet baselines confirmed; ledger reconciled.`
  Verification: `VERIFIED with DRIFT — pcre2_conformance.rs PASS_BASELINE=12_806 / FAIL_BASELINE=4 / PANIC=0 / SKIP=0 (matches the 12,806/4/0/0 narrative). A12 returned-capture parse+lowering present (CallReturning=0x46 vm.rs:219; ReturnedCaptureSubroutine ast.rs:136; convert_typed_subroutine_call_object parsing.rs:1906) — full capture-return VM semantics is the open PCRE2-1047-SYNTAX.1 follow-up (consistent). **DRIFT D1 found** — see "Drift Findings".`
  Commit: `leaf RETRO-AUDIT.6 (this slice)`

- ID: `RETRO-AUDIT.7`
  Status: `done`
  Goal: `Audit rgx-capi (A9 Phase 0/1 C ABI).`
  Acceptance: `Phase 1 surface present as documented.`
  Verification: `VERIFIED — rgx-capi/src/lib.rs exports rgx_compile/rgx_regex_free/rgx_regex_retain/rgx_is_match/rgx_find_first (pub unsafe extern "C") + rgx_last_error/rgx_runtime_version_{major,minor,patch} (pub extern "C"); include/rgx.h + STABILITY.md + cbindgen.toml + tests present. Matches BACKLOG A9 (Phase 0/1 shipped 2026-05-13). (Initial grep missed these by omitting "unsafe" — re-verified; NOT drift.)`
  Commit: `leaf RETRO-AUDIT.7 (this slice)`

- ID: `RETRO-AUDIT.8`
  Status: `done`
  Goal: `Record drift findings + dispatch to follow-up.`
  Acceptance: `Drift recorded; follow-up tree proposed.`
  Verification: `See "Drift Findings"; LEDGER-HYGIENE proposed in docs/TASK_TREE.md.`
  Commit: `leaf RETRO-AUDIT.8 (this slice)`

## Drift Findings

- **D1 — PGEN-RGX ledger `status:` field drift (KNOWN; RUST_CODEBASE_ANALYSIS
  #14/#292; sharpened by the maintainer 2026-06-15).** 86
  `pgen-issues/PGEN-RGX-*.yaml` files on disk; **16 files match `status: open`**
  — `0021, 0022, 0023, 0027, 0028, 0033, 0034, 0035, 0036, 0037, 0038, 0039,
  0053, 0073, 0078` plus `TEMPLATE.yaml` (not a report).
  **Maintainer-authoritative current state (PGEN authority, 2026-06-15):
  `PGEN-RGX-0078` is the ONLY active/open bug — PGEN has not yet had time to
  address it. EVERY other report has been addressed.** That includes `0073`
  (the original compile-time filing that `0078` re-files in PCRE2-relative
  terms — `0073` is now addressed/closed; earlier live-doc framing calling
  `0073` the active open tracker and `0078` superseded is corrected here) and
  all 13 stale-open files above. So **15 of the 16 `status: open` YAMLs are
  stale and should read `status: closed`; only `0078` stays open.** Secondary:
  docs say "88 reports filed (0001–0088)" but 86 report files exist on disk
  (gaps in the 0001–0088 range).
  **Disposition:** spawn `LEDGER-HYGIENE` (proposed) — batch-flip the 15 stale
  files (`0021/0022/0023/0027/0028/0033/0034/0035/0036/0037/0038/0039/0053`
  + `0073`) to `status: closed` with a resolution note, leave `0078` open,
  reconcile the report count, and align the live-doc/Book narrative to
  "0078 is the sole open tracker; all others addressed." This is a
  `pgen-issues/`-and-docs change (non-code), so it can be a single doc slice.
  **No code or behavior is affected** — the ratchet (12,806/4) is the binding
  correctness gate and is green.

- **No code/Book drift found** in C1, C2/TDFA/AC, host-integration layers, the
  A/B public-API surface, A9 Phase 1, or the conformance baselines: every
  audited subsystem artifact is present exactly as the docs/Book describe.

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| — | — | — | All `.1`–`.8` complete in this slice. `RETRO-AUDIT` closes; the one follow-up (`LEDGER-HYGIENE`) is captured as a Proposed tree. |

## Decisions

- `2026-06-15`: Audit altitude = subsystem-level verification + evidence
  cross-reference (not 70K-LOC re-proof). Rationale: the codebase is already
  gated by 1,197+ lib tests + the 12,806/4 conformance ratchet; re-proving
  gated code is waste — the audit's value is confirming docs↔codebase↔Book
  agreement and surfacing drift.
- `2026-06-15`: Drift D1 is recorded + dispatched to `LEDGER-HYGIENE` rather
  than fixed inline, to keep this leaf scoped to "audit", not "remediate".

## Open Questions

- None blocking. `LEDGER-HYGIENE` will resolve the exact closed-vs-open set and
  the report-count reconciliation.

## Blockers

- None.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-06-15` | `.1`–`.8` | codebase grep/ls evidence sweep (c1/, c2/, ac.rs, recursion.rs, file.rs, events.rs, execution.rs, vm.rs opcodes, lib.rs API + uses_*, rgx-capi exports, pcre2_conformance.rs baselines, book/src/internals/*); cross-referenced CHANGES/RUST_CODEBASE/BACKLOG/Book | `pass — all subsystems verified; D1 drift flagged` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.1`–`.8` | `Docs: retroactive audit of shipped code into trees (leaf TASKTREE-ADOPT.3)` | Audit slice; docs-only; not pushed unless user asks. |

## Changelog

- `2026-06-15`: Created by `TASKTREE-ADOPT.3`. All 8 audit leaves completed in
  one evidence-based slice; D1 (PGEN-RGX ledger status drift) flagged and
  dispatched to the `LEDGER-HYGIENE` proposed tree.
