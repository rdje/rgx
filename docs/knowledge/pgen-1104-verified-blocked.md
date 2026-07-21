---
id: pgen-1104-verified-blocked
title: The fast PGEN pin is ADOPTED (1.1.106) — parse 26.3x faster; the compile bottleneck is now RGX-side
answers:
  - "is PGEN regex parsing fast now"
  - "how fast is PGEN parse vs PCRE2 compile"
  - "what dominates Regex::compile time"
  - "should RGX optimize its own compile path"
  - "what happened to the PGEN speed campaign"
  - "what does adopting the fast PGEN pin require"
  - "which PGEN pin is shipped"
date: 2026-07-21
status: current
tags: [pgen, compile-perf, measurements]
evidence: pgen-issues/artifacts/PGEN-RGX-0078/measurements/ratio_table_1.1.106_adopted.md; pgen_parse_p50_1.1.106_adopted.txt
reverify: cargo run --release -p rgx-core --example pgen_compile_perf_dump --features pgen-parser
---

**ADOPTED 2026-07-21** (supersedes this card's former "verified but blocked"
state): `subs/pgen` is pinned at `d9d41c28`, PGEN regex release **1.1.106** /
integration contract **1.1.109**. The three adoption blockers (`PGEN-RGX-0089`
rejects-valid, `0090` cold-clone bootstrap, `0091` version-constant drift) were
all fixed upstream and verified on this pin — see [[pgen-sole-open-bug]].

**Measured on the adopted pin** (RGX standard instrument: default allocator,
release, p50/5000, same-session PCRE2 C baselines): raw PGEN parse geomean
**2,813 ns** — **26.3× faster** than the old `db6f8c68` (~74µs), **8.64× vs
PCRE2-no-JIT**, **1.20× vs PCRE2+JIT**; 3/8 corpus patterns parse faster than
PCRE2 *compiles with JIT*. Reproduces the 1.1.104 preview measurement within
+2.4%.

**Compile-phase inversion (the actionable fact):** raw parse is now only ~4% of
`Regex::compile`. The dominant costs are RGX-side — the AST-dump adapter
boundary (PGEN serialize → serde deserialize → walker) and the eager C2 program
build. RGX levers: `COMPILE-PERF-0078.3` (consume `ParseContent::Shaped`
natively or `to_json_value()` at the boundary — contract-sanctioned) and `.4`
(lazy C2 build). The old "PGEN dominates compile, no RGX fix helps" fact is
**obsolete**; the `<5×` bar (kept by director ruling, ADR 0005) is RGX's to
close.

**What adoption took (for the next bump):** delete `subs/pgen/generated/` and
re-run `make -C subs/pgen/rust SHELL=/bin/bash regex_parser_bootstrap` — the
idempotent `make bootstrap` will NOT refresh a stale parser after a pin change,
and a stale `generated/ebnf.rs` fails to compile against newer sources. Then
absorb REGEX-0098 (unknown named references now reject at parse time: match the
diagnostic CODE `E_PARSE_FAILURE`, never message prose) — 4 RGX test surfaces.
RGX consumes only the serialized `parse_regex_default_ast_dump` path, so the
Rust-level `ParseNode` slimming (72→48 bytes) and the
`ParseContent::Json`→`Shaped(PgenValue)` carrier change do **not** reach RGX;
`regex_ast_dump_schema_version` stayed `1` and the AST dumps are byte-identical
apart from `api_version`.
