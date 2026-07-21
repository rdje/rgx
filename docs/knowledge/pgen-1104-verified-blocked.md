---
id: pgen-1104-verified-blocked
title: PGEN fast pin verified (parse 26.9× faster; compile bottleneck now RGX-side) but adoption blocked by 0089
answers:
  - "is PGEN regex parsing fast now"
  - "how fast is PGEN parse vs PCRE2 compile"
  - "what dominates Regex::compile time"
  - "should RGX optimize its own compile path"
  - "what happened to the PGEN speed campaign"
  - "what does adopting the fast PGEN pin require"
date: 2026-07-21
status: current
tags: [pgen, compile-perf, measurements]
evidence: pgen-issues/artifacts/PGEN-RGX-0078/measurements/ratio_table_1.1.104_preview.md; phase_split_1.1.104_preview.txt
reverify: cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser  # on the adopted pin
---

Measured 2026-07-21 on preview pin `960dddaa` (rel 1.1.105/contract 1.1.108),
RGX standard instrument (default allocator, release, p50/5000, same-session
PCRE2 baselines): **raw PGEN parse geomean 2.75µs** (was ~74µs on `db6f8c68` →
**26.9× faster**), **8.44× vs PCRE2-no-JIT**, **1.17× vs PCRE2+JIT** (5/8
patterns beat PCRE2+JIT compile). PGEN's own closure config (fat-LTO +
mimalloc, direct path) reads ≈3.9×; its embedding-path release-day read was
6.6× — all instruments agree the `<5×`-vs-no-JIT line is not met, but the gap
vs the old pin collapsed 25×.

**Compile-phase inversion:** on the fast pin raw parse is only ~4–18% of
`Regex::compile` (geomean ≈42µs ⇒ ≈130× no-JIT). The dominant costs are now
RGX-side: the AST-dump adapter boundary (JSON serialize→serde→walker,
~10–25µs/pattern) and the eager C2 program build (~12–36µs). RGX levers:
`COMPILE-PERF-0078.3` (native `ParseContent::Shaped`/`to_json_value` fast
path — contract-sanctioned) and `.4` (lazy C2 build). The old "PGEN dominates
compile, no RGX fix helps" fact is OBSOLETE on the fast pin.

**Adoption checklist (when PGEN ships the 0089 fix):** bump pin → regenerate
(`make bootstrap`; if the cold-clone seed still refuses, see 0090's
`--bootstrap-mode` workaround) → absorb REGEX-0098 (4 tests: match error CODE
`E_PARSE_FAILURE`, never message text; `(?&word)`-style fixtures move to
must-reject) → full gate + conformance ratchet (expect long testinput2 wall
time until the `VM-LIMITS-SUBEXEC` tree's fix lands) → re-measure + update
Book performance chapter. Related: [[pgen-sole-open-bug]].
