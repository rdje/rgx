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
pub mod vm;

// Performance optimizations
pub mod cache;
pub mod simd;

// Code execution backends
#[cfg(feature = "javascript")]
pub mod javascript;
#[cfg(feature = "lua")]
pub mod lua;
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
pub use pattern::{CompiledPattern, Pattern};

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
            pattern: compiled,
            engine,
        };
        trace_exit!("api", "Regex::compile", "ok=true");
        Ok(regex)
    }

    /// Compile a regex with specific execution mode.
    ///
    /// This allows you to control the performance/feature tradeoff:
    /// - `ExecutionMode::Pure`: Maximum performance, no code execution
    /// - `ExecutionMode::Safe`: Code execution in sandboxed environments only
    /// - `ExecutionMode::Full`: All features enabled, including native callbacks
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
            pattern: compiled,
            engine,
        };
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

        let regex = Self {
            pattern: compiled,
            engine,
        };
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

        let regex = Self {
            pattern: compiled,
            engine,
        };
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
    fn parser_code_block_syntax_reports_explicit_unsupported_error() {
        let result = Regex::compile("(?{lua:return true})");
        assert!(result.is_err(), "Code block should not silently compile");
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("code-block syntax is parsed but not yet integrated into VM execution")
        );
    }

    #[test]
    fn parser_backreference_syntax_reports_explicit_unsupported_error() {
        let result = Regex::compile(r"(a)\1");
        assert!(result.is_err(), "Backreference should not silently compile");
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("backreferences are parsed but not yet integrated into VM execution"));
    }

    #[test]
    fn parser_recursion_syntax_reports_explicit_unsupported_error() {
        let result = Regex::compile("(?R)");
        assert!(result.is_err(), "Recursion should not silently compile");
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(msg.contains("recursion syntax is parsed but not yet integrated into VM execution"));
    }

    #[test]
    fn parser_conditional_syntax_reports_explicit_unsupported_error() {
        let result = Regex::compile("(?(1)a|b)");
        assert!(result.is_err(), "Conditional should not silently compile");
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("conditional syntax is parsed but not yet integrated into VM execution")
        );
    }

    #[test]
    fn parser_conditional_lookahead_syntax_reports_explicit_unsupported_error() {
        let result = Regex::compile("(?(?=ab)x|y)");
        assert!(
            result.is_err(),
            "Lookahead conditional should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("conditional syntax is parsed but not yet integrated into VM execution")
        );
    }

    #[test]
    fn parser_conditional_negative_lookbehind_syntax_reports_explicit_unsupported_error() {
        let result = Regex::compile("(?(?<!z)a|b)");
        assert!(
            result.is_err(),
            "Negative lookbehind conditional should not silently compile"
        );
        let msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("conditional syntax is parsed but not yet integrated into VM execution")
        );
    }

    #[test]
    fn capability_matrix_supported_parser_path_cases() {
        let cases = [
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
            ("(?<word>cat)", "xxcatyy", true),
            ("(?>ab|a)c", "abc", true),
            ("(?!cat)c", "car", true),
            ("(?!cat)c", "cat", false),
            ("(?<=x)a", "xa", true),
            ("(?<=x)a", "ba", false),
            ("(?<!x)a", "ba", true),
            ("(?=cat)c", "xxcat", true),
            ("(?<!x)a", "xa", false),
            (r"\Acat", "cat dog", true),
            (r"\Acat", "xxcat", false),
            ("dog$", "cat dog", true),
            ("dog$", "cat dog x", false),
            (r"dog\z", "cat dog", true),
            (r"dog\z", "cat dog\n", false),
            (r"dog\Z", "cat dog\n", true),
            (r"dog\Z", "cat dog\nx", false),
        ];

        for (pattern, input, expected) in cases {
            let regex = Regex::compile(pattern).unwrap_or_else(|e| {
                panic!("expected supported pattern '{pattern}' to compile: {e}")
            });
            assert_eq!(
                regex.is_match(input),
                expected,
                "unexpected match result for supported pattern '{pattern}' on input '{input}'"
            );
        }
    }

    #[test]
    fn capability_matrix_explicit_unsupported_compile_boundary_cases() {
        let cases = [
            (
                r"(a)\1",
                "backreferences are parsed but not yet integrated into VM execution",
            ),
            (
                "(?R)",
                "recursion syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?1)",
                "recursion syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?&word)",
                "recursion syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?{lua:return true})",
                "code-block syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?(1)a|b)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?(<word>)a|b)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?(word)a|b)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?(?=ab)x|y)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?(?!ab)x|y)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?(?<=z)a|b)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?(?<!z)a|b)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
        ];

        for (pattern, expected_msg) in cases {
            let err = match Regex::compile(pattern) {
                Ok(_) => panic!(
                    "expected pattern to be rejected at explicit compile boundary: {pattern}"
                ),
                Err(err) => err,
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
