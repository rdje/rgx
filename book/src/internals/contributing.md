# Contributing

This chapter is for people who want to work on RGX itself — fix a bug, add a feature, tune a benchmark, improve the book. If you are a user trying to match patterns, you want Parts I through V instead.

Contributing to RGX is deliberately straightforward: clone the repo, run the tests, make a change, run the tests again, open a PR. The rest of this chapter explains the details behind each of those steps.

## Setting up your development environment

**1. Install Rust.** RGX requires Rust 1.85 or newer (per `workspace.package.rust-version`). The easiest way to get a current Rust is via rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
```

**2. Clone the repository with submodules.** RGX depends on PGEN via a git submodule, so a plain `git clone` is not enough:

```bash
git clone --recurse-submodules https://github.com/rdje/rgx.git
cd rgx
```

If you already cloned without `--recurse-submodules`, fetch the submodule after the fact:

```bash
git submodule update --init --recursive
```

Without the submodule, the default build will fail because the PGEN parser crate is missing. The `subs/pgen` directory should contain a PGEN checkout after the command above.

**3. Build the workspace.**

```bash
cargo build
```

First-time builds take a while because PGEN, wasmtime, and the language runtimes all have to compile. Subsequent builds are incremental.

**4. Run the test suite.**

```bash
cargo test -p rgx-core
cargo test -p rgx-cli
cargo test -p rgx-bench
cargo test -p rgx-wasm
```

The four per-crate invocations exist instead of `cargo test --workspace` because the umbrella test path has shown intermittent hangs while rebuilding the submodule-backed PGEN dependency. The per-crate approach is what CI uses and what you should use locally.

If you want to run the same checks CI runs, use the local CI script:

```bash
./scripts/run-local-ci.sh
```

This runs the per-crate tests plus the feature-matrix checks (`pgen-parser`, `lua`, `javascript`, `rhai`, `wasm`, `all-languages`). It is what we run before every commit.

## The commit workflow

RGX has a repository-wide commit contract documented in `COMMIT.md`. Every contributor is expected to read and follow it. The short version:

1. **Make one logical change per commit.** If your change has two unrelated parts, make two commits.
2. **Run the full local CI before committing.** `./scripts/run-local-ci.sh` must pass.
3. **Write a commit message that explains the why, not just the what.** "Fix parser" is bad; "Preserve extended-char-class parity boundary" is good.
4. **Never force-push to `main`.** Use normal commits.
5. **Update the changelog.** `CHANGES.md` is the authoritative ledger — every shipped change needs an entry there.
6. **Update the roadmap if scope changes.** `ROADMAP.md` tracks forward-looking work; if your change affects what is in flight, update it.

The point of the commit contract is that the history is readable months later. A green CI run on every commit means bisecting a regression is always possible.

## Running tests

### The day-to-day loop

Inside the inner dev loop, you almost always run one of these:

```bash
cargo test -p rgx-core                    # full core suite
cargo test -p rgx-core vm::                # just VM tests
cargo test -p rgx-core your_test_name     # single test
cargo test -p rgx-bench                   # bench tests including parity
cargo test -p rgx-cli                     # CLI tests
```

Tests are fast enough that running the full `rgx-core` suite on every save is reasonable. Running the parity suite (`-p rgx-bench`) takes a bit longer because it loads PCRE2 fixtures.

### The feature matrix

RGX has several Cargo features that gate optional functionality:

| Feature | What it enables |
|---------|----------------|
| `pgen-parser` | The PGEN-backed parser path (default in normal builds) |
| `lua` | Lua code blocks via mlua |
| `javascript` | JavaScript code blocks via rquickjs |
| `rhai` | Rhai code blocks |
| `wasm` | WebAssembly modules via wasmtime |
| `all-languages` | Convenience alias for everything above |
| `trace` | Trace logging in the VM (adds overhead, off by default) |

When you change code that touches a feature-gated path, test it with the feature enabled:

```bash
cargo test -p rgx-core --features lua
cargo test -p rgx-core --features javascript
cargo test -p rgx-core --features all-languages
```

The local CI script covers this matrix automatically. If you want to catch feature-specific regressions early, run `./scripts/run-local-ci.sh`.

### The stress and adversarial suites

The stress tests and adversarial tests are in `rgx-core/tests/`. They run by default when you `cargo test -p rgx-core`, but they take noticeably longer than unit tests because they run thousands of iterations.

If you are doing fast iteration, you can skip them:

```bash
cargo test -p rgx-core --lib     # only in-source unit tests, no integration/stress
```

But before pushing, run the full suite at least once.

## Formatting and linting

RGX uses the standard Rust toolchain:

```bash
cargo fmt                        # apply formatting
cargo fmt --check                # check formatting without changing files
cargo clippy --all-targets -- -D warnings   # strict clippy
```

**The clippy gate is strict: zero errors, zero warnings on RGX-owned code.** A PR that introduces clippy warnings will be asked to fix them before landing. This is not aesthetic — clippy catches real bugs, and treating its warnings as non-negotiable is how we stay at zero.

Formatting is enforced via `cargo fmt --check` in CI. Run `cargo fmt` before committing and you will never fail that check.

## The two-track documentation requirement

RGX has **two sets of documentation** that must stay in sync:

1. **The mdBook** in `book/src/` — long-form, narrative, user-facing. This is the book you are reading right now. It is the primary onboarding experience.
2. **The live API docs** generated by `cargo doc` — reference material, kept close to the code. Rustdoc comments on every public item.

The rule is: **every user-facing change must update both**. If you add a new method to `Regex`, you write a rustdoc comment AND you update the relevant chapter in the book. If you change how a feature works, you update both descriptions.

This is extra work. The payoff is that RGX has a coherent story in both places — users who live in `cargo doc` and users who live in the book both get current information. Missing one half is a documentation bug.

Building the book locally:

```bash
mdbook serve book
```

This runs a live-reload server at `http://localhost:3000` so you can see book changes as you type.

Building the API docs:

```bash
cargo doc --open -p rgx-core
```

## Filing PGEN issues

When you find a bug that appears to be in the parser (the AST shape is wrong, an error is misreported, a valid pattern is rejected), the bug is probably in PGEN, not RGX. The workflow is documented in `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` and `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`, but the short version is:

1. **Create a minimal reproducer.** Reduce the pattern to the smallest form that still misbehaves.
2. **Verify it is a parser issue.** Run the same pattern through the legacy recursive-descent path (flip the `USE_PGEN` constant in `parsing.rs`) to see if the problem follows the parser backend. If it does, it is a PGEN bug. If it does not, it is in RGX's compiler/VM.
3. **Create a YAML file in `pgen-issues/`** following the `TEMPLATE.yaml` format. Number it sequentially (`PGEN-RGX-NNNN.yaml`).
4. **Fill in the fields:** reproducer, expected AST, observed AST, severity, workaround (if any), and current status.
5. **Commit the issue file** with a message like `Add PGEN issue PGEN-RGX-NNNN for <short description>`.
6. **File the issue upstream with PGEN.** Link the `pgen-issues/PGEN-RGX-NNNN.yaml` file and the upstream issue number so the two stay connected.

The `pgen-issues/` directory is the single place to look for "is this a known problem?" Keeping it up to date is part of the contract.

## Where to add tests

This comes up a lot in PR reviews: "where does this test belong?" The decision tree:

**Is it testing a single public API call?** Put it in `rgx-core/src/lib.rs` in a `#[cfg(test)]` block, or in the relevant module's tests.

**Is it testing interaction between two or more public API calls?** Put it in `rgx-core/tests/host_integration.rs`. This is where cross-layer and cross-feature tests live.

**Is it trying to break the engine?** Put it in `rgx-core/tests/adversarial.rs`. Pathological patterns, hostile inputs, resource exhaustion — this is the adversarial suite's job.

**Is it verifying an invariant on random inputs?** Put it in `rgx-core/tests/property_tests.rs` as a proptest. "For all X, Y holds."

**Is it sustained load or concurrency?** Put it in `rgx-core/tests/stress_tests.rs`. 100k iterations, thread pools, and resource leak checks live here.

**Is it comparing RGX output to PCRE2 output?** Put it in `rgx-bench/tests/pcre2_parity.rs` as a differential fixture.

**Is it a CLI behavior?** Put it in `rgx-cli/src/main.rs` tests.

**Is it a new fuzz invariant?** Add it to `fuzz/fuzz_targets/` as a new target or extend an existing one.

When in doubt, ask: "what could disprove the claim I am making?" and pick the suite where that test would be strongest.

## What makes a good PR

- **A clear problem statement.** What are you fixing or adding, and why?
- **A passing test suite.** All crates, all features you touched, clippy clean, rustfmt applied.
- **New tests for the new behavior.** If you added a feature, add unit tests and (for user-facing features) adversarial tests. If you fixed a bug, add a regression test.
- **Updated documentation.** Both the rustdoc comments and the book chapter, if the change is user-facing.
- **An entry in `CHANGES.md`.** One line, matching the style of the existing entries.
- **A roadmap update if scope changes.** If your PR closes a roadmap item or introduces a new one, update `ROADMAP.md`.
- **A commit message that explains the why.** See `COMMIT.md`.

PRs that follow this checklist tend to land the same day. PRs that skip parts of it get feedback asking for the missing pieces.

## The friendly part

RGX is built with a particular attitude: **honest about what works, careful about what does not, and warm toward people who want to help**. We take the code seriously but not ourselves. Bug reports are appreciated. Questions are appreciated. PRs that fix typos in the book are appreciated.

If you are unsure whether a change is in scope, open an issue first and we will talk. If you have never contributed to an open-source Rust project before and you are nervous, open a small PR (a typo fix, a test for an undocumented edge case, a doc improvement) and see how it goes. We will meet you where you are.

The engine is complicated. The project does not have to be.

## Further reading

- `README.md` — the repository map. Start here if you are lost.
- `CLAUDE.md` — non-negotiable project rules. Read this if you are an AI assistant working on the repo.
- `COMMIT.md` — the commit contract. Read this before your first commit.
- `CHANGES.md` — the authoritative ledger of shipped changes.
- `ROADMAP.md` — forward-looking planning.
- `docs/BACKLOG.md` — complete inventory of remaining work.
- `docs/TESTING_PHILOSOPHY.md` — the hostile skepticism doctrine.
- `docs/PCRE2_COMPATIBILITY_MATRIX.md` — what's shipped vs what's a gap.
- `docs/HOST_INTEGRATION_ARCHITECTURE.md` — the 6-layer host integration design.
- `docs/TECHNICAL_DECISIONS.md` — recorded engineering decisions and their tradeoffs.
- `PGEN_REGEX_PARSER_INTEGRATION_CONTRACT.md` — the RGX/PGEN boundary.
- `PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md` — the PGEN bug-reporting workflow.
- `pgen-issues/` — the live PGEN bug ledger.

And, of course, the rest of this book. Part I through V explains what RGX does; Part VI (this part) explains how and why. If you are about to work on the engine, Part VI is the part to have open in a tab.

Welcome aboard.
