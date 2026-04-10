//! C1: JIT compilation backend.
//!
//! C1 is the JIT (just-in-time) compilation tier of the RGX engine — the
//! second of the two tier-0 perf pushes. It coexists with the existing
//! backtracking VM in `vm.rs` and the C2 NFA/DFA hybrid in `c2/`. Patterns
//! that fall outside the JIT-eligible subset continue to run on the
//! interpreter unchanged.
//!
//! See `docs/C1_JIT_COMPILATION_DESIGN.md` for the full SOTA design
//! proposal, including the no-backtracking subset definition (§5),
//! Cranelift code generator choice (§4), per-opcode lowering plan (§5),
//! capture handling (§6), runtime helper layer (§7), engine dispatch
//! boundary (§8), differential testing strategy (§13), the 9-step phased
//! implementation plan (§14), and the priority-order rule (§1.0): **100%
//! accuracy first, lightning-fast second**.
//!
//! # Implementation status
//!
//! C1 is being built incrementally per the §14 phased plan. Each step
//! ships production-quality code; nothing here is throwaway.
//!
//! - **Step 0**: design proposal landed. ✅
//! - **Step 1 (this commit)**: standalone JIT host plumbing. ✅
//!   - Cargo `jit` feature flag wires Cranelift dependencies.
//!   - [`jit::JitHost`] wraps `cranelift_jit::JITModule` with the
//!     lifetime/ownership story for compiled function pointers.
//!   - [`runtime`] holds the C-ABI signatures for the runtime helper
//!     functions JIT'd code will call out to (currently just signatures
//!     and stubs; real implementations land in steps 6 / 7).
//!   - Smoke test in [`jit`] builds a tiny Cranelift function that
//!     returns the constant `42` and calls it through the JIT host
//!     wrapper, verifying the entire pipeline (target ISA selection,
//!     IR construction, compilation, finalisation, function-pointer
//!     transmute, native invocation) works end-to-end.
//!   - **Does NOT touch the engine.** No `Program::jit_eligible` field,
//!     no `Engine::should_use_jit` accessor, no dispatch wiring. The
//!     module is completely standalone — it can be compiled and tested
//!     in isolation, and removing it has zero effect on the rest of
//!     the engine.
//! - **Step 2**: JIT eligibility check (AST walker, `Program::jit_eligible`
//!   field). (planned)
//! - **Step 3**: codegen for the easy opcodes (`Char`, `DigitAscii`,
//!   `WordAscii`, `SpaceAscii`, `Split`, `Jump`, `Match`, `SaveStart`,
//!   `SaveEnd`, `Backtrack`, `StartText`, `EndText`, `WordBoundary`,
//!   `NonWordBoundary`). (planned)
//! - **Step 4**: capture trail in JIT'd code with the differential gate
//!   active. (planned)
//! - **Step 5**: engine dispatch wiring + 4-tier dispatch chain. (planned)
//! - **Step 6**: `CharClass(id)` and multi-byte literal support via
//!   runtime helpers. (planned)
//! - **Step 7**: runtime safety helpers (step counter, recursion depth,
//!   backtrack frame limit) inlined as Cranelift branches. (planned)
//! - **Step 8**: production cutover, benchmarks, Book chapter. (planned)
//!
//! # Cohabitation invariant
//!
//! C1 is built only for patterns that pass the JIT eligibility check
//! (lands in step 2). Patterns outside the eligible subset never reach
//! this module — they continue to run on the existing backtracking VM
//! (or the C2 DFA / Pike-VM where appropriate). The cohabitation rule
//! from design doc §12 is enforced at the dispatch boundary that lands
//! in step 5.
//!
//! # Why opt-in
//!
//! The `jit` Cargo feature is opt-in for step 1 instead of default-on.
//! Rationale: step 1 is the standalone plumbing that brings Cranelift
//! into the dependency tree. Until the differential gate (step 4) has
//! verified that JIT'd execution is byte-for-byte equivalent to the
//! interpreter on every test, the safe default for users installing
//! `rgx-core` is "no JIT, no new dependencies, behaviour unchanged".
//! The feature flips to default-on at the production cutover in step 8.
//! See design doc §11 for the full feature-gating story and §1.0 for
//! the priority-order rule that drives this decision.

pub mod jit;
pub mod runtime;

pub use jit::{JitHost, JitHostError};
