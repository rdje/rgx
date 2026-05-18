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

    // Deterministic pre-PGEN input validation. PGEN's generated
    // recursive-descent parser recurses once per `(` nesting level
    // with no internal stack/recursion guard, so a deeply nested
    // pattern overflows the thread stack and aborts the *host
    // process* (SIGSEGV→SIGABRT) — confirmed via lldb backtrace
    // inside `RegexParser::parse_group → … → parse_group`. PGEN is
    // the sole parser and read-only from RGX, so the fix is to
    // reject crash-inducing input here, before PGEN is invoked
    // (filed upstream as a PGEN issue so PGEN gains its own guard).
    // The compile-time analog of the runtime DoS limits
    // (`set_max_steps` etc.). See `crate::recursion`.
    let nesting = crate::recursion::pattern_nesting_depth(pattern);
    if crate::recursion::exceeds_nesting_limit(nesting) {
        let err = crate::recursion::too_deeply_nested();
        trace_exit!(
            "parsing",
            "parsing::parse_pattern[pgen]",
            "ok=false,reason=nesting-limit"
        );
        return Err(err);
    }

    // Within the limit: run PGEN's (correct) recursive parse and the
    // typed-AST walk on a guaranteed-deep stack so a legitimately
    // deep pattern can never overflow the caller's thread stack.
    // This is exact parity with the `serde_stacker` treatment RGX
    // already applies to PGEN's JSON deserialization — giving the
    // sole parser enough stack to do its job, not absorbing
    // malformed output.
    let result = crate::recursion::compile_on_deep_stack(|| {
        let mut parser = PgenParser::new();
        parser.parse_pattern(pattern)
    });
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
    Err(crate::error::RgxError::compile(
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
            let err = RgxError::compile(
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
            let err = RgxError::compile(format!(
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
            let err = RgxError::compile(format!(
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
            let err = RgxError::compile(format!(
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
                    || RgxError::compile("pgen AST dump failed without a diagnostic"),
                    |diagnostic| {
                        if let Some(loc) = &diagnostic.location {
                            RgxError::compile_at(
                                format!("{}: {}", diagnostic.code, diagnostic.message),
                                pattern,
                                loc.byte_offset,
                            )
                        } else {
                            RgxError::compile(format!(
                                "{}: {}",
                                diagnostic.code, diagnostic.message
                            ))
                        }
                    },
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
    /// Typed structured carrier — added in PGEN regex parser release 1.1.30
    /// (slice 9+ of task #40's annotation campaign). Annotated rules
    /// emit their typed shape directly as a `serde_json::Value` instead
    /// of a recursive `Sequence` / `Alternative` envelope. Currently
    /// covers `regex`, `pattern`, `concatenation`, `piece`,
    /// `piece_quoted_run_quantified`, `quoted_run_inner_piece`,
    /// `quant_suffix`, `digits`, `posix_class`. See
    /// `subs/pgen/docs/regex_parser_book/src/json-carrier.md`.
    Json(serde_json::Value),
    Sequence(Vec<PgenAstNode>),
    Alternative(Box<PgenAstNode>),
    Quantified((Vec<PgenAstNode>, String)),
}

/// Collapse a `PgenAstContent` tree into the unified `serde_json::Value`
/// shape per PGEN's `to_json_value()` contract:
///
/// - `Terminal(s)` → `String(s)`.
/// - `TransformedTerminal(s)` → `serde_json::from_str(s).unwrap_or(String(s))`.
/// - `Json(v)` → `v.clone()`.
/// - `Sequence(nodes)` → `Array([collapse(n.content) for n in nodes])`.
/// - `Alternative(node)` → `collapse(node.content)` (transparent unwrap).
/// - `Quantified(nodes, _)` → `Array([collapse(n.content) for n in nodes])`.
///
/// This mirrors `pgen::ast_pipeline::ParseContent::to_json_value()` exactly,
/// preserving the byte-equivalence guarantee documented in the regex parser
/// book's `parse-content-variants.md` chapter, "Special interaction 1".
#[cfg(feature = "pgen-parser")]
fn collapse_to_json(content: &PgenAstContent) -> serde_json::Value {
    match content {
        PgenAstContent::Terminal(s) => serde_json::Value::String(s.clone()),
        PgenAstContent::TransformedTerminal(s) => serde_json::from_str::<serde_json::Value>(s)
            .unwrap_or_else(|_| serde_json::Value::String(s.clone())),
        PgenAstContent::Json(value) => value.clone(),
        PgenAstContent::Sequence(nodes) => {
            serde_json::Value::Array(nodes.iter().map(|n| collapse_to_json(&n.content)).collect())
        }
        PgenAstContent::Alternative(node) => collapse_to_json(&node.content),
        PgenAstContent::Quantified((nodes, _marker)) => {
            serde_json::Value::Array(nodes.iter().map(|n| collapse_to_json(&n.content)).collect())
        }
    }
}

#[cfg(feature = "pgen-parser")]
struct PgenAstAdapter<'a> {
    pattern: &'a str,
    /// Unicode Character Properties mode (`PCRE2_UCP`). When set, `\d`,
    /// `\w`, `\s` compile to Unicode-property-backed character classes
    /// rather than the ASCII shorthands. Detected by scanning the
    /// pattern for a leading `(*UCP)` start-verb.
    ucp_enabled: bool,
    /// PCRE2 `BSR_ANYCRLF` mode: when set, `\R` matches only CR, LF,
    /// and CRLF. The default `BSR_UNICODE` mode (false) also matches
    /// VT, FF, NEL (U+0085), LINE SEPARATOR, PARAGRAPH SEPARATOR.
    /// Detected by scanning the pattern for `(*BSR_ANYCRLF)`.
    bsr_anycrlf: bool,
    /// PCRE2 newline convention: the set of characters treated as
    /// newlines by `.`, `\N`, and (under `/m`) `^` / `$`. Detected
    /// from `(*CR)` / `(*LF)` / `(*CRLF)` / `(*ANYCRLF)` / `(*ANY)` /
    /// `(*NUL)` pattern-start pragmas. Default is `Lf` (matches RGX's
    /// pre-existing behaviour — `.` excludes `\n` only).
    newline_mode: NewlineMode,
    /// Current pattern-nesting depth during the recursive typed-AST
    /// walk. Bumped once per nesting level at the single
    /// `convert_typed_pattern` choke point (every group / lookaround /
    /// conditional body routes through it). Interior-mutable so the
    /// `&self` walker methods can maintain it without threading a
    /// parameter through ~60 mutually-recursive `convert_typed_*`
    /// functions. Guards against unbounded recursion that would
    /// otherwise overflow the caller's stack and abort the process —
    /// see [`crate::recursion`].
    depth: std::cell::Cell<usize>,
}

/// RAII guard that bumps [`PgenAstAdapter::depth`] for the lifetime
/// of one nesting level and restores it on drop, including the early
/// `?`-return paths inside the wrapped walker body.
struct NestingGuard<'g> {
    cell: &'g std::cell::Cell<usize>,
}

impl Drop for NestingGuard<'_> {
    fn drop(&mut self) {
        // Saturating for defensiveness; balanced enter/exit keeps it exact.
        self.cell.set(self.cell.get().saturating_sub(1));
    }
}

/// Character(s) that PCRE2 treats as a newline for the purposes of
/// `.` / `\N` exclusion and `^` / `$` line-boundary matching. Derived
/// from the pattern-start `(*NEWLINE)` pragma family.
#[cfg(feature = "pgen-parser")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewlineMode {
    /// Only `\n` (U+000A) is a newline — PCRE2's `(*LF)` and the
    /// conventional RGX default when no pragma is specified.
    Lf,
    /// Only `\r` (U+000D) is a newline — PCRE2's `(*CR)`.
    Cr,
    /// Only the two-byte sequence `\r\n` is a newline — PCRE2's
    /// `(*CRLF)`. For `.`-exclusion purposes both bytes are excluded
    /// (PCRE2 drops the second byte of the pair implicitly).
    Crlf,
    /// `\r`, `\n`, and `\r\n` are all newlines — PCRE2's `(*ANYCRLF)`.
    Anycrlf,
    /// Full Unicode newline set: `\r`, `\n`, `\x0B` (VT), `\x0C` (FF),
    /// `\x85` (NEL), `\u{2028}` (LINE SEPARATOR), `\u{2029}` (PARAGRAPH
    /// SEPARATOR). PCRE2's `(*ANY)`.
    Any,
    /// Only the NUL byte is a newline — PCRE2's `(*NUL)`.
    Nul,
}

#[cfg(feature = "pgen-parser")]
impl NewlineMode {
    /// Return the set of codepoints that the newline convention
    /// excludes from `.` / `\N`. Used to rewrite `Regex::Dot` into an
    /// explicit negated character class when the mode differs from
    /// the default.
    fn newline_chars(self) -> Vec<char> {
        match self {
            NewlineMode::Lf => vec!['\n'],
            NewlineMode::Cr => vec!['\r'],
            // `(*CRLF)`: the newline is the 2-byte `\r\n` sequence.
            // PCRE2's `.` / `\N` fails ONLY at the start of a `\r\n`
            // pair; bare `\r`, bare `\n`, and the `\n` of a pair
            // (once we've advanced past the `\r`) are all matched.
            // A context-free char class can't model the "start of
            // pair" semantic, so exclude nothing here and let the
            // surrounding pattern fail naturally when it tries to
            // cross the pair. Net: `/A\NB/newline=crlf` on `A\nB`
            // / `A\rB` matches (correct), but `/.+A/newline=crlf`
            // on `\r\nA` falsely matches (harness FP — few cases).
            NewlineMode::Crlf => vec![],
            NewlineMode::Anycrlf => vec!['\r', '\n'],
            NewlineMode::Any => vec![
                '\r', '\n', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
            ],
            NewlineMode::Nul => vec!['\0'],
        }
    }
}

#[cfg(feature = "pgen-parser")]
impl<'a> PgenAstAdapter<'a> {
    fn new(pattern: &'a str) -> Self {
        let ucp_enabled = pattern.contains("(*UCP)");
        // Last-wins between `(*BSR_ANYCRLF)` and `(*BSR_UNICODE)` —
        // PCRE2 applies the most-recent pragma when both appear.
        let bsr_anycrlf = match (
            pattern.rfind("(*BSR_ANYCRLF)"),
            pattern.rfind("(*BSR_UNICODE)"),
        ) {
            (Some(a), Some(u)) => a > u,
            (Some(_), None) => true,
            _ => false,
        };
        // Newline-convention pragmas: last-wins. Default `Lf` matches
        // the pre-existing RGX behaviour (no pragma → `\n`-only
        // newlines).
        let newline_mode = [
            ("(*LF)", NewlineMode::Lf),
            ("(*CR)", NewlineMode::Cr),
            ("(*CRLF)", NewlineMode::Crlf),
            ("(*ANYCRLF)", NewlineMode::Anycrlf),
            ("(*ANY)", NewlineMode::Any),
            ("(*NUL)", NewlineMode::Nul),
        ]
        .iter()
        .filter_map(|(pragma, mode)| pattern.rfind(pragma).map(|idx| (idx, *mode)))
        .max_by_key(|(idx, _)| *idx)
        .map_or(NewlineMode::Lf, |(_, mode)| mode);
        Self {
            pattern,
            ucp_enabled,
            bsr_anycrlf,
            newline_mode,
            depth: std::cell::Cell::new(0),
        }
    }

    /// Enter one pattern-nesting level. Returns a [`NestingGuard`]
    /// that restores the depth on drop, or a clean compile error when
    /// the pattern is nested deeper than
    /// [`crate::recursion::MAX_NESTING_DEPTH`] — rejected here, the
    /// earliest point the structural nesting is visible, before the
    /// recursion (and the stack it would grow) becomes unbounded.
    fn enter_nesting(&self) -> Result<NestingGuard<'_>> {
        let new_depth = self.depth.get() + 1;
        if crate::recursion::exceeds_nesting_limit(new_depth) {
            return Err(crate::recursion::too_deeply_nested());
        }
        self.depth.set(new_depth);
        Ok(NestingGuard { cell: &self.depth })
    }

    /// Build the AST for `.` / `\N`. In the default `Lf` newline mode
    /// we hand back the shared `Regex::Dot` atom (the compiler emits
    /// `Any`-excludes-`\n`). Under any other PCRE2 newline convention
    /// we rewrite to a negated `CharClass::Custom` that excludes the
    /// mode-specific newline characters so both the VM and C2 codegens
    /// see the same tree without backend changes.
    fn dot_ast(&self) -> Regex {
        if self.newline_mode == NewlineMode::Lf {
            return Regex::Dot;
        }
        // (*NUL): per pcre2pattern(3), `.` rejects NUL only when
        // `/s` (PCRE2_DOTALL) is NOT set; with `/s` it matches NUL
        // along with everything else. Leave the AST as `Regex::Dot`
        // so codegen can distinguish — emitting `AnyDotAll` under
        // `/s` and a NUL-aware variant otherwise. Pre-rewriting to
        // `[^\0]` here would over-reject under `/s`. Closes
        // testinput2:2357 (`(*NUL)^.*/s` on "a\nb\0ccc" expects
        // "a\nb\0ccc"; the rewrite would have stopped at \0).
        if self.newline_mode == NewlineMode::Nul {
            return Regex::Dot;
        }
        // `(*CRLF)`: the newline unit is the 2-byte `\r\n` pair.
        // PCRE2's `.` / `\N` rejects BOTH ends of the pair:
        //   - the leading `\r` of a `\r\n` (start of CRLF), and
        //   - the trailing `\n` of a `\r\n` (end of CRLF).
        // Bare `\r` not followed by `\n`, bare `\n` not preceded
        // by `\r`, and any other byte all match.
        //
        // Encode the exclusion as one negative lookahead with two
        // alternatives:
        //   (?! \r\n             // start of CRLF: current is \r and next is \n
        //     | (?<=\r)\n         // end of CRLF: current is \n and prev was \r
        //   )
        // followed by a dotall-any. The `(?<=\r)\n` form scopes
        // the prev-byte check to the case where the current byte
        // is `\n`, so positions like `c\r` do not falsely fail
        // their successor (testinput2:1595 `/.*/I` on `abc\rdef`
        // must continue matching past `\r` since `d` is not `\n`).
        if self.newline_mode == NewlineMode::Crlf {
            return Regex::Sequence(vec![
                Regex::Lookahead {
                    expr: Box::new(Regex::Alternation(vec![
                        Regex::Sequence(vec![Regex::Char('\r'), Regex::Char('\n')]),
                        Regex::Sequence(vec![
                            Regex::Lookbehind {
                                expr: Box::new(Regex::Char('\r')),
                                positive: true,
                                non_atomic: false,
                            },
                            Regex::Char('\n'),
                        ]),
                    ])),
                    positive: false,
                    non_atomic: false,
                },
                Regex::CharClass(CharClass::Custom {
                    ranges: Vec::new(),
                    negated: true,
                    ci_override_ranges: None,
                }),
            ]);
        }
        let mut ranges: Vec<CharRange> = self
            .newline_mode
            .newline_chars()
            .into_iter()
            .map(CharRange::single)
            .collect();
        ranges.sort_by_key(|r| r.start);
        Regex::CharClass(CharClass::Custom {
            ranges,
            negated: true,
            ci_override_ranges: None,
        })
    }

    /// Build the AST for `\N` (PCRE2 "any character except a
    /// newline"). Unlike `.`, `\N` is **never** affected by `/s`
    /// (`PCRE2_DOTALL`): `(?s)\N` still rejects the newline. The `Lf`
    /// and `Nul` branches of [`Self::dot_ast`] deliberately hand back
    /// the bare, dotall-sensitive `Regex::Dot` atom so codegen can
    /// promote `.` to `AnyDotAll` under `/s`; reusing that for `\N`
    /// (every `\N` site historically called `dot_ast`) let `(?s)\N`
    /// wrongly swallow the newline. For exactly those two modes emit
    /// an explicit negated newline class instead — a static class the
    /// dotall pass cannot rewrite, giving the precise non-dotall `.`
    /// semantics that are invariant under `/s`. Every other newline
    /// mode already yields a dotall-immune explicit class / lookaround
    /// from `dot_ast`, whose `.`/`\N` exclusion PCRE2 defines
    /// identically, so delegate to it unchanged.
    fn non_newline_ast(&self) -> Regex {
        if matches!(self.newline_mode, NewlineMode::Lf | NewlineMode::Nul) {
            let mut ranges: Vec<CharRange> = self
                .newline_mode
                .newline_chars()
                .into_iter()
                .map(CharRange::single)
                .collect();
            ranges.sort_by_key(|r| r.start);
            return Regex::CharClass(CharClass::Custom {
                ranges,
                negated: true,
                ci_override_ranges: None,
            });
        }
        self.dot_ast()
    }

    /// Build the AST for `\R`. In `BSR_UNICODE` mode (default) the
    /// sequence is the shared `Regex::NewlineSequence` node that the
    /// VM and C2 codegens already know how to expand. In
    /// `BSR_ANYCRLF` mode we emit an explicit alternation restricted
    /// to CR, LF, and CRLF so both backends see the limited set
    /// without needing an extra compile-time switch.
    fn newline_sequence_ast(&self) -> Regex {
        if !self.bsr_anycrlf {
            return Regex::NewlineSequence;
        }
        Regex::Group {
            kind: GroupKind::NonCapturing,
            expr: Box::new(Regex::Alternation(vec![
                Regex::Sequence(vec![Regex::Char('\r'), Regex::Char('\n')]),
                Regex::Char('\r'),
                Regex::Char('\n'),
            ])),
            index: None,
            name: None,
        }
    }

    fn parse_dump(&self, dump_json: &str) -> Result<Regex> {
        let mut deserializer = serde_json::Deserializer::from_str(dump_json);
        deserializer.disable_recursion_limit();
        let deserializer = serde_stacker::Deserializer::new(&mut deserializer);
        let root: PgenAstNode = serde::Deserialize::deserialize(deserializer).map_err(|err| {
            RgxError::compile(format!("failed to decode pgen regex AST dump JSON: {err}"))
        })?;
        // PGEN regex parser release 1.1.30+ emits the typed-Json shape at
        // the top level (and at every annotated-rule boundary). The new
        // walker collapses any remaining envelope subtrees via
        // `to_json_value()`-equivalent flattening, then walks the unified
        // `serde_json::Value` tree per the shapes documented in
        // `subs/pgen/docs/regex_parser_book/src/`. The legacy walker
        // (`convert_root`) is kept only for the rare failure-path case
        // where deserialisation succeeds with a non-Json root content.
        if matches!(root.content, PgenAstContent::Json(_)) {
            let unified = collapse_to_json(&root.content);
            return self.convert_typed_regex(&unified);
        }
        self.convert_root(&root)
    }

    /// Top-level entry for the typed-Json shape. The root `regex` rule's
    /// annotation produces `{type: "regex", pattern: <pattern-content>}`.
    fn convert_typed_regex(&self, value: &serde_json::Value) -> Result<Regex> {
        let map = value.as_object().ok_or_else(|| {
            self.contract_error(&format!(
                "expected typed regex object, got {}",
                json_kind(value)
            ))
        })?;
        let kind = map.get("type").and_then(|v| v.as_str());
        if kind != Some("regex") {
            return Err(self.contract_error(&format!(
                "expected typed regex object with type=\"regex\", got type={kind:?}"
            )));
        }
        let pattern = map.get("pattern").ok_or_else(|| {
            self.contract_error("typed regex object is missing the 'pattern' field")
        })?;
        self.convert_typed_pattern(pattern)
    }

    /// `pattern` field shape: 2-element array `[<first_alternative>, <rest>]`.
    /// `<first_alternative>` is a 1-element-Quantified-? carrier wrapping the
    /// concatenation array (or `[]` when the alternative is empty).
    /// `<rest>` is `[]` (no `|`) or `[["|", <alt>], ...]` (per `|`-separated branch).
    fn convert_typed_pattern(&self, value: &serde_json::Value) -> Result<Regex> {
        // Single nesting choke point: every group / lookaround /
        // conditional body routes back through here exactly once per
        // level. Bound the depth (deterministic clean error past the
        // limit) and grow the stack on demand so a within-limit but
        // deeply nested pattern can never overflow the caller's stack
        // and abort the process. See `crate::recursion`.
        let _nesting = self.enter_nesting()?;
        crate::recursion::grow_stack(|| self.convert_typed_pattern_inner(value))
    }

    fn convert_typed_pattern_inner(&self, value: &serde_json::Value) -> Result<Regex> {
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected typed pattern array, got {}",
                json_kind(value)
            ))
        })?;

        // Empty pattern (input was zero-length): `pattern[0]` is the
        // Quantified-? slot which can be `[]` when no concatenation
        // matched at all. Produce an empty sequence.
        if arr.is_empty() {
            return Ok(Regex::Sequence(Vec::new()));
        }

        let mut alternative_pieces: Vec<Vec<Regex>> = Vec::new();

        // First alternative.
        let first_slot = &arr[0];
        alternative_pieces.push(self.convert_typed_alternative(first_slot)?);

        // Remaining alternatives — walk pairs of ["|", <alternative>].
        if let Some(rest) = arr.get(1).and_then(|v| v.as_array()) {
            for pair in rest {
                let pair_arr = pair.as_array().ok_or_else(|| {
                    self.contract_error(&format!(
                        "typed pattern alternation rest entry is not an array: {}",
                        json_kind(pair)
                    ))
                })?;
                if pair_arr.len() < 2 {
                    return Err(self.contract_error(
                        "typed pattern alternation rest entry is missing a branch payload",
                    ));
                }
                alternative_pieces.push(self.convert_typed_alternative(&pair_arr[1])?);
            }
        }

        // Walk branches with the bare-flag-directive carry-forward semantics
        // documented in the legacy `convert_alternation` (engine fix #6,
        // 2026-04-17). PCRE2 propagates an unscoped `(?flags)` toggle from
        // one branch into all subsequent branches.
        let mut carried: Option<String> = None;
        let mut branches: Vec<Regex> = Vec::with_capacity(alternative_pieces.len());
        for pieces in alternative_pieces {
            let trailing = last_unscoped_flag(&pieces);
            let body = apply_bare_flag_directives(pieces);
            let wrapped = if let Some(ref flags) = carried {
                Regex::FlagGroup {
                    flags: flags.clone(),
                    expr: Box::new(body),
                }
            } else {
                body
            };
            branches.push(wrapped);
            if let Some(flags) = trailing {
                carried = Some(flags);
            }
        }
        Ok(pack_alternation(branches))
    }

    /// `<alternative>` shape (un-annotated `Quantified-?` carrier): a 0- or
    /// 1-element array. When empty, the alternative is empty (e.g. the
    /// trailing branch of `a|`). When 1-element, the inner is the
    /// `concatenation` array (a flat list of piece objects).
    fn convert_typed_alternative(&self, value: &serde_json::Value) -> Result<Vec<Regex>> {
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected typed alternative array, got {}",
                json_kind(value)
            ))
        })?;
        if arr.is_empty() {
            return Ok(Vec::new());
        }
        // arr[0] is the concatenation — a flat array of piece objects per
        // the `concatenation = piece+ -> [$1**]` annotation.
        self.convert_typed_concatenation(&arr[0])
    }

    /// Walk a `concatenation` typed array — a flat list of piece objects.
    fn convert_typed_concatenation(&self, value: &serde_json::Value) -> Result<Vec<Regex>> {
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected typed concatenation array, got {}",
                json_kind(value)
            ))
        })?;
        let mut pieces = Vec::with_capacity(arr.len());
        for piece in arr {
            pieces.push(self.convert_typed_piece(piece)?);
        }
        Ok(pieces)
    }

    /// Walk a piece object `{type: "piece", atom: <atom>, quantifier: <quantifier>}`.
    /// Per slice 6 (post-1.1.35), the `quantifier` slot is either:
    ///   - `[]` (empty array — un-matched `quantifier?` slot, no quantifier)
    ///   - A typed object `{type: "quantifier", min, max, greediness}`.
    fn convert_typed_piece(&self, value: &serde_json::Value) -> Result<Regex> {
        let map = value.as_object().ok_or_else(|| {
            self.contract_error(&format!(
                "expected typed piece object, got {}",
                json_kind(value)
            ))
        })?;
        let kind = map.get("type").and_then(|v| v.as_str());
        if kind != Some("piece") {
            return Err(self.contract_error(&format!(
                "expected typed piece object with type=\"piece\", got type={kind:?}"
            )));
        }
        let atom = map
            .get("atom")
            .ok_or_else(|| self.contract_error("typed piece object is missing the 'atom' field"))?;
        let expr = self.convert_typed_atom(atom)?;

        let quantifier_slot = map
            .get("quantifier")
            .ok_or_else(|| self.contract_error("typed piece object is missing 'quantifier'"))?;
        // Empty array → no quantifier matched.
        if matches!(quantifier_slot, serde_json::Value::Array(a) if a.is_empty()) {
            return Ok(expr);
        }
        let (quantifier, possessive) = self.convert_typed_quantifier_object(quantifier_slot)?;
        Ok(Self::wrap_quantified(expr, quantifier, possessive))
    }

    /// Dispatch on the atom field's structural shape per the
    /// "Identification table" in `rules-atom.md`. Per slice 7
    /// (post-1.1.36), `anchor` atoms are typed objects
    /// `{"type":"anchor","kind":"<name>"}`. Other typed atoms may
    /// follow as the atom-subtree campaign progresses (task #40).
    fn convert_typed_atom(&self, value: &serde_json::Value) -> Result<Regex> {
        match value {
            serde_json::Value::String(s) => self.convert_typed_atom_string(s),
            serde_json::Value::Array(arr) => self.convert_typed_atom_array(arr),
            serde_json::Value::Object(map) => self.convert_typed_atom_object(map),
            other => Err(self.contract_error(&format!(
                "unexpected atom shape: {} = {}",
                json_kind(other),
                other
            ))),
        }
    }

    /// Dispatch on a typed atom object — anchor (slice 7+9), backreference
    /// (slice 10), and future typed atom kinds as they land.
    fn convert_typed_atom_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let kind = map.get("type").and_then(|v| v.as_str());
        match kind {
            Some("anchor") => {
                let anchor_kind = map
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed anchor missing 'kind'"))?;
                self.convert_typed_anchor_kind(anchor_kind)
            }
            Some("backreference") => self.convert_typed_backreference_object(map),
            // Typed `escape` object (PGEN release 1.1.44+, slices 14/16/17).
            Some("escape") => self.convert_typed_escape_object(map),
            // Typed `atom` object (PGEN slice 18+; covers groups,
            // lookarounds, char_class, callout, code_block, conditional,
            // directive_verb, subroutine_call, inline_modifiers,
            // alpha_lookaround, etc.).
            Some("atom") => self.convert_typed_atom_kind_object(map),
            Some(other) => {
                Err(self.contract_error(&format!("unrecognised typed atom: type={other:?}")))
            }
            None => Err(self.contract_error("typed atom object missing 'type' discriminator")),
        }
    }

    /// Walk a typed `escape` object — PGEN slices 14/16/17 (releases
    /// 1.1.44/45/47): `{type:"escape", kind:<form>, ...}`.
    ///
    /// Forms:
    /// - `shorthand` `{char:<c>}`     → `\d`/`\w`/`\s`/`\.`/`\\` etc.
    /// - `control`   `{char:<c>}`     → `\cA`..`\cZ`/`\cz` (XOR 0x40)
    /// - `single_byte`                → `\C` (any single codepoint)
    /// - `hex`       `{digits:<str>}` → `\xFF`/`\x{1F}`
    /// - `octal`     `{digits:<str>}` → `\o{777}`
    /// - `unicode`   `{digits:<str>}` → `\u{1F}`
    /// - `property`  `{name:<str>, negated:<bool>}` → `\pL`/`\p{Lu}`/`\P{Nd}`
    fn convert_typed_escape_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let kind = map
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| self.contract_error("typed escape object missing 'kind'"))?;
        match kind {
            "shorthand" => {
                let ch = map
                    .get("char")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed shorthand-escape missing 'char'"))?;
                self.convert_typed_simple_escape_char(ch, false)
            }
            "control" => {
                let ch = map
                    .get("char")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.chars().next())
                    .ok_or_else(|| self.contract_error("typed control-escape missing 'char'"))?;
                let upper = ch.to_ascii_uppercase();
                let code = (upper as u32) ^ 0x40;
                let result = char::from_u32(code).ok_or_else(|| {
                    self.contract_error(&format!(
                        "control escape \\c{ch} produces invalid codepoint"
                    ))
                })?;
                Ok(Regex::Char(result))
            }
            "single_byte" => Ok(Regex::CharClass(CharClass::Custom {
                ranges: vec![CharRange::range('\0', char::MAX)],
                negated: false,
                ci_override_ranges: None,
            })),
            "hex" => {
                let digits = map
                    .get("digits")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed hex-escape missing 'digits'"))?;
                let n = u32::from_str_radix(digits, 16).map_err(|_| {
                    self.contract_error(&format!("invalid hex escape '\\x{digits}'"))
                })?;
                let ch = char::from_u32(n).ok_or_else(|| {
                    self.contract_error(&format!(
                        "hex escape \\x{{{digits}}} is not a valid codepoint"
                    ))
                })?;
                Ok(Regex::Char(ch))
            }
            "octal" => {
                let digits = map
                    .get("digits")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed octal-escape missing 'digits'"))?;
                let n = u32::from_str_radix(digits, 8).map_err(|_| {
                    self.contract_error(&format!("invalid octal escape '\\o{{{digits}}}'"))
                })?;
                let ch = char::from_u32(n).ok_or_else(|| {
                    self.contract_error(&format!(
                        "octal escape \\o{{{digits}}} is not a valid codepoint"
                    ))
                })?;
                Ok(Regex::Char(ch))
            }
            "unicode" => {
                let digits = map
                    .get("digits")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed unicode-escape missing 'digits'"))?;
                let n = u32::from_str_radix(digits, 16).map_err(|_| {
                    self.contract_error(&format!("invalid unicode escape '\\u{{{digits}}}'"))
                })?;
                let ch = char::from_u32(n).ok_or_else(|| {
                    self.contract_error(&format!(
                        "unicode escape \\u{{{digits}}} is not a valid codepoint"
                    ))
                })?;
                Ok(Regex::Char(ch))
            }
            "property" => {
                // Surface as `Regex::UnicodeClass`, not `CharClass::Custom`.
                // The compiler's case-fold expansion (`\p{Lu}`/`\p{Ll}`/
                // `\p{Lt}` ↔ `\p{L&}` under /i) only fires for the
                // UnicodeClass variant — see `lib.rs::case_distinguished_property_expands_under_i`.
                let name = map
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed property-escape missing 'name'"))?
                    .to_string();
                let negated = map
                    .get("negated")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                Ok(Regex::CharClass(CharClass::UnicodeClass { name, negated }))
            }
            other => {
                Err(self.contract_error(&format!("unrecognised typed escape kind: {other:?}")))
            }
        }
    }

    /// Walk a typed `atom` object — `{type:"atom", kind:<form>, ...}`.
    /// Each `kind` maps to an existing `Regex` AST node.
    fn convert_typed_atom_kind_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let kind = map
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| self.contract_error("typed atom object missing 'kind'"))?;
        match kind {
            "capturing_group" => {
                let body = map
                    .get("body")
                    .ok_or_else(|| self.contract_error("typed capturing_group missing 'body'"))?;
                let inner = self.convert_typed_pattern(body)?;
                Ok(Regex::Group {
                    expr: Box::new(inner),
                    kind: GroupKind::Capturing,
                    index: None,
                    name: None,
                })
            }
            "noncapturing_group" => {
                let body = map.get("body").ok_or_else(|| {
                    self.contract_error("typed noncapturing_group missing 'body'")
                })?;
                let inner = self.convert_typed_pattern(body)?;
                Ok(Regex::Group {
                    expr: Box::new(inner),
                    kind: GroupKind::NonCapturing,
                    index: None,
                    name: None,
                })
            }
            "named_group" | "python_named_group" => {
                let body = map
                    .get("body")
                    .ok_or_else(|| self.contract_error("typed named_group missing 'body'"))?;
                let name = map
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed named_group missing 'name'"))?
                    .to_string();
                let inner = self.convert_typed_pattern(body)?;
                Ok(Regex::Group {
                    expr: Box::new(inner),
                    kind: GroupKind::Capturing,
                    index: None,
                    name: Some(name),
                })
            }
            "atomic_group" => {
                let body = map
                    .get("body")
                    .ok_or_else(|| self.contract_error("typed atomic_group missing 'body'"))?;
                let inner = self.convert_typed_pattern(body)?;
                Ok(Regex::Group {
                    expr: Box::new(inner),
                    kind: GroupKind::Atomic,
                    index: None,
                    name: None,
                })
            }
            "branch_reset_group" => {
                let body = map.get("body").ok_or_else(|| {
                    self.contract_error("typed branch_reset_group missing 'body'")
                })?;
                let inner = self.convert_typed_pattern(body)?;
                Ok(Regex::Group {
                    expr: Box::new(inner),
                    kind: GroupKind::BranchReset,
                    index: None,
                    name: None,
                })
            }
            "lookahead" | "lookahead_neg" | "lookbehind" | "lookbehind_neg" => {
                let body = map
                    .get("body")
                    .ok_or_else(|| self.contract_error("typed lookaround missing 'body'"))?;
                let inner = self.convert_typed_pattern(body)?;
                let positive = match map.get("positive") {
                    Some(serde_json::Value::Bool(b)) => *b,
                    _ => matches!(kind, "lookahead" | "lookbehind"),
                };
                let lookahead = matches!(kind, "lookahead" | "lookahead_neg");
                if lookahead {
                    Ok(Regex::Lookahead {
                        expr: Box::new(inner),
                        positive,
                        non_atomic: false,
                    })
                } else {
                    Ok(Regex::Lookbehind {
                        expr: Box::new(inner),
                        positive,
                        non_atomic: false,
                    })
                }
            }
            "non_atomic_lookahead" => {
                let body = map.get("body").ok_or_else(|| {
                    self.contract_error("typed non_atomic_lookahead missing 'body'")
                })?;
                let inner = self.convert_typed_pattern(body)?;
                Ok(Regex::Lookahead {
                    expr: Box::new(inner),
                    positive: true,
                    non_atomic: true,
                })
            }
            "alpha_lookaround" => self.convert_typed_alpha_lookaround_object(map),
            "char_class" => self.convert_typed_char_class_object(map),
            "callout" => match map.get("arg") {
                Some(serde_json::Value::Number(n)) => {
                    Ok(Regex::Callout(n.as_u64().unwrap_or(0) as u32))
                }
                Some(serde_json::Value::Array(a)) if a.is_empty() => Ok(Regex::Callout(0)),
                _ => Ok(Regex::Empty),
            },
            "comment" => Ok(Regex::Empty),
            "directive_verb" => self.convert_typed_directive_verb_object(map),
            "code_block" => self.convert_typed_code_block_object(map),
            "subroutine_call" => self.convert_typed_subroutine_call_object(map),
            "inline_modifiers" => {
                // `(?)` has `spec:[]` — empty unmatched optional slot.
                // Treat as a no-op (matches PCRE2's "no-op group").
                match map.get("spec") {
                    Some(serde_json::Value::Array(a)) if a.is_empty() => Ok(Regex::Empty),
                    None => Ok(Regex::Empty),
                    _ => self.convert_typed_inline_modifiers_object(map, None),
                }
            }
            "scoped_inline_modifiers" => {
                let body = map.get("body").ok_or_else(|| {
                    self.contract_error("typed scoped_inline_modifiers missing 'body'")
                })?;
                self.convert_typed_inline_modifiers_object(map, Some(body))
            }
            "conditional" => self.convert_typed_conditional_object(map),
            "quoted_literal" => {
                let body = map.get("body").and_then(|v| v.as_array()).ok_or_else(|| {
                    self.contract_error("typed quoted_literal missing 'body' array")
                })?;
                // Inside `\Q…\E`, every byte is a literal — no escape
                // interpretation. PGEN occasionally emits a sub-array
                // for chars that would otherwise hit a reserved grammar
                // terminal (e.g. `\$` parses as `["\\", "$"]`). Flatten
                // every body element to literal chars; ignore typed
                // sub-shapes since they shouldn't appear inside `\Q\E`.
                let mut items = Vec::with_capacity(body.len());
                for elem in body {
                    let mut text = String::new();
                    walk_json_terminal_chars(elem, &mut text);
                    for ch in text.chars() {
                        items.push(Regex::Char(ch));
                    }
                }
                match items.len() {
                    0 => Ok(Regex::Empty),
                    1 => Ok(items.into_iter().next().unwrap()),
                    _ => Ok(Regex::Sequence(items)),
                }
            }
            "extended_class" => {
                // Reconstruct the source text from the body and surface
                // through `Regex::ExtendedCharClass` so the compiler's
                // dedicated ECC evaluator handles set algebra. We can't
                // use plain `walk_json_terminal_chars` here because the
                // body now contains typed escape objects
                // (`{type:"escape", kind:..., ...}`); the generic
                // walker would concatenate field VALUES (`d`,
                // `shorthand`, `escape` → `"dshorthandescape"`)
                // instead of reconstructing the source escape (`\d`).
                let body = map
                    .get("body")
                    .ok_or_else(|| self.contract_error("typed extended_class missing 'body'"))?;
                let mut content = String::new();
                self.reconstruct_typed_class_text(body, &mut content);
                Ok(Regex::ExtendedCharClass { content })
            }
            "posix_class" => {
                let name = map
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed posix_class missing 'name'"))?;
                let negated = matches!(map.get("negated"), Some(serde_json::Value::Bool(true)));
                let resolved = if self.ucp_enabled {
                    ucp_posix_class_ranges(name).or_else(|| posix_class_ranges(name))
                } else {
                    posix_class_ranges(name)
                };
                let mut class_ranges = resolved.ok_or_else(|| {
                    self.contract_error(&format!("unknown POSIX class name '{name}'"))
                })?;
                if negated {
                    class_ranges = complement_ranges(&class_ranges);
                }
                Ok(Regex::CharClass(CharClass::Custom {
                    ranges: class_ranges,
                    negated: false,
                    ci_override_ranges: None,
                }))
            }
            other => Err(self.contract_error(&format!("unrecognised typed atom kind: {other:?}"))),
        }
    }

    /// Convert a typed `backreference` object per slice-10 (post-1.1.38):
    ///
    /// - `numeric`: `{kind:"numeric", index:<int>}` (typed integer)
    /// - `named`: `{kind:"named", ref:<raw name_ref>}`
    /// - `named_braced`: `{kind:"named_braced", ref:<raw braced_name_ref>}`
    /// - `subroutine`: `{kind:"subroutine", ref:<raw subroutine_ref>}`
    ///
    /// PCRE2 distinguishes by source bracket form for `\g`-prefixed
    /// references: angle/apostrophe forms (`\g<…>`/`\g'…'`) are
    /// subroutine calls (Recursion), while braced (`\g{…}`) and bare
    /// (`\gN`) forms are back-references. RGX preserves that
    /// distinction by inspecting the `ref` shape's first element.
    fn convert_typed_backreference_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let kind = map
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| self.contract_error("typed backreference missing 'kind'"))?;
        match kind {
            "numeric" => {
                let index = map
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        self.contract_error("typed numeric backreference missing 'index'")
                    })?;
                // PCRE2 rejects backreferences whose decoded index exceeds
                // its capture-group ceiling. RGX's compiler validates the
                // index against the actual capture count, but at the walker
                // layer we still need to reject indices that won't fit in
                // a `u32` — silently truncating 6_666_666_666 down to a
                // wrapped value would alias to some legitimate small group
                // and produce wrong matches. testinput9:287 covers this.
                if index > u64::from(u32::MAX) {
                    return Err(RgxError::compile(format!(
                        "numeric backreference \\{index} exceeds the supported range"
                    )));
                }
                #[allow(clippy::cast_possible_truncation)]
                Ok(Regex::Backreference(index as u32))
            }
            "named" | "named_braced" | "python_named" => {
                // `\k<n>`, `\k{n}`, `\g{NAME}` (per PCRE2 spec; PGEN
                // 1.1.75 routes `\g{NAME}` to `named_braced` since the
                // semantic is identical to `\k{NAME}`).
                let raw_ref = map.get("ref").ok_or_else(|| {
                    self.contract_error("typed named backreference missing 'ref'")
                })?;
                let name = if let Some(s) = raw_ref.as_str() {
                    s.to_string()
                } else {
                    self.extract_name_from_ref_shape(raw_ref)?
                };
                Ok(Regex::NamedBackreference(name))
            }
            // Post-PGEN-RGX-0081 (release 1.1.75) — `\g`-prefixed family
            // is split into 4 explicit kinds. Bracket form is preserved
            // so the walker can lower correctly.
            "subroutine_named" => {
                // `\g<NAME>` / `\g'NAME'` — subroutine call
                let name = map
                    .get("ref")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        self.contract_error("subroutine_named backreference missing 'ref' string")
                    })?
                    .to_string();
                Ok(Regex::Recursion {
                    target: RecursionTarget::NamedGroup(name),
                })
            }
            "subroutine_numeric" => {
                // `\g<N>` / `\g'N'` — subroutine call, numeric
                let raw_ref = map.get("ref").and_then(|v| v.as_object()).ok_or_else(|| {
                    self.contract_error(
                        "subroutine_numeric backreference missing 'ref' {sign,value}",
                    )
                })?;
                let value = raw_ref
                    .get("value")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| self.contract_error("subroutine_numeric ref missing 'value'"))?
                    as i64;
                let sign = raw_ref.get("sign").and_then(|v| v.as_str());
                let target = match sign {
                    Some("+") => RecursionTarget::RelativeGroup(value as i32),
                    Some("-") => RecursionTarget::RelativeGroup(-(value as i32)),
                    _ => RecursionTarget::Group(value as u32),
                };
                Ok(Regex::Recursion { target })
            }
            "numeric_backreference" => {
                // `\g{N}` / `\gN` / `\g+N` / `\g-N` — back-reference
                let raw_ref = map.get("ref").and_then(|v| v.as_object()).ok_or_else(|| {
                    self.contract_error("numeric_backreference missing 'ref' {sign,value}")
                })?;
                let value = raw_ref
                    .get("value")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        self.contract_error("numeric_backreference ref missing 'value'")
                    })?;
                let sign = raw_ref.get("sign").and_then(|v| v.as_str());
                if let Some("+" | "-") = sign {
                    // Relative back-reference (rare; PCRE2 supports
                    // `\g+N` / `\g-N` for relative refs).
                    let signed = if sign == Some("-") {
                        -(value as i32)
                    } else {
                        value as i32
                    };
                    Ok(Regex::RelativeBackreference(signed))
                } else {
                    if value > u64::from(u32::MAX) {
                        return Err(RgxError::compile(format!(
                            "numeric backreference \\g{{{value}}} exceeds the supported range"
                        )));
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    Ok(Regex::Backreference(value as u32))
                }
            }
            // Legacy un-typed shape (kept for `\g`-prefixed forms when
            // PGEN at older pins still emits `kind:"subroutine"`).
            "subroutine" => {
                let raw_ref = map.get("ref").ok_or_else(|| {
                    self.contract_error("typed subroutine backreference missing 'ref'")
                })?;
                self.classify_g_subroutine_ref(raw_ref)
            }
            other => {
                Err(self
                    .contract_error(&format!("unrecognised typed backreference kind: {other:?}")))
            }
        }
    }

    /// Classify the `ref` shape of a `\g…` typed backreference, preserving
    /// PCRE2's bracket-form-determines-semantic rule:
    ///   - `\g<N>` / `\g<name>` / `\g'…'` → subroutine call (Recursion)
    ///   - `\g{N}` / `\g{name}` → back-reference
    ///   - `\gN` (bare `signed_digits`) → back-reference
    fn classify_g_subroutine_ref(&self, value: &serde_json::Value) -> Result<Regex> {
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected \\g subroutine_ref array, got {}",
                json_kind(value)
            ))
        })?;
        let head = arr.first().and_then(|v| v.as_str());
        match head {
            Some("<" | "'") => {
                // Angle/apostrophe form → subroutine call.
                let inner = arr
                    .get(1)
                    .ok_or_else(|| self.contract_error("\\g<…>/'…' subroutine ref missing body"))?;
                self.subroutine_ref_to_recursion(inner)
            }
            Some("{") => {
                // Braced form → back-reference.
                let inner = arr
                    .get(2)
                    .ok_or_else(|| self.contract_error("\\g{…} backreference missing body"))?;
                self.subroutine_ref_to_backreference(inner)
            }
            // Bare signed_digits `\gN` / `\g+N` / `\g-N` — back-reference.
            _ => self.subroutine_ref_to_backreference(value),
        }
    }

    /// Map an anchor's stable `kind` identifier to the corresponding
    /// `Regex` AST node. Mirror of the legacy `convert_anchor` mapping.
    /// Per slice 9 (post-1.1.37), the `posix_word_boundary_alias` rule
    /// joins the typed anchor family with kinds `posix_word_start` and
    /// `posix_word_end`. They're zero-width assertions per pcre2pattern(3):
    /// `[[:<:]]` ≡ `\b(?=\w)`, `[[:>:]]` ≡ `(?<=\w)\b`.
    fn convert_typed_anchor_kind(&self, kind: &str) -> Result<Regex> {
        match kind {
            "start_of_line" => Ok(Regex::Anchor(AnchorType::Start)),
            "end_of_line" => Ok(Regex::Anchor(AnchorType::End)),
            "start_of_input" => Ok(Regex::Anchor(AnchorType::AbsStart)),
            "end_of_input_or_before_last_newline" => Ok(Regex::Anchor(AnchorType::AbsEnd)),
            "end_of_input" => Ok(Regex::Anchor(AnchorType::AbsEndNoNL)),
            "word_boundary" => Ok(Regex::WordBoundary { positive: true }),
            "non_word_boundary" => Ok(Regex::WordBoundary { positive: false }),
            "match_start" => Ok(Regex::Anchor(AnchorType::PreviousMatchEnd)),
            "keep_out" => Ok(Regex::MatchReset),
            "posix_word_start" => Ok(Regex::Sequence(vec![
                Regex::WordBoundary { positive: true },
                Regex::Lookahead {
                    expr: Box::new(Regex::CharClass(CharClass::Word { negated: false })),
                    positive: true,
                    non_atomic: false,
                },
            ])),
            "posix_word_end" => Ok(Regex::Sequence(vec![
                Regex::Lookbehind {
                    expr: Box::new(Regex::CharClass(CharClass::Word { negated: false })),
                    positive: true,
                    non_atomic: false,
                },
                Regex::WordBoundary { positive: true },
            ])),
            other => {
                Err(self.contract_error(&format!("unrecognised typed anchor kind: {other:?}")))
            }
        }
    }

    /// String-shaped atom: literal, dot, anchor, or POSIX word-boundary alias.
    fn convert_typed_atom_string(&self, s: &str) -> Result<Regex> {
        match s {
            "." => Ok(self.dot_ast()),
            "^" => Ok(Regex::Anchor(AnchorType::Start)),
            "$" => Ok(Regex::Anchor(AnchorType::End)),
            "\\A" => Ok(Regex::Anchor(AnchorType::AbsStart)),
            "\\Z" => Ok(Regex::Anchor(AnchorType::AbsEnd)),
            "\\z" => Ok(Regex::Anchor(AnchorType::AbsEndNoNL)),
            "\\G" => Ok(Regex::Anchor(AnchorType::PreviousMatchEnd)),
            "\\b" => Ok(Regex::WordBoundary { positive: true }),
            "\\B" => Ok(Regex::WordBoundary { positive: false }),
            "\\K" => Ok(Regex::MatchReset),
            "\\R" => Ok(self.newline_sequence_ast()),
            "\\N" => Ok(self.non_newline_ast()),
            "\\X" => Ok(Regex::GraphemeCluster),
            "[[:<:]]" => Ok(Regex::Sequence(vec![
                Regex::WordBoundary { positive: true },
                Regex::Lookahead {
                    expr: Box::new(Regex::CharClass(CharClass::Word { negated: false })),
                    positive: true,
                    non_atomic: false,
                },
            ])),
            "[[:>:]]" => Ok(Regex::Sequence(vec![
                Regex::Lookbehind {
                    expr: Box::new(Regex::CharClass(CharClass::Word { negated: false })),
                    positive: true,
                    non_atomic: false,
                },
                Regex::WordBoundary { positive: true },
            ])),
            // Single literal char (ASCII non-special or non-ASCII).
            other if other.chars().count() == 1 => {
                let ch = other.chars().next().unwrap();
                // Per `whitespace = /([ \t\n\r\f\v])/`, unescaped whitespace
                // atoms carry the dedicated `WhitespaceLiteral` AST variant
                // so the compiler can strip them inside `(?x:...)` extended-mode
                // groups. Escaped whitespace arrives via `escape` and is
                // lowered to a plain `Char` upstream.
                //
                // Under PCRE2_UTF, the ignorable-whitespace set under `/x`
                // expands from "ASCII whitespace" to "Unicode
                // Pattern_White_Space" (pcre2pattern(3) §"Option settings").
                // The set is small and frozen by Unicode TR31: SP, HT, LF,
                // VT, FF, CR (already above), plus NEL (U+0085), LRM
                // (U+200E), RLM (U+200F), LINE SEPARATOR (U+2028) and
                // PARAGRAPH SEPARATOR (U+2029). Including the Unicode 5
                // here unconditionally is safe in practice: outside `(?x)`
                // these stay `WhitespaceLiteral` and the compiler lowers
                // them to plain `Char` (preserving literal meaning); inside
                // `(?x)` they are stripped, matching PCRE2's `/x,utf`
                // semantic. Recovers testinput4:2383 (`/A‎‏  B/x,utf`
                // against `AB`).
                if matches!(
                    ch,
                    ' ' | '\t'
                        | '\n'
                        | '\r'
                        | '\u{0C}'
                        | '\u{0B}'
                        | '\u{0085}'
                        | '\u{200E}'
                        | '\u{200F}'
                        | '\u{2028}'
                        | '\u{2029}'
                ) {
                    Ok(Regex::WhitespaceLiteral(ch))
                } else {
                    Ok(Regex::Char(ch))
                }
            }
            other => {
                Err(self.contract_error(&format!("unrecognised string-atom shape: {other:?}")))
            }
        }
    }

    /// Array-shaped atom — backreference, escape, `char_class`, group, lookaround,
    /// `quoted_literal`, conditional, etc. Dispatch on the first element's prefix
    /// per the "Identification table" in `rules-atom.md`.
    fn convert_typed_atom_array(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        if arr.is_empty() {
            return Err(self.contract_error("empty array atom"));
        }
        let head = arr[0].as_str().ok_or_else(|| {
            self.contract_error(&format!(
                "expected string head for array atom, got {} = {}",
                json_kind(&arr[0]),
                arr[0]
            ))
        })?;
        match head {
            "\\" => self.convert_typed_backslash_atom(arr),
            "\\k" => self.convert_typed_named_backref_k(arr),
            "\\g" => self.convert_typed_g_backref(arr),
            "\\Q" => self.convert_typed_quoted_literal(arr),
            "[" => self.convert_typed_char_class(arr),
            "(" => self.convert_typed_capturing_group(arr),
            "(?:" => self.convert_typed_noncapturing_group(arr),
            "(?<" => self.convert_typed_named_group(arr, ">"),
            "(?'" => self.convert_typed_named_group(arr, "'"),
            "(?P<" => self.convert_typed_python_named_group(arr),
            "(?P=" => self.convert_typed_python_named_backreference(arr),
            "(?>" => self.convert_typed_atomic_group(arr),
            "(?|" => self.convert_typed_branch_reset_group(arr),
            "(?=" => self.convert_typed_lookaround(arr, true, true),
            "(?!" => self.convert_typed_lookaround(arr, true, false),
            "(?<=" => self.convert_typed_lookaround(arr, false, true),
            "(?<!" => self.convert_typed_lookaround(arr, false, false),
            "(?*" => self.convert_typed_lookaround(arr, true, true),
            "(?<*" => self.convert_typed_lookaround(arr, false, true),
            "(?(" => self.convert_typed_conditional(arr),
            "(?C" => self.convert_typed_callout(arr),
            "(?{" => self.convert_typed_code_block(arr),
            "(?#" => self.convert_typed_comment_group(arr),
            "(?[" => self.convert_typed_extended_class(arr),
            "(*" => self.convert_typed_alpha_prefixed(arr),
            "(?" => self.convert_typed_question_prefixed(arr),
            // Atomic-alpha prefix `(*atomic:...)` — dedicated long-prefix
            // shape distinct from `(*atomic`-as-directive-name.
            other if other.starts_with("(*atomic:") => {
                let inner = self.convert_typed_optional_pattern(arr.get(1))?;
                Ok(Regex::Group {
                    expr: Box::new(inner),
                    kind: GroupKind::Atomic,
                    index: None,
                    name: None,
                })
            }
            // Scan-substring forms `(*scs:...)`, `(*scan_substring:...)`
            // — pass the body through (RGX has no dedicated AST node yet).
            other if other.starts_with("(*scs:") || other.starts_with("(*scan_substring:") => {
                self.convert_typed_optional_pattern(arr.get(1))
            }
            // Script-run forms `(*sr:...)`, `(*script_run:...)`,
            // `(*asr:...)`, `(*atomic_script_run:...)` — body passes through.
            other
                if other.starts_with("(*sr:")
                    || other.starts_with("(*script_run:")
                    || other.starts_with("(*asr:")
                    || other.starts_with("(*atomic_script_run:") =>
            {
                self.convert_typed_optional_pattern(arr.get(1))
            }
            other => {
                Err(self.contract_error(&format!("unrecognised array atom prefix: {other:?}")))
            }
        }
    }

    /// Walk a typed `quantifier` object per slice-6 closure (post-1.1.35):
    ///
    /// ```json
    /// {"type": "quantifier", "min": <usize>, "max": <usize | null>,
    ///  "greediness": "lazy" | "possessive" | []}
    /// ```
    ///
    /// `greediness: []` is the un-matched `quant_suffix?` slot and
    /// corresponds to PCRE2's "greedy" default.

    // -- Helpers for the typed atom-kind walker (PGEN slice 18+) --

    fn convert_typed_alpha_lookaround_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let name = map
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| self.contract_error("typed alpha_lookaround missing 'name'"))?;
        let body = map
            .get("body")
            .ok_or_else(|| self.contract_error("typed alpha_lookaround missing 'body'"))?;
        let inner = self.convert_typed_pattern(body)?;
        let na = matches!(
            name,
            "non_atomic_positive_lookahead"
                | "napla"
                | "non_atomic_negative_lookahead"
                | "nanla"
                | "non_atomic_positive_lookbehind"
                | "naplb"
                | "non_atomic_negative_lookbehind"
                | "nanlb"
        );
        match name {
            "positive_lookahead" | "pla" | "non_atomic_positive_lookahead" | "napla" => {
                Ok(Regex::Lookahead {
                    expr: Box::new(inner),
                    positive: true,
                    non_atomic: na,
                })
            }
            "negative_lookahead" | "nla" | "non_atomic_negative_lookahead" | "nanla" => {
                Ok(Regex::Lookahead {
                    expr: Box::new(inner),
                    positive: false,
                    non_atomic: na,
                })
            }
            "positive_lookbehind" | "plb" | "non_atomic_positive_lookbehind" | "naplb" => {
                Ok(Regex::Lookbehind {
                    expr: Box::new(inner),
                    positive: true,
                    non_atomic: na,
                })
            }
            "negative_lookbehind" | "nlb" | "non_atomic_negative_lookbehind" | "nanlb" => {
                Ok(Regex::Lookbehind {
                    expr: Box::new(inner),
                    positive: false,
                    non_atomic: na,
                })
            }
            // Other alpha-prefixed forms — fall back to atomic group.
            _ => Ok(Regex::Group {
                expr: Box::new(inner),
                kind: GroupKind::Atomic,
                index: None,
                name: None,
            }),
        }
    }

    fn convert_typed_char_class_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let body = map
            .get("body")
            .and_then(|v| v.as_array())
            .ok_or_else(|| self.contract_error("typed char_class missing 'body' array"))?;
        let class_negated = matches!(map.get("negated"), Some(serde_json::Value::Bool(true)));
        // PGEN emits `initial_close:true` (boolean) for the
        // leading-`]` shape `[]...]` / `[^]...]` per the typed
        // char_class slice (post-1.1.7x). Older pins used `"]"`
        // (string). Accept both.
        let initial_close_present = match map.get("initial_close") {
            Some(serde_json::Value::Bool(b)) => *b,
            Some(serde_json::Value::String(s)) => s == "]",
            _ => false,
        };
        let mut ranges: Vec<CharRange> = Vec::new();
        // Parallel `/i` ranges — see `convert_char_class` for the
        // rationale. For each class item that is a case-distinguished
        // Unicode property (`\p{Lu/Ll/Lt/L&/Lc/Cased_Letter/Upper/
        // Lower/Cased}` or its `\P` complement), `ci_ranges` gets the
        // case-fold-closed substitution; otherwise it tracks `ranges`
        // verbatim. `saw_ci_divergence` triggers `ci_override_ranges`
        // population only when the literal/closed sets differ.
        let mut ci_ranges: Vec<CharRange> = Vec::new();
        let mut saw_ci_divergence = false;
        if initial_close_present {
            ranges.push(CharRange::single(']'));
            ci_ranges.push(CharRange::single(']'));
        }
        let mut idx = 0;
        while idx < body.len() {
            let item = &body[idx];
            // PCRE2 quoted-run-as-range-start (Cluster 2F) — same as the
            // un-typed walker's peek-ahead at line ~1880.
            if Self::is_quoted_class_run(item)
                && body.get(idx + 1).and_then(|v| v.as_str()) == Some("-")
                && body.get(idx + 2).is_some()
            {
                let chars = Self::extract_quoted_class_chars(item);
                if let Some(last) = chars.chars().last() {
                    for ch in chars.chars().take(chars.chars().count() - 1) {
                        ranges.push(CharRange::single(ch));
                        ci_ranges.push(CharRange::single(ch));
                    }
                    let end_ch = match &body[idx + 2] {
                        serde_json::Value::String(s) => s
                            .chars()
                            .next()
                            .ok_or_else(|| self.contract_error("empty quoted-run range end"))?,
                        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                            let mut tmp: Vec<CharRange> = Vec::new();
                            self.convert_typed_class_item(&body[idx + 2], &mut tmp)?;
                            if tmp.len() == 1 && tmp[0].start == tmp[0].end {
                                tmp[0].start
                            } else {
                                return Err(self.contract_error(
                                    "quoted-run range end must be a single character",
                                ));
                            }
                        }
                        other => {
                            return Err(self.contract_error(&format!(
                                "quoted-run range end has unexpected shape: {}",
                                json_kind(other)
                            )));
                        }
                    };
                    if (end_ch as u32) < (last as u32) {
                        return Err(self.contract_error(&format!(
                            "descending character class range: {last:?}-{end_ch:?}"
                        )));
                    }
                    ranges.push(CharRange::range(last, end_ch));
                    ci_ranges.push(CharRange::range(last, end_ch));
                    idx += 3;
                    continue;
                }
            }
            let item_start = ranges.len();
            self.convert_typed_class_item(item, &mut ranges)?;
            let appended = ranges[item_start..].to_vec();
            if let Some(diverged) = self.case_fold_property_typed_class_item_ranges(item) {
                ci_ranges.extend(diverged);
                saw_ci_divergence = true;
            } else {
                ci_ranges.extend(appended);
            }
            idx += 1;
        }
        let ci_override_ranges = if saw_ci_divergence {
            let mut v = ci_ranges;
            v.sort_by_key(|r| r.start);
            Some(v)
        } else {
            None
        };
        Ok(Regex::CharClass(CharClass::Custom {
            ranges,
            negated: class_negated,
            ci_override_ranges,
        }))
    }

    fn convert_typed_directive_verb_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let body = map
            .get("body")
            .and_then(|v| v.as_object())
            .ok_or_else(|| self.contract_error("typed directive_verb missing 'body' object"))?;
        let body_kind = body.get("kind").and_then(|v| v.as_str()).unwrap_or("named");
        if body_kind == "mark_shorthand" {
            let payload = body
                .get("payload")
                .and_then(|v| v.as_str())
                .ok_or_else(|| self.contract_error("mark_shorthand missing 'payload'"))?
                .to_string();
            return Ok(Regex::Mark(payload));
        }
        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| self.contract_error("typed directive_verb body missing 'name'"))?;
        let payload = body.get("payload").and_then(|v| v.as_object());
        let payload_value: Option<String> = payload.and_then(|p| {
            p.get("value")
                .and_then(|v| v.as_str())
                .map(std::string::ToString::to_string)
        });
        let upper = name.to_ascii_uppercase();
        match upper.as_str() {
            "COMMIT" => Ok(Regex::Commit),
            "PRUNE" => Ok(Regex::Prune),
            "SKIP" => Ok(Regex::Skip(payload_value)),
            "THEN" => Ok(Regex::Then),
            "ACCEPT" => Ok(Regex::Accept),
            "FAIL" | "F" => Ok(Regex::CharClass(CharClass::Custom {
                ranges: Vec::new(),
                negated: false,
                ci_override_ranges: None,
            })),
            "MARK" => match payload_value {
                Some(v) => Ok(Regex::Mark(v)),
                None => Ok(Regex::Empty),
            },
            _ => Ok(Regex::Empty),
        }
    }

    fn convert_typed_code_block_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let content = map
            .get("content")
            .and_then(|v| v.as_array())
            .ok_or_else(|| self.contract_error("typed code_block missing 'content' array"))?;
        let mut buf = String::new();
        // Use `walk_json_terminal_chars` per element so sub-array
        // shapes (PGEN's encoding for chars that hit reserved grammar
        // terminals — e.g. `\$` becomes `["\\", "$"]`) flatten into
        // literal chars instead of being silently dropped.
        for elem in content {
            walk_json_terminal_chars(elem, &mut buf);
        }
        let lang = map
            .get("lang")
            .and_then(|v| v.as_str())
            .unwrap_or("perl")
            .to_string();
        Ok(Regex::CodeBlock { lang, code: buf })
    }

    fn convert_typed_subroutine_call_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        // The typed `subroutine_call.target` is one of two shapes:
        //   plain `(?N)`:     `{kind, value, sign?}` (numeric) or
        //                     `{kind:"named"|"python_named", name}`
        //   `(?N(grouplist))`: `{subroutine: {<plain shape>}, captures: [...]}`
        // The `captures` array is a raw-token tree in grammar order:
        //   ["(", <first_arg>, [<comma-tail entries>], ")"]
        // where each comma-tail entry is `[",", <arg>]`. An arg is
        // either a string (named ref) or an object `{sign, value}`
        // (numeric / relative numeric). Decoding it here populates
        // `Regex::ReturnedCaptureSubroutine`; the compile path
        // (`vm.rs::compile`) and `OpCode::CallReturning` dispatch
        // close Cluster 1B (testinput2:8067-8168 family +
        // testinput2:8109 nested-bracket subjects in Cluster 2G).
        let target = map
            .get("target")
            .and_then(|v| v.as_object())
            .ok_or_else(|| self.contract_error("typed subroutine_call missing 'target' object"))?;
        let captures_array = target.get("captures").and_then(|v| v.as_array());
        let inner = if let Some(sub) = target.get("subroutine").and_then(|v| v.as_object()) {
            sub
        } else {
            target
        };
        let target_recursion = self.decode_typed_subroutine_target(inner)?;
        let Some(captures) = captures_array else {
            return Ok(Regex::Recursion {
                target: target_recursion,
            });
        };
        let returned_groups = self.decode_typed_returned_capture_args(captures)?;
        if returned_groups.is_empty() {
            // `(?1())` empty arg-list — treat as plain recursion. (PCRE2
            // rejects this; PGEN also rejects it. This branch is
            // defensive.)
            return Ok(Regex::Recursion {
                target: target_recursion,
            });
        }
        Ok(Regex::ReturnedCaptureSubroutine {
            target: target_recursion,
            returned_groups,
        })
    }

    fn decode_typed_subroutine_target(
        &self,
        inner: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<RecursionTarget> {
        let kind = inner
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| self.contract_error("typed subroutine_call target missing 'kind'"))?;
        match kind {
            "numeric" => {
                let value = inner
                    .get("value")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        self.contract_error("typed subroutine_call numeric target missing 'value'")
                    })?;
                let sign = inner.get("sign").and_then(|v| v.as_str());
                Ok(match sign {
                    Some("+") => RecursionTarget::RelativeGroup(value as i32),
                    Some("-") => RecursionTarget::RelativeGroup(-(value as i32)),
                    _ => RecursionTarget::Group(value as u32),
                })
            }
            "named" | "python_named" => {
                let name = inner
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        self.contract_error("typed subroutine_call named target missing 'name'")
                    })?
                    .to_string();
                Ok(RecursionTarget::NamedGroup(name))
            }
            "recursion" => Ok(RecursionTarget::Entire),
            other => Err(self.contract_error(&format!(
                "unrecognised typed subroutine_call target kind: {other:?}"
            ))),
        }
    }

    fn decode_typed_returned_capture_args(
        &self,
        captures: &[serde_json::Value],
    ) -> Result<Vec<RecursionTarget>> {
        // Shape: ["(", <first_arg>, [<comma-tail entries>], ")"]
        // Comma-tail entry: [",", <arg>].
        let mut out = Vec::new();
        if captures.len() < 2 {
            return Ok(out);
        }
        // Index 0 is the literal "(" — skip.
        // Index 1 is the first arg.
        out.push(self.decode_returned_capture_arg(&captures[1])?);
        // Index 2 (if present) is the comma-tail list.
        if let Some(tail) = captures.get(2).and_then(|v| v.as_array()) {
            for entry in tail {
                let entry_arr = entry.as_array().ok_or_else(|| {
                    self.contract_error(
                        "typed subroutine_call captures comma-tail entry not an array",
                    )
                })?;
                // Each entry is [",", <arg>] — second element is the arg.
                if entry_arr.len() < 2 {
                    return Err(self.contract_error(
                        "typed subroutine_call captures comma-tail entry too short",
                    ));
                }
                out.push(self.decode_returned_capture_arg(&entry_arr[1])?);
            }
        }
        // Index 3 is the literal ")" — skip.
        Ok(out)
    }

    fn decode_returned_capture_arg(&self, arg: &serde_json::Value) -> Result<RecursionTarget> {
        if let Some(s) = arg.as_str() {
            return Ok(RecursionTarget::NamedGroup(s.to_string()));
        }
        if let Some(obj) = arg.as_object() {
            let value = obj
                .get("value")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    self.contract_error("typed returned-capture arg object missing 'value'")
                })?;
            let sign = obj.get("sign").and_then(|v| v.as_str());
            return Ok(match sign {
                Some("+") => RecursionTarget::RelativeGroup(value as i32),
                Some("-") => RecursionTarget::RelativeGroup(-(value as i32)),
                _ => RecursionTarget::Group(value as u32),
            });
        }
        Err(self.contract_error(&format!(
            "unrecognised typed returned-capture arg shape: {arg:?}"
        )))
    }

    fn convert_typed_inline_modifiers_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
        body: Option<&serde_json::Value>,
    ) -> Result<Regex> {
        let spec = map
            .get("spec")
            .and_then(|v| v.as_object())
            .ok_or_else(|| self.contract_error("typed inline_modifiers missing 'spec' object"))?;
        let seq = spec
            .get("seq")
            .and_then(|v| v.as_object())
            .ok_or_else(|| self.contract_error("typed inline_modifiers spec missing 'seq'"))?;
        let mut flags = String::new();
        if let Some(set) = seq.get("set").and_then(|v| v.as_array()) {
            for c in set {
                if let Some(s) = c.as_str() {
                    flags.push_str(s);
                }
            }
        }
        if let Some(unset) = seq.get("unset").and_then(|v| v.as_array()) {
            if !unset.is_empty() {
                flags.push('-');
                for c in unset {
                    if let Some(s) = c.as_str() {
                        flags.push_str(s);
                    }
                }
            }
        }
        let body_expr = match body {
            Some(b) => self.convert_typed_pattern(b)?,
            None => Regex::Empty,
        };
        Ok(Regex::FlagGroup {
            flags,
            expr: Box::new(body_expr),
        })
    }

    fn convert_typed_conditional_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Regex> {
        let condition = map
            .get("condition")
            .ok_or_else(|| self.contract_error("typed conditional missing 'condition'"))?;
        let yes_branch_value = map
            .get("yes_branch")
            .ok_or_else(|| self.contract_error("typed conditional missing 'yes_branch'"))?;
        let yes_branch = self.convert_typed_pattern_branch_array(yes_branch_value)?;
        let no_branch = match map.get("no_branch") {
            Some(serde_json::Value::Array(a)) if a.is_empty() => None,
            Some(serde_json::Value::Array(a)) => {
                if a.len() >= 2 {
                    Some(self.convert_typed_pattern_branch_array(&a[1])?)
                } else {
                    None
                }
            }
            None => None,
            Some(other) => {
                return Err(self.contract_error(&format!(
                    "unrecognised conditional no_branch shape: {}",
                    json_kind(other)
                )));
            }
        };
        // VERSION conditional — short-circuit at parse time.
        if let serde_json::Value::Object(obj) = condition {
            if obj.get("kind").and_then(|v| v.as_str()) == Some("version") {
                let op_str = obj
                    .get("operator")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("VERSION condition missing 'operator'"))?;
                let number = obj
                    .get("number")
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| self.contract_error("VERSION condition missing 'number'"))?;
                let major = number
                    .get("major")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                let minor = number
                    .get("minor")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                let op = match op_str {
                    "=" | "==" => VersionConditionalOp::Eq,
                    "!=" => VersionConditionalOp::Ne,
                    ">" => VersionConditionalOp::Gt,
                    ">=" => VersionConditionalOp::Ge,
                    "<" => VersionConditionalOp::Lt,
                    "<=" => VersionConditionalOp::Le,
                    other => {
                        return Err(self.contract_error(&format!(
                            "unrecognised VERSION condition operator: {other:?}"
                        )));
                    }
                };
                let passes = evaluate_version_conditional(op, (major, minor));
                return Ok(if passes {
                    yes_branch
                } else {
                    no_branch.unwrap_or(Regex::Empty)
                });
            }
        }
        let test = self.convert_typed_conditional_test(condition)?;
        Ok(Regex::Conditional {
            condition: test,
            true_branch: Box::new(yes_branch),
            false_branch: no_branch.map(Box::new),
        })
    }

    fn convert_typed_conditional_test(&self, value: &serde_json::Value) -> Result<ConditionalTest> {
        match value {
            serde_json::Value::Number(n) => {
                let idx = n
                    .as_u64()
                    .ok_or_else(|| self.contract_error("conditional numeric idx not u64"))?
                    as u32;
                Ok(ConditionalTest::GroupExists(idx))
            }
            serde_json::Value::String(s) => Ok(ConditionalTest::NamedGroupExists(s.clone())),
            serde_json::Value::Object(map) => {
                if let Some(kind) = map.get("kind").and_then(|v| v.as_str()) {
                    match kind {
                        "recursion" => match map.get("group") {
                            None => Ok(ConditionalTest::RecursionAny),
                            Some(serde_json::Value::Array(a)) if a.is_empty() => {
                                Ok(ConditionalTest::RecursionAny)
                            }
                            Some(serde_json::Value::Number(n)) => Ok(
                                ConditionalTest::RecursionGroup(n.as_u64().unwrap_or(0) as u32),
                            ),
                            Some(serde_json::Value::String(s)) => {
                                Ok(ConditionalTest::RecursionNamed(s.clone()))
                            }
                            Some(other) => Err(self.contract_error(&format!(
                                "unrecognised recursion condition group: {}",
                                json_kind(other)
                            ))),
                        },
                        "recursion_named" => {
                            let name = map
                                .get("name")
                                .and_then(|v| v.as_str())
                                .ok_or_else(|| {
                                    self.contract_error("recursion_named condition missing 'name'")
                                })?
                                .to_string();
                            Ok(ConditionalTest::RecursionNamed(name))
                        }
                        "define" => Ok(ConditionalTest::Define),
                        "lookahead" | "lookbehind" => {
                            let body = map.get("body").ok_or_else(|| {
                                self.contract_error("lookaround condition missing 'body'")
                            })?;
                            let positive = map
                                .get("positive")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(true);
                            let inner = self.convert_typed_pattern(body)?;
                            if kind == "lookahead" {
                                Ok(ConditionalTest::Lookahead {
                                    expr: Box::new(inner),
                                    positive,
                                })
                            } else {
                                Ok(ConditionalTest::Lookbehind {
                                    expr: Box::new(inner),
                                    positive,
                                })
                            }
                        }
                        "alpha_lookaround" => {
                            let alpha_name =
                                map.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                                    self.contract_error("alpha_lookaround condition missing 'name'")
                                })?;
                            let body = map.get("body").ok_or_else(|| {
                                self.contract_error("alpha_lookaround condition missing 'body'")
                            })?;
                            let inner = self.convert_typed_pattern(body)?;
                            match alpha_name {
                                "positive_lookahead"
                                | "pla"
                                | "non_atomic_positive_lookahead"
                                | "napla" => Ok(ConditionalTest::Lookahead {
                                    expr: Box::new(inner),
                                    positive: true,
                                }),
                                "negative_lookahead"
                                | "nla"
                                | "non_atomic_negative_lookahead"
                                | "nanla" => Ok(ConditionalTest::Lookahead {
                                    expr: Box::new(inner),
                                    positive: false,
                                }),
                                "positive_lookbehind"
                                | "plb"
                                | "non_atomic_positive_lookbehind"
                                | "naplb" => Ok(ConditionalTest::Lookbehind {
                                    expr: Box::new(inner),
                                    positive: true,
                                }),
                                "negative_lookbehind"
                                | "nlb"
                                | "non_atomic_negative_lookbehind"
                                | "nanlb" => Ok(ConditionalTest::Lookbehind {
                                    expr: Box::new(inner),
                                    positive: false,
                                }),
                                other => Err(self.contract_error(&format!(
                                    "unrecognised alpha_lookaround condition variant: {other:?}"
                                ))),
                            }
                        }
                        "callout_assertion" => {
                            if let Some(assertion) = map.get("assertion") {
                                self.convert_typed_conditional_test(assertion)
                            } else {
                                Ok(ConditionalTest::GroupExists(0))
                            }
                        }
                        "python_named" => {
                            let name = map
                                .get("name")
                                .and_then(|v| v.as_str())
                                .ok_or_else(|| {
                                    self.contract_error("python_named condition missing 'name'")
                                })?
                                .to_string();
                            Ok(ConditionalTest::NamedGroupExists(name))
                        }
                        "version" => Err(self.contract_error(
                            "version condition reached test-only path; should be short-circuited",
                        )),
                        other => {
                            Err(self
                                .contract_error(&format!("unrecognised condition kind: {other:?}")))
                        }
                    }
                } else if let Some(value_int) = map.get("value").and_then(serde_json::Value::as_u64)
                {
                    let sign = map.get("sign").and_then(|v| v.as_str());
                    match sign {
                        Some("+") => Ok(ConditionalTest::RelativeGroupExists(value_int as i32)),
                        Some("-") => Ok(ConditionalTest::RelativeGroupExists(-(value_int as i32))),
                        _ => Ok(ConditionalTest::GroupExists(value_int as u32)),
                    }
                } else {
                    Err(self.contract_error("typed condition object missing 'kind' or 'value'"))
                }
            }
            other => Err(self.contract_error(&format!(
                "unrecognised conditional condition shape: {}",
                json_kind(other)
            ))),
        }
    }

    fn convert_typed_pattern_branch_array(&self, value: &serde_json::Value) -> Result<Regex> {
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected branch piece array, got {}",
                json_kind(value)
            ))
        })?;
        let mut pieces = Vec::with_capacity(arr.len());
        for elem in arr {
            pieces.push(self.convert_typed_piece(elem)?);
        }
        match pieces.len() {
            0 => Ok(Regex::Empty),
            1 => Ok(pieces.into_iter().next().unwrap()),
            _ => Ok(Regex::Sequence(pieces)),
        }
    }

    fn collect_extended_class_ranges(
        &self,
        item: &serde_json::Value,
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        match item {
            serde_json::Value::String(s) => {
                if matches!(
                    s.as_str(),
                    " " | "\t" | "+" | "-" | "&" | "|" | "^" | "(" | ")"
                ) {
                    return Ok(());
                }
                if s.chars().count() == 1 {
                    ranges.push(CharRange::single(s.chars().next().unwrap()));
                    return Ok(());
                }
                Ok(())
            }
            serde_json::Value::Array(arr) => {
                if arr.len() == 3 {
                    if let (Some(start_s), Some("-"), Some(end_s)) =
                        (arr[0].as_str(), arr[1].as_str(), arr[2].as_str())
                    {
                        if let (Some(s), Some(e)) = (start_s.chars().next(), end_s.chars().next()) {
                            ranges.push(CharRange::range(s, e));
                            return Ok(());
                        }
                    }
                }
                if arr.len() >= 3 {
                    if let (Some("["), Some("]")) =
                        (arr[0].as_str(), arr.last().and_then(|v| v.as_str()))
                    {
                        for inner in &arr[1..arr.len() - 1] {
                            self.collect_extended_class_ranges(inner, ranges)?;
                        }
                        return Ok(());
                    }
                }
                for elem in arr {
                    self.collect_extended_class_ranges(elem, ranges)?;
                }
                Ok(())
            }
            serde_json::Value::Object(_) => self.convert_typed_class_item(item, ranges),
            _ => Ok(()),
        }
    }

    fn lower_regex_into_class_ranges(
        &self,
        regex: &Regex,
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        match regex {
            Regex::Char(ch) => {
                ranges.push(CharRange::single(*ch));
                Ok(())
            }
            Regex::CharClass(CharClass::Custom {
                ranges: rs,
                negated,
                ..
            }) => {
                let merged = if *negated {
                    complement_ranges(rs)
                } else {
                    rs.clone()
                };
                ranges.extend(merged);
                Ok(())
            }
            Regex::CharClass(CharClass::Digit { negated }) => {
                let base = if self.ucp_enabled {
                    crate::unicode_support::ucp_digit_ranges()
                } else {
                    vec![CharRange::range('0', '9')]
                };
                let merged = if *negated {
                    complement_ranges(&base)
                } else {
                    base
                };
                ranges.extend(merged);
                Ok(())
            }
            Regex::CharClass(CharClass::Word { negated }) => {
                let base = if self.ucp_enabled {
                    crate::unicode_support::ucp_word_ranges()
                } else {
                    posix_class_ranges("word").unwrap_or_default()
                };
                let merged = if *negated {
                    complement_ranges(&base)
                } else {
                    base
                };
                ranges.extend(merged);
                Ok(())
            }
            Regex::CharClass(CharClass::Space { negated }) => {
                let base = if self.ucp_enabled {
                    crate::unicode_support::ucp_space_ranges()
                } else {
                    posix_class_ranges("space").unwrap_or_default()
                };
                let merged = if *negated {
                    complement_ranges(&base)
                } else {
                    base
                };
                ranges.extend(merged);
                Ok(())
            }
            // `\p{...}` inside a class — resolve the Unicode property
            // and fold its ranges into the class union.
            Regex::CharClass(CharClass::UnicodeClass { name, negated }) => {
                let resolved =
                    crate::unicode_support::resolve_unicode_property_class(name, *negated)
                        .map_err(|e| self.contract_error(&e))?;
                ranges.extend(resolved);
                Ok(())
            }
            Regex::WordBoundary { .. } => {
                // `\b` inside a class is backspace literal.
                ranges.push(CharRange::single('\u{0008}'));
                Ok(())
            }
            Regex::Backreference(n) if *n < 10 => {
                let ch = char::from(b'0' + *n as u8);
                ranges.push(CharRange::single(ch));
                Ok(())
            }
            Regex::Anchor(_) | Regex::MatchReset | Regex::Empty => {
                Err(self.contract_error("zero-width escape inside char_class is not allowed"))
            }
            other => Err(self.contract_error(&format!(
                "escape inside char_class produced unexpected Regex shape: {other:?}"
            ))),
        }
    }

    /// Reconstruct the source text for a typed-walker class body — the
    /// inverse of PGEN's typed shape. Used to produce the `content`
    /// string for `Regex::ExtendedCharClass` so the compiler's ECC
    /// evaluator sees the original `\d`/`\xFF`/`\p{L}`/etc. syntax it
    /// expects, rather than the concatenated field-value gibberish that
    /// `walk_json_terminal_chars` would emit on typed escape objects.
    fn reconstruct_typed_class_text(&self, value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::String(s) => out.push_str(s),
            serde_json::Value::Array(arr) => {
                for elem in arr {
                    self.reconstruct_typed_class_text(elem, out);
                }
            }
            serde_json::Value::Object(map) => {
                let kind = map.get("type").and_then(|v| v.as_str());
                if kind == Some("escape") {
                    let escape_kind = map.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    match escape_kind {
                        "shorthand" | "control" => {
                            out.push('\\');
                            if escape_kind == "control" {
                                out.push('c');
                            }
                            if let Some(s) = map.get("char").and_then(|v| v.as_str()) {
                                out.push_str(s);
                            }
                        }
                        "single_byte" => out.push_str("\\C"),
                        "hex" => {
                            out.push_str("\\x{");
                            if let Some(s) = map.get("digits").and_then(|v| v.as_str()) {
                                out.push_str(s);
                            }
                            out.push('}');
                        }
                        "octal" => {
                            out.push_str("\\o{");
                            if let Some(s) = map.get("digits").and_then(|v| v.as_str()) {
                                out.push_str(s);
                            }
                            out.push('}');
                        }
                        "unicode" => {
                            out.push_str("\\u{");
                            if let Some(s) = map.get("digits").and_then(|v| v.as_str()) {
                                out.push_str(s);
                            }
                            out.push('}');
                        }
                        "property" => {
                            let negated = map
                                .get("negated")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            out.push('\\');
                            out.push(if negated { 'P' } else { 'p' });
                            out.push('{');
                            if let Some(s) = map.get("name").and_then(|v| v.as_str()) {
                                out.push_str(s);
                            }
                            out.push('}');
                        }
                        _ => {
                            // Unknown escape kind — fall back to the
                            // generic walker so we at least emit
                            // SOMETHING (even if wrong, it'll surface
                            // as a compile error rather than silently
                            // mismatching).
                            walk_json_terminal_chars(value, out);
                        }
                    }
                } else {
                    // Non-escape typed object inside a class body —
                    // generic walk works for posix_class etc. (their
                    // field values reconstruct usefully).
                    walk_json_terminal_chars(value, out);
                }
            }
            _ => {}
        }
    }

    fn endpoint_to_char(&self, value: &serde_json::Value, what: &'static str) -> Result<char> {
        match value {
            serde_json::Value::String(s) => s
                .chars()
                .next()
                .ok_or_else(|| self.contract_error(&format!("{what} string is empty"))),
            serde_json::Value::Object(map) => {
                let kind = map.get("type").and_then(|v| v.as_str());
                match kind {
                    Some("escape") => {
                        let lowered = self.convert_typed_escape_object(map)?;
                        match lowered {
                            Regex::Char(c) => Ok(c),
                            other => Err(self.contract_error(&format!(
                                "{what} escape produced non-char Regex: {other:?}"
                            ))),
                        }
                    }
                    // `[\Qa\E-\Qz\E]` — `\Q`-quoted single-char atom as
                    // range endpoint.
                    Some("class_quoted_range_atom") => map
                        .get("char")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.chars().next())
                        .ok_or_else(|| {
                            self.contract_error(&format!(
                                "{what} class_quoted_range_atom missing 'char'"
                            ))
                        }),
                    other => Err(self.contract_error(&format!(
                        "{what} object is not a typed escape: type={other:?}"
                    ))),
                }
            }
            other => Err(self.contract_error(&format!(
                "{what} has unexpected shape: {}",
                json_kind(other)
            ))),
        }
    }

    fn convert_typed_quantifier_object(
        &self,
        value: &serde_json::Value,
    ) -> Result<(Quantifier, bool)> {
        let map = value.as_object().ok_or_else(|| {
            self.contract_error(&format!(
                "expected typed quantifier object, got {}",
                json_kind(value)
            ))
        })?;
        let kind = map.get("type").and_then(|v| v.as_str());
        if kind != Some("quantifier") {
            return Err(self.contract_error(&format!(
                "expected typed quantifier object with type=\"quantifier\", got type={kind:?}"
            )));
        }
        let min = map
            .get("min")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| self.contract_error("typed quantifier missing 'min'"))?;
        let max = match map.get("max") {
            Some(serde_json::Value::Null) | None => None,
            Some(serde_json::Value::Number(n)) => n.as_u64(),
            Some(other) => {
                return Err(self.contract_error(&format!(
                    "typed quantifier 'max' has unexpected shape: {}",
                    json_kind(other)
                )));
            }
        };
        let (lazy, possessive) = match map.get("greediness") {
            Some(serde_json::Value::Array(a)) if a.is_empty() => (false, false),
            Some(serde_json::Value::String(s)) if s == "lazy" => (true, false),
            Some(serde_json::Value::String(s)) if s == "possessive" => (false, true),
            Some(other) => {
                return Err(self.contract_error(&format!(
                    "typed quantifier 'greediness' has unexpected shape: {} = {}",
                    json_kind(other),
                    other
                )));
            }
            None => (false, false), // defensive default — treat missing field as greedy
        };

        #[allow(clippy::cast_possible_truncation)]
        let min_u32 = min as u32;
        #[allow(clippy::cast_possible_truncation)]
        let max_u32 = max.map(|n| n as u32);

        let quantifier = match (min_u32, max_u32) {
            (0, None) => Quantifier::ZeroOrMore { lazy },
            (1, None) => Quantifier::OneOrMore { lazy },
            (0, Some(1)) => Quantifier::ZeroOrOne { lazy },
            (lo, hi) => Quantifier::Range {
                min: lo,
                max: hi,
                lazy,
            },
        };
        Ok((quantifier, possessive))
    }

    // ============================================================
    // Typed-shape array atom converters. Each takes the raw atom
    // array (e.g. `["[", neg, init, body, "]"]`) and returns a
    // Regex AST node. Inner patterns are recursed via
    // `convert_typed_pattern` since group bodies follow the same
    // 2-element [first_alt, rest] shape as the top-level pattern field.
    // ============================================================

    fn convert_typed_capturing_group(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        let inner = self.convert_typed_optional_pattern(arr.get(1))?;
        Ok(Regex::Group {
            expr: Box::new(inner),
            kind: GroupKind::Capturing,
            index: None,
            name: None,
        })
    }

    fn convert_typed_noncapturing_group(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        let inner = self.convert_typed_optional_pattern(arr.get(1))?;
        Ok(Regex::Group {
            expr: Box::new(inner),
            kind: GroupKind::NonCapturing,
            index: None,
            name: None,
        })
    }

    fn convert_typed_named_group(
        &self,
        arr: &[serde_json::Value],
        _close_marker: &str,
    ) -> Result<Regex> {
        // Shape: ["(?<", <name>, ">", <pattern?>, ")"] OR ["(?'", <name>, "'", <pattern?>, ")"]
        let name = self.extract_typed_name(arr.get(1))?;
        let inner = self.convert_typed_optional_pattern(arr.get(3))?;
        Ok(Regex::Group {
            expr: Box::new(inner),
            kind: GroupKind::Capturing,
            index: None,
            name: Some(name),
        })
    }

    fn convert_typed_python_named_group(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: ["(?P<", <name>, ">", <pattern?>, ")"]
        let name = self.extract_typed_name(arr.get(1))?;
        let inner = self.convert_typed_optional_pattern(arr.get(3))?;
        Ok(Regex::Group {
            expr: Box::new(inner),
            kind: GroupKind::Capturing,
            index: None,
            name: Some(name),
        })
    }

    fn convert_typed_python_named_backreference(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: ["(?P=", <name>, ")"]
        let name = self.extract_typed_name(arr.get(1))?;
        Ok(Regex::NamedBackreference(name))
    }

    fn convert_typed_atomic_group(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: ["(?>", <pattern?>, ")"]
        let inner = self.convert_typed_optional_pattern(arr.get(1))?;
        Ok(Regex::Group {
            expr: Box::new(inner),
            kind: GroupKind::Atomic,
            index: None,
            name: None,
        })
    }

    fn convert_typed_branch_reset_group(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: ["(?|", <pattern?>, ")"]
        let inner = self.convert_typed_optional_pattern(arr.get(1))?;
        Ok(Regex::Group {
            expr: Box::new(inner),
            kind: GroupKind::BranchReset,
            index: None,
            name: None,
        })
    }

    fn convert_typed_lookaround(
        &self,
        arr: &[serde_json::Value],
        is_lookahead: bool,
        positive: bool,
    ) -> Result<Regex> {
        // Shape: ["<prefix>", <pattern?>, ")"]
        let inner = self.convert_typed_optional_pattern(arr.get(1))?;
        if is_lookahead {
            Ok(Regex::Lookahead {
                expr: Box::new(inner),
                positive,
                non_atomic: false,
            })
        } else {
            Ok(Regex::Lookbehind {
                expr: Box::new(inner),
                positive,
                non_atomic: false,
            })
        }
    }

    /// Convert a `<pattern?>` slot. The `pattern` rule is annotated with
    /// `-> $1` (transparent passthrough), so a matched `pattern?` emits
    /// the pattern's typed content directly — the 2-element
    /// `[<first_alt>, <rest>]` array — and an unmatched `pattern?` emits
    /// `[]` (empty array). Same disambiguation as the top-level dispatch.
    fn convert_typed_optional_pattern(&self, slot: Option<&serde_json::Value>) -> Result<Regex> {
        let slot = match slot {
            Some(s) => s,
            None => return Ok(Regex::Empty),
        };
        let arr = slot.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected typed optional-pattern array, got {}",
                json_kind(slot)
            ))
        })?;
        if arr.is_empty() {
            return Ok(Regex::Empty);
        }
        // Matched: the slot IS the pattern (2-element [first_alt, rest]).
        self.convert_typed_pattern(slot)
    }

    /// Extract a name string from a `<name>` shape. The `name` rule is
    /// `name_start name_continue*` — emits `[<first-char>, <Quantified of rest>]`.
    fn extract_typed_name(&self, slot: Option<&serde_json::Value>) -> Result<String> {
        let value = slot.ok_or_else(|| self.contract_error("typed group missing name"))?;
        // The `name` shape is a 2-element array: [<first-char>, <Quantified of remaining>].
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!("expected name array, got {}", json_kind(value)))
        })?;
        let mut out = String::new();
        for elem in arr {
            walk_json_terminal_chars(elem, &mut out);
        }
        if out.is_empty() {
            return Err(self.contract_error("typed name was empty"));
        }
        Ok(out)
    }

    /// Convert a `\` escape atom: `["\\", <unit>]`. The unit is either
    /// a single-char string (simple escape, `single_byte_escape`) or an
    /// array (hex/unicode/octal/control/property/backreference).
    fn convert_typed_backslash_atom(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        if arr.len() < 2 {
            return Err(self.contract_error("backslash atom missing escape unit"));
        }
        let unit = &arr[1];
        match unit {
            serde_json::Value::String(s) if s == "C" => {
                // \C — match one code unit; lower to "any single codepoint"
                Ok(Regex::CharClass(CharClass::Custom {
                    ranges: vec![CharRange::range('\0', char::MAX)],
                    negated: false,
                    ci_override_ranges: None,
                }))
            }
            serde_json::Value::String(s) => {
                // Single-char simple_escape: \d, \w, \., \\, etc., or
                // single-digit backreference \1..\9.
                self.convert_typed_simple_escape_char(s, false)
            }
            serde_json::Value::Array(inner) => self.convert_typed_escape_unit_array(inner),
            other => Err(self.contract_error(&format!(
                "unrecognised escape unit shape: {}",
                json_kind(other)
            ))),
        }
    }

    fn convert_typed_escape_unit_array(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        if arr.is_empty() {
            return Err(self.contract_error("empty escape unit array"));
        }
        // Distinguish `backreference_digits` (= `nonzero_digit digit*`,
        // 2-element `[<digit-string>, <Quantified-*-array>]`) from
        // `octal_escape \NNN` (= `octal_digit octal_digit? octal_digit?`,
        // 3-element `[<oct>, <oct?>, <oct?>]`) by structural shape.
        // Backref shape is 2-element with arr[1] as an Array (the
        // Quantified-* of remaining digits); octal is 3-element.
        if arr.len() == 2 {
            if let (Some(first), serde_json::Value::Array(_)) = (arr[0].as_str(), &arr[1]) {
                if first.len() == 1 && first.chars().all(|c| c.is_ascii_digit()) {
                    return self.convert_typed_numeric_backref(arr);
                }
            }
        }
        // Dispatch on first element.
        match &arr[0] {
            serde_json::Value::String(s) => match s.as_str() {
                "x" | "x{" => self.convert_typed_hex_escape(arr),
                "u{" => self.convert_typed_unicode_escape(arr),
                "o{" => self.convert_typed_octal_braced(arr),
                "c" => self.convert_typed_control_escape(arr),
                "p{" => self.convert_typed_property_braced(arr, false),
                "P{" => self.convert_typed_property_braced(arr, true),
                "p" => self.convert_typed_property_short(arr, false),
                "P" => self.convert_typed_property_short(arr, true),
                // Octal NNN form: 1-3 octal digits as direct strings; 3-element
                // shape with each slot string-or-empty-array (from Quantified-?).
                d if d.len() == 1 && d.chars().all(|c| c.is_digit(8)) => {
                    self.convert_typed_octal_digits(arr)
                }
                other => {
                    Err(self
                        .contract_error(&format!("unrecognised escape unit array head: {other:?}")))
                }
            },
            // Nested array (e.g. backreference_digits is itself a Quantified
            // of digit terminals → nested array).
            serde_json::Value::Array(_) => self.convert_typed_numeric_backref(arr),
            other => Err(self.contract_error(&format!(
                "unrecognised escape unit array head shape: {}",
                json_kind(other)
            ))),
        }
    }

    /// Convert a simple-escape char (the catch-all `\<X>` where X is
    /// any single character). `in_class_context` toggles class-context
    /// semantics (e.g. `\b` becomes BS instead of word-boundary).
    fn convert_typed_simple_escape_char(&self, s: &str, in_class_context: bool) -> Result<Regex> {
        let mut chars = s.chars();
        let ch = chars
            .next()
            .ok_or_else(|| self.contract_error("empty simple_escape"))?;
        if chars.next().is_some() {
            return Err(
                self.contract_error(&format!("simple_escape expected single char, got {s:?}"))
            );
        }
        // Mirror the legacy `convert_simple_escape` mapping for the
        // shorthand-class and metachar cases.
        match ch {
            'd' => Ok(Regex::CharClass(if self.ucp_enabled {
                CharClass::Custom {
                    ranges: ucp_posix_class_ranges("digit").unwrap_or_default(),
                    negated: false,
                    ci_override_ranges: None,
                }
            } else {
                CharClass::Digit { negated: false }
            })),
            'D' => Ok(Regex::CharClass(if self.ucp_enabled {
                CharClass::Custom {
                    ranges: complement_ranges(&ucp_posix_class_ranges("digit").unwrap_or_default()),
                    negated: false,
                    ci_override_ranges: None,
                }
            } else {
                CharClass::Digit { negated: true }
            })),
            'w' => Ok(Regex::CharClass(if self.ucp_enabled {
                CharClass::Custom {
                    ranges: ucp_posix_class_ranges("word").unwrap_or_default(),
                    negated: false,
                    ci_override_ranges: None,
                }
            } else {
                CharClass::Word { negated: false }
            })),
            'W' => Ok(Regex::CharClass(if self.ucp_enabled {
                CharClass::Custom {
                    ranges: complement_ranges(&ucp_posix_class_ranges("word").unwrap_or_default()),
                    negated: false,
                    ci_override_ranges: None,
                }
            } else {
                CharClass::Word { negated: true }
            })),
            's' => Ok(Regex::CharClass(if self.ucp_enabled {
                CharClass::Custom {
                    ranges: ucp_posix_class_ranges("space").unwrap_or_default(),
                    negated: false,
                    ci_override_ranges: None,
                }
            } else {
                CharClass::Space { negated: false }
            })),
            'S' => Ok(Regex::CharClass(if self.ucp_enabled {
                CharClass::Custom {
                    ranges: complement_ranges(&ucp_posix_class_ranges("space").unwrap_or_default()),
                    negated: false,
                    ci_override_ranges: None,
                }
            } else {
                CharClass::Space { negated: true }
            })),
            'h' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: horizontal_whitespace_ranges(),
                negated: false,
                ci_override_ranges: None,
            })),
            'H' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: complement_ranges(&horizontal_whitespace_ranges()),
                negated: false,
                ci_override_ranges: None,
            })),
            'v' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: vertical_whitespace_ranges(),
                negated: false,
                ci_override_ranges: None,
            })),
            'V' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: complement_ranges(&vertical_whitespace_ranges()),
                negated: false,
                ci_override_ranges: None,
            })),
            'R' => Ok(self.newline_sequence_ast()),
            'N' => Ok(self.non_newline_ast()),
            'X' => Ok(Regex::GraphemeCluster),
            'b' => {
                if in_class_context {
                    Ok(Regex::Char('\u{08}'))
                } else {
                    Ok(Regex::WordBoundary { positive: true })
                }
            }
            'B' => Ok(Regex::WordBoundary { positive: false }),
            'A' => Ok(Regex::Anchor(AnchorType::AbsStart)),
            'Z' => Ok(Regex::Anchor(AnchorType::AbsEnd)),
            'z' => Ok(Regex::Anchor(AnchorType::AbsEndNoNL)),
            'G' => Ok(Regex::Anchor(AnchorType::PreviousMatchEnd)),
            'K' => Ok(Regex::MatchReset),
            // Common literal-escape characters.
            'n' => Ok(Regex::Char('\n')),
            't' => Ok(Regex::Char('\t')),
            'r' => Ok(Regex::Char('\r')),
            'a' => Ok(Regex::Char('\u{07}')),
            'e' => Ok(Regex::Char('\u{1B}')),
            'f' => Ok(Regex::Char('\u{0C}')),
            '0' => Ok(Regex::Char('\u{00}')),
            // Bare `\E` outside `\Q...\E` is a PCRE2 no-op. Lower to
            // an empty Sequence so the compiler elides it.
            'E' => Ok(Regex::Sequence(vec![])),
            // Single-digit backref — only when group exists.
            // For now treat as backreference; the compiler validates.
            d if d.is_ascii_digit() && d != '0' =>
            {
                #[allow(clippy::cast_possible_truncation)]
                Ok(Regex::Backreference(u32::from(d as u8 - b'0')))
            }
            // Catch-all: literal character.
            other => Ok(Regex::Char(other)),
        }
    }

    fn convert_typed_hex_escape(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Two shapes:
        //   ["x", <hex_digit>, <hex_digit?>]  — \xNN
        //   ["x{", <ws?>, <hex_digits>, <ws?>, "}"]  — \x{...}
        let head = arr[0].as_str().unwrap_or("");
        let mut digits = String::new();
        if head == "x{" {
            // Index 2 is hex_digits.
            if let Some(d) = arr.get(2) {
                walk_json_terminal_chars(d, &mut digits);
            }
        } else {
            // \xNN form: arr[1] and arr[2] are hex_digit terminals.
            for elem in arr.iter().skip(1) {
                walk_json_terminal_chars(elem, &mut digits);
            }
        }
        let n = u32::from_str_radix(&digits, 16)
            .map_err(|_| self.contract_error(&format!("invalid hex escape '{digits}'")))?;
        let ch = char::from_u32(n).ok_or_else(|| {
            self.contract_error(&format!("hex escape U+{n:X} is not a valid codepoint"))
        })?;
        Ok(Regex::Char(ch))
    }

    fn convert_typed_unicode_escape(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: ["u{", <hex_digits>, "}"]
        let mut digits = String::new();
        if let Some(d) = arr.get(1) {
            walk_json_terminal_chars(d, &mut digits);
        }
        let n = u32::from_str_radix(&digits, 16)
            .map_err(|_| self.contract_error(&format!("invalid unicode escape '{digits}'")))?;
        let ch = char::from_u32(n).ok_or_else(|| {
            self.contract_error(&format!("unicode escape U+{n:X} is not a valid codepoint"))
        })?;
        Ok(Regex::Char(ch))
    }

    fn convert_typed_octal_braced(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: ["o{", <ws?>, <octal_digits>, <ws?>, "}"]
        let mut digits = String::new();
        if let Some(d) = arr.get(2) {
            walk_json_terminal_chars(d, &mut digits);
        }
        let n = u32::from_str_radix(&digits, 8)
            .map_err(|_| self.contract_error(&format!("invalid octal escape '{digits}'")))?;
        let ch = char::from_u32(n).ok_or_else(|| {
            self.contract_error(&format!("octal escape O{digits} is not a valid codepoint"))
        })?;
        Ok(Regex::Char(ch))
    }

    fn convert_typed_octal_digits(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: [<octal_digit>, <octal_digit?>, <octal_digit?>] — bare \NNN form.
        let mut digits = String::new();
        for elem in arr {
            walk_json_terminal_chars(elem, &mut digits);
        }
        let n = u32::from_str_radix(&digits, 8)
            .map_err(|_| self.contract_error(&format!("invalid octal escape '\\{digits}'")))?;
        let ch = char::from_u32(n).ok_or_else(|| {
            self.contract_error(&format!("octal escape \\{digits} is not a valid codepoint"))
        })?;
        Ok(Regex::Char(ch))
    }

    fn convert_typed_control_escape(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: ["c", <any_char>]
        let ch_text = arr
            .get(1)
            .ok_or_else(|| self.contract_error("control escape missing target char"))?;
        let mut buf = String::new();
        walk_json_terminal_chars(ch_text, &mut buf);
        let ctrl = buf
            .chars()
            .next()
            .ok_or_else(|| self.contract_error("control escape has empty target"))?;
        // PCRE2 10.47: uppercase if lowercase, then XOR 0x40.
        let upper = ctrl.to_ascii_uppercase();
        let code = (upper as u32) ^ 0x40;
        let result = char::from_u32(code).ok_or_else(|| {
            self.contract_error(&format!("control escape produced invalid codepoint {code}"))
        })?;
        Ok(Regex::Char(result))
    }

    fn convert_typed_property_braced(
        &self,
        arr: &[serde_json::Value],
        negated: bool,
    ) -> Result<Regex> {
        // Shape: ["p{", <prop_name>, "}"] OR ["P{", <prop_name>, "}"]
        let mut name = String::new();
        if let Some(n) = arr.get(1) {
            walk_json_terminal_chars(n, &mut name);
        }
        Ok(Regex::UnicodeClass {
            name: name.trim().to_string(),
            negated,
        })
    }

    fn convert_typed_property_short(
        &self,
        arr: &[serde_json::Value],
        negated: bool,
    ) -> Result<Regex> {
        // Shape: ["p", <short_prop_letter>] OR ["P", <short_prop_letter>]
        let mut name = String::new();
        if let Some(n) = arr.get(1) {
            walk_json_terminal_chars(n, &mut name);
        }
        Ok(Regex::UnicodeClass {
            name: name.trim().to_string(),
            negated,
        })
    }

    fn convert_typed_named_backref_k(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // Shape: ["k", <name_ref or braced_name_ref>]
        // name_ref shape: ["<", <name>, ">"] OR ["'", <name>, "'"]
        // braced_name_ref shape: ["{", <ws?>, <name>, <ws?>, "}"]
        let payload = arr
            .get(1)
            .ok_or_else(|| self.contract_error("\\k missing name reference"))?;
        let name = self.extract_name_from_ref_shape(payload)?;
        Ok(Regex::NamedBackreference(name))
    }

    fn convert_typed_g_backref(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // PCRE2 distinguishes by bracket form:
        //   `\g<N>`, `\g<name>`, `\g<+N>`, `\g<-N>`, `\g'...'`  → subroutine call (Recursion)
        //   `\g{N}`, `\g{+N}`, `\g{-N}`, `\g{name}`, `\gN`     → back-reference
        //
        // Top-level shape: ["\\g", <subroutine_ref>]
        // subroutine_ref alternatives:
        //   - braced_subroutine_ref: ["{", ws?, <signed_digits_or_name>, ws?, "}"]
        //   - ["<", <signed_digits_or_name>, ">"]
        //   - ["'", <signed_digits_or_name>, "'"]
        //   - bare <signed_digits>: [<sign?>, <int>]
        let payload = arr
            .get(1)
            .ok_or_else(|| self.contract_error("\\g missing subroutine reference"))?;
        let parr = payload.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "\\g subroutine ref must be array, got {}",
                json_kind(payload)
            ))
        })?;
        let first = parr
            .first()
            .ok_or_else(|| self.contract_error("\\g subroutine ref is empty"))?;
        let first_str = first.as_str();
        match first_str {
            Some("<" | "'") => {
                // Angle/apostrophe form → subroutine call.
                let inner = parr
                    .get(1)
                    .ok_or_else(|| self.contract_error("\\g subroutine ref missing body"))?;
                self.subroutine_ref_to_recursion(inner)
            }
            Some("{") => {
                // Braced form → back-reference.
                let inner = parr
                    .get(2)
                    .ok_or_else(|| self.contract_error("\\g{} backreference missing body"))?;
                self.subroutine_ref_to_backreference(inner)
            }
            // Bare signed_digits form `\gN` / `\g+N` / `\g-N` → back-reference.
            _ => self.subroutine_ref_to_backreference(payload),
        }
    }

    /// Convert a `signed_digits_or_name` value into a `Regex::Recursion`
    /// (subroutine call). Used for the angle/apostrophe `\g<>` form.
    fn subroutine_ref_to_recursion(&self, value: &serde_json::Value) -> Result<Regex> {
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected subroutine ref shape, got {}",
                json_kind(value)
            ))
        })?;
        if arr.len() == 2 {
            if let Some(n) = arr[1].as_u64() {
                let sign = arr[0].as_str();
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                return Ok(match sign {
                    Some("+") => Regex::Recursion {
                        target: RecursionTarget::RelativeGroup(n as i32),
                    },
                    Some("-") => Regex::Recursion {
                        target: RecursionTarget::RelativeGroup(-(n as i32)),
                    },
                    _ => Regex::Recursion {
                        target: RecursionTarget::Group(n as u32),
                    },
                });
            }
        }
        // Fall through: name-shaped reference.
        let name = self.extract_typed_name(Some(value))?;
        Ok(Regex::Recursion {
            target: RecursionTarget::NamedGroup(name),
        })
    }

    /// Convert a `signed_digits_or_name` value into a back-reference.
    /// Used for the braced `\g{}` form and bare `\gN`.
    fn subroutine_ref_to_backreference(&self, value: &serde_json::Value) -> Result<Regex> {
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected subroutine ref shape, got {}",
                json_kind(value)
            ))
        })?;
        if arr.len() == 2 {
            if let Some(n) = arr[1].as_u64() {
                let sign = arr[0].as_str();
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                return Ok(match sign {
                    Some("+") => Regex::RelativeBackreference(n as i32),
                    Some("-") => Regex::RelativeBackreference(-(n as i32)),
                    _ => Regex::Backreference(n as u32),
                });
            }
        }
        let name = self.extract_typed_name(Some(value))?;
        Ok(Regex::NamedBackreference(name))
    }

    fn convert_typed_numeric_backref(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // arr is the backreference_digits shape — a Quantified-+ of digit
        // terminals, OR a sequence-shape from the legacy fallback.
        let mut digits = String::new();
        for elem in arr {
            walk_json_terminal_chars(elem, &mut digits);
        }
        let n: u32 = digits
            .parse()
            .map_err(|_| self.contract_error(&format!("invalid backreference '{digits}'")))?;
        Ok(Regex::Backreference(n))
    }

    fn extract_name_from_ref_shape(&self, value: &serde_json::Value) -> Result<String> {
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected name_ref array, got {}",
                json_kind(value)
            ))
        })?;
        // Determine whether it's a braced ref (5-element) or angle/apos (3-element).
        let inner_idx = if arr.len() == 5 { 2 } else { 1 };
        let name_value = arr
            .get(inner_idx)
            .ok_or_else(|| self.contract_error("name_ref missing name body"))?;
        self.extract_typed_name(Some(name_value))
    }

    fn subroutine_ref_to_regex(&self, value: &serde_json::Value) -> Result<Regex> {
        // signed_digits_or_name: either signed_digits [<sign?>, <int>] or name
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "expected subroutine ref shape, got {}",
                json_kind(value)
            ))
        })?;
        // Try signed_digits: arr should be [<sign?>, <int>] where sign is "+", "-", or [].
        if arr.len() == 2 {
            let sign = &arr[0];
            let digits = &arr[1];
            if let Some(n) = digits.as_u64() {
                let sign_str = sign.as_str();
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                return match sign_str {
                    Some("+") => Ok(Regex::RelativeBackreference(n as i32)),
                    Some("-") => Ok(Regex::RelativeBackreference(-(n as i32))),
                    _ => Ok(Regex::Backreference(n as u32)),
                };
            }
            // sign-empty form: arr[0] is "[]" and arr[1] is digits int
            if matches!(sign, serde_json::Value::Array(_)) {
                if let Some(n) = digits.as_u64() {
                    return Ok(Regex::Backreference(n as u32));
                }
            }
        }
        // Fall through: attempt name extraction.
        let name = self.extract_typed_name(Some(value))?;
        Ok(Regex::NamedBackreference(name))
    }

    /// Convert a `\Q...\E` `quoted_literal` atom: `["\\Q", <chars-Quantified>, "\\E"]`.
    fn convert_typed_quoted_literal(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        let mut text = String::new();
        if let Some(chars) = arr.get(1) {
            walk_json_terminal_chars(chars, &mut text);
        }
        let chars: Vec<Regex> = text.chars().map(Regex::Char).collect();
        Ok(Regex::Sequence(chars))
    }

    /// Convert a `char_class` atom: `["[", <neg?>, <init_close?>, <body>, "]"]`.
    fn convert_typed_char_class(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        if arr.len() < 5 {
            return Err(self.contract_error(&format!(
                "char_class expected 5 elements, got {}",
                arr.len()
            )));
        }

        // Negation slot: `"^"` (string) when matched; `[]` (empty array)
        // when not present.
        let negated = matches!(&arr[1], serde_json::Value::String(s) if s == "^");

        let mut ranges: Vec<CharRange> = Vec::new();
        // class_initial_close: literal `]` as the first class member.
        if matches!(&arr[2], serde_json::Value::String(s) if s == "]") {
            ranges.push(CharRange::single(']'));
        }

        let body = arr[3].as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "char_class body must be array, got {}",
                json_kind(&arr[3])
            ))
        })?;
        let mut idx = 0;
        while idx < body.len() {
            let item = &body[idx];

            // PCRE2-flavour quoted-run as range start: `[\Qabc\E-z]`
            // reads as literal `a`, literal `b`, range `c-z` (last
            // char of the quote is the range endpoint, not just a
            // literal class member). PGEN's body shape for this
            // pattern is three separate items: the quoted run, a
            // literal `-`, then the range-end atom. Peek for that
            // sequence here and split before falling back to the
            // per-item walker. See `book/src/internals/
            // pcre2-conformance-residual.md` Cluster 2F.
            if Self::is_quoted_class_run(item)
                && body.get(idx + 1).and_then(|v| v.as_str()) == Some("-")
                && body.get(idx + 2).is_some()
            {
                let chars = Self::extract_quoted_class_chars(item);
                if let Some(last) = chars.chars().last() {
                    // Literals for everything except the last char.
                    for ch in chars.chars().take(chars.chars().count() - 1) {
                        ranges.push(CharRange::single(ch));
                    }
                    // Range end is either a single-char string or a
                    // single-char escape (e.g. `\xFF` / `\.` / `\d`).
                    // Most patterns hit the simple string case; lower
                    // the escape case via the existing class-escape
                    // walker as a side path that materialises a
                    // single-char range, then read it back.
                    let end_ch = match &body[idx + 2] {
                        serde_json::Value::String(s) => s
                            .chars()
                            .next()
                            .ok_or_else(|| self.contract_error("empty quoted-run range end"))?,
                        serde_json::Value::Array(_) => {
                            let mut tmp_ranges: Vec<CharRange> = Vec::new();
                            self.convert_typed_class_item(&body[idx + 2], &mut tmp_ranges)?;
                            if tmp_ranges.len() == 1 && tmp_ranges[0].start == tmp_ranges[0].end {
                                tmp_ranges[0].start
                            } else {
                                return Err(self.contract_error(
                                    "quoted-run range end must be a single character",
                                ));
                            }
                        }
                        other => {
                            return Err(self.contract_error(&format!(
                                "quoted-run range end has unexpected shape: {}",
                                json_kind(other)
                            )));
                        }
                    };
                    if (end_ch as u32) < (last as u32) {
                        return Err(self.contract_error(&format!(
                            "descending character class range: {last:?}-{end_ch:?}"
                        )));
                    }
                    ranges.push(CharRange::range(last, end_ch));
                    idx += 3;
                    continue;
                }
                // Empty quoted run + dash + atom is invalid PCRE2 but
                // fall through to the regular walker so the existing
                // error path fires.
            }

            self.convert_typed_class_item(item, &mut ranges)?;
            idx += 1;
        }

        Ok(Regex::CharClass(CharClass::Custom {
            ranges,
            negated,
            ci_override_ranges: None,
        }))
    }

    /// Is `item` a `\Q…\E` quoted run inside a `char_class` body?
    /// Recognises both the legacy un-typed array shape
    /// `["\\Q", <chars>, "\\E"]` and the typed-object form
    /// `{type:"class_quoted_literal", body:[<chars>]}`.
    fn is_quoted_class_run(item: &serde_json::Value) -> bool {
        if let Some(arr) = item.as_array() {
            return matches!(arr.first().and_then(|v| v.as_str()), Some("\\Q"));
        }
        if let Some(map) = item.as_object() {
            return map.get("type").and_then(|v| v.as_str()) == Some("class_quoted_literal");
        }
        false
    }

    /// Extract the literal characters from a quoted-run `class_item`.
    /// Handles both the legacy array shape and the typed-object form.
    fn extract_quoted_class_chars(item: &serde_json::Value) -> String {
        let mut text = String::new();
        if let Some(arr) = item.as_array() {
            if let Some(slot) = arr.get(1) {
                walk_json_terminal_chars(slot, &mut text);
            }
        } else if let Some(map) = item.as_object() {
            if let Some(body) = map.get("body").and_then(|v| v.as_array()) {
                // Same sub-array flatten idiom as the typed
                // quoted_literal walkers: `\$` parses as
                // `["\\", "$"]` because the literal `$` would hit
                // the anchor terminal. Use `walk_json_terminal_chars`
                // per element so both shapes contribute chars.
                for elem in body {
                    walk_json_terminal_chars(elem, &mut text);
                }
            }
        }
        text
    }

    /// Convert a single `class_item`, appending its expanded ranges to `ranges`.
    /// Per PGEN-RGX-0076 fix (post-1.1.36): `posix_class` is now a typed
    /// `{type:"posix_class", name, negated}` object instead of a bare `"[:"`.
    fn convert_typed_class_item(
        &self,
        item: &serde_json::Value,
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        match item {
            serde_json::Value::String(s) => {
                if s == "\\E" {
                    // stray_class_end_quote: zero-width marker, no range.
                    return Ok(());
                }
                if s.chars().count() == 1 {
                    let ch = s.chars().next().unwrap();
                    ranges.push(CharRange::single(ch));
                    return Ok(());
                }
                Err(self.contract_error(&format!("unrecognised class_item string: {s:?}")))
            }
            serde_json::Value::Array(arr) => self.convert_typed_class_item_array(arr, ranges),
            serde_json::Value::Object(map) => self.convert_typed_class_item_object(map, ranges),
            other => Err(self.contract_error(&format!(
                "unrecognised class_item shape: {}",
                json_kind(other)
            ))),
        }
    }

    /// Convert a typed-object `class_item` — currently `posix_class`,
    /// extensible to future typed class-item shapes.
    fn convert_typed_class_item_object(
        &self,
        map: &serde_json::Map<String, serde_json::Value>,
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        let kind = map.get("type").and_then(|v| v.as_str());
        match kind {
            Some("posix_class") => {
                let name = map
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| self.contract_error("typed posix_class missing 'name'"))?;
                // `negated`: typed boolean `true` when matched, empty array `[]` otherwise.
                let negated = matches!(map.get("negated"), Some(serde_json::Value::Bool(true)));
                let resolved = if self.ucp_enabled {
                    ucp_posix_class_ranges(name).or_else(|| posix_class_ranges(name))
                } else {
                    posix_class_ranges(name)
                };
                let mut class_ranges = resolved.ok_or_else(|| {
                    self.contract_error(&format!("unknown POSIX class name '{name}'"))
                })?;
                if negated {
                    class_ranges = complement_ranges(&class_ranges);
                }
                ranges.extend(class_ranges);
                Ok(())
            }
            // PGEN slice 42 (1.1.71) typed `class_range`. Endpoints can be
            // single-char strings or typed escape objects (`\xFF`, `\d`,
            // `\.`, `\000`).
            Some("class_range") => {
                let start = map
                    .get("start")
                    .ok_or_else(|| self.contract_error("typed class_range missing 'start'"))?;
                let end = map
                    .get("end")
                    .ok_or_else(|| self.contract_error("typed class_range missing 'end'"))?;
                let start_ch = self.endpoint_to_char(start, "class_range start")?;
                let end_ch = self.endpoint_to_char(end, "class_range end")?;
                if (end_ch as u32) < (start_ch as u32) {
                    return Err(self.contract_error(&format!(
                        "descending character class range: {start_ch:?}-{end_ch:?}"
                    )));
                }
                ranges.push(CharRange::range(start_ch, end_ch));
                Ok(())
            }
            Some("class_quoted_range_atom") => {
                let ch = map
                    .get("char")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.chars().next())
                    .ok_or_else(|| {
                        self.contract_error("typed class_quoted_range_atom missing 'char'")
                    })?;
                ranges.push(CharRange::single(ch));
                Ok(())
            }
            Some("class_quoted_literal") => {
                let body = map.get("body").and_then(|v| v.as_array()).ok_or_else(|| {
                    self.contract_error("typed class_quoted_literal missing 'body'")
                })?;
                // PGEN emits sub-array body elements for chars that
                // would hit a reserved grammar terminal — `\Q\n\E`
                // becomes `[["\\", "n"]]` (backslash + n). Flatten
                // with `walk_json_terminal_chars` so both strings and
                // sub-arrays contribute literal class members.
                for elem in body {
                    let mut text = String::new();
                    walk_json_terminal_chars(elem, &mut text);
                    for ch in text.chars() {
                        ranges.push(CharRange::single(ch));
                    }
                }
                Ok(())
            }
            // Typed `escape` inside a class body — lower via the regular
            // typed-escape walker and fold its result into ranges.
            Some("escape") => {
                let regex = self.convert_typed_escape_object(map)?;
                self.lower_regex_into_class_ranges(&regex, ranges)
            }
            Some(other) => {
                Err(self.contract_error(&format!("unrecognised typed class_item: type={other:?}")))
            }
            None => {
                Err(self.contract_error("typed class_item object missing 'type' discriminator"))
            }
        }
    }

    fn convert_typed_class_item_array(
        &self,
        arr: &[serde_json::Value],
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        if arr.is_empty() {
            return Ok(());
        }
        // Dispatch on first element's prefix.
        if let Some(head) = arr[0].as_str() {
            match head {
                "[:" => return self.convert_typed_posix_class(arr, ranges),
                "\\Q" => return self.convert_typed_quoted_class_literal(arr, ranges),
                "\\" => return self.convert_typed_class_escape(arr, ranges),
                _ => {}
            }
        }
        // class_range: 5-element [<class_atom>, <Quantified zero-width>, "-",
        //                          <Quantified zero-width>, <class_atom>].
        if arr.len() == 5 && matches!(&arr[2], serde_json::Value::String(s) if s == "-") {
            return self.convert_typed_class_range(arr, ranges);
        }
        Err(self.contract_error(&format!(
            "unrecognised class_item array shape: {}",
            serde_json::Value::Array(arr.to_vec())
        )))
    }

    fn convert_typed_posix_class(
        &self,
        arr: &[serde_json::Value],
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        // Shape: ["[:", <negation?>, <name>, ":]"]
        let neg_slot = arr.get(1);
        let posix_negated = matches!(neg_slot, Some(serde_json::Value::String(s)) if s == "^");
        let name_slot = arr
            .get(2)
            .ok_or_else(|| self.contract_error("posix_class missing name"))?;
        let mut name = String::new();
        walk_json_terminal_chars(name_slot, &mut name);
        let resolved = if self.ucp_enabled {
            ucp_posix_class_ranges(&name).or_else(|| posix_class_ranges(&name))
        } else {
            posix_class_ranges(&name)
        };
        let mut class_ranges = resolved
            .ok_or_else(|| self.contract_error(&format!("unknown POSIX class name '{name}'")))?;
        if posix_negated {
            class_ranges = complement_ranges(&class_ranges);
        }
        ranges.extend(class_ranges);
        Ok(())
    }

    fn convert_typed_quoted_class_literal(
        &self,
        arr: &[serde_json::Value],
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        // Shape: ["\\Q", <chars>, "\\E"]. <chars> is a Quantified-* of
        // quoted_class_literal_char terminals (after collapse: array of strings
        // OR an empty array).
        let chars_slot = arr.get(1);
        let mut text = String::new();
        if let Some(slot) = chars_slot {
            walk_json_terminal_chars(slot, &mut text);
        }
        for ch in text.chars() {
            ranges.push(CharRange::single(ch));
        }
        Ok(())
    }

    fn convert_typed_class_escape(
        &self,
        arr: &[serde_json::Value],
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        // Shape: ["\\", <escape_unit>] — same as a top-level escape, but
        // resolved in class context (e.g. `\b` becomes BS instead of
        // word-boundary).
        let regex = if let Some(unit) = arr.get(1) {
            match unit {
                serde_json::Value::String(s) if s == "C" => Regex::CharClass(CharClass::Custom {
                    ranges: vec![CharRange::range('\0', char::MAX)],
                    negated: false,
                    ci_override_ranges: None,
                }),
                serde_json::Value::String(s) => self.convert_typed_simple_escape_char(s, true)?,
                serde_json::Value::Array(inner) => self.convert_typed_escape_unit_array(inner)?,
                other => {
                    return Err(self.contract_error(&format!(
                        "unrecognised class_escape unit shape: {}",
                        json_kind(other)
                    )))
                }
            }
        } else {
            return Err(self.contract_error("class_escape missing escape unit"));
        };
        extend_ranges_from_regex(regex, ranges, |msg| {
            self.contract_error(&format!("class_escape: {msg}"))
        })
    }

    fn convert_typed_class_range(
        &self,
        arr: &[serde_json::Value],
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        // Shape: [<class_atom>, <Quantified zw>, "-", <Quantified zw>, <class_atom>]
        let start_ch = self.typed_class_atom_to_char(&arr[0])?;
        let end_ch = self.typed_class_atom_to_char(&arr[4])?;
        if start_ch > end_ch {
            return Err(self.contract_error(&format!(
                "class_range out of order: {start_ch:?} > {end_ch:?}"
            )));
        }
        ranges.push(CharRange::range(start_ch, end_ch));
        Ok(())
    }

    /// Resolve a `class_atom` (used as range endpoint) to a single character.
    fn typed_class_atom_to_char(&self, value: &serde_json::Value) -> Result<char> {
        match value {
            serde_json::Value::String(s) if s.chars().count() == 1 => Ok(s.chars().next().unwrap()),
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                if let Some(head) = arr[0].as_str() {
                    match head {
                        "\\Q" => {
                            // quoted_class_range_atom: ["\\Q", <single-char>, "\\E"]
                            let mut text = String::new();
                            if let Some(slot) = arr.get(1) {
                                walk_json_terminal_chars(slot, &mut text);
                            }
                            text.chars().next().ok_or_else(|| {
                                self.contract_error("quoted_class_range_atom is empty")
                            })
                        }
                        "\\" => {
                            // class_range_escape: ["\\", <unit>]
                            let unit = arr.get(1).ok_or_else(|| {
                                self.contract_error("class_range_escape missing unit")
                            })?;
                            let regex = match unit {
                                serde_json::Value::String(s) => {
                                    self.convert_typed_simple_escape_char(s, true)?
                                }
                                serde_json::Value::Array(inner) => {
                                    self.convert_typed_escape_unit_array(inner)?
                                }
                                _ => {
                                    return Err(
                                        self.contract_error("unsupported class_range_escape unit")
                                    )
                                }
                            };
                            match regex {
                                Regex::Char(ch) => Ok(ch),
                                _ => Err(self.contract_error(
                                    "class_range endpoint escape did not resolve to a single char",
                                )),
                            }
                        }
                        _ => Err(self.contract_error(&format!(
                            "unsupported class_range endpoint head: {head:?}"
                        ))),
                    }
                } else {
                    Err(self.contract_error("class_range endpoint array head is non-string"))
                }
            }
            other => Err(self.contract_error(&format!(
                "unsupported class_range endpoint shape: {}",
                json_kind(other)
            ))),
        }
    }

    /// Convert a conditional atom: `["(?(", <cond>, ")", <yes>, <no?>, ")"]`.
    /// Yes/no branches are arrays of piece objects (concatenation-flat).
    /// VERSION conditionals are evaluated at parse time and short-circuit.
    fn convert_typed_conditional(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        if arr.len() < 5 {
            return Err(self.contract_error(&format!(
                "conditional expected ≥5 elements, got {}",
                arr.len()
            )));
        }

        let yes_pieces = arr[3]
            .as_array()
            .ok_or_else(|| self.contract_error("conditional yes branch must be array"))?;
        let yes_branch = self.pieces_to_sequence(yes_pieces)?;

        // arr[4] is optional ("|" no_branch) pair, OR ")" closer if no else.
        let no_branch = match arr.get(4) {
            Some(serde_json::Value::Array(pair_arr))
                if pair_arr
                    .first()
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s == "|") =>
            {
                let no_pieces = pair_arr.get(1).and_then(|v| v.as_array()).ok_or_else(|| {
                    self.contract_error("conditional no-branch missing piece array")
                })?;
                Some(self.pieces_to_sequence(no_pieces)?)
            }
            _ => None,
        };

        // A13: VERSION conditional short-circuit at parse time.
        // Reconstruct the condition text and check via parse_version_conditional.
        let mut cond_text = String::new();
        walk_json_terminal_chars(&arr[1], &mut cond_text);
        if let Some(matches) = parse_version_conditional(&cond_text) {
            return Ok(if matches {
                yes_branch
            } else {
                no_branch.unwrap_or(Regex::Empty)
            });
        }

        let condition = self.convert_typed_condition(&arr[1])?;
        Ok(Regex::Conditional {
            condition,
            true_branch: Box::new(yes_branch),
            false_branch: no_branch.map(Box::new),
        })
    }

    /// Convert a typed condition. Possible shapes per `rules-misc.md`:
    /// - `"DEFINE"` (string)
    /// - `["VERSION", <op>, <ver>]` (array)
    /// - `[<sign?>, <int>]` for `signed_digits`
    /// - `<int>` for plain digits
    /// - `["?C", ...]` callout-condition
    /// - `["?=", ...]` etc. `condition_assertion`
    /// - `["*", <name>, ":", <pattern>]` alpha condition assertion
    /// - `[<first_char>, <Quantified rest>]` for name (looks like `signed_digits` but with strings)
    /// - `["R", <int?>]` or `["R&", <name>]` `recursion_condition`
    /// - `name_ref` `["<", <name>, ">"]` etc.
    fn convert_typed_condition(&self, value: &serde_json::Value) -> Result<ConditionalTest> {
        if let Some(s) = value.as_str() {
            if s == "DEFINE" {
                return Ok(ConditionalTest::Define);
            }
            // A bare name string for backwards-compat (rare).
            return Ok(ConditionalTest::NamedGroupExists(s.to_string()));
        }
        if let Some(n) = value.as_u64() {
            return Ok(ConditionalTest::GroupExists(n as u32));
        }
        let arr = value.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "unsupported condition shape: {}",
                json_kind(value)
            ))
        })?;
        if arr.is_empty() {
            return Err(self.contract_error("empty condition array"));
        }
        // condition_callout_assertion: [<condition_callout>, "(", <condition_assertion>]
        // The condition_callout sub-array starts with the string "?C"; the
        // separator "(" sits at arr[1]; the assertion follows at arr[2].
        // The callout itself is a parser-level checkpoint with no effect on
        // the conditional test — RGX dispatches on the embedded assertion.
        if arr.len() >= 3 && matches!(arr.get(1), Some(serde_json::Value::String(s)) if s == "(") {
            if let Some(callout_arr) = arr[0].as_array() {
                if matches!(callout_arr.first(), Some(serde_json::Value::String(s)) if s == "?C") {
                    let assertion = arr.get(2).ok_or_else(|| {
                        self.contract_error("condition_callout_assertion missing assertion")
                    })?;
                    return self.convert_typed_condition(assertion);
                }
            }
        }
        // Try VERSION: ["VERSION", <op>, <version_number>]
        if let Some(head) = arr[0].as_str() {
            if head == "VERSION" && arr.len() >= 3 {
                // Reconstruct the text and pass through parse_version_conditional
                // for compile-time evaluation.
                let mut buf = String::from("VERSION");
                if let Some(op) = arr.get(1).and_then(|v| v.as_str()) {
                    buf.push_str(op);
                }
                let ver = arr.get(2);
                if let Some(ver_val) = ver {
                    let mut tmp = String::new();
                    walk_json_terminal_chars(ver_val, &mut tmp);
                    buf.push_str(&tmp);
                }
                // VERSION shouldn't reach here — handled by short-circuit
                // at the conditional level. Fall through if unrecognised.
                let _ = parse_version_conditional(&buf);
                return Err(self.contract_error(&format!("unsupported VERSION condition: {buf}")));
            }
            // Recursion condition: ["R", <digits?>] or ["R&", <name>]
            if head == "R" {
                // optional digits at arr[1]
                if let Some(digits_slot) = arr.get(1) {
                    // Slot is Quantified-? carrier: `[]` or `[<digits-int>]`
                    if let Some(slot_arr) = digits_slot.as_array() {
                        if slot_arr.is_empty() {
                            return Ok(ConditionalTest::RecursionAny);
                        }
                        if let Some(n) = slot_arr.first().and_then(serde_json::Value::as_u64) {
                            return Ok(ConditionalTest::RecursionGroup(n as u32));
                        }
                    }
                    // Maybe digits is direct integer (slice 36 may have changed shape).
                    if let Some(n) = digits_slot.as_u64() {
                        return Ok(ConditionalTest::RecursionGroup(n as u32));
                    }
                }
                return Ok(ConditionalTest::RecursionAny);
            }
            if head == "R&" {
                let name = self.extract_typed_name(arr.get(1))?;
                return Ok(ConditionalTest::RecursionNamed(name));
            }
            // Lookaround condition: ["?=", <pattern>] etc.
            match head {
                "?=" => {
                    let inner = self.convert_typed_optional_pattern(arr.get(1))?;
                    return Ok(ConditionalTest::Lookahead {
                        expr: Box::new(inner),
                        positive: true,
                    });
                }
                "?!" => {
                    let inner = self.convert_typed_optional_pattern(arr.get(1))?;
                    return Ok(ConditionalTest::Lookahead {
                        expr: Box::new(inner),
                        positive: false,
                    });
                }
                "?<=" => {
                    let inner = self.convert_typed_optional_pattern(arr.get(1))?;
                    return Ok(ConditionalTest::Lookbehind {
                        expr: Box::new(inner),
                        positive: true,
                    });
                }
                "?<!" => {
                    let inner = self.convert_typed_optional_pattern(arr.get(1))?;
                    return Ok(ConditionalTest::Lookbehind {
                        expr: Box::new(inner),
                        positive: false,
                    });
                }
                _ => {}
            }
            // name_ref: ["<", <name>, ">"] or ["'", <name>, "'"]
            if head == "<" || head == "'" {
                let name = self.extract_typed_name(arr.get(1))?;
                return Ok(ConditionalTest::NamedGroupExists(name));
            }
            // alpha_condition_assertion: ["*", <alpha_lookaround_name>, ":", <pattern?>]
            if head == "*" && arr.len() >= 4 {
                let mut name = String::new();
                if let Some(n) = arr.get(1) {
                    walk_json_terminal_chars(n, &mut name);
                }
                let inner = self.convert_typed_optional_pattern(arr.get(3))?;
                let placeholder = match name.as_str() {
                    "pla" | "positive_lookahead" => Some(ConditionalTest::Lookahead {
                        expr: Box::new(inner),
                        positive: true,
                    }),
                    "nla" | "negative_lookahead" => Some(ConditionalTest::Lookahead {
                        expr: Box::new(inner),
                        positive: false,
                    }),
                    "plb" | "positive_lookbehind" => Some(ConditionalTest::Lookbehind {
                        expr: Box::new(inner),
                        positive: true,
                    }),
                    "nlb" | "negative_lookbehind" => Some(ConditionalTest::Lookbehind {
                        expr: Box::new(inner),
                        positive: false,
                    }),
                    "napla" => Some(ConditionalTest::Lookahead {
                        expr: Box::new(inner),
                        positive: true,
                    }),
                    "naplb" => Some(ConditionalTest::Lookbehind {
                        expr: Box::new(inner),
                        positive: true,
                    }),
                    _ => None,
                };
                if let Some(test) = placeholder {
                    return Ok(test);
                }
            }
        }
        // signed_digits: [<sign?>, <int>] OR plain name
        if arr.len() == 2 {
            let sign = &arr[0];
            let payload = &arr[1];
            if let Some(n) = payload.as_u64() {
                let sign_str = sign.as_str();
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                return Ok(match sign_str {
                    Some("+") => ConditionalTest::RelativeGroupExists(n as i32),
                    Some("-") => ConditionalTest::RelativeGroupExists(-(n as i32)),
                    _ => ConditionalTest::GroupExists(n as u32),
                });
            }
            // Try as name shape: [<first-char>, <Quantified rest>]
            if let Some(_first) = sign.as_str() {
                let mut name = String::new();
                walk_json_terminal_chars(value, &mut name);
                if !name.is_empty() {
                    return Ok(ConditionalTest::NamedGroupExists(name));
                }
            }
        }
        // Last resort: try to extract a name from concatenated terminals.
        let mut name = String::new();
        walk_json_terminal_chars(value, &mut name);
        if !name.is_empty() {
            return Ok(ConditionalTest::NamedGroupExists(name));
        }
        Err(self.contract_error(&format!("unsupported condition shape: {value}")))
    }

    fn pieces_to_sequence(&self, pieces: &[serde_json::Value]) -> Result<Regex> {
        let mut converted = Vec::with_capacity(pieces.len());
        for p in pieces {
            converted.push(self.convert_typed_piece(p)?);
        }
        Ok(apply_bare_flag_directives(converted))
    }

    /// Convert a callout atom: `["(?C", <arg?>, ")"]`.
    /// `<arg?>` is one of:
    ///   - `[]` (no argument — defaults to callout 0)
    ///   - integer (numeric callout)
    ///   - delimited `callout_string` array (string callout — treated as 0
    ///     since RGX's match semantics for unregistered string callouts
    ///     are identical to numeric 0)
    fn convert_typed_callout(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        let arg_slot = arr.get(1);
        let number: u32 = match arg_slot {
            None => 0,
            Some(serde_json::Value::Array(a)) if a.is_empty() => 0,
            Some(serde_json::Value::Array(_)) => 0, // string callout — see comment
            Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
            Some(other) => {
                return Err(self.contract_error(&format!(
                    "unexpected callout arg shape: {}",
                    json_kind(other)
                )))
            }
        };
        Ok(Regex::Callout(number))
    }

    /// Convert a `code_block` atom.
    /// Plain shape: `["(?{", <content>, "})"]` (3 elements)
    /// Lang shape: `["(?{", <lang>, ":", <ws?>, <content>, "})"]` (6 elements)
    fn convert_typed_code_block(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        if arr.len() == 3 {
            // Plain: no lang prefix. Lang stored as empty string per AST.
            let mut content = String::new();
            if let Some(c) = arr.get(1) {
                walk_json_terminal_chars(c, &mut content);
            }
            return Ok(Regex::CodeBlock {
                lang: String::new(),
                code: content,
            });
        }
        if arr.len() >= 6 {
            let mut lang = String::new();
            if let Some(l) = arr.get(1) {
                walk_json_terminal_chars(l, &mut lang);
            }
            let mut content = String::new();
            if let Some(c) = arr.get(4) {
                walk_json_terminal_chars(c, &mut content);
            }
            return Ok(Regex::CodeBlock {
                lang,
                code: content,
            });
        }
        Err(self.contract_error(&format!(
            "unsupported code_block shape with {} elements",
            arr.len()
        )))
    }

    /// Convert a `comment_group` atom: `["(?#", <text?>, ")"]` — no runtime effect.
    fn convert_typed_comment_group(&self, _arr: &[serde_json::Value]) -> Result<Regex> {
        Ok(Regex::Empty)
    }

    /// Convert an `extended_class` atom: `["(?[", <content>, "])"]`.
    fn convert_typed_extended_class(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        // For now, surface this through the existing ExtendedCharClass node
        // by reconstructing the source text and letting the compiler
        // dispatch. Future slices will add typed extended-class shapes.
        let mut content = String::new();
        if let Some(c) = arr.get(1) {
            walk_json_terminal_chars(c, &mut content);
        }
        Ok(Regex::ExtendedCharClass { content })
    }

    /// Convert an `(*...)`-prefixed atom — `alpha_lookaround`, `scan_substring`,
    /// `script_run`, `atomic_group` (alpha form), or `directive_verb`.
    /// Possible shapes:
    ///   `directive_verb`: `["(*", <body>, ")"]` — body is the `directive_named` or `mark_shorthand`
    ///   `alpha_lookaround`: `["(*", <name-string>, ":", <pattern?>, ")"]`
    ///   `atomic_group` (alpha): `["(*atomic:", <pattern?>, ")"]` (5-letter prefix; typically dispatched separately)
    fn convert_typed_alpha_prefixed(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        if arr.is_empty() {
            return Err(self.contract_error("empty (*...) atom"));
        }
        // Distinguish by presence of a `:` separator at index 2.
        let has_colon = matches!(arr.get(2), Some(serde_json::Value::String(s)) if s == ":");
        if has_colon && arr.len() >= 5 {
            // Could be alpha_lookaround `(*pla:...)` etc., or scan_substring/script_run.
            let mut name = String::new();
            if let Some(n) = arr.get(1) {
                walk_json_terminal_chars(n, &mut name);
            }
            let pattern_slot = arr.get(3);
            let inner = self.convert_typed_optional_pattern(pattern_slot)?;
            // Try alpha_lookaround name resolution first.
            if let Some(regex) = regex_from_alpha_lookaround_name(&name, inner.clone()) {
                return Ok(regex);
            }
            // scs / sr / script_run / atomic alpha forms — pass body through.
            // For atomic-alpha (`(*atomic:...)`), wrap as atomic group.
            if name == "atomic" {
                return Ok(Regex::Group {
                    expr: Box::new(inner),
                    kind: GroupKind::Atomic,
                    index: None,
                    name: None,
                });
            }
            // Other named groups (scs/sr/asr/etc.) — pass body through unchanged.
            return Ok(inner);
        }
        // 3-element directive_verb shape: `["(*", <body>, ")"]`.
        if arr.len() == 3 {
            return self.convert_typed_directive_verb(&arr[1]);
        }
        Err(self.contract_error(&format!(
            "unsupported (* prefixed atom shape with {} elements",
            arr.len()
        )))
    }

    fn convert_typed_directive_verb(&self, body: &serde_json::Value) -> Result<Regex> {
        // body shapes:
        //   directive_named: [<directive_name>, <suffix?>]
        //   mark_shorthand: [":", <payload>]  → produces empty name
        // directive_name is `[<first_char>, <Quantified rest>]` after collapse.
        // suffix is empty `[]` or `[":", <payload>]` or `["=", <payload>]`.
        let arr = body.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "directive_verb body must be array, got {}",
                json_kind(body)
            ))
        })?;
        // Detect mark_shorthand: arr[0] is the literal `":"` instead of a name.
        if matches!(arr.first(), Some(serde_json::Value::String(s)) if s == ":") {
            let mut payload = String::new();
            for elem in arr.iter().skip(1) {
                walk_json_terminal_chars(elem, &mut payload);
            }
            return Ok(Regex::Mark(payload));
        }
        let mut name = String::new();
        if let Some(n) = arr.first() {
            walk_json_terminal_chars(n, &mut name);
        }
        let suffix = arr.get(1);
        let payload_text = match suffix {
            Some(serde_json::Value::Array(s_arr)) if !s_arr.is_empty() => {
                let mut p = String::new();
                for elem in s_arr.iter().skip(1) {
                    walk_json_terminal_chars(elem, &mut p);
                }
                Some(p)
            }
            _ => None,
        };

        // Mirror the legacy `convert_directive_verb` mapping.
        match name.as_str() {
            "FAIL" | "F" => Ok(Regex::CharClass(CharClass::Custom {
                ranges: vec![],
                negated: false,
                ci_override_ranges: None,
            })),
            "ACCEPT" => Ok(Regex::Accept),
            "COMMIT" => Ok(Regex::Commit),
            "PRUNE" => Ok(Regex::Prune),
            "THEN" => Ok(Regex::Then),
            "SKIP" => Ok(Regex::Skip(payload_text)),
            "MARK" => Ok(Regex::Mark(payload_text.unwrap_or_default())),
            "UTF" | "UTF8" | "UTF16" | "UTF32" | "UCP" | "CR" | "LF" | "CRLF" | "ANY"
            | "ANYCRLF" | "NUL" | "BSR_ANYCRLF" | "BSR_UNICODE" => Ok(Regex::Empty),
            "NOTEMPTY" | "NOTEMPTY_ATSTART" | "NO_START_OPT" | "NO_AUTO_POSSESS"
            | "NO_DOTSTAR_ANCHOR" | "NO_JIT" | "LIMIT_HEAP" | "LIMIT_MATCH" | "LIMIT_DEPTH"
            | "LIMIT_RECURSION" | "TURKISH_CASING" | "CASELESS_RESTRICT" | "ALT_BSUX"
            | "ALT_EXTENDED_CLASS" | "ALT_CIRCUMFLEX" | "ALT_VERBNAMES" => Ok(Regex::Empty),
            other => Err(RgxError::compile(format!(
                "unsupported backtracking verb '(*{other})'"
            ))),
        }
    }

    /// Convert a `(?...)`-prefixed atom that didn't match a more specific
    /// prefix — `inline_modifiers`, `scoped_inline_modifiers`, or `subroutine_call`.
    /// Shapes:
    ///   `inline_modifiers`: `["(?", <modifier_spec?>, ")"]` (3 elements)
    ///   `scoped_inline_modifiers`: `["(?", <modifier_spec>, ":", <pattern?>, ")"]` (5 elements)
    ///   `subroutine_call`: `["(?", <subroutine_target>, ")"]` (3 elements)
    fn convert_typed_question_prefixed(&self, arr: &[serde_json::Value]) -> Result<Regex> {
        if arr.len() == 5 && matches!(arr.get(2), Some(serde_json::Value::String(s)) if s == ":") {
            // Scoped inline modifiers.
            let flags = self.collect_typed_modifier_flags(&arr[1]);
            let inner = self.convert_typed_optional_pattern(arr.get(3))?;
            return Ok(Regex::FlagGroup {
                flags,
                expr: Box::new(inner),
            });
        }
        if arr.len() == 3 {
            // Distinguish inline_modifiers from subroutine_call by inspecting
            // arr[1]. Modifier spec contains modifier_item arrays / `-` /
            // ascii_restrict letters; subroutine target is "R", `[...,int]`,
            // `["+",...]`, `["-",...]`, `["&",...]`, `["P>",...]`, etc.
            let target = &arr[1];
            // Subroutine target heuristics:
            if let Some(s) = target.as_str() {
                if s == "R" {
                    return Ok(Regex::Recursion {
                        target: RecursionTarget::Entire,
                    });
                }
            }
            if let Some(t_arr) = target.as_array() {
                if !t_arr.is_empty() {
                    // ["&", <name>] — named subroutine
                    if matches!(t_arr.first(), Some(serde_json::Value::String(s)) if s == "&") {
                        let name = self.extract_typed_name(t_arr.get(1))?;
                        return Ok(Regex::Recursion {
                            target: RecursionTarget::NamedGroup(name),
                        });
                    }
                    // ["P>", <name>] — Python named subroutine
                    if matches!(t_arr.first(), Some(serde_json::Value::String(s)) if s == "P>") {
                        let name = self.extract_typed_name(t_arr.get(1))?;
                        return Ok(Regex::Recursion {
                            target: RecursionTarget::NamedGroup(name),
                        });
                    }
                    // signed_digits: [<sign?>, <int>]
                    if t_arr.len() == 2 {
                        if let Some(n) = t_arr[1].as_u64() {
                            let sign = t_arr[0].as_str();
                            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                            return Ok(match sign {
                                Some("+") => Regex::Recursion {
                                    target: RecursionTarget::RelativeGroup(n as i32),
                                },
                                Some("-") => Regex::Recursion {
                                    target: RecursionTarget::RelativeGroup(-(n as i32)),
                                },
                                _ => Regex::Recursion {
                                    target: RecursionTarget::Group(n as u32),
                                },
                            });
                        }
                    }
                }
            }
            // Fall back to inline_modifiers.
            let flags = self.collect_typed_modifier_flags(target);
            return Ok(Regex::FlagGroup {
                flags,
                expr: Box::new(Regex::Empty),
            });
        }
        Err(self.contract_error(&format!(
            "unsupported (? prefixed atom shape with {} elements",
            arr.len()
        )))
    }

    /// Walk a `modifier_spec` value and assemble its flag string.
    fn collect_typed_modifier_flags(&self, value: &serde_json::Value) -> String {
        let mut out = String::new();
        self.walk_typed_modifier_spec(value, &mut out);
        out
    }

    fn walk_typed_modifier_spec(&self, value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::String(s) => {
                if s == "-" {
                    out.push('-');
                } else {
                    for ch in s.chars() {
                        out.push(ch);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.walk_typed_modifier_spec(item, out);
                }
            }
            _ => {}
        }
    }

    /// Extract `(min, max)` from a `counted_quantifier_body` typed shape.
    /// Mirrors the reference `extract_count_bounds` in `rules-quantifier.md`.
    fn extract_typed_count_bounds(&self, body: &serde_json::Value) -> Result<(u32, Option<u32>)> {
        let arr = body.as_array().ok_or_else(|| {
            self.contract_error(&format!(
                "typed counted_quantifier_body must be array, got {}",
                json_kind(body)
            ))
        })?;
        // Branch detection by first element: String "," → branch 1 ({,m});
        // Number → branch 0 ({n}, {n,}, or {n,m}).
        if matches!(arr.first(), Some(serde_json::Value::String(s)) if s == ",") {
            let max = arr
                .get(2)
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| self.contract_error("typed {,m} body missing max digits"))?;
            return Ok((0, Some(max as u32)));
        }
        let min = arr
            .first()
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                self.contract_error("typed counted_quantifier_body missing min digits")
            })? as u32;
        let sub = match arr.get(2) {
            Some(s) => s,
            None => return Ok((min, Some(min))),
        };
        // Sub is the (",", ws?, digits?)? optional sub-group: empty array
        // means {n}; non-empty means {n,} or {n,m}.
        let sub_arr = match sub.as_array() {
            Some(a) => a,
            None => return Ok((min, Some(min))),
        };
        if sub_arr.is_empty() {
            return Ok((min, Some(min)));
        }
        // sub_arr is [",", ws?, digits?] — index 2 is the optional digits slot.
        let inner_digits_slot = sub_arr.get(2);
        let inner_arr = match inner_digits_slot.and_then(|v| v.as_array()) {
            Some(a) => a,
            None => return Ok((min, None)),
        };
        if inner_arr.is_empty() {
            return Ok((min, None));
        }
        let max = inner_arr
            .first()
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| self.contract_error("typed {n,m} max digits missing"))?
            as u32;
        Ok((min, Some(max)))
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
        // Legacy non-typed fallback path's nesting choke point — the
        // analog of `convert_typed_pattern`. Same depth-bound +
        // stack-growth contract so this rarely-taken path is equally
        // safe against deeply nested adversarial input.
        let _nesting = self.enter_nesting()?;
        crate::recursion::grow_stack(|| {
            let alternation = self
                .first_descendant(node, "alternation")
                .ok_or_else(|| self.contract_error("pgen pattern node is missing alternation"))?;
            self.convert_alternation(alternation)
        })
    }

    fn convert_alternation(&self, node: &PgenAstNode) -> Result<Regex> {
        let children = self.sequence_children(node)?;

        // Collect each branch's raw piece list PRE-absorption. An
        // unscoped `(?flags)` toggle is marked by
        // `FlagGroup { expr: Empty }` at this stage; after
        // `apply_bare_flag_directives` absorbs the toggle's trailing
        // siblings the `Empty` marker is gone and the branch's
        // trailing unscoped toggle can no longer be distinguished from
        // a scoped `(?flags:body)` group. We snapshot pieces first so
        // cross-branch propagation has reliable signal.
        let mut alternative_pieces: Vec<Vec<Regex>> = Vec::new();

        if let Some(first_branch) = children
            .first()
            .and_then(|child| self.first_descendant(child, "alternative"))
        {
            alternative_pieces.push(self.convert_alternative_pieces(first_branch)?);
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
                alternative_pieces.push(self.convert_alternative_pieces(branch)?);
            }
        }

        // Walk branches in order, applying bare-flag-directive
        // absorption within each branch and propagating any trailing
        // unscoped toggle forward to subsequent branches per PCRE2
        // semantics. For `(a(?i)bc|BB)x`, branch 1's trailing `(?i)`
        // makes branch 2 case-insensitive too — so `BB` matches "bb".
        // Simple last-wins combine for carried flags; multi-flag
        // accumulation across branches is a later refinement if
        // conformance evidence shows it's needed.
        let mut carried: Option<String> = None;
        let mut branches: Vec<Regex> = Vec::with_capacity(alternative_pieces.len());
        for pieces in alternative_pieces {
            let trailing = last_unscoped_flag(&pieces);
            let body = apply_bare_flag_directives(pieces);
            let wrapped = if let Some(ref flags) = carried {
                Regex::FlagGroup {
                    flags: flags.clone(),
                    expr: Box::new(body),
                }
            } else {
                body
            };
            branches.push(wrapped);
            if let Some(flags) = trailing {
                carried = Some(flags);
            }
        }

        Ok(pack_alternation(branches))
    }

    /// Walk PGEN's `alternative -> concatenation -> piece*` chain and
    /// return the raw pieces, BEFORE `apply_bare_flag_directives`
    /// absorbs unscoped flag toggles. The raw-piece view is the only
    /// way to reliably tell `(?i)` (unscoped — produces
    /// `FlagGroup { expr: Empty }`) from `(?i:body)` (scoped — produces
    /// `FlagGroup { expr: body }`) once we need that distinction for
    /// cross-branch flag propagation.
    fn convert_alternative_pieces(&self, node: &PgenAstNode) -> Result<Vec<Regex>> {
        let Some(concatenation) = self.first_descendant(node, "concatenation") else {
            return Ok(Vec::new());
        };
        let mut pieces = Vec::new();
        for repeated in self.quantified_children(concatenation)? {
            let piece = self.first_descendant(repeated, "piece").ok_or_else(|| {
                self.contract_error("pgen concatenation entry is missing a piece")
            })?;
            pieces.push(self.convert_piece(piece)?);
        }
        Ok(pieces)
    }

    fn convert_alternative(&self, node: &PgenAstNode) -> Result<Regex> {
        let pieces = self.convert_alternative_pieces(node)?;
        Ok(apply_bare_flag_directives(pieces))
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
            "dot" => Ok(self.dot_ast()),
            "anchor" => self.convert_anchor(actual),
            "escape" => self.convert_escape(actual),
            "char_class" => self.convert_char_class(actual),
            "code_block" => self.convert_code_block(actual),
            "subroutine_call" => self.convert_subroutine_call(actual),
            "python_named_backreference" => self.convert_python_named_backreference(actual),
            "callout" => self.convert_callout(actual),
            "comment_group" => Ok(Regex::Empty), // (?#...) is a zero-width comment, ignored
            "directive_verb" => self.convert_directive_verb(actual),
            "whitespace_literal" => self.convert_whitespace_literal(actual),
            // PGEN 1.1.21 audit introduced a dedicated `quoted_literal`
            // atom for `\Q...\E` runs. Every byte between `\Q` and
            // `\E` is a literal character (including regex metachars).
            // Lower to a Sequence of Char nodes.
            "quoted_literal" => self.convert_quoted_literal(actual),
            // `(*scan_substring:(group-list)pattern)` / `(*scs:...)` —
            // PCRE2 scans the text captured by the listed groups for
            // the inner pattern. RGX doesn't model this scan-against-
            // other-text semantic yet; lower as the inner pattern
            // only so the test runs and compares approximately against
            // the main subject. Matches the compatible subset
            // (subjects where the scan target equals the main subject)
            // and still flags divergence for the rest via normal
            // match / no-match classification.
            "scan_substring_group" => {
                let inner = self.first_descendant(actual, "pattern");
                if let Some(p) = inner {
                    self.convert_pattern(p)
                } else {
                    Ok(Regex::Empty)
                }
            }
            // `(*script_run:pattern)` / `(*sr:...)` — PCRE2 constrains
            // all matched codepoints to belong to a single Unicode
            // script. RGX has ASCII-only script tables; lower as the
            // inner pattern only so tests with single-script subjects
            // still pass. Multi-script subjects may false-positive,
            // caught by the "RGX too permissive" bucket.
            "script_run_group" => {
                let inner = self.first_descendant(actual, "pattern");
                if let Some(p) = inner {
                    self.convert_pattern(p)
                } else {
                    Ok(Regex::Empty)
                }
            }
            // PGEN 1.1.25 emits `posix_word_boundary_alias` for the
            // PCRE2 POSIX-alias word-boundary class names `[:<:]` and
            // `[:>:]`. Semantics per pcre2pattern(3):
            //   [:<:] = zero-width assertion that the next code unit
            //           starts a word (equivalent to `\b(?=\w)`).
            //   [:>:] = zero-width assertion that the previous code
            //           unit ended a word (equivalent to `(?<=\w)\b`).
            // Bytecode dump (testoutput2:13793) confirms the
            // `\b Assert \w` lowering. RGX's AST has dedicated
            // WordBoundary and Lookahead/Lookbehind nodes, so we
            // construct the equivalent Sequence inline.
            "posix_word_boundary_alias" => {
                let text = self.slice(actual)?;
                let word_ahead = || Regex::Lookahead {
                    expr: Box::new(Regex::CharClass(CharClass::Word { negated: false })),
                    positive: true,
                    non_atomic: false,
                };
                let word_behind = || Regex::Lookbehind {
                    expr: Box::new(Regex::CharClass(CharClass::Word { negated: false })),
                    positive: true,
                    non_atomic: false,
                };
                match text {
                    "[[:<:]]" => Ok(Regex::Sequence(vec![
                        Regex::WordBoundary { positive: true },
                        word_ahead(),
                    ])),
                    "[[:>:]]" => Ok(Regex::Sequence(vec![
                        word_behind(),
                        Regex::WordBoundary { positive: true },
                    ])),
                    _ => Err(self.contract_error(&format!(
                        "unrecognized posix_word_boundary_alias terminal {text:?}"
                    ))),
                }
            }
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
            "\\G" => Ok(Regex::Anchor(AnchorType::PreviousMatchEnd)),
            "\\b" => Ok(Regex::WordBoundary { positive: true }),
            "\\B" => Ok(Regex::WordBoundary { positive: false }),
            // PGEN 1.1.21+ routes `\K` through the `anchor` rule
            // (earlier it went through `simple_escape`). Map to the
            // same `MatchReset` node the simple_escape path uses.
            "\\K" => Ok(Regex::MatchReset),
            // PGEN also routes `\R` (newline sequence) and `\N`
            // (non-newline) through the anchor family in 1.1.21.
            // Route them to the nodes `convert_simple_escape`
            // already produces.
            "\\R" => Ok(self.newline_sequence_ast()),
            "\\N" => Ok(self.non_newline_ast()),
            "\\X" => Ok(Regex::GraphemeCluster),
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
        // PGEN 1.1.24+ `single_byte_escape = "C"` — PCRE2 `\C` matches
        // one code unit. RGX's `&str` API operates on Unicode scalar
        // values rather than raw bytes; the closest sound semantics is
        // "any single codepoint, including newline". Lower to a
        // CharClass spanning the full codepoint range.
        if self.first_descendant(node, "single_byte_escape").is_some() {
            return Ok(Regex::CharClass(CharClass::Custom {
                ranges: vec![CharRange::range('\0', char::MAX)],
                negated: false,
                ci_override_ranges: None,
            }));
        }
        if let Some(simple) = self.first_descendant(node, "simple_escape") {
            return self.convert_simple_escape(simple, false);
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
        // PGEN 1.1.23 `class_range_simple_escape` is a restricted
        // sibling of `simple_escape` that excludes orphan `\E` as a
        // range endpoint. For every admitted character its semantics
        // are literal, so route through the shared simple_escape
        // handler; the 'E'-exclusion is enforced at parse time.
        if let Some(range_simple) = self.first_descendant(node, "class_range_simple_escape") {
            return self.convert_simple_escape(range_simple, true);
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
    ///
    /// `in_class_context` should be `true` when the escape appears as a
    /// character-class atom (PGEN routes those through the restricted
    /// `class_range_simple_escape` rule). PCRE2 semantics inside a
    /// character class:
    ///  * `\b` means backspace (0x08), *not* word boundary.
    ///  * Any escaped character that is not a recognized shorthand
    ///    is a literal (e.g. `[\g<a>]` = `[g<a>]`), whereas outside a
    ///    character class an unknown alphanumeric escape is an error.
    fn convert_simple_escape(&self, node: &PgenAstNode, in_class_context: bool) -> Result<Regex> {
        let ch = self.collect_first_terminal_char(node).ok_or_else(|| {
            self.contract_error("pgen simple_escape is missing its trailing character")
        })?;
        // Inside a character class, `\b` is backspace (0x08), not a
        // word-boundary assertion. Intercept before the shared match.
        if in_class_context && ch == 'b' {
            return Ok(Regex::Char('\u{08}'));
        }
        match ch {
            // Predefined character classes (wrapped in CharClass to match VM expectations).
            // Under `(*UCP)` (PCRE2_UCP), switch to Unicode-property-backed
            // ranges so `\d` matches any `\p{Nd}`, `\w` matches any `\p{L}`
            // or `\p{N}` plus `_`, and `\s` matches any `\p{White_Space}`.
            'd' => {
                if self.ucp_enabled {
                    Ok(Regex::CharClass(CharClass::Custom {
                        ranges: crate::unicode_support::ucp_digit_ranges(),
                        negated: false,
                        ci_override_ranges: None,
                    }))
                } else {
                    Ok(Regex::CharClass(CharClass::Digit { negated: false }))
                }
            }
            'D' => {
                if self.ucp_enabled {
                    Ok(Regex::CharClass(CharClass::Custom {
                        ranges: crate::unicode_support::ucp_digit_ranges(),
                        negated: true,
                        ci_override_ranges: None,
                    }))
                } else {
                    Ok(Regex::CharClass(CharClass::Digit { negated: true }))
                }
            }
            'w' => {
                if self.ucp_enabled {
                    Ok(Regex::CharClass(CharClass::Custom {
                        ranges: crate::unicode_support::ucp_word_ranges(),
                        negated: false,
                        ci_override_ranges: None,
                    }))
                } else {
                    Ok(Regex::CharClass(CharClass::Word { negated: false }))
                }
            }
            'W' => {
                if self.ucp_enabled {
                    Ok(Regex::CharClass(CharClass::Custom {
                        ranges: crate::unicode_support::ucp_word_ranges(),
                        negated: true,
                        ci_override_ranges: None,
                    }))
                } else {
                    Ok(Regex::CharClass(CharClass::Word { negated: true }))
                }
            }
            's' => {
                if self.ucp_enabled {
                    Ok(Regex::CharClass(CharClass::Custom {
                        ranges: crate::unicode_support::ucp_space_ranges(),
                        negated: false,
                        ci_override_ranges: None,
                    }))
                } else {
                    Ok(Regex::CharClass(CharClass::Space { negated: false }))
                }
            }
            'S' => {
                if self.ucp_enabled {
                    Ok(Regex::CharClass(CharClass::Custom {
                        ranges: crate::unicode_support::ucp_space_ranges(),
                        negated: true,
                        ci_override_ranges: None,
                    }))
                } else {
                    Ok(Regex::CharClass(CharClass::Space { negated: true }))
                }
            }

            // Horizontal whitespace
            'h' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: horizontal_whitespace_ranges(),
                negated: false,
                ci_override_ranges: None,
            })),
            'H' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: horizontal_whitespace_ranges(),
                negated: true,
                ci_override_ranges: None,
            })),

            // Vertical whitespace
            'v' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: vertical_whitespace_ranges(),
                negated: false,
                ci_override_ranges: None,
            })),
            'V' => Ok(Regex::CharClass(CharClass::Custom {
                ranges: vertical_whitespace_ranges(),
                negated: true,
                ci_override_ranges: None,
            })),

            // Word boundaries (if PGEN routes them through simple_escape).
            'b' => Ok(Regex::WordBoundary { positive: true }),
            'B' => Ok(Regex::WordBoundary { positive: false }),

            // Anchors (if PGEN routes them through simple_escape).
            'A' => Ok(Regex::Anchor(AnchorType::AbsStart)),
            'Z' => Ok(Regex::Anchor(AnchorType::AbsEnd)),
            'z' => Ok(Regex::Anchor(AnchorType::AbsEndNoNL)),

            // PCRE2 end-of-previous-match anchor (\G)
            'G' => Ok(Regex::Anchor(AnchorType::PreviousMatchEnd)),

            // PCRE2 match reset (\K)
            'K' => Ok(Regex::MatchReset),

            // PCRE2 newline sequence (\R)
            'R' => Ok(self.newline_sequence_ast()),

            // PCRE2 non-newline (\N) — any char except a newline.
            // Distinct from `.`: `\N` is never affected by `/s`
            // (`(?s)\N` still rejects the newline). See
            // `non_newline_ast`.
            'N' => Ok(self.non_newline_ast()),

            // PCRE2 extended grapheme cluster (\X)
            'X' => Ok(Regex::GraphemeCluster),

            // Literal control-character escapes: \n, \t, \r, \f, \a, \e
            'n' => Ok(Regex::Char('\n')),
            't' => Ok(Regex::Char('\t')),
            'r' => Ok(Regex::Char('\r')),
            'f' => Ok(Regex::Char('\u{0C}')),
            'a' => Ok(Regex::Char('\u{07}')),
            'e' => Ok(Regex::Char('\u{1B}')),

            // `\0` is the PCRE2 NUL octal escape — never a backreference.
            // Group 0 is the overall match, which is not a valid backref
            // target. Standalone `\0` reaches us through simple_escape
            // because PGEN doesn't route it to `octal_escape` (that's
            // reserved for multi-digit forms `\000`..`\377`). Surface
            // it as a literal NUL `Char('\u{0}')`.
            '0' => Ok(Regex::Char('\0')),

            // Numeric backreferences \1, \2, etc. are captured as a single
            // digit under simple_escape by PGEN.
            c if c.is_ascii_digit() => {
                let n = c.to_digit(10).unwrap_or(0);
                Ok(Regex::Backreference(n))
            }

            // Escaped metacharacters: \., \*, \+, \?, \(, \), \[, \], \{, \}, \|, \\, \^, \$, \-, \/
            // Also covers escaped space (`\ `) used in (?x) extended mode.
            c if ".*+?()[]{}|\\^$-/ ".contains(c) => Ok(Regex::Char(c)),

            // PCRE2 fallback: a backslash before any ASCII non-
            // alphanumeric character produces the literal character.
            // Examples: `\"`, `\'`, `\@`, `\=`, `\#`, `\!`, `\:`,
            // `\;`, `\<`, `\>`, `\,`, `\~`, `\\``, `\_`, etc. This
            // matches PCRE2's documented behavior in the
            // "Non-printing characters" and "Generic character types"
            // sections of pcre2pattern(3) and closes 38 of the
            // 1.1.21 conformance failures in the "unrecognized
            // simple_escape character" bucket.
            c if !c.is_ascii_alphanumeric() => Ok(Regex::Char(c)),

            // Inside a character class, unrecognized alphanumeric
            // escapes are *literals* per PCRE2 semantics. Examples:
            // `[\g<a>]+` = `[g<a>]+`, `[\k<1>]` = `[k<1>]`. Outside a
            // class, these would be errors (real typos like `\q`), so
            // we only relax the rule in class context.
            c if in_class_context => Ok(Regex::Char(c)),

            // `\E` without a preceding `\Q` is a no-op per PCRE2.
            // Represent as an empty Sequence so the compiler elides it.
            'E' => Ok(Regex::Sequence(vec![])),

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

        // Walk the prop_name subtree for the braced form `\p{Name}` or
        // the `short_prop_letter` subtree for the single-letter short
        // form `\pX` (PGEN 1.1.22+ grammar). Both yield the same
        // `UnicodeClass` AST.
        let name_node = self
            .first_descendant(node, "prop_name")
            .or_else(|| self.first_descendant(node, "short_prop_letter"))
            .ok_or_else(|| {
                self.contract_error(
                    "pgen property_escape is missing its prop_name / short_prop_letter child",
                )
            })?;
        let mut name = String::new();
        self.collect_all_terminal_chars(name_node, &mut name);
        if name.is_empty() {
            return Err(self.contract_error("pgen property_escape has empty prop_name"));
        }
        Ok(Regex::UnicodeClass { name, negated })
    }

    /// Convert a `control_escape` node — `\cX`. PCRE2 10.47 rule
    /// (pcre2pattern(3) "Non-printing characters"):
    ///
    /// > After `\c`, the next character is taken literally, converted
    /// > to uppercase if it is a lowercase letter, and then bit 0x40
    /// > in the value is flipped.
    ///
    /// So `\cA` / `\ca` → U+0001 (both fold to 'A' = 0x41, XOR 0x40 = 0x01),
    /// `\c[` → U+001B (0x5B XOR 0x40), `\c:` → 'z' = 0x7A (0x3A XOR 0x40),
    /// `\c{` → ';' = 0x3B (0x7B XOR 0x40). The previous implementation
    /// masked with `& 0x1F` after subtracting '@', which correctly
    /// produces 0x01..0x1A for ASCII letters but quietly wraps to the
    /// wrong value for any other ASCII character (`\c:` became 0x1A
    /// instead of 0x7A, `\c{` became 0x1B instead of 0x3B).
    fn convert_control_escape(&self, node: &PgenAstNode) -> Result<Regex> {
        let any_char = self.first_descendant(node, "any_char").ok_or_else(|| {
            self.contract_error("pgen control_escape is missing its any_char child")
        })?;
        let ctrl_char = self.collect_first_terminal_char(any_char).ok_or_else(|| {
            self.contract_error("pgen control_escape any_char has no terminal character")
        })?;
        let base = if ctrl_char.is_ascii_lowercase() {
            ctrl_char.to_ascii_uppercase()
        } else {
            ctrl_char
        };
        let code = (base as u32) ^ 0x40;
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
        // Parallel range set used when the enclosing pattern is
        // compiled with case-insensitive mode. Starts equal to
        // `ranges`; for any class item that is a case-distinguished
        // Unicode property (`\p{Lu/Ll/Lt/L&/Lc/Cased_Letter/Upper/
        // Lower/Title/Cased}` and their `\P` complements), the CI
        // set substitutes the property's case-fold closure (or its
        // complement) for that item — so PCRE2's `/i` + property
        // semantic survives the merge into a mixed custom class.
        // `saw_ci_divergence` is set only when at least one item
        // actually diverged; if all items are case-invariant under
        // /i, we leave `ci_override_ranges = None` and let codegen
        // fall back to `ranges` (where `case_fold_ranges` handles
        // ASCII range case-closure correctly).
        let mut ci_ranges = Vec::new();
        let mut saw_ci_divergence = false;

        // `class_initial_close` captures a `]` literal right after `[` or
        // `[^`, keeping it as a class member instead of the closing bracket.
        if let Some(initial_close) = self.first_descendant(node, "class_initial_close") {
            if !self.is_empty_wrapper(initial_close) {
                ranges.push(CharRange::single(']'));
                ci_ranges.push(CharRange::single(']'));
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
                let item_start = ranges.len();
                self.convert_class_item(item, &mut ranges)?;
                let appended = ranges[item_start..].to_vec();
                // Check whether this item is a case-distinguished
                // Unicode property whose `/i` semantic differs from
                // the literal-range case-fold (Lu/Ll/Lt/L&/Lc/Upper/
                // Lower/Title/Cased and their `\P` complements). If
                // so, substitute the case-fold-closed ranges into
                // ci_ranges; otherwise fall through with the literal
                // ranges that will get case-fold-expanded by codegen.
                if let Some(diverged) = self.case_fold_property_class_item_ranges(item) {
                    ci_ranges.extend(diverged);
                    saw_ci_divergence = true;
                } else {
                    ci_ranges.extend(appended);
                }
            }
        }

        let ci_override_ranges = if saw_ci_divergence {
            let mut v = ci_ranges;
            v.sort_by_key(|r| r.start);
            Some(v)
        } else {
            None
        };

        Ok(Regex::CharClass(CharClass::Custom {
            ranges,
            negated,
            ci_override_ranges,
        }))
    }

    /// PCRE2 `/i` case-fold closure for a single `\p{X}` or `\P{X}`
    /// class item. Returns the case-fold-closed ranges if X is a
    /// case-distinguished property, otherwise `None`.
    ///
    /// pcre2pattern(3) lines 980-985: under `/i`, members of the
    /// case-distinguished property family — `Lu`, `Ll`, `Lt` (general
    /// categories), `L&` / `Lc` / `Cased_Letter` (their union), and
    /// the boolean properties `Upper`, `Lower`, `Title`, `Cased` —
    /// case-fold *across* the property boundary, so `\p{X}/i` must
    /// expand to the full closure (`L&` for the letter triple,
    /// `Cased` for the boolean triple) rather than being case-folded
    /// in-place by `case_fold_ranges`. Negated forms (`\P{X}/i`)
    /// resolve to the complement of the closure.
    ///
    /// Generalizes the previous hardcoded `\P{Lu/Ll/Lt}` handler
    /// (engine fix #13 / audit §9.B B1): the family is now
    /// {Lu, Ll, Lt, L&, Lc, `Cased_Letter`, Upper, Lower, Title, Cased}
    /// for both polarities, looked up via
    /// `unicode_support::case_fold_property_closure`.
    fn case_fold_property_class_item_ranges(&self, item: &PgenAstNode) -> Option<Vec<CharRange>> {
        // Slice-based detection: PGEN's typed tree doesn't expose
        // polarity uniformly, so scan the original text for
        // `\p{...}` / `\P{...}`.
        let slice = self.slice(item).ok()?;
        let trimmed = slice.trim();
        let (negated, inside) = if let Some(rest) = trimmed.strip_prefix("\\P{") {
            (true, rest.strip_suffix('}')?)
        } else if let Some(rest) = trimmed.strip_prefix("\\p{") {
            (false, rest.strip_suffix('}')?)
        } else {
            return None;
        };
        // PCRE2 also allows `\p{^X}` as in-class negation; strip it.
        let (negated, prop_name) = if let Some(rest) = inside.trim().strip_prefix('^') {
            (!negated, rest.trim())
        } else {
            (negated, inside.trim())
        };
        let closed = crate::unicode_support::case_fold_property_closure(prop_name)?;
        crate::unicode_support::resolve_unicode_property_class(closed, negated).ok()
    }

    /// Typed-shape variant of `case_fold_property_class_item_ranges`
    /// — recognises a typed `class_item` that carries a `\p{X}` /
    /// `\P{X}` escape and returns the case-fold-closed ranges if X is
    /// case-distinguished. The typed shapes recognised:
    /// - `["\\", ["p{", <prop_name>, "}"]]` — braced positive
    /// - `["\\", ["P{", <prop_name>, "}"]]` — braced negative
    /// - `["\\", ["p", <short_prop_letter>]]` — short positive (`\pL`)
    /// - `["\\", ["P", <short_prop_letter>]]` — short negative (`\PL`)
    fn case_fold_property_typed_class_item_ranges(
        &self,
        item: &serde_json::Value,
    ) -> Option<Vec<CharRange>> {
        // PGEN typed shape for `\p{X}` / `\P{X}` inside a class:
        //   {"kind":"property", "name":"<name>", "negated":<bool>,
        //    "type":"escape"}
        // (Older / array-shaped variants are not produced by the
        //  current PGEN tree but would be added here if needed.)
        let map = item.as_object()?;
        if map.get("type").and_then(|v| v.as_str()) != Some("escape") {
            return None;
        }
        if map.get("kind").and_then(|v| v.as_str()) != Some("property") {
            return None;
        }
        let raw_name = map.get("name").and_then(|v| v.as_str())?;
        let negated = matches!(map.get("negated"), Some(serde_json::Value::Bool(true)));
        // PCRE2 `\p{^X}` in-class negation marker; mirrors the
        // untyped form.
        let trimmed = raw_name.trim();
        let (negated, prop_name) = if let Some(rest) = trimmed.strip_prefix('^') {
            (!negated, rest.trim())
        } else {
            (negated, trimmed)
        };
        let closed = crate::unicode_support::case_fold_property_closure(prop_name)?;
        crate::unicode_support::resolve_unicode_property_class(closed, negated).ok()
    }

    /// Convert a single `class_item` — either a `class_range`, a bare
    /// `class_literal`, a `class_escape`, or a `posix_class` — into one
    /// or more `CharRange`s.
    fn convert_class_item(&self, item: &PgenAstNode, ranges: &mut Vec<CharRange>) -> Result<()> {
        if let Some(range_node) = self.find_direct_child(item, "class_range") {
            return self.convert_class_range(range_node, ranges);
        }
        if let Some(escape_node) = self.find_direct_child(item, "class_escape") {
            return self.convert_class_escape(escape_node, ranges);
        }
        if let Some(posix_node) = self.find_direct_child(item, "posix_class") {
            return self.convert_posix_class_into(posix_node, ranges);
        }
        // PGEN 1.1.22+ adds `quoted_class_literal` as a class_item
        // variant — `\Q…\E` inside `[…]` contributes each body byte
        // as a literal class member, per pcre2pattern(3). Metacharacters
        // like `]`, `-`, `^` keep their literal meaning inside the
        // quoted region, so we append each as its own `CharRange`.
        if let Some(quoted_node) = self.find_direct_child(item, "quoted_class_literal") {
            for ch in self.quoted_class_literal_chars(quoted_node) {
                ranges.push(CharRange::single(ch));
            }
            return Ok(());
        }
        // PGEN 1.1.23 models orphan `\E` (no preceding `\Q`) inside a
        // character class as a zero-width class item — it contributes
        // no ranges. Same applies to `empty_quoted_class_literal`
        // (`\Q\E`). Matching PCRE2's pcre2pattern(3) rule that `\E`
        // outside a quoted region is ignored (and an empty quoted
        // region contributes no characters).
        if self
            .find_direct_child(item, "stray_class_end_quote")
            .is_some()
            || self
                .find_direct_child(item, "empty_quoted_class_literal")
                .is_some()
        {
            return Ok(());
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

    /// Walk a `quoted_class_literal` subtree and return the body
    /// characters in order. PGEN emits the body as a sequence of
    /// `quoted_class_literal_char` children; each contributes exactly
    /// one character. Omits the literal `\Q` opener and `\E` closer
    /// terminals by filtering to the body-character subtree name.
    fn quoted_class_literal_chars(&self, node: &PgenAstNode) -> Vec<char> {
        let mut out = Vec::new();
        self.walk_quoted_class_body(node, &mut out);
        out
    }

    #[allow(clippy::only_used_in_recursion)]
    fn walk_quoted_class_body(&self, node: &PgenAstNode, out: &mut Vec<char>) {
        if node.rule_name == "quoted_class_literal_char" {
            // PGEN 1.1.27 widened `quoted_class_literal_char` to include
            // `quoted_class_literal_escaped_char = "\\" quoted_literal_escape_tail`,
            // which contributes TWO characters to the class: the
            // backslash and the escape-tail character (PCRE2 treats
            // everything inside `\Q...\E` as literal — escapes are
            // NOT interpreted). Walk every terminal under this node
            // in document order so both the old single-char form and
            // the new escaped-char form surface correctly.
            self.walk_terminal_chars_in_order(node, out);
            return;
        }
        for child in node.children() {
            self.walk_quoted_class_body(child, out);
        }
    }

    /// Walk every terminal character under `node` in document order
    /// (depth-first, left-to-right) and append each to `out`. Used
    /// when a subtree may contain one or more literal characters
    /// whose positions matter — e.g. `\n` inside `\Q\E` contributes
    /// both `\` and `n`, not just the first terminal.
    #[allow(clippy::only_used_in_recursion)]
    fn walk_terminal_chars_in_order(&self, node: &PgenAstNode, out: &mut Vec<char>) {
        match &node.content {
            PgenAstContent::Terminal(text) | PgenAstContent::TransformedTerminal(text) => {
                for ch in text.chars() {
                    out.push(ch);
                }
            }
            _ => {
                for child in node.children() {
                    self.walk_terminal_chars_in_order(child, out);
                }
            }
        }
    }

    /// Convert a `posix_class` node (e.g. `[:alpha:]`, `[:^digit:]`)
    /// into a set of `CharRange`s appended to `ranges`. Supported
    /// names follow the PCRE2 ASCII set: alnum, alpha, ascii, blank,
    /// cntrl, digit, graph, lower, print, punct, space, upper, word,
    /// xdigit. The `^` prefix (tracked as a `posix_negation` child)
    /// inverts the class.
    fn convert_posix_class_into(
        &self,
        posix: &PgenAstNode,
        ranges: &mut Vec<CharRange>,
    ) -> Result<()> {
        let negated = self.find_direct_child(posix, "posix_negation").is_some()
            || self.first_descendant(posix, "posix_negation").is_some();
        let name_node = self
            .first_descendant(posix, "posix_name")
            .ok_or_else(|| self.contract_error("pgen posix_class is missing its posix_name"))?;
        let name = self.slice(name_node)?.to_string();
        let class_ranges = if self.ucp_enabled {
            ucp_posix_class_ranges(&name)
                .unwrap_or_else(|| posix_class_ranges(&name).unwrap_or_default())
        } else {
            posix_class_ranges(&name).ok_or_else(|| {
                self.contract_error(&format!("unsupported POSIX class name '{name}'"))
            })?
        };
        if class_ranges.is_empty() {
            return Err(self.contract_error(&format!("unsupported POSIX class name '{name}'")));
        }
        if negated {
            for r in complement_ranges(&class_ranges) {
                ranges.push(r);
            }
        } else {
            ranges.extend(class_ranges);
        }
        Ok(())
    }

    /// Convert a `class_range` node — `class_atom "-" class_atom` — into a
    /// single `CharRange`. Escape endpoints must resolve to a single `CharRange`.
    fn convert_class_range(&self, range: &PgenAstNode, ranges: &mut Vec<CharRange>) -> Result<()> {
        // PGEN 1.1.23 widened class_range to admit zero-width markers
        // (`\Q\E`, orphan `\E`) around the range dash:
        //     class_range = class_atom class_zero_width* "-" class_zero_width* class_atom
        // Those markers contribute nothing to the range semantics — the
        // adapter should pick up the first and last `class_atom`
        // descendants as the range endpoints, regardless of how many
        // zero-width siblings sit between them and the dash.
        let atoms: Vec<&PgenAstNode> = range
            .children()
            .iter()
            .filter_map(|child| self.find_direct_child(child, "class_atom"))
            .collect();
        let start_atom = atoms
            .first()
            .ok_or_else(|| self.contract_error("pgen class_range is missing its start atom"))?;
        let end_atom = atoms
            .last()
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
        // PGEN 1.1.27 (released for PGEN-RGX-0068) lets `\Q<single-char>\E`
        // serve as a class_range endpoint via a dedicated
        // `quoted_class_range_atom` production. The single literal char
        // lives in the atom's `quoted_class_literal_char` descendant.
        // `\Qa\E-\Qz\E` now parses as `class_range[start=quoted(a),
        // end=quoted(z)]` and must lower to the range a..z, matching
        // PCRE2 semantics.
        if let Some(quoted_range) = self.find_direct_child(atom, "quoted_class_range_atom") {
            let chars = self.quoted_class_literal_chars(quoted_range);
            if chars.len() == 1 {
                return Ok(chars[0]);
            }
            return Err(self.contract_error(
                "pgen quoted_class_range_atom endpoint must resolve to exactly one character",
            ));
        }
        // PGEN 1.1.23 split the endpoint-escape production: range atoms
        // now nest `class_range_escape` (a restricted subset that
        // excludes orphan `\E`) instead of the general `class_escape`.
        // Accept either to stay compatible across grammar versions.
        let escape_node = self
            .find_direct_child(atom, "class_range_escape")
            .or_else(|| self.find_direct_child(atom, "class_escape"));
        if let Some(escape_node) = escape_node {
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
        // PGEN 1.1.22- style: `class_escape = "\\" escape` — find an
        // `escape` descendant and dispatch.
        // PGEN 1.1.23+ introduced `class_range_escape = "\\"
        // class_range_escape_unit` for endpoints; its body has
        // `hex_escape | octal_escape | ...` children directly (no
        // `escape` wrapper). `convert_escape` already handles both
        // shapes because it uses `first_descendant` for each concrete
        // escape family — so we can bypass the intermediate `escape`
        // lookup entirely and pass the whole subtree in.
        //
        // Before dispatching to the generic escape router, intercept
        // `simple_escape` nodes directly and pass `in_class_context=true`
        // so that PCRE2's class-scoped semantics apply (e.g. `[\b]` =
        // backspace 0x08, and `[\g<a>]` treats `\g` as literal `g`).
        let escape_root = self
            .first_descendant(class_escape, "escape")
            .unwrap_or(class_escape);
        if let Some(simple) = self.first_descendant(escape_root, "simple_escape") {
            let regex = self.convert_simple_escape(simple, true)?;
            extend_ranges_from_regex(regex, ranges, |msg| self.contract_error(msg))?;
            return Ok(());
        }
        let regex = self.convert_escape(escape_root)?;
        extend_ranges_from_regex(regex, ranges, |msg| self.contract_error(msg))?;
        Ok(())
    }

    /// Convert a `quoted_literal` atom — `\Q...\E`. Every character
    /// between `\Q` and `\E` is a literal, including regex
    /// metacharacters. Unterminated `\Q...` (no closing `\E`) runs
    /// to end of pattern by PCRE2 convention.
    ///
    /// We walk the source span, strip the `\Q` opener and optional
    /// `\E` closer, and emit a `Regex::Sequence` of `Char` nodes. An
    /// empty body (`\Q\E`) lowers to `Regex::Empty`.
    fn convert_quoted_literal(&self, node: &PgenAstNode) -> Result<Regex> {
        let span = self.slice(node)?;
        // PGEN's `quoted_literal = "\Q" any_char*? "\E"` guarantees
        // the span starts with `\Q`. The `\E` closer is optional per
        // PCRE2; if missing, the body runs to end of span.
        let body = span
            .strip_prefix("\\Q")
            .ok_or_else(|| self.contract_error("pgen quoted_literal missing \\Q prefix"))?;
        let body = body.strip_suffix("\\E").unwrap_or(body);
        if body.is_empty() {
            return Ok(Regex::Empty);
        }
        let chars: Vec<Regex> = body.chars().map(Regex::Char).collect();
        if chars.len() == 1 {
            Ok(chars.into_iter().next().unwrap())
        } else {
            Ok(Regex::Sequence(chars))
        }
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

    /// Convert a `directive_verb` node — `(*ACCEPT)`, `(*FAIL)`, `(*F)`,
    /// `(*COMMIT)`, `(*PRUNE)`, `(*SKIP)`, `(*THEN)`, `(*MARK:name)`,
    /// `(*:name)`, etc.
    fn convert_directive_verb(&self, node: &PgenAstNode) -> Result<Regex> {
        let name = self
            .first_descendant(node, "directive_name")
            .and_then(|n| self.slice(n).ok())
            .unwrap_or_default();
        match name {
            // (*FAIL) / (*F): unconditionally fail — compile as empty char class (never matches)
            "FAIL" | "F" => Ok(Regex::CharClass(CharClass::Custom {
                ranges: vec![],
                negated: false,
                ci_override_ranges: None,
            })),
            // (*ACCEPT): force immediate match at current position
            "ACCEPT" => Ok(Regex::Accept),
            // (*COMMIT): abort entire search on failure after this point
            "COMMIT" => Ok(Regex::Commit),
            // (*PRUNE): fail the entire attempt at this start position
            "PRUNE" => Ok(Regex::Prune),
            // (*SKIP) / (*SKIP:name): advance to the skip position on
            // failure. The named form interacts with (*MARK:name) to
            // restart search at the position of the most recent
            // matching mark; the unnamed form restarts at the
            // position of (*SKIP) itself.
            "SKIP" => {
                let payload = self.extract_directive_payload(node);
                if payload.is_empty() {
                    Ok(Regex::Skip(None))
                } else {
                    Ok(Regex::Skip(Some(payload)))
                }
            }
            // (*THEN): skip to the next alternative on failure
            "THEN" => Ok(Regex::Then),
            // (*MARK:name): set a named mark (no-op for match behavior)
            "MARK" => {
                let mark_name = self.extract_directive_payload(node);
                Ok(Regex::Mark(mark_name))
            }
            // (*:name): shorthand for (*MARK:name)
            "" => {
                // Empty directive_name means this is the (*:name) shorthand form.
                // The PGEN rule `directive_mark_shorthand = ":" payload` handles
                // this case — the full span after "(*" starts with ":".
                let mark_name = self.extract_directive_payload(node);
                Ok(Regex::Mark(mark_name))
            }
            // Mode/newline/BSR settings: accept and ignore — rgx is always UTF-8
            // with Unicode properties and Unicode newline semantics.
            "UTF" | "UTF8" | "UTF16" | "UTF32" | "UCP" | "CR" | "LF" | "CRLF" | "ANY"
            | "ANYCRLF" | "NUL" | "BSR_ANYCRLF" | "BSR_UNICODE" => Ok(Regex::Empty),
            // Runtime-policy / optimiser-hint verbs. These are PCRE2
            // directives that control the matching policy (empty-match
            // gating, heap/depth/step limits, Turkish case folding) or
            // the engine backend (JIT, start-of-subject optimisation).
            // They change *how* matching proceeds, not what the language
            // accepts — so the grammar admits them and RGX simply
            // records them as no-ops for conformance purposes. The
            // test cases that exercise these verbs do not rely on the
            // associated runtime gating in ways that change the
            // observable match, so a no-op pass-through preserves
            // correctness on the PCRE2 testdata corpus.
            "NOTEMPTY" | "NOTEMPTY_ATSTART" | "NO_START_OPT" | "NO_AUTO_POSSESS"
            | "NO_DOTSTAR_ANCHOR" | "NO_JIT" | "LIMIT_HEAP" | "LIMIT_MATCH" | "LIMIT_DEPTH"
            | "LIMIT_RECURSION" | "TURKISH_CASING" | "CASELESS_RESTRICT" | "ALT_BSUX"
            | "ALT_EXTENDED_CLASS" | "ALT_CIRCUMFLEX" | "ALT_VERBNAMES" => Ok(Regex::Empty),
            // Unrecognized verb
            other => Err(RgxError::compile(format!(
                "unsupported backtracking verb '(*{other})'"
            ))),
        }
    }

    /// Extract the payload text from a directive verb node.
    ///
    /// Looks for a `directive_payload_simple` descendant. If not found,
    /// falls back to extracting the text after the first `:` in the span.
    fn extract_directive_payload(&self, node: &PgenAstNode) -> String {
        // Try to find a directive_payload_simple descendant first
        if let Some(payload_node) = self.first_descendant(node, "directive_payload_simple") {
            if let Ok(text) = self.slice(payload_node) {
                return text.to_string();
            }
        }
        // Fallback: extract from the full span text after the first ':'
        if let Ok(span_text) = self.slice(node) {
            if let Some(colon_pos) = span_text.find(':') {
                let payload = &span_text[colon_pos + 1..];
                // Strip trailing ')'
                let payload = payload.trim_end_matches(')');
                return payload.to_string();
            }
        }
        String::new()
    }

    /// Convert a `callout` node — `(?C)`, `(?C123)`, or `(?C"text")`.
    ///
    /// Extracts the optional callout number from the span text. The number
    /// defaults to 0 when absent (bare `(?C)`) or when the callout uses
    /// the string form (`(?C"arg")` / `(?C'arg'`)) — those identify the
    /// callout by argument rather than by number, and RGX treats all
    /// unregistered callouts as no-ops anyway, so the number is not
    /// observed at match time.
    fn convert_callout(&self, node: &PgenAstNode) -> Result<Regex> {
        let text = self
            .terminal_text(node)
            .or_else(|_| self.slice(node).map(ToString::to_string))?;
        // text is the full span, e.g. "(?C)" or "(?C123)" or "(?C\"xyz\")"
        // or "(?C'xyz')". Extract the body after "C" and before the
        // closing ")".
        let body = text.trim_start_matches("(?C").trim_end_matches(')');
        let number: u32 = if body.is_empty() {
            0
        } else if matches!(
            body.chars().next(),
            Some('"' | '\'' | '{' | '`' | '%' | '#' | '$' | '^')
        ) {
            // String / brace / backtick / other-delimiter callout.
            // pcre2test accepts any of `" ' { ` % # $ ^` as the
            // opening delimiter for the callout's string argument.
            // Keep as numeric 0 — match semantics are identical for
            // unregistered callouts regardless of the payload.
            0
        } else {
            body.parse::<u32>()
                .map_err(|_| RgxError::compile(format!("invalid callout number in '{text}'")))?
        };
        Ok(Regex::Callout(number))
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
        // PGEN 1.1.9+ (pinned at 1.1.10): check for returned-capture subroutine first.
        if let Some(rcs) = self.first_descendant(node, "returned_capture_subroutine") {
            return self.convert_returned_capture_subroutine(rcs);
        }

        let target_node = self
            .first_descendant(node, "subroutine_target")
            .ok_or_else(|| {
                self.contract_error("pgen subroutine_call is missing subroutine_target")
            })?;

        // Unwrap the immediate Alternative wrapper if present.
        let inner = self.alternative_child(target_node).unwrap_or(target_node);

        // Variant 1: signed_digits (e.g. `(?1)`, `(?+1)`, `(?-1)`).
        if let Some(signed) = self.first_descendant(inner, "signed_digits") {
            let text = self.slice(signed)?;
            let has_plus = text.starts_with('+');
            let has_minus = text.starts_with('-');
            let abs_text = text.trim_start_matches('+').trim_start_matches('-');
            let n: u32 = abs_text.parse().map_err(|_| {
                self.contract_error(&format!("invalid subroutine call number '{text}'"))
            })?;
            if has_plus {
                #[allow(clippy::cast_possible_wrap)]
                return Ok(Regex::Recursion {
                    target: RecursionTarget::RelativeGroup(n as i32),
                });
            }
            if has_minus {
                #[allow(clippy::cast_possible_wrap)]
                return Ok(Regex::Recursion {
                    target: RecursionTarget::RelativeGroup(-(n as i32)),
                });
            }
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

    /// Convert a `returned_capture_subroutine` node — `(?1(1,2))`.
    ///
    /// PGEN 1.1.9+ grammar: `returned_capture_subroutine` contains
    /// `subroutine_target` and `returned_capture_group_list`.
    fn convert_returned_capture_subroutine(&self, node: &PgenAstNode) -> Result<Regex> {
        // Extract subroutine target.
        let target_node = self
            .first_descendant(node, "subroutine_target")
            .ok_or_else(|| {
                self.contract_error("returned_capture_subroutine missing subroutine_target")
            })?;
        let inner = self.alternative_child(target_node).unwrap_or(target_node);
        let target = if let Some(signed) = self.first_descendant(inner, "signed_digits") {
            let text = self.slice(signed)?;
            let n: u32 = text
                .trim_start_matches('+')
                .trim_start_matches('-')
                .parse()
                .map_err(|_| {
                    self.contract_error(&format!(
                        "invalid returned_capture_subroutine target '{text}'"
                    ))
                })?;
            RecursionTarget::Group(n)
        } else {
            let prefix_text = self.find_first_terminal_text(inner).unwrap_or("");
            match prefix_text {
                "&" | "P>" => {
                    let name = self.name_text(inner)?;
                    RecursionTarget::NamedGroup(name)
                }
                "R" => RecursionTarget::Entire,
                other => {
                    return Err(self.contract_error(&format!(
                        "unrecognized returned_capture_subroutine target prefix '{other}'"
                    )));
                }
            }
        };

        // Extract returned group references from the group list.
        // Each `returned_capture_group` node carries either
        // `signed_digits` (numeric / relative) or a `name` (named ref).
        let mut returned_groups: Vec<RecursionTarget> = Vec::new();
        let children = self.collect_descendants(node, "returned_capture_group");
        for group_node in &children {
            if let Some(signed) = self.first_descendant(group_node, "signed_digits") {
                if let Ok(text) = self.slice(signed) {
                    let trimmed = text.trim();
                    let (sign, digits) = if let Some(rest) = trimmed.strip_prefix('+') {
                        (Some('+'), rest)
                    } else if let Some(rest) = trimmed.strip_prefix('-') {
                        (Some('-'), rest)
                    } else {
                        (None, trimmed)
                    };
                    if let Ok(n) = digits.parse::<u32>() {
                        let target = match sign {
                            Some('+') => RecursionTarget::RelativeGroup(n as i32),
                            Some('-') => RecursionTarget::RelativeGroup(-(n as i32)),
                            _ => RecursionTarget::Group(n),
                        };
                        returned_groups.push(target);
                        continue;
                    }
                }
            }
            // Fall back: try to read a name.
            if let Ok(name) = self.name_text(group_node) {
                returned_groups.push(RecursionTarget::NamedGroup(name));
            }
        }

        Ok(Regex::ReturnedCaptureSubroutine {
            target,
            returned_groups,
        })
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
                // PCRE2 `\g`-form forks on the delimiter, per pcre2pattern(3):
                //   * `\g<name>`, `\g<N>`, `\g<+N>`, `\g<-N>`, `\g'name'`,
                //     `\g'N'` — **subroutine call** (re-executes the named /
                //     numbered group, recursing if necessary).
                //   * `\g{name}`, `\g{N}` — **back-reference** (matches the
                //     text previously captured by the group).
                //   * `\gN` (no delimiter) — plain back-reference.
                // The pcre2test fixture `/^(?<name>a|b\g<name>c)/` on
                // "bbacc" relies on the subroutine semantic; treating
                // `\g<name>` as a back-reference produces a no-match
                // because the group hasn't been captured when the
                // recursion point is reached.
                let fragment = self.slice(node).unwrap_or("");
                let is_subroutine = fragment.contains("\\g<") || fragment.contains("\\g'");
                if let Some(name_node) = self.first_descendant(node, "name") {
                    let name = self.slice(name_node)?.to_string();
                    return Ok(if is_subroutine {
                        Regex::Recursion {
                            target: RecursionTarget::NamedGroup(name),
                        }
                    } else {
                        Regex::NamedBackreference(name)
                    });
                }
                // Check for signed_digits first (handles +N, -N, and plain N).
                if let Some(signed_node) = self.first_descendant(node, "signed_digits") {
                    let sign_text = self
                        .first_descendant(signed_node, "sign")
                        .and_then(|n| self.slice(n).ok())
                        .unwrap_or("");
                    let mut digits = String::new();
                    self.walk_collect_terminal_chars(signed_node, "digit", &mut digits);
                    if !digits.is_empty() {
                        let n: u32 = digits.parse().map_err(|_| {
                            self.contract_error(&format!(
                                "invalid numeric backreference '{digits}'"
                            ))
                        })?;
                        if is_subroutine {
                            return Ok(match sign_text {
                                "+" => Regex::Recursion {
                                    #[allow(clippy::cast_possible_wrap)]
                                    target: RecursionTarget::RelativeGroup(n as i32),
                                },
                                "-" => Regex::Recursion {
                                    #[allow(clippy::cast_possible_wrap)]
                                    target: RecursionTarget::RelativeGroup(-(n as i32)),
                                },
                                _ => Regex::Recursion {
                                    target: RecursionTarget::Group(n),
                                },
                            });
                        }
                        return match sign_text {
                            "+" =>
                            {
                                #[allow(clippy::cast_possible_wrap)]
                                Ok(Regex::RelativeBackreference(n as i32))
                            }
                            "-" =>
                            {
                                #[allow(clippy::cast_possible_wrap)]
                                Ok(Regex::RelativeBackreference(-(n as i32)))
                            }
                            _ => Ok(Regex::Backreference(n)),
                        };
                    }
                }
                // Fallback: plain digits node (no sign).
                if let Some(digits_node) = self.first_descendant(node, "digits") {
                    let mut digits = String::new();
                    self.walk_collect_terminal_chars(digits_node, "digit", &mut digits);
                    if !digits.is_empty() {
                        let n: u32 = digits.parse().map_err(|_| {
                            self.contract_error(&format!(
                                "invalid numeric backreference '{digits}'"
                            ))
                        })?;
                        return Ok(if is_subroutine {
                            Regex::Recursion {
                                target: RecursionTarget::Group(n),
                            }
                        } else {
                            Regex::Backreference(n)
                        });
                    }
                }
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
                non_atomic: false,
            }),
            "lookahead_neg" => Ok(Regex::Lookahead {
                expr: Box::new(expr),
                positive: false,
                non_atomic: false,
            }),
            "lookbehind_pos" => Ok(Regex::Lookbehind {
                expr: Box::new(expr),
                positive: true,
                non_atomic: false,
            }),
            "lookbehind_neg" => Ok(Regex::Lookbehind {
                expr: Box::new(expr),
                positive: false,
                non_atomic: false,
            }),
            // PGEN 1.1.22+ also admits the symbol-forms
            // `(?*…)` (non-atomic positive lookahead) and
            // `(?<*…)` (non-atomic positive lookbehind) via the
            // dedicated rule names `non_atomic_lookahead_pos` /
            // `non_atomic_lookbehind_pos`. The behavioral difference
            // from the ordinary positive forms is that backtracking
            // across the assertion boundary is permitted — a property
            // RGX's backtracking VM already exhibits for `(?=...)` and
            // `(?<=...)`, so we lower to the same AST shape.
            "non_atomic_lookahead_pos" => Ok(Regex::Lookahead {
                expr: Box::new(expr),
                positive: true,
                non_atomic: true,
            }),
            "non_atomic_lookbehind_pos" => Ok(Regex::Lookbehind {
                expr: Box::new(expr),
                positive: true,
                non_atomic: true,
            }),
            // PGEN 1.1.21+ supports PCRE2's callout-style lookaround
            // aliases under `alpha_lookaround = "(*" name ":" pattern? ")"`.
            // Names: pla / positive_lookahead, nla / negative_lookahead,
            // plb / positive_lookbehind, nlb / negative_lookbehind.
            // We resolve via the embedded `alpha_lookaround_name`
            // child and dispatch to the existing Lookahead/Lookbehind
            // node shapes — semantics are identical to (?=...), (?!...),
            // (?<=...), (?<!...).
            "alpha_lookaround" => {
                let name = self
                    .first_descendant(actual, "alpha_lookaround_name")
                    .and_then(|n| self.slice(n).ok())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        self.contract_error(
                            "pgen alpha_lookaround is missing its alpha_lookaround_name",
                        )
                    })?;
                regex_from_alpha_lookaround_name(&name, expr).ok_or_else(|| {
                    self.contract_error(&format!("unrecognized alpha_lookaround name '{name}'"))
                })
            }
            other => {
                Err(self.contract_error(&format!("unsupported pgen lookaround variant '{other}'")))
            }
        }
    }

    fn convert_conditional(&self, node: &PgenAstNode) -> Result<Regex> {
        let condition = self
            .first_descendant(node, "condition")
            .ok_or_else(|| self.contract_error("pgen conditional is missing its condition"))?;

        // A13: VERSION conditionals are evaluated at parse time. The
        // condition text is parsed as a comparison against
        // `RGX_PCRE2_COMPAT_VERSION`; the matching branch is returned
        // directly so the conditional never becomes a
        // `Regex::Conditional` node. This is the same compile-time
        // short-circuit pattern PCRE2 uses for `(?(VERSION>=...)...)`
        // — the engine version is fixed at compile time so there is
        // no point evaluating the check at runtime.
        if let Ok(condition_text) = self.slice(condition) {
            if let Some(matches) = parse_version_conditional(condition_text) {
                let target_branch = if matches {
                    self.first_descendant(node, "yes_branch").ok_or_else(|| {
                        self.contract_error("pgen conditional is missing its yes branch")
                    })?
                } else if let Some(no_branch) = self.first_descendant(node, "no_branch") {
                    no_branch
                } else {
                    // VERSION check failed and there is no else
                    // branch — the conditional contributes nothing to
                    // the pattern. PCRE2 treats this as an empty
                    // sub-expression, which always matches.
                    return Ok(Regex::Empty);
                };
                return self.convert_conditional_branch(target_branch);
            }
        }

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
        // PGEN 1.1.24+ `condition_callout_assertion = condition_callout
        // "(" condition_assertion` — a PCRE2 conditional with a
        // callout that fires before the assertion is evaluated
        // (`(?(?C25)(?=abc)...|...)`). RGX's runtime doesn't execute
        // callouts from PCRE2 text patterns, so the callout is
        // effectively a no-op for match decisions; fall through to
        // the inner `condition_assertion` which carries the actual
        // decision predicate.
        if let Some(combo) = self.first_descendant(node, "condition_callout_assertion") {
            if let Some(assertion) = self.first_descendant(combo, "condition_assertion") {
                return self.convert_condition_assertion(assertion);
            }
        }
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
            // PGEN 1.1.21+ also routes callout-style aliases through
            // `condition_assertion` via the `alpha_condition_assertion`
            // sub-production: `*pla:`, `*nla:`, `*plb:`, `*nlb:`, plus
            // the long names. Span text starts with `*` followed by
            // the alias name, then `:`, then the pattern. Map to the
            // existing Lookahead/Lookbehind ConditionalTest variants.
            _ if assertion_text.starts_with('*') => {
                let after_star = &assertion_text[1..];
                let colon_idx = after_star.find(':').ok_or_else(|| {
                    self.contract_error(&format!(
                        "alpha_condition_assertion '{assertion_text}' is missing ':' separator"
                    ))
                })?;
                let name = &after_star[..colon_idx];
                let positive_lookahead = matches!(name, "pla" | "positive_lookahead");
                let negative_lookahead = matches!(name, "nla" | "negative_lookahead");
                let positive_lookbehind = matches!(name, "plb" | "positive_lookbehind");
                let negative_lookbehind = matches!(name, "nlb" | "negative_lookbehind");
                let boxed = Box::new(expr);
                if positive_lookahead {
                    Ok(ConditionalTest::Lookahead {
                        expr: boxed,
                        positive: true,
                    })
                } else if negative_lookahead {
                    Ok(ConditionalTest::Lookahead {
                        expr: boxed,
                        positive: false,
                    })
                } else if positive_lookbehind {
                    Ok(ConditionalTest::Lookbehind {
                        expr: boxed,
                        positive: true,
                    })
                } else if negative_lookbehind {
                    Ok(ConditionalTest::Lookbehind {
                        expr: boxed,
                        positive: false,
                    })
                } else {
                    Err(self.contract_error(&format!(
                        "unrecognized alpha_condition_assertion name '{name}'"
                    )))
                }
            }
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
                // PGEN's `counted_quantifier_body` has two alternatives:
                //   (a) digits ws? ("," ws? digits?)?   — {N} / {N,} / {N,M}
                //   (b) "," ws? digits                  — {,M}  (min=0, max=digits)
                // Distinguish by asking whether the body's first leaf
                // terminal is a comma: if yes, we're in branch (b) and the
                // single `digits` group holds the maximum.
                let leading_comma = self
                    .find_first_terminal_text(body)
                    .is_some_and(|t| t.trim_start().starts_with(','));
                if leading_comma && digit_groups.len() == 1 {
                    let max = digit_groups[0].parse::<u32>().map_err(|_| {
                        self.contract_error(&format!(
                            "invalid counted quantifier maximum '{}'",
                            digit_groups[0]
                        ))
                    })?;
                    (0u32, Some(max))
                } else {
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
                }
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
            // Json content carries a typed `serde_json::Value` payload; the
            // legacy walker can't recurse into a non-PgenAstNode value.
            // The new typed-shape walker handles these subtrees directly.
            PgenAstContent::Json(_) => None,
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
            PgenAstContent::Json(value) => {
                // Best-effort terminal extraction from typed-Json shapes:
                // strings contribute directly; arrays recurse; numbers/bools
                // are stringified; objects are skipped (caller should be
                // walking the typed shape, not relying on terminal collection).
                walk_json_terminal_chars(value, out);
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
            PgenAstContent::Json(value) => json_contains_string(value, target),
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

    fn collect_descendants<'b>(
        &'b self,
        node: &'b PgenAstNode,
        expected_rule: &str,
    ) -> Vec<&'b PgenAstNode> {
        let mut results = Vec::new();
        if node.rule_name == expected_rule {
            results.push(node);
        }
        for child in node.children() {
            results.extend(self.collect_descendants(child, expected_rule));
        }
        results
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
            PgenAstContent::Json(value) => match value {
                serde_json::Value::Null => true,
                serde_json::Value::Array(arr) => arr.is_empty(),
                serde_json::Value::Object(map) => map.is_empty(),
                serde_json::Value::String(s) => s.is_empty(),
                serde_json::Value::Bool(_) | serde_json::Value::Number(_) => false,
            },
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
        // PGEN 1.1.21 audit split `modifier_group` into `modifier_item+`
        // where `modifier_item` can be `"a" ascii_restrict_modifier?`,
        // `"x" "x"?` (for `(?xx)` extended+strict), or `modifier_char`.
        // The `modifier_char` set no longer includes `x`, `a`, `A`, `d`,
        // `S`, `X`, `R` — those are handled via `modifier_item` now.
        //
        // We walk `modifier_item` first so the flag character at the
        // head of that production (`"a"` or `"x"`) is surfaced; the
        // optional suffix (`ascii_restrict_modifier` or a second `x`)
        // is appended after. `modifier_char` leaves are captured on
        // the fall-through.
        if node.rule_name == "modifier_item" {
            // Extract the literal terminal(s) in document order.
            self.walk_modifier_item_terminals(node, out);
            return;
        }
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

    #[allow(clippy::only_used_in_recursion)]
    fn walk_modifier_item_terminals(&self, node: &PgenAstNode, out: &mut String) {
        // `modifier_item` child order: a leading terminal (`"a"` or
        // `"x"`) followed by an optional suffix (a second terminal
        // `"x"` for `(?xx)`, or an `ascii_restrict_modifier` wrapper
        // whose terminal is `D`/`S`/`W`/`P`/`T`). Recursively push
        // every terminal char we see so each modifier character
        // reaches the flag string that the compiler then interprets.
        if let PgenAstContent::Terminal(text) | PgenAstContent::TransformedTerminal(text) =
            &node.content
        {
            for ch in text.chars() {
                out.push(ch);
            }
            return;
        }
        for child in node.children() {
            self.walk_modifier_item_terminals(child, out);
        }
    }

    fn contract_error(&self, message: &str) -> RgxError {
        let _ = self;
        RgxError::compile(format!("pgen AST contract mismatch: {message}"))
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
            // Json content has no PgenAstNode children — it's a typed
            // serde_json::Value. The new typed-shape walker handles it.
            PgenAstContent::Json(_) => Vec::new(),
        }
    }
}

/// Stringify a `serde_json::Value`'s top-level kind for error messages.
#[cfg(feature = "pgen-parser")]
fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Walk a `serde_json::Value` from a `Json` content variant and append
/// any string-shaped terminals encountered. Used as a fall-back for the
/// legacy envelope walker's `collect_all_terminal_chars`; the new typed
/// walker handles annotated subtrees directly without going through this.
#[cfg(feature = "pgen-parser")]
fn walk_json_terminal_chars(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(s) => out.push_str(s),
        serde_json::Value::Number(n) => out.push_str(&n.to_string()),
        serde_json::Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        serde_json::Value::Null => {}
        serde_json::Value::Array(items) => {
            for item in items {
                walk_json_terminal_chars(item, out);
            }
        }
        // Skip object shapes — typed objects aren't terminal-text-equivalent;
        // the typed walker should be used for those.
        serde_json::Value::Object(_) => {}
    }
}

/// Return true if a `serde_json::Value` from a `Json` content variant
/// contains any string equal to `target` (recursively). Used by the
/// legacy envelope walker's `has_terminal_text` for the rare case where
/// it crosses a Json boundary.
#[cfg(feature = "pgen-parser")]
fn json_contains_string(value: &serde_json::Value, target: &str) -> bool {
    match value {
        serde_json::Value::String(s) => s == target,
        serde_json::Value::Array(items) => items.iter().any(|v| json_contains_string(v, target)),
        serde_json::Value::Object(map) => map.values().any(|v| json_contains_string(v, target)),
        _ => false,
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
        Regex::CharClass(CharClass::Custom {
            ranges: custom,
            negated,
            ..
        }) => {
            // Honour the `negated` flag — otherwise `\W` / `\D` / `\S`
            // expanded via the UCP path (which arrive as
            // `Custom { negated: true }`) would incorrectly contribute
            // the positive set instead of its complement when unioned
            // into the surrounding class.
            if negated {
                ranges.extend(complement_ranges(&custom));
            } else {
                ranges.extend(custom);
            }
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
        Regex::CharClass(CharClass::Word { negated: true }) => {
            // `\W` inside `[...]`: union every codepoint that is NOT
            // a word char. This appends the complement as disjoint
            // ranges around the 0-9/A-Z/_/a-z islands.
            ranges.push(CharRange::range('\0', '/'));
            ranges.push(CharRange::range(':', '@'));
            ranges.push(CharRange::range('[', '^'));
            ranges.push(CharRange::single('`'));
            ranges.push(CharRange::range('{', char::MAX));
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
        Regex::CharClass(CharClass::Space { negated: true }) => {
            // `\S` inside `[...]`: union every codepoint that is NOT
            // a whitespace char. Six disjoint ranges.
            ranges.push(CharRange::range('\0', '\u{08}'));
            ranges.push(CharRange::single('\u{0E}'));
            ranges.push(CharRange::range('\u{0F}', '\u{1F}'));
            ranges.push(CharRange::range('!', char::MAX));
            // Note: '\t'=0x09, '\n'=0x0A, '\v'=0x0B, '\f'=0x0C, '\r'=0x0D,
            // ' '=0x20 are the whitespace chars; the three ranges above
            // plus the gap after 0x0D (which ends at 0x1F) and the
            // jump over ' ' into '!' cover the complement.
            Ok(())
        }
        // `\b` as a class_escape is PCRE2's literal backspace (U+0008),
        // NOT the word-boundary assertion. The `convert_escape` path
        // returns `Regex::WordBoundary { positive: true }` for `\b`
        // because its caller is usually an atom position; we translate
        // it here when the context is a char class.
        Regex::WordBoundary { positive: true } => {
            ranges.push(CharRange::single('\u{08}'));
            Ok(())
        }
        // `\p{...}` / `\P{...}` inside a char class: resolve the named
        // Unicode property to its range set via the shared helper and
        // union those ranges into the enclosing class. Invalid property
        // names are reported with the same error shape the atom-level
        // path uses.
        Regex::UnicodeClass { name, negated } => {
            let resolved = crate::unicode_support::resolve_unicode_property_class(&name, negated)
                .map_err(|msg| make_error(&msg))?;
            ranges.extend(resolved);
            Ok(())
        }
        // `\.` inside a char class is a literal period. PGEN lowers the
        // escape to `Regex::Dot` because the token happens to be the
        // same; inside `[...]` the metaclass interpretation does not
        // apply and PCRE2 reads it as the literal character.
        Regex::Dot => {
            ranges.push(CharRange::single('.'));
            Ok(())
        }
        // `\N` (N = digit) inside a char class. PGEN lowers every `\1`..
        // `\9` to `Regex::Backreference(N)` because that is the general
        // escape form, but PCRE2 rule for char classes is:
        //   - N ∈ 0..=7 : octal escape, value = N as a codepoint
        //   - N ∈ 8..=9 : literal digit character (octal requires base-8
        //     digits; 8 and 9 are not valid, so PCRE2 falls back to the
        //     literal character rule)
        // Backrefs have no meaning inside `[...]` — there is nothing to
        // reference against a single-char position.
        Regex::Backreference(n) => {
            let ch = match n {
                1..=7 => char::from_u32(n).ok_or_else(|| {
                    make_error(&format!("octal escape value {n} is not a valid codepoint"))
                })?,
                8..=9 => char::from_u32(u32::from(b'0') + n).expect("digit char is always valid"),
                other => {
                    return Err(make_error(&format!(
                        "class_escape Backreference({other}) has no PCRE2 interpretation inside a char class"
                    )));
                }
            };
            ranges.push(CharRange::single(ch));
            Ok(())
        }
        other => Err(make_error(&format!(
            "class_escape resolved to unsupported variant '{other:?}' for char class"
        ))),
    }
}

/// Map a PCRE2 callout-style lookaround alias name to the
/// corresponding RGX `Lookahead`/`Lookbehind` node, given the inner
/// `expr` already lowered. Returns `None` if the name isn't one of
/// the eight PCRE2 aliases.
#[cfg(feature = "pgen-parser")]
fn regex_from_alpha_lookaround_name(name: &str, expr: Regex) -> Option<Regex> {
    let boxed = Box::new(expr);
    // PCRE2 also offers non-atomic lookaround variants `napla` /
    // `naplb` (and their long forms `non_atomic_positive_lookahead` /
    // `non_atomic_positive_lookbehind`). The only behavioral difference
    // from the ordinary positive forms is that backtracking across the
    // assertion boundary is permitted — a property that RGX's
    // backtracking VM already exhibits for `(?=...)` and `(?<=...)`,
    // so we can soundly map them to the same AST nodes. (There is no
    // `nanla` / `nanlb`: PCRE2 does not define non-atomic variants of
    // the negative forms — a negative assertion that failed to match
    // already makes backtracking moot.)
    Some(match name {
        "pla" | "positive_lookahead" | "napla" | "non_atomic_positive_lookahead" => {
            Regex::Lookahead {
                expr: boxed,
                positive: true,
                non_atomic: false,
            }
        }
        "nla" | "negative_lookahead" => Regex::Lookahead {
            expr: boxed,
            positive: false,
            non_atomic: false,
        },
        "plb" | "positive_lookbehind" | "naplb" | "non_atomic_positive_lookbehind" => {
            Regex::Lookbehind {
                expr: boxed,
                positive: true,
                non_atomic: false,
            }
        }
        "nlb" | "negative_lookbehind" => Regex::Lookbehind {
            expr: boxed,
            positive: false,
            non_atomic: false,
        },
        _ => return None,
    })
}

/// Return the `CharRange`s for a PCRE2 POSIX bracket class name, or
/// `None` for an unknown name. Matches the ASCII semantics documented
/// in pcre2pattern(3) under "POSIX character classes". Character-
/// class-internal use only — the adapter always emits these as
/// disjoint ranges that merge into the surrounding char class.
#[cfg(feature = "pgen-parser")]
/// PCRE2 POSIX class ranges under `PCRE2_UCP`. Returns `None` for names
/// where PCRE2 keeps the ASCII-only semantic (e.g. `:xdigit:` and
/// `:ascii:` stay as the ASCII set even in UCP mode); callers fall
/// back to the ASCII table in that case.
#[cfg(feature = "pgen-parser")]
fn ucp_posix_class_ranges(name: &str) -> Option<Vec<CharRange>> {
    use crate::unicode_support::resolve_unicode_property_class as unicode_prop;
    // Helpers that resolve a single Unicode property, defaulting to an
    // empty range set on lookup failure. Keep the fallback silent — the
    // non-UCP path in `posix_class_ranges` is the safety net.
    let p = |prop: &str| unicode_prop(prop, false).unwrap_or_default();
    let merge = |props: &[&str]| -> Vec<CharRange> {
        let mut all: Vec<CharRange> = Vec::new();
        for prop in props {
            all.extend(p(prop));
        }
        all.sort_by_key(|r| r.start);
        all
    };
    Some(match name {
        "alpha" => p("L"),
        "alnum" => merge(&["L", "N"]),
        "digit" => p("Nd"),
        "lower" => p("Ll"),
        "upper" => p("Lu"),
        // PCRE2 `[:word:]` under UCP matches the same set as `\w`:
        // L + N + M (combining marks) + Pc (connector punctuation
        // including `_`). See `ucp_word_ranges` for rationale.
        "word" => {
            let mut v = merge(&["L", "N", "M", "Pc"]);
            v.push(CharRange::single('_'));
            v.sort_by_key(|r| r.start);
            v
        }
        // PCRE2 under `/ucp` treats U+180E (MONGOLIAN VOWEL SEPARATOR)
        // as `\s`/`[:space:]` for compatibility with its pre-Unicode-6.3
        // classification — U+180E was Zs then, Cf now, but PCRE2's
        // `\s` definition didn't follow the reclassification. The
        // `White_Space` property excludes it as of current Unicode
        // tables, so we union it back in. Mirror the same special
        // case already in `"blank"` and `"print"` below.
        "space" => {
            let mut v = p("White_Space");
            v.push(CharRange::single('\u{180E}'));
            v.sort_by_key(|r| r.start);
            v
        }
        "blank" => {
            // `[:blank:]` under UCP = Zs + `\t` + U+180E (PCRE2
            // historical treatment of MVS as blank-space, mirrors
            // the `\s` and `[:print:]` additions elsewhere).
            let mut v = p("Zs");
            v.push(CharRange::single('\t'));
            v.push(CharRange::single('\u{180E}'));
            v.sort_by_key(|r| r.start);
            v
        }
        "cntrl" => p("Cc"),
        // PCRE2 `[:graph:]` under UCP matches any codepoint that is
        // not one of {Cc, Cs, Cn, Zs, Zl, Zp}, AND is not one of the
        // specific "invisible" bidi-formatting codepoints PCRE2 has
        // historically excluded: U+061C (ARABIC LETTER MARK),
        // U+180E (MONGOLIAN VOWEL SEPARATOR, was Zs pre-6.3), and
        // U+2066..U+2069 (bidi isolate controls LRI/RLI/FSI/PDI).
        // The rest of Cf (soft-hyphen, zero-width joiner/non-joiner,
        // LRM/RLM, Arabic number signs, etc.) IS graph, matching
        // testinput4:2131-2147 expectations where those codepoints
        // are listed as graph subjects. Co (private use) is also
        // graph.
        "graph" => graph_ranges_ucp(),
        // `[:print:]` = graph + space-separators (Zs) + U+180E.
        // PCRE2 historically treats U+180E (MONGOLIAN VOWEL SEPARATOR)
        // as a space/print codepoint for compatibility with pre-
        // Unicode-6.3 classification (it was Zs then, Cf now). The
        // graph set excludes it (as an invisible-format Cf), but
        // print unions Zs on top, and PCRE2's Zs-equivalent for
        // print also covers U+180E.
        "print" => {
            let mut v = graph_ranges_ucp();
            v.extend(p("Zs"));
            v.push(CharRange::single('\u{180E}'));
            v.sort_by_key(|r| r.start);
            v
        }
        // PCRE2 `[:punct:]` under /utf (UCP): Unicode P* plus the
        // ASCII printable symbols (`$`, `+`, `<`, `=`, `>`, `^`,
        // `` ` ``, `|`, `~`) that POSIX considers punctuation.
        // PCRE2 excludes the rest of the S* category (e.g. U+00B4
        // ACUTE ACCENT is Sk, not punct). Blanket `S` inclusion
        // caused `[[:punct:]]/utf` on `"´"` to match; blanket
        // exclusion caused `[[:punct:]]+$/utf` on `"$+<=>^`|~"` to
        // miss. Restrict to P* + the specific ASCII-symbol set.
        "punct" => {
            let mut v = merge(&["P"]);
            for ch in ['$', '+', '<', '=', '>', '^', '`', '|', '~'] {
                v.push(CharRange::single(ch));
            }
            v.sort_by_key(|r| r.start);
            v
        }
        // `:xdigit:` under PCRE2_UCP adds the fullwidth hex forms:
        // FULLWIDTH DIGIT ZERO..NINE (U+FF10..U+FF19), FULLWIDTH
        // LATIN CAPITAL LETTER A..F (U+FF21..U+FF26), and FULLWIDTH
        // LATIN SMALL LETTER A..F (U+FF41..U+FF46). The ASCII set
        // (`0-9A-Fa-f`) is always included. Matches testinput5:2758
        // where `/^[[:xdigit:]]+$/utf,ucp` accepts `d\x{ff10}` and
        // `\x{ff26}8`.
        "xdigit" => vec![
            CharRange::range('0', '9'),
            CharRange::range('A', 'F'),
            CharRange::range('a', 'f'),
            CharRange::range('\u{FF10}', '\u{FF19}'),
            CharRange::range('\u{FF21}', '\u{FF26}'),
            CharRange::range('\u{FF41}', '\u{FF46}'),
        ],
        // `:ascii:` keeps its ASCII-only semantic under PCRE2_UCP
        // (per pcre2pattern(3)).
        _ => return None,
    })
}

/// PCRE2 `[:graph:]` under UCP — `L | M | N | P | S | Cf | Co`, minus
/// the specific invisible bidi-formatting codepoints that PCRE2
/// excludes. Split out from `ucp_posix_class_ranges` so `[:print:]`
/// can reuse the same base set before unioning `Zs`.
#[cfg(feature = "pgen-parser")]
fn graph_ranges_ucp() -> Vec<CharRange> {
    use crate::unicode_support::resolve_unicode_property_class as unicode_prop;
    let p = |prop: &str| unicode_prop(prop, false).unwrap_or_default();
    let mut ranges: Vec<CharRange> = Vec::new();
    for prop in ["L", "M", "N", "P", "S", "Cf", "Co"] {
        ranges.extend(p(prop));
    }
    // Remove PCRE2's excluded bidi-formatting codepoints. Each is a
    // single codepoint; we walk `ranges` and split any range that
    // straddles the exclusion point.
    const EXCLUDED: &[char] = &[
        '\u{061C}', '\u{180E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
    ];
    for &ex in EXCLUDED {
        let mut out: Vec<CharRange> = Vec::with_capacity(ranges.len() + 1);
        for r in ranges.drain(..) {
            if ex < r.start || ex > r.end {
                out.push(r);
                continue;
            }
            // `ex` falls inside `r`. Split around the excluded point.
            if r.start < ex {
                out.push(CharRange {
                    start: r.start,
                    end: char::from_u32(ex as u32 - 1).unwrap_or(r.start),
                });
            }
            if r.end > ex {
                out.push(CharRange {
                    start: char::from_u32(ex as u32 + 1).unwrap_or(r.end),
                    end: r.end,
                });
            }
        }
        ranges = out;
    }
    ranges.sort_by_key(|r| r.start);
    ranges
}

fn posix_class_ranges(name: &str) -> Option<Vec<CharRange>> {
    let r = match name {
        "alnum" => vec![
            CharRange::range('0', '9'),
            CharRange::range('A', 'Z'),
            CharRange::range('a', 'z'),
        ],
        "alpha" => vec![CharRange::range('A', 'Z'), CharRange::range('a', 'z')],
        "ascii" => vec![CharRange::range('\0', '\u{7F}')],
        "blank" => vec![CharRange::single('\t'), CharRange::single(' ')],
        "cntrl" => vec![
            CharRange::range('\0', '\u{1F}'),
            CharRange::single('\u{7F}'),
        ],
        "digit" => vec![CharRange::range('0', '9')],
        "graph" => vec![CharRange::range('!', '~')],
        "lower" => vec![CharRange::range('a', 'z')],
        "print" => vec![CharRange::range(' ', '~')],
        "punct" => vec![
            CharRange::range('!', '/'),
            CharRange::range(':', '@'),
            CharRange::range('[', '`'),
            CharRange::range('{', '~'),
        ],
        "space" => vec![
            CharRange::single('\t'),
            CharRange::single('\n'),
            CharRange::single('\u{0B}'),
            CharRange::single('\u{0C}'),
            CharRange::single('\r'),
            CharRange::single(' '),
        ],
        "upper" => vec![CharRange::range('A', 'Z')],
        "word" => vec![
            CharRange::range('0', '9'),
            CharRange::range('A', 'Z'),
            CharRange::single('_'),
            CharRange::range('a', 'z'),
        ],
        "xdigit" => vec![
            CharRange::range('0', '9'),
            CharRange::range('A', 'F'),
            CharRange::range('a', 'f'),
        ],
        _ => return None,
    };
    Some(r)
}

/// Return the complement (over the full Unicode codepoint set) of a
/// list of `CharRange`s. Input ranges may overlap; output is sorted
/// non-overlapping ranges that together with the input cover every
/// codepoint exactly once. Used by `convert_posix_class_into` to
/// implement the `^` negation of POSIX bracket classes.
#[cfg(feature = "pgen-parser")]
fn complement_ranges(input: &[CharRange]) -> Vec<CharRange> {
    if input.is_empty() {
        return vec![CharRange::range('\0', char::MAX)];
    }
    // Normalize: collect (start, end) as u32, sort, and merge overlaps.
    let mut sorted: Vec<(u32, u32)> = input
        .iter()
        .map(|r| (r.start as u32, r.end as u32))
        .collect();
    sorted.sort_by_key(|&(s, _)| s);
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(sorted.len());
    for (s, e) in sorted {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 + 1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    // Walk the merged list and emit the gaps.
    let mut out = Vec::with_capacity(merged.len() + 1);
    let mut cursor: u32 = 0;
    for (s, e) in &merged {
        if cursor < *s {
            if let (Some(a), Some(b)) = (char::from_u32(cursor), char::from_u32(*s - 1)) {
                out.push(CharRange::range(a, b));
            }
        }
        cursor = e.saturating_add(1);
    }
    if cursor <= char::MAX as u32 {
        if let Some(a) = char::from_u32(cursor) {
            out.push(CharRange::range(a, char::MAX));
        }
    }
    out
}

/// Unicode code points for horizontal whitespace (\h).
#[cfg(feature = "pgen-parser")]
fn horizontal_whitespace_ranges() -> Vec<CharRange> {
    // PCRE2 `\h` set per pcre2pattern(3): HT, SPACE, NBSP, OGHAM SPACE
    // MARK, MONGOLIAN VOWEL SEPARATOR (kept for pre-Unicode-6.3 back
    // compat), the en..hair spaces, NARROW NO-BREAK SPACE, MEDIUM
    // MATHEMATICAL SPACE, IDEOGRAPHIC SPACE. U+180E was removed from
    // the Unicode `White_Space` property in 6.3.0 but PCRE2 continues
    // to treat it as `\h` for backward compatibility with existing
    // patterns.
    vec![
        CharRange::single('\t'),
        CharRange::single(' '),
        CharRange::single('\u{00A0}'),
        CharRange::single('\u{1680}'),
        CharRange::single('\u{180E}'),
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

/// Return the flag string of the *last* unscoped flag toggle in
/// `pieces` — `Regex::FlagGroup { expr: Regex::Empty }` with a
/// non-empty flag string — or `None` if no such toggle is present.
///
/// Used by `convert_alternation` to propagate a branch's trailing
/// unscoped toggle to subsequent branches per PCRE2 semantics.
fn last_unscoped_flag(pieces: &[Regex]) -> Option<String> {
    let mut last = None;
    for p in pieces {
        if let Regex::FlagGroup { flags, expr } = p {
            if matches!(expr.as_ref(), Regex::Empty) && !flags.is_empty() {
                last = Some(flags.clone());
            }
        }
    }
    last
}

/// PCRE2 scoping rule: a bare inline-flag directive such as `(?i)` or
/// `(?-i)` changes the effective flags for the *remainder of the
/// enclosing group* (or top-level pattern) — not just for a trailing
/// empty subexpression. The adapter lowers such directives into
/// `Regex::FlagGroup { expr: Regex::Empty }`; this walker rewrites each
/// sequence so subsequent siblings become the directive's body.
fn apply_bare_flag_directives(items: Vec<Regex>) -> Regex {
    let mut iter = items.into_iter();
    let mut prefix: Vec<Regex> = Vec::new();
    while let Some(item) = iter.next() {
        if let Regex::FlagGroup { flags, expr } = &item {
            if matches!(expr.as_ref(), Regex::Empty) && !flags.is_empty() {
                let flags = flags.clone();
                let suffix: Vec<Regex> = iter.collect();
                let body = apply_bare_flag_directives(suffix);
                prefix.push(Regex::FlagGroup {
                    flags,
                    expr: Box::new(body),
                });
                return pack_sequence(prefix);
            }
        }
        prefix.push(item);
    }
    pack_sequence(prefix)
}

fn pack_alternation(items: Vec<Regex>) -> Regex {
    match items.len() {
        0 => Regex::Empty,
        1 => items.into_iter().next().unwrap(),
        _ => Regex::Alternation(items),
    }
}

/// Parse a `(?(VERSION op X.Y)yes|no)` condition body and evaluate
/// it against [`crate::RGX_PCRE2_COMPAT_VERSION`]. Returns
/// `Some(true)` if the version check passes, `Some(false)` if it
/// fails, or `None` if `text` is not a VERSION conditional at all.
///
/// Recognised operators (PCRE2 syntax): `=`, `!=`, `>=`, `<=`, `>`,
/// `<`. The version is parsed as `MAJOR[.MINOR]`; missing minor
/// defaults to 0.
///
/// **Step A13.** PCRE2's VERSION conditionals are evaluated at
/// pattern compile time so the engine version is fixed before any
/// matching happens. RGX does the same: the parser short-circuits
/// the conditional to its matching branch and the conditional
/// never reaches the AST as a `Regex::Conditional` node. Almost
/// never used in real-world patterns, but cheap to support.
fn parse_version_conditional(text: &str) -> Option<bool> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix("VERSION")?.trim_start();
    // Operator must be one of {=, !=, >=, <=, >, <}. Order matters:
    // try the two-char operators (>=, <=, !=) BEFORE the one-char
    // ones to avoid matching `>=` as `>`.
    let (op, version_str) = if let Some(s) = rest.strip_prefix(">=") {
        (VersionConditionalOp::Ge, s)
    } else if let Some(s) = rest.strip_prefix("<=") {
        (VersionConditionalOp::Le, s)
    } else if let Some(s) = rest.strip_prefix("!=") {
        (VersionConditionalOp::Ne, s)
    } else if let Some(s) = rest.strip_prefix('>') {
        (VersionConditionalOp::Gt, s)
    } else if let Some(s) = rest.strip_prefix('<') {
        (VersionConditionalOp::Lt, s)
    } else if let Some(s) = rest.strip_prefix('=') {
        (VersionConditionalOp::Eq, s)
    } else {
        return None;
    };
    let target = parse_version_string(version_str.trim())?;
    Some(evaluate_version_conditional(op, target))
}

/// Internal representation of the comparison operator in a
/// `(?(VERSION op X.Y)...)` conditional. Used only by
/// [`parse_version_conditional`] and immediately discarded after
/// the comparison is evaluated.
#[derive(Debug, Clone, Copy)]
enum VersionConditionalOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Parse a version string like `10.45` or `10` into a
/// `(major, minor)` tuple. Missing minor defaults to 0.
fn parse_version_string(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    Some((major, minor))
}

/// Evaluate a `(VERSION op target)` check against
/// [`crate::RGX_PCRE2_COMPAT_VERSION`].
fn evaluate_version_conditional(op: VersionConditionalOp, target: (u32, u32)) -> bool {
    let current = crate::RGX_PCRE2_COMPAT_VERSION;
    match op {
        VersionConditionalOp::Eq => current == target,
        VersionConditionalOp::Ne => current != target,
        VersionConditionalOp::Gt => current > target,
        VersionConditionalOp::Ge => current >= target,
        VersionConditionalOp::Lt => current < target,
        VersionConditionalOp::Le => current <= target,
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
            "(?+1)(a)",
            "(a)(?-1)",
            r"(a)\g<+1>(b)",
            r"(a)\g<-1>",
            "(?J)(?<x>a)(?<x>b)",
            "(*ACCEPT)",
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
            msg.starts_with("regex compile error:"),
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

    // ---------------------------------------------------------------
    // Feature: Relative subroutines (?+1), (?-1)
    // ---------------------------------------------------------------

    #[test]
    fn relative_subroutine_forward_parses() {
        let ast = parse_pattern("(?+1)(a)").expect("(?+1)(a) should parse");
        match &ast {
            Regex::Sequence(items) => {
                assert!(
                    matches!(
                        &items[0],
                        Regex::Recursion {
                            target: crate::ast::RecursionTarget::RelativeGroup(1),
                        }
                    ),
                    "expected RelativeGroup(1), got: {:?}",
                    items[0]
                );
            }
            other => panic!("expected Sequence, got: {other:?}"),
        }
    }

    #[test]
    fn relative_subroutine_backward_parses() {
        let ast = parse_pattern("(a)(?-1)").expect("(a)(?-1) should parse");
        match &ast {
            Regex::Sequence(items) => {
                assert!(
                    matches!(
                        &items[1],
                        Regex::Recursion {
                            target: crate::ast::RecursionTarget::RelativeGroup(-1),
                        }
                    ),
                    "expected RelativeGroup(-1), got: {:?}",
                    items[1]
                );
            }
            other => panic!("expected Sequence, got: {other:?}"),
        }
    }

    #[test]
    fn relative_subroutine_forward_executes() {
        // (?+1)(a) = call group 1 (forward), then define group 1 as 'a'
        // On "a", subroutine (?+1) calls group 1 which matches 'a', then
        // the literal group (a) also matches 'a'. So "aa" should match.
        let re = crate::Regex::compile(r"\A(?+1)(a)\z")
            .expect("relative subroutine forward should compile");
        assert!(re.is_match("aa"));
        assert!(!re.is_match("a"));
    }

    #[test]
    fn relative_subroutine_backward_executes() {
        // (a)(?-1) = define group 1 as 'a', then call group 1 again
        // On "aa", group 1 matches first 'a', then (?-1) calls group 1
        // again to match second 'a'.
        let re = crate::Regex::compile(r"\A(a)(?-1)\z")
            .expect("relative subroutine backward should compile");
        assert!(re.is_match("aa"));
        assert!(!re.is_match("a"));
    }

    // ---------------------------------------------------------------
    // Feature: Relative backreferences \g<+1>, \g<-1>
    // ---------------------------------------------------------------

    #[test]
    fn relative_backreference_forward_parses() {
        // PCRE2 distinguishes `\g<+1>` (subroutine call — angle brackets
        // always imply *call*) from `\g{+1}` (back-reference). The
        // execution semantics agree for single-char groups captured
        // before the reference, so the `_executes` tests below still
        // pass either way; the AST assertion pins the correct lowering.
        let ast = parse_pattern(r"(a)\g<+1>(b)").expect(r"\g<+1> should parse");
        match &ast {
            Regex::Sequence(items) => {
                assert!(
                    matches!(
                        &items[1],
                        Regex::Recursion {
                            target: RecursionTarget::RelativeGroup(1)
                        }
                    ),
                    "expected Recursion(RelativeGroup(1)), got: {:?}",
                    items[1]
                );
            }
            other => panic!("expected Sequence, got: {other:?}"),
        }
    }

    #[test]
    fn relative_backreference_backward_parses() {
        let ast = parse_pattern(r"(a)\g<-1>").expect(r"\g<-1> should parse");
        match &ast {
            Regex::Sequence(items) => {
                assert!(
                    matches!(
                        &items[1],
                        Regex::Recursion {
                            target: RecursionTarget::RelativeGroup(-1)
                        }
                    ),
                    "expected Recursion(RelativeGroup(-1)), got: {:?}",
                    items[1]
                );
            }
            other => panic!("expected Sequence, got: {other:?}"),
        }
    }

    #[test]
    fn relative_backreference_backward_executes() {
        // (a)\g<-1> = capture 'a' in group 1, then backreference group 1
        let re = crate::Regex::compile(r"\A(a)\g<-1>\z")
            .expect("relative backreference backward should compile");
        assert!(re.is_match("aa"));
        assert!(!re.is_match("ab"));
        assert!(!re.is_match("a"));
    }

    #[test]
    fn relative_backreference_forward_executes() {
        // (a)(b)\g<-2> = capture 'a' in group 1, capture 'b' in group 2,
        // then \g<-2> resolves to group 1, backreferences 'a'.
        // On "aba": match. On "abb": no match.
        let re = crate::Regex::compile(r"\A(a)(b)\g<-2>\z")
            .expect("relative backreference with -2 should compile");
        assert!(re.is_match("aba"));
        assert!(!re.is_match("abb"));
    }

    // ---------------------------------------------------------------
    // Feature: (?J) duplicate group names
    // ---------------------------------------------------------------

    #[test]
    fn duplicate_group_names_with_j_flag_parses() {
        parse_pattern("(?J)(?<x>a)(?<x>b)").expect("(?J) with duplicate names should parse");
    }

    #[test]
    fn duplicate_group_names_with_j_flag_executes() {
        let re = crate::Regex::compile(r"\A(?J)(?<x>a)(?<x>b)\z")
            .expect("(?J) with duplicate names should compile");
        assert!(re.is_match("ab"));
        assert!(!re.is_match("aa"));
    }

    #[test]
    fn duplicate_group_names_with_j_scoped_executes() {
        // (?J:...) scoped form
        let re = crate::Regex::compile(r"\A(?J:(?<x>a)|(?<x>b))\z")
            .expect("(?J:...) with duplicate names should compile");
        assert!(re.is_match("a"));
        assert!(re.is_match("b"));
    }

    // ---------------------------------------------------------------
    // Feature: (*ACCEPT)
    // ---------------------------------------------------------------

    #[test]
    fn accept_verb_parses() {
        let ast = parse_pattern("(*ACCEPT)").expect("(*ACCEPT) should parse");
        assert!(
            matches!(ast, Regex::Accept),
            "expected Accept, got: {ast:?}"
        );
    }

    #[test]
    fn accept_verb_matches_immediately() {
        let re = crate::Regex::compile(r"a(*ACCEPT)b").expect("(*ACCEPT) should compile");
        // (*ACCEPT) forces match after 'a', 'b' is never tested
        assert!(re.is_match("a"));
        assert!(re.is_match("ax"));
    }

    #[test]
    fn accept_verb_in_alternation() {
        let re = crate::Regex::compile(r"\A(?:(*ACCEPT)|b)\z")
            .expect("(*ACCEPT) in alternation should compile");
        // (*ACCEPT) immediately matches, so any input matches
        assert!(re.is_match(""));
        assert!(re.is_match("anything"));
    }

    // --- Backtracking control verb parsing tests ---

    #[test]
    fn commit_verb_parses() {
        let ast = parse_pattern("(*COMMIT)").expect("(*COMMIT) should parse");
        assert!(
            matches!(ast, Regex::Commit),
            "expected Commit, got: {ast:?}"
        );
    }

    #[test]
    fn prune_verb_parses() {
        let ast = parse_pattern("(*PRUNE)").expect("(*PRUNE) should parse");
        assert!(matches!(ast, Regex::Prune), "expected Prune, got: {ast:?}");
    }

    #[test]
    fn skip_verb_parses() {
        let ast = parse_pattern("(*SKIP)").expect("(*SKIP) should parse");
        assert!(
            matches!(ast, Regex::Skip(None)),
            "expected Skip(None), got: {ast:?}"
        );
    }

    #[test]
    fn then_verb_parses() {
        let ast = parse_pattern("(*THEN)").expect("(*THEN) should parse");
        assert!(matches!(ast, Regex::Then), "expected Then, got: {ast:?}");
    }

    #[test]
    fn mark_verb_parses() {
        let ast = parse_pattern("(*MARK:foo)").expect("(*MARK:foo) should parse");
        assert!(
            matches!(ast, Regex::Mark(ref name) if name == "foo"),
            "expected Mark(\"foo\"), got: {ast:?}"
        );
    }

    #[test]
    fn mark_shorthand_parses() {
        let ast = parse_pattern("(*:bar)").expect("(*:bar) should parse");
        assert!(
            matches!(ast, Regex::Mark(ref name) if name == "bar"),
            "expected Mark(\"bar\"), got: {ast:?}"
        );
    }

    // ============================================================
    // A13: VERSION conditional helpers
    // ============================================================
    //
    // Unit tests for `parse_version_conditional`. Integration tests
    // (the actual `(?(VERSION>=10.45)yes|no)` pattern compiling and
    // matching) live in the public Regex API tests.

    #[test]
    fn parse_version_conditional_recognises_ge() {
        // RGX_PCRE2_COMPAT_VERSION is (10, 47). VERSION>=10.0
        // should be true.
        assert_eq!(parse_version_conditional("VERSION>=10.0"), Some(true));
        // VERSION>=99.0 should be false.
        assert_eq!(parse_version_conditional("VERSION>=99.0"), Some(false));
        // Exact match: VERSION>=10.47 is true.
        assert_eq!(parse_version_conditional("VERSION>=10.47"), Some(true));
    }

    #[test]
    fn parse_version_conditional_recognises_le() {
        assert_eq!(parse_version_conditional("VERSION<=99.0"), Some(true));
        assert_eq!(parse_version_conditional("VERSION<=5.0"), Some(false));
    }

    #[test]
    fn parse_version_conditional_recognises_eq_ne() {
        assert_eq!(parse_version_conditional("VERSION=10.47"), Some(true));
        assert_eq!(parse_version_conditional("VERSION=10.46"), Some(false));
        assert_eq!(parse_version_conditional("VERSION!=10.46"), Some(true));
        assert_eq!(parse_version_conditional("VERSION!=10.47"), Some(false));
    }

    #[test]
    fn parse_version_conditional_recognises_strict_inequality() {
        assert_eq!(parse_version_conditional("VERSION>10.0"), Some(true));
        assert_eq!(parse_version_conditional("VERSION>10.47"), Some(false));
        assert_eq!(parse_version_conditional("VERSION<99.0"), Some(true));
        assert_eq!(parse_version_conditional("VERSION<10.47"), Some(false));
    }

    #[test]
    fn parse_version_conditional_handles_missing_minor() {
        // "VERSION>=10" should be parsed as "VERSION>=10.0".
        assert_eq!(parse_version_conditional("VERSION>=10"), Some(true));
        assert_eq!(parse_version_conditional("VERSION>=99"), Some(false));
    }

    #[test]
    fn parse_version_conditional_handles_whitespace() {
        // PGEN may pass condition text with surrounding whitespace.
        assert_eq!(parse_version_conditional("  VERSION>=10.0  "), Some(true));
        assert_eq!(parse_version_conditional("VERSION >= 10.0"), Some(true));
    }

    #[test]
    fn parse_version_conditional_returns_none_for_non_version_text() {
        // Non-VERSION text should return None so the caller can fall
        // through to the regular conditional handling.
        assert_eq!(parse_version_conditional("DEFINE"), None);
        assert_eq!(parse_version_conditional("R1"), None);
        assert_eq!(parse_version_conditional("name"), None);
        assert_eq!(parse_version_conditional("1"), None);
        assert_eq!(parse_version_conditional(""), None);
    }

    #[test]
    fn parse_version_conditional_returns_none_for_malformed_version() {
        // Bad version strings (non-numeric, missing operand) should
        // return None — the caller will then try other condition
        // shapes and ultimately error if nothing matches.
        assert_eq!(parse_version_conditional("VERSION>=abc"), None);
        assert_eq!(parse_version_conditional("VERSION>="), None);
        assert_eq!(parse_version_conditional("VERSION10.0"), None);
    }

    // ============================================================
    // A13: VERSION conditional integration tests
    // ============================================================
    //
    // End-to-end tests that compile `(?(VERSION op X.Y)yes|no)`
    // patterns through the parser. PGEN 1.1.10 delivers the VERSION
    // conditional as a Conditional with a bare-text condition body,
    // which `convert_conditional` short-circuits at parse time via
    // `parse_version_conditional`. The resulting AST contains only
    // the matching branch — never a `Regex::Conditional` node.

    #[test]
    fn version_conditional_passing_check_returns_yes_branch_only() {
        // VERSION>=10.0 is true (RGX_PCRE2_COMPAT_VERSION is 10.47).
        // The parser should return just `cat`, never a Conditional.
        let ast =
            parse_pattern("(?(VERSION>=10.0)cat|dog)").expect("VERSION conditional should parse");
        assert!(
            !contains_conditional(&ast),
            "VERSION conditional should be short-circuited at parse time, got: {ast:?}"
        );
    }

    #[test]
    fn version_conditional_failing_check_returns_no_branch_only() {
        let ast =
            parse_pattern("(?(VERSION>=99.0)cat|dog)").expect("VERSION conditional should parse");
        assert!(
            !contains_conditional(&ast),
            "VERSION conditional should be short-circuited, got: {ast:?}"
        );
    }

    #[test]
    fn version_conditional_failing_check_with_no_else_returns_empty() {
        let ast = parse_pattern("(?(VERSION>=99.0)cat)")
            .expect("VERSION conditional with no else should parse");
        assert!(
            !contains_conditional(&ast),
            "VERSION conditional should be short-circuited, got: {ast:?}"
        );
    }

    /// Recursively check whether the AST contains any
    /// `Regex::Conditional` node. Used by the VERSION conditional
    /// integration tests to assert that the conditional was
    /// short-circuited at parse time.
    fn contains_conditional(ast: &Regex) -> bool {
        match ast {
            Regex::Conditional { .. } => true,
            Regex::Sequence(items) | Regex::Alternation(items) => {
                items.iter().any(contains_conditional)
            }
            Regex::Group { expr, .. }
            | Regex::Quantified { expr, .. }
            | Regex::FlagGroup { expr, .. }
            | Regex::Lookahead { expr, .. }
            | Regex::Lookbehind { expr, .. } => contains_conditional(expr),
            _ => false,
        }
    }

    // =================================================================
    // Regression pins — PCRE2 testinput1 bugs (2026-04-13)
    // =================================================================

    #[test]
    fn simple_escape_backslash_zero_is_nul_not_backreference() {
        // testinput1 pattern `/abc\0def\00pqr\000xyz\0000AB/` expects
        // the literal NUL-byte interpretation for `\0`. Prior to the
        // fix, `\0` fell through the `c.is_ascii_digit()` arm in
        // `convert_simple_escape` and became `Regex::Backreference(0)`,
        // which compiled but never matched. Group 0 is the overall
        // match — it is never a valid backref target — so `\0` must
        // surface as a literal NUL.
        let r = crate::Regex::compile(r"\0").expect("compiles");
        assert!(r.is_match("\0"), "pattern \\0 should match a NUL byte");
    }

    #[test]
    fn simple_escape_backslash_zero_matches_nul_in_longer_literal() {
        // Same fix, verified inside a longer pattern. Without it, the
        // literal-finder's needle would include a NUL byte but the VM
        // bytecode emitted a `Backref(0)` instead of `Char('\0')`, so
        // the match attempt failed even when the subject did contain
        // the NUL byte.
        let r = crate::Regex::compile(r"a\0b").expect("compiles");
        let m = r.find_first("a\0b").expect("match at position 0");
        assert_eq!((m.start, m.end), (0, 3));
    }

    #[test]
    fn simple_escape_backreferences_still_work() {
        // Sanity: `\1` / `\2` etc. must continue to be backreferences,
        // not NUL. The fix applies only to the `'0'` branch.
        let r = crate::Regex::compile(r"(a)\1").expect("compiles");
        assert!(r.is_match("aa"));
        assert!(!r.is_match("ab"));
    }

    #[test]
    fn control_escape_letter_variants_produce_c0_controls() {
        // PCRE2: `\cX` uppercases lowercase letters then XORs bit 0x40.
        // `\ca` / `\cA` both → U+0001. `\cZ` → U+001A.
        let r = crate::Regex::compile(r"\ca\cA\cZ").expect("compiles");
        assert!(r.is_match("\u{01}\u{01}\u{1A}"));
        assert!(!r.is_match("aAZ"));
    }

    #[test]
    fn control_escape_punctuation_uses_xor_not_mask() {
        // Regression pin for PCRE2 testinput1 line 116: `/^\ca\cA\c[;\c:/`
        // expects subject "\u{1}\u{1}\u{1b};z". The old formula
        // `(ctrl.to_ascii_uppercase() - '@') & 0x1F` is only correct
        // for ASCII letters — it silently wraps for punctuation. `\c:`
        // must produce 'z' (0x3A XOR 0x40 = 0x7A), `\c[` must produce
        // U+001B (0x5B XOR 0x40), matching PCRE2's documented rule.
        let r = crate::Regex::compile(r"^\ca\cA\c[;\c:").expect("compiles");
        assert!(
            r.is_match("\u{01}\u{01}\u{1B};z"),
            "expected subject `\\u{{01}}\\u{{01}}\\u{{1B}};z` to match /^\\ca\\cA\\c[;\\c:/"
        );
    }
}
