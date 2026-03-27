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
- Unicode property classes (`\p{...}`, `\P{...}`): `parsed-only`
- Quantifiers (greedy/lazy `?`, `*`, `+`, counted ranges): `shipped`
- Groups:
  - capturing/non-capturing/named groups: `shipped`
  - atomic groups `(?>...)`: `shipped`
- Lookarounds:
  - lookahead/lookbehind (positive and negative): `shipped`
Representative test anchors:
- `rgx-core/src/lib.rs` parser-path lookaround/atomic/code-boundary tests
- `rgx-core/src/vm.rs` VM execution tests
## Embedded code execution
- `(?{lua:...})`: `mode-gated shipped`
- `(?{js:...})` / `(?{javascript:...})`: `mode-gated shipped`
- `(?{native:...})`: `mode-gated shipped` on the Rust API path in `ExecutionMode::Full` after registration via `Regex::register_native(...)`; the CLI path still has no native-registration surface
- `(?{wasm:...})`: `mode-gated shipped` on the Rust API path in `ExecutionMode::Safe` / `ExecutionMode::Full` after registration via `Regex::register_wasm_module(...)`; the CLI path still has no wasm-registration surface
Current behavior contract for the shipped slice:
- `ExecutionMode::Pure` rejects all code blocks.
- `ExecutionMode::Safe` accepts only the currently integrated sandboxed backends, with matching cargo features enabled:
  - `lua` requires the `lua` feature
  - `js` / `javascript` requires the `javascript` feature
  - `wasm` requires the `wasm` feature plus a registered module on the compiled `Regex`
  - initial wasm ABI is `module:function`, where `function` must be an exported `() -> i32` predicate (`0` = fail, non-zero = succeed)
  - malformed wasm call specs, unknown module names, and missing/invalid exports fail the current match path at runtime
- `ExecutionMode::Full` accepts the same sandboxed backends plus `native` code blocks on the Rust API path:
  - `native` requires registering a callback on the compiled `Regex`
  - unknown native callback names fail the current match path at runtime
- Code blocks are predicate checkpoints in the VM match path.
- Current overall match text (`arg[0]`), numbered captures, and named captures are exposed to the Lua/JavaScript/native execution layer via `ExecContext`.
- The initial wasm slice currently uses the smaller registered-module ABI above rather than exposing `ExecContext` to wasm modules.
- Code blocks participate in backtracking and may execute multiple times during one overall match search.
- Numeric and replacement return values are rejected in match mode for now.
Representative test anchors:
- `rgx-core/src/lib.rs` feature-gated Lua/JavaScript/native/wasm code-block tests
- `rgx-core/src/execution.rs` backend dispatch logic
## Advanced syntax still parsed but not runtime-integrated
- Backreferences (`\1`, ...): `parsed-only`
- Recursion (`(?R)`, `(?1)`, `(?&name)`): `parsed-only`
- Conditionals (`(?(...)yes|no)` currently supported parser condition forms): `parsed-only`
Behavior contract:
- These forms are accepted by parser/conformance tests where applicable.
- Compilation intentionally fails with explicit error messages until VM/runtime integration lands.
Representative test anchors:
- `rgx-core/src/lib.rs` explicit compile-boundary tests
- `rgx-core/src/parsing.rs` conformance + compile-boundary guardrail tests
## Conditional parser condition forms (current parser coverage)
- Group-exists: `(?(1)yes|no)` (`parsed-only` at runtime)
- Named-group-exists: `(?(<name>)yes|no)`, `(?(name)yes|no)` (`parsed-only` at runtime)
- Lookaround conditions:
  - `(?(?=expr)yes|no)`
  - `(?(?!expr)yes|no)`
  - `(?(?<=expr)yes|no)`
  - `(?(?<!expr)yes|no)`
  (`parsed-only` at runtime)
## Notes for roadmap usage
- This matrix is implementation-facing and must reflect verified behavior only.
- Aspirational goals (broader code-block language support, richer wasm ABI, richer result semantics, full PCRE2 parity) belong in `ROADMAP.md` and `PROJECT_VISION.md`.
- When a feature changes state, update:
  - this file
  - relevant tests
  - `CHANGES.md`
  - `RUST_CODEBASE_ANALYSIS.md`
  - `docs/USER_GUIDE.md` if user-visible behavior changed
