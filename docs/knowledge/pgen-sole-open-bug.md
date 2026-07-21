---
id: pgen-sole-open-bug
title: Zero open PGEN-RGX reports — all 91 (0001–0091) closed as of 2026-07-21
answers:
  - "which PGEN-RGX reports are still open"
  - "is PGEN-RGX-0078 still open"
  - "what is the only open PGEN bug"
  - "what PGEN issue tracks compile-time performance"
  - "why is the new fast PGEN pin not adopted"
  - "how many PGEN bugs has RGX filed"
date: 2026-07-21
status: current
tags: [pgen, ledger, compile-perf]
evidence: grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml returns NOTHING; 0089/0090/0091 closed with verification notes on pin d9d41c28
reverify: grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml   # must print nothing
---

**There are currently NO open PGEN-RGX reports.** All 91 filed against this
codebase (`0001`–`0091`) are closed. The `reverify` command must print nothing;
if it prints a file, this card is stale.

The last three to close were the fast-pin adoption blockers, all filed
2026-07-21 and all fixed upstream the same day (adopted pin `d9d41c28`, PGEN
release **1.1.106** / contract **1.1.109**):

- **`0089`** — `(?[\b])` / `(?[[\b]])` rejects-valid regression. Fixed upstream
  as ledger `REGEX-0115` (grammar-owned `extended_class_backspace_escape`).
  RGX-verified: both forms match U+0008 again; the two extended-class fixtures
  are green.
- **`0090`** — cold-clone `regex_parser_bootstrap` broken. RGX-verified by
  deleting `subs/pgen/generated/` and running the canonical target: it
  bootstraps end-to-end with no `--bootstrap-mode` workaround.
- **`0091`** — embedding version constants drifted from contract identity.
  RGX-verified: `parser_embedding_api_contract()` reports `1.1.106`/`1.1.109`,
  matching the contract document exactly.

**`PGEN-RGX-0078` (compile-time perf) closed earlier the same day** — the speed
campaign delivered and is now *shipped* in RGX, not merely verified: raw parse
geomean **2,813 ns** on the adopted pin = **26.3× faster** than the old
`db6f8c68`, **8.64× vs PCRE2-no-JIT**, **1.20× vs PCRE2+JIT** (3/8 patterns
parse faster than PCRE2 compiles with JIT). Bundle:
`pgen-issues/artifacts/PGEN-RGX-0078/measurements/ratio_table_1.1.106_adopted.md`.

The ROADMAP's `<5×`-vs-PCRE2-no-JIT bar is still unmet, and the director ruled
(ADR 0005) that it is KEPT — but closing it is now **RGX-side** work: raw parse
is only ~4% of `Regex::compile`; the adapter boundary and the eager C2 build
dominate (`COMPILE-PERF-0078.3` / `.4`).

Parser defects are fixed upstream in PGEN, never worked around in RGX
(`docs/decisions/0001`, PGEN sole parser).
Related cards: [[pgen-build-regenerate]], [[pgen-1104-verified-blocked]].
