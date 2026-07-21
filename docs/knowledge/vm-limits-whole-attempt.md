---
id: vm-limits-whole-attempt
title: Safety limits are whole-attempt — every VM dispatch loop shares one budget
answers:
  - "do set_max_steps limits cover lookbehind and lookahead bodies"
  - "how many execution dispatch loops does the VM have"
  - "where does the VM check max_steps"
  - "why did a variable-length lookbehind hang despite set_max_steps"
  - "what is absorb_sub_budget"
  - "do cloned exec contexts get their own step budget"
  - "why is the conformance harness step cap 64M"
  - "how many steps does a recursive palindrome pattern need"
date: 2026-07-21
status: current
tags: [vm, safety-limits, dos, execution]
evidence: rgx-core/src/vm.rs — step_budget_exhausted / absorb_sub_budget; loops execute_at, execute_subexpr_inner_full, execute_at_continuation; clone sites probe_subexpr, execute_assertion_subexpr, execute_lookbehind_assertion. Tests rgx-core/tests/adversarial.rs::limits_bound_*
reverify: grep -n "step_budget_exhausted\|absorb_sub_budget\|clone_exec_context" rgx-core/src/vm.rs
---

The VM has **exactly three execution dispatch loops**, not one:

1. `execute_at` — top-level match attempt.
2. `execute_subexpr_inner_full` — every sub-execution: assertion bodies,
   lookbehind candidate starts, quantifier bodies, subroutine/recursion bodies
   (via `invoke_subroutine_inner`), conditional and atomic-group probes.
3. `execute_at_continuation` — the async suspend/resume path.

All other `OpCode::try_from(code[ip])` sites in `vm.rs` are **static bytecode
walkers** (prefix-filter extraction, literal extraction, lookbehind codepoint
bounds, inline-class id remap), not executors — do not mistake them for
dispatch loops when auditing.

There are **exactly three cloned-context sites** (`clone_exec_context`):
`probe_subexpr`, `execute_assertion_subexpr`, `execute_lookbehind_assertion`.

**The invariant (since 2026-07-21, leaf `VM-LIMITS-SUBEXEC.1`): one match
attempt = one budget.** All three loops charge the same `ctx.step_count` and
check `max_steps` / `max_backtrack_frames` / `max_trail_entries`; all three
clone sites call `absorb_sub_budget` to fold the sub-context's spend back into
the parent. `clone_exec_context` copies `step_count` *in*; `absorb_sub_budget`
copies it *back out*. Both halves are required — before the fix only the first
existed, so every clone silently restarted the budget.

**Why this matters historically:** with only `execute_at` counting,
`/(?<=(\d{1,256}))X/` on the 8-byte subject `12345XYZ` (PCRE2 `testinput2:6509`)
ran **20+ minutes** at pegged CPU with `set_max_steps(1_000_000)` in force. It
now completes in ~37 ms. The same defect is the stated reason `testinput15` is
excluded from the conformance corpus (see `CONFORMANCE-TESTINPUT15`).

**Calibration consequence:** the conformance harness's step cap is
**64,000,000** (`rgx-core/tests/pcre2_conformance.rs`), not the old 1,000,000.
The old value was set while sub-execution work was invisible, so it never
measured real work. Under honest counting the worst *legitimate* corpus case —
`testinput1:4545` `/^(?:((.)(?1)\2|)|((.)(?3)\4|.))$/i` on the 31-char
palindrome "Satanoscillatemymetallicsonatas" — needs **~33M steps** (~0.5s) and
wrongly reported no-match at 1M. Frame (65,536) and depth (128) caps were
bisected and proven non-binding on that case. Do not "fix" a red ratchet by
lowering it: the ratchet stayed at 12,806 / 4 / 0 / 0 through this change.

The budget remains **per starting position** (each scan position gets a fresh
one), so a single `find_first` call costs up to (positions tried) × `max_steps`.
That is by design and documented in the Book → Core API → Safety Limits.
