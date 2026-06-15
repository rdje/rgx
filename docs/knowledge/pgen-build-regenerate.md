---
id: pgen-build-regenerate
title: Fresh clone / PGEN bump needs the generated parser regenerated before cargo build
answers:
  - "cargo build fails missing generated parser"
  - "subs/pgen generated files are missing"
  - "how do I regenerate the PGEN regex parser"
  - "why does include of return_annotation_parser.rs fail"
  - "what does the untracked subs/pgen status mean"
date: 2026-06-15
status: current
tags: [build, pgen, submodule, environment]
evidence: README.md "Build note"; docs/decisions/0002-pgen-submodule-readonly-regenerate.md
reverify: ls subs/pgen/generated/regex_parser.rs subs/pgen/generated/return_annotation_parser.rs
---

Since PGEN commit `0ed2b2ad`, PGEN no longer tracks its generated parser
artifacts (`subs/pgen/generated/*` — too large to vendor). On a fresh clone or
after every `subs/pgen` pin bump they are absent and `cargo build -p rgx-core`
fails (PGEN's `include!` of the generated parser). Fix: run
`make -C subs/pgen/rust SHELL=/bin/bash regex_parser_bootstrap` (idempotent
cold-clone bootstrap), then rebuild — never trust a stale `target/` after a bump.

`subs/pgen` is **read-only** from RGX; scope tooling (`cargo fmt -p rgx-core`,
never bare). The expected clean state shows only `?? subs/pgen` (regenerated
untracked `generated/*`) — never staged. Canonical home: `docs/decisions/0002`.
