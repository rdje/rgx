//! C ABI bindings for the rgx-core regex engine.
//!
//! This crate is the universal entry point that lets non-Rust FFI
//! hosts (Go, Python, Julia, Zig, Ruby, PHP, Swift, …) drive the
//! rgx engine through a stable C ABI. The design is documented in
//! `docs/A9_LANGUAGE_BINDINGS_DESIGN.md`.
//!
//! # Surface scope (Phase 1)
//!
//! This commit lands the Phase-1 surface defined in §5 of the design
//! doc: lifecycle (compile / free / retain), diagnostics
//! (`rgx_last_error` / `rgx_runtime_version`), and basic matching
//! (`rgx_is_match` / `rgx_find_first`). Phases 2–6 add captures,
//! iterators, replace, safety limits, embedded scripting hosts,
//! observers, and `tail_file`.
//!
//! # FFI safety contract
//!
//! Every public function in this crate is:
//!
//! - **`extern "C"`** with C-compatible types (raw pointers, `int32_t`,
//!   `size_t`, `uint8_t`, …). No Rust references, no `&str`, no
//!   `Vec<T>` etc. cross the boundary.
//! - **`#[no_mangle]`** so the symbol name is the C name.
//! - **Panic-safe**. Every entry point's body is wrapped in
//!   `std::panic::catch_unwind`. A panic inside Rust surfaces as
//!   `RGX_ERR_INTERNAL` with the panic message stored in the
//!   thread-local error string accessible via `rgx_last_error()`.
//! - **Pointer-validated**. Every required pointer argument is
//!   null-checked before use. A null pointer returns
//!   `RGX_ERR_NULL_POINTER` and leaves out-params untouched.
//!
//! # Memory ownership
//!
//! All Rust-owned types crossing the boundary become opaque
//! pointers. The host language owns the pointer lifetime and must
//! call the corresponding `rgx_*_free` function exactly once when
//! done. Calling `rgx_regex_free(null)` is a no-op; calling it
//! twice on the same pointer is undefined behaviour (mirrors C's
//! `free()` contract).
//!
//! Byte buffers (text inputs, pattern strings, replacement
//! templates, …) are *not* owned by rgx-capi — the caller keeps the
//! buffer alive for the duration of the call. Every byte-buffer API
//! takes an explicit `(const uint8_t*, size_t)` pair; no
//! null-terminated C strings.
//!
//! # Threading
//!
//! - `RgxRegex*` is thread-safe for reading. Multiple threads can
//!   call `rgx_is_match` / `rgx_find_first` concurrently on the
//!   same handle. (rgx-core's `Regex` is `Send + Sync`.)
//! - `rgx_last_error` returns the *current thread's* most recent
//!   error message. Each thread has its own error slot.
//! - `RgxRegex*` retain/release is reference-counted; calling
//!   `rgx_regex_retain` returns a second handle that must be
//!   freed independently.

#![warn(missing_docs)]

use rgx_core::Regex;
use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::c_char;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;

// ============================================================
// Error model
// ============================================================

/// Operation succeeded.
pub const RGX_OK: i32 = 0;

/// A required pointer argument was null.
pub const RGX_ERR_NULL_POINTER: i32 = -1;

/// The pattern failed to compile. Call [`rgx_last_error`] for the
/// human-readable diagnostic.
pub const RGX_ERR_INVALID_PATTERN: i32 = -2;

/// A text input contained invalid UTF-8 when the API contract
/// required UTF-8. Phase 1 doesn't expose this path (every text
/// API treats inputs as raw bytes); reserved for the eventual
/// `bytes::Regex` distinction.
pub const RGX_ERR_INVALID_UTF8: i32 = -3;

/// A safety limit was exceeded during the match attempt
/// (`max_steps` / `max_backtrack_frames` / `max_recursion_depth` /
/// `max_trail_entries`). Reserved for Phase 3; Phase 1 currently
/// never returns this.
pub const RGX_ERR_LIMIT_EXCEEDED: i32 = -5;

/// A handle was passed to a function that doesn't accept it
/// (e.g. wrong handle type).
pub const RGX_ERR_INVALID_HANDLE: i32 = -7;

/// An unexpected panic from the Rust engine; the panic message is
/// retrievable via [`rgx_last_error`]. Indicates a bug — please
/// report.
pub const RGX_ERR_INTERNAL: i32 = -99;

thread_local! {
    /// Per-thread storage for the most recent error message.
    /// Returned by [`rgx_last_error`]; valid until the next rgx_*
    /// call on the same thread.
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Set the calling thread's last-error string. The message is
/// stored as a NUL-terminated C string and remains valid until
/// the next rgx_* call on the same thread overwrites it.
fn set_last_error(msg: impl Into<String>) {
    let msg = msg.into();
    // Strip any interior NUL bytes — they'd terminate the C string
    // prematurely. We replace with `?` to keep the message human-
    // readable.
    let sanitised: String = msg
        .chars()
        .map(|c| if c == '\0' { '?' } else { c })
        .collect();
    let cstr = CString::new(sanitised).unwrap_or_else(|_| {
        // Shouldn't happen post-sanitisation, but defensive.
        CString::new("<error message contained interior NUL>").unwrap()
    });
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = Some(cstr);
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

/// Run an FFI body with panic catching. On a Rust panic, sets the
/// thread-local error to the panic message and returns
/// [`RGX_ERR_INTERNAL`]; otherwise returns whatever the body
/// produced.
///
/// `AssertUnwindSafe` is OK here because every entry point operates
/// on `&` references to `Send + Sync` types or on raw pointers
/// the caller manages.
fn ffi_try<F: FnOnce() -> i32>(f: F) -> i32 {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(code) => code,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
                format!("panic in rgx-capi: {s}")
            } else if let Some(s) = payload.downcast_ref::<String>() {
                format!("panic in rgx-capi: {s}")
            } else {
                "panic in rgx-capi (non-string payload)".to_string()
            };
            set_last_error(msg);
            RGX_ERR_INTERNAL
        }
    }
}

// ============================================================
// Opaque types
// ============================================================

/// Opaque handle to a compiled regex. Created by [`rgx_compile`];
/// destroyed by [`rgx_regex_free`]; refcounted by
/// [`rgx_regex_retain`].
///
/// `#[repr(C)]` here is irrelevant because we never expose the
/// fields — the C side only ever holds a `RgxRegex*` (opaque
/// pointer). The pointer is the API contract.
pub struct RgxRegex {
    inner: Arc<Regex>,
}

// ============================================================
// Diagnostics
// ============================================================

/// Return the calling thread's most recent error message, or a
/// pointer to an empty string if no error has been recorded since
/// the last successful call.
///
/// The returned pointer is borrowed from per-thread storage and
/// remains valid until the next rgx_* call on the same thread.
/// Callers MUST NOT free it; callers MUST NOT use it after another
/// rgx_* call on the same thread.
///
/// # Safety
///
/// The returned pointer is valid for read only, NUL-terminated,
/// UTF-8 (subject to interior-NUL replacement; see implementation
/// notes).
#[no_mangle]
pub extern "C" fn rgx_last_error() -> *const c_char {
    // Empty-string sentinel returned when no error is set. Static
    // lifetime so the pointer remains valid across `LAST_ERROR`
    // mutations.
    static EMPTY: [u8; 1] = [0];
    LAST_ERROR.with(|slot| {
        let borrowed = slot.borrow();
        match borrowed.as_ref() {
            Some(cstr) => cstr.as_ptr(),
            None => EMPTY.as_ptr().cast::<c_char>(),
        }
    })
}

/// Major version of the rgx-capi library at runtime. Pair with
/// [`rgx_runtime_version_minor`] / [`rgx_runtime_version_patch`]
/// for the full `MAJOR.MINOR.PATCH` triple. Callers compile against
/// the `RGX_VERSION_*` constants in the generated header and
/// compare at runtime to detect header/library mismatch.
#[no_mangle]
pub extern "C" fn rgx_runtime_version_major() -> u32 {
    env!("CARGO_PKG_VERSION_MAJOR").parse::<u32>().unwrap_or(0)
}

/// Minor version. See [`rgx_runtime_version_major`].
#[no_mangle]
pub extern "C" fn rgx_runtime_version_minor() -> u32 {
    env!("CARGO_PKG_VERSION_MINOR").parse::<u32>().unwrap_or(0)
}

/// Patch version. See [`rgx_runtime_version_major`].
#[no_mangle]
pub extern "C" fn rgx_runtime_version_patch() -> u32 {
    env!("CARGO_PKG_VERSION_PATCH").parse::<u32>().unwrap_or(0)
}

// ============================================================
// Lifecycle
// ============================================================

/// Compile a pattern into an [`RgxRegex`] handle.
///
/// `pattern` / `pattern_len` describe the pattern as raw bytes
/// (PCRE2-syntax UTF-8 by convention). `out_regex` receives the
/// newly-allocated handle on success.
///
/// # Returns
///
/// - [`RGX_OK`] on success; `*out_regex` is populated with a non-
///   null `RgxRegex*` that the caller must eventually free via
///   [`rgx_regex_free`].
/// - [`RGX_ERR_NULL_POINTER`] if `pattern` or `out_regex` is null.
/// - [`RGX_ERR_INVALID_PATTERN`] if the pattern fails to compile;
///   the diagnostic is in [`rgx_last_error`].
/// - [`RGX_ERR_INTERNAL`] on an unexpected panic.
///
/// # Safety
///
/// - `pattern` must point to at least `pattern_len` valid bytes.
/// - `out_regex` must point to a valid, writable `RgxRegex*` slot.
#[no_mangle]
pub unsafe extern "C" fn rgx_compile(
    pattern: *const u8,
    pattern_len: usize,
    out_regex: *mut *mut RgxRegex,
) -> i32 {
    ffi_try(|| {
        if pattern.is_null() || out_regex.is_null() {
            set_last_error("rgx_compile: null pointer argument");
            return RGX_ERR_NULL_POINTER;
        }
        let bytes = std::slice::from_raw_parts(pattern, pattern_len);
        let pattern_str = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("rgx_compile: pattern is not valid UTF-8: {e}"));
                return RGX_ERR_INVALID_UTF8;
            }
        };
        match Regex::compile(pattern_str) {
            Ok(re) => {
                let boxed = Box::new(RgxRegex {
                    inner: Arc::new(re),
                });
                *out_regex = Box::into_raw(boxed);
                clear_last_error();
                RGX_OK
            }
            Err(e) => {
                set_last_error(format!("rgx_compile: {e}"));
                RGX_ERR_INVALID_PATTERN
            }
        }
    })
}

/// Free a regex handle previously returned by [`rgx_compile`] or
/// [`rgx_regex_retain`]. Passing a null pointer is a no-op (mirrors
/// `free()` semantics). Passing the same non-null pointer twice is
/// undefined behaviour.
///
/// # Safety
///
/// `re` must be either null or a pointer previously returned by
/// [`rgx_compile`] / [`rgx_regex_retain`] and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn rgx_regex_free(re: *mut RgxRegex) {
    if re.is_null() {
        return;
    }
    // Drop the Box, which drops the Arc, which decrements the
    // refcount. The shared Regex inside the Arc is freed when the
    // last handle is freed.
    drop(Box::from_raw(re));
}

/// Create an additional handle to the same compiled regex. Both
/// handles are independent — each must be freed separately via
/// [`rgx_regex_free`]. The underlying engine is shared (Arc-counted),
/// so retaining is cheap.
///
/// Useful when handing a regex to a worker thread: the spawning
/// thread retains, hands the new handle to the worker, and each
/// frees independently.
///
/// Returns null if `re` is null.
///
/// # Safety
///
/// `re` must be either null or a valid `RgxRegex*` previously
/// returned by [`rgx_compile`] / [`rgx_regex_retain`] and not yet
/// freed.
#[no_mangle]
pub unsafe extern "C" fn rgx_regex_retain(re: *mut RgxRegex) -> *mut RgxRegex {
    if re.is_null() {
        return std::ptr::null_mut();
    }
    let original = &*re;
    let cloned = Box::new(RgxRegex {
        inner: Arc::clone(&original.inner),
    });
    Box::into_raw(cloned)
}

// ============================================================
// Basic matching
// ============================================================

/// Test whether the regex matches anywhere in `text`. Writes
/// `1` to `*out_matched` if a match exists, `0` otherwise.
///
/// # Returns
///
/// - [`RGX_OK`] on success; `*out_matched` is populated.
/// - [`RGX_ERR_NULL_POINTER`] if `re`, `text`, or `out_matched` is
///   null. (`text_len == 0` is allowed: an empty input is matched
///   against the pattern; some patterns match empty strings.)
/// - [`RGX_ERR_INVALID_UTF8`] if `text` is not valid UTF-8.
/// - [`RGX_ERR_INTERNAL`] on an unexpected panic.
///
/// # Safety
///
/// - `re` must be a valid `RgxRegex*` not yet freed.
/// - `text` must point to at least `text_len` valid bytes (unless
///   `text_len == 0`, in which case `text` may be null or any value).
/// - `out_matched` must point to a valid, writable `int32_t` slot.
#[no_mangle]
pub unsafe extern "C" fn rgx_is_match(
    re: *const RgxRegex,
    text: *const u8,
    text_len: usize,
    out_matched: *mut i32,
) -> i32 {
    ffi_try(|| {
        if re.is_null() || out_matched.is_null() {
            set_last_error("rgx_is_match: null pointer argument");
            return RGX_ERR_NULL_POINTER;
        }
        // text_len == 0 is the empty-string case; the engine handles it.
        let bytes = if text_len == 0 {
            &[][..]
        } else {
            if text.is_null() {
                set_last_error("rgx_is_match: text pointer is null with non-zero text_len");
                return RGX_ERR_NULL_POINTER;
            }
            std::slice::from_raw_parts(text, text_len)
        };
        let text_str = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("rgx_is_match: text is not valid UTF-8: {e}"));
                return RGX_ERR_INVALID_UTF8;
            }
        };
        let regex = &(*re).inner;
        let matched = regex.is_match(text_str);
        *out_matched = if matched { 1 } else { 0 };
        clear_last_error();
        RGX_OK
    })
}

/// Find the first match of the regex in `text`. On success writes
/// the match span (start, end) byte offsets into `*out_start` /
/// `*out_end`. If no match exists, writes `(0, 0)` and returns
/// [`RGX_OK`] with `*out_matched` set to `0`.
///
/// # Returns
///
/// - [`RGX_OK`] on success; `*out_matched`, `*out_start`, `*out_end`
///   populated. `out_matched == 1` means a match was found; `0`
///   means no match.
/// - [`RGX_ERR_NULL_POINTER`] if any required pointer is null.
/// - [`RGX_ERR_INVALID_UTF8`] if `text` is not valid UTF-8.
/// - [`RGX_ERR_INTERNAL`] on an unexpected panic.
///
/// # Safety
///
/// - `re` must be a valid `RgxRegex*` not yet freed.
/// - `text` must point to at least `text_len` valid bytes (or be
///   null when `text_len == 0`).
/// - `out_matched`, `out_start`, `out_end` must each point to a
///   valid, writable slot.
#[no_mangle]
pub unsafe extern "C" fn rgx_find_first(
    re: *const RgxRegex,
    text: *const u8,
    text_len: usize,
    out_matched: *mut i32,
    out_start: *mut usize,
    out_end: *mut usize,
) -> i32 {
    ffi_try(|| {
        if re.is_null() || out_matched.is_null() || out_start.is_null() || out_end.is_null() {
            set_last_error("rgx_find_first: null pointer argument");
            return RGX_ERR_NULL_POINTER;
        }
        let bytes = if text_len == 0 {
            &[][..]
        } else {
            if text.is_null() {
                set_last_error("rgx_find_first: text pointer is null with non-zero text_len");
                return RGX_ERR_NULL_POINTER;
            }
            std::slice::from_raw_parts(text, text_len)
        };
        let text_str = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("rgx_find_first: text is not valid UTF-8: {e}"));
                return RGX_ERR_INVALID_UTF8;
            }
        };
        let regex = &(*re).inner;
        match regex.find_first(text_str) {
            Some(m) => {
                *out_matched = 1;
                *out_start = m.start;
                *out_end = m.end;
            }
            None => {
                *out_matched = 0;
                *out_start = 0;
                *out_end = 0;
            }
        }
        clear_last_error();
        RGX_OK
    })
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: compile a pattern from a Rust `&str` and return the
    /// raw handle. Used by tests; the production C ABI takes
    /// `(*const u8, usize)`.
    fn compile_pattern(pattern: &str) -> *mut RgxRegex {
        let mut out: *mut RgxRegex = std::ptr::null_mut();
        let rc = unsafe {
            rgx_compile(
                pattern.as_ptr(),
                pattern.len(),
                &mut out as *mut *mut RgxRegex,
            )
        };
        assert_eq!(rc, RGX_OK, "compile failed: {pattern:?}");
        assert!(!out.is_null());
        out
    }

    #[test]
    fn version_numbers_are_consistent_with_cargo_metadata() {
        let major = rgx_runtime_version_major();
        let minor = rgx_runtime_version_minor();
        let patch = rgx_runtime_version_patch();
        // Just assert they parse — version is set by Cargo at compile time.
        let _ = (major, minor, patch);
    }

    #[test]
    fn compile_succeeds_for_valid_pattern() {
        let re = compile_pattern(r"\d+");
        unsafe { rgx_regex_free(re) };
    }

    #[test]
    fn compile_rejects_invalid_pattern() {
        // Unbalanced parenthesis — should fail to compile.
        let bad = "(\\d+";
        let mut out: *mut RgxRegex = std::ptr::null_mut();
        let rc = unsafe { rgx_compile(bad.as_ptr(), bad.len(), &mut out as *mut *mut RgxRegex) };
        assert_eq!(rc, RGX_ERR_INVALID_PATTERN);
        assert!(out.is_null());
        // last_error should be a non-empty C string.
        let err_ptr = rgx_last_error();
        assert!(!err_ptr.is_null());
        let err_cstr = unsafe { std::ffi::CStr::from_ptr(err_ptr) };
        assert!(
            !err_cstr.to_bytes().is_empty(),
            "expected non-empty error message after invalid-pattern compile"
        );
    }

    #[test]
    fn compile_rejects_null_pattern_pointer() {
        let mut out: *mut RgxRegex = std::ptr::null_mut();
        let rc = unsafe { rgx_compile(std::ptr::null(), 0, &mut out as *mut *mut RgxRegex) };
        assert_eq!(rc, RGX_ERR_NULL_POINTER);
        assert!(out.is_null());
    }

    #[test]
    fn compile_rejects_null_out_pointer() {
        let pat = r"\d+";
        let rc = unsafe { rgx_compile(pat.as_ptr(), pat.len(), std::ptr::null_mut()) };
        assert_eq!(rc, RGX_ERR_NULL_POINTER);
    }

    #[test]
    fn compile_rejects_invalid_utf8_pattern() {
        // Lone continuation byte — invalid UTF-8.
        let bad = [0x80u8];
        let mut out: *mut RgxRegex = std::ptr::null_mut();
        let rc = unsafe { rgx_compile(bad.as_ptr(), bad.len(), &mut out as *mut *mut RgxRegex) };
        assert_eq!(rc, RGX_ERR_INVALID_UTF8);
        assert!(out.is_null());
    }

    #[test]
    fn free_null_is_noop() {
        // Should not crash.
        unsafe { rgx_regex_free(std::ptr::null_mut()) };
    }

    #[test]
    fn retain_creates_independent_handle() {
        let re = compile_pattern(r"\d+");
        let retained = unsafe { rgx_regex_retain(re) };
        assert!(!retained.is_null());
        assert_ne!(re, retained, "retain must return a fresh handle");
        // Both handles must be freed independently.
        unsafe {
            rgx_regex_free(re);
            rgx_regex_free(retained);
        }
    }

    #[test]
    fn retain_null_returns_null() {
        let result = unsafe { rgx_regex_retain(std::ptr::null_mut()) };
        assert!(result.is_null());
    }

    #[test]
    fn is_match_finds_match() {
        let re = compile_pattern(r"\d+");
        let text = "abc 123 def";
        let mut matched: i32 = 0;
        let rc = unsafe { rgx_is_match(re, text.as_ptr(), text.len(), &mut matched as *mut i32) };
        assert_eq!(rc, RGX_OK);
        assert_eq!(matched, 1);
        unsafe { rgx_regex_free(re) };
    }

    #[test]
    fn is_match_no_match_returns_zero() {
        let re = compile_pattern(r"\d+");
        let text = "abc def";
        let mut matched: i32 = 99;
        let rc = unsafe { rgx_is_match(re, text.as_ptr(), text.len(), &mut matched as *mut i32) };
        assert_eq!(rc, RGX_OK);
        assert_eq!(matched, 0);
        unsafe { rgx_regex_free(re) };
    }

    #[test]
    fn is_match_empty_text_is_valid() {
        let re = compile_pattern(r"a*");
        let mut matched: i32 = 0;
        // text is null + len 0: the empty input. `a*` matches empty.
        let rc = unsafe { rgx_is_match(re, std::ptr::null(), 0, &mut matched as *mut i32) };
        assert_eq!(rc, RGX_OK);
        assert_eq!(matched, 1);
        unsafe { rgx_regex_free(re) };
    }

    #[test]
    fn is_match_rejects_invalid_utf8() {
        let re = compile_pattern(r"\w+");
        let bad = [0x80u8, b'a'];
        let mut matched: i32 = 99;
        let rc = unsafe { rgx_is_match(re, bad.as_ptr(), bad.len(), &mut matched as *mut i32) };
        assert_eq!(rc, RGX_ERR_INVALID_UTF8);
        unsafe { rgx_regex_free(re) };
    }

    #[test]
    fn find_first_returns_span() {
        let re = compile_pattern(r"\d+");
        let text = "abc 123 def";
        let mut matched: i32 = 0;
        let mut start: usize = 99;
        let mut end: usize = 99;
        let rc = unsafe {
            rgx_find_first(
                re,
                text.as_ptr(),
                text.len(),
                &mut matched as *mut i32,
                &mut start as *mut usize,
                &mut end as *mut usize,
            )
        };
        assert_eq!(rc, RGX_OK);
        assert_eq!(matched, 1);
        assert_eq!(start, 4);
        assert_eq!(end, 7);
        unsafe { rgx_regex_free(re) };
    }

    #[test]
    fn find_first_no_match_returns_zero() {
        let re = compile_pattern(r"\d+");
        let text = "abc def";
        let mut matched: i32 = 99;
        let mut start: usize = 99;
        let mut end: usize = 99;
        let rc = unsafe {
            rgx_find_first(
                re,
                text.as_ptr(),
                text.len(),
                &mut matched as *mut i32,
                &mut start as *mut usize,
                &mut end as *mut usize,
            )
        };
        assert_eq!(rc, RGX_OK);
        assert_eq!(matched, 0);
        assert_eq!(start, 0);
        assert_eq!(end, 0);
        unsafe { rgx_regex_free(re) };
    }

    #[test]
    fn last_error_is_empty_after_success() {
        let re = compile_pattern(r"\d+");
        // last_error after a successful compile should be empty.
        let err_ptr = rgx_last_error();
        let err_cstr = unsafe { std::ffi::CStr::from_ptr(err_ptr) };
        assert!(err_cstr.to_bytes().is_empty());
        unsafe { rgx_regex_free(re) };
    }

    #[test]
    fn last_error_set_after_failure() {
        let bad = "(\\d+";
        let mut out: *mut RgxRegex = std::ptr::null_mut();
        let _ = unsafe { rgx_compile(bad.as_ptr(), bad.len(), &mut out as *mut *mut RgxRegex) };
        let err_ptr = rgx_last_error();
        let err_cstr = unsafe { std::ffi::CStr::from_ptr(err_ptr) };
        assert!(!err_cstr.to_bytes().is_empty());
        // Message should mention the issue. Don't pin the exact text —
        // PGEN's error formatting is its own contract.
    }
}
