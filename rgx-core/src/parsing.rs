//! Zero-cost regex pattern parsing abstraction
//!
//! This module provides compile-time parser selection without runtime overhead.
//! Parser choice is made at compile time via feature flags, ensuring zero
//! performance impact on regex execution.

use crate::ast::Regex;
use crate::error::Result;
#[cfg(feature = "pgen-parser")]
use crate::{
    ast::{ConditionalTest, GroupKind, Quantifier},
    error::RgxError,
};
use crate::{low_log, trace_decision, trace_enter, trace_exit};
#[cfg(feature = "pgen-parser")]
use pgen::embedding_api::{
    parse_regex_default_ast_dump, parser_embedding_api_contract, AstDumpOptions, ParseStatus,
};
#[cfg(feature = "pgen-parser")]
use serde::Deserialize;

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
    let result = match PGEN_FEATURE_BACKEND {
        PgenFeatureBackend::Pgen => {
            low_log!("parsing", "pgen-parser feature enabled; using PGEN backend");
            let mut parser = PgenParser::new();
            parser.parse_pattern(pattern)
        }
        PgenFeatureBackend::RecursiveDescent => {
            low_log!(
                "parsing",
                "pgen-parser feature enabled; local switch is forcing recursive-descent backend"
            );
            let mut parser = RecursiveDescentParser::new();
            parser.parse_pattern(pattern)
        }
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
    trace_enter!("parsing", "parsing::parser_name[recursive-descent]");
    let name = "recursive-descent";
    trace_exit!(
        "parsing",
        "parsing::parser_name[recursive-descent]",
        "ok=true,name={}",
        name
    );
    name
}

#[cfg(feature = "pgen-parser")]
pub fn parser_name() -> &'static str {
    trace_enter!("parsing", "parsing::parser_name[pgen-feature]");
    let name = match PGEN_FEATURE_BACKEND {
        PgenFeatureBackend::Pgen => "pgen",
        PgenFeatureBackend::RecursiveDescent => "recursive-descent",
    };
    trace_exit!(
        "parsing",
        "parsing::parser_name[pgen-feature]",
        "ok=true,name={}",
        name
    );
    name
}

/// Get active parser capabilities (compile-time)
#[cfg(not(feature = "pgen-parser"))]
pub fn parser_capabilities() -> ParserCapabilities {
    trace_enter!("parsing", "parsing::parser_capabilities[recursive-descent]");
    let capabilities = ParserCapabilities {
        code_blocks: true,
        named_groups: true,
        perl_advanced: false,
        unicode_properties: true,
        lookarounds: true,
        error_recovery: false,
        syntax_highlighting: false,
    };
    trace_decision!(
        "parsing",
        "capabilities.perl_advanced",
        capabilities.perl_advanced,
        "recursive-descent advanced perl support flag"
    );
    trace_exit!(
        "parsing",
        "parsing::parser_capabilities[recursive-descent]",
        "ok=true,code_blocks={},named_groups={},lookarounds={},unicode_properties={},perl_advanced={},error_recovery={},syntax_highlighting={}",
        capabilities.code_blocks,
        capabilities.named_groups,
        capabilities.lookarounds,
        capabilities.unicode_properties,
        capabilities.perl_advanced,
        capabilities.error_recovery,
        capabilities.syntax_highlighting
    );
    capabilities
}

#[cfg(feature = "pgen-parser")]
pub fn parser_capabilities() -> ParserCapabilities {
    trace_enter!("parsing", "parsing::parser_capabilities[pgen-feature]");
    let capabilities = match PGEN_FEATURE_BACKEND {
        PgenFeatureBackend::Pgen => ParserCapabilities {
            code_blocks: true,
            named_groups: true,
            perl_advanced: false,
            unicode_properties: true,
            lookarounds: true,
            error_recovery: false,
            syntax_highlighting: false,
        },
        PgenFeatureBackend::RecursiveDescent => ParserCapabilities {
            code_blocks: true,
            named_groups: true,
            perl_advanced: false,
            unicode_properties: true,
            lookarounds: true,
            error_recovery: false,
            syntax_highlighting: false,
        },
    };
    trace_decision!(
        "parsing",
        "capabilities.perl_advanced",
        capabilities.perl_advanced,
        "pgen-feature advanced perl support flag"
    );
    trace_exit!(
        "parsing",
        "parsing::parser_capabilities[pgen-feature]",
        "ok=true,code_blocks={},named_groups={},lookarounds={},unicode_properties={},perl_advanced={},error_recovery={},syntax_highlighting={}",
        capabilities.code_blocks,
        capabilities.named_groups,
        capabilities.lookarounds,
        capabilities.unicode_properties,
        capabilities.perl_advanced,
        capabilities.error_recovery,
        capabilities.syntax_highlighting
    );
    capabilities
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
        trace_enter!("parsing", "RecursiveDescentParser::new");
        let parser = Self {};
        trace_exit!("parsing", "RecursiveDescentParser::new", "ok=true");
        parser
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
        trace_enter!("parsing", "RecursiveDescentParser::parser_name");
        let name = "recursive-descent";
        trace_exit!(
            "parsing",
            "RecursiveDescentParser::parser_name",
            "ok=true,name={}",
            name
        );
        name
    }

    fn capabilities(&self) -> ParserCapabilities {
        trace_enter!("parsing", "RecursiveDescentParser::capabilities");
        let capabilities = ParserCapabilities {
            code_blocks: true,
            named_groups: true,
            perl_advanced: false,
            unicode_properties: true, // Lexer supports this
            lookarounds: true,
            error_recovery: false,
            syntax_highlighting: false,
        };
        trace_exit!(
            "parsing",
            "RecursiveDescentParser::capabilities",
            "ok=true,code_blocks={},named_groups={},lookarounds={},unicode_properties={},perl_advanced={},error_recovery={},syntax_highlighting={}",
            capabilities.code_blocks,
            capabilities.named_groups,
            capabilities.lookarounds,
            capabilities.unicode_properties,
            capabilities.perl_advanced,
            capabilities.error_recovery,
            capabilities.syntax_highlighting
        );
        capabilities
    }
}

/// Placeholder for PGEN parser implementation
#[cfg(feature = "pgen-parser")]
pub struct PgenParser {
    // PGEN is stateless per call; this type is just an adapter shell.
}

#[cfg(feature = "pgen-parser")]
impl PgenParser {
    pub fn new() -> Self {
        trace_enter!("parsing", "PgenParser::new");
        let parser = Self {};
        trace_exit!("parsing", "PgenParser::new", "ok=true");
        parser
    }
}

#[cfg(feature = "pgen-parser")]
impl RegexParser for PgenParser {
    fn parse_pattern(&mut self, pattern: &str) -> Result<Regex> {
        trace_enter!(
            "parsing",
            "PgenParser::parse_pattern",
            "pattern_len={}",
            pattern.len()
        );

        let contract = parser_embedding_api_contract();
        if !contract.supports_regex_generated_backend {
            let err = RgxError::Compile(
                "pgen regex generated backend is unavailable; enable the generated backend before using rgx's pgen-parser feature"
                    .to_string(),
            );
            trace_exit!(
                "parsing",
                "PgenParser::parse_pattern",
                "ok=false,error={}",
                err
            );
            return Err(err);
        }
        if contract.regex_ast_dump_schema_version != 1 {
            let err = RgxError::Compile(format!(
                "pgen regex AST-dump schema {} is unsupported by rgx; expected schema 1",
                contract.regex_ast_dump_schema_version
            ));
            trace_exit!(
                "parsing",
                "PgenParser::parse_pattern",
                "ok=false,error={}",
                err
            );
            return Err(err);
        }
        if !version_at_least(&contract.regex_parser_release_version, (1, 1, 1)) {
            let err = RgxError::Compile(format!(
                "pgen regex parser release {} is too old for rgx integration; require at least 1.1.1",
                contract.regex_parser_release_version
            ));
            trace_exit!(
                "parsing",
                "PgenParser::parse_pattern",
                "ok=false,error={}",
                err
            );
            return Err(err);
        }
        if !version_at_least(&contract.regex_integration_contract_version, (1, 1, 1)) {
            let err = RgxError::Compile(format!(
                "pgen regex integration contract {} is too old for rgx integration; require at least 1.1.1",
                contract.regex_integration_contract_version
            ));
            trace_exit!(
                "parsing",
                "PgenParser::parse_pattern",
                "ok=false,error={}",
                err
            );
            return Err(err);
        }

        let dump_outcome = parse_regex_default_ast_dump(
            pattern,
            &AstDumpOptions {
                pretty: false,
                max_ast_bytes: None,
            },
        );
        let dump = match dump_outcome.ast_dump {
            Some(dump) if dump_outcome.status == ParseStatus::Success => dump,
            _ => {
                let err = dump_outcome
                    .diagnostic
                    .map(|diagnostic| RgxError::Compile(diagnostic.to_string()))
                    .unwrap_or_else(|| {
                        RgxError::Compile("pgen AST dump failed without a diagnostic".to_string())
                    });
                trace_exit!(
                    "parsing",
                    "PgenParser::parse_pattern",
                    "ok=false,error={}",
                    err
                );
                return Err(err);
            }
        };

        let adapter = PgenAstAdapter::new(pattern);
        let result = adapter.parse_dump(&dump.dump_json);
        trace_exit!(
            "parsing",
            "PgenParser::parse_pattern",
            "ok={}",
            result.is_ok()
        );
        result
    }

    fn parser_name(&self) -> &'static str {
        trace_enter!("parsing", "PgenParser::parser_name");
        let name = "pgen";
        trace_exit!(
            "parsing",
            "PgenParser::parser_name",
            "ok=true,name={}",
            name
        );
        name
    }

    fn capabilities(&self) -> ParserCapabilities {
        trace_enter!("parsing", "PgenParser::capabilities");
        let capabilities = ParserCapabilities {
            code_blocks: true,
            named_groups: true,
            perl_advanced: false,
            unicode_properties: true,
            lookarounds: true,
            error_recovery: false,
            syntax_highlighting: false,
        };
        trace_exit!(
            "parsing",
            "PgenParser::capabilities",
            "ok=true,code_blocks={},named_groups={},lookarounds={},unicode_properties={},perl_advanced={},error_recovery={},syntax_highlighting={}",
            capabilities.code_blocks,
            capabilities.named_groups,
            capabilities.lookarounds,
            capabilities.unicode_properties,
            capabilities.perl_advanced,
            capabilities.error_recovery,
            capabilities.syntax_highlighting
        );
        capabilities
    }
}

#[cfg(feature = "pgen-parser")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PgenFeatureBackend {
    Pgen,
    RecursiveDescent,
}

#[cfg(feature = "pgen-parser")]
const PGEN_FEATURE_BACKEND: PgenFeatureBackend = PgenFeatureBackend::Pgen;

#[cfg(feature = "pgen-parser")]
#[derive(Debug, Deserialize)]
struct PgenAstNode {
    rule_name: String,
    span: PgenAstSpan,
    content: PgenAstContent,
}

#[cfg(feature = "pgen-parser")]
#[derive(Debug, Deserialize)]
struct PgenAstSpan {
    start: usize,
    end: usize,
}

#[cfg(feature = "pgen-parser")]
#[derive(Debug, Deserialize)]
enum PgenAstContent {
    Terminal(String),
    TransformedTerminal(String),
    Sequence(Vec<PgenAstNode>),
    Alternative(Box<PgenAstNode>),
    Quantified((Vec<PgenAstNode>, String)),
}

#[cfg(feature = "pgen-parser")]
struct PgenAstAdapter<'a> {
    pattern: &'a str,
}

#[cfg(feature = "pgen-parser")]
impl<'a> PgenAstAdapter<'a> {
    fn new(pattern: &'a str) -> Self {
        Self { pattern }
    }

    fn parse_dump(&self, dump_json: &str) -> Result<Regex> {
        let root: PgenAstNode = serde_json::from_str(dump_json).map_err(|err| {
            RgxError::Compile(format!("failed to decode pgen regex AST dump JSON: {err}"))
        })?;
        self.convert_root(&root)
    }

    fn convert_root(&self, node: &PgenAstNode) -> Result<Regex> {
        match node.rule_name.as_str() {
            "regex" => {
                let pattern = self.first_descendant(node, "pattern").ok_or_else(|| {
                    self.contract_error("pgen regex dump is missing the top-level pattern node")
                })?;
                self.convert_pattern(pattern)
            }
            "pattern" => self.convert_pattern(node),
            other => Err(self.contract_error(&format!("unexpected pgen root node '{other}'"))),
        }
    }

    fn convert_pattern(&self, node: &PgenAstNode) -> Result<Regex> {
        let alternation = self
            .first_descendant(node, "alternation")
            .ok_or_else(|| self.contract_error("pgen pattern node is missing alternation"))?;
        self.convert_alternation(alternation)
    }

    fn convert_alternation(&self, node: &PgenAstNode) -> Result<Regex> {
        let children = self.sequence_children(node)?;
        let mut branches = Vec::new();

        if let Some(first_branch) = children
            .first()
            .and_then(|child| self.first_descendant(child, "alternative"))
        {
            branches.push(self.convert_alternative(first_branch)?);
        }

        if let Some(rest) = children.get(1) {
            for repeated in self.quantified_children(rest)? {
                let repeated_parts = self.sequence_children(repeated)?;
                if repeated_parts.len() < 2 {
                    return Err(self.contract_error(
                        "pgen alternation repeat entry is missing a branch payload",
                    ));
                }
                let branch = self
                    .first_descendant(&repeated_parts[1], "alternative")
                    .ok_or_else(|| {
                        self.contract_error(
                            "pgen alternation repeat entry is missing an alternative node",
                        )
                    })?;
                branches.push(self.convert_alternative(branch)?);
            }
        }

        Ok(pack_alternation(branches))
    }

    fn convert_alternative(&self, node: &PgenAstNode) -> Result<Regex> {
        let Some(concatenation) = self.first_descendant(node, "concatenation") else {
            return Ok(Regex::Empty);
        };
        self.convert_concatenation(concatenation)
    }

    fn convert_concatenation(&self, node: &PgenAstNode) -> Result<Regex> {
        let mut pieces = Vec::new();
        for repeated in self.quantified_children(node)? {
            let piece = self.first_descendant(repeated, "piece").ok_or_else(|| {
                self.contract_error("pgen concatenation entry is missing a piece")
            })?;
            pieces.push(self.convert_piece(piece)?);
        }
        Ok(pack_sequence(pieces))
    }

    fn convert_piece(&self, node: &PgenAstNode) -> Result<Regex> {
        let children = self.sequence_children(node)?;
        let atom = children
            .first()
            .and_then(|child| self.first_descendant(child, "atom"))
            .ok_or_else(|| self.contract_error("pgen piece is missing its atom"))?;
        let expr = self.convert_atom(atom)?;

        let Some(quantifier_slot) = children.get(1) else {
            return Ok(expr);
        };
        if self.is_empty_wrapper(quantifier_slot) {
            return Ok(expr);
        }

        let quantifier = self
            .first_descendant(quantifier_slot, "quantifier")
            .ok_or_else(|| self.contract_error("pgen piece quantifier slot is malformed"))?;
        let (quantifier, possessive) = self.convert_quantifier(quantifier)?;
        Ok(self.wrap_quantified(expr, quantifier, possessive))
    }

    fn convert_atom(&self, node: &PgenAstNode) -> Result<Regex> {
        let actual = self.alternative_child(node).unwrap_or(node);
        match actual.rule_name.as_str() {
            "group" | "capturing_group" | "noncapturing_group" | "named_group"
            | "python_named_group" | "atomic_group" => self.convert_group(actual),
            "lookaround" | "lookahead_pos" | "lookahead_neg" | "lookbehind_pos"
            | "lookbehind_neg" => self.convert_lookaround(actual),
            "conditional" => self.convert_conditional(actual),
            _ => self.parse_leaf_fragment(actual),
        }
    }

    fn convert_group(&self, node: &PgenAstNode) -> Result<Regex> {
        let actual = if node.rule_name == "group" {
            self.alternative_child(node).ok_or_else(|| {
                self.contract_error("pgen group wrapper is missing its concrete variant")
            })?
        } else {
            node
        };

        let expr = if let Some(pattern) = self.first_descendant(actual, "pattern") {
            self.convert_pattern(pattern)?
        } else {
            Regex::Empty
        };

        let (kind, name) = match actual.rule_name.as_str() {
            "capturing_group" => (GroupKind::Capturing, None),
            "noncapturing_group" => (GroupKind::NonCapturing, None),
            "atomic_group" => (GroupKind::Atomic, None),
            "named_group" | "python_named_group" => {
                let name = self
                    .first_descendant(actual, "name")
                    .ok_or_else(|| self.contract_error("pgen named group is missing its name"))?;
                (GroupKind::Capturing, Some(self.slice(name)?.to_string()))
            }
            other => {
                return Err(
                    self.contract_error(&format!("unsupported pgen group variant '{other}'"))
                )
            }
        };

        Ok(Regex::Group {
            expr: Box::new(expr),
            kind,
            index: None,
            name,
        })
    }

    fn convert_lookaround(&self, node: &PgenAstNode) -> Result<Regex> {
        let actual = if node.rule_name == "lookaround" {
            self.alternative_child(node).ok_or_else(|| {
                self.contract_error("pgen lookaround wrapper is missing its concrete variant")
            })?
        } else {
            node
        };

        let expr = if let Some(pattern) = self.first_descendant(actual, "pattern") {
            self.convert_pattern(pattern)?
        } else {
            Regex::Empty
        };

        match actual.rule_name.as_str() {
            "lookahead_pos" => Ok(Regex::Lookahead {
                expr: Box::new(expr),
                positive: true,
            }),
            "lookahead_neg" => Ok(Regex::Lookahead {
                expr: Box::new(expr),
                positive: false,
            }),
            "lookbehind_pos" => Ok(Regex::Lookbehind {
                expr: Box::new(expr),
                positive: true,
            }),
            "lookbehind_neg" => Ok(Regex::Lookbehind {
                expr: Box::new(expr),
                positive: false,
            }),
            other => {
                Err(self.contract_error(&format!("unsupported pgen lookaround variant '{other}'")))
            }
        }
    }

    fn convert_conditional(&self, node: &PgenAstNode) -> Result<Regex> {
        let condition = self
            .first_descendant(node, "condition")
            .ok_or_else(|| self.contract_error("pgen conditional is missing its condition"))?;
        let true_branch = self
            .first_descendant(node, "yes_branch")
            .ok_or_else(|| self.contract_error("pgen conditional is missing its yes branch"))?;
        let false_branch = self.first_descendant(node, "no_branch");

        Ok(Regex::Conditional {
            condition: self.convert_condition(condition)?,
            true_branch: Box::new(self.convert_conditional_branch(true_branch)?),
            false_branch: false_branch
                .map(|branch| self.convert_conditional_branch(branch).map(Box::new))
                .transpose()?,
        })
    }

    fn convert_conditional_branch(&self, node: &PgenAstNode) -> Result<Regex> {
        let actual = if matches!(node.rule_name.as_str(), "yes_branch" | "no_branch") {
            if let Some(branch) = self.first_descendant(node, "conditional_branch") {
                branch
            } else {
                return Ok(Regex::Empty);
            }
        } else {
            node
        };

        let mut pieces = Vec::new();
        for repeated in self.quantified_children(actual)? {
            let piece = self.first_descendant(repeated, "piece").ok_or_else(|| {
                self.contract_error("pgen conditional branch entry is missing a piece")
            })?;
            pieces.push(self.convert_piece(piece)?);
        }
        Ok(pack_sequence(pieces))
    }

    fn convert_condition(&self, node: &PgenAstNode) -> Result<ConditionalTest> {
        if let Some(assertion) = self.first_descendant(node, "condition_assertion") {
            let assertion_text = self.slice(assertion)?;
            let pattern = self.first_descendant(assertion, "pattern").ok_or_else(|| {
                self.contract_error("pgen condition assertion is missing its pattern")
            })?;
            let expr = self.convert_pattern(pattern)?;
            return match assertion_text.get(..2) {
                Some("?=") => Ok(ConditionalTest::Lookahead {
                    expr: Box::new(expr),
                    positive: true,
                }),
                Some("?!") => Ok(ConditionalTest::Lookahead {
                    expr: Box::new(expr),
                    positive: false,
                }),
                _ if assertion_text.starts_with("?<=") => Ok(ConditionalTest::Lookbehind {
                    expr: Box::new(expr),
                    positive: true,
                }),
                _ if assertion_text.starts_with("?<!") => Ok(ConditionalTest::Lookbehind {
                    expr: Box::new(expr),
                    positive: false,
                }),
                _ => Err(self.contract_error(&format!(
                    "unsupported pgen condition assertion '{assertion_text}'"
                ))),
            };
        }

        let text = self.slice(node)?.trim();
        if let Some(inner) = text
            .strip_prefix('<')
            .and_then(|value| value.strip_suffix('>'))
        {
            return Ok(ConditionalTest::NamedGroupExists(inner.to_string()));
        }
        if let Some(value) = text.strip_prefix('+') {
            let group = value.parse::<u32>().map_err(|_| {
                self.contract_error(&format!(
                    "invalid positive conditional group reference '{text}'"
                ))
            })?;
            return Ok(ConditionalTest::RelativeGroupExists(group as i32));
        }
        if text.starts_with('-') {
            let group = text.parse::<i32>().map_err(|_| {
                self.contract_error(&format!(
                    "invalid negative conditional group reference '{text}'"
                ))
            })?;
            if group == 0 {
                return Err(self.contract_error(&format!(
                    "invalid negative conditional group reference '{text}'"
                )));
            }
            return Ok(ConditionalTest::RelativeGroupExists(group));
        }
        if !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit()) {
            let group = text.parse::<u32>().map_err(|_| {
                self.contract_error(&format!("invalid numeric conditional reference '{text}'"))
            })?;
            return Ok(ConditionalTest::GroupExists(group));
        }
        if !text.is_empty() {
            return Ok(ConditionalTest::NamedGroupExists(text.to_string()));
        }

        Err(self.contract_error("unsupported empty pgen conditional condition"))
    }

    fn wrap_quantified(&self, expr: Regex, quantifier: Quantifier, possessive: bool) -> Regex {
        let quantified = Regex::Quantified {
            expr: Box::new(expr),
            quantifier,
        };

        if possessive {
            Regex::Group {
                expr: Box::new(quantified),
                kind: GroupKind::Atomic,
                index: None,
                name: None,
            }
        } else {
            quantified
        }
    }

    fn convert_quantifier(&self, node: &PgenAstNode) -> Result<(Quantifier, bool)> {
        let base = self
            .first_descendant(node, "quant_base")
            .ok_or_else(|| self.contract_error("pgen quantifier is missing quant_base"))?;
        let suffix = self
            .first_descendant(node, "quant_suffix")
            .map(|node| self.slice(node))
            .transpose()?
            .unwrap_or("");

        let possessive = suffix == "+";
        let lazy = suffix == "?";
        let base_text = self.slice(base)?;
        match base_text {
            "*" => Ok((Quantifier::ZeroOrMore { lazy }, possessive)),
            "+" => Ok((Quantifier::OneOrMore { lazy }, possessive)),
            "?" => Ok((Quantifier::ZeroOrOne { lazy }, possessive)),
            _ if base_text.starts_with('{') => {
                self.parse_counted_quantifier(base_text, lazy, possessive)
            }
            other => {
                Err(self.contract_error(&format!("unsupported pgen quantifier base '{other}'")))
            }
        }
    }

    fn parse_counted_quantifier(
        &self,
        text: &str,
        lazy: bool,
        possessive: bool,
    ) -> Result<(Quantifier, bool)> {
        let inner = text
            .strip_prefix('{')
            .and_then(|value| value.strip_suffix('}'))
            .ok_or_else(|| self.contract_error("invalid counted quantifier delimiters"))?
            .trim();

        let (min, max) = if let Some((left, right)) = inner.split_once(',') {
            let min = if left.trim().is_empty() {
                0
            } else {
                left.trim().parse::<u32>().map_err(|_| {
                    self.contract_error(&format!("invalid counted quantifier minimum '{left}'"))
                })?
            };
            let max = if right.trim().is_empty() {
                None
            } else {
                Some(right.trim().parse::<u32>().map_err(|_| {
                    self.contract_error(&format!("invalid counted quantifier maximum '{right}'"))
                })?)
            };
            (min, max)
        } else {
            let count = inner.parse::<u32>().map_err(|_| {
                self.contract_error(&format!("invalid counted quantifier value '{inner}'"))
            })?;
            (count, Some(count))
        };

        Ok((Quantifier::Range { min, max, lazy }, possessive))
    }

    fn parse_leaf_fragment(&self, node: &PgenAstNode) -> Result<Regex> {
        let fragment = self.slice(node)?;
        let mut parser = crate::parser::Parser::new(fragment)
            .map_err(|err| RgxError::Compile(err.to_string()))?;
        parser
            .parse()
            .map_err(|err| RgxError::Compile(err.to_string()))
    }

    fn first_descendant<'b>(
        &'b self,
        node: &'b PgenAstNode,
        expected_rule: &str,
    ) -> Option<&'b PgenAstNode> {
        if node.rule_name == expected_rule {
            return Some(node);
        }
        for child in node.children() {
            if let Some(found) = self.first_descendant(child, expected_rule) {
                return Some(found);
            }
        }
        None
    }

    fn alternative_child<'b>(&'b self, node: &'b PgenAstNode) -> Option<&'b PgenAstNode> {
        match &node.content {
            PgenAstContent::Alternative(child) => Some(child),
            _ => None,
        }
    }

    fn sequence_children<'b>(&'b self, node: &'b PgenAstNode) -> Result<&'b [PgenAstNode]> {
        match &node.content {
            PgenAstContent::Sequence(children) => Ok(children),
            other => Err(self.contract_error(&format!(
                "expected sequence content for '{}', got {other:?}",
                node.rule_name
            ))),
        }
    }

    fn quantified_children<'b>(&'b self, node: &'b PgenAstNode) -> Result<&'b [PgenAstNode]> {
        match &node.content {
            PgenAstContent::Quantified((children, _)) => Ok(children),
            other => Err(self.contract_error(&format!(
                "expected quantified content for '{}', got {other:?}",
                node.rule_name
            ))),
        }
    }

    fn is_empty_wrapper(&self, node: &PgenAstNode) -> bool {
        match &node.content {
            PgenAstContent::Sequence(children) => children.is_empty(),
            PgenAstContent::Quantified((children, _)) => children.is_empty(),
            PgenAstContent::Alternative(child) => self.is_empty_wrapper(child),
            PgenAstContent::Terminal(text) | PgenAstContent::TransformedTerminal(text) => {
                text.is_empty()
            }
        }
    }

    fn slice<'b>(&'b self, node: &PgenAstNode) -> Result<&'b str> {
        self.pattern
            .get(node.span.start..node.span.end)
            .ok_or_else(|| {
                self.contract_error(&format!(
                    "pgen node '{}' carried invalid span {}..{} for input length {}",
                    node.rule_name,
                    node.span.start,
                    node.span.end,
                    self.pattern.len()
                ))
            })
    }

    fn contract_error(&self, message: &str) -> RgxError {
        RgxError::Compile(format!("pgen AST contract mismatch: {message}"))
    }
}

#[cfg(feature = "pgen-parser")]
impl PgenAstNode {
    fn children(&self) -> Vec<&PgenAstNode> {
        match &self.content {
            PgenAstContent::Terminal(_) | PgenAstContent::TransformedTerminal(_) => Vec::new(),
            PgenAstContent::Sequence(children) => children.iter().collect(),
            PgenAstContent::Alternative(child) => vec![child.as_ref()],
            PgenAstContent::Quantified((children, _)) => children.iter().collect(),
        }
    }
}

#[cfg(feature = "pgen-parser")]
fn version_at_least(actual: &str, minimum: (u32, u32, u32)) -> bool {
    let mut parts = actual.split('.');
    let parsed = (
        parts.next().and_then(|part| part.parse::<u32>().ok()),
        parts.next().and_then(|part| part.parse::<u32>().ok()),
        parts.next().and_then(|part| part.parse::<u32>().ok()),
    );
    matches!(parsed, (Some(major), Some(minor), Some(patch)) if (major, minor, patch) >= minimum)
}

fn pack_sequence(items: Vec<Regex>) -> Regex {
    match items.len() {
        0 => Regex::Empty,
        1 => items.into_iter().next().unwrap(),
        _ => Regex::Sequence(items),
    }
}

fn pack_alternation(items: Vec<Regex>) -> Regex {
    match items.len() {
        0 => Regex::Empty,
        1 => items.into_iter().next().unwrap(),
        _ => Regex::Alternation(items),
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
        trace_enter!("parsing", "ParserConfig::default");
        let config = Self {
            preferred_parser: None,
            allow_experimental: false,
            auto_select: true,
        };
        trace_exit!(
            "parsing",
            "ParserConfig::default",
            "ok=true,preferred_parser=<none>,allow_experimental={},auto_select={}",
            config.allow_experimental,
            config.auto_select
        );
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{GroupKind, Regex};

    fn parser_contract_reference_fixtures() -> &'static [&'static str] {
        &[
            "",
            "abc",
            "a|b",
            "ab+",
            "a*+",
            "a++",
            "a?+",
            "a{2,3}+",
            r"\d{2,3}",
            r"\d{2,}",
            r"\D+",
            r"\Acat",
            r"dog$",
            r"dog\Z",
            r"dog\z",
            "(abc)",
            "(?:a)(?<word>b)(?>c)",
            "(?=ab)c",
            "(?!ab)c",
            "(?<=z)a",
            "(?<!x)a",
            "(?(1)a|b)",
            "(?(+1)a|b)",
            "(?(-1)a|b)",
            "(?(<word>)a)",
            "(?(<word>)a|b)",
            "(?(word)a|b)",
            "(?(?=ab)x|y)",
            "(?(?!ab)x|y)",
            "(?(?<=z)a|b)",
            "(?(?<!z)a|b)",
            "(?{lua:return true})",
            "(?{js:return true})",
            "(?{javascript:return true})",
            "(?{rhai:true})",
            "(?{native:cb})",
            "(?{wasm:mod:fn})",
            "(?R)",
            "(?1)",
            "(?&word)",
            r"(a)\1",
            r"\p{L}+",
        ]
    }

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
        match PGEN_FEATURE_BACKEND {
            PgenFeatureBackend::Pgen => assert_eq!(name, "pgen"),
            PgenFeatureBackend::RecursiveDescent => assert_eq!(name, "recursive-descent"),
        }
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
        for pattern in parser_contract_reference_fixtures() {
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
        for pattern in parser_contract_reference_fixtures() {
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
    fn parser_contract_parse_success_compile_validation_cases_remain_explicit() {
        let compiler = crate::compiler::Compiler::new();
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
        ];

        for (pattern, expected_msg) in cases {
            parse_pattern(pattern).unwrap_or_else(|e| {
                panic!("parser should accept contract fixture '{pattern}': {e}")
            });
            let err = match compiler.compile(pattern) {
                Ok(_) => panic!(
                    "pattern should fail with an explicit compile-time boundary/validation error: {pattern}"
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
