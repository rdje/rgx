//! Zero-cost regex pattern parsing abstraction
//!
//! This module provides compile-time parser selection without runtime overhead.
//! Parser choice is made at compile time via feature flags, ensuring zero
//! performance impact on regex execution.

use crate::ast::Regex;
use crate::error::Result;

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
    let mut parser = crate::parser::Parser::new(pattern)
        .map_err(|e| crate::error::RgxError::Compile(e.to_string()))?;
    parser.parse().map_err(|e| crate::error::RgxError::Compile(e.to_string()))
}

/// Zero-cost PGEN parser (when enabled)
#[cfg(feature = "pgen-parser")]
pub fn parse_pattern(pattern: &str) -> Result<Regex> {
    // TODO: Replace with actual PGEN parser when available
    // For now, fall back to recursive descent
    let mut parser = crate::parser::Parser::new(pattern)
        .map_err(|e| crate::error::RgxError::Compile(e.to_string()))?;
    parser.parse().map_err(|e| crate::error::RgxError::Compile(e.to_string()))
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
        code_blocks: false,
        named_groups: false,
        perl_advanced: false,
        unicode_properties: true,
        lookarounds: false,
        error_recovery: false,
        syntax_highlighting: false,
    }
}

#[cfg(feature = "pgen-parser")]
pub fn parser_capabilities() -> ParserCapabilities {
    ParserCapabilities {
        code_blocks: true,
        named_groups: true,
        perl_advanced: true,
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
            complexity_score: pattern.len() as u32 + pattern.matches(['(', '[', '{', '*', '+', '?']).count() as u32,
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
        // Use existing parser implementation
        let mut parser = crate::parser::Parser::new(pattern)
            .map_err(|e| crate::error::RgxError::Compile(e.to_string()))?;
        parser.parse().map_err(|e| crate::error::RgxError::Compile(e.to_string()))
    }

    fn parser_name(&self) -> &'static str {
        "recursive-descent"
    }

    fn capabilities(&self) -> ParserCapabilities {
        ParserCapabilities {
            code_blocks: false, // Not implemented yet
            named_groups: false, // Not implemented yet
            perl_advanced: false,
            unicode_properties: true, // Lexer supports this
            lookarounds: false, // Not implemented yet
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
            code_blocks: true, // PGEN parser will support all features
            named_groups: true,
            perl_advanced: true,
            unicode_properties: true,
            lookarounds: true,
            error_recovery: true,
            syntax_highlighting: true,
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
    }
}
