---
id: pcre2-conformance-ratchet
title: PCRE2 conformance ratchet — baseline location, value, and the 4 by-design residuals
answers:
  - "what is RGX's PCRE2 conformance pass count"
  - "where is the conformance ratchet baseline"
  - "what are PASS_BASELINE / FAIL_BASELINE"
  - "why are there 4 PCRE2 conformance residuals"
  - "are the conformance residuals bugs"
  - "what is the merge gate for parsing / VM / compiler changes"
date: 2026-06-15
status: current
tags: [conformance, pcre2, testing, gate]
evidence: rgx-core/tests/pcre2_conformance.rs:3408 (PASS_BASELINE=12_806 / FAIL_BASELINE=4 / PANIC_BASELINE=0 / SKIP_BASELINE=0)
reverify: grep -n PASS_BASELINE rgx-core/tests/pcre2_conformance.rs
---

RGX runs PCRE2 10.47's full `testinput1..29` corpus and ratchet-gates the result
in `rgx-core/tests/pcre2_conformance.rs`: **12,806 pass / 4 fail / 0 panic / 0
skip**. The 4 fails are **by-design, won't-fix** engine-model adjudications
(Unicode/code-point vs PCRE2 8-bit non-UTF), documented in the Book's PCRE2
Conformance Residual chapter — they are not RGX defects or PGEN bugs.

This ratchet is the **merge condition** for any change touching parsing, the
PGEN adapter, the VM/compiler, or the harness (run via
`RGX_RUN_CONFORMANCE=1 ./scripts/run-local-ci.sh`): a regression fails CI; an
improvement bumps the baseline in the same commit. Canonical home: the Book →
PCRE2 Conformance Residual + `docs/decisions/0004` (accuracy-first gate).
