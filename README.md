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
5. [`ROADMAP.md`](ROADMAP.md) and [`RUST_CODEBASE_ANALYSIS.md`](RUST_CODEBASE_ANALYSIS.md) for roadmap intent versus validated Rust implementation status.
6. [`DEVELOPMENT_NOTES.md`](DEVELOPMENT_NOTES.md) and [`MEMORY.md`](MEMORY.md) for current technical context and continuity.
7. [`COMMIT.md`](COMMIT.md) before making/committing changes.

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
- [`rgx-bench/src/lib.rs`](rgx-bench/src/lib.rs) — shared benchmark fixtures and synthetic input generators
- [`rgx-bench/src/bin/trend_capture.rs`](rgx-bench/src/bin/trend_capture.rs) — lightweight benchmark trend capture used by the local validation loop, including mode-scoped latest snapshots, timestamped history, same-mode delta reporting, rolling history summaries, a cross-mode overview with latest shared-pair context, label-paired quick/full summaries, rolling label-pair history with latest-pair callouts, optional capture labels, and explicit label-based baseline selection
- [`rgx-bench/tests/pcre2_parity.rs`](rgx-bench/tests/pcre2_parity.rs) — differential parity checks vs PCRE2

### CI / automation paths
- [`.github/workflows/ci.yml`](.github/workflows/ci.yml) — GitHub Actions workflow
- [`scripts/run-local-ci.sh`](scripts/run-local-ci.sh) — local-first CI entry point for explicit RGX package tests plus the `rgx-core` / `rgx-cli` feature-matrix validation path
- [`scripts/check-ci-paths.sh`](scripts/check-ci-paths.sh) — CI path/tracked-file guardrails
- [`scripts/capture-benchmark-trends.sh`](scripts/capture-benchmark-trends.sh) — quick benchmark trend capture wrapper that writes shared plus mode-scoped summaries, a cross-mode overview, label-paired quick/full summaries, rolling label-pair history, rolling history summaries, git-derived capture labels, forwards explicit baseline selection, and archives under `target/benchmark-trends/`

## Documentation index (all `.md` files)
### Root markdown files
- [`README.md`](README.md) — single entry point and navigation hub
- [`SESSION_BOOTSTRAP.md`](SESSION_BOOTSTRAP.md) — new-session bootstrap instructions for AI/LLM handoff
- [`CHANGES.md`](CHANGES.md) — authoritative change ledger
- [`COMMIT.md`](COMMIT.md) — commit workflow contract and invariants
- [`DEVELOPMENT_NOTES.md`](DEVELOPMENT_NOTES.md) — technical knowledge base
- [`MEMORY.md`](MEMORY.md) — continuity memory across sessions
- [`PGEN_REGEX_PARSER_INTEGRATION_COMPLAINT.md`](PGEN_REGEX_PARSER_INTEGRATION_COMPLAINT.md) — current RGX-side caveat list for the published PGEN regex integration contract
- [`PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md`](PGEN_REGEX_EMBEDDED_CODE_BLOCK_CONTRACT_PROPOSAL.md) — proposed embedded code-block contract shape to forward upstream
- [`PROJECT_VISION.md`](PROJECT_VISION.md) — long-term project direction
- [`ROADMAP.md`](ROADMAP.md) — execution roadmap (`Now`/`Next`/`Later`)
- [`RUST_CODEBASE_ANALYSIS.md`](RUST_CODEBASE_ANALYSIS.md) — live roadmap-grounded Rust workspace analysis
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
cargo test -p rgx-core
cargo test -p rgx-cli
cargo test -p rgx-bench
cargo test -p rgx-wasm
cargo test -p rgx-core vm::
cargo run --bin rgx-cli -- "cat|dog" "I have a cat"
```

Run the same CI checks locally before pushing:
```bash
git submodule update --init --recursive
./scripts/run-local-ci.sh
```
The default RGX build now expects the committed `subs/pgen` submodule carrying the pinned PGEN regex `1.1.2` fixes.
The quick benchmark capture path now keeps shared `latest.md` / `latest.tsv` plus mode-scoped `latest-quick.*` / `latest-full.*`, writes a cross-mode `overview.md` / `overview.tsv` that now also surfaces the newest shared quick/full label pair, writes label-paired quick/full summaries to `profile-pairs.md` / `profile-pairs.tsv`, writes rolling label-pair history to `profile-history.md` / `profile-history.tsv` with latest-pair improvement/regression callouts, writes rolling longitudinal summaries to `history-quick.*` / `history-full.*`, archives runs under `target/benchmark-trends/history/quick/` and `target/benchmark-trends/history/full/`, auto-compares only against the most recent archived capture from the same mode while still allowing an explicit archived baseline via `--compare-against` / `RGX_BENCHMARK_COMPARE_AGAINST`, accepts that explicit baseline as either a unix timestamp or `label:<text>`, and records an optional capture label via `--label` / `RGX_BENCHMARK_TREND_LABEL` that the wrapper defaults from the current git revision (`<short-sha>` or `<short-sha>-dirty`).

That submodule-backed path now covers:
- the default PGEN-backed RGX package test matrix (`rgx-core`, `rgx-cli`, `rgx-bench`, `rgx-wasm`)
- `rgx-core` feature checks for `pgen-parser`, `lua`, `javascript`, `rhai`, and `wasm`
- `rgx-cli` build/test coverage with `--features pgen-parser`
- combined-language build coverage through `--features all-languages`
- and it intentionally avoids the flakier umbrella `cargo test --workspace` path, which has shown intermittent hangs while rebuilding the submodule-backed `pgen` dependency

Fresh clones should use `git clone --recurse-submodules ...` or run `git submodule update --init --recursive` before building.
Because `subs/pgen` is private, hosted GitHub CI may need a `RGX_SUBMODULES_TOKEN` secret if the default `GITHUB_TOKEN` cannot read `rdje/pgen`.

Tracing examples:
```bash
cargo run --bin rgx-cli -- --verbosity low --trace-log "cat|dog" "I have a cat"
cargo run --bin rgx-cli -- --verbosity debug --trace-log "cat|dog" "I have a cat"
cargo run --bin rgx-cli -- --quiet --trace-log "cat|dog" "I have a cat"
```

CLI code-block examples:
```bash
cargo run --bin rgx-cli --features javascript -- --mode safe --var env=prod '(?{js:vars.env === "prod"})' ""
cargo run --bin rgx-cli --features rhai -- --mode safe --show-details 'foo|cat(?{rhai:return 7;})' "cat"
cargo run --bin rgx-cli --features wasm -- --mode safe --wasm-module truthy=./truthy.wasm '(?{wasm:truthy:evaluate})' ""
```

Legacy CLI aliases:
- `--debug` == `--verbosity high`
- `--trace` == `--verbosity debug`

## Current status snapshot
Most mature path today is the VM/compiler pipeline in `rgx-core`, with public API and CLI integrated.
Embedded code-block execution is now available on the public path for Lua, JavaScript, and Rhai code blocks in `ExecutionMode::Safe` / `ExecutionMode::Full` when the corresponding cargo feature is enabled, for registered wasm modules in `ExecutionMode::Safe` / `ExecutionMode::Full` with the `wasm` feature enabled, and for `native` callbacks in `ExecutionMode::Full` through the Rust API after registration on a compiled `Regex`.
Host-provided execution variables can now be set on compiled regexes via `Regex::set_variable(...)` and are snapshotted into Lua, JavaScript, Rhai, native, and wasm code-block evaluation.
The CLI now exposes that same host-variable slice through repeated `--var NAME=VALUE` flags for code-block-enabled patterns, can register named wasm modules through repeatable `--wasm-module NAME=PATH` when built with the `wasm` feature, and can surface top-level branch numbers plus winning-path `code_result` values through `--show-details` without changing the default plain-span output.
Lua, JavaScript, and Rhai now all support either bare expression bodies or explicit `return ...` bodies on the shipped inline-language path.
Lua and JavaScript source bodies now also expose `rgx.emit_numeric(...)` / `rgx.emit_replacement(...)`, while Rhai source bodies expose `emit_numeric(...)` / `emit_replacement(...)`, so statement-style code blocks can surface winning-path numeric or replacement payloads without relying only on direct return values.
Lua, JavaScript, Rhai, native, and wasm code blocks can now also return first-slice richer non-boolean match metadata: `find_first` and `find_all` expose the last winning-path value through `MatchResult.code_result` / `CodeBlockValue`.
The Rust API now also ships first dedicated numeric-result and replacement-oriented helpers on top of that slice: `find_first_numeric_with_code(...)` / `find_all_numeric_with_code(...)` collect winning-path `Numeric(f64)` payloads in match order, while `replace_first_with_code(...)` / `replace_all_with_code(...)` consume winning-path `Replacement(String)` payloads and preserve non-replacement matches unchanged.
The current wasm slice keeps the stable `(?{wasm:module:function})` / exported `() -> i32` predicate surface while optionally exposing `rgx` imports for current position, current match start/end/length metadata, top-level branch number when available, full input text, numbered captures, named captures, host-provided variables, and initial richer-result emission through `emit_numeric(...)` / `emit_replacement(...)`.
Unicode property classes, recursion/subroutine calls, numeric backreferences, and conditionals are now all shipped on the default regex path, including relative conditional-group references like `(?(+1)...)` / `(?(-1)...)`, recursion-condition forms like `(?(R)...)` / `(?(R1)...)` / `(?(R&name)...)`, and single-branch `DEFINE` blocks, plus compile-time rejection for invalid Unicode property names, invalid two-branch `DEFINE` forms, and missing recursive/backreference/conditional capture targets. The CLI now covers host variables, optional richer match-detail rendering, and file-backed wasm module registration for the shipped sandboxed code-block path, while native callback registration still remains Rust-API-only.
The default parser path now drives a real PGEN-backed parser adapter in `rgx-core/src/parsing.rs`, with a one-constant local switch for forcing the recursive-descent reference backend when needed.
Current PGEN regex integration review is intentionally constrained to the published `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` surface and its referenced contract documents.
The current integration dependency is now pinned as the `subs/pgen` submodule at PGEN 1.1.5 commit `11821c49a935bff28bcf72a8e63aacf3c2be3fd7`.
The quick benchmark validation slice is now a little more operationally useful: each local CI capture preserves mode-scoped benchmark history, keeps both shared and per-mode latest snapshots, writes an `overview.*` landing artifact that also exposes the newest shared quick/full label pair, writes rolling history summaries for the captured mode, writes `profile-pairs.*` summaries when quick/full captures share a label, writes `profile-history.*` so those shared-label quick/full pairs can be tracked longitudinally across revisions with latest-pair improvement/regression callouts, tags captures with a git-derived label by default, and summarizes ratio deltas without mixing quick-profile and full-profile baselines.
Read SESSION_BOOTSTRAP.md and start from there.
