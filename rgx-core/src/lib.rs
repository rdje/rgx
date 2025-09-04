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
//! │  WASM   │   Lua   │  Native Calls   │  ← Pluggable executors
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
//!     r"(\d{4})-(\d{2})-(\d{2})(?{native:validate_date})",
//!     ExecutionMode::Safe
//! )?;
//! let dates = validator.find_all("Born on 1985-03-15 and graduated 2007-06-22");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

// Core modules
pub mod ast;
pub mod lexer;
pub mod parser;
pub mod parsing;
pub mod token;
pub mod vm;
pub mod compiler;
pub mod execution;
pub mod pattern;
pub mod engine;

// Performance optimizations
pub mod simd;
pub mod cache;

// Code execution backends
#[cfg(feature = "wasm")]
pub mod wasm;
#[cfg(feature = "lua")]  
pub mod lua;
#[cfg(feature = "javascript")]
pub mod javascript;

// Error handling
pub mod error;

// Re-exports for convenience
pub use engine::{Engine, ExecutionMode, MatchResult};
pub use pattern::{Pattern, CompiledPattern};
pub use compiler::Compiler;
pub use error::{RgxError, Result};

/// High-performance regex matcher with optional code execution capabilities.
///
/// This is the main entry point for the `rgx` regex engine. It provides
/// a familiar interface similar to other regex libraries while offering
/// unprecedented performance and multi-language code execution.
pub struct Regex {
    pattern: CompiledPattern,
    engine: Engine,
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
    pub fn compile(pattern: &str) -> Result<Self> {
        let compiled = Compiler::new().compile(pattern)?;
        let engine = Engine::new(&compiled)?;
        
        Ok(Self {
            pattern: compiled,
            engine,
        })
    }

    /// Compile a regex with specific execution mode.
    ///
    /// This allows you to control the performance/feature tradeoff:
    /// - `ExecutionMode::Pure`: Maximum performance, no code execution
    /// - `ExecutionMode::Safe`: Code execution in sandboxed environments only
    /// - `ExecutionMode::Full`: All features enabled, including native callbacks
    pub fn with_mode(pattern: &str, mode: ExecutionMode) -> Result<Self> {
        let compiled = Compiler::with_mode(mode).compile(pattern)?;
        let engine = Engine::new(&compiled)?;
        
        Ok(Self {
            pattern: compiled,
            engine,
        })
    }

    /// Find all matches in the given text.
    ///
    /// This method is optimized for bulk processing and will use SIMD
    /// instructions when beneficial.
    pub fn find_all(&self, text: &str) -> Vec<MatchResult> {
        self.engine.find_all(text.as_bytes())
    }

    /// Find the first match in the given text.
    ///
    /// Optimized for early termination when only one match is needed.
    pub fn find_first(&self, text: &str) -> Option<MatchResult> {
        self.engine.find_first(text.as_bytes())
    }

    /// Test if the pattern matches the text (boolean result only).
    ///
    /// This is the fastest possible operation as it can terminate as soon
    /// as any match is found without capturing details.
    pub fn is_match(&self, text: &str) -> bool {
        self.engine.is_match(text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_regex_compilation() {
        let regex = Regex::compile(r"\d+").expect("Failed to compile simple regex");
        assert!(regex.is_match("123"));
        assert!(!regex.is_match("abc"));
    }

    #[test]
    fn email_pattern_matching() {
        let regex = Regex::compile(r"\b\w+@\w+\.\w+\b")
            .expect("Failed to compile email regex");
        
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
}
