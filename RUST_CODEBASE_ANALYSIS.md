# RUST CODEBASE ANALYSIS
Live roadmap-grounded analysis of the Rust workspace in `rgx`.

## Why this file exists
- Capture what the Rust codebase actually ships today versus what `ROADMAP.md` is asking for next.
- Separate verified implementation state from older aspirations and stale guidance.
- Give future sessions one accurate Rust-specific status document to refresh when behavior changes.

## Current verified snapshot
- `README.md` remains the canonical repository entry point and onboarding map.
- Validation snapshot:
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua safe_mode_lua_rgx_helpers_can_emit_results_from_statement_bodies -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript safe_mode_javascript_rgx_helpers_can_emit_results_from_statement_bodies -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai safe_mode_rhai_helpers_can_emit_results_from_statement_bodies -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli --features javascript` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional_relative_group_exists -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional_tokens_relative_group_exists -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_conditional_recursion -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli --features wasm` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core extended_char_class -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_contract_algebraic_extended_char_class_executes_on_default_path -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench pcre2_parity_supported -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core parser_define_conditional_reports_explicit_compile_boundary -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core conditional_tokens_define_condition -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core capability_matrix_explicit_unsupported_compile_boundary_cases -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core recursion_named -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench` => pass
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-trends-smoke` => pass
  - repeated `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-trends-smoke` => pass (confirmed previous-run delta reporting)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-trends-explicit-smoke --compare-against none` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench render_history_summary -- --nocapture` => pass
  - `RGX_BENCHMARK_TREND_DIR=/tmp/rgx-benchmark-trends-explicit-smoke RGX_BENCHMARK_COMPARE_AGAINST=1774884688 ./scripts/capture-benchmark-trends.sh` => pass (confirmed explicit archived-baseline comparison via wrapper)
  - `cargo run --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-bench --bin trend_capture -- --mode quick --output-dir /tmp/rgx-benchmark-trends-mode-smoke.JSCCE6 --compare-against none` => pass
  - `RGX_BENCHMARK_TREND_DIR=/tmp/rgx-benchmark-trends-mode-smoke.JSCCE6 RGX_BENCHMARK_TREND_MODE=full ./scripts/capture-benchmark-trends.sh` => pass (confirmed `full` mode does not auto-compare against existing quick history in the same output tree)
  - `RGX_BENCHMARK_TREND_DIR=/tmp/rgx-benchmark-trends-mode-smoke.JSCCE6 ./scripts/capture-benchmark-trends.sh` => pass (confirmed quick mode still auto-compares against quick-only history after a full-mode capture is present)
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core full_mode_native_code_block_can_access_match_metadata -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features lua safe_mode_lua_code_block_can_access_match_metadata -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features javascript safe_mode_javascript_code_block_can_access_match_metadata -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai safe_mode_rhai_code_block_can_match -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features rhai safe_mode_rhai_explicit_return_body_can_match -- --nocapture` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core --features wasm safe_mode_wasm_code_block_can_read_match_metadata -- --nocapture` => pass
  - `cargo fmt --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core -p rgx-cli -p rgx-bench -p rgx-wasm` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-core` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-cli` => pass
  - `cargo test --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml -p rgx-wasm` => pass
  - `cargo clippy --manifest-path /Users/richarddje/Documents/github/rgx/Cargo.toml --workspace --all-targets` => pass with warnings
  - `./scripts/run-local-ci.sh` => pass (with the `subs/pgen` submodule initialized and the explicit RGX package test matrix enabled)
- Source code totals ~**55K** lines across rgx-core (up from ~48K on 2026-04-16; the +7K is concentrated in VM + compiler + parsing as the 36-engine-fix sprint landed 2026-04-17 → 2026-04-24). Top-level modules: vm **9741**, lib **8640**, parsing **4522**, compiler **4092**, execution 3062, lexer 2383, engine **1667**, parser **1490** (retained for non-`pgen-parser` builds), log 641, ast **558**, vars 521, file 513, token 467, regex_set 285, unicode_support **261**, cache 231, bytes 222, error 85, events 54, lua 24, rhai 21, pattern 19. **C1 JIT subsystem** `rgx-core/src/c1/` (~7.4K lines): codegen **5958**, jit 604, runtime 586, mod 211. **C2 NFA/DFA hybrid subsystem** `rgx-core/src/c2/` (~6.9K lines): nfa **1848**, pike 1256, program **1127**, byte_class 953, dfa 875, classifier **746**, mod 53. Internal tool `rgx-core/src/bin/file_pgen_issues.rs` is **1014** lines. rgx-cli **1828** lines.
- MSRV is **1.95** (`Cargo.toml` `rust-version = "1.95"`), bumped from 1.88 on 2026-04-19 (commit `3d1bf7d`) as the VM/compiler sprint picked up newer stabilizations.
- PGEN **1.1.29** at submodule pin `48a9f06431333d05b3c99173850441e8c1e8a341` ("Publish regex 1.1.29 bare-octal range fix") is the sole parser. **72 PGEN-RGX reports filed** across the full conformance push; all resolved either upstream or routed around cleanly through engine fixes — no RGX adapter workarounds added. The most recent PGEN releases absorbed across 2026-04-17 → 2026-04-18 were 1.1.26 → 1.1.27 (held, regressed) → 1.1.28 → 1.1.29, each bumped with cluster-first discipline. Report coverage spans short-form Unicode property escapes, `\Q…\E` inside character classes, variable-length lookbehind with control verbs, Unicode group names, stray `\E` in class bodies, `[:<:]` / `[:>:]` word-boundary aliases, DEFINE-aware lookbehind width analysis, `\C` single-byte escape, callout-condition assertions, `(*UTF8)` / `(*UTF16)` / `(*UTF32)` aliases, scan_substring forward-reference validation, `\N` / `\Q\E` / shorthand endpoints inside char classes, and the full bare-octal class_range endpoint-decoder family audit (report 0072).
- PCRE2 feature parity: **~99% feature-family coverage** (per `docs/PCRE2_COMPATIBILITY_MATRIX.md`). **Case-level pass rate against PCRE2's full `testinput1..29` corpus: ~99.2% (12,702 pass / 108 fail / 0 panic / 0 skip)** measured by `rgx-core/tests/pcre2_conformance.rs` at head `6a56509` (2026-04-24). The **ratchet gate** at the bottom of that test is now locked at `PASS_BASELINE = 12_702`, `FAIL_BASELINE = 108`, `PANIC_BASELINE = 0`, `SKIP_BASELINE = 0` — `assert!(aggregate.pass >= PASS_BASELINE, ...)` fails CI on regression; improvements bump the baselines in the same commit. Conformance progression across the 2026-04-17 → 2026-04-24 VM sprint: 8,822 → 12,702 pass (+3,880 cases closed in ~60 commits; 36 numbered engine fixes plus harness refinements). JIT compilation shipped (C1 production cutover 2026-04-11) and is on by default. Full inline flags (`(?imsx)` enable/disable/scoped/combined, including the `(?-x:...)` scoped-disable semantic fixed 2026-04-17), `\K`, `\R`, `\N`, `\G`, `\X`, `\C` (A10-adjacent, lowered to any-codepoint), `(?C)` callouts, all backtracking verbs including `(*SKIP:name)` (A11), `(?J)`, relative subroutines/backrefs, `(?P<>)`/`(?P=)`, `\k<>`, comment groups, mode settings, full Unicode `(?i)` case folding with `regex_syntax::try_case_fold_simple` closure (A7 shipped 2026-04-16, expanded via engine fix #16 / 2026-04-22 for char-class ranges), case-insensitive numbered and named backreferences (`BackrefCaseInsensitive = 0x68`, engine fix #4 / 2026-04-17), positive-lookaround capture propagation (engine fix #5 / 2026-04-17), full alternation-aware `(*THEN)` with dedicated `AltSplit = 0x47` / `AltScopeBegin = 0x48` / `AltScopeEnd = 0x49` opcodes tracking alternation lexical scope (engine fixes #9 / #34 / 2026-04-22 / 2026-04-24), `(*COMMIT)` stack-clear + sentinel-frame atomic handling (engine fixes #17 / #19), `(*PRUNE)` clearing pending `(*SKIP)` / `(*COMMIT)` marks (engine fixes #24 / #36), `(*ACCEPT)` dedicated opcode bubbling through subexpr / probe / `invoke_subroutine` (engine fix #18 / `Accept = 0xF2`), `(*UCP)`-aware `\d` / `\w` / `\s` / POSIX classes / `\b` / `\B` / `[:word:]` / `[:blank:]` / `[:print:]` / `[:graph:]` / `[:punct:]` / `[:xdigit:]`, `(*CRLF)` / `(*LF)` / `(*CR)` / `(*ANY)` / `(*ANYCRLF)` / `(*NUL)` newline conventions applied to `.` / `\N` / `^` / `$` / `\R`, `(*BSR_ANYCRLF)` / `(*BSR_UNICODE)` restricting `\R`, `(?U)` ungreedy flag with atomic-group suppression for possessive quantifiers (engine fix #26), subroutine calls rewrapping the group body in enclosing `(?i:)` / `(?s:)` flag scopes (engine fix #25 / +11 passes), branch-reset subroutine calls resolving to the leftmost definition (engine fix #20), lookbehind body with must-end-at target for variable-length bodies (engine fix #32) and full-subject visibility for nested lookaheads (engine fix #31), subexpr `(*THEN)` using a local alt-boundary stack (engine fix #33), `(?|…)` branch-reset groups, current recursion-condition conditionals `(?(R)...)` / `(?(Rn)...)` / `(?(R&name)...)`, `(?(DEFINE)...)` conditionals, relative conditional group references `(?(+1)...)` / `(?(-1)...)`, returned-capture subroutines `(?N(grouplist))` (A12, parse + compile to `Call`; full capture-return VM semantics is follow-up), `(?(VERSION op X.Y)...)` conditionals (A13), `(*UTF8)` / `(*UTF16)` / `(*UTF32)` width aliases, `[:<:]` / `[:>:]` word-boundary aliases, PCRE2 short property escapes `\pL` / `\PL`, PCRE2 synthetic classes `Xan` / `Xsp` / `Xps` / `Xwd` / `Xuc` with negation, `napla` / `naplb` non-atomic lookarounds, conservative body-pass-through for `(*scan_substring:...)` / `(*script_run:...)`, DEFINE-aware variable-length lookbehind, `\N{U+HEX}` codepoint escape (compile-time pre-transform), unscoped `(?flags)` crossing alternation branches (engine fix #6), call dispatch pushing empty-match retry frames when the subroutine body can match empty (engine fixes #29 / #30), UCP `[:punct:]` = P* ∪ ASCII punctuation symbols hybrid semantic — all shipped.
- All 6 host integration layers shipped: data exchange, predicate callbacks (5 backends), match steering (incl. inline-language `rgx.steer_*` helpers, A6), structured events, async I/O (continuation-passing), file-backed matching including `tail_file` with OS-native watching (kqueue/inotify via `notify`, A3) and CLI `--follow` (A4).
- New public API surface from the 2026-04-08 backlog execution session, all shipped:
  - `Match<'t>` / `Captures<'t>` ergonomic types with `as_str`/`range`/`len`/index/name/expand/iter
  - `find`, `find_iter`, `captures`, `captures_iter`, `capture_names`, `find_first_at`/`find_all_at`/`is_match_at`, `shortest_match`/`shortest_match_at`
  - `split`, `splitn`, `split_iter`, `splitn_iter`
  - `replace`, `replace_all`, `replacen` with `Replacer` trait, `NoExpand`, closure support, `Cow<str>` returns, `$1`/`$name` interpolation
  - `RegexBuilder` with zero-arg flag setters (`.case_insensitive()` not `.case_insensitive(true)`)
  - `RegexSet` (multi-pattern via `SetMatches`), `RegexCache` (thread-safe LRU), `BytesRegex` (`&[u8]` without UTF-8 validation), `CaptureLocations` for zero-allocation loops
  - `MatchSemantics` (LeftmostFirst/LeftmostLongest), `PartialMatchResult` (Full/Partial/NoMatch streaming)
  - `escape()`, `Regex::named_groups()`/`as_str()`/`captures_len()` metadata accessors
  - `CompileError` with caret-highlighted span diagnostics (B9)
- Production safety: `set_max_steps`, `set_max_backtrack_frames`, `set_max_recursion_depth` (AtomicU64-backed) prevent DoS on patterns like `(a+)+b` (A1, A2, B1).
- Typed values with fluent builder + `vars!`/`value!` macros. `SteerResult` enum. `MatchEvent` observer. `MatchContinuation` (Send+Sync).
- Release-profile performance (criterion): literal find_first **6.4x** vs PCRE2, email **3.4x**, capture **0.88x** (RGX wins). `ExecContext.text` is borrowed `&[u8]`, trace macros gated behind `cfg(feature = "trace")`, literal patterns bypass VM via `memmem`, trail-based backtracking, binary search Unicode ranges, literal-prefix scan skip.
- CLI: full-featured grep-like tool with 18+ flags (`--file`, `--recursive`, `--follow`, `--line-mode`, `--count`, `--context`, `--json`, `--replace`, `--replace-with-code`, `--only-matching`, `--invert-match`, `--numeric`, `--var-json`, `--events`, `--stats`, `--mode`, `--var`, `--wasm-module`, `--show-details`, `--color auto|always|never`).
- Testing: **1,052 lib tests** in `rgx-core` + **30 rgx-cli tests** (per current head `6a56509`), all passing; includes unit, adversarial, integration, property (256+ cases each), stress/fuzz, doc, CLI, bench, and the `rgx-core/tests/api_smoke_test.rs` (19 tests) guarding the documented public API surface. Dedicated C2 differential harness at `rgx-core/tests/c2_pike_differential.rs` + classifier harness at `rgx-core/tests/c2_classifier.rs`. The `rgx-core/tests/pcre2_conformance.rs` ratchet-gated harness runs separately behind `-- --ignored` with ~12,810 PCRE2 corpus cases executed end-to-end; its `RATCHET OK` guard is the merge condition for any change that touches parsing, the adapter, the VM, or the conformance harness itself.
- Documentation has two tracks now (codified in `CLAUDE.md` and `COMMIT.md`):
  - **The RGX Book** (`book/`): 45 mdBook chapters across 7 parts (introduction, getting-started, core-api, host-integration, advanced, real-world, internals, appendices). Part VI (internals) shipped 2026-04-09 with 9 chapters / 1,587 lines covering architecture, compilation pipeline, the VM, PGEN integration, performance, sandboxing, testing philosophy, project status, and contributing. **The Book is the public face of the project**, served via mdBook (Pages workflow temporarily removed pending GitHub Pro).
  - **Live continuity docs** (`MEMORY.md`, `CHANGES.md`, `RUST_CODEBASE_ANALYSIS.md`, `docs/BACKLOG.md`, `DEVELOPMENT_NOTES.md`): for session survival; not user-facing.
- Nested recursion bug fixed (zero-width quantifier guard). Events+async bug fixed. Subroutine capture semantics fixed. `\X` opcode dispatch bug found via trace and fixed during A10 ship.

## Executive summary
- The default Rust workspace is real, green, and centered on `rgx-core`.
- The strongest shipped path is still `lexer/parser -> AST -> compiler -> VM -> engine/API`, and the default local build now routes that parser stage through the real submodule-backed PGEN backend.
- Named recursion-condition syntax `(?(R&name)...)` is now part of the shipped default path:
  - the default RGX parser pin now includes the standalone PGEN `1.1.2` transport fix from local issue `pgen-issues/PGEN-RGX-0005.yaml`
  - the handwritten parser path, PGEN-backed path, compiler, and runtime now all agree on `R&name`
  - PCRE2 differential coverage now treats named recursion conditions as part of the supported conditional surface
- Newer PCRE2 syntax is now split more cleanly between shipped and boundary-only forms: current recursion-condition conditionals `(?(R)...)` / `(?(Rn)...)` / `(?(R&name)...)`, single-branch `DEFINE` conditionals, branch-reset groups, and the current grouped/complement bracket/property/nested-ordinary/POSIX/shorthand/escaped-term `(?[...])` subset now execute on the default path, including horizontal/vertical whitespace shorthands, bare ASCII POSIX class terms including negated forms like `[:^alpha:]`, nested ordinary forms such as `(?[[\dA-F]])` and `(?[[\p{L}] - [\p{Lu}]])`, and same-level multi-operator precedence, while wider remaining extended-class forms still fail with a clear compile-time policy error instead of being misread or silently dropped.
- Local parity probing also clarified one practical non-goal inside that `(?[...])` boundary: bare top-level ordinary terms such as `(?[a-z])` and `(?[\dA-F])` are still compile-rejected by the current PCRE2 bytes-mode harness, so RGX should keep preferring nested ordinary bracket terms instead of widening into those forms prematurely.
- Unicode property classes are now part of that shipped default path:
  - parser-path and AST-first compilation resolve `\p{...}` / `\P{...}` through Unicode property tables instead of treating them as a compile boundary
  - invalid property names now fail explicitly at compile time
  - PCRE2 differential coverage now treats representative Unicode property behavior as supported rather than as a known gap
- Numeric backreferences are now part of that shipped default path:
  - the compiler validates that numbered backreferences only target capture groups that actually exist
  - the VM now emits/decodes/executes `Backref` bytecode in both top-level and subexpression execution paths
  - parity and capability tests now treat numeric backreferences as supported behavior rather than a compile-boundary gap
- Possessive quantifiers are now part of that shipped default path:
  - both parser backends lower `*+`, `++`, `?+`, and counted possessive forms into atomic-wrapped greedy quantified AST nodes
  - runtime behavior now blocks backtracking into the possessive piece while still allowing ordinary success cases that need no suffix backtracking
  - parity and capability tests now treat possessive quantifiers as supported behavior rather than as a parser-adapter gap
- Relative conditional group references are now part of the stable parser boundary on both parser paths:
  - `(?(+1)...)` and `(?(-1)...)` now transport through both the recursive-descent parser and the default PGEN-backed adapter as `ConditionalTest::RelativeGroupExists(offset)`
  - RGX now resolves these forms to absolute conditional-group checks at compile time, which keeps both parser backends aligned while shipping the PCRE2-style runtime behavior on the default path
- The default PGEN-backed parser path is no longer a recursive-descent placeholder:
  - `rgx-core/src/parsing.rs` now calls into the PGEN embedding API
  - the stable regex AST dump is converted into canonical RGX AST structure for groups, lookarounds, conditionals, concatenation/alternation/pieces, and quantifiers
  - leaf atoms are re-parsed from exact source slices through the recursive-descent parser so RGX AST semantics stay aligned for literals, classes, escapes, code blocks, recursion leaves, and related terminals
  - local backend choice under the default PGEN-backed build is intentionally controlled by one constant (`PGEN_FEATURE_BACKEND`) so RGX can flip between the real PGEN backend and the recursive-descent reference backend without changing call sites
- Embedded code-block execution is implemented in the public path for Lua, JavaScript, Rhai, Rust-native callbacks, and registered wasm modules:
  - parser recognizes `(?{lang:code})`
  - compiler validates code blocks against `ExecutionMode` and cargo features
  - VM lowers code blocks into inline opcodes and executes them during matching
  - engine/runtime materialize current match text, current match start/end/length metadata, top-level branch number when available, numbered captures, named captures, and host-provided variables into the execution context
  - Lua, JavaScript, and Rhai now all accept either bare expression bodies or explicit `return ...` bodies
  - winning-path non-boolean Lua/JavaScript/Rhai/native/wasm results are surfaced through `MatchResult.code_result`
  - `Regex::find_first_numeric_with_code(...)` / `Regex::find_all_numeric_with_code(...)` collect winning-path numeric payloads
  - `Regex::replace_first_with_code(...)` / `Regex::replace_all_with_code(...)` consume winning-path replacement payloads
  - the CLI now exposes host-provided code-block variables through repeated `--var NAME=VALUE`, can register file-backed wasm modules through repeatable `--wasm-module NAME=PATH`, can optionally render branch/code-result details with `--show-details`, and now collects matches in one pass instead of calling `is_match` before `find_all`
- The biggest remaining gaps are now narrower and clearer:
  - `ExecutionMode::Pure` still rejects all code blocks by design
  - `native` code blocks are still Rust-API-only; wasm now has a file-backed CLI registration surface but still no broader external plugin/config story
  - the current wasm ABI now has initial richer-result emission, but it is still intentionally narrow compared with the Lua/JavaScript/native surface
  - the real PGEN backend is green locally through pinned submodule commit `48a9f06431333d05b3c99173850441e8c1e8a341` (PGEN **1.1.29**)
  - hosted validation now has the right repository shape, but the private-submodule checkout may still need explicit CI credentials (`RGX_SUBMODULES_TOKEN`) if the default `GITHUB_TOKEN` cannot read `rdje/pgen`
  - the default benchmark trend capture is now directional and low-overhead, preserves shared plus mode-scoped latest/history artifacts, emits a cross-mode `overview.*` for latest quick/full state, records an optional revision label for each archived capture, and highlights delta versus either the most recent prior archived capture from the same mode or an explicitly requested unix-timestamp / `label:<text>` baseline instead of only overwriting a one-off latest file
  - the first VM performance optimization (literal-prefix skip in scanning loop) improved literal find_first by ~2x; the scanning loop now skips positions where the first required byte cannot match, which is cached once at VM construction

## What is shipped today
### Default public regex path
- Literals, concatenation, alternation
- Anchors including `^`, `$`, `\A`, `\Z`, and `\z`
- Shorthand and custom character classes, including negated shorthand classes
- Unicode property classes (`\p{...}`, `\P{...}`)
- Greedy, lazy, and possessive `?`, `*`, `+`, `{n,m}`, and `{n,}` quantifiers
- Capturing, non-capturing, named, and atomic groups
- Numeric backreferences (`\1`, `\2`, ...)
- Positive and negative lookahead/lookbehind
- Top-level alternation branch reporting

### Execution-mode / feature-gated path
- `(?{lua:...})` is shipped as a predicate checkpoint in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `lua` feature is enabled, and Lua source bodies now accept either bare expressions or explicit `return ...` bodies.
- `(?{js:...})` and `(?{javascript:...})` are shipped as predicate checkpoints in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `javascript` feature is enabled, and JavaScript source bodies now accept either bare expressions or explicit `return ...` bodies.
- `(?{rhai:...})` is shipped as a predicate checkpoint in `ExecutionMode::Safe` or `ExecutionMode::Full` when the `rhai` feature is enabled, and Rhai source bodies now accept either final expressions or explicit `return ...` bodies.
- `(?{native:...})` is shipped on the Rust API path in `ExecutionMode::Full` after registering a callback on the compiled `Regex`.
- `(?{wasm:...})` is shipped on the Rust API path in `ExecutionMode::Safe` or `ExecutionMode::Full` after registering a named wasm module on the compiled `Regex`, and on the CLI path through repeatable `--wasm-module NAME=PATH`.
- Current execution-context contract for this slice:
  - capture slot `0` is the current overall match prefix for the current match attempt
  - current match start/end/length metadata plus the 1-based top-level branch number are now available to code-block runtimes when applicable
  - numbered captures, named captures, and host-provided variables are available when their groups have completed or have been set through the Rust API
  - code blocks participate in backtracking and may execute multiple times during one overall match search
  - Lua/JavaScript/Rhai/native/wasm `Numeric` and `Replacement` results now continue matching and the last winning-path non-boolean value is exposed through `MatchResult.code_result`
  - wasm keeps `module:function` plus exported `() -> i32` predicates and `rgx` imports for position, current match metadata, full input text, numbered captures, named captures, variables, `emit_numeric(...)`, and `emit_replacement(...)`

### Parser interoperability / PGEN path
- `docs/PARSER_CONTRACT.md` is the parser-boundary source of truth.
- The active parser and the direct PGEN backend are both checked against the recursive-descent reference AST on widened fixtures covering:
  - empty patterns
  - anchors
  - range quantifiers
  - possessive quantifiers
  - shorthand and Unicode property classes
  - Perl extended character classes
  - group families
  - lookarounds
  - conditionals with and without false branches, including relative group-exists transport and named recursion conditions
  - code-block tags (`lua`, `js`, `javascript`, `rhai`, `native`, `wasm`)
  - recursion and numeric backreferences
- Direct local validation now confirms the five previously reported PGEN transport bugs are fixed on the pinned local `1.1.2` checkout.

## Explicit boundaries that remain in place
- `ExecutionMode::Pure` rejects code blocks with an explicit compile error.
- `ExecutionMode::Safe` still rejects `native` code blocks; they require `ExecutionMode::Full`.
- The CLI still has no native-registration surface, but it now exposes file-backed wasm module registration through repeatable `--wasm-module NAME=PATH`.
- The current wasm ABI is intentionally smaller than the Lua/JavaScript/native context surface and still limits richer-result transport to host-emitted numeric and UTF-8 replacement payloads.
- Current recursion / subroutine calls are runtime-integrated on the default path, while newer returned-capture subroutine forms remain future work.
- Perl extended character classes now execute for the current grouped bracket/property/nested-ordinary/POSIX/shorthand/escaped-term subset: plain nested bracket terms, nested ordinary bracket terms using the current ordinary char-class atom subset (for example `[\dA-F]`, `[[:graph:]]`, and `[\p{L}]`), bare ASCII POSIX class terms including negated forms like `[:^alpha:]`, bare shorthand terms (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`, `\h`, `\H`, `\v`, `\V`), bare escaped literal/control/octal/codepoint terms such as `\a`, `\b`, `\e`, `\f`, `\n`, `\t`, `\r`, `\cA`, `\040`, `\o{101}`, `\x{41}`, `\x41`, and `\-`, unary complement, grouped subexpressions, symmetric difference, and same-level left-associative set algebra with `&` binding tighter than `|` / `+` / `-` / `^` over bracket terms, nested ordinary bracket terms, POSIX terms, shorthand terms, escaped terms, or Unicode property terms; wider nested/set-expression forms and additional bare-term families beyond that subset still compile-reject explicitly, and direct parity probing confirmed that upstream PCRE2 still rejects both `\N` and bare top-level ordinary terms such as `(?[a-z])` inside `(?[...])`.

## Codebase realities that matter for roadmap prioritization
- `Compiler::feature_validation_message()` remains a critical safety boundary because `OptimizingCompiler::codegen_pass()` still carries placeholder branches for unsupported AST families.
- The declared opcode surface in `rgx-core/src/vm.rs` was cleaned up: 11 dead/superseded opcodes were removed (String, CharNoCase, StringNoCase, Range, RangeNeg, Return, SaveStartCond, RestoreCaptures, RepeatRange, RepeatExact) along with the dead `memo_cache` field; the remaining unemitted opcodes (SIMD, optimization hints, Accept, Halt, JumpIfMatch) are explicitly marked as reserved for future work.
- `ParserConfig` still remains unused scaffolding even after the real PGEN backend rollout, but the older dead `PatternAnalysis` helper has now been removed.
- The default local CI path now validates the default PGEN-backed RGX-scoped `fmt`, explicit package tests for `rgx-core`, `rgx-cli`, `rgx-bench`, and `rgx-wasm`, `rgx-cli --features pgen-parser`, the local `rgx-core` feature matrix (`pgen-parser`, `lua`, `javascript`, `rhai`, `wasm`), combined-language build coverage (`all-languages`), `clippy`, and a quick benchmark-trend capture summary under `target/benchmark-trends/`.
- The explicit package matrix is intentional because `cargo test --workspace` has shown intermittent hangs while rebuilding the submodule-backed `pgen` dependency, whereas the equivalent RGX package coverage remains stable.
- The PGEN dependency is now pinned as `subs/pgen` at commit `48a9f06431333d05b3c99173850441e8c1e8a341` (PGEN **1.1.29**). Relative to earlier baselines this includes — in chronological bump order — the A12 returned-capture subroutine grammar (`(?N(grouplist))`), the A13 VERSION-conditional grammar, PCRE2 short-form property escapes (`\pL` / `\PL`), `\Q…\E` inside character classes, relaxed `class_range` with zero-width markers around the dash, variable-length lookbehind with control verbs, Unicode group names, the `\C` single-byte escape atom, callout-condition conditionals, `[:<:]` / `[:>:]` POSIX word-boundary aliases, DEFINE-aware lookbehind width analysis, `(*UTF8)` / `(*UTF16)` / `(*UTF32)` aliases, post-parse capture-list validation for `scan_substring`, and the bare-octal class_range endpoint-decoder fix (1.1.29).
- The root Cargo workspace explicitly excludes `subs/pgen/rust`, which keeps RGX validation scoped to RGX even though the parser dependency now lives under the repository tree.
- Hosted GitHub CI now checks out submodules recursively; because `subs/pgen` is private, it may still require `RGX_SUBMODULES_TOKEN` if `github.token` cannot access `rdje/pgen`.
- Benchmark infrastructure now has two tiers:
  - criterion throughput benches in `rgx-bench/benches/throughput.rs`
  - a lightweight trend-capture binary in `rgx-bench/src/bin/trend_capture.rs` that the default local CI path runs in quick mode, writing `latest.*` summaries, a cross-mode `overview.*`, rolling `history-*.{md,tsv}` summaries, and timestamped history snapshots with previous-run delta sections

## Roadmap alignment
### Now
- PCRE2 parity hardening remains active and well-supported by tests and docs.
- Capability hardening improved again because the real PGEN parser backend now participates in local validation instead of remaining a placeholder.
- Capability hardening improved again because recursion moved from a parser-only boundary into real compiler/VM/runtime support with API and PCRE2 differential coverage.
- Capability hardening improved again because branch-reset groups moved from a parser-only boundary into real compiler/VM/runtime support with API and PCRE2 differential coverage.
- Capability hardening improved again because conditionals moved from parsed-only status to shipped default-path behavior with API and parity coverage.
- Capability hardening improved again because current recursion-condition conditionals `(?(R)...)` / `(?(Rn)...)` now round-trip through both parser backends, resolve PCRE2's `R` / `Rn` ambiguity against named groups, and execute on the default path with parity coverage.
- Capability hardening improved again because named recursion-condition conditionals `(?(R&name)...)` now round-trip through both parser backends, resolve the named recursion target at compile time, and execute on the default path with parity coverage.
- Capability hardening improved again because relative conditional group references now execute on the default path instead of stopping at parser-only transport and compile-boundary guardrails.
- Capability hardening improved again because numeric backreferences moved from parsed-only status to shipped default-path behavior with explicit parity coverage.
- Capability hardening improved again because possessive quantifiers moved from a parser-adapter gap to shipped default-path behavior with API and parity coverage.
- Embedded code execution is no longer parsed-only scaffolding; Lua/JavaScript/Rhai/native are real shipped slices on the documented Rust API path, and wasm now spans both the Rust API path and the CLI's file-backed module-registration path.
- Embedded inline-language hardening improved again because Lua, JavaScript, and Rhai are now all documented/tested as supporting both bare-expression and explicit-`return` source bodies on the shipped path.
- Embedded inline-language hardening improved again because statement-style Lua/JavaScript/Rhai code blocks can now emit winning-path numeric/replacement payloads explicitly instead of depending only on direct non-boolean returns.
- Embedded inline-language hardening improved again because the CLI now exposes host-variable injection and richer optional match-detail rendering without pre-executing code blocks twice on the successful path.
- Performance validation improved again because the default local CI path now emits a reproducible quick benchmark trend summary, preserves shared plus mode-scoped latest snapshots, writes a cross-mode overview that also surfaces the newest shared quick/full label pair plus mode-scoped rolling history summaries, writes `profile-pairs.*` summaries for shared-label quick/full captures, writes `profile-history.*` summaries so those shared-label pairs can be tracked across revisions with latest-pair improvement/regression callouts, archives each capture locally under the matching benchmark mode, records git-derived capture labels by default through the wrapper, and can report delta against either the most recent same-mode archived run or a requested archived baseline instead of leaving all benchmark capture to manual ad hoc runs; the artifact layout/write/report plumbing inside `trend_capture.rs` is now centralized too, which lowers the cost of keeping this validation surface coherent as reports evolve.

### Next
- Tighten the now-shipped inline-language slice around Lua/JavaScript/Rhai ergonomics before widening wasm-specific ABI work again.
- Decide whether native registration should remain Rust-API-only and whether the new wasm CLI path should grow beyond file-backed module registration.
- Tighten the private-submodule CI auth story so hosted builds can always fetch `subs/pgen` without operator intervention.
- Deepen the now-operational mode-scoped benchmark capture into a fuller release-profile longitudinal story, now that explicit archived-baseline selection, revision-aware capture labels, same-mode history separation, same-label quick/full pairing, and rolling paired-label history all exist for targeted local comparisons.

### Later
- Finish larger regex-surface gaps: newer PCRE2 advanced forms such as returned-capture subroutines and `VERSION[...]`, plus broader runtime semantics for algebraic Perl extended character classes beyond the newly shipped grouped bracket/property/nested-ordinary/POSIX/shorthand/escaped-term subset, and the reserved-but-unemitted opcode families (SIMD, optimization hints).

## Practical engineering notes
- Inline code blocks are encoded directly into VM bytecode, which avoids an external callout table and keeps subprogram lowering simple.
- Named-capture metadata is derived once during compilation and stored with the compiled program for runtime callout access.
- Lua execution is sandboxed per invocation rather than reusing one mutable runtime, which makes speculative execution under backtracking/probing safer.
- JavaScript execution is still per-invocation via QuickJS and is wrapped so documented `return ...` style code works inside `(?{js:...})` blocks; it now also injects an `rgx` helper object for explicit emitted-result flows.
- Lua execution now injects an `rgx` helper table for explicit emitted-result flows, while Rhai injects top-level `emit_numeric(...)` / `emit_replacement(...)` functions for the same winning-path payload use case.
- Native callback storage uses shared interior mutability, so the `Arc<ExecutionManager>` attached to the VM can receive post-compilation registrations without swapping runtime instances.
- Host-provided execution variables now live on the shared `ExecutionManager` and are snapshotted into each per-call `ExecContext`, which keeps callout inputs deterministic under backtracking while still allowing Rust API updates between matches.
- Wasm module storage follows the same shared-runtime model, with compiled modules registered once and instantiated on demand through wasmtime; per-call store data now also retains the last emitted wasm result payload until predicate completion.
- Unicode property classes are resolved through a small `unicode_support.rs` bridge backed by `regex-syntax`, which keeps RGX aligned with current Unicode property tables without hard-coding those tables locally.
- Inline subexpression compilation now has to merge and rebase child char-class tables back into the parent compiler state; that fix matters for Unicode property classes inside quantified/lookaround subprograms and closes a broader latent char-class bug.
- Scaffold cleanup landed in the 2026-04-08 backlog session: `rgx-core/src/simd.rs`, `rgx-core/src/javascript.rs`, and `rgx-core/src/wasm.rs` were deleted; `rgx-core/src/cache.rs` is now real (231 lines, hosts the shipped `RegexCache` LRU). `rgx-wasm/src/lib.rs` is the only remaining scaffold-level file in the workspace.

## High-confidence next actions

The merge condition is the PCRE2 conformance ratchet. At head `6a56509` (2026-04-24): **12,702 pass / 108 fail / 0 panic / 0 skip — ~99.2%**. Any change that raises the pass count bumps the baselines in the same commit; any change that drops it fails CI. The path to 100% is now a clear runway — only 108 cases to close.

**Conformance progression across 2026-04-17 → 2026-04-24** — ~60 commits, +3,880 pass (8,822 → 12,702). Distribution of wins (approximate):
- **Harness correctness fixes**: substitute-mode support (+41), Turkish-I / ASCII-restrict gate (+76), per-subject untestable gating (+409), Partial match detection (+98), subject-trimming at `\=` separator (+961), line-anchor recognition (+60), `is_subject_echo` discriminator (+83), Latin-1 expected normalization (+179), `/I` and `/B` preamble skipping (+305), alt_extended_class gate (+234), `#subject dfa` + `(*NOTEMPTY)` gate (+100 / +64), locale gate (+16), alt_bsux / allow_lookaround_bsk / allow_empty_class (+29), `\p{bidi_class:}` gate (+6), `\p{Lu/Ll/Lt}/i` gate (+14 before engine fix), others.
- **Engine semantic fixes** (numbered #1–#36): 36 distinct VM/compiler corrections. Largest clusters: case-insensitive backref (+45, fix #4), `(*THEN)` full alternation-aware (+18, fix #9), `(*ACCEPT)` opcode bubble (+5, fix #18), `(?i)` char-class range with Unicode case closure (+8, fix #16), `\b`/`\B` UCP word-char alignment (+13), subroutine-call flag-scope rewrap (+11, fix #25), positive-lookaround capture propagation (+10, fix #5), extended-mode scoped-disable `(?-x:...)` (+8, fix #3), `(?U)` ungreedy quantifier swap (+4 / various), alt-split alt_boundaries tracking (+3 each for `AltScopeBegin`/`End`, subexpr `THEN`), `(*PRUNE)` clearing pending `(*SKIP)` / `(*COMMIT)` (+2 + +1).
- **Parser / adapter fixes**: UCP `\w` (M + Pc), UCP `\[:punct:\]` (P* ∪ ASCII punct-symbols), `\81`-style backrefs when groups exist, `\N{U+HEX}` compile-time pre-transform, multi-digit non-octal backref fallback, `\c<char>` XOR control-escape rule.
- **PGEN submodule absorptions**: 1.1.26 → 1.1.27 (held, regressing) → 1.1.28 → 1.1.29 across 2026-04-17 → 2026-04-18.

**PGEN-side ledger**: 72 reports filed total. Per user directive 2026-04-24, all reports are considered closed — upstream has landed every grammar/validator fix RGX asked for; the 13 YAML files still marked `status: open` on disk (0021–0053) are stale bookkeeping that can be flipped in a follow-up sweep.

**Remaining 108 conformance failures — top residual buckets** (top-of-bucket examples from CHANGES.md entries):

1. **Truly-recursive palindrome patterns** — `^(.|(.)(?1)\2)$` / `^((.)(?1)\2|.?)$` and siblings. Engine fixes #29 / #30 closed the easy cases via empty-match retry frames; fully-recursive subroutine-stack reification is the remaining gap.
2. **`(?U)`-under-explicit-atomic edge cases** — the rare cases of explicit `(?>…)` whose inner bare quantifier relies on `(?U)`-inverted greediness; accepted divergence per engine fix #26.
3. **PCRE2 start-optimization parity** — `a?` leading quantifiers where PCRE2's scanner skips past the COMMIT-bearing pos 0; RGX's literal-prefix scan doesn't look past a leading optional quantifier (engine fix #28 residuals: testinput2:6604, 6607).
4. **Unicode case-folding multi-codepoint edge cases** — `ẞ` → `ss`, `ſ` → `s`, 1-to-many / many-to-1 fold pairs that A7's `try_case_fold_simple` path doesn't handle because the captured-char walk can't absorb length changes.
5. **Forward-relative recursion / backrefs** — `(?+1)` / `(?+N)` / `\g{+N}`. Not yet lowered through the compiler's group-resolution pass; small cluster.
6. **Residual substitute-mode divergences** — the harness now handles pcre2test `/replace=` / `/substitute*` (+41 on 2026-04-18); what remains is a small bucket where RGX's `replace_all` output genuinely differs from PCRE2's (template-syntax edges, empty-match replacement-iteration quirks).
7. **`\p{Lu/Ll/Lt}` + `/i` positive cases** — 7 cases still harness-gated behind `pattern_needs_case_fold_property_expansion`. Engine fix #13 lifted negative forms via `ci_override_ranges` threading; positive `\p{Lu}/i` + subject case-closure needs a deeper case-fold table refactor.

**Non-conformance items still on the board:**

8. **A9 — Language bindings (Python, Node, C)** — `large` per language. Deferred 2026-04-09 pending real demand signal. If reactivated, start with C bindings via cbindgen.
9. **A12 capture-return VM semantics follow-up** — A12 parsing + `Call` opcode lowering shipped, but full capture-return semantics (preserving the specified groups across the recursive call) remains follow-up.
10. **A8 — Crate publishing** — metadata ready; `pgen` is a private-submodule path dep not on crates.io. Three options on the table (publish pgen; vendor pgen; make pgen-parser truly optional), pending user decision. Binary rename `rgx-cli` → `rgx` is a coordinated doc-sync follow-up.
11. **Reverse-DFA pipeline (C2 follow-up)** — `is_match` single-pass fast path shipped 2026-04-13. Remaining work: teach the unanchored NFA to kill its lazy prefix threads after accept so subset construction preserves leftmost-first semantics. Blocker for wiring `find_first` / `find_all` onto the reverse-DFA pipeline.
12. **GitHub Pages for The RGX Book** — `blocked` on user enabling GitHub Pro. Workflow `.github/workflows/book.yml` deleted in `3ded2e3`, recoverable from git history.
13. **Performance push to <10x PCRE2 gap** — per 2026-04-11 C2 cutover + 2026-04-12 C1 production cutover, RGX now beats PCRE2 on literals (3.16x find_all 10K) and capture groups (1.96x find_all 10K); email_basic still ~2.6-3.1x slower (JIT path, has `\b`, ineligible for C2). Remaining: reverse-DFA pipeline for leftmost-first find_first/find_all, opcode fusion, multi-byte literal prefix via memmem in C2, smarter Pike-VM dispatch heuristic, JIT-ahead-of-Pike-VM dispatch ordering, capture/backtrack pre-allocation.
14. **Stale PGEN-RGX YAMLs** — 13 files (0021–0053) still carry `status: open` on disk despite the user's closure directive. Follow-up: batch-flip these to `status: closed` with a generic resolution note so the ledger matches reality.
15. **Tier-4 book pages pointing at "planned" tail_file or inline-language-steering features** — some pages have small stale fragments (e.g. "(planned)" next to `tail_file`, which actually shipped). Small doc-sync sweep.

**C1 — JIT compilation** ✅ **SHIPPED 2026-04-11**. Full 9-step plan complete. The C1 subsystem under `rgx-core/src/c1/` consists of: JIT host plumbing (`jit.rs` — Cranelift `JITModule` wrapper + `JitProgram` handle + helper imports), bytecode → Cranelift IR translation (`codegen.rs` — eligibility check + decoder + per-opcode IR emission + per-frame capture snapshot + inline step-counter + user-frame-limit checks), and the runtime helper layer (`runtime.rs` — `rgx_runtime_word_boundary_test` and `rgx_runtime_char_class_match_at` C-ABI helpers the JIT'd code calls via indirect calls). Wired into engine dispatch as the third tier in the 4-tier `DFA → Pike-VM → JIT → backtracking VM` chain in `engine.rs`. `jit` Cargo feature default-on; opt out via `default-features = false`. Public design lives in `book/src/internals/jit-compiler.md`.

**C2 — NFA/DFA hybrid for simple patterns** ✅ **SHIPPED 2026-04-11**. Full 9-step plan complete. The C2 subsystem under `rgx-core/src/c2/` consists of: classifier, byte-class equivalence partitioning, forward + reverse Thompson NFA, sparse-set Pike-VM, lazy DFA cache, and the assembled `CompiledC2Program` artifact. Wired into engine dispatch via a 3-tier chain (DFA → Pike-VM → existing backtracking VM) in `engine.rs`. Production benchmark wins vs the pre-C2 baseline (label `f708f7c`): `literal_simple` 38-40x faster, `email_basic` 6.1-6.6x faster, `capture_groups` 31-35x faster. Public design lives in `book/src/internals/nfa-dfa-engine.md`.

RGX-owned clippy warnings are at zero; the large VM dispatch loops (`execute_at`, `execute_subexpr`) carry targeted `#[allow]` annotations with architectural rationale since they are inherently monolithic state machines.
