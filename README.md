# rgx
`rgx` is a Rust regex engine project focused on a high-performance VM backend with a clean compile pipeline.

## Project objective
Build a robust, high-performance, extensible regex engine that:
- compiles patterns through `lexer -> parser -> AST -> compiler -> VM`,
- targets practical compatibility with mainstream regex behavior (with explicit known gaps),
- supports strict observability/tracing for fast debugging and safe evolution.

## Start here (fast ramp-up)
If you are new to the repo, use this order:
1. `README.md` (this file) for the full navigation map.
2. [`docs/USER_GUIDE.md`](docs/USER_GUIDE.md) for user-facing usage and behavior.
3. [`docs/CAPABILITY_MATRIX.md`](docs/CAPABILITY_MATRIX.md) for shipped vs scaffolded features.
4. [`docs/PCRE2_COMPATIBILITY_MATRIX.md`](docs/PCRE2_COMPATIBILITY_MATRIX.md) for parity status and known gaps.
5. [`DEVELOPMENT_NOTES.md`](DEVELOPMENT_NOTES.md) and [`MEMORY.md`](MEMORY.md) for current technical context and continuity.
6. [`COMMIT.md`](COMMIT.md) before making/committing changes.

## Repository path map (project files)
### Workspace / crates
- [`Cargo.toml`](Cargo.toml) — workspace manifest
- [`rgx-core/`](rgx-core/) — engine core crate
- [`rgx-cli/`](rgx-cli/) — command-line interface
- [`rgx-bench/`](rgx-bench/) — benchmark/parity harnesses
- [`rgx-wasm/`](rgx-wasm/) — wasm crate scaffold
- [`docs/`](docs/) — focused technical/user documentation

### Core engine code paths
- [`rgx-core/src/lib.rs`](rgx-core/src/lib.rs) — public API (`Regex`, compile/match entry points)
- [`rgx-core/src/lexer.rs`](rgx-core/src/lexer.rs) — lexical analysis
- [`rgx-core/src/parser.rs`](rgx-core/src/parser.rs) — recursive-descent parser
- [`rgx-core/src/ast.rs`](rgx-core/src/ast.rs) — AST definitions
- [`rgx-core/src/token.rs`](rgx-core/src/token.rs) — lexer token model + positional types
- [`rgx-core/src/parsing.rs`](rgx-core/src/parsing.rs) — parser abstraction and backend selection
- [`rgx-core/src/compiler.rs`](rgx-core/src/compiler.rs) — AST-to-program compiler boundary
- [`rgx-core/src/vm.rs`](rgx-core/src/vm.rs) — VM bytecode execution engine
- [`rgx-core/src/engine.rs`](rgx-core/src/engine.rs) — runtime dispatch on compiled patterns
- [`rgx-core/src/execution.rs`](rgx-core/src/execution.rs) — execution/callback runtime layer
- [`rgx-core/src/log.rs`](rgx-core/src/log.rs) — structured tracing and verbosity control
- [`rgx-core/src/error.rs`](rgx-core/src/error.rs) — error types
- [`rgx-core/src/pattern.rs`](rgx-core/src/pattern.rs) — compiled pattern model

### CLI / benchmark / parity paths
- [`rgx-cli/src/main.rs`](rgx-cli/src/main.rs) — CLI argument handling and invocation path
- [`rgx-bench/tests/pcre2_parity.rs`](rgx-bench/tests/pcre2_parity.rs) — differential parity checks vs PCRE2

### CI / automation paths
- [`.github/workflows/ci.yml`](.github/workflows/ci.yml) — GitHub Actions workflow
- [`scripts/run-local-ci.sh`](scripts/run-local-ci.sh) — local-first CI entry point (same checks the workflow runs)
- [`scripts/check-ci-paths.sh`](scripts/check-ci-paths.sh) — CI path/tracked-file guardrails

## Documentation index (all `.md` files)
### Root markdown files
- [`README.md`](README.md) — single entry point and navigation hub
- [`CHANGES.md`](CHANGES.md) — authoritative change ledger
- [`COMMIT.md`](COMMIT.md) — commit workflow contract and invariants
- [`DEVELOPMENT_NOTES.md`](DEVELOPMENT_NOTES.md) — technical knowledge base
- [`MEMORY.md`](MEMORY.md) — continuity memory across sessions
- [`PROJECT_VISION.md`](PROJECT_VISION.md) — long-term project direction
- [`ROADMAP.md`](ROADMAP.md) — execution roadmap (`Now`/`Next`/`Later`)
- [`WARP.md`](WARP.md) — Warp-specific repository guidance

### `docs/` markdown files
- [`docs/USER_GUIDE.md`](docs/USER_GUIDE.md) — end-user guide
- [`docs/CAPABILITY_MATRIX.md`](docs/CAPABILITY_MATRIX.md) — feature status matrix
- [`docs/PCRE2_COMPATIBILITY_MATRIX.md`](docs/PCRE2_COMPATIBILITY_MATRIX.md) — PCRE2 parity matrix
- [`docs/PARSER_CONTRACT.md`](docs/PARSER_CONTRACT.md) — parser interoperability contract
- [`docs/TECHNICAL_DECISIONS.md`](docs/TECHNICAL_DECISIONS.md) — architecture/design decisions
- [`docs/architecture.md`](docs/architecture.md) — architecture and data flow
## README maintenance policy
`README.md` is the project’s single entry point and should be updated when it becomes stale, including changes to:
- project objective/scope,
- repository structure or important file paths,
- markdown documentation inventory or onboarding order.

It does **not** need to be updated on every commit—only when those entry-point concerns change.

## Build, test, run
```bash
cargo build
cargo test --workspace
cargo test -p rgx-core vm::
cargo run --bin rgx-cli -- "cat|dog" "I have a cat"
```

Run the same CI checks locally before pushing:
```bash
./scripts/run-local-ci.sh
```

Tracing examples:
```bash
cargo run --bin rgx-cli -- --verbosity low --trace-log "cat|dog" "I have a cat"
cargo run --bin rgx-cli -- --verbosity debug --trace-log "cat|dog" "I have a cat"
cargo run --bin rgx-cli -- --quiet --trace-log "cat|dog" "I have a cat"
```

Legacy CLI aliases:
- `--debug` == `--verbosity high`
- `--trace` == `--verbosity debug`

## Current status snapshot
Most mature path today is the VM/compiler pipeline in `rgx-core`, with public API and CLI integrated.
Some advanced constructs are parsed but intentionally not yet integrated into VM execution (tracked explicitly in docs/matrices above).
