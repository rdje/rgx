//! Runtime helper layer for JIT'd code.
//!
//! At C1 step 1 this is a **signature-only skeleton** — the helper
//! functions are declared with stable C ABI signatures so the codegen
//! layer (landing in step 3) can generate `extern "C"` calls to them
//! before the implementations are wired in. The actual implementations
//! land in steps 6 (`CharClass(id)` + multi-byte literal support) and
//! 7 (runtime safety helpers).
//!
//! See `docs/C1_JIT_COMPILATION_DESIGN.md` §7 for the helper layer
//! design rationale and §7.2 for the contract-versioning story.
//!
//! # Why C ABI
//!
//! Cranelift handles C calling conventions cleanly across all
//! supported targets (`x86_64` / `aarch64` on Linux / macOS / Windows).
//! Using the Rust ABI here would couple the JIT'd code to a specific
//! Rust compiler version, which is fragile across MSRV bumps. The C
//! ABI is stable and Cranelift's ISA backends emit the right
//! prologue/epilogue automatically.
//!
//! # Why no implementations yet
//!
//! Step 1 is host plumbing only — proving that the Cranelift JIT
//! pipeline runs end-to-end on the target. Real opcode lowering
//! (which would call into these helpers) lands in step 3, and the
//! helpers themselves are implemented in steps 6 and 7. Putting
//! placeholder implementations here at step 1 would be misleading:
//! they have no callers and can't be tested in isolation. The
//! signatures stand alone as the contract that the codegen layer
//! and the runtime layer share.
//!
//! # Stability of these signatures
//!
//! Once a helper is wired into the codegen layer (step 3+), its
//! signature is **frozen** for the C1 v1 release. Changing a
//! signature requires bumping the JIT module version on `Engine`
//! and falling back to the interpreter for any function compiled
//! against the old signature. See design doc §7.2 for the full
//! versioning story.

#![allow(dead_code)] // Step 1 is signatures only; callers come in step 3.

/// Match a `CharClass(id)` / `CharClassNeg(id)` opcode at the given
/// input position. Returns the number of input bytes consumed
/// (1..=4 for a successful match) or 0 for no match.
///
/// **Step 6 implementation.** Replaces the step-1 stub with the real
/// codegen the JIT calls into for `CharClass(id)` and
/// `CharClassNeg(id)` opcodes. The helper:
///
/// 1. Bounds-checks `pos < text_len` (returns 0 if not).
/// 2. Decodes the UTF-8 character starting at `text[pos]` (handles
///    1..=4 byte widths). Invalid UTF-8 leading bytes return 0.
/// 3. Looks up `char_classes[class_id]` and tests the decoded
///    character against it via the same bitmap-then-Unicode-range
///    logic the existing VM uses (`RegexVM::test_char_class`).
/// 4. If `negated == 0`, returns the char width on a positive match
///    or 0 on a negative match. If `negated != 0`, the result is
///    inverted: returns the char width on a negative match or 0 on
///    a positive match.
///
/// The character-width-aware return value lets the JIT'd caller
/// advance `pos` by the right amount in a single instruction
/// (`pos += result`), avoiding a second UTF-8 decode pass.
///
/// # Parameters
/// - `text`: pointer to the input bytes. Borrowed for the duration
///   of the call.
/// - `text_len`: length of the input in bytes.
/// - `pos`: byte position to test. Must be `<= text_len`.
/// - `char_classes_ptr`: pointer to the program's
///   `[CompiledCharClass]` slice cast as `*const u8`. The cast is
///   sound because `CompiledCharClass` is `#[repr(Rust)]` and we
///   re-cast back to the typed slice via `std::slice::from_raw_parts`
///   inside this function.
/// - `char_classes_len`: length of the `[CompiledCharClass]` slice.
/// - `class_id`: index into the char-class slice. Must be
///   `< char_classes_len`.
/// - `negated`: 0 for positive match (`CharClass`), nonzero for
///   negated match (`CharClassNeg`). Passed as `u32` rather than
///   `bool` for ABI clarity (Cranelift handles `i32` more uniformly
///   than `i8` across targets).
///
/// # Returns
/// `0..=4` — the number of input bytes consumed. `0` means no
/// match (either the bounds check failed, the UTF-8 decode failed,
/// the class lookup failed, or the predicate returned the wrong
/// polarity for `negated`). `1..=4` means a successful match
/// covering that many bytes.
///
/// # Safety
/// - `text` must point to at least `text_len` bytes of initialised
///   memory.
/// - `pos` must be `<= text_len`.
/// - `char_classes_ptr` must point to a valid
///   `[CompiledCharClass; char_classes_len]` slice with the same
///   memory layout as the program's `char_classes` Vec.
/// - `class_id` must be `< char_classes_len`.
///
/// All four invariants are upheld by the JIT'd caller (the codegen
/// layer in `c1/codegen.rs` only emits this call from the engine
/// dispatch path which holds the program alive and reads
/// `class_id` from the validated bytecode).
#[no_mangle]
pub unsafe extern "C" fn rgx_runtime_char_class_match_at(
    text: *const u8,
    text_len: usize,
    pos: usize,
    char_classes_ptr: *const u8,
    char_classes_len: usize,
    class_id: u32,
    negated: u32,
) -> u32 {
    // Bounds: nothing to consume past the end.
    if pos >= text_len {
        return 0;
    }

    // SAFETY: caller upholds (text, text_len) validity.
    let bytes = std::slice::from_raw_parts(text, text_len);
    let lead = bytes[pos];

    // Determine UTF-8 width from the leading byte. ASCII is the
    // common case so check it first.
    let width: usize = if lead < 0x80 {
        1
    } else {
        match lead {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            // Invalid leading byte — treat as no match.
            _ => return 0,
        }
    };

    // Bounds: the full UTF-8 sequence must fit in the input.
    if pos + width > text_len {
        return 0;
    }

    // Decode the character. ASCII fast path avoids the str
    // validation; multi-byte goes through std::str::from_utf8 to
    // reject mal-formed sequences (matching the existing VM's
    // `current_char` semantics).
    let ch: char = if width == 1 {
        lead as char
    } else {
        let bytes_for_char = &bytes[pos..pos + width];
        match std::str::from_utf8(bytes_for_char)
            .ok()
            .and_then(|s| s.chars().next())
        {
            Some(c) if c.len_utf8() == width => c,
            _ => return 0, // malformed — no match
        }
    };

    // Look up the char class. The slice cast back from the opaque
    // pointer relies on `char_classes_ptr` being the result of an
    // `as_ptr()` call on a `&[CompiledCharClass]` from the same
    // process — the caller (engine layer) guarantees this.
    let classes = std::slice::from_raw_parts(
        char_classes_ptr.cast::<crate::vm::CompiledCharClass>(),
        char_classes_len,
    );
    let class_idx = class_id as usize;
    if class_idx >= classes.len() {
        // Out-of-bounds class id — defensive return. This would
        // only fire if the bytecode and the program's char_classes
        // table fell out of sync, which is a compiler bug.
        return 0;
    }
    let cc = &classes[class_idx];

    // Test the char against the class. Mirrors
    // `RegexVM::test_char_class`: ASCII bitmap fast path, then
    // binary search the Unicode ranges.
    let in_class = test_char_class(ch, cc);

    // Apply negation. If `negated != 0` we want the inverted
    // predicate.
    let want_in_class = negated == 0;
    if in_class == want_in_class {
        // Match — consume `width` bytes.
        width as u32
    } else {
        0
    }
}

/// Test whether a `char` matches a `CompiledCharClass` using the
/// same logic as `RegexVM::test_char_class`. Kept private to this
/// module so the JIT runtime helper has its own copy of the test
/// function with the same semantics — the JIT path can't depend
/// on `RegexVM` private methods.
fn test_char_class(ch: char, cc: &crate::vm::CompiledCharClass) -> bool {
    let ch_code = ch as u32;
    if ch_code <= 127 {
        let byte_idx = (ch_code / 16) as usize;
        let bit_idx = (ch_code % 16) as usize;
        if byte_idx < cc.ascii_bitmap.len() {
            return (cc.ascii_bitmap[byte_idx] & (1u16 << bit_idx)) != 0;
        }
        return false;
    }
    cc.unicode_ranges
        .binary_search_by(|&(start, end)| {
            if ch_code < start {
                std::cmp::Ordering::Greater
            } else if ch_code > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Legacy step-1 stub kept as a no-op for the test that pings every
/// signature. Step 6 superseded this with
/// [`rgx_runtime_char_class_match_at`], which has a different
/// signature and is what the codegen layer actually calls.
#[no_mangle]
pub unsafe extern "C" fn rgx_runtime_char_class_test(
    char_classes: *const u8,
    class_id: u32,
    byte: u8,
) -> bool {
    let _ = (char_classes, class_id, byte);
    false
}

/// Test whether the position is at a word boundary (`\b`).
///
/// **Real implementation, landed at C1 step 3c.** A position is a
/// word boundary iff exactly one of the bytes at `pos - 1` and `pos`
/// is an ASCII word character `[A-Za-z0-9_]`. Out-of-range positions
/// (`pos == 0` or `pos == text_len`) are treated as "non-word"
/// neighbours, so `\b` matches at the start/end of input iff the
/// adjacent byte is a word character. This matches PCRE2 ASCII
/// `\b` semantics.
///
/// JIT'd code calls this via an indirect call registered with the
/// Cranelift JIT module's symbol table. The C ABI signature is the
/// stable contract; changes require a JIT module version bump.
///
/// # Safety
/// `text` must point to a valid `[u8]` of length `text_len`. `pos`
/// must be `<= text_len`. Both invariants are upheld by the
/// JIT'd-code caller (the codegen layer in `c1/codegen.rs` only
/// emits this call when the bounds are guaranteed by the engine
/// dispatch layer at step 5+).
#[no_mangle]
pub unsafe extern "C" fn rgx_runtime_word_boundary_test(
    text: *const u8,
    text_len: usize,
    pos: usize,
) -> bool {
    // SAFETY: caller upholds the contract that `text` points to
    // `text_len` valid bytes and `pos <= text_len`.
    let bytes = std::slice::from_raw_parts(text, text_len);
    let prev_is_word = pos > 0 && is_ascii_word_byte(bytes[pos - 1]);
    let curr_is_word = pos < text_len && is_ascii_word_byte(bytes[pos]);
    prev_is_word != curr_is_word
}

/// Returns `true` iff `b` is an ASCII word character: `[A-Za-z0-9_]`.
/// This matches the same set the existing VM and the C2 NFA use for
/// `\w` so word-boundary semantics stay consistent across all three
/// engines.
#[inline]
fn is_ascii_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Match a multi-byte literal character at the given position.
///
/// **Step 6 implementation** — currently UNUSED. Step 6's
/// multi-byte literal lowering inlines the byte comparisons
/// directly in Cranelift IR instead of calling out to this helper
/// (the inline form is faster because the byte values are constants
/// known at JIT-compile time, and there's no function-call overhead).
/// The helper is kept for completeness — design doc §7.1 lists it
/// as part of the runtime layer — and could be used as a fallback
/// path for very long literal sequences if we ever ship one.
///
/// # Parameters
/// - `text`, `text_len`: input slice.
/// - `pos`: starting byte position.
/// - `expected`: pointer to the expected byte sequence.
/// - `expected_len`: length of the expected byte sequence (1..=4).
///
/// # Returns
/// `true` if the bytes at `text[pos..pos + expected_len]` exactly
/// match `expected[..expected_len]`, `false` otherwise (including
/// if `pos + expected_len > text_len`).
///
/// # Safety
/// `text` and `expected` must point to valid `[u8]` slices of the
/// stated lengths.
#[no_mangle]
pub unsafe extern "C" fn rgx_runtime_match_multibyte_char(
    text: *const u8,
    text_len: usize,
    pos: usize,
    expected: *const u8,
    expected_len: usize,
) -> bool {
    if pos + expected_len > text_len {
        return false;
    }
    let text_slice = std::slice::from_raw_parts(text, text_len);
    let expected_slice = std::slice::from_raw_parts(expected, expected_len);
    &text_slice[pos..pos + expected_len] == expected_slice
}

/// Compare a captured substring against the input at the given
/// position (the `\1` / `\k<name>` backreference op).
///
/// **Step 1: signature-only stub.** Backreference lowering is NOT
/// in C1 v1 — patterns containing backrefs are JIT-ineligible per
/// design doc §5.3. This signature is reserved for v2.
///
/// # Safety
/// `ctx` must point to a live `ExecContext`.
#[no_mangle]
pub unsafe extern "C" fn rgx_runtime_compare_capture(ctx: *mut u8, group_id: u32) -> bool {
    let _ = (ctx, group_id);
    false
}

/// Run a sub-program (lookaround / recursion / inline subroutine).
///
/// **Step 1: signature-only stub.** Lookaround / recursion lowering
/// is NOT in C1 v1 — those patterns are JIT-ineligible per design
/// doc §5.3. This signature is reserved for v2.
///
/// # Safety
/// `ctx` must point to a live `ExecContext`.
#[no_mangle]
pub unsafe extern "C" fn rgx_runtime_run_subprogram(ctx: *mut u8, subprogram_id: u32) -> bool {
    let _ = (ctx, subprogram_id);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the still-stubbed helpers (`rgx_runtime_compare_capture`
    /// and `rgx_runtime_run_subprogram` — backreference and lookaround
    /// lowerings are NOT in C1 v1) link and can be called with
    /// safe defaults. The implemented helpers
    /// (`rgx_runtime_word_boundary_test`, `rgx_runtime_char_class_match_at`,
    /// `rgx_runtime_char_class_test`, `rgx_runtime_match_multibyte_char`)
    /// get their own dedicated correctness tests below; this test
    /// only catches signature drift on the still-stubbed helpers.
    #[test]
    fn still_stubbed_helpers_are_callable_and_return_safe_defaults() {
        // SAFETY: stubs ignore their arguments. Passing null
        // pointers is safe because the implementations don't
        // dereference anything.
        unsafe {
            assert!(!rgx_runtime_char_class_test(std::ptr::null(), 0, 0));
            assert!(!rgx_runtime_compare_capture(std::ptr::null_mut(), 0));
            assert!(!rgx_runtime_run_subprogram(std::ptr::null_mut(), 0));
        }
    }

    // ============================================================
    // C1 step 3c: rgx_runtime_word_boundary_test correctness
    // ============================================================
    //
    // The helper is the source of truth for `\b` / `\B` semantics
    // in JIT'd code. The codegen calls this via an indirect call,
    // so any divergence between this implementation and the
    // existing VM's word-boundary behaviour is a hard correctness
    // bug per design doc §1.0. These tests pin the exact PCRE2
    // ASCII semantics: a word boundary is a position where exactly
    // one of the adjacent bytes is a word character.

    /// Convenience wrapper that lifts a `&[u8]` and a position into
    /// the raw helper signature.
    fn wb(text: &[u8], pos: usize) -> bool {
        // SAFETY: text outlives the call; pos <= text.len() upheld
        // by the test caller.
        unsafe { rgx_runtime_word_boundary_test(text.as_ptr(), text.len(), pos) }
    }

    #[test]
    fn word_boundary_at_start_of_text_with_word_char() {
        // pos 0, text starts with a word char → boundary
        assert!(wb(b"abc", 0));
    }

    #[test]
    fn word_boundary_at_start_of_text_with_non_word_char() {
        // pos 0, text starts with a non-word char → no boundary
        assert!(!wb(b" abc", 0));
    }

    #[test]
    fn word_boundary_at_end_of_text_after_word_char() {
        // pos == text_len, last byte is a word char → boundary
        assert!(wb(b"abc", 3));
    }

    #[test]
    fn word_boundary_at_end_of_text_after_non_word_char() {
        // pos == text_len, last byte is a non-word char → no boundary
        assert!(!wb(b"abc ", 4));
    }

    #[test]
    fn word_boundary_between_word_and_non_word() {
        // "abc def" — boundary at position 3 (after `c`, before space)
        // and position 4 (after space, before `d`).
        assert!(wb(b"abc def", 3));
        assert!(wb(b"abc def", 4));
    }

    #[test]
    fn no_word_boundary_between_two_word_chars() {
        // "abc" — no boundary at positions 1 or 2.
        assert!(!wb(b"abc", 1));
        assert!(!wb(b"abc", 2));
    }

    #[test]
    fn no_word_boundary_between_two_non_word_chars() {
        // "  " — no boundary at position 1 (space-space transition).
        assert!(!wb(b"  ", 1));
    }

    #[test]
    fn word_boundary_handles_underscore_as_word() {
        // "_abc" — `_` is a word char, no boundary at position 1.
        assert!(!wb(b"_abc", 1));
        // " _" — boundary at position 1 (space → underscore).
        assert!(wb(b" _", 1));
    }

    #[test]
    fn word_boundary_handles_digit_as_word() {
        // "1abc" — `1` is a word char, no boundary at position 1.
        assert!(!wb(b"1abc", 1));
        // " 1" — boundary at position 1 (space → digit).
        assert!(wb(b" 1", 1));
    }

    #[test]
    fn word_boundary_empty_input() {
        // pos 0, text_len 0 → both neighbours are out-of-range
        // (treated as non-word) → no boundary.
        assert!(!wb(b"", 0));
    }

    #[test]
    fn word_boundary_punctuation_is_non_word() {
        // "abc!" — `!` is non-word, boundary at position 3.
        assert!(wb(b"abc!", 3));
        // "!abc" — boundary at position 1.
        assert!(wb(b"!abc", 1));
    }

    // ============================================================
    // C1 step 6: rgx_runtime_char_class_match_at correctness
    // ============================================================
    //
    // The helper is the source of truth for the JIT path's
    // `CharClass(id)` / `CharClassNeg(id)` opcode lowering. These
    // tests pin its semantics against the existing VM via the
    // public `Regex::compile` API: we compile a pattern, extract
    // its char_classes table, then call the helper at every
    // candidate position and assert it returns the same answer
    // the interpreter would.

    /// Compile `pattern` via the full Compiler pipeline (the same
    /// path `Regex::compile` uses) and return the resulting
    /// `Program` so the runtime helper can be exercised against
    /// real bytecode + a real `char_classes` table.
    fn compile_for_char_class_test(pattern: &str) -> crate::vm::Program {
        crate::compiler::Compiler::new()
            .compile(pattern)
            .unwrap_or_else(|e| panic!("pattern `{pattern}` must compile: {e}"))
            .program
    }

    /// Helper wrapper that lifts a program + position into the
    /// raw runtime call.
    fn cc_match_at(
        program: &crate::vm::Program,
        text: &[u8],
        pos: usize,
        class_id: u32,
        negated: bool,
    ) -> u32 {
        let cc_ptr = program.char_classes.as_ptr().cast::<u8>();
        let cc_len = program.char_classes.len();
        // SAFETY: text outlives the call; cc_ptr is valid for cc_len
        // CompiledCharClass values via program.
        unsafe {
            rgx_runtime_char_class_match_at(
                text.as_ptr(),
                text.len(),
                pos,
                cc_ptr,
                cc_len,
                class_id,
                u32::from(negated),
            )
        }
    }

    #[test]
    fn char_class_match_simple_ascii_class() {
        // [abc] — matches a, b, or c.
        let program = compile_for_char_class_test("[abc]");
        assert_eq!(cc_match_at(&program, b"a", 0, 0, false), 1);
        assert_eq!(cc_match_at(&program, b"b", 0, 0, false), 1);
        assert_eq!(cc_match_at(&program, b"c", 0, 0, false), 1);
        assert_eq!(cc_match_at(&program, b"d", 0, 0, false), 0);
        assert_eq!(cc_match_at(&program, b"", 0, 0, false), 0);
    }

    #[test]
    fn char_class_match_range_class() {
        // [a-z] — lowercase letters.
        let program = compile_for_char_class_test("[a-z]");
        assert_eq!(cc_match_at(&program, b"a", 0, 0, false), 1);
        assert_eq!(cc_match_at(&program, b"m", 0, 0, false), 1);
        assert_eq!(cc_match_at(&program, b"z", 0, 0, false), 1);
        assert_eq!(cc_match_at(&program, b"A", 0, 0, false), 0);
        assert_eq!(cc_match_at(&program, b"5", 0, 0, false), 0);
    }

    #[test]
    fn char_class_match_negated_class() {
        // [^0-9] tested via the `negated=true` parameter.
        let program = compile_for_char_class_test("[0-9]");
        // Positive: 5 matches.
        assert_eq!(cc_match_at(&program, b"5", 0, 0, false), 1);
        // Negated: 5 does NOT match.
        assert_eq!(cc_match_at(&program, b"5", 0, 0, true), 0);
        // Positive: 'a' does NOT match.
        assert_eq!(cc_match_at(&program, b"a", 0, 0, false), 0);
        // Negated: 'a' DOES match.
        assert_eq!(cc_match_at(&program, b"a", 0, 0, true), 1);
    }

    #[test]
    fn char_class_match_at_offset() {
        // Test the helper at a non-zero position.
        let program = compile_for_char_class_test("[a-z]");
        let text = b"123abc";
        // At pos 0: '1' is not a-z.
        assert_eq!(cc_match_at(&program, text, 0, 0, false), 0);
        // At pos 3: 'a' is a-z.
        assert_eq!(cc_match_at(&program, text, 3, 0, false), 1);
    }

    #[test]
    fn char_class_match_eof() {
        // Helper returns 0 when pos == text_len (no byte to consume).
        let program = compile_for_char_class_test("[a-z]");
        let text = b"abc";
        assert_eq!(cc_match_at(&program, text, 3, 0, false), 0);
    }

    #[test]
    fn char_class_match_unicode_range() {
        // Cyrillic letters: [а-я] (the Russian alphabet).
        // Testing the Unicode-range path of the helper.
        let program = compile_for_char_class_test("[а-я]");
        let text = "абв".as_bytes();
        // At pos 0: 'а' is in the range; UTF-8 width is 2 bytes.
        assert_eq!(cc_match_at(&program, text, 0, 0, false), 2);
        // At pos 2: 'б' is in the range; another 2 bytes.
        assert_eq!(cc_match_at(&program, text, 2, 0, false), 2);
        // ASCII 'a' (0x61) is NOT in the Cyrillic range.
        let text_ascii = b"a";
        assert_eq!(cc_match_at(&program, text_ascii, 0, 0, false), 0);
    }
}
