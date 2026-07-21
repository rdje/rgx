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
fails (PGEN's `include!` of the generated parser). On a FRESH clone: run
**`make bootstrap`** at the RGX root (the simple wrapper — see
[[rgx-build-as-submodule]]), which runs
`make -C subs/pgen/rust SHELL=/bin/bash regex_parser_bootstrap` for you.

⚠️ **After a PIN BUMP, `make bootstrap` is NOT enough** (learned the hard way
2026-07-21 adopting 1.1.106). It is idempotent on *existence*: seeing a
`generated/` directory it prints "already generated — nothing to bootstrap" and
leaves the PREVIOUS pin's parser in place, so you silently keep testing the old
grammar. Worse, a stale `generated/ebnf.rs` may no longer compile against newer
PGEN sources at all (1.1.106 changed `ParseNode.span` to `u32`, giving ~1,500
type errors). The correct sequence is:

```bash
rm -rf subs/pgen/generated                                   # reproduce a cold clone
make -C subs/pgen/rust SHELL=/bin/bash regex_parser_bootstrap
```

Deleting `generated/` is safe and is NOT a submodule modification — it is
gitignored and untracked inside `subs/pgen`.

`subs/pgen` is **read-only** from RGX; scope tooling (`cargo fmt -p rgx-core`,
never bare). **Clean-state note:** through pin `db6f8c68` the expected clean
state showed `?? subs/pgen` (untracked `generated/*`). Since pin `d9d41c28`
(1.1.106) the submodule gitignores `generated/`, so a correct working tree is
now **completely clean** — no `?? subs/pgen` marker even though the generated
parser is present. Never stage it either way. Canonical home:
`docs/decisions/0002`.
