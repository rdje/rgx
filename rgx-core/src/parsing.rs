//! Zero-cost regex pattern parsing via PGEN
//!
//! This module provides the sole parsing backend for rgx: the PGEN-generated
//! parser.  All pattern text is fed through the PGEN embedding API and the
//! resulting AST dump is converted into the rgx-internal `Regex` AST.

use crate::ast::Regex;
use crate::error::Result;
#[cfg(feature = "pgen-parser")]
use crate::{
    ast::{
        AnchorType, CharClass, CharRange, ConditionalTest, GroupKind, Quantifier, RecursionTarget,
    },
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
/// the rgx engine to use PGEN-generated parsers or any other parsing backend
/// that can produce the standard AST.
pub trait RegexParser {
    /// Parse a regex pattern string into an AST
    ///
    /// This is the main entry point for parsing. Different implementations
    /// may use different internal representations but must all produce
    /// the same AST format for compatibility.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::RgxError`] when the parser cannot translate the
    /// pattern into a valid RGX AST.
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
#[allow(clippy::struct_excessive_bools)] // capability flags are naturally boolean
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

fn standard_parser_capabilities() -> ParserCapabilities {
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

/// Zero-cost PGEN parser
///
/// # Errors
///
/// Returns [`crate::error::RgxError`] when the active parser cannot parse the
/// provided pattern or when the embedded PGEN contract is incompatible.
#[cfg(feature = "pgen-parser")]
pub fn parse_pattern(pattern: &str) -> Result<Regex> {
    trace_enter!(
        "parsing",
        "parsing::parse_pattern[pgen]",
        "pattern_len={}",
        pattern.len()
    );
    low_log!("parsing", "Using PGEN backend");
    let mut parser = PgenParser::new();
    let result = parser.parse_pattern(pattern);
    trace_decision!(
        "parsing",
        "parse result is_ok()",
        result.is_ok(),
        "pgen parser boundary outcome"
    );
    trace_exit!(
        "parsing",
        "parsing::parse_pattern[pgen]",
        "ok={}",
        result.is_ok()
    );
    result
}

/// Stub when PGEN feature is disabled — parsing is unavailable.
///
/// # Errors
///
/// Always returns [`crate::error::RgxError::Compile`] because the pgen-parser
/// feature is required.
#[cfg(not(feature = "pgen-parser"))]
pub fn parse_pattern(_pattern: &str) -> Result<Regex> {
    Err(crate::error::RgxError::Compile(
        "rgx requires the pgen-parser feature for pattern parsing".to_string(),
    ))
}

/// Get the active parser name selected at compile time.
#[must_use]
pub fn parser_name() -> &'static str {
    trace_enter!("parsing", "parsing::parser_name[pgen]");
    let name = if cfg!(feature = "pgen-parser") {
        "pgen"
    } else {
        "unavailable"
    };
    trace_exit!(
        "parsing",
        "parsing::parser_name[pgen]",
        "ok=true,name={}",
        name
    );
    name
}

/// Get the active parser capabilities selected at compile time.
#[must_use]
pub fn parser_capabilities() -> ParserCapabilities {
    trace_enter!("parsing", "parsing::parser_capabilities[pgen]");
    let capabilities = standard_parser_capabilities();
    trace_decision!(
        "parsing",
        "capabilities.perl_advanced",
        capabilities.perl_advanced,
        "pgen advanced perl support flag"
    );
    trace_exit!(
        "parsing",
        "parsing::parser_capabilities[pgen]",
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

/// PGEN parser implementation
#[cfg(feature = "pgen-parser")]
#[derive(Default)]
pub struct PgenParser {
    // PGEN is stateless per call; this type is just an adapter shell.
}

#[cfg(feature = "pgen-parser")]
impl PgenParser {
    /// Create a new PGEN parser adapter.
    #[must_use]
    pub fn new() -> Self {
        trace_enter!("parsing", "PgenParser::new");
        let parser = Self::default();
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
                let err = dump_outcome.diagnostic.map_or_else(
                    || RgxError::Compile("pgen AST dump failed without a diagnostic".to_string()),
                    |diagnostic| RgxError::Compile(diagnostic.to_string()),
                );
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
        Ok(Self::wrap_quantified(expr, quantifier, possessive))
    }

    fn convert_atom(&self, node: &PgenAstNode) -> Result<Regex> {
        let actual = self.alternative_child(node).unwrap_or(node);
        match actual.rule_name.as_str() {
            "group" | "capturing_group" | "noncapturing_group" | "named_group"
            | "python_named_group" | "atomic_group" | "branch_reset_group" => {
                self.convert_group(actual)
            }
            "lookaround" | "lookahead_pos" | "lookahead_neg" | "lookbehind_pos"
            | "lookbehind_neg" => self.convert_lookaround(actual),
            "conditional" => self.convert_conditional(actual),
            "extended_class" => self.convert_extended_char_class(actual),
            "scoped_inline_modifiers" => self.convert_scoped_inline_modifiers(actual),
            "inline_modifiers" => self.convert_inline_modifiers(actual),
            "backreference" => self.convert_named_backreference(actual),
            // Native atom handlers — no builtin parser fallback
            "literal" => self.convert_literal(actual),
            "dot" => Ok(Regex::Dot),
            "anchor" => self.convert_anchor(actual),
            "escape" => self.convert_escape(actual),
            "char_class" => self.convert_char_class(actual),
            "code_block" => self.convert_code_block(actual),
            "subroutine_call" => self.convert_subroutine_call(actual),
            "python_named_backreference" => self.convert_python_named_backreference(actual),
            // Unsupported constructs
            "callout" => Err(RgxError::Compile(
                "unsupported: callout constructs are not supported by rgx".to_string(),
            )),
            "comment_group" => Err(RgxError::Compile(
                "unsupported: comment groups are not supported by rgx".to_string(),
            )),
            "directive_verb" => Err(RgxError::Compile(
                "unsupported: directive/backtracking verbs are not supported by rgx".to_string(),
            )),
            "whitespace_literal" => self.convert_whitespace_literal(actual),
            other => {
                Err(self.contract_error(&format!("unrecognized PGEN atom rule name '{other}'")))
            }
        }
    }

    // ---------------------------------------------------------------
    // Native atom converters
    // ---------------------------------------------------------------

    /// Convert a `literal` node — single literal character like `a`, `b`, `3`.
    fn convert_literal(&self, node: &PgenAstNode) -> Result<Regex> {
        let text = self
            .terminal_text(node)
            .or_else(|_| self.slice(node).map(ToString::to_string))?;
        let mut chars = text.chars();
        let ch = chars
            .next()
            .ok_or_else(|| self.contract_error("pgen literal node has empty content"))?;
        Ok(Regex::Char(ch))
    }

    /// Convert a `whitespace_literal` node — unescaped whitespace from PGEN.
    ///
    /// PGEN emits `whitespace_literal` for bare (unescaped) whitespace
    /// characters.  Inside `(?x:...)` extended-mode groups these represent
    /// insignificant whitespace that should be stripped; outside extended mode
    /// they are ordinary literal characters.
    ///
    /// We produce `Regex::WhitespaceLiteral(c)` so the compiler's
    /// `strip_extended_mode` pass can distinguish unescaped whitespace (which
    /// should be stripped in x-mode) from escaped whitespace (`\ ` etc.) which
    /// goes through the `escape` rule and produces a normal `Regex::Char`.
    fn convert_whitespace_literal(&self, node: &PgenAstNode) -> Result<Regex> {
        let text = self.slice(node)?;
        let ch = text
            .chars()
            .next()
            .ok_or_else(|| self.contract_error("pgen whitespace_literal node has empty content"))?;
        Ok(Regex::WhitespaceLiteral(ch))
    }

    /// Convert an `anchor` node — `^`, `$`, `\A`, `\Z`, `\z`, `\b`, `\B`.
    fn convert_anchor(&self, node: &PgenAstNode) -> Result<Regex> {
        let text = self
            .terminal_text(node)
            .or_else(|_| self.slice(node).map(ToString::to_string))?;
        match text.as_str() {
            "^" => Ok(Regex::Anchor(AnchorType::Start)),
            "$" => Ok(Regex::Anchor(AnchorType::End)),
            "\\A" => Ok(Regex::Anchor(AnchorType::AbsStart)),
            "\\Z" => Ok(Regex::Anchor(AnchorType::AbsEnd)),
            "\\z" => Ok(Regex::Anchor(AnchorType::AbsEndNoNL)),
            "\\b" => Ok(Regex::WordBoundary { positive: true }),
            "\\B" => Ok(Regex::WordBoundary { positive: false }),
            other => Err(self.contract_error(&format!("unrecognized anchor '{other}'"))),
        }
    }

    /// Convert an `escape` node — `\d`, `\D`, `\w`, `\W`, `\s`, `\S`, `\.`, `\n`, `\t`,
    /// `\r`, `\p{L}`, `\P{Greek}`, `\x41`, `\cA`, `\h`, `\H`, `\v`, `\V`, `\1`, etc.
    ///
    /// Dispatches on the structured child variant of `escape_unit` rather than
    /// re-scanning the span text.
    fn convert_escape(&self, node: &PgenAstNode) -> Result<Regex> {
        // Walk the `escape` Sequence[Terminal("\\"), escape_unit-wrapper]. Find
        // the concrete escape variant (simple_escape, hex_escape, property_escape,
        // control_escape, or octal_escape) and dispatch to the matching handler.
        if let Some(simple) = self.first_descendant(node, "simple_escape") {
            return self.convert_simple_escape(simple);
        }
        if let Some(hex) = self.first_descendant(node, "hex_escape") {
            return self.convert_hex_escape(hex);
        }
        if let Some(property) = self.first_descendant(node, "property_escape") {
            return self.convert_property_escape(property);
        }
        if let Some(control) = self.first_descendant(node, "control_escape") {
            return self.convert_control_escape(control);
        }
        if let Some(octal) = self.first_descendant(node, "octal_escape") {
            return self.convert_octal_escape(octal);
        }
        Err(self.contract_error(&format!(
            "pgen escape node '{}' has no recognized escape_unit child",
            node.rule_name
        )))
    }

    /// Convert a `simple_escape` node — the single character after `\` resolves
    /// to a shorthand class, anchor, literal control char, or metachar. This is
    /// the only escape handler that legitimately inspects the terminal character
    /// value because PGEN flattens all shorthand escapes through `any_char`.
    fn convert_simple_escape(&self, node: &PgenAstNode) -> Result<Regex> {
        let ch = self.collect_first_terminal_char(node).ok_or_else(|| {
            self.contract_error("pgen simple_escape is missing its trailing character")
        })?;
        match ch {
            // Predefined character classes (wrapped in CharClass to match VM expectations)
            'd' => Ok(Regex::CharClass(CharClass::Digit { negated: false })),
            'D' => Ok(Regex::CharClass(CharClass::Digit { negated: true })),
            'w' => Ok(Regex::CharClass(CharClass::Word { negated: false })),
            'W' => Ok(Regex::CharClass(CharClass::Word { negated: true })),
            's' => Ok(Regex::CharClass(CharClass::Space { negated: false })),
            'S' => Ok(Regex::CharClass(CharClass::Space { negated: true })),

            // Horizontal whitespace
            'h' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: horizontal_whitespace_ranges(),
                negated: false,
            })),
            'H' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: horizontal_whitespace_ranges(),
                negated: true,
            })),

            // Vertical whitespace
            'v' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: vertical_whitespace_ranges(),
                negated: false,
            })),
            'V' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: vertical_whitespace_ranges(),
                negated: true,
            })),

            // Word boundaries (if PGEN routes them through simple_escape).
            'b' => Ok(Regex::WordBoundary { positive: true }),
            'B' => Ok(Regex::WordBoundary { positive: false }),

            // Anchors (if PGEN routes them through simple_escape).
            'A' => Ok(Regex::Anchor(AnchorType::AbsStart)),
            'Z' => Ok(Regex::Anchor(AnchorType::AbsEnd)),
            'z' => Ok(Regex::Anchor(AnchorType::AbsEndNoNL)),

            // PCRE2 match reset (\K)
            'K' => Ok(Regex::MatchReset),

            // PCRE2 newline sequence (\R)
            'R' => Ok(Regex::NewlineSequence),

            // Literal control-character escapes: \n, \t, \r, \f, \a, \e
            'n' => Ok(Regex::Char('\n')),
            't' => Ok(Regex::Char('\t')),
            'r' => Ok(Regex::Char('\r')),
            'f' => Ok(Regex::Char('\u{0C}')),
            'a' => Ok(Regex::Char('\u{07}')),
            'e' => Ok(Regex::Char('\u{1B}')),

            // Numeric backreferences \1, \2, etc. are captured as a single
            // digit under simple_escape by PGEN.
            c if c.is_ascii_digit() => {
                let n = c.to_digit(10).unwrap_or(0);
                Ok(Regex::Backreference(n))
            }

            // Escaped metacharacters: \., \*, \+, \?, \(, \), \[, \], \{, \}, \|, \\, \^, \$, \-, \/
            // Also covers escaped space (`\ `) used in (?x) extended mode.
            c if ".*+?()[]{}|\\^$-/ ".contains(c) => Ok(Regex::Char(c)),

            other => {
                Err(self.contract_error(&format!("unrecognized simple_escape character '{other}'")))
            }
        }
    }

    /// Convert a `hex_escape` node — `\xNN` (Sequence of `x` + two
    /// `hex_digit`s) or `\x{NNNN}` (Sequence of `x{` + `hex_digits` list +
    /// `}`).
    fn convert_hex_escape(&self, node: &PgenAstNode) -> Result<Regex> {
        let mut hex_str = String::new();
        self.walk_collect_terminal_chars(node, "hex_digit", &mut hex_str);
        if hex_str.is_empty() {
            return Err(self.contract_error("pgen hex_escape has no hex_digit children"));
        }
        let code = u32::from_str_radix(&hex_str, 16)
            .map_err(|_| self.contract_error(&format!("invalid hex_escape digits '{hex_str}'")))?;
        let ch = char::from_u32(code).ok_or_else(|| {
            self.contract_error(&format!(
                "hex_escape value U+{code:X} is not a valid Unicode code point"
            ))
        })?;
        Ok(Regex::Char(ch))
    }

    /// Convert a `property_escape` node — `\p{Name}` / `\P{Name}`. Polarity is
    /// derived from the opening terminal (`p{` vs `P{`) and the property name
    /// is rebuilt by walking the `prop_name` subtree terminals.
    fn convert_property_escape(&self, node: &PgenAstNode) -> Result<Regex> {
        // Locate the leading brace terminal ("p{" or "P{") under the
        // property_escape Sequence to determine polarity.
        let children = self.sequence_children(node)?;
        let opener = children
            .first()
            .and_then(|child| self.find_first_terminal_text(child))
            .ok_or_else(|| {
                self.contract_error("pgen property_escape is missing its opening terminal")
            })?;
        let negated = opener.starts_with('P');

        // Walk the prop_name subtree and collect every terminal character.
        let name_node = self.first_descendant(node, "prop_name").ok_or_else(|| {
            self.contract_error("pgen property_escape is missing its prop_name child")
        })?;
        let mut name = String::new();
        self.collect_all_terminal_chars(name_node, &mut name);
        if name.is_empty() {
            return Err(self.contract_error("pgen property_escape has empty prop_name"));
        }
        Ok(Regex::UnicodeClass { name, negated })
    }

    /// Convert a `control_escape` node — `\cA` → control character. The letter
    /// following `c` is taken from the `any_char` subtree.
    fn convert_control_escape(&self, node: &PgenAstNode) -> Result<Regex> {
        let any_char = self.first_descendant(node, "any_char").ok_or_else(|| {
            self.contract_error("pgen control_escape is missing its any_char child")
        })?;
        let ctrl_char = self.collect_first_terminal_char(any_char).ok_or_else(|| {
            self.contract_error("pgen control_escape any_char has no terminal character")
        })?;
        let code = (ctrl_char.to_ascii_uppercase() as u32).wrapping_sub('@' as u32) & 0x1F;
        let ch = char::from_u32(code)
            .ok_or_else(|| self.contract_error("pgen control_escape produced invalid char"))?;
        Ok(Regex::Char(ch))
    }

    /// Convert an `octal_escape` node — 1..3 `octal_digit` terminals.
    fn convert_octal_escape(&self, node: &PgenAstNode) -> Result<Regex> {
        let mut oct_str = String::new();
        self.walk_collect_terminal_chars(node, "octal_digit", &mut oct_str);
        if oct_str.is_empty() {
            return Err(self.contract_error("pgen octal_escape has no octal_digit children"));
        }
        let code = u32::from_str_radix(&oct_str, 8).map_err(|_| {
            self.contract_error(&format!("invalid octal_escape digits '{oct_str}'"))
        })?;
        let ch = char::from_u32(code).ok_or_else(|| {
            self.contract_error(&format!(
                "octal_escape value {code} is not a valid Unicode code point"
            ))
        })?;
        Ok(Regex::Char(ch))
    }

    /// Convert a `char_class` node — `[a-z]`, `[^0-9]`, `[\d\w]`, etc.
    ///
    /// Walks PGEN's structured children (negation slot, optional
    /// `class_initial_close` for leading `]`, then each `class_item` in
    /// `class_body`) rather than relexing the span text.
    fn convert_char_class(&self, node: &PgenAstNode) -> Result<Regex> {
        let negated = self
            .first_descendant(node, "negation")
            .is_some_and(|n| !self.is_empty_wrapper(n));

        let mut ranges = Vec::new();

        // `class_initial_close` captures a `]` literal right after `[` or
        // `[^`, keeping it as a class member instead of the closing bracket.
        if let Some(initial_close) = self.first_descendant(node, "class_initial_close") {
            if !self.is_empty_wrapper(initial_close) {
                ranges.push(CharRange::single(']'));
            }
        }

        if let Some(body) = self.first_descendant(node, "class_body") {
            // `class_body` is a Quantified* list of wrappers each holding a
            // concrete `class_item`.
            for wrapper in self.quantified_children(body)? {
                let item = self
                    .first_descendant(wrapper, "class_item")
                    .ok_or_else(|| {
                        self.contract_error("pgen class_body entry is missing class_item")
                    })?;
                self.convert_class_item(item, &mut ranges)?;
            }
        }

        Ok(Regex::CharClass(CharClass::Custom { ranges, negated }))
    }

    /// Convert a single `class_item` — either a `class_range`, a bare
    /// `class_literal`, or a `class_escape` — into one or more `CharRange`s.
    fn convert_class_item(&self, item: &PgenAstNode, ranges: &mut Vec<CharRange>) -> Result<()> {
        if let Some(range_node) = self.find_direct_child(item, "class_range") {
            return self.convert_class_range(range_node, ranges);
        }
        if let Some(escape_node) = self.find_direct_child(item, "class_escape") {
            return self.convert_class_escape(escape_node, ranges);
        }
        if let Some(literal_node) = self.find_direct_child(item, "class_literal") {
            let ch = self
                .collect_first_terminal_char(literal_node)
                .ok_or_else(|| {
                    self.contract_error("pgen class_literal has no terminal character")
                })?;
            ranges.push(CharRange::single(ch));
            return Ok(());
        }
        Err(self.contract_error(&format!(
            "pgen class_item has no known variant under '{}'",
            item.rule_name
        )))
    }

    /// Convert a `class_range` node — `class_atom "-" class_atom` — into a
    /// single `CharRange`. Escape endpoints must resolve to a single `CharRange`.
    fn convert_class_range(&self, range: &PgenAstNode, ranges: &mut Vec<CharRange>) -> Result<()> {
        let children = self.sequence_children(range)?;
        let start_atom = children
            .first()
            .ok_or_else(|| self.contract_error("pgen class_range is missing its start atom"))?;
        let end_atom = children
            .get(2)
            .ok_or_else(|| self.contract_error("pgen class_range is missing its end atom"))?;
        let start = self.class_atom_char(start_atom)?;
        let end = self.class_atom_char(end_atom)?;
        ranges.push(CharRange::range(start, end));
        Ok(())
    }

    /// Resolve a `class_atom` wrapper (or a raw `class_literal`/`class_escape`)
    /// to the single character it represents, for use as a range endpoint.
    fn class_atom_char(&self, node: &PgenAstNode) -> Result<char> {
        let atom = self.first_descendant(node, "class_atom").unwrap_or(node);
        if let Some(escape_node) = self.find_direct_child(atom, "class_escape") {
            // For range endpoints an escape must resolve to a single character
            // (hex/octal/control/metachar literal). Shorthand classes are not
            // allowed as endpoints by the grammar.
            let mut tmp = Vec::new();
            self.convert_class_escape(escape_node, &mut tmp)?;
            if tmp.len() == 1 && tmp[0].start == tmp[0].end {
                return Ok(tmp[0].start);
            }
            return Err(
                self.contract_error("pgen class_range endpoint must resolve to a single character")
            );
        }
        if let Some(literal_node) = self.find_direct_child(atom, "class_literal") {
            return self
                .collect_first_terminal_char(literal_node)
                .ok_or_else(|| {
                    self.contract_error("pgen class_literal has no terminal character")
                });
        }
        self.collect_first_terminal_char(atom)
            .ok_or_else(|| self.contract_error("pgen class_atom has no terminal character"))
    }

    /// Convert a `class_escape` node (an `escape` subtree used as a class
    /// member) into `CharRange`s appended to `ranges`.
    fn convert_class_escape(
        &self,
        class_escape: &PgenAstNode,
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        let escape_node = self
            .first_descendant(class_escape, "escape")
            .ok_or_else(|| self.contract_error("pgen class_escape is missing its escape child"))?;
        let regex = self.convert_escape(escape_node)?;
        extend_ranges_from_regex(regex, ranges, |msg| self.contract_error(msg))?;
        Ok(())
    }

    /// Convert a `code_block` node — `(?{lua:...})`, `(?{native:cb})`.
    ///
    /// NOTE: PGEN's PEG ordering always selects `code_block_plain` for the
    /// payload, so the language prefix (`lua:`, `native:`, etc.) is NOT split
    /// out as a structured child node — it's fused into the opaque code text.
    /// We therefore keep span-text parsing here intentionally.
    fn convert_code_block(&self, node: &PgenAstNode) -> Result<Regex> {
        // PGEN 1.1.6+ produces code_block_lang with correct code_lang and
        // code_content spans for language-tagged code blocks.
        if let Some(lang_node) = self.first_descendant(node, "code_block_lang") {
            let lang = self
                .first_descendant(lang_node, "code_lang")
                .and_then(|n| self.slice(n).ok())
                .unwrap_or_default()
                .to_string();
            let code = self
                .first_descendant(lang_node, "code_content")
                .and_then(|n| self.slice(n).ok())
                .unwrap_or_default()
                .to_string();
            return Ok(Regex::CodeBlock { lang, code });
        }
        // Fallback for code_block_plain (untagged) or older PGEN versions
        let fragment = self.slice(node)?;
        let inner = fragment
            .strip_prefix("(?{")
            .and_then(|s| s.strip_suffix("})"))
            .ok_or_else(|| {
                self.contract_error(&format!(
                    "pgen code_block did not retain '(?{{...}})' delimiters in '{fragment}'"
                ))
            })?;
        if let Some(colon_pos) = inner.find(':') {
            let lang = inner[..colon_pos].to_string();
            let code = inner[colon_pos + 1..].to_string();
            Ok(Regex::CodeBlock { lang, code })
        } else {
            Ok(Regex::CodeBlock {
                lang: String::new(),
                code: inner.to_string(),
            })
        }
    }

    /// Convert a `subroutine_call` node — `(?R)`, `(?1)`, `(?&name)`,
    /// `(?P>name)`.
    ///
    /// PGEN grammar: `subroutine_call = "(?" subroutine_target ")"`, where
    /// `subroutine_target` has variants:
    ///   - `"&" name`           → named recursion
    ///   - `"P>" name`          → Python-style named recursion
    ///   - `"R" digits?`        → entire-pattern recursion (digits ignored)
    ///   - `signed_digits`      → group-index recursion
    ///
    /// We inspect the structured `subroutine_target` child to build the
    /// `Recursion` AST node.
    fn convert_subroutine_call(&self, node: &PgenAstNode) -> Result<Regex> {
        let target_node = self
            .first_descendant(node, "subroutine_target")
            .ok_or_else(|| {
                self.contract_error("pgen subroutine_call is missing subroutine_target")
            })?;

        // Unwrap the immediate Alternative wrapper if present.
        let inner = self.alternative_child(target_node).unwrap_or(target_node);

        // Variant 1: signed_digits (e.g. `(?1)`, `(?-1)`).
        if let Some(signed) = self.first_descendant(inner, "signed_digits") {
            let text = self.slice(signed)?;
            let n: u32 = text
                .trim_start_matches('+')
                .trim_start_matches('-')
                .parse()
                .map_err(|_| {
                    self.contract_error(&format!("invalid subroutine call number '{text}'"))
                })?;
            return Ok(Regex::Recursion {
                target: RecursionTarget::Group(n),
            });
        }

        // Variants 2–4 all shape as Sequence[Terminal prefix, payload].
        // Inspect the first terminal to dispatch.
        let prefix_text = self.find_first_terminal_text(inner).unwrap_or("");
        let target = match prefix_text {
            "&" | "P>" => {
                let name = self.name_text(inner)?;
                RecursionTarget::NamedGroup(name)
            }
            "R" => RecursionTarget::Entire,
            other => {
                return Err(self.contract_error(&format!(
                    "unrecognized pgen subroutine_target prefix '{other}'"
                )));
            }
        };
        Ok(Regex::Recursion { target })
    }

    /// Convert a `python_named_backreference` node — `(?P=name)`.
    ///
    /// PGEN grammar: `python_named_backreference = "(?P=" name ")"`. We read
    /// the `name` child's span text directly.
    fn convert_python_named_backreference(&self, node: &PgenAstNode) -> Result<Regex> {
        let name = self.name_text(node)?;
        Ok(Regex::NamedBackreference(name))
    }

    // ---------------------------------------------------------------
    // Existing structured-node converters
    // ---------------------------------------------------------------

    fn convert_scoped_inline_modifiers(&self, node: &PgenAstNode) -> Result<Regex> {
        // PGEN grammar: scoped_inline_modifiers = "(?" modifier_spec ":" pattern? ")"
        // Walk the structured `modifier_spec` subtree for flag characters and
        // convert the nested `pattern` child recursively via PGEN.
        let flags = if let Some(spec) = self.first_descendant(node, "modifier_spec") {
            self.collect_modifier_flags(spec)
        } else {
            String::new()
        };

        let body = if let Some(pattern_child) = self.first_descendant(node, "pattern") {
            self.convert_pattern(pattern_child)?
        } else {
            Regex::Empty
        };

        Ok(Regex::FlagGroup {
            flags,
            expr: Box::new(body),
        })
    }

    #[allow(clippy::unnecessary_wraps)] // keeps dispatch signature uniform with sibling converters
    fn convert_inline_modifiers(&self, node: &PgenAstNode) -> Result<Regex> {
        // PGEN grammar: inline_modifiers = "(?" modifier_spec? ")"
        // Walk the structured `modifier_spec` subtree for flag characters.
        let flags = if let Some(spec) = self.first_descendant(node, "modifier_spec") {
            self.collect_modifier_flags(spec)
        } else {
            String::new()
        };

        Ok(Regex::FlagGroup {
            flags,
            expr: Box::new(Regex::Empty),
        })
    }

    fn convert_named_backreference(&self, node: &PgenAstNode) -> Result<Regex> {
        // PGEN grammar:
        //   backreference = "\" backreference_digits
        //                 | "\k" name_ref
        //                 | "\k{" name "}"
        //                 | "\g" subroutine_ref
        //
        // Walk the structured children to build the appropriate AST node
        // instead of re-splitting the span text.
        let children = self.sequence_children(node)?;
        let prefix = children
            .first()
            .ok_or_else(|| self.contract_error("pgen backreference has no prefix terminal"))?;
        let prefix_text = self.find_first_terminal_text(prefix).unwrap_or("");

        match prefix_text {
            "\\" => {
                // Numeric backreference: \1, \12, ... via `backreference_digits`.
                let digits_node = self
                    .first_descendant(node, "backreference_digits")
                    .ok_or_else(|| {
                        self.contract_error(
                            "pgen numeric backreference is missing backreference_digits",
                        )
                    })?;
                let mut digits = String::new();
                self.walk_collect_terminal_chars(digits_node, "nonzero_digit", &mut digits);
                self.walk_collect_terminal_chars(digits_node, "digit", &mut digits);
                if digits.is_empty() {
                    return Err(self
                        .contract_error("pgen backreference_digits produced no digit characters"));
                }
                let n: u32 = digits.parse().map_err(|_| {
                    self.contract_error(&format!("invalid numeric backreference '{digits}'"))
                })?;
                Ok(Regex::Backreference(n))
            }
            "\\k" => {
                // \k<name>, \k'name': element_1 is a `name_ref` wrapper.
                let name = self.name_text(node)?;
                Ok(Regex::NamedBackreference(name))
            }
            "\\k{" => {
                // \k{name}: element_1 is directly a `name` node.
                let name = self.name_text(node)?;
                Ok(Regex::NamedBackreference(name))
            }
            "\\g" => {
                // \g{name}, \g{N}, \g<N>, \g<name>, \g'name':
                // element_1 is a `subroutine_ref` with a `signed_digits_or_name`
                // payload (either `name` or `signed_digits`).
                // PGEN 1.1.4+ correctly parses all \g forms including \g<1>.
                if let Some(name_node) = self.first_descendant(node, "name") {
                    return Ok(Regex::NamedBackreference(
                        self.slice(name_node)?.to_string(),
                    ));
                }
                if let Some(digits_node) = self.first_descendant(node, "digits") {
                    let mut digits = String::new();
                    self.walk_collect_terminal_chars(digits_node, "digit", &mut digits);
                    if !digits.is_empty() {
                        let n: u32 = digits.parse().map_err(|_| {
                            self.contract_error(&format!(
                                "invalid numeric backreference '{digits}'"
                            ))
                        })?;
                        return Ok(Regex::Backreference(n));
                    }
                }
                let fragment = self.slice(node)?;
                Err(self
                    .contract_error(&format!("unrecognized '\\g' backreference in '{fragment}'")))
            }
            other => {
                Err(self
                    .contract_error(&format!("unrecognized pgen backreference prefix '{other}'")))
            }
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
            "branch_reset_group" => (GroupKind::BranchReset, None),
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

    fn convert_extended_char_class(&self, node: &PgenAstNode) -> Result<Regex> {
        let content = self
            .first_descendant(node, "extended_class_content")
            .and_then(|n| self.slice(n).ok())
            .ok_or_else(|| {
                self.contract_error(
                    "pgen extended_class is missing its extended_class_content child",
                )
            })?;
        Ok(Regex::ExtendedCharClass {
            content: content.to_string(),
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
        // Lookaround assertion (already structurally handled)
        if let Some(assertion) = self.first_descendant(node, "condition_assertion") {
            return self.convert_condition_assertion(assertion);
        }
        // Recursion condition: R, R&name, R<N> — structured child from PGEN
        if let Some(rec) = self.first_descendant(node, "recursion_condition") {
            return self.convert_condition_recursion(rec);
        }
        // Name reference: <name> or 'name' — structured child from PGEN
        if let Some(name_ref) = self.first_descendant(node, "name_ref") {
            let name = self.name_text(name_ref)?;
            return Ok(ConditionalTest::NamedGroupExists(name));
        }
        // Signed digits: group number, +N, -N — structured child from PGEN
        if let Some(sd) = self.first_descendant(node, "signed_digits") {
            return self.convert_condition_signed_digits(sd);
        }
        // DEFINE keyword, bare name (R1 ambiguity PGEN-RGX-0010), or bare number
        self.convert_condition_text_fallback(node)
    }

    fn convert_condition_assertion(&self, assertion: &PgenAstNode) -> Result<ConditionalTest> {
        let assertion_text = self.slice(assertion)?;
        let pattern = self.first_descendant(assertion, "pattern").ok_or_else(|| {
            self.contract_error("pgen condition assertion is missing its pattern")
        })?;
        let expr = self.convert_pattern(pattern)?;
        match assertion_text.get(..2) {
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
        }
    }

    fn convert_condition_recursion(&self, rec: &PgenAstNode) -> Result<ConditionalTest> {
        if let Some(name_node) = self.first_descendant(rec, "name") {
            return Ok(ConditionalTest::RecursionNamed(
                self.slice(name_node)?.to_string(),
            ));
        }
        // R with digits = RecursionGroup, R alone = RecursionAny
        let mut digits = String::new();
        self.walk_collect_terminal_chars(rec, "digit", &mut digits);
        if !digits.is_empty() {
            let group: u32 = digits.parse().map_err(|_| {
                self.contract_error(&format!(
                    "invalid recursion conditional group digits '{digits}'"
                ))
            })?;
            if group == 0 {
                return Err(
                    self.contract_error("invalid recursion conditional group reference 'R0'")
                );
            }
            return Ok(ConditionalTest::RecursionGroup(group));
        }
        Ok(ConditionalTest::RecursionAny)
    }

    fn convert_condition_signed_digits(&self, sd: &PgenAstNode) -> Result<ConditionalTest> {
        let sign_text = self
            .first_descendant(sd, "sign")
            .and_then(|n| self.slice(n).ok())
            .unwrap_or("");
        let mut digits = String::new();
        self.walk_collect_terminal_chars(sd, "digit", &mut digits);
        let n: u32 = digits.parse().map_err(|_| {
            self.contract_error(&format!(
                "invalid conditional group reference digits '{digits}'"
            ))
        })?;
        match sign_text {
            "+" =>
            {
                #[allow(clippy::cast_possible_wrap)]
                Ok(ConditionalTest::RelativeGroupExists(n as i32))
            }
            "-" =>
            {
                #[allow(clippy::cast_possible_wrap)]
                Ok(ConditionalTest::RelativeGroupExists(-(n as i32)))
            }
            _ => Ok(ConditionalTest::GroupExists(n)),
        }
    }

    /// Handle DEFINE, bare name, and bare numeric fallback for condition
    /// nodes that lack structured children.
    fn convert_condition_text_fallback(&self, node: &PgenAstNode) -> Result<ConditionalTest> {
        let text = self.slice(node)?.trim();
        if text == "DEFINE" {
            return Ok(ConditionalTest::Define);
        }
        // Bare name reference (PGEN 1.1.7+ routes R/R1/R&name through
        // recursion_condition, so only genuine named groups reach here).
        if let Some(name_node) = self.first_descendant(node, "name") {
            let name = self.slice(name_node)?.to_string();
            return Ok(ConditionalTest::NamedGroupExists(name));
        }
        // Fallback: try as bare number
        if !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit()) {
            let group: u32 = text.parse().map_err(|_| {
                self.contract_error(&format!("invalid numeric conditional reference '{text}'"))
            })?;
            return Ok(ConditionalTest::GroupExists(group));
        }
        if !text.is_empty() {
            return Ok(ConditionalTest::NamedGroupExists(text.to_string()));
        }
        Err(self.contract_error("unsupported empty pgen conditional condition"))
    }

    fn wrap_quantified(expr: Regex, quantifier: Quantifier, possessive: bool) -> Regex {
        let quantified_expr = Regex::Quantified {
            expr: Box::new(expr),
            quantifier,
        };

        if possessive {
            Regex::Group {
                expr: Box::new(quantified_expr),
                kind: GroupKind::Atomic,
                index: None,
                name: None,
            }
        } else {
            quantified_expr
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
                self.parse_counted_quantifier(base, lazy, possessive)
            }
            other => {
                Err(self.contract_error(&format!("unsupported pgen quantifier base '{other}'")))
            }
        }
    }

    fn parse_counted_quantifier(
        &self,
        base_node: &PgenAstNode,
        lazy: bool,
        possessive: bool,
    ) -> Result<(Quantifier, bool)> {
        // Walk the `counted_quantifier_body` child structurally. It contains
        // `digits` groups (min, optional max) and an optional comma terminal
        // arranged as Sequence[element_0(digits), element_1, element_2(comma-group?)].
        if let Some(body) = self.first_descendant(base_node, "counted_quantifier_body") {
            // Collect all `digits` descendants in depth-first order.
            let mut digit_groups = Vec::new();
            self.collect_digit_groups(body, &mut digit_groups);

            // Check for a comma terminal anywhere under the body to
            // distinguish {N} from {N,} / {N,M}.
            let has_comma = self.has_terminal_text(body, ",");

            let (min, max) = if has_comma {
                let min = if digit_groups.is_empty() || digit_groups[0].is_empty() {
                    0
                } else {
                    digit_groups[0].parse::<u32>().map_err(|_| {
                        self.contract_error(&format!(
                            "invalid counted quantifier minimum '{}'",
                            digit_groups[0]
                        ))
                    })?
                };
                let max = digit_groups
                    .get(1)
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        s.parse::<u32>().map_err(|_| {
                            self.contract_error(&format!(
                                "invalid counted quantifier maximum '{s}'"
                            ))
                        })
                    })
                    .transpose()?;
                (min, max)
            } else {
                let count_str = digit_groups.first().map_or("", String::as_str);
                let count = count_str.parse::<u32>().map_err(|_| {
                    self.contract_error(&format!("invalid counted quantifier value '{count_str}'"))
                })?;
                (count, Some(count))
            };

            return Ok((Quantifier::Range { min, max, lazy }, possessive));
        }

        // Fallback: no counted_quantifier_body child (older PGEN) — parse
        // from span text. This is the only remaining text-parse path for
        // the counted quantifier.
        let text = self.slice(base_node)?;
        let inner = text
            .get(1..text.len().saturating_sub(1))
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

    // ---------------------------------------------------------------
    // Tree-navigation helpers
    // ---------------------------------------------------------------

    /// Extract the text from a `Terminal` or `TransformedTerminal` content node.
    fn terminal_text(&self, node: &PgenAstNode) -> std::result::Result<String, RgxError> {
        match &node.content {
            PgenAstContent::Terminal(text) | PgenAstContent::TransformedTerminal(text) => {
                Ok(text.clone())
            }
            _ => Err(self.contract_error(&format!(
                "expected terminal content for '{}', got non-terminal",
                node.rule_name
            ))),
        }
    }

    /// Return the first immediate (or wrapped-alternative) descendant of
    /// `node` whose rule matches `name`. Unlike `first_descendant`, this does
    /// not return `node` itself — it walks children.
    #[allow(clippy::only_used_in_recursion)]
    fn find_direct_child<'b>(
        &'b self,
        node: &'b PgenAstNode,
        name: &str,
    ) -> Option<&'b PgenAstNode> {
        for child in node.children() {
            if child.rule_name == name {
                return Some(child);
            }
            if let Some(found) = self.find_direct_child(child, name) {
                return Some(found);
            }
        }
        None
    }

    /// Return the first `Terminal`/`TransformedTerminal` text reached while
    /// walking the subtree rooted at `node`.
    #[allow(clippy::only_used_in_recursion)]
    fn find_first_terminal_text<'b>(&'b self, node: &'b PgenAstNode) -> Option<&'b str> {
        match &node.content {
            PgenAstContent::Terminal(text) | PgenAstContent::TransformedTerminal(text) => {
                Some(text.as_str())
            }
            PgenAstContent::Alternative(child) => self.find_first_terminal_text(child),
            PgenAstContent::Sequence(children) | PgenAstContent::Quantified((children, _)) => {
                children
                    .iter()
                    .find_map(|child| self.find_first_terminal_text(child))
            }
        }
    }

    /// Return the first character of the first terminal reached under `node`.
    fn collect_first_terminal_char(&self, node: &PgenAstNode) -> Option<char> {
        self.find_first_terminal_text(node)
            .and_then(|text| text.chars().next())
    }

    /// Walk the subtree rooted at `node`, and for every descendant whose
    /// rule name equals `rule`, append that node's first terminal character
    /// to `out`.
    fn walk_collect_terminal_chars(&self, node: &PgenAstNode, rule: &str, out: &mut String) {
        if node.rule_name == rule {
            if let Some(ch) = self.collect_first_terminal_char(node) {
                out.push(ch);
                return;
            }
        }
        for child in node.children() {
            self.walk_collect_terminal_chars(child, rule, out);
        }
    }

    /// Walk the subtree rooted at `node` and append every terminal string
    /// reached (in depth-first order) to `out`. Used for assembling names or
    /// multi-character terminal concatenations like property names.
    #[allow(clippy::only_used_in_recursion)]
    fn collect_all_terminal_chars(&self, node: &PgenAstNode, out: &mut String) {
        match &node.content {
            PgenAstContent::Terminal(text) | PgenAstContent::TransformedTerminal(text) => {
                out.push_str(text);
            }
            PgenAstContent::Alternative(child) => self.collect_all_terminal_chars(child, out),
            PgenAstContent::Sequence(children) | PgenAstContent::Quantified((children, _)) => {
                for child in children {
                    self.collect_all_terminal_chars(child, out);
                }
            }
        }
    }

    /// Walk the subtree rooted at `node` and collect all `digits` descendants
    /// in depth-first order. Each `digits` node's digit terminals are assembled
    /// into a single string and appended to `out`. Used for counted quantifier
    /// parsing where the grammar produces `digits` children for min and max.
    fn collect_digit_groups(&self, node: &PgenAstNode, out: &mut Vec<String>) {
        if node.rule_name == "digits" {
            let mut digits = String::new();
            self.walk_collect_terminal_chars(node, "digit", &mut digits);
            out.push(digits);
            return;
        }
        for child in node.children() {
            self.collect_digit_groups(child, out);
        }
    }

    /// Return `true` if any terminal node in the subtree has text equal to
    /// `target`. Used to detect comma presence in counted quantifier bodies
    /// without relying on its position in the tree.
    #[allow(clippy::only_used_in_recursion)]
    fn has_terminal_text(&self, node: &PgenAstNode, target: &str) -> bool {
        match &node.content {
            PgenAstContent::Terminal(text) | PgenAstContent::TransformedTerminal(text) => {
                text == target
            }
            PgenAstContent::Alternative(child) => self.has_terminal_text(child, target),
            PgenAstContent::Sequence(children) | PgenAstContent::Quantified((children, _)) => {
                children
                    .iter()
                    .any(|child| self.has_terminal_text(child, target))
            }
        }
    }

    #[allow(clippy::only_used_in_recursion)]
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
        let _ = self;
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

    #[allow(clippy::only_used_in_recursion)]
    fn is_empty_wrapper(&self, node: &PgenAstNode) -> bool {
        match &node.content {
            PgenAstContent::Sequence(children) | PgenAstContent::Quantified((children, _)) => {
                children.is_empty()
            }
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

    /// Find the first `name` descendant of `node` and return its span text.
    /// Used for extracting identifiers from `name_ref`, `python_named_backreference`,
    /// `subroutine_target`, and similar structured nodes.
    fn name_text(&self, node: &PgenAstNode) -> Result<String> {
        let name_node = self
            .first_descendant(node, "name")
            .ok_or_else(|| self.contract_error("pgen node is missing its 'name' child"))?;
        Ok(self.slice(name_node)?.to_string())
    }

    /// Walk a `modifier_spec` subtree (or `modifier_seq`/`modifier_group`) and
    /// collect the flag characters in order. A leading or mid-sequence `-`
    /// terminal is emitted as a `-` char to mark the transition to disable
    /// flags, mirroring what the span-text form produced.
    fn collect_modifier_flags(&self, node: &PgenAstNode) -> String {
        let mut out = String::new();
        self.walk_modifier_flags(node, &mut out);
        out
    }

    #[allow(clippy::only_used_in_recursion)]
    fn walk_modifier_flags(&self, node: &PgenAstNode, out: &mut String) {
        // `modifier_char` is a terminal leaf; capture its char directly.
        if node.rule_name == "modifier_char" {
            if let Some(ch) = self.collect_first_terminal_char(node) {
                out.push(ch);
            }
            return;
        }
        // A raw `-` terminal at a `modifier_seq` boundary marks disable.
        if let PgenAstContent::Terminal(text) | PgenAstContent::TransformedTerminal(text) =
            &node.content
        {
            if text == "-" {
                out.push('-');
            }
            return;
        }
        for child in node.children() {
            self.walk_modifier_flags(child, out);
        }
    }

    fn contract_error(&self, message: &str) -> RgxError {
        let _ = self;
        RgxError::Compile(format!("pgen AST contract mismatch: {message}"))
    }
}

#[cfg(feature = "pgen-parser")]
impl PgenAstNode {
    fn children(&self) -> Vec<&PgenAstNode> {
        match &self.content {
            PgenAstContent::Terminal(_) | PgenAstContent::TransformedTerminal(_) => Vec::new(),
            PgenAstContent::Sequence(children) | PgenAstContent::Quantified((children, _)) => {
                children.iter().collect()
            }
            PgenAstContent::Alternative(child) => vec![child.as_ref()],
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

/// Merge ranges derived from converting an escape into a sequence of
/// `CharRange`s suitable for a `Custom` char class body. Only outputs that
/// reduce cleanly to a list of ranges are accepted.
#[cfg(feature = "pgen-parser")]
fn extend_ranges_from_regex<F>(
    regex: Regex,
    ranges: &mut Vec<CharRange>,
    make_error: F,
) -> Result<()>
where
    F: Fn(&str) -> RgxError,
{
    match regex {
        Regex::Char(ch) => {
            ranges.push(CharRange::single(ch));
            Ok(())
        }
        Regex::CharClass(CharClass::Custom { ranges: custom, .. }) => {
            ranges.extend(custom);
            Ok(())
        }
        Regex::CharClass(CharClass::Digit { negated: false }) => {
            ranges.push(CharRange::range('0', '9'));
            Ok(())
        }
        Regex::CharClass(CharClass::Digit { negated: true }) => {
            ranges.push(CharRange::range('\0', '/'));
            ranges.push(CharRange::range(':', char::MAX));
            Ok(())
        }
        Regex::CharClass(CharClass::Word { negated: false }) => {
            ranges.push(CharRange::range('0', '9'));
            ranges.push(CharRange::range('A', 'Z'));
            ranges.push(CharRange::single('_'));
            ranges.push(CharRange::range('a', 'z'));
            Ok(())
        }
        Regex::CharClass(CharClass::Space { negated: false }) => {
            ranges.push(CharRange::single('\t'));
            ranges.push(CharRange::single('\n'));
            ranges.push(CharRange::single('\u{0B}'));
            ranges.push(CharRange::single('\u{0C}'));
            ranges.push(CharRange::single('\r'));
            ranges.push(CharRange::single(' '));
            Ok(())
        }
        other => Err(make_error(&format!(
            "class_escape resolved to unsupported variant '{other:?}' for char class"
        ))),
    }
}

/// Unicode code points for horizontal whitespace (\h).
#[cfg(feature = "pgen-parser")]
fn horizontal_whitespace_ranges() -> Vec<CharRange> {
    vec![
        CharRange::single('\t'),
        CharRange::single(' '),
        CharRange::single('\u{00A0}'),
        CharRange::single('\u{1680}'),
        CharRange::range('\u{2000}', '\u{200A}'),
        CharRange::single('\u{202F}'),
        CharRange::single('\u{205F}'),
        CharRange::single('\u{3000}'),
    ]
}

/// Unicode code points for vertical whitespace (\v).
#[cfg(feature = "pgen-parser")]
fn vertical_whitespace_ranges() -> Vec<CharRange> {
    vec![
        CharRange::range('\u{000A}', '\u{000D}'),
        CharRange::single('\u{0085}'),
        CharRange::single('\u{2028}'),
        CharRange::single('\u{2029}'),
    ]
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
            "(?|a|b)",
            "(?[[a-z]])",
            r"(?[[\dA-F]])",
            r"(?[[[:graph:]]])",
            r"(?[[\p{L}] - [\p{Lu}]])",
            "(?[[a-z] - [aeiou]])",
            r"(?[\d - [3]])",
            r"(?[\w & [a-z]])",
            r"(?[\D & [A-F]])",
            r"(?[ [:graph:] ])",
            r"(?[ [:^alpha:] ])",
            r"(?[ ![:alpha:] ])",
            r"(?[ [:alpha:] & [a-z\t] ])",
            r"(?[\h])",
            r"(?[\H])",
            r"(?[\v])",
            r"(?[\V])",
            r"(?[\p{L} & \p{Lu}])",
            r"(?[ ![0-9] ])",
            r"(?[ ([a-z] - [aeiou]) & [b-d] ])",
            r"(?[ [AC] ^ [BC] ])",
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
            "(?(R)a|b)",
            "(?(R1)a|b)",
            "(?(R&word)a|b)",
            "(?(DEFINE)a)",
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
            "(?i:abc)",
            "(?ms:^a.b)",
            "(?i)",
            "(?ms)",
            r"(?<word>a)\k<word>",
            r"(?<word>a)\k'word'",
        ]
    }

    #[derive(Clone, Copy)]
    struct ExtendedCharClassExecutionFixture {
        pattern: &'static str,
        matches_input: &'static str,
        rejects_input: &'static str,
        description: &'static str,
    }

    fn assert_extended_char_class_execution_fixture(fixture: ExtendedCharClassExecutionFixture) {
        let regex = crate::Regex::compile(fixture.pattern).unwrap_or_else(|e| {
            panic!(
                "{} fixture should compile on the default path: pattern='{}', error={e}",
                fixture.description, fixture.pattern
            )
        });
        assert!(
            regex.is_match(fixture.matches_input),
            "{} fixture should match '{}'",
            fixture.description,
            fixture.matches_input
        );
        assert!(
            !regex.is_match(fixture.rejects_input),
            "{} fixture should reject '{}'",
            fixture.description,
            fixture.rejects_input
        );
    }

    const SIMPLE_EXTENDED_CHAR_CLASS_EXECUTION_FIXTURES: &[ExtendedCharClassExecutionFixture] =
        &[ExtendedCharClassExecutionFixture {
            pattern: r"\A(?[[a-z]])+\z",
            matches_input: "abcxyz",
            rejects_input: "abc123",
            description: "simple extended character class",
        }];

    const ALGEBRAIC_EXTENDED_CHAR_CLASS_EXECUTION_FIXTURES: &[ExtendedCharClassExecutionFixture] =
        &[
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[[\dA-F]])+\z",
                matches_input: "FACE204",
                rejects_input: "face_",
                description: "nested ordinary shorthand/range extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[[[:graph:]]])+\z",
                matches_input: "AZ9!",
                rejects_input: "AZ 9",
                description: "nested ordinary POSIX extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[[\p{L}] - [\p{Lu}]])+\z",
                matches_input: "facet",
                rejects_input: "Face",
                description: "nested ordinary property algebra extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[[a-z] - [aeiou]])+\z",
                matches_input: "bcdfxyz",
                rejects_input: "facet",
                description: "algebraic extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\p{L} & \p{Lu}])+\z",
                matches_input: "ABCXYZ",
                rejects_input: "ABcXYZ",
                description: "property algebra extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\d - [3]])+\z",
                matches_input: "20479",
                rejects_input: "1234",
                description: "digit shorthand extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\w & [a-z]])+\z",
                matches_input: "facet",
                rejects_input: "face_",
                description: "word shorthand extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\D & [A-F]])+\z",
                matches_input: "FACE",
                rejects_input: "FA3E",
                description: "negated shorthand extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ [:graph:] ])+\z",
                matches_input: "AZ9!",
                rejects_input: "AZ 9",
                description: "POSIX graph extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ [:^alpha:] ])+\z",
                matches_input: "19?!",
                rejects_input: "A1",
                description: "negated POSIX alpha extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ ![:alpha:] ])+\z",
                matches_input: "19?!",
                rejects_input: "A1",
                description: "complemented POSIX alpha extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ [:alpha:] & [a-z\t] ])+\z",
                matches_input: "facet",
                rejects_input: "Face\t",
                description: "POSIX alpha algebra extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\h])+\z",
                matches_input: " \t\u{00A0}\u{1680}\u{202F}\u{3000}",
                rejects_input: "\n \t",
                description: "horizontal-whitespace extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\H])+\z",
                matches_input: "A\nB",
                rejects_input: " \t\u{00A0}",
                description: "negated horizontal-whitespace extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\v])+\z",
                matches_input: "\n\u{000B}\u{0085}\u{2028}\u{2029}",
                rejects_input: " \n",
                description: "vertical-whitespace extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\V])+\z",
                matches_input: "A \u{00A0}\t",
                rejects_input: "\n\u{0085}\u{2028}",
                description: "negated vertical-whitespace extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\x{41} - [B]])+\z",
                matches_input: "AAAA",
                rejects_input: "AAB",
                description: "hex-escape extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\n | \t])+\z",
                matches_input: "\n\t\n",
                rejects_input: " \n",
                description: "control-escape extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\a | \b | \e | \f])+\z",
                matches_input: "\u{07}\u{08}\u{1B}\u{0C}\u{07}",
                rejects_input: "\u{07}A",
                description: "control-literal extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\cA | [B]])+\z",
                matches_input: "\u{0001}B\u{0001}",
                rejects_input: "ABC",
                description: "control-letter extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[\040 | \011 | \o{101}])+\z",
                matches_input: " \tA\t ",
                rejects_input: "\nA",
                description: "octal-escape extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ ![0-9] ])+\z",
                matches_input: "abcXYZ!",
                rejects_input: "abc123",
                description: "complement extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ ([a-z] - [aeiou]) & [b-d] ])+\z",
                matches_input: "bcdb",
                rejects_input: "bef",
                description: "grouped algebra extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ [AC] ^ [BC] ])+\z",
                matches_input: "ABBA",
                rejects_input: "AC",
                description: "symmetric-difference extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ [a-f] | [d-z] & [m-p] ])+\z",
                matches_input: "abcmnop",
                rejects_input: "xyz",
                description: "same-level precedence extended character class",
            },
            ExtendedCharClassExecutionFixture {
                pattern: r"\A(?[ [a-z] - [aeiou] + [0-9] - [5] ])+\z",
                matches_input: "bcdf0249xyz",
                rejects_input: "face5",
                description: "chained low-precedence extended character class",
            },
        ];

    #[test]
    fn test_zero_cost_parsing() {
        let result = parse_pattern("abc");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parser_name() {
        let name = parser_name();
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
    fn parser_contract_active_parser_fixtures() {
        for pattern in parser_contract_reference_fixtures() {
            parse_pattern(pattern)
                .unwrap_or_else(|e| panic!("active parser failed for pattern '{pattern}': {e}"));
        }
    }

    #[cfg(feature = "pgen-parser")]
    #[test]
    fn parser_contract_pgen_backend_fixtures() {
        for pattern in parser_contract_reference_fixtures() {
            let mut pgen = PgenParser::new();
            pgen.parse_pattern(pattern)
                .unwrap_or_else(|e| panic!("pgen parser failed for pattern '{pattern}': {e}"));
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
            (
                "(?(R2)a|b)",
                "conditional '(?(R2)...)' refers to missing capture group",
            ),
            (
                "(?(R&missing)a|b)",
                "conditional '(?(R&missing)...)' refers to missing named capture group",
            ),
            (
                r"(?[a-z])",
                crate::compiler::EXTENDED_CHAR_CLASS_SUBSET_MESSAGE,
            ),
        ];

        for (pattern, expected_msg) in cases {
            parse_pattern(pattern).unwrap_or_else(|e| {
                panic!("parser should accept contract fixture '{pattern}': {e}")
            });
            let Err(err) = compiler.compile(pattern) else {
                panic!(
                    "pattern should fail with an explicit compile-time boundary/validation error: {pattern}"
                )
            };
            assert!(
                err.to_string().contains(expected_msg),
                "unexpected compile boundary message for pattern '{pattern}': {err}"
            );
        }
    }

    #[test]
    fn parser_contract_branch_reset_group_executes_on_default_path() {
        let regex = crate::Regex::compile(r"\A(?|(a)|(b))\1\z")
            .expect("branch-reset fixture should compile on the default path");
        assert!(regex.is_match("aa"));
        assert!(regex.is_match("bb"));
        assert!(!regex.is_match("ab"));
    }

    #[test]
    fn parser_contract_simple_extended_char_class_executes_on_default_path() {
        for fixture in SIMPLE_EXTENDED_CHAR_CLASS_EXECUTION_FIXTURES {
            assert_extended_char_class_execution_fixture(*fixture);
        }
    }

    #[test]
    fn parser_contract_algebraic_extended_char_class_executes_on_default_path() {
        for fixture in ALGEBRAIC_EXTENDED_CHAR_CLASS_EXECUTION_FIXTURES {
            assert_extended_char_class_execution_fixture(*fixture);
        }
    }

    #[test]
    fn inline_flag_case_insensitive() {
        let re = crate::Regex::compile("(?i)abc").unwrap();
        assert!(re.is_match("ABC"));
        assert!(re.is_match("abc"));
    }

    #[test]
    fn inline_flag_multiline() {
        let re = crate::Regex::compile("(?m)^a").unwrap();
        assert!(re.is_match("b\na"));
    }

    #[test]
    fn inline_flag_combined() {
        let re = crate::Regex::compile("(?im)^abc").unwrap();
        assert!(re.is_match("x\nABC"));
    }
}
