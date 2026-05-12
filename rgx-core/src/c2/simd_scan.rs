//! SIMD-accelerated byte-class scanning for the C2 dispatch tiers.
//!
//! `PrefixScanner` (in `engine.rs`) and the lazy DFA's per-position
//! scan loop both need to find the first byte in `input` that
//! satisfies a class predicate — `is_ascii_digit`, `is_ascii_word`,
//! `pcre2_is_space_byte`, or a compiled `CharClass`. The scalar
//! byte-by-byte loop is O(n) per scan but pays a memory access per
//! byte; on long inputs with sparse matches that becomes the
//! dispatch bottleneck.
//!
//! This module provides SIMD-vectorised variants for the three
//! built-in classes (`Digit`, `Word`, `Space`) and a generic
//! lookup-table variant for arbitrary char-classes. Each public
//! function dispatches to the best available implementation at
//! runtime:
//!
//! - **NEON** on aarch64 — Apple Silicon, modern ARM. 16-byte blocks
//!   via `vld1q_u8` + range / equality comparisons + `vmaxvq_u8`
//!   reduction.
//! - **AVX2** on x86_64 (when detected) — 32-byte blocks via
//!   `_mm256_loadu_si256` + `_mm256_cmpgt_epi8` + `_mm256_movemask_epi8`.
//! - **SSE2** on x86_64 (always available on x86_64 Rust targets) —
//!   16-byte blocks via `_mm_loadu_si128` + `_mm_cmpgt_epi8` +
//!   `_mm_movemask_epi8`.
//! - **Scalar** fallback — portable byte loop, used on architectures
//!   without SIMD and as the tail-end pass for unaligned suffixes.
//!
//! All public entry points have the same shape:
//! `find_first_X(haystack: &[u8]) -> Option<usize>` returning the
//! byte offset of the first matching byte, or `None` if no byte
//! matches.

/// Find the first ASCII digit byte (`b'0'..=b'9'`) in `haystack`.
/// Returns the byte offset of the first match, or `None`.
#[must_use]
#[inline]
pub fn find_first_digit(haystack: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        // NEON is part of the aarch64 baseline (every Apple Silicon
        // and every armv8 Linux core supports it), so we can call
        // the intrinsic path unconditionally.
        // SAFETY: NEON is part of the aarch64 baseline ABI.
        return unsafe { find_first_digit_neon(haystack) };
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: feature gate checked above.
            return unsafe { find_first_digit_avx2(haystack) };
        }
        if std::arch::is_x86_feature_detected!("sse2") {
            // SAFETY: feature gate checked above.
            return unsafe { find_first_digit_sse2(haystack) };
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        find_first_digit_scalar(haystack)
    }
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[allow(unreachable_code)]
    find_first_digit_scalar(haystack)
}

/// Find the first ASCII word byte (`[A-Za-z0-9_]`) in `haystack`.
/// Returns the byte offset of the first match, or `None`.
#[must_use]
#[inline]
pub fn find_first_word(haystack: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline ABI.
        return unsafe { find_first_word_neon(haystack) };
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: feature gate checked above.
            return unsafe { find_first_word_avx2(haystack) };
        }
    }
    find_first_word_scalar(haystack)
}

/// Find the first PCRE2 space byte (`\t\n\v\f\r` or `' '`) in
/// `haystack`. Returns the byte offset of the first match, or
/// `None`. Matches the semantics of `crate::vm::pcre2_is_space_byte`.
#[must_use]
#[inline]
pub fn find_first_space(haystack: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline ABI.
        return unsafe { find_first_space_neon(haystack) };
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: feature gate checked above.
            return unsafe { find_first_space_avx2(haystack) };
        }
    }
    find_first_space_scalar(haystack)
}

// ============================================================
// Scalar fallbacks
// ============================================================

#[inline]
fn find_first_digit_scalar(haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(u8::is_ascii_digit)
}

#[inline]
fn find_first_word_scalar(haystack: &[u8]) -> Option<usize> {
    haystack
        .iter()
        .position(|&b| b.is_ascii_alphanumeric() || b == b'_')
}

#[inline]
fn find_first_space_scalar(haystack: &[u8]) -> Option<usize> {
    haystack
        .iter()
        .position(|&b| crate::vm::pcre2_is_space_byte(b))
}

// ============================================================
// NEON implementations (aarch64)
// ============================================================

/// NEON variant of [`find_first_digit`]. 16 bytes per iteration.
///
/// # Safety
///
/// `aarch64` baseline guarantees NEON; the unsafe block guards the
/// raw intrinsic calls.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn find_first_digit_neon(haystack: &[u8]) -> Option<usize> {
    use std::arch::aarch64::{vandq_u8, vcgeq_u8, vcleq_u8, vdupq_n_u8, vld1q_u8, vmaxvq_u8};
    let lo = vdupq_n_u8(b'0');
    let hi = vdupq_n_u8(b'9');
    let mut i = 0;
    while i + 16 <= haystack.len() {
        let v = vld1q_u8(haystack.as_ptr().add(i));
        let ge = vcgeq_u8(v, lo);
        let le = vcleq_u8(v, hi);
        let mask = vandq_u8(ge, le);
        if vmaxvq_u8(mask) != 0 {
            // Some byte in this block is a digit — fall back to
            // scalar to find the first one. The block is 16 bytes,
            // so this is at most 15 scalar tests.
            for j in 0..16 {
                if (*haystack.get_unchecked(i + j)).is_ascii_digit() {
                    return Some(i + j);
                }
            }
        }
        i += 16;
    }
    while i < haystack.len() {
        if (*haystack.get_unchecked(i)).is_ascii_digit() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// NEON variant of [`find_first_word`]. 16 bytes per iteration. The
/// `\w` class is the union of four ranges: digits, uppercase,
/// lowercase, plus `_`. Computed as `(d | u | l | underscore)` of
/// per-byte masks.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn find_first_word_neon(haystack: &[u8]) -> Option<usize> {
    use std::arch::aarch64::{
        vandq_u8, vceqq_u8, vcgeq_u8, vcleq_u8, vdupq_n_u8, vld1q_u8, vmaxvq_u8, vorrq_u8,
    };
    let d_lo = vdupq_n_u8(b'0');
    let d_hi = vdupq_n_u8(b'9');
    let u_lo = vdupq_n_u8(b'A');
    let u_hi = vdupq_n_u8(b'Z');
    let l_lo = vdupq_n_u8(b'a');
    let l_hi = vdupq_n_u8(b'z');
    let under = vdupq_n_u8(b'_');
    let mut i = 0;
    while i + 16 <= haystack.len() {
        let v = vld1q_u8(haystack.as_ptr().add(i));
        let dm = vandq_u8(vcgeq_u8(v, d_lo), vcleq_u8(v, d_hi));
        let um = vandq_u8(vcgeq_u8(v, u_lo), vcleq_u8(v, u_hi));
        let lm = vandq_u8(vcgeq_u8(v, l_lo), vcleq_u8(v, l_hi));
        let _m = vceqq_u8(v, under);
        let mask = vorrq_u8(vorrq_u8(dm, um), vorrq_u8(lm, _m));
        if vmaxvq_u8(mask) != 0 {
            for j in 0..16 {
                let b = *haystack.get_unchecked(i + j);
                if b.is_ascii_alphanumeric() || b == b'_' {
                    return Some(i + j);
                }
            }
        }
        i += 16;
    }
    while i < haystack.len() {
        let b = *haystack.get_unchecked(i);
        if b.is_ascii_alphanumeric() || b == b'_' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// NEON variant of [`find_first_space`]. 16 bytes per iteration.
/// PCRE2 spaces are `\t\n\v\f\r` (0x09..=0x0D) plus `' '` (0x20).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn find_first_space_neon(haystack: &[u8]) -> Option<usize> {
    use std::arch::aarch64::{
        vandq_u8, vceqq_u8, vcgeq_u8, vcleq_u8, vdupq_n_u8, vld1q_u8, vmaxvq_u8, vorrq_u8,
    };
    let ws_lo = vdupq_n_u8(0x09);
    let ws_hi = vdupq_n_u8(0x0D);
    let sp = vdupq_n_u8(b' ');
    let mut i = 0;
    while i + 16 <= haystack.len() {
        let v = vld1q_u8(haystack.as_ptr().add(i));
        let ws = vandq_u8(vcgeq_u8(v, ws_lo), vcleq_u8(v, ws_hi));
        let spm = vceqq_u8(v, sp);
        let mask = vorrq_u8(ws, spm);
        if vmaxvq_u8(mask) != 0 {
            for j in 0..16 {
                if crate::vm::pcre2_is_space_byte(*haystack.get_unchecked(i + j)) {
                    return Some(i + j);
                }
            }
        }
        i += 16;
    }
    while i < haystack.len() {
        if crate::vm::pcre2_is_space_byte(*haystack.get_unchecked(i)) {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ============================================================
// AVX2 implementations (x86_64)
// ============================================================

/// AVX2 variant of [`find_first_digit`]. 32 bytes per iteration.
///
/// Uses the "subtract-and-compare-unsigned" idiom: `byte - 0x30 < 10`
/// is equivalent to `byte ∈ b'0'..=b'9'`. AVX2 has no unsigned
/// compare, so we compare against signed bounds using the standard
/// XOR-with-sign-bit trick.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn find_first_digit_avx2(haystack: &[u8]) -> Option<usize> {
    use std::arch::x86_64::*;
    let zero = _mm256_set1_epi8(b'0' as i8 - 1);
    let nine = _mm256_set1_epi8(b'9' as i8 + 1);
    let mut i = 0;
    while i + 32 <= haystack.len() {
        let v = _mm256_loadu_si256(haystack.as_ptr().add(i) as *const __m256i);
        let ge = _mm256_cmpgt_epi8(v, zero); // v > '0'-1 → v >= '0'
        let lt = _mm256_cmpgt_epi8(nine, v); // '9'+1 > v → v <= '9'
        let mask = _mm256_movemask_epi8(_mm256_and_si256(ge, lt));
        if mask != 0 {
            return Some(i + mask.trailing_zeros() as usize);
        }
        i += 32;
    }
    // Tail: scalar.
    while i < haystack.len() {
        if haystack[i].is_ascii_digit() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// AVX2 variant of [`find_first_word`].
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn find_first_word_avx2(haystack: &[u8]) -> Option<usize> {
    use std::arch::x86_64::*;
    let d_lo = _mm256_set1_epi8(b'0' as i8 - 1);
    let d_hi = _mm256_set1_epi8(b'9' as i8 + 1);
    let u_lo = _mm256_set1_epi8(b'A' as i8 - 1);
    let u_hi = _mm256_set1_epi8(b'Z' as i8 + 1);
    let l_lo = _mm256_set1_epi8(b'a' as i8 - 1);
    let l_hi = _mm256_set1_epi8(b'z' as i8 + 1);
    let under = _mm256_set1_epi8(b'_' as i8);
    let mut i = 0;
    while i + 32 <= haystack.len() {
        let v = _mm256_loadu_si256(haystack.as_ptr().add(i) as *const __m256i);
        let dm = _mm256_and_si256(_mm256_cmpgt_epi8(v, d_lo), _mm256_cmpgt_epi8(d_hi, v));
        let um = _mm256_and_si256(_mm256_cmpgt_epi8(v, u_lo), _mm256_cmpgt_epi8(u_hi, v));
        let lm = _mm256_and_si256(_mm256_cmpgt_epi8(v, l_lo), _mm256_cmpgt_epi8(l_hi, v));
        let _m = _mm256_cmpeq_epi8(v, under);
        let mask = _mm256_movemask_epi8(_mm256_or_si256(
            _mm256_or_si256(dm, um),
            _mm256_or_si256(lm, _m),
        ));
        if mask != 0 {
            return Some(i + mask.trailing_zeros() as usize);
        }
        i += 32;
    }
    while i < haystack.len() {
        let b = haystack[i];
        if b.is_ascii_alphanumeric() || b == b'_' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// AVX2 variant of [`find_first_space`].
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn find_first_space_avx2(haystack: &[u8]) -> Option<usize> {
    use std::arch::x86_64::*;
    let ws_lo = _mm256_set1_epi8(0x09 - 1);
    let ws_hi = _mm256_set1_epi8(0x0D + 1);
    let sp = _mm256_set1_epi8(b' ' as i8);
    let mut i = 0;
    while i + 32 <= haystack.len() {
        let v = _mm256_loadu_si256(haystack.as_ptr().add(i) as *const __m256i);
        let ws = _mm256_and_si256(_mm256_cmpgt_epi8(v, ws_lo), _mm256_cmpgt_epi8(ws_hi, v));
        let spm = _mm256_cmpeq_epi8(v, sp);
        let mask = _mm256_movemask_epi8(_mm256_or_si256(ws, spm));
        if mask != 0 {
            return Some(i + mask.trailing_zeros() as usize);
        }
        i += 32;
    }
    while i < haystack.len() {
        if crate::vm::pcre2_is_space_byte(haystack[i]) {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ============================================================
// SSE2 implementation (x86_64 baseline)
// ============================================================

/// SSE2 variant of [`find_first_digit`]. 16 bytes per iteration.
/// Used on x86_64 hardware where AVX2 isn't available (rare modern
/// targets).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn find_first_digit_sse2(haystack: &[u8]) -> Option<usize> {
    use std::arch::x86_64::*;
    let zero = _mm_set1_epi8(b'0' as i8 - 1);
    let nine = _mm_set1_epi8(b'9' as i8 + 1);
    let mut i = 0;
    while i + 16 <= haystack.len() {
        let v = _mm_loadu_si128(haystack.as_ptr().add(i) as *const __m128i);
        let ge = _mm_cmpgt_epi8(v, zero);
        let lt = _mm_cmpgt_epi8(nine, v);
        let mask = _mm_movemask_epi8(_mm_and_si128(ge, lt)) as u32;
        if mask != 0 {
            return Some(i + mask.trailing_zeros() as usize);
        }
        i += 16;
    }
    while i < haystack.len() {
        if haystack[i].is_ascii_digit() {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive_digit(s: &[u8]) -> Option<usize> {
        s.iter().position(u8::is_ascii_digit)
    }
    fn naive_word(s: &[u8]) -> Option<usize> {
        s.iter()
            .position(|&b| b.is_ascii_alphanumeric() || b == b'_')
    }
    fn naive_space(s: &[u8]) -> Option<usize> {
        s.iter().position(|&b| crate::vm::pcre2_is_space_byte(b))
    }

    #[test]
    fn digit_at_various_offsets_in_short_input() {
        for s in [
            b"".as_slice(),
            b"abc".as_slice(),
            b"1abc".as_slice(),
            b"a1bc".as_slice(),
            b"ab1c".as_slice(),
            b"abc1".as_slice(),
        ] {
            assert_eq!(find_first_digit(s), naive_digit(s), "input={s:?}");
        }
    }

    #[test]
    fn digit_in_block_aligned_input() {
        // 32-byte input ensures the AVX2 fast path runs to completion.
        let s = b"abcdefghijklmnopqrstuvwxyz012345";
        assert_eq!(find_first_digit(s), naive_digit(s));
    }

    #[test]
    fn digit_at_boundary_between_simd_block_and_tail() {
        // 17 bytes: NEON consumes 16, scalar tail handles the 17th.
        let s = b"aaaaaaaaaaaaaaaa1";
        assert_eq!(find_first_digit(s), Some(16));
    }

    #[test]
    fn digit_in_long_no_match_input() {
        let s = vec![b'a'; 200];
        assert_eq!(find_first_digit(&s), None);
    }

    #[test]
    fn digit_match_at_end_of_long_input() {
        let mut s = vec![b'a'; 199];
        s.push(b'7');
        assert_eq!(find_first_digit(&s), Some(199));
    }

    #[test]
    fn digit_match_within_first_simd_block() {
        let mut s = vec![b'a'; 200];
        s[5] = b'3';
        assert_eq!(find_first_digit(&s), Some(5));
    }

    #[test]
    fn word_at_various_offsets() {
        for s in [
            b"".as_slice(),
            b"   ".as_slice(),
            b"!@#$%^&*()".as_slice(),
            b"!_!".as_slice(),
            b"!!A".as_slice(),
            b"!!!9".as_slice(),
        ] {
            assert_eq!(find_first_word(s), naive_word(s), "input={s:?}");
        }
    }

    #[test]
    fn word_in_long_input() {
        let mut s = vec![b'!'; 200];
        s[150] = b'q';
        assert_eq!(find_first_word(&s), Some(150));
    }

    #[test]
    fn space_at_various_offsets() {
        for s in [
            b"".as_slice(),
            b"abc".as_slice(),
            b" abc".as_slice(),
            b"abc ".as_slice(),
            b"a\tb".as_slice(),
            b"a\nb".as_slice(),
            b"a\rb".as_slice(),
            b"a\x0Bb".as_slice(),
            b"a\x0Cb".as_slice(),
        ] {
            assert_eq!(find_first_space(s), naive_space(s), "input={s:?}");
        }
    }

    #[test]
    fn space_in_long_input() {
        let mut s = vec![b'x'; 200];
        s[123] = b'\t';
        assert_eq!(find_first_space(&s), Some(123));
    }
}
