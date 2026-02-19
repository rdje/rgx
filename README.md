# rgx
rgx is a Rust regex engine project focused on a high-performance VM backend and a clean compile pipeline.

## Current status
This repository has a working end-to-end path for core regex features:
- lexer -> parser -> AST -> compiler -> VM execution
- `Regex::compile`, `is_match`, `find_first`, `find_all`
- VM-focused tests passing (12 VM tests)
- workspace tests passing at the time of this update

The most mature component is the VM/compiler path in `rgx-core`.

## What works well today
- Literal matching, alternation, anchors, basic character classes
- Core quantifiers (`*`, `+`, `?`, simple `{n,m}` paths)
- Capture group tracking in VM tests
- Group support for capturing, non-capturing `(?:...)`, named groups `(?<name>...)`, and atomic groups `(?>...)` (no-backtracking semantics)
- Parser-independent compilation from AST via public API (`Regex::from_ast`)
- Lookaround support (positive/negative lookahead + lookbehind) via parser syntax and AST
- Built-in top-level alternation branch reporting via `MatchResult.matched_branch_number` (1-based)
- CLI usage for basic regex matching via `rgx-cli`

## Current limitations
- Advanced parser syntax is still partial (conditionals, recursion, and code-block syntax are not yet fully parsed/wired)
- Inline code-block regex syntax like `(?{lua:...})` is not yet available through the current parser path
- A number of advanced opcodes/features are declared but not fully implemented
- JavaScript/WASM integration is scaffolded but not production-ready in the user-facing regex path

## Quick start
```bash
cargo build
cargo test --workspace
cargo test -p rgx-core vm::
cargo run --bin rgx-cli -- "cat|dog" "I have a cat"
```

## Repository structure
- `rgx-core/`: regex engine core (AST, parser, compiler, VM)
- `rgx-cli/`: command-line interface
- `rgx-bench/`: benchmarks (including PCRE2 comparison harness)
- `rgx-wasm/`: WASM crate scaffold
- `docs/`: concise technical docs

## Documentation map
- `CHANGES.md`: living progress ledger (authoritative change history)
- `ROADMAP.md`: live forward-looking roadmap tracker (`Now` / `Next` / `Later`)
- `DEVELOPMENT_NOTES.md`: technical knowledge base and current engineering notes
- `PROJECT_VISION.md`: long-term direction and goals
- `docs/USER_GUIDE.md`: live end-user guide (layered by depth from quick start to gory details)
- `docs/architecture.md`: current architecture and data flow
- `docs/TECHNICAL_DECISIONS.md`: major design decisions and tradeoffs
