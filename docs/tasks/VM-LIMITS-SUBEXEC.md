# VM-LIMITS-SUBEXEC: safety limits must bound sub-execution paths

## Metadata

- Tree ID: `VM-LIMITS-SUBEXEC`
- Status: `done`
- Roadmap lane: `Now — production safety (A1/A2/B1 family)`
- Created: `2026-07-21`
- Last updated: `2026-07-21`
- Owner: repo-local workflow

## Goal

`set_max_steps` / `set_max_backtrack_frames` / `set_max_recursion_depth` must
bound EVERY VM execution path, including nested sub-executions. Discovered
2026-07-21 during the PGEN 1.1.104 adoption attempt: the PCRE2 conformance
harness (which caps every case at 1M steps / 64K backtrack frames precisely to
keep pathological cases at ~10ms) sat 20+ minutes of pegged CPU inside
`execute_subexpr_inner_full` (vm.rs:7560/7999-8005, backtrack_stack.push loop)
evaluating the variable-length lookbehind of testinput2:6509
`/(?<=(\d{1,256}))X/` on the 8-byte subject `12345XYZ` — the limits did not
fire. The same pattern hangs the CLI (no limits) on the CURRENT pin db6f8c68
too (≥20s, verified via `timeout 20 target/debug/rgx '(?<=(\d{1,256}))X'
'12345XYZ'` → exit 124 on the 2026-05-19 binary), so the underlying blowup is
pin-independent; the limits-coverage gap is RGX's own DoS-surface bug (the
production-safety contract A1/A2/B1 promises these limits prevent exactly
this).

## Non-Goals

- Not a PGEN issue (the parse is correct; the blowup is VM-side execution).
- Not a rewrite of lookbehind evaluation strategy (a separate perf concern);
  this tree is about the LIMIT CONTRACT holding everywhere.

## Acceptance Criteria

- With harness-style limits set, `/(?<=(\d{1,256}))X/` on `12345XYZ`
  terminates (limit-hit or match) in well under 100ms.
- Family audit (family-fix doctrine): every sub-execution entry point —
  lookahead, lookbehind (`execute_lookbehind_assertion` /
  `execute_subexpr_ending_at` / `execute_subexpr_inner_full`), subroutine
  invocation, conditional probes, atomic-group probes — provably shares the
  same step/backtrack counters as the main loop (one counter, one budget).
- Regression tests for the named sibling paths; full gate + conformance
  ratchet green.

## Task Tree

- ID: `VM-LIMITS-SUBEXEC`
  Status: `done`
  Goal: `Make safety limits bound all sub-execution paths.`
  Children: `.1` `.2`

- ID: `VM-LIMITS-SUBEXEC.1`
  Status: `done`
  Goal: `Audit + fix: thread the shared step/backtrack budget through every subexpr execution entry point (the lookbehind path first — it is the proven-unbounded one); add regression tests incl. the testinput2:6509 shape under harness limits.`
  Acceptance: `Acceptance criteria 1–3 above; sibling named (family gate): lookahead probes and subroutine bodies verified to consume the same budget.`
  Verification: `2026-07-21 — see Verification Log.`
  Commit: `Bound every VM sub-execution by the attempt's budget (leaf VM-LIMITS-SUBEXEC.1)`

- ID: `VM-LIMITS-SUBEXEC.2`
  Status: `done`
  Goal: `Root-cause the suspected 1.1.81-vs-1.1.104 harness divergence for testinput2:6509.`
  Acceptance: `The divergence is explained with evidence; PGEN report filed iff PGEN-side drift.`
  Verification: `2026-07-21 — RESOLVED: NO pin divergence exists. The AST dumps for /(?<=(\d{1,256}))X/ are BYTE-IDENTICAL on 1.1.81 (db6f8c68) and 1.1.104 (960dddaa) (parseability_probe --parse-dump-ast-pretty, diff clean). Therefore the compiled program is identical and the limits-uncovered slow path fires on BOTH pins: past old-pin conformance runs were also spending ~tens of minutes inside testinput2:6509 unnoticed (the ratchet records green, not duration — the case does terminate eventually; the 2026-07-21 run was killed at ~25 min, not proven divergent). No PGEN report warranted. Side-effect of fixing .1: full-corpus conformance wall-time should drop dramatically.`
  Commit: `(investigation was docs-only; recorded in the session's docs commit)`

## Current Frontier

Empty — both leaves are `done`; the tree is closed.

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| — | `VM-LIMITS-SUBEXEC.1` | `done` | Shipped 2026-07-21. |
| — | `VM-LIMITS-SUBEXEC.2` | `done` | Resolved 2026-07-21 (no pin divergence). |

## Decisions

- `2026-07-21`: Tracked as its own tree (not under COMPILE-PERF-0078 — this is
  a correctness/safety bug independent of the PGEN bump that exposed it).
- `2026-07-21` (`.1`): **One attempt, one budget.** Rather than give each
  sub-execution its own limit, every dispatch loop charges the same
  `ctx.step_count`, and a sub-execution running on a cloned context folds its
  spend back into the parent (`absorb_sub_budget`). Alternative considered and
  rejected: per-sub-execution budgets — they compose multiplicatively
  (`starts.len()` × body budget), which is precisely the blowup being fixed.
- `2026-07-21` (`.1`): Budget exhaustion inside a sub-expression reports
  "body did not match", mirroring the main loop's "no match at this start
  position". No new error channel — the shipped contract is that limits
  degrade to no-match, not to an exception.
- `2026-07-21` (`.1`): The frame check inside the subexpr loop sums the outer
  stack with the local one. Deeper nesting levels hold their own local stacks
  and are bounded transitively by `max_steps`, since every push costs a step.
- `2026-07-21` (`.1`): When the honest counting turned two legitimate
  conformance cases red, the fix was to **recalibrate the harness cap
  (1M → 64M), not to rebaseline the ratchet**. The cases match in PCRE2 and
  must match in RGX; a baseline bump would have booked an accuracy regression
  as progress. The old cap was only ever meaningful under the bug.

## Open Questions

- ~~Does `execute_subexpr_inner_full` keep its own local `backtrack_stack`
  (observed at vm.rs:8001-8005) without consulting `ctx` limits, or does it
  check a budget that is simply not shared/decremented?~~ **Answered `.1`:
  both.** The local stack was never checked against `max_backtrack_frames`,
  AND the loop never incremented or consulted `ctx.step_count` — the body ran
  entirely off-budget. Cloned contexts compounded it: `clone_exec_context`
  copied `step_count` in but nothing ever copied it back out.

- **Adjacent, deliberately out of scope** (noted so it is not lost): setting any
  runtime limit gates Pike-VM OFF (`Engine::should_dispatch_to_c2`,
  `engine.rs`), because Pike-VM does not honour the limits — so limit-using
  callers are pushed onto the backtracking VM. That is a *dispatch/performance*
  trade-off, not a DoS hole (Pike-VM is linear-time), and it is already tracked
  in the Book's PCRE2 Conformance Audit chapter (Cluster 1G, Options A/B). It
  is not part of this tree.

## Blockers

- None.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-07-21` | — | discovery evidence: lldb backtraces (thread `pcre2_conformance_big_stack` in `execute_subexpr_inner_full`, subject `12345XYZ`, case line_number 6509), 20+ min pegged CPU with harness limits set; CLI `timeout 20` exit 124 on db6f8c68-era binary | `tree opened` |
| `2026-07-21` | `.1` | pre-fix reproduction re-confirmed on the shipped pin: `timeout 15 ./target/debug/rgx '(?<=(\d{1,256}))X' '12345XYZ'` → exit 124 (15.0s wall, CPU pegged) | `gap confirmed` |
| `2026-07-21` | `.1` | acceptance criterion 1 — release-build timing under harness limits (1M steps / 64K frames / depth 128): `(?<=(\d{1,256}))X` on `12345XYZ` = **37.4 ms** (matched, correct result) vs 20+ min before. Siblings: `(?<!(\d{1,256}))X` 31.3 ms; `(?=(a+)+b)c` on 60×`a` 294.8 ms; `(?(DEFINE)(?<p>(a+)+b))(?&p)c` 297.2 ms; `((a+)+)+b` 318.7 ms — all bounded (the ~300 ms figures are 61 start positions × a 1M-step per-attempt budget, i.e. the documented per-attempt semantics, not an escape) | `PASS` |
| `2026-07-21` | `.1` | acceptance criterion 2 — family audit: the VM has exactly three execution dispatch loops (`execute_at`, `execute_subexpr_inner_full`, `execute_at_continuation`; every other `OpCode::try_from(code[ip])` site is a static bytecode walker, not an executor) and exactly three cloned-context sites (`probe_subexpr`, `execute_assertion_subexpr`, `execute_lookbehind_assertion`). All three loops now charge `ctx.step_count` + check frames/trail; all three clone sites fold their spend back. Subroutine/recursion bodies (`invoke_subroutine_inner`) and conditional/atomic probes execute on the *shared* ctx through `execute_subexpr`, so they are covered by the same loop | `PASS` |
| `2026-07-21` | `.1` | acceptance criterion 3 — `cargo test -p rgx-core --test adversarial`: 51 passed / 0 failed (6 new limit-coverage tests: variable-length lookbehind, lookahead body, negative lookbehind, subroutine body, nested quantifier body, plus a no-semantic-drift check and an unlimited-stays-unlimited check) | `PASS` |
| `2026-07-21` | `.1` | **first** full-gate run FAILED the ratchet: 12,804 pass / 6 fail vs baseline 12,806 / 4. Both regressions were `testinput1:4545` `/^(?:((.)(?1)\2\|)\|((.)(?3)\4\|.))$/i` (palindrome subjects). Root-caused by bisecting each limit independently on that case: `steps=1M` alone → no-match; `frames=64K` alone → match; `depth=128` alone → match; unlimited → match in ~0.49s. Threshold measured by escalation: 32M steps → no-match, 36M → match (21-char subject: 4M no / 8M yes). Conclusion: **the harness's 1M step cap was calibrated against an engine that under-counted** — it never measured sub-execution work. Cap recalibrated 1M → **64M** (~1.9× the worst legitimate case); frames/depth unchanged (proven non-binding) | `regression root-caused, not rebaselined` |
| `2026-07-21` | `.1` | conformance re-run at the recalibrated cap: **12,806 pass / 4 fail / 0 panic / 0 skip — original baseline restored, RATCHET OK**, in **60.32s** wall (the discovery-session run was killed at ~25 min). Cost of the 64× cap raise: +8.7s over the same sweep at 1M | `PASS` |
| `2026-07-21` | `.1` | full gate `RGX_RUN_CONFORMANCE=1 ./scripts/run-local-ci.sh` re-run end-to-end on the final tree (green receipt) | `see CHANGES.md entry` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| `.1` | `Bound every VM sub-execution by the attempt's budget (leaf VM-LIMITS-SUBEXEC.1)` | Fix + 6 regression tests + Book safety-limits/the-vm/performance updates. |
| `.2` | (investigation was docs-only; recorded in the 2026-07-21 `COMPILE-PERF-0078.1` docs commit) | No pin divergence existed. |

## Changelog

- `2026-07-21`: Created from the PGEN-1.1.104 adoption-attempt findings
  (session: COMPILE-PERF-0078.1 execution).
- `2026-07-21`: `.1` shipped; both leaves `done`; tree closed.
