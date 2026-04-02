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
pub mod ast;
pub mod compiler;
pub mod engine;
pub mod execution;
pub mod lexer;
pub mod parser;
pub mod parsing;
pub mod pattern;
pub mod token;
mod unicode_support;
pub mod vm;

// Performance optimizations
pub mod cache;
pub mod simd;

// Code execution backends
#[cfg(feature = "javascript")]
pub mod javascript;
#[cfg(feature = "lua")]
pub mod lua;
#[cfg(feature = "rhai")]
pub mod rhai;
#[cfg(feature = "wasm")]
pub mod wasm;

// Error handling
pub mod error;

// Logging system
pub mod log;

// Re-exports for convenience
pub use compiler::Compiler;
pub use engine::{Engine, ExecutionMode, MatchResult};
pub use error::{Result, RgxError};
pub use execution::{CodeBlockValue, ExecContext, ExecResult};
pub use pattern::{CompiledPattern, Pattern};

/// High-performance regex matcher with optional code execution capabilities.
///
/// This is the main entry point for the `rgx` regex engine. It provides
/// a familiar interface similar to other regex libraries while offering
/// unprecedented performance and multi-language code execution.
pub struct Regex {
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

        let regex = Self { engine };
        trace_exit!("api", "Regex::compile", "ok=true");
        Ok(regex)
    }

    /// Compile a regex with specific execution mode.
    ///
    /// This allows you to control the performance/feature tradeoff:
    /// - `ExecutionMode::Pure`: Maximum performance, no code execution
    /// - `ExecutionMode::Safe`: Code execution in sandboxed environments only
    /// - `ExecutionMode::Full`: enables the native-callback path in addition to the sandboxed backends
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

        let regex = Self { engine };
        trace_exit!("api", "Regex::with_mode", "ok=true");
        Ok(regex)
    }

    /// Compile a regex directly from a pre-built AST.
    ///
    /// This enables parser-independent development, testing, and benchmarking
    /// of the compiler/VM/engine pipeline.
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

        let regex = Self { engine };
        trace_exit!("api", "Regex::from_ast", "ok=true");
        Ok(regex)
    }

    /// Compile a regex from AST using a specific execution mode.
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

        let regex = Self { engine };
        trace_exit!("api", "Regex::from_ast_with_mode", "ok=true");
        Ok(regex)
    }

    /// Find all matches in the given text.
    ///
    /// This method is optimized for bulk processing and will use SIMD
    /// instructions when beneficial.
    pub fn find_all(&self, text: &str) -> Vec<MatchResult> {
        trace_enter!("api", "Regex::find_all", "text_len={}", text.len());
        let matches = self.engine.find_all(text.as_bytes());
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
    pub fn find_first(&self, text: &str) -> Option<MatchResult> {
        trace_enter!("api", "Regex::find_first", "text_len={}", text.len());
        let first = self.engine.find_first(text.as_bytes());
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

    /// Replace the first match using a winning-path `CodeBlockValue::Replacement`.
    ///
    /// Matches that do not surface a replacement payload are copied through
    /// unchanged, which keeps this API safe to use with mixed predicate and
    /// replacement-style code-block patterns.
    pub fn replace_first_with_code(&self, text: &str) -> String {
        trace_enter!(
            "api",
            "Regex::replace_first_with_code",
            "text_len={}",
            text.len()
        );
        let replaced = if let Some(first) = self.find_first(text) {
            self.apply_code_replacements(text, std::iter::once(first))
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
    pub fn replace_all_with_code(&self, text: &str) -> String {
        trace_enter!(
            "api",
            "Regex::replace_all_with_code",
            "text_len={}",
            text.len()
        );
        let replaced = self.apply_code_replacements(text, self.find_all(text));
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
    pub fn find_all_numeric_with_code(&self, text: &str) -> Vec<f64> {
        trace_enter!(
            "api",
            "Regex::find_all_numeric_with_code",
            "text_len={}",
            text.len()
        );
        let numeric = self.collect_numeric_code_results(self.find_all(text));
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
    pub fn is_match(&self, text: &str) -> bool {
        trace_enter!("api", "Regex::is_match", "text_len={}", text.len());
        let matched = self.engine.is_match(text.as_bytes());
        trace_decision!(
            "api",
            "engine.is_match(text)",
            matched,
            "boolean API match result"
        );
        trace_exit!("api", "Regex::is_match", "ok=true,matched={}", matched);
        matched
    }

    /// Register a native callback for `(?{native:...})` code blocks on this compiled regex.
    pub fn register_native<F>(&self, name: impl Into<String>, callback: F) -> Result<()>
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        let name = name.into();
        trace_enter!("api", "Regex::register_native", "name={}", name);
        let result = self.engine.register_native(name, callback);
        trace_exit!("api", "Regex::register_native", "ok={}", result.is_ok());
        result
    }

    /// Register a named wasm module for `(?{wasm:module:function})` code blocks on this compiled regex.
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
        let result = self.engine.set_variable(name, value);
        trace_exit!("api", "Regex::set_variable", "ok={}", result.is_ok());
        result
    }

    fn apply_code_replacements<I>(&self, text: &str, matches: I) -> String
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

    fn collect_numeric_code_results<I>(&self, matches: I) -> Vec<f64>
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
    fn parser_backreference_to_missing_group_reports_compile_error() {
        let result = Regex::compile(r"(a)\2");
        assert!(
            result.is_err(),
            "Backreference to a missing group should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains(r"backreference '\2' refers to missing capture group"));
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
}
