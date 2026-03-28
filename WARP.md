# WARP.md
Repository-local guidance for Warp/Oz when working in `rgx`.
## Read this first
- `README.md` is the canonical repository entry point and onboarding map.
- Use this file only as a supplement for current-state caveats, common validation commands, and repository-specific implementation notes.
## Current implementation snapshot
- The real shipped path is `lexer/parser -> AST -> compiler -> VM -> engine/API` inside `rgx-core`.
- Shipped on the default public path:
  - literals, alternation, anchors including `\A`, `\Z`, `\z`
  - shorthand/custom character classes
  - greedy and lazy quantifiers, including counted ranges
  - named groups, atomic groups, lookahead, and lookbehind
  - top-level branch reporting
- Shipped on the execution-mode / feature-gated path:
  - `(?{lua:...})` code blocks in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `lua` feature is enabled
  - `(?{js:...})` / `(?{javascript:...})` code blocks in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `javascript` feature is enabled
  - `(?{native:...})` code blocks on the Rust API path in `ExecutionMode::Full` after callback registration on the compiled `Regex`
  - `(?{wasm:...})` predicate code blocks on the Rust API path in `ExecutionMode::Safe` or `ExecutionMode::Full` after module registration on the compiled `Regex`
  - Lua/JavaScript/native can surface the last winning-path numeric or replacement value through `MatchResult.code_result`
  - the Rust API can collect winning-path `Numeric(f64)` payloads through `Regex::find_first_numeric_with_code(...)` / `Regex::find_all_numeric_with_code(...)` and consume winning-path `Replacement(String)` payloads through `Regex::replace_first_with_code(...)` / `Regex::replace_all_with_code(...)`; wasm remains predicate-only
- Explicit boundaries still in place:
  - `ExecutionMode::Pure` rejects all code blocks
  - the shipped native/wasm slices are currently Rust-API-only because the CLI does not expose registration
  - the current wasm ABI keeps `module:function` -> exported `() -> i32` and adds `rgx` host imports for current position, full input text, numbered captures, named captures, and host-provided variables set through `Regex::set_variable(...)`; richer non-boolean result handling is still deferred there
  - numeric results are currently surfaced through match metadata plus dedicated numeric helper APIs; the replacement-oriented API layer still consumes only `Replacement(String)`
  - backreferences, recursion, conditionals, and Unicode property classes remain parsed-but-unintegrated
- `pgen-parser` is still a parser-contract validation path backed by fallback behavior, not a truly separate parser backend.
## Useful commands
```bash
cargo test --workspace
cargo test -p rgx-core
cargo test -p rgx-core --features pgen-parser
cargo test -p rgx-core --features lua
cargo test -p rgx-core --features javascript
cargo test -p rgx-core --features wasm
cargo check -p rgx-core --features all-languages
cargo clippy --workspace --all-targets
cargo run --bin rgx-cli -- "cat|dog" "I have a cat"
```
## Files worth checking while working
- `README.md` for repository navigation
- `RUST_CODEBASE_ANALYSIS.md` for the live Rust implementation assessment
- `docs/CAPABILITY_MATRIX.md` for shipped vs parsed-only vs scaffolded status
- `docs/PGEN_ISSUE_TRACKING.md` and `pgen-issues/` for local PGEN parser bug tracking during integration
- `docs/USER_GUIDE.md` for current user-facing semantics
- `DEVELOPMENT_NOTES.md`, `MEMORY.md`, and `CHANGES.md` for continuity and recent changes
## Current priorities
1. Expand code-block support beyond the current first richer-result plus numeric/replacement helper slice, especially wasm richer-result handling.
2. Keep capability/user/state docs truthful as features move from parsed-only to shipped.
3. Replace the fallback-backed `pgen-parser` path with a real parser backend.
4. Improve the performance-validation loop so benchmark claims are continuously grounded.
