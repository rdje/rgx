# Testing Philosophy

RGX's testing philosophy is short enough to fit on a sticky note:

> **The engine is guilty until proven innocent at every corner.**

The test suite is written in the spirit of **hostile skepticism**. Every test exists to prove a claim wrong, not to confirm that the happy path works. If a test always passes, it is not testing hard enough. The goal is to shake the engine until something breaks — then fix what broke, add a regression test, and shake harder.

This is not the style of testing you find in most libraries. It is the style you find in compilers, databases, and cryptography primitives: places where a wrong answer is worse than a crash, and where the ways to be wrong are so numerous that only systematic paranoia will cover them.

## Why this matters

A regex engine has more ways to be wrong than almost any other piece of code of comparable size.

- The input space is infinite. Any string is a potential input.
- The pattern space is infinite. Any pattern is a potential program.
- The interaction between patterns and inputs is combinatorial.
- Backtracking means state has to be restored correctly at every branch — off-by-ones are silent and nearly invisible.
- The feature interactions multiply: captures + lookaheads + backrefs + conditionals + code blocks + steering + async + events, all in the same dispatch loop.
- Performance and correctness are entangled — a fast path bug and a slow path bug look identical to users but require different tests to catch.

The only way to stay sane in this space is to assume the engine is broken until proven otherwise, and to build the test suite as an adversary instead of a cheerleader.

## The test taxonomy

RGX's test suite is layered. Each layer catches a different class of bug, and you need all of them because no single layer is enough.

| Suite | File | Purpose | Approximate count |
|-------|------|---------|-------------------|
| Unit tests | `rgx-core/src/lib.rs` + per-module `#[cfg(test)]` | Core API behavior, single features | ~480 |
| Integration | `rgx-core/tests/host_integration.rs` | All 6 host layers, cross-layer combinations | ~55 |
| Adversarial | `rgx-core/tests/adversarial.rs` | Try to break it. Pathological patterns, abuse. | ~44 |
| Property-based | `rgx-core/tests/property_tests.rs` | Random inputs, invariants, 256+ cases each | 11 properties |
| Stress / soak | `rgx-core/tests/stress_tests.rs` | 100k inputs, concurrency, sustained load | ~21 |
| API smoke | `rgx-core/tests/api_smoke_test.rs` | Public API doesn't regress | a handful |
| PCRE2 parity | `rgx-bench/tests/pcre2_parity.rs` | Differential accuracy vs PCRE2 | ~250 |
| Fuzz targets | `fuzz/fuzz_targets/` | cargo-fuzz continuous fuzzing | 4 targets |
| CLI | `rgx-cli/src/main.rs` tests | Command-line behavior | ~10 |

The approximate total is **~633 tests** on the default test paths, not counting property-test cases (each property generates 256+ inputs per run) or fuzz iterations (which run continuously when enabled).

The point is not the number. It is the **shape** — every layer does something the others cannot.

## Layer 1: Unit tests

Unit tests live next to the code they test. Every module has a `#[cfg(test)] mod tests` block with focused tests for the specific functions in that module. The lexer has lexer tests, the compiler has compiler tests, the VM has VM tests.

Unit tests are fast — the full unit suite runs in a few seconds — and they are the first line of defense. When you change a function, you run the unit tests for its module and you know within seconds whether you broke something.

Unit tests are also where we test **single features in isolation**. Does `(?i)` fold ASCII correctly? Does `\d` match all ten digit characters? Does `[a-z]` reject uppercase? Each of these is a small, sharp test. Thousands of small sharp tests add up to meaningful coverage.

## Layer 2: Integration tests

Integration tests live in `rgx-core/tests/` and are compiled against the public API of the crate. They cannot see private internals, which is exactly the point: they test what users will actually experience.

The most important integration file is `host_integration.rs`, which covers all six host integration layers and their cross-layer combinations. These tests exist because the host layers are the place where RGX's differentiation lives — and they are also the most complex interactions in the codebase. Variables + callbacks + steering + events + async + file scanning, in every combination, with realistic patterns.

If an integration test fails, the bug is probably not in one module but in the interaction between two. The integration layer is where we catch "I forgot that the event observer has to fire **before** the async suspension, not after."

## Layer 3: Adversarial tests

Adversarial tests are the purest expression of the "guilty until proven innocent" doctrine. They live in `adversarial.rs` and they are written by an imaginary attacker trying to break the engine.

The categories of attack:

- **Pathological backtracking.** `(a+)+b` on `"aaaaaaaaaaaaaaaaaa"`. `(a|a|a)*b` with no `b`. Catastrophic nesting.
- **Resource exhaustion.** Patterns that allocate lots of capture groups. Patterns with huge counted quantifiers. Deeply nested groups.
- **Hostile inputs.** UTF-8 boundary attacks. Zero-length matches at every position. Empty input, single-byte input, maximum-codepoint input.
- **Feature abuse.** Recursion 100 levels deep. Alternations with 200 branches. Callbacks that return garbage.
- **Interaction attacks.** `(*COMMIT)` inside `(?=...)` inside a recursive call with a backreference.

Adversarial tests are not supposed to be pretty. They are supposed to be mean. The rule is: if you can think of a way to make the engine behave badly, write a test that tries it. When the test finds a real bug, you now have a regression test forever.

## Layer 4: Property tests

Property tests use `proptest` to generate random inputs and verify that **invariants** hold. The invariant style is different from example-based tests: instead of asserting "input X produces output Y," you assert "for all inputs, this property is true."

RGX has 11 property tests, each of which runs 256+ randomly generated cases per test run. Some examples of the invariants we test:

- **`find_first` is never worse than the first result of `find_all`.** If `find_all` returns a non-empty list, `find_first` must return the same first element.
- **`is_match` agrees with `find_first.is_some()`.** Two code paths, same answer.
- **Captures are always within the overall match range.** A capture cannot extend past the match.
- **`replace_all` is idempotent on patterns that don't match their own replacement.** Running it twice produces the same output as running it once.
- **Escaping round-trips.** `regex::escape(s)` followed by compilation and matching against `s` always succeeds.

Property tests catch bugs that no human would think to test. "This pattern happens to generate 13 characters of Unicode followed by a newline at position 40," — no one writes that by hand, but proptest will generate it eventually, and when the engine misbehaves, proptest shrinks the input to the minimal failing case automatically.

Proptest is one of the best tools in the suite. It has found real bugs that unit tests and adversarial tests missed.

## Layer 5: Stress tests

Stress tests are about sustained load and concurrency. They live in `stress_tests.rs` and they look nothing like the other suites.

A typical stress test compiles a pattern, then runs it against **100,000 randomly generated inputs** in a loop and asserts that nothing crashes and no invariants are violated. Another stress test spawns a dozen threads, each matching the same compiled regex against different inputs simultaneously, to verify that `Regex` is actually `Send + Sync` in practice (not just by type signature).

Stress tests catch:

- **Memory leaks.** If the capture vector or backtrack stack is not cleared between matches, 100k iterations will exhaust memory.
- **Non-determinism.** A race condition under concurrency will show up here, not in unit tests.
- **Long-tail latency.** A rare backtrack that takes 100ms will appear in 100k iterations and nowhere else.
- **State pollution between matches.** If a global somewhere is not reset, iteration N will fail because of iteration N-1.

Stress tests are slower than unit tests — a full stress run takes minutes — so they are not the inner-loop test. But they run on every CI build and they catch a class of bugs that nothing else can.

## Layer 6: Differential parity vs PCRE2

The parity suite lives in `rgx-bench/tests/pcre2_parity.rs`. It is roughly 250 pattern+input fixtures, each of which is run through **both RGX and PCRE2**, and the results are compared.

This is the ultimate correctness test. PCRE2 is the target of the compatibility effort, so any divergence between RGX and PCRE2 is either a bug in RGX or a deliberately documented exception. The parity suite catches drift the moment it happens.

The fixtures cover the feature matrix: literals, classes, quantifiers, alternation, captures, backreferences, lookarounds, conditionals, subroutines, anchors, boundaries, Unicode properties, POSIX classes, extended char classes, all of it. When a new feature ships, the first thing that happens is adding parity fixtures for it.

The parity suite is how we claim ~98% PCRE2 compatibility without flinching: it is not a theoretical claim, it is a measurable count of fixtures that pass. When a new PCRE2 version adds a syntax form, adding fixtures is how we track whether we have caught up.

## Layer 7: Fuzz targets

RGX has four `cargo-fuzz` targets under `fuzz/fuzz_targets/`:

- **`fuzz_compile`** — fuzz the compiler with random pattern strings. Catches panics, assertion failures, and stack overflows during parsing/compilation.
- **`fuzz_match`** — fuzz the match engine with random pattern+input pairs. Catches panics during execution.
- **`fuzz_replace`** — fuzz the replacement API with random patterns, inputs, and replacement strings.
- **`fuzz_roundtrip`** — fuzz that compile-match-compare invariants hold under random inputs.

Fuzzing is continuous: when run, these targets generate millions of inputs per hour and automatically minimize any crashes they find to the smallest reproducing input. Fuzzing catches the bugs that no human would ever write — patterns that exercise specific byte sequences, edge cases in UTF-8 decoding, unusual escape sequences that no test fixture thought to include.

This is backlog item **C3** and it is shipped.

## Layer 8: API smoke tests

The API smoke test file is the shortest of the bunch. It compiles a handful of patterns, runs each one through the main public entry points (`compile`, `is_match`, `find_first`, `find_all`, `captures`, `replace`, `replace_all`, `split`), and asserts basic sanity.

The point is not coverage. The point is **regression detection**. If someone accidentally breaks the public API surface — changes a return type, removes a method, introduces a required parameter — the smoke test catches it immediately. It is a canary, not a detailed check.

## The "claims to prove" approach

One idea threads through every layer of the test suite: **RGX makes claims, and each claim must have a test that could disprove it.**

Examples of claims and the tests that challenge them:

- **"Trail-based backtracking restores state correctly."** Test: run a pattern with 50 backtracks through a callback that reads captures on every invocation. Verify captures are correct at each step.
- **"Zero overhead when no event observer is registered."** Test: benchmark with and without observer registration. The numbers must match within noise.
- **"Continuations are Send + Sync."** Test: actually send a continuation through a channel, resume it on a different thread pool, under load. Not just `assert_send_sync::<T>()`.
- **"Prefix filter skips impossible positions."** Test: create a pattern where the filter should skip 999 positions and the match is at position 999. Verify it finds the match.
- **"memmem fast path matches the VM path."** Test: compile a literal pattern, run it through both paths, compare results on 10,000 inputs. Any divergence is a bug.
- **"Events don't affect match behavior."** Test: run the same match with and without an observer. Compare results.
- **"Async suspension preserves full VM state."** Test: suspend mid-match, resume, verify captures are identical to the non-suspended run.

Every one of these claims has a test. Writing them down this way is a forcing function: if you cannot articulate the claim as "here is what would disprove it," you do not understand the claim well enough to test it.

## Known untested gaps

Hostile skepticism requires honesty about what is **not** tested. Here are combinations we know could hide bugs but do not have dedicated coverage yet:

- **Recursion + steering.** What happens if a callback steers inside a recursive subroutine?
- **Events during async suspension.** Do events fire before suspension? After resume? Both?
- **File scanning + async callbacks.** Can you suspend during a file scan?
- **Variable mutation between `find_all` matches.** Does the second match see updated variables?
- **Capture groups across `\K` boundaries.** Are captures correct when match start is reset?
- **Backtracking verbs inside lookaheads.** Does `(*COMMIT)` inside `(?=...)` affect the outer match?
- **Steering + zero-width matches.** What if `Accept` fires at a zero-width position in `find_all`?
- **Deep recursion + trail backtracking.** Does the trail correctly restore captures after deep recursive calls?
- **Concurrent `set_variable` + matching.** Is it safe to update variables from one thread while another thread is matching?

These are tracked explicitly because writing them down makes them actionable. When we ship a test for one of these, it moves from this list into the stress or integration suite.

## The process

1. **Every bug fix ships with a regression test** that would have caught the bug.
2. **Every new feature ships with adversarial tests** that try to break it in combination with existing features.
3. **Testing is never done.** The test suite grows with the engine. There is no "enough tests."
4. **Property tests generate inputs no human would think of.** Every invariant the engine claims should be a property test eventually.
5. **Stress tests run thousands of iterations** to catch non-determinism and resource leaks.
6. **Adversarial tests simulate hostile users** who push every feature to its limits.
7. **Parity tests catch PCRE2 drift** the moment a change affects a fixture.
8. **Fuzz targets run continuously** to find the bugs nothing else will.

The whole suite runs via `./scripts/run-local-ci.sh` before every push. The same suite runs in GitHub Actions CI on every PR. A change that breaks any of it does not land.

## Enforcing the gate: the receipt guard

"A change that breaks the gate does not land" is only true if the gate is actually run, actually green, and *known* to be green for the exact content committed. In May 2026 all of those failed at once: a `cargo test -p rgx-core` stack-overflow regression rode along for six weeks because (1) the mandatory gate was being satisfied with *targeted* / `--lib` runs instead of the full suite; (2) a pipeline exit-masking trap — `cargo test … | tail` returns the *filter's* exit status (0), hiding the cargo failure / `SIGABRT` underneath; (3) hosted CI couldn't build at all (a toolchain-vs-MSRV pin mismatch); and (4) the canonical gate runner `run-local-ci.sh` had itself been red on entirely benign source (an absolute-path audit whose Windows-drive heuristic false-matched the ubiquitous `:\n` in Rust format strings), so contributors had quietly stopped running it.

The structural fix is a **gate receipt**:

- `run-local-ci.sh` runs every step under `set -euo pipefail` with no exit-masking. Only if *all* of them pass does it write a receipt — a deterministic hash of the gate-affecting worktree content (Rust / Cargo / CI / scripts; docs and the read-only `subs/pgen` submodule are excluded so post-gate documentation sync doesn't false-invalidate it) — into `.git/`.
- A tracked `pre-commit` hook (`scripts/git-hooks/pre-commit`, activated once per clone via `./scripts/setup-hooks.sh`) recomputes that identity at commit time and **refuses the commit** unless a fresh receipt matches exactly. Documentation-only commits pass without a receipt — they cannot change a Rust gate result.

The only way past a red gate is the explicit, loud `git commit --no-verify`, which the commit workflow requires to be justified in the commit message. "Self-reported green while actually red" is no longer something a tired session or a masked pipe can produce: the receipt only exists if the full gate genuinely passed, and the commit is physically blocked without it.

The hardening proved itself the moment it shipped: the first full-gate run's *background notification said "exit code 0"* — the masking trap exactly — while the gate had in fact failed at the path audit and **no receipt was written**. Verifying by receipt rather than by pipeline exit caught the truth immediately. The lesson is general: **never conclude "pass" from filtered output; assert the real status.**

## Verified book examples

Every code example in this book is something a reader will copy, paste, and expect to work. An example that does not compile is a broken promise — and "nothing in this book is a promise that cannot be verified" has to apply to the book's *own* code too.

`mdbook test` can't do this for an external crate (it invokes `rustdoc` without `--extern rgx_core=…`, so every `use rgx_core::…` fails to resolve). Instead, chapters are pulled into `rgx-core` itself as doctests — `rgx-core/src/book_doctests.rs` does `#[cfg(doctest)] #[doc = include_str!("…/chapter.md")]` per chapter. **`cargo test -p rgx-core` then compiles and runs every example as a real crate doctest**, with `rgx_core` and its whole dependency graph resolving natively — exactly the experience of a user who pasted the snippet into a project depending on `rgx-core`. No new CI: this rides the existing mandatory gate (receipt-guarded, ratcheted), so a broken example *cannot* land.

And it *cannot ship either*: the `Deploy Book` workflow runs `cargo test -p rgx-core --doc` **before** `mdbook build` / the Pages upload, and the deploy job `needs:` it — so if any wired example fails to compile or run, the book is **not published**. A broken example can neither be committed nor deployed.

The annotation contract keeps every visible snippet both clean and verified:

- ```` ```rust ```` — compiled **and run** (pure-API examples).
- ```` ```rust,no_run ```` — compiled & type-checked, not executed (servers, file/network IO, long-running, feature-gated). This still proves *it will compile for the user* — the common copy-paste failure mode is an API that drifted, which `no_run` catches.
- ```` ```rust,ignore ```` — last resort only, each with a one-line justification; not verified.
- Hidden `# ` lines carry imports and `fn main` so the *visible* snippet stays exactly what the reader pastes, while the compiled unit is complete.
- Genuinely non-Rust illustrative blocks are fenced `text`, not `rust` — honest about what is and isn't runnable.

Coverage is a **ratchet**, the same idiom as the PCRE2 conformance gate: chapters are wired in `book_doctests.rs` incrementally (highest-traffic first), `book/.examples-verified-chapters` records the count, and `scripts/check-book-examples.sh` (run by `run-local-ci.sh`) fails if it ever shrinks — you cannot re-`ignore` a block or un-wire a chapter to dodge the gate; the verified set only grows. The HTTP Router chapter was the first wired (the example the gap was first reported against); the rest follow chapter by chapter.

## What this buys us

The payoff is confidence. When RGX claims "98% PCRE2 parity," that claim is backed by 250 parity fixtures. When it claims "correct under backtracking," that claim is backed by stress tests with thousands of iterations. When it claims "zero-overhead events," that claim is backed by benchmarks.

Nothing in this book is a promise that cannot be verified. If you doubt a claim, find the corresponding test — it is in the suite somewhere, and if it is not, that is a documentation bug and we will fix it.

## Next: the project

The engine is tested. The code is shipped. What is the state of the project? Head to [Project Status & Roadmap](./project-status.md) next.
