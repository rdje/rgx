//! C1 codegen layer.
//!
//! Step 2 (shipped) added the **JIT eligibility check**
//! [`is_jit_eligible`]. Step 3a (this module's current state) adds the
//! first slice of **codegen**: [`compile_program`] translates a
//! linear, capture-less, single-byte literal program into a Cranelift
//! function with C ABI signature
//! `unsafe extern "C" fn(text: *const u8, text_len: usize, pos: usize) -> isize`.
//! The function returns the new position on a successful match (i.e.
//! `pos + match_length`) and `-1` on no match.
//!
//! Step 3a deliberately scopes the codegen to the simplest possible
//! coherent slice: programs whose bytecode is exclusively `Char`
//! opcodes (with single-byte payloads, i.e. ASCII literals)
//! optionally wrapped in group-0 `SaveStart` / `SaveEnd` markers and
//! terminated by `Match`. Anything else is rejected with
//! `JitHostError::CodegenUnsupported` and the caller falls back to
//! the interpreter. Subsequent step 3 commits widen the codegen to
//! built-in char classes (3b), anchors (3b), and control flow with
//! backtracking (3c). Step 4 adds capture trail handling and turns
//! the differential gate active.
//!
//! The narrow scope is intentional per design doc §1.0: each commit
//! ships a slice that is byte-for-byte correct against the
//! interpreter on every input it accepts, instead of a partial
//! implementation that's "almost right". Step 3a's correctness is
//! locked by hand-curated unit tests; step 4's differential gate
//! extends that coverage to every JIT-eligible test in the suite.
//!
//! See `docs/C1_JIT_COMPILATION_DESIGN.md` §5.3 for the complete list
//! of patterns the JIT refuses on the first pass and §1.0 for the
//! priority-order rule (100% accuracy first) that drives the
//! conservative defaults below.
//!
//! # Why a separate eligibility check
//!
//! The C2 classifier (in `c2/classifier.rs`) already walks the AST and
//! decides whether a pattern can use the C2 NFA/DFA hybrid engine. C1
//! has its own subset because the JIT and the C2 hybrid have different
//! constraints: C2 needs a regular language; C1 needs an opcode set the
//! Cranelift backend can lower. The two subsets overlap heavily but
//! aren't identical — for example, C1 happily JIT's patterns with
//! lazy quantifiers (which C2's DFA can't handle) and C2 happily
//! handles patterns with `\b` assertions (which the C1 v1 JIT also
//! handles). The two checks live in separate modules so neither has
//! to know about the other's internals.
//!
//! # Design
//!
//! The check has two layers:
//!
//! 1. **Quick rejects from `ProgramFlags`** — the existing compiler
//!    populates `flags.has_backrefs`, `flags.has_lookarounds`, and
//!    `flags.has_code_blocks` at compile time. These cover the most
//!    common ineligible patterns and short-circuit the bytecode walk.
//!
//! 2. **Bytecode walk** — for the cases the flags don't cover
//!    (backtracking verbs, atomic groups, conditionals, recursion,
//!    `\K` / `\G` / `\X`, never-emitted opcodes), the function walks
//!    the bytecode opcode-by-opcode using the same operand-size
//!    convention the VM uses internally. Any unknown opcode, any
//!    forbidden opcode, or any malformed operand layout returns
//!    `false` (defensive).
//!
//! False negatives (missing an opportunity to JIT) are a perf miss —
//! the pattern continues to run on the existing interpreter and
//! produces correct results. False positives (claiming eligibility
//! and then having codegen fail mid-pattern) would silently break
//! correctness and are forbidden by §1.0 of the design doc. The
//! conservative bias is intentional.

use crate::c1::jit::{JitHost, JitHostError};
use crate::vm::{OpCode, Program};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    types, AbiParam, Function, InstBuilder, MemFlags, StackSlotData, StackSlotKind, UserFuncName,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch, Variable};
use cranelift_module::{FuncId, Linkage};

/// Maximum number of backtrack frames the JIT'd function can hold
/// on its stack-allocated `bt_stack`. Patterns whose backtracking
/// depth would exceed this bail with -1 (no match) at runtime; the
/// engine layer at step 5 can fall back to the interpreter for
/// patterns that exhaust the JIT's `bt_stack`.
///
/// 256 frames × 16 bytes per frame = 4 KiB total. Comfortable for
/// any realistic pattern shape; the optimized quantifier opcodes
/// (`StarGreedy` etc.) handle deep loops without consuming the
/// `bt_stack`.
const C1_BACKTRACK_STACK_FRAMES: i64 = 256;
const C1_BACKTRACK_FRAME_BYTES: i64 = 16; // 8 bytes saved_pc + 8 bytes saved_pos
#[allow(clippy::cast_possible_truncation)] // 256 * 16 = 4096 — fits in u32 by construction
#[allow(clippy::cast_sign_loss)] // both factors are positive constants
const C1_BACKTRACK_STACK_BYTES: u32 = (C1_BACKTRACK_STACK_FRAMES * C1_BACKTRACK_FRAME_BYTES) as u32;

/// Returns `true` iff the JIT will accept the given compiled program.
///
/// **C1 step 2 deliverable.** The check is conservative: it returns
/// `false` for any pattern containing an opcode the v1 JIT doesn't
/// know how to lower, OR any pattern flagged at compile time as
/// containing backreferences / lookaround / code blocks, OR any
/// pattern that uses recursion (non-empty `subroutines` vec).
///
/// The JIT-eligible subset includes:
/// - Single-character literals (`Char`, `Any`, `AnyDotAll`)
/// - Built-in character classes (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`)
/// - Custom character classes (`CharClass`, `CharClassNeg`)
/// - Anchors (`^`, `$`, `\A`, `\z`, `\Z`, `\b`, `\B`)
/// - Control flow (`Jump`, `Split`, `SplitLazy`)
/// - Capture groups (`SaveStart`, `SaveEnd`)
/// - Optimized quantifier opcodes (`QuestionGreedy`, `QuestionLazy`,
///   `StarGreedy`, `StarLazy`, `PlusGreedy`, `PlusLazy`)
/// - Top-level alternation tracking (`SetAlternative`)
/// - Termination (`Match`, `Fail`)
///
/// The JIT-ineligible subset includes:
/// - Backreferences (`Backref`)
/// - Lookahead / lookbehind (`Lookahead`, `LookaheadNeg`, `Lookbehind`,
///   `LookbehindNeg`, `JumpIfMatch`, `JumpIfNoMatch`)
/// - Recursion / subroutines (`Call`, plus any pattern with non-empty
///   `subroutines` vec)
/// - Inline code blocks (`CodeBlock`)
/// - Atomic groups + possessive quantifiers (`AtomicStart`, `AtomicEnd`)
/// - Backtracking verbs (`Commit`, `Prune`, `VerbSkip`, `Then`, `Mark`)
/// - `\K` / `\G` / `\X` (`MatchReset`, `PreviousMatchEnd`,
///   `GraphemeCluster`) — deferred to a future pass
/// - All reserved / never-emitted opcodes — defensive
///
/// # Stability
///
/// Once a pattern is declared JIT-eligible by this function, it
/// commits the JIT to producing byte-for-byte identical results to
/// the interpreter for every input. The eligibility list is therefore
/// a contract — extending it requires a corresponding codegen step
/// AND differential test coverage for the new opcode family. See
/// design doc §1.0 (priority order: accuracy first).
#[must_use]
pub fn is_jit_eligible(program: &Program) -> bool {
    // Layer 1: quick rejects from compile-time program flags. These
    // are populated by the existing compiler and cover the most
    // common ineligible cases without needing to walk the bytecode.
    if program.flags.has_backrefs || program.flags.has_lookarounds || program.flags.has_code_blocks
    {
        return false;
    }

    // Note: we deliberately do NOT check `program.subroutines.is_empty()`.
    // The compiler populates `subroutines[0]` with the whole-pattern
    // bytecode for *every* pattern (so `(?R)` can dispatch to it),
    // regardless of whether the pattern actually uses recursion.
    // Recursion is detected by the bytecode walk below — the `Call`
    // opcode is the only way subroutines become reachable, and the
    // walk rejects it as ineligible.

    // Layer 2: walk the bytecode looking for ineligible opcodes
    // that the flags don't cover (backtracking verbs, atomic groups,
    // conditionals, `\K` / `\G` / `\X`, recursion via `Call`,
    // never-emitted opcodes) and stepping past operands so we don't
    // misinterpret operand bytes as opcodes.
    walk_bytecode_eligibility(&program.code)
}

/// Walk a bytecode buffer and return `true` iff every opcode in it
/// is in the JIT-eligible subset and every operand layout is valid.
///
/// Returns `false` on:
/// - Any unknown opcode byte (defensive against bytecode corruption
///   or future opcode additions that the eligibility check hasn't
///   been updated to handle).
/// - Any opcode in the ineligible subset (per `is_opcode_jit_eligible`).
/// - Any operand layout that runs past the end of the bytecode buffer
///   (defensive against malformed programs).
fn walk_bytecode_eligibility(code: &[u8]) -> bool {
    let mut ip = 0;
    while ip < code.len() {
        let Ok(op) = OpCode::try_from(code[ip]) else {
            // Unknown opcode byte — treat as ineligible for safety.
            return false;
        };
        if !is_opcode_jit_eligible(op) {
            return false;
        }
        ip += 1;

        // Optimized quantifier opcodes wrap an INLINE subprogram in
        // their operand bytes. We must recurse into that subprogram
        // because it can contain ineligible opcodes (e.g. `\X+`
        // emits `PlusGreedy(GraphemeCluster)` and `(?R)?` emits
        // `QuestionGreedy(Call)`). Skipping past the operand bytes
        // without inspecting them would silently mark these patterns
        // as eligible — a correctness violation per design doc §1.0.
        // The same recursion structure is used by
        // `RegexVM::rebase_inline_char_class_ids` in `vm.rs` for the
        // analogous reason.
        if matches!(
            op,
            OpCode::QuestionGreedy
                | OpCode::QuestionLazy
                | OpCode::StarGreedy
                | OpCode::StarLazy
                | OpCode::PlusGreedy
                | OpCode::PlusLazy
        ) {
            let rest = &code[ip..];
            let Some(&length_byte) = rest.first() else {
                return false;
            };
            let length = length_byte as usize;
            if rest.len() < 1 + length {
                return false;
            }
            let inner = &rest[1..=length];
            if !walk_bytecode_eligibility(inner) {
                return false;
            }
            ip += 1 + length;
            continue;
        }

        let Some(operand_size) = eligible_opcode_operand_size(op, &code[ip..]) else {
            // Malformed operand layout for an otherwise-eligible
            // opcode (e.g. a `Char` with a length byte that runs
            // past the end of the buffer). Refuse defensively.
            return false;
        };
        ip += operand_size;
    }
    true
}

/// Returns `true` iff this opcode is in the C1 v1 JIT-eligible subset.
///
/// See [`is_jit_eligible`] for the full list and rationale.
#[must_use]
fn is_opcode_jit_eligible(op: OpCode) -> bool {
    match op {
        // === Eligible: literal matching and char classes ===
        OpCode::Char
        | OpCode::Any
        | OpCode::AnyDotAll
        | OpCode::DigitAscii
        | OpCode::DigitAsciiNeg
        | OpCode::WordAscii
        | OpCode::WordAsciiNeg
        | OpCode::SpaceAscii
        | OpCode::SpaceAsciiNeg
        | OpCode::CharClass
        | OpCode::CharClassNeg

        // === Eligible: anchors and boundaries ===
        | OpCode::StartLine
        | OpCode::EndLine
        | OpCode::StartText
        | OpCode::EndText
        | OpCode::EndTextOrNL
        | OpCode::WordBoundary
        | OpCode::NonWordBoundary

        // === Eligible: control flow ===
        | OpCode::Jump
        | OpCode::Split
        | OpCode::SplitLazy

        // === Eligible: capture groups ===
        | OpCode::SaveStart
        | OpCode::SaveEnd

        // === Eligible: optimized quantifier opcodes ===
        | OpCode::QuestionGreedy
        | OpCode::QuestionLazy
        | OpCode::StarGreedy
        | OpCode::StarLazy
        | OpCode::PlusGreedy
        | OpCode::PlusLazy

        // === Eligible: alternation tracking ===
        | OpCode::SetAlternative

        // === Eligible: termination ===
        | OpCode::Match
        | OpCode::Fail => true,

        // === Ineligible: deferred to future passes ===
        OpCode::MatchReset       // \K
        | OpCode::PreviousMatchEnd // \G
        | OpCode::GraphemeCluster  // \X

        // === Ineligible: lookaround ===
        | OpCode::Lookahead
        | OpCode::LookaheadNeg
        | OpCode::Lookbehind
        | OpCode::LookbehindNeg

        // === Ineligible: atomic groups + possessive quantifiers ===
        | OpCode::AtomicStart
        | OpCode::AtomicEnd

        // === Ineligible: backreferences and inline code ===
        | OpCode::Backref
        | OpCode::CodeBlock

        // === Ineligible: conditionals + recursion ===
        | OpCode::JumpIfMatch
        | OpCode::JumpIfNoMatch
        | OpCode::Call

        // === Ineligible: backtracking verbs ===
        | OpCode::Commit
        | OpCode::Prune
        | OpCode::VerbSkip
        | OpCode::Then
        | OpCode::Mark

        // === Ineligible: reserved / never-emitted opcodes (defensive) ===
        | OpCode::SimdFind
        | OpCode::SimdString
        | OpCode::SimdCharClass
        | OpCode::SimdAny
        | OpCode::HotPath
        | OpCode::Memoize
        | OpCode::ClearMemo
        | OpCode::Prefetch
        | OpCode::Accept
        | OpCode::Halt => false,
    }
}

/// Returns the number of operand bytes that follow `op` in the
/// bytecode, given the bytes immediately after the opcode (`rest`).
///
/// Returns `None` if the operand layout is malformed (e.g. a length
/// prefix that runs past the end of `rest`). Mirrors the operand
/// sizes the existing VM uses (see `RegexVM::rebase_inline_char_class_ids`
/// in `vm.rs` for the canonical reference).
///
/// This function only handles operand layouts for opcodes in the
/// JIT-eligible subset — caller must check eligibility first via
/// [`is_opcode_jit_eligible`]. The match arms include eligible
/// opcodes only; ineligible opcodes return `Some(0)` here so the
/// caller can detect them via the eligibility check rather than
/// here.
fn eligible_opcode_operand_size(op: OpCode, rest: &[u8]) -> Option<usize> {
    match op {
        // 1 byte length prefix + length bytes (UTF-8 of the literal char)
        OpCode::Char => {
            let length = *rest.first()? as usize;
            // 1 length byte + length payload bytes
            if rest.len() < 1 + length {
                return None;
            }
            Some(1 + length)
        }

        // 1 byte operand: char class id (`CharClass`/`CharClassNeg`),
        // group id (`SaveStart`/`SaveEnd`), or alternative number
        // (`SetAlternative`).
        OpCode::CharClass
        | OpCode::CharClassNeg
        | OpCode::SaveStart
        | OpCode::SaveEnd
        | OpCode::SetAlternative => {
            if rest.is_empty() {
                return None;
            }
            Some(1)
        }

        // 2 byte signed offset
        OpCode::Jump | OpCode::Split | OpCode::SplitLazy => {
            if rest.len() < 2 {
                return None;
            }
            Some(2)
        }

        // Optimized quantifier opcodes wrap an inline subprogram:
        // 1 byte length prefix + length bytes of subprogram bytecode.
        OpCode::QuestionGreedy
        | OpCode::QuestionLazy
        | OpCode::StarGreedy
        | OpCode::StarLazy
        | OpCode::PlusGreedy
        | OpCode::PlusLazy => {
            let length = *rest.first()? as usize;
            if rest.len() < 1 + length {
                return None;
            }
            Some(1 + length)
        }

        // No operands.
        OpCode::Any
        | OpCode::AnyDotAll
        | OpCode::DigitAscii
        | OpCode::DigitAsciiNeg
        | OpCode::WordAscii
        | OpCode::WordAsciiNeg
        | OpCode::SpaceAscii
        | OpCode::SpaceAsciiNeg
        | OpCode::StartLine
        | OpCode::EndLine
        | OpCode::StartText
        | OpCode::EndText
        | OpCode::EndTextOrNL
        | OpCode::WordBoundary
        | OpCode::NonWordBoundary
        | OpCode::Match
        | OpCode::Fail => Some(0),

        // Ineligible opcodes — caller has already checked eligibility
        // via `is_opcode_jit_eligible`, so reaching this branch means
        // the eligibility table and the operand-size table have
        // drifted apart. Return `None` to refuse rather than risk
        // misadvancing the walker.
        _ => None,
    }
}

// ============================================================
// C1 step 3 — linear codegen architecture
// ============================================================

/// **C1 step 3 signature.** The shape of the JIT'd function returned
/// by [`compile_program`]. Documents the C ABI contract callers
/// transmute the raw function pointer to.
///
/// Step 3a introduced this signature for pure literal programs;
/// step 3b extends it to handle built-in character classes
/// (`\d` / `\D` / `\w` / `\W` / `\s` / `\S`) and simple anchors
/// (`\A` / `\z`). The signature is unchanged: the JIT'd function
/// tests the pattern at *exactly* `pos` (it does not scan), and
/// returns the new position on a successful match or `-1` on no
/// match. Subsequent step 3 sub-commits widen the codegen further
/// (control flow at step 3c, capture trail at step 4) without
/// changing the signature.
///
/// # Parameters
/// - `text`: pointer to the input bytes (borrow lifetime managed by
///   the caller; must outlive the call)
/// - `text_len`: length of the input in bytes
/// - `pos`: byte position to test the pattern at
///
/// # Returns
/// - `>= 0`: the new position after a successful match (`pos +
///   match_length`)
/// - `-1`: the pattern did not match at `pos`
///
/// # Safety
/// Callers must ensure `text` points to at least `text_len` bytes of
/// initialized memory and that `pos <= text_len`. The function
/// performs its own bounds check before any byte loads, but it
/// trusts the caller-supplied pointer / length / position to refer
/// to a valid slice.
pub type Step3aJittedFn =
    unsafe extern "C" fn(text: *const u8, text_len: usize, pos: usize) -> isize;

/// Pre-decoded representation of a single JIT'd opcode.
///
/// The codegen layer decodes a `Program`'s bytecode into a
/// `Vec<JitOp>` and then emits one Cranelift basic block per `JitOp`.
/// This decoupling has two benefits:
///
/// 1. The bytecode walker (which has to handle every opcode's
///    operand-size convention) is separate from the codegen layer
///    (which only cares about the *semantic* opcode kind).
/// 2. The codegen layer can iterate over the `JitOp` list once and
///    generate IR linearly, with each block knowing exactly what
///    comes after it without having to re-walk operands.
///
/// At step 3b the variants cover the linear opcode subset: literal
/// bytes, the six built-in ASCII char-class opcodes, two simple
/// anchors, group-0 capture wrappers (treated as no-op for now;
/// captures land at step 4), and the terminating `Match`. Step 3c
/// will add control-flow variants (`Split`, `Jump`) and step 4 will
/// add real capture handling.
#[derive(Debug, Clone, Copy)]
enum JitOp {
    /// Single-byte literal `Char(b)` — consume one byte equal to `b`.
    Char(u8),
    /// `\d` (negated=false) or `\D` (negated=true) — consume one
    /// byte that is (or is not) an ASCII digit `0..=9`.
    DigitAscii { negated: bool },
    /// `\w` / `\W` — consume one byte that is (or is not) an ASCII
    /// word character `[A-Za-z0-9_]`.
    WordAscii { negated: bool },
    /// `\s` / `\S` — consume one byte that is (or is not) an ASCII
    /// whitespace character. Whitespace = space, tab, newline,
    /// carriage return, form feed, vertical tab. Matches the same
    /// set as `b.is_ascii_whitespace()` in `std`.
    SpaceAscii { negated: bool },
    /// `\A` — zero-width assertion: matches iff `pos == 0`.
    StartText,
    /// `\z` — zero-width assertion: matches iff `pos == text_len`.
    EndText,
    /// `\b` (negated=false) or `\B` (negated=true) — zero-width
    /// assertion that consults the runtime helper
    /// [`crate::c1::runtime::rgx_runtime_word_boundary_test`] for
    /// the boundary check. The codegen lowers this to an indirect
    /// call into the registered helper symbol.
    WordBoundary { negated: bool },
    /// Group-0 capture wrapper — accepted as no-op at step 3a/3b.
    /// The engine layer (step 5) will reconstruct group 0 from the
    /// entry pos and the returned end pos. Capture group ids 1+
    /// require the capture trail and land at step 4. Variant carries
    /// `which` (Start vs End) so step 4 can extend it without a
    /// decoder change.
    SaveGroupZero {
        // Step 3b: field reserved for step 4 capture-trail codegen.
        #[allow(dead_code)]
        which: SaveSlot,
    },
    /// `Split` (greedy) — try the next op (fall-through) first; on
    /// backtrack, resume at op `branch_b_op_idx`. Pushes
    /// `(branch_b_op_idx, current_pos)` onto the backtrack stack
    /// and falls through to the next `op_block`.
    Split {
        /// Op index to resume at on backtrack. Resolved by the
        /// decoder from the bytecode's u16 forward offset.
        branch_b_op_idx: usize,
    },
    /// `SplitLazy` — try op `branch_b_op_idx` first; on backtrack,
    /// resume at the next op (fall-through). Pushes
    /// `(next_op_idx, current_pos)` onto the backtrack stack and
    /// jumps to `op_blocks[branch_b_op_idx]`. Mirror of `Split`
    /// with the branches swapped — gives lazy quantifier semantics.
    SplitLazy {
        /// Op index of the first branch to try.
        branch_b_op_idx: usize,
    },
    /// `Jump` — unconditional jump to op `target_op_idx`. No
    /// backtrack interaction.
    Jump {
        /// Op index to jump to. Resolved by the decoder from the
        /// bytecode's u16 forward offset.
        target_op_idx: usize,
    },
    /// `SetAlternative(idx)` — top-level alternation tracking
    /// metadata. The existing VM uses this to populate
    /// `MatchResult.matched_branch_number`. The JIT'd function only
    /// returns `isize` (the new pos), not a full `MatchResult`, so
    /// we treat this op as a no-op for step 3 — `pos` is unchanged
    /// and we just jump to the next block. The engine layer at
    /// step 5 will handle the branch-number contract by inspecting
    /// the matched span externally (or via a separate codegen
    /// extension).
    SetAlternative,
    /// `Match` — terminate with success and return the current pos.
    Match,
}

/// Which slot of a capture group a `SaveGroupZero` op refers to.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Step 3b: variants reserved for step 4 capture-trail codegen.
enum SaveSlot {
    Start,
    End,
}

/// JIT-compile a linear single-byte literal program into a Cranelift
/// function and return its [`FuncId`].
///
/// **C1 step 3a deliverable.** This function only handles the
/// simplest possible coherent slice of the JIT-eligible subset:
/// programs whose bytecode is exclusively `Char` opcodes (with
/// single-byte payloads, i.e. ASCII literals) optionally wrapped in
/// group-0 `SaveStart` / `SaveEnd` markers and terminated by `Match`.
/// Anything else (multi-byte `Char`, char classes, anchors, control
/// flow, captures for groups 1+, ...) is rejected with
/// [`JitHostError::CodegenUnsupported`] and the caller is expected
/// to fall back to the interpreter for that pattern.
///
/// The narrow scope is intentional per design doc §1.0 (100%
/// accuracy first): each step 3 sub-commit ships a slice that is
/// byte-for-byte correct against the interpreter on every input it
/// accepts. Subsequent commits widen the codegen.
///
/// # JIT'd function shape
///
/// The compiled function has the C ABI signature documented at
/// [`Step3aJittedFn`] — it takes a pointer to a byte slice, a
/// length, and a starting position, and returns the new position on
/// a successful match (`pos + N` where `N` is the literal length) or
/// `-1` on no match.
///
/// # Caller invariants
///
/// - The caller must invoke [`JitHost::finalize_definitions`] *after*
///   this function returns and *before* calling the function pointer
///   retrieved via [`JitHost::get_finalized_fn`]. Definitions are
///   not executable until finalisation.
/// - The function pointer is only valid for the lifetime of the
///   `JitHost` it was compiled into. Dropping the host invalidates
///   any held pointers.
///
/// # Errors
///
/// - [`JitHostError::CodegenUnsupported`] if the program contains
///   any opcode outside the step 3a subset.
/// - [`JitHostError::ModuleError`] if Cranelift fails to declare or
///   define the function.
///
/// # Example (test-only)
///
/// ```ignore
/// let mut host = JitHost::new()?;
/// let program = Compiler::new().compile("abc")?.program;
/// let func_id = compile_program(&program, &mut host)?;
/// host.finalize_definitions()?;
/// let raw = host.get_finalized_fn(func_id);
/// let f: Step3aJittedFn = unsafe { std::mem::transmute(raw) };
/// let text = b"abcdef";
/// let new_pos = unsafe { f(text.as_ptr(), text.len(), 0) };
/// assert_eq!(new_pos, 3);
/// ```
#[allow(clippy::too_many_lines)] // long because it builds the entire IR in one pass; the architecture is naturally monolithic
pub fn compile_program(program: &Program, host: &mut JitHost) -> Result<FuncId, JitHostError> {
    // Eligibility short-circuit. `compile_program` trusts that
    // anything `is_jit_eligible` accepts is something it might be
    // able to lower — `decode_program` below applies the per-step
    // narrower acceptance check.
    if !is_jit_eligible(program) {
        return Err(JitHostError::CodegenUnsupported(
            "program is not in the JIT-eligible subset (see is_jit_eligible)".to_string(),
        ));
    }

    // Decode the bytecode into a list of `JitOp` values. The decoder
    // is the per-step gate: anything outside the current step's
    // codegen subset returns `CodegenUnsupported` with a descriptive
    // message identifying the offending opcode.
    let ops = decode_program(&program.code)?;

    // Build the Cranelift function signature: 3 i64 params (text
    // pointer, text len, pos), 1 i64 return (new pos or -1).
    // Cranelift uses I64 on 64-bit hosts; we'd need a target-pointer
    // type query for 32-bit, which isn't a supported target anyway.
    let mut sig = host.make_signature();
    sig.params.push(AbiParam::new(types::I64));
    sig.params.push(AbiParam::new(types::I64));
    sig.params.push(AbiParam::new(types::I64));
    sig.returns.push(AbiParam::new(types::I64));

    // Use a name unique within the module so multiple programs can
    // be compiled into the same JitHost without colliding.
    let name = format!("rgx_jit_step3_{}", host.next_func_index());
    let func_id = host.declare_function(&name, Linkage::Local, &sig)?;

    // Import the runtime helper(s) the codegen might need into
    // this function. The helpers are registered with the JIT
    // module's symbol table in `JitHost::new`; here we declare
    // them as imports inside *this* function so the codegen layer
    // can issue indirect calls. The `FuncRef` is scoped to the
    // function, not the module — each `compile_program` call
    // imports its own.
    //
    // Step 3c imports only the word-boundary helper. Step 6+ will
    // add char-class and multi-byte helpers as they become needed.
    let mut function = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    let word_boundary_ref = if ops
        .iter()
        .any(|op| matches!(op, JitOp::WordBoundary { .. }))
    {
        Some(host.import_word_boundary_helper(&mut function)?)
    } else {
        None
    };

    // Allocate the backtrack stack slot on the JIT'd function's
    // stack frame. 256 frames × 16 bytes per frame = 4 KiB. Each
    // frame holds (saved_pc: i64, saved_pos: i64) where saved_pc
    // is an op index into `op_blocks` and saved_pos is the input
    // position to restore on backtrack. Allocated up front so the
    // codegen layer can reference it from any op_block.
    let bt_stack_slot = function.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        C1_BACKTRACK_STACK_BYTES,
    ));

    // Build the IR using a per-opcode block-per-block layout. The
    // function's mutable state — `pos`, `bt_top`, plus the function
    // params `text_ptr` / `text_len` — is held in Cranelift
    // `Variable`s instead of being passed between blocks via block
    // parameters. The Variable approach is required because step 3d.2's
    // backtrack-dispatch path needs to restore `pos` from the saved
    // frame on a `br_table` jump, and `br_table` does not accept
    // per-target arguments. The other Variables (`bt_top`, `text_ptr`,
    // `text_len`) ride along for consistency and so any block reached
    // via `failure_dispatch` has access to them via `use_var` without
    // SSA dominance concerns.
    {
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut fb_ctx);

        // Variables. Each is declared once, used/defined across
        // every block as needed. Cranelift's SSA pass auto-inserts
        // phi nodes wherever multiple predecessors converge with
        // different values.
        let pos_var = Variable::from_u32(0);
        let bt_top_var = Variable::from_u32(1);
        let text_ptr_var = Variable::from_u32(2);
        let text_len_var = Variable::from_u32(3);
        builder.declare_var(pos_var, types::I64);
        builder.declare_var(bt_top_var, types::I64);
        builder.declare_var(text_ptr_var, types::I64);
        builder.declare_var(text_len_var, types::I64);

        // Allocate all blocks up front so we can target the next
        // op's block by index when emitting each op.
        let entry = builder.create_block();
        let success_block = builder.create_block();
        let fail_block = builder.create_block();
        let failure_dispatch_block = builder.create_block();
        let op_blocks: Vec<_> = ops.iter().map(|_| builder.create_block()).collect();

        // === Entry block: load function params into Variables, init
        // bt_top to 0, jump into the first op block (or directly to
        // success if there are no ops, which shouldn't happen but is
        // handled defensively).
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let entry_text_ptr = builder.block_params(entry)[0];
        let entry_text_len = builder.block_params(entry)[1];
        let entry_init_pos = builder.block_params(entry)[2];
        builder.def_var(text_ptr_var, entry_text_ptr);
        builder.def_var(text_len_var, entry_text_len);
        builder.def_var(pos_var, entry_init_pos);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.def_var(bt_top_var, zero);
        let first_target = op_blocks.first().copied().unwrap_or(success_block);
        builder.ins().jump(first_target, &[]);
        builder.seal_block(entry);

        // === Per-op blocks: emit IR for each JitOp. Each block reads
        // the current state from Variables, applies the op-specific
        // semantics, and either updates Variables + jumps to the next
        // op_block (success edge), or jumps to `failure_dispatch_block`
        // (fail edge). The Match op jumps to success_block.
        //
        // Note: we deliberately do NOT seal op_blocks inside this
        // loop. Each op_block can receive an additional predecessor
        // edge from `failure_dispatch_block` via the `br_table`,
        // which is built AFTER this loop. Cranelift requires all
        // predecessors to be known at seal time, so the seal must
        // wait until after `failure_dispatch_block` is built. The
        // sealing happens in a second pass below.
        for (i, op) in ops.iter().enumerate() {
            let block = op_blocks[i];
            builder.switch_to_block(block);

            // The "next op index" is `i + 1` (or `op_blocks.len()` if
            // this is the last op, which is the Match terminator).
            // The "next block" for a successful step is the next op
            // block, or the success block if this is the last op.
            // (Match always jumps to success_block directly via
            // `emit_jit_op` and ignores `next_block`.)
            let next_op_idx = i + 1;
            let next_block = op_blocks.get(next_op_idx).copied().unwrap_or(success_block);

            emit_jit_op(
                &mut builder,
                *op,
                next_op_idx,
                pos_var,
                text_ptr_var,
                text_len_var,
                bt_top_var,
                bt_stack_slot,
                &op_blocks,
                next_block,
                failure_dispatch_block,
                fail_block,
                success_block,
                word_boundary_ref,
            );
        }

        // === Failure dispatch block: pop a backtrack frame and
        // resume at the saved op index, with the saved pos restored
        // into `pos_var`. If the bt_stack is empty, jump to the
        // global fail_block (return -1). All consuming-op fail
        // edges go through here so backtracking is automatic.
        builder.switch_to_block(failure_dispatch_block);
        let bt_top = builder.use_var(bt_top_var);
        let bt_top_zero = builder.ins().icmp_imm(IntCC::Equal, bt_top, 0);
        let pop_block = builder.create_block();
        builder
            .ins()
            .brif(bt_top_zero, fail_block, &[], pop_block, &[]);
        builder.seal_block(failure_dispatch_block);

        builder.switch_to_block(pop_block);
        let new_bt_top = builder.ins().iadd_imm(bt_top, -1);
        builder.def_var(bt_top_var, new_bt_top);
        // Compute frame address: stack_addr(bt_stack_slot) + new_bt_top * 16.
        let frame_offset = builder.ins().imul_imm(new_bt_top, C1_BACKTRACK_FRAME_BYTES);
        let stack_base = builder.ins().stack_addr(types::I64, bt_stack_slot, 0);
        let frame_addr = builder.ins().iadd(stack_base, frame_offset);
        let saved_pc = builder
            .ins()
            .load(types::I64, MemFlags::trusted(), frame_addr, 0);
        let saved_pos = builder
            .ins()
            .load(types::I64, MemFlags::trusted(), frame_addr, 8);
        builder.def_var(pos_var, saved_pos);

        // Dispatch via `cranelift_frontend::Switch` which handles
        // br_table construction AND the SSA-pass-inserted block
        // parameters (phi nodes for the Variables) correctly. The
        // low-level `JumpTableData` API would require us to know
        // the implicit block-call args ahead of time, which is
        // impossible because the SSA pass inserts them later.
        // `Switch` defers the construction so the args resolve
        // automatically when the blocks are sealed below.
        let mut switch = Switch::new();
        for (idx, &op_block) in op_blocks.iter().enumerate() {
            switch.set_entry(idx as u128, op_block);
        }
        switch.emit(&mut builder, saved_pc, fail_block);
        builder.seal_block(pop_block);

        // Now that `pop_block`'s `br_table` has registered every
        // op_block as a predecessor, we can finally seal the
        // op_blocks. (Sealing during the per-op-block emission loop
        // would have failed because the br_table predecessor
        // wouldn't have been recorded yet, and Cranelift's SSA pass
        // requires all predecessors to be known at seal time.)
        for &op_block in &op_blocks {
            builder.seal_block(op_block);
        }

        // === Success block: return the current pos (read from the
        // pos_var Variable, which the Match op set last).
        builder.switch_to_block(success_block);
        let final_pos = builder.use_var(pos_var);
        builder.ins().return_(&[final_pos]);
        builder.seal_block(success_block);

        // === Fail block: return -1.
        builder.switch_to_block(fail_block);
        let neg_one = builder.ins().iconst(types::I64, -1);
        builder.ins().return_(&[neg_one]);
        builder.seal_block(fail_block);

        builder.finalize();
    }

    host.define_function(func_id, function)?;
    Ok(func_id)
}

/// **C1 step 5 entry point.** Compile a `Program` into a complete
/// `JitProgram` ready for engine dispatch. This is a thin wrapper
/// over [`compile_program`] that:
///
/// 1. Creates a fresh [`JitHost`].
/// 2. Calls [`compile_program`] to JIT-compile the program. On
///    failure (`CodegenUnsupported` or any other host error), the
///    `JitHost` is dropped and the error is propagated.
/// 3. Calls [`JitHost::finalize_definitions`] to make the function
///    pointer executable.
/// 4. Wraps the host + func_id into a [`JitProgram`].
///
/// Used by `Engine::new` (in `engine.rs`) to optionally JIT-compile
/// every newly-built engine. Patterns the JIT can't handle return
/// `Err(JitHostError::CodegenUnsupported)` and the engine layer
/// stores `None` for `jit_program`, falling back to the existing
/// dispatch chain (DFA → Pike-VM → interpreter).
///
/// # Errors
///
/// - [`JitHostError::HostNotSupported`] if Cranelift can't build a
///   JIT host for the current target.
/// - [`JitHostError::CodegenUnsupported`] if the program is outside
///   the JIT-eligible subset.
/// - [`JitHostError::ModuleError`] if Cranelift fails to declare,
///   define, or finalise the function.
pub fn compile_program_to_jit_program(
    program: &Program,
) -> Result<crate::c1::JitProgram, JitHostError> {
    let mut host = JitHost::new()?;
    let func_id = compile_program(program, &mut host)?;
    host.finalize_definitions()?;
    Ok(crate::c1::JitProgram::new(host, func_id))
}

/// Emit Cranelift IR for a single [`JitOp`] inside its dedicated
/// block. The caller has already switched the builder to the op's
/// block and obtained the current `pos` from its block parameter.
///
/// Each op either advances `pos` and jumps to `next_block` (passing
/// the new pos) or jumps to `fail_block`. The `Match` op terminates
/// by jumping to `success_block` with the current pos.
///
/// **Step 3b/3c/3d.** This function handles the JIT-eligible opcode
/// subset: literals, char classes, simple anchors, word boundaries,
/// group-0 capture wrappers, control flow with backtracking
/// (`Split` / `SplitLazy` / `Jump`), and the `Match` terminator.
///
/// Word boundary handling uses an indirect call to the runtime
/// helper [`crate::c1::runtime::rgx_runtime_word_boundary_test`]
/// via the `word_boundary_ref` parameter, which `compile_program`
/// imports into the current function via
/// [`crate::c1::jit::JitHost::import_word_boundary_helper`] when
/// any `WordBoundary` op appears in the program.
///
/// Control-flow handling uses the stack-allocated backtrack array
/// allocated by `compile_program` (`bt_stack_slot`) plus the
/// `bt_top_var` Variable counter. `Split` / `SplitLazy` push
/// `(saved_pc, current_pos)` onto the stack and increment `bt_top`;
/// consuming-op failures jump to `failure_dispatch_block` which
/// pops a frame and resumes via `br_table`.
///
/// `next_op_idx` is the op index of the fall-through next op (used
/// by `SplitLazy` to record the `saved_pc` on the backtrack stack).
/// `op_blocks` is the full `op_block` table (used by `Jump` /
/// `SplitLazy` to dispatch to forward targets by index).
#[allow(clippy::too_many_arguments)] // each parameter is conceptually distinct and there's no good grouping
#[allow(clippy::too_many_lines)] // long because it dispatches every JitOp variant; refactoring would just split arbitrarily
fn emit_jit_op(
    builder: &mut FunctionBuilder,
    op: JitOp,
    next_op_idx: usize,
    pos_var: Variable,
    text_ptr_var: Variable,
    text_len_var: Variable,
    bt_top_var: Variable,
    bt_stack_slot: cranelift_codegen::ir::StackSlot,
    op_blocks: &[cranelift_codegen::ir::Block],
    next_block: cranelift_codegen::ir::Block,
    failure_dispatch_block: cranelift_codegen::ir::Block,
    fail_block: cranelift_codegen::ir::Block,
    success_block: cranelift_codegen::ir::Block,
    word_boundary_ref: Option<cranelift_codegen::ir::FuncRef>,
) {
    let pos = builder.use_var(pos_var);
    let text_ptr = builder.use_var(text_ptr_var);
    let text_len = builder.use_var(text_len_var);
    match op {
        JitOp::Char(b) => {
            emit_consume_byte_with_test(
                builder,
                pos,
                pos_var,
                text_ptr,
                text_len,
                next_block,
                failure_dispatch_block,
                |fb, byte| fb.ins().icmp_imm(IntCC::Equal, byte, i64::from(b)),
            );
        }
        JitOp::DigitAscii { negated } => {
            emit_consume_byte_with_test(
                builder,
                pos,
                pos_var,
                text_ptr,
                text_len,
                next_block,
                failure_dispatch_block,
                |fb, byte| emit_digit_byte_test(fb, byte, negated),
            );
        }
        JitOp::WordAscii { negated } => {
            emit_consume_byte_with_test(
                builder,
                pos,
                pos_var,
                text_ptr,
                text_len,
                next_block,
                failure_dispatch_block,
                |fb, byte| emit_word_byte_test(fb, byte, negated),
            );
        }
        JitOp::SpaceAscii { negated } => {
            emit_consume_byte_with_test(
                builder,
                pos,
                pos_var,
                text_ptr,
                text_len,
                next_block,
                failure_dispatch_block,
                |fb, byte| emit_space_byte_test(fb, byte, negated),
            );
        }
        JitOp::StartText => {
            // Zero-width: matches iff pos == 0. No bytes consumed,
            // so pos_var is left unchanged. On failure, dispatch to
            // failure_dispatch so any backtrack frames can be tried.
            let cond = builder.ins().icmp_imm(IntCC::Equal, pos, 0);
            builder
                .ins()
                .brif(cond, next_block, &[], failure_dispatch_block, &[]);
        }
        JitOp::EndText => {
            // Zero-width: matches iff pos == text_len. No bytes
            // consumed, so pos_var is left unchanged.
            let cond = builder.ins().icmp(IntCC::Equal, pos, text_len);
            builder
                .ins()
                .brif(cond, next_block, &[], failure_dispatch_block, &[]);
        }
        JitOp::WordBoundary { negated } => {
            let func_ref = word_boundary_ref
                .expect("WordBoundary op requires the helper import; compile_program is buggy");
            let call = builder.ins().call(func_ref, &[text_ptr, text_len, pos]);
            let raw_result = builder.inst_results(call)[0];
            let is_boundary = builder.ins().icmp_imm(IntCC::NotEqual, raw_result, 0);
            if negated {
                builder
                    .ins()
                    .brif(is_boundary, failure_dispatch_block, &[], next_block, &[]);
            } else {
                builder
                    .ins()
                    .brif(is_boundary, next_block, &[], failure_dispatch_block, &[]);
            }
        }
        JitOp::SaveGroupZero { which: _ } => {
            // Step 3b: group-0 wrappers are no-op. The engine layer
            // (step 5) reconstructs group 0 from entry pos + returned
            // end pos. Step 4 will replace this with real capture
            // trail handling for groups 1+. pos_var is left unchanged.
            builder.ins().jump(next_block, &[]);
        }
        JitOp::SetAlternative => {
            // No-op: the JIT'd function returns only `isize`, not a
            // full `MatchResult`, so we don't need to track branch
            // numbers. pos_var is unchanged.
            builder.ins().jump(next_block, &[]);
        }
        JitOp::Split { branch_b_op_idx } => {
            // Greedy split: try the next op (fall-through) first.
            // On backtrack, resume at op_blocks[branch_b_op_idx].
            // Push (branch_b_op_idx, current_pos) onto bt_stack and
            // jump to next_block.
            emit_backtrack_push(
                builder,
                pos,
                bt_top_var,
                bt_stack_slot,
                branch_b_op_idx,
                next_block,
                fail_block,
            );
        }
        JitOp::SplitLazy { branch_b_op_idx } => {
            // Lazy split: try op_blocks[branch_b_op_idx] first. On
            // backtrack, resume at the next op (fall-through).
            // Push (next_op_idx, current_pos) onto bt_stack and
            // jump to op_blocks[branch_b_op_idx].
            let target_block = op_blocks
                .get(branch_b_op_idx)
                .copied()
                .unwrap_or(success_block);
            emit_backtrack_push(
                builder,
                pos,
                bt_top_var,
                bt_stack_slot,
                next_op_idx,
                target_block,
                fail_block,
            );
        }
        JitOp::Jump { target_op_idx } => {
            // Unconditional forward jump. No backtrack interaction.
            let target_block = op_blocks
                .get(target_op_idx)
                .copied()
                .unwrap_or(success_block);
            builder.ins().jump(target_block, &[]);
        }
        JitOp::Match => {
            // Terminate with success. pos_var is left unchanged
            // — the success block reads it via use_var to produce
            // the return value.
            let _ = next_block; // unused for Match
            let _ = pos_var; // unchanged on Match
            let _ = pos; // success block reads pos_var fresh
            builder.ins().jump(success_block, &[]);
        }
    }
}

/// Emit IR that pushes a backtrack frame onto the stack-allocated
/// `bt_stack` and then jumps to `success_block` (the destination
/// on the "took the branch we're committing to" edge).
///
/// The frame stored is `(saved_pc as i64, current_pos as i64)`.
/// `saved_pc` is the op index to resume at on a future backtrack
/// pop. `current_pos` is the position at the time of the push.
///
/// On `bt_stack` overflow (`bt_top` would exceed
/// `C1_BACKTRACK_STACK_FRAMES`), the codegen jumps to
/// `overflow_block` which returns -1 — the JIT cannot handle
/// patterns whose backtracking depth exceeds the fixed `bt_stack`
/// size, and the engine layer at step 5 will fall back to the
/// interpreter for those patterns.
#[allow(clippy::too_many_arguments)] // each parameter is conceptually distinct
fn emit_backtrack_push(
    builder: &mut FunctionBuilder,
    pos: cranelift_codegen::ir::Value,
    bt_top_var: Variable,
    bt_stack_slot: cranelift_codegen::ir::StackSlot,
    saved_pc_idx: usize,
    success_block: cranelift_codegen::ir::Block,
    overflow_block: cranelift_codegen::ir::Block,
) {
    let bt_top = builder.use_var(bt_top_var);

    // Overflow check: if bt_top >= C1_BACKTRACK_STACK_FRAMES,
    // jump to overflow_block (which returns -1).
    let at_capacity = builder.ins().icmp_imm(
        IntCC::UnsignedGreaterThanOrEqual,
        bt_top,
        C1_BACKTRACK_STACK_FRAMES,
    );
    let push_block = builder.create_block();
    builder
        .ins()
        .brif(at_capacity, overflow_block, &[], push_block, &[]);

    builder.switch_to_block(push_block);
    builder.seal_block(push_block);

    // Compute frame address: stack_addr(bt_stack_slot) + bt_top * 16.
    let frame_offset = builder.ins().imul_imm(bt_top, C1_BACKTRACK_FRAME_BYTES);
    let stack_base = builder.ins().stack_addr(types::I64, bt_stack_slot, 0);
    let frame_addr = builder.ins().iadd(stack_base, frame_offset);

    // Store saved_pc at frame_addr + 0 (i64). `saved_pc_idx` is
    // an op index that always fits in i64 (op counts are bounded
    // by the bytecode walker — single u8 length prefixes — so the
    // count is never anywhere near 2^63). The cast is safe by
    // construction; the `try_from` makes the bound explicit.
    let saved_pc_const = i64::try_from(saved_pc_idx)
        .expect("saved_pc_idx fits in i64 by construction (bytecode op count is small)");
    let saved_pc_val = builder.ins().iconst(types::I64, saved_pc_const);
    builder
        .ins()
        .store(MemFlags::trusted(), saved_pc_val, frame_addr, 0);

    // Store current_pos at frame_addr + 8 (i64).
    builder.ins().store(MemFlags::trusted(), pos, frame_addr, 8);

    // Increment bt_top.
    let new_bt_top = builder.ins().iadd_imm(bt_top, 1);
    builder.def_var(bt_top_var, new_bt_top);

    // Jump to the "took the branch we're committing to" target.
    builder.ins().jump(success_block, &[]);
}

/// Helper: emit IR for a "consume one byte and apply a predicate"
/// opcode. The predicate closure builds the per-byte test in
/// Cranelift IR (returning an i8 boolean value: 0 = fail, 1 = pass).
///
/// The emitted IR:
/// 1. Bounds check: `pos < text_len`. If not, jump to fail.
/// 2. Load `text[pos]` as an i8.
/// 3. Apply the predicate closure to get a boolean.
/// 4. If true, write `pos + 1` into `pos_var` and jump to
///    `next_block`. Else jump to fail (`pos_var` left unchanged so
///    the backtrack-dispatch path at step 3d.2 can restore from
///    the stack-saved pos).
#[allow(clippy::too_many_arguments)] // each parameter is conceptually distinct and there's no good grouping
fn emit_consume_byte_with_test<F>(
    builder: &mut FunctionBuilder,
    pos: cranelift_codegen::ir::Value,
    pos_var: Variable,
    text_ptr: cranelift_codegen::ir::Value,
    text_len: cranelift_codegen::ir::Value,
    next_block: cranelift_codegen::ir::Block,
    fail_block: cranelift_codegen::ir::Block,
    predicate: F,
) where
    F: FnOnce(&mut FunctionBuilder, cranelift_codegen::ir::Value) -> cranelift_codegen::ir::Value,
{
    // Bounds check: pos < text_len. If pos == text_len there's no
    // byte to consume, so the op fails.
    let in_bounds = builder.ins().icmp(IntCC::UnsignedLessThan, pos, text_len);
    let load_block = builder.create_block();
    builder
        .ins()
        .brif(in_bounds, load_block, &[], fail_block, &[]);
    builder.switch_to_block(load_block);
    builder.seal_block(load_block);

    // Load text[pos].
    let byte_addr = builder.ins().iadd(text_ptr, pos);
    let byte = builder
        .ins()
        .load(types::I8, MemFlags::trusted(), byte_addr, 0);

    // Apply the predicate.
    let cond = predicate(builder, byte);

    // Pre-compute the advanced pos. Cranelift's optimizer will
    // dead-strip this on the fail edge since pos_var is only
    // written on the success edge below.
    let new_pos = builder.ins().iadd_imm(pos, 1);
    let advance_block = builder.create_block();
    builder
        .ins()
        .brif(cond, advance_block, &[], fail_block, &[]);

    // Success edge: write the new pos into pos_var and continue.
    builder.switch_to_block(advance_block);
    builder.seal_block(advance_block);
    builder.def_var(pos_var, new_pos);
    builder.ins().jump(next_block, &[]);
}

/// Helper: emit IR for the ASCII digit test `b >= '0' && b <= '9'`,
/// optionally negated. Returns a Cranelift boolean value.
fn emit_digit_byte_test(
    builder: &mut FunctionBuilder,
    byte: cranelift_codegen::ir::Value,
    negated: bool,
) -> cranelift_codegen::ir::Value {
    let ge = builder
        .ins()
        .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, byte, 0x30); // '0'
    let le = builder
        .ins()
        .icmp_imm(IntCC::UnsignedLessThanOrEqual, byte, 0x39); // '9'
    let in_range = builder.ins().band(ge, le);
    if negated {
        builder.ins().bxor_imm(in_range, 1)
    } else {
        in_range
    }
}

/// Helper: emit IR for the ASCII word-character test
/// `(b >= 'A' && b <= 'Z') || (b >= 'a' && b <= 'z') || (b >= '0' && b <= '9') || b == '_'`,
/// optionally negated. Returns a Cranelift boolean value.
fn emit_word_byte_test(
    builder: &mut FunctionBuilder,
    byte: cranelift_codegen::ir::Value,
    negated: bool,
) -> cranelift_codegen::ir::Value {
    let upper_lo = builder
        .ins()
        .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, byte, 0x41); // 'A'
    let upper_hi = builder
        .ins()
        .icmp_imm(IntCC::UnsignedLessThanOrEqual, byte, 0x5A); // 'Z'
    let in_upper = builder.ins().band(upper_lo, upper_hi);

    let lower_lo = builder
        .ins()
        .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, byte, 0x61); // 'a'
    let lower_hi = builder
        .ins()
        .icmp_imm(IntCC::UnsignedLessThanOrEqual, byte, 0x7A); // 'z'
    let in_lower = builder.ins().band(lower_lo, lower_hi);

    let digit_lo = builder
        .ins()
        .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, byte, 0x30); // '0'
    let digit_hi = builder
        .ins()
        .icmp_imm(IntCC::UnsignedLessThanOrEqual, byte, 0x39); // '9'
    let in_digit = builder.ins().band(digit_lo, digit_hi);

    let is_underscore = builder.ins().icmp_imm(IntCC::Equal, byte, 0x5F); // '_'

    let alpha = builder.ins().bor(in_upper, in_lower);
    let alphanum = builder.ins().bor(alpha, in_digit);
    let word = builder.ins().bor(alphanum, is_underscore);

    if negated {
        builder.ins().bxor_imm(word, 1)
    } else {
        word
    }
}

/// Helper: emit IR for the ASCII whitespace test against the same
/// six bytes `b.is_ascii_whitespace()` matches in `std`: space
/// (0x20), tab (0x09), newline (0x0A), carriage return (0x0D), form
/// feed (0x0C), vertical tab (0x0B). Returns a Cranelift boolean.
fn emit_space_byte_test(
    builder: &mut FunctionBuilder,
    byte: cranelift_codegen::ir::Value,
    negated: bool,
) -> cranelift_codegen::ir::Value {
    let is_space_char = builder.ins().icmp_imm(IntCC::Equal, byte, 0x20);
    let is_tab_char = builder.ins().icmp_imm(IntCC::Equal, byte, 0x09);
    let is_newline_char = builder.ins().icmp_imm(IntCC::Equal, byte, 0x0A);
    let is_carriage_return = builder.ins().icmp_imm(IntCC::Equal, byte, 0x0D);
    let is_form_feed = builder.ins().icmp_imm(IntCC::Equal, byte, 0x0C);
    let is_vertical_tab = builder.ins().icmp_imm(IntCC::Equal, byte, 0x0B);

    let space_or_tab = builder.ins().bor(is_space_char, is_tab_char);
    let newline_or_cr = builder.ins().bor(is_newline_char, is_carriage_return);
    let form_or_vert = builder.ins().bor(is_form_feed, is_vertical_tab);
    let pair_one = builder.ins().bor(space_or_tab, newline_or_cr);
    let space = builder.ins().bor(pair_one, form_or_vert);

    if negated {
        builder.ins().bxor_imm(space, 1)
    } else {
        space
    }
}

/// Walk a program's bytecode and decode it into a `Vec<JitOp>`.
///
/// The decoder is the per-step gate: any opcode outside the current
/// step's codegen subset returns `CodegenUnsupported` with a
/// descriptive message identifying the offending opcode. Step 3d
/// (the current state) accepts:
///
/// - `Char(len=1)` — single-byte ASCII literal
/// - `DigitAscii` / `DigitAsciiNeg`
/// - `WordAscii` / `WordAsciiNeg`
/// - `SpaceAscii` / `SpaceAsciiNeg`
/// - `StartText` (`\A`) / `EndText` (`\z`)
/// - `WordBoundary` / `NonWordBoundary` (via runtime helper)
/// - `SaveStart(0)` / `SaveEnd(0)` (group-0 wrappers, no-op)
/// - `Split` / `SplitLazy` / `Jump` (control flow with backtrack)
/// - `Match` (terminator)
///
/// Anything else (multi-byte `Char`, line anchors, `\Z` / `\X` /
/// `\K`, optimized quantifier opcodes like `StarGreedy`, captures
/// for groups 1+, ...) returns a descriptive `CodegenUnsupported`
/// error and the caller falls back to the interpreter for that
/// pattern.
///
/// # Two-pass walker
///
/// Step 3d.2 introduced the two-pass walk because `Split` / `Jump`
/// / `SplitLazy` opcodes carry forward byte offsets that must be
/// resolved to op-index targets so the codegen layer can dispatch
/// via `op_blocks[op_idx]`. The first pass builds a map from byte
/// offsets to op indices; the second pass decodes each op and
/// resolves any forward target via `binary_search` on the map.
#[allow(clippy::too_many_lines)] // long because it dispatches every supported opcode; refactoring would just split arbitrarily
fn decode_program(code: &[u8]) -> Result<Vec<JitOp>, JitHostError> {
    // Pass 1: walk the bytecode collecting the byte offset where
    // each opcode starts. This is needed by pass 2 to resolve
    // Split/Jump forward targets to op indices. The walker uses
    // the same operand-size convention as `eligible_opcode_operand_size`
    // (the canonical reference is `RegexVM::rebase_inline_char_class_ids`
    // in `vm.rs`).
    let op_positions = collect_op_positions(code)?;

    // Pass 2: decode each opcode into a `JitOp`. Forward targets
    // (Split / Jump / SplitLazy) are resolved by computing
    // `target_byte = ip_after_operand + offset` and looking up the
    // corresponding op index in `op_positions` via binary search.
    let mut ops = Vec::with_capacity(op_positions.len());
    let mut ip = 0;
    let mut saw_match = false;

    while ip < code.len() {
        let Ok(op) = OpCode::try_from(code[ip]) else {
            return Err(JitHostError::CodegenUnsupported(format!(
                "unknown opcode byte 0x{:02X} at ip={ip}",
                code[ip]
            )));
        };
        ip += 1;

        match op {
            OpCode::Char => {
                let Some(&len_byte) = code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(
                        "truncated Char opcode (missing length prefix)".to_string(),
                    ));
                };
                let length = len_byte as usize;
                ip += 1;
                if length != 1 {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3 only handles single-byte Char literals; \
                         got {length}-byte Char (multi-byte literals land at step 6)"
                    )));
                }
                let Some(&byte) = code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(
                        "truncated Char opcode (missing payload byte)".to_string(),
                    ));
                };
                ops.push(JitOp::Char(byte));
                ip += 1;
            }
            OpCode::DigitAscii => ops.push(JitOp::DigitAscii { negated: false }),
            OpCode::DigitAsciiNeg => ops.push(JitOp::DigitAscii { negated: true }),
            OpCode::WordAscii => ops.push(JitOp::WordAscii { negated: false }),
            OpCode::WordAsciiNeg => ops.push(JitOp::WordAscii { negated: true }),
            OpCode::SpaceAscii => ops.push(JitOp::SpaceAscii { negated: false }),
            OpCode::SpaceAsciiNeg => ops.push(JitOp::SpaceAscii { negated: true }),
            OpCode::StartText => ops.push(JitOp::StartText),
            OpCode::EndText => ops.push(JitOp::EndText),
            OpCode::WordBoundary => ops.push(JitOp::WordBoundary { negated: false }),
            OpCode::NonWordBoundary => ops.push(JitOp::WordBoundary { negated: true }),
            OpCode::Split => {
                // 2-byte u16 forward offset. Target = ip_after_operand + offset.
                let target_idx = decode_forward_target(code, &mut ip, &op_positions, "Split")?;
                ops.push(JitOp::Split {
                    branch_b_op_idx: target_idx,
                });
            }
            OpCode::SplitLazy => {
                let target_idx = decode_forward_target(code, &mut ip, &op_positions, "SplitLazy")?;
                ops.push(JitOp::SplitLazy {
                    branch_b_op_idx: target_idx,
                });
            }
            OpCode::Jump => {
                let target_idx = decode_forward_target(code, &mut ip, &op_positions, "Jump")?;
                ops.push(JitOp::Jump {
                    target_op_idx: target_idx,
                });
            }
            OpCode::SetAlternative => {
                // Skip the 1-byte alternative-index operand. The
                // op is a no-op in JIT'd code (we don't track
                // branch numbers in the JIT path).
                if ip >= code.len() {
                    return Err(JitHostError::CodegenUnsupported(
                        "truncated SetAlternative opcode (missing index operand)".to_string(),
                    ));
                }
                ip += 1;
                ops.push(JitOp::SetAlternative);
            }
            OpCode::PlusGreedy => {
                // Step 3e.1 lowering: PlusGreedy(inner) →
                // [inner_jit_ops..., Split{exit}, Jump{back to inner_start}]
                //
                // The first iteration of inner is mandatory; the
                // Split-based loop handles 2nd+ iterations with
                // greedy backtracking via the step 3d.2 backtrack
                // stack. Restricted to "simple linear inner"
                // subprograms (no nested control flow); nested
                // optimized quantifiers will land in a later step.
                emit_plus_quantifier(code, &mut ip, &mut ops, "PlusGreedy", false)?;
            }
            OpCode::PlusLazy => {
                // Step 3e.4 lowering: PlusLazy(inner) →
                // [inner_jit_ops..., SplitLazy{exit}, Jump{back to inner_start}]
                //
                // Same shape as PlusGreedy but with SplitLazy. The
                // first iteration is still mandatory. After it,
                // SplitLazy jumps to exit FIRST (try one iteration
                // = the minimum for `+`), and on backtrack falls
                // through to Jump → inner_start (try one more
                // iteration). Lazy `+?` matches the minimum number
                // of iterations consistent with the rest of the
                // pattern matching.
                emit_plus_quantifier(code, &mut ip, &mut ops, "PlusLazy", true)?;
            }
            OpCode::QuestionGreedy => {
                // Step 3e.3 lowering: QuestionGreedy(inner) →
                // [Split{exit}, inner_jit_ops...]
                //
                // The simplest of the optimized quantifier
                // lowerings: a Split followed by the inner, with
                // NO Jump back. The Split pushes (exit_op_idx,
                // current_pos) and falls through to the inner. If
                // the inner succeeds, it advances pos and the
                // last inner op falls through to the next outer
                // op (= exit, via the per-op-block sequence). If
                // the inner fails, failure_dispatch pops the
                // frame and dispatches to exit at the saved pos.
                // No loop because `?` is "zero or one".
                emit_question_quantifier(code, &mut ip, &mut ops, "QuestionGreedy", false)?;
            }
            OpCode::QuestionLazy => {
                // Step 3e.4 lowering: QuestionLazy(inner) →
                // [SplitLazy{exit}, inner_jit_ops...]
                //
                // Same shape as QuestionGreedy but with SplitLazy
                // instead of Split. SplitLazy jumps to exit FIRST
                // (zero iterations) and on backtrack falls through
                // to the inner (one iteration). Lazy `??` matches
                // as few iterations as possible.
                emit_question_quantifier(code, &mut ip, &mut ops, "QuestionLazy", true)?;
            }
            OpCode::StarGreedy => {
                // Step 3e.2 lowering: StarGreedy(inner) →
                // [Split{exit}, inner_jit_ops..., Jump{back to Split}]
                //
                // The Split sits BEFORE the inner so zero iterations
                // is a valid match — on the very first visit, the
                // Split pushes (exit, current_pos) onto the bt_stack,
                // and if the inner immediately fails, failure_dispatch
                // pops the frame and exits at the saved (=current) pos.
                // For non-zero iterations, each successful inner pass
                // jumps back to the Split which pushes another frame
                // and tries again, accumulating one frame per
                // iteration so backtracking can shrink toward zero.
                emit_star_quantifier(code, &mut ip, &mut ops, "StarGreedy", false)?;
            }
            OpCode::StarLazy => {
                // Step 3e.4 lowering: StarLazy(inner) →
                // [SplitLazy{exit}, inner_jit_ops..., Jump{back to SplitLazy}]
                //
                // Same shape as StarGreedy but with SplitLazy. The
                // SplitLazy jumps to exit FIRST (try zero iterations
                // first), and on backtrack falls through to the
                // inner (try one more iteration). Each successful
                // iteration loops back to the SplitLazy which pushes
                // another frame; backtracking grows the iteration
                // count UP toward whatever satisfies the rest of
                // the pattern.
                emit_star_quantifier(code, &mut ip, &mut ops, "StarLazy", true)?;
            }
            OpCode::SaveStart | OpCode::SaveEnd => {
                let Some(&group_id) = code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "truncated {op:?} opcode (missing group id)"
                    )));
                };
                if group_id != 0 {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3 only accepts group-0 capture wrappers; \
                         got {op:?} for group {group_id} (capture trail lands at step 4)"
                    )));
                }
                ip += 1;
                let which = if op == OpCode::SaveStart {
                    SaveSlot::Start
                } else {
                    SaveSlot::End
                };
                ops.push(JitOp::SaveGroupZero { which });
            }
            OpCode::Match => {
                saw_match = true;
                if ip != code.len() {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3 expects Match to terminate the program; \
                         got {} trailing bytes after Match",
                        code.len() - ip
                    )));
                }
                ops.push(JitOp::Match);
                break;
            }
            other => {
                return Err(JitHostError::CodegenUnsupported(format!(
                    "step 3 does not yet support {other:?} (lands in a later step)"
                )));
            }
        }
    }

    if !saw_match {
        return Err(JitHostError::CodegenUnsupported(
            "step 3 requires a Match opcode at end of program".to_string(),
        ));
    }

    Ok(ops)
}

/// Pass-1 walker for `decode_program`. Collects `(byte_offset,
/// jit_op_idx)` pairs so pass 2 can resolve Split/Jump forward
/// targets to JIT op indices. The `jit_op_idx` is the index of the
/// FIRST `JitOp` emitted for the bytecode opcode at `byte_offset` —
/// most opcodes unfold to exactly 1 `JitOp`, but optimized
/// quantifier opcodes (`PlusGreedy` etc.) unfold to multiple via
/// the Split/Jump-based loop lowering at step 3e.1.
///
/// Returns `Err(CodegenUnsupported)` on any opcode the per-step
/// codegen subset rejects, on truncated bytecode, or on operand
/// layouts that run past the end of the buffer.
fn collect_op_positions(code: &[u8]) -> Result<Vec<(usize, usize)>, JitHostError> {
    let mut positions = Vec::new();
    let mut byte_ip = 0;
    let mut jit_op_idx = 0;
    while byte_ip < code.len() {
        positions.push((byte_ip, jit_op_idx));
        let Ok(op) = OpCode::try_from(code[byte_ip]) else {
            return Err(JitHostError::CodegenUnsupported(format!(
                "unknown opcode byte 0x{:02X} at ip={byte_ip}",
                code[byte_ip]
            )));
        };
        // Reject ineligible opcodes here so pass 2 can rely on the
        // eligible-only operand-size table without falling through.
        if !is_opcode_jit_eligible(op) {
            return Err(JitHostError::CodegenUnsupported(format!(
                "step 3 does not yet support {op:?} (lands in a later step)"
            )));
        }
        let bytecode_op_start = byte_ip;
        byte_ip += 1;
        let Some(operand_size) = eligible_opcode_operand_size(op, &code[byte_ip..]) else {
            return Err(JitHostError::CodegenUnsupported(format!(
                "truncated {op:?} opcode at ip={bytecode_op_start}"
            )));
        };
        // How many JitOps does this bytecode opcode unfold to?
        // Most opcodes unfold to 1 JitOp; optimized quantifier
        // opcodes (PlusGreedy at step 3e.1) unfold to several via
        // the Split/Jump-based loop lowering.
        let jit_op_count = compute_jit_op_count(op, &code[byte_ip..byte_ip + operand_size])?;
        jit_op_idx += jit_op_count;
        byte_ip += operand_size;
    }
    Ok(positions)
}

/// Returns the number of `JitOp`s that the given bytecode opcode
/// unfolds into, given its operand bytes (without the opcode byte
/// itself). Most opcodes return 1; optimized quantifier opcodes
/// return more.
///
/// Step 3e.1/3e.2/3e.3/3e.4: all six optimized quantifier opcodes
/// are implemented as unfolding quantifiers — `PlusGreedy`,
/// `StarGreedy`, `QuestionGreedy`, `PlusLazy`, `StarLazy`,
/// `QuestionLazy`. The lazy variants share the same unfolded
/// counts as their greedy counterparts (only the Split→SplitLazy
/// substitution differs in the codegen). `Plus*`/`Star*` unfold
/// to `inner_count + 2` (Split + inner + Jump or inner + Split +
/// Jump). `Question*` unfolds to `inner_count + 1` because `?` is
/// "zero or one" with no loop — just Split + inner.
fn compute_jit_op_count(op: OpCode, operand_bytes: &[u8]) -> Result<usize, JitHostError> {
    match op {
        OpCode::PlusGreedy
        | OpCode::StarGreedy
        | OpCode::QuestionGreedy
        | OpCode::PlusLazy
        | OpCode::StarLazy
        | OpCode::QuestionLazy => {
            // Plus/Star (greedy or lazy): inner + Split + Jump = +2 ops.
            // Question (greedy or lazy): inner + Split = +1 op (no loop).
            let length_byte = operand_bytes.first().copied().ok_or_else(|| {
                JitHostError::CodegenUnsupported(format!(
                    "truncated {op:?} opcode (missing length prefix)"
                ))
            })? as usize;
            if operand_bytes.len() < 1 + length_byte {
                return Err(JitHostError::CodegenUnsupported(format!(
                    "truncated {op:?} opcode (length prefix exceeds operand bytes)"
                )));
            }
            let inner_bytes = &operand_bytes[1..=length_byte];
            let inner_jit_count = simple_inner_jit_op_count(inner_bytes)?;
            let extra = if matches!(op, OpCode::QuestionGreedy | OpCode::QuestionLazy) {
                1 // Split only
            } else {
                2 // Split + Jump
            };
            Ok(inner_jit_count + extra)
        }
        // Every other supported opcode unfolds to 1 JitOp.
        _ => Ok(1),
    }
}

/// Returns the `JitOp` count for a "simple linear" inner
/// subprogram — the subset of opcodes the step 3e.1 `PlusGreedy`
/// lowering accepts. Each opcode in the inner subprogram
/// contributes exactly 1 `JitOp`; nested optimized quantifiers,
/// control-flow opcodes, and the `Match` terminator are rejected.
fn simple_inner_jit_op_count(inner_code: &[u8]) -> Result<usize, JitHostError> {
    let mut count = 0;
    let mut ip = 0;
    while ip < inner_code.len() {
        let Ok(op) = OpCode::try_from(inner_code[ip]) else {
            return Err(JitHostError::CodegenUnsupported(format!(
                "unknown opcode byte 0x{:02X} in PlusGreedy inner subprogram at ip={ip}",
                inner_code[ip]
            )));
        };
        if !is_simple_inner_opcode(op) {
            return Err(JitHostError::CodegenUnsupported(format!(
                "PlusGreedy inner subprogram contains {op:?} which is not in \
                 the step 3e.1 simple-inner subset (lands in a later step)"
            )));
        }
        ip += 1;
        let Some(operand_size) = eligible_opcode_operand_size(op, &inner_code[ip..]) else {
            return Err(JitHostError::CodegenUnsupported(format!(
                "truncated {op:?} opcode inside PlusGreedy inner subprogram"
            )));
        };
        ip += operand_size;
        count += 1;
    }
    Ok(count)
}

/// Returns `true` iff the opcode is in the "simple linear inner"
/// subset that step 3e.1's `PlusGreedy` lowering accepts. This is a
/// subset of `is_opcode_jit_eligible` that excludes control-flow
/// opcodes (`Split` / `Jump` / `SplitLazy`), optimized quantifier
/// opcodes (`PlusGreedy` / `StarGreedy` / `QuestionGreedy` and
/// their lazy forms), and the `Match` terminator.
fn is_simple_inner_opcode(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::Char
            | OpCode::DigitAscii
            | OpCode::DigitAsciiNeg
            | OpCode::WordAscii
            | OpCode::WordAsciiNeg
            | OpCode::SpaceAscii
            | OpCode::SpaceAsciiNeg
            | OpCode::StartText
            | OpCode::EndText
            | OpCode::WordBoundary
            | OpCode::NonWordBoundary
            | OpCode::SaveStart
            | OpCode::SaveEnd
    )
}

/// Decode a "simple linear inner" subprogram (bytecode bytes from
/// the inline operand of a `PlusGreedy` / `StarGreedy` / etc.
/// opcode) into `JitOp`s and append them to `ops`. The subset
/// accepts only opcodes from `is_simple_inner_opcode` — anything
/// else (`Split` / `Jump` / nested optimized quantifiers / `Match`)
/// returns `CodegenUnsupported` so the caller falls back to the
/// interpreter.
///
/// The inner subprogram does NOT have a trailing `Match` opcode —
/// it's a fragment, not a complete program. The `PlusGreedy`
/// lowering in `decode_program` adds the loop tail (`Split` +
/// `Jump`) after the inner `JitOp`s.
fn decode_simple_inner_into(inner_code: &[u8], ops: &mut Vec<JitOp>) -> Result<(), JitHostError> {
    let mut ip = 0;
    while ip < inner_code.len() {
        let Ok(op) = OpCode::try_from(inner_code[ip]) else {
            return Err(JitHostError::CodegenUnsupported(format!(
                "unknown opcode byte 0x{:02X} in PlusGreedy inner subprogram at ip={ip}",
                inner_code[ip]
            )));
        };
        if !is_simple_inner_opcode(op) {
            return Err(JitHostError::CodegenUnsupported(format!(
                "PlusGreedy inner subprogram contains {op:?} which is not in \
                 the step 3e.1 simple-inner subset (lands in a later step)"
            )));
        }
        ip += 1;
        match op {
            OpCode::Char => {
                let Some(&len_byte) = inner_code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(
                        "truncated Char inside PlusGreedy inner (missing length prefix)"
                            .to_string(),
                    ));
                };
                let length = len_byte as usize;
                ip += 1;
                if length != 1 {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3 only handles single-byte Char literals; \
                         got {length}-byte Char inside PlusGreedy inner"
                    )));
                }
                let Some(&byte) = inner_code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(
                        "truncated Char inside PlusGreedy inner (missing payload byte)".to_string(),
                    ));
                };
                ip += 1;
                ops.push(JitOp::Char(byte));
            }
            OpCode::DigitAscii => ops.push(JitOp::DigitAscii { negated: false }),
            OpCode::DigitAsciiNeg => ops.push(JitOp::DigitAscii { negated: true }),
            OpCode::WordAscii => ops.push(JitOp::WordAscii { negated: false }),
            OpCode::WordAsciiNeg => ops.push(JitOp::WordAscii { negated: true }),
            OpCode::SpaceAscii => ops.push(JitOp::SpaceAscii { negated: false }),
            OpCode::SpaceAsciiNeg => ops.push(JitOp::SpaceAscii { negated: true }),
            OpCode::StartText => ops.push(JitOp::StartText),
            OpCode::EndText => ops.push(JitOp::EndText),
            OpCode::WordBoundary => ops.push(JitOp::WordBoundary { negated: false }),
            OpCode::NonWordBoundary => ops.push(JitOp::WordBoundary { negated: true }),
            OpCode::SaveStart | OpCode::SaveEnd => {
                let Some(&group_id) = inner_code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "truncated {op:?} inside PlusGreedy inner (missing group id)"
                    )));
                };
                if group_id != 0 {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3 only accepts group-0 capture wrappers; \
                         got {op:?} for group {group_id} inside PlusGreedy inner"
                    )));
                }
                ip += 1;
                let which = if op == OpCode::SaveStart {
                    SaveSlot::Start
                } else {
                    SaveSlot::End
                };
                ops.push(JitOp::SaveGroupZero { which });
            }
            // is_simple_inner_opcode rejected anything else above.
            _ => unreachable!(
                "is_simple_inner_opcode allowed {op:?} but the decoder doesn't handle it"
            ),
        }
    }
    Ok(())
}

/// Read the inline subprogram operand of an optimized quantifier
/// opcode (`PlusGreedy` / `StarGreedy` / `QuestionGreedy` and their
/// lazy variants). The operand layout is `[length: u8, body: [u8;
/// length]]`. Advances `ip` past the length prefix and the body.
///
/// Returns the slice of body bytes (a borrow into `code`).
fn read_inline_subprogram<'a>(
    code: &'a [u8],
    ip: &mut usize,
    op_name: &str,
) -> Result<&'a [u8], JitHostError> {
    let Some(&len_byte) = code.get(*ip) else {
        return Err(JitHostError::CodegenUnsupported(format!(
            "truncated {op_name} opcode (missing length prefix)"
        )));
    };
    let length = len_byte as usize;
    *ip += 1;
    if *ip + length > code.len() {
        return Err(JitHostError::CodegenUnsupported(format!(
            "truncated {op_name} opcode (length prefix exceeds remaining bytes)"
        )));
    }
    let inner_bytes = &code[*ip..*ip + length];
    *ip += length;
    Ok(inner_bytes)
}

/// Emit `JitOp`s for a `?` quantifier (greedy or lazy). The lowering
/// is `[Split{exit}, inner_jit_ops...]` for greedy or
/// `[SplitLazy{exit}, inner_jit_ops...]` for lazy. No Jump back
/// because `?` is "zero or one" with no loop. The greedy variant
/// tries the inner first (fall-through) and exits on backtrack;
/// the lazy variant tries exit first (zero iterations) and falls
/// through to inner on backtrack.
fn emit_question_quantifier(
    code: &[u8],
    ip: &mut usize,
    ops: &mut Vec<JitOp>,
    op_name: &str,
    lazy: bool,
) -> Result<(), JitHostError> {
    let inner_bytes = read_inline_subprogram(code, ip, op_name)?;

    let split_op_idx = ops.len();
    let inner_count = simple_inner_jit_op_count(inner_bytes)?;
    let exit_op_idx = split_op_idx + inner_count + 1;
    if lazy {
        ops.push(JitOp::SplitLazy {
            branch_b_op_idx: exit_op_idx,
        });
    } else {
        ops.push(JitOp::Split {
            branch_b_op_idx: exit_op_idx,
        });
    }

    let inner_start_op_idx = ops.len();
    debug_assert_eq!(inner_start_op_idx, split_op_idx + 1);
    decode_simple_inner_into(inner_bytes, ops)?;
    debug_assert_eq!(
        ops.len() - inner_start_op_idx,
        inner_count,
        "step 3e.3/3e.4 {op_name} unfolded count drift between pass 1 and pass 2"
    );
    debug_assert_eq!(
        ops.len(),
        exit_op_idx,
        "step 3e.3/3e.4 {op_name} emitted count != computed exit_op_idx"
    );
    Ok(())
}

/// Emit `JitOp`s for a `*` quantifier (greedy or lazy). The lowering
/// is `[Split{exit}, inner_jit_ops..., Jump{back to Split}]` for
/// greedy or `[SplitLazy{exit}, inner_jit_ops..., Jump{back to
/// SplitLazy}]` for lazy. The Jump targets the Split (NOT
/// `inner_start`) so each iteration pushes a fresh `bt_stack` frame.
/// The greedy variant tries the inner first; the lazy variant
/// tries exit first.
fn emit_star_quantifier(
    code: &[u8],
    ip: &mut usize,
    ops: &mut Vec<JitOp>,
    op_name: &str,
    lazy: bool,
) -> Result<(), JitHostError> {
    let inner_bytes = read_inline_subprogram(code, ip, op_name)?;

    let split_op_idx = ops.len();
    let inner_count = simple_inner_jit_op_count(inner_bytes)?;
    let exit_op_idx = split_op_idx + inner_count + 2;
    if lazy {
        ops.push(JitOp::SplitLazy {
            branch_b_op_idx: exit_op_idx,
        });
    } else {
        ops.push(JitOp::Split {
            branch_b_op_idx: exit_op_idx,
        });
    }

    let inner_start_op_idx = ops.len();
    debug_assert_eq!(inner_start_op_idx, split_op_idx + 1);
    decode_simple_inner_into(inner_bytes, ops)?;
    debug_assert_eq!(
        ops.len() - inner_start_op_idx,
        inner_count,
        "step 3e.2/3e.4 {op_name} unfolded count drift between pass 1 and pass 2"
    );

    ops.push(JitOp::Jump {
        target_op_idx: split_op_idx,
    });

    debug_assert_eq!(
        ops.len(),
        exit_op_idx,
        "step 3e.2/3e.4 {op_name} emitted count != computed exit_op_idx"
    );
    Ok(())
}

/// Emit `JitOp`s for a `+` quantifier (greedy or lazy). The lowering
/// is `[inner_jit_ops..., Split{exit}, Jump{back to inner_start}]`
/// for greedy or `[inner_jit_ops..., SplitLazy{exit}, Jump{back to
/// inner_start}]` for lazy. The first iteration of inner is
/// mandatory; the Split-based loop handles 2nd+ iterations. The
/// greedy variant tries another iteration first; the lazy variant
/// tries exit first (one iteration is the minimum for `+`).
fn emit_plus_quantifier(
    code: &[u8],
    ip: &mut usize,
    ops: &mut Vec<JitOp>,
    op_name: &str,
    lazy: bool,
) -> Result<(), JitHostError> {
    let inner_bytes = read_inline_subprogram(code, ip, op_name)?;

    let inner_start_op_idx = ops.len();
    decode_simple_inner_into(inner_bytes, ops)?;
    let inner_end_op_idx = ops.len();
    debug_assert_eq!(
        inner_end_op_idx - inner_start_op_idx,
        simple_inner_jit_op_count(inner_bytes)?,
        "step 3e.1/3e.4 {op_name} unfolded count drift between pass 1 and pass 2"
    );

    let exit_op_idx = ops.len() + 2;
    if lazy {
        ops.push(JitOp::SplitLazy {
            branch_b_op_idx: exit_op_idx,
        });
    } else {
        ops.push(JitOp::Split {
            branch_b_op_idx: exit_op_idx,
        });
    }
    ops.push(JitOp::Jump {
        target_op_idx: inner_start_op_idx,
    });
    Ok(())
}

/// Decode a 2-byte u16 forward offset from the bytecode (the
/// operand of `Split` / `SplitLazy` / `Jump`) and resolve it to a
/// JIT op index via binary search on `op_positions`. Advances `ip`
/// past the operand bytes.
///
/// The encoding (per the existing VM dispatch in `vm.rs`):
/// - Operand at `code[ip..ip+2]` is `u16::from_le_bytes`.
/// - Target byte = `ip + 2 + offset` (the offset is from
///   immediately after the operand, NOT from the opcode start).
/// - Target byte must exactly match a bytecode op start, otherwise
///   the bytecode is malformed and we bail with `CodegenUnsupported`.
fn decode_forward_target(
    code: &[u8],
    ip: &mut usize,
    op_positions: &[(usize, usize)],
    op_name: &str,
) -> Result<usize, JitHostError> {
    if *ip + 1 >= code.len() {
        return Err(JitHostError::CodegenUnsupported(format!(
            "truncated {op_name} opcode (missing 2-byte forward offset)"
        )));
    }
    let offset = u16::from_le_bytes([code[*ip], code[*ip + 1]]) as usize;
    *ip += 2;
    let target_byte = *ip + offset;
    op_positions
        .binary_search_by_key(&target_byte, |&(byte, _)| byte)
        .map(|idx| op_positions[idx].1)
        .map_err(|_| {
            JitHostError::CodegenUnsupported(format!(
                "{op_name} forward offset {offset} (target byte {target_byte}) \
                 does not land on an op start; bytecode is malformed or the \
                 target is mid-operand"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;

    /// Compile a pattern through the full PGEN + compiler pipeline
    /// (mirrors what `Regex::compile` does internally) and return the
    /// resulting `Program` so the eligibility check can be exercised
    /// against real bytecode.
    fn compile(pattern: &str) -> Program {
        Compiler::new()
            .compile(pattern)
            .unwrap_or_else(|e| panic!("pattern `{pattern}` must compile: {e}"))
            .program
    }

    fn assert_eligible(pattern: &str) {
        let program = compile(pattern);
        assert!(
            is_jit_eligible(&program),
            "pattern `{pattern}` should be JIT-eligible but isn't"
        );
    }

    fn assert_ineligible(pattern: &str) {
        let program = compile(pattern);
        assert!(
            !is_jit_eligible(&program),
            "pattern `{pattern}` should be JIT-ineligible but isn't"
        );
    }

    // ============================================================
    // Hand-curated truth table — eligible patterns
    // ============================================================

    #[test]
    fn eligible_simple_literal() {
        assert_eligible("abc");
    }

    #[test]
    fn eligible_single_character() {
        assert_eligible("a");
    }

    #[test]
    fn eligible_dot() {
        assert_eligible(".");
    }

    #[test]
    fn eligible_dot_all_flag() {
        // (?s) flag is fine — it changes the dot semantics but
        // doesn't introduce any ineligible opcode.
        assert_eligible("(?s).");
    }

    #[test]
    fn eligible_digit_class() {
        assert_eligible(r"\d");
    }

    #[test]
    fn eligible_digit_negated() {
        assert_eligible(r"\D");
    }

    #[test]
    fn eligible_word_class() {
        assert_eligible(r"\w");
    }

    #[test]
    fn eligible_space_class() {
        assert_eligible(r"\s");
    }

    #[test]
    fn eligible_custom_char_class() {
        assert_eligible("[a-z]");
    }

    #[test]
    fn eligible_negated_char_class() {
        assert_eligible("[^0-9]");
    }

    #[test]
    fn eligible_alternation_simple() {
        assert_eligible("cat|dog|bird");
    }

    #[test]
    fn eligible_greedy_star() {
        assert_eligible("a*");
    }

    #[test]
    fn eligible_greedy_plus() {
        assert_eligible("a+");
    }

    #[test]
    fn eligible_optional() {
        assert_eligible("a?");
    }

    #[test]
    fn eligible_lazy_star() {
        assert_eligible("a*?");
    }

    #[test]
    fn eligible_lazy_plus() {
        assert_eligible("a+?");
    }

    #[test]
    fn eligible_counted_quantifier() {
        assert_eligible("a{3,5}");
    }

    #[test]
    fn eligible_anchor_start_text() {
        assert_eligible(r"\Aabc");
    }

    #[test]
    fn eligible_anchor_end_text() {
        assert_eligible(r"abc\z");
    }

    #[test]
    fn eligible_anchor_start_line() {
        assert_eligible("^abc");
    }

    #[test]
    fn eligible_anchor_end_line() {
        assert_eligible("abc$");
    }

    #[test]
    fn eligible_word_boundary() {
        assert_eligible(r"\bword\b");
    }

    #[test]
    fn eligible_non_word_boundary() {
        assert_eligible(r"\Bword");
    }

    #[test]
    fn eligible_capture_group() {
        assert_eligible(r"(\d+)");
    }

    #[test]
    fn eligible_multiple_capture_groups() {
        assert_eligible(r"(\d{4})-(\d{2})-(\d{2})");
    }

    #[test]
    fn eligible_non_capturing_group() {
        assert_eligible("(?:abc)+");
    }

    #[test]
    fn eligible_realistic_email_like_pattern() {
        assert_eligible(r"\w+@\w+\.\w+");
    }

    #[test]
    fn eligible_realistic_log_pattern() {
        assert_eligible(r"\bERROR\s+\d+");
    }

    #[test]
    fn eligible_realistic_iso_date() {
        assert_eligible(r"\d{4}-\d{2}-\d{2}");
    }

    // ============================================================
    // Hand-curated truth table — ineligible patterns
    // ============================================================

    #[test]
    fn ineligible_backreference_numeric() {
        assert_ineligible(r"(\w+)\s+\1");
    }

    #[test]
    fn ineligible_lookahead_positive() {
        assert_ineligible("foo(?=bar)");
    }

    #[test]
    fn ineligible_lookahead_negative() {
        assert_ineligible("foo(?!bar)");
    }

    #[test]
    fn ineligible_lookbehind_positive() {
        assert_ineligible("(?<=foo)bar");
    }

    #[test]
    fn ineligible_lookbehind_negative() {
        assert_ineligible("(?<!foo)bar");
    }

    #[test]
    fn ineligible_atomic_group() {
        assert_ineligible("(?>a+)");
    }

    #[test]
    fn ineligible_possessive_quantifier_star() {
        // a*+ is lowered to an atomic group; ineligible.
        assert_ineligible("a*+");
    }

    #[test]
    fn ineligible_possessive_quantifier_plus() {
        assert_ineligible("a++");
    }

    #[test]
    fn ineligible_possessive_quantifier_optional() {
        assert_ineligible("a?+");
    }

    #[test]
    fn ineligible_recursion_full() {
        // (?R) is the whole-pattern recursion form. The compiler
        // populates `subroutines` for any recursive pattern, which
        // the eligibility check rejects via the subroutines.is_empty()
        // gate.
        assert_ineligible(r"a(?R)?b");
    }

    #[test]
    fn ineligible_mark_verb() {
        assert_ineligible("(*MARK:foo)abc");
    }

    #[test]
    fn ineligible_commit_verb() {
        assert_ineligible("a(*COMMIT)b");
    }

    #[test]
    fn ineligible_prune_verb() {
        assert_ineligible("a(*PRUNE)b");
    }

    #[test]
    fn ineligible_skip_verb() {
        assert_ineligible("a(*SKIP)b");
    }

    #[test]
    fn ineligible_match_reset_k() {
        assert_ineligible(r"foo\Kbar");
    }

    #[test]
    fn ineligible_grapheme_cluster_x() {
        assert_ineligible(r"\X+");
    }

    // ============================================================
    // Edge cases
    // ============================================================

    #[test]
    fn eligible_alternation_inside_capture_group() {
        assert_eligible("(cat|dog)");
    }

    #[test]
    fn eligible_nested_groups() {
        assert_eligible("((a)b)");
    }

    #[test]
    fn eligible_many_quantifiers() {
        // Email-like: every component is quantified but no quantifier
        // is nested inside another. Should be eligible.
        assert_eligible(r"\w+@\w+\.\w+");
    }

    #[test]
    fn eligible_character_class_inside_quantifier() {
        assert_eligible("[a-z]+");
    }

    #[test]
    fn eligible_complex_realistic_pattern() {
        // Anchored timestamp + log level + message head. No
        // backreferences, no lookaround, no recursion, no atomic
        // groups, no verbs, no `\K` / `\G` / `\X`. Should be
        // eligible.
        assert_eligible(r"\A\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\s+(ERROR|WARN|INFO)\s+\w+");
    }

    // ============================================================
    // C1 step 3a — literal codegen
    // ============================================================
    //
    // Each test JIT-compiles a real `Program` (built via the normal
    // compile pipeline) and exercises the resulting native function
    // pointer through its C ABI signature. The host is held across
    // every call to keep the executable mapping alive (per the
    // lifetime invariant documented on `JitHost::get_finalized_fn`).

    /// Compile a pattern to a `Program`, JIT-compile it via
    /// step 3a, and return both the host and the typed function
    /// pointer. The caller MUST keep the host alive for the
    /// lifetime of the function pointer.
    fn jit_compile(pattern: &str) -> (JitHost, Step3aJittedFn) {
        let program = compile(pattern);
        let mut host = JitHost::new().expect("JitHost::new must succeed");
        let func_id = compile_program(&program, &mut host)
            .unwrap_or_else(|e| panic!("compile_program for `{pattern}` failed: {e}"));
        host.finalize_definitions().expect("finalize must succeed");
        let raw = host.get_finalized_fn(func_id);
        assert!(!raw.is_null());
        // SAFETY: The IR signature `(i64, i64, i64) -> i64` matches
        // the `Step3aJittedFn` C ABI signature exactly. The function
        // pointer is alive for the lifetime of `host`, returned
        // alongside it so the caller keeps the host pinned across
        // every call.
        let func: Step3aJittedFn = unsafe { std::mem::transmute(raw) };
        (host, func)
    }

    #[test]
    fn step3a_single_char_match_at_position_zero() {
        let (_host, func) = jit_compile("a");
        let text = b"abc";
        // SAFETY: text outlives the call; pos is within bounds.
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(result, 1, "matching `a` at pos 0 of \"abc\" must return 1");
    }

    #[test]
    fn step3a_single_char_no_match() {
        let (_host, func) = jit_compile("a");
        let text = b"xyz";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(
            result, -1,
            "matching `a` at pos 0 of \"xyz\" must return -1"
        );
    }

    #[test]
    fn step3a_three_char_literal_match() {
        let (_host, func) = jit_compile("abc");
        let text = b"abcdef";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(
            result, 3,
            "matching `abc` at pos 0 of \"abcdef\" must return 3"
        );
    }

    #[test]
    fn step3a_three_char_literal_match_at_offset() {
        let (_host, func) = jit_compile("abc");
        let text = b"xyzabcdef";
        let result = unsafe { func(text.as_ptr(), text.len(), 3) };
        assert_eq!(
            result, 6,
            "matching `abc` at pos 3 of \"xyzabcdef\" must return 6"
        );
    }

    #[test]
    fn step3a_three_char_literal_partial_no_match() {
        let (_host, func) = jit_compile("abc");
        let text = b"abx";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(
            result, -1,
            "partial-prefix `ab` of `abc` against \"abx\" must return -1"
        );
    }

    #[test]
    fn step3a_three_char_literal_short_input_no_match() {
        let (_host, func) = jit_compile("abc");
        let text = b"ab";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(
            result, -1,
            "two-byte input \"ab\" must reject 3-byte literal `abc`"
        );
    }

    #[test]
    fn step3a_three_char_literal_offset_at_end_no_match() {
        let (_host, func) = jit_compile("abc");
        let text = b"abcdef";
        // Starting at pos 4, only 2 bytes remain — bounds check
        // must reject the match attempt.
        let result = unsafe { func(text.as_ptr(), text.len(), 4) };
        assert_eq!(result, -1, "starting at pos 4 leaves only 2 bytes");
    }

    #[test]
    fn step3a_three_char_literal_at_pos_equals_text_len() {
        let (_host, func) = jit_compile("abc");
        let text = b"abcdef";
        // Starting at pos == text_len: 0 bytes remain, must reject.
        let result = unsafe { func(text.as_ptr(), text.len(), text.len()) };
        assert_eq!(result, -1, "no bytes left starting at text.len()");
    }

    #[test]
    fn step3a_long_literal_match() {
        let (_host, func) = jit_compile("hello world");
        let text = b"hello world!";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(result, 11);
    }

    #[test]
    fn step3a_long_literal_no_match_first_byte_mismatch() {
        let (_host, func) = jit_compile("hello world");
        let text = b"Hello world!";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(
            result, -1,
            "first-byte mismatch (lowercase vs uppercase) must reject"
        );
    }

    #[test]
    fn step3a_long_literal_no_match_last_byte_mismatch() {
        let (_host, func) = jit_compile("hello world");
        let text = b"hello worlD!";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(
            result, -1,
            "last-byte mismatch must reject (whole literal must match)"
        );
    }

    #[test]
    fn step3a_multiple_programs_on_one_host() {
        // Compile two distinct literal programs into the same host
        // and call both. Validates the unique-name allocation in
        // `JitHost::next_func_index` and the per-function lookup via
        // `get_finalized_fn`.
        let mut host = JitHost::new().expect("host construction must succeed");
        let prog_abc = compile("abc");
        let prog_xyz = compile("xyz");
        let id_abc = compile_program(&prog_abc, &mut host).expect("abc compile");
        let id_xyz = compile_program(&prog_xyz, &mut host).expect("xyz compile");
        host.finalize_definitions().expect("finalize");
        // SAFETY: signature is fixed Step3aJittedFn for every step 3a
        // compiled function; host outlives the calls.
        let f_abc: Step3aJittedFn = unsafe { std::mem::transmute(host.get_finalized_fn(id_abc)) };
        let f_xyz: Step3aJittedFn = unsafe { std::mem::transmute(host.get_finalized_fn(id_xyz)) };
        let text = b"abcxyz";
        unsafe {
            assert_eq!(f_abc(text.as_ptr(), text.len(), 0), 3);
            assert_eq!(f_xyz(text.as_ptr(), text.len(), 3), 6);
            assert_eq!(f_abc(text.as_ptr(), text.len(), 3), -1);
            assert_eq!(f_xyz(text.as_ptr(), text.len(), 0), -1);
        }
    }

    // ----- step 3a refusal cases -----
    //
    // Patterns outside the step 3a subset must be rejected with
    // CodegenUnsupported. The eligibility check (step 2) accepts
    // these but step 3a's narrower scope rejects them; the caller
    // would fall back to the interpreter.

    fn assert_codegen_unsupported(pattern: &str) {
        let program = compile(pattern);
        let mut host = JitHost::new().expect("host construction must succeed");
        let result = compile_program(&program, &mut host);
        match result {
            Err(JitHostError::CodegenUnsupported(_)) => {}
            Err(other) => {
                panic!("pattern `{pattern}` should be CodegenUnsupported but got {other:?}")
            }
            Ok(_) => {
                panic!("pattern `{pattern}` should be CodegenUnsupported but compiled successfully")
            }
        }
    }

    // Step 3b widens the codegen to char classes and simple anchors,
    // so the patterns that step 3a refused (\d, \Aabc) are now
    // accepted. The remaining step3a_refuses_* tests cover patterns
    // step 3b STILL refuses (alternation, quantifiers, captures for
    // groups 1+, multi-byte literals, JIT-ineligible patterns).

    #[test]
    fn step3a_refuses_dot() {
        // Dot (`.`) lowers to AnyDotAll/Any opcodes which involve
        // UTF-8 byte advancement; deferred to step 6.
        assert_codegen_unsupported(".");
    }

    // Note: `step3a_refuses_alternation` was removed at step 3d.2
    // because alternation patterns like `a|b` are now correctly
    // accepted via the Split/Jump/SplitLazy codegen. Positive
    // tests for alternation live in the step 3d.2 section below.

    // Note: `step3a_refuses_quantifier` was removed at step 3e.1
    // because `a+` (PlusGreedy) is now accepted via decoder
    // unfolding into [Char 'a', Split, Jump]. Positive tests for
    // PlusGreedy live in the step 3e.1 section below.

    #[test]
    fn step3a_refuses_capture_group() {
        // Capturing group with explicit group id 1 — group 0 wrappers
        // are accepted, group 1+ require capture trail (step 4).
        assert_codegen_unsupported("(abc)");
    }

    #[test]
    fn step3a_refuses_multibyte_literal() {
        // Non-ASCII literal compiles to a multi-byte Char opcode;
        // step 3a only handles single-byte payloads.
        assert_codegen_unsupported("é");
    }

    #[test]
    fn step3a_refuses_jit_ineligible_pattern_via_eligibility_check() {
        // The is_jit_eligible short-circuit fires first when the
        // pattern is outside the broader JIT subset.
        assert_codegen_unsupported(r"(\w+)\1");
    }

    // ============================================================
    // C1 step 3b — built-in char classes + simple anchors
    // ============================================================

    // ----- Built-in char class opcodes -----

    #[test]
    fn step3b_digit_match() {
        let (_host, func) = jit_compile(r"\d");
        let text = b"5xyz";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(result, 1, "\\d must match digit `5`");
    }

    #[test]
    fn step3b_digit_no_match_alpha() {
        let (_host, func) = jit_compile(r"\d");
        let text = b"x";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(result, -1, "\\d must reject non-digit `x`");
    }

    #[test]
    fn step3b_digit_no_match_empty() {
        let (_host, func) = jit_compile(r"\d");
        let text = b"";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(result, -1, "\\d must reject empty input");
    }

    #[test]
    fn step3b_digit_negated_match_alpha() {
        let (_host, func) = jit_compile(r"\D");
        let text = b"x";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(result, 1, "\\D must match non-digit `x`");
    }

    #[test]
    fn step3b_digit_negated_no_match_digit() {
        let (_host, func) = jit_compile(r"\D");
        let text = b"5";
        let result = unsafe { func(text.as_ptr(), text.len(), 0) };
        assert_eq!(result, -1, "\\D must reject digit `5`");
    }

    #[test]
    fn step3b_word_match_letter() {
        let (_host, func) = jit_compile(r"\w");
        let text = b"x";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_word_match_digit() {
        let (_host, func) = jit_compile(r"\w");
        let text = b"7";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_word_match_underscore() {
        let (_host, func) = jit_compile(r"\w");
        let text = b"_";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_word_no_match_punctuation() {
        let (_host, func) = jit_compile(r"\w");
        let text = b"!";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, -1);
    }

    #[test]
    fn step3b_word_no_match_space() {
        let (_host, func) = jit_compile(r"\w");
        let text = b" ";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, -1);
    }

    #[test]
    fn step3b_word_negated_match_punctuation() {
        let (_host, func) = jit_compile(r"\W");
        let text = b"!";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_word_negated_no_match_letter() {
        let (_host, func) = jit_compile(r"\W");
        let text = b"a";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, -1);
    }

    #[test]
    fn step3b_space_match_space() {
        let (_host, func) = jit_compile(r"\s");
        let text = b" ";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_space_match_tab() {
        let (_host, func) = jit_compile(r"\s");
        let text = b"\t";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_space_match_newline() {
        let (_host, func) = jit_compile(r"\s");
        let text = b"\n";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_space_match_carriage_return() {
        let (_host, func) = jit_compile(r"\s");
        let text = b"\r";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_space_match_form_feed() {
        let (_host, func) = jit_compile(r"\s");
        let text = b"\x0c";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_space_match_vertical_tab() {
        let (_host, func) = jit_compile(r"\s");
        let text = b"\x0b";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_space_no_match_letter() {
        let (_host, func) = jit_compile(r"\s");
        let text = b"x";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, -1);
    }

    #[test]
    fn step3b_space_negated_match_letter() {
        let (_host, func) = jit_compile(r"\S");
        let text = b"x";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, 1);
    }

    #[test]
    fn step3b_space_negated_no_match_space() {
        let (_host, func) = jit_compile(r"\S");
        let text = b" ";
        assert_eq!(unsafe { func(text.as_ptr(), text.len(), 0) }, -1);
    }

    // ----- Combinations: literals + char classes -----

    #[test]
    fn step3b_digit_then_literal() {
        // \dx — digit followed by literal `x`
        let (_host, func) = jit_compile(r"\dx");
        unsafe {
            let yes = b"5xy";
            assert_eq!(func(yes.as_ptr(), yes.len(), 0), 2);
            let no_first = b"ax";
            assert_eq!(func(no_first.as_ptr(), no_first.len(), 0), -1);
            let no_second = b"5y";
            assert_eq!(func(no_second.as_ptr(), no_second.len(), 0), -1);
        }
    }

    #[test]
    fn step3b_digit_digit_dash_digit_digit() {
        // \d\d-\d\d — common timestamp shape, fully linear
        let (_host, func) = jit_compile(r"\d\d-\d\d");
        unsafe {
            let yes = b"12-34abc";
            assert_eq!(func(yes.as_ptr(), yes.len(), 0), 5);
            let no = b"1a-34";
            assert_eq!(func(no.as_ptr(), no.len(), 0), -1);
        }
    }

    #[test]
    fn step3b_word_word_word() {
        // \w\w\w — three word characters
        let (_host, func) = jit_compile(r"\w\w\w");
        unsafe {
            let yes = b"abc";
            assert_eq!(func(yes.as_ptr(), yes.len(), 0), 3);
            let yes_mixed = b"a1_xyz";
            assert_eq!(func(yes_mixed.as_ptr(), yes_mixed.len(), 0), 3);
            let no = b"a!c";
            assert_eq!(func(no.as_ptr(), no.len(), 0), -1);
        }
    }

    // ----- Anchors: \A and \z -----

    #[test]
    fn step3b_anchor_start_text_at_pos_zero() {
        // \Aabc — only matches at the very start of the input
        let (_host, func) = jit_compile(r"\Aabc");
        unsafe {
            let text = b"abcdef";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 3);
        }
    }

    #[test]
    fn step3b_anchor_start_text_at_offset_no_match() {
        let (_host, func) = jit_compile(r"\Aabc");
        unsafe {
            // Same text but starting at pos 3 — \A wants pos == 0,
            // so the anchor fails even though `abc` would otherwise
            // match at this position in `xyzabcdef`.
            let text = b"xyzabcdef";
            assert_eq!(func(text.as_ptr(), text.len(), 3), -1);
        }
    }

    #[test]
    fn step3b_anchor_end_text_at_text_end() {
        // abc\z — only matches when the literal ends exactly at
        // text_len.
        let (_host, func) = jit_compile(r"abc\z");
        unsafe {
            let text = b"xyzabc";
            assert_eq!(func(text.as_ptr(), text.len(), 3), 6);
        }
    }

    #[test]
    fn step3b_anchor_end_text_with_trailing_no_match() {
        let (_host, func) = jit_compile(r"abc\z");
        unsafe {
            // `abc` matches but is not at the end of the input.
            let text = b"abcdef";
            assert_eq!(func(text.as_ptr(), text.len(), 0), -1);
        }
    }

    #[test]
    fn step3b_anchor_start_and_end_full_match() {
        // \Aabc\z — matches iff the input is exactly "abc"
        let (_host, func) = jit_compile(r"\Aabc\z");
        unsafe {
            let exact = b"abc";
            assert_eq!(func(exact.as_ptr(), exact.len(), 0), 3);
            let too_long = b"abcd";
            assert_eq!(func(too_long.as_ptr(), too_long.len(), 0), -1);
            let too_short = b"ab";
            assert_eq!(func(too_short.as_ptr(), too_short.len(), 0), -1);
        }
    }

    // ----- Step 3b/3c refusal cases -----
    //
    // Patterns that the eligibility check (step 2) accepts but
    // the current step's narrower codegen still refuses. Each must
    // return CodegenUnsupported so the engine layer (step 5+) falls
    // back to the interpreter.
    //
    // Note: word boundaries (\b / \B) were originally refused at
    // step 3b but are now accepted at step 3c via the runtime
    // helper. The corresponding refusal tests are gone; positive
    // tests live in the step 3c section below.

    #[test]
    fn step3b_caret_lowers_to_start_text_in_non_multiline_mode() {
        // In non-multiline (PCRE2 default) mode, `^` is equivalent
        // to `\A` and lowers to the StartText opcode — which step 3b
        // accepts. Verify the behaviour matches `\Aabc` exactly.
        let (_host, func) = jit_compile("^abc");
        unsafe {
            let text = b"abcdef";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 3);
            // At an offset, `^` (= `\A`) refuses because it requires
            // pos == 0.
            let with_prefix = b"xyzabcdef";
            assert_eq!(func(with_prefix.as_ptr(), with_prefix.len(), 3), -1);
        }
    }

    #[test]
    fn step3b_refuses_end_line_anchor() {
        // `$` in non-multiline mode lowers to EndLine (which has
        // newline-aware semantics at the end of input) — distinct
        // from `\z`. Step 3b refuses; deferred to a later step.
        assert_codegen_unsupported("abc$");
    }

    #[test]
    fn step3b_refuses_end_text_or_nl() {
        // \Z (EndTextOrNL) needs newline detection; deferred.
        assert_codegen_unsupported(r"abc\Z");
    }

    // ============================================================
    // C1 step 3c — word boundaries via runtime helper
    // ============================================================
    //
    // Each test JIT-compiles a pattern that uses `\b` or `\B` and
    // verifies the JIT'd function calls into the runtime helper
    // correctly. The helper itself is unit-tested directly in
    // `c1::runtime::tests`; these tests verify the indirect call
    // codegen, the symbol registration in `JitHost::new`, and the
    // pass-through-pos-on-success contract for zero-width opcodes.

    #[test]
    fn step3c_word_boundary_at_start_of_text() {
        // \bword — \b at pos 0 with word char following → boundary
        let (_host, func) = jit_compile(r"\bword");
        unsafe {
            let text = b"word";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 4);
        }
    }

    #[test]
    fn step3c_word_boundary_at_offset_after_space() {
        // \bword starting at pos 4 of "abc word" — boundary because
        // pos 3 is a space and pos 4 is `w`.
        let (_host, func) = jit_compile(r"\bword");
        unsafe {
            let text = b"abc word";
            assert_eq!(func(text.as_ptr(), text.len(), 4), 8);
        }
    }

    #[test]
    fn step3c_word_boundary_no_boundary_in_middle() {
        // \bword starting at pos 1 of "aword" — no boundary because
        // pos 0 is `a` (word char) and pos 1 is `w` (word char).
        let (_host, func) = jit_compile(r"\bword");
        unsafe {
            let text = b"aword";
            assert_eq!(func(text.as_ptr(), text.len(), 1), -1);
        }
    }

    #[test]
    fn step3c_word_boundary_at_end_of_text() {
        // word\b — \b after the literal at end of text → boundary
        let (_host, func) = jit_compile(r"word\b");
        unsafe {
            let text = b"word";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 4);
        }
    }

    #[test]
    fn step3c_word_boundary_in_middle_of_text() {
        // word\b followed by a non-word char
        let (_host, func) = jit_compile(r"word\b");
        unsafe {
            let text = b"word ";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 4);
        }
    }

    #[test]
    fn step3c_word_boundary_no_boundary_followed_by_word() {
        // word\b followed by another word char → no boundary at end
        let (_host, func) = jit_compile(r"word\b");
        unsafe {
            let text = b"words";
            assert_eq!(func(text.as_ptr(), text.len(), 0), -1);
        }
    }

    #[test]
    fn step3c_word_boundary_both_sides_full_match() {
        // \bword\b — both anchored
        let (_host, func) = jit_compile(r"\bword\b");
        unsafe {
            assert_eq!(func(b"word".as_ptr(), 4, 0), 4);
            assert_eq!(func(b" word ".as_ptr(), 6, 1), 5);
            // Surrounded by word chars on both sides → no match
            assert_eq!(func(b"awordb".as_ptr(), 6, 1), -1);
        }
    }

    #[test]
    fn step3c_non_word_boundary_between_word_chars() {
        // \Bword starting at pos 1 of "aword" — \B fires because
        // pos 0 (a) and pos 1 (w) are both word chars.
        let (_host, func) = jit_compile(r"\Bword");
        unsafe {
            let text = b"aword";
            assert_eq!(func(text.as_ptr(), text.len(), 1), 5);
        }
    }

    #[test]
    fn step3c_non_word_boundary_refuses_at_actual_boundary() {
        // \Bword at pos 0 of "word" — \B fails because pos 0 is a
        // real word boundary.
        let (_host, func) = jit_compile(r"\Bword");
        unsafe {
            let text = b"word";
            assert_eq!(func(text.as_ptr(), text.len(), 0), -1);
        }
    }

    #[test]
    fn step3c_word_boundary_with_digit() {
        // \b123 starting at the start of "123" — `1` is a word
        // char (digit), pos 0 has no preceding char, so \b fires.
        let (_host, func) = jit_compile(r"\b123");
        unsafe {
            let text = b"123";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 3);
        }
    }

    #[test]
    fn step3c_word_boundary_with_underscore() {
        // _ is a word char, so `\b_x` should match at start of text
        let (_host, func) = jit_compile(r"\b_x");
        unsafe {
            let text = b"_x";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 2);
        }
    }

    #[test]
    fn step3c_word_boundary_with_char_class() {
        // \b\d+\b doesn't compile (it has a quantifier) — instead
        // verify \b\d works at the start of "5"
        let (_host, func) = jit_compile(r"\b\d");
        unsafe {
            let text = b"5";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 1);
        }
    }

    // ============================================================
    // C1 step 3d.2 — control flow + backtracking
    // ============================================================
    //
    // These tests JIT-compile patterns that use Split / Jump /
    // SplitLazy (alternations and quantifiers that the existing
    // compiler emits as control-flow opcodes rather than the
    // optimized quantifier opcodes) and verify the backtracking
    // dispatch correctly handles failed first-branch attempts.

    #[test]
    fn step3d_simple_alternation_first_branch_matches() {
        // cat|dog — try `cat` first, on failure backtrack to `dog`.
        let (_host, func) = jit_compile("cat|dog");
        unsafe {
            let text = b"cat";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 3);
        }
    }

    #[test]
    fn step3d_simple_alternation_second_branch_matches() {
        // cat|dog against "dog" — first branch fails, backtrack
        // pops the saved frame and dispatches to the `dog` branch.
        let (_host, func) = jit_compile("cat|dog");
        unsafe {
            let text = b"dog";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 3);
        }
    }

    #[test]
    fn step3d_simple_alternation_neither_matches() {
        let (_host, func) = jit_compile("cat|dog");
        unsafe {
            let text = b"bird";
            assert_eq!(func(text.as_ptr(), text.len(), 0), -1);
        }
    }

    #[test]
    fn step3d_three_branch_alternation() {
        // cat|dog|bird — three branches, two backtrack frames
        // possible. Each branch should match its own input.
        let (_host, func) = jit_compile("cat|dog|bird");
        unsafe {
            assert_eq!(func(b"cat".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"dog".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"bird".as_ptr(), 4, 0), 4);
            assert_eq!(func(b"fish".as_ptr(), 4, 0), -1);
        }
    }

    #[test]
    fn step3d_alternation_with_char_classes() {
        // \d|\w — try digit first (the more specific class), on
        // failure try word char (the broader class). For `5`,
        // both branches match — first one wins.
        let (_host, func) = jit_compile(r"\d|\w");
        unsafe {
            // `5` is both a digit and a word char — first branch
            // (\d) matches.
            assert_eq!(func(b"5".as_ptr(), 1, 0), 1);
            // `a` is a word char but not a digit — first branch
            // fails, backtrack to second branch which matches.
            assert_eq!(func(b"a".as_ptr(), 1, 0), 1);
            // `!` is neither — both branches fail.
            assert_eq!(func(b"!".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3d_alternation_with_anchored_branches() {
        // \Acat|\Adog — both branches anchored. Only matches at
        // the start of text.
        let (_host, func) = jit_compile(r"\Acat|\Adog");
        unsafe {
            assert_eq!(func(b"cat".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"dog".as_ptr(), 3, 0), 3);
            // Starting at offset, \A fails for both branches.
            assert_eq!(func(b"xcat".as_ptr(), 4, 1), -1);
        }
    }

    #[test]
    fn step3d_alternation_with_overlap_first_wins() {
        // ab|abc — `ab` and `abc` both match `abc`, but the first
        // branch is tried first. PCRE2 uses leftmost-first
        // semantics: the first matching branch wins.
        let (_host, func) = jit_compile("ab|abc");
        unsafe {
            // First branch (`ab`) matches and returns 2 — even
            // though the second branch could also match and
            // return 3. PCRE2 leftmost-first semantics.
            assert_eq!(func(b"abc".as_ptr(), 3, 0), 2);
        }
    }

    #[test]
    fn step3d_alternation_with_partial_first_match() {
        // ab|c — `ab` partially matches `ac` (consumes `a`, fails
        // on `c`). Backtrack to second branch `c` which fails at
        // pos 0. Result: -1.
        let (_host, func) = jit_compile("ab|c");
        unsafe {
            // `ab` against `abc` matches first branch.
            assert_eq!(func(b"abc".as_ptr(), 3, 0), 2);
            // `c` against `c` matches second branch.
            assert_eq!(func(b"c".as_ptr(), 1, 0), 1);
            // `ac` partially matches first branch (consumes `a`,
            // then `b` fails), backtracks to second branch (`c`)
            // at pos 0 (the saved pos), `c` doesn't match `a`,
            // so the whole pattern fails.
            assert_eq!(func(b"ac".as_ptr(), 2, 0), -1);
        }
    }

    #[test]
    fn step3d_alternation_position_restored_on_backtrack() {
        // The key test for backtrack-pos-restoration: a pattern
        // where the first branch consumes some bytes before
        // failing, so the second branch must see the ORIGINAL
        // position (not the advanced one).
        // \dxy|\dab — first tries `\dxy`, second tries `\dab`.
        let (_host, func) = jit_compile(r"\dxy|\dab");
        unsafe {
            // `5xy...` matches first branch
            assert_eq!(func(b"5xy".as_ptr(), 3, 0), 3);
            // `5ab...` — first branch consumes `5`, then `x` !=
            // `a` fails. Backtrack restores pos to 0 (NOT 1) and
            // dispatches to second branch, which then matches.
            assert_eq!(func(b"5ab".as_ptr(), 3, 0), 3);
            // `5cd...` — both branches fail, return -1.
            assert_eq!(func(b"5cd".as_ptr(), 3, 0), -1);
        }
    }

    #[test]
    fn step3d_nested_alternation_via_non_capturing_group() {
        // (?:cat|dog)|bird — non-capturing group keeps the pattern
        // step 3-eligible (group 1+ would require step 4 capture
        // trail). Tests that nested Split structures backtrack
        // correctly.
        let (_host, func) = jit_compile("(?:cat|dog)|bird");
        unsafe {
            assert_eq!(func(b"cat".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"dog".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"bird".as_ptr(), 4, 0), 4);
            assert_eq!(func(b"fish".as_ptr(), 4, 0), -1);
        }
    }

    // ============================================================
    // C1 step 3e.1 — PlusGreedy via decoder unfolding
    // ============================================================
    //
    // PlusGreedy(inner) lowers to [inner_jit_ops..., Split, Jump]
    // where the first iteration of inner is mandatory and the
    // Split-based loop handles 2nd+ iterations with greedy
    // backtracking. Inner restricted to "simple linear" subset
    // (no nested control flow); nested optimized quantifiers will
    // land in a later step.

    #[test]
    fn step3e1_plus_greedy_single_char_match_one() {
        // a+ against `a` — exactly one iteration
        let (_host, func) = jit_compile("a+");
        unsafe {
            let text = b"a";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_single_char_match_many() {
        // a+ against `aaaa` — four iterations
        let (_host, func) = jit_compile("a+");
        unsafe {
            let text = b"aaaa";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 4);
        }
    }

    #[test]
    fn step3e1_plus_greedy_single_char_no_match() {
        // a+ against `bbb` — first (mandatory) iteration fails
        let (_host, func) = jit_compile("a+");
        unsafe {
            let text = b"bbb";
            assert_eq!(func(text.as_ptr(), text.len(), 0), -1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_single_char_partial() {
        // a+ against `aab` — consume `aa`, stop at `b`
        let (_host, func) = jit_compile("a+");
        unsafe {
            let text = b"aab";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 2);
        }
    }

    #[test]
    fn step3e1_plus_greedy_followed_by_literal() {
        // a+b — must match a's then a b. Greedy `a+` consumes
        // all leading a's then `b` matches.
        let (_host, func) = jit_compile("a+b");
        unsafe {
            let text = b"aaab";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 4);
            // No b at the end → greedy a+ consumes all, then b
            // fails, backtracks all the way, fails entirely.
            let text2 = b"aaaa";
            assert_eq!(func(text2.as_ptr(), text2.len(), 0), -1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_backtrack_to_satisfy_following_op() {
        // a+a — `a+` consumes greedily then `a` must match. The
        // greedy a+ over-consumes (eats all the a's), then `a`
        // fails (no more chars), backtracks one iteration so the
        // last a matches.
        let (_host, func) = jit_compile("a+a");
        unsafe {
            // `aa` — a+ consumes both; backtrack to a+ consuming
            // 1, then `a` matches the second.
            assert_eq!(func(b"aa".as_ptr(), 2, 0), 2);
            // `aaa` — a+ consumes 3, backtrack to 2, `a` matches.
            assert_eq!(func(b"aaa".as_ptr(), 3, 0), 3);
            // `a` alone — a+ consumes 1, then `a` fails (eof),
            // backtrack would shrink to 0 iterations but `a+`
            // requires at least 1 → fail.
            assert_eq!(func(b"a".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_digit() {
        // \d+ matches digit sequence
        let (_host, func) = jit_compile(r"\d+");
        unsafe {
            assert_eq!(func(b"42".as_ptr(), 2, 0), 2);
            assert_eq!(func(b"123abc".as_ptr(), 6, 0), 3);
            assert_eq!(func(b"abc".as_ptr(), 3, 0), -1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_word() {
        // \w+ matches word-character sequence
        let (_host, func) = jit_compile(r"\w+");
        unsafe {
            assert_eq!(func(b"hello".as_ptr(), 5, 0), 5);
            assert_eq!(func(b"hello world".as_ptr(), 11, 0), 5);
            assert_eq!(func(b"!".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_space() {
        // \s+ matches whitespace
        let (_host, func) = jit_compile(r"\s+");
        unsafe {
            assert_eq!(func(b"   ".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"\t \n".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"abc".as_ptr(), 3, 0), -1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_multi_char_inner() {
        // (?:ab)+ — repeated two-char sequence. The inner has 2
        // ops; this exercises the multi-op simple-inner path.
        let (_host, func) = jit_compile("(?:ab)+");
        unsafe {
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
            assert_eq!(func(b"abab".as_ptr(), 4, 0), 4);
            assert_eq!(func(b"ababab".as_ptr(), 6, 0), 6);
            // `abc` — first iteration `ab` matches, second
            // iteration starts at pos 2 with `c`, fails on `a`
            // mismatch. Result: pos=2.
            assert_eq!(func(b"abc".as_ptr(), 3, 0), 2);
            // `aba` — first iteration `ab` matches, second
            // iteration starts at pos 2 with `a`, then needs `b`
            // at pos 3 → eof, fails. Backtrack: but at this
            // point bt_top has 1 frame from after the first
            // successful iteration, pop it → exit at pos=2.
            assert_eq!(func(b"aba".as_ptr(), 3, 0), 2);
        }
    }

    #[test]
    fn step3e1_plus_greedy_anchored() {
        // \A\d+\z — anchored digit sequence (whole input must be digits)
        let (_host, func) = jit_compile(r"\A\d+\z");
        unsafe {
            assert_eq!(func(b"123".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
            assert_eq!(func(b"123abc".as_ptr(), 6, 0), -1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_email_like() {
        // \w+@\w+\.\w+ — email-like pattern with three quantifiers
        let (_host, func) = jit_compile(r"\w+@\w+\.\w+");
        unsafe {
            let text = b"user@example.com";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 16);
            let text2 = b"abc";
            assert_eq!(func(text2.as_ptr(), text2.len(), 0), -1);
        }
    }

    #[test]
    fn step3e1_plus_greedy_with_alternation() {
        // \d+|word — quantifier in alternation
        let (_host, func) = jit_compile(r"\d+|word");
        unsafe {
            assert_eq!(func(b"123".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"word".as_ptr(), 4, 0), 4);
            assert_eq!(func(b"xxx".as_ptr(), 3, 0), -1);
        }
    }

    // ============================================================
    // C1 step 3e.2 — StarGreedy via decoder unfolding
    // ============================================================
    //
    // StarGreedy(inner) lowers to [Split{exit}, inner_jit_ops...,
    // Jump{back to Split}]. The Split sits BEFORE the inner so
    // zero iterations is a valid match — on the very first visit,
    // the Split pushes (exit, current_pos), and if the inner
    // immediately fails, failure_dispatch pops the frame and
    // exits at the saved (=current) pos. For non-zero iterations,
    // each successful inner pass jumps back to the Split which
    // pushes another frame. Greedy backtracking shrinks toward
    // zero iterations.

    #[test]
    fn step3e2_star_greedy_zero_iterations_match_empty() {
        // a* against `bbb` — zero iterations is a valid match
        let (_host, func) = jit_compile("a*");
        unsafe {
            let text = b"bbb";
            // Match span (0, 0): zero iterations of `a` is fine.
            assert_eq!(func(text.as_ptr(), text.len(), 0), 0);
        }
    }

    #[test]
    fn step3e2_star_greedy_zero_iterations_empty_input() {
        let (_host, func) = jit_compile("a*");
        unsafe {
            let text = b"";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 0);
        }
    }

    #[test]
    fn step3e2_star_greedy_one_iteration() {
        // a* against `a` — exactly one iteration
        let (_host, func) = jit_compile("a*");
        unsafe {
            let text = b"a";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 1);
        }
    }

    #[test]
    fn step3e2_star_greedy_many_iterations() {
        let (_host, func) = jit_compile("a*");
        unsafe {
            let text = b"aaaaa";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 5);
        }
    }

    #[test]
    fn step3e2_star_greedy_partial() {
        let (_host, func) = jit_compile("a*");
        unsafe {
            let text = b"aaab";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 3);
        }
    }

    #[test]
    fn step3e2_star_greedy_followed_by_literal() {
        // a*b — `a*` greedily consumes a's, then `b` matches
        let (_host, func) = jit_compile("a*b");
        unsafe {
            // Zero a's then b
            assert_eq!(func(b"b".as_ptr(), 1, 0), 1);
            // Three a's then b
            assert_eq!(func(b"aaab".as_ptr(), 4, 0), 4);
            // No b in input → fails after backtracking through all
            // possible `a*` lengths down to zero.
            assert_eq!(func(b"aaa".as_ptr(), 3, 0), -1);
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
        }
    }

    #[test]
    fn step3e2_star_greedy_backtrack_to_satisfy_following_op() {
        // a*a — `a*` greedily over-consumes (eats all the a's),
        // then `a` fails (no more chars), backtracks one iteration
        // so the trailing `a` matches. Unlike `a+a`, this should
        // succeed even on a single `a` because `a*` can consume
        // zero and the trailing `a` matches the only a.
        let (_host, func) = jit_compile("a*a");
        unsafe {
            // Single `a` — `a*` consumes 0, then `a` matches.
            assert_eq!(func(b"a".as_ptr(), 1, 0), 1);
            // Two `a`s — `a*` consumes 1, then `a` matches.
            assert_eq!(func(b"aa".as_ptr(), 2, 0), 2);
            // Three `a`s — `a*` consumes 2, then `a` matches.
            assert_eq!(func(b"aaa".as_ptr(), 3, 0), 3);
            // No `a` at all — `a*` consumes 0, then `a` fails (eof
            // or wrong byte) → no match.
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
            assert_eq!(func(b"b".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e2_star_greedy_digit() {
        let (_host, func) = jit_compile(r"\d*");
        unsafe {
            assert_eq!(func(b"42".as_ptr(), 2, 0), 2);
            // Zero iterations OK
            assert_eq!(func(b"abc".as_ptr(), 3, 0), 0);
            assert_eq!(func(b"".as_ptr(), 0, 0), 0);
        }
    }

    #[test]
    fn step3e2_star_greedy_word() {
        let (_host, func) = jit_compile(r"\w*");
        unsafe {
            assert_eq!(func(b"hello".as_ptr(), 5, 0), 5);
            assert_eq!(func(b"!".as_ptr(), 1, 0), 0);
        }
    }

    #[test]
    fn step3e2_star_greedy_space() {
        let (_host, func) = jit_compile(r"\s*");
        unsafe {
            assert_eq!(func(b"   ".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"abc".as_ptr(), 3, 0), 0);
        }
    }

    #[test]
    fn step3e2_star_greedy_multi_char_inner() {
        // (?:ab)* — repeated two-char sequence with zero allowed
        let (_host, func) = jit_compile("(?:ab)*");
        unsafe {
            assert_eq!(func(b"".as_ptr(), 0, 0), 0);
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
            assert_eq!(func(b"abab".as_ptr(), 4, 0), 4);
            // Partial: `aba` — first iteration `ab` matches at 0..2,
            // second iteration starts at pos 2 with `a`, then needs
            // `b` at pos 3 → eof, fails. Backtrack: pop the frame
            // pushed by the second Split visit (at pos 2). Exit at
            // pos 2.
            assert_eq!(func(b"aba".as_ptr(), 3, 0), 2);
            // No match at all → zero iterations
            assert_eq!(func(b"xyz".as_ptr(), 3, 0), 0);
        }
    }

    #[test]
    fn step3e2_star_greedy_anchored() {
        // \A\d*\z — anchored star: input must be all digits (or empty)
        let (_host, func) = jit_compile(r"\A\d*\z");
        unsafe {
            assert_eq!(func(b"123".as_ptr(), 3, 0), 3);
            // Empty input matches
            assert_eq!(func(b"".as_ptr(), 0, 0), 0);
            // Mixed input fails
            assert_eq!(func(b"123abc".as_ptr(), 6, 0), -1);
        }
    }

    #[test]
    fn step3e2_star_greedy_with_alternation() {
        // \d*|word — first branch matches anything (incl. empty),
        // so it always wins. The pattern matches everything at
        // any pos.
        let (_host, func) = jit_compile(r"\d*|word");
        unsafe {
            assert_eq!(func(b"123".as_ptr(), 3, 0), 3);
            // First branch matches empty at pos 0 (zero iterations).
            assert_eq!(func(b"abc".as_ptr(), 3, 0), 0);
        }
    }

    #[test]
    fn step3e2_star_followed_by_plus() {
        // a*b+ — combined Star and Plus quantifiers
        let (_host, func) = jit_compile("a*b+");
        unsafe {
            // Zero a's, one b
            assert_eq!(func(b"b".as_ptr(), 1, 0), 1);
            // Three a's, two b's
            assert_eq!(func(b"aaabb".as_ptr(), 5, 0), 5);
            // Zero a's, three b's
            assert_eq!(func(b"bbb".as_ptr(), 3, 0), 3);
            // No b at all → fails
            assert_eq!(func(b"aaa".as_ptr(), 3, 0), -1);
        }
    }

    // ============================================================
    // C1 step 3e.3 — QuestionGreedy via decoder unfolding
    // ============================================================
    //
    // QuestionGreedy(inner) lowers to [Split{exit}, inner_jit_ops...].
    // The simplest of the optimized quantifier lowerings: a Split
    // followed by the inner, with NO Jump back. `?` is "zero or
    // one" so there's no loop. The Split pushes (exit, current_pos)
    // and falls through to the inner. If the inner succeeds, it
    // falls through to the next outer op (= exit) via the
    // per-op-block sequence. If the inner fails, failure_dispatch
    // pops the frame and dispatches to exit at the saved pos.

    #[test]
    fn step3e3_question_greedy_zero_match() {
        // a? against `b` — zero iterations is a valid match
        let (_host, func) = jit_compile("a?");
        unsafe {
            let text = b"b";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 0);
        }
    }

    #[test]
    fn step3e3_question_greedy_zero_match_empty_input() {
        let (_host, func) = jit_compile("a?");
        unsafe {
            let text = b"";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 0);
        }
    }

    #[test]
    fn step3e3_question_greedy_one_match() {
        // a? against `a` — one iteration
        let (_host, func) = jit_compile("a?");
        unsafe {
            let text = b"a";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 1);
        }
    }

    #[test]
    fn step3e3_question_greedy_one_match_then_more() {
        // a? against `aaa` — greedy: takes the one a, returns 1
        let (_host, func) = jit_compile("a?");
        unsafe {
            let text = b"aaa";
            assert_eq!(func(text.as_ptr(), text.len(), 0), 1);
        }
    }

    #[test]
    fn step3e3_question_greedy_followed_by_literal_match() {
        // a?b — `a?` matches zero or one a, then `b` matches
        let (_host, func) = jit_compile("a?b");
        unsafe {
            // `b` alone → zero a's, then b
            assert_eq!(func(b"b".as_ptr(), 1, 0), 1);
            // `ab` → one a, then b
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
        }
    }

    #[test]
    fn step3e3_question_greedy_followed_by_literal_no_match() {
        // a?b against input with no b → fails
        let (_host, func) = jit_compile("a?b");
        unsafe {
            assert_eq!(func(b"a".as_ptr(), 1, 0), -1);
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
            assert_eq!(func(b"x".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e3_question_greedy_backtrack() {
        // a?a — `a?` greedily takes the a, then `a` needs another
        // a. If only one a in input, `a?` greedily takes it,
        // trailing `a` fails (eof), backtrack `a?` to zero a's,
        // trailing `a` matches the only a.
        let (_host, func) = jit_compile("a?a");
        unsafe {
            // Single `a` — backtrack from `a?=1` to `a?=0` so
            // trailing `a` matches.
            assert_eq!(func(b"a".as_ptr(), 1, 0), 1);
            // Two `a`s — `a?` takes one, trailing `a` matches.
            assert_eq!(func(b"aa".as_ptr(), 2, 0), 2);
            // Empty → fails (no a to match the trailing `a`).
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
            // Wrong char → fails.
            assert_eq!(func(b"b".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e3_question_greedy_digit() {
        let (_host, func) = jit_compile(r"\d?");
        unsafe {
            assert_eq!(func(b"5".as_ptr(), 1, 0), 1);
            assert_eq!(func(b"a".as_ptr(), 1, 0), 0);
            assert_eq!(func(b"".as_ptr(), 0, 0), 0);
        }
    }

    #[test]
    fn step3e3_question_greedy_word() {
        let (_host, func) = jit_compile(r"\w?");
        unsafe {
            assert_eq!(func(b"x".as_ptr(), 1, 0), 1);
            assert_eq!(func(b"!".as_ptr(), 1, 0), 0);
        }
    }

    #[test]
    fn step3e3_question_greedy_multi_char_inner() {
        // (?:ab)? — optional two-char sequence
        let (_host, func) = jit_compile("(?:ab)?");
        unsafe {
            // No `ab` at all → zero iterations
            assert_eq!(func(b"".as_ptr(), 0, 0), 0);
            assert_eq!(func(b"xyz".as_ptr(), 3, 0), 0);
            // `ab` at start → one iteration
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
            assert_eq!(func(b"abxyz".as_ptr(), 5, 0), 2);
            // Just `a` → inner fails on the missing `b`,
            // backtrack to zero iterations.
            assert_eq!(func(b"a".as_ptr(), 1, 0), 0);
        }
    }

    #[test]
    fn step3e3_question_greedy_anchored() {
        // \Aa?\z — anchored: input must be empty or exactly `a`
        let (_host, func) = jit_compile(r"\Aa?\z");
        unsafe {
            assert_eq!(func(b"".as_ptr(), 0, 0), 0);
            assert_eq!(func(b"a".as_ptr(), 1, 0), 1);
            // `aa` fails: `a?` matches one, `\z` fails because
            // there's another `a`. Backtrack to zero iterations,
            // `\z` still fails at pos 0. Result: -1.
            assert_eq!(func(b"aa".as_ptr(), 2, 0), -1);
            assert_eq!(func(b"b".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e3_question_greedy_combined_with_plus() {
        // a?b+ — optional a then one or more b's
        let (_host, func) = jit_compile("a?b+");
        unsafe {
            assert_eq!(func(b"b".as_ptr(), 1, 0), 1);
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
            assert_eq!(func(b"abbb".as_ptr(), 4, 0), 4);
            assert_eq!(func(b"bbb".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"a".as_ptr(), 1, 0), -1);
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
        }
    }

    // ============================================================
    // C1 step 3e.4 — lazy quantifier variants
    // ============================================================
    //
    // Lazy quantifiers (`??`, `*?`, `+?`) match as FEW iterations
    // as possible while still allowing the rest of the pattern to
    // match. The lowering uses SplitLazy instead of Split, which
    // swaps the branch ordering: try exit first (zero/minimum
    // iterations), and on backtrack take one more iteration.
    //
    // The most informative tests are the ones where greedy and
    // lazy produce DIFFERENT results — pure standalone tests like
    // `a*?` against `aaa` would return 0 (zero iterations is the
    // lazy minimum) whereas `a*` against `aaa` returns 3.

    // ----- QuestionLazy `??` -----

    #[test]
    fn step3e4_question_lazy_zero_match_when_standalone() {
        // a?? against `a` — lazy `?` prefers zero iterations.
        // Standalone (no following op) the zero-iteration choice
        // wins immediately because there's nothing for the
        // backtracking to satisfy.
        let (_host, func) = jit_compile("a??");
        unsafe {
            // Returns 0 (zero iterations) NOT 1 (greedy would
            // return 1 here).
            assert_eq!(func(b"a".as_ptr(), 1, 0), 0);
        }
    }

    #[test]
    fn step3e4_question_lazy_one_match_when_required() {
        // a??a — lazy `?` prefers zero, but the following `a`
        // needs the `a?` to match if there's only one `a` to
        // share. Wait — for single `a`, lazy `a??` matches zero,
        // then trailing `a` matches the only `a`. So result is 1.
        // For two `a`s, lazy matches zero, trailing matches one
        // → result 1.
        let (_host, func) = jit_compile("a??a");
        unsafe {
            // Single `a` → lazy `a??` zero, then `a` matches → 1
            assert_eq!(func(b"a".as_ptr(), 1, 0), 1);
            // Two `a`s → lazy `a??` zero, then `a` matches first → 1
            // (NOT 2 — lazy stays at minimum even when more is possible)
            assert_eq!(func(b"aa".as_ptr(), 2, 0), 1);
            // No `a`s → fails
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
            assert_eq!(func(b"b".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e4_question_lazy_followed_by_b() {
        // a??b — lazy `a?` then `b`. For `b`, zero a's then b →
        // returns 1. For `ab`, zero a's then `b` doesn't match
        // (char is `a`), backtrack to one `a`, then `b` matches.
        let (_host, func) = jit_compile("a??b");
        unsafe {
            // `b` alone → zero a's + b → 1
            assert_eq!(func(b"b".as_ptr(), 1, 0), 1);
            // `ab` → zero a's first, but then b at pos 0 fails
            // (char is `a`), backtrack to one a, then b at pos 1
            // matches → 2
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
            // No b at all → fails
            assert_eq!(func(b"a".as_ptr(), 1, 0), -1);
        }
    }

    // ----- StarLazy `*?` -----

    #[test]
    fn step3e4_star_lazy_zero_match_when_standalone() {
        // a*? against `aaa` — standalone lazy star prefers zero.
        let (_host, func) = jit_compile("a*?");
        unsafe {
            // Returns 0 (zero iterations) NOT 3.
            assert_eq!(func(b"aaa".as_ptr(), 3, 0), 0);
            assert_eq!(func(b"".as_ptr(), 0, 0), 0);
            assert_eq!(func(b"bbb".as_ptr(), 3, 0), 0);
        }
    }

    #[test]
    fn step3e4_star_lazy_minimum_to_satisfy_following() {
        // a*?b — lazy star matches the minimum number of a's
        // needed to allow b to match. For `aab`, that's 2.
        let (_host, func) = jit_compile("a*?b");
        unsafe {
            // `b` → zero a's + b → 1
            assert_eq!(func(b"b".as_ptr(), 1, 0), 1);
            // `ab` → zero a's, b at pos 0 fails, one a, b at pos 1 matches → 2
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
            // `aab` → zero a's then b fails, one a then b fails
            // (char is `a`), two a's then b matches → 3
            assert_eq!(func(b"aab".as_ptr(), 3, 0), 3);
            // `aaab` → similar, three a's → 4
            assert_eq!(func(b"aaab".as_ptr(), 4, 0), 4);
            // No b → fails
            assert_eq!(func(b"aaa".as_ptr(), 3, 0), -1);
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
        }
    }

    #[test]
    fn step3e4_star_lazy_digit() {
        // \d*? against `123` standalone — zero iterations
        let (_host, func) = jit_compile(r"\d*?");
        unsafe {
            assert_eq!(func(b"123".as_ptr(), 3, 0), 0);
        }
    }

    #[test]
    fn step3e4_star_lazy_multi_char_inner() {
        // (?:ab)*?ab — lazy multi-char inner; lazy prefers zero
        // iterations of `ab`, but then needs the trailing `ab`.
        let (_host, func) = jit_compile("(?:ab)*?ab");
        unsafe {
            // `ab` → zero iterations of `(?:ab)*?` then trailing
            // `ab` matches → 2
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
            // `abab` → zero iterations would leave trailing `ab`
            // matching the first `ab` → 2 (lazy: minimum first)
            assert_eq!(func(b"abab".as_ptr(), 4, 0), 2);
        }
    }

    // ----- PlusLazy `+?` -----

    #[test]
    fn step3e4_plus_lazy_one_match_when_standalone() {
        // a+? against `aaa` — lazy `+` matches the minimum (one).
        let (_host, func) = jit_compile("a+?");
        unsafe {
            // Returns 1 (the minimum for `+`) NOT 3 (greedy)
            assert_eq!(func(b"aaa".as_ptr(), 3, 0), 1);
            // Single a → 1
            assert_eq!(func(b"a".as_ptr(), 1, 0), 1);
            // No a → fails (the mandatory first iteration)
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
            assert_eq!(func(b"b".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e4_plus_lazy_minimum_to_satisfy_following() {
        // a+?b — lazy plus matches the minimum number of a's then
        // b. For `ab` that's 1; for `aab` that's 2 (need to grow
        // until b matches).
        let (_host, func) = jit_compile("a+?b");
        unsafe {
            // `ab` → one a, then b → 2
            assert_eq!(func(b"ab".as_ptr(), 2, 0), 2);
            // `aab` → one a then b fails (next char is `a`),
            // backtrack to two a's then b matches → 3
            assert_eq!(func(b"aab".as_ptr(), 3, 0), 3);
            // `aaab` → grows to three a's → 4
            assert_eq!(func(b"aaab".as_ptr(), 4, 0), 4);
            // No b → fails
            assert_eq!(func(b"aaa".as_ptr(), 3, 0), -1);
            // No a → fails (mandatory first iteration)
            assert_eq!(func(b"b".as_ptr(), 1, 0), -1);
        }
    }

    #[test]
    fn step3e4_plus_lazy_digit() {
        // \d+? against `123` standalone — minimum of 1
        let (_host, func) = jit_compile(r"\d+?");
        unsafe {
            assert_eq!(func(b"123".as_ptr(), 3, 0), 1);
            assert_eq!(func(b"5".as_ptr(), 1, 0), 1);
            assert_eq!(func(b"abc".as_ptr(), 3, 0), -1);
        }
    }

    #[test]
    fn step3e4_plus_lazy_word_then_anchor() {
        // \w+?\z — lazy word chars to end of text. Has to grow
        // all the way to consume the entire input.
        let (_host, func) = jit_compile(r"\w+?\z");
        unsafe {
            assert_eq!(func(b"abc".as_ptr(), 3, 0), 3);
            assert_eq!(func(b"a".as_ptr(), 1, 0), 1);
            assert_eq!(func(b"".as_ptr(), 0, 0), -1);
        }
    }

    // ----- Lazy vs Greedy comparison test -----

    #[test]
    fn step3e4_lazy_vs_greedy_produce_different_results() {
        // The classic distinction: `a*b` and `a*?b` against `aaab`.
        // Both end up consuming all 3 a's because `b` needs to
        // match. The match span is the same (4 bytes). Where they
        // differ is in what they'd report for INTERNAL captures
        // (which we don't track in the JIT yet) and which paths
        // they explore.
        //
        // For an externally-observable difference, we need a
        // pattern where the OVERALL match length differs. Use
        // standalone quantifiers: `a*` vs `a*?` against `aaa`.
        let (_star_greedy_host, star_greedy_fn) = jit_compile("a*");
        let (_star_lazy_host, star_lazy_fn) = jit_compile("a*?");
        unsafe {
            assert_eq!(
                star_greedy_fn(b"aaa".as_ptr(), 3, 0),
                3,
                "greedy `a*` consumes all"
            );
            assert_eq!(
                star_lazy_fn(b"aaa".as_ptr(), 3, 0),
                0,
                "lazy `a*?` consumes none"
            );
        }

        // Same for `a+` vs `a+?`
        let (_plus_greedy_host, plus_greedy_fn) = jit_compile("a+");
        let (_plus_lazy_host, plus_lazy_fn) = jit_compile("a+?");
        unsafe {
            assert_eq!(plus_greedy_fn(b"aaa".as_ptr(), 3, 0), 3);
            assert_eq!(plus_lazy_fn(b"aaa".as_ptr(), 3, 0), 1);
        }

        // And `a?` vs `a??`
        let (_question_greedy_host, question_greedy_fn) = jit_compile("a?");
        let (_question_lazy_host, question_lazy_fn) = jit_compile("a??");
        unsafe {
            assert_eq!(question_greedy_fn(b"a".as_ptr(), 1, 0), 1);
            assert_eq!(question_lazy_fn(b"a".as_ptr(), 1, 0), 0);
        }
    }

    // ============================================================
    // C1 step 4a — corpus-based differential gate
    // ============================================================
    //
    // For each (pattern, input) pair in the hand-curated corpus,
    // compile the pattern through both the JIT path
    // (`compile_program` from this module) and the existing
    // interpreter (`Regex::compile`), then assert the match spans
    // are byte-for-byte equivalent. Patterns the JIT can't handle
    // (CodegenUnsupported) are skipped — they would route through
    // the interpreter in production anyway.
    //
    // The JIT'd function tests the pattern at *exactly* `pos`, so
    // the test harness wraps it in a scan loop that tries every
    // position from 0 to text.len() (inclusive) and returns the
    // leftmost successful match — mimicking the interpreter's
    // `find_first` scan semantics.
    //
    // This is the design doc step 4 "differential gate active"
    // piece for the existing JIT subset. Capture trail (step 4b)
    // is a separate commit.

    /// Wrap a JIT'd `Step3aJittedFn` in a scan loop. For each
    /// position from `0..=text.len()` (inclusive — to allow empty
    /// matches at end of text), call the JIT'd function and
    /// return the leftmost (start, end) where the function
    /// returned a non-negative value. Returns `None` if no
    /// position produces a match.
    fn jit_find_first_via_scan(func: Step3aJittedFn, text: &[u8]) -> Option<(usize, usize)> {
        for start in 0..=text.len() {
            // SAFETY: text outlives the call; func is alive for
            // the lifetime of the host the caller still owns.
            let result = unsafe { func(text.as_ptr(), text.len(), start) };
            if result >= 0 {
                // The `>= 0` check above proves the cast is safe.
                #[allow(clippy::cast_sign_loss)]
                let end = result as usize;
                return Some((start, end));
            }
        }
        None
    }

    /// Compile `pattern` via both the JIT and the interpreter,
    /// then assert that for every input in `inputs` they produce
    /// byte-for-byte identical match spans. Returns a bool
    /// indicating whether the JIT path was actually exercised
    /// (false = pattern wasn't JIT-eligible at this step's
    /// codegen subset, so the test was skipped).
    fn assert_jit_interp_equivalent(pattern: &str, inputs: &[&[u8]]) -> bool {
        // Build the interpreter Regex via the public API.
        let regex = match crate::Regex::compile(pattern) {
            Ok(r) => r,
            Err(e) => panic!("interpreter compile failed for `{pattern}`: {e}"),
        };

        // Build the JIT'd function via compile_program. Skip if
        // the pattern is outside the current step's codegen subset.
        let program = compile(pattern);
        let mut host = JitHost::new().expect("JitHost::new must succeed");
        let func_id = match compile_program(&program, &mut host) {
            Ok(id) => id,
            Err(JitHostError::CodegenUnsupported(_)) => return false,
            Err(other) => panic!("compile_program for `{pattern}` failed: {other}"),
        };
        host.finalize_definitions().expect("finalize must succeed");
        // SAFETY: signature `(i64, i64, i64) -> i64` matches the
        // Step3aJittedFn C ABI; host outlives the calls.
        let func: Step3aJittedFn = unsafe { std::mem::transmute(host.get_finalized_fn(func_id)) };

        for input in inputs {
            // Interpreter result via the public Regex API.
            let interp_text =
                std::str::from_utf8(input).expect("differential corpus inputs must be valid UTF-8");
            let interp_span = regex.find_first(interp_text).map(|m| (m.start, m.end));

            // JIT result via scan-loop wrapper.
            let jit_span = jit_find_first_via_scan(func, input);

            assert_eq!(
                interp_span, jit_span,
                "differential divergence on pattern={pattern:?} input={input:?}: \
                 interp={interp_span:?} jit={jit_span:?}"
            );
        }
        true
    }

    // ----- Differential corpus: literals -----

    #[test]
    fn differential_pure_literals() {
        assert!(assert_jit_interp_equivalent(
            "abc",
            &[b"abc", b"abcdef", b"xyzabc", b"xabcy", b"ab", b"", b"xyz", b"AAAAabc",]
        ));
    }

    #[test]
    fn differential_single_char_literals() {
        assert!(assert_jit_interp_equivalent(
            "a",
            &[b"a", b"", b"aaa", b"b", b"xa"]
        ));
    }

    // ----- Differential corpus: char classes -----

    #[test]
    fn differential_digit_class() {
        assert!(assert_jit_interp_equivalent(
            r"\d",
            &[b"5", b"a", b"", b"abc123", b"123abc", b"_5"]
        ));
    }

    #[test]
    fn differential_word_class() {
        assert!(assert_jit_interp_equivalent(
            r"\w",
            &[b"a", b"5", b"_", b"!", b" ", b"", b"hello world"]
        ));
    }

    #[test]
    fn differential_space_class() {
        assert!(assert_jit_interp_equivalent(
            r"\s",
            &[b" ", b"\t", b"\n", b"a", b"", b"  abc"]
        ));
    }

    #[test]
    fn differential_negated_char_classes() {
        assert!(assert_jit_interp_equivalent(
            r"\D",
            &[b"a", b"5", b" ", b"!"]
        ));
        assert!(assert_jit_interp_equivalent(
            r"\W",
            &[b"!", b"a", b"_", b" "]
        ));
        assert!(assert_jit_interp_equivalent(
            r"\S",
            &[b"a", b" ", b"\t", b"!"]
        ));
    }

    // ----- Differential corpus: anchors -----

    #[test]
    fn differential_start_text_anchor() {
        assert!(assert_jit_interp_equivalent(
            r"\Aabc",
            &[b"abc", b"abcdef", b"xabc", b"", b"abxabc"]
        ));
    }

    #[test]
    fn differential_end_text_anchor() {
        assert!(assert_jit_interp_equivalent(
            r"abc\z",
            &[b"abc", b"xabc", b"abcd", b"", b"abc\n"]
        ));
    }

    #[test]
    fn differential_both_anchors() {
        assert!(assert_jit_interp_equivalent(
            r"\Aabc\z",
            &[b"abc", b"abcd", b"abx", b"", b"xabc"]
        ));
    }

    #[test]
    fn differential_word_boundary() {
        assert!(assert_jit_interp_equivalent(
            r"\bword\b",
            &[
                b"word",
                b"abc word def",
                b"aword",
                b"worda",
                b"awordb",
                b"",
                b"word!",
                b"!word",
            ]
        ));
    }

    // ----- Differential corpus: alternations -----

    #[test]
    fn differential_simple_alternation() {
        assert!(assert_jit_interp_equivalent(
            "cat|dog",
            &[b"cat", b"dog", b"bird", b"", b"catdog", b"dogcat", b"xcat"]
        ));
    }

    #[test]
    fn differential_three_branch_alternation() {
        assert!(assert_jit_interp_equivalent(
            "cat|dog|bird",
            &[b"cat", b"dog", b"bird", b"fish", b""]
        ));
    }

    #[test]
    fn differential_alternation_with_overlap() {
        // ab|abc — leftmost-first wins, returns 2-char match even
        // when 3-char would also match.
        assert!(assert_jit_interp_equivalent(
            "ab|abc",
            &[b"abc", b"ab", b"a", b""]
        ));
    }

    // ----- Differential corpus: quantifiers (greedy) -----

    #[test]
    fn differential_plus_greedy() {
        assert!(assert_jit_interp_equivalent(
            r"\d+",
            &[b"5", b"123", b"123abc", b"abc", b"", b"a1b2c3"]
        ));
    }

    #[test]
    fn differential_star_greedy() {
        assert!(assert_jit_interp_equivalent(
            r"\d*",
            &[b"5", b"123", b"abc", b"", b"abc123"]
        ));
    }

    #[test]
    fn differential_question_greedy() {
        assert!(assert_jit_interp_equivalent(
            r"\d?",
            &[b"5", b"a", b"", b"55", b"abc"]
        ));
    }

    // ----- Differential corpus: quantifiers (lazy) -----

    #[test]
    fn differential_plus_lazy() {
        assert!(assert_jit_interp_equivalent(
            r"\d+?",
            &[b"5", b"123", b"abc", b""]
        ));
    }

    #[test]
    fn differential_star_lazy() {
        assert!(assert_jit_interp_equivalent(
            r"\d*?",
            &[b"5", b"123", b"abc", b""]
        ));
    }

    #[test]
    fn differential_question_lazy() {
        assert!(assert_jit_interp_equivalent(
            r"\d??",
            &[b"5", b"a", b"", b"55"]
        ));
    }

    // ----- Differential corpus: combinations -----

    #[test]
    fn differential_quantifier_followed_by_literal() {
        assert!(assert_jit_interp_equivalent(
            r"\d+x",
            &[b"5x", b"123x", b"x", b"", b"abc5x", b"5", b"123"]
        ));
    }

    #[test]
    fn differential_anchor_class_quantifier_anchor() {
        assert!(assert_jit_interp_equivalent(
            r"\A\d+\z",
            &[b"123", b"5", b"", b"123abc", b"abc"]
        ));
    }

    #[test]
    fn differential_email_like() {
        assert!(assert_jit_interp_equivalent(
            r"\w+@\w+\.\w+",
            &[
                b"user@example.com",
                b"a@b.c",
                b"hello world",
                b"user@example",
                b"@.com",
                b"",
            ]
        ));
    }

    #[test]
    fn differential_word_alternation() {
        assert!(assert_jit_interp_equivalent(
            r"\w+|word",
            &[b"hello", b"123", b"!", b"", b"word", b"a"]
        ));
    }

    #[test]
    fn differential_combined_quantifiers() {
        assert!(assert_jit_interp_equivalent(
            r"a*b+",
            &[b"b", b"ab", b"aaab", b"bbb", b"aaa", b"", b"abbabb"]
        ));
    }

    #[test]
    fn differential_lazy_with_following() {
        // The classic lazy-vs-greedy distinction: `a*?b` matches
        // the minimum a's needed to allow b. Both engines must
        // return the same match span.
        assert!(assert_jit_interp_equivalent(
            r"a*?b",
            &[b"b", b"ab", b"aab", b"aaab", b"aaa", b""]
        ));
    }

    #[test]
    fn differential_word_boundary_with_class_quantifier() {
        assert!(assert_jit_interp_equivalent(
            r"\b\d+\b",
            &[
                b"123",
                b"abc 123 def",
                b"123abc",
                b"abc123",
                b"",
                b" 123",
                b"123 ",
            ]
        ));
    }

    #[test]
    fn differential_multi_anchor_pattern() {
        assert!(assert_jit_interp_equivalent(
            r"\Ahello\b",
            &[b"hello", b"hello world", b"helloworld", b"", b"xhello"]
        ));
    }
}
