# CHANGES
This is the living progress ledger for rgx.

## How this file is used
- Append new entries at the top (newest first).
- Record what changed, why it changed, and how it was validated.
- Keep entries factual and implementation-focused.

## Entry template
### YYYY-MM-DD - Short title
- Scope:
- Changes:
- Validation:
- Notes/impact:

## Entries
### 2026-02-18 - Added parser and codegen support for `(?:...)` and `(?<name>...)`
- Scope: `rgx-core` lexer/parser/compiler integration
- Changes:
  - Extended lexer group parsing to emit:
    - `Token::NonCapturingGroupStart` for `(?:...)`
    - `Token::NamedGroupStart { name }` for `(?<name>...)`
  - Extended parser to build AST `Regex::Group` nodes for both syntaxes
  - Updated VM compiler group codegen to preserve group kind semantics:
    - capturing groups emit capture save opcodes
    - non-capturing groups compile without allocating captures
  - Added lexer/parser tests for both new syntaxes
- Validation:
  - `cargo test -p rgx-core` passed (42 tests)
  - CLI smoke tests passed:
    - `rgx-cli "(?:cat|dog)" "pet dog"` -> `4..7`
    - `rgx-cli "(?<word>cat)" "catnap"` -> `0..3`
- Notes/impact:
  - Brings parser behavior closer to common regex expectations for grouping semantics
  - Does not yet add lookaround or inline code-block parser support
### 2026-02-18 - Documentation quality reset and consolidation
- Scope: repository markdown documentation set
- Changes:
  - Rewrote core docs for accuracy and maintainability: `README.md`, `CHANGES.md`, `DEVELOPMENT_NOTES.md`, `PROJECT_VISION.md`, `docs/architecture.md`, `docs/TECHNICAL_DECISIONS.md`
  - Removed stale/redundant docs that conflicted with current implementation state:
    - `ROADMAP.md`
    - `docs/GETTING_STARTED.md`
    - `docs/extensibility.md`
    - `docs/implementation-status.md`
    - `docs/vm-implementation-guide.md`
  - Established this file (`CHANGES.md`) as the explicit living progress tracker
- Validation:
  - Verified documentation set for internal consistency
  - Confirmed retained docs now separate current status from long-term vision
- Notes/impact:
  - Reduced doc/code drift
  - Created one stable progress ledger for future sessions

### 2025-10-06 - Performance benchmark baseline and Lua foundation
- Scope: benchmarking and execution infrastructure
- Changes:
  - Added benchmark baseline for rgx vs PCRE2 in `rgx-bench`
  - Added Lua execution infrastructure foundation and execution-manager scaffolding
- Validation:
  - Benchmark harness runs and records comparative throughput/compile metrics
- Notes/impact:
  - Established measurable baseline for future optimization work

### 2025-09-07 - VM milestone completion
- Scope: `rgx-core` VM and compiler path
- Changes:
  - Built comprehensive VM execution engine and multi-pass compiler structure
  - Added VM tests covering core feature paths
- Validation:
  - VM test suite established and passing for covered features
- Notes/impact:
  - Enabled practical end-to-end regex execution through the VM backend

### 2025-09-02 to 2025-09-04 - Project bootstrap and parser/compiler foundation
- Scope: workspace setup and core compilation pipeline
- Changes:
  - Initialized workspace crates (`rgx-core`, `rgx-cli`, `rgx-bench`, `rgx-wasm`)
  - Implemented early lexer/parser/AST/compiler foundations
- Validation:
  - Early crate compilation and base tests
- Notes/impact:
  - Established architecture used by all later work
