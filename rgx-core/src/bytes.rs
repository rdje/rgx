//! Byte-oriented regex matching on `&[u8]` without requiring valid UTF-8.
//!
//! This module provides [`BytesRegex`], which accepts arbitrary byte slices
//! as input. Use this for binary protocols, mixed-encoding log files, or
//! any input that may not be valid UTF-8.
//!
//! ```rust,no_run
//! # use rgx_core::bytes::BytesRegex;
//! let re = BytesRegex::compile(r"\d+").unwrap();
//! assert!(re.is_match(b"abc 123"));
//! let m = re.find(b"abc 123").unwrap();
//! assert_eq!(m.as_bytes(), b"123");
//! ```
//!
//! # Behavior on non-UTF-8 input
//!
//! - `.` matches any single byte (not Unicode scalar)
//! - `\w`, `\d`, `\s` operate on ASCII only
//! - Unicode properties (`\p{L}`) may produce unexpected results
//! - Match positions are byte offsets

use crate::engine::{Engine, ExecutionMode};
use crate::error::Result;
use crate::pattern::CompiledPattern;
use crate::Compiler;

/// A compiled regex that matches against `&[u8]` input without UTF-8 validation.
pub struct BytesRegex {
    engine: Engine,
    pattern: String,
}

/// A match result from a [`BytesRegex`], referencing the input bytes.
#[derive(Clone, Debug)]
pub struct BytesMatch<'t> {
    text: &'t [u8],
    start: usize,
    end: usize,
}

impl<'t> BytesMatch<'t> {
    /// The matched byte slice.
    #[must_use]
    pub fn as_bytes(&self) -> &'t [u8] {
        &self.text[self.start..self.end]
    }

    /// Start byte offset.
    #[must_use]
    pub fn start(&self) -> usize {
        self.start
    }

    /// End byte offset (exclusive).
    #[must_use]
    pub fn end(&self) -> usize {
        self.end
    }

    /// Byte range of the match.
    #[must_use]
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Whether the match is zero-length.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

impl BytesRegex {
    /// Compile a regex pattern for byte-oriented matching.
    ///
    /// # Errors
    /// Returns [`RgxError`](crate::error::RgxError) if the pattern is invalid.
    pub fn compile(pattern: &str) -> Result<Self> {
        let compiled = Compiler::new().compile(pattern)?;
        let engine = Engine::new(&compiled)?;
        Ok(Self {
            engine,
            pattern: pattern.to_string(),
        })
    }

    /// Compile with a specific execution mode.
    ///
    /// # Errors
    /// Returns [`RgxError`](crate::error::RgxError) if the pattern is invalid.
    pub fn with_mode(pattern: &str, mode: ExecutionMode) -> Result<Self> {
        let compiled = Compiler::with_mode(mode).compile(pattern)?;
        let engine = Engine::new(&compiled)?;
        Ok(Self {
            engine,
            pattern: pattern.to_string(),
        })
    }

    /// Test if the pattern matches anywhere in the input bytes.
    #[must_use]
    pub fn is_match(&self, text: &[u8]) -> bool {
        self.find(text).is_some()
    }

    /// Find the first match in the input bytes.
    #[must_use]
    pub fn find<'t>(&self, text: &'t [u8]) -> Option<BytesMatch<'t>> {
        let text_str = bytes_as_str(text);
        self.engine.vm_find_first(text_str).map(|m| BytesMatch {
            text,
            start: m.start,
            end: m.end,
        })
    }

    /// Find all non-overlapping matches in the input bytes.
    #[must_use]
    pub fn find_all<'t>(&self, text: &'t [u8]) -> Vec<BytesMatch<'t>> {
        let text_str = bytes_as_str(text);
        self.engine
            .vm_find_all(text_str)
            .into_iter()
            .map(|m| BytesMatch {
                text,
                start: m.start,
                end: m.end,
            })
            .collect()
    }

    /// The original pattern string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.pattern
    }
}

/// Reinterpret bytes as a `&str` without UTF-8 validation.
///
/// # Safety
/// The VM operates on bytes internally and only uses UTF-8 decoding
/// for character-level operations. On non-UTF-8 input, `.` matches
/// individual bytes and character classes operate on ASCII.
fn bytes_as_str(bytes: &[u8]) -> &str {
    // SAFETY: The VM's scanning loop operates on raw bytes. Character-level
    // operations (advance_char, current_char) use `str::chars()` which will
    // produce U+FFFD replacement characters for invalid sequences, but won't
    // panic or cause UB. Match positions remain valid byte offsets.
    unsafe { std::str::from_utf8_unchecked(bytes) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_regex_basic() {
        let re = BytesRegex::compile(r"\d+").unwrap();
        assert!(re.is_match(b"abc 123"));
        let m = re.find(b"abc 123").unwrap();
        assert_eq!(m.as_bytes(), b"123");
        assert_eq!(m.start(), 4);
        assert_eq!(m.end(), 7);
    }

    #[test]
    fn bytes_regex_no_match() {
        let re = BytesRegex::compile(r"\d+").unwrap();
        assert!(!re.is_match(b"no digits"));
    }

    #[test]
    fn bytes_regex_find_all() {
        let re = BytesRegex::compile(r"\d+").unwrap();
        let matches = re.find_all(b"a1 b22 c333");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].as_bytes(), b"1");
        assert_eq!(matches[1].as_bytes(), b"22");
        assert_eq!(matches[2].as_bytes(), b"333");
    }

    #[test]
    fn bytes_regex_non_utf8_input() {
        let re = BytesRegex::compile(r"abc").unwrap();
        // Input with invalid UTF-8 bytes around the match
        let input: &[u8] = &[0xFF, 0xFE, b'a', b'b', b'c', 0xFF];
        let m = re.find(input).unwrap();
        assert_eq!(m.as_bytes(), b"abc");
        assert_eq!(m.start(), 2);
        assert_eq!(m.end(), 5);
    }

    #[test]
    fn bytes_regex_binary_pattern() {
        let re = BytesRegex::compile(r"\x00\x01\x02").unwrap();
        let input: &[u8] = &[0x00, 0x01, 0x02, 0x03];
        let m = re.find(input).unwrap();
        assert_eq!(m.as_bytes(), &[0x00, 0x01, 0x02]);
    }

    #[test]
    fn bytes_match_methods() {
        let re = BytesRegex::compile(r"test").unwrap();
        let m = re.find(b"a test here").unwrap();
        assert_eq!(m.range(), 2..6);
        assert_eq!(m.len(), 4);
        assert!(!m.is_empty());
    }

    #[test]
    fn bytes_regex_as_str() {
        let re = BytesRegex::compile(r"\d+").unwrap();
        assert_eq!(re.as_str(), r"\d+");
    }
}
