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
### 2026-02-19 - Implemented atomic-group no-backtracking runtime semantics
- Scope: `rgx-core` VM/compiler behavior for `(?>...)` groups
- Changes:
  - Updated compiler codegen for `GroupKind::Atomic` to emit:
    - `OpCode::AtomicStart`
    - inner expression
    - `OpCode::AtomicEnd`
  - Implemented VM runtime handling for atomic opcodes:
    - marks/tracks backtrack-stack depth at atomic-group entry
    - truncates internal backtrack frames on atomic-group success
  - Preserved atomic marker stack state across backtrack restores
  - Added opcode decoding for `AtomicStart`/`AtomicEnd`
  - Added parser-path API tests verifying atomic semantics:
    - `(?>a|ab)c` does not match `abc`
    - `(a|ab)c` matches `abc`
    - `(?>ab|a)c` matches `abc`
- Validation:
  - `cargo test -p rgx-core` passed (59 tests)
- Notes/impact:
  - Delivers actual atomic-group behavior instead of prior scaffolded no-op handling
  - Improves regex semantics parity for atomic-group constructs in parser path
### 2026-02-19 - Added parser-path lookaround syntax support
- Scope: `rgx-core` lexer/parser and compile-path behavior alignment
- Changes:
  - Extended group-token lexing to recognize:
    - positive lookahead `(?=...)`
    - negative lookahead `(?!...)`
    - positive lookbehind `(?<=...)`
    - negative lookbehind `(?<!...)`
    - atomic-group start `(?>...)`
  - Extended parser atom handling to build AST nodes for lookaround tokens and atomic groups
  - Added lexer tests for lookaround tokenization
  - Added parser tests for lookaround and atomic-group parsing
  - Added API tests through `Regex::compile(...)` for parser-path lookahead/lookbehind semantics
- Validation:
  - `cargo test -p rgx-core` passed (57 tests)
- Notes/impact:
  - Closes a parser-vs-AST gap for lookaround support
  - Keeps AST-first path available while parser completeness work continues for other advanced constructs
### 2026-02-19 - Clarified strategic goals: PCRE2 parity + broader code-block languages
- Scope: vision/roadmap/notes alignment for project direction
- Changes:
  - Updated `PROJECT_VISION.md` to explicitly target practical parity with PCRE2 for:
    - feature coverage
    - speed
    - matching accuracy
  - Updated `ROADMAP.md` with explicit PCRE2 parity workstream and multi-language code-block expansion goals
  - Updated `DEVELOPMENT_NOTES.md` to capture this goal clarification and re-prioritize immediate work accordingly
  - Updated `docs/TECHNICAL_DECISIONS.md` with explicit decision records for:
    - PCRE2 parity as north-star target
    - staged multi-language code-block expansion (including Julia)
- Validation:
  - Reviewed cross-doc consistency and wording to ensure goals are clearly marked as targets, not currently shipped guarantees
- Notes/impact:
  - Makes strategic direction explicit for future sessions and contributors
  - Reduces ambiguity between current capabilities and long-term parity goals
### 2026-02-19 - Added live roadmap tracker and layered end-user guide
- Scope: repository documentation structure and usability
- Changes:
  - Added `ROADMAP.md` as a live forward-looking tracker with:
    - maintenance workflow
    - explicit status legend
    - structured `Now` / `Next` / `Later` sections
  - Added `docs/USER_GUIDE.md` as a live end-user guide with layered depth:
    - Level 0 quick start
    - Level 1 practical usage
    - Level 2 advanced AST-first usage
    - Level 3 behavior semantics and implementation-facing details
  - Updated `README.md` documentation map to include both new docs
  - Updated `DEVELOPMENT_NOTES.md` documentation policy to include maintenance intent for both docs
- Validation:
  - Verified documentation links and cross-references for consistency
  - Content reviewed for alignment with current shipped behavior and known parser-path limits
- Notes/impact:
  - Establishes dedicated live planning and user-facing guidance surfaces
  - Improves onboarding for both contributors and end users at different depth levels
### 2026-02-19 - Added AST-first lookbehind support in compiler and VM
- Scope: `rgx-core` VM/compiler assertion semantics (parser-independent path)
- Changes:
  - Implemented AST codegen for lookbehind assertions:
    - `Regex::Lookbehind { positive: true }` -> `OpCode::Lookbehind`
    - `Regex::Lookbehind { positive: false }` -> `OpCode::LookbehindNeg`
  - Implemented VM execution semantics for lookbehind opcodes in:
    - main executor
    - sub-expression executor
  - Added bounded lookbehind assertion evaluation helper that requires the assertion sub-expression to end at current position
  - Extended opcode decoding (`TryFrom<u8>`) for `Lookbehind` and `LookbehindNeg`
  - Removed duplicate lookahead opcode branch in VM executor and bounded character reads by execution context end
  - Added parser-independent public API tests for positive and negative lookbehind behavior
- Validation:
  - `cargo test -p rgx-core` passed (51 tests)
- Notes/impact:
  - Enables AST-first progress on lookbehind assertions without parser syntax dependency
  - Parser syntax for lookbehind remains pending in parser path
### 2026-02-18 - Added built-in 1-based top-level branch number reporting
- Scope: `rgx-core` compiler/engine/public API semantics for top-level alternations
- Changes:
  - Restricted alternative tracking instrumentation to top-level alternation codegen paths
  - Exposed a single user-facing field on match results:
    - `MatchResult.matched_branch_number: Option<usize>`
  - Mapped internal alternative indices to user-facing 1-based branch numbers
  - Added/updated API tests to verify:
    - top-level alternation branch number exposure
    - nested alternation does not override top-level branch selection
- Validation:
  - `cargo test -p rgx-core` passed (49 tests)
- Notes/impact:
  - Removes user friction from 0-based IDs while preserving deterministic branch reporting
  - Keeps branch reporting semantics focused on the top-level alternation contract
### 2026-02-18 - Added AST-first lookahead support in compiler and VM
- Scope: `rgx-core` VM/compiler execution semantics (parser-independent path)
- Changes:
  - Implemented AST codegen for lookahead assertions:
    - `Regex::Lookahead { positive: true }` -> `OpCode::Lookahead`
    - `Regex::Lookahead { positive: false }` -> `OpCode::LookaheadNeg`
  - Implemented VM execution semantics for lookahead opcodes in:
    - main executor
    - sub-expression executor
  - Added non-consuming assertion evaluation helper so lookahead does not mutate parent context
  - Extended opcode decoding (`TryFrom<u8>`) for `Lookahead` and `LookaheadNeg`
  - Added parser-independent public API tests for positive and negative lookahead behavior
- Validation:
  - `cargo test -p rgx-core` passed (46 tests)
- Notes/impact:
  - Enables continued feature progress on advanced assertions without depending on parser readiness
  - Parser syntax for lookarounds remains pending; AST-first workflow is the current delivery path
### 2026-02-18 - Added parser-independent compile path for AST-driven development
- Scope: `rgx-core` compiler/API and feature-gating
- Changes:
  - Added explicit `pgen-parser` feature in `rgx-core/Cargo.toml` to match existing cfg usage and upcoming PGEN integration
  - Added `Compiler::compile_ast(ast)` to compile VM programs directly from AST without parsing
  - Added public parserless entry points:
    - `Regex::from_ast(ast)`
    - `Regex::from_ast_with_mode(ast, mode)`
  - Added tests exercising AST-driven compilation and matching via public API
- Validation:
  - `cargo test -p rgx-core` passed after these changes
- Notes/impact:
  - Unblocks VM/compiler/engine feature work while PGEN parser is still in active design
  - Reduces dependency on parser completeness for core-engine progress
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
