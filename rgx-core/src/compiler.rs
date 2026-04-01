use crate::ast::Regex as RegexAst;
use crate::ast::{CharClass, CharRange};
use crate::engine::ExecutionMode;
use crate::error::{Result, RgxError};
use crate::parser::Parser as ReferenceParser;
use crate::parsing;
use crate::pattern::CompiledPattern;
use crate::unicode_support::resolve_unicode_property_class;
use crate::vm::OptimizingCompiler as VMCompiler;
use crate::{debug_log, low_log, trace_decision, trace_enter, trace_exit, trace_log};

/// Compiler that transforms regex patterns into optimized execution programs
pub struct Compiler {
    mode: ExecutionMode,
}

type ScalarRange = (u32, u32);

const UNICODE_SCALAR_UNIVERSE: [ScalarRange; 2] = [(0x0000, 0xD7FF), (0xE000, 0x10FFFF)];
const ASCII_DIGIT_RANGES: [ScalarRange; 1] = [('0' as u32, '9' as u32)];
const ASCII_WORD_RANGES: [ScalarRange; 4] = [
    ('0' as u32, '9' as u32),
    ('A' as u32, 'Z' as u32),
    ('_' as u32, '_' as u32),
    ('a' as u32, 'z' as u32),
];
const ASCII_SPACE_RANGES: [ScalarRange; 2] = [(0x09, 0x0D), (' ' as u32, ' ' as u32)];

#[derive(Clone, Copy)]
enum ExtendedCharClassOperator {
    Union,
    Difference,
    Intersection,
    SymmetricDifference,
}

impl ExtendedCharClassOperator {
    fn from_char(ch: char) -> Option<Self> {
        match ch {
            '|' | '+' => Some(Self::Union),
            '-' => Some(Self::Difference),
            '&' => Some(Self::Intersection),
            '^' => Some(Self::SymmetricDifference),
            _ => None,
        }
    }

    fn precedence(self) -> u8 {
        match self {
            Self::Intersection => 2,
            Self::Union | Self::Difference | Self::SymmetricDifference => 1,
        }
    }

    fn apply(self, lhs: ScalarRangeSet, rhs: ScalarRangeSet) -> ScalarRangeSet {
        match self {
            Self::Union => lhs.union(&rhs),
            Self::Difference => lhs.difference(&rhs),
            Self::Intersection => lhs.intersection(&rhs),
            Self::SymmetricDifference => lhs.difference(&rhs).union(&rhs.difference(&lhs)),
        }
    }
}

struct ExtendedCharClassCursor<'a> {
    input: &'a str,
    offset: usize,
}

impl<'a> ExtendedCharClassCursor<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, offset: 0 }
    }

    fn is_eof(&self) -> bool {
        self.offset >= self.input.len()
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.offset..]
    }

    fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn consume_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.offset += ch.len_utf8();
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_char(), Some(ch) if ch.is_whitespace()) {
            self.consume_char();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScalarRangeSet {
    ranges: Vec<ScalarRange>,
}

pub(crate) const EXTENDED_CHAR_CLASS_SUBSET_MESSAGE: &str = "Perl extended character classes '(?[...])' currently support bracket/property terms, bare shorthand terms ('\\d', '\\D', '\\w', '\\W', '\\s', '\\S'), bare escaped literal/codepoint terms such as '\\n', '\\t', '\\x{41}', and '\\-', unary complement ('!'), grouped subexpressions, and left-associative set algebra with '&' binding tighter than '|', '+', '-', and '^' in rgx, such as '(?[ \\x{41} - [B] ])', '(?[ \\n | \\t ])', '(?[ [a-f] | [d-z] & [m-p] ])', or '(?[ [a-z] - [aeiou] + [0-9] - [5] ])'; wider set-expression forms and additional bare-term families beyond the current bracket/property/shorthand/escaped-term subset remain unsupported";

impl ScalarRangeSet {
    fn new(ranges: Vec<ScalarRange>) -> Self {
        Self {
            ranges: Self::normalize(ranges),
        }
    }

    fn from_char(ch: char) -> Self {
        Self::new(vec![(ch as u32, ch as u32)])
    }

    fn from_char_ranges(ranges: &[CharRange]) -> Self {
        Self::new(
            ranges
                .iter()
                .map(|range| (range.start as u32, range.end as u32))
                .collect(),
        )
    }

    fn from_unicode_property(name: &str, negated: bool) -> Result<Self> {
        let ranges = resolve_unicode_property_class(name, negated).map_err(RgxError::Compile)?;
        Ok(Self::from_char_ranges(&ranges))
    }

    fn from_ascii_ranges(ranges: &[ScalarRange], negated: bool) -> Self {
        let set = Self::new(ranges.to_vec());
        if negated {
            set.complement()
        } else {
            set
        }
    }

    fn from_char_class(char_class: &crate::ast::CharClass) -> Result<Self> {
        match char_class {
            crate::ast::CharClass::Digit { negated } => {
                Ok(Self::from_ascii_ranges(&ASCII_DIGIT_RANGES, *negated))
            }
            crate::ast::CharClass::Word { negated } => {
                Ok(Self::from_ascii_ranges(&ASCII_WORD_RANGES, *negated))
            }
            crate::ast::CharClass::Space { negated } => {
                Ok(Self::from_ascii_ranges(&ASCII_SPACE_RANGES, *negated))
            }
            crate::ast::CharClass::Custom { ranges, negated } => {
                let set = Self::from_char_ranges(ranges);
                if *negated {
                    Ok(set.complement())
                } else {
                    Ok(set)
                }
            }
            crate::ast::CharClass::UnicodeClass { name, negated } => {
                Self::from_unicode_property(name, *negated)
            }
        }
    }

    fn union(&self, other: &Self) -> Self {
        let mut combined = self.ranges.clone();
        combined.extend_from_slice(&other.ranges);
        Self::new(combined)
    }

    fn intersection(&self, other: &Self) -> Self {
        let mut result = Vec::new();
        let mut lhs_index = 0usize;
        let mut rhs_index = 0usize;

        while lhs_index < self.ranges.len() && rhs_index < other.ranges.len() {
            let (lhs_start, lhs_end) = self.ranges[lhs_index];
            let (rhs_start, rhs_end) = other.ranges[rhs_index];
            let start = lhs_start.max(rhs_start);
            let end = lhs_end.min(rhs_end);

            if start <= end {
                result.push((start, end));
            }

            if lhs_end < rhs_end {
                lhs_index += 1;
            } else {
                rhs_index += 1;
            }
        }

        Self::new(result)
    }

    fn difference(&self, other: &Self) -> Self {
        let mut result = Vec::new();
        let mut rhs_index = 0usize;

        for (lhs_start, lhs_end) in &self.ranges {
            let mut cursor = *lhs_start;

            while rhs_index < other.ranges.len() && other.ranges[rhs_index].1 < cursor {
                rhs_index += 1;
            }

            let mut scan_index = rhs_index;
            while scan_index < other.ranges.len() && other.ranges[scan_index].0 <= *lhs_end {
                let (rhs_start, rhs_end) = other.ranges[scan_index];
                if rhs_start > cursor {
                    result.push((cursor, rhs_start.saturating_sub(1)));
                }

                cursor = rhs_end.saturating_add(1);
                if cursor > *lhs_end {
                    break;
                }
                scan_index += 1;
            }

            if cursor <= *lhs_end {
                result.push((cursor, *lhs_end));
            }
        }

        Self::new(result)
    }

    fn complement(&self) -> Self {
        Self::new(UNICODE_SCALAR_UNIVERSE.to_vec()).difference(self)
    }

    fn to_char_ranges(&self) -> Result<Vec<CharRange>> {
        self.ranges
            .iter()
            .map(|(start, end)| {
                let start = char::from_u32(*start)
                    .ok_or_else(Compiler::extended_char_class_subset_error)?;
                let end =
                    char::from_u32(*end).ok_or_else(Compiler::extended_char_class_subset_error)?;
                Ok(CharRange::range(start, end))
            })
            .collect()
    }

    fn normalize(mut ranges: Vec<ScalarRange>) -> Vec<ScalarRange> {
        ranges.sort_by_key(|range| range.0);

        let mut merged: Vec<ScalarRange> = Vec::new();
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut() {
                if start <= last.1.saturating_add(1) {
                    last.1 = last.1.max(end);
                } else {
                    merged.push((start, end));
                }
            } else {
                merged.push((start, end));
            }
        }

        merged
    }
}

impl Compiler {
    /// Create new compiler with pure execution mode (maximum performance)
    pub fn new() -> Self {
        trace_enter!("compiler", "Compiler::new");
        let compiler = Self {
            mode: ExecutionMode::Pure,
        };
        trace_exit!(
            "compiler",
            "Compiler::new",
            "ok=true,mode={:?}",
            compiler.mode
        );
        compiler
    }

    /// Create compiler with specific execution mode
    pub fn with_mode(mode: ExecutionMode) -> Self {
        trace_enter!("compiler", "Compiler::with_mode", "mode={:?}", mode);
        let compiler = Self { mode };
        trace_decision!(
            "compiler",
            "mode == ExecutionMode::Pure",
            mode == ExecutionMode::Pure,
            "constructor mode selection"
        );
        trace_exit!(
            "compiler",
            "Compiler::with_mode",
            "ok=true,mode={:?}",
            compiler.mode
        );
        compiler
    }

    /// Compile regex pattern into optimized bytecode program
    pub fn compile(&self, pattern: &str) -> Result<CompiledPattern> {
        trace_enter!(
            "compiler",
            "Compiler::compile",
            "pattern_len={}, mode={:?}",
            pattern.len(),
            self.mode
        );
        low_log!("compiler", "");
        low_log!("compiler", "=== COMPILER PIPELINE START ===");
        debug_log!("compiler", "=== STARTING COMPILATION ===");
        debug_log!("compiler", "Pattern: '{}'", pattern);
        debug_log!("compiler", "Mode: {:?}", self.mode);

        if pattern.is_empty() {
            trace_decision!(
                "compiler",
                "pattern.is_empty()",
                true,
                "reject compile request with explicit compile error"
            );
            debug_log!("compiler", "ERROR: Empty pattern");
            trace_exit!(
                "compiler",
                "Compiler::compile",
                "error=empty pattern compile failure"
            );
            return Err(RgxError::Compile("empty pattern".into()));
        }
        trace_decision!(
            "compiler",
            "pattern.is_empty()",
            false,
            "continue with parser and bytecode compilation"
        );

        // Parse pattern into AST using zero-cost compile-time selected parser
        debug_log!("compiler", "Parsing pattern into AST...");
        let ast = parsing::parse_pattern(pattern)?;
        let result = self.compile_ast_with_label(ast, pattern);
        trace_exit!("compiler", "Compiler::compile", "ok={}", result.is_ok());
        result
    }

    /// Compile a pre-built AST into optimized VM bytecode.
    ///
    /// This enables parser-independent development of VM/compiler/engine
    /// while parser work progresses in parallel.
    pub fn compile_ast(&self, ast: RegexAst) -> Result<CompiledPattern> {
        trace_enter!(
            "compiler",
            "Compiler::compile_ast",
            "mode={:?}, ast={:?}",
            self.mode,
            ast
        );
        debug_log!("compiler", "=== STARTING AST-ONLY COMPILATION ===");
        debug_log!("compiler", "Mode: {:?}", self.mode);
        let result = self.compile_ast_with_label(ast, "<ast>");
        trace_exit!("compiler", "Compiler::compile_ast", "ok={}", result.is_ok());
        result
    }

    fn compile_ast_with_label(&self, ast: RegexAst, raw_label: &str) -> Result<CompiledPattern> {
        trace_enter!(
            "compiler",
            "Compiler::compile_ast_with_label",
            "raw_label={}, mode={:?}",
            raw_label,
            self.mode
        );
        let ast = Self::assign_capture_indices(ast);
        let ast = Self::lower_extended_char_classes(ast)?;
        debug_log!("compiler", "AST: {:?}", ast);
        if let Some(msg) = Self::parser_boundary_validation_message(&ast) {
            trace_exit!(
                "compiler",
                "Compiler::compile_ast_with_label",
                "error={}",
                msg
            );
            return Err(RgxError::Compile(msg));
        }
        let total_groups = Self::max_capture_group(&ast);
        let named_groups = Self::collect_named_groups(&ast);
        let ast = Self::resolve_relative_conditionals(ast, total_groups)?;
        let ast = Self::resolve_recursion_conditionals(ast, total_groups, &named_groups)?;

        if let Some(msg) = Self::backreference_validation_message(&ast) {
            trace_exit!(
                "compiler",
                "Compiler::compile_ast_with_label",
                "error={}",
                msg
            );
            return Err(RgxError::Compile(msg));
        }
        if let Some(msg) = self.feature_validation_message(&ast, total_groups, &named_groups) {
            trace_decision!(
                "compiler",
                "feature_validation_message(ast).is_some()",
                true,
                "rejecting AST at compile boundary: {}",
                msg
            );
            trace_exit!(
                "compiler",
                "Compiler::compile_ast_with_label",
                "error={}",
                msg
            );
            return Err(RgxError::Compile(msg.to_string()));
        }
        trace_decision!(
            "compiler",
            "feature_validation_message(ast).is_some()",
            false,
            "AST is eligible for VM compilation"
        );

        // Compile AST into optimized VM bytecode
        debug_log!("compiler", "Compiling AST to VM bytecode...");
        let mut vm_compiler = VMCompiler::with_named_groups(named_groups.clone());
        let mut program = vm_compiler.compile(&ast);
        program.named_groups = named_groups;

        debug_log!("compiler", "Program compiled:");
        debug_log!(
            "compiler",
            "  - Bytecode length: {} bytes",
            program.code.len()
        );
        debug_log!(
            "compiler",
            "  - Character classes: {}",
            program.char_classes.len()
        );
        debug_log!(
            "compiler",
            "  - String literals: {}",
            program.string_literals.len()
        );
        debug_log!("compiler", "  - Capture groups: {}", program.num_groups);
        debug_log!("compiler", "  - Flags: {:?}", program.flags);
        debug_log!("compiler", "  - Stats: {:?}", program.stats);

        trace_log!("compiler", "Full program: {:?}", program);

        // Hex dump the bytecode for debugging
        crate::log::hex_dump("compiler", "VM Bytecode", &program.code);

        debug_log!("compiler", "=== COMPILATION COMPLETE ===");
        low_log!("compiler", "=== COMPILER PIPELINE COMPLETE ===");
        low_log!("compiler", "");
        trace_exit!(
            "compiler",
            "Compiler::compile_ast_with_label",
            "bytecode_len={}, groups={}",
            program.code.len(),
            program.num_groups
        );
        Ok(CompiledPattern {
            raw: raw_label.to_string(),
            mode: self.mode,
            ast,
            program,
        })
    }

    fn resolve_relative_conditionals(ast: RegexAst, total_groups: u32) -> Result<RegexAst> {
        let (ast, resolved_groups) =
            Self::resolve_relative_conditionals_inner(ast, 0, total_groups)?;
        debug_assert_eq!(resolved_groups, total_groups);
        Ok(ast)
    }

    fn extended_char_class_subset_error() -> RgxError {
        RgxError::Compile(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE.to_string())
    }

    fn lower_extended_char_classes(ast: RegexAst) -> Result<RegexAst> {
        match ast {
            RegexAst::Sequence(items) => Ok(RegexAst::Sequence(
                items
                    .into_iter()
                    .map(Self::lower_extended_char_classes)
                    .collect::<Result<Vec<_>>>()?,
            )),
            RegexAst::Alternation(items) => Ok(RegexAst::Alternation(
                items
                    .into_iter()
                    .map(Self::lower_extended_char_classes)
                    .collect::<Result<Vec<_>>>()?,
            )),
            RegexAst::Quantified { expr, quantifier } => Ok(RegexAst::Quantified {
                expr: Box::new(Self::lower_extended_char_classes(*expr)?),
                quantifier,
            }),
            RegexAst::Group {
                expr,
                kind,
                index,
                name,
            } => Ok(RegexAst::Group {
                expr: Box::new(Self::lower_extended_char_classes(*expr)?),
                kind,
                index,
                name,
            }),
            RegexAst::Lookahead { expr, positive } => Ok(RegexAst::Lookahead {
                expr: Box::new(Self::lower_extended_char_classes(*expr)?),
                positive,
            }),
            RegexAst::Lookbehind { expr, positive } => Ok(RegexAst::Lookbehind {
                expr: Box::new(Self::lower_extended_char_classes(*expr)?),
                positive,
            }),
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => Ok(RegexAst::Conditional {
                condition: Self::lower_extended_char_class_condition(condition)?,
                true_branch: Box::new(Self::lower_extended_char_classes(*true_branch)?),
                false_branch: false_branch
                    .map(|branch| Self::lower_extended_char_classes(*branch).map(Box::new))
                    .transpose()?,
            }),
            RegexAst::ExtendedCharClass { content } => {
                Self::lower_extended_char_class_content(content)
            }
            other => Ok(other),
        }
    }

    fn lower_extended_char_class_condition(
        condition: crate::ast::ConditionalTest,
    ) -> Result<crate::ast::ConditionalTest> {
        match condition {
            crate::ast::ConditionalTest::Lookahead { expr, positive } => {
                Ok(crate::ast::ConditionalTest::Lookahead {
                    expr: Box::new(Self::lower_extended_char_classes(*expr)?),
                    positive,
                })
            }
            crate::ast::ConditionalTest::Lookbehind { expr, positive } => {
                Ok(crate::ast::ConditionalTest::Lookbehind {
                    expr: Box::new(Self::lower_extended_char_classes(*expr)?),
                    positive,
                })
            }
            crate::ast::ConditionalTest::GroupExists(group) => {
                Ok(crate::ast::ConditionalTest::GroupExists(group))
            }
            crate::ast::ConditionalTest::RelativeGroupExists(offset) => {
                Ok(crate::ast::ConditionalTest::RelativeGroupExists(offset))
            }
            crate::ast::ConditionalTest::NamedGroupExists(name) => {
                Ok(crate::ast::ConditionalTest::NamedGroupExists(name))
            }
            crate::ast::ConditionalTest::RecursionAny => {
                Ok(crate::ast::ConditionalTest::RecursionAny)
            }
            crate::ast::ConditionalTest::RecursionGroup(group) => {
                Ok(crate::ast::ConditionalTest::RecursionGroup(group))
            }
            crate::ast::ConditionalTest::RecursionNamed(name) => {
                Ok(crate::ast::ConditionalTest::RecursionNamed(name))
            }
            crate::ast::ConditionalTest::Define => Ok(crate::ast::ConditionalTest::Define),
        }
    }

    fn lower_extended_char_class_content(content: String) -> Result<RegexAst> {
        Ok(RegexAst::CharClass(crate::ast::CharClass::Custom {
            ranges: Self::resolve_extended_char_class_ranges(&content)?,
            negated: false,
        }))
    }

    fn extract_simple_extended_char_class_body(content: &str) -> Option<&str> {
        if !content.starts_with('[') {
            return None;
        }

        let mut depth = 0usize;
        let mut escaped = false;
        let mut closed_at = None;

        for (idx, ch) in content.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                '[' => depth = depth.saturating_add(1),
                ']' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        closed_at = Some(idx + ch.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }

        if depth != 0 || closed_at != Some(content.len()) || content.len() < 2 {
            return None;
        }

        Some(&content[1..content.len() - 1])
    }

    fn validate_simple_char_class_body(content: &str) -> Result<()> {
        if content.is_empty() {
            return Err(Self::extended_char_class_subset_error());
        }

        let mut index = 0usize;
        let mut chars = content.chars();

        while let Some(ch) = chars.next() {
            if ch.is_whitespace() {
                return Err(Self::extended_char_class_subset_error());
            }

            if ch == '\\' {
                let Some(escaped) = chars.next() else {
                    return Err(Self::extended_char_class_subset_error());
                };

                match escaped {
                    'n' | 't' | 'r' | '\\' | ']' | '-' | '^' => {}
                    _ => return Err(Self::extended_char_class_subset_error()),
                }

                index += 2;
                continue;
            }

            match ch {
                '[' | ']' | '&' | '+' | '|' | '!' => {
                    return Err(Self::extended_char_class_subset_error());
                }
                '^' if index != 0 => {
                    return Err(Self::extended_char_class_subset_error());
                }
                _ => {}
            }

            index += 1;
        }

        Ok(())
    }

    fn resolve_extended_char_class_ranges(content: &str) -> Result<Vec<CharRange>> {
        let mut cursor = ExtendedCharClassCursor::new(content);
        let resolved = Self::parse_extended_char_class_expr(&mut cursor)?;
        cursor.skip_whitespace();
        if !cursor.is_eof() {
            return Err(Self::extended_char_class_subset_error());
        }

        resolved.to_char_ranges()
    }

    fn parse_extended_char_class_expr(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        Self::parse_extended_char_class_binary_expr(cursor, 1)
    }

    // PCRE2 extended classes give `&` tighter binding than the other shipped
    // same-level operators, which are otherwise left-associative.
    fn parse_extended_char_class_binary_expr(
        cursor: &mut ExtendedCharClassCursor<'_>,
        min_precedence: u8,
    ) -> Result<ScalarRangeSet> {
        let mut lhs = Self::parse_extended_char_class_unary(cursor)?;

        loop {
            let Some(operator) = Self::peek_extended_char_class_operator(cursor) else {
                return Ok(lhs);
            };

            if operator.precedence() < min_precedence {
                return Ok(lhs);
            }

            cursor.consume_char();
            let rhs =
                Self::parse_extended_char_class_binary_expr(cursor, operator.precedence() + 1)?;
            lhs = operator.apply(lhs, rhs);
        }
    }

    fn peek_extended_char_class_operator(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Option<ExtendedCharClassOperator> {
        cursor.skip_whitespace();
        cursor
            .peek_char()
            .and_then(ExtendedCharClassOperator::from_char)
    }

    fn parse_extended_char_class_unary(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        cursor.skip_whitespace();
        if matches!(cursor.peek_char(), Some('!')) {
            cursor.consume_char();
            return Ok(Self::parse_extended_char_class_unary(cursor)?.complement());
        }

        Self::parse_extended_char_class_term(cursor)
    }

    fn parse_extended_char_class_term(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        cursor.skip_whitespace();
        match cursor.peek_char() {
            Some('[') => {
                let term = Self::consume_extended_char_class_bracket_term(cursor)?;
                Self::resolve_extended_bracket_term_ranges(term)
            }
            Some('\\') => Self::resolve_extended_escape_term(cursor),
            Some('(') => {
                cursor.consume_char();
                let inner = Self::parse_extended_char_class_expr(cursor)?;
                cursor.skip_whitespace();
                match cursor.consume_char() {
                    Some(')') => Ok(inner),
                    _ => Err(Self::extended_char_class_subset_error()),
                }
            }
            _ => Err(Self::extended_char_class_subset_error()),
        }
    }

    fn consume_extended_char_class_bracket_term<'a>(
        cursor: &mut ExtendedCharClassCursor<'a>,
    ) -> Result<&'a str> {
        let start = cursor.offset;
        let mut depth = 0usize;
        let mut escaped = false;

        while let Some(ch) = cursor.consume_char() {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                '[' => depth = depth.saturating_add(1),
                ']' => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(Self::extended_char_class_subset_error)?;
                    if depth == 0 {
                        return Ok(&cursor.input[start..cursor.offset]);
                    }
                }
                _ => {}
            }
        }

        Err(Self::extended_char_class_subset_error())
    }

    fn resolve_extended_bracket_term_ranges(term: &str) -> Result<ScalarRangeSet> {
        let body = Self::extract_simple_extended_char_class_body(term)
            .ok_or_else(Self::extended_char_class_subset_error)?;
        Self::validate_simple_char_class_body(body)?;

        let mut parser =
            ReferenceParser::new(term).map_err(|_| Self::extended_char_class_subset_error())?;
        let lowered = parser
            .parse()
            .map_err(|_| Self::extended_char_class_subset_error())?;

        match lowered {
            RegexAst::CharClass(char_class) => Self::resolve_char_class_scalar_ranges(&char_class),
            _ => Err(Self::extended_char_class_subset_error()),
        }
    }

    fn resolve_extended_escape_term(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        let slash = cursor.consume_char();
        let Some(kind) = cursor.consume_char() else {
            return Err(Self::extended_char_class_subset_error());
        };

        if slash != Some('\\') {
            return Err(Self::extended_char_class_subset_error());
        }

        if let Some(ch) = Self::resolve_extended_literal_escape(kind) {
            return Ok(ScalarRangeSet::from_char(ch));
        }

        match kind {
            'd' => Self::resolve_char_class_scalar_ranges(&CharClass::Digit { negated: false }),
            'D' => Self::resolve_char_class_scalar_ranges(&CharClass::Digit { negated: true }),
            'w' => Self::resolve_char_class_scalar_ranges(&CharClass::Word { negated: false }),
            'W' => Self::resolve_char_class_scalar_ranges(&CharClass::Word { negated: true }),
            's' => Self::resolve_char_class_scalar_ranges(&CharClass::Space { negated: false }),
            'S' => Self::resolve_char_class_scalar_ranges(&CharClass::Space { negated: true }),
            'x' => Self::resolve_extended_hex_escape_term(cursor),
            'p' | 'P' => Self::resolve_extended_unicode_property_escape_term(kind, cursor),
            _ => Err(Self::extended_char_class_subset_error()),
        }
    }

    fn resolve_extended_literal_escape(kind: char) -> Option<char> {
        match kind {
            'n' => Some('\n'),
            't' => Some('\t'),
            'r' => Some('\r'),
            'f' => Some('\u{0C}'),
            'a' => Some('\u{07}'),
            'e' => Some('\u{1B}'),
            escaped @ ('.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}'
            | '|' | '\\' | '-') => Some(escaped),
            _ => None,
        }
    }

    fn resolve_extended_hex_escape_term(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        let mut hex_digits = String::new();

        if cursor.peek_char() == Some('{') {
            cursor.consume_char();
            let mut closed = false;
            while let Some(ch) = cursor.consume_char() {
                if ch == '}' {
                    closed = true;
                    break;
                }
                if !ch.is_ascii_hexdigit() {
                    return Err(Self::extended_char_class_subset_error());
                }
                hex_digits.push(ch);
            }

            if hex_digits.is_empty() || !closed {
                return Err(Self::extended_char_class_subset_error());
            }
        } else {
            while hex_digits.len() < 2 {
                let Some(ch) = cursor.peek_char() else {
                    break;
                };
                if !ch.is_ascii_hexdigit() {
                    break;
                }
                hex_digits.push(ch);
                cursor.consume_char();
            }

            if hex_digits.is_empty() {
                return Err(Self::extended_char_class_subset_error());
            }
        }

        let code_point = u32::from_str_radix(&hex_digits, 16)
            .map_err(|_| Self::extended_char_class_subset_error())?;
        let ch = char::from_u32(code_point).ok_or_else(Self::extended_char_class_subset_error)?;
        Ok(ScalarRangeSet::from_char(ch))
    }

    fn resolve_extended_unicode_property_escape_term(
        kind: char,
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        let name = Self::consume_extended_braced_name(cursor)?;
        ScalarRangeSet::from_unicode_property(&name, kind == 'P')
    }

    fn consume_extended_braced_name(cursor: &mut ExtendedCharClassCursor<'_>) -> Result<String> {
        if cursor.consume_char() != Some('{') {
            return Err(Self::extended_char_class_subset_error());
        }

        let mut name = String::new();
        while let Some(ch) = cursor.consume_char() {
            if ch == '}' {
                if name.is_empty() {
                    return Err(Self::extended_char_class_subset_error());
                }
                return Ok(name);
            }
            name.push(ch);
        }

        Err(Self::extended_char_class_subset_error())
    }

    fn resolve_char_class_scalar_ranges(
        char_class: &crate::ast::CharClass,
    ) -> Result<ScalarRangeSet> {
        ScalarRangeSet::from_char_class(char_class)
    }

    fn assign_capture_indices(ast: RegexAst) -> RegexAst {
        let (ast, _) = Self::assign_capture_indices_inner(ast, 1);
        ast
    }

    fn assign_capture_indices_inner(ast: RegexAst, next_group: u32) -> (RegexAst, u32) {
        match ast {
            RegexAst::Sequence(items) => {
                let mut next = next_group;
                let mut assigned = Vec::with_capacity(items.len());
                for item in items {
                    let (item, assigned_next) = Self::assign_capture_indices_inner(item, next);
                    next = assigned_next;
                    assigned.push(item);
                }
                (RegexAst::Sequence(assigned), next)
            }
            RegexAst::Alternation(items) => {
                let mut next = next_group;
                let mut assigned = Vec::with_capacity(items.len());
                for item in items {
                    let (item, assigned_next) = Self::assign_capture_indices_inner(item, next);
                    next = assigned_next;
                    assigned.push(item);
                }
                (RegexAst::Alternation(assigned), next)
            }
            RegexAst::Quantified { expr, quantifier } => {
                let (expr, next) = Self::assign_capture_indices_inner(*expr, next_group);
                (
                    RegexAst::Quantified {
                        expr: Box::new(expr),
                        quantifier,
                    },
                    next,
                )
            }
            RegexAst::Group {
                expr, kind, name, ..
            } => match kind {
                crate::ast::GroupKind::Capturing => {
                    let group_id = next_group;
                    let (expr, next) =
                        Self::assign_capture_indices_inner(*expr, group_id.saturating_add(1));
                    (
                        RegexAst::Group {
                            expr: Box::new(expr),
                            kind,
                            index: Some(group_id),
                            name,
                        },
                        next,
                    )
                }
                crate::ast::GroupKind::BranchReset => {
                    let (expr, next) = Self::assign_branch_reset_expr(*expr, next_group);
                    (
                        RegexAst::Group {
                            expr: Box::new(expr),
                            kind,
                            index: None,
                            name,
                        },
                        next,
                    )
                }
                crate::ast::GroupKind::NonCapturing | crate::ast::GroupKind::Atomic => {
                    let (expr, next) = Self::assign_capture_indices_inner(*expr, next_group);
                    (
                        RegexAst::Group {
                            expr: Box::new(expr),
                            kind,
                            index: None,
                            name,
                        },
                        next,
                    )
                }
            },
            RegexAst::Lookahead { expr, positive } => {
                let (expr, next) = Self::assign_capture_indices_inner(*expr, next_group);
                (
                    RegexAst::Lookahead {
                        expr: Box::new(expr),
                        positive,
                    },
                    next,
                )
            }
            RegexAst::Lookbehind { expr, positive } => {
                let (expr, next) = Self::assign_capture_indices_inner(*expr, next_group);
                (
                    RegexAst::Lookbehind {
                        expr: Box::new(expr),
                        positive,
                    },
                    next,
                )
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let (condition, next_after_condition) =
                    Self::assign_capture_indices_condition(condition, next_group);
                let (true_branch, next_after_true) =
                    Self::assign_capture_indices_inner(*true_branch, next_after_condition);
                let (false_branch, next_after_false) = if let Some(false_branch) = false_branch {
                    let (false_branch, next_after_false) =
                        Self::assign_capture_indices_inner(*false_branch, next_after_true);
                    (Some(Box::new(false_branch)), next_after_false)
                } else {
                    (None, next_after_true)
                };
                (
                    RegexAst::Conditional {
                        condition,
                        true_branch: Box::new(true_branch),
                        false_branch,
                    },
                    next_after_false,
                )
            }
            RegexAst::Char(_)
            | RegexAst::CharClass(_)
            | RegexAst::Dot
            | RegexAst::Digit { .. }
            | RegexAst::Word { .. }
            | RegexAst::Space { .. }
            | RegexAst::UnicodeClass { .. }
            | RegexAst::ExtendedCharClass { .. }
            | RegexAst::Anchor(_)
            | RegexAst::WordBoundary { .. }
            | RegexAst::Backreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Empty => (ast, next_group),
        }
    }

    fn assign_branch_reset_expr(expr: RegexAst, next_group: u32) -> (RegexAst, u32) {
        match expr {
            RegexAst::Alternation(items) => {
                let mut max_next = next_group;
                let mut assigned = Vec::with_capacity(items.len());
                for item in items {
                    let (item, assigned_next) =
                        Self::assign_capture_indices_inner(item, next_group);
                    max_next = max_next.max(assigned_next);
                    assigned.push(item);
                }
                (RegexAst::Alternation(assigned), max_next)
            }
            other => Self::assign_capture_indices_inner(other, next_group),
        }
    }

    fn assign_capture_indices_condition(
        condition: crate::ast::ConditionalTest,
        next_group: u32,
    ) -> (crate::ast::ConditionalTest, u32) {
        match condition {
            crate::ast::ConditionalTest::Lookahead { expr, positive } => {
                let (expr, next) = Self::assign_capture_indices_inner(*expr, next_group);
                (
                    crate::ast::ConditionalTest::Lookahead {
                        expr: Box::new(expr),
                        positive,
                    },
                    next,
                )
            }
            crate::ast::ConditionalTest::Lookbehind { expr, positive } => {
                let (expr, next) = Self::assign_capture_indices_inner(*expr, next_group);
                (
                    crate::ast::ConditionalTest::Lookbehind {
                        expr: Box::new(expr),
                        positive,
                    },
                    next,
                )
            }
            crate::ast::ConditionalTest::GroupExists(group) => {
                (crate::ast::ConditionalTest::GroupExists(group), next_group)
            }
            crate::ast::ConditionalTest::RelativeGroupExists(offset) => (
                crate::ast::ConditionalTest::RelativeGroupExists(offset),
                next_group,
            ),
            crate::ast::ConditionalTest::NamedGroupExists(name) => (
                crate::ast::ConditionalTest::NamedGroupExists(name),
                next_group,
            ),
            crate::ast::ConditionalTest::RecursionAny => {
                (crate::ast::ConditionalTest::RecursionAny, next_group)
            }
            crate::ast::ConditionalTest::RecursionGroup(group) => (
                crate::ast::ConditionalTest::RecursionGroup(group),
                next_group,
            ),
            crate::ast::ConditionalTest::RecursionNamed(name) => (
                crate::ast::ConditionalTest::RecursionNamed(name),
                next_group,
            ),
            crate::ast::ConditionalTest::Define => {
                (crate::ast::ConditionalTest::Define, next_group)
            }
        }
    }

    fn resolve_relative_conditionals_inner(
        ast: RegexAst,
        opened_groups: u32,
        total_groups: u32,
    ) -> Result<(RegexAst, u32)> {
        match ast {
            RegexAst::Sequence(items) => {
                let mut next_opened = opened_groups;
                let mut resolved = Vec::with_capacity(items.len());
                for item in items {
                    let (item, opened_after_item) =
                        Self::resolve_relative_conditionals_inner(item, next_opened, total_groups)?;
                    next_opened = opened_after_item;
                    resolved.push(item);
                }
                Ok((RegexAst::Sequence(resolved), next_opened))
            }
            RegexAst::Alternation(items) => {
                let mut next_opened = opened_groups;
                let mut resolved = Vec::with_capacity(items.len());
                for item in items {
                    let (item, opened_after_item) =
                        Self::resolve_relative_conditionals_inner(item, next_opened, total_groups)?;
                    next_opened = opened_after_item;
                    resolved.push(item);
                }
                Ok((RegexAst::Alternation(resolved), next_opened))
            }
            RegexAst::Quantified { expr, quantifier } => {
                let (expr, opened_after_expr) =
                    Self::resolve_relative_conditionals_inner(*expr, opened_groups, total_groups)?;
                Ok((
                    RegexAst::Quantified {
                        expr: Box::new(expr),
                        quantifier,
                    },
                    opened_after_expr,
                ))
            }
            RegexAst::Group {
                expr,
                kind,
                index,
                name,
            } => {
                let (expr, opened_after_expr) =
                    if matches!(kind, crate::ast::GroupKind::BranchReset) {
                        Self::resolve_relative_conditionals_branch_reset(
                            *expr,
                            opened_groups,
                            total_groups,
                        )?
                    } else {
                        let inner_opened = if matches!(kind, crate::ast::GroupKind::Capturing) {
                            index.unwrap_or_else(|| opened_groups.saturating_add(1))
                        } else {
                            opened_groups
                        };
                        Self::resolve_relative_conditionals_inner(
                            *expr,
                            inner_opened,
                            total_groups,
                        )?
                    };
                Ok((
                    RegexAst::Group {
                        expr: Box::new(expr),
                        kind,
                        index,
                        name,
                    },
                    opened_after_expr,
                ))
            }
            RegexAst::Lookahead { expr, positive } => {
                let (expr, opened_after_expr) =
                    Self::resolve_relative_conditionals_inner(*expr, opened_groups, total_groups)?;
                Ok((
                    RegexAst::Lookahead {
                        expr: Box::new(expr),
                        positive,
                    },
                    opened_after_expr,
                ))
            }
            RegexAst::Lookbehind { expr, positive } => {
                let (expr, opened_after_expr) =
                    Self::resolve_relative_conditionals_inner(*expr, opened_groups, total_groups)?;
                Ok((
                    RegexAst::Lookbehind {
                        expr: Box::new(expr),
                        positive,
                    },
                    opened_after_expr,
                ))
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let (condition, opened_after_condition) = Self::resolve_relative_conditional_test(
                    condition,
                    opened_groups,
                    total_groups,
                )?;
                let (true_branch, opened_after_true) = Self::resolve_relative_conditionals_inner(
                    *true_branch,
                    opened_after_condition,
                    total_groups,
                )?;
                let (false_branch, opened_after_false) = if let Some(false_branch) = false_branch {
                    let (false_branch, opened_after_false) =
                        Self::resolve_relative_conditionals_inner(
                            *false_branch,
                            opened_after_true,
                            total_groups,
                        )?;
                    (Some(Box::new(false_branch)), opened_after_false)
                } else {
                    (None, opened_after_true)
                };
                Ok((
                    RegexAst::Conditional {
                        condition,
                        true_branch: Box::new(true_branch),
                        false_branch,
                    },
                    opened_after_false,
                ))
            }
            RegexAst::Char(_)
            | RegexAst::CharClass(_)
            | RegexAst::Dot
            | RegexAst::Digit { .. }
            | RegexAst::Word { .. }
            | RegexAst::Space { .. }
            | RegexAst::UnicodeClass { .. }
            | RegexAst::ExtendedCharClass { .. }
            | RegexAst::Anchor(_)
            | RegexAst::WordBoundary { .. }
            | RegexAst::Backreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Empty => Ok((ast, opened_groups)),
        }
    }

    fn resolve_relative_conditionals_branch_reset(
        expr: RegexAst,
        opened_groups: u32,
        total_groups: u32,
    ) -> Result<(RegexAst, u32)> {
        match expr {
            RegexAst::Alternation(items) => {
                let mut resolved = Vec::with_capacity(items.len());
                let mut max_opened = opened_groups;
                for item in items {
                    let (item, opened_after_item) = Self::resolve_relative_conditionals_inner(
                        item,
                        opened_groups,
                        total_groups,
                    )?;
                    max_opened = max_opened.max(opened_after_item);
                    resolved.push(item);
                }
                Ok((RegexAst::Alternation(resolved), max_opened))
            }
            other => Self::resolve_relative_conditionals_inner(other, opened_groups, total_groups),
        }
    }

    fn parser_boundary_validation_message(ast: &RegexAst) -> Option<String> {
        match ast {
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => items
                .iter()
                .find_map(Self::parser_boundary_validation_message),
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => Self::parser_boundary_validation_message(expr),
            RegexAst::Group { expr, .. } => Self::parser_boundary_validation_message(expr),
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let condition_message = match condition {
                    crate::ast::ConditionalTest::Lookahead { expr, .. }
                    | crate::ast::ConditionalTest::Lookbehind { expr, .. } => {
                        Self::parser_boundary_validation_message(expr)
                    }
                    crate::ast::ConditionalTest::GroupExists(_)
                    | crate::ast::ConditionalTest::RelativeGroupExists(_)
                    | crate::ast::ConditionalTest::NamedGroupExists(_)
                    | crate::ast::ConditionalTest::RecursionAny
                    | crate::ast::ConditionalTest::RecursionGroup(_)
                    | crate::ast::ConditionalTest::RecursionNamed(_)
                    | crate::ast::ConditionalTest::Define => None,
                };
                condition_message
                    .or_else(|| Self::parser_boundary_validation_message(true_branch))
                    .or_else(|| {
                        false_branch
                            .as_ref()
                            .and_then(|branch| Self::parser_boundary_validation_message(branch))
                    })
            }
            RegexAst::Char(_)
            | RegexAst::CharClass(_)
            | RegexAst::Dot
            | RegexAst::Digit { .. }
            | RegexAst::Word { .. }
            | RegexAst::Space { .. }
            | RegexAst::UnicodeClass { .. }
            | RegexAst::ExtendedCharClass { .. }
            | RegexAst::Anchor(_)
            | RegexAst::WordBoundary { .. }
            | RegexAst::Backreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Empty => None,
        }
    }

    fn resolve_relative_conditional_test(
        condition: crate::ast::ConditionalTest,
        opened_groups: u32,
        total_groups: u32,
    ) -> Result<(crate::ast::ConditionalTest, u32)> {
        match condition {
            crate::ast::ConditionalTest::RelativeGroupExists(offset) => {
                let resolved =
                    Self::resolve_relative_group_reference(offset, opened_groups, total_groups)?;
                Ok((
                    crate::ast::ConditionalTest::GroupExists(resolved),
                    opened_groups,
                ))
            }
            crate::ast::ConditionalTest::Lookahead { expr, positive } => {
                let (expr, opened_after_expr) =
                    Self::resolve_relative_conditionals_inner(*expr, opened_groups, total_groups)?;
                Ok((
                    crate::ast::ConditionalTest::Lookahead {
                        expr: Box::new(expr),
                        positive,
                    },
                    opened_after_expr,
                ))
            }
            crate::ast::ConditionalTest::Lookbehind { expr, positive } => {
                let (expr, opened_after_expr) =
                    Self::resolve_relative_conditionals_inner(*expr, opened_groups, total_groups)?;
                Ok((
                    crate::ast::ConditionalTest::Lookbehind {
                        expr: Box::new(expr),
                        positive,
                    },
                    opened_after_expr,
                ))
            }
            crate::ast::ConditionalTest::GroupExists(group) => Ok((
                crate::ast::ConditionalTest::GroupExists(group),
                opened_groups,
            )),
            crate::ast::ConditionalTest::NamedGroupExists(name) => Ok((
                crate::ast::ConditionalTest::NamedGroupExists(name),
                opened_groups,
            )),
            crate::ast::ConditionalTest::RecursionAny => {
                Ok((crate::ast::ConditionalTest::RecursionAny, opened_groups))
            }
            crate::ast::ConditionalTest::RecursionGroup(group) => Ok((
                crate::ast::ConditionalTest::RecursionGroup(group),
                opened_groups,
            )),
            crate::ast::ConditionalTest::RecursionNamed(name) => Ok((
                crate::ast::ConditionalTest::RecursionNamed(name),
                opened_groups,
            )),
            crate::ast::ConditionalTest::Define => {
                Ok((crate::ast::ConditionalTest::Define, opened_groups))
            }
        }
    }

    fn resolve_recursion_conditionals(
        ast: RegexAst,
        total_groups: u32,
        named_groups: &std::collections::HashMap<String, u32>,
    ) -> Result<RegexAst> {
        match ast {
            RegexAst::Sequence(items) => Ok(RegexAst::Sequence(
                items
                    .into_iter()
                    .map(|item| {
                        Self::resolve_recursion_conditionals(item, total_groups, named_groups)
                    })
                    .collect::<Result<Vec<_>>>()?,
            )),
            RegexAst::Alternation(items) => Ok(RegexAst::Alternation(
                items
                    .into_iter()
                    .map(|item| {
                        Self::resolve_recursion_conditionals(item, total_groups, named_groups)
                    })
                    .collect::<Result<Vec<_>>>()?,
            )),
            RegexAst::Quantified { expr, quantifier } => Ok(RegexAst::Quantified {
                expr: Box::new(Self::resolve_recursion_conditionals(
                    *expr,
                    total_groups,
                    named_groups,
                )?),
                quantifier,
            }),
            RegexAst::Lookahead { expr, positive } => Ok(RegexAst::Lookahead {
                expr: Box::new(Self::resolve_recursion_conditionals(
                    *expr,
                    total_groups,
                    named_groups,
                )?),
                positive,
            }),
            RegexAst::Lookbehind { expr, positive } => Ok(RegexAst::Lookbehind {
                expr: Box::new(Self::resolve_recursion_conditionals(
                    *expr,
                    total_groups,
                    named_groups,
                )?),
                positive,
            }),
            RegexAst::Group {
                expr,
                kind,
                index,
                name,
            } => Ok(RegexAst::Group {
                expr: Box::new(Self::resolve_recursion_conditionals(
                    *expr,
                    total_groups,
                    named_groups,
                )?),
                kind,
                index,
                name,
            }),
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => Ok(RegexAst::Conditional {
                condition: Self::resolve_recursion_conditional_test(
                    condition,
                    total_groups,
                    named_groups,
                )?,
                true_branch: Box::new(Self::resolve_recursion_conditionals(
                    *true_branch,
                    total_groups,
                    named_groups,
                )?),
                false_branch: false_branch
                    .map(|branch| {
                        Self::resolve_recursion_conditionals(*branch, total_groups, named_groups)
                            .map(Box::new)
                    })
                    .transpose()?,
            }),
            other => Ok(other),
        }
    }

    fn resolve_recursion_conditional_test(
        condition: crate::ast::ConditionalTest,
        total_groups: u32,
        named_groups: &std::collections::HashMap<String, u32>,
    ) -> Result<crate::ast::ConditionalTest> {
        match condition {
            crate::ast::ConditionalTest::RecursionAny => {
                if named_groups.contains_key("R") {
                    Ok(crate::ast::ConditionalTest::NamedGroupExists(
                        "R".to_string(),
                    ))
                } else {
                    Ok(crate::ast::ConditionalTest::RecursionAny)
                }
            }
            crate::ast::ConditionalTest::RecursionGroup(group) => {
                let named_override = format!("R{group}");
                if named_groups.contains_key(&named_override) {
                    Ok(crate::ast::ConditionalTest::NamedGroupExists(
                        named_override,
                    ))
                } else if group == 0 || group > total_groups {
                    Err(RgxError::Compile(format!(
                        "conditional '(?(R{group})...)' refers to missing capture group"
                    )))
                } else {
                    Ok(crate::ast::ConditionalTest::RecursionGroup(group))
                }
            }
            crate::ast::ConditionalTest::RecursionNamed(name) => {
                let group = named_groups.get(&name).copied().ok_or_else(|| {
                    RgxError::Compile(format!(
                        "conditional '(?(R&{name})...)' refers to missing named capture group"
                    ))
                })?;
                Ok(crate::ast::ConditionalTest::RecursionGroup(group))
            }
            crate::ast::ConditionalTest::Lookahead { expr, positive } => {
                Ok(crate::ast::ConditionalTest::Lookahead {
                    expr: Box::new(Self::resolve_recursion_conditionals(
                        *expr,
                        total_groups,
                        named_groups,
                    )?),
                    positive,
                })
            }
            crate::ast::ConditionalTest::Lookbehind { expr, positive } => {
                Ok(crate::ast::ConditionalTest::Lookbehind {
                    expr: Box::new(Self::resolve_recursion_conditionals(
                        *expr,
                        total_groups,
                        named_groups,
                    )?),
                    positive,
                })
            }
            other => Ok(other),
        }
    }

    fn resolve_relative_group_reference(
        offset: i32,
        opened_groups: u32,
        total_groups: u32,
    ) -> Result<u32> {
        let missing_reference = || {
            RgxError::Compile(format!(
                "conditional '(?({offset:+})...)' refers to missing capture group"
            ))
        };

        if offset == 0 {
            return Err(missing_reference());
        }

        let resolved = if offset > 0 {
            opened_groups.checked_add(offset as u32)
        } else {
            let distance = offset.unsigned_abs();
            if distance > opened_groups {
                None
            } else {
                Some(opened_groups - distance + 1)
            }
        }
        .filter(|group| *group > 0 && *group <= total_groups)
        .ok_or_else(missing_reference)?;

        Ok(resolved)
    }

    fn feature_validation_message(
        &self,
        ast: &RegexAst,
        total_groups: u32,
        named_groups: &std::collections::HashMap<String, u32>,
    ) -> Option<String> {
        match ast {
            RegexAst::CodeBlock { lang, code } => self.code_block_validation_message(lang, code),
            RegexAst::UnicodeClass { name, negated } => {
                resolve_unicode_property_class(name, *negated).err()
            }
            RegexAst::ExtendedCharClass { .. } => {
                Some(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE.to_string())
            }
            RegexAst::CharClass(crate::ast::CharClass::UnicodeClass { name, negated }) => {
                resolve_unicode_property_class(name, *negated).err()
            }
            RegexAst::Recursion { target } => match target {
                crate::ast::RecursionTarget::Entire => None,
                crate::ast::RecursionTarget::Group(group) => {
                    if *group > total_groups {
                        Some(format!(
                            "recursive subroutine '(?{group})' refers to missing capture group"
                        ))
                    } else {
                        None
                    }
                }
                crate::ast::RecursionTarget::NamedGroup(name) => {
                    if named_groups.contains_key(name) {
                        None
                    } else {
                        Some(format!(
                            "recursive subroutine '(?&{name})' refers to missing named capture group"
                        ))
                    }
                }
            },
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => items
                .iter()
                .find_map(|item| self.feature_validation_message(item, total_groups, named_groups)),
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => {
                self.feature_validation_message(expr, total_groups, named_groups)
            }
            RegexAst::Group { expr, .. } => {
                self.feature_validation_message(expr, total_groups, named_groups)
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let condition_message = match condition {
                    crate::ast::ConditionalTest::GroupExists(group) => {
                        if *group > total_groups {
                            Some(format!(
                                "conditional '(?({group})...)' refers to missing capture group"
                            ))
                        } else {
                            None
                        }
                    }
                    crate::ast::ConditionalTest::NamedGroupExists(name) => {
                        if named_groups.contains_key(name) {
                            None
                        } else {
                            Some(format!(
                                "conditional '(?({name})...)' refers to missing named capture group"
                            ))
                        }
                    }
                    crate::ast::ConditionalTest::RecursionAny => None,
                    crate::ast::ConditionalTest::RecursionGroup(group) => {
                        if *group > total_groups {
                            Some(format!(
                                "conditional '(?(R{group})...)' refers to missing capture group"
                            ))
                        } else {
                            None
                        }
                    }
                    crate::ast::ConditionalTest::RecursionNamed(name) => {
                        if named_groups.contains_key(name) {
                            None
                        } else {
                            Some(format!(
                                "conditional '(?(R&{name})...)' refers to missing named capture group"
                            ))
                        }
                    }
                    crate::ast::ConditionalTest::Define => {
                        if false_branch.is_some() {
                            Some(
                                "conditional '(?(DEFINE)...)' does not support a false branch"
                                    .to_string(),
                            )
                        } else {
                            None
                        }
                    }
                    crate::ast::ConditionalTest::RelativeGroupExists(offset) => Some(format!(
                        "internal compiler error: unresolved relative conditional group reference '(?({offset:+})...)'"
                    )),
                    crate::ast::ConditionalTest::Lookahead { expr, .. }
                    | crate::ast::ConditionalTest::Lookbehind { expr, .. } => {
                        self.feature_validation_message(expr, total_groups, named_groups)
                    }
                };
                condition_message
                    .or_else(|| {
                        self.feature_validation_message(true_branch, total_groups, named_groups)
                    })
                    .or_else(|| {
                        false_branch.as_ref().and_then(|branch| {
                            self.feature_validation_message(branch, total_groups, named_groups)
                        })
                    })
            }
            _ => None,
        }
    }

    fn backreference_validation_message(ast: &RegexAst) -> Option<String> {
        let total_groups = Self::max_capture_group(ast);
        Self::backreference_validation_message_inner(ast, total_groups)
    }

    fn backreference_validation_message_inner(ast: &RegexAst, total_groups: u32) -> Option<String> {
        match ast {
            RegexAst::Backreference(group) if *group > total_groups => Some(format!(
                "backreference '\\{}' refers to missing capture group",
                group
            )),
            RegexAst::Backreference(_) => None,
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => items
                .iter()
                .find_map(|item| Self::backreference_validation_message_inner(item, total_groups)),
            RegexAst::Quantified { expr, .. }
            | RegexAst::Group { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => {
                Self::backreference_validation_message_inner(expr, total_groups)
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let condition_message = match condition {
                    crate::ast::ConditionalTest::Lookahead { expr, .. }
                    | crate::ast::ConditionalTest::Lookbehind { expr, .. } => {
                        Self::backreference_validation_message_inner(expr, total_groups)
                    }
                    crate::ast::ConditionalTest::GroupExists(_)
                    | crate::ast::ConditionalTest::RelativeGroupExists(_)
                    | crate::ast::ConditionalTest::NamedGroupExists(_)
                    | crate::ast::ConditionalTest::RecursionAny
                    | crate::ast::ConditionalTest::RecursionGroup(_)
                    | crate::ast::ConditionalTest::RecursionNamed(_)
                    | crate::ast::ConditionalTest::Define => None,
                };
                condition_message
                    .or_else(|| {
                        Self::backreference_validation_message_inner(true_branch, total_groups)
                    })
                    .or_else(|| {
                        false_branch.as_ref().and_then(|branch| {
                            Self::backreference_validation_message_inner(branch, total_groups)
                        })
                    })
            }
            RegexAst::Char(_)
            | RegexAst::CharClass(_)
            | RegexAst::Dot
            | RegexAst::Digit { .. }
            | RegexAst::Word { .. }
            | RegexAst::Space { .. }
            | RegexAst::UnicodeClass { .. }
            | RegexAst::ExtendedCharClass { .. }
            | RegexAst::Anchor(_)
            | RegexAst::WordBoundary { .. }
            | RegexAst::Recursion { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Empty => None,
        }
    }

    fn code_block_validation_message(&self, lang: &str, code: &str) -> Option<String> {
        if lang.len() > usize::from(u8::MAX) {
            return Some(
                "code-block language identifier exceeds VM operand size limits".to_string(),
            );
        }
        if code.len() > usize::from(u16::MAX) {
            return Some("code-block body exceeds VM operand size limits".to_string());
        }
        match self.mode {
            ExecutionMode::Pure => {
                Some("code blocks require ExecutionMode::Safe or ExecutionMode::Full".to_string())
            }
            ExecutionMode::Safe => match lang {
                "lua" => {
                    if cfg!(feature = "lua") {
                        None
                    } else {
                        Some("lua code blocks require the `lua` cargo feature".to_string())
                    }
                }
                "js" | "javascript" => {
                    if cfg!(feature = "javascript") {
                        None
                    } else {
                        Some(
                            "javascript code blocks require the `javascript` cargo feature"
                                .to_string(),
                        )
                    }
                }
                "rhai" => {
                    if cfg!(feature = "rhai") {
                        None
                    } else {
                        Some("rhai code blocks require the `rhai` cargo feature".to_string())
                    }
                }
                "wasm" => {
                    if cfg!(feature = "wasm") {
                        None
                    } else {
                        Some("wasm code blocks require the `wasm` cargo feature".to_string())
                    }
                }
                "native" => Some("native code blocks require ExecutionMode::Full".to_string()),
                other => Some(format!("unsupported code-block language: {other}")),
            },
            ExecutionMode::Full => match lang {
                "lua" => {
                    if cfg!(feature = "lua") {
                        None
                    } else {
                        Some("lua code blocks require the `lua` cargo feature".to_string())
                    }
                }
                "js" | "javascript" => {
                    if cfg!(feature = "javascript") {
                        None
                    } else {
                        Some(
                            "javascript code blocks require the `javascript` cargo feature"
                                .to_string(),
                        )
                    }
                }
                "rhai" => {
                    if cfg!(feature = "rhai") {
                        None
                    } else {
                        Some("rhai code blocks require the `rhai` cargo feature".to_string())
                    }
                }
                "wasm" => {
                    if cfg!(feature = "wasm") {
                        None
                    } else {
                        Some("wasm code blocks require the `wasm` cargo feature".to_string())
                    }
                }
                "native" => None,
                other => Some(format!("unsupported code-block language: {other}")),
            },
        }
    }

    fn max_capture_group(ast: &RegexAst) -> u32 {
        match ast {
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => {
                items.iter().map(Self::max_capture_group).max().unwrap_or(0)
            }
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => Self::max_capture_group(expr),
            RegexAst::Group {
                expr, kind, index, ..
            } => {
                let current = if matches!(kind, crate::ast::GroupKind::Capturing) {
                    index.unwrap_or(0)
                } else {
                    0
                };
                current.max(Self::max_capture_group(expr))
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let condition_max = match condition {
                    crate::ast::ConditionalTest::Lookahead { expr, .. }
                    | crate::ast::ConditionalTest::Lookbehind { expr, .. } => {
                        Self::max_capture_group(expr)
                    }
                    crate::ast::ConditionalTest::GroupExists(_)
                    | crate::ast::ConditionalTest::RelativeGroupExists(_)
                    | crate::ast::ConditionalTest::NamedGroupExists(_)
                    | crate::ast::ConditionalTest::RecursionAny
                    | crate::ast::ConditionalTest::RecursionGroup(_)
                    | crate::ast::ConditionalTest::RecursionNamed(_)
                    | crate::ast::ConditionalTest::Define => 0,
                };
                let true_max = Self::max_capture_group(true_branch);
                let false_max = false_branch
                    .as_ref()
                    .map_or(0, |branch| Self::max_capture_group(branch));
                condition_max.max(true_max).max(false_max)
            }
            RegexAst::Char(_)
            | RegexAst::CharClass(_)
            | RegexAst::Dot
            | RegexAst::Digit { .. }
            | RegexAst::Word { .. }
            | RegexAst::Space { .. }
            | RegexAst::UnicodeClass { .. }
            | RegexAst::ExtendedCharClass { .. }
            | RegexAst::Anchor(_)
            | RegexAst::WordBoundary { .. }
            | RegexAst::Backreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Empty => 0,
        }
    }

    fn collect_named_groups(ast: &RegexAst) -> std::collections::HashMap<String, u32> {
        let mut named_groups = std::collections::HashMap::new();
        Self::collect_named_groups_inner(ast, &mut named_groups);
        named_groups
    }

    fn collect_named_groups_inner(
        ast: &RegexAst,
        named_groups: &mut std::collections::HashMap<String, u32>,
    ) {
        match ast {
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => {
                for item in items {
                    Self::collect_named_groups_inner(item, named_groups);
                }
            }
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => {
                Self::collect_named_groups_inner(expr, named_groups);
            }
            RegexAst::Group {
                expr,
                kind,
                index,
                name,
            } => {
                if matches!(kind, crate::ast::GroupKind::Capturing) {
                    if let (Some(name), Some(group_id)) = (name, *index) {
                        named_groups.insert(name.clone(), group_id);
                    }
                }
                Self::collect_named_groups_inner(expr, named_groups);
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                match condition {
                    crate::ast::ConditionalTest::Lookahead { expr, .. }
                    | crate::ast::ConditionalTest::Lookbehind { expr, .. } => {
                        Self::collect_named_groups_inner(expr, named_groups);
                    }
                    crate::ast::ConditionalTest::GroupExists(_)
                    | crate::ast::ConditionalTest::RelativeGroupExists(_)
                    | crate::ast::ConditionalTest::NamedGroupExists(_)
                    | crate::ast::ConditionalTest::RecursionAny
                    | crate::ast::ConditionalTest::RecursionGroup(_)
                    | crate::ast::ConditionalTest::RecursionNamed(_)
                    | crate::ast::ConditionalTest::Define => {}
                }
                Self::collect_named_groups_inner(true_branch, named_groups);
                if let Some(false_branch) = false_branch {
                    Self::collect_named_groups_inner(false_branch, named_groups);
                }
            }
            RegexAst::Char(_)
            | RegexAst::CharClass(_)
            | RegexAst::Dot
            | RegexAst::Digit { .. }
            | RegexAst::Word { .. }
            | RegexAst::Space { .. }
            | RegexAst::UnicodeClass { .. }
            | RegexAst::ExtendedCharClass { .. }
            | RegexAst::Anchor(_)
            | RegexAst::WordBoundary { .. }
            | RegexAst::Backreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Empty => {}
        }
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{CharClass, CharRange};

    fn range_contains(ranges: &[CharRange], target: char) -> bool {
        ranges
            .iter()
            .any(|range| target >= range.start && target <= range.end)
    }

    #[test]
    fn extract_simple_extended_char_class_body_accepts_single_nested_class() {
        assert_eq!(
            Compiler::extract_simple_extended_char_class_body("[a-z]"),
            Some("a-z")
        );
        assert_eq!(
            Compiler::extract_simple_extended_char_class_body("[^0-9]"),
            Some("^0-9")
        );
    }

    #[test]
    fn extract_simple_extended_char_class_body_rejects_trailing_content() {
        assert_eq!(
            Compiler::extract_simple_extended_char_class_body("[a-z][0-9]"),
            None
        );
        assert_eq!(
            Compiler::extract_simple_extended_char_class_body("a-z]"),
            None
        );
    }

    #[test]
    fn lower_extended_char_class_content_maps_simple_range_to_char_class() {
        let lowered = Compiler::lower_extended_char_class_content("[a-z]".to_string())
            .expect("Expected simple nested range to lower into a plain char class");

        assert_eq!(
            lowered,
            RegexAst::CharClass(CharClass::Custom {
                ranges: vec![CharRange::range('a', 'z')],
                negated: false,
            })
        );
    }

    #[test]
    fn lower_extended_char_class_content_maps_simple_negation_to_char_class() {
        let lowered = Compiler::lower_extended_char_class_content("[^0-9]".to_string())
            .expect("Expected simple nested negated range to lower into a plain char class");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(
            !negated,
            "negated subset should lower into explicit complement ranges"
        );
        assert!(range_contains(&ranges, 'a'));
        assert!(range_contains(&ranges, '!'));
        assert!(!range_contains(&ranges, '5'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_single_difference_operator() {
        let lowered = Compiler::lower_extended_char_class_content("[a-z] - [aeiou]".to_string())
            .expect("Expected single-operator difference to lower into a plain char class");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'b'));
        assert!(range_contains(&ranges, 'z'));
        assert!(!range_contains(&ranges, 'a'));
        assert!(!range_contains(&ranges, 'e'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_property_intersection() {
        let lowered = Compiler::lower_extended_char_class_content(r"\p{L} & \p{Lu}".to_string())
            .expect("Expected property intersection to lower into a plain char class");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'A'));
        assert!(range_contains(&ranges, 'Z'));
        assert!(!range_contains(&ranges, 'a'));
        assert!(!range_contains(&ranges, '7'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_unary_complement() {
        let lowered = Compiler::lower_extended_char_class_content("![0-9]".to_string())
            .expect("Expected unary complement to lower into a plain char class");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'a'));
        assert!(range_contains(&ranges, '!'));
        assert!(!range_contains(&ranges, '5'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_grouped_algebra() {
        let lowered =
            Compiler::lower_extended_char_class_content("([a-z] - [aeiou]) & [b-d]".to_string())
                .expect("Expected grouped algebra to lower into a plain char class");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'b'));
        assert!(range_contains(&ranges, 'd'));
        assert!(!range_contains(&ranges, 'a'));
        assert!(!range_contains(&ranges, 'e'));
        assert!(!range_contains(&ranges, 'f'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_symmetric_difference() {
        let lowered = Compiler::lower_extended_char_class_content("[AC] ^ [BC]".to_string())
            .expect("Expected symmetric difference to lower into a plain char class");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'A'));
        assert!(range_contains(&ranges, 'B'));
        assert!(!range_contains(&ranges, 'C'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_same_level_multi_operator_precedence() {
        let lowered =
            Compiler::lower_extended_char_class_content("[a-f] | [d-z] & [m-p]".to_string())
                .expect("Expected same-level multi-operator form to lower with '&' precedence");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'a'));
        assert!(range_contains(&ranges, 'f'));
        assert!(range_contains(&ranges, 'm'));
        assert!(range_contains(&ranges, 'p'));
        assert!(!range_contains(&ranges, 'g'));
        assert!(!range_contains(&ranges, 'z'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_left_associative_low_precedence_chain() {
        let lowered = Compiler::lower_extended_char_class_content(
            "[a-z] - [aeiou] + [0-9] - [5]".to_string(),
        )
        .expect("Expected chained low-precedence operators to lower left-associatively");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'b'));
        assert!(range_contains(&ranges, 'z'));
        assert!(range_contains(&ranges, '0'));
        assert!(range_contains(&ranges, '9'));
        assert!(!range_contains(&ranges, 'a'));
        assert!(!range_contains(&ranges, 'e'));
        assert!(!range_contains(&ranges, '5'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_left_associative_intersection_chain() {
        let lowered =
            Compiler::lower_extended_char_class_content("[a-z] & [d-z] & [m-p]".to_string())
                .expect("Expected chained intersections to lower left-associatively");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'm'));
        assert!(range_contains(&ranges, 'p'));
        assert!(!range_contains(&ranges, 'd'));
        assert!(!range_contains(&ranges, 'z'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_digit_shorthand_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\d - [3]".to_string())
            .expect("Expected bare digit shorthand set term to lower into the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '0'));
        assert!(range_contains(&ranges, '9'));
        assert!(!range_contains(&ranges, '3'));
        assert!(!range_contains(&ranges, 'a'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_negated_bare_shorthand_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\D & [A-F]".to_string())
            .expect("Expected negated bare shorthand set term to lower into the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'A'));
        assert!(range_contains(&ranges, 'F'));
        assert!(!range_contains(&ranges, '3'));
        assert!(!range_contains(&ranges, 'Z'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_hex_escape_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\x{41} - [B]".to_string())
            .expect("Expected bare hex-escape set term to lower into the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'A'));
        assert!(!range_contains(&ranges, 'B'));
        assert!(!range_contains(&ranges, 'C'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_control_escape_terms() {
        let lowered = Compiler::lower_extended_char_class_content(r"\n | \t".to_string())
            .expect("Expected bare control-escape set terms to lower into the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '\n'));
        assert!(range_contains(&ranges, '\t'));
        assert!(!range_contains(&ranges, ' '));
        assert!(!range_contains(&ranges, 'A'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_escaped_operator_literal_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\- | [A]".to_string())
            .expect("Expected escaped operator literal term to remain part of the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '-'));
        assert!(range_contains(&ranges, 'A'));
        assert!(!range_contains(&ranges, 'B'));
    }

    #[test]
    fn lower_extended_char_class_content_rejects_unclosed_hex_escape() {
        let err = Compiler::lower_extended_char_class_content(r"\x{41".to_string())
            .expect_err("Expected malformed hex escape to stay behind the explicit boundary");
        assert!(
            err.to_string().contains(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE),
            "unexpected malformed-escape boundary message: {err}"
        );
    }

    #[test]
    fn scalar_range_set_normalizes_and_merges_adjacent_ranges() {
        let set = ScalarRangeSet::new(vec![
            ('d' as u32, 'f' as u32),
            ('a' as u32, 'c' as u32),
            ('g' as u32, 'g' as u32),
        ]);

        assert_eq!(set.ranges, vec![('a' as u32, 'g' as u32)]);
    }

    #[test]
    fn scalar_range_set_difference_splits_overlapping_ranges() {
        let lhs = ScalarRangeSet::new(vec![('a' as u32, 'z' as u32)]);
        let rhs = ScalarRangeSet::new(vec![('d' as u32, 'f' as u32), ('m' as u32, 'p' as u32)]);

        let difference = lhs.difference(&rhs);

        assert_eq!(
            difference.ranges,
            vec![
                ('a' as u32, 'c' as u32),
                ('g' as u32, 'l' as u32),
                ('q' as u32, 'z' as u32),
            ]
        );
    }
}
