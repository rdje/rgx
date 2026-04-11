//! C1 codegen layer.
//!
//! At C1 step 2 this module hosts only the **JIT eligibility check** —
//! [`is_jit_eligible`] decides whether the JIT will accept a compiled
//! [`Program`]. The actual codegen functions (lowering bytecode opcodes
//! into Cranelift IR) land in step 3 and grow this file alongside.
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

use crate::vm::{OpCode, Program};

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
}
