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
// C1 step 3a — codegen for linear single-byte literal programs
// ============================================================

/// **C1 step 3a signature.** The shape of the JIT'd function returned
/// by [`compile_program`] in step 3a. Documents the C ABI contract
/// callers transmute the raw function pointer to.
///
/// # Parameters
/// - `text`: pointer to the input bytes (borrow lifetime managed by
///   the caller; must outlive the call)
/// - `text_len`: length of the input in bytes
/// - `pos`: byte position to test the pattern at
///
/// # Returns
/// - `>= 0`: the new position after a successful match (`pos +
///   pattern_length`)
/// - `-1`: the pattern did not match at `pos`
///
/// The function tests the pattern at *exactly* `pos` — it does not
/// scan. Scanning is the caller's responsibility (typically the
/// engine dispatch loop, which lands at C1 step 5).
///
/// # Safety
/// Callers must ensure `text` points to at least `text_len` bytes of
/// initialized memory and that `pos <= text_len`. The function
/// performs its own bounds check before any byte loads, but it
/// trusts the caller-supplied pointer / length / position to refer
/// to a valid slice.
pub type Step3aJittedFn =
    unsafe extern "C" fn(text: *const u8, text_len: usize, pos: usize) -> isize;

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
    // Eligibility short-circuit. The compile function trusts that
    // anything is_jit_eligible accepts is something it can lower —
    // step 3a additionally restricts the accepted set via
    // extract_step3a_literal below.
    if !is_jit_eligible(program) {
        return Err(JitHostError::CodegenUnsupported(
            "program is not in the JIT-eligible subset (see is_jit_eligible)".to_string(),
        ));
    }

    // Walk the bytecode and extract the literal byte sequence. If
    // anything outside the step 3a subset appears, bail with a
    // descriptive error so the caller can fall back to the
    // interpreter for this pattern.
    let literal = extract_step3a_literal(&program.code)?;

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
    let name = format!("rgx_jit_step3a_{}", host.next_func_index());
    let func_id = host.declare_function(&name, Linkage::Local, &sig)?;

    // Build the IR.
    let mut function = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    {
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut fb_ctx);

        let entry = builder.create_block();
        let success_block = builder.create_block();
        let fail_block = builder.create_block();

        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);

        let text_ptr = builder.block_params(entry)[0];
        let text_len = builder.block_params(entry)[1];
        let pos = builder.block_params(entry)[2];

        // needed = pos + literal.len()
        let literal_len = i64::try_from(literal.len()).map_err(|_| {
            JitHostError::CodegenUnsupported(format!(
                "literal length {} does not fit in i64",
                literal.len()
            ))
        })?;
        let needed = builder.ins().iadd_imm(pos, literal_len);

        // if needed > text_len: jump to fail
        let bounds_ok = builder
            .ins()
            .icmp(IntCC::UnsignedLessThanOrEqual, needed, text_len);
        let first_check = builder.create_block();
        builder
            .ins()
            .brif(bounds_ok, first_check, &[], fail_block, &[]);
        // Entry block has no more incoming branches; seal it now so
        // Cranelift can finalise SSA value definitions for it before
        // we move on to the per-byte chain.
        builder.seal_block(entry);

        // Per-byte comparison chain.
        builder.switch_to_block(first_check);
        let base_ptr = builder.ins().iadd(text_ptr, pos);

        // For each literal byte, load text[pos + i] and compare
        // against the expected byte. On mismatch jump to fail; on
        // match fall through to the next byte (or to success after
        // the last byte).
        let mut current_block = first_check;
        for (i, &expected) in literal.iter().enumerate() {
            // Load the byte at offset i from base_ptr. Cranelift's
            // `load` accepts an i32 immediate offset; the literal
            // length is bounded by the bytecode walker's u8 length
            // prefixes, so this conversion is essentially infallible
            // — but we surface any overflow as `CodegenUnsupported`
            // rather than panicking, per design doc §1.0 (every
            // failure mode is a controlled error).
            let byte_offset = i32::try_from(i).map_err(|_| {
                JitHostError::CodegenUnsupported(format!(
                    "literal byte index {i} exceeds Cranelift's i32 load offset"
                ))
            })?;
            let loaded = builder
                .ins()
                .load(types::I8, MemFlags::trusted(), base_ptr, byte_offset);
            // Compare with expected byte. iconst sign-extends, but
            // we use icmp_imm with an i64 expected value because
            // Cranelift's icmp_imm wants an i64 immediate.
            let expected_const = i64::from(expected);
            let matches = builder.ins().icmp_imm(IntCC::Equal, loaded, expected_const);

            // The next block is either the next byte's check or the
            // success block on the last iteration.
            let next_block = if i + 1 == literal.len() {
                success_block
            } else {
                builder.create_block()
            };
            builder
                .ins()
                .brif(matches, next_block, &[], fail_block, &[]);
            builder.seal_block(current_block);
            current_block = next_block;
            builder.switch_to_block(current_block);
        }

        // success_block: return needed (the new position).
        builder.ins().return_(&[needed]);
        builder.seal_block(success_block);

        // fail_block: return -1.
        builder.switch_to_block(fail_block);
        let neg_one = builder.ins().iconst(types::I64, -1);
        builder.ins().return_(&[neg_one]);
        builder.seal_block(fail_block);

        builder.finalize();
    }

    host.define_function(func_id, function)?;
    Ok(func_id)
}

/// Walk a program's bytecode and extract the literal byte sequence
/// it represents, OR return a `CodegenUnsupported` error if it
/// contains anything outside the step 3a subset.
///
/// Step 3a accepts:
/// - `Char(len=1)` opcodes — extracted into the literal byte sequence
/// - `SaveStart(0)` / `SaveEnd(0)` opcodes — accepted as no-op (the
///   caller computes group 0 from the start position and the returned
///   end position; explicit per-group capture tracking lands at step 4)
/// - A trailing `Match` opcode
///
/// Anything else returns a `CodegenUnsupported` error with a message
/// identifying the offending opcode.
fn extract_step3a_literal(code: &[u8]) -> Result<Vec<u8>, JitHostError> {
    let mut literal = Vec::new();
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
                // 1 byte length prefix + length payload.
                let Some(&len_byte) = code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(
                        "truncated Char opcode (missing length prefix)".to_string(),
                    ));
                };
                let length = len_byte as usize;
                ip += 1;
                if length != 1 {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3a only handles single-byte Char literals; \
                         got {length}-byte Char (multi-byte literals land at step 6)"
                    )));
                }
                let Some(&byte) = code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(
                        "truncated Char opcode (missing payload byte)".to_string(),
                    ));
                };
                literal.push(byte);
                ip += 1;
            }
            OpCode::SaveStart | OpCode::SaveEnd => {
                // 1 byte group id. Step 3a accepts group 0 wrappers
                // as no-op (the engine layer reconstructs group 0
                // from the entry pos + returned end pos at step 5).
                // Higher group ids would need real capture tracking
                // and are deferred to step 4.
                let Some(&group_id) = code.get(ip) else {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "truncated {op:?} opcode (missing group id)"
                    )));
                };
                if group_id != 0 {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3a only accepts group-0 capture wrappers; \
                         got {op:?} for group {group_id} (capture trail lands at step 4)"
                    )));
                }
                ip += 1;
            }
            OpCode::Match => {
                saw_match = true;
                // The bytecode should end here. We tolerate trailing
                // bytes only if they aren't reachable, but the
                // existing compiler always emits Match as the last
                // opcode of the main bytecode, so anything after it
                // is unexpected.
                if ip != code.len() {
                    return Err(JitHostError::CodegenUnsupported(format!(
                        "step 3a expects Match to terminate the program; \
                         got {} trailing bytes after Match",
                        code.len() - ip
                    )));
                }
                break;
            }
            other => {
                return Err(JitHostError::CodegenUnsupported(format!(
                    "step 3a does not yet support {other:?} (lands in a later step)"
                )));
            }
        }
    }

    if !saw_match {
        return Err(JitHostError::CodegenUnsupported(
            "step 3a requires a Match opcode at end of program".to_string(),
        ));
    }

    Ok(literal)
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

    #[test]
    fn step3a_refuses_char_class() {
        assert_codegen_unsupported(r"\d");
    }

    #[test]
    fn step3a_refuses_dot() {
        assert_codegen_unsupported(".");
    }

    #[test]
    fn step3a_refuses_anchor() {
        assert_codegen_unsupported(r"\Aabc");
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
}
