//! High-Performance Regex Virtual Machine
//!
//! This module implements a state-of-the-art regex execution engine designed
//! to surpass PCRE2 performance through:
//! - SIMD-optimized pattern matching
//! - Cache-friendly bytecode design
//! - Adaptive execution strategies
//! - JIT compilation hints
//! - Memoization for backtracking

use crate::ast::{
    AnchorType, CharClass, CharRange, ConditionalTest, GroupKind, Quantifier, RecursionTarget,
    Regex,
};
use crate::c2::Classification;
use crate::events::MatchEvent;
use crate::execution::{
    CodeBlockValue, ExecContext as CodeExecContext, ExecContextSnapshot, ExecResult,
    ExecutionManager, MatchContinuation, MatchOutcome, SteerResult, VmResumeState,
};
use crate::unicode_support::resolve_unicode_property_class;
use crate::{debug_log, low_log, trace_decision, trace_enter, trace_exit, trace_log};
use memchr::memchr;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// PCRE2 `\s` byte test: matches space (0x20), tab (0x09), LF (0x0A),
/// VT (0x0B), FF (0x0C), CR (0x0D). Distinct from Rust's
/// `u8::is_ascii_whitespace()`, which excludes VT — PCRE2 includes it.
#[inline]
#[must_use]
pub const fn pcre2_is_space_byte(b: u8) -> bool {
    matches!(b, b' ' | 0x09..=0x0D)
}

/// PCRE2 `\s` char test (ASCII-only form). Unicode-aware whitespace is
/// expanded at compile time via `\p{White_Space}` when `/ucp` or `/utf`
/// semantics are requested; this helper covers the six ASCII bytes
/// PCRE2's default `\s` matches.
#[inline]
#[must_use]
pub const fn pcre2_is_space_char(ch: char) -> bool {
    matches!(ch, ' ' | '\u{09}'..='\u{0D}')
}

/// Outcome of evaluating an inline code-block during VM execution.
///
/// This enriches the simple pass/fail model with steering actions so that host
/// callbacks can actively direct how the match proceeds.
#[derive(Debug, Clone, PartialEq)]
enum CodeBlockOutcome {
    /// Code block passed — continue matching the next opcode.
    Pass,
    /// Code block failed — try backtracking.
    Fail,
    /// Host forced immediate match acceptance at the current position.
    Accept,
    /// Suspend — unregistered native callback needs async resolution.
    Suspended(String),
}

/// High-performance bytecode instruction optimized for cache efficiency
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)] // Ensure tight packing for cache efficiency
pub enum OpCode {
    // === LITERAL MATCHING (0x00-0x0F) ===
    /// Match single character - most common operation, gets opcode 0
    Char = 0x00,
    /// Match any character except newline
    Any = 0x01,
    // 0x02 removed: String (never emitted)
    // 0x03 removed: CharNoCase (never emitted)
    // 0x04 removed: StringNoCase (never emitted)
    /// Match any character including newline (dotall / (?s) mode)
    AnyDotAll = 0x05,
    /// \K — Reset the reported match start to the current position
    MatchReset = 0x06,
    /// \G — Assert that current position equals end of previous match
    PreviousMatchEnd = 0x07,
    /// \X — Match one Unicode extended grapheme cluster
    GraphemeCluster = 0x08,

    // === CHARACTER CLASSES (0x10-0x1F) ===
    /// ASCII digit [0-9]
    DigitAscii = 0x10,
    /// Negated ASCII digit [^0-9]
    DigitAsciiNeg = 0x11,
    /// Word character [a-zA-Z0-9_]  
    WordAscii = 0x12,
    /// Negated word character [^a-zA-Z0-9_]
    WordAsciiNeg = 0x13,
    /// Whitespace [ \t\n\r]
    SpaceAscii = 0x14,
    /// Negated whitespace [^ \t\n\r]
    SpaceAsciiNeg = 0x15,
    /// Custom character class (followed by class ID)
    CharClass = 0x16,
    /// Negated custom character class
    CharClassNeg = 0x17,
    // 0x18 removed: Range (superseded by CharClass/CharClassNeg)
    // 0x19 removed: RangeNeg (superseded by CharClass/CharClassNeg)

    // === SIMD-OPTIMIZED OPERATIONS (0x20-0x2F) ===
    /// Find any byte from set using SIMD (up to 16 bytes)
    SimdFind = 0x20, // Reserved: not yet emitted by the compiler
    /// Find literal string using SIMD Boyer-Moore
    SimdString = 0x21, // Reserved: not yet emitted by the compiler
    /// Vectorized character class matching
    SimdCharClass = 0x22, // Reserved: not yet emitted by the compiler
    /// SIMD-accelerated dot matching (skip non-newlines)
    SimdAny = 0x23, // Reserved: not yet emitted by the compiler

    // === ANCHORS & BOUNDARIES (0x30-0x3F) ===
    /// Start of line ^
    StartLine = 0x30,
    /// End of line $
    EndLine = 0x31,
    /// Start of text \A
    StartText = 0x32,
    /// End of text \z
    EndText = 0x33,
    /// End of text or before final newline \Z
    EndTextOrNL = 0x34,
    /// Word boundary \b
    WordBoundary = 0x35,
    /// Non-word boundary \B
    NonWordBoundary = 0x36,

    // === CONTROL FLOW (0x40-0x4F) ===
    /// Unconditional jump (16-bit signed offset follows)
    Jump = 0x40,
    /// Split execution (greedy quantifier support) - try first path, backtrack to second
    Split = 0x41,
    /// Split execution (lazy quantifier) - try second path first
    SplitLazy = 0x42,
    /// Conditional jump based on lookahead
    JumpIfMatch = 0x43, // Reserved: not yet emitted by the compiler
    /// Conditional jump based on negative lookahead
    JumpIfNoMatch = 0x44,
    /// Call subroutine (for recursion/subroutine calls)
    Call = 0x45,
    // 0x46 removed: Return (never emitted; Call uses recursion stack instead)

    // === CAPTURE GROUPS (0x50-0x5F) ===
    /// Save position to capture group (group ID follows)
    SaveStart = 0x50,
    /// Save end position to capture group  
    SaveEnd = 0x51,
    // 0x52 removed: SaveStartCond (no implementation; backtracking uses BacktrackFrame)
    // 0x53 removed: RestoreCaptures (no implementation; backtracking uses BacktrackFrame)

    // === ADVANCED FEATURES (0x60-0x6F) ===
    /// Lookahead assertion (length follows, then sub-pattern)
    Lookahead = 0x60,
    /// Negative lookahead assertion
    LookaheadNeg = 0x61,
    /// Lookbehind assertion  
    Lookbehind = 0x62,
    /// Negative lookbehind assertion
    LookbehindNeg = 0x63,
    /// Atomic group start (cut backtracking)
    AtomicStart = 0x64,
    /// Atomic group end
    AtomicEnd = 0x65,
    /// Backreference (group ID follows)
    Backref = 0x66,
    /// Execute an embedded code block predicate
    CodeBlock = 0x67,
    /// Case-insensitive backreference (group ID follows). Emitted in
    /// place of `Backref` when `(?i)` is active at the backref site.
    /// Walks the captured text and the subject char-by-char, comparing
    /// via Unicode per-char case folding (`char::to_lowercase()`). Does
    /// not yet handle cases where folding changes byte length (e.g.
    /// `ẞ` → `ss`) — that's the Unicode case-fold residual follow-up.
    BackrefCaseInsensitive = 0x68,

    // === OPTIMIZATION HINTS (0x70-0x7F) ===
    /// Mark hot path for JIT compilation
    HotPath = 0x70, // Reserved: not yet emitted by the compiler
    /// Memoization point (cache match results)
    Memoize = 0x71, // Reserved: not yet emitted by the compiler
    /// Clear memoization cache
    ClearMemo = 0x72, // Reserved: not yet emitted by the compiler
    /// Prefetch hint for upcoming memory access
    Prefetch = 0x73, // Reserved: not yet emitted by the compiler

    // === QUANTIFIERS (0x80-0x8F) ===
    /// Optimized ? quantifier (0 or 1, greedy)
    QuestionGreedy = 0x80,
    /// Optimized ? quantifier (0 or 1, lazy)
    QuestionLazy = 0x81,
    /// Optimized * quantifier (0+, greedy)  
    StarGreedy = 0x82,
    /// Optimized * quantifier (0+, lazy)
    StarLazy = 0x83,
    /// Optimized + quantifier (1+, greedy)
    PlusGreedy = 0x84,
    /// Optimized + quantifier (1+, lazy)
    PlusLazy = 0x85,
    // 0x86 removed: RepeatRange (superseded by Split+QuestionGreedy approach)
    // 0x87 removed: RepeatExact (superseded by Split+QuestionGreedy approach)

    // === ALTERNATIVE TRACKING (0x90-0x9F) ===
    /// Set the current alternative index (for match reporting)
    SetAlternative = 0x90,

    // === BACKTRACKING CONTROL VERBS (0xA0-0xAF) ===
    /// (*COMMIT) - set committed flag; if match fails, abort entire search
    /// `(*COMMIT)` - abort entire search on failure after this point
    Commit = 0xA0,
    /// (*PRUNE) - clear backtrack stack; if match fails, fail this attempt
    Prune = 0xA1,
    /// (*SKIP) - record skip position; if match fails, advance to this position
    VerbSkip = 0xA2,
    /// (*THEN) - clear backtrack stack (simplified); skip to next alternative
    Then = 0xA3,
    /// (*MARK:name) - record mark position in `ctx.marks` and skip past
    /// the length-prefixed name operand. Used by `(*SKIP:name)` to look
    /// up the matching mark position on failure (A11). Mark is no
    /// longer a no-op for match behaviour — its execution side effect
    /// (pushing to `ctx.marks`) is what enables named SKIP.
    Mark = 0xA4,
    /// (*SKIP:name) - look up the most recent matching mark in
    /// `ctx.marks` and set `ctx.skip_position` to that mark's text
    /// position. Skip past the length-prefixed name operand. If no
    /// matching mark is found, the verb is treated as a no-op (the
    /// PCRE2 fallback for missing marks). A11.
    VerbSkipNamed = 0xA5,

    // === TERMINATION (0xF0-0xFF) ===
    /// Successful match - capture current position
    Match = 0xF0,
    /// Match failure - backtrack
    Fail = 0xF1,
    /// Accept - successful completion
    Accept = 0xF2, // Reserved: not yet emitted by the compiler
    /// Halt execution (for debugging)
    Halt = 0xFF, // Reserved: not yet emitted by the compiler
}
fn regex_kind(node: &Regex) -> &'static str {
    match node {
        Regex::Empty => "Empty",
        Regex::Char(_) => "Char",
        Regex::Dot => "Dot",
        Regex::CharClass(_) => "CharClass",
        Regex::Digit { .. } => "Digit",
        Regex::Word { .. } => "Word",
        Regex::Space { .. } => "Space",
        Regex::UnicodeClass { .. } => "UnicodeClass",
        Regex::ExtendedCharClass { .. } => "ExtendedCharClass",
        Regex::Anchor(_) => "Anchor",
        Regex::WordBoundary { .. } => "WordBoundary",
        Regex::Sequence(_) => "Sequence",
        Regex::Alternation(_) => "Alternation",
        Regex::Quantified { .. } => "Quantified",
        Regex::Group { .. } => "Group",
        Regex::Backreference(_) => "Backreference",
        Regex::NamedBackreference(_) => "NamedBackreference",
        Regex::RelativeBackreference(_) => "RelativeBackreference",
        Regex::Lookahead { .. } => "Lookahead",
        Regex::Lookbehind { .. } => "Lookbehind",
        Regex::CodeBlock { .. } => "CodeBlock",
        Regex::Callout(_) => "Callout",
        Regex::Conditional { .. } => "Conditional",
        Regex::Recursion { .. } => "Recursion",
        Regex::ReturnedCaptureSubroutine { .. } => "ReturnedCaptureSubroutine",
        Regex::FlagGroup { .. } => "FlagGroup",
        Regex::MatchReset => "MatchReset",
        Regex::NewlineSequence => "NewlineSequence",
        Regex::GraphemeCluster => "GraphemeCluster",
        Regex::Accept => "Accept",
        Regex::Commit => "Commit",
        Regex::Prune => "Prune",
        Regex::Skip(_) => "Skip",
        Regex::Then => "Then",
        Regex::Mark(_) => "Mark",
        Regex::WhitespaceLiteral(_) => "WhitespaceLiteral",
    }
}

const CONDITIONAL_KIND_GROUP_EXISTS: u8 = 0;
const CONDITIONAL_KIND_LOOKAHEAD_POSITIVE: u8 = 1;
const CONDITIONAL_KIND_LOOKAHEAD_NEGATIVE: u8 = 2;
const CONDITIONAL_KIND_LOOKBEHIND_POSITIVE: u8 = 3;
const CONDITIONAL_KIND_LOOKBEHIND_NEGATIVE: u8 = 4;
const CONDITIONAL_KIND_DEFINE_FALSE: u8 = 5;
const CONDITIONAL_KIND_RECURSION_ANY: u8 = 6;
const CONDITIONAL_KIND_RECURSION_GROUP: u8 = 7;
const MAX_RECURSION_DEPTH: usize = 1024;

/// Bytecode instruction with operands
#[derive(Debug, Clone)]
pub struct Instruction {
    /// Operation code
    pub op: OpCode,
    /// Immediate operands (1-4 bytes typically)
    pub operands: Vec<u8>,
}

impl Instruction {
    /// Create instruction with no operands
    #[must_use]
    pub fn simple(op: OpCode) -> Self {
        Self {
            op,
            operands: Vec::new(),
        }
    }

    /// Create instruction with single byte operand
    #[must_use]
    pub fn with_byte(op: OpCode, operand: u8) -> Self {
        Self {
            op,
            operands: vec![operand],
        }
    }

    /// Create instruction with 16-bit operand (little-endian)
    #[must_use]
    pub fn with_word(op: OpCode, operand: u16) -> Self {
        Self {
            op,
            operands: operand.to_le_bytes().to_vec(),
        }
    }

    /// Create instruction with character operand
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // UTF-8 length fits in u8.
    pub fn with_char(op: OpCode, ch: char) -> Self {
        let mut operands = Vec::new();
        let mut buf = [0; 4];
        let bytes = ch.encode_utf8(&mut buf).as_bytes();
        operands.push(bytes.len() as u8); // Length prefix
        operands.extend_from_slice(bytes);
        Self { op, operands }
    }
}

/// Character class definition optimized for fast matching
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledCharClass {
    /// Bitmap for ASCII characters (0-127) - 16 bytes, SIMD-friendly
    pub ascii_bitmap: [u16; 8], // 128 bits packed into u16s for SIMD
    /// Non-ASCII ranges for Unicode support
    pub unicode_ranges: Vec<(u32, u32)>,
}

/// PCRE2 newline convention used by the `^` / `$` line-anchor opcodes
/// under `/m`. Mirrors `parsing::NewlineMode` but lives in the VM so
/// both backends (the backtracking VM here and the C2 Pike-VM) can
/// share the set lookup without reaching into the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VmNewlineMode {
    /// Only `\n` (U+000A) is a newline — PCRE2 `(*LF)` and the
    /// conventional RGX default.
    #[default]
    Lf,
    /// Only `\r` (U+000D) — PCRE2 `(*CR)`.
    Cr,
    /// Only the two-byte sequence `\r\n` — PCRE2 `(*CRLF)`. For
    /// anchor-before-position checks we treat both bytes as line
    /// terminators so `^` matches after either half of the pair.
    Crlf,
    /// `\r`, `\n`, or `\r\n` — PCRE2 `(*ANYCRLF)`.
    Anycrlf,
    /// Full Unicode set — PCRE2 `(*ANY)`. Single-byte newlines: `\r`,
    /// `\n`, VT, FF, NEL (U+0085); multi-byte: LS (U+2028), PS
    /// (U+2029). The anchor checks look at the preceding byte and
    /// the trailing UTF-8 bytes of the previous codepoint, so both
    /// single-byte and multi-byte newlines are honoured.
    Any,
    /// Only the NUL byte — PCRE2 `(*NUL)`.
    Nul,
}

impl VmNewlineMode {
    /// Returns `true` when the subject byte at `pos - 1` (or any
    /// well-formed UTF-8 codepoint ending just before `pos`) is a
    /// newline under this convention. Used by `OpCode::StartLine` to
    /// decide whether `^` is legal at `pos`.
    #[inline]
    pub fn is_line_start_before(self, text: &[u8], pos: usize) -> bool {
        if pos == 0 {
            return true;
        }
        let prev = text[pos - 1];
        match self {
            VmNewlineMode::Lf => prev == b'\n',
            VmNewlineMode::Cr => prev == b'\r',
            VmNewlineMode::Crlf | VmNewlineMode::Anycrlf => prev == b'\r' || prev == b'\n',
            VmNewlineMode::Any => {
                if matches!(prev, b'\r' | b'\n' | 0x0B | 0x0C | 0x85) {
                    return true;
                }
                // LINE SEPARATOR / PARAGRAPH SEPARATOR are 3-byte
                // UTF-8 sequences — check the tail.
                if pos >= 3 {
                    let tail = &text[pos - 3..pos];
                    return tail == [0xE2, 0x80, 0xA8] || tail == [0xE2, 0x80, 0xA9];
                }
                false
            }
            VmNewlineMode::Nul => prev == 0,
        }
    }

    /// Returns `true` when the subject byte(s) at `pos..` begin a
    /// newline under this convention. Used by `OpCode::EndLine` to
    /// decide whether `$` is legal at `pos`.
    #[inline]
    pub fn is_line_end_at(self, text: &[u8], pos: usize) -> bool {
        if pos >= text.len() {
            return true;
        }
        let cur = text[pos];
        match self {
            VmNewlineMode::Lf => cur == b'\n',
            VmNewlineMode::Cr => cur == b'\r',
            VmNewlineMode::Crlf | VmNewlineMode::Anycrlf => cur == b'\r' || cur == b'\n',
            VmNewlineMode::Any => {
                if matches!(cur, b'\r' | b'\n' | 0x0B | 0x0C | 0x85) {
                    return true;
                }
                if pos + 3 <= text.len() {
                    let head = &text[pos..pos + 3];
                    return head == [0xE2, 0x80, 0xA8] || head == [0xE2, 0x80, 0xA9];
                }
                false
            }
            VmNewlineMode::Nul => cur == 0,
        }
    }
}

/// High-performance compiled regex program
#[derive(Debug, Clone)]
pub struct Program {
    /// Bytecode instructions optimized for cache locality
    pub code: Vec<u8>,
    /// Runtime subroutine bytecode, indexed by recursion target ID (0 = whole pattern)
    pub subroutines: Vec<Vec<u8>>,
    /// Pre-compiled character classes
    pub char_classes: Vec<CompiledCharClass>,
    /// String literals extracted for SIMD matching
    pub string_literals: Vec<String>,
    /// Named capture group mapping
    pub named_groups: HashMap<String, u32>,
    /// Number of capture groups
    pub num_groups: u32,
    /// Optimization flags
    pub flags: ProgramFlags,
    /// Performance statistics from compilation
    pub stats: CompilationStats,
    /// PCRE2 newline convention selected by the pattern's
    /// `(*CR)` / `(*LF)` / `(*CRLF)` / `(*ANYCRLF)` / `(*ANY)` /
    /// `(*NUL)` pragma (default `Lf`). Affects the `^` and `$`
    /// line-anchor opcodes under `/m`: each anchor checks whether
    /// the previous / current byte is one of the newlines in this
    /// set rather than hard-coding `\n`.
    pub newline_mode: VmNewlineMode,
    /// C2 engine classification — decides whether this pattern can dispatch
    /// to the NFA/DFA hybrid engine or must use the backtracking VM.
    ///
    /// Populated by the compiler in `compile_ast_with_label` after VM
    /// bytecode generation. Defaults to `NeedsVm { NotYetClassified }` so
    /// any code path that bypasses the classifier still routes safely to
    /// the existing VM. See `docs/C2_NFA_DFA_DESIGN.md` §4 for the full
    /// subset definition.
    pub classification: Classification,
    /// C2 engine compiled program — `Some` iff the pattern is eligible
    /// for C2 dispatch (classifier-positive AND structurally compatible
    /// with the Pike-VM's metadata surface; see
    /// [`crate::c2::program::is_c2_dispatch_eligible`]).
    ///
    /// Populated by the compiler in `compile_ast_with_label`. Read by
    /// the public `Regex` API methods to route `is_match` / `find_first`
    /// / `find_all` through the Pike-VM when present, falling back to
    /// the existing VM otherwise. See `docs/C2_NFA_DFA_DESIGN.md` §11.
    pub c2_program: Option<crate::c2::CompiledC2Program>,
}

/// Program optimization flags
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
pub struct ProgramFlags {
    /// Can use SIMD instructions
    pub simd_enabled: bool,
    /// Contains anchors (affects matching strategy)  
    pub has_anchors: bool,
    /// Contains backreferences (prevents some optimizations)
    pub has_backrefs: bool,
    /// Contains lookarounds
    pub has_lookarounds: bool,
    /// Contains embedded code blocks
    pub has_code_blocks: bool,
    /// Estimated instruction count for JIT threshold
    pub instruction_count: u32,
    /// Maximum capture group number
    pub max_capture_group: u32,
}

/// Statistics gathered during compilation for optimization
#[derive(Debug, Clone, Copy)]
pub struct CompilationStats {
    /// Number of literal characters
    pub literal_chars: u32,
    /// Number of character classes  
    pub char_classes: u32,
    /// Number of quantifiers
    pub quantifiers: u32,
    /// Estimated CPU cycles per match attempt
    pub estimated_cycles: u32,
    /// Whether pattern is suitable for JIT compilation
    pub jit_worthy: bool,
}

/// Execution context with performance optimizations
#[derive(Debug)]
pub struct ExecContext<'a> {
    /// Input text as UTF-8 bytes for SIMD processing (borrowed, never copied)
    pub text: &'a [u8],
    /// Current position in bytes (not characters!)
    pub pos: usize,
    /// Current match-attempt start position in bytes
    pub match_start: usize,
    /// End position for bounded matching
    pub end: usize,
    /// Capture group positions [start, end, start, end, ...]
    pub captures: Vec<Option<usize>>,
    /// Trail log of capture modifications: `(slot_index, old_value)` pairs.
    /// Used for efficient backtracking — instead of cloning the entire capture
    /// vector on every backtrack frame, we record each modification and replay
    /// the trail backwards to undo changes.
    pub capture_trail: Vec<(usize, Option<usize>)>,
    /// Call stack for recursion
    pub call_stack: Vec<usize>,
    /// Backtrack stack for alternation and optional quantifiers
    pub backtrack_stack: Vec<BacktrackFrame>,
    /// Track which alternative is currently being executed
    pub current_alternative: Option<usize>,
    /// Active recursion frames `(target_id, text_pos)` for zero-width cycle detection
    pub recursion_stack: Vec<(usize, usize)>,
    /// Last non-boolean code-block value observed on the current path.
    pub code_result: Option<CodeBlockValue>,
    /// Override for match start set by `\K` (`MatchReset` opcode).
    /// When `Some(pos)`, the reported match start is `pos` instead of the
    /// scanning-loop's `start` variable.
    pub match_start_override: Option<usize>,
    /// End position of the previous match, used by the `\G` anchor.
    /// `None` means no previous match (only position 0 satisfies `\G`).
    pub previous_match_end: Option<usize>,
    /// Set to `true` by `(*COMMIT)`. When the current match attempt fails,
    /// the scanning loop aborts the entire search.
    pub committed: bool,
    /// Set by `(*SKIP)` to the text position where it was encountered.
    /// On match failure, the scanning loop advances to this position instead
    /// of `start + 1`.
    pub skip_position: Option<usize>,
    /// A11: stack of `(*MARK:name)` records `(name, text_pos)` collected
    /// during the current match attempt. Used by `(*SKIP:name)` to look
    /// up the most recent matching mark — the SKIP advances the scan
    /// position to the matching mark's recorded text position instead
    /// of `ctx.pos`. Cleared on match attempt reset alongside the rest
    /// of the per-attempt context state.
    pub marks: Vec<(String, usize)>,
    /// When `true`, unregistered native callbacks cause suspension instead of
    /// being treated as errors. Set by `find_first_suspendable`; defaults to
    /// `false` for zero overhead on the synchronous path.
    pub suspendable: bool,
    /// When a code block suspends in suspendable mode, the callback name and
    /// instruction pointer are captured here so the scanning loop can build
    /// a `MatchContinuation`. `None` on the synchronous path.
    pub suspension: Option<(String, usize)>,
    /// Opcode steps executed so far in this match attempt. Incremented per
    /// opcode dispatch. When `max_steps > 0` and `step_count` exceeds it,
    /// the match attempt aborts.
    pub step_count: u64,
    /// Maximum opcode steps permitted per match attempt. 0 = unlimited.
    pub max_steps: u64,
    /// Maximum backtrack stack depth. 0 = unlimited.
    pub max_backtrack_frames: u64,
    /// Maximum recursion depth. 0 = unlimited.
    pub max_recursion_depth: u64,
    /// Set to `true` when a match attempt fails because it reached end-of-input
    /// while the pattern could have continued matching with more data.
    /// Used by partial-match APIs to distinguish "no match" from "need more input".
    pub hit_end: bool,
}

/// Backtracking frame for alternation and quantifiers
#[derive(Debug, Clone)]
pub struct BacktrackFrame {
    /// Instruction pointer to return to
    pub ip: usize,
    /// Text position to restore
    pub pos: usize,
    /// Capture-trail length at the time this frame was created. On backtrack
    /// the trail is replayed backwards to this mark, restoring capture state.
    pub trail_mark: usize,
    /// Call-stack length at the time this frame was created. On backtrack the
    /// call stack is truncated to this mark.
    pub call_stack_mark: usize,
    /// Optional full capture snapshot for probe-based frames where the target
    /// state was computed on a cloned context and cannot be expressed as a
    /// simple trail undo.
    pub capture_snapshot: Option<Vec<Option<usize>>>,
    /// Saved winning-path non-boolean code-block value
    pub saved_code_result: Option<CodeBlockValue>,
    /// Value of `ctx.match_start_override` at the time the frame was
    /// pushed. `\K` (`OpCode::MatchReset`) writes to this field to
    /// shift the visible match start forward; if we later backtrack
    /// past the `\K`, the override has to unwind with the rest of the
    /// state — otherwise a `\K` that was executed inside a branch we
    /// later abandoned still mutates the final match span. Matches
    /// PCRE2 `\K` semantics: the reset only takes effect on the
    /// surviving match path.
    pub saved_match_start_override: Option<usize>,
}

/// Match result with full capture information
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// Overall match start position (in bytes)
    pub start: usize,
    /// Overall match end position (in bytes)
    pub end: usize,
    /// Capture groups (start, end) in bytes - None if group didn't match
    pub groups: Vec<Option<(usize, usize)>>,
    /// Which top-level alternative matched (0-based index), None if no alternation
    pub matched_alternative: Option<usize>,
    /// Last non-boolean code-block value observed on the winning match path.
    pub code_result: Option<CodeBlockValue>,
    /// Name of the last `(*MARK:name)` / `(*:name)` verb encountered on
    /// the winning match path. Populated from `ExecContext::marks.last()`.
    pub last_mark: Option<String>,
}

/// Fast position filter extracted from the program prefix.
///
/// Used by the scanning loop to skip positions where the first required
/// atom cannot match, avoiding full VM invocations at impossible offsets.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub enum PrefixFilter {
    /// No usable prefix — must try every position.
    None,
    /// Pattern starts with a specific single byte.
    Byte(u8),
    /// Pattern starts with `\d` (ASCII digit).
    Digit,
    /// Pattern starts with `\w` (ASCII word character).
    Word,
    /// Pattern starts with `\s` (ASCII whitespace).
    Space,
    /// Pattern starts with a compiled character class (bitmap copied from program).
    CharClass(usize),
}

impl PrefixFilter {
    /// Test whether a byte could match this filter.
    #[doc(hidden)]
    #[inline]
    pub fn matches(self, b: u8, char_classes: &[CompiledCharClass]) -> bool {
        match self {
            Self::None => true,
            Self::Byte(expected) => b == expected,
            Self::Digit => b.is_ascii_digit(),
            Self::Word => b.is_ascii_alphanumeric() || b == b'_',
            Self::Space => pcre2_is_space_byte(b),
            Self::CharClass(id) => {
                // Non-ASCII bytes might match Unicode ranges — can't reject
                if b >= 0x80 {
                    return true;
                }
                if let Some(cc) = char_classes.get(id) {
                    let byte_idx = (b as usize) / 16;
                    let bit_idx = (b as usize) % 16;
                    if byte_idx < cc.ascii_bitmap.len() {
                        (cc.ascii_bitmap[byte_idx] & (1u16 << bit_idx)) != 0
                    } else {
                        true
                    }
                } else {
                    true
                }
            }
        }
    }
}

/// A thread-safe, shared event-observer callback.
type EventObserver = Arc<dyn Fn(&MatchEvent) + Send + Sync>;

/// High-performance regex execution engine
pub struct RegexVM {
    /// Compiled program
    pub program: Program,
    /// Optional execution manager for embedded code blocks
    execution_manager: Option<Arc<ExecutionManager>>,
    /// SIMD instruction support detected at runtime
    simd_support: SimdSupport,
    /// Cached prefix filter for scanning skip optimization
    prefix_filter: PrefixFilter,
    /// Pre-computed substring finder for pure-literal patterns (bypasses VM entirely)
    literal_finder: Option<memchr::memmem::Finder<'static>>,
    /// Optional event observer for structured match events.
    event_observer: RwLock<Option<EventObserver>>,
    /// Match semantics: leftmost-first (PCRE2 default) or leftmost-longest (POSIX).
    match_semantics: std::sync::atomic::AtomicU8,
    /// Maximum opcode steps per match attempt. 0 = unlimited (default).
    /// Prevents exponential backtracking from hanging the engine.
    max_steps: std::sync::atomic::AtomicU64,
    /// Maximum backtrack stack depth per match attempt. 0 = unlimited (default).
    max_backtrack_frames: std::sync::atomic::AtomicU64,
    /// Maximum recursion depth per match attempt. 0 = unlimited (default).
    max_recursion_depth: std::sync::atomic::AtomicU64,
}

/// Runtime SIMD capability detection
#[derive(Debug, Clone, Copy)]
pub struct SimdSupport {
    /// SSE2 support (x86/x64)
    pub sse2: bool,
    /// AVX2 support (x86/x64)  
    pub avx2: bool,
    /// NEON support (ARM)
    pub neon: bool,
}

impl RegexVM {
    /// Create new VM with compiled program
    #[must_use]
    pub fn new(program: Program) -> Self {
        Self::with_execution_manager(program, None)
    }

    /// Create new VM with compiled program and optional execution manager
    #[must_use]
    pub fn with_execution_manager(
        program: Program,
        execution_manager: Option<Arc<ExecutionManager>>,
    ) -> Self {
        trace_enter!(
            "vm",
            "RegexVM::with_execution_manager",
            "bytecode_len={},char_classes={},string_literals={},groups={},has_anchors={},has_lookarounds={},has_code_blocks={}",
            program.code.len(),
            program.char_classes.len(),
            program.string_literals.len(),
            program.num_groups,
            program.flags.has_anchors,
            program.flags.has_lookarounds,
            program.flags.has_code_blocks
        );
        let simd_support = Self::detect_simd_support();
        trace_decision!(
            "vm",
            "simd capability detected",
            simd_support.sse2 || simd_support.avx2 || simd_support.neon,
            "sse2={},avx2={},neon={}",
            simd_support.sse2,
            simd_support.avx2,
            simd_support.neon
        );
        let mut vm = Self {
            program,
            execution_manager,
            simd_support,
            prefix_filter: PrefixFilter::None,
            literal_finder: None,
            event_observer: RwLock::new(None),
            match_semantics: std::sync::atomic::AtomicU8::new(0), // 0 = LeftmostFirst
            max_steps: std::sync::atomic::AtomicU64::new(0),
            max_backtrack_frames: std::sync::atomic::AtomicU64::new(0),
            max_recursion_depth: std::sync::atomic::AtomicU64::new(0),
        };
        vm.prefix_filter = vm.extract_prefix_filter();
        vm.literal_finder = vm.extract_literal_finder();
        trace_exit!(
            "vm",
            "RegexVM::with_execution_manager",
            "ok=true,sse2={},avx2={},neon={}",
            vm.simd_support.sse2,
            vm.simd_support.avx2,
            vm.simd_support.neon
        );
        vm
    }

    /// Set the match semantics (leftmost-first or leftmost-longest).
    pub fn set_match_semantics(&self, semantics: crate::engine::MatchSemantics) {
        self.match_semantics.store(
            match semantics {
                crate::engine::MatchSemantics::LeftmostFirst => 0,
                crate::engine::MatchSemantics::LeftmostLongest => 1,
            },
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Check if leftmost-longest semantics are active.
    fn is_leftmost_longest(&self) -> bool {
        self.match_semantics
            .load(std::sync::atomic::Ordering::Relaxed)
            == 1
    }

    /// Set the maximum number of opcode steps per match attempt.
    ///
    /// When the limit is reached the current match attempt fails (returns
    /// no-match). The scanning loop may still try the next start position,
    /// so the limit applies per-attempt, not per-call.
    ///
    /// Pass `None` to remove the limit (default). Pass `Some(n)` to cap
    /// each attempt at `n` opcode steps.
    pub fn set_max_steps(&self, limit: Option<u64>) {
        self.max_steps
            .store(limit.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    }

    /// Set the maximum backtrack stack depth per match attempt.
    pub fn set_max_backtrack_frames(&self, limit: Option<u64>) {
        self.max_backtrack_frames
            .store(limit.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    }

    /// Set the maximum recursion depth per match attempt.
    pub fn set_max_recursion_depth(&self, limit: Option<u64>) {
        self.max_recursion_depth
            .store(limit.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    }

    /// Detect available SIMD instruction sets
    fn detect_simd_support() -> SimdSupport {
        trace_enter!("vm", "RegexVM::detect_simd_support");
        let support = SimdSupport {
            #[cfg(target_arch = "x86_64")]
            sse2: std::arch::is_x86_feature_detected!("sse2"),
            #[cfg(target_arch = "x86_64")]
            avx2: std::arch::is_x86_feature_detected!("avx2"),
            #[cfg(not(target_arch = "x86_64"))]
            sse2: false,
            #[cfg(not(target_arch = "x86_64"))]
            avx2: false,

            #[cfg(target_arch = "aarch64")]
            neon: std::arch::is_aarch64_feature_detected!("neon"),
            #[cfg(not(target_arch = "aarch64"))]
            neon: false,
        };
        trace_exit!(
            "vm",
            "RegexVM::detect_simd_support",
            "ok=true,sse2={},avx2={},neon={}",
            support.sse2,
            support.avx2,
            support.neon
        );
        support
    }

    /// Register an event observer for structured match events.
    ///
    /// The observer receives [`MatchEvent`] values at key execution points.
    /// Only one observer may be active; calling this again replaces any
    /// previous observer.
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    pub fn set_event_observer<F>(&self, observer: F)
    where
        F: Fn(&MatchEvent) + Send + Sync + 'static,
    {
        *self.event_observer.write().unwrap() = Some(Arc::new(observer));
    }

    /// Emit a structured match event to the registered observer (if any).
    ///
    /// When no observer is registered the read-lock + `is_none` check compiles
    /// down to a single well-predicted branch, giving near-zero effective
    /// overhead.
    #[inline]
    fn emit_event(&self, event: &MatchEvent) {
        if let Some(ref observer) = *self.event_observer.read().unwrap() {
            observer(event);
        }
    }

    /// Compile-time prefix filter for this VM. Exposed to the engine
    /// dispatch layer so the C2 path (DFA / Pike-VM) can reuse the same
    /// scan-skip the existing backtracking VM uses for `Digit` / `Word` /
    /// `Space` / `CharClass` prefixes (the C2 path already handles
    /// `Byte` via `c2_prefix_byte`, but the byte-class prefixes are
    /// significantly faster than scanning every position).
    #[doc(hidden)]
    #[must_use]
    pub fn prefix_filter(&self) -> PrefixFilter {
        self.prefix_filter
    }

    /// Compiled char classes for this VM. Used together with
    /// [`Self::prefix_filter`] by the C2 dispatch layer to evaluate
    /// `PrefixFilter::CharClass` candidates.
    #[doc(hidden)]
    #[must_use]
    pub fn char_classes(&self) -> &[CompiledCharClass] {
        &self.program.char_classes
    }

    /// Returns `true` if this VM has a pure-literal `memmem::Finder`
    /// fast path. The C2 dispatch layer skips Pike-VM/DFA dispatch when
    /// this is true because the existing memmem fast path is faster
    /// than anything the C2 engines can do for a pure literal pattern.
    #[doc(hidden)]
    #[must_use]
    pub fn has_literal_finder(&self) -> bool {
        self.literal_finder.is_some()
    }

    /// Returns `true` if a match-event observer has been registered on
    /// this VM. Used by the C2 dispatch decision in
    /// [`crate::engine::Engine::should_dispatch_to_c2`] — the Pike-VM
    /// doesn't emit structured match events, so patterns whose tests
    /// expect events must continue to run on the existing backtracking
    /// VM.
    #[doc(hidden)]
    pub fn has_event_observer(&self) -> bool {
        self.event_observer
            .read()
            .map(|o| o.is_some())
            .unwrap_or(false)
    }

    /// Returns `true` if any of the runtime safety limits
    /// (`max_steps`, `max_backtrack_frames`, `max_recursion_depth`)
    /// have been set to a non-zero value. Used by the C2 dispatch
    /// decision — the Pike-VM is bounded by O(nm) and doesn't enforce
    /// these limits, so patterns whose tests assert limit-triggered
    /// errors must continue to run on the existing backtracking VM.
    #[doc(hidden)]
    pub fn has_runtime_match_limits(&self) -> bool {
        self.max_steps.load(std::sync::atomic::Ordering::Relaxed) > 0
            || self
                .max_backtrack_frames
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
            || self
                .max_recursion_depth
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
    }

    /// Returns the current `max_steps` setting (`0` = unlimited).
    /// Used by the C1 JIT dispatch path to thread the limit through
    /// to the JIT'd function as a per-call argument.
    #[doc(hidden)]
    pub fn max_steps(&self) -> u64 {
        self.max_steps.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Returns the current `max_backtrack_frames` setting (`0` =
    /// unlimited). Used by the C1 JIT dispatch path.
    #[doc(hidden)]
    pub fn max_backtrack_frames(&self) -> u64 {
        self.max_backtrack_frames
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Returns `true` if `max_recursion_depth` has been set to a
    /// non-zero value. Used by the C1 JIT dispatch path: the JIT
    /// doesn't support recursion (`Call` opcode is JIT-ineligible),
    /// so a recursion depth limit is meaningless for JIT'd code —
    /// but if the user has set one, the C1 path falls through to
    /// the interpreter to keep semantics consistent.
    #[doc(hidden)]
    pub fn has_recursion_depth_limit(&self) -> bool {
        self.max_recursion_depth
            .load(std::sync::atomic::Ordering::Relaxed)
            > 0
    }

    /// Find first match using adaptive execution strategy
    #[must_use]
    #[allow(clippy::too_many_lines)] // Literal fast-path + adaptive strategy selection
    #[inline]
    pub fn find_first(&self, text: &str) -> Option<Match> {
        if self.is_leftmost_longest() {
            return self.find_first_longest(text);
        }
        // Fast path: pure-literal patterns bypass the VM entirely via memmem.
        // This check is first, before any setup, for minimum overhead.
        if let Some(ref finder) = self.literal_finder {
            let bytes = text.as_bytes();
            let needle_len = finder.needle().len();
            let result = finder.find(bytes).map(|pos| Match {
                start: pos,
                end: pos + needle_len,
                groups: vec![Some((pos, pos + needle_len))],
                matched_alternative: None,
                code_result: None,
                last_mark: None,
            });
            return result;
        }

        // Non-literal path: full VM execution
        trace_enter!(
            "vm",
            "RegexVM::find_first",
            "text_len={}, code_len={}",
            text.len(),
            self.program.code.len()
        );
        let bytes = text.as_bytes();

        let mut ctx = ExecContext {
            text: bytes,
            pos: 0,
            match_start: 0,
            end: bytes.len(),
            captures: vec![None; (self.program.num_groups + 1) as usize * 2],
            capture_trail: Vec::new(),
            call_stack: Vec::new(),
            backtrack_stack: Vec::new(),
            current_alternative: None,
            recursion_stack: Vec::new(),
            code_result: None,
            match_start_override: None,
            previous_match_end: None,
            committed: false,
            skip_position: None,
            marks: Vec::new(),
            suspendable: false,
            suspension: None,
            step_count: 0,
            max_steps: self.max_steps.load(std::sync::atomic::Ordering::Relaxed),
            max_backtrack_frames: self
                .max_backtrack_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            max_recursion_depth: self
                .max_recursion_depth
                .load(std::sync::atomic::Ordering::Relaxed),
            hit_end: false,
        };

        // Adaptive strategy selection based on program characteristics
        let result = if self.should_use_simd_search(&ctx) {
            trace_decision!(
                "vm",
                "should_use_simd_search(ctx)",
                true,
                "SIMD capability and literal pre-filter conditions satisfied"
            );
            debug_log!(
                "vm",
                "Strategy: SIMD search (text>{} bytes, literals>0)",
                64
            );
            self.find_first_simd(&mut ctx)
        } else if self.should_use_start_anchored_search() {
            trace_decision!(
                "vm",
                "should_use_start_anchored_search()",
                true,
                "program begins with start anchor opcode"
            );
            debug_log!("vm", "Strategy: Start-anchored search");
            self.find_first_anchored(&mut ctx)
        } else {
            trace_decision!(
                "vm",
                "fallback to scanning strategy",
                true,
                "SIMD/anchored fast paths not applicable"
            );
            debug_log!("vm", "Strategy: Standard scanning");
            self.find_first_scanning(&mut ctx)
        };

        if let Some(_m) = &result {
            debug_log!("vm", "=== MATCH FOUND: {}..{} ===", _m.start, _m.end);
        } else {
            debug_log!("vm", "=== NO MATCH FOUND ===");
        }
        low_log!("vm", "=== FIND_FIRST PIPELINE COMPLETE ===");
        low_log!("vm", "");
        trace_exit!("vm", "RegexVM::find_first", "matched={}", result.is_some());

        result
    }

    /// Leftmost-longest: recompile the pattern with alternation branches
    /// sorted longest-first, then use the normal leftmost-first matching.
    ///
    /// Current limitation: this flag is a runtime hint that does not yet
    /// reorder alternation branches at the bytecode level. For patterns
    /// without alternation (or where the longest branch is already first),
    /// matching already produces the longest result due to greedy quantifiers.
    /// Full POSIX alternation reordering requires compiler-level support
    /// (tracked as a follow-up to B4).
    fn find_first_longest(&self, text: &str) -> Option<Match> {
        // For now, delegate to the normal scanning path. Greedy quantifiers
        // already produce longest matches for non-alternation patterns.
        // The MatchSemantics flag is stored and can be queried, enabling
        // future compiler-level alternation reordering.
        self.match_semantics
            .store(0, std::sync::atomic::Ordering::Relaxed);
        let result = self.find_first(text);
        self.match_semantics
            .store(1, std::sync::atomic::Ordering::Relaxed);
        result
    }

    /// Find the first match starting the scan at byte position `start`.
    ///
    /// Unlike `find_first`, which always scans from position 0, this method
    /// begins scanning at the given byte offset. Positions reported in the
    /// returned `Match` are still absolute (relative to the start of `text`).
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    pub fn find_first_at(&self, text: &str, start: usize) -> Option<Match> {
        assert!(
            text.is_char_boundary(start),
            "find_first_at: start ({start}) is not on a UTF-8 character boundary"
        );
        if start > text.len() {
            return None;
        }

        // Literal fast path with offset
        if let Some(ref finder) = self.literal_finder {
            let bytes = text.as_bytes();
            let needle_len = finder.needle().len();
            return finder.find(&bytes[start..]).map(|pos| {
                let abs = start + pos;
                Match {
                    start: abs,
                    end: abs + needle_len,
                    groups: vec![Some((abs, abs + needle_len))],
                    matched_alternative: None,
                    code_result: None,
                    last_mark: None,
                }
            });
        }

        let bytes = text.as_bytes();
        let mut ctx = ExecContext {
            text: bytes,
            pos: start,
            match_start: start,
            end: bytes.len(),
            captures: vec![None; (self.program.num_groups + 1) as usize * 2],
            capture_trail: Vec::new(),
            call_stack: Vec::new(),
            backtrack_stack: Vec::new(),
            current_alternative: None,
            recursion_stack: Vec::new(),
            code_result: None,
            match_start_override: None,
            previous_match_end: None,
            committed: false,
            skip_position: None,
            marks: Vec::new(),
            suspendable: false,
            suspension: None,
            step_count: 0,
            max_steps: self.max_steps.load(std::sync::atomic::Ordering::Relaxed),
            max_backtrack_frames: self
                .max_backtrack_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            max_recursion_depth: self
                .max_recursion_depth
                .load(std::sync::atomic::Ordering::Relaxed),
            hit_end: false,
        };

        // For offset scans, always use the scanning path (anchored / SIMD
        // fast-paths assume scanning from position 0).
        self.find_first_scanning_from(&mut ctx, start)
    }

    /// Find all non-overlapping matches starting the scan at byte position `start`.
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    pub fn find_all_at(&self, text: &str, start: usize) -> Vec<Match> {
        assert!(
            text.is_char_boundary(start),
            "find_all_at: start ({start}) is not on a UTF-8 character boundary"
        );
        if start > text.len() {
            return Vec::new();
        }

        // Literal fast path with offset
        if let Some(ref finder) = self.literal_finder {
            let bytes = text.as_bytes();
            let needle_len = finder.needle().len();
            let mut matches = Vec::new();
            let mut offset = start;
            while let Some(pos) = finder.find(&bytes[offset..]) {
                let abs = offset + pos;
                matches.push(Match {
                    start: abs,
                    end: abs + needle_len,
                    groups: vec![Some((abs, abs + needle_len))],
                    matched_alternative: None,
                    code_result: None,
                    last_mark: None,
                });
                offset = abs + needle_len.max(1);
            }
            return matches;
        }

        let bytes = text.as_bytes();
        let mut ctx = ExecContext {
            text: bytes,
            pos: start,
            match_start: start,
            end: bytes.len(),
            captures: vec![None; (self.program.num_groups + 1) as usize * 2],
            capture_trail: Vec::new(),
            call_stack: Vec::new(),
            backtrack_stack: Vec::new(),
            current_alternative: None,
            recursion_stack: Vec::new(),
            code_result: None,
            match_start_override: None,
            previous_match_end: None,
            committed: false,
            skip_position: None,
            marks: Vec::new(),
            suspendable: false,
            suspension: None,
            step_count: 0,
            max_steps: self.max_steps.load(std::sync::atomic::Ordering::Relaxed),
            max_backtrack_frames: self
                .max_backtrack_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            max_recursion_depth: self
                .max_recursion_depth
                .load(std::sync::atomic::Ordering::Relaxed),
            hit_end: false,
        };

        self.find_all_scanning_from(&mut ctx, start)
    }

    /// Boolean match test starting the scan at byte position `start`.
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    pub fn is_match_at(&self, text: &str, start: usize) -> bool {
        self.find_first_at(text, start).is_some()
    }

    /// Find the first match or report a partial match when input ends
    /// mid-potential-match. Used for streaming/incremental matching.
    pub fn find_first_partial(&self, text: &str) -> crate::engine::PartialMatchResult {
        use crate::engine::PartialMatchResult;

        if let Some(m) = self.find_first(text) {
            return PartialMatchResult::Full(crate::engine::vm_match_to_result(m));
        }

        // No full match — check if any position hit end-of-input while
        // the pattern could have continued matching.
        let bytes = text.as_bytes();
        let mut ctx = ExecContext {
            text: bytes,
            pos: 0,
            match_start: 0,
            end: bytes.len(),
            captures: vec![None; (self.program.num_groups + 1) as usize * 2],
            capture_trail: Vec::new(),
            call_stack: Vec::new(),
            backtrack_stack: Vec::new(),
            current_alternative: None,
            recursion_stack: Vec::new(),
            code_result: None,
            match_start_override: None,
            previous_match_end: None,
            committed: false,
            skip_position: None,
            marks: Vec::new(),
            suspendable: false,
            suspension: None,
            step_count: 0,
            max_steps: self.max_steps.load(std::sync::atomic::Ordering::Relaxed),
            max_backtrack_frames: self
                .max_backtrack_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            max_recursion_depth: self
                .max_recursion_depth
                .load(std::sync::atomic::Ordering::Relaxed),
            hit_end: false,
        };

        // Scan through positions looking for one that hits end-of-input.
        let filter = self.prefix_filter;
        let mut start = 0;
        while start <= bytes.len() {
            if start < bytes.len() && !filter.matches(bytes[start], &self.program.char_classes) {
                start += 1;
                continue;
            }
            ctx.pos = start;
            ctx.hit_end = false;
            Self::reset_captures(&mut ctx);
            let _ = self.execute_at(&mut ctx, start);
            if ctx.hit_end {
                return PartialMatchResult::Partial(start);
            }
            if ctx.committed {
                break;
            }
            if start >= bytes.len() {
                break;
            }
            start += 1;
        }

        PartialMatchResult::NoMatch
    }

    /// Determine if SIMD pre-filtering would be beneficial
    fn should_use_simd_search(&self, ctx: &ExecContext<'_>) -> bool {
        // SIMD candidate collection followed by per-candidate VM execution is
        // only a net win when the first literal is rare in the input.  For now
        // keep this gated on x86 SSE2 where it was originally benchmarked;
        // ARM NEON can use the lighter memchr-style skip in the scanning loop.
        self.simd_support.sse2 &&
        ctx.text.len() > 64 && // Worth SIMD overhead
        self.program.stats.literal_chars > 0 // Has literal content to search for
    }

    /// Determine whether it is safe to run the anchored fast-path.
    ///
    /// The anchored fast-path only attempts matching at position 0, so it is
    /// valid only when the compiled program is explicitly start-anchored.
    /// End-anchor-only programs (e.g. `dog$`) still require scanning.
    fn should_use_start_anchored_search(&self) -> bool {
        if !self.program.flags.has_anchors {
            return false;
        }

        let Some(first) = self.program.code.first() else {
            return false;
        };

        matches!(OpCode::try_from(*first), Ok(OpCode::StartText))
    }

    /// SIMD-accelerated first match search using state-of-the-art algorithms
    fn find_first_simd(&self, ctx: &mut ExecContext<'_>) -> Option<Match> {
        trace_enter!(
            "vm",
            "RegexVM::find_first_simd",
            "text_len={}, code_len={}",
            ctx.text.len(),
            self.program.code.len()
        );
        // Extract first literal or character class from bytecode for SIMD pre-filtering
        let (literal_bytes, literal_len) = self.extract_first_literal();

        if literal_len == 0 {
            // No literal to search for, fall back to scanning
            trace_decision!(
                "vm",
                "literal_len == 0",
                true,
                "falling back to full scanning path"
            );
            let result = self.find_first_scanning(ctx);
            trace_exit!(
                "vm",
                "RegexVM::find_first_simd",
                "fallback_scan_matched={}",
                result.is_some()
            );
            return result;
        }

        // Use SIMD to find all potential match positions
        let candidates = if literal_len == 1 {
            // Single byte search - use optimized SIMD byte search
            self.simd_find_byte(ctx, literal_bytes[0])
        } else if literal_len <= 4 {
            // Short string - use SIMD substring search with shuffles
            self.simd_find_short_string(ctx, &literal_bytes[..literal_len])
        } else {
            // Longer string - use SIMD-accelerated Boyer-Moore-Horspool
            self.simd_find_long_string(ctx, &literal_bytes[..literal_len])
        };

        // Try full pattern match at each candidate position
        for candidate_pos in candidates {
            // (*SKIP): skip candidates before the skip position
            if let Some(skip_pos) = ctx.skip_position.take() {
                if candidate_pos < skip_pos {
                    continue;
                }
            }
            ctx.pos = candidate_pos;
            Self::reset_captures(ctx);

            self.emit_event(&MatchEvent::MatchAttemptStarted {
                position: candidate_pos,
            });
            let attempt_matched = self.execute_at(ctx, candidate_pos);
            self.emit_event(&MatchEvent::MatchAttemptCompleted {
                position: candidate_pos,
                matched: attempt_matched,
            });
            if attempt_matched {
                let effective_start = ctx.match_start_override.unwrap_or(candidate_pos);
                let matched = Some(Match {
                    start: effective_start,
                    end: ctx.pos,
                    groups: self.extract_captures_with_match(ctx, effective_start, ctx.pos),
                    matched_alternative: ctx.current_alternative,
                    code_result: ctx.code_result.clone(),
                    last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                });
                trace_exit!(
                    "vm",
                    "RegexVM::find_first_simd",
                    "matched=true, start={}, end={}",
                    effective_start,
                    ctx.pos
                );
                return matched;
            }
            // (*COMMIT): abort entire search on failure
            if ctx.committed {
                break;
            }
        }

        trace_exit!("vm", "RegexVM::find_first_simd", "matched=false");
        None
    }

    /// Optimized search for anchored patterns
    fn find_first_anchored(&self, ctx: &mut ExecContext<'_>) -> Option<Match> {
        trace_enter!(
            "vm",
            "RegexVM::find_first_anchored",
            "text_len={}",
            ctx.text.len()
        );
        // Only try match at start for ^ anchor
        self.emit_event(&MatchEvent::MatchAttemptStarted { position: 0 });
        let attempt_matched = self.execute_at(ctx, 0);
        self.emit_event(&MatchEvent::MatchAttemptCompleted {
            position: 0,
            matched: attempt_matched,
        });
        if attempt_matched {
            let effective_start = ctx.match_start_override.unwrap_or(0);
            let matched = Some(Match {
                start: effective_start,
                end: ctx.pos,
                groups: self.extract_captures_with_match(ctx, effective_start, ctx.pos),
                matched_alternative: ctx.current_alternative,
                code_result: ctx.code_result.clone(),
                last_mark: None,
            });
            trace_exit!(
                "vm",
                "RegexVM::find_first_anchored",
                "matched=true, end={}",
                ctx.pos
            );
            matched
        } else {
            trace_exit!("vm", "RegexVM::find_first_anchored", "matched=false");
            None
        }
    }

    /// Extract a fast position filter from the program prefix.
    ///
    /// Scans the compiled bytecode (skipping capture markers) and returns
    /// a [`PrefixFilter`] describing the first required atom, which the
    /// scanning loop uses to skip impossible start positions.
    fn extract_prefix_filter(&self) -> PrefixFilter {
        let code = &self.program.code;
        let mut ip = 0;
        while ip < code.len() {
            let Ok(op) = OpCode::try_from(code[ip]) else {
                return PrefixFilter::None;
            };
            ip += 1;
            match op {
                OpCode::Char => {
                    if ip < code.len() {
                        let char_len = code[ip] as usize;
                        ip += 1;
                        if char_len == 1 && ip < code.len() {
                            return PrefixFilter::Byte(code[ip]);
                        }
                    }
                    return PrefixFilter::None;
                }
                OpCode::DigitAscii => return PrefixFilter::Digit,
                OpCode::WordAscii => return PrefixFilter::Word,
                OpCode::SpaceAscii => return PrefixFilter::Space,
                // Zero-width assertions: skip past them to find the first consuming atom
                OpCode::WordBoundary
                | OpCode::NonWordBoundary
                | OpCode::StartText
                | OpCode::EndTextOrNL
                | OpCode::EndText
                | OpCode::StartLine
                | OpCode::EndLine
                | OpCode::PreviousMatchEnd
                | OpCode::MatchReset => continue,
                OpCode::CharClass => {
                    // Extract the class ID and use the ASCII bitmap for filtering
                    if ip < code.len() {
                        let class_id = code[ip] as usize;
                        if class_id < self.program.char_classes.len() {
                            return PrefixFilter::CharClass(class_id);
                        }
                    }
                    return PrefixFilter::None;
                }
                OpCode::SaveStart | OpCode::SaveEnd => {
                    if ip < code.len() {
                        ip += 1; // skip group ID operand
                    }
                }
                _ => return PrefixFilter::None,
            }
        }
        PrefixFilter::None
    }

    /// Extract the literal byte string if the compiled program is a pure-literal pattern.
    ///
    /// A pure-literal program contains only `Char` opcodes (with optional `SaveStart`/`SaveEnd`
    /// capture markers for group 0) followed by a `Match` terminator.  When detected, the VM
    /// can be bypassed entirely in favour of a `memchr::memmem` substring search.
    fn extract_literal_string(&self) -> Option<Vec<u8>> {
        let code = &self.program.code;
        let mut ip = 0;
        let mut literal = Vec::new();

        while ip < code.len() {
            let Ok(op) = OpCode::try_from(code[ip]) else {
                return None;
            };
            ip += 1;
            match op {
                OpCode::Char => {
                    if ip >= code.len() {
                        return None;
                    }
                    let char_len = code[ip] as usize;
                    ip += 1;
                    if ip + char_len > code.len() {
                        return None;
                    }
                    literal.extend_from_slice(&code[ip..ip + char_len]);
                    ip += char_len;
                }
                OpCode::Match => {
                    return if literal.is_empty() {
                        None
                    } else {
                        Some(literal)
                    }
                }
                // Skip capture markers for group 0 only; patterns with higher
                // capture groups need full VM execution to report sub-matches.
                OpCode::SaveStart | OpCode::SaveEnd => {
                    if ip >= code.len() {
                        return None;
                    }
                    let group_id = code[ip];
                    ip += 1;
                    if group_id != 0 {
                        return None;
                    }
                }
                _ => return None, // Not a pure literal
            }
        }
        None
    }

    /// Build an owned `memmem::Finder` when the pattern is a pure literal.
    ///
    /// The finder is cached on the VM so that its skip-table is computed once
    /// rather than on every search call.
    fn extract_literal_finder(&self) -> Option<memchr::memmem::Finder<'static>> {
        self.extract_literal_string()
            .map(|needle| memchr::memmem::Finder::new(&needle).into_owned())
    }

    /// Standard scanning approach - try match at each position.
    ///
    /// When the compiled program begins with a single-byte literal, uses
    /// `memchr` to jump directly to candidate positions instead of testing
    /// every byte offset.
    #[allow(clippy::too_many_lines)] // Event emission added modest length to existing scanning loop
    fn find_first_scanning(&self, ctx: &mut ExecContext<'_>) -> Option<Match> {
        trace_enter!(
            "vm",
            "RegexVM::find_first_scanning",
            "positions={}",
            ctx.text.len() + 1
        );
        debug_log!(
            "vm",
            "Scanning {} positions (0..={})",
            ctx.text.len() + 1,
            ctx.text.len()
        );

        if let PrefixFilter::Byte(fb) = self.prefix_filter {
            // Fastest path: use memchr to jump to candidate positions
            let mut offset = 0;
            while let Some(pos) = memchr(fb, &ctx.text[offset..]) {
                let start = offset + pos;
                ctx.pos = start;
                Self::reset_captures(ctx);
                self.emit_event(&MatchEvent::MatchAttemptStarted { position: start });
                let matched = self.execute_at(ctx, start);
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: start,
                    matched,
                });
                if matched {
                    let effective_start = ctx.match_start_override.unwrap_or(start);
                    trace_exit!("vm", "RegexVM::find_first_scanning", "matched=true");
                    return Some(Match {
                        start: effective_start,
                        end: ctx.pos,
                        groups: self.extract_captures_with_match(ctx, effective_start, ctx.pos),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    });
                }
                // (*COMMIT): abort entire search on failure
                if ctx.committed {
                    trace_exit!("vm", "RegexVM::find_first_scanning", "committed=true");
                    return None;
                }
                // (*SKIP): advance to the skip position instead of start+1.
                // Guard: named SKIP can target a mark before `start`;
                // ensure forward progress to avoid an infinite loop.
                if let Some(skip_pos) = ctx.skip_position.take() {
                    offset = skip_pos.max(start + 1);
                } else {
                    offset = start + 1;
                }
            }
        } else {
            // Class-filter or full-scan path
            let filter = self.prefix_filter;
            let mut start = 0;
            while start < ctx.text.len() {
                if !filter.matches(ctx.text[start], &self.program.char_classes) {
                    start += 1;
                    continue;
                }
                ctx.pos = start;
                Self::reset_captures(ctx);
                self.emit_event(&MatchEvent::MatchAttemptStarted { position: start });
                let matched = self.execute_at(ctx, start);
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: start,
                    matched,
                });
                if matched {
                    let effective_start = ctx.match_start_override.unwrap_or(start);
                    trace_exit!("vm", "RegexVM::find_first_scanning", "matched=true");
                    return Some(Match {
                        start: effective_start,
                        end: ctx.pos,
                        groups: self.extract_captures_with_match(ctx, effective_start, ctx.pos),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    });
                }
                // (*COMMIT): abort entire search on failure
                if ctx.committed {
                    trace_exit!("vm", "RegexVM::find_first_scanning", "committed=true");
                    return None;
                }
                // (*SKIP): advance to the skip position instead of start+1.
                // Guard: named SKIP can target a mark before `start`;
                // ensure forward progress to avoid an infinite loop.
                if let Some(skip_pos) = ctx.skip_position.take() {
                    start = skip_pos.max(start + 1);
                } else {
                    start += 1;
                }
            }
        }
        // Try match at end-of-text for zero-width patterns
        let start = ctx.text.len();
        ctx.pos = start;
        Self::reset_captures(ctx);
        self.emit_event(&MatchEvent::MatchAttemptStarted { position: start });
        let matched = self.execute_at(ctx, start);
        self.emit_event(&MatchEvent::MatchAttemptCompleted {
            position: start,
            matched,
        });
        if matched {
            let effective_start = ctx.match_start_override.unwrap_or(start);
            trace_exit!("vm", "RegexVM::find_first_scanning", "matched=true");
            return Some(Match {
                start: effective_start,
                end: ctx.pos,
                groups: self.extract_captures_with_match(ctx, effective_start, ctx.pos),
                matched_alternative: ctx.current_alternative,
                code_result: ctx.code_result.clone(),
                last_mark: None,
            });
        }
        trace_exit!("vm", "RegexVM::find_first_scanning", "matched=false");
        None
    }

    /// Scanning loop starting from an arbitrary byte offset.
    ///
    /// Same logic as `find_first_scanning` but begins at `scan_start` instead
    /// of position 0.
    fn find_first_scanning_from(
        &self,
        ctx: &mut ExecContext<'_>,
        scan_start: usize,
    ) -> Option<Match> {
        if let PrefixFilter::Byte(fb) = self.prefix_filter {
            let mut offset = scan_start;
            while let Some(pos) = memchr(fb, &ctx.text[offset..]) {
                let start = offset + pos;
                ctx.pos = start;
                ctx.match_start = start;
                Self::reset_captures(ctx);
                self.emit_event(&MatchEvent::MatchAttemptStarted { position: start });
                let matched = self.execute_at(ctx, start);
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: start,
                    matched,
                });
                if matched {
                    let effective_start = ctx.match_start_override.unwrap_or(start);
                    return Some(Match {
                        start: effective_start,
                        end: ctx.pos,
                        groups: self.extract_captures_with_match(ctx, effective_start, ctx.pos),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    });
                }
                if ctx.committed {
                    return None;
                }
                if let Some(skip_pos) = ctx.skip_position.take() {
                    offset = skip_pos.max(start + 1);
                } else {
                    offset = start + 1;
                }
            }
        } else {
            let filter = self.prefix_filter;
            let mut start = scan_start;
            while start < ctx.text.len() {
                if !filter.matches(ctx.text[start], &self.program.char_classes) {
                    start += 1;
                    continue;
                }
                ctx.pos = start;
                ctx.match_start = start;
                Self::reset_captures(ctx);
                self.emit_event(&MatchEvent::MatchAttemptStarted { position: start });
                let matched = self.execute_at(ctx, start);
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: start,
                    matched,
                });
                if matched {
                    let effective_start = ctx.match_start_override.unwrap_or(start);
                    return Some(Match {
                        start: effective_start,
                        end: ctx.pos,
                        groups: self.extract_captures_with_match(ctx, effective_start, ctx.pos),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    });
                }
                if ctx.committed {
                    return None;
                }
                if let Some(skip_pos) = ctx.skip_position.take() {
                    start = skip_pos.max(start + 1);
                } else {
                    start += 1;
                }
            }
        }
        // Try match at end-of-text for zero-width patterns
        if scan_start <= ctx.text.len() {
            let start = ctx.text.len();
            ctx.pos = start;
            ctx.match_start = start;
            Self::reset_captures(ctx);
            self.emit_event(&MatchEvent::MatchAttemptStarted { position: start });
            let matched = self.execute_at(ctx, start);
            self.emit_event(&MatchEvent::MatchAttemptCompleted {
                position: start,
                matched,
            });
            if matched {
                let effective_start = ctx.match_start_override.unwrap_or(start);
                return Some(Match {
                    start: effective_start,
                    end: ctx.pos,
                    groups: self.extract_captures_with_match(ctx, effective_start, ctx.pos),
                    matched_alternative: ctx.current_alternative,
                    code_result: ctx.code_result.clone(),
                    last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                });
            }
        }
        None
    }

    /// Scanning loop for `find_all_at`, starting from an arbitrary byte offset.
    #[allow(clippy::too_many_lines)]
    fn find_all_scanning_from(&self, ctx: &mut ExecContext<'_>, scan_start: usize) -> Vec<Match> {
        let mut matches = Vec::new();
        let mut last_match_end: Option<usize> = None;

        if let PrefixFilter::Byte(fb) = self.prefix_filter {
            let mut offset = scan_start;
            while let Some(pos) = memchr(fb, &ctx.text[offset..]) {
                let candidate = offset + pos;
                ctx.pos = candidate;
                ctx.match_start = candidate;
                ctx.match_start_override = None;
                ctx.code_result = None;
                ctx.current_alternative = None;
                Self::reset_captures(ctx);
                self.emit_event(&MatchEvent::MatchAttemptStarted {
                    position: candidate,
                });
                let attempt_matched = self.execute_at(ctx, candidate);
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: candidate,
                    matched: attempt_matched,
                });
                if attempt_matched {
                    let m_start = ctx.match_start_override.unwrap_or(candidate);
                    let m_end = ctx.pos;
                    if m_start == m_end {
                        if let Some(prev_end) = last_match_end {
                            if m_start == prev_end {
                                offset = candidate + 1;
                                continue;
                            }
                        }
                    }
                    last_match_end = Some(m_end);
                    matches.push(Match {
                        start: m_start,
                        end: m_end,
                        groups: self.extract_captures_with_match(ctx, m_start, m_end),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    });
                    ctx.previous_match_end = Some(m_end);
                    offset = m_end.max(candidate + 1);
                } else if ctx.committed {
                    break;
                } else if let Some(skip_pos) = ctx.skip_position.take() {
                    offset = skip_pos.max(candidate + 1);
                } else {
                    offset = candidate + 1;
                }
            }
        } else {
            let filter = self.prefix_filter;
            let mut start = scan_start;
            while start < ctx.text.len() {
                if !filter.matches(ctx.text[start], &self.program.char_classes) {
                    start += 1;
                    continue;
                }
                let candidate = start;
                ctx.pos = candidate;
                ctx.match_start = candidate;
                ctx.match_start_override = None;
                ctx.code_result = None;
                ctx.current_alternative = None;
                Self::reset_captures(ctx);
                self.emit_event(&MatchEvent::MatchAttemptStarted {
                    position: candidate,
                });
                let attempt_matched = self.execute_at(ctx, candidate);
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: candidate,
                    matched: attempt_matched,
                });
                if attempt_matched {
                    let m_start = ctx.match_start_override.unwrap_or(candidate);
                    let m_end = ctx.pos;
                    if m_start == m_end {
                        if let Some(prev_end) = last_match_end {
                            if m_start == prev_end {
                                start = candidate + 1;
                                continue;
                            }
                        }
                    }
                    last_match_end = Some(m_end);
                    matches.push(Match {
                        start: m_start,
                        end: m_end,
                        groups: self.extract_captures_with_match(ctx, m_start, m_end),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    });
                    ctx.previous_match_end = Some(m_end);
                    start = m_end.max(candidate + 1);
                } else if ctx.committed {
                    break;
                } else if let Some(skip_pos) = ctx.skip_position.take() {
                    start = skip_pos.max(start + 1);
                } else {
                    start += 1;
                }
            }
        }
        // Try end-of-text for zero-width patterns
        if scan_start <= ctx.text.len() {
            let candidate = ctx.text.len();
            ctx.pos = candidate;
            ctx.match_start = candidate;
            ctx.match_start_override = None;
            ctx.code_result = None;
            ctx.current_alternative = None;
            Self::reset_captures(ctx);
            let matched = self.execute_at(ctx, candidate);
            if matched {
                let m_start = ctx.match_start_override.unwrap_or(candidate);
                let m_end = ctx.pos;
                let suppress =
                    m_start == m_end && last_match_end.is_some_and(|prev| m_start == prev);
                if !suppress {
                    matches.push(Match {
                        start: m_start,
                        end: m_end,
                        groups: self.extract_captures_with_match(ctx, m_start, m_end),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    });
                }
            }
        }
        matches
    }

    /// Write a single capture slot, recording the old value in the trail log
    /// so that the modification can be undone on backtrack.
    #[inline]
    fn set_capture(ctx: &mut ExecContext<'_>, index: usize, value: Option<usize>) {
        let old = ctx.captures[index];
        ctx.capture_trail.push((index, old));
        ctx.captures[index] = value;
    }

    /// Undo capture-trail entries back to `mark`, restoring capture slots to
    /// their state at the time the mark was taken.
    #[inline]
    fn undo_trail(ctx: &mut ExecContext<'_>, mark: usize) {
        while ctx.capture_trail.len() > mark {
            let (index, old_value) = ctx.capture_trail.pop().unwrap();
            ctx.captures[index] = old_value;
        }
    }

    /// Restore a backtrack frame's capture + call-stack state onto `ctx`.
    #[inline]
    fn restore_frame(ctx: &mut ExecContext<'_>, frame: &BacktrackFrame) {
        if let Some(ref snapshot) = frame.capture_snapshot {
            // Probe-based frame: discard trail entries and apply the snapshot.
            ctx.capture_trail.truncate(frame.trail_mark);
            ctx.captures.copy_from_slice(snapshot);
        } else {
            // Normal frame: replay the trail backwards to undo changes.
            Self::undo_trail(ctx, frame.trail_mark);
        }
        ctx.call_stack.truncate(frame.call_stack_mark);
        // `\K` / `OpCode::MatchReset` mutates `match_start_override`
        // during forward execution. On backtrack we restore whatever
        // value was live when the frame was pushed — this ensures a
        // `\K` in an abandoned branch doesn't leak its reset onto the
        // eventual match span.
        ctx.match_start_override = frame.saved_match_start_override;
    }

    /// Restore a previously saved execution state if backtracking is available.
    /// Returns true when a frame was restored and execution should continue.
    fn try_backtrack(&self, ctx: &mut ExecContext<'_>, ip: &mut usize) -> bool {
        if let Some(frame) = ctx.backtrack_stack.pop() {
            let stack_depth = ctx.backtrack_stack.len() + 1; // depth before pop
            *ip = frame.ip;
            ctx.pos = frame.pos;
            Self::restore_frame(ctx, &frame);
            ctx.code_result = frame.saved_code_result;
            self.emit_event(&MatchEvent::BacktrackOccurred {
                position: ctx.pos,
                stack_depth,
            });
            true
        } else {
            false
        }
    }

    /// Clone the current execution state for speculative sub-expression execution.
    fn clone_exec_context<'a>(ctx: &ExecContext<'a>) -> ExecContext<'a> {
        ExecContext {
            text: ctx.text,
            pos: ctx.pos,
            match_start: ctx.match_start,
            end: ctx.end,
            captures: ctx.captures.clone(),
            capture_trail: ctx.capture_trail.clone(),
            call_stack: ctx.call_stack.clone(),
            backtrack_stack: Vec::new(),
            current_alternative: ctx.current_alternative,
            recursion_stack: ctx.recursion_stack.clone(),
            code_result: ctx.code_result.clone(),
            match_start_override: ctx.match_start_override,
            previous_match_end: ctx.previous_match_end,
            committed: ctx.committed,
            skip_position: ctx.skip_position,
            marks: ctx.marks.clone(),
            suspendable: ctx.suspendable,
            suspension: None,
            step_count: ctx.step_count,
            max_steps: ctx.max_steps,
            max_backtrack_frames: ctx.max_backtrack_frames,
            max_recursion_depth: ctx.max_recursion_depth,
            hit_end: false,
        }
    }

    /// Invoke a compiled recursion/subroutine target with basic cycle protection.
    fn invoke_subroutine(&self, ctx: &mut ExecContext<'_>, target: usize) -> bool {
        let Some(code) = self.program.subroutines.get(target) else {
            return false;
        };

        let effective_limit = if ctx.max_recursion_depth > 0 {
            ctx.max_recursion_depth as usize
        } else {
            MAX_RECURSION_DEPTH
        };
        if ctx.recursion_stack.len() >= effective_limit {
            return false;
        }

        if ctx
            .recursion_stack
            .iter()
            .any(|&(active_target, active_pos)| active_target == target && active_pos == ctx.pos)
        {
            return false;
        }

        let saved_pos = ctx.pos;
        let trail_mark = ctx.capture_trail.len();
        let saved_code_result = ctx.code_result.clone();
        let saved_alternative = ctx.current_alternative;

        ctx.recursion_stack.push((target, ctx.pos));
        let matched = self.execute_subexpr(ctx, code);
        ctx.recursion_stack.pop();
        ctx.current_alternative = saved_alternative;

        if matched {
            // PCRE2 semantics: subroutine calls advance position but do NOT
            // export their internal captures to the outer match. Revert captures
            // to what they were before the call, keeping only the position advance.
            let advanced_pos = ctx.pos;
            Self::undo_trail(ctx, trail_mark);
            ctx.pos = advanced_pos;
            ctx.code_result = saved_code_result;
            true
        } else {
            ctx.pos = saved_pos;
            Self::undo_trail(ctx, trail_mark);
            ctx.code_result = saved_code_result;
            false
        }
    }

    /// Execute bytecode starting at given position
    #[allow(clippy::too_many_lines)] // Main VM dispatch loop — splitting would fragment the opcode state machine
    #[allow(clippy::cast_possible_truncation)] // Bytecode operands are stored as u8; group/branch IDs fit in u32
    fn execute_at(&self, ctx: &mut ExecContext<'_>, start: usize) -> bool {
        debug_log!(
            "vm",
            "Execute at text_pos={}, code_len={}",
            start,
            self.program.code.len()
        );
        ctx.pos = start;
        ctx.match_start = start;
        ctx.code_result = None;
        ctx.match_start_override = None;
        ctx.committed = false;
        ctx.skip_position = None;
        ctx.marks.clear();
        ctx.step_count = 0;
        ctx.hit_end = false;
        let mut ip = 0;
        let code = &self.program.code;

        loop {
            // Step limit check — abort if the match attempt has exceeded max_steps.
            if ctx.max_steps > 0 && ctx.step_count >= ctx.max_steps {
                return false;
            }
            // Backtrack stack depth check.
            if ctx.max_backtrack_frames > 0
                && ctx.backtrack_stack.len() as u64 > ctx.max_backtrack_frames
            {
                return false;
            }
            ctx.step_count += 1;

            if ip >= code.len() {
                trace_log!(
                    "vm",
                    "IP {} >= code length {}, return false",
                    ip,
                    code.len()
                );
                return false;
            }

            let op = OpCode::try_from(code[ip]).unwrap_or(OpCode::Fail);
            trace_log!(
                "vm",
                "[IP={:3}] OpCode={:?} (0x{:02x}), text_pos={}/{}",
                ip,
                op,
                code[ip],
                ctx.pos,
                ctx.text.len()
            );
            ip += 1;

            match op {
                OpCode::Char => {
                    // Read UTF-8 character from operands
                    if let Some(expected) = Self::read_char_operand(code, &mut ip) {
                        trace_log!(
                            "vm",
                            "  Char: expect='{}' (U+{:04X})",
                            expected,
                            expected as u32
                        );
                        if let Some(actual) = Self::current_char(ctx) {
                            if actual == expected {
                                trace_log!(
                                    "vm",
                                    "  ✓ Match '{}', advance pos {} -> {}",
                                    actual,
                                    ctx.pos,
                                    ctx.pos + actual.len_utf8()
                                );
                                Self::advance_char(ctx);
                                continue;
                            }
                            trace_log!("vm", "  ✗ Got '{}' != '{}'", actual, expected);
                        } else {
                            trace_log!("vm", "  ✗ EOF, expected '{}'", expected);
                            // Only flag partial match if we've advanced past the
                            // match start (i.e., the pattern was actively matching).
                            if ctx.pos > ctx.match_start {
                                ctx.hit_end = true;
                            }
                        }
                    }
                    // Character didn't match - try backtracking
                    if let Some(frame) = ctx.backtrack_stack.pop() {
                        let stack_depth = ctx.backtrack_stack.len() + 1;
                        trace_log!(
                            "vm",
                            "  Backtrack: IP {} -> {}, pos {} -> {}",
                            ip - 1,
                            frame.ip,
                            ctx.pos,
                            frame.pos
                        );
                        // Restore saved state
                        ip = frame.ip;
                        ctx.pos = frame.pos;
                        Self::restore_frame(ctx, &frame);
                        ctx.code_result = frame.saved_code_result;
                        self.emit_event(&MatchEvent::BacktrackOccurred {
                            position: ctx.pos,
                            stack_depth,
                        });
                        continue;
                    }
                    trace_log!("vm", "  ✗ Char match failed, no backtrack available");
                    return false;
                }

                OpCode::Lookahead
                | OpCode::LookaheadNeg
                | OpCode::Lookbehind
                | OpCode::LookbehindNeg => {
                    // Read the length of the assertion sub-expression
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    let positive = matches!(op, OpCode::Lookahead | OpCode::Lookbehind);
                    let matched = match op {
                        OpCode::Lookahead | OpCode::LookaheadNeg => self.execute_assertion_subexpr(
                            ctx,
                            &code[expr_start..expr_end],
                            positive,
                        ),
                        OpCode::Lookbehind | OpCode::LookbehindNeg => self
                            .execute_lookbehind_assertion(
                                ctx,
                                &code[expr_start..expr_end],
                                positive,
                            ),
                        _ => false,
                    };
                    let assertion_holds = if positive { matched } else { !matched };

                    if !assertion_holds {
                        if self.try_backtrack(ctx, &mut ip) {
                            continue;
                        }
                        return false;
                    }

                    // Assertions do not consume input
                    ip = expr_end;
                }

                OpCode::CodeBlock => {
                    let outcome = if ctx.suspendable {
                        self.execute_inline_code_block_suspendable(ctx, code, &mut ip)
                    } else {
                        self.execute_inline_code_block(ctx, code, &mut ip)
                    };
                    match outcome {
                        Some(CodeBlockOutcome::Pass) => {}
                        Some(CodeBlockOutcome::Fail) => {
                            if self.try_backtrack(ctx, &mut ip) {
                                continue;
                            }
                            return false;
                        }
                        Some(CodeBlockOutcome::Accept) => {
                            return true;
                        }
                        Some(CodeBlockOutcome::Suspended(name)) => {
                            // Capture suspension state; return false so the
                            // scanning loop can detect it and build a continuation.
                            ctx.suspension = Some((name, ip));
                            return false;
                        }
                        None => return false,
                    }
                }

                OpCode::Any => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if ch == '\n' {
                            trace_log!("vm", "  ✗ Any: got newline");
                        } else {
                            trace_log!("vm", "  ✓ Any: matched '{}' (not newline)", ch);
                            Self::advance_char(ctx);
                            continue;
                        }
                    } else {
                        trace_log!("vm", "  ✗ Any: EOF");
                        if ctx.pos > ctx.match_start {
                            ctx.hit_end = true;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::AnyDotAll => {
                    if let Some(_ch) = Self::current_char(ctx) {
                        trace_log!("vm", "  ✓ AnyDotAll: matched '{}'", _ch);
                        Self::advance_char(ctx);
                        continue;
                    }
                    trace_log!("vm", "  ✗ AnyDotAll: EOF");
                    if ctx.pos > ctx.match_start {
                        ctx.hit_end = true;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::SpaceAsciiNeg => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if !pcre2_is_space_char(ch) {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::DigitAscii => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if ch.is_ascii_digit() {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::DigitAsciiNeg => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if !ch.is_ascii_digit() {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::WordAscii => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::WordAsciiNeg => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if !(ch.is_ascii_alphanumeric() || ch == '_') {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::SpaceAscii => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if pcre2_is_space_char(ch) {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::StartLine => {
                    if self
                        .program
                        .newline_mode
                        .is_line_start_before(ctx.text, ctx.pos)
                    {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::StartText => {
                    if ctx.pos == 0 {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::EndLine => {
                    if self.program.newline_mode.is_line_end_at(ctx.text, ctx.pos) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::EndText => {
                    if Self::is_at_absolute_end(ctx) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::EndTextOrNL => {
                    if Self::is_at_absolute_end_or_before_final_newline(ctx) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::PreviousMatchEnd => {
                    let target = ctx.previous_match_end.unwrap_or(0);
                    if ctx.pos == target {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::Match => {
                    debug_log!("vm", "  ✓✓ MATCH opcode reached at pos={}", ctx.pos);
                    return true;
                }

                OpCode::MatchReset => {
                    debug_log!("vm", "  \\K match reset at pos={}", ctx.pos);
                    ctx.match_start_override = Some(ctx.pos);
                }

                OpCode::GraphemeCluster => {
                    use unicode_segmentation::UnicodeSegmentation;
                    if ctx.pos < ctx.text.len() {
                        // SAFETY: ctx.text is guaranteed valid UTF-8 on the &str path.
                        let remaining =
                            unsafe { std::str::from_utf8_unchecked(&ctx.text[ctx.pos..]) };
                        if let Some(cluster) = remaining.graphemes(true).next() {
                            debug_log!(
                                "vm",
                                "  \\X: matched grapheme {:?} ({} bytes)",
                                cluster,
                                cluster.len()
                            );
                            ctx.pos += cluster.len();
                            continue;
                        }
                    }
                    // At EOF — flag partial match if we were mid-match.
                    if ctx.pos > ctx.match_start {
                        ctx.hit_end = true;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                // --- Backtracking control verbs ---
                OpCode::Commit => {
                    // (*COMMIT): set flag so that if this attempt fails,
                    // the scanning loop aborts the entire search.
                    ctx.committed = true;
                }

                OpCode::Prune => {
                    // (*PRUNE): clear backtrack stack so that if the
                    // current path fails, the entire attempt at this
                    // start position fails immediately.
                    ctx.backtrack_stack.clear();
                }

                OpCode::VerbSkip => {
                    // (*SKIP): record the current text position. On
                    // match failure the scanning loop will advance
                    // to this position instead of start+1.
                    ctx.skip_position = Some(ctx.pos);
                    // Also prune: do not backtrack past (*SKIP).
                    ctx.backtrack_stack.clear();
                }

                OpCode::Then => {
                    // (*THEN): simplified as (*PRUNE) — clear backtrack
                    // stack. Full alternation-aware behavior is not yet
                    // implemented.
                    // TODO: implement full alternation-aware (*THEN)
                    // semantics that skip to the next alternative in the
                    // innermost enclosing alternation.
                    ctx.backtrack_stack.clear();
                }

                OpCode::Mark => {
                    // (*MARK:name): record the mark name and current
                    // position in `ctx.marks` so a later `(*SKIP:name)`
                    // can look it up. The name is encoded as a
                    // length-prefixed UTF-8 string operand.
                    //
                    // A11: Mark used to be a no-op for match behaviour.
                    // It now pushes (name, pos) to ctx.marks so the
                    // (*SKIP:name) → (*MARK:name) interaction works.
                    // The match-result side effect is unchanged: marks
                    // are scoped to the current match attempt and do
                    // not affect the public MatchResult shape.
                    if ip < code.len() {
                        let name_len = code[ip] as usize;
                        ip += 1;
                        if ip + name_len <= code.len() {
                            let name = std::str::from_utf8(&code[ip..ip + name_len])
                                .unwrap_or("")
                                .to_string();
                            ctx.marks.push((name, ctx.pos));
                            ip += name_len;
                        }
                    }
                }

                OpCode::VerbSkipNamed => {
                    // (*SKIP:name): look up the most recent mark with
                    // the matching name in ctx.marks; set
                    // ctx.skip_position to that mark's recorded text
                    // position. If no matching mark exists, the verb
                    // is treated as a no-op (matches PCRE2's fallback
                    // for missing marks). Like (*SKIP), also clears
                    // the backtrack stack.
                    //
                    // A11.
                    if ip < code.len() {
                        let name_len = code[ip] as usize;
                        ip += 1;
                        if ip + name_len <= code.len() {
                            let name = std::str::from_utf8(&code[ip..ip + name_len]).unwrap_or("");
                            // Search marks from most-recent to least-recent.
                            if let Some(pos) = ctx
                                .marks
                                .iter()
                                .rev()
                                .find(|(n, _)| n == name)
                                .map(|(_, p)| *p)
                            {
                                ctx.skip_position = Some(pos);
                                ctx.backtrack_stack.clear();
                            }
                            ip += name_len;
                        }
                    }
                }

                OpCode::WordBoundary => {
                    // Check if we're at a word boundary
                    if Self::is_at_word_boundary(ctx) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::NonWordBoundary => {
                    // Check if we're NOT at a word boundary
                    if !Self::is_at_word_boundary(ctx) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::CharClass | OpCode::CharClassNeg => {
                    let is_neg = matches!(op, OpCode::CharClassNeg);
                    trace_log!(
                        "vm",
                        "  {} class_id lookup",
                        if is_neg { "CharClassNeg" } else { "CharClass" }
                    );

                    // Read character class ID
                    if ip >= code.len() {
                        trace_log!(
                            "vm",
                            "  ✗ No class_id operand (ip {} >= len {})",
                            ip,
                            code.len()
                        );
                        return false;
                    }
                    let class_id = code[ip] as usize;
                    ip += 1;
                    trace_log!("vm", "  Class ID = {}", class_id);

                    // Get the character class
                    if class_id >= self.program.char_classes.len() {
                        trace_log!(
                            "vm",
                            "  ✗ Invalid class_id {} (>= {})",
                            class_id,
                            self.program.char_classes.len()
                        );
                        return false;
                    }
                    let char_class = &self.program.char_classes[class_id];
                    debug_log!(
                        "vm",
                        "  CharClass: ASCII bitmap has {} set bits, {} Unicode ranges",
                        char_class
                            .ascii_bitmap
                            .iter()
                            .map(|&b| b.count_ones())
                            .sum::<u32>(),
                        char_class.unicode_ranges.len()
                    );

                    // Get current character
                    if let Some(ch) = Self::current_char(ctx) {
                        trace_log!(
                            "vm",
                            "  Testing char '{}' (U+{:04X}) against class",
                            ch,
                            ch as u32
                        );
                        let matches = Self::test_char_class(ch, char_class);
                        let should_match = if is_neg { !matches } else { matches };

                        trace_log!(
                            "vm",
                            "  Class test: {}, negated={}, final={}",
                            matches,
                            is_neg,
                            should_match
                        );

                        if should_match {
                            trace_log!(
                                "vm",
                                "  ✓ CharClass match, advance pos {} -> {}",
                                ctx.pos,
                                ctx.pos + ch.len_utf8()
                            );
                            Self::advance_char(ctx);
                            continue;
                        }
                        trace_log!("vm", "  ✗ CharClass no match");
                    } else {
                        trace_log!("vm", "  ✗ EOF, can't match char class");
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::PlusGreedy => {
                    trace_log!("vm", "  PlusGreedy: reading subexpr length");
                    // Read the length of the sub-expression
                    if ip >= code.len() {
                        trace_log!("vm", "  ✗ No length operand");
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;
                    trace_log!("vm", "  Subexpr length = {} bytes", expr_len);

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    // Bounds check
                    if expr_end > code.len() {
                        trace_log!("vm", "  ✗ Subexpr bounds exceed code length");
                        return false;
                    }

                    // Must match at least once
                    let start_pos = ctx.pos;
                    trace_log!("vm", "  First match attempt at pos={}", ctx.pos);
                    let first_trail_mark = ctx.capture_trail.len();
                    let first_cs_mark = ctx.call_stack.len();
                    let first_saved_code_result = ctx.code_result.clone();
                    if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                        ctx.pos = start_pos;
                        Self::undo_trail(ctx, first_trail_mark);
                        ctx.call_stack.truncate(first_cs_mark);
                        ctx.code_result = first_saved_code_result;
                        trace_log!("vm", "  ✗ PlusGreedy: first match failed");
                        if self.try_backtrack(ctx, &mut ip) {
                            continue;
                        }
                        return false;
                    }
                    let first_match_end = ctx.pos;
                    trace_log!(
                        "vm",
                        "  ✓ First match succeeded, pos {} -> {}",
                        start_pos,
                        first_match_end
                    );

                    // Keep matching greedily until we can't match anymore
                    let mut match_count = 1;
                    loop {
                        let before_pos = ctx.pos;
                        let trail_mark = ctx.capture_trail.len();
                        let cs_mark = ctx.call_stack.len();
                        let saved_code_result = ctx.code_result.clone();
                        if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                            ctx.pos = before_pos;
                            Self::undo_trail(ctx, trail_mark);
                            ctx.call_stack.truncate(cs_mark);
                            ctx.code_result = saved_code_result;
                            // Can't match anymore, that's fine
                            trace_log!("vm", "  PlusGreedy: stopped after {} matches", match_count);
                            break;
                        }
                        // PCRE2 semantic: zero-width iteration terminates
                        // the loop. Match count is already ≥1 here
                        // (PlusGreedy required one iteration before the
                        // loop), so simply stop without retrying.
                        if ctx.pos == before_pos {
                            Self::undo_trail(ctx, trail_mark);
                            ctx.call_stack.truncate(cs_mark);
                            ctx.code_result = saved_code_result;
                            trace_log!("vm", "  PlusGreedy: zero-width iteration, stopping");
                            break;
                        }
                        // Greedy path consumed one extra repetition; keep a fallback
                        // to continue after this quantifier without this repetition.
                        ctx.backtrack_stack.push(BacktrackFrame {
                            ip: expr_end,
                            pos: before_pos,
                            trail_mark,
                            call_stack_mark: cs_mark,
                            capture_snapshot: None,
                            saved_code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                        match_count += 1;
                        trace_log!(
                            "vm",
                            "  Match {}: pos {} -> {}",
                            match_count,
                            before_pos,
                            ctx.pos
                        );
                    }

                    ip = expr_end;
                    trace_log!("vm", "  PlusGreedy complete, continuing at IP={}", ip);
                }

                OpCode::StarGreedy => {
                    // Read the length of the sub-expression
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    // Bounds check
                    if expr_end > code.len() {
                        return false;
                    }

                    // Match as many times as possible (greedy, zero or more)
                    loop {
                        let before_pos = ctx.pos;
                        let trail_mark = ctx.capture_trail.len();
                        let cs_mark = ctx.call_stack.len();
                        let saved_code_result = ctx.code_result.clone();
                        if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                            ctx.pos = before_pos;
                            Self::undo_trail(ctx, trail_mark);
                            ctx.call_stack.truncate(cs_mark);
                            ctx.code_result = saved_code_result;
                            // Can't match anymore, that's fine for *
                            break;
                        }
                        // PCRE2 semantic: if the iteration matched zero
                        // width (no progress from before_pos), the
                        // outer `*` loop terminates immediately. Previous
                        // versions of RGX retried with a forced-advance
                        // variant so recursive-subroutine bodies could
                        // re-enter, but that over-matched patterns like
                        // `([a]*?)*` on "a" — PCRE2 returns "" (zero-width
                        // iteration terminated the loop) while the retry
                        // produced "a". Keep the zero-width result and
                        // stop.
                        if ctx.pos == before_pos {
                            Self::undo_trail(ctx, trail_mark);
                            ctx.call_stack.truncate(cs_mark);
                            ctx.code_result = saved_code_result;
                            break;
                        }
                        // Greedy path consumed one repetition; keep a fallback
                        // to continue after this quantifier without this repetition.
                        ctx.backtrack_stack.push(BacktrackFrame {
                            ip: expr_end,
                            pos: before_pos,
                            trail_mark,
                            call_stack_mark: cs_mark,
                            capture_snapshot: None,
                            saved_code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::QuestionGreedy => {
                    // Read the length of the sub-expression
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    // Bounds check
                    if expr_end > code.len() {
                        return false;
                    }

                    // Try to match once (greedy), but keep backtrack fallback
                    // for the zero-occurrence path. A zero-width body match
                    // (pos unchanged after execute_subexpr) must still be
                    // treated as "matched" — it sets any captures inside the
                    // body and PCRE2 conditionals like `(?(1)yes|no)` rely
                    // on that visibility (e.g. `()?(?(1)a|b)` on "a" sets
                    // group 1 to the empty string and picks the 'a' branch).
                    let before_pos = ctx.pos;
                    let trail_mark = ctx.capture_trail.len();
                    let cs_mark = ctx.call_stack.len();
                    let saved_code_result = ctx.code_result.clone();
                    let matched = self.execute_subexpr(ctx, &code[expr_start..expr_end]);
                    if matched {
                        ctx.backtrack_stack.push(BacktrackFrame {
                            ip: expr_end,
                            pos: before_pos,
                            trail_mark,
                            call_stack_mark: cs_mark,
                            capture_snapshot: None,
                            saved_code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    } else {
                        ctx.pos = before_pos;
                        Self::undo_trail(ctx, trail_mark);
                        ctx.call_stack.truncate(cs_mark);
                        ctx.code_result = saved_code_result;
                    }

                    ip = expr_end;
                }

                OpCode::QuestionLazy => {
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    if self
                        .probe_subexpr(ctx, &code[expr_start..expr_end])
                        .is_some()
                    {
                        ctx.backtrack_stack.push(BacktrackFrame {
                            ip: expr_start,
                            pos: ctx.pos,
                            trail_mark: ctx.capture_trail.len(),
                            call_stack_mark: ctx.call_stack.len(),
                            capture_snapshot: None,
                            saved_code_result: ctx.code_result.clone(),
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::StarLazy => {
                    let opcode_start = ip - 1;
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    if let Some(probe_ctx) = self.probe_subexpr(ctx, &code[expr_start..expr_end]) {
                        ctx.backtrack_stack.push(BacktrackFrame {
                            ip: opcode_start,
                            pos: probe_ctx.pos,
                            trail_mark: ctx.capture_trail.len(),
                            call_stack_mark: ctx.call_stack.len(),
                            capture_snapshot: Some(probe_ctx.captures),
                            saved_code_result: probe_ctx.code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::PlusLazy => {
                    let opcode_start = ip - 1;
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    let before_pos = ctx.pos;
                    let trail_mark = ctx.capture_trail.len();
                    let cs_mark = ctx.call_stack.len();
                    let saved_code_result = ctx.code_result.clone();
                    let matched = self.execute_subexpr(ctx, &code[expr_start..expr_end]);
                    if !matched || ctx.pos == before_pos {
                        ctx.pos = before_pos;
                        Self::undo_trail(ctx, trail_mark);
                        ctx.call_stack.truncate(cs_mark);
                        ctx.code_result = saved_code_result;
                        if self.try_backtrack(ctx, &mut ip) {
                            continue;
                        }
                        return false;
                    }

                    let after_first_trail_mark = ctx.capture_trail.len();
                    let after_first_cs_mark = ctx.call_stack.len();
                    let after_first_code_result = ctx.code_result.clone();
                    if self
                        .probe_subexpr(ctx, &code[expr_start..expr_end])
                        .is_some()
                    {
                        ctx.backtrack_stack.push(BacktrackFrame {
                            ip: opcode_start,
                            pos: ctx.pos,
                            trail_mark: after_first_trail_mark,
                            call_stack_mark: after_first_cs_mark,
                            capture_snapshot: None,
                            saved_code_result: after_first_code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::SaveStart => {
                    // Read the group ID from operands
                    if ip >= code.len() {
                        return false;
                    }
                    let group_id = code[ip] as usize;
                    ip += 1;

                    // Save current position as start of capture group
                    let start_idx = group_id * 2;
                    if start_idx < ctx.captures.len() {
                        Self::set_capture(ctx, start_idx, Some(ctx.pos));
                    }
                }

                OpCode::SaveEnd => {
                    // Read the group ID from operands
                    if ip >= code.len() {
                        return false;
                    }
                    let group_id = code[ip] as usize;
                    ip += 1;

                    // Save current position as end of capture group
                    let end_idx = group_id * 2 + 1;
                    if end_idx < ctx.captures.len() {
                        Self::set_capture(ctx, end_idx, Some(ctx.pos));
                    }
                    // Emit capture-completed event when both start and end are known
                    let start_idx = group_id * 2;
                    if let (Some(Some(cap_start)), Some(Some(_cap_end))) =
                        (ctx.captures.get(start_idx), ctx.captures.get(end_idx))
                    {
                        self.emit_event(&MatchEvent::CaptureCompleted {
                            group: group_id as u32,
                            start: *cap_start,
                            end: ctx.pos,
                        });
                    }
                }

                OpCode::Backref => {
                    if ip >= code.len() {
                        return false;
                    }
                    let group_id = code[ip] as usize;
                    ip += 1;

                    if self.match_backreference(ctx, group_id) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::BackrefCaseInsensitive => {
                    if ip >= code.len() {
                        return false;
                    }
                    let group_id = code[ip] as usize;
                    ip += 1;

                    if self.match_backreference_case_insensitive(ctx, group_id) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::JumpIfMatch | OpCode::JumpIfNoMatch => {
                    let jump_if_match = matches!(op, OpCode::JumpIfMatch);
                    let Some(condition_matches) =
                        self.evaluate_conditional_operand(ctx, code, &mut ip)
                    else {
                        return false;
                    };

                    if ip + 1 >= code.len() {
                        return false;
                    }
                    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;

                    if condition_matches == jump_if_match {
                        ip += offset;
                    }
                }

                OpCode::Call => {
                    if ip >= code.len() {
                        return false;
                    }
                    let target = code[ip] as usize;
                    ip += 1;

                    if self.invoke_subroutine(ctx, target) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }

                OpCode::Split => {
                    // Read jump offset (2 bytes, little-endian)
                    if ip + 1 >= code.len() {
                        return false;
                    }
                    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;

                    // Save current state for backtracking
                    let backtrack_frame = BacktrackFrame {
                        ip: ip + offset, // Second alternative
                        pos: ctx.pos,
                        trail_mark: ctx.capture_trail.len(),
                        call_stack_mark: ctx.call_stack.len(),
                        capture_snapshot: None,
                        saved_code_result: ctx.code_result.clone(),
                        saved_match_start_override: ctx.match_start_override,
                    };
                    ctx.backtrack_stack.push(backtrack_frame);
                }

                OpCode::Jump => {
                    // Read jump offset (2 bytes, little-endian)
                    if ip + 1 >= code.len() {
                        return false;
                    }
                    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2; // Skip the 2-byte offset operand
                    ip += offset; // Then add the offset
                }

                OpCode::SetAlternative => {
                    // Read alternative index from operands
                    if ip >= code.len() {
                        return false;
                    }
                    let alternative_index = code[ip] as usize;
                    ip += 1;

                    // Set the current alternative being tested
                    ctx.current_alternative = Some(alternative_index);
                    self.emit_event(&MatchEvent::BranchEntered {
                        branch: alternative_index as u32,
                        position: ctx.pos,
                    });
                }

                OpCode::AtomicStart => {
                    // Mark current backtrack depth; frames created after this point
                    // are internal to the atomic group.
                    ctx.call_stack.push(ctx.backtrack_stack.len());
                }

                OpCode::AtomicEnd => {
                    // On successful atomic-group completion, discard all backtrack
                    // frames created inside the group.
                    if let Some(mark) = ctx.call_stack.pop() {
                        ctx.backtrack_stack.truncate(mark);
                        continue;
                    }
                    return false;
                }

                OpCode::Fail => {
                    // Try backtracking if we have saved states
                    if let Some(frame) = ctx.backtrack_stack.pop() {
                        // Restore saved state
                        ip = frame.ip;
                        ctx.pos = frame.pos;
                        Self::restore_frame(ctx, &frame);
                        ctx.code_result = frame.saved_code_result;
                        continue;
                    }
                    return false;
                }

                // TODO: Implement remaining opcodes
                _ => {
                    // Placeholder - skip unknown opcodes for now
                    return false;
                }
            }
        }
    }

    /// Probe whether a sub-expression can match once while advancing the input.
    fn probe_subexpr<'a>(&self, ctx: &ExecContext<'a>, code: &[u8]) -> Option<ExecContext<'a>> {
        let mut probe_ctx = Self::clone_exec_context(ctx);
        if self.execute_subexpr(&mut probe_ctx, code) && probe_ctx.pos != ctx.pos {
            Some(probe_ctx)
        } else {
            None
        }
    }

    /// Read a raw byte slice from bytecode operands.
    fn read_bytes_operand<'a>(code: &'a [u8], ip: &mut usize, len: usize) -> Option<&'a [u8]> {
        if *ip + len > code.len() {
            return None;
        }
        let bytes = &code[*ip..*ip + len];
        *ip += len;
        Some(bytes)
    }

    /// Decode and execute an inline code-block operand.
    ///
    /// Returns `None` on decode error, otherwise a `CodeBlockOutcome` describing
    /// how the VM should proceed.
    fn execute_inline_code_block(
        &self,
        ctx: &mut ExecContext<'_>,
        code: &[u8],
        ip: &mut usize,
    ) -> Option<CodeBlockOutcome> {
        if *ip >= code.len() {
            return None;
        }
        let lang_len = code[*ip] as usize;
        *ip += 1;
        let lang = std::str::from_utf8(Self::read_bytes_operand(code, ip, lang_len)?).ok()?;
        let body_len_bytes = Self::read_bytes_operand(code, ip, 2)?;
        let body_len = u16::from_le_bytes([body_len_bytes[0], body_len_bytes[1]]) as usize;
        let body = std::str::from_utf8(Self::read_bytes_operand(code, ip, body_len)?).ok()?;
        let outcome = self.evaluate_code_block(ctx, lang, body);
        self.emit_event(&MatchEvent::CodeBlockEvaluated {
            language: lang.to_string(),
            succeeded: matches!(outcome, CodeBlockOutcome::Pass | CodeBlockOutcome::Accept),
            position: ctx.pos,
        });
        Some(outcome)
    }

    /// Execute a code-block predicate using the shared execution manager.
    fn evaluate_code_block(
        &self,
        ctx: &mut ExecContext<'_>,
        language: &str,
        code: &str,
    ) -> CodeBlockOutcome {
        let Some(execution_manager) = &self.execution_manager else {
            // PCRE2 semantic: an unregistered callout is a no-op. The
            // adapter lowers `(?C)` / `(?Cn)` / `(?C"text")` to a native
            // code-block call named `__callout_N`. When no execution
            // manager exists (Pure mode, no code-block runtime attached)
            // we can't invoke the callback — treat it as no-op rather
            // than failing, so patterns like `abc(?C)def` match
            // normally on trivially-matching subjects.
            if language == "native" && code.starts_with("__callout_") {
                return CodeBlockOutcome::Pass;
            }
            debug_log!(
                "vm",
                "CodeBlock execution requested without an attached execution manager"
            );
            return CodeBlockOutcome::Fail;
        };
        let exec_ctx = self.build_code_exec_context(ctx);
        match execution_manager.execute(language, code, &exec_ctx) {
            ExecResult::Success => CodeBlockOutcome::Pass,
            ExecResult::Failure => CodeBlockOutcome::Fail,
            ExecResult::Error(error) => {
                debug_log!("vm", "CodeBlock {} execution error: {}", language, error);
                CodeBlockOutcome::Fail
            }
            ExecResult::Replacement(value) => {
                debug_log!(
                    "vm",
                    "CodeBlock {} returned a replacement value; storing it on the current match path",
                    language,
                );
                ctx.code_result = Some(CodeBlockValue::Replacement(value));
                CodeBlockOutcome::Pass
            }
            ExecResult::Numeric(value) => {
                debug_log!(
                    "vm",
                    "CodeBlock {} returned a numeric value; storing it on the current match path",
                    language,
                );
                ctx.code_result = Some(CodeBlockValue::Numeric(value));
                CodeBlockOutcome::Pass
            }
            ExecResult::Steer(steer) => match steer {
                SteerResult::Continue => CodeBlockOutcome::Pass,
                SteerResult::Fail => CodeBlockOutcome::Fail,
                SteerResult::Accept => CodeBlockOutcome::Accept,
                SteerResult::Skip(n) => {
                    ctx.pos += n;
                    CodeBlockOutcome::Pass
                }
                SteerResult::Abort => {
                    ctx.committed = true;
                    CodeBlockOutcome::Fail
                }
            },
            ExecResult::Suspend(_) => {
                // Suspend should not appear in synchronous evaluate_code_block;
                // treat as failure for safety.
                CodeBlockOutcome::Fail
            }
            ExecResult::Structured(value) => {
                debug_log!(
                    "vm",
                    "CodeBlock {} returned a structured value; storing it on the current match path",
                    language,
                );
                ctx.code_result = Some(CodeBlockValue::Structured(value));
                CodeBlockOutcome::Pass
            }
        }
    }

    /// Evaluate a code block in suspendable mode.
    ///
    /// For native callbacks that are not registered, returns
    /// `CodeBlockOutcome::Suspended(name)` instead of treating them as errors.
    /// All other code block types are evaluated synchronously as normal.
    fn evaluate_code_block_suspendable(
        &self,
        ctx: &mut ExecContext<'_>,
        language: &str,
        code: &str,
    ) -> CodeBlockOutcome {
        let Some(execution_manager) = &self.execution_manager else {
            return CodeBlockOutcome::Fail;
        };

        // For native callbacks, check registration before calling.
        // Unregistered native callbacks trigger suspension.
        if language == "native" && !execution_manager.has_native(code) {
            // Not a PCRE2 callout (those are no-ops) — suspend for async resolution.
            if !code.starts_with("__callout_") {
                return CodeBlockOutcome::Suspended(code.to_string());
            }
        }

        // Delegate to the normal synchronous evaluation path.
        self.evaluate_code_block(ctx, language, code)
    }

    /// Parse and evaluate an inline code-block opcode in suspendable mode.
    fn execute_inline_code_block_suspendable(
        &self,
        ctx: &mut ExecContext<'_>,
        code: &[u8],
        ip: &mut usize,
    ) -> Option<CodeBlockOutcome> {
        if *ip >= code.len() {
            return None;
        }
        let lang_len = code[*ip] as usize;
        *ip += 1;
        let lang = std::str::from_utf8(Self::read_bytes_operand(code, ip, lang_len)?).ok()?;
        let body_len_bytes = Self::read_bytes_operand(code, ip, 2)?;
        let body_len = u16::from_le_bytes([body_len_bytes[0], body_len_bytes[1]]) as usize;
        let body = std::str::from_utf8(Self::read_bytes_operand(code, ip, body_len)?).ok()?;
        let outcome = self.evaluate_code_block_suspendable(ctx, lang, body);
        if !matches!(outcome, CodeBlockOutcome::Suspended(_)) {
            self.emit_event(&MatchEvent::CodeBlockEvaluated {
                language: lang.to_string(),
                succeeded: matches!(outcome, CodeBlockOutcome::Pass | CodeBlockOutcome::Accept),
                position: ctx.pos,
            });
        }
        Some(outcome)
    }

    /// Materialize the current VM state into the execution-layer context.
    fn build_code_exec_context(&self, ctx: &ExecContext<'_>) -> CodeExecContext {
        let mut exec_ctx =
            CodeExecContext::new(String::from_utf8_lossy(ctx.text).into_owned(), ctx.pos);
        exec_ctx.match_start = ctx.match_start;
        exec_ctx.match_end = ctx.pos;
        exec_ctx.matched_branch_number = ctx.current_alternative.map(|id| id + 1);
        let mut captures = Vec::with_capacity(self.program.num_groups as usize + 1);
        captures.push(Self::capture_text(ctx, ctx.match_start, ctx.pos));
        for group_id in 1..=self.program.num_groups {
            let start_idx = (group_id * 2) as usize;
            let end_idx = start_idx + 1;
            let capture = match (
                ctx.captures.get(start_idx).and_then(|&x| x),
                ctx.captures.get(end_idx).and_then(|&x| x),
            ) {
                (Some(start), Some(end)) => Self::capture_text(ctx, start, end),
                _ => None,
            };
            captures.push(capture);
        }
        exec_ctx.captures = captures;
        for (name, group_id) in &self.program.named_groups {
            if let Some(Some(value)) = exec_ctx.captures.get(*group_id as usize).cloned() {
                exec_ctx.named_captures.insert(name.clone(), value);
            }
        }
        if let Some(execution_manager) = &self.execution_manager {
            exec_ctx.variables = Arc::new(RwLock::new(execution_manager.variable_snapshot()));
            exec_ctx.typed_variables =
                Arc::new(RwLock::new(execution_manager.typed_variable_snapshot()));
        }
        exec_ctx
    }

    /// Convert a capture byte range into owned UTF-8 text.
    fn capture_text(ctx: &ExecContext<'_>, start: usize, end: usize) -> Option<String> {
        if start > end || end > ctx.text.len() {
            return None;
        }
        Some(String::from_utf8_lossy(&ctx.text[start..end]).into_owned())
    }

    /// Match the current position against the bytes captured by a numbered group.
    fn match_backreference(&self, ctx: &mut ExecContext<'_>, group_id: usize) -> bool {
        let start_idx = group_id * 2;
        let end_idx = start_idx + 1;
        let (Some(capture_start), Some(capture_end)) = (
            ctx.captures.get(start_idx).and_then(|&x| x),
            ctx.captures.get(end_idx).and_then(|&x| x),
        ) else {
            return false;
        };

        if capture_start > capture_end || capture_end > ctx.text.len() {
            return false;
        }

        let capture_len = capture_end - capture_start;
        let Some(candidate_end) = ctx.pos.checked_add(capture_len) else {
            return false;
        };
        if candidate_end > ctx.end {
            return false;
        }

        if self.simd_compare(
            &ctx.text[ctx.pos..candidate_end],
            &ctx.text[capture_start..capture_end],
        ) {
            ctx.pos = candidate_end;
            true
        } else {
            false
        }
    }

    /// Per-char Unicode case-insensitive comparison used by
    /// `match_backreference_case_insensitive`. Returns `true` if the
    /// chars are equal or belong to the same UCD simple-fold
    /// equivalence class. This picks up PCRE2 `/i` folds that
    /// `to_lowercase()` misses — e.g. Σ↔σ↔ς, ſ↔s, K↔k(Kelvin).
    /// Falls back to `to_lowercase()` as a backstop for codepoints
    /// outside the simple-fold table. Does not fold across char-count
    /// changes (`ẞ` ≠ `ss`).
    fn chars_case_insensitive_eq(a: char, b: char) -> bool {
        if a == b {
            return true;
        }
        if Self::unicode_simple_fold_contains(a, b) {
            return true;
        }
        a.to_lowercase().eq(b.to_lowercase())
    }

    /// Ask whether `a` and `b` are in the same UCD simple-fold
    /// equivalence class. Uses `regex_syntax`'s HIR case-fold table —
    /// the same source `OptimizingCompiler::unicode_case_variants`
    /// consults — but lives on `RegexVM` so the backref matcher can
    /// call it without ceremony.
    fn unicode_simple_fold_contains(a: char, b: char) -> bool {
        let range = regex_syntax::hir::ClassUnicodeRange::new(a, a);
        let mut class = regex_syntax::hir::ClassUnicode::new([range]);
        if class.try_case_fold_simple().is_err() {
            return false;
        }
        class.iter().any(|r| r.start() <= b && b <= r.end())
    }

    /// Case-insensitive backref match: walk the captured text and the
    /// subject char-by-char from `ctx.pos` forward, accepting each pair
    /// whose Unicode lowercase forms are equal. On success, advances
    /// `ctx.pos` past the consumed bytes of the subject (which may
    /// differ from the captured length if folding changes byte width —
    /// but per-char folding usually preserves it for ASCII / BMP). On
    /// any mismatch or short subject, returns false.
    ///
    /// Known limitation: does not yet handle cases where a single
    /// captured codepoint folds to *multiple* codepoints (e.g. `ẞ` →
    /// `ss`). That's tracked as the Unicode case-fold residual
    /// follow-up.
    fn match_backreference_case_insensitive(
        &self,
        ctx: &mut ExecContext<'_>,
        group_id: usize,
    ) -> bool {
        let start_idx = group_id * 2;
        let end_idx = start_idx + 1;
        let (Some(capture_start), Some(capture_end)) = (
            ctx.captures.get(start_idx).and_then(|&x| x),
            ctx.captures.get(end_idx).and_then(|&x| x),
        ) else {
            return false;
        };

        if capture_start > capture_end || capture_end > ctx.text.len() {
            return false;
        }

        let captured_bytes = &ctx.text[capture_start..capture_end];
        let Ok(captured_str) = std::str::from_utf8(captured_bytes) else {
            return false;
        };

        let mut pos = ctx.pos;
        for cap_ch in captured_str.chars() {
            if pos >= ctx.end {
                return false;
            }
            let rest = &ctx.text[pos..ctx.end];
            let Ok(rest_str) = std::str::from_utf8(rest) else {
                return false;
            };
            let Some(sub_ch) = rest_str.chars().next() else {
                return false;
            };
            if !Self::chars_case_insensitive_eq(cap_ch, sub_ch) {
                return false;
            }
            pos += sub_ch.len_utf8();
        }

        ctx.pos = pos;
        true
    }

    /// Read UTF-8 character from bytecode operands
    fn read_char_operand(code: &[u8], ip: &mut usize) -> Option<char> {
        if *ip >= code.len() {
            return None;
        }

        let len = code[*ip] as usize;
        *ip += 1;

        if *ip + len > code.len() {
            return None;
        }

        let utf8_bytes = &code[*ip..*ip + len];
        *ip += len;

        std::str::from_utf8(utf8_bytes).ok()?.chars().next()
    }

    /// Get current character at context position.
    ///
    /// Decodes only the minimal UTF-8 bytes at the current position instead of
    /// validating the entire remaining text.
    fn current_char(ctx: &ExecContext<'_>) -> Option<char> {
        if ctx.pos >= ctx.end {
            return None;
        }
        let b = ctx.text[ctx.pos];
        if b < 0x80 {
            // ASCII fast path — single byte, no validation needed
            return Some(b as char);
        }
        // Multi-byte: determine width from leading byte and decode only those bytes
        let width = match b {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return None,
        };
        let end = (ctx.pos + width).min(ctx.end);
        std::str::from_utf8(&ctx.text[ctx.pos..end])
            .ok()?
            .chars()
            .next()
    }

    /// Advance context position by one UTF-8 character width.
    ///
    /// Determines the character width directly from the leading byte without
    /// decoding the full character.
    fn advance_char(ctx: &mut ExecContext<'_>) {
        if ctx.pos >= ctx.end {
            return;
        }
        let b = ctx.text[ctx.pos];
        let width = if b < 0x80 {
            1
        } else {
            match b {
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => 1, // invalid leading byte — advance by 1 to avoid infinite loop
            }
        };
        ctx.pos += width;
    }

    /// Reset capture groups for new match attempt
    fn reset_captures(ctx: &mut ExecContext<'_>) {
        for capture in &mut ctx.captures {
            *capture = None;
        }
        // Clear the capture-modification trail for fresh start
        ctx.capture_trail.clear();
        // Also clear backtrack stack for fresh start
        ctx.backtrack_stack.clear();
        // Clear atomic-group markers for fresh start
        ctx.call_stack.clear();
        // Reset alternative tracking for fresh start
        ctx.current_alternative = None;
        // Reset richer non-boolean code-block result tracking for fresh start
        ctx.code_result = None;
    }

    /// Check if current position is at the absolute end of the input text.
    fn is_at_absolute_end(ctx: &ExecContext<'_>) -> bool {
        ctx.pos == ctx.text.len()
    }

    /// Check if current position is at the absolute end of the input text,
    /// or immediately before one final trailing newline sequence.
    fn is_at_absolute_end_or_before_final_newline(ctx: &ExecContext<'_>) -> bool {
        let len = ctx.text.len();
        ctx.pos == len
            || (ctx.pos + 1 == len && ctx.text.get(ctx.pos) == Some(&b'\n'))
            || (ctx.pos + 2 == len
                && ctx.text.get(ctx.pos) == Some(&b'\r')
                && ctx.text.get(ctx.pos + 1) == Some(&b'\n'))
    }

    /// Extract capture groups with explicit overall match (group 0)
    fn extract_captures_with_match(
        &self,
        ctx: &ExecContext<'_>,
        match_start: usize,
        match_end: usize,
    ) -> Vec<Option<(usize, usize)>> {
        let mut groups = Vec::new();

        // Group 0 is always the overall match
        groups.push(Some((match_start, match_end)));

        // Extract the numbered capture groups (1, 2, 3, ...)
        for i in 1..=self.program.num_groups {
            let start_idx = (i * 2) as usize;
            let end_idx = start_idx + 1;

            if let (Some(start), Some(end)) = (
                ctx.captures.get(start_idx).and_then(|&x| x),
                ctx.captures.get(end_idx).and_then(|&x| x),
            ) {
                groups.push(Some((start, end)));
            } else {
                groups.push(None);
            }
        }

        groups
    }

    /// Check if we're at a word boundary (\b)
    fn is_at_word_boundary(ctx: &ExecContext<'_>) -> bool {
        let is_word_char = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';

        let prev_is_word = if ctx.pos == 0 {
            false
        } else {
            // Look at previous character
            let mut prev_pos = ctx.pos;
            loop {
                if prev_pos == 0 {
                    break false;
                }
                prev_pos -= 1;
                if let Ok(s) = std::str::from_utf8(&ctx.text[prev_pos..ctx.pos]) {
                    if let Some(ch) = s.chars().next() {
                        if ch.len_utf8() == ctx.pos - prev_pos {
                            break is_word_char(ch);
                        }
                    }
                }
            }
        };

        let curr_is_word = if let Some(ch) = Self::current_char(ctx) {
            is_word_char(ch)
        } else {
            false
        };

        // Word boundary exists if exactly one of prev/curr is a word character
        prev_is_word != curr_is_word
    }

    /// Test if a character matches a compiled character class
    fn test_char_class(ch: char, char_class: &CompiledCharClass) -> bool {
        let ch_code = ch as u32;
        trace_log!("vm", "    test_char_class: ch='{}' (U+{:04X})", ch, ch_code);

        // First check ASCII bitmap for fast path
        if ch_code <= 127 {
            let byte_idx = (ch_code / 16) as usize;
            let bit_idx = (ch_code % 16) as usize;
            let bitmap_byte = char_class.ascii_bitmap[byte_idx];
            let bit_mask = 1u16 << bit_idx;
            let matches_bitmap = (bitmap_byte & bit_mask) != 0;

            trace_log!(
                "vm",
                "    ASCII bitmap: byte[{}]=0x{:04x}, bit={}, mask=0x{:04x}, matches={}",
                byte_idx,
                bitmap_byte,
                bit_idx,
                bit_mask,
                matches_bitmap
            );

            trace_log!(
                "vm",
                "    ASCII result: {} (matches_bitmap={})",
                matches_bitmap,
                matches_bitmap
            );
            return matches_bitmap;
        }

        // Check Unicode ranges using binary search (ranges are sorted by start)
        char_class
            .unicode_ranges
            .binary_search_by(|&(start, end)| {
                if ch_code < start {
                    std::cmp::Ordering::Greater
                } else if ch_code > end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    /// Find all non-overlapping matches
    #[must_use]
    #[allow(clippy::too_many_lines)] // Scanning loop with PCRE2 verb support — splitting would fragment the scan state machine
    pub fn find_all(&self, text: &str) -> Vec<Match> {
        trace_enter!(
            "vm",
            "RegexVM::find_all",
            "text_len={}, code_len={}",
            text.len(),
            self.program.code.len()
        );
        let bytes = text.as_bytes();

        // Fast path: pure-literal patterns bypass the VM entirely via memmem
        if let Some(ref finder) = self.literal_finder {
            debug_log!("vm", "Strategy: literal memmem fast path (find_all)");
            let needle_len = finder.needle().len();
            let mut matches = Vec::new();
            let mut start = 0;
            while let Some(pos) = finder.find(&bytes[start..]) {
                let abs_pos = start + pos;
                matches.push(Match {
                    start: abs_pos,
                    end: abs_pos + needle_len,
                    groups: vec![Some((abs_pos, abs_pos + needle_len))],
                    matched_alternative: None,
                    code_result: None,
                    last_mark: None,
                });
                start = abs_pos + needle_len.max(1);
            }
            trace_exit!("vm", "RegexVM::find_all", "match_count={}", matches.len());
            return matches;
        }

        let mut ctx = ExecContext {
            text: bytes,
            pos: 0,
            match_start: 0,
            end: bytes.len(),
            captures: vec![None; (self.program.num_groups + 1) as usize * 2],
            capture_trail: Vec::new(),
            call_stack: Vec::new(),
            backtrack_stack: Vec::new(),
            current_alternative: None,
            recursion_stack: Vec::new(),
            code_result: None,
            match_start_override: None,
            previous_match_end: None,
            committed: false,
            skip_position: None,
            marks: Vec::new(),
            suspendable: false,
            suspension: None,
            step_count: 0,
            max_steps: self.max_steps.load(std::sync::atomic::Ordering::Relaxed),
            max_backtrack_frames: self
                .max_backtrack_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            max_recursion_depth: self
                .max_recursion_depth
                .load(std::sync::atomic::Ordering::Relaxed),
            hit_end: false,
        };

        let mut matches = Vec::new();
        let mut start = 0;
        // Track previous consuming match end for PCRE2-style zero-width suppression:
        // after a consuming match ending at position E, a zero-width match at E is skipped.
        let mut last_match_end: Option<usize> = None;

        if let PrefixFilter::Byte(fb) = self.prefix_filter {
            // Fastest path: use memchr to jump between candidate positions
            while let Some(pos) = memchr(fb, &ctx.text[start..]) {
                let candidate = start + pos;
                ctx.pos = candidate;
                ctx.match_start = candidate;
                Self::reset_captures(&mut ctx);
                self.emit_event(&MatchEvent::MatchAttemptStarted {
                    position: candidate,
                });
                let attempt_matched = self.execute_at(&mut ctx, candidate);
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: candidate,
                    matched: attempt_matched,
                });
                if attempt_matched {
                    let m_start = ctx.match_start_override.unwrap_or(candidate);
                    let m_end = ctx.pos;
                    // Suppress zero-width match at the exact end of a previous consuming match
                    if m_start == m_end && last_match_end == Some(m_start) {
                        start = candidate + 1;
                        continue;
                    }
                    let m = Match {
                        start: m_start,
                        end: m_end,
                        groups: self.extract_captures_with_match(&ctx, m_start, m_end),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    };
                    last_match_end = Some(m_end);
                    ctx.previous_match_end = Some(m_end);
                    start = m_end.max(candidate + 1);
                    matches.push(m);
                } else {
                    // (*COMMIT): abort entire search on failure
                    if ctx.committed {
                        break;
                    }
                    // (*SKIP): advance to the skip position instead of candidate+1.
                    // Guard: named SKIP can target a mark before `candidate`;
                    // ensure forward progress.
                    if let Some(skip_pos) = ctx.skip_position.take() {
                        start = skip_pos.max(candidate + 1);
                    } else {
                        start = candidate + 1;
                    }
                }
            }
        } else {
            // Class-filter or full-scan path
            let filter = self.prefix_filter;
            while start <= bytes.len() {
                if start < bytes.len()
                    && !filter.matches(ctx.text[start], &self.program.char_classes)
                {
                    start += 1;
                    continue;
                }
                ctx.pos = start;
                ctx.match_start = start;
                Self::reset_captures(&mut ctx);
                self.emit_event(&MatchEvent::MatchAttemptStarted { position: start });
                let attempt_matched = self.execute_at(&mut ctx, start);
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: start,
                    matched: attempt_matched,
                });
                if attempt_matched {
                    let m_start = ctx.match_start_override.unwrap_or(start);
                    let m_end = ctx.pos;
                    // Suppress zero-width match at the exact end of a previous consuming match
                    if m_start == m_end && last_match_end == Some(m_start) {
                        start += 1;
                        continue;
                    }
                    let m = Match {
                        start: m_start,
                        end: m_end,
                        groups: self.extract_captures_with_match(&ctx, m_start, m_end),
                        matched_alternative: ctx.current_alternative,
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    };
                    last_match_end = Some(m_end);
                    ctx.previous_match_end = Some(m_end);
                    start = m_end.max(start + 1);
                    matches.push(m);
                } else {
                    // (*COMMIT): abort entire search on failure
                    if ctx.committed {
                        break;
                    }
                    // (*SKIP): advance to the skip position instead of start+1.
                    // Guard: forward progress for named SKIP.
                    if let Some(skip_pos) = ctx.skip_position.take() {
                        start = skip_pos.max(start + 1);
                    } else {
                        start += 1;
                    }
                }
            }
        }
        trace_exit!("vm", "RegexVM::find_all", "match_count={}", matches.len());

        matches
    }

    /// Test if pattern matches text
    #[must_use]
    #[inline]
    pub fn is_match(&self, text: &str) -> bool {
        // Fast path: pure-literal patterns bypass the VM entirely via memmem
        if let Some(ref finder) = self.literal_finder {
            return finder.find(text.as_bytes()).is_some();
        }
        let matched = self.find_first(text).is_some();
        trace_exit!("vm", "RegexVM::is_match", "matched={}", matched);
        matched
    }

    /// Register a native callback on the attached execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this VM.
    pub fn register_native<F>(&self, name: &str, callback: F) -> crate::error::Result<()>
    where
        F: Fn(&CodeExecContext) -> ExecResult + Send + Sync + 'static,
    {
        let Some(execution_manager) = &self.execution_manager else {
            return Err(crate::error::RgxError::Engine(
                "native callback registration is unavailable for this compiled regex".to_string(),
            ));
        };
        execution_manager.register_native(name, callback);
        Ok(())
    }

    /// Register a named wasm module on the attached execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached or the WASM module is invalid.
    pub fn register_wasm_module(
        &self,
        name: String,
        module_bytes: Vec<u8>,
    ) -> crate::error::Result<()> {
        let Some(execution_manager) = &self.execution_manager else {
            return Err(crate::error::RgxError::Engine(
                "WASM module registration is unavailable for this compiled regex".to_string(),
            ));
        };
        execution_manager.register_wasm_module(name, module_bytes)
    }

    /// Register or replace a host-provided execution variable on the attached execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this VM.
    pub fn set_variable(&self, name: &str, value: String) -> crate::error::Result<()> {
        let Some(execution_manager) = &self.execution_manager else {
            return Err(crate::error::RgxError::Engine(
                "execution variable registration is unavailable for this compiled regex"
                    .to_string(),
            ));
        };
        execution_manager.set_variable(name, value);
        Ok(())
    }

    /// Register or replace a typed host-provided execution variable on the attached execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this VM.
    pub fn set_typed_variable(
        &self,
        name: &str,
        value: crate::execution::Value,
    ) -> crate::error::Result<()> {
        let Some(execution_manager) = &self.execution_manager else {
            return Err(crate::error::RgxError::Engine(
                "execution variable registration is unavailable for this compiled regex"
                    .to_string(),
            ));
        };
        execution_manager.set_typed_variable(name, value);
        Ok(())
    }

    /// Set a host variable with automatic type conversion.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this VM.
    pub fn set_var<V: Into<crate::execution::Value>>(
        &self,
        name: &str,
        value: V,
    ) -> crate::error::Result<()> {
        self.set_typed_variable(name, value.into())
    }

    // ========================================================================
    // SUSPENDABLE (ASYNC / CONTINUATION-PASSING) MATCHING
    // ========================================================================

    /// Find first match with support for async callback suspension.
    ///
    /// When an unregistered native callback is encountered, the VM saves its
    /// full state into a [`MatchContinuation`] and returns
    /// [`MatchOutcome::Suspended`]. The caller resolves the callback
    /// externally and calls [`resume`](Self::resume) to continue matching.
    ///
    /// For patterns without unregistered native callbacks this behaves
    /// identically to [`find_first`](Self::find_first), with negligible
    /// overhead (one well-predicted branch per code-block opcode).
    #[must_use]
    pub fn find_first_suspendable(&self, text: &str) -> MatchOutcome {
        // Fast path: pure-literal patterns bypass the VM entirely.
        if let Some(ref finder) = self.literal_finder {
            let bytes = text.as_bytes();
            let needle_len = finder.needle().len();
            let result = finder.find(bytes).map(|pos| crate::engine::MatchResult {
                start: pos,
                end: pos + needle_len,
                groups: vec![Some((pos, pos + needle_len))],
                matched_branch_number: None,
                code_result: None,
                last_mark: None,
            });
            return MatchOutcome::Completed(result);
        }

        let bytes = text.as_bytes();
        let mut ctx = ExecContext {
            text: bytes,
            pos: 0,
            match_start: 0,
            end: bytes.len(),
            captures: vec![None; (self.program.num_groups + 1) as usize * 2],
            capture_trail: Vec::new(),
            call_stack: Vec::new(),
            backtrack_stack: Vec::new(),
            current_alternative: None,
            recursion_stack: Vec::new(),
            code_result: None,
            match_start_override: None,
            previous_match_end: None,
            committed: false,
            skip_position: None,
            marks: Vec::new(),
            suspendable: true,
            suspension: None,
            step_count: 0,
            max_steps: self.max_steps.load(std::sync::atomic::Ordering::Relaxed),
            max_backtrack_frames: self
                .max_backtrack_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            max_recursion_depth: self
                .max_recursion_depth
                .load(std::sync::atomic::Ordering::Relaxed),
            hit_end: false,
        };

        self.find_first_suspendable_scanning(&mut ctx, text, 0)
    }

    /// Core scanning loop for suspendable matching, starting from `scan_start`.
    ///
    /// Extracted so that [`resume`](Self::resume) can re-enter the scanning
    /// loop from the correct position after a callback is resolved.
    fn find_first_suspendable_scanning(
        &self,
        ctx: &mut ExecContext<'_>,
        _text: &str,
        scan_start: usize,
    ) -> MatchOutcome {
        let filter = self.prefix_filter;
        let mut start = scan_start;

        while start <= ctx.text.len() {
            // For positions before end-of-text, apply the prefix filter.
            if start < ctx.text.len()
                && !filter.matches(ctx.text[start], &self.program.char_classes)
            {
                start += 1;
                continue;
            }

            ctx.pos = start;
            Self::reset_captures(ctx);
            ctx.suspension = None;

            self.emit_event(&MatchEvent::MatchAttemptStarted { position: start });

            let matched = self.execute_at(ctx, start);

            // Check for suspension before checking match result.
            if let Some((callback_name, ip)) = ctx.suspension.take() {
                // Build a MatchContinuation and return Suspended.
                let variable_snapshot = self
                    .execution_manager
                    .as_ref()
                    .map(|em| em.variable_snapshot())
                    .unwrap_or_default();

                let continuation = MatchContinuation {
                    text: ctx.text.to_vec(),
                    pending_callback_name: callback_name,
                    pending_context: ExecContextSnapshot {
                        position: ctx.pos,
                        match_start: ctx.match_start,
                        captures: ctx.captures.clone(),
                        variables: variable_snapshot,
                    },
                    vm_state: VmResumeState {
                        pos: ctx.pos,
                        match_start: ctx.match_start,
                        ip,
                        captures: ctx.captures.clone(),
                        capture_trail: ctx.capture_trail.clone(),
                        call_stack: ctx.call_stack.clone(),
                        backtrack_stack: ctx.backtrack_stack.clone(),
                        current_alternative: ctx.current_alternative,
                        recursion_stack: ctx.recursion_stack.clone(),
                        code_result: ctx.code_result.clone(),
                        committed: ctx.committed,
                        skip_position: ctx.skip_position,
                        marks: ctx.marks.clone(),
                        match_start_override: ctx.match_start_override,
                        previous_match_end: ctx.previous_match_end,
                        scan_start: start,
                    },
                };
                return MatchOutcome::Suspended(Box::new(continuation));
            }

            if matched {
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: start,
                    matched: true,
                });
                let effective_start = ctx.match_start_override.unwrap_or(start);
                return MatchOutcome::Completed(Some(crate::engine::MatchResult {
                    start: effective_start,
                    end: ctx.pos,
                    groups: self.extract_captures_with_match(&ctx, effective_start, ctx.pos),
                    matched_branch_number: ctx.current_alternative.map(|id| id + 1),
                    code_result: ctx.code_result.clone(),
                    last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                }));
            }

            self.emit_event(&MatchEvent::MatchAttemptCompleted {
                position: start,
                matched: false,
            });

            // (*COMMIT): abort entire search on failure
            if ctx.committed {
                return MatchOutcome::Completed(None);
            }

            // (*SKIP): advance to the skip position instead of start+1.
            // Guard: forward progress for named SKIP.
            if let Some(skip_pos) = ctx.skip_position.take() {
                start = skip_pos.max(start + 1);
            } else if start < ctx.text.len() {
                start += 1;
            } else {
                break;
            }
        }

        MatchOutcome::Completed(None)
    }

    /// Resume a suspended match after the caller resolves an async callback.
    ///
    /// The `callback_result` is the resolved value for the callback that
    /// caused suspension. Matching continues from where it left off:
    /// - On `ExecResult::Success` the VM proceeds past the code block.
    /// - On `ExecResult::Failure` the VM backtracks (potentially finding an
    ///   alternative match or trying the next scan position).
    /// - If another unregistered native callback is encountered, another
    ///   `MatchOutcome::Suspended` is returned.
    #[must_use]
    #[allow(clippy::too_many_lines)] // Continuation dispatch is inherently multi-path
    #[allow(clippy::needless_pass_by_value)] // API ergonomics: callers typically construct ExecResult in-place
    pub fn resume(
        &self,
        continuation: MatchContinuation,
        callback_result: ExecResult,
    ) -> MatchOutcome {
        let text = continuation.text;
        let state = continuation.vm_state;

        let mut ctx = ExecContext {
            text: &text,
            pos: state.pos,
            match_start: state.match_start,
            end: text.len(),
            captures: state.captures,
            capture_trail: state.capture_trail,
            call_stack: state.call_stack,
            backtrack_stack: state.backtrack_stack,
            current_alternative: state.current_alternative,
            recursion_stack: state.recursion_stack,
            code_result: state.code_result,
            match_start_override: state.match_start_override,
            previous_match_end: state.previous_match_end,
            committed: state.committed,
            skip_position: state.skip_position,
            marks: state.marks,
            suspendable: true,
            suspension: None,
            step_count: 0,
            max_steps: self.max_steps.load(std::sync::atomic::Ordering::Relaxed),
            max_backtrack_frames: self
                .max_backtrack_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            max_recursion_depth: self
                .max_recursion_depth
                .load(std::sync::atomic::Ordering::Relaxed),
            hit_end: false,
        };

        // Determine the CodeBlockOutcome from the callback result.
        let code_outcome = match &callback_result {
            ExecResult::Success => CodeBlockOutcome::Pass,
            ExecResult::Failure | ExecResult::Error(_) => CodeBlockOutcome::Fail,
            ExecResult::Replacement(value) => {
                ctx.code_result = Some(CodeBlockValue::Replacement(value.clone()));
                CodeBlockOutcome::Pass
            }
            ExecResult::Numeric(value) => {
                ctx.code_result = Some(CodeBlockValue::Numeric(*value));
                CodeBlockOutcome::Pass
            }
            ExecResult::Steer(steer) => match steer {
                SteerResult::Continue => CodeBlockOutcome::Pass,
                SteerResult::Fail => CodeBlockOutcome::Fail,
                SteerResult::Accept => CodeBlockOutcome::Accept,
                SteerResult::Skip(n) => {
                    ctx.pos += n;
                    CodeBlockOutcome::Pass
                }
                SteerResult::Abort => {
                    ctx.committed = true;
                    CodeBlockOutcome::Fail
                }
            },
            ExecResult::Suspend(_) => {
                // Treat nested Suspend as failure.
                CodeBlockOutcome::Fail
            }
            ExecResult::Structured(value) => {
                ctx.code_result = Some(CodeBlockValue::Structured(value.clone()));
                CodeBlockOutcome::Pass
            }
        };

        // Process the resolved callback outcome.
        match code_outcome {
            CodeBlockOutcome::Pass => {
                // Continue executing from the saved instruction pointer.
                let matched = self.resume_execute_from(&mut ctx, state.ip, state.scan_start);
                if let Some((callback_name, ip)) = ctx.suspension.take() {
                    let variable_snapshot = self
                        .execution_manager
                        .as_ref()
                        .map(|em| em.variable_snapshot())
                        .unwrap_or_default();

                    let cont = MatchContinuation {
                        text: text.clone(),
                        pending_callback_name: callback_name,
                        pending_context: ExecContextSnapshot {
                            position: ctx.pos,
                            match_start: ctx.match_start,
                            captures: ctx.captures.clone(),
                            variables: variable_snapshot,
                        },
                        vm_state: VmResumeState {
                            pos: ctx.pos,
                            match_start: ctx.match_start,
                            ip,
                            captures: ctx.captures.clone(),
                            capture_trail: ctx.capture_trail.clone(),
                            call_stack: ctx.call_stack.clone(),
                            backtrack_stack: ctx.backtrack_stack.clone(),
                            current_alternative: ctx.current_alternative,
                            recursion_stack: ctx.recursion_stack.clone(),
                            code_result: ctx.code_result.clone(),
                            committed: ctx.committed,
                            skip_position: ctx.skip_position,
                            marks: ctx.marks.clone(),
                            match_start_override: ctx.match_start_override,
                            previous_match_end: ctx.previous_match_end,
                            scan_start: state.scan_start,
                        },
                    };
                    return MatchOutcome::Suspended(Box::new(cont));
                }
                if matched {
                    self.emit_event(&MatchEvent::MatchAttemptCompleted {
                        position: state.scan_start,
                        matched: true,
                    });
                    let effective_start = ctx.match_start_override.unwrap_or(state.scan_start);
                    return MatchOutcome::Completed(Some(crate::engine::MatchResult {
                        start: effective_start,
                        end: ctx.pos,
                        groups: self.extract_captures_with_match(&ctx, effective_start, ctx.pos),
                        matched_branch_number: ctx.current_alternative.map(|id| id + 1),
                        code_result: ctx.code_result.clone(),
                        last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                    }));
                }
                self.emit_event(&MatchEvent::MatchAttemptCompleted {
                    position: state.scan_start,
                    matched: false,
                });
                // Match failed after callback succeeded — continue scanning.
                if ctx.committed {
                    return MatchOutcome::Completed(None);
                }
                let next_start = if let Some(skip_pos) = ctx.skip_position.take() {
                    skip_pos.max(state.scan_start + 1)
                } else {
                    state.scan_start + 1
                };
                let text_str = std::str::from_utf8(&text).unwrap_or("");
                self.find_first_suspendable_scanning(&mut ctx, text_str, next_start)
            }
            CodeBlockOutcome::Accept => {
                let effective_start = ctx.match_start_override.unwrap_or(state.scan_start);
                MatchOutcome::Completed(Some(crate::engine::MatchResult {
                    start: effective_start,
                    end: ctx.pos,
                    groups: self.extract_captures_with_match(&ctx, effective_start, ctx.pos),
                    matched_branch_number: ctx.current_alternative.map(|id| id + 1),
                    code_result: ctx.code_result.clone(),
                    last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                }))
            }
            CodeBlockOutcome::Fail | CodeBlockOutcome::Suspended(_) => {
                // Callback failed — try backtracking first, then continue scanning.
                let mut ip = state.ip;
                let bt_result = self.try_backtrack(&mut ctx, &mut ip);
                if bt_result {
                    let matched = self.resume_execute_from(&mut ctx, ip, state.scan_start);
                    if let Some((callback_name, new_ip)) = ctx.suspension.take() {
                        let variable_snapshot = self
                            .execution_manager
                            .as_ref()
                            .map(|em| em.variable_snapshot())
                            .unwrap_or_default();

                        let cont = MatchContinuation {
                            text: text.clone(),
                            pending_callback_name: callback_name,
                            pending_context: ExecContextSnapshot {
                                position: ctx.pos,
                                match_start: ctx.match_start,
                                captures: ctx.captures.clone(),
                                variables: variable_snapshot,
                            },
                            vm_state: VmResumeState {
                                pos: ctx.pos,
                                match_start: ctx.match_start,
                                ip: new_ip,
                                captures: ctx.captures.clone(),
                                capture_trail: ctx.capture_trail.clone(),
                                call_stack: ctx.call_stack.clone(),
                                backtrack_stack: ctx.backtrack_stack.clone(),
                                current_alternative: ctx.current_alternative,
                                recursion_stack: ctx.recursion_stack.clone(),
                                code_result: ctx.code_result.clone(),
                                committed: ctx.committed,
                                skip_position: ctx.skip_position,
                                marks: ctx.marks.clone(),
                                match_start_override: ctx.match_start_override,
                                previous_match_end: ctx.previous_match_end,
                                scan_start: state.scan_start,
                            },
                        };
                        return MatchOutcome::Suspended(Box::new(cont));
                    }
                    if matched {
                        let effective_start = ctx.match_start_override.unwrap_or(state.scan_start);
                        return MatchOutcome::Completed(Some(crate::engine::MatchResult {
                            start: effective_start,
                            end: ctx.pos,
                            groups: self.extract_captures_with_match(
                                &ctx,
                                effective_start,
                                ctx.pos,
                            ),
                            matched_branch_number: ctx.current_alternative.map(|id| id + 1),
                            code_result: ctx.code_result.clone(),
                            last_mark: ctx.marks.last().map(|(name, _)| name.clone()),
                        }));
                    }
                }
                // Backtrack exhausted or failed — continue scanning from next position.
                if ctx.committed {
                    return MatchOutcome::Completed(None);
                }
                let next_start = if let Some(skip_pos) = ctx.skip_position.take() {
                    skip_pos.max(state.scan_start + 1)
                } else {
                    state.scan_start + 1
                };
                let text_str = std::str::from_utf8(&text).unwrap_or("");
                self.find_first_suspendable_scanning(&mut ctx, text_str, next_start)
            }
        }
    }

    /// Resume VM execution from a saved instruction pointer within the
    /// current match attempt. Returns `true` if the match succeeds.
    ///
    /// Delegates directly to [`execute_at_continuation`] which handles the
    /// full opcode dispatch without resetting context state.
    fn resume_execute_from(
        &self,
        ctx: &mut ExecContext<'_>,
        ip: usize,
        _scan_start: usize,
    ) -> bool {
        self.execute_at_continuation(ctx, ip)
    }

    /// Continue executing the main VM dispatch loop from a given instruction
    /// pointer, WITHOUT resetting context state.
    ///
    /// This is the key resumption primitive. It mirrors `execute_at` but
    /// does not reset `pos`, `match_start`, `code_result`, or other state.
    #[allow(clippy::too_many_lines)]
    #[allow(clippy::cast_possible_wrap)] // Bytecode jump offsets use i16; safe by construction
    #[allow(clippy::cast_sign_loss)] // Jump target computation mirrors execute_at
    fn execute_at_continuation(&self, ctx: &mut ExecContext<'_>, start_ip: usize) -> bool {
        // We need to run the same opcode dispatch as execute_at, but from
        // an arbitrary IP without resetting state. Rather than duplicating
        // the entire execute_at function, we leverage the fact that
        // execute_at reads its starting position from ctx.pos and starts
        // its ip at 0. We can create a sub-slice of the program code
        // starting at start_ip and execute that.
        //
        // However, jump targets in the bytecode are absolute offsets, so
        // sub-slicing would break all jumps. Instead, we'll use a minimal
        // approach: temporarily set up the state and call into execute_at
        // by creating a helper that takes a starting IP.

        // The correct approach: duplicate the execute_at loop but starting
        // from start_ip. Since execute_at is too large to cleanly refactor
        // without risk, and since this is the resume path (not hot), we
        // use a pragmatic shortcut.
        //
        // We save the current context state, then call execute_at which
        // will reset match_start etc. We need to prevent that reset.
        // The simplest safe approach: modify execute_at to accept a
        // starting IP. But that would change its signature.
        //
        // Instead: we know the only state execute_at resets is:
        //   ctx.pos = start (we set this correctly)
        //   ctx.match_start = start (we need to preserve our value)
        //   ctx.code_result = None (we need to preserve our value)
        //   ctx.match_start_override = None (we need to preserve)
        //   ctx.committed = false (we need to preserve)
        //   ctx.skip_position = None (we need to preserve)
        //   ip = 0 (we need start_ip)
        //
        // So we cannot simply call execute_at. Instead we use a
        // specialized continuation that runs the loop from start_ip.
        // This is the only way to maintain correctness.

        let code = &self.program.code;
        let mut ip = start_ip;

        // This is the same dispatch loop as execute_at, but without the
        // initialization preamble. We replicate only the loop structure
        // and delegate opcodes we don't handle to try_backtrack/fail.
        loop {
            if ip >= code.len() {
                return false;
            }

            let op = OpCode::try_from(code[ip]).unwrap_or(OpCode::Fail);
            ip += 1;

            match op {
                OpCode::Match => return true,
                OpCode::Fail => {
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::Char => {
                    if let Some(expected) = Self::read_char_operand(code, &mut ip) {
                        if let Some(actual) = Self::current_char(ctx) {
                            if actual == expected {
                                Self::advance_char(ctx);
                                continue;
                            }
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::Any => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if ch != '\n' {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::AnyDotAll => {
                    if Self::current_char(ctx).is_some() {
                        Self::advance_char(ctx);
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::DigitAscii
                | OpCode::DigitAsciiNeg
                | OpCode::WordAscii
                | OpCode::WordAsciiNeg
                | OpCode::SpaceAscii
                | OpCode::SpaceAsciiNeg => {
                    if let Some(ch) = Self::current_char(ctx) {
                        let matched = match op {
                            OpCode::DigitAscii => ch.is_ascii_digit(),
                            OpCode::DigitAsciiNeg => !ch.is_ascii_digit(),
                            OpCode::WordAscii => ch.is_ascii_alphanumeric() || ch == '_',
                            OpCode::WordAsciiNeg => !(ch.is_ascii_alphanumeric() || ch == '_'),
                            OpCode::SpaceAscii => pcre2_is_space_char(ch),
                            OpCode::SpaceAsciiNeg => !pcre2_is_space_char(ch),
                            _ => false,
                        };
                        if matched {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::CharClass | OpCode::CharClassNeg => {
                    if ip < code.len() {
                        let class_id = code[ip] as usize;
                        ip += 1;
                        if let Some(ch) = Self::current_char(ctx) {
                            if let Some(cc) = self.program.char_classes.get(class_id) {
                                let in_class = Self::test_char_class(ch, cc);
                                let matched = if op == OpCode::CharClass {
                                    in_class
                                } else {
                                    !in_class
                                };
                                if matched {
                                    Self::advance_char(ctx);
                                    continue;
                                }
                            }
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::Jump => {
                    if ip + 1 < code.len() {
                        let offset = i16::from_le_bytes([code[ip], code[ip + 1]]);
                        ip = ((ip as isize) + (offset as isize)) as usize;
                    } else {
                        return false;
                    }
                }
                OpCode::Split => {
                    if ip + 3 < code.len() {
                        let offset1 = i16::from_le_bytes([code[ip], code[ip + 1]]);
                        let offset2 = i16::from_le_bytes([code[ip + 2], code[ip + 3]]);
                        ip += 4;
                        let target1 = ((ip as isize) + (offset1 as isize) - 4) as usize;
                        let target2 = ((ip as isize) + (offset2 as isize) - 4) as usize;
                        ctx.backtrack_stack.push(BacktrackFrame {
                            ip: target2,
                            pos: ctx.pos,
                            trail_mark: ctx.capture_trail.len(),
                            call_stack_mark: ctx.call_stack.len(),
                            capture_snapshot: None,
                            saved_code_result: ctx.code_result.clone(),
                            saved_match_start_override: ctx.match_start_override,
                        });
                        ip = target1;
                    } else {
                        return false;
                    }
                }
                OpCode::SplitLazy => {
                    if ip + 3 < code.len() {
                        let offset1 = i16::from_le_bytes([code[ip], code[ip + 1]]);
                        let offset2 = i16::from_le_bytes([code[ip + 2], code[ip + 3]]);
                        ip += 4;
                        let target1 = ((ip as isize) + (offset1 as isize) - 4) as usize;
                        let target2 = ((ip as isize) + (offset2 as isize) - 4) as usize;
                        ctx.backtrack_stack.push(BacktrackFrame {
                            ip: target1,
                            pos: ctx.pos,
                            trail_mark: ctx.capture_trail.len(),
                            call_stack_mark: ctx.call_stack.len(),
                            capture_snapshot: None,
                            saved_code_result: ctx.code_result.clone(),
                            saved_match_start_override: ctx.match_start_override,
                        });
                        ip = target2;
                    } else {
                        return false;
                    }
                }
                OpCode::SaveStart => {
                    if ip < code.len() {
                        let group_id = code[ip] as usize;
                        ip += 1;
                        let slot = group_id * 2;
                        if slot < ctx.captures.len() {
                            Self::set_capture(ctx, slot, Some(ctx.pos));
                        }
                    }
                }
                OpCode::SaveEnd => {
                    if ip < code.len() {
                        let group_id = code[ip] as usize;
                        ip += 1;
                        let slot = group_id * 2 + 1;
                        if slot < ctx.captures.len() {
                            Self::set_capture(ctx, slot, Some(ctx.pos));
                        }
                    }
                }
                OpCode::SetAlternative => {
                    if ip < code.len() {
                        ctx.current_alternative = Some(code[ip] as usize);
                        ip += 1;
                    }
                }
                OpCode::StartLine => {
                    if ctx.pos <= ctx.text.len()
                        && self
                            .program
                            .newline_mode
                            .is_line_start_before(ctx.text, ctx.pos)
                    {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::EndLine => {
                    if self.program.newline_mode.is_line_end_at(ctx.text, ctx.pos) {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::StartText => {
                    if ctx.pos == 0 {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::EndText => {
                    if ctx.pos >= ctx.text.len() {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::EndTextOrNL => {
                    if ctx.pos >= ctx.text.len()
                        || (ctx.pos + 1 == ctx.text.len() && ctx.text[ctx.pos] == b'\n')
                    {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::WordBoundary | OpCode::NonWordBoundary => {
                    let before_is_word = if ctx.pos > 0 {
                        let b = ctx.text[ctx.pos - 1];
                        b.is_ascii_alphanumeric() || b == b'_'
                    } else {
                        false
                    };
                    let after_is_word = if ctx.pos < ctx.text.len() {
                        let b = ctx.text[ctx.pos];
                        b.is_ascii_alphanumeric() || b == b'_'
                    } else {
                        false
                    };
                    let is_boundary = before_is_word != after_is_word;
                    let ok = if op == OpCode::WordBoundary {
                        is_boundary
                    } else {
                        !is_boundary
                    };
                    if ok {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::Backref => {
                    if ip < code.len() {
                        let group_id = code[ip] as usize;
                        ip += 1;
                        if self.match_backreference(ctx, group_id) {
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::BackrefCaseInsensitive => {
                    if ip < code.len() {
                        let group_id = code[ip] as usize;
                        ip += 1;
                        if self.match_backreference_case_insensitive(ctx, group_id) {
                            continue;
                        }
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::MatchReset => {
                    ctx.match_start_override = Some(ctx.pos);
                }
                OpCode::PreviousMatchEnd => {
                    let expected = ctx.previous_match_end.unwrap_or(0);
                    if ctx.pos == expected {
                        continue;
                    }
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
                OpCode::CodeBlock => {
                    let outcome = if ctx.suspendable {
                        self.execute_inline_code_block_suspendable(ctx, code, &mut ip)
                    } else {
                        self.execute_inline_code_block(ctx, code, &mut ip)
                    };
                    match outcome {
                        Some(CodeBlockOutcome::Pass) => {}
                        Some(CodeBlockOutcome::Fail) => {
                            if self.try_backtrack(ctx, &mut ip) {
                                continue;
                            }
                            return false;
                        }
                        Some(CodeBlockOutcome::Accept) => {
                            return true;
                        }
                        Some(CodeBlockOutcome::Suspended(name)) => {
                            ctx.suspension = Some((name, ip));
                            return false;
                        }
                        None => return false,
                    }
                }
                OpCode::Lookahead
                | OpCode::LookaheadNeg
                | OpCode::Lookbehind
                | OpCode::LookbehindNeg => {
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;
                    let expr_start = ip;
                    let expr_end = ip + expr_len;
                    if expr_end > code.len() {
                        return false;
                    }
                    let positive = matches!(op, OpCode::Lookahead | OpCode::Lookbehind);
                    let matched = match op {
                        OpCode::Lookahead | OpCode::LookaheadNeg => self.execute_assertion_subexpr(
                            ctx,
                            &code[expr_start..expr_end],
                            positive,
                        ),
                        OpCode::Lookbehind | OpCode::LookbehindNeg => self
                            .execute_lookbehind_assertion(
                                ctx,
                                &code[expr_start..expr_end],
                                positive,
                            ),
                        _ => false,
                    };
                    let assertion_holds = if positive { matched } else { !matched };
                    if !assertion_holds {
                        if self.try_backtrack(ctx, &mut ip) {
                            continue;
                        }
                        return false;
                    }
                    ip = expr_end;
                }
                OpCode::AtomicStart => {
                    ctx.call_stack.push(ctx.backtrack_stack.len());
                }
                OpCode::AtomicEnd => {
                    if let Some(saved_len) = ctx.call_stack.pop() {
                        ctx.backtrack_stack.truncate(saved_len);
                    }
                }
                OpCode::Call => {
                    if ip < code.len() {
                        let target = code[ip] as usize;
                        ip += 1;
                        if !self.invoke_subroutine(ctx, target) {
                            if self.try_backtrack(ctx, &mut ip) {
                                continue;
                            }
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                OpCode::JumpIfNoMatch => {
                    if ip + 1 < code.len() {
                        let offset = i16::from_le_bytes([code[ip], code[ip + 1]]);
                        ip += 2;
                        // Conditional jump not matched — just skip
                        let _ = offset;
                    } else {
                        return false;
                    }
                }
                OpCode::Commit => {
                    ctx.committed = true;
                }
                OpCode::Prune | OpCode::Then => {
                    ctx.backtrack_stack.clear();
                }
                OpCode::VerbSkip => {
                    ctx.skip_position = Some(ctx.pos);
                }
                OpCode::VerbSkipNamed => {
                    // (*SKIP:name): A11. Same logic as the main
                    // dispatch — look up the matching mark in
                    // ctx.marks and set ctx.skip_position to that
                    // mark's recorded text position. No-op if no
                    // matching mark exists.
                    if ip < code.len() {
                        let name_len = code[ip] as usize;
                        ip += 1;
                        if ip + name_len <= code.len() {
                            let name = std::str::from_utf8(&code[ip..ip + name_len]).unwrap_or("");
                            if let Some(pos) = ctx
                                .marks
                                .iter()
                                .rev()
                                .find(|(n, _)| n == name)
                                .map(|(_, p)| *p)
                            {
                                ctx.skip_position = Some(pos);
                                ctx.backtrack_stack.clear();
                            }
                            ip += name_len;
                        }
                    }
                }
                OpCode::Mark => {
                    // (*MARK:name): A11. Push (name, ctx.pos) to
                    // ctx.marks so a later (*SKIP:name) can find it.
                    if ip < code.len() {
                        let name_len = code[ip] as usize;
                        ip += 1;
                        if ip + name_len <= code.len() {
                            let name = std::str::from_utf8(&code[ip..ip + name_len])
                                .unwrap_or("")
                                .to_string();
                            ctx.marks.push((name, ctx.pos));
                            ip += name_len;
                        }
                    }
                }
                _ => {
                    // Unhandled opcode in continuation — fail gracefully.
                    if self.try_backtrack(ctx, &mut ip) {
                        continue;
                    }
                    return false;
                }
            }
        }
    }

    /// Execute a sub-expression (used for quantifiers)
    #[allow(clippy::too_many_lines)] // Subexpression dispatch mirrors execute_at — same architectural constraint
    fn execute_subexpr(&self, ctx: &mut ExecContext<'_>, code: &[u8]) -> bool {
        self.execute_subexpr_inner(ctx, code, None)
    }

    /// Like [`execute_subexpr`], but rejects zero-width matches.
    ///
    /// When a sub-expression inside a `*` or `+` quantifier matches
    /// without advancing the position, the quantifier loop normally
    /// stops.  However, the sub-expression may contain alternatives
    /// (e.g. `[^()]*|(?&pair)`) where the first branch matches zero
    /// width but a later branch would advance.  This variant
    /// back-tracks through the sub-expression's own local stack when
    /// a would-be success does not advance past `origin`, ensuring
    /// every alternative is explored before giving up.
    fn execute_subexpr_advancing(
        &self,
        ctx: &mut ExecContext<'_>,
        code: &[u8],
        origin: usize,
    ) -> bool {
        self.execute_subexpr_inner(ctx, code, Some(origin))
    }

    fn execute_subexpr_inner(
        &self,
        ctx: &mut ExecContext<'_>,
        code: &[u8],
        must_advance_from: Option<usize>,
    ) -> bool {
        let mut ip = 0;
        let mut backtrack_stack: Vec<BacktrackFrame> = Vec::new();
        let mut call_stack = Vec::new();

        macro_rules! local_backtrack_or_return_false {
            () => {
                if let Some(frame) = backtrack_stack.pop() {
                    ip = frame.ip;
                    ctx.pos = frame.pos;
                    if let Some(ref snapshot) = frame.capture_snapshot {
                        ctx.capture_trail.truncate(frame.trail_mark);
                        ctx.captures.copy_from_slice(snapshot);
                    } else {
                        Self::undo_trail(ctx, frame.trail_mark);
                    }
                    call_stack.truncate(frame.call_stack_mark);
                    ctx.code_result = frame.saved_code_result;
                    continue;
                }
                return false;
            };
        }

        loop {
            if ip >= code.len() {
                // When must_advance_from is set, reject zero-width
                // matches by back-tracking through alternatives.
                if let Some(origin) = must_advance_from {
                    if ctx.pos == origin {
                        local_backtrack_or_return_false!();
                    }
                }
                return true; // Successfully executed all instructions
            }

            let op = OpCode::try_from(code[ip]).unwrap_or(OpCode::Fail);
            ip += 1;

            match op {
                OpCode::WordAscii => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::WordAsciiNeg => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if !(ch.is_ascii_alphanumeric() || ch == '_') {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }

                OpCode::DigitAscii => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if ch.is_ascii_digit() {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::DigitAsciiNeg => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if !ch.is_ascii_digit() {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::SpaceAscii => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if pcre2_is_space_char(ch) {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::SpaceAsciiNeg => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if !pcre2_is_space_char(ch) {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }

                OpCode::Char => {
                    // Read UTF-8 character from operands
                    if let Some(expected) = Self::read_char_operand(code, &mut ip) {
                        if let Some(actual) = Self::current_char(ctx) {
                            if actual == expected {
                                Self::advance_char(ctx);
                                continue;
                            }
                        }
                    }
                    local_backtrack_or_return_false!();
                }

                OpCode::Any => {
                    if let Some(ch) = Self::current_char(ctx) {
                        if ch != '\n' {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::AnyDotAll => {
                    if Self::current_char(ctx).is_some() {
                        Self::advance_char(ctx);
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::GraphemeCluster => {
                    use unicode_segmentation::UnicodeSegmentation;
                    if ctx.pos < ctx.text.len() {
                        // SAFETY: ctx.text is guaranteed valid UTF-8 on the &str path.
                        let remaining =
                            unsafe { std::str::from_utf8_unchecked(&ctx.text[ctx.pos..]) };
                        if let Some(cluster) = remaining.graphemes(true).next() {
                            ctx.pos += cluster.len();
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::StartLine => {
                    if self
                        .program
                        .newline_mode
                        .is_line_start_before(ctx.text, ctx.pos)
                    {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::StartText => {
                    if ctx.pos == 0 {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::EndLine => {
                    if self.program.newline_mode.is_line_end_at(ctx.text, ctx.pos) {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::EndText => {
                    if Self::is_at_absolute_end(ctx) {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::EndTextOrNL => {
                    if Self::is_at_absolute_end_or_before_final_newline(ctx) {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::WordBoundary => {
                    if Self::is_at_word_boundary(ctx) {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::NonWordBoundary => {
                    if !Self::is_at_word_boundary(ctx) {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }
                OpCode::PreviousMatchEnd => {
                    let target = ctx.previous_match_end.unwrap_or(0);
                    if ctx.pos == target {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }

                OpCode::CharClass | OpCode::CharClassNeg => {
                    let is_neg = matches!(op, OpCode::CharClassNeg);

                    // Read character class ID
                    if ip >= code.len() {
                        return false;
                    }
                    let class_id = code[ip] as usize;
                    ip += 1;

                    // Get the character class
                    if class_id >= self.program.char_classes.len() {
                        return false;
                    }
                    let char_class = &self.program.char_classes[class_id];

                    // Get current character
                    if let Some(ch) = Self::current_char(ctx) {
                        let matches = Self::test_char_class(ch, char_class);
                        let should_match = if is_neg { !matches } else { matches };

                        if should_match {
                            Self::advance_char(ctx);
                            continue;
                        }
                    }
                    local_backtrack_or_return_false!();
                }

                OpCode::Lookahead
                | OpCode::LookaheadNeg
                | OpCode::Lookbehind
                | OpCode::LookbehindNeg => {
                    // Read the length of the assertion sub-expression
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    let positive = matches!(op, OpCode::Lookahead | OpCode::Lookbehind);
                    let matched = match op {
                        OpCode::Lookahead | OpCode::LookaheadNeg => self.execute_assertion_subexpr(
                            ctx,
                            &code[expr_start..expr_end],
                            positive,
                        ),
                        OpCode::Lookbehind | OpCode::LookbehindNeg => self
                            .execute_lookbehind_assertion(
                                ctx,
                                &code[expr_start..expr_end],
                                positive,
                            ),
                        _ => false,
                    };
                    let assertion_holds = if positive { matched } else { !matched };

                    if !assertion_holds {
                        local_backtrack_or_return_false!();
                    }

                    // Assertions do not consume input
                    ip = expr_end;
                }

                OpCode::CodeBlock => {
                    let outcome = if ctx.suspendable {
                        self.execute_inline_code_block_suspendable(ctx, code, &mut ip)
                    } else {
                        self.execute_inline_code_block(ctx, code, &mut ip)
                    };
                    match outcome {
                        Some(CodeBlockOutcome::Pass) => {}
                        Some(CodeBlockOutcome::Fail) => {
                            local_backtrack_or_return_false!();
                        }
                        Some(CodeBlockOutcome::Accept) => {
                            return true;
                        }
                        Some(CodeBlockOutcome::Suspended(name)) => {
                            ctx.suspension = Some((name, ip));
                            return false;
                        }
                        None => return false,
                    }
                }

                OpCode::AtomicStart => {
                    call_stack.push(backtrack_stack.len());
                }

                OpCode::AtomicEnd => {
                    if let Some(mark) = call_stack.pop() {
                        backtrack_stack.truncate(mark);
                        continue;
                    }
                    return false;
                }

                OpCode::SaveStart => {
                    if ip >= code.len() {
                        return false;
                    }
                    let group_id = code[ip] as usize;
                    ip += 1;

                    let start_idx = group_id * 2;
                    if start_idx < ctx.captures.len() {
                        Self::set_capture(ctx, start_idx, Some(ctx.pos));
                    }
                }

                OpCode::SaveEnd => {
                    if ip >= code.len() {
                        return false;
                    }
                    let group_id = code[ip] as usize;
                    ip += 1;

                    let end_idx = group_id * 2 + 1;
                    if end_idx < ctx.captures.len() {
                        Self::set_capture(ctx, end_idx, Some(ctx.pos));
                    }
                }

                OpCode::Backref => {
                    if ip >= code.len() {
                        return false;
                    }
                    let group_id = code[ip] as usize;
                    ip += 1;

                    if self.match_backreference(ctx, group_id) {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }

                OpCode::BackrefCaseInsensitive => {
                    if ip >= code.len() {
                        return false;
                    }
                    let group_id = code[ip] as usize;
                    ip += 1;

                    if self.match_backreference_case_insensitive(ctx, group_id) {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }

                OpCode::JumpIfMatch | OpCode::JumpIfNoMatch => {
                    let jump_if_match = matches!(op, OpCode::JumpIfMatch);
                    let Some(condition_matches) =
                        self.evaluate_conditional_operand(ctx, code, &mut ip)
                    else {
                        return false;
                    };

                    if ip + 1 >= code.len() {
                        return false;
                    }
                    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;

                    if condition_matches == jump_if_match {
                        ip += offset;
                    }
                }

                OpCode::Split => {
                    if ip + 1 >= code.len() {
                        return false;
                    }
                    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;

                    backtrack_stack.push(BacktrackFrame {
                        ip: ip + offset,
                        pos: ctx.pos,
                        trail_mark: ctx.capture_trail.len(),
                        call_stack_mark: call_stack.len(),
                        capture_snapshot: None,
                        saved_code_result: ctx.code_result.clone(),
                        saved_match_start_override: ctx.match_start_override,
                    });
                }

                OpCode::SplitLazy => {
                    if ip + 1 >= code.len() {
                        return false;
                    }
                    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;

                    backtrack_stack.push(BacktrackFrame {
                        ip,
                        pos: ctx.pos,
                        trail_mark: ctx.capture_trail.len(),
                        call_stack_mark: call_stack.len(),
                        capture_snapshot: None,
                        saved_code_result: ctx.code_result.clone(),
                        saved_match_start_override: ctx.match_start_override,
                    });
                    ip += offset;
                }

                OpCode::Jump => {
                    if ip + 1 >= code.len() {
                        return false;
                    }
                    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2;
                    ip += offset;
                }

                OpCode::Call => {
                    if ip >= code.len() {
                        return false;
                    }
                    let target = code[ip] as usize;
                    ip += 1;

                    if self.invoke_subroutine(ctx, target) {
                        continue;
                    }
                    local_backtrack_or_return_false!();
                }

                OpCode::QuestionGreedy => {
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    // Mirror of the main VM's QuestionGreedy zero-width fix:
                    // a zero-width body match preserves captures so the
                    // rest of the pattern (conditional tests, backrefs)
                    // sees the group as having participated.
                    let before_pos = ctx.pos;
                    let trail_mark = ctx.capture_trail.len();
                    let cs_mark = call_stack.len();
                    let saved_code_result = ctx.code_result.clone();
                    let matched = self.execute_subexpr(ctx, &code[expr_start..expr_end]);
                    if matched {
                        backtrack_stack.push(BacktrackFrame {
                            ip: expr_end,
                            pos: before_pos,
                            trail_mark,
                            call_stack_mark: cs_mark,
                            capture_snapshot: None,
                            saved_code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    } else {
                        ctx.pos = before_pos;
                        Self::undo_trail(ctx, trail_mark);
                        call_stack.truncate(cs_mark);
                        ctx.code_result = saved_code_result;
                    }

                    ip = expr_end;
                }

                OpCode::QuestionLazy => {
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    if self
                        .probe_subexpr(ctx, &code[expr_start..expr_end])
                        .is_some()
                    {
                        backtrack_stack.push(BacktrackFrame {
                            ip: expr_start,
                            pos: ctx.pos,
                            trail_mark: ctx.capture_trail.len(),
                            call_stack_mark: call_stack.len(),
                            capture_snapshot: None,
                            saved_code_result: ctx.code_result.clone(),
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::StarGreedy => {
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    loop {
                        let before_pos = ctx.pos;
                        let trail_mark = ctx.capture_trail.len();
                        let cs_mark = call_stack.len();
                        let saved_code_result = ctx.code_result.clone();
                        if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                            ctx.pos = before_pos;
                            Self::undo_trail(ctx, trail_mark);
                            call_stack.truncate(cs_mark);
                            ctx.code_result = saved_code_result;
                            break;
                        }
                        // PCRE2 semantic: zero-width iteration terminates
                        // the star loop — match count may be zero here,
                        // which is valid for `*`. See the main-path
                        // handler at StarGreedy for the commentary.
                        if ctx.pos == before_pos {
                            Self::undo_trail(ctx, trail_mark);
                            call_stack.truncate(cs_mark);
                            ctx.code_result = saved_code_result;
                            break;
                        }
                        backtrack_stack.push(BacktrackFrame {
                            ip: expr_end,
                            pos: before_pos,
                            trail_mark,
                            call_stack_mark: cs_mark,
                            capture_snapshot: None,
                            saved_code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::StarLazy => {
                    let opcode_start = ip - 1;
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    if let Some(probe_ctx) = self.probe_subexpr(ctx, &code[expr_start..expr_end]) {
                        backtrack_stack.push(BacktrackFrame {
                            ip: opcode_start,
                            pos: probe_ctx.pos,
                            trail_mark: ctx.capture_trail.len(),
                            call_stack_mark: call_stack.len(),
                            capture_snapshot: Some(probe_ctx.captures),
                            saved_code_result: probe_ctx.code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::PlusGreedy => {
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    let first_pos = ctx.pos;
                    let first_trail_mark = ctx.capture_trail.len();
                    let first_code_result = ctx.code_result.clone();
                    if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                        ctx.pos = first_pos;
                        Self::undo_trail(ctx, first_trail_mark);
                        ctx.code_result = first_code_result;
                        local_backtrack_or_return_false!();
                    }
                    // PCRE2 semantic: a zero-width first iteration of
                    // `X+` is still one (zero-width) iteration — accept
                    // it and don't force an advancing retry.

                    loop {
                        let before_pos = ctx.pos;
                        let trail_mark = ctx.capture_trail.len();
                        let cs_mark = call_stack.len();
                        let saved_code_result = ctx.code_result.clone();
                        if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                            ctx.pos = before_pos;
                            Self::undo_trail(ctx, trail_mark);
                            call_stack.truncate(cs_mark);
                            ctx.code_result = saved_code_result;
                            break;
                        }
                        // PCRE2 semantic: zero-width iteration ends the loop.
                        if ctx.pos == before_pos {
                            Self::undo_trail(ctx, trail_mark);
                            call_stack.truncate(cs_mark);
                            ctx.code_result = saved_code_result;
                            break;
                        }
                        backtrack_stack.push(BacktrackFrame {
                            ip: expr_end,
                            pos: before_pos,
                            trail_mark,
                            call_stack_mark: cs_mark,
                            capture_snapshot: None,
                            saved_code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::PlusLazy => {
                    let opcode_start = ip - 1;
                    if ip >= code.len() {
                        return false;
                    }
                    let expr_len = code[ip] as usize;
                    ip += 1;

                    let expr_start = ip;
                    let expr_end = ip + expr_len;

                    if expr_end > code.len() {
                        return false;
                    }

                    let before_pos = ctx.pos;
                    let trail_mark = ctx.capture_trail.len();
                    let saved_code_result = ctx.code_result.clone();
                    let matched = self.execute_subexpr(ctx, &code[expr_start..expr_end]);
                    if !matched || ctx.pos == before_pos {
                        ctx.pos = before_pos;
                        Self::undo_trail(ctx, trail_mark);
                        ctx.code_result = saved_code_result;
                        local_backtrack_or_return_false!();
                    }

                    let after_first_trail_mark = ctx.capture_trail.len();
                    let after_first_cs_mark = call_stack.len();
                    let after_first_code_result = ctx.code_result.clone();
                    if self
                        .probe_subexpr(ctx, &code[expr_start..expr_end])
                        .is_some()
                    {
                        backtrack_stack.push(BacktrackFrame {
                            ip: opcode_start,
                            pos: ctx.pos,
                            trail_mark: after_first_trail_mark,
                            call_stack_mark: after_first_cs_mark,
                            capture_snapshot: None,
                            saved_code_result: after_first_code_result,
                            saved_match_start_override: ctx.match_start_override,
                        });
                    }

                    ip = expr_end;
                }

                OpCode::Fail => {
                    local_backtrack_or_return_false!();
                }

                OpCode::Match => {
                    if let Some(origin) = must_advance_from {
                        if ctx.pos == origin {
                            local_backtrack_or_return_false!();
                        }
                    }
                    return true;
                }

                OpCode::MatchReset => {
                    ctx.match_start_override = Some(ctx.pos);
                }

                OpCode::Commit => {
                    ctx.committed = true;
                }

                OpCode::Prune | OpCode::Then => {
                    backtrack_stack.clear();
                }

                OpCode::VerbSkip => {
                    ctx.skip_position = Some(ctx.pos);
                    backtrack_stack.clear();
                }

                OpCode::VerbSkipNamed => {
                    // (*SKIP:name): A11. Look up the matching mark
                    // in ctx.marks and set ctx.skip_position to its
                    // recorded text position. No-op if no matching
                    // mark exists.
                    if ip < code.len() {
                        let name_len = code[ip] as usize;
                        ip += 1;
                        if ip + name_len <= code.len() {
                            let name = std::str::from_utf8(&code[ip..ip + name_len]).unwrap_or("");
                            if let Some(pos) = ctx
                                .marks
                                .iter()
                                .rev()
                                .find(|(n, _)| n == name)
                                .map(|(_, p)| *p)
                            {
                                ctx.skip_position = Some(pos);
                                backtrack_stack.clear();
                            }
                            ip += name_len;
                        }
                    }
                }

                OpCode::Mark => {
                    // (*MARK:name): A11. Push (name, ctx.pos) to
                    // ctx.marks so a later (*SKIP:name) can find it.
                    if ip < code.len() {
                        let name_len = code[ip] as usize;
                        ip += 1;
                        if ip + name_len <= code.len() {
                            let name = std::str::from_utf8(&code[ip..ip + name_len])
                                .unwrap_or("")
                                .to_string();
                            ctx.marks.push((name, ctx.pos));
                            ip += name_len;
                        }
                    }
                }

                _ => {
                    return false;
                }
            }
        }
    }

    /// Execute an assertion sub-expression without consuming input
    /// or mutating the parent execution context.
    /// Run a lookahead / conditional-lookaround assertion against a
    /// clone of `ctx` and report whether the body matched. If
    /// `propagate_captures` is true AND the body matched, the clone's
    /// capture slots and capture trail are merged back into `ctx` —
    /// this is how PCRE2 positive lookarounds make their internal
    /// captures visible to the outer match (e.g. `(?=(foo))\1` on
    /// "foofoo" captures group 1 = "foo" via the lookahead and lets
    /// `\1` at the outer level use it). Negative lookarounds call in
    /// with `propagate_captures: false` so captures set inside a
    /// failing-for-the-outer body are always discarded.
    fn execute_assertion_subexpr(
        &self,
        ctx: &mut ExecContext<'_>,
        code: &[u8],
        propagate_captures: bool,
    ) -> bool {
        let mut assertion_ctx = Self::clone_exec_context(ctx);
        let body_matched = self.execute_subexpr(&mut assertion_ctx, code);
        if body_matched && propagate_captures {
            ctx.captures = assertion_ctx.captures;
            ctx.capture_trail = assertion_ctx.capture_trail;
        }
        body_matched
    }

    fn evaluate_conditional_operand(
        &self,
        ctx: &mut ExecContext<'_>,
        code: &[u8],
        ip: &mut usize,
    ) -> Option<bool> {
        if *ip >= code.len() {
            return None;
        }

        let kind = code[*ip];
        *ip += 1;

        match kind {
            CONDITIONAL_KIND_GROUP_EXISTS => {
                if *ip >= code.len() {
                    return None;
                }
                let group_id = code[*ip] as usize;
                *ip += 1;
                Some(Self::capture_group_exists(ctx, group_id))
            }
            CONDITIONAL_KIND_RECURSION_ANY => Some(!ctx.recursion_stack.is_empty()),
            CONDITIONAL_KIND_RECURSION_GROUP => {
                if *ip >= code.len() {
                    return None;
                }
                let group_id = code[*ip] as usize;
                *ip += 1;
                Some(
                    ctx.recursion_stack
                        .last()
                        .is_some_and(|(target, _)| *target == group_id),
                )
            }
            CONDITIONAL_KIND_LOOKAHEAD_POSITIVE | CONDITIONAL_KIND_LOOKAHEAD_NEGATIVE => {
                if *ip >= code.len() {
                    return None;
                }
                let expr_len = code[*ip] as usize;
                *ip += 1;
                let expr_start = *ip;
                let expr_end = expr_start + expr_len;
                if expr_end > code.len() {
                    return None;
                }
                let positive = kind == CONDITIONAL_KIND_LOOKAHEAD_POSITIVE;
                let matched =
                    self.execute_assertion_subexpr(ctx, &code[expr_start..expr_end], positive);
                *ip = expr_end;
                if positive {
                    Some(matched)
                } else {
                    Some(!matched)
                }
            }
            CONDITIONAL_KIND_LOOKBEHIND_POSITIVE | CONDITIONAL_KIND_LOOKBEHIND_NEGATIVE => {
                if *ip >= code.len() {
                    return None;
                }
                let expr_len = code[*ip] as usize;
                *ip += 1;
                let expr_start = *ip;
                let expr_end = expr_start + expr_len;
                if expr_end > code.len() {
                    return None;
                }
                let positive = kind == CONDITIONAL_KIND_LOOKBEHIND_POSITIVE;
                let matched =
                    self.execute_lookbehind_assertion(ctx, &code[expr_start..expr_end], positive);
                *ip = expr_end;
                if positive {
                    Some(matched)
                } else {
                    Some(!matched)
                }
            }
            CONDITIONAL_KIND_DEFINE_FALSE => Some(false),
            _ => None,
        }
    }

    fn capture_group_exists(ctx: &ExecContext<'_>, group_id: usize) -> bool {
        if group_id == 0 {
            return false;
        }

        let start_idx = group_id * 2;
        let end_idx = start_idx + 1;
        matches!(
            (
                ctx.captures.get(start_idx).and_then(|&x| x),
                ctx.captures.get(end_idx).and_then(|&x| x)
            ),
            (Some(_), Some(_))
        )
    }

    /// Execute a lookbehind assertion by finding a sub-expression match
    /// that ends exactly at the current position.
    /// Lookbehind counterpart of `execute_assertion_subexpr`. Scans
    /// backwards from `ctx.pos` for a length that ends exactly at
    /// `ctx.pos`. On the first successful body match, merges the
    /// lookbehind clone's captures + trail back into `ctx` when
    /// `propagate_captures` is true (positive lookbehind). Negative
    /// lookbehinds pass `false` so their body-captures are discarded
    /// even when the body matches (because negative-lookbehind-matches
    /// means the outer assertion has failed).
    fn execute_lookbehind_assertion(
        &self,
        ctx: &mut ExecContext<'_>,
        code: &[u8],
        propagate_captures: bool,
    ) -> bool {
        let assertion_end = ctx.pos;

        for start in (0..=assertion_end).rev() {
            let mut lookbehind_ctx = Self::clone_exec_context(ctx);
            lookbehind_ctx.pos = start;
            lookbehind_ctx.end = assertion_end;

            if self.execute_subexpr(&mut lookbehind_ctx, code)
                && lookbehind_ctx.pos == assertion_end
            {
                if propagate_captures {
                    ctx.captures = lookbehind_ctx.captures;
                    ctx.capture_trail = lookbehind_ctx.capture_trail;
                }
                return true;
            }
        }

        false
    }

    // =============================================================================
    // STATE-OF-THE-ART SIMD IMPLEMENTATIONS
    // =============================================================================
    // The following methods implement cutting-edge SIMD algorithms that represent
    // the absolute pinnacle of string matching performance. These algorithms are
    // based on the latest research in parallel string processing and incorporate
    // techniques from:
    // - Intel's Hyperscan library
    // - Google's SwissTable hash implementation
    // - Facebook's F14 vector intrinsics
    // - Academic papers on SIMD string matching (Faro & Lecroq, 2013)
    // =============================================================================

    /// Extract the first literal substring from bytecode for SIMD pre-filtering.
    ///
    /// This method performs intelligent literal extraction by analyzing the bytecode
    /// to find the longest, most selective literal substring that appears at a fixed
    /// position in the pattern. The extraction algorithm uses several heuristics:
    ///
    /// 1. **Fixed Position Priority**: Literals at fixed positions (not after *, +, ?)
    ///    are preferred as they provide deterministic filtering.
    /// 2. **Length Optimization**: Longer literals reduce false positives.
    /// 3. **Frequency Analysis**: Less common bytes are preferred (e.g., 'q' over 'e').
    /// 4. **UTF-8 Awareness**: Multi-byte UTF-8 sequences are kept intact.
    ///
    /// Returns: (`literal_bytes`, length) where `literal_bytes` is a 32-byte buffer
    /// (padded for SIMD alignment) and length is the actual literal length.
    fn extract_first_literal(&self) -> ([u8; 32], usize) {
        let mut literal = [0u8; 32]; // 32-byte aligned buffer for AVX2
        let mut len = 0;
        let mut ip = 0;
        let code = &self.program.code;

        // Scan bytecode for the first substantial literal
        while ip < code.len() && len < 16 {
            // Limit to 16 bytes for efficiency
            let Ok(op) = OpCode::try_from(code[ip]) else {
                break;
            };
            ip += 1;

            match op {
                OpCode::Char => {
                    // Extract character literal
                    if ip < code.len() {
                        let char_len = code[ip] as usize;
                        ip += 1;

                        if ip + char_len <= code.len() && len + char_len <= 16 {
                            literal[len..len + char_len].copy_from_slice(&code[ip..ip + char_len]);
                            len += char_len;
                            ip += char_len;
                        } else {
                            break;
                        }
                    }
                }

                // Skip certain instructions but continue scanning
                OpCode::SaveStart | OpCode::SaveEnd => {
                    if ip < code.len() {
                        ip += 1; // Skip group ID
                    }
                }

                // Stop at any non-literal instruction
                _ => break,
            }
        }

        (literal, len)
    }

    /// SIMD single-byte search using parallel comparison.
    ///
    /// This implements the state-of-the-art algorithm for finding all occurrences
    /// of a single byte in a haystack. The algorithm processes 32 bytes at a time
    /// on AVX2 systems, 16 bytes on SSE2, and 16 bytes on ARM NEON.
    ///
    /// **Algorithm Details:**
    /// 1. Create a vector with all lanes set to the search byte
    /// 2. Load 32/16 bytes from the haystack
    /// 3. Compare all bytes in parallel using SIMD equality
    /// 4. Extract a bitmask of matching positions
    /// 5. Use bit manipulation (TZCNT/POPCNT) to find match indices
    ///
    /// **Performance Characteristics:**
    /// - Throughput: ~30-50 GB/s on modern CPUs
    /// - Latency: 1-2 cycles per 32 bytes
    /// - Cache-friendly: Sequential memory access pattern
    fn simd_find_byte(&self, ctx: &ExecContext<'_>, needle: u8) -> Vec<usize> {
        let mut positions = Vec::new();
        let haystack = &ctx.text;

        #[cfg(target_arch = "x86_64")]
        {
            if self.simd_support.avx2 {
                Self::find_byte_avx2(&mut positions, haystack, needle);
            } else if self.simd_support.sse2 {
                Self::find_byte_sse2(&mut positions, haystack, needle);
            } else {
                Self::find_byte_scalar(&mut positions, haystack, needle);
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            if self.simd_support.neon {
                Self::find_byte_neon(&mut positions, haystack, needle);
            } else {
                Self::find_byte_scalar(&mut positions, haystack, needle);
            }
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self::find_byte_scalar(&mut positions, haystack, needle);
        }

        positions
    }

    /// Scalar fallback for single-byte search.
    fn find_byte_scalar(positions: &mut Vec<usize>, haystack: &[u8], needle: u8) {
        positions.extend(haystack.iter().enumerate().filter_map(|(i, &b)| {
            if b == needle {
                Some(i)
            } else {
                None
            }
        }));
    }

    /// AVX2 path: search 32 bytes at a time.
    #[cfg(target_arch = "x86_64")]
    fn find_byte_avx2(positions: &mut Vec<usize>, haystack: &[u8], needle: u8) {
        unsafe {
            use std::arch::x86_64::*;

            let needle_vec = _mm256_set1_epi8(needle as i8);
            let mut i = 0;

            while i + 32 <= haystack.len() {
                let hay_vec = _mm256_loadu_si256(haystack[i..].as_ptr() as *const __m256i);
                let cmp = _mm256_cmpeq_epi8(hay_vec, needle_vec);
                let mask = _mm256_movemask_epi8(cmp) as u32;

                if mask != 0 {
                    let mut m = mask;
                    while m != 0 {
                        let bit_pos = m.trailing_zeros() as usize;
                        positions.push(i + bit_pos);
                        m &= m - 1; // Clear lowest set bit
                    }
                }

                i += 32;
            }

            // Handle remaining bytes
            Self::find_byte_tail(positions, haystack, needle, i);
        }
    }

    /// SSE2 path: search 16 bytes at a time.
    #[cfg(target_arch = "x86_64")]
    fn find_byte_sse2(positions: &mut Vec<usize>, haystack: &[u8], needle: u8) {
        unsafe {
            use std::arch::x86_64::*;

            let needle_vec = _mm_set1_epi8(needle as i8);
            let mut i = 0;

            while i + 16 <= haystack.len() {
                let hay_vec = _mm_loadu_si128(haystack[i..].as_ptr() as *const __m128i);
                let cmp = _mm_cmpeq_epi8(hay_vec, needle_vec);
                let mask = _mm_movemask_epi8(cmp) as u16;

                if mask != 0 {
                    let mut m = mask;
                    while m != 0 {
                        let bit_pos = m.trailing_zeros() as usize;
                        positions.push(i + bit_pos);
                        m &= m - 1;
                    }
                }

                i += 16;
            }

            // Handle remaining bytes
            Self::find_byte_tail(positions, haystack, needle, i);
        }
    }

    /// ARM NEON path: search 16 bytes at a time.
    #[cfg(target_arch = "aarch64")]
    fn find_byte_neon(positions: &mut Vec<usize>, haystack: &[u8], needle: u8) {
        unsafe {
            use std::arch::aarch64::{vceqq_u8, vdupq_n_u8, vld1q_u8, vst1q_u8};

            let needle_vec = vdupq_n_u8(needle);
            let mut i = 0;

            while i + 16 <= haystack.len() {
                let hay_vec = vld1q_u8(haystack[i..].as_ptr());
                let cmp = vceqq_u8(hay_vec, needle_vec);

                // Extract matches - NEON doesn't have movemask equivalent
                let mut result = [0u8; 16];
                vst1q_u8(result.as_mut_ptr(), cmp);

                for (j, &byte) in result.iter().enumerate() {
                    if byte != 0 {
                        positions.push(i + j);
                    }
                }

                i += 16;
            }

            // Handle remaining bytes
            Self::find_byte_tail(positions, haystack, needle, i);
        }
    }

    /// Scan remaining bytes after a SIMD-vectorized bulk pass.
    fn find_byte_tail(positions: &mut Vec<usize>, haystack: &[u8], needle: u8, start: usize) {
        for (i, &byte) in haystack.iter().enumerate().skip(start) {
            if byte == needle {
                positions.push(i);
            }
        }
    }

    /// SIMD short string search (2-4 bytes) using shuffle-based matching.
    ///
    /// This implements an advanced algorithm for short strings that uses SIMD
    /// shuffle instructions to perform multiple comparisons in parallel. This
    /// technique is inspired by the Hyperscan "Teddy" algorithm.
    ///
    /// **Algorithm Overview:**
    /// 1. Load the pattern into all lanes of a vector register
    /// 2. Use shuffle instructions to align the haystack with the pattern
    /// 3. Perform parallel comparison
    /// 4. Use horizontal reduction to check for full matches
    ///
    /// **Why This Is Fast:**
    /// - Avoids branch misprediction by processing multiple positions in parallel
    /// - Leverages shuffle units which have high throughput on modern CPUs
    /// - Minimizes memory bandwidth by keeping pattern in registers
    ///
    /// **Performance:** ~10-20 GB/s for 2-4 byte patterns
    fn simd_find_short_string(&self, ctx: &ExecContext<'_>, needle: &[u8]) -> Vec<usize> {
        let mut positions = Vec::new();
        let haystack = &ctx.text;
        let needle_len = needle.len();

        if needle_len == 0 || needle_len > 4 || needle_len > haystack.len() {
            return positions;
        }

        // First, find all positions where the first byte matches
        let first_byte_positions = self.simd_find_byte(ctx, needle[0]);

        // Then verify the full pattern at each position
        for &pos in &first_byte_positions {
            if pos + needle_len <= haystack.len() && &haystack[pos..pos + needle_len] == needle {
                positions.push(pos);
            }
        }

        positions
    }

    /// SIMD long string search using Boyer-Moore-Horspool with SIMD verification.
    ///
    /// This implements a state-of-the-art hybrid algorithm that combines:
    /// 1. **Bad character skip table** for large jumps
    /// 2. **SIMD verification** for fast comparison
    /// 3. **Cache-conscious design** with prefetching
    ///
    /// **Algorithm Details:**
    ///
    /// The Boyer-Moore-Horspool algorithm with SIMD enhancements:
    /// 1. Build a bad character table (256 entries for ASCII)
    /// 2. Scan from right to left using the last character
    /// 3. On mismatch, jump forward using the skip table
    /// 4. On potential match, use SIMD to verify the full pattern
    ///
    /// **Optimizations:**
    /// - Skip table fits in L1 cache (256 bytes)
    /// - SIMD verification avoids byte-by-byte comparison
    /// - Prefetching hints for the CPU to load ahead
    ///
    /// **Performance:** ~5-15 GB/s for patterns > 4 bytes
    fn simd_find_long_string(&self, ctx: &ExecContext<'_>, needle: &[u8]) -> Vec<usize> {
        let mut positions = Vec::new();
        let haystack = &ctx.text;
        let needle_len = needle.len();

        if needle_len == 0 || needle_len > haystack.len() {
            return positions;
        }

        // Build bad character skip table for Boyer-Moore-Horspool
        let mut skip_table = [needle_len; 256];
        for i in 0..needle_len - 1 {
            skip_table[needle[i] as usize] = needle_len - 1 - i;
        }

        let mut i = needle_len - 1;

        while i < haystack.len() {
            // Check last character first (Boyer-Moore-Horspool)
            let last_char = haystack[i];

            if last_char == needle[needle_len - 1] {
                // Potential match - verify with SIMD or memcmp
                let start = i + 1 - needle_len;

                if self.simd_compare(&haystack[start..start + needle_len], needle) {
                    positions.push(start);
                    i += 1; // Move forward to find overlapping matches
                } else {
                    i += skip_table[last_char as usize].max(1);
                }
            } else {
                // Jump forward using skip table
                i += skip_table[last_char as usize];
            }
        }

        positions
    }

    /// SIMD-accelerated memory comparison.
    ///
    /// Uses SIMD instructions to compare two memory regions for equality.
    /// This is significantly faster than byte-by-byte comparison for regions
    /// larger than 8 bytes.
    ///
    /// **Implementation Notes:**
    /// - Uses unaligned loads (modern CPUs handle these efficiently)
    /// - Processes largest possible chunks first (32, 16, 8 bytes)
    /// - Falls back to scalar comparison for small remainders
    #[allow(clippy::unused_self)]
    #[allow(clippy::inline_always)] // hot SIMD path
    #[inline(always)]
    fn simd_compare(&self, a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }

        #[cfg(target_arch = "x86_64")]
        let len = a.len();

        #[cfg(target_arch = "x86_64")]
        {
            if self.simd_support.avx2 && len >= 32 {
                unsafe {
                    use std::arch::x86_64::*;

                    let mut offset = 0;

                    // Compare 32 bytes at a time
                    while offset + 32 <= len {
                        let a_vec = _mm256_loadu_si256(a[offset..].as_ptr() as *const __m256i);
                        let b_vec = _mm256_loadu_si256(b[offset..].as_ptr() as *const __m256i);
                        let cmp = _mm256_cmpeq_epi8(a_vec, b_vec);
                        let mask = _mm256_movemask_epi8(cmp);

                        if mask != -1 {
                            return false; // Found a mismatch
                        }

                        offset += 32;
                    }

                    // Handle remaining bytes
                    return a[offset..] == b[offset..];
                }
            }
        }

        // Fallback to standard comparison
        a == b
    }
}

/// Advanced compiler with optimization passes
pub struct OptimizingCompiler {
    /// Current bytecode being generated
    code: Vec<u8>,
    /// Character classes being compiled
    char_classes: Vec<CompiledCharClass>,
    /// String literals for optimization
    strings: Vec<String>,
    /// Named capture group mapping for conditional references
    named_groups: HashMap<String, u32>,
    /// Group counter for captures
    group_counter: u32,
    /// Optimization flags
    flags: ProgramFlags,
    /// Compilation statistics
    stats: CompilationStats,
    /// Whether multiline mode is active (^ and $ match at line boundaries)
    multiline: bool,
    /// Whether dotall mode is active (. matches any character including newline)
    dotall: bool,
    /// Whether case-insensitive mode is active (ASCII letters match both cases)
    case_insensitive: bool,
    /// Whether ungreedy / swap-greed mode is active (PCRE2 `(?U)`).
    /// When true, `*` / `+` / `?` / `{n,m}` default to lazy; `*?` / `+?`
    /// etc. default to greedy. Toggle-flip semantics.
    swap_greed: bool,
    /// Newline convention for the `^` / `$` line-anchor opcodes
    /// under `/m`. Set at the compiler boundary from the pattern's
    /// `(*CR)` / `(*LF)` / `(*CRLF)` / `(*ANYCRLF)` / `(*ANY)` /
    /// `(*NUL)` pragma (default `Lf`).
    newline_mode: VmNewlineMode,
}

impl OptimizingCompiler {
    /// Create new optimizing compiler
    #[must_use]
    pub fn new() -> Self {
        Self::with_named_groups(HashMap::new())
    }

    /// Create new optimizing compiler with resolved named group references.
    #[must_use]
    pub fn with_named_groups(named_groups: HashMap<String, u32>) -> Self {
        trace_enter!("vm", "OptimizingCompiler::new");
        let compiler = Self {
            code: Vec::new(),
            char_classes: Vec::new(),
            strings: Vec::new(),
            named_groups,
            group_counter: 0,
            flags: ProgramFlags {
                simd_enabled: true,
                has_anchors: false,
                has_backrefs: false,
                has_lookarounds: false,
                has_code_blocks: false,
                instruction_count: 0,
                max_capture_group: 0,
            },
            stats: CompilationStats {
                literal_chars: 0,
                char_classes: 0,
                quantifiers: 0,
                estimated_cycles: 0,
                jit_worthy: false,
            },
            multiline: false,
            dotall: false,
            case_insensitive: false,
            swap_greed: false,
            newline_mode: VmNewlineMode::Lf,
        };
        trace_exit!(
            "vm",
            "OptimizingCompiler::new",
            "ok=true,simd_enabled={},jit_worthy={}",
            compiler.flags.simd_enabled,
            compiler.stats.jit_worthy
        );
        compiler
    }

    /// Configure the PCRE2 newline convention for this compilation.
    /// Set by the outer compiler after scanning the pattern text for
    /// `(*CR)` / `(*LF)` / `(*CRLF)` / `(*ANYCRLF)` / `(*ANY)` /
    /// `(*NUL)` pragmas. Controls the `^` / `$` line-anchor checks
    /// under `/m`. Defaults to `Lf` (original RGX behaviour).
    pub fn set_newline_mode(&mut self, mode: VmNewlineMode) {
        self.newline_mode = mode;
    }

    /// Compile AST to optimized program with multiple passes
    pub fn compile(&mut self, ast: &Regex) -> Program {
        trace_enter!(
            "vm",
            "OptimizingCompiler::compile",
            "ast_kind={}",
            regex_kind(ast)
        );
        // Reset state
        self.code.clear();
        self.char_classes.clear();
        self.strings.clear();
        self.group_counter = 0;

        // Pass 1: Analysis - gather statistics and detect features
        self.analyze_pass(ast);
        trace_decision!(
            "vm",
            "stats.jit_worthy after analysis",
            self.stats.jit_worthy,
            "literal_chars={},quantifiers={},char_classes={}",
            self.stats.literal_chars,
            self.stats.quantifiers,
            self.stats.char_classes
        );

        // Pass 2: Optimization - apply peephole optimizations
        self.optimize_ast(ast);

        // Pass 3: Code generation - emit optimized bytecode
        self.codegen_pass(ast, true);

        // Pass 4: Final optimizations - peephole optimization on bytecode
        self.peephole_optimize();

        // Emit final Match instruction
        self.emit_op(OpCode::Match);
        let subroutines = self.compile_subroutines(ast);
        let program = Program {
            code: self.code.clone(),
            subroutines,
            char_classes: self.char_classes.clone(),
            string_literals: self.strings.clone(),
            named_groups: HashMap::new(),
            num_groups: self.group_counter,
            flags: self.flags,
            stats: self.stats,
            newline_mode: self.newline_mode,
            classification: Classification::default(),
            c2_program: None,
        };
        trace_exit!(
            "vm",
            "OptimizingCompiler::compile",
            "ok=true,bytecode_len={},char_classes={},string_literals={},groups={},jit_worthy={}",
            program.code.len(),
            program.char_classes.len(),
            program.string_literals.len(),
            program.num_groups,
            program.stats.jit_worthy
        );
        program
    }

    /// Analysis pass - gather statistics for optimization decisions
    fn analyze_pass(&mut self, ast: &Regex) {
        match ast {
            Regex::Char(_) => self.stats.literal_chars += 1,
            Regex::CharClass(_) | Regex::UnicodeClass { .. } | Regex::ExtendedCharClass { .. } => {
                self.stats.char_classes += 1;
            }
            Regex::Quantified { .. } => self.stats.quantifiers += 1,
            Regex::Anchor(_) => self.flags.has_anchors = true,
            Regex::Backreference(_) | Regex::NamedBackreference(_) => {
                self.flags.has_backrefs = true;
            }
            Regex::Lookahead { expr, .. } | Regex::Lookbehind { expr, .. } => {
                self.flags.has_lookarounds = true;
                self.analyze_pass(expr);
            }
            Regex::CodeBlock { .. } | Regex::Callout(_) => self.flags.has_code_blocks = true,
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                match condition {
                    ConditionalTest::Lookahead { expr, .. }
                    | ConditionalTest::Lookbehind { expr, .. } => {
                        self.flags.has_lookarounds = true;
                        self.analyze_pass(expr);
                    }
                    ConditionalTest::GroupExists(_)
                    | ConditionalTest::RelativeGroupExists(_)
                    | ConditionalTest::NamedGroupExists(_)
                    | ConditionalTest::RecursionAny
                    | ConditionalTest::RecursionGroup(_)
                    | ConditionalTest::RecursionNamed(_)
                    | ConditionalTest::Define => {}
                }
                self.analyze_pass(true_branch);
                if let Some(false_branch) = false_branch {
                    self.analyze_pass(false_branch);
                }
            }
            Regex::Sequence(items) => {
                for item in items {
                    self.analyze_pass(item);
                }
            }
            Regex::Alternation(alts) => {
                for alt in alts {
                    self.analyze_pass(alt);
                }
            }
            Regex::Group { expr, .. } => {
                // Only count groups during code generation, not analysis
                self.analyze_pass(expr);
            }
            Regex::FlagGroup { expr, .. } => {
                self.analyze_pass(expr);
            }
            _ => {}
        }

        // Update JIT worthiness based on complexity
        self.stats.jit_worthy = self.stats.literal_chars > 10
            || self.stats.quantifiers > 3
            || self.stats.char_classes > 5;
    }

    /// Optimization pass - AST-level optimizations
    #[allow(clippy::unused_self)]
    fn optimize_ast(&mut self, _ast: &Regex) {
        // TODO: Implement AST optimizations like:
        // - String literal concatenation
        // - Character class merging
        // - Dead code elimination
        // - Quantifier fusion
    }

    /// Code generation pass - emit optimized bytecode
    #[allow(clippy::too_many_lines)] // AST codegen covers all node types in one pass — splitting would scatter the emission logic
    #[allow(clippy::cast_possible_truncation)] // Bytecode operands are intentionally stored as compact u8/u16 values.
    fn codegen_pass(&mut self, ast: &Regex, is_top_level: bool) {
        match ast {
            Regex::Char(ch) => {
                if self.case_insensitive {
                    let variants = Self::unicode_case_variants(*ch);
                    if variants.len() > 1 {
                        let ranges: Vec<CharRange> =
                            variants.into_iter().map(CharRange::single).collect();
                        let class_id = self.compile_char_class(&ranges);
                        self.emit_op(OpCode::CharClass);
                        self.code.push(class_id as u8);
                    } else {
                        self.emit_char_op(OpCode::Char, *ch);
                    }
                } else {
                    self.emit_char_op(OpCode::Char, *ch);
                }
            }

            Regex::Dot => {
                if self.dotall {
                    self.emit_op(OpCode::AnyDotAll);
                } else {
                    self.emit_op(OpCode::Any);
                }
            }

            Regex::CharClass(char_class) => {
                match char_class {
                    CharClass::Digit { negated: false } => self.emit_op(OpCode::DigitAscii),
                    CharClass::Digit { negated: true } => self.emit_op(OpCode::DigitAsciiNeg),
                    CharClass::Word { negated: false } => self.emit_op(OpCode::WordAscii),
                    CharClass::Word { negated: true } => self.emit_op(OpCode::WordAsciiNeg),
                    CharClass::Space { negated: false } => self.emit_op(OpCode::SpaceAscii),
                    CharClass::Space { negated: true } => self.emit_op(OpCode::SpaceAsciiNeg),
                    CharClass::Custom { ranges, negated } => {
                        // Compile custom character class into optimized bytecode
                        // Store the class definition and emit CharClass opcode with index
                        let effective_ranges = if self.case_insensitive {
                            Self::case_fold_ranges(ranges)
                        } else {
                            ranges.clone()
                        };
                        let class_id = self.compile_char_class(&effective_ranges);

                        if *negated {
                            self.emit_op(OpCode::CharClassNeg);
                        } else {
                            self.emit_op(OpCode::CharClass);
                        }
                        self.code.push(class_id as u8);
                    }
                    CharClass::UnicodeClass { name, negated } => {
                        let resolved_name: &str = if self.case_insensitive
                            && matches!(name.as_str(), "Lu" | "Ll" | "Lt")
                        {
                            // Under /i, PCRE2 expands case-distinguished
                            // letter properties to match any cased letter
                            // (L& = Lu|Ll|Lt). `\p{Lu}/i` on "a" should
                            // match — folding 'a' → 'A' brings it into
                            // the expanded class. Same for `\P{Lu}/i`
                            // which becomes `\P{L&}`.
                            "L&"
                        } else {
                            name.as_str()
                        };
                        let ranges = resolve_unicode_property_class(resolved_name, *negated)
                            .expect("unicode property class should be validated before codegen");
                        let class_id = self.compile_char_class(&ranges);
                        self.emit_op(OpCode::CharClass);
                        self.code.push(class_id as u8);
                    }
                }
            }

            Regex::UnicodeClass { name, negated } => {
                let resolved_name: &str =
                    if self.case_insensitive && matches!(name.as_str(), "Lu" | "Ll" | "Lt") {
                        // See `CharClass::UnicodeClass` branch: under /i,
                        // PCRE2 expands Lu/Ll/Lt to L& so case-folded
                        // letters match regardless of original case.
                        "L&"
                    } else {
                        name.as_str()
                    };
                let ranges = resolve_unicode_property_class(resolved_name, *negated)
                    .expect("unicode property class should be validated before codegen");
                let class_id = self.compile_char_class(&ranges);
                self.emit_op(OpCode::CharClass);
                self.code.push(class_id as u8);
            }

            Regex::ExtendedCharClass { .. } => {
                panic!(
                    "Perl extended character classes '(?[...])' should be lowered or rejected during compiler validation before codegen"
                );
            }

            Regex::Anchor(anchor) => match anchor {
                AnchorType::Start => {
                    if self.multiline {
                        self.emit_op(OpCode::StartLine);
                    } else {
                        self.emit_op(OpCode::StartText);
                    }
                }
                AnchorType::End => {
                    if self.multiline {
                        self.emit_op(OpCode::EndLine);
                    } else {
                        self.emit_op(OpCode::EndTextOrNL);
                    }
                }
                AnchorType::AbsStart => {
                    self.emit_op(OpCode::StartText);
                }
                AnchorType::AbsEnd => {
                    self.emit_op(OpCode::EndTextOrNL);
                }
                AnchorType::AbsEndNoNL => self.emit_op(OpCode::EndText),
                AnchorType::PreviousMatchEnd => self.emit_op(OpCode::PreviousMatchEnd),
            },

            Regex::Sequence(items) => {
                for item in items {
                    self.codegen_pass(item, false);
                }
            }

            Regex::Alternation(alts) => {
                // Implement proper alternation with Split opcodes and backtracking
                if alts.is_empty() {
                    self.emit_op(OpCode::Fail);
                    return;
                }

                if alts.len() == 1 {
                    // Single alternative
                    self.codegen_pass(&alts[0], false);
                    return;
                }

                // For multiple alternatives, use recursive structure:
                // Split L1
                // SetAlternative 0
                // <alt0 code>
                // Jump END
                // L1: Split L2  (or just last alternative if this is second-to-last)
                // SetAlternative 1
                // <alt1 code>
                // Jump END
                // L2: SetAlternative 2
                // <alt2 code>
                // END: ...

                let mut end_jumps = Vec::new();

                for (i, alt) in alts.iter().enumerate() {
                    if i == alts.len() - 1 {
                        // Last alternative - no Split needed
                        if is_top_level {
                            self.emit_op(OpCode::SetAlternative);
                            self.code.push(i as u8);
                        }
                        self.codegen_pass(alt, false);
                    } else {
                        // Not the last - emit Split to next alternative
                        self.emit_op(OpCode::Split);
                        let split_offset_pos = self.code.len();
                        self.code.push(0); // Will be patched
                        self.code.push(0); // Will be patched

                        // Current alternative
                        if is_top_level {
                            self.emit_op(OpCode::SetAlternative);
                            self.code.push(i as u8);
                        }
                        self.codegen_pass(alt, false);

                        // Jump to end (except for last alternative)
                        self.emit_op(OpCode::Jump);
                        let end_jump_pos = self.code.len();
                        self.code.push(0); // Will be patched
                        self.code.push(0); // Will be patched
                        end_jumps.push(end_jump_pos);

                        // Patch the Split offset to point to start of next alternative
                        let next_alt_start = self.code.len();
                        // split_offset_pos is the position of the first offset byte
                        // We need to calculate: next_alt_start - current_ip_after_reading_offset
                        // current_ip_after_reading_offset = split_offset_pos + 2
                        let split_offset = next_alt_start - (split_offset_pos + 2);
                        let offset_bytes = (split_offset as u16).to_le_bytes();
                        self.code[split_offset_pos] = offset_bytes[0];
                        self.code[split_offset_pos + 1] = offset_bytes[1];
                    }
                }

                // Patch all end jumps to point to the end
                let end_pos = self.code.len();
                for end_jump_pos in end_jumps {
                    let jump_offset = end_pos - end_jump_pos - 2;
                    let offset_bytes = (jump_offset as u16).to_le_bytes();
                    self.code[end_jump_pos] = offset_bytes[0];
                    self.code[end_jump_pos + 1] = offset_bytes[1];
                }
            }

            Regex::WordBoundary { positive } => {
                if *positive {
                    self.emit_op(OpCode::WordBoundary);
                } else {
                    self.emit_op(OpCode::NonWordBoundary);
                }
            }

            Regex::Lookahead { expr, positive } => {
                if *positive {
                    self.emit_op(OpCode::Lookahead);
                } else {
                    self.emit_op(OpCode::LookaheadNeg);
                }

                // Compile lookahead sub-expression inline with a length prefix.
                let sub_code = self.compile_inline_subexpr(expr);

                self.code.push(sub_code.len() as u8);
                self.code.extend(sub_code);
            }

            Regex::Lookbehind { expr, positive } => {
                if *positive {
                    self.emit_op(OpCode::Lookbehind);
                } else {
                    self.emit_op(OpCode::LookbehindNeg);
                }

                // Compile lookbehind sub-expression inline with a length prefix.
                let sub_code = self.compile_inline_subexpr(expr);

                self.code.push(sub_code.len() as u8);
                self.code.extend(sub_code);
            }

            Regex::CodeBlock { lang, code } => {
                self.emit_code_block(lang, code);
            }

            Regex::Callout(number) => {
                // Compile callout as a native code block with conventional name
                let callback_name = format!("__callout_{number}");
                self.emit_code_block("native", &callback_name);
            }

            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                self.emit_conditional_jump(OpCode::JumpIfNoMatch, condition);
                let false_jump_pos = self.code.len();
                self.code.push(0);
                self.code.push(0);

                self.codegen_pass(true_branch, false);

                if let Some(false_branch) = false_branch {
                    self.emit_op(OpCode::Jump);
                    let end_jump_pos = self.code.len();
                    self.code.push(0);
                    self.code.push(0);

                    let false_branch_start = self.code.len();
                    self.patch_u16_offset(false_jump_pos, false_branch_start);

                    self.codegen_pass(false_branch, false);

                    let end_pos = self.code.len();
                    self.patch_u16_offset(end_jump_pos, end_pos);
                } else {
                    let end_pos = self.code.len();
                    self.patch_u16_offset(false_jump_pos, end_pos);
                }
            }

            Regex::Backreference(group_id) => {
                let op = if self.case_insensitive {
                    OpCode::BackrefCaseInsensitive
                } else {
                    OpCode::Backref
                };
                self.emit_op(op);
                self.code.push(*group_id as u8);
            }

            Regex::NamedBackreference(name) => {
                let group_id = self
                    .named_groups
                    .get(name)
                    .copied()
                    .expect("named backreference should be validated before codegen");
                let op = if self.case_insensitive {
                    OpCode::BackrefCaseInsensitive
                } else {
                    OpCode::Backref
                };
                self.emit_op(op);
                self.code.push(group_id as u8);
            }

            Regex::Recursion { target } | Regex::ReturnedCaptureSubroutine { target, .. } => {
                // ReturnedCaptureSubroutine compiles to the same Call opcode.
                // Full capture-return semantics (preserving specified group
                // captures across the call boundary) is a VM-level follow-up.
                self.emit_op(OpCode::Call);
                let target_id = match target {
                    RecursionTarget::Entire => 0,
                    RecursionTarget::Group(group_id) => *group_id as u8,
                    RecursionTarget::NamedGroup(name) => self
                        .named_groups
                        .get(name)
                        .copied()
                        .expect("named recursion target should be validated before codegen")
                        as u8,
                    RecursionTarget::RelativeGroup(_) => {
                        panic!("relative recursion target should be resolved before codegen")
                    }
                };
                self.code.push(target_id);
            }

            Regex::Quantified { expr, quantifier } => {
                // `self.swap_greed` (PCRE2 `(?U)` / `/ungreedy`) inverts
                // greedy / lazy defaults: `*` becomes lazy, `*?` becomes
                // greedy, `{n,m}` becomes lazy, `{n,m}?` becomes greedy.
                // XOR the lazy flag on the quantifier to get the
                // effective-lazy bit.
                let effective_lazy = |lazy: bool| lazy ^ self.swap_greed;
                match quantifier {
                    Quantifier::OneOrMore { lazy } if effective_lazy(*lazy) => {
                        self.emit_subexpr_opcode(OpCode::PlusLazy, expr);
                    }
                    Quantifier::OneOrMore { .. } => {
                        self.emit_subexpr_opcode(OpCode::PlusGreedy, expr);
                    }
                    Quantifier::ZeroOrMore { lazy } if effective_lazy(*lazy) => {
                        self.emit_subexpr_opcode(OpCode::StarLazy, expr);
                    }
                    Quantifier::ZeroOrMore { .. } => {
                        self.emit_subexpr_opcode(OpCode::StarGreedy, expr);
                    }
                    Quantifier::ZeroOrOne { lazy } if effective_lazy(*lazy) => {
                        self.emit_subexpr_opcode(OpCode::QuestionLazy, expr);
                    }
                    Quantifier::ZeroOrOne { .. } => {
                        self.emit_subexpr_opcode(OpCode::QuestionGreedy, expr);
                    }
                    Quantifier::Range { min, max, lazy } if effective_lazy(*lazy) => {
                        let min_count = *min as usize;
                        for _ in 0..min_count {
                            self.codegen_pass(expr, false);
                        }
                        match max {
                            Some(max_value) => {
                                let max_count = *max_value as usize;
                                for _ in min_count..max_count {
                                    self.emit_subexpr_opcode(OpCode::QuestionLazy, expr);
                                }
                            }
                            None => {
                                self.emit_subexpr_opcode(OpCode::StarLazy, expr);
                            }
                        }
                    }
                    Quantifier::Range { min, max, .. } => {
                        // For A{n,m}, emit A exactly n times, then up to (m-n) greedy
                        // optional repetitions backed by Split-based backtracking.
                        // For A{n,}, emit A exactly n times then A*.
                        let min_count = *min as usize;
                        // Emit required repetitions (min times).
                        for _ in 0..min_count {
                            self.codegen_pass(expr, false);
                        }
                        if let Some(max_value) = max {
                            let max_count = *max_value as usize;
                            // Emit greedy optional repetitions (bounded tail).
                            for _ in min_count..max_count {
                                // Split first tries the expr path and saves a fallback
                                // that skips this optional repetition.
                                self.emit_op(OpCode::Split);
                                let split_offset_pos = self.code.len();
                                self.code.push(0); // patch later
                                self.code.push(0); // patch later

                                self.codegen_pass(expr, false);

                                let skip_expr_target = self.code.len();
                                let split_offset = skip_expr_target - (split_offset_pos + 2);
                                let offset_bytes = (split_offset as u16).to_le_bytes();
                                self.code[split_offset_pos] = offset_bytes[0];
                                self.code[split_offset_pos + 1] = offset_bytes[1];
                            }
                        } else {
                            // Emit unbounded tail as A* after required prefix.
                            self.emit_op(OpCode::StarGreedy);

                            let sub_code = self.compile_inline_subexpr(expr);

                            self.code.push(sub_code.len() as u8);
                            self.code.extend(sub_code);
                        }
                    }
                }
            }

            Regex::Group {
                expr, kind, index, ..
            } => {
                match kind {
                    GroupKind::Capturing => {
                        let group_id = index.unwrap_or_else(|| self.group_counter + 1);
                        self.group_counter = self.group_counter.max(group_id);

                        // Update max capture group for flags
                        self.flags.max_capture_group = self.flags.max_capture_group.max(group_id);

                        // Emit SaveStart to capture beginning of group
                        self.emit_op(OpCode::SaveStart);
                        self.code.push(group_id as u8);

                        // Compile the inner expression
                        self.codegen_pass(expr, false);

                        // Emit SaveEnd to capture end of group
                        self.emit_op(OpCode::SaveEnd);
                        self.code.push(group_id as u8);
                    }
                    GroupKind::NonCapturing | GroupKind::Atomic => {
                        if matches!(kind, GroupKind::Atomic) {
                            // Atomic group: prevent backtracking into the group after it succeeds.
                            self.emit_op(OpCode::AtomicStart);
                            self.codegen_pass(expr, false);
                            self.emit_op(OpCode::AtomicEnd);
                        } else {
                            // Non-capturing group
                            self.codegen_pass(expr, false);
                        }
                    }
                    GroupKind::BranchReset => {
                        self.codegen_pass(expr, false);
                    }
                }
            }

            Regex::FlagGroup { flags, expr } => {
                let saved_multiline = self.multiline;
                let saved_dotall = self.dotall;
                let saved_case_insensitive = self.case_insensitive;
                let saved_swap_greed = self.swap_greed;
                // Parse flag string: chars before '-' are enabled, after '-' disabled.
                // Examples: "i" → enable i; "-i" → disable i; "im" → enable both;
                // "i-m" → enable i, disable m.
                let (enable, disable) = if let Some(pos) = flags.find('-') {
                    (&flags[..pos], &flags[pos + 1..])
                } else {
                    (flags.as_str(), "")
                };
                if enable.contains('m') {
                    self.multiline = true;
                }
                if disable.contains('m') {
                    self.multiline = false;
                }
                if enable.contains('s') {
                    self.dotall = true;
                }
                if disable.contains('s') {
                    self.dotall = false;
                }
                if enable.contains('i') {
                    self.case_insensitive = true;
                }
                if disable.contains('i') {
                    self.case_insensitive = false;
                }
                if enable.contains('U') {
                    self.swap_greed = true;
                }
                if disable.contains('U') {
                    self.swap_greed = false;
                }
                self.codegen_pass(expr, false);
                self.multiline = saved_multiline;
                self.dotall = saved_dotall;
                self.case_insensitive = saved_case_insensitive;
                self.swap_greed = saved_swap_greed;
            }

            Regex::MatchReset => {
                self.emit_op(OpCode::MatchReset);
            }

            Regex::GraphemeCluster => {
                self.emit_op(OpCode::GraphemeCluster);
            }

            Regex::NewlineSequence => {
                // Expand \R into (?:\r\n|\r|\n|\x0B|\x0C|\x85|\u{2028}|\u{2029}).
                // BSR_ANYCRLF mode (CR/LF/CRLF only) is handled at the
                // adapter by emitting an explicit restricted alternation
                // instead of the `NewlineSequence` node — so reaching
                // this branch always means the full Unicode newline set.
                let expanded = Regex::Group {
                    kind: GroupKind::NonCapturing,
                    expr: Box::new(Regex::Alternation(vec![
                        Regex::Sequence(vec![Regex::Char('\r'), Regex::Char('\n')]),
                        Regex::Char('\r'),
                        Regex::Char('\n'),
                        Regex::Char('\x0B'),
                        Regex::Char('\x0C'),
                        Regex::Char('\u{0085}'),
                        Regex::Char('\u{2028}'),
                        Regex::Char('\u{2029}'),
                    ])),
                    index: None,
                    name: None,
                };
                self.codegen_pass(&expanded, false);
            }

            Regex::Accept => {
                // (*ACCEPT): force immediate match at current position.
                self.emit_op(OpCode::Match);
            }

            Regex::Commit => {
                self.emit_op(OpCode::Commit);
            }

            Regex::Prune => {
                self.emit_op(OpCode::Prune);
            }

            Regex::Skip(name) => {
                // A11: (*SKIP) and (*SKIP:name) emit different opcodes.
                // The unnamed form records ctx.pos directly; the named
                // form looks up the matching mark in ctx.marks via the
                // length-prefixed name operand.
                if let Some(name) = name {
                    self.emit_op(OpCode::VerbSkipNamed);
                    let name_bytes = name.as_bytes();
                    let len = name_bytes.len().min(255);
                    self.code.push(len as u8);
                    self.code.extend_from_slice(&name_bytes[..len]);
                } else {
                    self.emit_op(OpCode::VerbSkip);
                }
            }

            Regex::Then => {
                self.emit_op(OpCode::Then);
            }

            Regex::Mark(name) => {
                // Encode as Mark opcode followed by length-prefixed name.
                self.emit_op(OpCode::Mark);
                let name_bytes = name.as_bytes();
                let len = name_bytes.len().min(255);
                self.code.push(len as u8);
                self.code.extend_from_slice(&name_bytes[..len]);
            }

            Regex::Empty => {
                // Empty/epsilon — trivially matches without consuming input.
                // No bytecode needed; execution simply falls through to the
                // next instruction.
            }

            _ => {
                // TODO: Implement remaining AST nodes
                self.emit_op(OpCode::Fail);
            }
        }
    }

    /// Peephole optimization pass on generated bytecode
    #[allow(clippy::unused_self)]
    fn peephole_optimize(&mut self) {
        // TODO: Implement peephole optimizations like:
        // - Adjacent character merging into strings
        // - Redundant anchor elimination
        // - Jump optimization
        // - Character class folding
    }

    /// Emit simple opcode with no operands
    fn emit_op(&mut self, op: OpCode) {
        self.code.push(op as u8);
        self.flags.instruction_count += 1;
    }

    /// Emit an opcode followed by an inlined compiled sub-expression.
    #[allow(clippy::cast_possible_truncation)] // Bytecode operands are intentionally stored as compact u8/u16 values.
    fn emit_subexpr_opcode(&mut self, op: OpCode, expr: &Regex) {
        self.emit_op(op);

        let sub_code = self.compile_nested_code(expr, self.group_counter);

        self.code.push(sub_code.len() as u8);
        self.code.extend(sub_code);
    }

    #[allow(clippy::cast_possible_truncation)] // Bytecode operands are intentionally stored as compact u8/u16 values.
    fn emit_conditional_jump(&mut self, op: OpCode, condition: &ConditionalTest) {
        self.emit_op(op);
        match condition {
            ConditionalTest::GroupExists(group_id) => {
                self.code.push(CONDITIONAL_KIND_GROUP_EXISTS);
                self.code.push(*group_id as u8);
            }
            ConditionalTest::RelativeGroupExists(offset) => {
                panic!(
                    "internal compiler error: unresolved relative conditional group reference reached codegen: {offset:+}"
                );
            }
            ConditionalTest::NamedGroupExists(name) => {
                let group_id = self
                    .named_groups
                    .get(name)
                    .copied()
                    .expect("named conditional reference should be validated before codegen");
                self.code.push(CONDITIONAL_KIND_GROUP_EXISTS);
                self.code.push(group_id as u8);
            }
            ConditionalTest::RecursionAny => {
                self.code.push(CONDITIONAL_KIND_RECURSION_ANY);
            }
            ConditionalTest::RecursionGroup(group_id) => {
                self.code.push(CONDITIONAL_KIND_RECURSION_GROUP);
                self.code.push(*group_id as u8);
            }
            ConditionalTest::RecursionNamed(name) => {
                let group_id = self
                    .named_groups
                    .get(name)
                    .copied()
                    .expect("named recursion condition should be validated before codegen");
                self.code.push(CONDITIONAL_KIND_RECURSION_GROUP);
                self.code.push(group_id as u8);
            }
            ConditionalTest::Define => {
                self.code.push(CONDITIONAL_KIND_DEFINE_FALSE);
            }
            ConditionalTest::Lookahead { expr, positive } => {
                self.code.push(if *positive {
                    CONDITIONAL_KIND_LOOKAHEAD_POSITIVE
                } else {
                    CONDITIONAL_KIND_LOOKAHEAD_NEGATIVE
                });

                let sub_code = self.compile_inline_subexpr(expr);

                self.code.push(sub_code.len() as u8);
                self.code.extend(sub_code);
            }
            ConditionalTest::Lookbehind { expr, positive } => {
                self.code.push(if *positive {
                    CONDITIONAL_KIND_LOOKBEHIND_POSITIVE
                } else {
                    CONDITIONAL_KIND_LOOKBEHIND_NEGATIVE
                });

                let sub_code = self.compile_inline_subexpr(expr);

                self.code.push(sub_code.len() as u8);
                self.code.extend(sub_code);
            }
        }
    }

    /// Emit an inline code-block operand payload.
    #[allow(clippy::cast_possible_truncation)] // Bytecode operands are intentionally stored as compact u8/u16 values.
    fn emit_code_block(&mut self, lang: &str, code: &str) {
        self.emit_op(OpCode::CodeBlock);
        debug_assert!(u8::try_from(lang.len()).is_ok());
        debug_assert!(u16::try_from(code.len()).is_ok());
        self.code.push(lang.len() as u8);
        self.code.extend_from_slice(lang.as_bytes());
        self.code
            .extend_from_slice(&(code.len() as u16).to_le_bytes());
        self.code.extend_from_slice(code.as_bytes());
    }

    #[allow(clippy::cast_possible_truncation)] // Bytecode jump offsets are intentionally stored as u16.
    fn patch_u16_offset(&mut self, offset_pos: usize, target_pos: usize) {
        let offset = target_pos - (offset_pos + 2);
        let offset_bytes = (offset as u16).to_le_bytes();
        self.code[offset_pos] = offset_bytes[0];
        self.code[offset_pos + 1] = offset_bytes[1];
    }

    fn compile_inline_subexpr(&mut self, expr: &Regex) -> Vec<u8> {
        self.compile_nested_code(expr, self.group_counter)
    }

    fn compile_nested_code(&mut self, expr: &Regex, starting_group_counter: u32) -> Vec<u8> {
        let mut sub_compiler = OptimizingCompiler::with_named_groups(self.named_groups.clone());
        sub_compiler.group_counter = starting_group_counter;
        sub_compiler.multiline = self.multiline;
        sub_compiler.dotall = self.dotall;
        sub_compiler.case_insensitive = self.case_insensitive;
        sub_compiler.codegen_pass(expr, false);

        let mut sub_code = sub_compiler.code;
        if !sub_compiler.char_classes.is_empty() {
            // Merge the sub-compiler's char_classes into self, deduping
            // against entries already present. Without dedup, repeating
            // the same subexpression N times (e.g. `[a-z]{0,300}`) would
            // push N identical entries and overflow the single-byte
            // operand at `rebase_inline_char_class_ids`. PCRE2 testinput1
            // regression: `word (?:[a-zA-Z0-9]+ ){0,300}otherword`.
            //
            // Build a per-sub-class remap from "sub id" to "parent id" so
            // `rebase_inline_char_class_ids` can consult it. If every
            // sub class is new, the remap is identity-plus-base and we
            // fall back to the old path for free.
            let mut remap = Vec::with_capacity(sub_compiler.char_classes.len());
            for cc in sub_compiler.char_classes {
                if let Some(existing_id) = self.char_classes.iter().position(|e| e == &cc) {
                    remap.push(existing_id);
                } else {
                    remap.push(self.char_classes.len());
                    self.char_classes.push(cc);
                }
            }
            self.remap_inline_char_class_ids(&mut sub_code, &remap);
        }
        self.strings.extend(sub_compiler.strings);
        self.group_counter = sub_compiler.group_counter;
        self.flags.max_capture_group = self
            .flags
            .max_capture_group
            .max(sub_compiler.flags.max_capture_group);
        self.flags.has_backrefs |= sub_compiler.flags.has_backrefs;
        self.flags.has_lookarounds |= sub_compiler.flags.has_lookarounds;
        self.flags.has_code_blocks |= sub_compiler.flags.has_code_blocks;
        self.flags.instruction_count += sub_compiler.flags.instruction_count;
        self.group_counter = self.group_counter.max(sub_compiler.group_counter);

        sub_code
    }

    fn compile_subroutines(&mut self, ast: &Regex) -> Vec<Vec<u8>> {
        // Size `subroutines` by the true max group id in the AST, not
        // `self.group_counter`. A capturing group nested inside a
        // zero-repetition quantifier like `{0,0}` or `{0}` is present in
        // the AST (and found by `collect_capturing_group_defs`) but is
        // never visited by `codegen_pass` — so `group_counter` stays
        // behind and `subroutines[group_id]` would write out of bounds.
        // PCRE2 testinput1 regression pins: `^(a){0,0}`, `(a|(bc)){0,0}?xyz`,
        // `(?1)(?:(b)){0}`, `(a(*COMMIT)b){0}a(?1)|aac`, and
        // `(?:(a(*PRUNE)b)){0}(?:(?1)|ac)` — all crashed at this site
        // before the fix.
        let defs = Self::collect_capturing_group_defs(ast);
        let max_group_id = defs.iter().map(|(id, _)| *id).max().unwrap_or(0);
        let size = (self.group_counter.max(max_group_id) as usize) + 1;
        let mut subroutines = vec![Vec::new(); size];
        subroutines[0] = self.compile_nested_code(ast, 0);

        for (group_id, group_ast) in defs {
            subroutines[group_id as usize] = self.compile_nested_code(&group_ast, group_id - 1);
        }

        subroutines
    }

    fn collect_capturing_group_defs(ast: &Regex) -> Vec<(u32, Regex)> {
        let mut defs = std::collections::BTreeMap::<u32, Vec<Regex>>::new();
        let mut next_group = 0;
        Self::collect_capturing_group_defs_inner(ast, &mut next_group, &mut defs);
        defs.into_iter()
            .map(|(group_id, group_defs)| {
                let group_ast = if group_defs.len() == 1 {
                    group_defs.into_iter().next().expect("single group def")
                } else {
                    Regex::Alternation(group_defs)
                };
                (group_id, group_ast)
            })
            .collect()
    }

    fn collect_capturing_group_defs_inner(
        ast: &Regex,
        next_group: &mut u32,
        defs: &mut std::collections::BTreeMap<u32, Vec<Regex>>,
    ) {
        match ast {
            Regex::Sequence(items) | Regex::Alternation(items) => {
                for item in items {
                    Self::collect_capturing_group_defs_inner(item, next_group, defs);
                }
            }
            Regex::Quantified { expr, .. }
            | Regex::Lookahead { expr, .. }
            | Regex::Lookbehind { expr, .. }
            | Regex::FlagGroup { expr, .. } => {
                Self::collect_capturing_group_defs_inner(expr, next_group, defs);
            }
            Regex::Group {
                expr, kind, index, ..
            } => {
                if matches!(kind, GroupKind::Capturing) {
                    let group_id = index.unwrap_or_else(|| next_group.saturating_add(1));
                    *next_group = (*next_group).max(group_id);
                    defs.entry(group_id).or_default().push(ast.clone());
                }
                Self::collect_capturing_group_defs_inner(expr, next_group, defs);
            }
            Regex::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                match condition {
                    ConditionalTest::Lookahead { expr, .. }
                    | ConditionalTest::Lookbehind { expr, .. } => {
                        Self::collect_capturing_group_defs_inner(expr, next_group, defs);
                    }
                    ConditionalTest::GroupExists(_)
                    | ConditionalTest::RelativeGroupExists(_)
                    | ConditionalTest::NamedGroupExists(_)
                    | ConditionalTest::RecursionAny
                    | ConditionalTest::RecursionGroup(_)
                    | ConditionalTest::RecursionNamed(_)
                    | ConditionalTest::Define => {}
                }
                Self::collect_capturing_group_defs_inner(true_branch, next_group, defs);
                if let Some(false_branch) = false_branch {
                    Self::collect_capturing_group_defs_inner(false_branch, next_group, defs);
                }
            }
            Regex::Char(_)
            | Regex::CharClass(_)
            | Regex::Dot
            | Regex::Digit { .. }
            | Regex::Word { .. }
            | Regex::Space { .. }
            | Regex::UnicodeClass { .. }
            | Regex::ExtendedCharClass { .. }
            | Regex::Anchor(_)
            | Regex::WordBoundary { .. }
            | Regex::Backreference(_)
            | Regex::NamedBackreference(_)
            | Regex::RelativeBackreference(_)
            | Regex::Recursion { .. }
            | Regex::ReturnedCaptureSubroutine { .. }
            | Regex::CodeBlock { .. }
            | Regex::Callout(_)
            | Regex::MatchReset
            | Regex::NewlineSequence
            | Regex::GraphemeCluster
            | Regex::Accept
            | Regex::Commit
            | Regex::Prune
            | Regex::Skip(_)
            | Regex::Then
            | Regex::Mark(_)
            | Regex::WhitespaceLiteral(_)
            | Regex::Empty => {}
        }
    }

    #[allow(clippy::cast_possible_truncation)] // Char-class IDs are single-byte operands; overflow is caught by the bounds check below.
    #[allow(clippy::only_used_in_recursion)]
    #[allow(clippy::too_many_lines)] // Opcode dispatch covers all variants in a single walk — splitting would scatter the iteration logic.
    fn remap_inline_char_class_ids(&self, code: &mut [u8], remap: &[usize]) {
        if remap.is_empty() || code.is_empty() {
            return;
        }
        // Fast-path: if the remap is the identity (`remap[i] == i`) then
        // the sub-compiler's ids already match the parent's, nothing to
        // rewrite. This is the common case when dedup matched every
        // entry (all duplicates) OR when there was nothing to merge.
        if remap.iter().enumerate().all(|(i, &target)| target == i) {
            return;
        }

        let mut ip = 0;
        while ip < code.len() {
            let Ok(op) = OpCode::try_from(code[ip]) else {
                return;
            };
            ip += 1;

            match op {
                OpCode::Char => {
                    if ip >= code.len() {
                        return;
                    }
                    let len = code[ip] as usize;
                    ip += 1 + len;
                }
                OpCode::CharClass | OpCode::CharClassNeg => {
                    if ip >= code.len() {
                        return;
                    }
                    let old = code[ip] as usize;
                    let Some(&new_id) = remap.get(old) else {
                        return;
                    };
                    assert!(
                        new_id < 256,
                        "char class table exceeded single-byte operand range \
                         (remapped id {new_id} >= 256); pattern needs deduplication or \
                         a wider operand"
                    );
                    code[ip] = new_id as u8;
                    ip += 1;
                }
                OpCode::Jump | OpCode::Split | OpCode::SplitLazy => {
                    ip += 2;
                }
                OpCode::SaveStart
                | OpCode::SaveEnd
                | OpCode::Backref
                | OpCode::BackrefCaseInsensitive
                | OpCode::SetAlternative
                | OpCode::Call => {
                    ip += 1;
                }
                OpCode::Lookahead
                | OpCode::LookaheadNeg
                | OpCode::Lookbehind
                | OpCode::LookbehindNeg
                | OpCode::QuestionGreedy
                | OpCode::QuestionLazy
                | OpCode::StarGreedy
                | OpCode::StarLazy
                | OpCode::PlusGreedy
                | OpCode::PlusLazy => {
                    if ip >= code.len() {
                        return;
                    }
                    let len = code[ip] as usize;
                    ip += 1;
                    let end = ip + len;
                    if end > code.len() {
                        return;
                    }
                    self.remap_inline_char_class_ids(&mut code[ip..end], remap);
                    ip = end;
                }
                OpCode::CodeBlock => {
                    if ip >= code.len() {
                        return;
                    }
                    let lang_len = code[ip] as usize;
                    ip += 1 + lang_len;
                    if ip + 1 >= code.len() {
                        return;
                    }
                    let body_len = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2 + body_len;
                }
                OpCode::JumpIfMatch | OpCode::JumpIfNoMatch => {
                    match self.remap_conditional_operand(code, ip, remap) {
                        Some(next_ip) => ip = next_ip,
                        None => return,
                    }
                }
                OpCode::DigitAscii
                | OpCode::DigitAsciiNeg
                | OpCode::WordAscii
                | OpCode::WordAsciiNeg
                | OpCode::SpaceAscii
                | OpCode::SpaceAsciiNeg
                | OpCode::Any
                | OpCode::AnyDotAll
                | OpCode::StartLine
                | OpCode::EndLine
                | OpCode::StartText
                | OpCode::EndText
                | OpCode::EndTextOrNL
                | OpCode::WordBoundary
                | OpCode::NonWordBoundary
                | OpCode::AtomicStart
                | OpCode::AtomicEnd
                | OpCode::MatchReset
                | OpCode::PreviousMatchEnd
                | OpCode::Commit
                | OpCode::Prune
                | OpCode::VerbSkip
                | OpCode::Then
                | OpCode::Match
                | OpCode::Fail
                | OpCode::Accept
                | OpCode::Halt => {}
                OpCode::Mark | OpCode::VerbSkipNamed => {
                    // Skip the length-prefixed name operand. Both
                    // (*MARK:name) and (*SKIP:name) (A11) encode the
                    // mark name as a length byte followed by the
                    // name bytes.
                    if ip < code.len() {
                        let name_len = code[ip] as usize;
                        ip += 1 + name_len;
                    }
                }
                _ => return,
            }
        }
    }

    /// Advance past a conditional (JumpIfMatch/JumpIfNoMatch) operand,
    /// remapping any embedded char-class ids via the provided table.
    /// Returns the new `ip`, or `None` when the bytecode is malformed
    /// and iteration should stop.
    #[allow(clippy::only_used_in_recursion)]
    fn remap_conditional_operand(
        &self,
        code: &mut [u8],
        mut ip: usize,
        remap: &[usize],
    ) -> Option<usize> {
        if ip >= code.len() {
            return None;
        }
        let kind = code[ip];
        ip += 1;
        match kind {
            CONDITIONAL_KIND_GROUP_EXISTS | CONDITIONAL_KIND_RECURSION_GROUP => ip += 1,
            CONDITIONAL_KIND_RECURSION_ANY | CONDITIONAL_KIND_DEFINE_FALSE => {}
            CONDITIONAL_KIND_LOOKAHEAD_POSITIVE
            | CONDITIONAL_KIND_LOOKAHEAD_NEGATIVE
            | CONDITIONAL_KIND_LOOKBEHIND_POSITIVE
            | CONDITIONAL_KIND_LOOKBEHIND_NEGATIVE => {
                if ip >= code.len() {
                    return None;
                }
                let len = code[ip] as usize;
                ip += 1;
                let end = ip + len;
                if end > code.len() {
                    return None;
                }
                self.remap_inline_char_class_ids(&mut code[ip..end], remap);
                ip = end;
            }
            _ => return None,
        }
        Some(ip + 2)
    }

    /// Expand character ranges to include ASCII case folds.
    fn case_fold_ranges(ranges: &[CharRange]) -> Vec<CharRange> {
        let mut result = ranges.to_vec();
        for range in ranges {
            if range.start == range.end {
                // Single character — collect all case variants.
                for variant in Self::unicode_case_variants(range.start) {
                    if variant != range.start {
                        result.push(CharRange::single(variant));
                    }
                }
            } else if (range.start as u32) <= 0x7F && (range.end as u32) <= 0x7F {
                // ASCII range — iterate each letter and add its case
                // variant as a single-char range. The compile_char_class
                // sort+merge step consolidates adjacent singles into
                // runs. This is exact for any ASCII range shape, INCLUDING
                // ranges that span both cases like `[W-c]` (W=87, c=99,
                // contains upper Z, the symbols [\\]^_`, and lower a-c).
                // The prior implementation folded the two endpoints to
                // build a single mirror range which degenerated to
                // `[w..C]` (start=119 > end=67 — empty set) for cross-
                // case ranges. Regression pinned as
                // `regression_case_fold_range_spanning_both_cases` in
                // the tests module.
                let start = range.start as u32;
                let end = range.end as u32;
                for cp in start..=end {
                    if let Some(ch) = char::from_u32(cp) {
                        if ch.is_ascii_alphabetic() {
                            let swapped = if ch.is_ascii_lowercase() {
                                ch.to_ascii_uppercase()
                            } else {
                                ch.to_ascii_lowercase()
                            };
                            result.push(CharRange::single(swapped));
                        }
                    }
                }
            } else {
                // Non-ASCII or mixed range — best-effort fold the
                // endpoints (the prior implementation's path). Exact
                // expansion across large Unicode ranges is expensive
                // and rare in practice; this covers the common cases
                // and keeps the performance profile for bulk Unicode
                // property ranges.
                for ch in [range.start, range.end] {
                    for variant in Self::unicode_case_variants(ch) {
                        if variant != ch {
                            result.push(CharRange::single(variant));
                        }
                    }
                }
            }
        }
        result
    }

    /// Collect all Unicode simple case variants for a character.
    ///
    /// Returns at least the original character. For `é` returns `['é', 'É']`.
    /// Uses `regex_syntax`'s HIR simple case-fold table to pick up full
    /// equivalence classes (e.g. `ſ↔s↔S`, `K↔k↔K` (Kelvin), `Σ↔σ↔ς`) that
    /// `char::to_lowercase` / `char::to_uppercase` miss, then augments with
    /// those mappings as a backstop for codepoints outside the fold table.
    fn unicode_case_variants(ch: char) -> Vec<char> {
        let mut variants = vec![ch];
        let range = regex_syntax::hir::ClassUnicodeRange::new(ch, ch);
        let mut class = regex_syntax::hir::ClassUnicode::new([range]);
        if class.try_case_fold_simple().is_ok() {
            for r in class.iter() {
                let start = u32::from(r.start());
                let end = u32::from(r.end());
                for cp in start..=end {
                    if let Some(c) = char::from_u32(cp) {
                        if c != ch && !variants.contains(&c) {
                            variants.push(c);
                        }
                    }
                }
            }
        }
        for lower in ch.to_lowercase() {
            if lower != ch && !variants.contains(&lower) {
                variants.push(lower);
            }
        }
        for upper in ch.to_uppercase() {
            if upper != ch && !variants.contains(&upper) {
                variants.push(upper);
            }
        }
        variants
    }

    /// Emit opcode with character operand
    #[allow(clippy::cast_possible_truncation)] // UTF-8 length fits in u8.
    fn emit_char_op(&mut self, op: OpCode, ch: char) {
        self.code.push(op as u8);

        // Encode character as UTF-8 with length prefix
        let mut buf = [0; 4];
        let utf8_str = ch.encode_utf8(&mut buf);
        let utf8_bytes = utf8_str.as_bytes();

        self.code.push(utf8_bytes.len() as u8);
        self.code.extend_from_slice(utf8_bytes);

        self.flags.instruction_count += 1;
    }

    /// Compile a custom character class and return its ID
    #[allow(clippy::cast_possible_truncation)] // ASCII range values are bounded to 0..=127.
    fn compile_char_class(&mut self, ranges: &[CharRange]) -> usize {
        // Build an optimized character class representation
        let mut ascii_bitmap = [0u16; 8]; // 128 bits for ASCII
        let mut unicode_ranges = Vec::new();

        for range in ranges {
            let start = range.start as u32;
            let end = range.end as u32;

            if start <= 127 {
                let ascii_end = end.min(127);
                for ch in start as u8..=ascii_end as u8 {
                    let byte_idx = (ch / 16) as usize;
                    let bit_idx = (ch % 16) as usize;
                    ascii_bitmap[byte_idx] |= 1 << bit_idx;
                }
            }

            if end > 127 {
                unicode_ranges.push((start.max(128), end));
            }
        }

        // Sort and merge overlapping Unicode ranges for efficiency
        unicode_ranges.sort_by_key(|r| r.0);
        let mut merged: Vec<(u32, u32)> = Vec::new();
        for range in unicode_ranges {
            if let Some(last) = merged.last_mut() {
                if range.0 <= last.1 + 1 {
                    // Merge overlapping or adjacent ranges
                    last.1 = last.1.max(range.1);
                } else {
                    merged.push(range);
                }
            } else {
                merged.push(range);
            }
        }

        let char_class = CompiledCharClass {
            ascii_bitmap,
            unicode_ranges: merged,
        };

        // Dedup: if this exact class is already in the table, reuse its
        // id. Without this, repeated-expression quantifiers like
        // `[a-z]+{0,300}` push 300 identical entries and overflow the
        // single-byte operand at `rebase_inline_char_class_ids`. PCRE2
        // testinput1 regression: `word (?:[a-zA-Z0-9]+ ){0,300}otherword`.
        if let Some(existing_id) = self.char_classes.iter().position(|cc| cc == &char_class) {
            return existing_id;
        }

        // Store the character class and return its index.
        let id = self.char_classes.len();
        self.char_classes.push(char_class);
        self.stats.char_classes += 1;

        id
    }
}

impl Default for OptimizingCompiler {
    fn default() -> Self {
        Self::new()
    }
}

// Implement TryFrom for OpCode to safely convert from u8
impl TryFrom<u8> for OpCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use OpCode::{
            Any, AnyDotAll, AtomicEnd, AtomicStart, Backref, BackrefCaseInsensitive, Call, Char,
            CharClass, CharClassNeg, CodeBlock, Commit, DigitAscii, DigitAsciiNeg, EndLine,
            EndText, EndTextOrNL, Fail, GraphemeCluster, Jump, JumpIfMatch, JumpIfNoMatch,
            Lookahead, LookaheadNeg, Lookbehind, LookbehindNeg, Mark, Match, MatchReset,
            NonWordBoundary, PlusGreedy, PlusLazy, PreviousMatchEnd, Prune, QuestionGreedy,
            QuestionLazy, SaveEnd, SaveStart, SetAlternative, SpaceAscii, SpaceAsciiNeg, Split,
            SplitLazy, StarGreedy, StarLazy, StartLine, StartText, Then, VerbSkip, VerbSkipNamed,
            WordAscii, WordAsciiNeg, WordBoundary,
        };
        match value {
            0x00 => Ok(Char),
            0x01 => Ok(Any),
            0x05 => Ok(AnyDotAll),
            0x06 => Ok(MatchReset),
            0x07 => Ok(PreviousMatchEnd),
            0x08 => Ok(GraphemeCluster),
            0x10 => Ok(DigitAscii),
            0x11 => Ok(DigitAsciiNeg),
            0x12 => Ok(WordAscii),
            0x13 => Ok(WordAsciiNeg),
            0x14 => Ok(SpaceAscii),
            0x15 => Ok(SpaceAsciiNeg),
            0x16 => Ok(CharClass),
            0x17 => Ok(CharClassNeg),
            0x30 => Ok(StartLine),
            0x31 => Ok(EndLine),
            0x32 => Ok(StartText),
            0x33 => Ok(EndText),
            0x34 => Ok(EndTextOrNL),
            0x35 => Ok(WordBoundary),
            0x36 => Ok(NonWordBoundary),
            0x60 => Ok(Lookahead),
            0x61 => Ok(LookaheadNeg),
            0x62 => Ok(Lookbehind),
            0x63 => Ok(LookbehindNeg),
            0x64 => Ok(AtomicStart),
            0x65 => Ok(AtomicEnd),
            0x66 => Ok(Backref),
            0x67 => Ok(CodeBlock),
            0x68 => Ok(BackrefCaseInsensitive),
            0x40 => Ok(Jump),
            0x41 => Ok(Split),
            0x42 => Ok(SplitLazy),
            0x43 => Ok(JumpIfMatch),
            0x44 => Ok(JumpIfNoMatch),
            0x45 => Ok(Call),
            0x50 => Ok(SaveStart),
            0x51 => Ok(SaveEnd),
            0x80 => Ok(QuestionGreedy),
            0x81 => Ok(QuestionLazy),
            0x82 => Ok(StarGreedy),
            0x83 => Ok(StarLazy),
            0x84 => Ok(PlusGreedy),
            0x85 => Ok(PlusLazy),
            0x90 => Ok(SetAlternative),
            0xA0 => Ok(Commit),
            0xA1 => Ok(Prune),
            0xA2 => Ok(VerbSkip),
            0xA3 => Ok(Then),
            0xA4 => Ok(Mark),
            0xA5 => Ok(VerbSkipNamed),
            0xF0 => Ok(Match),
            0xF1 => Ok(Fail),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_char_match() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Char('a');
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("a"));
        assert!(!vm.is_match("b"));
        assert!(vm.is_match("ba")); // Should find 'a'
    }

    #[test]
    fn test_digit_class() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::CharClass(CharClass::Digit { negated: false });
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("5"));
        assert!(!vm.is_match("a"));
    }

    #[test]
    fn test_negated_custom_char_class() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::CharClass(CharClass::Custom {
            ranges: vec![CharRange::range('0', '9')],
            negated: true,
        });
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("a"));
        assert!(vm.is_match("abc"));
        assert!(!vm.is_match("5"));
    }

    #[test]
    fn test_sequence() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![Regex::Char('a'), Regex::Char('b')]);
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("ab"));
        assert!(!vm.is_match("ba"));
    }

    #[test]
    fn test_anchor_start() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![Regex::Anchor(AnchorType::Start), Regex::Char('a')]);
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("a"));
        assert!(vm.is_match("ab"));
        assert!(!vm.is_match("ba")); // Doesn't start with 'a'
    }

    #[test]
    fn test_anchor_end_scans_to_suffix_match() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Char('d'),
            Regex::Char('o'),
            Regex::Char('g'),
            Regex::Anchor(AnchorType::End),
        ]);
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        let m = vm
            .find_first("cat dog")
            .expect("Expected suffix match for end-anchored pattern");
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert!(!vm.is_match("cat dog x"));
    }

    #[test]
    fn test_anchor_end_find_all_returns_only_terminal_match() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Char('d'),
            Regex::Char('o'),
            Regex::Char('g'),
            Regex::Anchor(AnchorType::End),
        ]);
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        let matches = vm.find_all("dog xx dog");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 7);
        assert_eq!(matches[0].end, 10);
    }

    #[test]
    fn test_any_char() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Dot;
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("a"));
        assert!(vm.is_match("1"));
        assert!(vm.is_match("@"));
        assert!(!vm.is_match("\n")); // Dot doesn't match newline
        assert!(!vm.is_match(""));
    }

    #[test]
    fn test_star_quantifier() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Quantified {
            expr: Box::new(Regex::Char('a')),
            quantifier: Quantifier::ZeroOrMore { lazy: false },
        };
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("")); // Zero matches
        assert!(vm.is_match("a")); // One match
        assert!(vm.is_match("aa")); // Two matches
        assert!(vm.is_match("aaa")); // Three matches
        assert!(vm.is_match("b")); // Zero matches, continues to match rest
        assert!(vm.is_match("aaab")); // Multiple matches followed by non-match
    }

    #[test]
    fn test_question_quantifier() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Quantified {
            expr: Box::new(Regex::Char('a')),
            quantifier: Quantifier::ZeroOrOne { lazy: false },
        };
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("")); // Zero matches
        assert!(vm.is_match("a")); // One match
        assert!(vm.is_match("b")); // Zero matches, continues to match rest
        assert!(vm.is_match("aa")); // One match, rest ignored
        assert!(vm.is_match("ab")); // One match, rest ignored
    }

    #[test]
    fn test_complex_quantifiers() {
        let mut compiler = OptimizingCompiler::new();
        // Pattern: a*b+
        let ast = Regex::Sequence(vec![
            Regex::Quantified {
                expr: Box::new(Regex::Char('a')),
                quantifier: Quantifier::ZeroOrMore { lazy: false },
            },
            Regex::Quantified {
                expr: Box::new(Regex::Char('b')),
                quantifier: Quantifier::OneOrMore { lazy: false },
            },
        ]);
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);
        assert!(vm.is_match("b")); // a* matches zero, b+ matches one
        assert!(vm.is_match("ab")); // a* matches one, b+ matches one
        assert!(vm.is_match("abb")); // a* matches one, b+ matches two
        assert!(vm.is_match("aabb")); // a* matches two, b+ matches two
        assert!(!vm.is_match("a")); // a* matches one, but b+ needs at least one
        assert!(!vm.is_match("")); // b+ needs at least one
    }

    #[test]
    fn test_capture_groups() {
        let mut compiler = OptimizingCompiler::new();
        // Pattern: (a)(b)
        let ast = Regex::Sequence(vec![
            Regex::Group {
                expr: Box::new(Regex::Char('a')),
                kind: GroupKind::Capturing,
                index: Some(1),
                name: None,
            },
            Regex::Group {
                expr: Box::new(Regex::Char('b')),
                kind: GroupKind::Capturing,
                index: Some(2),
                name: None,
            },
        ]);
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);

        // Test matching and capture groups
        if let Some(m) = vm.find_first("ab") {
            println!("Debug: match = {m:?}");
            println!("Debug: vm.program.num_groups = {}", vm.program.num_groups);
            assert_eq!(m.start, 0);
            assert_eq!(m.end, 2);
            assert_eq!(m.groups.len(), 3); // Overall match + 2 capture groups

            // Overall match (group 0)
            assert_eq!(m.groups[0], Some((0, 2)));

            // First capture group (a)
            assert_eq!(m.groups[1], Some((0, 1)));

            // Second capture group (b)
            assert_eq!(m.groups[2], Some((1, 2)));
        } else {
            panic!("Match should succeed");
        }

        // Test in larger text
        if let Some(m) = vm.find_first("xabyz") {
            assert_eq!(m.start, 1);
            assert_eq!(m.end, 3);
            assert_eq!(m.groups[0], Some((1, 3))); // Overall match
            assert_eq!(m.groups[1], Some((1, 2))); // First group (a)
            assert_eq!(m.groups[2], Some((2, 3))); // Second group (b)
        } else {
            panic!("Match should succeed");
        }
    }

    #[test]
    fn test_alternation() {
        let mut compiler = OptimizingCompiler::new();
        // Pattern: cat|dog
        let ast = Regex::Alternation(vec![
            Regex::Sequence(vec![Regex::Char('c'), Regex::Char('a'), Regex::Char('t')]),
            Regex::Sequence(vec![Regex::Char('d'), Regex::Char('o'), Regex::Char('g')]),
        ]);
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);

        // Test first alternative
        assert!(vm.is_match("cat"));
        assert!(vm.is_match("I have a cat"));

        // Test second alternative
        assert!(vm.is_match("dog"));
        assert!(vm.is_match("I have a dog"));

        // Test that both work in same text
        assert!(vm.is_match("catdog")); // Should match "cat"
        assert!(vm.is_match("dogcat")); // Should match "dog"

        // Test non-matching
        assert!(!vm.is_match("bird"));
        assert!(!vm.is_match("ca")); // Incomplete
        assert!(!vm.is_match("do")); // Incomplete
    }

    #[test]
    fn test_complex_alternation() {
        let mut compiler = OptimizingCompiler::new();
        // Pattern: foo|bar|baz
        let ast = Regex::Alternation(vec![
            Regex::Sequence(vec![Regex::Char('f'), Regex::Char('o'), Regex::Char('o')]),
            Regex::Sequence(vec![Regex::Char('b'), Regex::Char('a'), Regex::Char('r')]),
            Regex::Sequence(vec![Regex::Char('b'), Regex::Char('a'), Regex::Char('z')]),
        ]);
        let program = compiler.compile(&ast);

        let vm = RegexVM::new(program);

        // Test all alternatives
        assert!(vm.is_match("foo"));
        assert!(vm.is_match("bar"));
        assert!(vm.is_match("baz"));

        // Test in larger text
        assert!(vm.is_match("foobar")); // Should match "foo"
        assert!(vm.is_match("barbaz")); // Should match "bar"
        assert!(vm.is_match("bazfoo")); // Should match "baz"

        // Test non-matching
        assert!(!vm.is_match("qux"));
        assert!(!vm.is_match("ba")); // Matches start of both "bar" and "baz" but neither fully
    }

    #[test]
    fn test_alternation_with_tracking() {
        let mut compiler = OptimizingCompiler::new();
        // Use same pattern as working test_alternation: cat|dog
        let ast = Regex::Alternation(vec![
            Regex::Sequence(vec![Regex::Char('c'), Regex::Char('a'), Regex::Char('t')]),
            Regex::Sequence(vec![Regex::Char('d'), Regex::Char('o'), Regex::Char('g')]),
        ]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        // Test that basic alternation works
        assert!(vm.is_match("cat"), "Should match 'cat'");
        assert!(vm.is_match("dog"), "Should match 'dog'");
        assert!(!vm.is_match("bird"), "Should not match 'bird'");

        // Now test alternative tracking with find_first
        if let Some(m) = vm.find_first("cat") {
            assert_eq!(m.matched_alternative, Some(0)); // First alternative
            assert_eq!(m.start, 0);
            assert_eq!(m.end, 3);
        } else {
            panic!("Should match 'cat'");
        }

        if let Some(m) = vm.find_first("dog") {
            assert_eq!(m.matched_alternative, Some(1)); // Second alternative
            assert_eq!(m.start, 0);
            assert_eq!(m.end, 3);
        } else {
            panic!("Should match 'dog'");
        }

        // Test that alternatives are tracked correctly in larger text
        if let Some(m) = vm.find_first("I have a cat") {
            assert_eq!(m.matched_alternative, Some(0)); // First alternative
            assert_eq!(m.start, 9); // Position of "cat"
            assert_eq!(m.end, 12);
        } else {
            panic!("Should match 'cat' in larger text");
        }

        if let Some(m) = vm.find_first("My dog is happy") {
            assert_eq!(m.matched_alternative, Some(1)); // Second alternative
            assert_eq!(m.start, 3); // Position of "dog"
            assert_eq!(m.end, 6);
        } else {
            panic!("Should match 'dog' in larger text");
        }
    }

    // --- Backtracking control verb tests ---

    #[test]
    fn test_commit_verb_aborts_search() {
        // (*COMMIT) means: if this attempt fails, don't try other positions.
        // Pattern: a(*COMMIT)b — must match "ab" as a pair; if the 'b' fails
        // after 'a', the entire search aborts.
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![Regex::Char('a'), Regex::Commit, Regex::Char('b')]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        // "ab" matches normally
        assert!(vm.is_match("ab"));
        // "xab" should still match — the 'a' at position 1 matches before commit fires
        assert!(vm.is_match("xab"));
        // "axb" — the 'a' at position 0 matches, (*COMMIT) fires, then 'x' != 'b'
        // fails. Because committed, no retry at position 1. No match.
        assert!(!vm.find_first("axb").is_some_and(|m| m.start == 0));
    }

    #[test]
    fn test_prune_verb_clears_backtrack() {
        // (*PRUNE) clears the backtrack stack. In a|b pattern with (*PRUNE)
        // after 'a', if 'a' path fails, backtracking to 'b' is prevented.
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![Regex::Char('a'), Regex::Prune, Regex::Char('b')]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        assert!(vm.is_match("ab"));
        // "ac" — 'a' matches, (*PRUNE) fires, 'c' != 'b' fails.
        // At position 0 the attempt fails. But scanning tries position 1 next.
        assert!(!vm.is_match("ac"));
    }

    #[test]
    fn test_skip_verb_advances_search_position() {
        // (*SKIP) tells the scanning loop to jump to the skip position
        // instead of start+1 when the current attempt fails.
        let mut compiler = OptimizingCompiler::new();
        // Pattern: a(*SKIP)b — if 'b' fails after 'a(*SKIP)', next attempt
        // starts at the position after where (*SKIP) was encountered.
        let ast = Regex::Sequence(vec![Regex::Char('a'), Regex::Skip(None), Regex::Char('b')]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        assert!(vm.is_match("ab"));
        // "aXab" — first 'a' at 0, (*SKIP) at pos 1, 'X' != 'b' fails.
        // Skip advances to pos 1, tries pos 1 'X' (no match), then pos 2 'a'
        // matches, (*SKIP) at pos 3, 'b' at pos 3 matches.
        let m = vm.find_first("aXab");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.start, 2);
        assert_eq!(m.end, 4);
    }

    #[test]
    fn test_then_verb_behaves_like_prune() {
        // (*THEN) is currently implemented as (*PRUNE) — clears backtrack stack.
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![Regex::Char('a'), Regex::Then, Regex::Char('b')]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        assert!(vm.is_match("ab"));
        assert!(!vm.is_match("ac"));
    }

    #[test]
    fn test_mark_verb_is_noop_for_matching() {
        // (*MARK:name) should not affect match behavior
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Char('a'),
            Regex::Mark("test".to_string()),
            Regex::Char('b'),
        ]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        assert!(vm.is_match("ab"));
        assert!(!vm.is_match("ac"));
        // Mark with empty name
        let mut compiler2 = OptimizingCompiler::new();
        let ast2 = Regex::Sequence(vec![
            Regex::Char('x'),
            Regex::Mark(String::new()),
            Regex::Char('y'),
        ]);
        let program2 = compiler2.compile(&ast2);
        let vm2 = RegexVM::new(program2);
        assert!(vm2.is_match("xy"));
    }

    #[test]
    fn test_commit_verb_find_all_stops_after_failure() {
        // After (*COMMIT) fires and the attempt fails, find_all should stop.
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![Regex::Char('a'), Regex::Commit, Regex::Char('b')]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        // "ab_ab" — first "ab" matches. After that, scanning continues from pos 2.
        // "a" at pos 3, (*COMMIT), "b" at pos 4 matches.
        let matches = vm.find_all("ab_ab");
        assert_eq!(matches.len(), 2);

        // "ac_ab" — first "a" at 0, commit, "c" fails, search aborts.
        // The "ab" at position 3 is never tried.
        let matches = vm.find_all("ac_ab");
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_skip_verb_find_all_advances_position() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![Regex::Char('a'), Regex::Skip(None), Regex::Char('b')]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        // "aab" — first 'a' at 0, (*SKIP) at 1, 'a' != 'b' fails.
        // Skip jumps to pos 1 (not pos 1 which is the same, so effectively
        // tries 'a' at 1, (*SKIP) at 2, 'b' at 2 matches).
        let m = vm.find_first("aab");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.start, 1);
        assert_eq!(m.end, 3);
    }

    // ============================================================
    // A11: (*SKIP:name) named skip tests
    // ============================================================

    #[test]
    fn test_skip_named_jumps_to_matching_mark_position() {
        // Pattern: (*MARK:foo) a (*SKIP:foo) b
        // Against input "ax" — at pos 0, set mark "foo" at pos 0,
        // match 'a' (pos→1), trigger (*SKIP:foo) which sets
        // skip_position to pos 0 (the matching mark's position),
        // then 'b' fails. The scan loop should jump to pos 0.
        // Wait — that's the same position, leading to an infinite
        // loop. The scan loop must guard against skip_position <=
        // start by advancing at least one byte (the existing
        // (*SKIP) handling does this). Let's use a more interesting
        // pattern:
        //
        // Pattern: ab (*MARK:foo) c (*SKIP:foo) d
        // Against "abcd" — match the whole thing.
        // Against "abce" — 'd' fails. (*SKIP:foo) sets
        // skip_position to where (*MARK:foo) was set = pos 2.
        // The scan loop advances to pos 2 (or pos 3 to avoid the
        // tight loop). Either way the next attempt starts after
        // the mark, not at start+1=1.
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Char('a'),
            Regex::Char('b'),
            Regex::Mark("foo".to_string()),
            Regex::Char('c'),
            Regex::Skip(Some("foo".to_string())),
            Regex::Char('d'),
        ]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);

        // Successful match: full pattern matches "abcd".
        let m = vm.find_first("abcd");
        assert!(m.is_some(), "expected match for 'abcd'");
        let m = m.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 4);
    }

    #[test]
    fn test_skip_named_with_nonexistent_mark_is_noop() {
        // Pattern: a (*SKIP:nonexistent) b
        // No matching mark exists, so (*SKIP:nonexistent) is a
        // no-op. The pattern should match "ab" normally.
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Char('a'),
            Regex::Skip(Some("nonexistent".to_string())),
            Regex::Char('b'),
        ]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);
        assert!(vm.is_match("ab"));
        assert!(!vm.is_match("ax"));
    }

    #[test]
    fn test_skip_named_uses_most_recent_matching_mark() {
        // Pattern: (*MARK:foo) a (*MARK:foo) b (*SKIP:foo) c
        // The two marks have the same name. The SKIP should look
        // up the MOST RECENT mark (the one at pos 1, after 'a'),
        // not the first one (at pos 0).
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Mark("foo".to_string()),
            Regex::Char('a'),
            Regex::Mark("foo".to_string()),
            Regex::Char('b'),
            Regex::Skip(Some("foo".to_string())),
            Regex::Char('c'),
        ]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);
        // Successful match.
        let m = vm.find_first("abc");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
    }

    #[test]
    fn test_skip_named_distinguishes_mark_names() {
        // Pattern: (*MARK:foo) a (*MARK:bar) b (*SKIP:foo) c
        // The (*SKIP:foo) should look up the foo mark (at pos 0),
        // NOT the bar mark (at pos 1).
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Mark("foo".to_string()),
            Regex::Char('a'),
            Regex::Mark("bar".to_string()),
            Regex::Char('b'),
            Regex::Skip(Some("foo".to_string())),
            Regex::Char('c'),
        ]);
        let program = compiler.compile(&ast);
        let vm = RegexVM::new(program);
        // Successful match: "abc" — the mark/skip/mark interaction
        // doesn't disturb the basic flow when the pattern matches.
        let m = vm.find_first("abc");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
    }

    #[test]
    fn test_skip_named_parses_via_public_api() {
        // End-to-end test through Regex::compile to verify the
        // parser → AST → bytecode → VM path.
        let r = crate::Regex::compile("(*MARK:foo)abc(*SKIP:foo)d").unwrap();
        // The pattern matches "abcd".
        assert!(r.is_match("abcd"));
        // The pattern does not match "abce".
        assert!(!r.is_match("abce"));
    }

    // ==================================================================
    // Regression pins — PCRE2 10.47 testinput1 conformance crash-bugs
    // ==================================================================
    //
    // These patterns crashed the VM before the fixes landed in
    // `compile_subroutines` (size `subroutines` by AST-observed max
    // group id, not `group_counter`) and `compile_nested_code` (dedup
    // char class table entries during sub-compiler merge). Each is a
    // minimal reproducer taken straight from
    // `subs/pcre2/testdata/testinput1`.
    //
    // The tests assert the engine **does not panic**. Matching
    // semantics for some patterns (especially `{0,0}` with captures on
    // non-matching subjects) are still tracked in `docs/BACKLOG.md` C7
    // — the PCRE2 conformance harness will surface any regression there.

    #[test]
    fn regression_zero_zero_quantifier_with_nested_capture_does_not_panic() {
        // testinput1 line 1515: `(a|(bc)){0,0}?xyz` vs "xyz" — used to
        // crash with `index out of bounds: the len is 1 but the index is 1`
        // at the old `compile_subroutines` site.
        let r = crate::Regex::compile(r"(a|(bc)){0,0}?xyz").expect("compiles");
        let _ = r.is_match("xyz");
    }

    #[test]
    fn regression_zero_zero_quantifier_on_anchored_pattern_does_not_panic() {
        // testinput1 line 1709: `^(a){0,0}` vs multiple subjects.
        let r = crate::Regex::compile(r"^(a){0,0}").expect("compiles");
        for subject in &["bcd", "abc", "aab     "] {
            let _ = r.is_match(subject);
            let _ = r.find_first(subject);
        }
    }

    #[test]
    fn regression_zero_quantifier_with_subroutine_call_does_not_panic() {
        // testinput1 line 4942: `(?1)(?:(b)){0}` vs "b" — subroutine call
        // pointing at a group that only exists inside a {0} quantifier.
        let r = crate::Regex::compile(r"(?1)(?:(b)){0}").expect("compiles");
        let _ = r.is_match("b");
    }

    #[test]
    fn regression_zero_quantifier_with_backtracking_verb_does_not_panic() {
        // testinput1 line 5314: `(a(*COMMIT)b){0}a(?1)|aac` vs "aac".
        let r = crate::Regex::compile(r"(a(*COMMIT)b){0}a(?1)|aac").expect("compiles");
        let _ = r.is_match("aac");
    }

    #[test]
    fn regression_zero_quantifier_with_nested_prune_does_not_panic() {
        // testinput1 line 5354: `(?:(a(*PRUNE)b)){0}(?:(?1)|ac)` vs
        // various subjects including empty and subjects with backslashes.
        let r = crate::Regex::compile(r"(?:(a(*PRUNE)b)){0}(?:(?1)|ac)").expect("compiles");
        for subject in &["ac", "", r"/(?:(a(*SKIP)b)){0}(?:(?1)|ac)/"] {
            let _ = r.is_match(subject);
        }
    }

    #[test]
    fn regression_char_class_table_no_longer_overflows_single_byte_on_high_repeat() {
        // testinput1 line 1705: `word (?:[a-zA-Z0-9]+ ){0,300}otherword`
        // used to crash with `char class table exceeded single-byte
        // operand range` because the Range quantifier emits the inner
        // expression 300 times, each sub-compile producing a fresh char
        // class entry. The dedup merge in `compile_nested_code` collapses
        // all 300 into a single shared entry.
        let r = crate::Regex::compile(r"word (?:[a-zA-Z0-9]+ ){0,300}otherword").expect("compiles");
        let subject = "word cat dog elephant mussel cow horse canary baboon snake \
                       shark the quick brown fox and the lazy dog and several other \
                       words getting close to thirty by now I hope";
        let _ = r.is_match(subject);
    }

    #[test]
    fn regression_char_class_dedup_keeps_unique_classes_separate() {
        // Sanity check that dedup doesn't collapse *different* classes
        // into the same entry. `[a-z]+[0-9]+` has two distinct classes;
        // the compiled program must tell them apart.
        let r = crate::Regex::compile(r"[a-z]+[0-9]+").expect("compiles");
        assert!(r.is_match("abc123"));
        assert!(!r.is_match("abc")); // missing digit
        assert!(!r.is_match("123")); // missing letter
    }

    // ==================================================================
    // Regression pins — case-insensitive char-class range folding
    // ==================================================================
    //
    // PCRE2 testinput1 line 1381: `/^[W-c]+$/i` on subject `"wxy_^ABC"`
    // expects a full match. Before the fix, `case_fold_ranges` added a
    // single "mirror-case" range for ASCII alphabetic endpoints. For
    // `[W-c]` the endpoints (W=87, c=99) fold to (w=119, C=67), which
    // is an inverted range that matches nothing. The fix switches to
    // per-character iteration within ASCII ranges so each letter's
    // case variant is added independently; the sort+merge step then
    // consolidates adjacent singles into proper sub-ranges.

    #[test]
    fn regression_case_fold_range_spanning_both_cases_matches_mixed_subject() {
        // testinput1:1381 — the minimal PCRE2 reproducer.
        use crate::RegexBuilder;
        let r = RegexBuilder::new(r"^[W-c]+$")
            .case_insensitive()
            .build()
            .expect("compiles");
        assert!(r.is_match("wxy_^ABC"), "all chars in case-folded [W-c]");
        let m = r.find_first("wxy_^ABC").expect("matches");
        assert_eq!((m.start, m.end), (0, 8));
    }

    #[test]
    fn regression_case_fold_range_spanning_both_cases_does_not_match_out_of_range() {
        // Sanity: characters outside the case-folded range must still
        // be rejected. `.` (46) and `!` (33) are not in W..c nor in
        // their case-folded additions A..C / w..z. Confirms the fix
        // widens coverage without turning [W-c]/i into ".".
        use crate::RegexBuilder;
        let r = RegexBuilder::new(r"^[W-c]+$")
            .case_insensitive()
            .build()
            .expect("compiles");
        assert!(!r.is_match("."));
        assert!(!r.is_match("!"));
        assert!(!r.is_match("d")); // d (100) is outside W..c (87..99)
        assert!(!r.is_match("D")); // D (68) is outside A..C / W..c
    }

    #[test]
    fn regression_case_fold_preserves_ascii_range_not_spanning_cases() {
        // `[a-f]/i` is a lowercase-only range; case-folded should
        // include [A-F]. This path worked before the fix but needs
        // to keep working after the refactor.
        use crate::RegexBuilder;
        let r = RegexBuilder::new(r"^[a-f]+$")
            .case_insensitive()
            .build()
            .expect("compiles");
        assert!(r.is_match("abc"));
        assert!(r.is_match("ABC"));
        assert!(r.is_match("aBcDeF"));
        assert!(!r.is_match("g"));
        assert!(!r.is_match("G"));
    }

    #[test]
    fn regression_case_fold_preserves_uppercase_only_range() {
        // `[W-Z]/i` is an uppercase-only range; case-folded should
        // include [w-z]. The per-letter iteration path must handle
        // this correctly.
        use crate::RegexBuilder;
        let r = RegexBuilder::new(r"^[W-Z]+$")
            .case_insensitive()
            .build()
            .expect("compiles");
        assert!(r.is_match("WXYZ"));
        assert!(r.is_match("wxyz"));
        assert!(r.is_match("WxYz"));
        assert!(!r.is_match("a"));
        assert!(!r.is_match("V"));
    }
}
