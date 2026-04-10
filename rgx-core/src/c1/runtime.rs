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

/// Test whether a single byte matches a compiled character class.
///
/// **Step 1: signature-only stub.** Implementation lands in step 6
/// alongside `CharClass` opcode lowering in `c1/codegen.rs`. The real
/// version will look up the compiled char class by ID in the
/// program's char-class table and test the byte against the ASCII
/// bitmap (or the Unicode range table for non-ASCII bytes).
///
/// # Parameters
/// - `char_classes`: pointer to the program's `[CompiledCharClass]`
///   slice. Borrowed for the duration of the call; the JIT'd caller
///   keeps the program alive.
/// - `class_id`: index into the char-class slice.
/// - `byte`: the input byte to test.
///
/// # Returns
/// `true` (1) if the byte is in the class, `false` (0) otherwise.
///
/// # Safety
/// `char_classes` must point to a valid `[CompiledCharClass]` slice
/// with at least `class_id + 1` elements. The caller (always JIT'd
/// code emitted from `c1/codegen.rs`) is responsible for satisfying
/// this invariant.
#[no_mangle]
pub unsafe extern "C" fn rgx_runtime_char_class_test(
    char_classes: *const u8,
    class_id: u32,
    byte: u8,
) -> bool {
    // Step 1 stub. The real implementation in step 6 will:
    //   let classes = std::slice::from_raw_parts(
    //       char_classes.cast::<CompiledCharClass>(),
    //       class_id as usize + 1,
    //   );
    //   classes[class_id as usize].matches(byte)
    let _ = (char_classes, class_id, byte);
    false
}

/// Test whether the position is at a word boundary (`\b`).
///
/// **Step 1: signature-only stub.** Implementation lands in step 7.
/// The real version will compare the word-character status of the
/// bytes at `pos - 1` and `pos`, returning `true` if exactly one of
/// them is a word character (or one is the start/end of input).
///
/// # Safety
/// `text` must point to a valid `[u8]` of length `text_len`. `pos`
/// must be `<= text_len`.
#[no_mangle]
pub unsafe extern "C" fn rgx_runtime_word_boundary_test(
    text: *const u8,
    text_len: usize,
    pos: usize,
) -> bool {
    let _ = (text, text_len, pos);
    false
}

/// Match a multi-byte literal character at the given position.
///
/// **Step 1: signature-only stub.** Implementation lands in step 6
/// alongside the `Char` opcode lowering for non-ASCII codepoints.
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
    let _ = (text, text_len, pos, expected, expected_len);
    false
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

    /// Verify the signature stubs link and can be called. They all
    /// return `false` (the safe default) at step 1; the real
    /// behaviours land in steps 6 and 7. The point of this test is
    /// to catch any signature drift between the stubs declared here
    /// and the function signatures in any future caller — a stub
    /// that fails to link is a hard error before any C1 dispatch is
    /// wired up.
    #[test]
    fn step_one_stubs_are_callable_and_return_safe_defaults() {
        // SAFETY: all stubs are signature-only and ignore their
        // arguments at step 1. Passing null pointers and zero
        // lengths is safe because the implementations don't
        // dereference anything.
        unsafe {
            assert!(!rgx_runtime_char_class_test(std::ptr::null(), 0, 0));
            assert!(!rgx_runtime_word_boundary_test(std::ptr::null(), 0, 0));
            assert!(!rgx_runtime_match_multibyte_char(
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
                0
            ));
            assert!(!rgx_runtime_compare_capture(std::ptr::null_mut(), 0));
            assert!(!rgx_runtime_run_subprogram(std::ptr::null_mut(), 0));
        }
    }
}
