# ROADMAP
Live forward-looking tracker for rgx.

## Purpose
- Track what we are actively building, what is next, and what is deferred.
- Keep planning separate from shipped history (`CHANGES.md`).
- Reduce handoff friction across sessions.

## How to maintain this file
- Update at least when scope or priorities materially change.
- Keep entries concrete and implementation-oriented.
- Move items across sections (`Now`, `Next`, `Later`, `Done`) as status changes.
- Link validation and shipped details in `CHANGES.md` once completed.

## Status legend
- `planned`: scoped but not started
- `in-progress`: active implementation
- `blocked`: cannot proceed yet
- `done`: completed and validated (then move to `CHANGES.md`)

## Now (active)
### PCRE2 parity program (features, speed, accuracy)
- Status: `in-progress`
- Goal: converge toward practical parity with PCRE2 in capabilities and runtime behavior.
- Scope:
  - maintain a compatibility matrix against PCRE2 feature areas
  - use differential tests to catch semantic mismatches
  - track benchmark parity trends in `rgx-bench`
### Parser-independent engine maturity
- Status: `in-progress`
- Goal: continue delivering advanced regex semantics through AST-first paths while parser syntax catches up.
- Scope:
  - extend assertion/group behavior in VM/compiler
  - add API-level tests for behavior guarantees

### Parser completeness path (toward PGEN integration)
- Status: `planned`
- Goal: support advanced group/assertion syntax in parser path to match AST-first capabilities.
- Scope:
  - align parser tokenization/AST output with VM-supported constructs
  - keep parser behavior consistent with API tests

### Parser interoperability contract and conformance harness
- Status: `in-progress`
- Goal: define and enforce a stable parser boundary so PGEN integration is seamless.
- Scope:
  - maintain a versioned contract in `docs/PARSER_CONTRACT.md`
  - keep fixture-based parser conformance tests in `rgx-core/src/parsing.rs`
  - enforce parse-success/compile-unsupported boundary checks for unintegrated runtime features

### Capability matrix hardening
- Status: `planned`
- Goal: document and test exactly what is shipped vs scaffolded.
- Scope:
  - expand integration tests for user-facing APIs
  - keep docs synchronized with verified behavior

## Next (near-term)
### Performance validation loop
- Status: `planned`
- Goal: tighten benchmark-driven optimization workflow.
- Scope:
  - run and track `rgx-bench` baselines against recent changes
  - prioritize optimizations with measurable impact

### Embedded code-path integration clarity
- Status: `planned`
- Goal: define explicit readiness gates for multi-language code-block paths (JavaScript, Lua, Julia, and additional languages).
- Scope:
  - parser/VM integration boundaries
  - safety model and capability boundaries in docs

### Multi-language code-block runtime expansion
- Status: `planned`
- Goal: extend code-block runtime support beyond initial languages while preserving deterministic behavior and safety guarantees.
- Scope:
  - language runtime integration sequence (JS/Lua first, Julia and others next)
  - shared execution contracts, resource limits, and sandbox controls

## Later (strategic)
### Broader feature coverage
- Status: `planned`
- Scope: deeper advanced regex features beyond current verified set.

### Binding/runtime expansion
- Status: `planned`
- Scope: production-ready external bindings and runtime targets after core stability gates.

## Done recently (snapshot)
- Built-in top-level branch reporting with user-facing 1-based branch number.
- AST-first lookahead support in compiler/VM and API tests.
- AST-first lookbehind support in compiler/VM and API tests.

Detailed implementation history and validation remain in `CHANGES.md`.
