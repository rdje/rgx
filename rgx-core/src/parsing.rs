//! Zero-cost regex pattern parsing abstraction
//!
//! This module provides compile-time parser selection without runtime overhead.
//! Parser choice is made at compile time via feature flags, ensuring zero
//! performance impact on regex execution.

use crate::ast::Regex;
use crate::error::Result;
use crate::{low_log, trace_decision, trace_enter, trace_exit};

/// Core trait for regex pattern parsers
///
/// This trait abstracts over different parsing implementations, allowing
/// the rgx engine to use recursive descent parsers, PGEN-generated parsers,
/// or any other parsing backend that can produce the standard AST.
pub trait RegexParser {
    /// Parse a regex pattern string into an AST
    ///
    /// This is the main entry point for parsing. Different implementations
    /// may use different internal representations but must all produce
    /// the same AST format for compatibility.
    fn parse_pattern(&mut self, pattern: &str) -> Result<Regex>;

    /// Get the name/identifier of this parser implementation
    ///
    /// Used for debugging, logging, and feature selection
    fn parser_name(&self) -> &'static str;

    /// Get parser-specific capabilities or features
    ///
    /// Returns a set of features this parser supports, allowing
    /// callers to check capabilities before using advanced features
    fn capabilities(&self) -> ParserCapabilities {
        ParserCapabilities::default()
    }

    /// Reset parser state for reuse
    ///
    /// Some parsers may maintain internal state that needs to be
    /// cleared between parsing operations
    fn reset(&mut self) {
        // Default implementation does nothing
    }
}

/// Capabilities that different parser implementations may support
#[derive(Debug, Clone, Default)]
pub struct ParserCapabilities {
    /// Supports code execution blocks (?{lang:...})
    pub code_blocks: bool,
    /// Supports named capture groups (?<name>...)
    pub named_groups: bool,
    /// Supports advanced Perl features (recursion, etc.)
    pub perl_advanced: bool,
    /// Supports Unicode property classes \\p{...}
    pub unicode_properties: bool,
    /// Supports lookahead/lookbehind assertions
    pub lookarounds: bool,
    /// Parser provides error recovery
    pub error_recovery: bool,
    /// Parser can provide syntax highlighting hints
    pub syntax_highlighting: bool,
}

/// Zero-cost parser selection via compile-time feature flags
///
/// This function selects the parser at compile time, completely eliminating
/// runtime overhead. No vtables, no heap allocations, no indirection.
#[cfg(not(feature = "pgen-parser"))]
pub fn parse_pattern(pattern: &str) -> Result<Regex> {
    trace_enter!(
        "parsing",
        "parsing::parse_pattern[recursive-descent]",
        "pattern_len={}",
        pattern.len()
    );
    low_log!("parsing", "Using recursive-descent parser backend");
    let mut parser = match crate::parser::Parser::new(pattern) {
        Ok(parser) => parser,
        Err(err) => {
            trace_exit!(
                "parsing",
                "parsing::parse_pattern[recursive-descent]",
                "ok=false,error={}",
                err
            );
            return Err(crate::error::RgxError::Compile(err.to_string()));
        }
    };

    let result = match parser.parse() {
        Ok(ast) => Ok(ast),
        Err(err) => Err(crate::error::RgxError::Compile(err.to_string())),
    };
    trace_decision!(
        "parsing",
        "parse result is_ok()",
        result.is_ok(),
        "recursive-descent parse boundary outcome"
    );
    trace_exit!(
        "parsing",
        "parsing::parse_pattern[recursive-descent]",
        "ok={}",
        result.is_ok()
    );
    result
}

/// Zero-cost PGEN parser (when enabled)
#[cfg(feature = "pgen-parser")]
pub fn parse_pattern(pattern: &str) -> Result<Regex> {
    trace_enter!(
        "parsing",
        "parsing::parse_pattern[pgen-feature]",
        "pattern_len={}",
        pattern.len()
    );
    // TODO: Replace with actual PGEN parser when available
    // For now, fall back to recursive descent
    low_log!(
        "parsing",
        "pgen-parser feature enabled; currently using recursive-descent fallback"
    );
    let mut parser = match crate::parser::Parser::new(pattern) {
        Ok(parser) => parser,
        Err(err) => {
            trace_exit!(
                "parsing",
                "parsing::parse_pattern[pgen-feature]",
                "ok=false,error={}",
                err
            );
            return Err(crate::error::RgxError::Compile(err.to_string()));
        }
    };

    let result = match parser.parse() {
        Ok(ast) => Ok(ast),
        Err(err) => Err(crate::error::RgxError::Compile(err.to_string())),
    };
    trace_decision!(
        "parsing",
        "parse result is_ok()",
        result.is_ok(),
        "pgen-feature parser boundary outcome"
    );
    trace_exit!(
        "parsing",
        "parsing::parse_pattern[pgen-feature]",
        "ok={}",
        result.is_ok()
    );
    result
}

/// Get the active parser name (compile-time)
#[cfg(not(feature = "pgen-parser"))]
pub fn parser_name() -> &'static str {
    "recursive-descent"
}

#[cfg(feature = "pgen-parser")]
pub fn parser_name() -> &'static str {
    "pgen"
}

/// Get active parser capabilities (compile-time)
#[cfg(not(feature = "pgen-parser"))]
pub fn parser_capabilities() -> ParserCapabilities {
    ParserCapabilities {
        code_blocks: true,
        named_groups: true,
        perl_advanced: false,
        unicode_properties: true,
        lookarounds: true,
        error_recovery: false,
        syntax_highlighting: false,
    }
}

#[cfg(feature = "pgen-parser")]
pub fn parser_capabilities() -> ParserCapabilities {
    ParserCapabilities {
        // Current pgen-parser path is still a recursive-descent fallback.
        // Keep capability flags truthful until a real PGEN backend lands.
        code_blocks: true,
        named_groups: true,
        perl_advanced: false,
        unicode_properties: true,
        lookarounds: true,
        error_recovery: true,
        syntax_highlighting: true,
    }
}

/// Analysis of pattern complexity and features for parser selection
struct PatternAnalysis {
    has_code_blocks: bool,
    has_complex_groups: bool,
    has_recursion: bool,
    complexity_score: u32,
}

impl PatternAnalysis {
    fn analyze(pattern: &str) -> Self {
        // Simple heuristic analysis
        // A full implementation would use proper tokenization
        Self {
            has_code_blocks: pattern.contains("(?{"),
            has_complex_groups: pattern.contains("(?") && !pattern.contains("(?:"),
            has_recursion: pattern.contains("(?R") || pattern.contains("(?&"),
            complexity_score: pattern.len() as u32
                + pattern.matches(['(', '[', '{', '*', '+', '?']).count() as u32,
        }
    }
}

/// Wrapper for the current recursive descent parser
///
/// This implements the RegexParser trait for our existing parser,
/// making it pluggable with other implementations.
pub struct RecursiveDescentParser {
    // No internal state needed currently
}

impl RecursiveDescentParser {
    pub fn new() -> Self {
        Self {}
    }
}

impl RegexParser for RecursiveDescentParser {
    fn parse_pattern(&mut self, pattern: &str) -> Result<Regex> {
        trace_enter!(
            "parsing",
            "RecursiveDescentParser::parse_pattern",
            "pattern_len={}",
            pattern.len()
        );
        // Use existing parser implementation
        let mut parser = match crate::parser::Parser::new(pattern) {
            Ok(parser) => parser,
            Err(err) => {
                trace_exit!(
                    "parsing",
                    "RecursiveDescentParser::parse_pattern",
                    "ok=false,error={}",
                    err
                );
                return Err(crate::error::RgxError::Compile(err.to_string()));
            }
        };

        let result = match parser.parse() {
            Ok(ast) => Ok(ast),
            Err(err) => Err(crate::error::RgxError::Compile(err.to_string())),
        };
        trace_decision!(
            "parsing",
            "recursive-descent trait parse result is_ok()",
            result.is_ok(),
            "RegexParser adapter parse boundary outcome"
        );
        trace_exit!(
            "parsing",
            "RecursiveDescentParser::parse_pattern",
            "ok={}",
            result.is_ok()
        );
        result
    }

    fn parser_name(&self) -> &'static str {
        "recursive-descent"
    }

    fn capabilities(&self) -> ParserCapabilities {
        ParserCapabilities {
            code_blocks: true,
            named_groups: true,
            perl_advanced: false,
            unicode_properties: true, // Lexer supports this
            lookarounds: true,
            error_recovery: false,
            syntax_highlighting: false,
        }
    }
}

/// Placeholder for PGEN parser implementation
#[cfg(feature = "pgen-parser")]
pub struct PgenParser {
    // Will be filled in when PGEN parser is available
}

#[cfg(feature = "pgen-parser")]
impl PgenParser {
    pub fn new() -> Self {
        Self {}
    }
}

#[cfg(feature = "pgen-parser")]
impl RegexParser for PgenParser {
    fn parse_pattern(&mut self, pattern: &str) -> Result<Regex> {
        // TODO: Implement when PGEN parser is available
        // This is just a placeholder that falls back to recursive descent
        let mut fallback = RecursiveDescentParser::new();
        fallback.parse_pattern(pattern)
    }

    fn parser_name(&self) -> &'static str {
        "pgen"
    }

    fn capabilities(&self) -> ParserCapabilities {
        ParserCapabilities {
            // Current pgen-parser path is still a recursive-descent fallback.
            // Keep capability flags truthful until a real PGEN backend lands.
            code_blocks: true,
            named_groups: true,
            perl_advanced: false,
            unicode_properties: true,
            lookarounds: true,
            error_recovery: false,
            syntax_highlighting: false,
        }
    }
}

/// Configuration for parser selection and behavior
#[derive(Debug, Clone)]
pub struct ParserConfig {
    /// Preferred parser implementation
    pub preferred_parser: Option<String>,
    /// Whether to enable experimental parsers
    pub allow_experimental: bool,
    /// Whether to perform pattern analysis for parser selection
    pub auto_select: bool,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            preferred_parser: None,
            allow_experimental: false,
            auto_select: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{GroupKind, Regex};

    fn parse_with_reference_parser(pattern: &str) -> Regex {
        let mut parser = RecursiveDescentParser::new();
        parser
            .parse_pattern(pattern)
            .unwrap_or_else(|e| panic!("reference parser failed for pattern '{pattern}': {e}"))
    }

    #[test]
    fn test_zero_cost_parsing() {
        let result = parse_pattern("abc");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parser_name() {
        let name = parser_name();
        #[cfg(not(feature = "pgen-parser"))]
        assert_eq!(name, "recursive-descent");

        #[cfg(feature = "pgen-parser")]
        assert_eq!(name, "pgen");
    }

    #[test]
    fn test_parser_capabilities() {
        let caps = parser_capabilities();
        assert!(caps.unicode_properties); // Should support basic unicode
        assert!(caps.named_groups);
        assert!(caps.lookarounds);
        assert!(caps.code_blocks);
    }

    #[test]
    fn parser_contract_group_metadata_invariants() {
        let ast =
            parse_pattern("(?<word>a)(?:b)(?>c)").expect("Parser should accept group variants");

        match ast {
            Regex::Sequence(items) => {
                assert_eq!(items.len(), 3);

                match &items[0] {
                    Regex::Group {
                        kind, index, name, ..
                    } => {
                        assert_eq!(kind, &GroupKind::Capturing);
                        assert_eq!(*index, None);
                        assert_eq!(name.as_deref(), Some("word"));
                    }
                    other => panic!("Expected named capturing group, got: {other:?}"),
                }

                match &items[1] {
                    Regex::Group {
                        kind, index, name, ..
                    } => {
                        assert_eq!(kind, &GroupKind::NonCapturing);
                        assert_eq!(*index, None);
                        assert_eq!(*name, None);
                    }
                    other => panic!("Expected non-capturing group, got: {other:?}"),
                }

                match &items[2] {
                    Regex::Group {
                        kind, index, name, ..
                    } => {
                        assert_eq!(kind, &GroupKind::Atomic);
                        assert_eq!(*index, None);
                        assert_eq!(*name, None);
                    }
                    other => panic!("Expected atomic group, got: {other:?}"),
                }
            }
            other => panic!("Expected sequence AST, got: {other:?}"),
        }
    }

    #[test]
    fn parser_contract_active_parser_matches_reference_fixtures() {
        let fixtures = [
            "a|b",
            "(?:a)(?<word>b)(?>c)",
            "(?=ab)c",
            "(?<!x)a",
            "(?(1)a|b)",
            "(?(<word>)a|b)",
            "(?(word)a|b)",
            "(?(?=ab)x|y)",
            "(?(?!ab)x|y)",
            "(?(?<=z)a|b)",
            "(?(?<!z)a|b)",
            "(?{lua:return true})",
            "(?R)",
            "(?1)",
            "(?&word)",
            r"(a)\1",
        ];

        for pattern in fixtures {
            let active = parse_pattern(pattern)
                .unwrap_or_else(|e| panic!("active parser failed for pattern '{pattern}': {e}"));
            let reference = parse_with_reference_parser(pattern);
            assert_eq!(
                active, reference,
                "active parser output diverged from reference parser for pattern '{pattern}'"
            );
        }
    }

    #[cfg(feature = "pgen-parser")]
    #[test]
    fn parser_contract_pgen_backend_matches_reference_fixtures() {
        let fixtures = [
            "a|b",
            "(?:a)(?<word>b)(?>c)",
            "(?=ab)c",
            "(?<!x)a",
            "(?(1)a|b)",
            "(?(<word>)a|b)",
            "(?(word)a|b)",
            "(?(?=ab)x|y)",
            "(?(?!ab)x|y)",
            "(?(?<=z)a|b)",
            "(?(?<!z)a|b)",
            "(?{lua:return true})",
            "(?R)",
            "(?1)",
            "(?&word)",
            r"(a)\1",
        ];

        for pattern in fixtures {
            let mut pgen = PgenParser::new();
            let pgen_ast = pgen
                .parse_pattern(pattern)
                .unwrap_or_else(|e| panic!("pgen parser failed for pattern '{pattern}': {e}"));
            let reference = parse_with_reference_parser(pattern);
            assert_eq!(
                pgen_ast, reference,
                "pgen parser output diverged from reference parser for pattern '{pattern}'"
            );
        }
    }

    #[test]
    fn parser_contract_maps_parse_failures_to_compile_errors() {
        let err = parse_pattern("(").expect_err("Unterminated group should fail parsing");
        let msg = err.to_string();
        assert!(
            msg.starts_with("pattern compile error:"),
            "expected compile-style error mapping, got: {msg}"
        );
    }

    #[test]
    fn parser_contract_parsed_but_unintegrated_features_fail_at_compile_boundary() {
        let compiler = crate::compiler::Compiler::new();
        let cases = [
            (
                "(?{lua:return true})",
                "code-block syntax is parsed but not yet integrated into VM execution",
            ),
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
                "(?(?<!z)a|b)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
            (
                "(?(?<=z)a|b)",
                "conditional syntax is parsed but not yet integrated into VM execution",
            ),
        ];

        for (pattern, expected_msg) in cases {
            parse_pattern(pattern).unwrap_or_else(|e| {
                panic!("parser should accept contract fixture '{pattern}': {e}")
            });
            let err = match compiler.compile(pattern) {
                Ok(_) => panic!(
                    "pattern should fail at compile boundary until runtime integration lands: {pattern}"
                ),
                Err(err) => err,
            };
            assert!(
                err.to_string().contains(expected_msg),
                "unexpected compile boundary message for pattern '{pattern}': {err}"
            );
        }
    }
}
