# CAPABILITY MATRIX
Live shipped-vs-scaffolded feature status for `rgx`.
## Purpose
- Provide a single, concrete view of what is currently user-usable.
- Distinguish parser recognition from compiler/VM/runtime execution support.
- Reduce ambiguity during roadmap prioritization and future parser/backend swaps.
## Status labels
- `shipped`: parser + compiler + VM/user API path works and is covered by tests on the default public path.
- `mode-gated shipped`: the feature is real and validated, but only in explicitly documented execution modes and/or cargo-feature configurations.
- `parsed-only`: parser accepts/builds AST, but compile/runtime path is intentionally blocked with explicit unsupported errors.
- `scaffolded`: declarations/placeholders exist, but the feature is not yet user-ready.
## Core regex features (default user API path)
- Literals, concatenation, alternation: `shipped`
- Anchors (`^`, `$`, `\A`, `\Z`, `\z`) and word boundaries: `shipped`
- Character classes (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`, custom classes): `shipped`
- Unicode property classes (`\p{...}`, `\P{...}`): `shipped`
- Perl extended character classes `(?[...])`: `parsed-only`
  - parser accepts and preserves the extended-class payload explicitly
  - compiler rejects it explicitly until RGX defines downstream set-algebra/runtime policy
- Quantifiers (greedy/lazy/possessive `?`, `*`, `+`, counted ranges): `shipped`
- Groups:
  - capturing/non-capturing/named groups: `shipped`
  - atomic groups `(?>...)`: `shipped`
  - branch-reset groups `(?|...)`: `parsed-only`
    - parser accepts and preserves the branch-reset wrapper explicitly
    - compiler rejects it explicitly until RGX defines capture renumbering/runtime policy
- Lookarounds:
  - lookahead/lookbehind (positive and negative): `shipped`
Representative test anchors:
- `rgx-core/src/lib.rs` parser-path lookaround/atomic/code-boundary tests
- `rgx-core/src/vm.rs` VM execution tests
- `rgx-bench/tests/pcre2_parity.rs` differential parity cases for quantifier, possessive-quantifier, and atomic-group behavior
## Embedded code execution
- `(?{lua:...})`: `mode-gated shipped`
- `(?{js:...})` / `(?{javascript:...})`: `mode-gated shipped`
- `(?{rhai:...})`: `mode-gated shipped`
- `(?{native:...})`: `mode-gated shipped` on the Rust API path in `ExecutionMode::Full` after registration via `Regex::register_native(...)`; the CLI path still has no native-registration surface
- `(?{wasm:...})`: `mode-gated shipped` on the Rust API path in `ExecutionMode::Safe` / `ExecutionMode::Full` after registration via `Regex::register_wasm_module(...)`, and on the CLI path through repeatable `--wasm-module NAME=PATH` when built with the `wasm` feature
Current behavior contract for the shipped slice:
- `ExecutionMode::Pure` rejects all code blocks.
- `ExecutionMode::Safe` accepts only the currently integrated sandboxed backends, with matching cargo features enabled:
  - `lua` requires the `lua` feature
  - `js` / `javascript` requires the `javascript` feature
  - `rhai` requires the `rhai` feature
  - `wasm` requires the `wasm` feature plus a registered module on the compiled `Regex`
  - host-provided execution variables can be set on the compiled `Regex` via `Regex::set_variable(...)`
  - current wasm ABI keeps `module:function`, where `function` must be an exported `() -> i32` predicate (`0` = fail, non-zero = succeed)
  - wasm modules may optionally import context and result helpers from the `rgx` namespace:
    - `position() -> i32`
    - `match_start() -> i32`
    - `match_end() -> i32`
    - `match_length() -> i32`
    - `branch_number() -> i32` (`-1` when the current path is not inside a top-level alternation arm)
    - `text_length() -> i32`
    - `text_read(ptr, offset, len) -> i32`
    - `capture_count() -> i32`
    - `capture_length(index) -> i32` (`-1` when the capture slot is unavailable)
    - `capture_read(index, ptr, offset, len) -> i32` (`-1` when the capture slot is unavailable)
    - `named_capture_count() -> i32`
    - `named_capture_name_length(index) -> i32` (`-1` when the named-capture slot is unavailable)
    - `named_capture_name_read(index, ptr, offset, len) -> i32` (`-1` when the named-capture slot is unavailable)
    - `named_capture_value_length(index) -> i32` (`-1` when the named-capture slot is unavailable)
    - `named_capture_value_read(index, ptr, offset, len) -> i32` (`-1` when the named-capture slot is unavailable)
    - `variable_count() -> i32`
    - `variable_name_length(index) -> i32` (`-1` when the variable slot is unavailable)
    - `variable_name_read(index, ptr, offset, len) -> i32` (`-1` when the variable slot is unavailable)
    - `variable_value_length(index) -> i32` (`-1` when the variable slot is unavailable)
    - `variable_value_read(index, ptr, offset, len) -> i32` (`-1` when the variable slot is unavailable)
    - `emit_numeric(value: f64)`
    - `emit_replacement(ptr, len)`
  - named captures and host-provided variables are exposed to wasm through deterministic lexicographic ordering by name
  - `text_read`, `capture_read`, `named_capture_name_read`, `named_capture_value_read`, `variable_name_read`, `variable_value_read`, and `emit_replacement` require the module to export linear memory as `memory`
  - `emit_numeric` / `emit_replacement` set the winning-path non-boolean payload for the current code block; the last emitted wasm payload wins if a module emits more than once before returning
  - emitted wasm payloads are used only when the exported predicate returns non-zero; a `0` predicate result still fails the current match path
  - malformed wasm call specs, malformed context reads, unknown module names, invalid UTF-8 replacement payloads, and missing/invalid exports or guest-memory interactions fail the current match path at runtime
- `ExecutionMode::Full` accepts the same sandboxed backends plus `native` code blocks on the Rust API path:
  - `native` requires registering a callback on the compiled `Regex`
  - unknown native callback names fail the current match path at runtime
- Code blocks are predicate checkpoints in the VM match path.
- Current overall match text (`arg[0]`), current match start/end/length metadata, top-level branch number when available, numbered captures, named captures, and host-provided variables are exposed to the Lua/JavaScript/Rhai/native execution layer via `ExecContext`, script globals, and `ExecContext` helper methods.
- The shipped CLI now exposes host-provided variables for code-block-enabled patterns through repeated `--var NAME=VALUE` and file-backed wasm module registration through repeatable `--wasm-module NAME=PATH`, while native registration still remains Rust-API-only.
- Current inline/source-body authoring expectations:
  - Lua supports both bare expression bodies and explicit `return ...` bodies
  - JavaScript supports both bare expression bodies and explicit `return ...` bodies
  - Rhai supports both final expression values and explicit `return ...` bodies
- `find_first` / `find_all` now expose `MatchResult.code_result`, which preserves the last winning-path `Numeric` or `Replacement` value from Lua/JavaScript/Rhai/native/wasm code blocks.
- `Regex::find_first_numeric_with_code(...)` / `Regex::find_all_numeric_with_code(...)` now collect winning-path `Numeric(f64)` values in match order and skip non-numeric matches.
- `Regex::replace_first_with_code(...)` / `Regex::replace_all_with_code(...)` now consume winning-path `Replacement(String)` values and copy non-replacement matches through unchanged.
- Wasm currently exposes a smaller import-based context/result slice (position, current match metadata, full input text, numbered captures, named captures, host-provided variables, numeric emission, replacement emission) rather than the fuller Lua/JavaScript/Rhai/native binding surface.
- Code blocks participate in backtracking and may execute multiple times during one overall match search.
Representative test anchors:
- `rgx-core/src/lib.rs` feature-gated Lua/JavaScript/Rhai/native/wasm code-block tests
- `rgx-core/src/execution.rs` backend dispatch logic
## Advanced syntax shipped on the default runtime path
- Recursion / subroutine calls (`(?R)`, `(?1)`, `(?&name)`): `shipped`
- Numeric backreferences (`\1`, `\2`, ...): `shipped`
- Unicode property classes (`\p{...}`, `\P{...}`): `shipped`
- Conditionals (`(?(...)yes|no)` current supported parser condition forms): `shipped`
Behavior contract:
- Recursion executes through guarded runtime subroutine calls against the whole pattern or the referenced capturing group, and compilation fails explicitly when a numbered or named recursion target does not exist.
- Backreferences match the exact bytes captured by the referenced numbered group on the current winning path.
- Unicode property classes resolve through maintained Unicode property/script tables on the default runtime path.
- Conditionals evaluate their test on the current match path and execute only the selected branch.
  - `DEFINE` is treated as always false, so its single branch acts as a definition-only block and runtime behavior falls through as an empty else.
- Compilation fails explicitly when a recursion target, numeric backreference, or conditional numbered/named/relative-group reference refers to a capture group that does not exist in the pattern, or when a Unicode property name is invalid.
Representative test anchors:
- `rgx-core/src/lib.rs` recursion, numeric backreference, Unicode property, and conditional runtime/compile-boundary tests
- `rgx-bench/tests/pcre2_parity.rs` differential parity cases for recursion, numeric backreferences, Unicode property classes, and conditionals
## Conditional runtime coverage (current shipped parser forms)
- Group-exists: `(?(1)yes|no)` (`shipped`)
- Relative-group-exists: `(?(+1)yes|no)`, `(?(-1)yes|no)` (`shipped`)
- Named-group-exists: `(?(<name>)yes|no)`, `(?(name)yes|no)` (`shipped`)
- `DEFINE` conditionals: `(?(DEFINE)yes)` (`shipped`)
  - the `DEFINE` test is treated as always false on the current match path
  - the single branch remains available for numbered and named subroutine definitions while runtime behavior falls through as an empty else
  - `(?(DEFINE)yes|no)` compile-rejects explicitly because RGX follows PCRE2's single-branch rule for `DEFINE`
- Lookaround conditions:
  - `(?(?=expr)yes|no)`
  - `(?(?!expr)yes|no)`
  - `(?(?<=expr)yes|no)`
  - `(?(?<!expr)yes|no)`
  (`shipped`)
## Notes for roadmap usage
- This matrix is implementation-facing and must reflect verified behavior only.
- Aspirational goals (broader code-block language support, richer wasm ABI, richer result semantics, full PCRE2 parity) belong in `ROADMAP.md` and `PROJECT_VISION.md`.
- When a feature changes state, update:
  - this file
  - relevant tests
  - `CHANGES.md`
  - `RUST_CODEBASE_ANALYSIS.md`
  - `docs/USER_GUIDE.md` if user-visible behavior changed
