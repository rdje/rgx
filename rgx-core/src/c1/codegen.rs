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
use cranelift_codegen::ir::{types, AbiParam, Function, InstBuilder, MemFlags, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage};

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

    // Build the IR using a per-opcode block-per-block layout. Each
    // op block takes the current `pos` as its single block parameter
    // and either advances pos and jumps to the next op's block, or
    // jumps to the fail block on a mismatch. The Match op jumps to
    // the success block, which returns the final pos.
    {
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut fb_ctx);

        // Allocate all blocks up front so we can target the next
        // op's block by index when emitting each op.
        let entry = builder.create_block();
        let success_block = builder.create_block();
        let fail_block = builder.create_block();
        let op_blocks: Vec<_> = ops.iter().map(|_| builder.create_block()).collect();

        // Each op block takes the current pos as a single i64
        // parameter. Same for the success block (it returns whatever
        // pos is when the Match op fires).
        for &b in &op_blocks {
            builder.append_block_param(b, types::I64);
        }
        builder.append_block_param(success_block, types::I64);

        // === Entry block: load function params and jump into the
        // first op block (or directly to success if there are no
        // ops, which shouldn't happen but is handled defensively).
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let text_ptr = builder.block_params(entry)[0];
        let text_len = builder.block_params(entry)[1];
        let init_pos = builder.block_params(entry)[2];
        let first_target = op_blocks.first().copied().unwrap_or(success_block);
        builder.ins().jump(first_target, &[init_pos]);
        builder.seal_block(entry);

        // === Per-op blocks: emit IR for each JitOp. Each block jumps
        // to the next op's block (passing the new pos) or to the
        // fail block. The Match op jumps to success_block.
        for (i, op) in ops.iter().enumerate() {
            let block = op_blocks[i];
            builder.switch_to_block(block);
            let pos = builder.block_params(block)[0];

            // The "next block" for a successful step is the next op
            // block, or the success block if this is the last op.
            // (Match always jumps to success_block directly via
            // `emit_jit_op` and ignores `next_block`.)
            let next_block = op_blocks.get(i + 1).copied().unwrap_or(success_block);

            emit_jit_op(
                &mut builder,
                *op,
                pos,
                text_ptr,
                text_len,
                next_block,
                fail_block,
                success_block,
                word_boundary_ref,
            );
            builder.seal_block(block);
        }

        // === Success block: return the pos parameter.
        builder.switch_to_block(success_block);
        let final_pos = builder.block_params(success_block)[0];
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

/// Emit Cranelift IR for a single [`JitOp`] inside its dedicated
/// block. The caller has already switched the builder to the op's
/// block and obtained the current `pos` from its block parameter.
///
/// Each op either advances `pos` and jumps to `next_block` (passing
/// the new pos) or jumps to `fail_block`. The `Match` op terminates
/// by jumping to `success_block` with the current pos.
///
/// **Step 3b/3c.** This function handles the linear opcode subset
/// (`Char`, char classes, `StartText`/`EndText`, `SaveGroupZero`,
/// `Match`) from step 3b plus the word-boundary opcodes (`\b`/`\B`)
/// added in step 3c. Step 3d will extend it for control-flow
/// opcodes (`Split`, `Jump`) which need a backtrack stack.
///
/// Word boundary handling uses an indirect call to the runtime
/// helper [`crate::c1::runtime::rgx_runtime_word_boundary_test`]
/// via the `word_boundary_ref` parameter, which `compile_program`
/// imports into the current function via
/// [`crate::c1::jit::JitHost::import_word_boundary_helper`] when
/// any `WordBoundary` op appears in the program.
#[allow(clippy::too_many_arguments)] // each parameter is conceptually distinct and there's no good grouping
fn emit_jit_op(
    builder: &mut FunctionBuilder,
    op: JitOp,
    pos: cranelift_codegen::ir::Value,
    text_ptr: cranelift_codegen::ir::Value,
    text_len: cranelift_codegen::ir::Value,
    next_block: cranelift_codegen::ir::Block,
    fail_block: cranelift_codegen::ir::Block,
    success_block: cranelift_codegen::ir::Block,
    word_boundary_ref: Option<cranelift_codegen::ir::FuncRef>,
) {
    match op {
        JitOp::Char(b) => {
            emit_consume_byte_with_test(
                builder,
                pos,
                text_ptr,
                text_len,
                next_block,
                fail_block,
                |fb, byte| fb.ins().icmp_imm(IntCC::Equal, byte, i64::from(b)),
            );
        }
        JitOp::DigitAscii { negated } => {
            emit_consume_byte_with_test(
                builder,
                pos,
                text_ptr,
                text_len,
                next_block,
                fail_block,
                |fb, byte| emit_digit_byte_test(fb, byte, negated),
            );
        }
        JitOp::WordAscii { negated } => {
            emit_consume_byte_with_test(
                builder,
                pos,
                text_ptr,
                text_len,
                next_block,
                fail_block,
                |fb, byte| emit_word_byte_test(fb, byte, negated),
            );
        }
        JitOp::SpaceAscii { negated } => {
            emit_consume_byte_with_test(
                builder,
                pos,
                text_ptr,
                text_len,
                next_block,
                fail_block,
                |fb, byte| emit_space_byte_test(fb, byte, negated),
            );
        }
        JitOp::StartText => {
            // Zero-width: matches iff pos == 0. No bytes consumed,
            // so the next block sees the same pos.
            let cond = builder.ins().icmp_imm(IntCC::Equal, pos, 0);
            builder
                .ins()
                .brif(cond, next_block, &[pos], fail_block, &[]);
        }
        JitOp::EndText => {
            // Zero-width: matches iff pos == text_len. No bytes
            // consumed, so the next block sees the same pos.
            let cond = builder.ins().icmp(IntCC::Equal, pos, text_len);
            builder
                .ins()
                .brif(cond, next_block, &[pos], fail_block, &[]);
        }
        JitOp::WordBoundary { negated } => {
            // Zero-width: calls the runtime helper
            // `rgx_runtime_word_boundary_test(text, text_len, pos)`
            // which returns a bool (i8). For \b: pass-through if
            // the helper returned non-zero. For \B: pass-through
            // if the helper returned zero.
            //
            // The helper is imported into the function by
            // `compile_program` via `JitHost::import_word_boundary_helper`
            // and passed in as `word_boundary_ref`. If we reach
            // this branch without an import, it's a codegen layer
            // bug — we expect that anywhere a `WordBoundary` op
            // appears, the import was performed up front. Use
            // `expect` to surface the bug loudly.
            let func_ref = word_boundary_ref
                .expect("WordBoundary op requires the helper import; compile_program is buggy");
            let call = builder.ins().call(func_ref, &[text_ptr, text_len, pos]);
            let raw_result = builder.inst_results(call)[0];
            // The helper returns i8 (the C ABI bool). Compare
            // against zero to get a Cranelift boolean we can branch
            // on. For \B (negated), we invert the test by swapping
            // the branch targets — equivalent to `!returned`.
            let is_boundary = builder.ins().icmp_imm(IntCC::NotEqual, raw_result, 0);
            if negated {
                builder
                    .ins()
                    .brif(is_boundary, fail_block, &[], next_block, &[pos]);
            } else {
                builder
                    .ins()
                    .brif(is_boundary, next_block, &[pos], fail_block, &[]);
            }
        }
        JitOp::SaveGroupZero { which: _ } => {
            // Step 3b: group-0 wrappers are no-op. The engine layer
            // (step 5) reconstructs group 0 from entry pos + returned
            // end pos. Step 4 will replace this with real capture
            // trail handling for groups 1+. Just thread pos through.
            builder.ins().jump(next_block, &[pos]);
        }
        JitOp::Match => {
            // Terminate with success: pos becomes the return value.
            let _ = next_block; // unused for Match
            builder.ins().jump(success_block, &[pos]);
        }
    }
}

/// Helper: emit IR for a "consume one byte and apply a predicate"
/// opcode. The predicate closure builds the per-byte test in
/// Cranelift IR (returning an i8 boolean value: 0 = fail, 1 = pass).
///
/// The emitted IR:
/// 1. Bounds check: `pos < text_len`. If not, jump to fail.
/// 2. Load `text[pos]` as an i8.
/// 3. Apply the predicate closure to get a boolean.
/// 4. If true, jump to `next_block` with `pos + 1`. Else jump to fail.
fn emit_consume_byte_with_test<F>(
    builder: &mut FunctionBuilder,
    pos: cranelift_codegen::ir::Value,
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

    // If the predicate matched, advance pos and jump to the next op.
    // Otherwise jump to fail.
    let new_pos = builder.ins().iadd_imm(pos, 1);
    builder
        .ins()
        .brif(cond, next_block, &[new_pos], fail_block, &[]);
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
/// descriptive message identifying the offending opcode. Step 3b
/// accepts:
///
/// - `Char(len=1)` — single-byte ASCII literal
/// - `DigitAscii` / `DigitAsciiNeg`
/// - `WordAscii` / `WordAsciiNeg`
/// - `SpaceAscii` / `SpaceAsciiNeg`
/// - `StartText` (`\A`) / `EndText` (`\z`)
/// - `SaveStart(0)` / `SaveEnd(0)` (group-0 wrappers, no-op)
/// - `Match` (terminator)
///
/// Anything else (multi-byte `Char`, line anchors, word boundaries,
/// `\Z` / `\X` / `\K`, control-flow opcodes, captures for groups
/// 1+, ...) returns a descriptive `CodegenUnsupported` error and
/// the caller falls back to the interpreter for that pattern.
fn decode_program(code: &[u8]) -> Result<Vec<JitOp>, JitHostError> {
    let mut ops = Vec::new();
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
            OpCode::SaveStart | OpCode::SaveEnd => {
                let Some(&group_id) = code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "truncated {op:?} opcode (missing group id)"
                    )));
                };
                if group_id != 0 {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3b only accepts group-0 capture wrappers; \
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
                    "step 3b does not yet support {other:?} (lands in a later step)"
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

    #[test]
    fn step3a_refuses_alternation() {
        assert_codegen_unsupported("a|b");
    }

    #[test]
    fn step3a_refuses_quantifier() {
        assert_codegen_unsupported("a+");
    }

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
}
