---
id: pgen-sole-open-bug
title: PGEN-RGX-0078 is the sole open PGEN bug; it replaces 0073; all others addressed
answers:
  - "which PGEN-RGX reports are still open"
  - "is PGEN-RGX-0073 still open"
  - "what is the only open PGEN bug"
  - "did 0078 replace 0073"
  - "what PGEN issue tracks compile-time performance"
date: 2026-06-15
status: current
tags: [pgen, ledger, compile-perf]
evidence: pgen-issues/PGEN-RGX-0078.yaml (status open); PGEN-RGX-0073.yaml (status closed, superseded_by 0078)
reverify: grep -l '^status: open' pgen-issues/PGEN-RGX-*.yaml
---

Per the PGEN maintainer (2026-06-15), **`PGEN-RGX-0078` is the sole active/open
PGEN-RGX report** — the compile-time-performance tracker (PGEN regex parse is
≈63–86% of `Regex::compile`; PGEN-side, sole-parser design, no RGX fix). It
**replaces `PGEN-RGX-0073`** (now `status: closed`, `resolution: superseded`).
**Every other report has been addressed.** The `reverify` command must return
exactly `PGEN-RGX-0078`.

Parser defects are fixed upstream in PGEN, never worked around in RGX
(`docs/decisions/0001`, PGEN sole parser). Closed by the `LEDGER-HYGIENE` task-tree.
Related card: [[pgen-build-regenerate]].
