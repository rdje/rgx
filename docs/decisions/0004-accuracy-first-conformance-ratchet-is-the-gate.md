# 0004 — Accuracy-first: the PCRE2 differential conformance ratchet is the merge condition

- Status: accepted
- Date: 2026-06-15
- Topic: quality / testing
- Supersedes harness-only memory: `feedback_accuracy_first_then_speed`

## Context

RGX targets sign-off-level quality. Correctness against PCRE2 is verified by a
differential conformance harness (`rgx-core/tests/pcre2_conformance.rs`) run over
the full `testinput1..29` corpus, with a ratchet gate at
`PASS_BASELINE`/`FAIL_BASELINE`/`PANIC_BASELINE`/`SKIP_BASELINE` (currently
**12,806 / 4 / 0 / 0**). Speed work (compile-time and match-time) is valuable but
must never trade away correctness.

## Decision

- **100% accuracy first, speed second.** A change is not done until correctness
  is proven; performance targets only count after correctness holds.
- **The conformance ratchet is THE merge condition**, not a nice-to-have. Any
  change touching parsing, the PGEN adapter, the VM/compiler, or the harness must
  run the ratchet (`RGX_RUN_CONFORMANCE=1 ./scripts/run-local-ci.sh`) and keep it
  green. A regression fails CI; an improvement bumps the baselines in the same
  commit (with in-source justification for any intentional rebaseline).
- The 4 residuals are by-design Unicode/8-bit engine-model adjudications
  (won't-fix), documented in the Book.

## Consequences

- Perf trees (`PERF-SOTA-GAPS`, `COMPILE-PERF-0078`, `RUNTIME-REMEASURE`) carry
  "ratchet held" as an explicit acceptance criterion on every leaf.
- "Done" means gate-green by a fresh receipt, never eyeballed/filtered output
  (cf. the 2026-04-07→05-18 deep-nesting masking incident).
