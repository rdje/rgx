# Decision records (layer C)

Durable, cross-cutting facts and decisions for RGX — constraints, conventions,
environment quirks, and "tried X, failed because Y." This is **layer C** of the
[memory architecture](../../MEMORY_ARCHITECTURE.md): one ADR-style record per
file (*Context → Decision → Consequences*), append-once, supersede-don't-mutate.

These records are the **in-repo system of record** for facts that were previously
held only in harness-home memory (`~/.claude/…`) — so they survive a harness or
model switch. Some are also mirrored as one-line rules in `CLAUDE.md`; the ADR
here is the authoritative, durable version.

| ID | Title | Status | Topic |
| --- | --- | --- | --- |
| [0001](0001-pgen-sole-parser-no-workarounds.md) | PGEN is the sole parser — no builtin fallback, no RGX-side parser workarounds | accepted | parser / PGEN |
| [0002](0002-pgen-submodule-readonly-regenerate.md) | `subs/pgen` is read-only; regenerate `generated/*` after every PGEN bump | accepted | build / PGEN |
| [0003](0003-release-strategy-cratesio-on-hold.md) | crates.io publication is on hold; the PGEN regex parser is the eventual vehicle; user is the trigger | accepted | release |
| [0004](0004-accuracy-first-conformance-ratchet-is-the-gate.md) | Accuracy-first: the PCRE2 differential conformance ratchet is the merge condition | accepted | quality / testing |
| [0005](0005-compile-perf-5x-bar-kept-rgx-side.md) | The `<5×`-of-PCRE2 compile bar is kept; closing it is now RGX-side work | accepted | performance / compile-time |
| [0006](0006-doctrine-enforcement-architecture-adopted.md) | The Doctrine Enforcement Architecture is adopted; every mechanizable doctrine gets a check | accepted | governance / enforcement |

## Conventions

- File name: `NNNN-kebab-title.md` (zero-padded, monotonic).
- Each record carries: `Status` (proposed/accepted/superseded), `Date`, and the
  three sections Context / Decision / Consequences.
- To change a fact: add a new record (or set the old one's Status to
  `superseded by NNNN`). Never silently rewrite history.
- Add the new row to this index in the same commit.
