---
id: pgen-sole-open-bug
title: Open PGEN reports are 0089/0090/0091 (fast-pin blockers); 0078 closed upstream 2026-07-21
answers:
  - "which PGEN-RGX reports are still open"
  - "is PGEN-RGX-0078 still open"
  - "what is the only open PGEN bug"
  - "what PGEN issue tracks compile-time performance"
  - "why is the new fast PGEN pin not adopted"
date: 2026-07-21
status: current
tags: [pgen, ledger, compile-perf]
evidence: pgen-issues/PGEN-RGX-0089.yaml/0090/0091 (status open); PGEN-RGX-0078.yaml (status closed 2026-07-21)
reverify: grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml
---

**`PGEN-RGX-0078` (compile-time perf) is CLOSED** — PGEN's speed campaign
delivered (RGX-verified 2026-07-21 on preview pin `960dddaa`: raw parse 26.9×
faster; 8.44× vs PCRE2-no-JIT / 1.17× vs +JIT; bundle in
`pgen-issues/artifacts/PGEN-RGX-0078/measurements/*_1.1.104_preview*`).
Upstream adjudicated closure against its director's re-based absolute sub-1µs
bar; the RGX `<5×` ROADMAP bar is unmet (8.44×) and awaiting a director ruling
(see `docs/tasks/COMPILE-PERF-0078.md`).

**Open PGEN-RGX reports (all filed 2026-07-21 against the fast pin, blocking
its adoption):** `0089` — `(?[\b])`/`(?[[\b]])` rejects-valid regression
(PCRE2-oracle-proven); `0090` — cold-clone `regex_parser_bootstrap` broken
(seed REFUSED + exit-0); `0091` — embedding version constants drifted from the
contract identity (0086-class). The shipped pin stays `db6f8c68` (rel 1.1.81)
until a PGEN release fixes 0089. The `reverify` command must return exactly
0089/0090/0091.

Parser defects are fixed upstream in PGEN, never worked around in RGX
(`docs/decisions/0001`, PGEN sole parser).
Related cards: [[pgen-build-regenerate]], [[pgen-1104-verified-blocked]].
