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
//! - **Step 1**: standalone JIT host plumbing. ✅
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
//! - **Step 2 (this commit)**: JIT eligibility check. ✅
//!   - [`codegen::is_jit_eligible`] walks a compiled [`crate::vm::Program`]
//!     and decides whether the JIT will accept the pattern. The check
//!     uses two layers: a quick reject from `ProgramFlags`
//!     (`has_backrefs` / `has_lookarounds` / `has_code_blocks` /
//!     non-empty `subroutines`) followed by a bytecode walk that
//!     looks for ineligible opcodes the flags don't cover (atomic
//!     groups, conditionals, backtracking verbs, `\K` / `\G` / `\X`,
//!     reserved opcodes).
//!   - Hand-curated truth table in `codegen::tests` covers ~45
//!     pattern shapes: literals, char classes, alternations, every
//!     quantifier flavour, anchors, capture groups, realistic
//!     patterns; vs ineligible cases for each forbidden opcode
//!     family.
//!   - **Still does NOT touch the engine.** No `Program::jit_eligible`
//!     field, no `Engine::should_use_jit` accessor, no dispatch
//!     wiring. The check stands alone as a pure function on
//!     `&Program`. Engine wiring lands in step 5 only after the
//!     codegen and capture-trail steps are differentially verified.
//! - **Step 3**: codegen for the easy opcodes (`Char`, `DigitAscii`,
//!   `WordAscii`, `SpaceAscii`, `Split`, `Jump`, `Match`, `SaveStart`,
//!   `SaveEnd`, `Backtrack`, `StartText`, `EndText`, `WordBoundary`,
//!   `NonWordBoundary`). ✅ (substeps 3a–3e.4)
//! - **Step 4a**: corpus-based differential test harness. ✅
//! - **Step 4b**: capture trail in JIT'd code. ✅
//!   - JIT'd function signature extended from
//!     `(text, text_len, pos) -> isize` to
//!     `(text, text_len, pos, captures_ptr) -> isize` so the JIT
//!     can write capture spans for groups 1+ alongside the overall
//!     match. The new type alias is [`JittedFn`]; the old name
//!     [`Step3aJittedFn`] is kept as a backwards-compatible alias.
//!   - Per-frame **capture snapshot**: each backtrack frame in the
//!     stack-allocated `bt_stack` carries a copy of the captures
//!     buffer at the moment of the matching `Split`/`SplitLazy`
//!     push. On a backtrack-pop the snapshot is restored, undoing
//!     all capture writes since the push in one shot. This is the
//!     simpler alternative to the per-modification trail described
//!     in design doc §6.1 (both approaches are byte-for-byte
//!     equivalent under the differential gate).
//!   - Per-frame size grows from 16 bytes (steps 3a–4a) to
//!     `16 + 16 * (num_groups + 1)` bytes; eligibility caps user
//!     groups at `C1_MAX_USER_GROUPS = 16` so the per-function
//!     stack budget stays bounded (~72 KiB at the cap).
//!   - Decoder accepts `SaveStart(g)` / `SaveEnd(g)` for any group
//!     id (previously only `g == 0` was accepted). Patterns like
//!     `(\d+)`, `(a+)b`, `(\w+)@(\w+)\.(\w+)` are now JIT-eligible.
//!   - Engine `try_jit_*` methods allocate a captures buffer of
//!     size `2 * (num_groups + 1)`, reset it between calls, and
//!     read it back into `MatchResult.groups` after a successful
//!     match. The capture buffer state is undefined on a `-1`
//!     return — the engine layer resets it before every call.
//!   - 14 new step-4b tests in `c1::codegen::tests::step4b_*`
//!     covering single/multi-capture patterns, capture-with-
//!     backtrack, lazy capture quantifiers, anchored captures,
//!     nested alternation in captures, and the eligibility cap.
//! - **Step 5**: engine dispatch wiring + 4-tier dispatch chain. ✅
//!   - New [`JitProgram`] type encapsulating `JitHost + FuncId` with
//!     `unsafe impl Send` documented for the read-only-after-finalize
//!     invariant.
//!   - New `Engine::should_use_jit` runtime gate, mirroring
//!     `should_dispatch_to_c2` (no event observer, no runtime safety
//!     limits).
//!   - New `Engine::try_jit_is_match` / `try_jit_find_first` /
//!     `try_jit_find_all` methods, each using `PrefixScanner` for
//!     skip acceleration.
//!   - 4-tier dispatch chain in `Regex::find_first` / `find_all` /
//!     `is_match`: **DFA → Pike-VM → JIT → interpreter**. JIT goes
//!     AFTER Pike-VM (deviation from design doc §8) because Pike-VM
//!     is the safety net for nested-quantifier patterns where the
//!     JIT could blow up exponentially.
//!   - Top-level alternation patterns are excluded from the JIT
//!     (mirrors C2 dispatch) because the JIT'd function signature
//!     returns only the match span, not `matched_branch_number`.
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

pub mod codegen;
pub mod jit;
pub mod runtime;

pub use codegen::{
    compile_program, compile_program_to_jit_program, is_jit_eligible, JittedFn, Step3aJittedFn,
};
pub use jit::{JitHost, JitHostError, JitProgram};
