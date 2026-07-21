# 0005 — the `<5×`-of-PCRE2 compile bar is KEPT; closing it is now RGX-side work

- Status: accepted
- Date: 2026-07-21
- Topic: performance / compile-time target
- Decided by: director (session ruling, 2026-07-21)

## Context

PGEN closed its `PGEN-RGX-0078` speed campaign (2026-07-21) against its own
director's re-based ABSOLUTE bar (sub-1µs corpus geomean), explicitly retiring
the relative `<5×`-vs-PCRE2 line upstream. RGX verified the delivery on the
preview pin `960dddaa`: raw parse 26.9× faster; **8.44× vs PCRE2-no-JIT** /
1.17× vs PCRE2+JIT on RGX's standard instrument — a 25× improvement, but the
RGX ROADMAP bar (`Regex::compile` geomean `<5×` of PCRE2-no-JIT compile) is
still unmet. Phase-splitting shows the bottleneck INVERTED: raw PGEN parse is
now only ~4–18% of `Regex::compile`; the dominant costs are RGX's own AST-dump
adapter boundary (~10–25µs/pattern) and the eager C2 program build
(~12–36µs). The question "keep or re-base RGX's `<5×` bar?" was surfaced to
the director.

## Decision

**Keep the `<5×` objective. The ball is now in RGX's court.** RGX does not
follow PGEN in re-basing to an absolute bar; the `COMPILE-PERF-0078` tree's
acceptance criterion (`Regex::compile` geomean `<5×` of PCRE2-10.47-no-JIT on
the 8-pattern corpus, RGX standard methodology) stands unchanged, and the
remaining gap is owned by RGX-side leaves: `.3` (adapter-boundary fast path —
native `ParseContent` consumption instead of the JSON round-trip), `.4`
(lazy/skippable C2 program construction), `.2` (trivial-pattern
short-circuit), executed after the fast pin is adopted (blocked on the
`PGEN-RGX-0089` fix release).

## Consequences

- `COMPILE-PERF-0078` remains the owning tree; its acceptance bar is
  unchanged; no ROADMAP target retirement.
- Further PGEN-side parse speedups are welcome but no longer the plan of
  record for reaching `<5×`; RGX work is.
- Measurement discipline unchanged: RGX's default-allocator standard
  instrument is the ground truth for the bar (PGEN's fat-LTO+mimalloc
  configuration numbers are informative, not the criterion).
