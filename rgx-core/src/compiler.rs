use crate::ast::Regex as RegexAst;
use crate::ast::{CharClass, CharRange};
use crate::c2;
use crate::engine::ExecutionMode;
use crate::error::{Result, RgxError};
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

const UNICODE_SCALAR_UNIVERSE: [ScalarRange; 2] = [(0x0000, 0xD7FF), (0xE000, 0x0010_FFFF)];
const ASCII_DIGIT_RANGES: [ScalarRange; 1] = [('0' as u32, '9' as u32)];
const ASCII_WORD_RANGES: [ScalarRange; 4] = [
    ('0' as u32, '9' as u32),
    ('A' as u32, 'Z' as u32),
    ('_' as u32, '_' as u32),
    ('a' as u32, 'z' as u32),
];
const ASCII_ALPHA_RANGES: [ScalarRange; 2] = [('A' as u32, 'Z' as u32), ('a' as u32, 'z' as u32)];
const ASCII_ALNUM_RANGES: [ScalarRange; 3] = [
    ('0' as u32, '9' as u32),
    ('A' as u32, 'Z' as u32),
    ('a' as u32, 'z' as u32),
];
const ASCII_BLANK_RANGES: [ScalarRange; 2] = [(0x09, 0x09), (0x20, 0x20)];
const ASCII_CNTRL_RANGES: [ScalarRange; 2] = [(0x00, 0x1F), (0x7F, 0x7F)];
const ASCII_GRAPH_RANGES: [ScalarRange; 1] = [(0x21, 0x7E)];
const ASCII_LOWER_RANGES: [ScalarRange; 1] = [('a' as u32, 'z' as u32)];
const ASCII_PRINT_RANGES: [ScalarRange; 1] = [(0x20, 0x7E)];
const ASCII_PUNCT_RANGES: [ScalarRange; 4] =
    [(0x21, 0x2F), (0x3A, 0x40), (0x5B, 0x60), (0x7B, 0x7E)];
const ASCII_SPACE_RANGES: [ScalarRange; 2] = [(0x09, 0x0D), (' ' as u32, ' ' as u32)];
const ASCII_UPPER_RANGES: [ScalarRange; 1] = [('A' as u32, 'Z' as u32)];
const ASCII_XDIGIT_RANGES: [ScalarRange; 3] = [
    ('0' as u32, '9' as u32),
    ('A' as u32, 'F' as u32),
    ('a' as u32, 'f' as u32),
];
const ASCII_ASCII_RANGES: [ScalarRange; 1] = [(0x00, 0x7F)];
const PCRE_HORIZONTAL_SPACE_RANGES: [ScalarRange; 9] = [
    (0x09, 0x09),
    (0x20, 0x20),
    (0xA0, 0xA0),
    (0x1680, 0x1680),
    (0x180E, 0x180E),
    (0x2000, 0x200A),
    (0x202F, 0x202F),
    (0x205F, 0x205F),
    (0x3000, 0x3000),
];
const PCRE_VERTICAL_SPACE_RANGES: [ScalarRange; 3] = [(0x0A, 0x0D), (0x85, 0x85), (0x2028, 0x2029)];

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

    fn apply(self, lhs: &ScalarRangeSet, rhs: &ScalarRangeSet) -> ScalarRangeSet {
        match self {
            Self::Union => lhs.union(rhs),
            Self::Difference => lhs.difference(rhs),
            Self::Intersection => lhs.intersection(rhs),
            Self::SymmetricDifference => lhs.difference(rhs).union(&rhs.difference(lhs)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AsciiPosixClass {
    Alnum,
    Alpha,
    Ascii,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Word,
    Xdigit,
}

impl AsciiPosixClass {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "alnum" => Some(Self::Alnum),
            "alpha" => Some(Self::Alpha),
            "ascii" => Some(Self::Ascii),
            "blank" => Some(Self::Blank),
            "cntrl" => Some(Self::Cntrl),
            "digit" => Some(Self::Digit),
            "graph" => Some(Self::Graph),
            "lower" => Some(Self::Lower),
            "print" => Some(Self::Print),
            "punct" => Some(Self::Punct),
            "space" => Some(Self::Space),
            "upper" => Some(Self::Upper),
            "word" => Some(Self::Word),
            "xdigit" => Some(Self::Xdigit),
            _ => None,
        }
    }

    fn ranges(self) -> &'static [ScalarRange] {
        match self {
            Self::Alnum => &ASCII_ALNUM_RANGES,
            Self::Alpha => &ASCII_ALPHA_RANGES,
            Self::Ascii => &ASCII_ASCII_RANGES,
            Self::Blank => &ASCII_BLANK_RANGES,
            Self::Cntrl => &ASCII_CNTRL_RANGES,
            Self::Digit => &ASCII_DIGIT_RANGES,
            Self::Graph => &ASCII_GRAPH_RANGES,
            Self::Lower => &ASCII_LOWER_RANGES,
            Self::Print => &ASCII_PRINT_RANGES,
            Self::Punct => &ASCII_PUNCT_RANGES,
            Self::Space => &ASCII_SPACE_RANGES,
            Self::Upper => &ASCII_UPPER_RANGES,
            Self::Word => &ASCII_WORD_RANGES,
            Self::Xdigit => &ASCII_XDIGIT_RANGES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExtendedPosixClassSpec {
    class: AsciiPosixClass,
    negated: bool,
}

#[derive(Clone)]
struct ExtendedNestedCharClassAtom {
    set: ScalarRangeSet,
    scalar: Option<u32>,
}

impl ExtendedNestedCharClassAtom {
    fn from_char(ch: char) -> Self {
        Self {
            set: ScalarRangeSet::from_char(ch),
            scalar: Some(ch as u32),
        }
    }

    fn from_set(set: ScalarRangeSet) -> Self {
        let scalar = match set.ranges.as_slice() {
            [(start, end)] if start == end => Some(*start),
            _ => None,
        };

        Self { set, scalar }
    }
}

#[derive(Clone, Copy)]
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

pub(crate) const EXTENDED_CHAR_CLASS_SUBSET_MESSAGE: &str = "Perl extended character classes '(?[...])' currently support bracket/property terms, bare POSIX class terms in current ASCII forms such as '[:alpha:]' and '[:graph:]', nested ordinary bracket terms that use the current ordinary char-class atom subset (for example '[\\dA-F]', '[[:graph:]]', or '[\\p{L}]'), bare shorthand terms ('\\d', '\\D', '\\w', '\\W', '\\s', '\\S', '\\h', '\\H', '\\v', '\\V'), bare escaped single-character/codepoint terms such as '\\a', '\\b', '\\e', '\\f', '\\n', '\\t', '\\r', '\\cA', '\\040', '\\o{101}', '\\x{41}', and '\\-', unary complement ('!'), grouped subexpressions, and left-associative set algebra with '&' binding tighter than '|', '+', '-', and '^' in rgx, such as '(?[ [:graph:] ])', '(?[[\\dA-F]])', '(?[[\\p{L}] - [\\p{Lu}]])', '(?[ \\a | \\b | \\e | \\f ])', '(?[ \\040 | \\011 ])', '(?[ \\cA | [B] ])', or '(?[ [a-z] - [aeiou] + [0-9] - [5] ])'; wider set-expression forms and additional bare-term families beyond the current bracket/property/nested-ordinary/POSIX/shorthand/escaped-term subset remain unsupported";

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
        let ranges = resolve_unicode_property_class(name, negated).map_err(RgxError::compile)?;
        Ok(Self::from_char_ranges(&ranges))
    }

    fn from_builtin_ranges(ranges: &[ScalarRange], negated: bool) -> Self {
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
                Ok(Self::from_builtin_ranges(&ASCII_DIGIT_RANGES, *negated))
            }
            crate::ast::CharClass::Word { negated } => {
                Ok(Self::from_builtin_ranges(&ASCII_WORD_RANGES, *negated))
            }
            crate::ast::CharClass::Space { negated } => {
                Ok(Self::from_builtin_ranges(&ASCII_SPACE_RANGES, *negated))
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
    #[must_use]
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
    #[must_use]
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
    ///
    /// # Errors
    /// Returns `RgxError::Compile` if the pattern is empty or contains invalid syntax.
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

        trace_decision!(
            "compiler",
            "pattern.is_empty()",
            pattern.is_empty(),
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
    ///
    /// # Errors
    /// Returns `RgxError::Compile` if bytecode generation fails for the given AST.
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
        let ast = Self::lower_flag_toggles(ast);
        let ast = Self::strip_extended_mode(ast);
        let ast = Self::lower_extended_char_classes(ast)?;
        debug_log!("compiler", "AST: {:?}", ast);
        if let Some(msg) = Self::parser_boundary_validation_message(&ast) {
            trace_exit!(
                "compiler",
                "Compiler::compile_ast_with_label",
                "error={}",
                msg
            );
            return Err(RgxError::compile(msg));
        }
        let total_groups = Self::max_capture_group(&ast);
        let named_groups = Self::collect_named_groups(&ast);
        let ast = Self::resolve_relative_conditionals(ast, total_groups)?;
        let ast = Self::resolve_recursion_conditionals(ast, total_groups, &named_groups)?;
        let ast = Self::resolve_octal_backreferences(ast, total_groups);

        if let Some(msg) = Self::backreference_validation_message(&ast) {
            trace_exit!(
                "compiler",
                "Compiler::compile_ast_with_label",
                "error={}",
                msg
            );
            return Err(RgxError::compile(msg));
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
            return Err(RgxError::compile(msg.to_string()));
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

        // C2 step 1: classify the AST against the no-backtracking subset.
        // See `docs/C2_NFA_DFA_DESIGN.md` §4 and `c2::classifier`.
        program.classification = c2::classify(&ast);
        debug_log!(
            "compiler",
            "  - C2 classification: {:?}",
            program.classification
        );

        // C2 step 4c: build the C2 program for dispatch when the pattern
        // is classifier-positive AND structurally eligible for Pike-VM
        // dispatch (see `c2::program::is_c2_dispatch_eligible`). Stored
        // on `Program.c2_program`; the public `Regex` API methods read
        // this field to route `is_match` / `find_first` / `find_all`
        // through the Pike-VM when present.
        program.c2_program = if matches!(program.classification, c2::Classification::NoBacktracking)
            && c2::program::is_c2_dispatch_eligible(&ast)
        {
            Some(c2::CompiledC2Program::build_from_ast(&ast))
        } else {
            None
        };
        debug_log!(
            "compiler",
            "  - C2 dispatch eligible: {}",
            program.c2_program.is_some()
        );

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

    /// PCRE2 octal-or-literal fallback for numeric backreferences
    /// that don't name an existing group. `\NNN` is parsed by PGEN
    /// as `Backreference(NNN)` because the grammar can't know the
    /// capture count upfront.
    ///
    /// Per pcre2pattern(3) "Non-printing characters" and "Back
    /// references": if the decimal number N is < 10 **or** there
    /// are at least N capturing groups before this escape, the
    /// whole sequence is a back reference. Otherwise the escape
    /// is **up to three leading octal digits** (0..=7) followed
    /// by any remaining decimal digits as literal characters.
    ///
    /// Examples:
    /// - `(abc)\123` (group 123 missing, all digits octal) →
    ///   byte 0o123 = 0x53 = 'S'.
    /// - `(abc)\223` / `(abc)\323` → 0o223 / 0o323.
    /// - `\214748364` (no groups, 9 digits, first three octal)
    ///   → Char(0o214) = U+008C followed by literal "748364".
    /// - `\89` (no groups, no octal-leading digit) → literal "89".
    /// - `\199` (no groups, one leading octal digit) → Char(0o1)
    ///   followed by literal "99".
    ///
    /// Single-digit `\8` / `\9` with no matching group stays a
    /// `Backreference(n)` and errors at
    /// `backreference_validation_message` — PCRE2's "N < 10 is
    /// always a back reference" rule fires for lone 8 / 9 (we
    /// cannot take 0 octal digits and emit literal "8" / "9" the
    /// way we do for multi-digit forms, because that would
    /// silently change a likely-invalid pattern into a literal
    /// and hide the bug).
    ///
    /// `Backreference(0)` is handled upstream by
    /// `convert_simple_escape` (which routes `\0` directly to
    /// `Char('\0')`); it doesn't reach this transform.
    fn resolve_octal_backreferences(ast: RegexAst, total_groups: u32) -> RegexAst {
        match ast {
            RegexAst::Backreference(n) if n > total_groups => {
                let s = n.to_string();
                let bytes = s.as_bytes();

                // Single-digit 8 / 9 without a matching group keeps
                // the Backreference(n) shape so the validator can
                // surface a clean compile error. Multi-digit cases
                // fall through to the octal-then-literal rule below.
                if bytes.len() == 1 && bytes[0] >= b'8' {
                    return RegexAst::Backreference(n);
                }

                // Count leading octal digits (0..=7), at most 3.
                let mut octal_count = 0;
                while octal_count < 3 && octal_count < bytes.len() && bytes[octal_count] < b'8' {
                    octal_count += 1;
                }

                let mut items: Vec<RegexAst> = Vec::new();

                if octal_count > 0 {
                    // Safe because every byte in `bytes` is an ASCII
                    // digit by construction (`n.to_string()`).
                    let octal_str = std::str::from_utf8(&bytes[..octal_count])
                        .expect("decimal digits are ASCII");
                    let Ok(code) = u32::from_str_radix(octal_str, 8) else {
                        return RegexAst::Backreference(n);
                    };
                    // PCRE2 accepts octal escapes up to 0o777 = 0xFF
                    // (one byte). For three-digit 0o400..=0o777
                    // (128..=255), Rust's `char::from_u32` accepts
                    // 128..=255 as a Unicode codepoint which encodes
                    // to TWO UTF-8 bytes — diverges from PCRE2's
                    // single-byte literal semantics. For the initial
                    // fallback we surface the Unicode codepoint;
                    // byte-accurate matching for 128..=255 is
                    // follow-up work (BACKLOG C7).
                    match char::from_u32(code) {
                        Some(ch) => items.push(RegexAst::Char(ch)),
                        None => return RegexAst::Backreference(n),
                    }
                }

                // Remaining decimal digits are literal characters.
                for &b in &bytes[octal_count..] {
                    items.push(RegexAst::Char(b as char));
                }

                match items.len() {
                    0 => RegexAst::Backreference(n),
                    1 => items.into_iter().next().expect("just checked len==1"),
                    _ => RegexAst::Sequence(items),
                }
            }
            RegexAst::Sequence(items) => RegexAst::Sequence(
                items
                    .into_iter()
                    .map(|item| Self::resolve_octal_backreferences(item, total_groups))
                    .collect(),
            ),
            RegexAst::Alternation(items) => RegexAst::Alternation(
                items
                    .into_iter()
                    .map(|item| Self::resolve_octal_backreferences(item, total_groups))
                    .collect(),
            ),
            RegexAst::Quantified { expr, quantifier } => RegexAst::Quantified {
                expr: Box::new(Self::resolve_octal_backreferences(*expr, total_groups)),
                quantifier,
            },
            RegexAst::Group {
                expr,
                kind,
                index,
                name,
            } => RegexAst::Group {
                expr: Box::new(Self::resolve_octal_backreferences(*expr, total_groups)),
                kind,
                index,
                name,
            },
            RegexAst::Lookahead { expr, positive } => RegexAst::Lookahead {
                expr: Box::new(Self::resolve_octal_backreferences(*expr, total_groups)),
                positive,
            },
            RegexAst::Lookbehind { expr, positive } => RegexAst::Lookbehind {
                expr: Box::new(Self::resolve_octal_backreferences(*expr, total_groups)),
                positive,
            },
            RegexAst::FlagGroup { flags, expr } => RegexAst::FlagGroup {
                flags,
                expr: Box::new(Self::resolve_octal_backreferences(*expr, total_groups)),
            },
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => RegexAst::Conditional {
                condition,
                true_branch: Box::new(Self::resolve_octal_backreferences(
                    *true_branch,
                    total_groups,
                )),
                false_branch: false_branch
                    .map(|b| Box::new(Self::resolve_octal_backreferences(*b, total_groups))),
            },
            other => other,
        }
    }

    fn extended_char_class_subset_error() -> RgxError {
        RgxError::compile(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE.to_string())
    }

    /// Rewrite non-scoped flag toggles so that `FlagGroup { expr: Empty }` in a
    /// sequence absorbs the remaining siblings.
    ///
    /// For example, `Sequence([FlagGroup("i", Empty), Char('a'), Char('b')])`
    /// becomes `Sequence([FlagGroup("i", Sequence([Char('a'), Char('b')]))])`,
    /// which collapses to `FlagGroup("i", Sequence([Char('a'), Char('b')]))`.
    ///
    /// Unscoped toggles also cross alternation branch boundaries per PCRE2:
    /// `(a(?i)bc|BB)x` on "bbx" matches because `(?i)` in branch 1 extends to
    /// the end of the enclosing group and therefore applies to branch 2's
    /// `BB` as well. We detect per-branch unscoped toggles *pre-recursion*
    /// (an `FG(_, Empty)` marker emitted by `convert_inline_modifiers` —
    /// distinguishable from `convert_scoped_inline_modifiers`'s
    /// `FG(_, pattern)` because scoped forms always carry a body) and wrap
    /// subsequent branches in the carried `FlagGroup` before recursing.
    fn lower_flag_toggles(ast: RegexAst) -> RegexAst {
        // First, recurse into children.
        let ast = match ast {
            RegexAst::Sequence(items) => {
                RegexAst::Sequence(items.into_iter().map(Self::lower_flag_toggles).collect())
            }
            RegexAst::Alternation(items) => {
                // PCRE2: an unscoped `(?flags)` toggle at branch K's
                // position extends its effect to branches K+1..N of the
                // same alternation. Walk branches in order, carrying
                // the latest trailing unscoped flag forward.
                let mut carried: Option<String> = None;
                let mut new_branches: Vec<RegexAst> = Vec::with_capacity(items.len());
                for branch in items {
                    // Detect this branch's trailing unscoped toggle
                    // BEFORE lowering — `FG(_, Empty)` is the raw
                    // marker; after lowering the FG will have absorbed
                    // its branch-local siblings and can no longer be
                    // told apart from `(?flags:body)`.
                    let next_carry = Self::unscoped_trailing_flag(&branch);
                    let lowered = Self::lower_flag_toggles(branch);
                    let wrapped = if let Some(ref flags) = carried {
                        RegexAst::FlagGroup {
                            flags: flags.clone(),
                            expr: Box::new(lowered),
                        }
                    } else {
                        lowered
                    };
                    new_branches.push(wrapped);
                    if let Some(flags) = next_carry {
                        // Simple last-wins combine. Multi-flag
                        // interactions across branches (e.g. `(?i)|(?m)`
                        // where branch 3 should see both) fall through
                        // this heuristic and can be refined later if
                        // conformance evidence demands it.
                        carried = Some(flags);
                    }
                }
                RegexAst::Alternation(new_branches)
            }
            RegexAst::Quantified { expr, quantifier } => RegexAst::Quantified {
                expr: Box::new(Self::lower_flag_toggles(*expr)),
                quantifier,
            },
            RegexAst::Group {
                expr,
                kind,
                index,
                name,
            } => RegexAst::Group {
                expr: Box::new(Self::lower_flag_toggles(*expr)),
                kind,
                index,
                name,
            },
            RegexAst::Lookahead { expr, positive } => RegexAst::Lookahead {
                expr: Box::new(Self::lower_flag_toggles(*expr)),
                positive,
            },
            RegexAst::Lookbehind { expr, positive } => RegexAst::Lookbehind {
                expr: Box::new(Self::lower_flag_toggles(*expr)),
                positive,
            },
            RegexAst::FlagGroup { flags, expr } => RegexAst::FlagGroup {
                flags,
                expr: Box::new(Self::lower_flag_toggles(*expr)),
            },
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => RegexAst::Conditional {
                condition,
                true_branch: Box::new(Self::lower_flag_toggles(*true_branch)),
                false_branch: false_branch.map(|fb| Box::new(Self::lower_flag_toggles(*fb))),
            },
            other => other,
        };

        // Now handle the rewrite: scan sequences for FlagGroup { expr: Empty }.
        match ast {
            RegexAst::Sequence(items) => {
                // Check if any element is a FlagGroup with Empty body.
                let has_toggle = items.iter().any(|item| {
                    matches!(item, RegexAst::FlagGroup { expr, .. } if matches!(expr.as_ref(), RegexAst::Empty))
                });
                if !has_toggle {
                    return match items.len() {
                        0 => RegexAst::Empty,
                        1 => items.into_iter().next().unwrap(),
                        _ => RegexAst::Sequence(items),
                    };
                }
                // Rewrite: when we find a FlagGroup { flags, Empty }, absorb
                // all subsequent siblings into its body.
                let mut result = Vec::new();
                let mut iter = items.into_iter().peekable();
                while let Some(item) = iter.next() {
                    if let RegexAst::FlagGroup { flags, expr } = &item {
                        if matches!(expr.as_ref(), RegexAst::Empty) {
                            let rest: Vec<RegexAst> = iter.collect();
                            let body = match rest.len() {
                                0 => RegexAst::Empty,
                                1 => rest.into_iter().next().unwrap(),
                                _ => RegexAst::Sequence(rest),
                            };
                            result.push(RegexAst::FlagGroup {
                                flags: flags.clone(),
                                expr: Box::new(body),
                            });
                            break;
                        }
                    }
                    result.push(item);
                }
                match result.len() {
                    0 => RegexAst::Empty,
                    1 => result.into_iter().next().unwrap(),
                    _ => RegexAst::Sequence(result),
                }
            }
            other => other,
        }
    }

    /// Scan a branch (pre-lowering) for a top-level unscoped flag toggle —
    /// `FG(flags, Empty)`, the marker shape `convert_inline_modifiers`
    /// produces for `(?flags)` (distinct from scoped `(?flags:body)` which
    /// always emits an FG with a real body). Returns the last such flag
    /// string in document order, since later toggles in the same branch
    /// override earlier ones. Only looks at the branch's direct children:
    /// toggles nested inside a `Group` / `Lookahead` / etc. are PCRE2-scoped
    /// to that nested context and don't leak out to the enclosing
    /// alternation.
    fn unscoped_trailing_flag(branch: &RegexAst) -> Option<String> {
        match branch {
            RegexAst::FlagGroup { flags, expr } if matches!(expr.as_ref(), RegexAst::Empty) => {
                Some(flags.clone())
            }
            RegexAst::Sequence(items) => {
                let mut last: Option<String> = None;
                for item in items {
                    if let RegexAst::FlagGroup { flags, expr } = item {
                        if matches!(expr.as_ref(), RegexAst::Empty) {
                            last = Some(flags.clone());
                        }
                    }
                }
                last
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------------
    // Extended / verbose mode (`(?x:...)`) whitespace & comment stripping
    // ------------------------------------------------------------------

    /// Strip unescaped whitespace and `#`-comments from `(?x:...)` scopes.
    ///
    /// In PCRE2 extended mode:
    /// * Unescaped ASCII whitespace (`Char(c)` where `c.is_ascii_whitespace()`)
    ///   is ignored.
    /// * `#` starts a comment that runs to the end of the line (or the end of the
    ///   sequence, whichever comes first).
    /// * Escaped whitespace (`\ `, `\t`, etc.) is preserved — PGEN already
    ///   converts those to `Char` via the `escape` rule, not `whitespace_literal`,
    ///   so they are never subject to stripping here.
    /// * Character classes (`[...]`) are parsed into `CharClass` AST nodes by
    ///   PGEN, so their internal whitespace is naturally unaffected.
    ///
    /// This pass runs after `lower_flag_toggles` (which lifts non-scoped `(?x)`
    /// toggles into `FlagGroup` wrappers) so that both `(?x:...)` and standalone
    /// `(?x)` forms are handled uniformly.
    fn strip_extended_mode(ast: RegexAst) -> RegexAst {
        Self::strip_extended_inner(ast, false)
    }

    /// Recursively process the AST. `in_x_mode` tracks whether the current
    /// subtree is inside a `(?x:...)` scope.
    ///
    /// `WhitespaceLiteral(c)` nodes (from PGEN's `whitespace_literal` rule)
    /// represent unescaped whitespace.  Inside x-mode they are stripped;
    /// outside x-mode they are lowered to ordinary `Char(c)`.
    ///
    /// Escaped whitespace (`\ `) goes through the `escape` rule and produces
    /// normal `Char(' ')` nodes that are always preserved.
    fn strip_extended_inner(ast: RegexAst, in_x_mode: bool) -> RegexAst {
        match ast {
            RegexAst::FlagGroup { flags, expr } => {
                // Parse the flag string the same way the VM codegen does:
                // characters before the `-` are enabled, characters after
                // `-` are disabled. `"x"` / `"ix"` enable x-mode; `"-x"`
                // / `"i-x"` disable it; omit 'x' altogether → inherit the
                // outer state. The previous implementation used
                // `flags.contains('x')`, which fired for both `"x"` and
                // `"-x"` and silently *enabled* x-mode inside `(?-x:...)`.
                // PCRE2 testinput1:3921 `/(?x)(?-x: \s*#\s*)/` on subject
                // "#" was the first case to hit this: the leading literal
                // space inside the `(?-x: ...)` group must stay
                // significant, so "#" (no leading space) must NOT match.
                let (enable, disable) = if let Some(pos) = flags.find('-') {
                    (&flags[..pos], &flags[pos + 1..])
                } else {
                    (flags.as_str(), "")
                };
                let x_active = if disable.contains('x') {
                    false
                } else if enable.contains('x') {
                    true
                } else {
                    in_x_mode
                };
                RegexAst::FlagGroup {
                    flags,
                    expr: Box::new(Self::strip_extended_inner(*expr, x_active)),
                }
            }
            RegexAst::Sequence(items) => {
                // First recurse into children (propagating x-mode context into
                // sub-expressions like groups and alternations), then handle
                // whitespace / comment stripping at the sequence level.
                //
                // IMPORTANT: we do NOT convert WhitespaceLiteral to Empty
                // inside the per-item recursion for sequences. We must keep
                // them intact so that `strip_x_mode_sequence` can see newline
                // WhitespaceLiteral nodes when terminating `#`-comments.
                let items: Vec<RegexAst> = items
                    .into_iter()
                    .map(|item| match &item {
                        // Keep WhitespaceLiteral as-is for sequence-level processing.
                        RegexAst::WhitespaceLiteral(_) => item,
                        _ => Self::strip_extended_inner(item, in_x_mode),
                    })
                    .collect();
                let items = if in_x_mode {
                    Self::strip_x_mode_sequence(items)
                } else {
                    // Outside x-mode, lower WhitespaceLiteral to Char.
                    items
                        .into_iter()
                        .map(|item| match item {
                            RegexAst::WhitespaceLiteral(c) => RegexAst::Char(c),
                            other => other,
                        })
                        .collect()
                };
                match items.len() {
                    0 => RegexAst::Empty,
                    1 => items.into_iter().next().unwrap(),
                    _ => RegexAst::Sequence(items),
                }
            }
            RegexAst::Alternation(items) => {
                let items: Vec<RegexAst> = items
                    .into_iter()
                    .map(|item| Self::strip_extended_inner(item, in_x_mode))
                    .collect();
                RegexAst::Alternation(items)
            }
            RegexAst::Quantified { expr, quantifier } => RegexAst::Quantified {
                expr: Box::new(Self::strip_extended_inner(*expr, in_x_mode)),
                quantifier,
            },
            RegexAst::Group {
                expr,
                kind,
                index,
                name,
            } => RegexAst::Group {
                expr: Box::new(Self::strip_extended_inner(*expr, in_x_mode)),
                kind,
                index,
                name,
            },
            RegexAst::Lookahead { expr, positive } => RegexAst::Lookahead {
                expr: Box::new(Self::strip_extended_inner(*expr, in_x_mode)),
                positive,
            },
            RegexAst::Lookbehind { expr, positive } => RegexAst::Lookbehind {
                expr: Box::new(Self::strip_extended_inner(*expr, in_x_mode)),
                positive,
            },
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => RegexAst::Conditional {
                condition,
                true_branch: Box::new(Self::strip_extended_inner(*true_branch, in_x_mode)),
                false_branch: false_branch
                    .map(|fb| Box::new(Self::strip_extended_inner(*fb, in_x_mode))),
            },
            // WhitespaceLiteral outside a Sequence: lower or strip directly.
            RegexAst::WhitespaceLiteral(c) => {
                if in_x_mode {
                    RegexAst::Empty
                } else {
                    RegexAst::Char(c)
                }
            }
            // All other nodes pass through unchanged.
            other => other,
        }
    }

    /// Strip `WhitespaceLiteral` nodes and `#`-comments from a sequence
    /// inside an extended-mode scope.
    ///
    /// This operates on the *raw* sequence items (before `WhitespaceLiteral`
    /// is lowered) so that newline whitespace literals can correctly terminate
    /// `#`-comments.
    fn strip_x_mode_sequence(items: Vec<RegexAst>) -> Vec<RegexAst> {
        let mut result = Vec::with_capacity(items.len());
        let mut iter = items.into_iter();
        while let Some(item) = iter.next() {
            match &item {
                // Drop unescaped whitespace.
                RegexAst::WhitespaceLiteral(_) | RegexAst::Empty => {}
                // `#` starts a comment — skip until a newline or end of
                // sequence. Both Char('\n') and WhitespaceLiteral('\n') count
                // as the newline terminator.
                RegexAst::Char('#') => {
                    for rest in iter.by_ref() {
                        let is_newline = matches!(
                            &rest,
                            RegexAst::Char('\n') | RegexAst::WhitespaceLiteral('\n')
                        );
                        if is_newline {
                            break;
                        }
                    }
                }
                _ => result.push(item),
            }
        }
        result
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
            RegexAst::FlagGroup { flags, expr } => Ok(RegexAst::FlagGroup {
                flags,
                expr: Box::new(Self::lower_extended_char_classes(*expr)?),
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
                Self::lower_extended_char_class_content(&content)
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

    fn lower_extended_char_class_content(content: &str) -> Result<RegexAst> {
        Ok(RegexAst::CharClass(crate::ast::CharClass::Custom {
            ranges: Self::resolve_extended_char_class_ranges(content)?,
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
            lhs = operator.apply(&lhs, &rhs);
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
        if let Some(posix) = Self::resolve_extended_posix_class_term(term)? {
            return Ok(posix);
        }

        let body = Self::extract_simple_extended_char_class_body(term)
            .ok_or_else(Self::extended_char_class_subset_error)?;
        Self::resolve_extended_nested_char_class_body(body)
    }

    fn resolve_extended_posix_class_term(term: &str) -> Result<Option<ScalarRangeSet>> {
        let Some(body) = Self::extract_simple_extended_char_class_body(term) else {
            return Ok(None);
        };

        let Some(spec) = Self::parse_extended_posix_class_spec(body)? else {
            return Ok(None);
        };

        Ok(Some(ScalarRangeSet::from_builtin_ranges(
            spec.class.ranges(),
            spec.negated,
        )))
    }

    fn parse_extended_posix_class_spec(body: &str) -> Result<Option<ExtendedPosixClassSpec>> {
        let Some(spec) = Self::extract_extended_posix_class_spec(body) else {
            return Ok(None);
        };

        let (negated, name) = spec
            .strip_prefix('^')
            .map_or((false, spec), |stripped| (true, stripped));
        let Some(class) = AsciiPosixClass::from_name(name) else {
            return Err(Self::extended_char_class_subset_error());
        };

        Ok(Some(ExtendedPosixClassSpec { class, negated }))
    }

    fn extract_extended_posix_class_spec(body: &str) -> Option<&str> {
        body.strip_prefix(':')
            .and_then(|inner| inner.strip_suffix(':'))
            .or_else(|| {
                body.strip_prefix("[:")
                    .and_then(|inner| inner.strip_suffix(":]"))
            })
    }

    #[cfg(test)]
    fn resolve_posix_class_ranges(name: &str) -> Option<&'static [ScalarRange]> {
        AsciiPosixClass::from_name(name).map(AsciiPosixClass::ranges)
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
            '0'..='7' => Self::resolve_extended_octal_escape_term(kind, cursor),
            'c' => Self::resolve_extended_control_escape_term(cursor),
            'd' => Self::resolve_char_class_scalar_ranges(&CharClass::Digit { negated: false }),
            'D' => Self::resolve_char_class_scalar_ranges(&CharClass::Digit { negated: true }),
            'w' => Self::resolve_char_class_scalar_ranges(&CharClass::Word { negated: false }),
            'W' => Self::resolve_char_class_scalar_ranges(&CharClass::Word { negated: true }),
            's' => Self::resolve_char_class_scalar_ranges(&CharClass::Space { negated: false }),
            'S' => Self::resolve_char_class_scalar_ranges(&CharClass::Space { negated: true }),
            'h' => Ok(ScalarRangeSet::from_builtin_ranges(
                &PCRE_HORIZONTAL_SPACE_RANGES,
                false,
            )),
            'H' => Ok(ScalarRangeSet::from_builtin_ranges(
                &PCRE_HORIZONTAL_SPACE_RANGES,
                true,
            )),
            'v' => Ok(ScalarRangeSet::from_builtin_ranges(
                &PCRE_VERTICAL_SPACE_RANGES,
                false,
            )),
            'V' => Ok(ScalarRangeSet::from_builtin_ranges(
                &PCRE_VERTICAL_SPACE_RANGES,
                true,
            )),
            'o' => Self::resolve_extended_braced_octal_escape_term(cursor),
            'x' => Self::resolve_extended_hex_escape_term(cursor),
            'p' | 'P' => Self::resolve_extended_unicode_property_escape_term(kind, cursor),
            _ => Err(Self::extended_char_class_subset_error()),
        }
    }

    fn resolve_extended_nested_char_class_body(body: &str) -> Result<ScalarRangeSet> {
        if body.is_empty() {
            return Err(Self::extended_char_class_subset_error());
        }

        let mut cursor = ExtendedCharClassCursor::new(body);
        let negated = matches!(cursor.peek_char(), Some('^'));
        if negated {
            cursor.consume_char();
        }

        let mut resolved = ScalarRangeSet::new(Vec::new());
        let mut saw_atom = false;

        while !cursor.is_eof() {
            let atom = Self::parse_extended_nested_char_class_atom(&mut cursor)?;
            saw_atom = true;

            if let Some(start) = atom.scalar {
                let snapshot = cursor;
                if cursor.peek_char() == Some('-') {
                    cursor.consume_char();
                    if !cursor.is_eof() {
                        if let Ok(end_atom) =
                            Self::parse_extended_nested_char_class_atom(&mut cursor)
                        {
                            if let Some(end) = end_atom.scalar {
                                if end < start {
                                    return Err(Self::extended_char_class_subset_error());
                                }
                                resolved = resolved.union(&ScalarRangeSet::new(vec![(start, end)]));
                                continue;
                            }
                        }
                    }
                    cursor = snapshot;
                }
            }

            resolved = resolved.union(&atom.set);
        }

        if !saw_atom {
            return Err(Self::extended_char_class_subset_error());
        }

        if negated {
            Ok(resolved.complement())
        } else {
            Ok(resolved)
        }
    }

    fn parse_extended_nested_char_class_atom(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ExtendedNestedCharClassAtom> {
        match cursor.peek_char() {
            Some('\\') => Ok(ExtendedNestedCharClassAtom::from_set(
                Self::resolve_extended_escape_term(cursor)?,
            )),
            Some('[') => {
                let term = Self::consume_extended_char_class_bracket_term(cursor)?;
                let Some(posix) = Self::resolve_extended_posix_class_term(term)? else {
                    return Err(Self::extended_char_class_subset_error());
                };
                Ok(ExtendedNestedCharClassAtom::from_set(posix))
            }
            Some(ch) if ch.is_whitespace() => Err(Self::extended_char_class_subset_error()),
            Some(ch) => {
                cursor.consume_char();
                Ok(ExtendedNestedCharClassAtom::from_char(ch))
            }
            None => Err(Self::extended_char_class_subset_error()),
        }
    }

    fn resolve_extended_literal_escape(kind: char) -> Option<char> {
        match kind {
            'b' => Some('\u{08}'),
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

    fn scalar_range_set_from_code_point(code_point: u32) -> Result<ScalarRangeSet> {
        let ch = char::from_u32(code_point).ok_or_else(Self::extended_char_class_subset_error)?;
        Ok(ScalarRangeSet::from_char(ch))
    }

    fn scalar_range_set_from_radix_digits(digits: &str, radix: u32) -> Result<ScalarRangeSet> {
        let code_point = u32::from_str_radix(digits, radix)
            .map_err(|_| Self::extended_char_class_subset_error())?;
        Self::scalar_range_set_from_code_point(code_point)
    }

    fn is_extended_radix_digit(ch: char, radix: u32) -> bool {
        match radix {
            8 => matches!(ch, '0'..='7'),
            16 => ch.is_ascii_hexdigit(),
            _ => false,
        }
    }

    fn consume_extended_braced_radix_digits(
        cursor: &mut ExtendedCharClassCursor<'_>,
        radix: u32,
    ) -> Result<String> {
        if cursor.consume_char() != Some('{') {
            return Err(Self::extended_char_class_subset_error());
        }

        let mut digits = String::new();
        let mut closed = false;

        while let Some(ch) = cursor.consume_char() {
            if ch == '}' {
                closed = true;
                break;
            }
            if !Self::is_extended_radix_digit(ch, radix) {
                return Err(Self::extended_char_class_subset_error());
            }
            digits.push(ch);
        }

        if digits.is_empty() || !closed {
            return Err(Self::extended_char_class_subset_error());
        }

        Ok(digits)
    }

    fn resolve_extended_octal_escape_term(
        initial: char,
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        let mut octal_digits = String::from(initial);

        while octal_digits.len() < 3 {
            let Some(ch) = cursor.peek_char() else {
                break;
            };
            if !matches!(ch, '0'..='7') {
                break;
            }
            octal_digits.push(ch);
            cursor.consume_char();
        }

        Self::scalar_range_set_from_radix_digits(&octal_digits, 8)
    }

    fn resolve_extended_control_escape_term(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        let Some(ch) = cursor.consume_char() else {
            return Err(Self::extended_char_class_subset_error());
        };

        let normalized = match ch {
            'a'..='z' => ch.to_ascii_uppercase(),
            'A'..='Z' | '@' | '[' | '\\' | ']' | '^' | '_' | '?' => ch,
            _ => return Err(Self::extended_char_class_subset_error()),
        };

        Self::scalar_range_set_from_code_point((normalized as u32) ^ 0x40)
    }

    fn resolve_extended_braced_octal_escape_term(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        let octal_digits = Self::consume_extended_braced_radix_digits(cursor, 8)?;
        Self::scalar_range_set_from_radix_digits(&octal_digits, 8)
    }

    fn resolve_extended_hex_escape_term(
        cursor: &mut ExtendedCharClassCursor<'_>,
    ) -> Result<ScalarRangeSet> {
        let hex_digits = if cursor.peek_char() == Some('{') {
            Self::consume_extended_braced_radix_digits(cursor, 16)?
        } else {
            let mut digits = String::new();
            while digits.len() < 2 {
                let Some(ch) = cursor.peek_char() else {
                    break;
                };
                if !Self::is_extended_radix_digit(ch, 16) {
                    break;
                }
                digits.push(ch);
                cursor.consume_char();
            }

            if digits.is_empty() {
                return Err(Self::extended_char_class_subset_error());
            }
            digits
        };

        Self::scalar_range_set_from_radix_digits(&hex_digits, 16)
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

    pub(crate) fn assign_capture_indices(ast: RegexAst) -> RegexAst {
        let (ast, _) = Self::assign_capture_indices_inner(ast, 1);
        ast
    }

    #[allow(clippy::too_many_lines)]
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
            RegexAst::FlagGroup { flags, expr } => {
                let (expr, next) = Self::assign_capture_indices_inner(*expr, next_group);
                (
                    RegexAst::FlagGroup {
                        flags,
                        expr: Box::new(expr),
                    },
                    next,
                )
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
            } => Self::assign_capture_indices_group(*expr, kind, name, next_group),
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
            | RegexAst::NamedBackreference(_)
            | RegexAst::RelativeBackreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::ReturnedCaptureSubroutine { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Callout(_)
            | RegexAst::MatchReset
            | RegexAst::NewlineSequence
            | RegexAst::GraphemeCluster
            | RegexAst::Accept
            | RegexAst::Commit
            | RegexAst::Prune
            | RegexAst::Skip(_)
            | RegexAst::Then
            | RegexAst::Mark(_)
            | RegexAst::WhitespaceLiteral(_)
            | RegexAst::Empty => (ast, next_group),
        }
    }

    fn assign_capture_indices_group(
        expr: RegexAst,
        kind: crate::ast::GroupKind,
        name: Option<String>,
        next_group: u32,
    ) -> (RegexAst, u32) {
        match kind {
            crate::ast::GroupKind::Capturing => {
                let group_id = next_group;
                let (expr, next) =
                    Self::assign_capture_indices_inner(expr, group_id.saturating_add(1));
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
                let (expr, next) = Self::assign_branch_reset_expr(expr, next_group);
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
                let (expr, next) = Self::assign_capture_indices_inner(expr, next_group);
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

    #[allow(clippy::too_many_lines)]
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
            RegexAst::FlagGroup { flags, expr } => {
                let (expr, opened_after_expr) =
                    Self::resolve_relative_conditionals_inner(*expr, opened_groups, total_groups)?;
                Ok((
                    RegexAst::FlagGroup {
                        flags,
                        expr: Box::new(expr),
                    },
                    opened_after_expr,
                ))
            }
            RegexAst::Group {
                expr,
                kind,
                index,
                name,
            } => Self::resolve_relative_conditionals_group(
                *expr,
                kind,
                index,
                name,
                opened_groups,
                total_groups,
            ),
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
            } => Self::resolve_relative_conditionals_conditional(
                condition,
                *true_branch,
                false_branch.map(|b| *b),
                opened_groups,
                total_groups,
            ),
            RegexAst::Recursion {
                target: crate::ast::RecursionTarget::RelativeGroup(offset),
            } => {
                let resolved =
                    Self::resolve_relative_group_reference(offset, opened_groups, total_groups)?;
                Ok((
                    RegexAst::Recursion {
                        target: crate::ast::RecursionTarget::Group(resolved),
                    },
                    opened_groups,
                ))
            }
            RegexAst::RelativeBackreference(offset) => {
                let resolved =
                    Self::resolve_relative_group_reference(offset, opened_groups, total_groups)?;
                Ok((RegexAst::Backreference(resolved), opened_groups))
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
            | RegexAst::NamedBackreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::ReturnedCaptureSubroutine { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Callout(_)
            | RegexAst::MatchReset
            | RegexAst::NewlineSequence
            | RegexAst::GraphemeCluster
            | RegexAst::Accept
            | RegexAst::Commit
            | RegexAst::Prune
            | RegexAst::Skip(_)
            | RegexAst::Then
            | RegexAst::Mark(_)
            | RegexAst::WhitespaceLiteral(_)
            | RegexAst::Empty => Ok((ast, opened_groups)),
        }
    }

    fn resolve_relative_conditionals_group(
        expr: RegexAst,
        kind: crate::ast::GroupKind,
        index: Option<u32>,
        name: Option<String>,
        opened_groups: u32,
        total_groups: u32,
    ) -> Result<(RegexAst, u32)> {
        let (expr, opened_after_expr) = if matches!(kind, crate::ast::GroupKind::BranchReset) {
            Self::resolve_relative_conditionals_branch_reset(expr, opened_groups, total_groups)?
        } else {
            let inner_opened = if matches!(kind, crate::ast::GroupKind::Capturing) {
                index.unwrap_or_else(|| opened_groups.saturating_add(1))
            } else {
                opened_groups
            };
            Self::resolve_relative_conditionals_inner(expr, inner_opened, total_groups)?
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

    fn resolve_relative_conditionals_conditional(
        condition: crate::ast::ConditionalTest,
        true_branch: RegexAst,
        false_branch: Option<RegexAst>,
        opened_groups: u32,
        total_groups: u32,
    ) -> Result<(RegexAst, u32)> {
        let (condition, opened_after_condition) =
            Self::resolve_relative_conditional_test(condition, opened_groups, total_groups)?;
        let (true_branch, opened_after_true) = Self::resolve_relative_conditionals_inner(
            true_branch,
            opened_after_condition,
            total_groups,
        )?;
        let (false_branch, opened_after_false) = if let Some(false_branch) = false_branch {
            let (false_branch, opened_after_false) = Self::resolve_relative_conditionals_inner(
                false_branch,
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
            | RegexAst::Lookbehind { expr, .. }
            | RegexAst::Group { expr, .. }
            | RegexAst::FlagGroup { expr, .. } => Self::parser_boundary_validation_message(expr),
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
            | RegexAst::NamedBackreference(_)
            | RegexAst::RelativeBackreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::ReturnedCaptureSubroutine { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Callout(_)
            | RegexAst::MatchReset
            | RegexAst::NewlineSequence
            | RegexAst::GraphemeCluster
            | RegexAst::Accept
            | RegexAst::Commit
            | RegexAst::Prune
            | RegexAst::Skip(_)
            | RegexAst::Then
            | RegexAst::Mark(_)
            | RegexAst::WhitespaceLiteral(_)
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
                    Err(RgxError::compile(format!(
                        "conditional '(?(R{group})...)' refers to missing capture group"
                    )))
                } else {
                    Ok(crate::ast::ConditionalTest::RecursionGroup(group))
                }
            }
            crate::ast::ConditionalTest::RecursionNamed(name) => {
                let group = named_groups.get(&name).copied().ok_or_else(|| {
                    RgxError::compile(format!(
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
            RgxError::compile(format!(
                "conditional '(?({offset:+})...)' refers to missing capture group"
            ))
        };

        if offset == 0 {
            return Err(missing_reference());
        }

        let resolved = if offset > 0 {
            #[allow(clippy::cast_sign_loss)] // Sign is validated positive above.
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
            RegexAst::UnicodeClass { name, negated }
            | RegexAst::CharClass(crate::ast::CharClass::UnicodeClass { name, negated }) => {
                resolve_unicode_property_class(name, *negated).err()
            }
            RegexAst::ExtendedCharClass { .. } => {
                Some(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE.to_string())
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
                crate::ast::RecursionTarget::RelativeGroup(_) => {
                    // Should already be resolved by resolve_relative_conditionals
                    None
                }
            },
            RegexAst::NamedBackreference(name) => {
                if named_groups.contains_key(name) {
                    None
                } else {
                    Some(format!(
                        "named backreference '\\k<{name}>' refers to missing named capture group"
                    ))
                }
            }
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => items
                .iter()
                .find_map(|item| self.feature_validation_message(item, total_groups, named_groups)),
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. }
            | RegexAst::Group { expr, .. }
            | RegexAst::FlagGroup { expr, .. } => {
                self.feature_validation_message(expr, total_groups, named_groups)
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => self
                .validate_conditional_test(
                    condition,
                    false_branch.is_some(),
                    total_groups,
                    named_groups,
                )
                .or_else(|| {
                    self.feature_validation_message(true_branch, total_groups, named_groups)
                })
                .or_else(|| {
                    false_branch.as_ref().and_then(|branch| {
                        self.feature_validation_message(branch, total_groups, named_groups)
                    })
                }),
            _ => None,
        }
    }

    fn validate_conditional_test(
        &self,
        condition: &crate::ast::ConditionalTest,
        has_false_branch: bool,
        total_groups: u32,
        named_groups: &std::collections::HashMap<String, u32>,
    ) -> Option<String> {
        match condition {
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
                if has_false_branch {
                    Some(
                        "conditional '(?(DEFINE)...)' does not support a false branch".to_string(),
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
        }
    }

    fn backreference_validation_message(ast: &RegexAst) -> Option<String> {
        let total_groups = Self::max_capture_group(ast);
        Self::backreference_validation_message_inner(ast, total_groups)
    }

    fn backreference_validation_message_inner(ast: &RegexAst, total_groups: u32) -> Option<String> {
        match ast {
            RegexAst::Backreference(group) if *group > total_groups => Some(format!(
                "backreference '\\{group}' refers to missing capture group"
            )),
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => items
                .iter()
                .find_map(|item| Self::backreference_validation_message_inner(item, total_groups)),
            RegexAst::Quantified { expr, .. }
            | RegexAst::Group { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. }
            | RegexAst::FlagGroup { expr, .. } => {
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
            RegexAst::Backreference(_)
            | RegexAst::NamedBackreference(_)
            | RegexAst::RelativeBackreference(_)
            | RegexAst::Char(_)
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
            | RegexAst::ReturnedCaptureSubroutine { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Callout(_)
            | RegexAst::MatchReset
            | RegexAst::NewlineSequence
            | RegexAst::GraphemeCluster
            | RegexAst::Accept
            | RegexAst::Commit
            | RegexAst::Prune
            | RegexAst::Skip(_)
            | RegexAst::Then
            | RegexAst::Mark(_)
            | RegexAst::WhitespaceLiteral(_)
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
            | RegexAst::Lookbehind { expr, .. }
            | RegexAst::FlagGroup { expr, .. } => Self::max_capture_group(expr),
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
            | RegexAst::NamedBackreference(_)
            | RegexAst::RelativeBackreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::ReturnedCaptureSubroutine { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Callout(_)
            | RegexAst::MatchReset
            | RegexAst::NewlineSequence
            | RegexAst::GraphemeCluster
            | RegexAst::Accept
            | RegexAst::Commit
            | RegexAst::Prune
            | RegexAst::Skip(_)
            | RegexAst::Then
            | RegexAst::Mark(_)
            | RegexAst::WhitespaceLiteral(_)
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
            | RegexAst::Lookbehind { expr, .. }
            | RegexAst::FlagGroup { expr, .. } => {
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
            | RegexAst::NamedBackreference(_)
            | RegexAst::RelativeBackreference(_)
            | RegexAst::Recursion { .. }
            | RegexAst::ReturnedCaptureSubroutine { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Callout(_)
            | RegexAst::MatchReset
            | RegexAst::NewlineSequence
            | RegexAst::GraphemeCluster
            | RegexAst::Accept
            | RegexAst::Commit
            | RegexAst::Prune
            | RegexAst::Skip(_)
            | RegexAst::Then
            | RegexAst::Mark(_)
            | RegexAst::WhitespaceLiteral(_)
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
        let lowered = Compiler::lower_extended_char_class_content("[a-z]")
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
        let lowered = Compiler::lower_extended_char_class_content("[^0-9]")
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
        let lowered = Compiler::lower_extended_char_class_content("[a-z] - [aeiou]")
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
        let lowered = Compiler::lower_extended_char_class_content(r"\p{L} & \p{Lu}")
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
        let lowered = Compiler::lower_extended_char_class_content("![0-9]")
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
    fn lower_extended_char_classes_recurses_into_flag_group() {
        // Regression: `(?i)(?[...])` left the ExtendedCharClass node
        // inside a FlagGroup unlowered because the transform's
        // fall-through `other => Ok(other)` branch caught FlagGroup.
        // That resulted in a panic at vm.rs codegen_pass when the
        // case-insensitive prefix was used via RegexBuilder. Surfaced
        // by testinput4 lines 3066 / 3081 in the PCRE2 conformance
        // harness; minimal reproducer below.
        use crate::Regex;
        let r = Regex::compile(r"(?i)(?[ [\p{Lu}1] ^ \p{Ll} ])").expect("compiles");
        // With /i case-folding: the set symmetric-difference of
        // (uppercase letters or '1') XOR (lowercase letters) reduces
        // to the '1' character after case-folding collapses `\p{Lu}`
        // onto `\p{Ll}`. Semantics aren't the test target here; the
        // test is that the pattern compiles and matches without
        // panicking.
        let _ = r.is_match("1");
        let _ = r.is_match("a");
        let _ = r.is_match("A");
        let _ = r.is_match("_");
    }

    #[test]
    fn lower_extended_char_classes_recurses_into_nested_flag_group() {
        // Same bug would apply with nested FlagGroup containers.
        use crate::Regex;
        let r = Regex::compile(r"(?i)((?m)(?[[a-c]]))").expect("nested flag groups compile");
        let _ = r.is_match("a");
        let _ = r.is_match("B");
    }

    #[test]
    fn lower_extended_char_class_content_maps_grouped_algebra() {
        let lowered = Compiler::lower_extended_char_class_content("([a-z] - [aeiou]) & [b-d]")
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
        let lowered = Compiler::lower_extended_char_class_content("[AC] ^ [BC]")
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
        let lowered = Compiler::lower_extended_char_class_content("[a-f] | [d-z] & [m-p]")
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
        let lowered = Compiler::lower_extended_char_class_content("[a-z] - [aeiou] + [0-9] - [5]")
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
        let lowered = Compiler::lower_extended_char_class_content("[a-z] & [d-z] & [m-p]")
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
        let lowered = Compiler::lower_extended_char_class_content(r"\d - [3]")
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
        let lowered = Compiler::lower_extended_char_class_content(r"\D & [A-F]")
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
    fn lower_extended_char_class_content_maps_nested_ordinary_shorthand_and_range_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"[\dA-F]")
            .expect("Expected nested ordinary class to accept current shorthand and range atoms");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '0'));
        assert!(range_contains(&ranges, '9'));
        assert!(range_contains(&ranges, 'A'));
        assert!(range_contains(&ranges, 'F'));
        assert!(!range_contains(&ranges, 'G'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_nested_ordinary_posix_term() {
        let lowered = Compiler::lower_extended_char_class_content("[[:graph:]]")
            .expect("Expected nested ordinary class to accept current POSIX atoms");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'A'));
        assert!(range_contains(&ranges, '9'));
        assert!(range_contains(&ranges, '!'));
        assert!(!range_contains(&ranges, ' '));
    }

    #[test]
    fn lower_extended_char_class_content_maps_nested_ordinary_property_term_inside_algebra() {
        let lowered = Compiler::lower_extended_char_class_content(r"[\p{L}] - [\p{Lu}]").expect(
            "Expected nested ordinary class property atoms to compose with shipped algebra",
        );

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'a'));
        assert!(range_contains(&ranges, 'z'));
        assert!(!range_contains(&ranges, 'A'));
        assert!(!range_contains(&ranges, 'Z'));
        assert!(!range_contains(&ranges, '1'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_hex_escape_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\x{41} - [B]")
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
        let lowered = Compiler::lower_extended_char_class_content(r"\n | \t")
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
    fn lower_extended_char_class_content_maps_bare_control_literal_escape_terms() {
        let lowered = Compiler::lower_extended_char_class_content(r"\a | \b | \e | \f")
            .expect("Expected bare control-literal escape terms to lower into the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '\u{07}'));
        assert!(range_contains(&ranges, '\u{08}'));
        assert!(range_contains(&ranges, '\u{1B}'));
        assert!(range_contains(&ranges, '\u{0C}'));
        assert!(!range_contains(&ranges, '\n'));
        assert!(!range_contains(&ranges, 'A'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_control_letter_escape_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\cA | [B]")
            .expect("Expected bare control-letter escape to lower into the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '\u{0001}'));
        assert!(range_contains(&ranges, 'B'));
        assert!(!range_contains(&ranges, 'A'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_octal_escape_terms() {
        let lowered = Compiler::lower_extended_char_class_content(r"\040 | \011 | \o{101}")
            .expect("Expected bare octal escapes to lower into the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, ' '));
        assert!(range_contains(&ranges, '\t'));
        assert!(range_contains(&ranges, 'A'));
        assert!(!range_contains(&ranges, 'B'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_escaped_operator_literal_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\- | [A]")
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
    fn lower_extended_char_class_content_maps_bare_horizontal_shorthand_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\h").expect(
            "Expected horizontal-whitespace shorthand to remain part of the shipped subset",
        );

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '\t'));
        assert!(range_contains(&ranges, ' '));
        assert!(range_contains(&ranges, '\u{00A0}'));
        assert!(range_contains(&ranges, '\u{1680}'));
        assert!(range_contains(&ranges, '\u{202F}'));
        assert!(range_contains(&ranges, '\u{3000}'));
        assert!(!range_contains(&ranges, '\n'));
        assert!(!range_contains(&ranges, 'A'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_negated_bare_horizontal_shorthand_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\H").expect(
            "Expected negated horizontal-whitespace shorthand to remain part of the shipped subset",
        );

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '\n'));
        assert!(range_contains(&ranges, 'A'));
        assert!(!range_contains(&ranges, '\t'));
        assert!(!range_contains(&ranges, ' '));
        assert!(!range_contains(&ranges, '\u{00A0}'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_vertical_shorthand_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\v")
            .expect("Expected vertical-whitespace shorthand to remain part of the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '\n'));
        assert!(range_contains(&ranges, '\u{000B}'));
        assert!(range_contains(&ranges, '\u{0085}'));
        assert!(range_contains(&ranges, '\u{2028}'));
        assert!(range_contains(&ranges, '\u{2029}'));
        assert!(!range_contains(&ranges, ' '));
        assert!(!range_contains(&ranges, '\u{00A0}'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_negated_bare_vertical_shorthand_term() {
        let lowered = Compiler::lower_extended_char_class_content(r"\V").expect(
            "Expected negated vertical-whitespace shorthand to remain part of the shipped subset",
        );

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, ' '));
        assert!(range_contains(&ranges, '\t'));
        assert!(range_contains(&ranges, '\u{00A0}'));
        assert!(!range_contains(&ranges, '\n'));
        assert!(!range_contains(&ranges, '\u{2028}'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_posix_graph_term() {
        let lowered = Compiler::lower_extended_char_class_content("[:graph:]")
            .expect("Expected bare POSIX graph term to remain part of the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'A'));
        assert!(range_contains(&ranges, '9'));
        assert!(range_contains(&ranges, '!'));
        assert!(!range_contains(&ranges, ' '));
        assert!(!range_contains(&ranges, '\n'));
        assert!(!range_contains(&ranges, '\u{0001}'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_negated_bare_posix_alpha_term() {
        let lowered = Compiler::lower_extended_char_class_content("[:^alpha:]")
            .expect("Expected negated bare POSIX alpha term to remain part of the shipped subset");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '1'));
        assert!(range_contains(&ranges, '!'));
        assert!(!range_contains(&ranges, 'A'));
        assert!(!range_contains(&ranges, 'z'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_bare_posix_alpha_term_inside_algebra() {
        let lowered = Compiler::lower_extended_char_class_content(r"[:alpha:] & [a-z\t]")
            .expect("Expected bare POSIX alpha term to compose with shipped algebra");

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, 'a'));
        assert!(range_contains(&ranges, 'z'));
        assert!(!range_contains(&ranges, 'A'));
        assert!(!range_contains(&ranges, '\t'));
        assert!(!range_contains(&ranges, '1'));
    }

    #[test]
    fn lower_extended_char_class_content_maps_complemented_bare_posix_alpha_term() {
        let lowered = Compiler::lower_extended_char_class_content("![:alpha:]").expect(
            "Expected complemented bare POSIX alpha term to remain part of the shipped subset",
        );

        let RegexAst::CharClass(CharClass::Custom { ranges, negated }) = lowered else {
            panic!("expected lowered custom char class");
        };
        assert!(!negated);
        assert!(range_contains(&ranges, '1'));
        assert!(range_contains(&ranges, '!'));
        assert!(!range_contains(&ranges, 'A'));
        assert!(!range_contains(&ranges, 'z'));
    }

    #[test]
    fn parse_extended_posix_class_spec_accepts_current_ascii_forms() {
        assert_eq!(
            Compiler::parse_extended_posix_class_spec(":alpha:")
                .expect("Expected bare ASCII POSIX alpha body to parse"),
            Some(ExtendedPosixClassSpec {
                class: AsciiPosixClass::Alpha,
                negated: false,
            })
        );
        assert_eq!(
            Compiler::parse_extended_posix_class_spec(":^graph:")
                .expect("Expected negated bare ASCII POSIX graph body to parse"),
            Some(ExtendedPosixClassSpec {
                class: AsciiPosixClass::Graph,
                negated: true,
            })
        );
        assert_eq!(
            Compiler::parse_extended_posix_class_spec("[:word:]")
                .expect("Expected nested bare ASCII POSIX word body to parse"),
            Some(ExtendedPosixClassSpec {
                class: AsciiPosixClass::Word,
                negated: false,
            })
        );
    }

    #[test]
    fn parse_extended_posix_class_spec_rejects_unknown_ascii_class_names() {
        let err = Compiler::parse_extended_posix_class_spec(":emoji:")
            .expect_err("Expected unknown POSIX class names to stay behind the subset boundary");
        assert!(
            err.to_string().contains(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE),
            "unexpected POSIX-spec boundary message: {err}"
        );
    }

    #[test]
    fn parse_extended_posix_class_spec_ignores_non_posix_bodies() {
        assert_eq!(
            Compiler::parse_extended_posix_class_spec("a-z").expect(
                "Expected non-POSIX simple bodies to stay available for ordinary class lowering"
            ),
            None
        );
        assert_eq!(
            Compiler::parse_extended_posix_class_spec(r"\d").expect(
                "Expected escape bodies to stay available for ordinary escape-term lowering"
            ),
            None
        );
    }

    #[test]
    fn resolve_posix_class_ranges_matches_ascii_posix_registry() {
        assert_eq!(
            Compiler::resolve_posix_class_ranges("graph"),
            Some(&ASCII_GRAPH_RANGES[..])
        );
        assert_eq!(
            Compiler::resolve_posix_class_ranges("word"),
            Some(&ASCII_WORD_RANGES[..])
        );
        assert_eq!(Compiler::resolve_posix_class_ranges("emoji"), None);
    }

    #[test]
    fn lower_extended_char_class_content_rejects_unclosed_hex_escape() {
        let err = Compiler::lower_extended_char_class_content(r"\x{41")
            .expect_err("Expected malformed hex escape to stay behind the explicit boundary");
        assert!(
            err.to_string().contains(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE),
            "unexpected malformed-escape boundary message: {err}"
        );
    }

    #[test]
    fn lower_extended_char_class_content_rejects_malformed_octal_escape() {
        let err = Compiler::lower_extended_char_class_content(r"\o{8}")
            .expect_err("Expected malformed octal escape to stay behind the explicit boundary");
        assert!(
            err.to_string().contains(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE),
            "unexpected malformed-octal boundary message: {err}"
        );
    }

    #[test]
    fn lower_extended_char_class_content_rejects_invalid_control_escape() {
        let err = Compiler::lower_extended_char_class_content(r"\c1")
            .expect_err("Expected invalid control escape to stay behind the explicit boundary");
        assert!(
            err.to_string().contains(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE),
            "unexpected invalid-control boundary message: {err}"
        );
    }

    #[test]
    fn consume_extended_braced_radix_digits_accepts_current_octal_and_hex_forms() {
        let mut octal_cursor = ExtendedCharClassCursor::new("{101}");
        assert_eq!(
            Compiler::consume_extended_braced_radix_digits(&mut octal_cursor, 8)
                .expect("Expected braced octal digits to parse"),
            "101"
        );
        assert!(octal_cursor.is_eof());

        let mut hex_cursor = ExtendedCharClassCursor::new("{41}");
        assert_eq!(
            Compiler::consume_extended_braced_radix_digits(&mut hex_cursor, 16)
                .expect("Expected braced hex digits to parse"),
            "41"
        );
        assert!(hex_cursor.is_eof());
    }

    #[test]
    fn consume_extended_braced_radix_digits_rejects_empty_invalid_or_unclosed_forms() {
        for (body, radix) in [("{}", 8), ("{8}", 8), ("{41", 16)] {
            let mut cursor = ExtendedCharClassCursor::new(body);
            let err = Compiler::consume_extended_braced_radix_digits(&mut cursor, radix)
                .expect_err("Expected malformed braced radix digits to stay behind the boundary");
            assert!(
                err.to_string().contains(EXTENDED_CHAR_CLASS_SUBSET_MESSAGE),
                "unexpected malformed braced-digit boundary message for {body}: {err}"
            );
        }
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
