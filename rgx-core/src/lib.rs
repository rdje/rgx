//! # RGX Core - High-Performance Regex Engine
//!
//! The `rgx-core` crate provides a cutting-edge regex engine designed to surpass
//! PCRE2 performance while enabling multi-language code execution within patterns.
//!
//! ## Performance Philosophy
//!
//! - **Zero-cost abstractions**: Features you don't use have zero overhead
//! - **SIMD-first design**: Vectorized operations wherever possible  
//! - **Cache-friendly data structures**: Optimized for modern CPU architectures
//! - **Graduated execution**: Fast paths for simple patterns, full power when needed
//!
//! ## Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │           Pattern Compiler          │  ← Compile-time optimization
//! ├─────────────────────────────────────┤
//! │         SIMD Engine Core            │  ← Vectorized execution
//! ├─────────────────────────────────────┤
//! │       Code Execution Layer          │  ← Multi-language support
//! ├─────────┬─────────┬─────────────────┤
//! │  Rhai   │ Lua/JS  │ Native/WASM     │  ← Pluggable executors
//! └─────────┴─────────┴─────────────────┘
//! ```
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use rgx_core::{Regex, ExecutionMode};
//!
//! // Pure regex - maximum performance
//! let email_pattern = Regex::compile(r"\b\w+@\w+\.\w+\b")?;
//! let matches = email_pattern.find_all("Contact us at admin@example.com");
//!
//! // With code execution - enhanced functionality  
//! let validator = Regex::with_mode(
//!     r#"(\d{4})-(\d{2})-(\d{2})(?{lua:return tonumber(arg[2]) <= 12 and tonumber(arg[3]) <= 31})"#,
//!     ExecutionMode::Safe
//! )?;
//! let dates = validator.find_all("Born on 1985-03-15 and graduated 2007-06-22");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

// Allow unsafe code for SIMD optimizations only
#![allow(unsafe_code)]
#![warn(missing_docs, clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

// Core modules
/// Abstract syntax tree types for regex patterns.
pub mod ac;
pub mod ast;
/// Byte-oriented regex matching on `&[u8]` without requiring valid UTF-8.
pub mod bytes;
/// C1: JIT compilation backend (Cranelift).
///
/// See `docs/C1_JIT_COMPILATION_DESIGN.md` for the full design proposal.
/// Currently at step 1 of the §14 phased plan: standalone JIT host plumbing
/// (Cranelift `JITModule` wrapper, runtime helper skeleton, smoke test).
/// No engine wiring, no opcode lowering, no dispatch — that lands in steps
/// 3–5. Gated behind the `jit` Cargo feature; opt-in until the production
/// cutover at step 8.
#[cfg(feature = "jit")]
pub mod c1;
/// C2: NFA/DFA hybrid engine for the no-backtracking subset.
///
/// See `docs/C2_NFA_DFA_DESIGN.md` for the full design proposal. Shipped
/// 2026-04-11 — all 9 steps (0–8) complete. The dispatch chain in
/// [`engine::Engine`] routes classifier-positive patterns through the lazy
/// DFA (DFA-eligible) or the sparse-set Pike-VM (nested-quantifier) and
/// falls back to the existing backtracking VM otherwise.
pub mod c2;
/// Thread-safe compilation cache for regex patterns.
pub mod cache;
/// Pattern-to-program compiler logic.
pub mod compiler;
/// Execution-engine entry points.
pub mod engine;
/// Structured match events for debugging, profiling, and observability.
pub mod events;
/// Code-block execution runtime support.
pub mod execution;
/// Regex pattern tokenization.
pub mod lexer;
/// Recursive-descent parser implementation.
/// Recursive-descent parser — deprecated; retained only for non-PGEN builds.
#[cfg(not(feature = "pgen-parser"))]
pub mod parser;
/// Zero-cost parser abstraction and backend selection.
pub mod parsing;
/// Compiled-pattern data structures.
pub mod pattern;
/// Multi-pattern simultaneous matching.
pub mod regex_set;
/// Token and source-position types.
pub mod token;
mod unicode_support;
/// Virtual machine bytecode and runtime execution.
pub mod vm;

// Code execution backends
#[cfg(feature = "lua")]
pub mod lua;
#[cfg(feature = "rhai")]
pub mod rhai;

// File-backed matching
/// File-backed matching — scan files directly without loading into a String.
pub mod file;

// Error handling
/// Shared error types and result aliases.
pub mod error;

// Logging system
pub mod log;

// Fluent variable builder
/// Fluent builder API for host variables — see [`vars::VarsBuilder`].
pub mod vars;

/// PCRE2 compatibility version that this RGX engine claims.
///
/// Used by `(?(VERSION>=X.Y)yes|no)` conditionals which are
/// evaluated at parse time and short-circuited to the matching
/// branch. The conditional never reaches the AST as a
/// `Regex::Conditional` node — instead the parser returns the
/// branch directly.
///
/// Currently set to `(10, 47)` because the RGX feature surface
/// (per `docs/PCRE2_COMPATIBILITY_MATRIX.md` ~98% parity) tracks
/// PCRE2 10.47 as its target. Bump this when the parity matrix
/// is re-aligned to a newer PCRE2 release.
pub const RGX_PCRE2_COMPAT_VERSION: (u32, u32) = (10, 47);

// Re-exports for convenience
pub use cache::RegexCache;
pub use compiler::Compiler;
pub use engine::{Engine, ExecutionMode, MatchResult, MatchSemantics, PartialMatchResult};
// Note: Match, Captures, SubCaptureMatches, escape are defined directly in this file.
pub use error::{Result, RgxError};
pub use events::MatchEvent;
pub use execution::{
    CodeBlockValue, ExecContext, ExecContextSnapshot, ExecResult, MatchContinuation, MatchOutcome,
    SteerResult, Value,
};
pub use file::FileMatch;
pub use pattern::{CompiledPattern, Pattern};
pub use regex_set::{RegexSet, SetMatches};
pub use vars::VarsBuilder;

/// Advance `idx` to the next UTF-8 character boundary in `text`, or `text.len()`.
fn next_char_boundary(text: &str, idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    let mut i = idx;
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

// ────────────────────────────────────────────────────────────
// B18: escape() — escape regex metacharacters for safe concatenation
// ────────────────────────────────────────────────────────────

/// Escape all regex metacharacters in `text` so it can be used as a literal
/// pattern.
///
/// ```rust,no_run
/// # use rgx_core::escape;
/// assert_eq!(escape("a.b+c"), r"a\.b\+c");
/// ```
#[must_use]
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        if matches!(
            ch,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

// ────────────────────────────────────────────────────────────
// B14: Match<'a> — ergonomic match access
// ────────────────────────────────────────────────────────────

/// A single match with a borrowed reference to the matched text.
///
/// Returned by [`Captures::get`] and convertible from [`MatchResult`] via
/// [`Regex::find`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match<'t> {
    text: &'t str,
    start: usize,
    end: usize,
}

impl<'t> Match<'t> {
    /// The byte offset of the start of the match.
    #[must_use]
    pub fn start(&self) -> usize {
        self.start
    }

    /// The byte offset immediately after the end of the match.
    #[must_use]
    pub fn end(&self) -> usize {
        self.end
    }

    /// The matched substring.
    #[must_use]
    pub fn as_str(&self) -> &'t str {
        &self.text[self.start..self.end]
    }

    /// The byte range of the match.
    #[must_use]
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    /// The length of the match in bytes.
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

// ────────────────────────────────────────────────────────────
// B13: Captures<'a> — ergonomic capture group access
// ────────────────────────────────────────────────────────────

/// Capture groups from a single regex match, with ergonomic access by index
/// or name.
///
/// Returned by [`Regex::captures`].
#[derive(Clone, Debug)]
pub struct Captures<'t> {
    text: &'t str,
    groups: Vec<Option<(usize, usize)>>,
    named: std::sync::Arc<std::collections::HashMap<String, u32>>,
    /// Parallel multi-id named-group map (registration order). Used
    /// by substitute template `$name` expansion to pick the first
    /// SET duplicate when a name has multiple definitions — PCRE2
    /// `(?J)` / alternation-dupnames semantic. For non-dupnames
    /// patterns each Vec has exactly one id, identical to `named`.
    named_all: std::sync::Arc<std::collections::HashMap<String, Vec<u32>>>,
    /// Name of the last `(*MARK:name)` / `(*:name)` verb hit on the
    /// winning match path, if any. Exposed to replacement templates
    /// as `${*MARK}` / `$*MARK` and to users via [`Captures::mark`].
    last_mark: Option<String>,
}

impl<'t> Captures<'t> {
    /// Name of the last `(*MARK:name)` / `(*:name)` control verb
    /// encountered on the winning match path, or `None` if no mark
    /// was hit. Mirrors PCRE2's `*MARK` introspection.
    #[must_use]
    pub fn mark(&self) -> Option<&str> {
        self.last_mark.as_deref()
    }

    /// Get a capture group by index.
    ///
    /// Index 0 is the overall match. Returns `None` if the group did not
    /// participate in the match.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<Match<'t>> {
        self.groups.get(i).and_then(|slot| {
            slot.map(|(s, e)| Match {
                text: self.text,
                start: s,
                end: e,
            })
        })
    }

    /// Get a capture group by name.
    #[must_use]
    pub fn name(&self, name: &str) -> Option<Match<'t>> {
        self.named.get(name).and_then(|&idx| self.get(idx as usize))
    }

    /// The number of capture groups (including group 0).
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether there are no capture groups.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Expand a replacement template with `$1`, `$name`, `${name}`, `$&`, `$$`
    /// and write the result to `dst`.
    pub fn expand(&self, replacement: &str, dst: &mut String) {
        let str_groups: Vec<Option<&str>> = self
            .groups
            .iter()
            .map(|slot| slot.map(|(s, e)| &self.text[s..e]))
            .collect();
        Regex::interpolate_replacement_ext(
            replacement,
            &str_groups,
            &self.named,
            Some(&self.named_all),
            self.last_mark.as_deref(),
            dst,
        );
    }

    /// Iterator over all capture groups.
    pub fn iter(&self) -> SubCaptureMatches<'_, 't> {
        SubCaptureMatches { caps: self, idx: 0 }
    }
}

impl<'t> std::ops::Index<usize> for Captures<'t> {
    type Output = str;
    fn index(&self, i: usize) -> &str {
        self.get(i)
            .map(|m| m.as_str())
            .unwrap_or_else(|| panic!("no group at index {i}"))
    }
}

impl<'t> std::ops::Index<&str> for Captures<'t> {
    type Output = str;
    fn index(&self, name: &str) -> &str {
        self.name(name)
            .map(|m| m.as_str())
            .unwrap_or_else(|| panic!("no group named '{name}'"))
    }
}

/// Iterator over sub-capture groups inside a [`Captures`].
pub struct SubCaptureMatches<'c, 't> {
    caps: &'c Captures<'t>,
    idx: usize,
}

impl<'c, 't> Iterator for SubCaptureMatches<'c, 't> {
    type Item = Option<Match<'t>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.caps.len() {
            return None;
        }
        let item = self.caps.get(self.idx);
        self.idx += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.caps.len() - self.idx;
        (remaining, Some(remaining))
    }
}

impl<'c, 't> ExactSizeIterator for SubCaptureMatches<'c, 't> {}

// ────────────────────────────────────────────────────────────
// B20: CaptureLocations — reusable capture storage
// ────────────────────────────────────────────────────────────

/// Pre-allocated capture group storage for zero-allocation matching loops.
///
/// Create once with [`Regex::capture_locations`], then reuse across calls
/// to [`Regex::captures_read`] to avoid allocating a new `Vec` per match.
///
/// ```rust,no_run
/// # use rgx_core::Regex;
/// let re = Regex::compile(r"(\d+)-(\w+)").unwrap();
/// let mut locs = re.capture_locations();
/// if re.captures_read("item 42-abc", &mut locs).is_some() {
///     assert_eq!(locs.get(1), Some((5, 7)));   // "42"
///     assert_eq!(locs.get(2), Some((8, 11)));   // "abc"
/// }
/// ```
#[derive(Clone, Debug)]
pub struct CaptureLocations {
    slots: Vec<Option<(usize, usize)>>,
}

impl CaptureLocations {
    /// Get the byte offset pair for capture group `i`.
    ///
    /// Index 0 is the overall match. Returns `None` if the group did not
    /// participate in the match.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<(usize, usize)> {
        self.slots.get(i).copied().flatten()
    }

    /// The number of slots (including group 0).
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether there are no slots.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

// ────────────────────────────────────────────────────────────
// B12: Iterator-based APIs
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// B16: Replacer trait — pluggable replacement strategy
// ────────────────────────────────────────────────────────────

/// Trait for types that can produce replacement text for regex matches.
///
/// Implemented for:
/// - `&str` / `String` / `&String` — template with `$1`, `$name` interpolation
/// - `FnMut(&Captures) -> T` where `T: AsRef<str>` — closure-based replacement
/// - [`NoExpand`] — literal string, no interpolation
pub trait Replacer {
    /// Append the replacement for `caps` to `dst`.
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String);

    /// If the replacement is a fixed string with no capture references, return
    /// it here. This lets the engine skip capture extraction entirely.
    fn no_expansion(&mut self) -> Option<std::borrow::Cow<'_, str>> {
        None
    }
}

impl Replacer for &str {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        caps.expand(self, dst);
    }

    fn no_expansion(&mut self) -> Option<std::borrow::Cow<'_, str>> {
        // Fast-path only when the replacement contains neither a `$`
        // (capture reference / meta) nor a `\` (PCRE2-style backslash
        // escape — `\n`, `\u`, `\$`, `\045`, `\x{...}`, `\U`, `\L`, ...)
        // nor a leading `[N]` (PCRE2 substitute buffer-size hint — the
        // interpolator strips the prefix).
        if !self.contains('$') && !self.contains('\\') && !starts_with_length_hint(self) {
            Some(std::borrow::Cow::Borrowed(self))
        } else {
            None
        }
    }
}

impl Replacer for String {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        caps.expand(self, dst);
    }

    fn no_expansion(&mut self) -> Option<std::borrow::Cow<'_, str>> {
        if !self.contains('$') && !self.contains('\\') && !starts_with_length_hint(self) {
            Some(std::borrow::Cow::Borrowed(self))
        } else {
            None
        }
    }
}

impl Replacer for &String {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        caps.expand(self, dst);
    }

    fn no_expansion(&mut self) -> Option<std::borrow::Cow<'_, str>> {
        if !self.contains('$') && !self.contains('\\') && !starts_with_length_hint(self) {
            Some(std::borrow::Cow::Borrowed(self))
        } else {
            None
        }
    }
}

impl<F, T> Replacer for F
where
    F: FnMut(&Captures<'_>) -> T,
    T: AsRef<str>,
{
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        dst.push_str(self(caps).as_ref());
    }
}

/// Wrapper that prevents `$1`/`$name` interpolation in a replacement string.
///
/// ```rust,no_run
/// # use rgx_core::{Regex, NoExpand};
/// let re = Regex::compile(r"\d+").unwrap();
/// let result = re.replace("price 42", NoExpand("$$$"));
/// assert_eq!(result, "price $$$");
/// ```
pub struct NoExpand<'s>(pub &'s str);

impl<'s> Replacer for NoExpand<'s> {
    fn replace_append(&mut self, _caps: &Captures<'_>, dst: &mut String) {
        dst.push_str(self.0);
    }

    fn no_expansion(&mut self) -> Option<std::borrow::Cow<'_, str>> {
        Some(std::borrow::Cow::Borrowed(self.0))
    }
}

/// Lazy iterator over successive non-overlapping matches.
///
/// Created by [`Regex::find_iter`].
pub struct FindIter<'r, 't> {
    regex: &'r Regex,
    text: &'t str,
    last_end: usize,
    last_match_end: Option<usize>,
    done: bool,
}

impl<'r, 't> Iterator for FindIter<'r, 't> {
    type Item = Match<'t>;

    fn next(&mut self) -> Option<Match<'t>> {
        if self.done {
            return None;
        }
        loop {
            let m = self.regex.find_first_at(self.text, self.last_end)?;
            let start = m.start;
            let end = m.end;
            // Zero-width match suppression at the same position as previous match end
            if start == end {
                if let Some(prev) = self.last_match_end {
                    if start == prev {
                        // Advance past this position
                        if self.last_end >= self.text.len() {
                            self.done = true;
                            return None;
                        }
                        // Advance by one UTF-8 character
                        self.last_end = next_char_boundary(self.text, self.last_end + 1);
                        continue;
                    }
                }
            }
            self.last_match_end = Some(end);
            self.last_end = if start == end {
                next_char_boundary(self.text, end + 1)
            } else {
                end
            };
            return Some(Match {
                text: self.text,
                start,
                end,
            });
        }
    }
}

impl<'r, 't> std::iter::FusedIterator for FindIter<'r, 't> {}

/// Lazy iterator over successive non-overlapping matches with capture groups.
///
/// Created by [`Regex::captures_iter`].
pub struct CaptureIter<'r, 't> {
    inner: FindIter<'r, 't>,
    named: std::sync::Arc<std::collections::HashMap<String, u32>>,
    named_all: std::sync::Arc<std::collections::HashMap<String, Vec<u32>>>,
}

impl<'r, 't> Iterator for CaptureIter<'r, 't> {
    type Item = Captures<'t>;

    fn next(&mut self) -> Option<Captures<'t>> {
        // We need the full MatchResult (with groups), not just the Match.
        // Re-derive from find_first_at which returns MatchResult with groups.
        if self.inner.done {
            return None;
        }
        loop {
            let mr = self
                .inner
                .regex
                .find_first_at(self.inner.text, self.inner.last_end)?;
            let start = mr.start;
            let end = mr.end;
            // Zero-width suppression (same logic as FindIter)
            if start == end {
                if let Some(prev) = self.inner.last_match_end {
                    if start == prev {
                        if self.inner.last_end >= self.inner.text.len() {
                            self.inner.done = true;
                            return None;
                        }
                        self.inner.last_end =
                            next_char_boundary(self.inner.text, self.inner.last_end + 1);
                        continue;
                    }
                }
            }
            self.inner.last_match_end = Some(end);
            self.inner.last_end = if start == end {
                next_char_boundary(self.inner.text, end + 1)
            } else {
                end
            };
            return Some(Captures {
                text: self.inner.text,
                groups: mr.groups,
                named: self.named.clone(),
                named_all: self.named_all.clone(),
                last_mark: mr.last_mark,
            });
        }
    }
}

impl<'r, 't> std::iter::FusedIterator for CaptureIter<'r, 't> {}

/// Lazy iterator over substrings delimited by regex matches.
///
/// Created by [`Regex::split_iter`].
pub struct SplitIter<'r, 't> {
    finder: FindIter<'r, 't>,
    last_end: usize,
    done: bool,
}

impl<'r, 't> Iterator for SplitIter<'r, 't> {
    type Item = &'t str;

    fn next(&mut self) -> Option<&'t str> {
        if self.done {
            return None;
        }
        match self.finder.next() {
            Some(m) => {
                let piece = &self.finder.text[self.last_end..m.start()];
                self.last_end = m.end();
                Some(piece)
            }
            None => {
                self.done = true;
                Some(&self.finder.text[self.last_end..])
            }
        }
    }
}

impl<'r, 't> std::iter::FusedIterator for SplitIter<'r, 't> {}

/// Lazy iterator over substrings delimited by regex matches, with a limit.
///
/// Created by [`Regex::splitn_iter`].
pub struct SplitNIter<'r, 't> {
    finder: FindIter<'r, 't>,
    last_end: usize,
    limit: usize,
    count: usize,
    done: bool,
}

impl<'r, 't> Iterator for SplitNIter<'r, 't> {
    type Item = &'t str;

    fn next(&mut self) -> Option<&'t str> {
        if self.done {
            return None;
        }
        self.count += 1;
        // If we've reached the limit, return the remainder
        if self.count >= self.limit {
            self.done = true;
            return Some(&self.finder.text[self.last_end..]);
        }
        match self.finder.next() {
            Some(m) => {
                let piece = &self.finder.text[self.last_end..m.start()];
                self.last_end = m.end();
                Some(piece)
            }
            None => {
                self.done = true;
                Some(&self.finder.text[self.last_end..])
            }
        }
    }
}

impl<'r, 't> std::iter::FusedIterator for SplitNIter<'r, 't> {}

/// Iterator over capture group names.
///
/// Created by [`Regex::capture_names`].
pub struct CaptureNames<'r> {
    named: &'r std::collections::HashMap<String, u32>,
    num_groups: u32,
    idx: u32,
}

impl<'r> Iterator for CaptureNames<'r> {
    type Item = Option<&'r str>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx > self.num_groups {
            return None;
        }
        let current = self.idx;
        self.idx += 1;
        if current == 0 {
            return Some(None); // Group 0 is unnamed
        }
        // Find the name for this group number
        let name = self
            .named
            .iter()
            .find(|(_, &num)| num == current)
            .map(|(name, _)| name.as_str());
        Some(name)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.num_groups + 1 - self.idx) as usize;
        (remaining, Some(remaining))
    }
}

impl<'r> ExactSizeIterator for CaptureNames<'r> {}

// ────────────────────────────────────────────────────────────
// B11: RegexBuilder — fluent compilation with flag overrides
// ────────────────────────────────────────────────────────────

/// Builder for configuring and compiling a [`Regex`] with flag overrides.
///
/// ```rust,no_run
/// # use rgx_core::RegexBuilder;
/// let re = RegexBuilder::new(r"hello world")
///     .case_insensitive()
///     .multi_line()
///     .build()
///     .unwrap();
/// assert!(re.is_match("HELLO WORLD"));
/// ```
pub struct RegexBuilder {
    pattern: String,
    mode: ExecutionMode,
    case_insensitive: bool,
    multi_line: bool,
    dot_matches_new_line: bool,
    swap_greed: bool,
    ignore_whitespace: bool,
}

impl RegexBuilder {
    /// Create a new builder for the given pattern.
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            mode: ExecutionMode::Pure,
            case_insensitive: false,
            multi_line: false,
            dot_matches_new_line: false,
            swap_greed: false,
            ignore_whitespace: false,
        }
    }

    /// Set the execution mode.
    #[must_use]
    pub fn mode(mut self, mode: ExecutionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Enable case-insensitive matching (`(?i)`).
    ///
    /// Call with no argument to enable (the common case), or pass `false`
    /// to explicitly disable.
    #[must_use]
    pub fn case_insensitive(self) -> Self {
        self.set_case_insensitive(true)
    }

    /// Enable or disable case-insensitive matching (`(?i)`).
    #[must_use]
    pub fn set_case_insensitive(mut self, yes: bool) -> Self {
        self.case_insensitive = yes;
        self
    }

    /// Enable multi-line mode (`(?m)`), where `^`/`$` match line boundaries.
    #[must_use]
    pub fn multi_line(self) -> Self {
        self.set_multi_line(true)
    }

    /// Enable or disable multi-line mode (`(?m)`).
    #[must_use]
    pub fn set_multi_line(mut self, yes: bool) -> Self {
        self.multi_line = yes;
        self
    }

    /// Enable dot-matches-newline mode (`(?s)`), where `.` matches `\n`.
    #[must_use]
    pub fn dot_matches_new_line(self) -> Self {
        self.set_dot_matches_new_line(true)
    }

    /// Enable or disable dot-matches-newline mode (`(?s)`).
    #[must_use]
    pub fn set_dot_matches_new_line(mut self, yes: bool) -> Self {
        self.dot_matches_new_line = yes;
        self
    }

    /// Enable swap-greed mode, where quantifiers are lazy by default.
    #[must_use]
    pub fn swap_greed(self) -> Self {
        self.set_swap_greed(true)
    }

    /// Enable or disable swap-greed mode.
    #[must_use]
    pub fn set_swap_greed(mut self, yes: bool) -> Self {
        self.swap_greed = yes;
        self
    }

    /// Enable extended/verbose mode (`(?x)`), where whitespace and `#`
    /// comments are ignored.
    #[must_use]
    pub fn ignore_whitespace(self) -> Self {
        self.set_ignore_whitespace(true)
    }

    /// Enable or disable extended/verbose mode (`(?x)`).
    #[must_use]
    pub fn set_ignore_whitespace(mut self, yes: bool) -> Self {
        self.ignore_whitespace = yes;
        self
    }

    /// Compile the regex with the configured options.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] if the pattern is invalid.
    pub fn build(self) -> Result<Regex> {
        let flags = self.flag_prefix();
        let effective_pattern = if flags.is_empty() {
            self.pattern.clone()
        } else {
            // PCRE2 requires pattern-start verbs such as `(*NUL)`,
            // `(*CRLF)`, `(*UTF)`, `(*TURKISH_CASING)`, and
            // `(*LIMIT_...)` to appear before anything else in the
            // pattern. Inline-flag groups `(?flags)` must not be
            // inserted before them. Find the end of any leading
            // `(*VERB[:name])` run and place `(?flags)` after.
            let split = leading_start_verb_end(&self.pattern);
            let (verbs, rest) = self.pattern.split_at(split);
            format!("{verbs}(?{flags}){rest}")
        };
        if self.mode == ExecutionMode::Pure {
            Regex::compile(&effective_pattern)
        } else {
            Regex::with_mode(&effective_pattern, self.mode)
        }
    }

    /// Build the inline flag prefix string from enabled flags.
    fn flag_prefix(&self) -> String {
        let mut flags = String::new();
        if self.case_insensitive {
            flags.push('i');
        }
        if self.multi_line {
            flags.push('m');
        }
        if self.dot_matches_new_line {
            flags.push('s');
        }
        if self.ignore_whitespace {
            flags.push('x');
        }
        // swap_greed doesn't map to a standard inline flag — we'd need
        // compiler support. For now it's a no-op placeholder.
        flags
    }
}

/// Return the byte offset just past any run of leading PCRE2
/// pattern-start verbs `(*NAME)` or `(*NAME:ARG)` at the beginning of
/// `pattern`. Returns `0` if the pattern does not begin with such a
/// verb. Used by `RegexBuilder::build` to insert `(?flags)` after the
/// verb run rather than before it, preserving PCRE2's requirement
/// that pattern-start verbs precede every other construct.
/// Return `true` when `template` begins with a `[digits]` PCRE2
/// buffer-size hint — shape `[` followed by one or more ASCII digits
/// followed by `]`. Used by the fast-path check on `Replacer::no_expansion`
/// so hinted templates still route through the interpolator (which
/// strips the prefix).
fn starts_with_length_hint(template: &str) -> bool {
    let bytes = template.as_bytes();
    if bytes.first() != Some(&b'[') {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i > 1 && i < bytes.len() && bytes[i] == b']'
}

/// Strip a leading `[N]` buffer-size hint from a PCRE2 substitute
/// template, if present. PCRE2's `pcre2_substitute` treats a template
/// that begins with `[digits]` as carrying an advisory output-buffer
/// size — the prefix is consumed before interpolation and never
/// appears in the produced replacement text. RGX has no notion of a
/// fixed output buffer, so the hint is semantically a no-op, and we
/// strip it so templates like `[10]XYZ` observe the same emission
/// behaviour as PCRE2.
fn strip_substitute_length_hint(template: &str) -> &str {
    if !template.starts_with('[') {
        return template;
    }
    let bytes = template.as_bytes();
    let mut i = 1;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 1 && i < bytes.len() && bytes[i] == b']' {
        &template[i + 1..]
    } else {
        template
    }
}

fn leading_start_verb_end(pattern: &str) -> usize {
    let bytes = pattern.as_bytes();
    let mut pos = 0;
    loop {
        if !bytes[pos..].starts_with(b"(*") {
            return pos;
        }
        let mut depth = 1;
        let mut i = pos + 2;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'\\' if i + 1 < bytes.len() => i += 2,
                b'(' => {
                    depth += 1;
                    i += 1;
                }
                b')' => {
                    depth -= 1;
                    i += 1;
                }
                _ => i += 1,
            }
        }
        if depth != 0 {
            return pos;
        }
        pos = i;
    }
}

// ============================================================
// C1 step 5 — JIT dispatch helpers
// ============================================================
//
// These thin wrappers call into `Engine`'s JIT dispatch methods
// when the `jit` feature is enabled and return `None` (no-op)
// when it isn't. The non-jit stubs let `Regex::find_first` /
// `find_all` / `is_match` use the same dispatch chain regardless
// of whether the feature is on, avoiding `#[cfg]` clutter at the
// public API.

#[cfg(feature = "jit")]
fn jit_dispatch_find_first(engine: &Engine, input: &[u8]) -> Option<Option<MatchResult>> {
    engine.try_jit_find_first(input)
}

#[cfg(not(feature = "jit"))]
fn jit_dispatch_find_first(_engine: &Engine, _input: &[u8]) -> Option<Option<MatchResult>> {
    None
}

#[cfg(feature = "jit")]
fn jit_dispatch_find_all(engine: &Engine, input: &[u8]) -> Option<Vec<MatchResult>> {
    engine.try_jit_find_all(input)
}

#[cfg(not(feature = "jit"))]
fn jit_dispatch_find_all(_engine: &Engine, _input: &[u8]) -> Option<Vec<MatchResult>> {
    None
}

#[cfg(feature = "jit")]
fn jit_dispatch_is_match(engine: &Engine, input: &[u8]) -> Option<bool> {
    engine.try_jit_is_match(input)
}

#[cfg(not(feature = "jit"))]
fn jit_dispatch_is_match(_engine: &Engine, _input: &[u8]) -> Option<bool> {
    None
}

/// High-performance regex matcher with optional code execution capabilities.
///
/// This is the main entry point for the `rgx` regex engine. It provides
/// a familiar interface similar to other regex libraries while offering
/// unprecedented performance and multi-language code execution.
pub struct Regex {
    engine: Engine,
    /// The original pattern string, kept for `as_str()`.
    pattern: String,
}

impl Regex {
    /// Compile a regex pattern for maximum performance.
    ///
    /// This method analyzes the pattern at compile time and selects the
    /// optimal execution strategy. Pure regex patterns will use the fastest
    /// possible code path.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use rgx_core::Regex;
    ///
    /// let regex = Regex::compile(r"\d{3}-\d{2}-\d{4}")?;
    /// let matches = regex.find_all("SSN: 123-45-6789");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when the pattern cannot be compiled or when the
    /// compiled program cannot be lowered into an executable engine.
    pub fn compile(pattern: &str) -> Result<Self> {
        trace_enter!("api", "Regex::compile", "pattern_len={}", pattern.len());
        let compiled = match Compiler::new().compile(pattern) {
            Ok(compiled) => compiled,
            Err(err) => {
                trace_exit!("api", "Regex::compile", "ok=false,error={}", err);
                return Err(err);
            }
        };
        let engine = match Engine::new(&compiled) {
            Ok(engine) => engine,
            Err(err) => {
                trace_exit!("api", "Regex::compile", "ok=false,error={}", err);
                return Err(err);
            }
        };

        let regex = Self {
            engine,
            pattern: pattern.to_string(),
        };
        trace_exit!("api", "Regex::compile", "ok=true");
        Ok(regex)
    }

    /// C2 engine classification for this compiled pattern.
    ///
    /// Returns the `Classification` decided by the C2 pattern classifier
    /// at compile time. At C2 step 1 this is metadata only — the engine
    /// still always dispatches through the backtracking VM. Runtime
    /// dispatch on this field lands in C2 step 4 (Pike-VM).
    ///
    /// This accessor is doc-hidden because the public introspection API
    /// (e.g. `uses_c2() -> bool`) is design doc Q8 and lands in C2 step 8
    /// alongside the production cutover. Until then, this method exists
    /// for tests, internal callers, and the differential testing harness.
    ///
    /// See `docs/C2_NFA_DFA_DESIGN.md` §4 for the no-backtracking subset
    /// definition.
    #[doc(hidden)]
    #[must_use]
    pub fn classification(&self) -> &c2::Classification {
        self.engine.classification()
    }

    /// Compile a regex with specific execution mode.
    ///
    /// This allows you to control the performance/feature tradeoff:
    /// - `ExecutionMode::Pure`: Maximum performance, no code execution
    /// - `ExecutionMode::Safe`: Code execution in sandboxed environments only
    /// - `ExecutionMode::Full`: enables the native-callback path in addition to the sandboxed backends
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when the pattern is invalid for the requested mode
    /// or when engine construction fails.
    pub fn with_mode(pattern: &str, mode: ExecutionMode) -> Result<Self> {
        trace_enter!(
            "api",
            "Regex::with_mode",
            "pattern_len={},mode={:?}",
            pattern.len(),
            mode
        );
        let compiled = match Compiler::with_mode(mode).compile(pattern) {
            Ok(compiled) => compiled,
            Err(err) => {
                trace_exit!("api", "Regex::with_mode", "ok=false,error={}", err);
                return Err(err);
            }
        };
        let engine = match Engine::new(&compiled) {
            Ok(engine) => engine,
            Err(err) => {
                trace_exit!("api", "Regex::with_mode", "ok=false,error={}", err);
                return Err(err);
            }
        };

        let regex = Self {
            engine,
            pattern: pattern.to_string(),
        };
        trace_exit!("api", "Regex::with_mode", "ok=true");
        Ok(regex)
    }

    /// Compile a regex directly from a pre-built AST.
    ///
    /// This enables parser-independent development, testing, and benchmarking
    /// of the compiler/VM/engine pipeline.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when AST compilation or engine construction fails.
    pub fn from_ast(ast: ast::Regex) -> Result<Self> {
        trace_enter!("api", "Regex::from_ast");
        let compiled = match Compiler::new().compile_ast(ast) {
            Ok(compiled) => compiled,
            Err(err) => {
                trace_exit!("api", "Regex::from_ast", "ok=false,error={}", err);
                return Err(err);
            }
        };
        let engine = match Engine::new(&compiled) {
            Ok(engine) => engine,
            Err(err) => {
                trace_exit!("api", "Regex::from_ast", "ok=false,error={}", err);
                return Err(err);
            }
        };

        let regex = Self {
            engine,
            pattern: String::new(),
        };
        trace_exit!("api", "Regex::from_ast", "ok=true");
        Ok(regex)
    }

    /// Compile a regex from AST using a specific execution mode.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when AST compilation or engine construction fails
    /// for the requested mode.
    pub fn from_ast_with_mode(ast: ast::Regex, mode: ExecutionMode) -> Result<Self> {
        trace_enter!("api", "Regex::from_ast_with_mode", "mode={:?}", mode);
        let compiled = match Compiler::with_mode(mode).compile_ast(ast) {
            Ok(compiled) => compiled,
            Err(err) => {
                trace_exit!("api", "Regex::from_ast_with_mode", "ok=false,error={}", err);
                return Err(err);
            }
        };
        let engine = match Engine::new(&compiled) {
            Ok(engine) => engine,
            Err(err) => {
                trace_exit!("api", "Regex::from_ast_with_mode", "ok=false,error={}", err);
                return Err(err);
            }
        };

        let regex = Self {
            engine,
            pattern: String::new(),
        };
        trace_exit!("api", "Regex::from_ast_with_mode", "ok=true");
        Ok(regex)
    }

    /// Find all matches in the given text.
    ///
    /// This method is optimized for bulk processing and will use SIMD
    /// instructions when beneficial.
    #[must_use]
    pub fn find_all(&self, text: &str) -> Vec<MatchResult> {
        trace_enter!("api", "Regex::find_all", "text_len={}", text.len());
        // Pure-literal short-circuit (see `find_first` for rationale).
        if self.engine.has_literal_finder() {
            let matches = self.engine.vm_find_all(text);
            trace_exit!(
                "api",
                "Regex::find_all",
                "ok=true,matches={},path=literal_finder",
                matches.len()
            );
            return matches;
        }
        // 5-tier dispatch — AC → DFA → Pike-VM → JIT → interpreter.
        // AC fires only for top-level alternations of pure ASCII
        // literals (`cat|dog|bird`); the rest of the chain is the
        // 4-tier DFA → Pike-VM → JIT → interpreter path. The JIT
        // tier sits AFTER Pike-VM because Pike-VM is the safety net
        // for patterns that risk catastrophic backtracking — the
        // JIT inherits that risk since it's a JIT'd backtracking
        // VM, not an NFA simulator.
        let matches = if let Some(ac_result) = self.engine.try_ac_find_all(text.as_bytes()) {
            ac_result
        } else if let Some(dfa_result) = self.engine.try_dfa_find_all(text.as_bytes()) {
            dfa_result
        } else if let Some(pike_result) = self.engine.try_pike_find_all(text.as_bytes()) {
            pike_result
        } else if let Some(jit_result) = jit_dispatch_find_all(&self.engine, text.as_bytes()) {
            jit_result
        } else {
            self.engine.vm_find_all(text)
        };
        trace_decision!(
            "api",
            "matches.is_empty()",
            matches.is_empty(),
            "find_all result cardinality={}",
            matches.len()
        );
        trace_exit!(
            "api",
            "Regex::find_all",
            "ok=true,matches={}",
            matches.len()
        );
        matches
    }

    /// Find the first match in the given text.
    ///
    /// Optimized for early termination when only one match is needed.
    #[must_use]
    pub fn find_first(&self, text: &str) -> Option<MatchResult> {
        trace_enter!("api", "Regex::find_first", "text_len={}", text.len());
        // Pure-literal short-circuit: when the VM has a
        // `memmem::Finder` for the pattern, all four C2/JIT
        // dispatch helpers gate on `has_literal_finder` and return
        // None. Calling them is pure overhead (~100-200ns/call on
        // 32-byte inputs). Skip the chain entirely.
        if self.engine.has_literal_finder() {
            let first = self.engine.vm_find_first(text);
            trace_exit!(
                "api",
                "Regex::find_first",
                "ok=true,found={},path=literal_finder",
                first.is_some()
            );
            return first;
        }
        // 5-tier dispatch — AC → DFA → Pike-VM → JIT → interpreter.
        // AC fires only for top-level alternations of pure ASCII
        // literals (`cat|dog|bird`); the rest of the chain is the
        // 4-tier DFA → Pike-VM → JIT → interpreter path. See
        // `find_all` for the ordering rationale.
        let first = if let Some(ac_result) = self.engine.try_ac_find_first(text.as_bytes()) {
            ac_result
        } else if let Some(dfa_result) = self.engine.try_dfa_find_first(text.as_bytes()) {
            dfa_result
        } else if let Some(pike_result) = self.engine.try_pike_find_first(text.as_bytes()) {
            pike_result
        } else if let Some(jit_result) = jit_dispatch_find_first(&self.engine, text.as_bytes()) {
            jit_result
        } else {
            self.engine.vm_find_first(text)
        };
        trace_decision!(
            "api",
            "first.is_some()",
            first.is_some(),
            "find_first completed"
        );
        trace_exit!(
            "api",
            "Regex::find_first",
            "ok=true,found={}",
            first.is_some()
        );
        first
    }

    /// Find the first match with support for async callback suspension.
    ///
    /// This is the suspendable counterpart to [`find_first`](Self::find_first).
    /// When an unregistered native callback is encountered during matching,
    /// execution suspends and returns [`MatchOutcome::Suspended`] with a
    /// [`MatchContinuation`] that captures the full VM state. The caller
    /// resolves the callback externally and calls [`resume`](Self::resume)
    /// to continue matching.
    ///
    /// For patterns without unregistered native callbacks, this behaves
    /// identically to `find_first` with negligible overhead.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use rgx_core::{Regex, ExecutionMode, ExecResult, MatchOutcome};
    ///
    /// let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
    /// let mut outcome = re.find_first_suspendable("hello cat");
    /// loop {
    ///     match outcome {
    ///         MatchOutcome::Completed(result) => {
    ///             // result is Option<MatchResult>
    ///             break;
    ///         }
    ///         MatchOutcome::Suspended(continuation) => {
    ///             // Resolve the callback externally
    ///             let _name = &continuation.pending_callback_name;
    ///             outcome = re.resume(*continuation, ExecResult::Success);
    ///         }
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn find_first_suspendable(&self, text: &str) -> MatchOutcome {
        trace_enter!(
            "api",
            "Regex::find_first_suspendable",
            "text_len={}",
            text.len()
        );
        let outcome = self.engine.vm_find_first_suspendable(text);
        trace_exit!(
            "api",
            "Regex::find_first_suspendable",
            "ok=true,suspended={}",
            matches!(outcome, MatchOutcome::Suspended(_))
        );
        outcome
    }

    /// Resume a suspended match after the caller resolves an async callback.
    ///
    /// The `callback_result` is the resolved value for the callback that
    /// caused suspension. Matching continues from where it left off:
    /// - On [`ExecResult::Success`] the VM proceeds past the code block.
    /// - On [`ExecResult::Failure`] the VM backtracks (potentially finding
    ///   an alternative match or trying the next scan position).
    /// - If another unregistered native callback is encountered, another
    ///   [`MatchOutcome::Suspended`] is returned, enabling chained
    ///   resolution.
    #[must_use]
    pub fn resume(
        &self,
        continuation: MatchContinuation,
        callback_result: ExecResult,
    ) -> MatchOutcome {
        trace_enter!(
            "api",
            "Regex::resume",
            "callback_name={}",
            continuation.pending_callback_name
        );
        let outcome = self.engine.resume(continuation, callback_result);
        trace_exit!(
            "api",
            "Regex::resume",
            "ok=true,suspended={}",
            matches!(outcome, MatchOutcome::Suspended(_))
        );
        outcome
    }

    /// Convenience method for async runtimes that resolves callbacks via a
    /// user-provided async resolver function.
    ///
    /// This drives the suspend/resume loop automatically, calling `resolver`
    /// each time a native callback needs resolution.
    ///
    /// Works with any async runtime (tokio, async-std, smol, etc.).
    pub async fn find_first_async<F, Fut>(&self, text: &str, resolver: F) -> Option<MatchResult>
    where
        F: Fn(String, ExecContextSnapshot) -> Fut,
        Fut: std::future::Future<Output = ExecResult>,
    {
        let mut outcome = self.find_first_suspendable(text);
        loop {
            match outcome {
                MatchOutcome::Completed(result) => return result,
                MatchOutcome::Suspended(continuation) => {
                    let name = continuation.pending_callback_name.clone();
                    let ctx = continuation.pending_context.clone();
                    let result = resolver(name, ctx).await;
                    outcome = self.resume(*continuation, result);
                }
            }
        }
    }

    /// Replace the first match using a winning-path `CodeBlockValue::Replacement`.
    ///
    /// Matches that do not surface a replacement payload are copied through
    /// unchanged, which keeps this API safe to use with mixed predicate and
    /// replacement-style code-block patterns.
    #[must_use]
    pub fn replace_first_with_code(&self, text: &str) -> String {
        trace_enter!(
            "api",
            "Regex::replace_first_with_code",
            "text_len={}",
            text.len()
        );
        let replaced = if let Some(first) = self.find_first(text) {
            Self::apply_code_replacements(text, std::iter::once(first))
        } else {
            text.to_string()
        };
        trace_exit!(
            "api",
            "Regex::replace_first_with_code",
            "ok=true,output_len={}",
            replaced.len()
        );
        replaced
    }

    /// Replace all matches using winning-path `CodeBlockValue::Replacement` values.
    ///
    /// Matches that do not surface a replacement payload are copied through
    /// unchanged, which keeps this API safe to use with mixed predicate and
    /// replacement-style code-block patterns.
    #[must_use]
    pub fn replace_all_with_code(&self, text: &str) -> String {
        trace_enter!(
            "api",
            "Regex::replace_all_with_code",
            "text_len={}",
            text.len()
        );
        let replaced = Self::apply_code_replacements(text, self.find_all(text));
        trace_exit!(
            "api",
            "Regex::replace_all_with_code",
            "ok=true,output_len={}",
            replaced.len()
        );
        replaced
    }

    /// Find the first winning-path `CodeBlockValue::Numeric` surfaced by any match.
    ///
    /// Matches whose winning path produces only predicate-style or replacement-style
    /// results are skipped, which keeps this API useful with mixed code-block patterns.
    #[must_use]
    pub fn find_first_numeric_with_code(&self, text: &str) -> Option<f64> {
        trace_enter!(
            "api",
            "Regex::find_first_numeric_with_code",
            "text_len={}",
            text.len()
        );
        let numeric = self
            .find_all(text)
            .into_iter()
            .find_map(|m| Self::numeric_code_result(m.code_result.as_ref()));
        trace_decision!(
            "api",
            "numeric.is_some()",
            numeric.is_some(),
            "find_first_numeric_with_code completed"
        );
        trace_exit!(
            "api",
            "Regex::find_first_numeric_with_code",
            "ok=true,found={}",
            numeric.is_some()
        );
        numeric
    }

    /// Collect all winning-path `CodeBlockValue::Numeric` values surfaced by matches.
    ///
    /// Matches whose winning path produces only predicate-style or replacement-style
    /// results are skipped, preserving match-order numeric output for mixed patterns.
    #[must_use]
    pub fn find_all_numeric_with_code(&self, text: &str) -> Vec<f64> {
        trace_enter!(
            "api",
            "Regex::find_all_numeric_with_code",
            "text_len={}",
            text.len()
        );
        let numeric = Self::collect_numeric_code_results(self.find_all(text));
        trace_exit!(
            "api",
            "Regex::find_all_numeric_with_code",
            "ok=true,count={}",
            numeric.len()
        );
        numeric
    }

    /// Test if the pattern matches the text (boolean result only).
    ///
    /// This is the fastest possible operation as it can terminate as soon
    /// as any match is found without capturing details.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        trace_enter!("api", "Regex::is_match", "text_len={}", text.len());
        // Pure-literal short-circuit (see `find_first` for rationale).
        if self.engine.has_literal_finder() {
            let matched = self.engine.vm_is_match(text);
            trace_exit!(
                "api",
                "Regex::is_match",
                "ok=true,matched={},path=literal_finder",
                matched
            );
            return matched;
        }
        // 5-tier dispatch — AC → DFA → Pike-VM → JIT → interpreter.
        // AC fires only for top-level alternations of pure ASCII
        // literals (`cat|dog|bird`); the rest of the chain is
        // unchanged. See `find_all` for the DFA → Pike-VM → JIT
        // ordering rationale.
        let matched = if let Some(matched) = self.engine.try_ac_is_match(text.as_bytes()) {
            matched
        } else if let Some(matched) = self.engine.try_dfa_is_match(text.as_bytes()) {
            matched
        } else if let Some(matched) = self.engine.try_pike_is_match(text.as_bytes()) {
            matched
        } else if let Some(matched) = jit_dispatch_is_match(&self.engine, text.as_bytes()) {
            matched
        } else {
            self.engine.vm_is_match(text)
        };
        trace_decision!(
            "api",
            "engine.is_match(text)",
            matched,
            "boolean API match result"
        );
        trace_exit!("api", "Regex::is_match", "ok=true,matched={}", matched);
        matched
    }

    /// Find the first match starting the scan at byte position `start`.
    ///
    /// This is the position-aware counterpart to [`find_first`](Self::find_first).
    /// The engine begins scanning at `start` rather than position 0, but
    /// positions in the returned [`MatchResult`] are still absolute (relative
    /// to the beginning of `text`).
    ///
    /// Useful for tokenization, parsing, and custom scanning loops where the
    /// caller controls the scan cursor.
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn find_first_at(&self, text: &str, start: usize) -> Option<MatchResult> {
        self.engine.vm_find_first_at(text, start)
    }

    /// Find all non-overlapping matches starting the scan at byte position `start`.
    ///
    /// This is the position-aware counterpart to [`find_all`](Self::find_all).
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn find_all_at(&self, text: &str, start: usize) -> Vec<MatchResult> {
        self.engine.vm_find_all_at(text, start)
    }

    /// Boolean match test starting the scan at byte position `start`.
    ///
    /// This is the position-aware counterpart to [`is_match`](Self::is_match).
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn is_match_at(&self, text: &str, start: usize) -> bool {
        self.engine.vm_is_match_at(text, start)
    }

    /// Split the input text by the pattern, returning the substrings between matches.
    ///
    /// This behaves like [`str::split`] but uses a regex as the delimiter.
    /// Empty strings are included when matches are adjacent or at the
    /// beginning/end of the input.
    ///
    /// ```rust,no_run
    /// # use rgx_core::Regex;
    /// let re = Regex::compile(r"[,;\s]+").unwrap();
    /// let parts = re.split("one, two; three  four");
    /// assert_eq!(parts, vec!["one", "two", "three", "four"]);
    /// ```
    #[must_use]
    pub fn split<'a>(&self, text: &'a str) -> Vec<&'a str> {
        let matches = self.find_all(text);
        if matches.is_empty() {
            return vec![text];
        }
        let mut parts = Vec::with_capacity(matches.len() + 1);
        let mut last_end = 0;
        for m in &matches {
            parts.push(&text[last_end..m.start]);
            last_end = m.end;
        }
        parts.push(&text[last_end..]);
        parts
    }

    /// Split the input text by the pattern, returning at most `limit` substrings.
    ///
    /// The last element contains the remainder of the string after `limit - 1`
    /// splits. If `limit` is 0, behaves identically to [`split`](Self::split).
    ///
    /// ```rust,no_run
    /// # use rgx_core::Regex;
    /// let re = Regex::compile(r",").unwrap();
    /// let parts = re.splitn("a,b,c,d", 3);
    /// assert_eq!(parts, vec!["a", "b", "c,d"]);
    /// ```
    #[must_use]
    pub fn splitn<'a>(&self, text: &'a str, limit: usize) -> Vec<&'a str> {
        if limit == 0 {
            return self.split(text);
        }
        if limit == 1 {
            return vec![text];
        }
        let matches = self.find_all(text);
        if matches.is_empty() {
            return vec![text];
        }
        let max_splits = limit - 1;
        let mut parts = Vec::with_capacity(limit);
        let mut last_end = 0;
        for m in matches.iter().take(max_splits) {
            parts.push(&text[last_end..m.start]);
            last_end = m.end;
        }
        parts.push(&text[last_end..]);
        parts
    }

    /// Replace the first match using a [`Replacer`].
    ///
    /// Returns `Cow::Borrowed(text)` when there is no match (zero allocation).
    ///
    /// The replacer can be:
    /// - A `&str` or `String` — template with `$1`/`$name`/`$&`/`$$` interpolation
    /// - A closure `|caps: &Captures| -> impl AsRef<str>` — programmatic replacement
    /// - A [`NoExpand`] wrapper — literal string, no interpolation
    ///
    /// ```rust,no_run
    /// # use rgx_core::Regex;
    /// let re = Regex::compile(r"(\w+)\s(\w+)").unwrap();
    /// // Template interpolation:
    /// assert_eq!(re.replace("hello world", "$2 $1"), "world hello");
    /// // Closure:
    /// let result = re.replace("hello world", |caps: &rgx_core::Captures| {
    ///     caps[1].to_uppercase()
    /// });
    /// assert_eq!(result, "HELLO world");
    /// ```
    #[must_use]
    pub fn replace<'t, R: Replacer>(&self, text: &'t str, mut rep: R) -> std::borrow::Cow<'t, str> {
        let Some(mr) = self.engine.vm_find_first(text) else {
            return std::borrow::Cow::Borrowed(text);
        };
        let mut result = String::with_capacity(text.len());
        result.push_str(&text[..mr.start]);
        if let Some(literal) = rep.no_expansion() {
            result.push_str(&literal);
        } else {
            let caps = Captures {
                text,
                groups: mr.groups,
                named: std::sync::Arc::new(self.engine.named_groups().clone()),
                named_all: std::sync::Arc::new(self.engine.named_groups_all().clone()),
                last_mark: mr.last_mark,
            };
            rep.replace_append(&caps, &mut result);
        }
        result.push_str(&text[mr.end..]);
        std::borrow::Cow::Owned(result)
    }

    /// Replace all non-overlapping matches using a [`Replacer`].
    ///
    /// Returns `Cow::Borrowed(text)` when there are no matches.
    /// See [`replace`](Self::replace) for replacer options.
    #[must_use]
    pub fn replace_all<'t, R: Replacer>(&self, text: &'t str, rep: R) -> std::borrow::Cow<'t, str> {
        self.replacen(text, 0, rep)
    }

    /// Replace up to `limit` non-overlapping matches using a [`Replacer`].
    ///
    /// `replacen(text, 0, rep)` replaces all. `replacen(text, 1, rep)` replaces
    /// only the first.
    #[must_use]
    pub fn replacen<'t, R: Replacer>(
        &self,
        text: &'t str,
        limit: usize,
        mut rep: R,
    ) -> std::borrow::Cow<'t, str> {
        let matches = self.engine.vm_find_all(text);
        if matches.is_empty() {
            return std::borrow::Cow::Borrowed(text);
        }
        let effective = if limit == 0 {
            matches.len()
        } else {
            limit.min(matches.len())
        };
        let literal = rep.no_expansion().map(|c| c.into_owned());
        let named = std::sync::Arc::new(self.engine.named_groups().clone());
        let named_all = std::sync::Arc::new(self.engine.named_groups_all().clone());
        let mut result = String::with_capacity(text.len());
        let mut last_end = 0;
        for m in matches.iter().take(effective) {
            result.push_str(&text[last_end..m.start]);
            if let Some(ref lit) = literal {
                result.push_str(lit);
            } else {
                let caps = Captures {
                    text,
                    groups: m.groups.clone(),
                    named: named.clone(),
                    named_all: named_all.clone(),
                    last_mark: m.last_mark.clone(),
                };
                rep.replace_append(&caps, &mut result);
            }
            last_end = m.end;
        }
        result.push_str(&text[last_end..]);
        std::borrow::Cow::Owned(result)
    }

    /// Find the first match, returning a [`Match`] that borrows the input text.
    ///
    /// This is the ergonomic counterpart to [`find_first`](Self::find_first).
    /// Use `m.as_str()` to get the matched substring directly.
    #[must_use]
    pub fn find<'t>(&self, text: &'t str) -> Option<Match<'t>> {
        self.find_first(text).map(|m| Match {
            text,
            start: m.start,
            end: m.end,
        })
    }

    /// Get capture groups for the first match.
    ///
    /// Returns a [`Captures`] object with ergonomic access by index or name.
    #[must_use]
    pub fn captures<'t>(&self, text: &'t str) -> Option<Captures<'t>> {
        self.find_first(text).map(|m| Captures {
            text,
            groups: m.groups,
            named: std::sync::Arc::new(self.engine.named_groups().clone()),
            named_all: std::sync::Arc::new(self.engine.named_groups_all().clone()),
            last_mark: m.last_mark,
        })
    }

    /// Return only the end byte offset of the first match.
    ///
    /// Faster than [`find`](Self::find) when you only need to know *where*
    /// a match ends (e.g., tokenizers, validators).
    #[must_use]
    pub fn shortest_match(&self, text: &str) -> Option<usize> {
        self.find_first(text).map(|m| m.end)
    }

    /// Return only the end byte offset of the first match starting at `start`.
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn shortest_match_at(&self, text: &str, start: usize) -> Option<usize> {
        self.find_first_at(text, start).map(|m| m.end)
    }

    /// The original pattern string used to compile this regex.
    ///
    /// Returns an empty string for regexes compiled from AST.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.pattern
    }

    /// The number of capture groups (including group 0 for the overall match).
    #[must_use]
    pub fn captures_len(&self) -> usize {
        self.engine.num_groups() as usize + 1
    }

    /// Create a reusable [`CaptureLocations`] buffer sized for this regex.
    ///
    /// Use with [`captures_read`](Self::captures_read) to avoid per-match
    /// allocation in tight loops.
    #[must_use]
    pub fn capture_locations(&self) -> CaptureLocations {
        CaptureLocations {
            slots: vec![None; self.captures_len()],
        }
    }

    /// Fill `locs` with capture positions for the first match, returning
    /// the overall match as a [`Match`].
    ///
    /// This avoids allocating a new `Vec` per match — ideal for loops
    /// that process millions of inputs.
    #[must_use]
    pub fn captures_read<'t>(
        &self,
        text: &'t str,
        locs: &mut CaptureLocations,
    ) -> Option<Match<'t>> {
        let mr = self.find_first(text)?;
        // Copy group positions into the reusable buffer.
        for (i, slot) in locs.slots.iter_mut().enumerate() {
            *slot = mr.groups.get(i).copied().flatten();
        }
        Some(Match {
            text,
            start: mr.start,
            end: mr.end,
        })
    }

    /// Fill `locs` with capture positions for the first match starting at
    /// `start`, returning the overall match as a [`Match`].
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn captures_read_at<'t>(
        &self,
        text: &'t str,
        start: usize,
        locs: &mut CaptureLocations,
    ) -> Option<Match<'t>> {
        let mr = self.find_first_at(text, start)?;
        for (i, slot) in locs.slots.iter_mut().enumerate() {
            *slot = mr.groups.get(i).copied().flatten();
        }
        Some(Match {
            text,
            start: mr.start,
            end: mr.end,
        })
    }

    /// Iterator over capture group names.
    ///
    /// Yields `None` for unnamed groups (including group 0) and
    /// `Some(name)` for named groups.
    pub fn capture_names(&self) -> CaptureNames<'_> {
        CaptureNames {
            named: self.engine.named_groups(),
            num_groups: self.engine.num_groups(),
            idx: 0,
        }
    }

    /// Lazy iterator over successive non-overlapping matches.
    ///
    /// Unlike [`find_all`](Self::find_all), this does not allocate a `Vec`.
    /// Matches are found on demand as the iterator is advanced.
    pub fn find_iter<'r, 't>(&'r self, text: &'t str) -> FindIter<'r, 't> {
        FindIter {
            regex: self,
            text,
            last_end: 0,
            last_match_end: None,
            done: false,
        }
    }

    /// Lazy iterator over successive non-overlapping matches with capture groups.
    ///
    /// Each item is a [`Captures`] object with ergonomic group access.
    pub fn captures_iter<'r, 't>(&'r self, text: &'t str) -> CaptureIter<'r, 't> {
        CaptureIter {
            inner: self.find_iter(text),
            named: std::sync::Arc::new(self.engine.named_groups().clone()),
            named_all: std::sync::Arc::new(self.engine.named_groups_all().clone()),
        }
    }

    /// Lazy iterator over substrings delimited by regex matches.
    ///
    /// Unlike [`split`](Self::split), this does not allocate a `Vec`.
    pub fn split_iter<'r, 't>(&'r self, text: &'t str) -> SplitIter<'r, 't> {
        SplitIter {
            finder: self.find_iter(text),
            last_end: 0,
            done: false,
        }
    }

    /// Lazy iterator over substrings delimited by regex matches, with a limit.
    ///
    /// The last item contains the unsplit remainder. Unlike
    /// [`splitn`](Self::splitn), this does not allocate a `Vec`.
    pub fn splitn_iter<'r, 't>(&'r self, text: &'t str, limit: usize) -> SplitNIter<'r, 't> {
        SplitNIter {
            finder: self.find_iter(text),
            last_end: 0,
            limit,
            count: 0,
            done: limit == 0,
        }
    }

    /// Set the maximum number of VM opcode steps per match attempt.
    ///
    /// Prevents exponential backtracking from hanging the engine on
    /// pathological patterns like `(a+)+b`. When the limit is reached the
    /// match attempt fails (returns no-match). The scanning loop may still
    /// try other start positions.
    ///
    /// Pass `None` to remove the limit (default). Pass `Some(n)` to cap
    /// each attempt at `n` opcode steps.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use rgx_core::Regex;
    /// let re = Regex::compile(r"(a+)+b").unwrap();
    /// re.set_max_steps(Some(10_000));
    /// // On pathological input, this returns None instead of hanging:
    /// assert!(re.find_first("aaaaaaaaaaaaaaaaaaaac").is_none());
    /// ```
    pub fn set_max_steps(&self, limit: Option<u64>) {
        self.engine.set_max_steps(limit);
    }

    /// Set the maximum backtrack stack depth per match attempt.
    ///
    /// When the limit is exceeded the match attempt fails. Pass `None`
    /// to remove the limit (default).
    pub fn set_max_backtrack_frames(&self, limit: Option<u64>) {
        self.engine.set_max_backtrack_frames(limit);
    }

    /// Set the maximum recursion depth per match attempt.
    ///
    /// Overrides the default hard limit of 1024. Pass `None` to revert
    /// to the default.
    pub fn set_max_recursion_depth(&self, limit: Option<u64>) {
        self.engine.set_max_recursion_depth(limit);
    }

    /// Find the first match, or report a partial match when the input ends
    /// while the pattern could still be matching.
    ///
    /// Useful for streaming/incremental matching where input arrives in chunks.
    ///
    /// Returns:
    /// - `Full(MatchResult)` — a complete match was found
    /// - `Partial(offset)` — the input ended while a match was in progress
    ///   at byte offset `offset`. Appending more data may complete the match.
    /// - `NoMatch` — no match is possible even with more data
    ///
    /// ```rust,no_run
    /// # use rgx_core::{Regex, PartialMatchResult};
    /// let re = Regex::compile(r"hello world").unwrap();
    /// match re.find_first_partial("hello wor") {
    ///     PartialMatchResult::Partial(_) => println!("need more input"),
    ///     PartialMatchResult::Full(m) => println!("matched: {}..{}", m.start, m.end),
    ///     PartialMatchResult::NoMatch => println!("no match possible"),
    /// }
    /// ```
    #[must_use]
    pub fn find_first_partial(&self, text: &str) -> PartialMatchResult {
        self.engine.vm_find_first_partial(text)
    }

    /// Set the match semantics.
    ///
    /// - [`LeftmostFirst`](MatchSemantics::LeftmostFirst) (default): first alternative wins.
    ///   `a|ab` on "ab" → "a".
    /// - [`LeftmostLongest`](MatchSemantics::LeftmostLongest): longest match at each position wins.
    ///   `a|ab` on "ab" → "ab".
    pub fn set_match_semantics(&self, semantics: MatchSemantics) {
        self.engine.set_match_semantics(semantics);
    }

    /// Register a native callback for `(?{native:...})` code blocks on this compiled regex.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when the compiled regex has no execution manager or
    /// when the callback cannot be registered.
    pub fn register_native<F>(&self, name: impl Into<String>, callback: F) -> Result<()>
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        let name = name.into();
        trace_enter!("api", "Regex::register_native", "name={}", name);
        let result = self.engine.register_native(&name, callback);
        trace_exit!("api", "Regex::register_native", "ok={}", result.is_ok());
        result
    }

    /// Register a PCRE2-style callout handler by number.
    ///
    /// `(?C)` invokes callout 0, `(?C123)` invokes callout 123. Internally this
    /// registers a native callback named `__callout_N`.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when the compiled regex has no execution manager.
    pub fn register_callout<F>(&self, number: u32, callback: F) -> Result<()>
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        let name = format!("__callout_{number}");
        self.register_native(name, callback)
    }

    /// Register a named wasm module for `(?{wasm:module:function})` code blocks on this compiled regex.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when the compiled regex has no execution manager or
    /// when the wasm module cannot be compiled or registered.
    pub fn register_wasm_module(
        &self,
        name: impl Into<String>,
        module_bytes: impl AsRef<[u8]>,
    ) -> Result<()> {
        let name = name.into();
        let module_bytes = module_bytes.as_ref().to_vec();
        trace_enter!(
            "api",
            "Regex::register_wasm_module",
            "name={},byte_len={}",
            name,
            module_bytes.len()
        );
        let result = self.engine.register_wasm_module(name, module_bytes);
        trace_exit!(
            "api",
            "Regex::register_wasm_module",
            "ok={}",
            result.is_ok()
        );
        result
    }

    /// Register or replace a host-provided execution variable for code-block evaluation.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when the compiled regex has no execution manager.
    pub fn set_variable(&self, name: impl Into<String>, value: impl Into<String>) -> Result<()> {
        let name = name.into();
        let value = value.into();
        trace_enter!(
            "api",
            "Regex::set_variable",
            "name={},value_len={}",
            name,
            value.len()
        );
        let result = self.engine.set_variable(&name, value);
        trace_exit!("api", "Regex::set_variable", "ok={}", result.is_ok());
        result
    }

    /// Register or replace a typed host-provided execution variable for code-block evaluation.
    ///
    /// When a typed variable is set, the legacy string variable (accessible via
    /// [`ExecContext::variable`]) is also updated with the `Display` representation
    /// of the value for backward compatibility.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when the compiled regex has no execution manager.
    pub fn set_typed_variable(&self, name: impl Into<String>, value: Value) -> Result<()> {
        let name = name.into();
        trace_enter!("api", "Regex::set_typed_variable", "name={}", name);
        let result = self.engine.set_typed_variable(&name, value);
        trace_exit!("api", "Regex::set_typed_variable", "ok={}", result.is_ok());
        result
    }

    /// Set a host variable with automatic type conversion.
    ///
    /// Accepts strings, integers, floats, booleans, arrays, and maps:
    /// ```ignore
    /// re.set_var("threshold", 100)?;
    /// re.set_var("rate", 0.08)?;
    /// re.set_var("debug", true)?;
    /// re.set_var("name", "alice")?;
    /// re.set_var("tags", vec!["a", "b", "c"])?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`RgxError`] when the compiled regex has no execution manager.
    pub fn set_var<V: Into<Value>>(&self, name: &str, value: V) -> Result<()> {
        self.set_typed_variable(name, value.into())
    }

    /// Start a fluent builder for host variables.
    ///
    /// Returns a [`VarsBuilder`](vars::VarsBuilder) that lets you set scalars,
    /// arrays, and nested maps without constructing [`Value`] manually:
    ///
    /// ```rust,no_run
    /// # use rgx_core::{Regex, ExecutionMode};
    /// let re = Regex::with_mode(r".", ExecutionMode::Full).unwrap();
    /// re.vars()
    ///     .set("env", "prod")
    ///     .hash("db")
    ///         .set("host", "localhost")
    ///         .set("port", 5432_i64)
    ///         .done();
    /// ```
    #[must_use]
    pub fn vars(&self) -> vars::VarsBuilder<'_> {
        vars::VarsBuilder::new(self)
    }

    /// Set multiple host variables from a [`Value::Map`].
    ///
    /// Designed to work with the [`value!`] macro for JSON-style declarations:
    ///
    /// ```rust,no_run
    /// # use rgx_core::{Regex, value};
    /// let re = Regex::compile("test").unwrap();
    /// re.set_vars(value!({
    ///     "env" => "prod",
    ///     "port" => 8080_i64,
    ///     "db" => {
    ///         "host" => "localhost",
    ///         "replicas" => ["r1.example.com", "r2.example.com"]
    ///     }
    /// }));
    /// ```
    pub fn set_vars(&self, map: Value) {
        if let Value::Map(entries) = map {
            for (key, val) in entries {
                let _ = self.set_typed_variable(&key, val);
            }
        }
    }

    /// Register an event observer for structured match events.
    ///
    /// The observer receives [`MatchEvent`] values at key execution points
    /// such as match-attempt start/completion, backtrack, capture completion,
    /// and code-block evaluation. Events are fire-and-forget and do not
    /// affect match behavior.
    ///
    /// Only one observer may be active; calling this again replaces any
    /// previous observer.
    ///
    /// # Errors
    ///
    /// This method is infallible; the `Result` wrapper is retained for
    /// forward-compatibility with future observer validation.
    pub fn on_event<F>(&self, observer: F) -> Result<()>
    where
        F: Fn(&MatchEvent) + Send + Sync + 'static,
    {
        trace_enter!("api", "Regex::on_event");
        self.engine.set_event_observer(observer);
        trace_exit!("api", "Regex::on_event", "ok=true");
        Ok(())
    }

    fn apply_code_replacements<I>(text: &str, matches: I) -> String
    where
        I: IntoIterator<Item = MatchResult>,
    {
        let mut output = String::with_capacity(text.len());
        let mut cursor = 0;

        for m in matches {
            if m.start > cursor {
                output.push_str(&text[cursor..m.start]);
            }

            match m.code_result {
                Some(CodeBlockValue::Replacement(value)) => output.push_str(&value),
                _ => output.push_str(&text[m.start..m.end]),
            }

            cursor = m.end;
        }

        output.push_str(&text[cursor..]);
        output
    }

    fn collect_numeric_code_results<I>(matches: I) -> Vec<f64>
    where
        I: IntoIterator<Item = MatchResult>,
    {
        trace_enter!("api", "Regex::collect_numeric_code_results");
        let numeric = matches
            .into_iter()
            .filter_map(|m| Self::numeric_code_result(m.code_result.as_ref()))
            .collect::<Vec<_>>();
        trace_exit!(
            "api",
            "Regex::collect_numeric_code_results",
            "ok=true,count={}",
            numeric.len()
        );
        numeric
    }

    fn numeric_code_result(code_result: Option<&CodeBlockValue>) -> Option<f64> {
        match code_result {
            Some(CodeBlockValue::Numeric(value)) => Some(*value),
            _ => None,
        }
    }

    /// Extract capture groups from a `MatchResult` as `(&str, Option<&str>)`
    /// tuples for interpolation. Index 0 = full match.
    fn capture_groups_for_match<'a>(&self, text: &'a str, m: &MatchResult) -> Vec<Option<&'a str>> {
        m.groups
            .iter()
            .map(|slot| slot.map(|(s, e)| &text[s..e]))
            .collect()
    }

    /// Interpolate `$0`, `$1`, `$name`, `${name}`, `$$`, `$&` in a
    /// replacement string, appending the result to `out`.
    ///
    /// `last_mark` carries the PCRE2 `(*MARK:name)` value threaded
    /// through from the match result. When present, `${*MARK}` /
    /// `$*MARK` in the template expand to that name.
    fn interpolate_replacement(
        replacement: &str,
        groups: &[Option<&str>],
        named: &std::collections::HashMap<String, u32>,
        last_mark: Option<&str>,
        out: &mut String,
    ) {
        Self::interpolate_replacement_ext(replacement, groups, named, None, last_mark, out);
    }

    /// Extended form of `interpolate_replacement` that threads the
    /// multi-id named-group map through to `push_group_by_ref_ext` so
    /// `$name` template references with dupnames (PCRE2 `(?J)`) pick
    /// whichever definition actually captured.
    fn interpolate_replacement_ext(
        replacement: &str,
        groups: &[Option<&str>],
        named: &std::collections::HashMap<String, u32>,
        named_all: Option<&std::collections::HashMap<String, Vec<u32>>>,
        last_mark: Option<&str>,
        out: &mut String,
    ) {
        // PCRE2 accepts a leading `[N]` in a substitute template as
        // an advisory buffer-size hint (pcre2_substitute stripped it
        // from the emitted replacement). RGX has no equivalent buffer
        // allocation, so we simply strip the prefix to match PCRE2's
        // observable output for templates like `[10]XYZ`.
        let replacement = strip_substitute_length_hint(replacement);
        #[derive(Clone, Copy)]
        #[allow(clippy::upper_case_acronyms)]
        enum CaseChange {
            None,
            Upper,
            Lower,
        }
        fn first_upper(ch: char) -> char {
            ch.to_uppercase().next().unwrap_or(ch)
        }
        fn first_lower(ch: char) -> char {
            ch.to_lowercase().next().unwrap_or(ch)
        }
        // Case-change state for `\u`, `\l`, `\U`, `\L`, `\E`.
        // `next` applies to the very next produced character; `region`
        // applies to every character produced until `\E`. `next` takes
        // precedence over `region` for a single character, then clears.
        let mut case_next = CaseChange::None;
        let mut case_region = CaseChange::None;
        let mut push_with_case =
            |out: &mut String, s: &str, next: &mut CaseChange, region: CaseChange| {
                for ch in s.chars() {
                    let applied = match *next {
                        CaseChange::None => match region {
                            CaseChange::None => ch,
                            CaseChange::Upper => first_upper(ch),
                            CaseChange::Lower => first_lower(ch),
                        },
                        CaseChange::Upper => first_upper(ch),
                        CaseChange::Lower => first_lower(ch),
                    };
                    out.push(applied);
                    *next = CaseChange::None;
                }
            };

        let bytes = replacement.as_bytes();
        let len = bytes.len();
        let mut i = 0;
        while i < len {
            if bytes[i] == b'$' && i + 1 < len {
                i += 1;
                if bytes[i] == b'$' {
                    push_with_case(out, "$", &mut case_next, case_region);
                    i += 1;
                } else if bytes[i] == b'&' {
                    if let Some(Some(s)) = groups.first() {
                        push_with_case(out, s, &mut case_next, case_region);
                    }
                    i += 1;
                } else if bytes[i] == b'{' {
                    if let Some(close) = replacement[i + 1..].find('}') {
                        let inner = &replacement[i + 1..i + 1 + close];
                        let mut scratch = String::new();
                        if inner == "*MARK" {
                            // PCRE2 `${*MARK}` — expand to the last
                            // `(*MARK:name)` name hit on the match path
                            // (empty string when no mark was seen).
                            if let Some(mark) = last_mark {
                                scratch.push_str(mark);
                            }
                        } else {
                            Self::push_group_by_ref_ext(
                                inner,
                                groups,
                                named,
                                named_all,
                                &mut scratch,
                            );
                        }
                        push_with_case(out, &scratch, &mut case_next, case_region);
                        i = i + 2 + close;
                    } else {
                        push_with_case(out, "${", &mut case_next, case_region);
                        i += 1;
                    }
                } else if bytes[i] == b'*' {
                    // PCRE2 `$*MARK` — bare form (no braces).
                    if replacement[i..].starts_with("*MARK") {
                        if let Some(mark) = last_mark {
                            let mut scratch = String::new();
                            scratch.push_str(mark);
                            push_with_case(out, &scratch, &mut case_next, case_region);
                        }
                        i += "*MARK".len();
                    } else {
                        push_with_case(out, "$*", &mut case_next, case_region);
                        i += 1;
                    }
                } else if bytes[i].is_ascii_digit() {
                    let start = i;
                    while i < len && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if let Ok(idx) = replacement[start..i].parse::<usize>() {
                        if let Some(Some(s)) = groups.get(idx) {
                            push_with_case(out, s, &mut case_next, case_region);
                        }
                    }
                } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                    let start = i;
                    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        i += 1;
                    }
                    let name = &replacement[start..i];
                    let mut scratch = String::new();
                    Self::push_group_by_ref_ext(name, groups, named, named_all, &mut scratch);
                    push_with_case(out, &scratch, &mut case_next, case_region);
                } else {
                    push_with_case(out, "$", &mut case_next, case_region);
                }
            } else if bytes[i] == b'\\' && i + 1 < len {
                // Perl-style backslash escapes in the replacement string.
                // Covers `\\`, `\$`, control-char escapes (`\n`, `\r`, ...),
                // octal / hex numeric escapes, `\NN` group back-references,
                // and `\u \l \U \L \E` case-change directives per
                // pcre2pattern(3) §"REPLACEMENT STRINGS" semantics.
                i += 1;
                let c = bytes[i];
                match c {
                    b'\\' => {
                        push_with_case(out, "\\", &mut case_next, case_region);
                        i += 1;
                    }
                    b'$' => {
                        push_with_case(out, "$", &mut case_next, case_region);
                        i += 1;
                    }
                    b'n' => {
                        push_with_case(out, "\n", &mut case_next, case_region);
                        i += 1;
                    }
                    b'r' => {
                        push_with_case(out, "\r", &mut case_next, case_region);
                        i += 1;
                    }
                    b't' => {
                        push_with_case(out, "\t", &mut case_next, case_region);
                        i += 1;
                    }
                    b'a' => {
                        push_with_case(out, "\u{07}", &mut case_next, case_region);
                        i += 1;
                    }
                    b'e' => {
                        push_with_case(out, "\u{1B}", &mut case_next, case_region);
                        i += 1;
                    }
                    b'f' => {
                        push_with_case(out, "\u{0C}", &mut case_next, case_region);
                        i += 1;
                    }
                    b'u' => {
                        case_next = CaseChange::Upper;
                        i += 1;
                    }
                    b'l' => {
                        case_next = CaseChange::Lower;
                        i += 1;
                    }
                    b'U' => {
                        case_region = CaseChange::Upper;
                        i += 1;
                    }
                    b'L' => {
                        case_region = CaseChange::Lower;
                        i += 1;
                    }
                    b'E' => {
                        case_region = CaseChange::None;
                        i += 1;
                    }
                    b'x' => {
                        // `\x{N...}` hex escape. Pcre2 also accepts
                        // `\xHH` (two hex digits, no braces) but the
                        // replacement forms we encounter in the
                        // conformance tests are brace-wrapped.
                        if i + 1 < len && bytes[i + 1] == b'{' {
                            if let Some(close) = replacement[i + 2..].find('}') {
                                let hex = &replacement[i + 2..i + 2 + close];
                                if let Ok(cp) = u32::from_str_radix(hex, 16) {
                                    if let Some(ch) = char::from_u32(cp) {
                                        let mut buf = [0u8; 4];
                                        let s = ch.encode_utf8(&mut buf);
                                        push_with_case(out, s, &mut case_next, case_region);
                                    }
                                }
                                i = i + 3 + close;
                            } else {
                                push_with_case(out, "\\x{", &mut case_next, case_region);
                                i += 2;
                            }
                        } else if i + 2 < len
                            && bytes[i + 1].is_ascii_hexdigit()
                            && bytes[i + 2].is_ascii_hexdigit()
                        {
                            let hex = &replacement[i + 1..i + 3];
                            if let Ok(b) = u8::from_str_radix(hex, 16) {
                                if let Some(ch) = char::from_u32(u32::from(b)) {
                                    let mut buf = [0u8; 4];
                                    let s = ch.encode_utf8(&mut buf);
                                    push_with_case(out, s, &mut case_next, case_region);
                                }
                            }
                            i += 3;
                        } else {
                            push_with_case(out, "\\x", &mut case_next, case_region);
                            i += 1;
                        }
                    }
                    b'o' => {
                        // `\o{N...}` octal escape.
                        if i + 1 < len && bytes[i + 1] == b'{' {
                            if let Some(close) = replacement[i + 2..].find('}') {
                                let oct = &replacement[i + 2..i + 2 + close];
                                if let Ok(cp) = u32::from_str_radix(oct, 8) {
                                    if let Some(ch) = char::from_u32(cp) {
                                        let mut buf = [0u8; 4];
                                        let s = ch.encode_utf8(&mut buf);
                                        push_with_case(out, s, &mut case_next, case_region);
                                    }
                                }
                                i = i + 3 + close;
                            } else {
                                push_with_case(out, "\\o{", &mut case_next, case_region);
                                i += 2;
                            }
                        } else {
                            push_with_case(out, "\\o", &mut case_next, case_region);
                            i += 1;
                        }
                    }
                    d if d.is_ascii_digit() => {
                        // PCRE2 / Perl replacement-string digit escapes
                        // are deliberately dual-role:
                        //   * `\0` (and `\0NN`) — always octal (the
                        //     leading zero marks the numeric form).
                        //   * `\N` (N ∈ 1..9) — back-reference to
                        //     capture group N when that group exists
                        //     (including `None`-valued groups that
                        //     participated but didn't match); falls
                        //     back to octal otherwise. PCRE2's actual
                        //     rule is slightly more permissive for
                        //     multi-digit forms but this single-digit
                        //     check covers the common conformance
                        //     patterns (`\1`, `\2`, …) without
                        //     breaking octal literals like `\045`
                        //     against patterns with no captures.
                        if d != b'0' && ((d - b'0') as usize) < groups.len() {
                            let idx = (d - b'0') as usize;
                            if let Some(Some(s)) = groups.get(idx) {
                                push_with_case(out, s, &mut case_next, case_region);
                            }
                            i += 1;
                        } else {
                            let start = i;
                            let mut end = i + 1;
                            while end < len
                                && end - start < 3
                                && bytes[end].is_ascii_digit()
                                && bytes[end] < b'8'
                            {
                                end += 1;
                            }
                            let oct = &replacement[start..end];
                            if let Ok(cp) = u32::from_str_radix(oct, 8) {
                                if let Some(ch) = char::from_u32(cp) {
                                    let mut buf = [0u8; 4];
                                    let s = ch.encode_utf8(&mut buf);
                                    push_with_case(out, s, &mut case_next, case_region);
                                }
                            }
                            i = end;
                        }
                    }
                    _ => {
                        // Unknown escape: emit the character literally,
                        // preserving PCRE2's "a backslash before any
                        // non-metacharacter is the character itself"
                        // convention.
                        push_with_case(out, &replacement[i..i + 1], &mut case_next, case_region);
                        i += 1;
                    }
                }
            } else {
                let start = i;
                i += 1;
                push_with_case(out, &replacement[start..i], &mut case_next, case_region);
            }
        }
    }

    /// Resolve a group reference (number or name) and append to `out`.
    fn push_group_by_ref(
        reference: &str,
        groups: &[Option<&str>],
        named: &std::collections::HashMap<String, u32>,
        out: &mut String,
    ) {
        Self::push_group_by_ref_ext(reference, groups, named, None, out);
    }

    /// Extended form of `push_group_by_ref` that respects duplicate
    /// named groups: when a name has multiple registered ids and the
    /// single-id lookup returns an UNSET slot, iterate every id and
    /// emit the first one that is actually set. Matches PCRE2
    /// `pcre2_substitute` semantic for `(?J)` / alternation dupnames
    /// where `$name` picks whichever definition actually captured.
    fn push_group_by_ref_ext(
        reference: &str,
        groups: &[Option<&str>],
        named: &std::collections::HashMap<String, u32>,
        named_all: Option<&std::collections::HashMap<String, Vec<u32>>>,
        out: &mut String,
    ) {
        if let Ok(idx) = reference.parse::<usize>() {
            if let Some(Some(s)) = groups.get(idx) {
                out.push_str(s);
            }
            return;
        }
        // Prefer the multi-id map when available: find the first set
        // slot across all registered ids for this name. Falls back to
        // the single-id map for back-compat.
        if let Some(all) = named_all {
            if let Some(ids) = all.get(reference) {
                for &id in ids {
                    if let Some(Some(s)) = groups.get(id as usize) {
                        out.push_str(s);
                        return;
                    }
                }
                return;
            }
        }
        if let Some(&group_num) = named.get(reference) {
            if let Some(Some(s)) = groups.get(group_num as usize) {
                out.push_str(s);
            }
        }
    }

    /// Named capture group map: group name → 1-based group number.
    #[must_use]
    pub fn named_groups(&self) -> &std::collections::HashMap<String, u32> {
        self.engine.named_groups()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{GroupKind, Regex as RegexAst};

    #[test]
    fn basic_regex_compilation() {
        let regex = Regex::compile(r"\d+").expect("Failed to compile simple regex");
        assert!(regex.is_match("123"));
        assert!(!regex.is_match("abc"));
    }

    #[test]
    fn email_pattern_matching() {
        let regex = Regex::compile(r"\b\w+@\w+\.\w+\b").expect("Failed to compile email regex");

        let matches = regex.find_all("Contact admin@example.com or support@test.org");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn pure_performance_mode() {
        let regex = Regex::with_mode(r"\d{3}-\d{2}-\d{4}", ExecutionMode::Pure)
            .expect("Failed to compile in pure mode");

        assert!(regex.is_match("123-45-6789"));
        assert!(!regex.is_match("not-a-ssn"));
    }

    #[test]
    fn ast_compilation_without_parser() {
        let ast = RegexAst::Alternation(vec![
            RegexAst::Sequence(vec![
                RegexAst::Char('c'),
                RegexAst::Char('a'),
                RegexAst::Char('t'),
            ]),
            RegexAst::Sequence(vec![
                RegexAst::Char('d'),
                RegexAst::Char('o'),
                RegexAst::Char('g'),
            ]),
        ]);

        let regex = Regex::from_ast(ast).expect("Failed to compile AST directly");
        assert!(regex.is_match("dog"));
        assert!(regex.is_match("I saw a cat"));
        assert!(!regex.is_match("bird"));
    }

    #[test]
    fn ast_compilation_with_atomic_group_scaffold() {
        let ast = RegexAst::Group {
            expr: Box::new(RegexAst::Sequence(vec![
                RegexAst::Char('f'),
                RegexAst::Char('o'),
                RegexAst::Char('o'),
            ])),
            kind: GroupKind::Atomic,
            index: None,
            name: None,
        };

        let regex = Regex::from_ast_with_mode(ast, ExecutionMode::Pure)
            .expect("Failed to compile atomic-group AST directly");
        assert!(regex.is_match("foo"));
        assert!(regex.is_match("xxfooyy"));
        assert!(!regex.is_match("bar"));
    }

    #[test]
    fn ast_positive_lookahead_no_consume() {
        let ast = RegexAst::Sequence(vec![
            RegexAst::Lookahead {
                expr: Box::new(RegexAst::Sequence(vec![
                    RegexAst::Char('c'),
                    RegexAst::Char('a'),
                    RegexAst::Char('t'),
                ])),
                positive: true,
            },
            RegexAst::Char('c'),
        ]);

        let regex =
            Regex::from_ast(ast).expect("Failed to compile positive-lookahead AST directly");
        let m = regex.find_first("xxcat").expect("Expected lookahead match");
        assert_eq!(m.start, 2);
        assert_eq!(m.end, 3); // Lookahead itself must not consume input
    }

    #[test]
    fn ast_negative_lookahead() {
        let ast = RegexAst::Sequence(vec![
            RegexAst::Lookahead {
                expr: Box::new(RegexAst::Char('x')),
                positive: false,
            },
            RegexAst::Dot,
        ]);

        let regex =
            Regex::from_ast(ast).expect("Failed to compile negative-lookahead AST directly");
        assert!(regex.is_match("a"));
        assert!(!regex.is_match("x"));
    }

    #[test]
    fn ast_positive_lookbehind_no_consume() {
        let ast = RegexAst::Sequence(vec![
            RegexAst::Lookbehind {
                expr: Box::new(RegexAst::Sequence(vec![
                    RegexAst::Char('c'),
                    RegexAst::Char('a'),
                    RegexAst::Char('t'),
                ])),
                positive: true,
            },
            RegexAst::Char('d'),
        ]);

        let regex =
            Regex::from_ast(ast).expect("Failed to compile positive-lookbehind AST directly");
        let m = regex
            .find_first("xxcatd")
            .expect("Expected lookbehind match");
        assert_eq!(m.start, 5);
        assert_eq!(m.end, 6); // Lookbehind itself must not consume input
    }

    #[test]
    fn ast_negative_lookbehind() {
        let ast = RegexAst::Sequence(vec![
            RegexAst::Lookbehind {
                expr: Box::new(RegexAst::Char('x')),
                positive: false,
            },
            RegexAst::Char('a'),
        ]);

        let regex =
            Regex::from_ast(ast).expect("Failed to compile negative-lookbehind AST directly");
        assert!(regex.is_match("ba"));
        assert!(!regex.is_match("xa"));
    }

    #[test]
    fn ast_numeric_backreference_matches_previous_capture() {
        let ast = RegexAst::Sequence(vec![
            RegexAst::Group {
                expr: Box::new(RegexAst::Sequence(vec![
                    RegexAst::Char('a'),
                    RegexAst::Char('b'),
                ])),
                kind: GroupKind::Capturing,
                index: None,
                name: None,
            },
            RegexAst::Backreference(1),
        ]);

        let regex = Regex::from_ast(ast).expect("Failed to compile backreference AST directly");
        assert!(regex.is_match("abab"));
        assert!(!regex.is_match("abac"));
    }

    #[test]
    fn ast_conditional_group_exists_selects_runtime_branch() {
        let ast = RegexAst::Sequence(vec![
            RegexAst::Anchor(crate::ast::AnchorType::AbsStart),
            RegexAst::Quantified {
                expr: Box::new(RegexAst::Group {
                    expr: Box::new(RegexAst::Char('a')),
                    kind: GroupKind::Capturing,
                    index: None,
                    name: None,
                }),
                quantifier: crate::ast::Quantifier::ZeroOrOne { lazy: false },
            },
            RegexAst::Conditional {
                condition: crate::ast::ConditionalTest::GroupExists(1),
                true_branch: Box::new(RegexAst::Char('b')),
                false_branch: Some(Box::new(RegexAst::Char('c'))),
            },
            RegexAst::Anchor(crate::ast::AnchorType::AbsEndNoNL),
        ]);

        let regex = Regex::from_ast(ast).expect("Failed to compile conditional AST directly");
        assert!(regex.is_match("ab"));
        assert!(regex.is_match("c"));
        assert!(!regex.is_match("ac"));
    }

    #[test]
    fn ast_relative_conditional_group_exists_resolves_runtime_branch() {
        let backward = RegexAst::Sequence(vec![
            RegexAst::Anchor(crate::ast::AnchorType::AbsStart),
            RegexAst::Quantified {
                expr: Box::new(RegexAst::Group {
                    expr: Box::new(RegexAst::Char('a')),
                    kind: GroupKind::Capturing,
                    index: None,
                    name: None,
                }),
                quantifier: crate::ast::Quantifier::ZeroOrOne { lazy: false },
            },
            RegexAst::Conditional {
                condition: crate::ast::ConditionalTest::RelativeGroupExists(-1),
                true_branch: Box::new(RegexAst::Char('b')),
                false_branch: Some(Box::new(RegexAst::Char('c'))),
            },
            RegexAst::Anchor(crate::ast::AnchorType::AbsEndNoNL),
        ]);

        let backward = Regex::from_ast(backward)
            .expect("Failed to compile backward relative-conditional AST directly");
        assert!(backward.is_match("ab"));
        assert!(backward.is_match("c"));
        assert!(!backward.is_match("ac"));

        let forward = RegexAst::Sequence(vec![
            RegexAst::Anchor(crate::ast::AnchorType::AbsStart),
            RegexAst::Conditional {
                condition: crate::ast::ConditionalTest::RelativeGroupExists(1),
                true_branch: Box::new(RegexAst::Char('a')),
                false_branch: Some(Box::new(RegexAst::Char('b'))),
            },
            RegexAst::Group {
                expr: Box::new(RegexAst::Char('a')),
                kind: GroupKind::Capturing,
                index: None,
                name: None,
            },
            RegexAst::Anchor(crate::ast::AnchorType::AbsEndNoNL),
        ]);

        let forward = Regex::from_ast(forward)
            .expect("Failed to compile forward relative-conditional AST directly");
        assert!(forward.is_match("ba"));
        assert!(!forward.is_match("aa"));
    }

    #[test]
    fn parser_positive_lookahead_syntax() {
        let regex =
            Regex::compile("(?=cat)c").expect("Failed to compile parser-path lookahead syntax");
        let m = regex
            .find_first("xxcat")
            .expect("Expected parser-path lookahead match");
        assert_eq!(m.start, 2);
        assert_eq!(m.end, 3);
    }
    #[test]
    fn parser_negative_lookahead_syntax() {
        let regex =
            Regex::compile("(?!cat)c").expect("Failed to compile parser-path lookahead syntax");
        assert!(regex.is_match("car"));
        assert!(!regex.is_match("cat"));
    }

    #[test]
    fn parser_positive_lookbehind_syntax() {
        let regex =
            Regex::compile("(?<=x)a").expect("Failed to compile parser-path lookbehind syntax");
        assert!(regex.is_match("xa"));
        assert!(!regex.is_match("ba"));
    }

    #[test]
    fn parser_negative_lookbehind_syntax() {
        let regex =
            Regex::compile("(?<!x)a").expect("Failed to compile parser-path lookbehind syntax");
        assert!(regex.is_match("ba"));
        assert!(!regex.is_match("xa"));
    }

    #[test]
    fn parser_numeric_backreference_matches_previous_capture() {
        let regex = Regex::compile(r"(a)\1").expect("Failed to compile numeric backreference");
        let m = regex
            .find_first("baa")
            .expect("Expected numeric backreference match");
        assert_eq!(m.start, 1);
        assert_eq!(m.end, 3);
        assert!(!regex.is_match("bab"));
    }

    #[test]
    fn parser_numeric_backreference_restores_captures_under_backtracking() {
        let regex =
            Regex::compile(r"(a|ab)\1").expect("Failed to compile backtracking backreference");
        let m = regex
            .find_first("abab")
            .expect("Expected backreference match after alternation backtracking");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 4);
    }

    #[test]
    fn parser_numeric_backreference_inside_lookahead_uses_existing_capture() {
        let regex = Regex::compile(r"(ab)(?=\1)\1")
            .expect("Failed to compile lookahead backreference pattern");
        let m = regex
            .find_first("zababx")
            .expect("Expected lookahead backreference match");
        assert_eq!(m.start, 1);
        assert_eq!(m.end, 5);
        assert!(!regex.is_match("zabacx"));
    }

    #[test]
    fn parser_negated_shorthand_character_classes() {
        let first_cases = [
            (r"\D+", "123abc45", (3, 6)),
            (r"\W+", "ab!!cd", (2, 4)),
            (r"\S+", "  ab  ", (2, 4)),
        ];

        for (pattern, input, expected_span) in first_cases {
            let regex = Regex::compile(pattern)
                .unwrap_or_else(|e| panic!("failed to compile {pattern}: {e}"));
            let m = regex
                .find_first(input)
                .unwrap_or_else(|| panic!("expected first match for pattern {pattern}"));
            assert_eq!((m.start, m.end), expected_span);
        }

        let all_cases = [
            (r"\D+", "123abc45!!", vec![(3, 6), (8, 10)]),
            (r"\W+", "ab!!cd??", vec![(2, 4), (6, 8)]),
            (r"\S+", "  ab\tcd  ", vec![(2, 4), (5, 7)]),
        ];

        for (pattern, input, expected_spans) in all_cases {
            let regex = Regex::compile(pattern)
                .unwrap_or_else(|e| panic!("failed to compile {pattern}: {e}"));
            let spans: Vec<(usize, usize)> = regex
                .find_all(input)
                .into_iter()
                .map(|m| (m.start, m.end))
                .collect();
            assert_eq!(spans, expected_spans);
        }
    }

    #[test]
    fn parser_negated_shorthand_character_classes_no_match() {
        let cases = [(r"\D+", "123"), (r"\W+", "abc_123"), (r"\S+", " \t\n")];

        for (pattern, input) in cases {
            let regex = Regex::compile(pattern)
                .unwrap_or_else(|e| panic!("failed to compile {pattern}: {e}"));
            assert!(
                !regex.is_match(input),
                "unexpected match for pattern {pattern}"
            );
            assert!(
                regex.find_first(input).is_none(),
                "unexpected first match for pattern {pattern}"
            );
            assert!(
                regex.find_all(input).is_empty(),
                "unexpected find_all matches for pattern {pattern}"
            );
        }
    }

    #[test]
    fn parser_end_anchor_suffix_match() {
        let regex = Regex::compile("dog$").expect("Failed to compile end-anchor syntax");
        let m = regex
            .find_first("cat dog")
            .expect("Expected suffix match for end-anchor pattern");
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert!(!regex.is_match("cat dog x"));
    }

    #[test]
    fn parser_end_anchor_find_all_only_terminal_match() {
        let regex = Regex::compile("dog$").expect("Failed to compile end-anchor syntax");
        let matches = regex.find_all("dog xx dog");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 7);
        assert_eq!(matches[0].end, 10);
    }

    #[test]
    fn parser_absolute_start_anchor_matches_only_at_text_start() {
        let regex =
            Regex::compile(r"\Acat").expect("Failed to compile absolute-start anchor syntax");
        let m = regex
            .find_first("cat dog")
            .expect("Expected absolute-start anchor match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
        assert!(!regex.is_match("xxcat"));
    }

    #[test]
    fn parser_absolute_end_anchor_requires_true_end_of_text() {
        let regex = Regex::compile(r"dog\z").expect("Failed to compile absolute-end anchor syntax");
        let m = regex
            .find_first("cat dog")
            .expect("Expected absolute-end anchor match");
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert!(!regex.is_match("cat dog\n"));
    }

    #[test]
    fn parser_absolute_end_or_newline_anchor_allows_one_final_newline() {
        let regex =
            Regex::compile(r"dog\Z").expect("Failed to compile end-or-final-newline anchor syntax");
        let m = regex
            .find_first("cat dog\n")
            .expect("Expected end-or-final-newline anchor match");
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert!(regex.is_match("cat dog"));
        assert!(!regex.is_match("cat dog\nx"));
    }

    #[test]
    fn parser_range_quantifier_scans_to_earliest_valid_span() {
        let regex = Regex::compile(r"\d{2,3}").expect("Failed to compile range-quantifier syntax");

        let first = regex
            .find_first("x1y22z333")
            .expect("Expected first range-quantifier match");
        assert_eq!(first.start, 3);
        assert_eq!(first.end, 5);

        let all = regex.find_all("x1 y22 z333 w4444");
        let spans: Vec<(usize, usize)> = all.into_iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(4, 6), (8, 11), (13, 16)]);
    }

    #[test]
    fn parser_range_quantifier_backtracks_when_followed_by_literal() {
        let regex =
            Regex::compile(r"\d{2,3}3").expect("Failed to compile range-quantifier suffix pattern");
        let m = regex
            .find_first("123")
            .expect("Expected bounded-range backtracking match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
    }

    #[test]
    fn parser_range_quantifier_suffix_prefers_longest_valid_span() {
        let regex =
            Regex::compile(r"\d{2,3}3").expect("Failed to compile range-quantifier suffix pattern");
        let m = regex
            .find_first("2233")
            .expect("Expected bounded-range greedy suffix match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 4);
    }

    #[test]
    fn parser_range_quantifier_suffix_find_all_spans() {
        let regex =
            Regex::compile(r"\d{2,3}3").expect("Failed to compile range-quantifier suffix pattern");
        let all = regex.find_all("123 2233 993 4443");
        let spans: Vec<(usize, usize)> = all.into_iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(0, 3), (4, 8), (9, 12), (13, 17)]);
    }

    #[test]
    fn parser_unbounded_range_quantifier_scans_to_earliest_valid_span() {
        let regex =
            Regex::compile(r"\d{2,}").expect("Failed to compile unbounded range-quantifier syntax");
        let first = regex
            .find_first("x1y22z333")
            .expect("Expected first unbounded range-quantifier match");
        assert_eq!(first.start, 3);
        assert_eq!(first.end, 5);

        let all = regex.find_all("x1 y22 z333 w4444");
        let spans: Vec<(usize, usize)> = all.into_iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(4, 6), (8, 11), (13, 17)]);
    }

    #[test]
    fn parser_unbounded_range_quantifier_suffix_backtracks_and_prefers_longest() {
        let regex = Regex::compile(r"\d{2,}3")
            .expect("Failed to compile unbounded range-quantifier suffix pattern");

        let backtrack = regex
            .find_first("123")
            .expect("Expected unbounded-range suffix backtracking match");
        assert_eq!(backtrack.start, 0);
        assert_eq!(backtrack.end, 3);

        let greedy = regex
            .find_first("2233")
            .expect("Expected unbounded-range greedy suffix match");
        assert_eq!(greedy.start, 0);
        assert_eq!(greedy.end, 4);
    }

    #[test]
    fn parser_unbounded_range_quantifier_suffix_find_all_spans() {
        let regex = Regex::compile(r"\d{2,}3")
            .expect("Failed to compile unbounded range-quantifier suffix pattern");
        let all = regex.find_all("123 2233 993 4443");
        let spans: Vec<(usize, usize)> = all.into_iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(0, 3), (4, 8), (9, 12), (13, 17)]);
    }

    #[test]
    fn parser_star_quantifier_backtracks_for_suffix() {
        let regex =
            Regex::compile("a*a").expect("Failed to compile star-quantifier suffix pattern");
        let m = regex
            .find_first("a")
            .expect("Expected star-quantifier suffix backtracking match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 1);
    }

    #[test]
    fn parser_plus_quantifier_backtracks_for_suffix() {
        let regex =
            Regex::compile("a+a").expect("Failed to compile plus-quantifier suffix pattern");
        let m = regex
            .find_first("aa")
            .expect("Expected plus-quantifier suffix backtracking match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 2);
    }

    #[test]
    fn parser_question_quantifier_backtracks_for_suffix() {
        let regex =
            Regex::compile("ab?b").expect("Failed to compile question-quantifier suffix pattern");
        let m = regex
            .find_first("ab")
            .expect("Expected question-quantifier suffix backtracking match");
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 2);
    }

    #[test]
    fn parser_lazy_question_quantifier_prefers_zero_width_match() {
        let regex =
            Regex::compile("a??").expect("Failed to compile lazy question-quantifier pattern");

        let first = regex
            .find_first("b")
            .expect("Expected zero-width lazy question match");
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 0);

        let all = regex.find_all("b");
        let spans: Vec<(usize, usize)> = all.into_iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(0, 0), (1, 1)]);
    }

    #[test]
    fn parser_lazy_star_quantifier_prefers_shortest_valid_span() {
        let regex = Regex::compile("ab*?").expect("Failed to compile lazy star-quantifier pattern");
        let first = regex.find_first("abbb").expect("Expected lazy star match");
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 1);
    }

    #[test]
    fn parser_lazy_star_and_plus_quantifiers_backtrack_for_suffix() {
        let star = Regex::compile("ab*?c").expect("Failed to compile lazy star suffix pattern");
        let plus = Regex::compile("ab+?c").expect("Failed to compile lazy plus suffix pattern");

        let star_match = star
            .find_first("abbbc")
            .expect("Expected lazy star suffix backtracking match");
        assert_eq!(star_match.start, 0);
        assert_eq!(star_match.end, 5);

        let plus_match = plus
            .find_first("abbbc")
            .expect("Expected lazy plus suffix backtracking match");
        assert_eq!(plus_match.start, 0);
        assert_eq!(plus_match.end, 5);
    }

    #[test]
    fn parser_lazy_bounded_and_unbounded_ranges_prefer_shortest_suffix_match() {
        let bounded = Regex::compile(r"\d{2,3}?3")
            .expect("Failed to compile lazy bounded range-quantifier suffix pattern");
        let unbounded = Regex::compile(r"\d{2,}?3")
            .expect("Failed to compile lazy unbounded range-quantifier suffix pattern");

        let bounded_match = bounded
            .find_first("2233")
            .expect("Expected lazy bounded-range suffix match");
        assert_eq!(bounded_match.start, 0);
        assert_eq!(bounded_match.end, 3);

        let unbounded_match = unbounded
            .find_first("2233")
            .expect("Expected lazy unbounded-range suffix match");
        assert_eq!(unbounded_match.start, 0);
        assert_eq!(unbounded_match.end, 3);
    }

    #[test]
    fn parser_possessive_quantifiers_block_backtracking_for_suffix() {
        let star =
            Regex::compile(r"\Aa*+a\z").expect("Failed to compile possessive star suffix pattern");
        let plus =
            Regex::compile(r"\Aa++a\z").expect("Failed to compile possessive plus suffix pattern");
        let question = Regex::compile(r"\Aa?+a\z")
            .expect("Failed to compile possessive question suffix pattern");
        let range = Regex::compile(r"\A\d{2,3}+3\z")
            .expect("Failed to compile possessive bounded-range suffix pattern");

        assert!(!star.is_match("aaaa"));
        assert!(!plus.is_match("aaaa"));
        assert!(!question.is_match("a"));
        assert!(!range.is_match("123"));

        let greedy_star = Regex::compile(r"\Aa*a\z")
            .expect("Failed to compile greedy star suffix control pattern");
        let greedy_plus = Regex::compile(r"\Aa+a\z")
            .expect("Failed to compile greedy plus suffix control pattern");
        let greedy_question = Regex::compile(r"\Aa?a\z")
            .expect("Failed to compile greedy question suffix control pattern");
        let greedy_range = Regex::compile(r"\A\d{2,3}3\z")
            .expect("Failed to compile greedy bounded-range suffix control pattern");

        assert!(greedy_star.is_match("aaaa"));
        assert!(greedy_plus.is_match("aaaa"));
        assert!(greedy_question.is_match("a"));
        assert!(greedy_range.is_match("123"));
    }

    #[test]
    fn parser_possessive_quantifiers_match_when_no_backtracking_is_needed() {
        let star =
            Regex::compile(r"\Aa*+b\z").expect("Failed to compile possessive star success pattern");
        let plus =
            Regex::compile(r"\Aa++b\z").expect("Failed to compile possessive plus success pattern");
        let question = Regex::compile(r"\Aa?+b\z")
            .expect("Failed to compile possessive question success pattern");
        let range = Regex::compile(r"\Aa{2,3}+b\z")
            .expect("Failed to compile possessive bounded-range success pattern");

        assert!(star.is_match("aaab"));
        assert!(plus.is_match("aaab"));
        assert!(question.is_match("ab"));
        assert!(range.is_match("aaab"));
    }

    #[test]
    fn parser_atomic_group_blocks_backtracking() {
        let atomic = Regex::compile("(?>a|ab)c").expect("Failed to compile atomic-group pattern");
        let non_atomic = Regex::compile("(a|ab)c").expect("Failed to compile non-atomic pattern");

        assert!(!atomic.is_match("abc"));
        assert!(non_atomic.is_match("abc"));
    }

    #[test]
    fn parser_atomic_group_can_match_first_branch_without_backtrack() {
        let regex = Regex::compile("(?>ab|a)c").expect("Failed to compile atomic-group pattern");
        assert!(regex.is_match("abc"));
    }

    #[test]
    fn parser_code_block_syntax_requires_non_pure_mode() {
        let result = Regex::compile("(?{lua:return true})");
        assert!(result.is_err(), "Code block should not silently compile");
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("code blocks require ExecutionMode::Safe or ExecutionMode::Full"));
    }

    #[test]
    fn safe_mode_native_code_blocks_require_full_mode() {
        let result = Regex::with_mode("(?{native:validate})", ExecutionMode::Safe);
        assert!(
            result.is_err(),
            "Native code block should require Full mode"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("native code blocks require ExecutionMode::Full"));
    }

    #[test]
    fn full_mode_native_code_block_can_use_registered_callback() {
        let regex = Regex::with_mode(
            r"(?<word>cat)(?{native:validate_word})",
            ExecutionMode::Full,
        )
        .expect("Failed to compile native code block pattern");
        regex
            .register_native("validate_word", |ctx| {
                if ctx.current_match() == Some("cat")
                    && ctx.group(1) == Some("cat")
                    && ctx.named("word") == Some("cat")
                {
                    ExecResult::Success
                } else {
                    ExecResult::Failure
                }
            })
            .expect("Failed to register native callback");
        assert!(regex.is_match("cat"));
        assert!(!regex.is_match("dog"));
    }

    #[test]
    fn full_mode_native_code_block_can_access_host_variables() {
        let regex = Regex::with_mode(r"(?{native:check_env})", ExecutionMode::Full)
            .expect("Failed to compile native variable pattern");
        regex
            .set_variable("env", "prod")
            .expect("Failed to register execution variable");
        regex
            .register_native("check_env", |ctx| {
                if ctx.variable("env").as_deref() == Some("prod") {
                    ExecResult::Success
                } else {
                    ExecResult::Failure
                }
            })
            .expect("Failed to register native callback");
        assert!(regex.is_match(""));
        regex
            .set_variable("env", "dev")
            .expect("Failed to replace execution variable");
        assert!(!regex.is_match(""));
    }

    #[test]
    fn full_mode_native_code_block_can_access_match_metadata() {
        let regex = Regex::with_mode(
            r"foo|cat(?{native:check_match_metadata})",
            ExecutionMode::Full,
        )
        .expect("Failed to compile native match-metadata pattern");
        regex
            .register_native("check_match_metadata", |ctx| {
                if ctx.current_match() == Some("cat")
                    && ctx.match_start() == 2
                    && ctx.match_end() == 5
                    && ctx.match_length() == 3
                    && ctx.matched_branch_number() == Some(2)
                {
                    ExecResult::Success
                } else {
                    ExecResult::Failure
                }
            })
            .expect("Failed to register native callback");

        let first = regex
            .find_first("xxcat")
            .expect("Expected native match-metadata match");
        assert_eq!((first.start, first.end), (2, 5));
        assert_eq!(first.matched_branch_number, Some(2));
    }

    #[test]
    fn full_mode_native_code_block_find_all_surfaces_replacement_results() {
        let regex = Regex::with_mode(r"(?<ch>.)(?{native:emit_char})", ExecutionMode::Full)
            .expect("Failed to compile native richer-result pattern");
        regex
            .register_native("emit_char", |ctx| {
                ExecResult::Replacement(ctx.named("ch").unwrap_or_default().to_string())
            })
            .expect("Failed to register native callback");

        let matches = regex.find_all("ab");
        let spans = matches.iter().map(|m| (m.start, m.end)).collect::<Vec<_>>();
        let code_results = matches
            .into_iter()
            .map(|m| m.code_result)
            .collect::<Vec<_>>();

        assert_eq!(spans, vec![(0, 1), (1, 2)]);
        assert_eq!(
            code_results,
            vec![
                Some(CodeBlockValue::Replacement("a".to_string())),
                Some(CodeBlockValue::Replacement("b".to_string())),
            ]
        );
    }

    #[test]
    fn full_mode_native_replace_with_code_uses_replacement_payloads() {
        let regex = Regex::with_mode(r"(?<word>cat)(?{native:emit_upper})", ExecutionMode::Full)
            .expect("Failed to compile native replacement pattern");
        regex
            .register_native("emit_upper", |ctx| {
                ExecResult::Replacement(ctx.named("word").unwrap_or_default().to_uppercase())
            })
            .expect("Failed to register native callback");

        assert_eq!(regex.replace_first_with_code("cat dog cat"), "CAT dog cat");
        assert_eq!(regex.replace_all_with_code("cat dog cat"), "CAT dog CAT");
    }

    #[test]
    fn full_mode_native_replace_with_code_preserves_original_match_without_replacement() {
        let regex = Regex::with_mode(r"cat(?{native:emit_numeric})", ExecutionMode::Full)
            .expect("Failed to compile native numeric replacement-fallback pattern");
        regex
            .register_native("emit_numeric", |_| ExecResult::Numeric(7.0))
            .expect("Failed to register native callback");

        assert_eq!(regex.replace_first_with_code("cat dog"), "cat dog");
        assert_eq!(regex.replace_all_with_code("cat dog cat"), "cat dog cat");
    }

    #[test]
    fn full_mode_native_replace_first_with_code_uses_winning_path_replacement() {
        let regex = Regex::with_mode(r"a*(?{native:emit_path})a", ExecutionMode::Full)
            .expect("Failed to compile native backtracking replacement pattern");
        regex
            .register_native("emit_path", |ctx| {
                let replacement = if ctx.current_match() == Some("") {
                    "EMPTY"
                } else {
                    "NONEMPTY"
                };
                ExecResult::Replacement(replacement.to_string())
            })
            .expect("Failed to register native callback");

        assert_eq!(regex.replace_first_with_code("a"), "EMPTY");
    }

    #[test]
    fn full_mode_native_find_all_numeric_with_code_collects_numeric_payloads() {
        let regex = Regex::with_mode(r"(?<digit>\d)(?{native:emit_digit})", ExecutionMode::Full)
            .expect("Failed to compile native numeric collection pattern");
        regex
            .register_native("emit_digit", |ctx| {
                let value = ctx
                    .named("digit")
                    .and_then(|digit| digit.parse::<f64>().ok())
                    .unwrap_or_default();
                ExecResult::Numeric(value)
            })
            .expect("Failed to register native callback");

        assert_eq!(regex.find_first_numeric_with_code("7a8"), Some(7.0));
        assert_eq!(regex.find_all_numeric_with_code("7a8"), vec![7.0, 8.0]);
    }

    #[test]
    fn full_mode_native_numeric_helpers_skip_non_numeric_payloads() {
        let regex = Regex::with_mode(r"(?<ch>.)(?{native:emit_mixed})", ExecutionMode::Full)
            .expect("Failed to compile native mixed-payload pattern");
        regex
            .register_native("emit_mixed", |ctx| match ctx.named("ch") {
                Some("1") => ExecResult::Numeric(1.0),
                Some("2") => ExecResult::Numeric(2.0),
                Some(other) => ExecResult::Replacement(other.to_uppercase()),
                None => ExecResult::Success,
            })
            .expect("Failed to register native callback");

        assert_eq!(regex.find_first_numeric_with_code("a1b2"), Some(1.0));
        assert_eq!(regex.find_all_numeric_with_code("a1b2"), vec![1.0, 2.0]);
        assert_eq!(regex.find_first_numeric_with_code("ab"), None);
        assert!(regex.find_all_numeric_with_code("ab").is_empty());
    }

    #[test]
    fn full_mode_native_find_first_numeric_with_code_uses_winning_path_numeric() {
        let regex = Regex::with_mode(r"a*(?{native:emit_len})a", ExecutionMode::Full)
            .expect("Failed to compile native backtracking numeric pattern");
        regex
            .register_native("emit_len", |ctx| {
                let match_len = u32::try_from(ctx.current_match().unwrap_or_default().len())
                    .expect("test match length fits in u32");
                ExecResult::Numeric(f64::from(match_len))
            })
            .expect("Failed to register native callback");

        assert_eq!(regex.find_first_numeric_with_code("a"), Some(0.0));
    }

    #[test]
    fn full_mode_native_code_block_fails_when_callback_is_missing() {
        let regex = Regex::with_mode("(?{native:missing})", ExecutionMode::Full)
            .expect("Failed to compile native code block pattern");
        assert!(!regex.is_match(""));
    }

    #[test]
    fn register_native_requires_attached_execution_manager() {
        let regex = Regex::with_mode("cat", ExecutionMode::Full).expect("Failed to compile regex");
        let result = regex.register_native("noop", |_| ExecResult::Success);
        assert!(
            result.is_err(),
            "Registration should fail without runtime manager"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("native callback registration is unavailable for this compiled regex"));
    }

    #[test]
    fn set_variable_requires_attached_execution_manager() {
        let regex = Regex::with_mode("cat", ExecutionMode::Full).expect("Failed to compile regex");
        let result = regex.set_variable("env", "prod");
        assert!(
            result.is_err(),
            "Variable registration should fail without runtime manager"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("execution variable registration is unavailable for this compiled regex")
        );
    }

    #[cfg(not(feature = "wasm"))]
    #[test]
    fn safe_mode_wasm_code_blocks_require_wasm_feature() {
        let result = Regex::with_mode("(?{wasm:module:function})", ExecutionMode::Safe);
        assert!(
            result.is_err(),
            "WASM code block should require the wasm feature"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("wasm code blocks require the `wasm` cargo feature"));
    }

    #[cfg(feature = "wasm")]
    fn test_wasm_module_bytes(source: &str) -> Vec<u8> {
        wat::parse_str(source).expect("Failed to assemble WAT test module")
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_use_registered_module() {
        let regex = Regex::with_mode("(?{wasm:truthy:evaluate})", ExecutionMode::Safe)
            .expect("Failed to compile WASM code block pattern");
        regex
            .register_wasm_module(
                "truthy",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (func (export "evaluate") (result i32)
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM module");
        assert!(regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_defaults_to_no_non_boolean_result_payload() {
        let regex = Regex::with_mode("(?{wasm:truthy:evaluate})", ExecutionMode::Safe)
            .expect("Failed to compile WASM payload pattern");
        regex
            .register_wasm_module(
                "truthy",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (func (export "evaluate") (result i32)
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM module");

        let first = regex.find_first("").expect("Expected WASM predicate match");
        assert_eq!(first.code_result, None);
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_emit_numeric_results() {
        let regex = Regex::with_mode(r"a(?{wasm:calc:emit_one_point_five})", ExecutionMode::Safe)
            .expect("Failed to compile WASM numeric-result pattern");
        regex
            .register_wasm_module(
                "calc",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "emit_numeric" (func $emit_numeric (param f64)))
                        (func (export "emit_one_point_five") (result i32)
                            f64.const 1.5
                            call $emit_numeric
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM numeric-result module");

        let first = regex
            .find_first("aa")
            .expect("Expected WASM numeric-result match");
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 1);
        assert_eq!(first.code_result, Some(CodeBlockValue::Numeric(1.5)));
        assert_eq!(regex.find_first_numeric_with_code("aa"), Some(1.5));
        assert_eq!(regex.find_all_numeric_with_code("aa"), vec![1.5, 1.5]);
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_last_emitted_result_wins() {
        let regex = Regex::with_mode("(?{wasm:calc:emit_multiple})", ExecutionMode::Safe)
            .expect("Failed to compile WASM multi-result pattern");
        regex
            .register_wasm_module(
                "calc",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "emit_numeric" (func $emit_numeric (param f64)))
                        (func (export "emit_multiple") (result i32)
                            f64.const 1.0
                            call $emit_numeric
                            f64.const 2.5
                            call $emit_numeric
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM multi-result module");

        let first = regex
            .find_first("")
            .expect("Expected WASM multi-result match");
        assert_eq!(first.code_result, Some(CodeBlockValue::Numeric(2.5)));
        assert_eq!(regex.find_first_numeric_with_code(""), Some(2.5));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_emit_replacement_results() {
        let regex = Regex::with_mode("cat(?{wasm:emit:cat_upper})", ExecutionMode::Safe)
            .expect("Failed to compile WASM replacement pattern");
        regex
            .register_wasm_module(
                "emit",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "emit_replacement" (func $emit_replacement (param i32 i32)))
                        (memory (export "memory") 1)
                        (data (i32.const 0) "CAT")
                        (func (export "cat_upper") (result i32)
                            i32.const 0
                            i32.const 3
                            call $emit_replacement
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM replacement module");

        let first = regex
            .find_first("cat dog cat")
            .expect("Expected WASM replacement-result match");
        assert_eq!(
            first.code_result,
            Some(CodeBlockValue::Replacement("CAT".to_string()))
        );
        assert_eq!(regex.replace_first_with_code("cat dog cat"), "CAT dog cat");
        assert_eq!(regex.replace_all_with_code("cat dog cat"), "CAT dog CAT");
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_fails_for_invalid_utf8_replacement_payload() {
        let regex = Regex::with_mode("(?{wasm:emit:invalid_utf8})", ExecutionMode::Safe)
            .expect("Failed to compile WASM invalid UTF-8 pattern");
        regex
            .register_wasm_module(
                "emit",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "emit_replacement" (func $emit_replacement (param i32 i32)))
                        (memory (export "memory") 1)
                        (data (i32.const 0) "\ff")
                        (func (export "invalid_utf8") (result i32)
                            i32.const 0
                            i32.const 1
                            call $emit_replacement
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM invalid UTF-8 module");

        assert!(!regex.is_match(""));
        assert_eq!(regex.find_first(""), None);
        assert_eq!(regex.replace_first_with_code(""), "");
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_discards_emitted_result_on_failure() {
        let regex = Regex::with_mode("(?{wasm:calc:emit_then_fail})", ExecutionMode::Safe)
            .expect("Failed to compile WASM failed-result pattern");
        regex
            .register_wasm_module(
                "calc",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "emit_numeric" (func $emit_numeric (param f64)))
                        (func (export "emit_then_fail") (result i32)
                            f64.const 9.0
                            call $emit_numeric
                            i32.const 0
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM failed-result module");

        assert!(!regex.is_match(""));
        assert_eq!(regex.find_first(""), None);
        assert_eq!(regex.find_first_numeric_with_code(""), None);
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_read_variables() {
        let regex = Regex::with_mode("(?{wasm:ctx:variables_are_sorted})", ExecutionMode::Safe)
            .expect("Failed to compile WASM variable pattern");
        regex
            .set_variable("zeta", "dog")
            .expect("Failed to register execution variable");
        regex
            .set_variable("alpha", "cat")
            .expect("Failed to register execution variable");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "variable_count" (func $variable_count (result i32)))
                        (import "rgx" "variable_name_length" (func $variable_name_length (param i32) (result i32)))
                        (import "rgx" "variable_name_read" (func $variable_name_read (param i32 i32 i32 i32) (result i32)))
                        (import "rgx" "variable_value_length" (func $variable_value_length (param i32) (result i32)))
                        (import "rgx" "variable_value_read" (func $variable_value_read (param i32 i32 i32 i32) (result i32)))
                        (memory (export "memory") 1)
                        (func (export "variables_are_sorted") (result i32)
                            (local $copied i32)
                            call $variable_count
                            i32.const 2
                            i32.ne
                            if (result i32)
                                i32.const 0
                            else
                                i32.const 0
                                call $variable_name_length
                                i32.const 5
                                i32.ne
                                if (result i32)
                                    i32.const 0
                                else
                                    i32.const 0
                                    i32.const 0
                                    i32.const 0
                                    i32.const 5
                                    call $variable_name_read
                                    local.tee $copied
                                    i32.const 5
                                    i32.ne
                                    if (result i32)
                                        i32.const 0
                                    else
                                        i32.const 0
                                        call $variable_value_length
                                        i32.const 3
                                        i32.ne
                                        if (result i32)
                                            i32.const 0
                                        else
                                            i32.const 0
                                            i32.const 16
                                            i32.const 0
                                            i32.const 3
                                            call $variable_value_read
                                            local.tee $copied
                                            i32.const 3
                                            i32.ne
                                            if (result i32)
                                                i32.const 0
                                            else
                                                i32.const 1
                                                call $variable_name_length
                                                i32.const 4
                                                i32.ne
                                                if (result i32)
                                                    i32.const 0
                                                else
                                                    i32.const 1
                                                    i32.const 32
                                                    i32.const 0
                                                    i32.const 4
                                                    call $variable_name_read
                                                    local.tee $copied
                                                    i32.const 4
                                                    i32.ne
                                                    if (result i32)
                                                        i32.const 0
                                                    else
                                                        i32.const 1
                                                        call $variable_value_length
                                                        i32.const 3
                                                        i32.ne
                                                        if (result i32)
                                                            i32.const 0
                                                        else
                                                            i32.const 1
                                                            i32.const 48
                                                            i32.const 0
                                                            i32.const 3
                                                            call $variable_value_read
                                                            local.tee $copied
                                                            i32.const 3
                                                            i32.ne
                                                            if (result i32)
                                                                i32.const 0
                                                            else
                                                                i32.const 0
                                                                i32.load8_u
                                                                i32.const 97
                                                                i32.eq
                                                                i32.const 1
                                                                i32.load8_u
                                                                i32.const 108
                                                                i32.eq
                                                                i32.and
                                                                i32.const 2
                                                                i32.load8_u
                                                                i32.const 112
                                                                i32.eq
                                                                i32.and
                                                                i32.const 3
                                                                i32.load8_u
                                                                i32.const 104
                                                                i32.eq
                                                                i32.and
                                                                i32.const 4
                                                                i32.load8_u
                                                                i32.const 97
                                                                i32.eq
                                                                i32.and
                                                                i32.const 16
                                                                i32.load8_u
                                                                i32.const 99
                                                                i32.eq
                                                                i32.and
                                                                i32.const 17
                                                                i32.load8_u
                                                                i32.const 97
                                                                i32.eq
                                                                i32.and
                                                                i32.const 18
                                                                i32.load8_u
                                                                i32.const 116
                                                                i32.eq
                                                                i32.and
                                                                i32.const 32
                                                                i32.load8_u
                                                                i32.const 122
                                                                i32.eq
                                                                i32.and
                                                                i32.const 33
                                                                i32.load8_u
                                                                i32.const 101
                                                                i32.eq
                                                                i32.and
                                                                i32.const 34
                                                                i32.load8_u
                                                                i32.const 116
                                                                i32.eq
                                                                i32.and
                                                                i32.const 35
                                                                i32.load8_u
                                                                i32.const 97
                                                                i32.eq
                                                                i32.and
                                                                i32.const 48
                                                                i32.load8_u
                                                                i32.const 100
                                                                i32.eq
                                                                i32.and
                                                                i32.const 49
                                                                i32.load8_u
                                                                i32.const 111
                                                                i32.eq
                                                                i32.and
                                                                i32.const 50
                                                                i32.load8_u
                                                                i32.const 103
                                                                i32.eq
                                                                i32.and
                                                            end
                                                        end
                                                    end
                                                end
                                            end
                                        end
                                    end
                                end
                            end
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM variable module");
        assert!(regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_reports_missing_variable_slots() {
        let regex = Regex::with_mode(
            r#"(?{wasm:ctx:missing_variable_slot_is_unavailable})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile WASM missing-variable pattern");
        regex
            .set_variable("env", "prod")
            .expect("Failed to register execution variable");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "variable_count" (func $variable_count (result i32)))
                        (import "rgx" "variable_name_length" (func $variable_name_length (param i32) (result i32)))
                        (import "rgx" "variable_value_length" (func $variable_value_length (param i32) (result i32)))
                        (func (export "missing_variable_slot_is_unavailable") (result i32)
                            call $variable_count
                            i32.const 1
                            i32.eq
                            i32.const 1
                            call $variable_name_length
                            i32.const -1
                            i32.eq
                            i32.and
                            i32.const 1
                            call $variable_value_length
                            i32.const -1
                            i32.eq
                            i32.and
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM missing-variable module");
        assert!(regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_read_current_position() {
        let regex = Regex::with_mode("a(?{wasm:ctx:position_is_one})", ExecutionMode::Safe)
            .expect("Failed to compile WASM position pattern");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "position" (func $position (result i32)))
                        (func (export "position_is_one") (result i32)
                            call $position
                            i32.const 1
                            i32.eq
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM position module");
        assert!(regex.is_match("a"));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_read_match_metadata() {
        let regex = Regex::with_mode(
            "foo|cat(?{wasm:ctx:match_metadata_is_visible})",
            ExecutionMode::Safe,
        )
        .expect("Failed to compile WASM match-metadata pattern");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "position" (func $position (result i32)))
                        (import "rgx" "match_start" (func $match_start (result i32)))
                        (import "rgx" "match_end" (func $match_end (result i32)))
                        (import "rgx" "match_length" (func $match_length (result i32)))
                        (import "rgx" "branch_number" (func $branch_number (result i32)))
                        (func (export "match_metadata_is_visible") (result i32)
                            call $position
                            i32.const 5
                            i32.eq
                            call $match_start
                            i32.const 2
                            i32.eq
                            i32.and
                            call $match_end
                            i32.const 5
                            i32.eq
                            i32.and
                            call $match_length
                            i32.const 3
                            i32.eq
                            i32.and
                            call $branch_number
                            i32.const 2
                            i32.eq
                            i32.and
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM match-metadata module");
        let first = regex
            .find_first("xxcat")
            .expect("Expected WASM match-metadata match");
        assert_eq!((first.start, first.end), (2, 5));
        assert_eq!(first.matched_branch_number, Some(2));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_reports_missing_branch_number() {
        let regex = Regex::with_mode(
            "cat(?{wasm:ctx:branch_number_is_unavailable})",
            ExecutionMode::Safe,
        )
        .expect("Failed to compile WASM missing-branch pattern");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "branch_number" (func $branch_number (result i32)))
                        (func (export "branch_number_is_unavailable") (result i32)
                            call $branch_number
                            i32.const -1
                            i32.eq
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM missing-branch module");
        assert!(regex.is_match("cat"));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_read_input_text() {
        let regex = Regex::with_mode("cat(?{wasm:ctx:input_is_cat_dog})", ExecutionMode::Safe)
            .expect("Failed to compile WASM text-read pattern");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "text_length" (func $text_length (result i32)))
                        (import "rgx" "text_read" (func $text_read (param i32 i32 i32) (result i32)))
                        (memory (export "memory") 1)
                        (func (export "input_is_cat_dog") (result i32)
                            (local $copied i32)
                            call $text_length
                            i32.const 7
                            i32.ne
                            if (result i32)
                                i32.const 0
                            else
                                i32.const 0
                                i32.const 0
                                i32.const 7
                                call $text_read
                                local.tee $copied
                                i32.const 7
                                i32.ne
                                if (result i32)
                                    i32.const 0
                                else
                                    i32.const 0
                                    i32.load8_u
                                    i32.const 99
                                    i32.eq
                                    i32.const 1
                                    i32.load8_u
                                    i32.const 97
                                    i32.eq
                                    i32.and
                                    i32.const 2
                                    i32.load8_u
                                    i32.const 116
                                    i32.eq
                                    i32.and
                                    i32.const 3
                                    i32.load8_u
                                    i32.const 32
                                    i32.eq
                                    i32.and
                                    i32.const 4
                                    i32.load8_u
                                    i32.const 100
                                    i32.eq
                                    i32.and
                                    i32.const 5
                                    i32.load8_u
                                    i32.const 111
                                    i32.eq
                                    i32.and
                                    i32.const 6
                                    i32.load8_u
                                    i32.const 103
                                    i32.eq
                                    i32.and
                                end
                            end
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM text-read module");
        assert!(regex.is_match("cat dog"));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_read_numbered_captures() {
        let regex = Regex::with_mode(
            r#"(?<word>cat)(?{wasm:ctx:capture_one_is_cat})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile WASM capture-read pattern");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "capture_count" (func $capture_count (result i32)))
                        (import "rgx" "capture_length" (func $capture_length (param i32) (result i32)))
                        (import "rgx" "capture_read" (func $capture_read (param i32 i32 i32 i32) (result i32)))
                        (memory (export "memory") 1)
                        (func (export "capture_one_is_cat") (result i32)
                            (local $copied i32)
                            call $capture_count
                            i32.const 2
                            i32.ne
                            if (result i32)
                                i32.const 0
                            else
                                i32.const 0
                                call $capture_length
                                i32.const 3
                                i32.ne
                                if (result i32)
                                    i32.const 0
                                else
                                    i32.const 1
                                    call $capture_length
                                    i32.const 3
                                    i32.ne
                                    if (result i32)
                                        i32.const 0
                                    else
                                        i32.const 1
                                        i32.const 0
                                        i32.const 0
                                        i32.const 3
                                        call $capture_read
                                        local.tee $copied
                                        i32.const 3
                                        i32.ne
                                        if (result i32)
                                            i32.const 0
                                        else
                                            i32.const 0
                                            i32.load8_u
                                            i32.const 99
                                            i32.eq
                                            i32.const 1
                                            i32.load8_u
                                            i32.const 97
                                            i32.eq
                                            i32.and
                                            i32.const 2
                                            i32.load8_u
                                            i32.const 116
                                            i32.eq
                                            i32.and
                                        end
                                    end
                                end
                            end
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM capture-read module");
        assert!(regex.is_match("cat"));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_can_read_named_captures() {
        let regex = Regex::with_mode(
            r#"(?<zeta>cat)(?<alpha>dog)(?{wasm:ctx:named_captures_are_sorted})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile WASM named-capture pattern");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "named_capture_count" (func $named_capture_count (result i32)))
                        (import "rgx" "named_capture_name_length" (func $named_capture_name_length (param i32) (result i32)))
                        (import "rgx" "named_capture_name_read" (func $named_capture_name_read (param i32 i32 i32 i32) (result i32)))
                        (import "rgx" "named_capture_value_length" (func $named_capture_value_length (param i32) (result i32)))
                        (import "rgx" "named_capture_value_read" (func $named_capture_value_read (param i32 i32 i32 i32) (result i32)))
                        (memory (export "memory") 1)
                        (func (export "named_captures_are_sorted") (result i32)
                            (local $copied i32)
                            call $named_capture_count
                            i32.const 2
                            i32.ne
                            if (result i32)
                                i32.const 0
                            else
                                i32.const 0
                                call $named_capture_name_length
                                i32.const 5
                                i32.ne
                                if (result i32)
                                    i32.const 0
                                else
                                    i32.const 0
                                    i32.const 0
                                    i32.const 0
                                    i32.const 5
                                    call $named_capture_name_read
                                    local.tee $copied
                                    i32.const 5
                                    i32.ne
                                    if (result i32)
                                        i32.const 0
                                    else
                                        i32.const 0
                                        call $named_capture_value_length
                                        i32.const 3
                                        i32.ne
                                        if (result i32)
                                            i32.const 0
                                        else
                                            i32.const 0
                                            i32.const 16
                                            i32.const 0
                                            i32.const 3
                                            call $named_capture_value_read
                                            local.tee $copied
                                            i32.const 3
                                            i32.ne
                                            if (result i32)
                                                i32.const 0
                                            else
                                                i32.const 1
                                                call $named_capture_name_length
                                                i32.const 4
                                                i32.ne
                                                if (result i32)
                                                    i32.const 0
                                                else
                                                    i32.const 1
                                                    i32.const 32
                                                    i32.const 0
                                                    i32.const 4
                                                    call $named_capture_name_read
                                                    local.tee $copied
                                                    i32.const 4
                                                    i32.ne
                                                    if (result i32)
                                                        i32.const 0
                                                    else
                                                        i32.const 1
                                                        call $named_capture_value_length
                                                        i32.const 3
                                                        i32.ne
                                                        if (result i32)
                                                            i32.const 0
                                                        else
                                                            i32.const 1
                                                            i32.const 48
                                                            i32.const 0
                                                            i32.const 3
                                                            call $named_capture_value_read
                                                            local.tee $copied
                                                            i32.const 3
                                                            i32.ne
                                                            if (result i32)
                                                                i32.const 0
                                                            else
                                                                i32.const 0
                                                                i32.load8_u
                                                                i32.const 97
                                                                i32.eq
                                                                i32.const 1
                                                                i32.load8_u
                                                                i32.const 108
                                                                i32.eq
                                                                i32.and
                                                                i32.const 2
                                                                i32.load8_u
                                                                i32.const 112
                                                                i32.eq
                                                                i32.and
                                                                i32.const 3
                                                                i32.load8_u
                                                                i32.const 104
                                                                i32.eq
                                                                i32.and
                                                                i32.const 4
                                                                i32.load8_u
                                                                i32.const 97
                                                                i32.eq
                                                                i32.and
                                                                i32.const 16
                                                                i32.load8_u
                                                                i32.const 100
                                                                i32.eq
                                                                i32.and
                                                                i32.const 17
                                                                i32.load8_u
                                                                i32.const 111
                                                                i32.eq
                                                                i32.and
                                                                i32.const 18
                                                                i32.load8_u
                                                                i32.const 103
                                                                i32.eq
                                                                i32.and
                                                                i32.const 32
                                                                i32.load8_u
                                                                i32.const 122
                                                                i32.eq
                                                                i32.and
                                                                i32.const 33
                                                                i32.load8_u
                                                                i32.const 101
                                                                i32.eq
                                                                i32.and
                                                                i32.const 34
                                                                i32.load8_u
                                                                i32.const 116
                                                                i32.eq
                                                                i32.and
                                                                i32.const 35
                                                                i32.load8_u
                                                                i32.const 97
                                                                i32.eq
                                                                i32.and
                                                                i32.const 48
                                                                i32.load8_u
                                                                i32.const 99
                                                                i32.eq
                                                                i32.and
                                                                i32.const 49
                                                                i32.load8_u
                                                                i32.const 97
                                                                i32.eq
                                                                i32.and
                                                                i32.const 50
                                                                i32.load8_u
                                                                i32.const 116
                                                                i32.eq
                                                                i32.and
                                                            end
                                                        end
                                                    end
                                                end
                                            end
                                        end
                                    end
                                end
                            end
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM named-capture module");
        assert!(regex.is_match("catdog"));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_reports_missing_named_capture_slots() {
        let regex = Regex::with_mode(
            r#"(?<word>cat)(?{wasm:ctx:missing_named_capture_slot_is_unavailable})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile WASM missing-named-capture pattern");
        regex
            .register_wasm_module(
                "ctx",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "named_capture_count" (func $named_capture_count (result i32)))
                        (import "rgx" "named_capture_name_length" (func $named_capture_name_length (param i32) (result i32)))
                        (import "rgx" "named_capture_value_length" (func $named_capture_value_length (param i32) (result i32)))
                        (func (export "missing_named_capture_slot_is_unavailable") (result i32)
                            call $named_capture_count
                            i32.const 1
                            i32.eq
                            i32.const 1
                            call $named_capture_name_length
                            i32.const -1
                            i32.eq
                            i32.and
                            i32.const 1
                            call $named_capture_value_length
                            i32.const -1
                            i32.eq
                            i32.and
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM missing-named-capture module");
        assert!(regex.is_match("cat"));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_fails_when_context_reads_require_exported_memory() {
        let regex = Regex::with_mode("(?{wasm:no_memory:evaluate})", ExecutionMode::Safe)
            .expect("Failed to compile WASM missing-memory pattern");
        regex
            .register_wasm_module(
                "no_memory",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "text_read" (func $text_read (param i32 i32 i32) (result i32)))
                        (func (export "evaluate") (result i32)
                            i32.const 0
                            i32.const 0
                            i32.const 1
                            call $text_read
                            drop
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM missing-memory module");
        assert!(!regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_fails_for_invalid_guest_memory_writes() {
        let regex = Regex::with_mode("(?{wasm:bad_write:evaluate})", ExecutionMode::Safe)
            .expect("Failed to compile WASM invalid-write pattern");
        regex
            .register_wasm_module(
                "bad_write",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "text_read" (func $text_read (param i32 i32 i32) (result i32)))
                        (memory (export "memory") 1)
                        (func (export "evaluate") (result i32)
                            i32.const 70000
                            i32.const 0
                            i32.const 1
                            call $text_read
                            drop
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM invalid-write module");
        assert!(!regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_fails_for_malformed_context_reads() {
        let regex = Regex::with_mode("(?{wasm:bad_context:evaluate})", ExecutionMode::Safe)
            .expect("Failed to compile WASM malformed-context pattern");
        regex
            .register_wasm_module(
                "bad_context",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (import "rgx" "capture_length" (func $capture_length (param i32) (result i32)))
                        (func (export "evaluate") (result i32)
                            i32.const -1
                            call $capture_length
                            drop
                            i32.const 1
                        )
                    )
                    "#,
                ),
            )
            .expect("Failed to register WASM malformed-context module");
        assert!(!regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_fails_for_missing_module() {
        let regex = Regex::with_mode("(?{wasm:missing:evaluate})", ExecutionMode::Safe)
            .expect("Failed to compile WASM code block pattern");
        assert!(!regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_fails_for_malformed_spec() {
        let regex = Regex::with_mode("(?{wasm:malformed})", ExecutionMode::Safe)
            .expect("Failed to compile malformed WASM code block pattern");
        assert!(!regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn safe_mode_wasm_code_block_fails_for_invalid_export_signature() {
        let regex = Regex::with_mode("(?{wasm:bad_sig:evaluate})", ExecutionMode::Safe)
            .expect("Failed to compile WASM code block pattern");
        regex
            .register_wasm_module(
                "bad_sig",
                test_wasm_module_bytes(
                    r#"
                    (module
                        (func (export "evaluate"))
                    )
                    "#,
                ),
            )
            .expect("Failed to register bad-signature WASM module");
        assert!(!regex.is_match(""));
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn register_wasm_module_requires_attached_execution_manager() {
        let regex = Regex::with_mode("cat", ExecutionMode::Full).expect("Failed to compile regex");
        let result = regex.register_wasm_module(
            "noop",
            test_wasm_module_bytes(
                r#"
                (module
                    (func (export "evaluate") (result i32)
                        i32.const 1
                    )
                )
                "#,
            ),
        );
        assert!(
            result.is_err(),
            "WASM registration should fail without runtime manager"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("WASM module registration is unavailable for this compiled regex"));
    }

    #[cfg(not(feature = "lua"))]
    #[test]
    fn safe_mode_lua_code_blocks_require_lua_feature() {
        let result = Regex::with_mode("(?{lua:return true})", ExecutionMode::Safe);
        assert!(
            result.is_err(),
            "Lua code block should require the lua feature"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("lua code blocks require the `lua` cargo feature"));
    }

    #[cfg(not(feature = "javascript"))]
    #[test]
    fn safe_mode_javascript_code_blocks_require_javascript_feature() {
        let result = Regex::with_mode("(?{js:return true})", ExecutionMode::Safe);
        assert!(
            result.is_err(),
            "JavaScript code block should require the javascript feature"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("javascript code blocks require the `javascript` cargo feature"));
    }

    #[cfg(not(feature = "rhai"))]
    #[test]
    fn safe_mode_rhai_code_blocks_require_rhai_feature() {
        let result = Regex::with_mode("(?{rhai:true})", ExecutionMode::Safe);
        assert!(
            result.is_err(),
            "Rhai code block should require the rhai feature"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("rhai code blocks require the `rhai` cargo feature"));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_code_block_can_access_variables() {
        let regex = Regex::with_mode(r#"(?{lua:return vars.env == "prod"})"#, ExecutionMode::Safe)
            .expect("Failed to compile Lua variable pattern");
        regex
            .set_variable("env", "prod")
            .expect("Failed to register execution variable");
        assert!(regex.is_match(""));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_expression_body_can_match() {
        let regex = Regex::with_mode(
            r#"(?<word>cat)(?{lua:named.word == "cat"})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Lua expression-body pattern");
        assert!(regex.is_match("cat"));
        assert!(!regex.is_match("dog"));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_code_block_can_access_named_captures() {
        let regex = Regex::with_mode(
            r#"(?<word>cat)(?{lua:return named.word == "cat"})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Lua code block pattern");
        assert!(regex.is_match("cat"));
        assert!(!regex.is_match("dog"));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_code_block_can_access_match_metadata() {
        let regex = Regex::with_mode(
            r#"foo|cat(?{lua:return match_start == 2 and match_end == 5 and match_length == 3 and branch_number == 2})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Lua match-metadata pattern");
        let first = regex
            .find_first("xxcat")
            .expect("Expected Lua match-metadata match");
        assert_eq!((first.start, first.end), (2, 5));
        assert_eq!(first.matched_branch_number, Some(2));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_code_block_participates_in_backtracking() {
        let regex = Regex::with_mode(r#"a*(?{lua:return arg[0] == ""})a"#, ExecutionMode::Safe)
            .expect("Failed to compile Lua backtracking pattern");
        assert!(regex.is_match("a"));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_code_block_surfaces_numeric_results_in_match_mode() {
        let regex = Regex::with_mode(r"(?{lua:return 1})", ExecutionMode::Safe)
            .expect("Failed to compile Lua numeric-result pattern");
        assert!(regex.is_match(""));
        let first = regex
            .find_first("")
            .expect("Expected Lua numeric-result match");
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 0);
        assert_eq!(first.code_result, Some(CodeBlockValue::Numeric(1.0)));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_code_block_backtracking_restores_winning_result() {
        let regex = Regex::with_mode(
            r#"a*(?{lua:return string.len(arg[0])})a"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Lua backtracking-result pattern");

        let first = regex
            .find_first("a")
            .expect("Expected Lua backtracking-result match");
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 1);
        assert_eq!(first.code_result, Some(CodeBlockValue::Numeric(0.0)));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_code_block_helpers_surface_numeric_and_replacement_results() {
        let numeric = Regex::with_mode(r"(?{lua:return 1})", ExecutionMode::Safe)
            .expect("Failed to compile Lua numeric-helper pattern");
        assert_eq!(numeric.find_first_numeric_with_code(""), Some(1.0));
        assert_eq!(numeric.find_all_numeric_with_code(""), vec![1.0]);

        let replacement = Regex::with_mode(r#"cat(?{lua:return "CAT"})"#, ExecutionMode::Safe)
            .expect("Failed to compile Lua replacement-helper pattern");
        assert_eq!(
            replacement.replace_first_with_code("cat dog cat"),
            "CAT dog cat"
        );
        assert_eq!(
            replacement.replace_all_with_code("cat dog cat"),
            "CAT dog CAT"
        );
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_expression_body_helpers_surface_numeric_and_replacement_results() {
        let numeric = Regex::with_mode(r"(?{lua:1})", ExecutionMode::Safe)
            .expect("Failed to compile Lua numeric-expression helper pattern");
        assert_eq!(numeric.find_first_numeric_with_code(""), Some(1.0));
        assert_eq!(numeric.find_all_numeric_with_code(""), vec![1.0]);

        let replacement = Regex::with_mode(r#"cat(?{lua:"CAT"})"#, ExecutionMode::Safe)
            .expect("Failed to compile Lua replacement-expression helper pattern");
        assert_eq!(
            replacement.replace_first_with_code("cat dog cat"),
            "CAT dog cat"
        );
        assert_eq!(
            replacement.replace_all_with_code("cat dog cat"),
            "CAT dog CAT"
        );
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_rgx_helpers_can_emit_results_from_statement_bodies() {
        let numeric = Regex::with_mode(
            r#"(?{lua:rgx.emit_numeric(7); return true})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Lua emitted-numeric pattern");
        assert_eq!(numeric.find_first_numeric_with_code(""), Some(7.0));

        let replacement = Regex::with_mode(
            r#"cat(?{lua:rgx.emit_replacement("CAT"); return true})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Lua emitted-replacement pattern");
        assert_eq!(
            replacement.replace_first_with_code("cat dog cat"),
            "CAT dog cat"
        );
    }

    #[cfg(feature = "lua")]
    #[test]
    fn safe_mode_lua_emitted_result_is_ignored_on_failure() {
        let regex = Regex::with_mode(
            r#"(?{lua:rgx.emit_numeric(7); return false})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Lua emitted-failure pattern");
        assert!(!regex.is_match(""));
        assert_eq!(regex.find_first_numeric_with_code(""), None);
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn safe_mode_javascript_code_block_can_access_variables() {
        let regex = Regex::with_mode(
            r#"(?{js:return vars.env === "prod";})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript variable pattern");
        regex
            .set_variable("env", "prod")
            .expect("Failed to register execution variable");
        assert!(regex.is_match(""));
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn safe_mode_javascript_code_block_can_match() {
        let regex = Regex::with_mode(
            r#"(?<word>cat)(?{js:return named.word === "cat";})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript code block pattern");
        assert!(regex.is_match("cat"));
        assert!(!regex.is_match("dog"));
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn safe_mode_javascript_code_block_can_access_match_metadata() {
        let regex = Regex::with_mode(
            r#"foo|cat(?{js:return match_start === 2 && match_end === 5 && match_length === 3 && branch_number === 2;})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript match-metadata pattern");

        let first = regex
            .find_first("xxcat")
            .expect("Expected JavaScript match-metadata match");
        assert_eq!((first.start, first.end), (2, 5));
        assert_eq!(first.matched_branch_number, Some(2));
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn safe_mode_javascript_expression_body_can_fail_match() {
        let regex = Regex::with_mode(
            r#"(?<word>cat)(?{js:named.word === "dog"})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript expression-body pattern");

        assert!(!regex.is_match("cat"));
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn safe_mode_javascript_code_blocks_use_last_non_boolean_result() {
        let regex = Regex::with_mode(
            r#"(?{js:return 1;})(?{js:return "done";})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript richer-result pattern");

        let first = regex
            .find_first("")
            .expect("Expected JavaScript richer-result match");
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 0);
        assert_eq!(
            first.code_result,
            Some(CodeBlockValue::Replacement("done".to_string()))
        );
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn safe_mode_javascript_expression_body_helpers_surface_numeric_and_replacement_results() {
        let numeric = Regex::with_mode(
            r#"(?<digit>\d)(?{js:Number(named.digit)})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript numeric-helper pattern");
        assert_eq!(numeric.find_first_numeric_with_code("7a8"), Some(7.0));
        assert_eq!(numeric.find_all_numeric_with_code("7a8"), vec![7.0, 8.0]);

        let replacement = Regex::with_mode(r#"cat(?{js:"CAT"})"#, ExecutionMode::Safe)
            .expect("Failed to compile JavaScript replacement-helper pattern");
        assert_eq!(
            replacement.replace_first_with_code("cat dog cat"),
            "CAT dog cat"
        );
        assert_eq!(
            replacement.replace_all_with_code("cat dog cat"),
            "CAT dog CAT"
        );
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn safe_mode_javascript_rgx_helpers_can_emit_results_from_statement_bodies() {
        let numeric = Regex::with_mode(
            r#"(?{js:rgx.emit_numeric(7); return true;})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript emitted-numeric pattern");
        assert_eq!(numeric.find_first_numeric_with_code(""), Some(7.0));

        let replacement = Regex::with_mode(
            r#"cat(?{js:rgx.emit_replacement("CAT"); return true;})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript emitted-replacement pattern");
        assert_eq!(
            replacement.replace_first_with_code("cat dog cat"),
            "CAT dog cat"
        );
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn safe_mode_javascript_rgx_helper_last_emitted_value_wins() {
        let regex = Regex::with_mode(
            r#"(?{js:rgx.emit_numeric(1); rgx.emit_numeric(2); return true;})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile JavaScript repeated-emission pattern");
        assert_eq!(regex.find_first_numeric_with_code(""), Some(2.0));
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_code_block_can_access_variables() {
        let regex = Regex::with_mode(r#"(?{rhai: vars["env"] == "prod"})"#, ExecutionMode::Safe)
            .expect("Failed to compile Rhai variable pattern");
        regex
            .set_variable("env", "prod")
            .expect("Failed to register execution variable");
        assert!(regex.is_match(""));
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_code_block_can_match() {
        let regex = Regex::with_mode(
            r#"(?<word>cat)(?{rhai: named["word"] == "cat"})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Rhai code block pattern");
        assert!(regex.is_match("cat"));
        assert!(!regex.is_match("dog"));
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_explicit_return_body_can_match() {
        let regex = Regex::with_mode(
            r#"(?<word>cat)(?{rhai: return named["word"] == "cat";})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Rhai explicit-return pattern");
        assert!(regex.is_match("cat"));
        assert!(!regex.is_match("dog"));
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_code_block_can_access_match_metadata() {
        let regex = Regex::with_mode(
            r#"foo|cat(?{rhai: match_start == 2 && match_end == 5 && match_length == 3 && branch_number == 2})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Rhai match-metadata pattern");

        let first = regex
            .find_first("xxcat")
            .expect("Expected Rhai match-metadata match");
        assert_eq!((first.start, first.end), (2, 5));
        assert_eq!(first.matched_branch_number, Some(2));
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_code_block_participates_in_backtracking() {
        let regex = Regex::with_mode(r#"a*(?{rhai: arg[0] == ""})a"#, ExecutionMode::Safe)
            .expect("Failed to compile Rhai backtracking pattern");
        assert!(regex.is_match("a"));
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_code_blocks_use_last_non_boolean_result() {
        let regex = Regex::with_mode(r#"(?{rhai: 1})(?{rhai: "done"})"#, ExecutionMode::Safe)
            .expect("Failed to compile Rhai richer-result pattern");

        let first = regex
            .find_first("")
            .expect("Expected Rhai richer-result match");
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 0);
        assert_eq!(
            first.code_result,
            Some(CodeBlockValue::Replacement("done".to_string()))
        );
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_code_block_helpers_surface_numeric_and_replacement_results() {
        let numeric = Regex::with_mode(r"(?{rhai: 1})", ExecutionMode::Safe)
            .expect("Failed to compile Rhai numeric-helper pattern");
        assert_eq!(numeric.find_first_numeric_with_code(""), Some(1.0));
        assert_eq!(numeric.find_all_numeric_with_code(""), vec![1.0]);

        let replacement = Regex::with_mode(r#"cat(?{rhai: "CAT"})"#, ExecutionMode::Safe)
            .expect("Failed to compile Rhai replacement-helper pattern");
        assert_eq!(
            replacement.replace_first_with_code("cat dog cat"),
            "CAT dog cat"
        );
        assert_eq!(
            replacement.replace_all_with_code("cat dog cat"),
            "CAT dog CAT"
        );
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_explicit_return_helpers_surface_numeric_and_replacement_results() {
        let numeric = Regex::with_mode(r"(?{rhai: return 1;})", ExecutionMode::Safe)
            .expect("Failed to compile Rhai explicit-return numeric-helper pattern");
        assert_eq!(numeric.find_first_numeric_with_code(""), Some(1.0));
        assert_eq!(numeric.find_all_numeric_with_code(""), vec![1.0]);

        let replacement = Regex::with_mode(r#"cat(?{rhai: return "CAT";})"#, ExecutionMode::Safe)
            .expect("Failed to compile Rhai explicit-return replacement-helper pattern");
        assert_eq!(
            replacement.replace_first_with_code("cat dog cat"),
            "CAT dog cat"
        );
        assert_eq!(
            replacement.replace_all_with_code("cat dog cat"),
            "CAT dog CAT"
        );
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_helpers_can_emit_results_from_statement_bodies() {
        let numeric = Regex::with_mode(
            r#"(?{rhai: emit_numeric(7); return true;})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Rhai emitted-numeric pattern");
        assert_eq!(numeric.find_first_numeric_with_code(""), Some(7.0));

        let replacement = Regex::with_mode(
            r#"cat(?{rhai: emit_replacement("CAT"); return true;})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Rhai emitted-replacement pattern");
        assert_eq!(
            replacement.replace_first_with_code("cat dog cat"),
            "CAT dog cat"
        );
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn safe_mode_rhai_emitted_result_is_ignored_on_failure() {
        let regex = Regex::with_mode(
            r#"(?{rhai: emit_numeric(7); return false;})"#,
            ExecutionMode::Safe,
        )
        .expect("Failed to compile Rhai emitted-failure pattern");
        assert!(!regex.is_match(""));
        assert_eq!(regex.find_first_numeric_with_code(""), None);
    }

    #[test]
    fn parser_single_digit_8_or_9_backref_to_missing_group_reports_compile_error() {
        // PCRE2 rule: `\N` with N < 10 is **always** a back reference,
        // and a lone 8 / 9 can't fall back to an octal literal because
        // 8 and 9 aren't octal digits. Keep this as a compile error so
        // obvious typos surface cleanly rather than silently becoming
        // a literal '8' or '9'. (Multi-digit non-octal forms like
        // `\89` have a well-defined PCRE2 literal interpretation —
        // see `parser_multi_digit_non_octal_backref_becomes_literal`.)
        let result = Regex::compile(r"(a)\9");
        assert!(
            result.is_err(),
            "Backreference to a missing group with non-octal digit should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains(r"backreference '\9' refers to missing capture group"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn parser_single_digit_backreference_to_missing_group_becomes_octal() {
        // PCRE2 semantics (per the PCRE2 10.47 manual): `\N` with N
        // less than 10 AND no group N is an octal escape. `\2` with
        // no group 2 is byte 0x02. This mirrors PCRE2 testinput1's
        // `/abc\0def/`, `/(abc)\123/`, and similar single/multi-digit
        // patterns.
        let r = Regex::compile(r"\2")
            .expect("single-digit backref with no group should compile as octal");
        // 0x02 is the character matched by `\2` in octal.
        assert!(r.is_match("\x02"));
    }

    #[test]
    fn parser_multi_digit_non_octal_backref_becomes_literal() {
        // PCRE2 rule: a multi-digit `\NN...N` where the whole decimal
        // number is > total_groups reads up to three leading octal
        // digits as an octal escape; any remaining decimal digits
        // stand for themselves as literal characters. First digit 8
        // or 9 ⇒ zero octal digits consumed ⇒ the whole sequence is
        // literal.
        let r =
            Regex::compile(r"\89").expect("\\89 with no groups should compile as literal \"89\"");
        assert!(
            r.is_match("89"),
            "\\89 should match the literal text \"89\""
        );

        // Leading 1 is a valid octal digit; 9 is not. So `\199`
        // consumes one octal digit (0o1 = '\u{1}') then literal "99".
        let r = Regex::compile(r"\199")
            .expect("\\199 with no groups should compile as Char(0o1) + literal \"99\"");
        assert!(
            r.is_match("\u{01}99"),
            "\\199 should match U+0001 followed by \"99\""
        );
        assert!(
            !r.is_match("199"),
            "\\199 must not match literal \"199\" — the leading 1 is octal, not decimal"
        );
    }

    #[test]
    fn parser_nine_digit_backref_becomes_octal_triplet_plus_literal() {
        // Regression for PCRE2 testinput1:6539 (`/\214748364/`).
        // With no groups, `\214748364` reads the first three digits
        // as octal (0o214 = 140 = U+008C) and the remaining six as
        // literal characters "748364". Before this fix the decimal
        // 214_748_364 was rejected as a missing-group back reference.
        let r = Regex::compile(r"\214748364")
            .expect("\\214748364 with no groups should compile as U+008C + \"748364\"");
        assert!(
            r.is_match("\u{8C}748364"),
            "\\214748364 should match U+008C followed by \"748364\""
        );
        assert!(!r.is_match("214748364"));
    }

    #[test]
    fn parser_entire_pattern_recursion_executes() {
        let regex =
            Regex::compile("a(?R)?b").expect("Failed to compile whole-pattern recursion syntax");
        let nested = regex
            .find_first("xxaaabbbzz")
            .expect("Expected whole-pattern recursion match");
        assert_eq!((nested.start, nested.end), (2, 8));
        assert!(regex.is_match("ab"));
        assert!(!regex.is_match("ccc"));
    }

    #[test]
    fn parser_numbered_group_recursion_executes() {
        let regex = Regex::compile(r"\A(a(?1)?b)\z")
            .expect("Failed to compile numbered-group recursion syntax");
        assert!(regex.is_match("ab"));
        assert!(regex.is_match("aaabbb"));
        assert!(!regex.is_match("aabbb"));
    }

    #[test]
    fn parser_named_group_recursion_executes() {
        let regex = Regex::compile(r"\A(?<word>a(?&word)?b)\z")
            .expect("Failed to compile named-group recursion syntax");
        assert!(regex.is_match("ab"));
        assert!(regex.is_match("aaabbb"));
        assert!(!regex.is_match("aabbb"));
    }

    #[test]
    fn parser_missing_group_recursion_reports_compile_error() {
        let result = Regex::compile("(?2)");
        assert!(
            result.is_err(),
            "Recursive call to a missing group should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("recursive subroutine '(?2)' refers to missing capture group"));
    }

    #[test]
    fn parser_missing_named_group_recursion_reports_compile_error() {
        let result = Regex::compile("(?&missing)");
        assert!(
            result.is_err(),
            "Recursive call to a missing named group should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg
            .contains("recursive subroutine '(?&missing)' refers to missing named capture group"));
    }

    #[test]
    fn parser_conditional_group_exists_selects_runtime_branch() {
        let regex = Regex::compile(r"\A(a)?(?(1)b|c)\z")
            .expect("Failed to compile group-exists conditional syntax");
        assert!(regex.is_match("ab"));
        assert!(regex.is_match("c"));
        assert!(!regex.is_match("ac"));
    }

    #[test]
    fn parser_conditional_relative_group_exists_selects_runtime_branch() {
        let backward = Regex::compile(r"\A(a)?(?(-1)b|c)\z")
            .expect("Failed to compile backward relative-group conditional syntax");
        assert!(backward.is_match("ab"));
        assert!(backward.is_match("c"));
        assert!(!backward.is_match("ac"));

        let forward = Regex::compile(r"\A(?(+1)a|b)(a)\z")
            .expect("Failed to compile forward relative-group conditional syntax");
        assert!(forward.is_match("ba"));
        assert!(!forward.is_match("aa"));
    }

    #[test]
    fn parser_unicode_property_letters_match_runtime_path() {
        let regex = Regex::compile(r"\p{L}+")
            .expect("Failed to compile Unicode property class for letters");
        assert!(regex.is_match("abc"));
        assert!(regex.is_match("é"));
        assert!(regex.is_match("β"));
        assert!(!regex.is_match("123"));

        let first = regex
            .find_first("123β45")
            .expect("Expected Unicode letter match");
        assert_eq!((first.start, first.end), (3, 5));
    }

    #[test]
    fn parser_unicode_property_negation_matches_runtime_path() {
        let regex = Regex::compile(r"\P{L}+")
            .expect("Failed to compile negated Unicode property class for letters");
        assert!(regex.is_match("123"));
        assert!(regex.is_match("!"));
        assert!(!regex.is_match("abc"));
        assert!(!regex.is_match("β"));
    }

    #[test]
    fn parser_unicode_property_script_value_matches_runtime_path() {
        let regex =
            Regex::compile(r"\p{Greek}+").expect("Failed to compile Unicode script property class");
        assert!(regex.is_match("β"));
        assert!(regex.is_match("Ω"));
        assert!(!regex.is_match("abc"));
    }

    #[test]
    fn parser_invalid_unicode_property_reports_compile_error() {
        let result = Regex::compile(r"\p{Definitely_Not_A_Real_Property}");
        assert!(
            result.is_err(),
            "Invalid Unicode property should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("invalid Unicode property class"),
            "unexpected invalid-property compile message: {msg}"
        );
    }

    #[test]
    fn ast_unicode_property_class_executes() {
        let ast = RegexAst::UnicodeClass {
            name: "L".to_string(),
            negated: false,
        };

        let regex = Regex::from_ast(ast).expect("AST Unicode property class should compile");
        assert!(regex.is_match("é"));
        assert!(!regex.is_match("1"));
    }

    #[test]
    fn parser_conditional_named_group_exists_selects_runtime_branch() {
        let angle = Regex::compile(r"\A(?<g>a)?(?(<g>)b|c)\z")
            .expect("Failed to compile named-group conditional syntax");
        assert!(angle.is_match("ab"));
        assert!(angle.is_match("c"));
        assert!(!angle.is_match("ac"));

        let bare = Regex::compile(r"\A(?<g>a)?(?(g)b|c)\z")
            .expect("Failed to compile bare named-group conditional syntax");
        assert!(bare.is_match("ab"));
        assert!(bare.is_match("c"));
        assert!(!bare.is_match("ac"));
    }

    #[test]
    fn parser_conditional_recursion_any_selects_runtime_branch() {
        let regex = Regex::compile(r"a(?(R)b|c)(?R)?d")
            .expect("Failed to compile recursion-any conditional syntax");
        assert!(regex.is_match("acd"));
        assert!(regex.is_match("acabdd"));
        assert!(!regex.is_match("abd"));
    }

    #[test]
    fn parser_conditional_recursion_group_selects_runtime_branch() {
        let regex = Regex::compile(r"\A(a(?(R1)b|c)(?1)?d)\z")
            .expect("Failed to compile recursion-group conditional syntax");
        assert!(regex.is_match("acd"));
        assert!(regex.is_match("acabdd"));
        assert!(!regex.is_match("abd"));
    }

    #[test]
    fn parser_conditional_recursion_named_selects_runtime_branch() {
        let regex = Regex::compile(r"\A(?<word>a(?(R&word)b|c)(?&word)?d)\z")
            .expect("Failed to compile recursion-named conditional syntax");
        assert!(regex.is_match("acd"));
        assert!(regex.is_match("acabdd"));
        assert!(!regex.is_match("abd"));
    }

    #[test]
    fn parser_conditional_recursion_name_ambiguity_prefers_named_group_exists() {
        let regex = Regex::compile(r"\A(?<R>a)?(?(R)b|c)\z")
            .expect("Failed to compile ambiguous R conditional syntax");
        assert!(regex.is_match("ab"));
        assert!(regex.is_match("c"));
        assert!(!regex.is_match("ac"));
    }

    #[test]
    fn parser_conditional_recursion_group_name_ambiguity_prefers_named_group_exists() {
        let regex = Regex::compile(r"\A(?<R1>a)?(?(R1)b|c)\z")
            .expect("Failed to compile ambiguous R1 conditional syntax");
        assert!(regex.is_match("ab"));
        assert!(regex.is_match("c"));
        assert!(!regex.is_match("ac"));
    }

    #[test]
    fn parser_conditional_without_false_branch_acts_like_empty_else() {
        let regex = Regex::compile(r"\A(a)?(?(1)b)d\z")
            .expect("Failed to compile single-branch conditional");
        assert!(regex.is_match("abd"));
        assert!(regex.is_match("d"));
        assert!(!regex.is_match("ad"));
    }

    #[test]
    fn parser_conditional_lookaround_forms_select_runtime_branch() {
        let lookahead =
            Regex::compile("(?(?=ab)a|z)b").expect("Failed to compile lookahead conditional");
        assert!(lookahead.is_match("ab"));
        assert!(lookahead.is_match("zb"));
        assert!(!lookahead.is_match("xb"));

        let negative_lookahead = Regex::compile("(?(?!ab)z|a)b")
            .expect("Failed to compile negative-lookahead conditional");
        assert!(negative_lookahead.is_match("ab"));
        assert!(negative_lookahead.is_match("zb"));
        assert!(!negative_lookahead.is_match("xb"));

        let lookbehind =
            Regex::compile("(?(?<=x)a|b)").expect("Failed to compile lookbehind conditional");
        let lookbehind_match = lookbehind
            .find_first("xa")
            .expect("Expected lookbehind conditional match");
        assert_eq!((lookbehind_match.start, lookbehind_match.end), (1, 2));
        assert!(lookbehind.is_match("b"));

        let negative_lookbehind = Regex::compile("(?(?<!x)b|a)")
            .expect("Failed to compile negative-lookbehind conditional");
        let negative_lookbehind_match = negative_lookbehind
            .find_first("xa")
            .expect("Expected negative-lookbehind conditional match");
        assert_eq!(
            (
                negative_lookbehind_match.start,
                negative_lookbehind_match.end
            ),
            (1, 2)
        );
        assert!(negative_lookbehind.is_match("b"));
    }

    #[test]
    fn parser_conditional_missing_group_reports_compile_error() {
        let result = Regex::compile("(a)?(?(2)b|c)");
        assert!(
            result.is_err(),
            "Conditional missing-group reference should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("conditional '(?(2)...)' refers to missing capture group"));
    }

    #[test]
    fn parser_conditional_missing_named_group_reports_compile_error() {
        let result = Regex::compile("(?<g>a)?(?(missing)b|c)");
        assert!(
            result.is_err(),
            "Conditional missing named-group reference should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("conditional '(?(missing)...)' refers to missing named capture group"));
    }

    #[test]
    fn parser_conditional_missing_recursion_group_reports_compile_error() {
        let result = Regex::compile("(?(R2)a|b)");
        assert!(
            result.is_err(),
            "Conditional missing recursion-group reference should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("conditional '(?(R2)...)' refers to missing capture group"));
    }

    #[test]
    fn parser_conditional_missing_named_recursion_group_reports_compile_error() {
        let result = Regex::compile("(?(R&missing)a|b)");
        assert!(
            result.is_err(),
            "Conditional missing named recursion-group reference should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("conditional '(?(R&missing)...)' refers to missing named capture group")
        );
    }

    #[test]
    fn parser_define_conditional_with_false_branch_reports_compile_error() {
        let result = Regex::compile("(?(DEFINE)a|b)");
        assert!(
            result.is_err(),
            "DEFINE conditional with false branch should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("conditional '(?(DEFINE)...)' does not support a false branch"));
    }

    #[test]
    fn parser_define_conditional_without_false_branch_acts_like_empty_else() {
        let regex = Regex::compile(r"\A(?(DEFINE)a)\z")
            .expect("Failed to compile DEFINE conditional without false branch");
        assert!(regex.is_match(""));
        assert!(!regex.is_match("a"));
    }

    #[test]
    fn parser_define_conditional_can_define_numbered_subroutine_for_later_use() {
        let regex = Regex::compile(r"\A(?(DEFINE)(a+))(?1)\z")
            .expect("Failed to compile DEFINE conditional with numbered subroutine definition");
        assert!(regex.is_match("aaa"));
        assert!(!regex.is_match("bbb"));
    }

    #[test]
    fn parser_define_conditional_can_define_named_subroutine_for_later_use() {
        let regex = Regex::compile(r"\A(?(DEFINE)(?<word>a+))(?&word)\z")
            .expect("Failed to compile DEFINE conditional with named subroutine definition");
        assert!(regex.is_match("aaa"));
        assert!(!regex.is_match("bbb"));
    }

    #[test]
    fn ast_branch_reset_group_shares_capture_slot_across_alternatives() {
        let ast = RegexAst::Sequence(vec![
            RegexAst::Group {
                expr: Box::new(RegexAst::Alternation(vec![
                    RegexAst::Group {
                        expr: Box::new(RegexAst::Char('a')),
                        kind: GroupKind::Capturing,
                        index: None,
                        name: None,
                    },
                    RegexAst::Group {
                        expr: Box::new(RegexAst::Char('b')),
                        kind: GroupKind::Capturing,
                        index: None,
                        name: None,
                    },
                ])),
                kind: GroupKind::BranchReset,
                index: None,
                name: None,
            },
            RegexAst::Backreference(1),
        ]);

        let regex = Regex::from_ast(ast).expect("Failed to compile branch-reset AST directly");
        assert!(regex.is_match("aa"));
        assert!(regex.is_match("bb"));
        assert!(!regex.is_match("ab"));
    }

    #[test]
    fn parser_branch_reset_group_shares_capture_slot_across_alternatives() {
        let regex =
            Regex::compile(r"\A(?|(a)|(b))\1\z").expect("Failed to compile branch-reset syntax");
        assert!(regex.is_match("aa"));
        assert!(regex.is_match("bb"));
        assert!(!regex.is_match("ab"));
    }

    #[test]
    fn parser_branch_reset_group_uses_max_branch_arity_for_following_references() {
        let regex = Regex::compile(r"\A(?|(a)(b)|c)(?(2)d|e)\z")
            .expect("Failed to compile branch-reset conditional pattern");
        assert!(regex.is_match("abd"));
        assert!(regex.is_match("ce"));
        assert!(!regex.is_match("abe"));
        assert!(!regex.is_match("cd"));
    }

    #[derive(Clone, Copy)]
    struct ParserExtendedCharClassExecutionFixture {
        pattern: &'static str,
        matches_input: &'static str,
        rejects_input: &'static str,
        description: &'static str,
    }

    fn assert_parser_extended_char_class_execution_fixture(
        fixture: ParserExtendedCharClassExecutionFixture,
    ) {
        let regex = Regex::compile(fixture.pattern).unwrap_or_else(|e| {
            panic!(
                "Failed to compile {} pattern '{}': {e}",
                fixture.description, fixture.pattern
            )
        });
        assert!(
            regex.is_match(fixture.matches_input),
            "{} pattern should match '{}'",
            fixture.description,
            fixture.matches_input
        );
        assert!(
            !regex.is_match(fixture.rejects_input),
            "{} pattern should reject '{}'",
            fixture.description,
            fixture.rejects_input
        );
    }

    const SIMPLE_PARSER_EXTENDED_CHAR_CLASS_EXECUTION_FIXTURES:
        &[ParserExtendedCharClassExecutionFixture] = &[
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[[a-z]])+\z",
            matches_input: "abcxyz",
            rejects_input: "abc123",
            description: "simple extended character class range",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[[^0-9]])+\z",
            matches_input: "abcXYZ",
            rejects_input: "abc123",
            description: "negated extended character class",
        },
    ];

    const ALGEBRAIC_PARSER_EXTENDED_CHAR_CLASS_EXECUTION_FIXTURES:
        &[ParserExtendedCharClassExecutionFixture] = &[
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[[\dA-F]])+\z",
            matches_input: "FACE204",
            rejects_input: "face_",
            description: "nested ordinary shorthand/range extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[[[:graph:]]])+\z",
            matches_input: "AZ9!",
            rejects_input: "AZ 9",
            description: "nested ordinary POSIX extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[[\p{L}] - [\p{Lu}]])+\z",
            matches_input: "facet",
            rejects_input: "Face",
            description: "nested ordinary property algebra extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[[a-z] - [aeiou]])+\z",
            matches_input: "bcdfxyz",
            rejects_input: "facet",
            description: "difference-style extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\p{L} & \p{Lu}])+\z",
            matches_input: "ABCXYZ",
            rejects_input: "ABcXYZ",
            description: "property-intersection extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ ![0-9] ])+\z",
            matches_input: "abcXYZ!",
            rejects_input: "abc123",
            description: "complemented extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ ([a-z] - [aeiou]) & [b-d] ])+\z",
            matches_input: "bcdb",
            rejects_input: "bef",
            description: "grouped-algebra extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ [AC] ^ [BC] ])+\z",
            matches_input: "ABBA",
            rejects_input: "AC",
            description: "symmetric-difference extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ [a-f] | [d-z] & [m-p] ])+\z",
            matches_input: "abcmnop",
            rejects_input: "xyz",
            description: "same-level precedence extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ [a-z] - [aeiou] + [0-9] - [5] ])+\z",
            matches_input: "bcdf0249xyz",
            rejects_input: "face5",
            description: "multi-operator chain extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\d - [3]])+\z",
            matches_input: "20479",
            rejects_input: "1234",
            description: "digit-shorthand extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\w & [a-z]])+\z",
            matches_input: "facet",
            rejects_input: "face_",
            description: "word-shorthand extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\D & [A-F]])+\z",
            matches_input: "FACE",
            rejects_input: "FA3E",
            description: "negated-shorthand extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ [:graph:] ])+\z",
            matches_input: "AZ9!",
            rejects_input: "AZ 9",
            description: "bare POSIX graph extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ [:^alpha:] ])+\z",
            matches_input: "19?!",
            rejects_input: "A1",
            description: "negated bare POSIX alpha extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ ![:alpha:] ])+\z",
            matches_input: "19?!",
            rejects_input: "A1",
            description: "complemented bare POSIX alpha extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[ [:alpha:] & [a-z\t] ])+\z",
            matches_input: "facet",
            rejects_input: "Face\t",
            description: "POSIX-alpha algebra extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\x{41} - [B]])+\z",
            matches_input: "AAAA",
            rejects_input: "AAB",
            description: "hex-escape extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\n | \t])+\z",
            matches_input: "\n\t\n",
            rejects_input: " \n",
            description: "control-escape extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\a | \b | \e | \f])+\z",
            matches_input: "\u{07}\u{08}\u{1B}\u{0C}\u{07}",
            rejects_input: "\u{07}A",
            description: "control-literal extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\cA | [B]])+\z",
            matches_input: "\u{0001}B\u{0001}",
            rejects_input: "ABC",
            description: "control-letter extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\040 | \011 | \o{101}])+\z",
            matches_input: " \tA\t ",
            rejects_input: "\nA",
            description: "octal-escape extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\h])+\z",
            matches_input: " \t\u{00A0}\u{1680}\u{202F}\u{3000}",
            rejects_input: "\n \t",
            description: "horizontal-whitespace extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\H])+\z",
            matches_input: "A\nB",
            rejects_input: " \t\u{00A0}",
            description: "negated horizontal-whitespace extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\v])+\z",
            matches_input: "\n\u{000B}\u{0085}\u{2028}\u{2029}",
            rejects_input: " \n",
            description: "vertical-whitespace extended character class",
        },
        ParserExtendedCharClassExecutionFixture {
            pattern: r"\A(?[\V])+\z",
            matches_input: "A \u{00A0}\t",
            rejects_input: "\n\u{0085}\u{2028}",
            description: "negated vertical-whitespace extended character class",
        },
    ];

    #[test]
    fn parser_extended_char_class_simple_cases_execute_on_default_path() {
        for fixture in SIMPLE_PARSER_EXTENDED_CHAR_CLASS_EXECUTION_FIXTURES {
            assert_parser_extended_char_class_execution_fixture(*fixture);
        }
    }

    #[test]
    fn parser_extended_char_class_algebraic_cases_execute_on_default_path() {
        for fixture in ALGEBRAIC_PARSER_EXTENDED_CHAR_CLASS_EXECUTION_FIXTURES {
            assert_parser_extended_char_class_execution_fixture(*fixture);
        }
    }

    #[test]
    fn parser_extended_char_class_requires_nested_simple_syntax() {
        let result = Regex::compile(r"(?[a-z])");
        assert!(
            result.is_err(),
            "unsupported extended character class should fail"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains(crate::compiler::EXTENDED_CHAR_CLASS_SUBSET_MESSAGE),
            "unexpected extended-char-class compile-boundary message: {msg}"
        );
    }

    #[test]
    fn parser_conditional_missing_relative_group_reports_compile_error() {
        let cases = [
            (
                "(?(+1)a|b)",
                "conditional '(?(+1)...)' refers to missing capture group",
            ),
            (
                "(?(-1)a|b)",
                "conditional '(?(-1)...)' refers to missing capture group",
            ),
        ];

        for (pattern, expected_msg) in cases {
            let result = Regex::compile(pattern);
            assert!(
                result.is_err(),
                "Relative conditional missing-group reference should not silently compile: {pattern}"
            );
            let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
            assert!(
                msg.contains(expected_msg),
                "unexpected missing relative-group compile message for pattern '{pattern}': {msg}"
            );
        }
    }

    const CAPABILITY_MATRIX_SUPPORTED_PARSER_PATH_CASES: &[(&str, &str, bool)] = &[
        ("cat|dog", "pet dog", true),
        (r"\d{2,3}", "id 1234", true),
        (r"\d{2,}", "x1y22", true),
        (r"\d{2,}", "x1y", false),
        (r"\d{2,3}3", "x123y", true),
        (r"\d{2,3}3", "x12y", false),
        (r"\d{2,}3", "x123y", true),
        (r"\d{2,}3", "x12y", false),
        ("a*a", "a", true),
        ("a+a", "aa", true),
        ("ab?b", "ab", true),
        (r"\Aa*+a\z", "aaaa", false),
        (r"\Aa*+b\z", "aaab", true),
        (r"\Aa++a\z", "aaaa", false),
        (r"\Aa?+a\z", "a", false),
        (r"\A\d{2,3}+3\z", "123", false),
        (r"\Aa{2,3}+b\z", "aaab", true),
        ("(?<word>cat)", "xxcatyy", true),
        (r"(a)\1", "baa", true),
        (r"(a)\1", "bab", false),
        (r"(a|ab)\1", "abab", true),
        (r"(ab)(?=\1)\1", "zababx", true),
        ("(?>ab|a)c", "abc", true),
        ("(?!cat)c", "car", true),
        ("(?!cat)c", "cat", false),
        ("(?<=x)a", "xa", true),
        ("(?<=x)a", "ba", false),
        ("(?<!x)a", "ba", true),
        ("(?=cat)c", "xxcat", true),
        ("(?<!x)a", "xa", false),
        (r"\p{L}+", "123β", true),
        (r"\p{L}+", "123", false),
        (r"\P{L}+", "123!", true),
        (r"\P{L}+", "β", false),
        (r"\A(?[[a-z]])+\z", "abcxyz", true),
        (r"\A(?[[a-z]])+\z", "abc123", false),
        (r"\A(?[[^0-9]])+\z", "abcXYZ", true),
        (r"\A(?[[^0-9]])+\z", "abc123", false),
        (r"\A(?[[\dA-F]])+\z", "FACE204", true),
        (r"\A(?[[\dA-F]])+\z", "face_", false),
        (r"\A(?[[[:graph:]]])+\z", "AZ9!", true),
        (r"\A(?[[[:graph:]]])+\z", "AZ 9", false),
        (r"\A(?[[\p{L}] - [\p{Lu}]])+\z", "facet", true),
        (r"\A(?[[\p{L}] - [\p{Lu}]])+\z", "Face", false),
        (r"\A(?[[a-z] - [aeiou]])+\z", "bcdfxyz", true),
        (r"\A(?[[a-z] - [aeiou]])+\z", "facet", false),
        (r"\A(?[\p{L} & \p{Lu}])+\z", "ABCXYZ", true),
        (r"\A(?[\p{L} & \p{Lu}])+\z", "ABcXYZ", false),
        (r"\A(?[\d - [3]])+\z", "20479", true),
        (r"\A(?[\d - [3]])+\z", "1234", false),
        (r"\A(?[\w & [a-z]])+\z", "facet", true),
        (r"\A(?[\w & [a-z]])+\z", "face_", false),
        (r"\A(?[\D & [A-F]])+\z", "FACE", true),
        (r"\A(?[\D & [A-F]])+\z", "FA3E", false),
        (r"\A(?[ [:graph:] ])+\z", "AZ9!", true),
        (r"\A(?[ [:graph:] ])+\z", "AZ 9", false),
        (r"\A(?[ [:^alpha:] ])+\z", "19?!", true),
        (r"\A(?[ [:^alpha:] ])+\z", "A1", false),
        (r"\A(?[ ![:alpha:] ])+\z", "19?!", true),
        (r"\A(?[ ![:alpha:] ])+\z", "A1", false),
        (r"\A(?[ [:alpha:] & [a-z\t] ])+\z", "facet", true),
        (r"\A(?[ [:alpha:] & [a-z\t] ])+\z", "Face\t", false),
        (r"\A(?[\h])+\z", " \t\u{00A0}\u{1680}\u{202F}\u{3000}", true),
        (r"\A(?[\h])+\z", "\n \t", false),
        (r"\A(?[\H])+\z", "A\nB", true),
        (r"\A(?[\H])+\z", " \t\u{00A0}", false),
        (r"\A(?[\v])+\z", "\n\u{000B}\u{0085}\u{2028}\u{2029}", true),
        (r"\A(?[\v])+\z", " \n", false),
        (r"\A(?[\V])+\z", "A \u{00A0}\t", true),
        (r"\A(?[\V])+\z", "\n\u{0085}\u{2028}", false),
        (r"\A(?[ ![0-9] ])+\z", "abcXYZ!", true),
        (r"\A(?[ ![0-9] ])+\z", "abc123", false),
        (r"\A(?[ ([a-z] - [aeiou]) & [b-d] ])+\z", "bcdb", true),
        (r"\A(?[ ([a-z] - [aeiou]) & [b-d] ])+\z", "bef", false),
        (r"\A(?[ [AC] ^ [BC] ])+\z", "ABBA", true),
        (r"\A(?[ [AC] ^ [BC] ])+\z", "AC", false),
        (r"\A(a)?(?(1)b|c)\z", "ab", true),
        (r"\A(a)?(?(1)b|c)\z", "c", true),
        (r"\A(a)?(?(1)b|c)\z", "ac", false),
        (r"\A(a)?(?(-1)b|c)\z", "ab", true),
        (r"\A(a)?(?(-1)b|c)\z", "c", true),
        (r"\A(a)?(?(-1)b|c)\z", "ac", false),
        (r"\A(?<g>a)?(?(g)b|c)\z", "ab", true),
        (r"\A(?<g>a)?(?(g)b|c)\z", "c", true),
        (r"a(?(R)b|c)(?R)?d", "acd", true),
        (r"a(?(R)b|c)(?R)?d", "acabdd", true),
        (r"a(?(R)b|c)(?R)?d", "abd", false),
        (r"\A(a(?(R1)b|c)(?1)?d)\z", "acd", true),
        (r"\A(a(?(R1)b|c)(?1)?d)\z", "acabdd", true),
        (r"\A(a(?(R1)b|c)(?1)?d)\z", "abd", false),
        (r"\A(?(DEFINE)(a+))\z", "", true),
        (r"\A(?(DEFINE)(?<word>a+))(?&word)\z", "aaa", true),
        (r"\A(?(+1)a|b)(a)\z", "ba", true),
        (r"\A(?(+1)a|b)(a)\z", "aa", false),
        ("a(?R)?b", "aaabbb", true),
        ("a(?R)?b", "ccc", false),
        (r"\A(a(?1)?b)\z", "aaabbb", true),
        (r"\A(a(?1)?b)\z", "aabbb", false),
        (r"\A(?<word>a(?&word)?b)\z", "aaabbb", true),
        (r"\A(?<word>a(?&word)?b)\z", "aabbb", false),
        ("(?(?=ab)a|z)b", "ab", true),
        ("(?(?=ab)a|z)b", "zb", true),
        ("(?(?=ab)a|z)b", "xb", false),
        ("(?(?!ab)z|a)b", "ab", true),
        ("(?(?!ab)z|a)b", "zb", true),
        ("(?(?<=x)a|b)", "xa", true),
        ("(?(?<=x)a|b)", "b", true),
        ("(?(?<!x)b|a)", "xa", true),
        ("(?(?<!x)b|a)", "b", true),
        (r"\Acat", "cat dog", true),
        (r"\Acat", "xxcat", false),
        ("dog$", "cat dog", true),
        ("dog$", "cat dog x", false),
        (r"dog\z", "cat dog", true),
        (r"dog\z", "cat dog\n", false),
        (r"dog\Z", "cat dog\n", true),
        (r"dog\Z", "cat dog\nx", false),
    ];

    fn assert_supported_parser_path_case(pattern: &str, input: &str, expected: bool) {
        let regex = Regex::compile(pattern)
            .unwrap_or_else(|e| panic!("expected supported pattern '{pattern}' to compile: {e}"));
        assert_eq!(
            regex.is_match(input),
            expected,
            "unexpected match result for supported pattern '{pattern}' on input '{input}'"
        );
    }

    #[test]
    fn capability_matrix_supported_parser_path_cases() {
        for (pattern, input, expected) in CAPABILITY_MATRIX_SUPPORTED_PARSER_PATH_CASES {
            assert_supported_parser_path_case(pattern, input, *expected);
        }
    }

    #[test]
    fn capability_matrix_explicit_compile_boundary_and_validation_cases() {
        let cases = [
            (
                "(?{lua:return true})",
                "code blocks require ExecutionMode::Safe or ExecutionMode::Full",
            ),
            (
                "(?(+1)a|b)",
                "conditional '(?(+1)...)' refers to missing capture group",
            ),
            (
                "(?(-1)a|b)",
                "conditional '(?(-1)...)' refers to missing capture group",
            ),
            (
                "(?(R2)a|b)",
                "conditional '(?(R2)...)' refers to missing capture group",
            ),
            (
                r"(?[a-z])",
                crate::compiler::EXTENDED_CHAR_CLASS_SUBSET_MESSAGE,
            ),
        ];

        for (pattern, expected_msg) in cases {
            let Err(err) = Regex::compile(pattern) else {
                panic!("expected pattern to be rejected at explicit compile boundary: {pattern}");
            };
            assert!(
                err.to_string().contains(expected_msg),
                "unexpected compile boundary message for pattern '{pattern}': {err}"
            );
        }
    }

    #[test]
    fn top_level_branch_id_exposed() {
        let regex = Regex::compile("cat|dog|bird").expect("Failed to compile alternation");
        let m = regex.find_first("xxdogxx").expect("Expected a match");
        assert_eq!(m.matched_branch_number, Some(2)); // 1-based top-level branch number
    }

    #[test]
    fn top_level_branch_id_not_overridden_by_nested_alternation() {
        let ast = RegexAst::Alternation(vec![
            RegexAst::Sequence(vec![
                RegexAst::Char('a'),
                RegexAst::Alternation(vec![RegexAst::Char('1'), RegexAst::Char('2')]),
            ]),
            RegexAst::Sequence(vec![
                RegexAst::Char('b'),
                RegexAst::Alternation(vec![RegexAst::Char('3'), RegexAst::Char('4')]),
            ]),
        ]);

        let regex = Regex::from_ast(ast).expect("Failed to compile nested alternation AST");
        let m = regex
            .find_first("b3")
            .expect("Expected nested alternation match");
        assert_eq!(m.matched_branch_number, Some(2)); // Must report top-level branch number
    }

    #[test]
    fn single_arm_alternation_has_no_branch_number() {
        let ast = RegexAst::Alternation(vec![RegexAst::Sequence(vec![
            RegexAst::Char('c'),
            RegexAst::Char('a'),
            RegexAst::Char('t'),
        ])]);

        let regex = Regex::from_ast(ast).expect("Failed to compile single-arm alternation AST");
        let m = regex.find_first("xxcatxx").expect("Expected a match");
        assert_eq!(m.matched_branch_number, None);
    }

    #[test]
    fn scoped_multiline_caret_matches_after_newline() {
        let re = Regex::compile("(?m:^a)").unwrap();
        assert!(re.is_match("b\na"));
        assert!(re.is_match("a"));
    }

    #[test]
    fn scoped_multiline_dollar_matches_before_newline() {
        let re = Regex::compile("(?m:a$)").unwrap();
        assert!(re.is_match("a\nb"));
        assert!(re.is_match("a"));
    }

    #[test]
    fn multiline_does_not_leak_outside_scope() {
        let re = Regex::compile("(?m:^a$)|^b$").unwrap();
        // ^b$ should NOT match after newline (outside multiline scope)
        assert!(!re.is_match("x\nb"));
        // But (?m:^a$) should match after newline
        assert!(re.is_match("x\na"));
    }

    #[test]
    fn scoped_dotall_dot_matches_newline() {
        let re = Regex::compile("(?s:a.b)").unwrap();
        assert!(re.is_match("a\nb"));
        assert!(re.is_match("axb"));
    }

    #[test]
    fn dotall_does_not_leak_outside_scope() {
        let re = Regex::compile("(?s:a.b)|c.d").unwrap();
        assert!(re.is_match("a\nb")); // dotall in scope
        assert!(!re.is_match("c\nd")); // not dotall outside
        assert!(re.is_match("cxd")); // normal dot outside
    }

    #[test]
    fn default_dot_does_not_match_newline() {
        let re = Regex::compile("a.b").unwrap();
        assert!(!re.is_match("a\nb"));
        assert!(re.is_match("axb"));
    }

    #[test]
    fn scoped_case_insensitive_literal_match() {
        let re = Regex::compile("(?i:abc)").unwrap();
        assert!(re.is_match("ABC"));
        assert!(re.is_match("AbC"));
        assert!(re.is_match("abc"));
    }

    #[test]
    fn case_insensitive_does_not_leak() {
        let re = Regex::compile("(?i:abc)def").unwrap();
        assert!(re.is_match("ABCdef"));
        assert!(!re.is_match("ABCDef"));
    }

    #[test]
    fn case_insensitive_char_class() {
        let re = Regex::compile("(?i:[a-z])+").unwrap();
        let m = re.find_first("Hello").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    fn case_insensitive_non_letter() {
        let re = Regex::compile("(?i:a1b)").unwrap();
        assert!(re.is_match("A1B"));
        assert!(!re.is_match("A2B"));
    }

    #[test]
    fn named_backreference_basic() {
        let re = Regex::compile(r"(?<word>\w+)\s+\k<word>").unwrap();
        assert!(re.is_match("the the"));
        assert!(!re.is_match("the that"));
    }

    #[test]
    fn named_backreference_quote_style() {
        let re = Regex::compile(r"(?<x>a)\k'x'").unwrap();
        assert!(re.is_match("aa"));
    }

    #[test]
    fn named_backreference_missing_group_reports_compile_error() {
        let result = Regex::compile(r"(a)\k<missing>");
        assert!(
            result.is_err(),
            "Named backreference to a missing group should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("named backreference"));
    }

    #[test]
    fn python_style_named_group() {
        let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})").unwrap();
        assert!(re.is_match("2026-04"));
        assert!(!re.is_match("26-4"));
    }

    #[test]
    fn python_style_named_backreference() {
        let re = Regex::compile(r"(?P<x>ab)(?P=x)").unwrap();
        assert!(re.is_match("abab"));
        assert!(!re.is_match("abcd"));
    }

    #[test]
    fn python_style_mixed_with_standard() {
        // Python-style group with standard \k backreference
        let re = Regex::compile(r"(?P<w>\w+)\s+\k<w>").unwrap();
        assert!(re.is_match("the the"));
    }

    #[test]
    fn braced_octal_escape_matches_codepoint() {
        // PGEN-RGX-0006 regression: \o{101} should decode to 'A' (U+0041),
        // not misparse as literal 'o' followed by {101} counted quantifier.
        let re = Regex::compile(r"\o{101}").unwrap();
        assert!(re.is_match("A"));
        assert!(!re.is_match("o"));
        assert!(!re.is_match("ooooo"));
    }

    #[test]
    fn braced_octal_escape_various_values() {
        // \o{60} = 0o60 = 48 = '0'
        let re = Regex::compile(r"\o{60}").unwrap();
        assert!(re.is_match("0"));
        // \o{141} = 0o141 = 97 = 'a'
        let re = Regex::compile(r"\o{141}").unwrap();
        assert!(re.is_match("a"));
    }

    #[test]
    fn flag_negation_disables_case_insensitive() {
        let re = Regex::compile("(?i:a)(?-i:B)").unwrap();
        assert!(re.is_match("aB")); // a case-insensitive, B case-sensitive
        assert!(re.is_match("AB")); // A matches case-insensitive, B exact
        assert!(!re.is_match("ab")); // b does not match B case-sensitively
    }

    #[test]
    fn flag_negation_disables_multiline() {
        let re = Regex::compile("(?m:^a)").unwrap();
        assert!(re.is_match("x\na")); // multiline: ^ matches after newline
        let re2 = Regex::compile("(?-m:^a)").unwrap();
        assert!(!re2.is_match("x\na")); // non-multiline: ^ only matches at text start
        assert!(re2.is_match("abc")); // matches at text start
    }

    #[test]
    fn flag_negation_disables_dotall() {
        let re = Regex::compile("(?s:a.b)(?-s:c.d)").unwrap();
        assert!(re.is_match("a\nbcxd")); // a.b dotall matches \n, c.d normal
        assert!(!re.is_match("a\nbc\nd")); // c.d in non-dotall won't match \n
    }

    #[test]
    fn flag_enable_and_disable_combined() {
        let re = Regex::compile("(?i-s:a.b)").unwrap();
        assert!(re.is_match("Axb")); // case-insensitive
        assert!(!re.is_match("A\nb")); // dotall disabled, . won't match \n
    }

    // ---- Extended / verbose mode (`(?x:...)`) ----------------------------

    #[test]
    fn extended_mode_ignores_whitespace() {
        let re = Regex::compile("(?x: a b c )").unwrap();
        assert!(re.is_match("abc"));
        assert!(!re.is_match("a b c"));
    }

    #[test]
    fn extended_mode_ignores_comments() {
        let re = Regex::compile("(?x: a  # match letter a\n b)").unwrap();
        assert!(re.is_match("ab"));
    }

    #[test]
    fn extended_mode_escaped_space_preserved() {
        let re = Regex::compile(r"(?x: a\ b )").unwrap();
        assert!(re.is_match("a b"));
        assert!(!re.is_match("ab"));
    }

    #[test]
    fn extended_mode_class_space_preserved() {
        let re = Regex::compile("(?x: a[ ]b )").unwrap();
        assert!(re.is_match("a b"));
    }

    #[test]
    fn extended_mode_scoped() {
        let re = Regex::compile("(?x: a b ) c d").unwrap();
        assert!(re.is_match("ab c d")); // ab from x-mode, " c d" literal outside
        assert!(!re.is_match("abc d")); // space before c is required
    }

    #[test]
    fn unscoped_flag_toggle_extends_across_alternation_branches() {
        // PCRE2 testinput1:2321 — `/(a(?i)bc|BB)x/` on "bbx" expects
        // match. The unscoped `(?i)` in branch 1 extends to the end of
        // the enclosing group, so branch 2's `BB` should match
        // case-insensitively against "bb". Before the fix, RGX scoped
        // `(?i)` to branch 1 only and branch 2's `BB` was byte-exact.
        let re = Regex::compile("(a(?i)bc|BB)x").unwrap();
        let m = re.find_first("bbx").expect("expected match for bbx");
        assert_eq!((m.start, m.end), (0, 3));
        // Branch 1 is still exercised for "ABCx" (both branches sharing
        // case-insensitive `bc`).
        assert!(re.is_match("aBCx"));
        assert!(re.is_match("abcx"));
    }

    #[test]
    fn scoped_flag_toggle_does_not_leak_to_later_alternation_branch() {
        // Regression pin: `(?i:foo)|bar` — the SCOPED `(?i:foo)`
        // form should NOT propagate `i` to branch 2. My
        // unscoped-toggle heuristic checks for `FG(_, Empty)`
        // pre-lowering; `(?i:foo)` emits `FG("i", Sequence(f,o,o))`
        // with a non-Empty body, so it is correctly ignored and
        // branch 2 stays case-sensitive.
        let re = Regex::compile("(?i:foo)|bar").unwrap();
        assert!(re.is_match("FOO")); // branch 1 matches case-insensitively
        assert!(re.is_match("bar")); // branch 2 matches exactly
        assert!(!re.is_match("BAR")); // branch 2 is NOT case-insensitive
    }

    #[test]
    fn extended_mode_scoped_disable_restores_literal_whitespace() {
        // PCRE2 testinput1:3921 `/(?x)(?-x: \s*#\s*)/` on subject "#"
        // expects NO match. `(?x)` sets extended mode globally; inside
        // `(?-x: ...)` the inverse `-x` disables extended mode for the
        // group body, which makes the leading LITERAL space significant.
        // The body ` \s*#\s*` (with a leading real space) therefore
        // requires at least one whitespace character before the '#'.
        // Subject "#" has no leading whitespace, so the group shouldn't
        // match, and the outer pattern shouldn't match.
        let re = Regex::compile(r"(?x)(?-x: \s*#\s*)").unwrap();
        assert!(
            !re.is_match("#"),
            "`(?x)(?-x: \\s*#\\s*)` must NOT match \"#\" — leading literal space inside the disable group is required"
        );
        // Sanity: it DOES match when the subject has a leading space.
        assert!(
            re.is_match(" # "),
            "`(?x)(?-x: \\s*#\\s*)` should match \" # \""
        );
    }

    #[test]
    fn case_insensitive_numbered_backref_matches_folded_text() {
        // PCRE2 testinput1:1458 — `/(abc)\1/i` on subject "ABCabc"
        // expects match. Previously RGX emitted `OpCode::Backref` which
        // does byte-exact comparison, so "abc" (captured by `\1` after
        // folded matching of "ABC") wouldn't match against "abc" in the
        // subject — because the captured text preserves "ABC" as-is and
        // byte-comparison with lowercase "abc" fails. The new
        // `BackrefCaseInsensitive` opcode walks captured and subject
        // chars together with per-char Unicode folding.
        let re = Regex::compile("(?i)(abc)\\1").unwrap();
        assert!(re.is_match("ABCabc"));
        assert!(re.is_match("abcABC"));
        assert!(re.is_match("AbCaBc"));
        assert!(!re.is_match("abcXYZ"));
    }

    #[test]
    fn case_insensitive_named_backref_matches_folded_text() {
        // Same fix applies to `\k<name>` / `(?P=name)` style named
        // backreferences inside `(?i)` scope.
        let re = Regex::compile(r"(?i)(?<w>cat)\k<w>").unwrap();
        assert!(re.is_match("CATcat"));
        assert!(re.is_match("catCAT"));
        assert!(re.is_match("CaTcAt"));
        assert!(!re.is_match("catdog"));
    }

    #[test]
    fn case_sensitive_backref_still_byte_exact() {
        // Regression pin: without `(?i)`, `\1` must stay byte-exact.
        // The old `OpCode::Backref` path is still selected when
        // `case_insensitive` is false, so `(abc)\1` on "ABCabc" must
        // fail — the capture holds "ABC" and the backref needs "ABC"
        // byte-for-byte.
        let re = Regex::compile(r"(abc)\1").unwrap();
        assert!(re.is_match("abcabc"));
        assert!(!re.is_match("ABCabc"));
    }

    #[test]
    fn positive_lookbehind_captures_propagate_to_outer_scope() {
        // PCRE2 testinput1:2305 — `/(?<=(foo))bar\1/` on "foobarfoo"
        // expects match "barfoo". The positive lookbehind captures
        // group 1 = "foo" *inside* its body, and PCRE2 semantics say
        // those captures are visible to subsequent parts of the outer
        // pattern. Before this fix, RGX ran the lookbehind on a clone
        // of the exec context and dropped the clone, so `\1` at the
        // outer level saw an unset group and failed to match.
        let re = Regex::compile(r"(?<=(foo))bar\1").unwrap();
        let m = re.find_first("foobarfoo").expect("expected match");
        assert_eq!((m.start, m.end), (3, 9));
    }

    #[test]
    fn positive_lookahead_captures_propagate_to_outer_scope() {
        // Same rule for lookahead: `(?=(foo))...\1` should let the
        // outer backref see the lookahead's capture. PCRE2:
        //   /(?=(foo))\1bar/ on "foobar" → match "foobar" (group 1 = "foo").
        let re = Regex::compile(r"(?=(foo))\1bar").unwrap();
        let m = re.find_first("foobar").expect("expected match");
        assert_eq!((m.start, m.end), (0, 6));
    }

    #[test]
    fn negative_lookaround_captures_do_not_leak() {
        // Negative lookarounds must NOT leak captures. Regression pin:
        // `(?!(foo))abc(\w*)` — the negative lookahead at the start
        // prevents "foo" from matching group 1 *even transiently*; the
        // outer match proceeds and group 2 captures "xyz". If we were
        // propagating captures from negative-lookaround bodies, group 1
        // could accidentally surface a stale "foo" from an earlier
        // position; we don't, so it stays unset.
        let re = Regex::compile(r"(?!(foo))abc(\w*)").unwrap();
        let caps = re.captures("abcxyz").expect("expected match");
        let overall = caps.get(0).expect("group 0");
        assert_eq!((overall.start(), overall.end()), (0, 6));
        assert!(caps.get(1).is_none(), "group 1 should not be set");
        assert_eq!(caps.get(2).map(|m| m.as_str()), Some("xyz"));
    }

    #[test]
    fn skip_in_failing_assertion_propagates_to_outer_scope() {
        // `(?=b(*SKIP)a)bn|bnn` on "bnn" — PCRE2 no match. The lookahead
        // body matches 'b', fires (*SKIP), then fails at 'a'. PCRE2
        // propagates SKIP's position to the outer match attempt,
        // aborting it at pos 0. Without propagation, alt 2 "bnn" would
        // match at pos 0 (false positive). Mirror of engine fix #28's
        // COMMIT-in-failing-assertion propagation. Verified via
        // testinput1:5630.
        let re = Regex::compile(r"(?=b(*SKIP)a)bn|bnn").unwrap();
        assert!(re.find_first("bnn").is_none());
    }

    #[test]
    fn dupnames_conditional_checks_any_definition() {
        // `(?:a(?<digit>[0-5])|b(?<digit>[4-7]))c(?(<digit>)d|e)` on
        // "a4cd" — PCRE2 matches "a4cd". Two `(?<digit>)` groups
        // share the same name; the `(?(<digit>)...)` conditional
        // must succeed iff EITHER instance captured. Before this
        // fix RGX's HashMap<String, u32> overwrote alt 1's group
        // id with alt 2's, so the conditional checked only alt 2's
        // group and failed when alt 1 matched.
        let re = Regex::compile(r"(?:a(?<digit>[0-5])|b(?<digit>[4-7]))c(?(<digit>)d|e)").unwrap();
        let m = re
            .find_first("a4cd")
            .expect("dupnames conditional must see alt 1's digit group");
        assert_eq!((m.start, m.end), (0, 4));
        // Symmetric: alt 2 captures its digit group; conditional sees it.
        let m = re
            .find_first("b4cd")
            .expect("dupnames conditional must see alt 2's digit group");
        assert_eq!((m.start, m.end), (0, 4));
        // Neither alt captures → no match (outer `(?:a...|b...)` fails).
        assert!(re.find_first("xce").is_none());
    }

    #[test]
    fn dot_under_crlf_rejects_both_ends_of_pair() {
        // PCRE2 `(*CRLF)` rule: `.` matches everything EXCEPT
        //   (a) `\r` immediately followed by `\n` (start of CRLF)
        //   (b) `\n` immediately preceded by `\r` (end of CRLF)
        // Bare `\r`/`\n` (not part of a pair) and any other byte match.
        // Without the lookbehind in case (b), `.+foo` on "\r\nfoo" matches
        // starting at pos 1 (the `\n`) which PCRE2 rejects.
        let re = Regex::compile(r"(*CRLF).+foo").unwrap();
        assert!(re.find_first("\r\nfoo").is_none());

        let re = Regex::compile(r"(*CRLF).+A").unwrap();
        assert!(re.find_first("\r\nA").is_none());

        // Bare `\r` not followed by `\n` is matched by `.`.
        let re = Regex::compile(r"(*CRLF).*").unwrap();
        let m = re.find_first("abc\rdef").expect("expected full match");
        assert_eq!((m.start, m.end), (0, 7));

        // Bare `\n` not preceded by `\r` is matched by `.`.
        let re = Regex::compile(r"(*CRLF).+foo").unwrap();
        let m = re.find_first("\nfoo").expect("expected full match");
        assert_eq!((m.start, m.end), (0, 4));
    }

    #[test]
    fn skip_in_failing_negative_lookahead_absorbs_verb() {
        // Symmetric regression pin: a negative assertion whose body
        // fails must absorb (*SKIP), not leak it. `(?!b(*SKIP)a)bnn`
        // on "bnn" — the negative lookahead's body matches 'b', fires
        // (*SKIP), fails at 'a'. Since body failure = negative assertion
        // success, the outer match proceeds. RGX should then match "bnn"
        // via the outer literal — (*SKIP) must NOT propagate out and
        // abort the match.
        let re = Regex::compile(r"(?!b(*SKIP)a)bnn").unwrap();
        let m = re
            .find_first("bnn")
            .expect("negative lookahead should succeed");
        assert_eq!((m.start, m.end), (0, 3));
    }

    #[test]
    fn extended_mode_toggle_then_scoped_disable_preserves_outer() {
        // After `(?-x: ...)` exits, the outer `(?x)` mode should still
        // apply. Verify by putting literal whitespace in the tail that
        // only x-mode could possibly strip.
        let re = Regex::compile(r"(?x) a (?-x: b ) c ").unwrap();
        // Inside (?x): "a" absorbed, (?-x: b ) requires " b " literal,
        // then "c" absorbed. Overall: /a b c/ (with exactly those spaces
        // around 'b' required by the inner disable).
        assert!(re.is_match("a b c"));
        assert!(!re.is_match("abc")); // inner disable reinstated the spaces
    }

    #[test]
    fn zero_width_plus_iteration_keeps_empty_first_match() {
        // Regression pin: `X+` with a body that can match empty should
        // accept the first zero-width iteration and stop. Previously
        // RGX's PlusGreedy forced the first body match to advance,
        // producing wrong match spans for patterns like
        // `([a]*?)+`. Now: same behavior as `*` for empty bodies.
        let re = Regex::compile(r"([a]*?)+").unwrap();
        let m = re.find_first("a").expect("zero-width match at 0");
        assert_eq!((m.start, m.end), (0, 0));
    }

    #[test]
    fn pcre2_space_includes_vertical_tab() {
        // Regression pin: PCRE2's `\s` matches vertical tab (U+000B),
        // which Rust's `char::is_ascii_whitespace()` excludes. RGX's
        // pcre2_is_space_char helper explicitly includes VT to preserve
        // parity with PCRE2.
        let re = Regex::compile(r"\sabc").unwrap();
        let m = re
            .find_first("\x0babc")
            .expect("\\s must match VT (U+000B)");
        assert_eq!((m.start, m.end), (0, 4));
    }

    #[test]
    fn bare_upper_bound_quantifier_parses_as_zero_to_n() {
        // Regression pin: `{,N}` is PCRE2 shorthand for `{0,N}` (max-only).
        // Previously RGX's PGEN adapter misread the leading-comma
        // `counted_quantifier_body` alternative as `{N,}` (min-only),
        // unbounding `a{,3}B` into an at-least-three match instead of
        // at-most-three. Fixed by probing the body's first leaf
        // terminal — if it's a comma, the sole `digits` child holds
        // the maximum, not the minimum.
        let re = Regex::compile(r"a{,3}B").unwrap();
        let m = re.find_first("B").expect("{,3} accepts zero 'a's");
        assert_eq!((m.start, m.end), (0, 1));
        let m = re.find_first("aaaaB").expect("{,3} caps at three 'a's");
        // Cap at 3 a's + B: span is length 4 (positions 1..5).
        assert_eq!(m.end - m.start, 4);
    }

    #[test]
    fn class_context_simple_escape_accepts_alphabetic_as_literal() {
        // Regression pin: `[\g<a>]+` = `[g<a>]+` per PCRE2 semantics —
        // inside a character class an escaped alphabetic that is not a
        // shorthand (like `\d`) is treated as the literal character.
        let re = Regex::compile(r"^[\g<a>]+").unwrap();
        let m = re
            .find_first("ggg<<<aaa>>>")
            .expect("class-context backslash-g is literal g");
        assert_eq!(m.end - m.start, "ggg<<<aaa>>>".len());
    }

    #[test]
    fn bare_end_of_quote_escape_is_noop() {
        // Regression pin: `\E` without a preceding `\Q` is a no-op per
        // PCRE2. `\Eabc` should compile and match "abc" exactly.
        let re = Regex::compile(r"^\Eabc").unwrap();
        let m = re
            .find_first("abc")
            .expect("\\E is a no-op outside \\Q...\\E");
        assert_eq!((m.start, m.end), (0, 3));
    }

    #[test]
    fn class_context_backslash_b_is_backspace() {
        // Regression pin: inside a character class, `\b` means
        // backspace (0x08), not a word-boundary assertion.
        let re = Regex::compile(r"[\b]").unwrap();
        let m = re
            .find_first("\u{08}x")
            .expect("class-context \\b matches backspace 0x08");
        assert_eq!((m.start, m.end), (0, 1));
    }

    #[test]
    fn case_insensitive_backref_uses_simple_fold() {
        // Regression pin: `(σάμος) \1/i` should match "ΣΆΜΟΣ σάμος"
        // because Σ↔σ and ς↔σ are simple-fold equivalents under PCRE2
        // `/i` semantics. Previously the backref matcher used
        // `char::to_lowercase` which misses ς (final sigma).
        let re = Regex::compile(r"(?i)(σάμος) \1").unwrap();
        assert!(re.find_first("ΣΆΜΟΣ σάμος").is_some());
        assert!(re.find_first("σάμος ΣΆΜΟΣ").is_some());
    }

    #[test]
    fn unicode_property_caret_prefix_negates() {
        // Regression pin: `\p{^Lu}` is PCRE2 in-class negation —
        // equivalent to `\P{Lu}`. Whitespace around the `^` is
        // tolerated.
        let re = Regex::compile(r"^\p{^Lu}+").unwrap();
        assert!(re
            .find_first("abcD")
            .map(|m| m.end - m.start == 3)
            .unwrap_or(false));
        // `\p{  ^ Lu }` should also compile (whitespace variant).
        assert!(Regex::compile(r"^\p{  ^ Lu }+").is_ok());
    }

    #[test]
    fn unicode_property_cs_surrogate_matches_nothing() {
        // Regression pin: `\p{Cs}` (Surrogate) is an empty match set
        // under Rust's `&str` (surrogates are not valid char scalar
        // values). The negated form `\P{Cs}` matches any codepoint.
        let re = Regex::compile(r"\p{Cs}").unwrap();
        assert!(re.find_first("abc\u{1F600}").is_none());
        let re_neg = Regex::compile(r"^\P{Cs}+$").unwrap();
        assert!(re_neg.is_match("abc\u{1F600}"));
    }

    #[test]
    fn callout_with_backtick_body_compiles_as_noop() {
        // Regression pin: pcre2test accepts ` ` ' { % # $ ^ ` as
        // callout string delimiters. RGX treats the body as opaque
        // and lowers to a no-op callout regardless of delimiter.
        assert!(Regex::compile(r"(?C`code`)abc")
            .unwrap()
            .find_first("abc")
            .is_some());
        assert!(Regex::compile(r"(?C%text%)abc")
            .unwrap()
            .find_first("abc")
            .is_some());
    }

    #[test]
    fn callouts_compile_as_noops_by_default() {
        // Regression pin: PCRE2 callouts `(?C)`, `(?Cn)`, `(?C"text")`,
        // `(?C'text')` compile and behave as no-ops when no callback
        // handler is registered — matching PCRE2's default
        // "unregistered callout is a no-op" policy. Previously RGX
        // rejected string callouts at compile time and lowered numeric
        // callouts to a native-callback code block that failed at match
        // time when no handler was wired.
        assert!(Regex::compile(r"abc(?C)def")
            .unwrap()
            .find_first("abcdef")
            .is_some());
        assert!(Regex::compile(r"abc(?C1)def")
            .unwrap()
            .find_first("abcdef")
            .is_some());
        assert!(Regex::compile(r#"(?C"hello")abc"#)
            .unwrap()
            .find_first("abc")
            .is_some());
        assert!(Regex::compile(r"(?C'mark')\d")
            .unwrap()
            .find_first("x42")
            .is_some());
    }

    #[test]
    fn optional_empty_capture_is_visible_to_conditional() {
        // Regression pin: `()?(?(1)a|b)` on "a" should match "a" —
        // the `()?` greedy-tries the match branch, capturing group 1
        // as empty, and the conditional `(?(1)a|b)` sees group 1
        // participated and takes the `a` branch. Previously RGX
        // treated zero-width `?` matches as "didn't match" and undid
        // the capture trail, so the conditional fell through to `b`
        // and the pattern failed on "a".
        let re = Regex::compile(r"()?(?(1)a|b)").unwrap();
        assert!(re.find_first("a").is_some(), "()?(?(1)a|b) must match 'a'");
        assert!(
            re.find_first("b").is_some(),
            "()?(?(1)a|b) must still match 'b'"
        );
        assert!(
            re.find_first("c").is_none(),
            "()?(?(1)a|b) must not match 'c'"
        );
    }

    #[test]
    fn line_anchors_honour_newline_pragma_under_m() {
        // Regression pin: under `/m`, `^` and `$` treat as line
        // boundaries the newline set selected by the PCRE2 pragma.
        // `(*CR)(?m)^b` on "a\rb" matches because CR is the newline;
        // on "a\nb" it doesn't because `\n` isn't a newline under CR.
        let cr = Regex::compile(r"(*CR)(?m)^b").unwrap();
        assert!(cr.is_match("a\rb"));
        assert!(!cr.is_match("a\nb"));
        // `(*ANYCRLF)` accepts either CR or LF at line boundaries.
        let any_crlf = Regex::compile(r"(*ANYCRLF)(?m)^b").unwrap();
        assert!(any_crlf.is_match("a\rb"));
        assert!(any_crlf.is_match("a\nb"));
        // Default `Lf` mode is preserved when no pragma is given.
        let default = Regex::compile(r"(?m)^b").unwrap();
        assert!(default.is_match("a\nb"));
        assert!(!default.is_match("a\rb"));
    }

    #[test]
    fn newline_pragmas_change_dot_exclusion_set() {
        // Regression pin: PCRE2 `(*CR)` / `(*LF)` / `(*CRLF)` /
        // `(*ANYCRLF)` / `(*ANY)` / `(*NUL)` pragmas change the
        // newline convention — which characters `.` / `\N` refuse
        // to match. Default is `(*LF)` (only `\n` excluded).
        // `(*CR)` mode: only `\r` excluded, so `.` matches `\n`.
        let cr = Regex::compile(r"(*CR)^a.b").unwrap();
        assert!(cr.is_match("a\nb"));
        assert!(!cr.is_match("a\rb"));
        // `(*ANY)` mode: all Unicode newlines excluded.
        let any = Regex::compile(r"(*ANY)^a.b").unwrap();
        assert!(!any.is_match("a\nb"));
        assert!(!any.is_match("a\u{85}b"));
        assert!(any.is_match("aXb"));
        // `(*NUL)` mode: only `\0` excluded.
        let nul = Regex::compile(r"(*NUL)^a.b").unwrap();
        assert!(!nul.is_match("a\0b"));
        assert!(nul.is_match("a\nb"));
        // `\N` mirrors `.`: same exclusion set.
        let cr_n = Regex::compile(r"(*CR)a\Nb").unwrap();
        assert!(cr_n.is_match("a\nb"));
        assert!(!cr_n.is_match("a\rb"));
    }

    #[test]
    fn bsr_anycrlf_restricts_backslash_r() {
        // Regression pin: PCRE2 `(*BSR_ANYCRLF)` limits `\R` to
        // CR / LF / CRLF. The default `BSR_UNICODE` mode (also
        // explicitly via `(*BSR_UNICODE)`) matches VT, FF, NEL, LS,
        // PS in addition. Last-wins between multiple pragmas.
        let any = Regex::compile(r"(*BSR_ANYCRLF)a\Rb").unwrap();
        assert!(any.is_match("a\rb"));
        assert!(any.is_match("a\nb"));
        assert!(any.is_match("a\r\nb"));
        assert!(!any.is_match("a\u{85}b"));
        assert!(!any.is_match("a\u{0B}b"));
        // Default mode — Unicode newline set.
        let uni = Regex::compile(r"a\Rb").unwrap();
        assert!(uni.is_match("a\u{85}b"));
        assert!(uni.is_match("a\u{2028}b"));
        // Last-wins when both pragmas appear.
        let switched = Regex::compile(r"(*BSR_UNICODE)(*BSR_ANYCRLF)a\Rb").unwrap();
        assert!(!switched.is_match("a\u{85}b"));
    }

    #[test]
    fn ungreedy_flag_swaps_quantifier_greediness() {
        // Regression pin: PCRE2 `(?U)` (and pcre2test `/ungreedy`)
        // inverts greedy/lazy defaults — `*` becomes lazy, `*?` becomes
        // greedy, same for `+`, `?`, and `{n,m}`.
        assert_eq!(
            Regex::compile(r"(?U)<.*>")
                .unwrap()
                .find("abc<def>ghi<klm>nop")
                .map(|m| m.as_str().to_string()),
            Some("<def>".to_string())
        );
        assert_eq!(
            Regex::compile(r"(?U)={3,}")
                .unwrap()
                .find("abc========def")
                .map(|m| m.as_str().to_string()),
            Some("===".to_string())
        );
        // `(?U)<.*?>` — inside the U region the `?` becomes greedy so
        // `<.*?>` matches the longest bracketed run.
        assert_eq!(
            Regex::compile(r"(?U)<.*?>")
                .unwrap()
                .find("abc<def>ghi<klm>nop")
                .map(|m| m.as_str().to_string()),
            Some("<def>ghi<klm>".to_string())
        );
        // Without `(?U)`, default greediness is preserved.
        assert_eq!(
            Regex::compile(r"<.*>")
                .unwrap()
                .find("abc<def>ghi<klm>nop")
                .map(|m| m.as_str().to_string()),
            Some("<def>ghi<klm>".to_string())
        );
    }

    #[test]
    fn horizontal_whitespace_includes_mongolian_vowel_separator() {
        // Regression pin: PCRE2's `\h` keeps U+180E (MONGOLIAN VOWEL
        // SEPARATOR) in the horizontal-whitespace set for backward
        // compatibility, even though Unicode 6.3 removed it from the
        // `White_Space` property. testinput5:292 `[\h]{3,}/B` expects
        // the run starting at U+1680 to include U+180E.
        let re = Regex::compile(r"\h").unwrap();
        assert!(re.is_match("\u{180e}"));
        assert!(re.is_match("\u{1680}"));
        assert!(re.is_match("\u{2000}"));
    }

    #[test]
    fn xsp_xps_expand_to_unicode_whitespace() {
        // Regression pin: PCRE2's `\p{Xsp}` / `\p{Xps}` (Perl-style
        // whitespace) include `\p{Z}` (Zs+Zl+Zp) in addition to the
        // C0 controls HT/LF/VT/FF/CR. testinput5:1054 expects
        // `/^>\p{Xsp}+/utf` to match NBSP, OGHAM SPACE MARK, LINE
        // SEPARATOR, etc.
        let xsp = Regex::compile(r"\p{Xsp}").unwrap();
        assert!(xsp.is_match("\u{00A0}")); // NBSP — Zs
        assert!(xsp.is_match("\u{1680}")); // OGHAM SPACE MARK — Zs
        assert!(xsp.is_match("\u{2028}")); // LINE SEPARATOR — Zl
        assert!(xsp.is_match("\u{2029}")); // PARAGRAPH SEPARATOR — Zp
        assert!(xsp.is_match("\t"));
        // Xwd includes Unicode letters + numbers + underscore.
        let xwd = Regex::compile(r"\p{Xwd}").unwrap();
        assert!(xwd.is_match("ζ")); // Greek letter
        assert!(xwd.is_match("٠")); // Arabic digit
        assert!(xwd.is_match("_"));
    }

    #[test]
    fn quantifier_retargets_across_transparent_atoms() {
        // Regression pin: PCRE2 treats `(?#...)` comments and /x-mode
        // whitespace as transparent for quantifier attachment. A
        // quantifier that PGEN parses as applying to the transparent
        // filler (Empty for comments, WhitespaceLiteral for /x
        // whitespace) must be transferred to the preceding real
        // atom at compile time, matching PCRE2's effective pattern.
        // `^a(?#xxx){3}c` = `^a{3}c` = match "aaac".
        let re = Regex::compile(r"^a(?#xxx){3}c").unwrap();
        assert!(re.is_match("aaac"));
        let re = Regex::compile(r"(?x)b *c").unwrap();
        assert!(re.is_match("bbbc"));
        // Multiple comments between atom and quantifier also retarget.
        let re = Regex::compile(r"^a(?#a)(?#b){3}c").unwrap();
        assert!(re.is_match("aaac"));
    }

    #[test]
    fn case_distinguished_property_expands_under_i() {
        // Regression pin: under /i, PCRE2 expands every case-
        // distinguished Unicode property to its case-fold closure —
        // {Lu, Ll, Lt, L&, Lc, Cased_Letter} → L& and the boolean
        // {Upper, Lower, Title, Cased} → Cased (pcre2pattern(3)
        // lines 980-985). The closure applies to both `\p{X}` and
        // `\P{X}` forms, both standalone and inside a character
        // class, dispatched uniformly via
        // `unicode_support::case_fold_property_closure`.

        // === General-category letter triple: closure is L& ===
        for prop in &["Lu", "Ll", "Lt"] {
            let pos = Regex::compile(&format!("(?i)\\p{{{prop}}}")).unwrap();
            assert!(pos.is_match("a"), "/i \\p{{{prop}}} should match 'a'");
            assert!(pos.is_match("A"), "/i \\p{{{prop}}} should match 'A'");
            assert!(pos.is_match("ǅ"), "/i \\p{{{prop}}} should match Lt 'ǅ'");
            assert!(!pos.is_match("1"), "/i \\p{{{prop}}} should not match '1'");

            let neg = Regex::compile(&format!("(?i)\\P{{{prop}}}")).unwrap();
            assert!(!neg.is_match("a"), "/i \\P{{{prop}}} should not match 'a'");
            assert!(!neg.is_match("A"), "/i \\P{{{prop}}} should not match 'A'");
            assert!(
                !neg.is_match("ǅ"),
                "/i \\P{{{prop}}} should not match Lt 'ǅ'"
            );
            assert!(neg.is_match("1"), "/i \\P{{{prop}}} should match '1'");
        }

        // === Boolean case-distinction properties: closure is Cased ===
        // (Title is a general-category value Lt above, not a
        // standalone boolean property — PCRE2 / regex_syntax reject
        // `\p{Title}` for the same reason.)
        for prop in &["Upper", "Lower", "Cased"] {
            let pos = Regex::compile(&format!("(?i)\\p{{{prop}}}")).unwrap();
            assert!(pos.is_match("a"), "/i \\p{{{prop}}} should match Cased 'a'");
            assert!(pos.is_match("A"), "/i \\p{{{prop}}} should match Cased 'A'");
            assert!(!pos.is_match("1"), "/i \\p{{{prop}}} should not match '1'");

            let neg = Regex::compile(&format!("(?i)\\P{{{prop}}}")).unwrap();
            assert!(!neg.is_match("a"), "/i \\P{{{prop}}} should not match 'a'");
            assert!(!neg.is_match("A"), "/i \\P{{{prop}}} should not match 'A'");
            assert!(neg.is_match("1"), "/i \\P{{{prop}}} should match '1'");
        }

        // === Without /i, the original property semantic survives ===
        let upper = Regex::compile(r"\p{Lu}").unwrap();
        assert!(!upper.is_match("a"));
        assert!(upper.is_match("A"));
        let upper_bool = Regex::compile(r"\p{Upper}").unwrap();
        assert!(!upper_bool.is_match("a"));
        assert!(upper_bool.is_match("A"));

        // === Inside a class — same closure rule applies ===
        let mixed = Regex::compile(r"(?i)[\p{Upper}\d]").unwrap();
        assert!(mixed.is_match("a"));
        assert!(mixed.is_match("5"));
        assert!(!mixed.is_match("א")); // Lo, not cased
        let neg_class = Regex::compile(r"(?i)[\P{Lu}]").unwrap();
        assert!(!neg_class.is_match("a"));
        assert!(neg_class.is_match("1"));
        assert!(neg_class.is_match("א"));
    }

    #[test]
    fn ucp_graph_includes_format_and_private_use() {
        // Regression pin: PCRE2's `[:graph:]` under UCP matches
        // Cf (format) and Co (private-use) codepoints even though
        // pcre2pattern(3) describes the set as L+M+N+P+S. The
        // implementation behavior is the source of truth — testinput4
        // line 3422 expects `[[:graph:]]+$/utf,ucp` to match
        // `Cf-property:\x{ad}\x{600}…` (Cf chars).
        let re = Regex::compile(r"(*UCP)^[[:graph:]]+$").unwrap();
        assert!(re.is_match("\u{200b}\u{200c}\u{200d}")); // Cf
        assert!(re.is_match("\u{e000}")); // Co (private-use)
        assert!(re.is_match("Letter:ABC")); // L + P
    }

    #[test]
    fn g_bracketed_is_subroutine_call_not_backref() {
        // Regression pin: `\g<name>`, `\g<N>`, `\g<+N>`, `\g<-N>`,
        // `\g'name'`, `\g'N'` are **subroutine calls** per PCRE2
        // (re-execute the named/numbered group). `\g{name}`, `\g{N}`,
        // and `\gN` remain back-references.
        // Previously RGX lowered every `\g` form to NamedBackreference,
        // which misfired on self-recursive forms like
        // `^(?<name>a|b\g<name>c)` where the group hasn't captured
        // when the reference is reached.
        let recursive = Regex::compile(r"^(?<name>a|b\g<name>c)").unwrap();
        assert!(recursive.is_match("bac"));
        assert!(recursive.is_match("bbacc"));
        assert!(recursive.is_match("bbbaccc"));
        // Numeric subroutine call.
        let numeric = Regex::compile(r"^(a|b\g<1>c)").unwrap();
        assert!(numeric.is_match("bac"));
        assert!(numeric.is_match("bbacc"));
        // `\g{N}` stays a back-reference and fails the recursive
        // pattern because the group hasn't captured yet.
        let brace = Regex::compile(r"^(a|b\g{1}c)").unwrap();
        assert!(!brace.is_match("bac"));
        // Ordinary back-reference usage keeps working.
        let backref = Regex::compile(r"(\w+) \g{1}").unwrap();
        assert!(backref.is_match("hello hello"));
    }

    #[test]
    fn substitute_template_strips_length_hint_prefix() {
        // Regression pin: PCRE2 accepts a leading `[N]` in a
        // substitute template as an advisory output-buffer size hint.
        // The prefix is stripped before the actual substitution, so
        // `[10]XYZ` emits `XYZ` not `[10]XYZ`.
        let re = Regex::compile(r"abc").unwrap();
        assert_eq!(re.replace("123abc123", r"[10]XYZ"), "123XYZ123");
        // Guard: only `[digits]` stripped — `[abc]` or unclosed `[`
        // stay literal so character-class-looking templates still
        // round-trip.
        assert_eq!(re.replace("abc", r"[abc]"), "[abc]");
        assert_eq!(re.replace("abc", r"[10XYZ"), "[10XYZ");
    }

    #[test]
    fn substitute_template_mark_name() {
        // Regression pin: `${*MARK}` / `$*MARK` in a replacement
        // template expand to the name of the last `(*MARK:name)` or
        // `(*:name)` verb hit on the winning match path, per PCRE2
        // substitute semantics. Absent a mark, the expansion is empty.
        let re = Regex::compile(r"(*:pear)apple|(*:orange)lemon|(*:strawberry)blackberry").unwrap();
        assert_eq!(
            re.replace_all("apple lemon blackberry", r"${*MARK}"),
            "pear orange strawberry"
        );
        assert_eq!(re.replace("apple", r"<$*MARK>"), "<pear>");
        // No mark on the match path → empty expansion.
        let plain = Regex::compile(r"abc").unwrap();
        assert_eq!(plain.replace("abc", r"<${*MARK}>"), "<>");
        // `Captures::mark()` also exposes the mark name.
        let caps = re.captures("apple").unwrap();
        assert_eq!(caps.mark(), Some("pear"));
    }

    #[test]
    fn substitute_template_single_digit_is_backref() {
        // Regression pin: `\N` (N=1..9) in a replacement template is
        // a capture-group back-reference (Perl / PCRE2 convention).
        // Only `\0...` routes to the octal escape — the leading zero
        // marks the numeric literal form.
        let re = Regex::compile(r"a(b)(c)").unwrap();
        assert_eq!(re.replace("abc", r">\1<"), ">b<");
        assert_eq!(re.replace("abc", r">\2<"), ">c<");
        // `\0` still decodes as NUL (octal 0).
        assert_eq!(re.replace("abc", r">\0<"), ">\0<");
        // `\045` still decodes as octal ('%' — 0x25).
        assert_eq!(re.replace("abc", r">\045<"), ">%<");
    }

    #[test]
    fn substitute_template_backslash_escapes() {
        // Regression pin: Replacement templates honour Perl-style
        // backslash escapes — control chars, octal, hex, case change.
        let re = Regex::compile(r"a").unwrap();
        let out = re.replace_all("aaa", r"\n");
        assert_eq!(out, "\n\n\n");
        let out = re.replace_all("a", r"\045");
        assert_eq!(out, "%"); // octal 45 = '%'
        let out = re.replace_all("a", r"\o{45}");
        assert_eq!(out, "%");
        let out = re.replace_all("a", r"\x{25}");
        assert_eq!(out, "%");
        let out = re.replace_all("a", r"\\");
        assert_eq!(out, "\\");
        let out = re.replace_all("a", r"\$");
        assert_eq!(out, "$");
    }

    #[test]
    fn substitute_template_case_change() {
        // Regression pin: `\u`, `\l`, `\U`, `\L`, `\E` change case of
        // subsequent template content, matching PCRE2 semantics.
        let re = Regex::compile(r"(\w+)").unwrap();
        let out = re.replace("hello", r"\u$1");
        assert_eq!(out, "Hello");
        let out = re.replace("HELLO", r"\l$1");
        assert_eq!(out, "hELLO");
        let out = re.replace("MixedCase", r"\U$1\E done");
        assert_eq!(out, "MIXEDCASE done");
        let out = re.replace("MixedCase", r"\L$1\E done");
        assert_eq!(out, "mixedcase done");
    }

    #[test]
    fn ucp_pragma_unicodefies_shorthand_classes() {
        // Regression pin: `(*UCP)` switches \d / \w / \s to
        // Unicode-property-backed character classes.
        let re = Regex::compile(r"(*UCP)^\d+").unwrap();
        // Arabic digit zero (U+0660) and Tamil digit nine (U+0BEF)
        // are \p{Nd} — they should match under UCP.
        assert!(re.find_first("1\u{0660}\u{0BEF}").is_some());
        // Without UCP, \d is ASCII-only.
        let ascii = Regex::compile(r"^\d+").unwrap();
        let m = ascii.find_first("1\u{0660}").expect("ASCII 1 matches");
        assert_eq!(m.end - m.start, 1);

        let re = Regex::compile(r"(*UCP)^\w+").unwrap();
        // Greek letter ζ (U+03B6) and Arabic digit ٠ (U+0660) are both
        // in L/N — UCP \w should cover them.
        let m = re
            .find_first("Az_\u{03B6}\u{0660}_")
            .expect("UCP \\w matches Unicode letters and digits");
        let subj = "Az_\u{03B6}\u{0660}_";
        assert_eq!(m.end - m.start, subj.len());

        let re = Regex::compile(r"(*UCP)^>\s+").unwrap();
        // OGHAM SPACE MARK (U+1680) is \p{Zs} / White_Space — UCP \s should match.
        assert!(re.find_first("> \u{1680}\t").is_some());
    }

    #[test]
    fn ucp_pragma_unicodefies_posix_classes() {
        // Regression pin: `(*UCP)` widens POSIX character classes to
        // their Unicode-aware counterparts.
        let re = Regex::compile(r"(*UCP)^[[:alpha:]]+").unwrap();
        let m = re
            .find_first("Azζ\u{0660}")
            .expect("UCP [[:alpha:]] matches Unicode letters");
        // A=1, z=1, ζ=2 bytes. Arabic zero U+0660 is Nd (not a letter),
        // so the class should exclude it.
        assert_eq!(m.end - m.start, "Azζ".len());

        let re = Regex::compile(r"(*UCP)^[[:digit:]]+").unwrap();
        let m = re
            .find_first("1\u{0660}abc")
            .expect("UCP [[:digit:]] matches Arabic digit");
        assert_eq!(m.end - m.start, "1\u{0660}".len());
    }

    #[test]
    fn runtime_policy_verbs_compile_as_noops() {
        // Regression pin: PCRE2 runtime/policy directives like
        // (*NOTEMPTY), (*NO_JIT), (*LIMIT_HEAP=N) are parsed and
        // compiled as no-ops — they set execution hints that don't
        // affect the match on well-formed subjects.
        for pattern in &["(*NOTEMPTY)abc", "(*NO_JIT)abc", "(*NO_START_OPT)abc"] {
            let re = Regex::compile(pattern)
                .unwrap_or_else(|e| panic!("{pattern:?} should compile: {e}"));
            assert!(re.find_first("abc").is_some(), "{pattern:?} should match");
        }
    }

    #[test]
    fn nonempty_quantifier_body_still_advances() {
        // Sanity: the fix for empty-body quantifier termination must
        // not break quantifiers whose body actually matches characters.
        // `a*` on "aaab" must still consume the three 'a's greedily.
        let re = Regex::compile(r"a*").unwrap();
        let m = re.find_first("aaab").expect("greedy consumes");
        assert_eq!((m.start, m.end), (0, 3));

        let re = Regex::compile(r"a+").unwrap();
        let m = re.find_first("aaab").expect("plus consumes");
        assert_eq!((m.start, m.end), (0, 3));
    }

    // ======================================================================
    // \K (Match Reset) tests
    // ======================================================================

    #[test]
    fn match_reset_basic() {
        let re = Regex::compile(r"foo\Kbar").unwrap();
        let m = re.find_first("foobar").unwrap();
        assert_eq!((m.start, m.end), (3, 6)); // reports "bar" not "foobar"
    }

    #[test]
    fn match_reset_no_match_without_prefix() {
        let re = Regex::compile(r"foo\Kbar").unwrap();
        assert!(!re.is_match("bar")); // "foo" prefix still required
    }

    #[test]
    fn match_reset_in_longer_text() {
        let re = Regex::compile(r"foo\Kbar").unwrap();
        let m = re.find_first("xxfoobarxx").unwrap();
        assert_eq!((m.start, m.end), (5, 8)); // "bar" within "xxfoobarxx"
    }

    #[test]
    fn match_reset_find_all() {
        let re = Regex::compile(r"foo\Kbar").unwrap();
        let all = re.find_all("foobar foobar");
        assert_eq!(all.len(), 2);
        assert_eq!((all[0].start, all[0].end), (3, 6));
        assert_eq!((all[1].start, all[1].end), (10, 13));
    }

    // ======================================================================
    // \R (Newline Sequence) tests
    // ======================================================================

    #[test]
    fn newline_sequence_crlf() {
        let re = Regex::compile(r"\R").unwrap();
        let m = re.find_first("\r\n").unwrap();
        assert_eq!((m.start, m.end), (0, 2)); // CRLF is one \R match
    }

    #[test]
    fn newline_sequence_lf() {
        let re = Regex::compile(r"\R").unwrap();
        assert!(re.is_match("\n"));
    }

    #[test]
    fn newline_sequence_cr() {
        let re = Regex::compile(r"\R").unwrap();
        let m = re.find_first("\r").unwrap();
        assert_eq!((m.start, m.end), (0, 1));
    }

    #[test]
    fn newline_sequence_vertical_tab() {
        let re = Regex::compile(r"\R").unwrap();
        assert!(re.is_match("\x0B"));
    }

    #[test]
    fn newline_sequence_form_feed() {
        let re = Regex::compile(r"\R").unwrap();
        assert!(re.is_match("\x0C"));
    }

    #[test]
    fn newline_sequence_nel() {
        let re = Regex::compile(r"\R").unwrap();
        assert!(re.is_match("\u{0085}"));
    }

    #[test]
    fn newline_sequence_line_separator() {
        let re = Regex::compile(r"\R").unwrap();
        assert!(re.is_match("\u{2028}"));
    }

    #[test]
    fn newline_sequence_paragraph_separator() {
        let re = Regex::compile(r"\R").unwrap();
        assert!(re.is_match("\u{2029}"));
    }

    #[test]
    fn newline_sequence_find_all() {
        let re = Regex::compile(r"\R").unwrap();
        let all = re.find_all("a\r\nb\nc");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn newline_sequence_no_match_on_regular_text() {
        let re = Regex::compile(r"\R").unwrap();
        assert!(!re.is_match("abc"));
    }

    #[test]
    fn non_newline_escape() {
        let re = Regex::compile(r"\N+").unwrap();
        let m = re.find_first("abc\ndef").unwrap();
        assert_eq!((m.start, m.end), (0, 3)); // stops at \n
    }

    #[test]
    fn non_newline_does_not_match_newline() {
        let re = Regex::compile(r"\N").unwrap();
        assert!(!re.is_match("\n"));
        assert!(re.is_match("x"));
    }

    #[test]
    fn fail_verb_causes_no_match() {
        let re = Regex::compile("a(*FAIL)").unwrap();
        assert!(!re.is_match("a"));
    }

    #[test]
    fn fail_verb_in_alternation() {
        let re = Regex::compile("a(*FAIL)|b").unwrap();
        assert!(re.is_match("b"));
        assert!(!re.is_match("a"));
    }

    // ======================================================================
    // \G (Previous Match End Anchor) tests
    // ======================================================================

    #[test]
    fn previous_match_end_anchor_find_all_contiguous() {
        // Classic tokenizer: \G\w+\s* matches contiguous word+space tokens
        let re = Regex::compile(r"\G\w+\s*").unwrap();
        let all = re.find_all("hello world foo");
        assert_eq!(all.len(), 3);
        assert_eq!(&"hello world foo"[all[0].start..all[0].end], "hello ");
        assert_eq!(&"hello world foo"[all[1].start..all[1].end], "world ");
        assert_eq!(&"hello world foo"[all[2].start..all[2].end], "foo");
    }

    #[test]
    fn previous_match_end_anchor_stops_at_gap() {
        // \G\d+ on "123 456" should only match "123" because the space
        // creates a gap where \G fails.
        let re = Regex::compile(r"\G\d+").unwrap();
        let all = re.find_all("123 456");
        assert_eq!(all.len(), 1);
        assert_eq!(&"123 456"[all[0].start..all[0].end], "123");
    }

    #[test]
    fn previous_match_end_anchor_find_first_at_start() {
        // For find_first, \G matches at position 0
        let re = Regex::compile(r"\Gabc").unwrap();
        let m = re.find_first("abcdef").unwrap();
        assert_eq!((m.start, m.end), (0, 3));
    }

    #[test]
    fn previous_match_end_anchor_find_first_no_match_not_at_start() {
        // \G only matches at position 0 for find_first, so "xxabc" fails
        let re = Regex::compile(r"\Gabc").unwrap();
        assert!(re.find_first("xxabc").is_none());
    }

    #[test]
    fn previous_match_end_anchor_alternation() {
        // \G can be used with alternation
        let re = Regex::compile(r"\G(?:\d+|\w+)\s*").unwrap();
        let all = re.find_all("abc 123 xyz");
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn previous_match_end_anchor_empty_input() {
        let re = Regex::compile(r"\G\w+").unwrap();
        let all = re.find_all("");
        assert!(all.is_empty());
    }

    // ======================================================================
    // (?C) Callout tests
    // ======================================================================

    #[test]
    fn callout_default_is_noop() {
        // (?C) with no registered callout should be a no-op (match succeeds)
        let re = Regex::with_mode("a(?C)b", ExecutionMode::Full).unwrap();
        assert!(re.is_match("ab"));
    }

    #[test]
    fn callout_numbered_is_noop_when_unregistered() {
        // (?C123) with no registered callout should be a no-op
        let re = Regex::with_mode("a(?C123)b", ExecutionMode::Full).unwrap();
        assert!(re.is_match("ab"));
    }

    #[test]
    fn callout_registered_handler_called() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let re = Regex::with_mode("a(?C)b", ExecutionMode::Full).unwrap();
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        re.register_callout(0, move |_ctx| {
            cc.fetch_add(1, Ordering::SeqCst);
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("ab"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn callout_numbered_handler_called() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let re = Regex::with_mode("a(?C42)b", ExecutionMode::Full).unwrap();
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        re.register_callout(42, move |_ctx| {
            cc.fetch_add(1, Ordering::SeqCst);
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("ab"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn callout_failure_prevents_match() {
        let re = Regex::with_mode("a(?C)b", ExecutionMode::Full).unwrap();
        re.register_callout(0, |_ctx| ExecResult::Failure).unwrap();
        assert!(!re.is_match("ab"));
    }

    #[test]
    fn callout_in_find_all() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let re = Regex::with_mode(r"\w+(?C)", ExecutionMode::Full).unwrap();
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        re.register_callout(0, move |_ctx| {
            cc.fetch_add(1, Ordering::SeqCst);
            ExecResult::Success
        })
        .unwrap();
        let all = re.find_all("abc def ghi");
        assert_eq!(all.len(), 3);
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    // ====================================================================
    // Layer 3 — Match Steering tests
    // ====================================================================

    #[test]
    fn steer_continue_acts_like_success() {
        let re = Regex::with_mode(r"(?<x>cat)(?{native:check})", ExecutionMode::Full).unwrap();
        re.register_native("check", |_ctx| ExecResult::Steer(SteerResult::Continue))
            .unwrap();
        assert!(re.is_match("cat"));
    }

    #[test]
    fn steer_fail_acts_like_failure() {
        let re = Regex::with_mode(r"cat(?{native:reject})", ExecutionMode::Full).unwrap();
        re.register_native("reject", |_ctx| ExecResult::Steer(SteerResult::Fail))
            .unwrap();
        assert!(!re.is_match("cat"));
    }

    #[test]
    fn steer_accept_forces_match() {
        let re = Regex::with_mode(r"cat(?{native:accept_now})dog", ExecutionMode::Full).unwrap();
        re.register_native("accept_now", |_ctx| ExecResult::Steer(SteerResult::Accept))
            .unwrap();
        // Should match "cat" even though "dog" hasn't been seen
        let m = re.find_first("catdog").unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3); // ends at position after "cat", before "dog"
    }

    #[test]
    fn steer_skip_advances_position() {
        let re = Regex::with_mode(r"(?{native:skip3})abc", ExecutionMode::Full).unwrap();
        re.register_native("skip3", |_ctx| ExecResult::Steer(SteerResult::Skip(3)))
            .unwrap();
        // Pattern starts at pos 0, skip3 advances to pos 3, then "abc" tries from pos 3
        let m = re.find_first("xxxabc").unwrap();
        assert_eq!((m.start, m.end), (0, 6));
    }

    #[test]
    fn steer_abort_stops_search() {
        let re = Regex::with_mode(r"cat(?{native:abort_search})", ExecutionMode::Full).unwrap();
        re.register_native("abort_search", |_ctx| ExecResult::Steer(SteerResult::Abort))
            .unwrap();
        // "cat" matches but abort prevents the match from being reported
        // AND prevents trying further positions
        assert!(!re.is_match("cat dog cat"));
    }

    #[test]
    fn event_observer_receives_match_attempt_events() {
        use std::sync::{Arc, Mutex};
        // Use a non-literal pattern so the VM path is exercised (pure literals
        // bypass the VM via memmem and therefore skip event emission).
        let re = Regex::compile(r"c.t").unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        re.on_event(move |event| {
            events_clone.lock().unwrap().push(event.clone());
        })
        .unwrap();
        re.find_first("dog cat");
        let collected = events.lock().unwrap();
        // Should have MatchAttemptStarted and MatchAttemptCompleted events
        assert!(collected
            .iter()
            .any(|e| matches!(e, MatchEvent::MatchAttemptStarted { .. })));
        assert!(collected
            .iter()
            .any(|e| matches!(e, MatchEvent::MatchAttemptCompleted { matched: true, .. })));
    }

    #[test]
    fn event_observer_zero_overhead_when_none() {
        // Just verify no crash/overhead when no observer is set
        let re = Regex::compile("cat").unwrap();
        assert!(re.is_match("cat"));
    }

    #[test]
    fn event_observer_receives_backtrack_events() {
        use std::sync::{Arc, Mutex};
        let re = Regex::compile("a*ab").unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        re.on_event(move |event| {
            events_clone.lock().unwrap().push(event.clone());
        })
        .unwrap();
        re.find_first("aab");
        let collected = events.lock().unwrap();
        assert!(collected
            .iter()
            .any(|e| matches!(e, MatchEvent::BacktrackOccurred { .. })));
    }

    // ====================================================================
    // LAYER 5: ASYNC / CONTINUATION-PASSING TESTS
    // ====================================================================

    #[test]
    fn suspendable_completes_without_async_callbacks() {
        let re = Regex::compile("cat").unwrap();
        match re.find_first_suspendable("hello cat") {
            MatchOutcome::Completed(Some(m)) => assert_eq!((m.start, m.end), (6, 9)),
            MatchOutcome::Completed(None) => panic!("expected a match, got None"),
            MatchOutcome::Suspended(_) => panic!("expected completed match, got suspension"),
        }
    }

    #[test]
    fn suspendable_no_match_completes() {
        let re = Regex::compile("dog").unwrap();
        match re.find_first_suspendable("hello cat") {
            MatchOutcome::Completed(None) => {} // expected
            MatchOutcome::Completed(Some(m)) => {
                panic!("expected no match, got {}..{}", m.start, m.end)
            }
            MatchOutcome::Suspended(_) => panic!("expected completed, got suspension"),
        }
    }

    #[test]
    fn suspendable_suspends_on_unregistered_native() {
        let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
        // Don't register "check" — it should suspend
        match re.find_first_suspendable("cat") {
            MatchOutcome::Suspended(cont) => {
                assert_eq!(cont.pending_callback_name, "check");
                // Resume with success
                match re.resume(*cont, ExecResult::Success) {
                    MatchOutcome::Completed(Some(m)) => {
                        assert_eq!((m.start, m.end), (0, 3));
                    }
                    other => panic!("expected completed match after resume, got {:?}", other),
                }
            }
            other => panic!("expected suspension, got {:?}", other),
        }
    }

    #[test]
    fn suspendable_resume_with_failure_backtracks() {
        let re = Regex::with_mode(r"cat(?{native:check})|dog", ExecutionMode::Full).unwrap();
        match re.find_first_suspendable("catdog") {
            MatchOutcome::Suspended(cont) => {
                assert_eq!(cont.pending_callback_name, "check");
                // Resume with failure — should backtrack and find "dog"
                match re.resume(*cont, ExecResult::Failure) {
                    MatchOutcome::Completed(Some(m)) => {
                        assert_eq!((m.start, m.end), (3, 6));
                    }
                    other => panic!("expected dog match after check failure, got {:?}", other),
                }
            }
            other => panic!("expected suspension, got {:?}", other),
        }
    }

    #[test]
    fn suspendable_registered_callback_does_not_suspend() {
        let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
        // Register the callback — should NOT suspend
        re.register_native("check", |_ctx| ExecResult::Success)
            .unwrap();
        match re.find_first_suspendable("cat") {
            MatchOutcome::Completed(Some(m)) => {
                assert_eq!((m.start, m.end), (0, 3));
            }
            MatchOutcome::Completed(None) => panic!("expected match"),
            MatchOutcome::Suspended(_) => {
                panic!("should not suspend when callback is registered")
            }
        }
    }

    #[test]
    fn suspendable_resume_with_replacement_value() {
        let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
        match re.find_first_suspendable("cat") {
            MatchOutcome::Suspended(cont) => {
                match re.resume(*cont, ExecResult::Replacement("kitten".to_string())) {
                    MatchOutcome::Completed(Some(m)) => {
                        assert_eq!((m.start, m.end), (0, 3));
                        assert_eq!(
                            m.code_result,
                            Some(CodeBlockValue::Replacement("kitten".to_string()))
                        );
                    }
                    other => panic!("expected completed match, got {:?}", other),
                }
            }
            other => panic!("expected suspension, got {:?}", other),
        }
    }

    #[test]
    fn suspendable_resume_with_numeric_value() {
        let re = Regex::with_mode(r"cat(?{native:score})", ExecutionMode::Full).unwrap();
        match re.find_first_suspendable("cat") {
            MatchOutcome::Suspended(cont) => {
                assert_eq!(cont.pending_callback_name, "score");
                match re.resume(*cont, ExecResult::Numeric(42.0)) {
                    MatchOutcome::Completed(Some(m)) => {
                        assert_eq!(m.code_result, Some(CodeBlockValue::Numeric(42.0)));
                    }
                    other => panic!("expected completed match, got {:?}", other),
                }
            }
            other => panic!("expected suspension, got {:?}", other),
        }
    }

    #[test]
    fn suspendable_context_snapshot_has_correct_position() {
        let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
        match re.find_first_suspendable("hello cat") {
            MatchOutcome::Suspended(cont) => {
                assert_eq!(cont.pending_context.match_start, 6);
                // Position should be at the end of "cat" (position 9)
                assert_eq!(cont.pending_context.position, 9);
            }
            other => panic!("expected suspension, got {:?}", other),
        }
    }

    #[test]
    fn continuation_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MatchContinuation>();
    }

    #[test]
    fn suspendable_pure_pattern_fast_path() {
        // Pure literal pattern should take the literal_finder fast path
        let re = Regex::compile("hello").unwrap();
        match re.find_first_suspendable("say hello world") {
            MatchOutcome::Completed(Some(m)) => {
                assert_eq!((m.start, m.end), (4, 9));
            }
            other => panic!("expected completed match, got {:?}", other),
        }
    }

    #[test]
    fn suspendable_multiple_suspensions_chained() {
        // Pattern with two consecutive code blocks — both unregistered
        let re = Regex::with_mode(
            r"cat(?{native:first})(?{native:second})",
            ExecutionMode::Full,
        )
        .unwrap();
        let mut outcome = re.find_first_suspendable("cat");

        // First suspension
        match outcome {
            MatchOutcome::Suspended(cont) => {
                assert_eq!(cont.pending_callback_name, "first");
                outcome = re.resume(*cont, ExecResult::Success);
            }
            other => panic!("expected first suspension, got {:?}", other),
        }

        // Second suspension
        match outcome {
            MatchOutcome::Suspended(cont) => {
                assert_eq!(cont.pending_callback_name, "second");
                outcome = re.resume(*cont, ExecResult::Success);
            }
            other => panic!("expected second suspension, got {:?}", other),
        }

        // Final completion
        match outcome {
            MatchOutcome::Completed(Some(m)) => {
                assert_eq!((m.start, m.end), (0, 3));
            }
            other => panic!("expected completed match, got {:?}", other),
        }
    }

    #[test]
    fn suspendable_sync_path_unaffected() {
        // Verify that the synchronous find_first path still works correctly
        // when patterns have registered callbacks
        let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
        re.register_native("check", |_ctx| ExecResult::Success)
            .unwrap();

        // Synchronous path
        let sync_result = re.find_first("cat");
        assert!(sync_result.is_some());
        assert_eq!(sync_result.as_ref().unwrap().start, 0);
        assert_eq!(sync_result.as_ref().unwrap().end, 3);

        // Suspendable path should also work
        match re.find_first_suspendable("cat") {
            MatchOutcome::Completed(Some(m)) => {
                assert_eq!((m.start, m.end), (0, 3));
            }
            other => panic!("expected completed, got {:?}", other),
        }
    }

    // ========================================================================
    // TYPED VALUE TESTS
    // ========================================================================

    #[test]
    fn typed_variable_int() {
        let re = Regex::with_mode(r"(?<n>\d+)(?{native:check})", ExecutionMode::Full).unwrap();
        re.set_typed_variable("threshold", Value::Int(100)).unwrap();
        re.register_native("check", |ctx| {
            let n: i64 = ctx.named("n").unwrap().parse().unwrap();
            let threshold = ctx
                .typed_variable("threshold")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if n > threshold {
                ExecResult::Success
            } else {
                ExecResult::Failure
            }
        })
        .unwrap();
        assert!(re.is_match("150"));
        assert!(!re.is_match("50"));
    }

    #[test]
    fn typed_variable_array() {
        let re = Regex::with_mode(r"(?<word>\w+)(?{native:in_list})", ExecutionMode::Full).unwrap();
        re.set_typed_variable(
            "allowed",
            Value::Array(vec![
                Value::String("cat".into()),
                Value::String("dog".into()),
            ]),
        )
        .unwrap();
        re.register_native("in_list", |ctx| {
            let word = ctx.named("word").unwrap_or("");
            let allowed = ctx
                .typed_variable("allowed")
                .and_then(|v| match v {
                    Value::Array(arr) => Some(arr),
                    _ => None,
                })
                .unwrap_or_default();
            if allowed.iter().any(|v| v.as_str() == Some(word)) {
                ExecResult::Success
            } else {
                ExecResult::Failure
            }
        })
        .unwrap();
        assert!(re.is_match("cat"));
        assert!(!re.is_match("bird"));
    }

    #[test]
    fn typed_variable_map() {
        let re = Regex::with_mode(r"(?<code>\w+)(?{native:lookup})", ExecutionMode::Full).unwrap();
        re.set_typed_variable(
            "codes",
            Value::Map(vec![
                ("US".into(), Value::String("United States".into())),
                ("UK".into(), Value::String("United Kingdom".into())),
            ]),
        )
        .unwrap();
        re.register_native("lookup", |ctx| {
            let code = ctx.named("code").unwrap_or("");
            let codes = ctx
                .typed_variable("codes")
                .and_then(|v| match v {
                    Value::Map(map) => Some(map),
                    _ => None,
                })
                .unwrap_or_default();
            if codes.iter().any(|(k, _)| k == code) {
                ExecResult::Success
            } else {
                ExecResult::Failure
            }
        })
        .unwrap();
        assert!(re.is_match("US"));
        assert!(!re.is_match("XX"));
    }

    #[test]
    fn structured_result() {
        let re = Regex::with_mode(r"(?<n>\d+)(?{native:enrich})", ExecutionMode::Full).unwrap();
        re.register_native("enrich", |ctx| {
            let n: i64 = ctx.named("n").unwrap().parse().unwrap();
            ExecResult::Structured(Value::Map(vec![
                ("original".into(), Value::Int(n)),
                ("doubled".into(), Value::Int(n * 2)),
                ("is_even".into(), Value::Bool(n % 2 == 0)),
            ]))
        })
        .unwrap();
        let m = re.find_first("42").unwrap();
        if let Some(CodeBlockValue::Structured(v)) = &m.code_result {
            assert_eq!(v.as_map().unwrap().len(), 3);
        } else {
            panic!("expected Structured code_result");
        }
    }

    #[test]
    fn string_variable_backward_compat() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.set_variable("key", "value").unwrap();
        re.register_native("check", |ctx| {
            assert_eq!(ctx.variable("key"), Some("value".to_string()));
            // Also accessible as typed
            assert_eq!(
                ctx.typed_variable("key")
                    .and_then(|v| v.as_str().map(String::from)),
                Some("value".to_string())
            );
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    #[test]
    fn typed_variable_int_backward_compat_string() {
        // When a typed variable is set, the string variable should also be set
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.set_typed_variable("threshold", Value::Int(42)).unwrap();
        re.register_native("check", |ctx| {
            // String variable should return the Display representation
            assert_eq!(ctx.variable("threshold"), Some("42".to_string()));
            // Typed variable should return the original Int
            assert_eq!(
                ctx.typed_variable("threshold").and_then(|v| v.as_i64()),
                Some(42)
            );
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    #[test]
    fn set_var_ergonomic_int() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.set_var("n", 42_i64).unwrap();
        re.register_native("check", |ctx| {
            assert_eq!(ctx.var_int("n"), Some(42));
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    #[test]
    fn set_var_ergonomic_vec_str() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.set_var("tags", vec!["a", "b", "c"]).unwrap();
        re.register_native("check", |ctx| {
            let tags = ctx.var_array("tags").unwrap();
            assert_eq!(tags.len(), 3);
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    #[test]
    fn value_array_builder() {
        let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
        re.set_var("nums", Value::array([1_i64, 2, 3])).unwrap();
        re.register_native("check", |ctx| {
            let nums = ctx.var_array("nums").unwrap();
            assert_eq!(nums.len(), 3);
            assert_eq!(nums[0].as_i64(), Some(1));
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }

    // === B10: find_at / is_match_at / find_all_at ===

    #[test]
    fn find_first_at_skips_earlier_matches() {
        let re = Regex::compile(r"\d+").unwrap();
        let text = "abc 123 xyz 456";
        // From position 0 → finds "123"
        let m = re.find_first(text).unwrap();
        assert_eq!(&text[m.start..m.end], "123");
        // From position 8 → skips "123", finds "456"
        let m = re.find_first_at(text, 8).unwrap();
        assert_eq!(&text[m.start..m.end], "456");
    }

    #[test]
    fn find_first_at_returns_none_past_end() {
        let re = Regex::compile(r"\d+").unwrap();
        assert!(re.find_first_at("abc 123", 7).is_none());
    }

    #[test]
    fn find_first_at_from_zero_same_as_find_first() {
        let re = Regex::compile(r"cat").unwrap();
        let text = "the cat sat";
        assert_eq!(re.find_first_at(text, 0), re.find_first(text));
    }

    #[test]
    fn find_all_at_starts_from_offset() {
        let re = Regex::compile(r"\w+").unwrap();
        let text = "one two three";
        let all = re.find_all(text);
        let from_4 = re.find_all_at(text, 4);
        // find_all gets 3 words; find_all_at(4) gets "two" and "three"
        assert_eq!(all.len(), 3);
        assert_eq!(from_4.len(), 2);
        assert_eq!(&text[from_4[0].start..from_4[0].end], "two");
        assert_eq!(&text[from_4[1].start..from_4[1].end], "three");
    }

    #[test]
    fn is_match_at_basic() {
        let re = Regex::compile(r"world").unwrap();
        let text = "hello world";
        assert!(!re.is_match_at(text, 7)); // "orld" — no match
        assert!(re.is_match_at(text, 6)); // "world" starts at 6
    }

    #[test]
    fn find_first_at_positions_are_absolute() {
        let re = Regex::compile(r"\d+").unwrap();
        let text = "aaa 123 bbb 456";
        let m = re.find_first_at(text, 10).unwrap();
        // "456" starts at position 12 — absolute, not relative to start=10
        assert_eq!(m.start, 12);
        assert_eq!(m.end, 15);
    }

    #[test]
    fn find_first_at_with_captures() {
        let re = Regex::compile(r"(\w+)@(\w+)").unwrap();
        let text = "a@b then c@d";
        let m = re.find_first_at(text, 4).unwrap();
        assert_eq!(&text[m.start..m.end], "c@d");
        // Group 1 = "c", Group 2 = "d"
        assert_eq!(m.groups[1], Some((9, 10)));
        assert_eq!(m.groups[2], Some((11, 12)));
    }

    #[test]
    #[should_panic(expected = "not on a UTF-8 character boundary")]
    fn find_first_at_panics_on_non_boundary() {
        let re = Regex::compile(r".").unwrap();
        let text = "café";
        // 'é' is 2 bytes (positions 3,4), so position 4 is mid-char
        re.find_first_at(text, 4);
    }

    // === B8: split / splitn ===

    #[test]
    fn split_basic() {
        let re = Regex::compile(r"[,\s]+").unwrap();
        let parts = re.split("one, two, three");
        assert_eq!(parts, vec!["one", "two", "three"]);
    }

    #[test]
    fn split_no_match_returns_whole_string() {
        let re = Regex::compile(r"\d+").unwrap();
        let parts = re.split("no digits here");
        assert_eq!(parts, vec!["no digits here"]);
    }

    #[test]
    fn split_at_boundaries_produces_empty_strings() {
        let re = Regex::compile(r",").unwrap();
        let parts = re.split(",a,,b,");
        assert_eq!(parts, vec!["", "a", "", "b", ""]);
    }

    #[test]
    fn split_empty_input() {
        let re = Regex::compile(r",").unwrap();
        let parts = re.split("");
        assert_eq!(parts, vec![""]);
    }

    #[test]
    fn splitn_limits_result_count() {
        let re = Regex::compile(r",").unwrap();
        let parts = re.splitn("a,b,c,d,e", 3);
        assert_eq!(parts, vec!["a", "b", "c,d,e"]);
    }

    #[test]
    fn splitn_limit_1_returns_whole_string() {
        let re = Regex::compile(r",").unwrap();
        let parts = re.splitn("a,b,c", 1);
        assert_eq!(parts, vec!["a,b,c"]);
    }

    #[test]
    fn splitn_limit_0_is_unlimited() {
        let re = Regex::compile(r",").unwrap();
        let parts_0 = re.splitn("a,b,c", 0);
        let parts_all = re.split("a,b,c");
        assert_eq!(parts_0, parts_all);
    }

    #[test]
    fn splitn_limit_exceeds_splits() {
        let re = Regex::compile(r",").unwrap();
        let parts = re.splitn("a,b", 10);
        assert_eq!(parts, vec!["a", "b"]);
    }

    // === B6: replace / replace_all with $1 interpolation ===

    #[test]
    fn replace_numbered_groups() {
        let re = Regex::compile(r"(\w+)\s(\w+)").unwrap();
        let result = re.replace("hello world", "$2 $1");
        assert_eq!(result, "world hello");
    }

    #[test]
    fn replace_all_numbered_groups() {
        let re = Regex::compile(r"(\w+)-(\w+)").unwrap();
        let result = re.replace_all("foo-bar baz-qux", "$2-$1");
        assert_eq!(result, "bar-foo qux-baz");
    }

    #[test]
    fn replace_dollar_ampersand_is_full_match() {
        let re = Regex::compile(r"\w+").unwrap();
        let result = re.replace_all("foo bar", "[$&]");
        assert_eq!(result, "[foo] [bar]");
    }

    #[test]
    fn replace_escaped_dollar() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replace("price 42", "$$$&");
        assert_eq!(result, "price $42");
    }

    #[test]
    fn replace_braced_group_ref() {
        let re = Regex::compile(r"(\d+)").unwrap();
        let result = re.replace("value=42", "${1}00");
        assert_eq!(result, "value=4200");
    }

    #[test]
    fn replace_named_group() {
        let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})").unwrap();
        let result = re.replace("2025-03", "$month/$year");
        assert_eq!(result, "03/2025");
    }

    #[test]
    fn replace_named_group_braced() {
        let re = Regex::compile(r"(?P<y>\d{4})-(?P<m>\d{2})-(?P<d>\d{2})").unwrap();
        let result = re.replace("2025-03-15", "${d}/${m}/${y}");
        assert_eq!(result, "15/03/2025");
    }

    #[test]
    fn replace_no_match_returns_original() {
        let re = Regex::compile(r"\d+").unwrap();
        assert_eq!(re.replace("no digits", "X"), "no digits");
    }

    #[test]
    fn replace_first_only() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replace("a1b2c3", "X");
        assert_eq!(result, "aXb2c3");
    }

    #[test]
    fn replace_all_exhaustive() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replace_all("a1b2c3", "X");
        assert_eq!(result, "aXbXcX");
    }

    #[test]
    fn replace_group_0_is_full_match() {
        let re = Regex::compile(r"(\w+)").unwrap();
        let result = re.replace("hello", "[$0]");
        assert_eq!(result, "[hello]");
    }

    // === MatchResult groups field ===

    #[test]
    fn match_result_groups_populated() {
        let re = Regex::compile(r"(\d+)-(\w+)").unwrap();
        let m = re.find_first("abc 123-xyz def").unwrap();
        assert_eq!(m.groups[0], Some((4, 11))); // full match
        assert_eq!(m.groups[1], Some((4, 7))); // group 1: "123"
        assert_eq!(m.groups[2], Some((8, 11))); // group 2: "xyz"
    }

    #[test]
    fn match_result_optional_group_is_none() {
        let re = Regex::compile(r"(a)(b)?c").unwrap();
        let m = re.find_first("ac").unwrap();
        assert_eq!(m.groups[1], Some((0, 1))); // "a"
        assert_eq!(m.groups[2], None); // "b" didn't participate
    }

    // === named_groups accessor ===

    #[test]
    fn named_groups_accessor() {
        let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})").unwrap();
        let ng = re.named_groups();
        assert_eq!(ng.get("year").copied(), Some(1));
        assert_eq!(ng.get("month").copied(), Some(2));
    }

    // === A1: Step limits ===

    #[test]
    fn step_limit_prevents_catastrophic_backtracking() {
        // (a+)+b is the classic exponential backtracking pattern.
        // Without limits, matching against "aaa...c" hangs the engine.
        let re = Regex::compile(r"(a+)+b").unwrap();
        re.set_max_steps(Some(10_000));
        // With a step limit, the engine aborts instead of hanging.
        let result = re.find_first("aaaaaaaaaaaaaaaaaaaaac");
        assert!(result.is_none());
    }

    #[test]
    fn step_limit_does_not_prevent_valid_matches() {
        let re = Regex::compile(r"(a+)+b").unwrap();
        re.set_max_steps(Some(10_000));
        // This should still match fine — no pathological backtracking.
        let m = re.find_first("aaab").unwrap();
        assert_eq!(&"aaab"[m.start..m.end], "aaab");
    }

    #[test]
    fn step_limit_none_is_unlimited() {
        let re = Regex::compile(r"\d+").unwrap();
        re.set_max_steps(None); // Explicitly unlimited
        assert!(re.is_match("123"));
    }

    #[test]
    fn step_limit_applies_per_attempt() {
        // With a very low limit, no position can complete.
        let re = Regex::compile(r".{5}").unwrap();
        re.set_max_steps(Some(3)); // Too low to match 5 chars
        assert!(re.find_first("abcdefgh").is_none());
    }

    // === A2: Memory limits ===

    #[test]
    fn backtrack_frame_limit_prevents_stack_explosion() {
        // a* generates one backtrack frame per character matched.
        let re = Regex::compile(r"a*b").unwrap();
        re.set_max_backtrack_frames(Some(5));
        // Input with many 'a's but no 'b' — forces many backtrack frames.
        let result = re.find_first("aaaaaaaaaaaaaaaa");
        assert!(result.is_none());
    }

    #[test]
    fn backtrack_frame_limit_does_not_prevent_valid_matches() {
        let re = Regex::compile(r"a*b").unwrap();
        re.set_max_backtrack_frames(Some(100));
        let m = re.find_first("aaab").unwrap();
        assert_eq!(&"aaab"[m.start..m.end], "aaab");
    }

    #[test]
    fn recursion_depth_limit_custom() {
        // (a(?1)?b): each nesting level = one recursion call.
        // "ab" = 0 calls, "aabb" = 1, "aaabbb" = 2, "aaaabbbb" = 3, etc.
        // set_max_recursion_depth(Some(N)) allows up to N recursion calls.
        let re = Regex::compile(r"(a(?1)?b)").unwrap();

        // Limit 1: allows 1 recursion call → "aabb" matches, "aaabbb" degrades to "aabb"
        re.set_max_recursion_depth(Some(1));
        let m = re.find_first("aabb").unwrap();
        assert_eq!(&"aabb"[m.start..m.end], "aabb");
        let m = re.find_first("aaabbb").unwrap();
        assert_eq!(&"aaabbb"[m.start..m.end], "aabb");

        // Limit 3: allows 3 recursion calls → "aaaabbbb" matches fully
        re.set_max_recursion_depth(Some(3));
        let m = re.find_first("aaaabbbb").unwrap();
        assert_eq!(&"aaaabbbb"[m.start..m.end], "aaaabbbb");
        // But "aaaaabbbbb" (4 calls) degrades to "aaaabbbb"
        let m = re.find_first("aaaaabbbbb").unwrap();
        assert_eq!(&"aaaaabbbbb"[m.start..m.end], "aaaabbbb");
    }

    #[test]
    fn recursion_depth_limit_none_uses_default() {
        let re = Regex::compile(r"(a(?1)?b)").unwrap();
        re.set_max_recursion_depth(None); // Uses default (1024)
        let m = re.find_first("aabb");
        assert!(m.is_some());
    }

    // === B18: escape() ===

    #[test]
    fn escape_metacharacters() {
        assert_eq!(escape("hello"), "hello");
        assert_eq!(escape("a.b"), r"a\.b");
        assert_eq!(escape("(a+)+b"), r"\(a\+\)\+b");
        assert_eq!(escape("[foo]"), r"\[foo\]");
        assert_eq!(escape("a|b"), r"a\|b");
        assert_eq!(escape("^$"), r"\^\$");
        assert_eq!(escape("a{3}"), r"a\{3\}");
        assert_eq!(escape(r"a\b"), r"a\\b");
    }

    #[test]
    fn escaped_string_matches_literally() {
        let text = "price is $3.50 (USD)";
        let pattern = escape("$3.50 (USD)");
        let re = Regex::compile(&pattern).unwrap();
        assert!(re.is_match(text));
    }

    // === B14: Match type ===

    #[test]
    fn find_returns_match_with_as_str() {
        let re = Regex::compile(r"\d+").unwrap();
        let m = re.find("abc 42 xyz").unwrap();
        assert_eq!(m.as_str(), "42");
        assert_eq!(m.start(), 4);
        assert_eq!(m.end(), 6);
        assert_eq!(m.range(), 4..6);
        assert_eq!(m.len(), 2);
        assert!(!m.is_empty());
    }

    #[test]
    fn find_zero_width_match() {
        let re = Regex::compile(r"^").unwrap();
        let m = re.find("hello").unwrap();
        assert_eq!(m.as_str(), "");
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    // === B13: Captures wrapper ===

    #[test]
    fn captures_by_index() {
        let re = Regex::compile(r"(\d{4})-(\d{2})-(\d{2})").unwrap();
        let caps = re.captures("Date: 2025-03-15").unwrap();
        assert_eq!(&caps[0], "2025-03-15");
        assert_eq!(&caps[1], "2025");
        assert_eq!(&caps[2], "03");
        assert_eq!(&caps[3], "15");
    }

    #[test]
    fn captures_by_name() {
        let re = Regex::compile(r"(?P<y>\d{4})-(?P<m>\d{2})-(?P<d>\d{2})").unwrap();
        let caps = re.captures("Date: 2025-03-15").unwrap();
        assert_eq!(&caps["y"], "2025");
        assert_eq!(&caps["m"], "03");
        assert_eq!(&caps["d"], "15");
        assert_eq!(caps.name("y").unwrap().as_str(), "2025");
    }

    #[test]
    fn captures_get_returns_none_for_missing() {
        let re = Regex::compile(r"(a)(b)?c").unwrap();
        let caps = re.captures("ac").unwrap();
        assert!(caps.get(1).is_some());
        assert!(caps.get(2).is_none()); // group 2 didn't participate
    }

    #[test]
    fn captures_expand() {
        let re = Regex::compile(r"(?P<first>\w+)\s(?P<last>\w+)").unwrap();
        let caps = re.captures("John Doe").unwrap();
        let mut out = String::new();
        caps.expand("$last, $first", &mut out);
        assert_eq!(out, "Doe, John");
    }

    #[test]
    fn captures_iter_yields_all_groups() {
        let re = Regex::compile(r"(a)(b)(c)").unwrap();
        let caps = re.captures("abc").unwrap();
        let strs: Vec<_> = caps.iter().map(|m| m.map(|m| m.as_str())).collect();
        assert_eq!(strs, vec![Some("abc"), Some("a"), Some("b"), Some("c")]);
    }

    #[test]
    fn captures_len_method() {
        let re = Regex::compile(r"(a)(b)(c)").unwrap();
        let caps = re.captures("abc").unwrap();
        assert_eq!(caps.len(), 4); // group 0 + 3 captures
    }

    // === B15: replacen ===

    #[test]
    fn replacen_limits_replacements() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replacen("a1b2c3d4", 2, "X");
        assert_eq!(result, "aXbXc3d4");
    }

    #[test]
    fn replacen_zero_means_all() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replacen("a1b2c3", 0, "X");
        assert_eq!(result, "aXbXcX");
    }

    #[test]
    fn replacen_one_is_replace_first() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replacen("a1b2c3", 1, "X");
        assert_eq!(result, "aXb2c3");
    }

    // === B21: Cow<str> replace ===

    #[test]
    fn replace_no_match_borrows() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replace("no digits", "X");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn replace_with_match_owns() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replace("abc 42 xyz", "X");
        assert!(matches!(result, std::borrow::Cow::Owned(_)));
        assert_eq!(result, "abc X xyz");
    }

    // === B19: Metadata ===

    #[test]
    fn as_str_returns_original_pattern() {
        let re = Regex::compile(r"\d{3}-\d{4}").unwrap();
        assert_eq!(re.as_str(), r"\d{3}-\d{4}");
    }

    #[test]
    fn captures_len_on_regex() {
        let re = Regex::compile(r"(a)(b)(c)").unwrap();
        assert_eq!(re.captures_len(), 4); // group 0 + 3 groups
    }

    #[test]
    fn captures_len_no_groups() {
        let re = Regex::compile(r"abc").unwrap();
        assert_eq!(re.captures_len(), 1); // just group 0
    }

    // === B12: Iterator APIs ===

    #[test]
    fn find_iter_basic() {
        let re = Regex::compile(r"\d+").unwrap();
        let matches: Vec<_> = re.find_iter("a1b22c333").map(|m| m.as_str()).collect();
        assert_eq!(matches, vec!["1", "22", "333"]);
    }

    #[test]
    fn find_iter_no_match() {
        let re = Regex::compile(r"\d+").unwrap();
        let matches: Vec<_> = re.find_iter("abc").collect();
        assert!(matches.is_empty());
    }

    #[test]
    fn find_iter_agrees_with_find_all() {
        let re = Regex::compile(r"\w+").unwrap();
        let text = "one two three";
        let iter_results: Vec<_> = re.find_iter(text).map(|m| (m.start(), m.end())).collect();
        let all_results: Vec<_> = re.find_all(text).iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(iter_results, all_results);
    }

    #[test]
    fn captures_iter_basic() {
        let re = Regex::compile(r"(\w+)=(\d+)").unwrap();
        let pairs: Vec<_> = re
            .captures_iter("a=1 b=2 c=3")
            .map(|c| (c[1].to_string(), c[2].to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string()),
                ("c".to_string(), "3".to_string()),
            ]
        );
    }

    #[test]
    fn split_iter_basic() {
        let re = Regex::compile(r"[,\s]+").unwrap();
        let parts: Vec<_> = re.split_iter("one, two, three").collect();
        assert_eq!(parts, vec!["one", "two", "three"]);
    }

    #[test]
    fn split_iter_agrees_with_split() {
        let re = Regex::compile(r",").unwrap();
        let text = ",a,,b,";
        let iter_parts: Vec<_> = re.split_iter(text).collect();
        let vec_parts = re.split(text);
        assert_eq!(iter_parts, vec_parts);
    }

    #[test]
    fn splitn_iter_basic() {
        let re = Regex::compile(r",").unwrap();
        let parts: Vec<_> = re.splitn_iter("a,b,c,d,e", 3).collect();
        assert_eq!(parts, vec!["a", "b", "c,d,e"]);
    }

    #[test]
    fn splitn_iter_limit_zero_is_empty() {
        let re = Regex::compile(r",").unwrap();
        let parts: Vec<_> = re.splitn_iter("a,b,c", 0).collect();
        assert!(parts.is_empty());
    }

    #[test]
    fn capture_names_basic() {
        let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})-(\d{2})").unwrap();
        let names: Vec<_> = re.capture_names().collect();
        // Group 0 = None, group 1 = "year", group 2 = "month", group 3 = None (unnamed)
        assert_eq!(names.len(), 4);
        assert_eq!(names[0], None);
        assert_eq!(names[1], Some("year"));
        assert_eq!(names[2], Some("month"));
        assert_eq!(names[3], None);
    }

    #[test]
    fn capture_names_exact_size() {
        let re = Regex::compile(r"(a)(b)").unwrap();
        let names = re.capture_names();
        assert_eq!(names.len(), 3); // ExactSizeIterator
    }

    #[test]
    fn find_iter_fused() {
        let re = Regex::compile(r"\d").unwrap();
        let mut iter = re.find_iter("a1");
        assert!(iter.next().is_some());
        assert!(iter.next().is_none());
        assert!(iter.next().is_none()); // FusedIterator
    }

    // === B16: Replacer trait ===

    #[test]
    fn replace_with_closure() {
        let re = Regex::compile(r"\w+").unwrap();
        let result = re.replace_all("hello world", |caps: &Captures| caps[0].to_uppercase());
        assert_eq!(result, "HELLO WORLD");
    }

    #[test]
    fn replace_closure_with_captures() {
        let re = Regex::compile(r"(\w+)\s(\w+)").unwrap();
        let result = re.replace("John Doe", |caps: &Captures| {
            format!("{}, {}", &caps[2], &caps[1])
        });
        assert_eq!(result, "Doe, John");
    }

    #[test]
    fn replace_with_no_expand() {
        let re = Regex::compile(r"\d+").unwrap();
        // NoExpand prevents $1 from being interpreted
        let result = re.replace("price 42", NoExpand("$1"));
        assert_eq!(result, "price $1");
    }

    #[test]
    fn replace_all_with_closure_counter() {
        let re = Regex::compile(r"\w+").unwrap();
        let mut count = 0;
        let result = re.replace_all("a b c", |_caps: &Captures| {
            count += 1;
            count.to_string()
        });
        assert_eq!(result, "1 2 3");
    }

    #[test]
    fn replacen_with_closure() {
        let re = Regex::compile(r"\d+").unwrap();
        let result = re.replacen("a1b2c3", 2, |caps: &Captures| format!("[{}]", &caps[0]));
        assert_eq!(result, "a[1]b[2]c3");
    }

    #[test]
    fn replace_literal_no_dollar_skips_expansion() {
        let re = Regex::compile(r"\d+").unwrap();
        // "X" has no '$', so no_expansion() returns Some → fast path
        let result = re.replace_all("a1b2c3", "X");
        assert_eq!(result, "aXbXcX");
    }

    // === B17: shortest_match ===

    #[test]
    fn shortest_match_returns_end_position() {
        let re = Regex::compile(r"\d+").unwrap();
        assert_eq!(re.shortest_match("abc 42 xyz"), Some(6)); // end of "42"
    }

    #[test]
    fn shortest_match_no_match() {
        let re = Regex::compile(r"\d+").unwrap();
        assert_eq!(re.shortest_match("abc"), None);
    }

    #[test]
    fn shortest_match_at_from_offset() {
        let re = Regex::compile(r"\d+").unwrap();
        let text = "12 abc 34";
        assert_eq!(re.shortest_match_at(text, 3), Some(9)); // end of "34"
    }

    // === B11: RegexBuilder ===

    #[test]
    fn regex_builder_case_insensitive() {
        let re = RegexBuilder::new(r"hello")
            .case_insensitive()
            .build()
            .unwrap();
        assert!(re.is_match("HELLO"));
        assert!(re.is_match("Hello"));
        assert!(re.is_match("hello"));
    }

    #[test]
    fn regex_builder_multi_line() {
        let re = RegexBuilder::new(r"^line$").multi_line().build().unwrap();
        assert!(re.is_match("first\nline\nlast"));
    }

    #[test]
    fn regex_builder_dot_all() {
        let re = RegexBuilder::new(r"a.b")
            .dot_matches_new_line()
            .build()
            .unwrap();
        assert!(re.is_match("a\nb"));
    }

    #[test]
    fn regex_builder_extended() {
        let re = RegexBuilder::new(
            r"
            \d{3}   # area code
            -
            \d{4}   # number
        ",
        )
        .ignore_whitespace()
        .build()
        .unwrap();
        assert!(re.is_match("555-1234"));
    }

    #[test]
    fn regex_builder_combined_flags() {
        let re = RegexBuilder::new(r"^hello.world$")
            .case_insensitive()
            .multi_line()
            .dot_matches_new_line()
            .build()
            .unwrap();
        assert!(re.is_match("prefix\nHELLO\nWORLD\nsuffix"));
    }

    #[test]
    fn regex_builder_with_mode() {
        let re = RegexBuilder::new(r"\d+")
            .mode(ExecutionMode::Safe)
            .build()
            .unwrap();
        assert!(re.is_match("42"));
    }

    #[test]
    fn regex_builder_no_flags_same_as_compile() {
        let re1 = Regex::compile(r"\d+").unwrap();
        let re2 = RegexBuilder::new(r"\d+").build().unwrap();
        let text = "abc 123 xyz";
        assert_eq!(
            re1.find_first(text).map(|m| (m.start, m.end)),
            re2.find_first(text).map(|m| (m.start, m.end))
        );
    }

    // === B20: CaptureLocations ===

    #[test]
    fn capture_locations_basic() {
        let re = Regex::compile(r"(\d+)-(\w+)").unwrap();
        let mut locs = re.capture_locations();
        let m = re.captures_read("item 42-abc end", &mut locs).unwrap();
        assert_eq!(m.as_str(), "42-abc");
        assert_eq!(locs.get(0), Some((5, 11))); // full match
        assert_eq!(locs.get(1), Some((5, 7))); // "42"
        assert_eq!(locs.get(2), Some((8, 11))); // "abc"
    }

    #[test]
    fn capture_locations_reuse() {
        let re = Regex::compile(r"(\w+)").unwrap();
        let mut locs = re.capture_locations();

        let m1 = re.captures_read("hello", &mut locs).unwrap();
        assert_eq!(m1.as_str(), "hello");
        assert_eq!(locs.get(1), Some((0, 5)));

        let m2 = re.captures_read("world", &mut locs).unwrap();
        assert_eq!(m2.as_str(), "world");
        assert_eq!(locs.get(1), Some((0, 5)));
    }

    #[test]
    fn capture_locations_no_match() {
        let re = Regex::compile(r"\d+").unwrap();
        let mut locs = re.capture_locations();
        assert!(re.captures_read("abc", &mut locs).is_none());
    }

    #[test]
    fn capture_locations_optional_group() {
        let re = Regex::compile(r"(a)(b)?c").unwrap();
        let mut locs = re.capture_locations();
        re.captures_read("ac", &mut locs).unwrap();
        assert!(locs.get(1).is_some()); // "a" matched
        assert!(locs.get(2).is_none()); // "b" didn't participate
    }

    #[test]
    fn capture_locations_at_offset() {
        let re = Regex::compile(r"(\d+)").unwrap();
        let mut locs = re.capture_locations();
        let m = re.captures_read_at("aa 11 bb 22", 5, &mut locs).unwrap();
        assert_eq!(m.as_str(), "22");
        assert_eq!(locs.get(1), Some((9, 11)));
    }

    #[test]
    fn capture_locations_len() {
        let re = Regex::compile(r"(a)(b)(c)").unwrap();
        let locs = re.capture_locations();
        assert_eq!(locs.len(), 4); // group 0 + 3
        assert!(!locs.is_empty());
    }

    // === A7: Unicode case folding ===

    #[test]
    fn unicode_case_fold_accented_letters() {
        let re = Regex::compile(r"(?i)café").unwrap();
        assert!(re.is_match("café"));
        assert!(re.is_match("CAFÉ"));
        assert!(re.is_match("Café"));
        assert!(re.is_match("caFÉ"));
    }

    #[test]
    fn unicode_case_fold_greek() {
        let re = Regex::compile(r"(?i)αβγ").unwrap();
        assert!(re.is_match("αβγ"));
        assert!(re.is_match("ΑΒΓ"));
        assert!(re.is_match("Αβγ"));
    }

    #[test]
    fn unicode_case_fold_cyrillic() {
        let re = Regex::compile(r"(?i)москва").unwrap();
        assert!(re.is_match("москва"));
        assert!(re.is_match("МОСКВА"));
        assert!(re.is_match("Москва"));
    }

    #[test]
    fn unicode_case_fold_builder() {
        let re = RegexBuilder::new(r"café")
            .case_insensitive()
            .build()
            .unwrap();
        assert!(re.is_match("CAFÉ"));
    }

    #[test]
    fn unicode_case_fold_char_class() {
        let re = Regex::compile(r"(?i)[àéîöü]").unwrap();
        assert!(re.is_match("À"));
        assert!(re.is_match("É"));
        assert!(re.is_match("Î"));
        assert!(re.is_match("Ö"));
        assert!(re.is_match("Ü"));
    }

    #[test]
    fn unicode_case_fold_ascii_still_works() {
        let re = Regex::compile(r"(?i)hello").unwrap();
        assert!(re.is_match("HELLO"));
        assert!(re.is_match("Hello"));
        assert!(re.is_match("hElLo"));
    }

    // === B4: Match semantics ===

    #[test]
    fn leftmost_first_is_default() {
        // Default behavior: first alternative wins.
        let re = Regex::compile(r"a|ab").unwrap();
        let m = re.find("ab").unwrap();
        assert_eq!(m.as_str(), "a"); // first alternative
    }

    #[test]
    fn leftmost_longest_greedy_quantifiers_already_longest() {
        // Greedy quantifiers naturally produce the longest match.
        // LeftmostLongest doesn't change behavior for patterns without alternation.
        let re = Regex::compile(r"\w+").unwrap();
        re.set_match_semantics(MatchSemantics::LeftmostLongest);
        let m = re.find("hello world").unwrap();
        assert_eq!(m.as_str(), "hello");
    }

    #[test]
    fn leftmost_longest_alternation_workaround() {
        // For alternation, put the longest branch first to get POSIX behavior.
        // `ab|a` instead of `a|ab` — the longer alternative is tried first.
        let re = Regex::compile(r"ab|a").unwrap();
        re.set_match_semantics(MatchSemantics::LeftmostLongest);
        let m = re.find("ab").unwrap();
        assert_eq!(m.as_str(), "ab");
    }

    #[test]
    fn leftmost_longest_semantics_flag_stored() {
        // The flag is stored and can influence future compiler-level reordering.
        let re = Regex::compile(r"a|ab").unwrap();
        re.set_match_semantics(MatchSemantics::LeftmostLongest);
        // Currently returns "a" (first-match behavior) — full POSIX alternation
        // reordering is a compiler-level follow-up.
        let m = re.find("ab").unwrap();
        assert_eq!(m.as_str(), "a");
    }

    #[test]
    fn leftmost_longest_no_match() {
        let re = Regex::compile(r"\d+").unwrap();
        re.set_match_semantics(MatchSemantics::LeftmostLongest);
        assert!(re.find("abc").is_none());
    }

    #[test]
    fn leftmost_first_unchanged_when_no_alternation() {
        let re = Regex::compile(r"\d+").unwrap();
        re.set_match_semantics(MatchSemantics::LeftmostLongest);
        let m = re.find("abc 123 def").unwrap();
        // Greedy quantifier already matches longest — semantics don't change this.
        assert_eq!(m.as_str(), "123");
    }

    // === A14: Partial matching ===

    #[test]
    fn partial_match_full() {
        let re = Regex::compile(r"hello world").unwrap();
        match re.find_first_partial("hello world") {
            PartialMatchResult::Full(m) => assert_eq!(m.start, 0),
            other => panic!("expected Full, got {other:?}"),
        }
    }

    #[test]
    fn partial_match_partial() {
        let re = Regex::compile(r"hello world").unwrap();
        match re.find_first_partial("hello wor") {
            PartialMatchResult::Partial(offset) => assert_eq!(offset, 0),
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn partial_match_no_match() {
        let re = Regex::compile(r"hello").unwrap();
        match re.find_first_partial("xyz") {
            PartialMatchResult::NoMatch => {}
            other => panic!("expected NoMatch, got {other:?}"),
        }
    }

    #[test]
    fn partial_match_at_boundary() {
        let re = Regex::compile(r"\d{4}-\d{2}-\d{2}").unwrap();
        // Full date matches
        assert!(matches!(
            re.find_first_partial("2025-03-15"),
            PartialMatchResult::Full(_)
        ));
        // Partial date — input ends mid-match
        assert!(matches!(
            re.find_first_partial("2025-03"),
            PartialMatchResult::Partial(_)
        ));
        // No digits at all
        assert!(matches!(
            re.find_first_partial("abc"),
            PartialMatchResult::NoMatch
        ));
    }

    #[test]
    fn partial_match_empty_input() {
        let re = Regex::compile(r"abc").unwrap();
        // Empty input can't match "abc" but could with more data
        // (pattern starts matching at position 0)
        match re.find_first_partial("") {
            PartialMatchResult::NoMatch | PartialMatchResult::Partial(_) => {}
            other => panic!("expected NoMatch or Partial, got {other:?}"),
        }
    }

    // === A10: \X extended grapheme cluster ===

    #[test]
    fn grapheme_cluster_basic() {
        match Regex::compile(r"\X") {
            Err(e) => panic!("COMPILE FAILED: {e}"),
            Ok(re) => {
                let all = re.find_all("a");
                assert_eq!(all.len(), 1, "expected 1 match in 'a', got {}", all.len());
                let m = re.find("hello").unwrap();
                assert_eq!(m.as_str(), "h");
            }
        }
    }

    #[test]
    fn grapheme_cluster_combining_marks() {
        let re = Regex::compile(r"\X").unwrap();
        // e + combining acute (U+0301) = one grapheme cluster
        let text = "e\u{0301}x";
        let m = re.find(text).unwrap();
        assert_eq!(m.as_str(), "e\u{0301}");
        assert_eq!(m.len(), 3); // e(1) + combining(2) = 3 bytes
    }

    #[test]
    fn grapheme_cluster_emoji() {
        let re = Regex::compile(r"\X").unwrap();
        let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
        let m = re.find(family).unwrap();
        assert_eq!(m.as_str(), family); // entire ZWJ sequence is one grapheme
    }

    #[test]
    fn grapheme_cluster_find_all() {
        let re = Regex::compile(r"\X").unwrap();
        let text = "cafe\u{0301}";
        let all: Vec<_> = re.find_iter(text).map(|m| m.as_str()).collect();
        assert_eq!(all, vec!["c", "a", "f", "e\u{0301}"]);
    }

    #[test]
    fn grapheme_cluster_quantifier() {
        let re = Regex::compile(r"\X{3}").unwrap();
        let m = re.find("abc").unwrap();
        assert_eq!(m.as_str(), "abc");
    }

    // === A12: Returned-capture subroutines ===

    #[test]
    fn returned_capture_subroutine_compiles() {
        // (?1(1)) — call group 1, return captures from group 1
        let result = Regex::compile(r"(a)(?1(1))");
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    #[test]
    fn returned_capture_subroutine_matches() {
        // The pattern compiles and matches (currently same semantics as (?1))
        let re = Regex::compile(r"(a)(?1(1))").unwrap();
        assert!(re.is_match("aa"));
    }

    // === Reverse-DFA pipeline: leftmost-first find_first ===

    #[test]
    fn pipeline_find_first_returns_leftmost_non_greedy() {
        // `\w\w` on "abc": first match is "ab" (0, 2), NOT "bc" (1, 3).
        let re = Regex::compile(r"\w\w").unwrap();
        let m = re.find_first("abc").expect("should match");
        assert_eq!((m.start, m.end), (0, 2));
    }

    #[test]
    fn pipeline_find_first_extends_greedy_after_start_recovery() {
        // `a+` on "baaab": first accept is after one 'a' at end=2, but
        // greedy extension takes the match through end=4. Step 3 of
        // the pipeline runs the forward-anchored DFA from the
        // recovered start to produce the greedy end.
        let re = Regex::compile(r"a+").unwrap();
        let m = re.find_first("baaab").expect("should match");
        assert_eq!((m.start, m.end), (1, 4));
    }

    #[test]
    fn pipeline_find_first_handles_empty_match_at_start() {
        // `a*` on "xay": leftmost is the empty match at position 0.
        let re = Regex::compile(r"a*").unwrap();
        let m = re.find_first("xay").expect("should match");
        assert_eq!((m.start, m.end), (0, 0));
    }

    #[test]
    fn pipeline_find_first_single_char_class_leftmost() {
        // `\d` on "abc123": leftmost digit is '1' at position 3.
        let re = Regex::compile(r"\d").unwrap();
        let m = re.find_first("abc123").expect("should match");
        assert_eq!((m.start, m.end), (3, 4));
    }

    #[test]
    fn pipeline_find_first_repeated_literal_returns_leftmost() {
        // Literal `a` on "xaxa": leftmost is (1, 2).
        let re = Regex::compile("a").unwrap();
        let m = re.find_first("xaxa").expect("should match");
        assert_eq!((m.start, m.end), (1, 2));
    }

    #[test]
    fn pipeline_find_first_respects_no_match() {
        let re = Regex::compile(r"\d{4}").unwrap();
        assert!(re.find_first("abc 123 xy").is_none());
    }

    // === Reverse-DFA pipeline: find_all ===
    //
    // Pipeline iterates match-by-match: forward-unanchored
    // find_first_accept_at(pos) → reverse-anchored bounded at `pos` →
    // forward-anchored greedy end → Pike-VM captures. The bounded
    // reverse walk is the key — without it the reverse DFA could
    // locate a start inside the previously-consumed span for the
    // second iteration onward.

    #[test]
    fn pipeline_find_all_non_greedy_non_overlapping() {
        // `\w\w` on "abcdef": matches "ab" (0,2), "cd" (2,4), "ef" (4,6).
        let re = Regex::compile(r"\w\w").unwrap();
        let ms = re.find_all("abcdef");
        let spans: Vec<(usize, usize)> = ms.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(0, 2), (2, 4), (4, 6)]);
    }

    #[test]
    fn pipeline_find_all_greedy_extended_spans() {
        // `a+` on "baaababaa": matches "aaa" (1,4), "a" (5,6), "aa" (7,9).
        let re = Regex::compile(r"a+").unwrap();
        let ms = re.find_all("baaababaa");
        let spans: Vec<(usize, usize)> = ms.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(1, 4), (5, 6), (7, 9)]);
    }

    #[test]
    fn pipeline_find_all_single_char_class_all_digits() {
        // `\d` on "ab12cd34ef": matches at each digit position.
        let re = Regex::compile(r"\d").unwrap();
        let ms = re.find_all("ab12cd34ef");
        let spans: Vec<(usize, usize)> = ms.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(2, 3), (3, 4), (6, 7), (7, 8)]);
    }

    #[test]
    fn pipeline_find_all_empty_match_pattern_with_advance_rules() {
        // `a*` on "bab": `a*` accepts empty at every position, plus the
        // "a" in the middle. Expected matches per find_all's rules:
        //   - empty at 0
        //   - "a" at (1, 2)
        //   - empty at 3 (end)
        //   (empty at 2 is dropped because it's adjacent to the
        //    previous non-empty match ending at 2)
        let re = Regex::compile(r"a*").unwrap();
        let ms = re.find_all("bab");
        let spans: Vec<(usize, usize)> = ms.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(0, 0), (1, 2), (3, 3)]);
    }

    #[test]
    fn pipeline_find_all_no_matches_returns_empty() {
        let re = Regex::compile(r"\d{4}").unwrap();
        assert!(re.find_all("abc 123 xy").is_empty());
    }

    // === Inner-literal fast-fail (no-match acceleration) ===
    //
    // For patterns whose match must contain a specific byte (e.g.,
    // `@` for email, `-` for date), dispatch helpers memchr the
    // input first; if the byte is absent, no match can exist and
    // they short-circuit without running the DFA. These tests pin
    // behavioral equivalence: same answer as the slow path, just
    // faster on no-match inputs.

    #[test]
    fn inner_literal_fast_fail_no_match_for_email_pattern_without_at_sign() {
        // `\w+@\w+\.\w+` requires `@`. Input has no `@` → no match.
        let re = Regex::compile(r"\w+@\w+\.\w+").unwrap();
        assert!(re.find_first("abcdef ghijkl").is_none());
        assert!(!re.is_match("abcdef ghijkl"));
        assert_eq!(re.find_all("abcdef ghijkl").len(), 0);
    }

    #[test]
    fn inner_literal_fast_fail_still_finds_match_when_present() {
        // Same pattern, input contains `@`. Result identical to
        // pre-fast-fail behavior.
        let re = Regex::compile(r"\w+@\w+\.\w+").unwrap();
        let m = re.find_first("ping foo@bar.com pong").expect("match");
        assert_eq!(&"ping foo@bar.com pong"[m.start..m.end], "foo@bar.com");
    }

    #[test]
    fn inner_literal_fast_fail_for_date_pattern_without_dash() {
        // `\d{3}-\d{2}-\d{4}` requires `-`. Input has digits but no
        // dashes → no match.
        let re = Regex::compile(r"\d{3}-\d{2}-\d{4}").unwrap();
        assert!(re.find_first("123 45 6789").is_none());
        assert!(!re.is_match("123 45 6789"));
    }

    #[test]
    fn inner_literal_fast_fail_does_not_misfire_on_pure_class_pattern() {
        // `\d+` has no required literal byte. Fast-fail must not
        // trigger; the regular dispatch path runs.
        let re = Regex::compile(r"\d+").unwrap();
        let m = re.find_first("abc 42 xyz").expect("match");
        assert_eq!(&"abc 42 xyz"[m.start..m.end], "42");
    }

    // === Aho-Corasick dispatch for literal alternation ===
    //
    // Top-level alternations of pure ASCII literals
    // (`cat|dog|bird`) are handled by an AC automaton built at
    // compile time. AC's `MatchKind::LeftmostFirst` honours PCRE2's
    // alternation semantics; the 0-based pattern_id is mapped to
    // the 1-based matched_branch_number that the existing API
    // contract uses.

    #[test]
    fn ac_dispatch_finds_first_literal_in_alternation() {
        let re = Regex::compile(r"cat|dog|bird").unwrap();
        let m = re.find_first("the dog runs fast").expect("match");
        assert_eq!((m.start, m.end), (4, 7));
        assert_eq!(m.matched_branch_number, Some(2)); // dog is branch 2
    }

    #[test]
    fn ac_dispatch_honours_leftmost_first_semantics_on_overlap() {
        // `a|abc` — when both could match at position 0, the first
        // branch wins (leftmost-first). PCRE2 semantics.
        let re = Regex::compile(r"a|abc").unwrap();
        let m = re.find_first("abc").expect("match");
        assert_eq!((m.start, m.end), (0, 1)); // "a", not "abc"
        assert_eq!(m.matched_branch_number, Some(1));
    }

    #[test]
    fn ac_dispatch_finds_all_literal_matches() {
        let re = Regex::compile(r"cat|dog").unwrap();
        let ms = re.find_all("cat and dog and cat");
        let spans: Vec<(usize, usize)> = ms.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(0, 3), (8, 11), (16, 19)]);
        let branches: Vec<Option<usize>> = ms.iter().map(|m| m.matched_branch_number).collect();
        assert_eq!(branches, vec![Some(1), Some(2), Some(1)]);
    }

    #[test]
    fn ac_dispatch_is_match_for_literal_alternation() {
        let re = Regex::compile(r"ERROR|WARN|FATAL").unwrap();
        assert!(re.is_match("[2026-04-25 14:00:00] ERROR connection refused"));
        assert!(!re.is_match("[2026-04-25 14:00:00] DEBUG everything fine"));
    }

    #[test]
    fn ac_dispatch_no_match_returns_none() {
        let re = Regex::compile(r"cat|dog|bird").unwrap();
        assert!(re.find_first("only fish here").is_none());
        assert!(re.find_all("only fish here").is_empty());
        assert!(!re.is_match("only fish here"));
    }

    #[test]
    fn ac_dispatch_does_not_misfire_on_non_alternation_pattern() {
        // `\d+` is not a literal alternation — must not hit AC; the
        // regular dispatch chain returns the right answer.
        let re = Regex::compile(r"\d+").unwrap();
        let m = re.find_first("abc 42 xyz").expect("match");
        assert_eq!(&"abc 42 xyz"[m.start..m.end], "42");
    }

    // === Multi-byte memmem prefilter ===
    //
    // For patterns whose leading run is a multi-byte literal (e.g.
    // `http` for `https?://\S+`), the dispatch's PrefixScanner uses
    // `memchr::memmem::Finder` instead of single-byte `memchr`.
    // Vastly more selective on real inputs (one `h` per ~13 ASCII
    // bytes vs one `http` per real URL).

    #[test]
    fn memmem_prefix_finds_url_match_after_skip() {
        let re = Regex::compile(r"https?://\S+").unwrap();
        let text = "lorem ipsum hat hot http://example.com here";
        let m = re.find_first(text).expect("match");
        assert_eq!(&text[m.start..m.end], "http://example.com");
    }

    #[test]
    fn memmem_prefix_returns_none_on_no_match_input() {
        // Input contains no `http` substring → memmem returns None,
        // dispatch returns no-match without running the DFA.
        let re = Regex::compile(r"https?://\S+").unwrap();
        assert!(re.find_first("only hat hot here, no urls").is_none());
    }

    #[test]
    fn memmem_prefix_finds_all_urls() {
        let re = Regex::compile(r"https?://\S+").unwrap();
        let text = "see http://a.com and https://b.com for details";
        let ms = re.find_all(text);
        let strs: Vec<&str> = ms.iter().map(|m| &text[m.start..m.end]).collect();
        assert_eq!(strs, vec!["http://a.com", "https://b.com"]);
    }

    #[test]
    fn pipeline_find_all_reverse_walk_bounded_preserves_non_overlap() {
        // Repeated-pattern regression: without bounding the reverse walk
        // at `pos`, iteration 2 of find_all could re-locate iteration
        // 1's match start and loop or report overlapping spans. This
        // test pins that the bounded reverse walk advances correctly.
        let re = Regex::compile(r"\w+").unwrap();
        let ms = re.find_all("aa bb cc");
        let spans: Vec<(usize, usize)> = ms.iter().map(|m| (m.start, m.end)).collect();
        assert_eq!(spans, vec![(0, 2), (3, 5), (6, 8)]);
    }
}
