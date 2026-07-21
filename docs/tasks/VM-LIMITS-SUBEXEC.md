# VM-LIMITS-SUBEXEC: safety limits must bound sub-execution paths

## Metadata

- Tree ID: `VM-LIMITS-SUBEXEC`
- Status: `active`
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
  Status: `active`
  Goal: `Make safety limits bound all sub-execution paths.`
  Children: `.1` `.2`

- ID: `VM-LIMITS-SUBEXEC.1`
  Status: `pending`
  Goal: `Audit + fix: thread the shared step/backtrack budget through every subexpr execution entry point (the lookbehind path first — it is the proven-unbounded one); add regression tests incl. the testinput2:6509 shape under harness limits.`
  Acceptance: `Acceptance criteria 1–3 above; sibling named (family gate): lookahead probes and subroutine bodies verified to consume the same budget.`
  Verification: `pending`
  Commit: `pending`

- ID: `VM-LIMITS-SUBEXEC.2`
  Status: `done`
  Goal: `Root-cause the suspected 1.1.81-vs-1.1.104 harness divergence for testinput2:6509.`
  Acceptance: `The divergence is explained with evidence; PGEN report filed iff PGEN-side drift.`
  Verification: `2026-07-21 — RESOLVED: NO pin divergence exists. The AST dumps for /(?<=(\d{1,256}))X/ are BYTE-IDENTICAL on 1.1.81 (db6f8c68) and 1.1.104 (960dddaa) (parseability_probe --parse-dump-ast-pretty, diff clean). Therefore the compiled program is identical and the limits-uncovered slow path fires on BOTH pins: past old-pin conformance runs were also spending ~tens of minutes inside testinput2:6509 unnoticed (the ratchet records green, not duration — the case does terminate eventually; the 2026-07-21 run was killed at ~25 min, not proven divergent). No PGEN report warranted. Side-effect of fixing .1: full-corpus conformance wall-time should drop dramatically.`
  Commit: `(investigation was docs-only; recorded in the session's docs commit)`

## Current Frontier

| Order | Leaf | Status | Why next |
| --- | --- | --- | --- |
| 1 | `VM-LIMITS-SUBEXEC.1` | `pending` | Proven-unbounded DoS surface; violates the shipped safety contract. |
| 2 | `VM-LIMITS-SUBEXEC.2` | `pending` | Explains the adoption-attempt divergence; cheap once .1's instrumentation exists. |

## Decisions

- `2026-07-21`: Tracked as its own tree (not under COMPILE-PERF-0078 — this is
  a correctness/safety bug independent of the PGEN bump that exposed it).

## Open Questions

- Does `execute_subexpr_inner_full` keep its own local `backtrack_stack`
  (observed at vm.rs:8001-8005) without consulting `ctx` limits, or does it
  check a budget that is simply not shared/decremented? Resolve in `.1`.

## Blockers

- None.

## Verification Log

| Date | Leaf | Checks | Result |
| --- | --- | --- | --- |
| `2026-07-21` | — | discovery evidence: lldb backtraces (thread `pcre2_conformance_big_stack` in `execute_subexpr_inner_full`, subject `12345XYZ`, case line_number 6509), 20+ min pegged CPU with harness limits set; CLI `timeout 20` exit 124 on db6f8c68-era binary | `tree opened` |

## Commit Log

| Leaf | Commit subject or reference | Notes |
| --- | --- | --- |
| — | `pending` | — |

## Changelog

- `2026-07-21`: Created from the PGEN-1.1.104 adoption-attempt findings
  (session: COMPILE-PERF-0078.1 execution).
