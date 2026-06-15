# 0001 — PGEN is the sole parser; no builtin fallback, no RGX-side parser workarounds

- Status: accepted
- Date: 2026-06-15
- Topic: parser / PGEN integration
- Supersedes harness-only memory: `feedback_no_pgen_workarounds`

## Context

RGX's regex syntax is parsed exclusively by the PGEN-generated parser (consumed
via the embedding API, vendored as the `subs/pgen` submodule). There is no
hand-written builtin parser on the default path (the recursive-descent
`parser.rs` is a reference backend behind a constant, not a fallback). When PGEN
produces a wrong AST, rejects a valid pattern, or accepts an invalid one, the
temptation is to "absorb" the malformed output with adapter code in
`rgx-core/src/parsing.rs`.

## Decision

- **PGEN is the single source of truth for parsing.** No builtin parser fallback
  on the default path.
- **Parsing defects are fixed upstream in PGEN, never worked around in RGX.** If
  PGEN's parser is wrong, first confirm it is genuinely a PGEN bug (use
  `parseability_probe` / verify the AST dump), then file a structured report in
  `pgen-issues/` per `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md` (cluster-first).
- **Never add RGX adapter code that compensates for malformed PGEN output.**

## Consequences

- The PGEN-RGX issue ledger (`pgen-issues/`) is the channel for parser
  correctness; RGX carries no parser-shim debt.
- Some fixes block on a PGEN release + a `subs/pgen` pin bump (a code change,
  task-tree-owned — see ADR [0002](0002-pgen-submodule-readonly-regenerate.md)).
- The current open PGEN-side tracker is `PGEN-RGX-0078` (compile-time perf,
  replaces `0073`); all other reports are addressed.
