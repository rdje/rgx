//! High-Performance Regex Virtual Machine
//!
//! This module implements a state-of-the-art regex execution engine designed
//! to surpass PCRE2 performance through:
//! - SIMD-optimized pattern matching
//! - Cache-friendly bytecode design
//! - Adaptive execution strategies
//! - JIT compilation hints
//! - Memoization for backtracking

use crate::ast::{Regex, Quantifier, CharClass, AnchorType, GroupKind};
use std::collections::HashMap;

/// High-performance bytecode instruction optimized for cache efficiency
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)] // Ensure tight packing for cache efficiency
pub enum OpCode {
    // === LITERAL MATCHING (0x00-0x0F) ===
    /// Match single character - most common operation, gets opcode 0
    Char = 0x00,
    /// Match any character except newline
    Any = 0x01, 
    /// Match literal string (length in next byte, followed by UTF-8 bytes)
    String = 0x02,
    /// Case-insensitive character match
    CharNoCase = 0x03,
    /// Case-insensitive string match
    StringNoCase = 0x04,
    
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
    /// Range check [a-z] style (followed by start, end chars)
    Range = 0x18,
    /// Negated range check
    RangeNeg = 0x19,
    
    // === SIMD-OPTIMIZED OPERATIONS (0x20-0x2F) ===
    /// Find any byte from set using SIMD (up to 16 bytes)
    SimdFind = 0x20,
    /// Find literal string using SIMD Boyer-Moore
    SimdString = 0x21,
    /// Vectorized character class matching
    SimdCharClass = 0x22,
    /// SIMD-accelerated dot matching (skip non-newlines)
    SimdAny = 0x23,
    
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
    JumpIfMatch = 0x43,
    /// Conditional jump based on negative lookahead
    JumpIfNoMatch = 0x44,
    /// Call subroutine (for recursion/subroutine calls)
    Call = 0x45,
    /// Return from subroutine
    Return = 0x46,
    
    // === CAPTURE GROUPS (0x50-0x5F) ===
    /// Save position to capture group (group ID follows)
    SaveStart = 0x50,
    /// Save end position to capture group  
    SaveEnd = 0x51,
    /// Conditional save (only if group doesn't exist)
    SaveStartCond = 0x52,
    /// Restore previous capture state (for backtracking)
    RestoreCaptures = 0x53,
    
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
    
    // === OPTIMIZATION HINTS (0x70-0x7F) ===
    /// Mark hot path for JIT compilation
    HotPath = 0x70,
    /// Memoization point (cache match results)
    Memoize = 0x71,
    /// Clear memoization cache
    ClearMemo = 0x72,
    /// Prefetch hint for upcoming memory access
    Prefetch = 0x73,
    
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
    /// Range quantifier {n,m} - min/max in next 2 bytes
    RepeatRange = 0x86,
    /// Exact repeat quantifier {n} - count in next byte
    RepeatExact = 0x87,
    
    // === ALTERNATIVE TRACKING (0x90-0x9F) ===
    /// Set the current alternative index (for match reporting)
    SetAlternative = 0x90,
    
    // === TERMINATION (0xF0-0xFF) ===
    /// Successful match - capture current position
    Match = 0xF0,
    /// Match failure - backtrack
    Fail = 0xF1,
    /// Accept - successful completion
    Accept = 0xF2,
    /// Halt execution (for debugging)
    Halt = 0xFF,
}

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
    pub fn simple(op: OpCode) -> Self {
        Self { op, operands: Vec::new() }
    }
    
    /// Create instruction with single byte operand
    pub fn with_byte(op: OpCode, operand: u8) -> Self {
        Self { op, operands: vec![operand] }
    }
    
    /// Create instruction with 16-bit operand (little-endian)
    pub fn with_word(op: OpCode, operand: u16) -> Self {
        Self { 
            op, 
            operands: operand.to_le_bytes().to_vec(),
        }
    }
    
    /// Create instruction with character operand  
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
#[derive(Debug, Clone)]
pub struct CompiledCharClass {
    /// Bitmap for ASCII characters (0-127) - 16 bytes, SIMD-friendly
    pub ascii_bitmap: [u16; 8], // 128 bits packed into u16s for SIMD
    /// Non-ASCII ranges for Unicode support
    pub unicode_ranges: Vec<(u32, u32)>,
    /// Whether this class is negated
    pub negated: bool,
}

/// High-performance compiled regex program
#[derive(Debug, Clone)]
pub struct Program {
    /// Bytecode instructions optimized for cache locality
    pub code: Vec<u8>,
    /// Pre-compiled character classes
    pub char_classes: Vec<CompiledCharClass>, 
    /// String literals extracted for SIMD matching
    pub string_literals: Vec<String>,
    /// Number of capture groups
    pub num_groups: u32,
    /// Optimization flags
    pub flags: ProgramFlags,
    /// Performance statistics from compilation
    pub stats: CompilationStats,
}

/// Program optimization flags
#[derive(Debug, Clone, Copy)]
pub struct ProgramFlags {
    /// Can use SIMD instructions
    pub simd_enabled: bool,
    /// Contains anchors (affects matching strategy)  
    pub has_anchors: bool,
    /// Contains backreferences (prevents some optimizations)
    pub has_backrefs: bool,
    /// Contains lookarounds
    pub has_lookarounds: bool,
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
pub struct ExecContext {
    /// Input text as UTF-8 bytes for SIMD processing
    pub text: Vec<u8>,
    /// Current position in bytes (not characters!)
    pub pos: usize,
    /// End position for bounded matching
    pub end: usize,
    /// Capture group positions [start, end, start, end, ...]
    pub captures: Vec<Option<usize>>,
    /// Memoization cache for backtracking
    pub memo_cache: HashMap<(usize, usize), bool>,
    /// Call stack for recursion
    pub call_stack: Vec<usize>,
    /// Backtrack stack for alternation and optional quantifiers
    pub backtrack_stack: Vec<BacktrackFrame>,
    /// Track which alternative is currently being executed
    pub current_alternative: Option<usize>,
}

/// Backtracking frame for alternation and quantifiers
#[derive(Debug, Clone)]
pub struct BacktrackFrame {
    /// Instruction pointer to return to
    pub ip: usize,
    /// Text position to restore
    pub pos: usize,
    /// Saved capture state
    pub saved_captures: Vec<Option<usize>>,
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
}

/// High-performance regex execution engine
pub struct RegexVM {
    /// Compiled program
    pub program: Program,
    /// SIMD instruction support detected at runtime
    simd_support: SimdSupport,
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
    pub fn new(program: Program) -> Self {
        Self {
            program,
            simd_support: Self::detect_simd_support(),
        }
    }

    /// Detect available SIMD instruction sets
    fn detect_simd_support() -> SimdSupport {
        SimdSupport {
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
        }
    }

    /// Find first match using adaptive execution strategy
    pub fn find_first(&self, text: &str) -> Option<Match> {
        let bytes = text.as_bytes();
        let mut ctx = ExecContext {
            text: bytes.to_vec(),
            pos: 0,
            end: bytes.len(),
            captures: vec![None; (self.program.num_groups + 1) as usize * 2],
            memo_cache: HashMap::new(),
            call_stack: Vec::new(),
            backtrack_stack: Vec::new(),
            current_alternative: None,
        };

        // Adaptive strategy selection based on program characteristics
        if self.should_use_simd_search(&ctx) {
            self.find_first_simd(&mut ctx)
        } else if self.program.flags.has_anchors {
            self.find_first_anchored(&mut ctx)
        } else {
            self.find_first_scanning(&mut ctx)
        }
    }

    /// Determine if SIMD pre-filtering would be beneficial  
    fn should_use_simd_search(&self, ctx: &ExecContext) -> bool {
        self.simd_support.sse2 && 
        ctx.text.len() > 64 && // Worth SIMD overhead
        self.program.stats.literal_chars > 0 // Has literal content to search for
    }

    /// SIMD-accelerated first match search
    fn find_first_simd(&self, ctx: &mut ExecContext) -> Option<Match> {
        // TODO: Implement SIMD string search as pre-filter
        // For now, fall back to scanning approach
        self.find_first_scanning(ctx)
    }

    /// Optimized search for anchored patterns
    fn find_first_anchored(&self, ctx: &mut ExecContext) -> Option<Match> {
        // Only try match at start for ^ anchor
        if self.execute_at(ctx, 0) {
            Some(Match {
                start: 0,
                end: ctx.pos,
                groups: self.extract_captures(ctx),
                matched_alternative: ctx.current_alternative,
            })
        } else {
            None
        }
    }

    /// Standard scanning approach - try match at each position
    fn find_first_scanning(&self, ctx: &mut ExecContext) -> Option<Match> {
        for start in 0..=ctx.text.len() {
            ctx.pos = start;
            self.reset_captures(ctx);
            
            if self.execute_at(ctx, start) {
                return Some(Match {
                    start,
                    end: ctx.pos,
                    groups: self.extract_captures_with_match(ctx, start, ctx.pos),
                    matched_alternative: ctx.current_alternative,
                });
            }
        }
        None
    }

    /// Execute bytecode starting at given position
    fn execute_at(&self, ctx: &mut ExecContext, start: usize) -> bool {
        ctx.pos = start;
        let mut ip = 0;
        let code = &self.program.code;
        
        loop {
            if ip >= code.len() {
                return false;
            }

            let op = OpCode::try_from(code[ip]).unwrap_or(OpCode::Fail);
            ip += 1;

            match op {
                OpCode::Char => {
                    // Read UTF-8 character from operands
                    if let Some(expected) = self.read_char_operand(code, &mut ip) {
                        if let Some(actual) = self.current_char(ctx) {
                            if actual == expected {
                                self.advance_char(ctx);
                                continue;
                            }
                        }
                    }
                    // Character didn't match - try backtracking
                    if let Some(frame) = ctx.backtrack_stack.pop() {
                        // Restore saved state
                        ip = frame.ip;
                        ctx.pos = frame.pos;
                        ctx.captures = frame.saved_captures;
                        continue;
                    }
                    return false;
                }

                OpCode::Any => {
                    if let Some(ch) = self.current_char(ctx) {
                        if ch != '\n' {
                            self.advance_char(ctx);
                            continue;
                        }
                    }
                    return false;
                }

                OpCode::DigitAscii => {
                    if let Some(ch) = self.current_char(ctx) {
                        if ch.is_ascii_digit() {
                            self.advance_char(ctx);
                            continue;
                        }
                    }
                    return false;
                }

                OpCode::WordAscii => {
                    if let Some(ch) = self.current_char(ctx) {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            self.advance_char(ctx);
                            continue;
                        }
                    }
                    return false;
                }

                OpCode::SpaceAscii => {
                    if let Some(ch) = self.current_char(ctx) {
                        if ch.is_ascii_whitespace() {
                            self.advance_char(ctx);
                            continue;
                        }
                    }
                    return false;
                }

                OpCode::StartLine => {
                    if ctx.pos == 0 || (ctx.pos > 0 && ctx.text[ctx.pos - 1] == b'\n') {
                        continue;
                    }
                    return false;
                }

                OpCode::EndLine => {
                    if ctx.pos >= ctx.text.len() || ctx.text[ctx.pos] == b'\n' {
                        continue;
                    }
                    return false;
                }

                OpCode::Match => {
                    return true;
                }

                OpCode::WordBoundary => {
                    // Check if we're at a word boundary
                    if self.is_at_word_boundary(ctx) {
                        continue;
                    }
                    return false;
                }

                OpCode::NonWordBoundary => {
                    // Check if we're NOT at a word boundary
                    if !self.is_at_word_boundary(ctx) {
                        continue;
                    }
                    return false;
                }

                OpCode::PlusGreedy => {
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
                    
                    // Must match at least once
                    let _start_pos = ctx.pos;
                    if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                        return false;
                    }
                    
                    // Keep matching greedily until we can't match anymore
                    loop {
                        let before_pos = ctx.pos;
                        if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                            // Can't match anymore, that's fine
                            break;
                        }
                        // If we didn't advance, avoid infinite loop
                        if ctx.pos == before_pos {
                            break;
                        }
                    }
                    
                    ip = expr_end;
                    continue;
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
                        if !self.execute_subexpr(ctx, &code[expr_start..expr_end]) {
                            // Can't match anymore, that's fine for *
                            break;
                        }
                        // If we didn't advance, avoid infinite loop
                        if ctx.pos == before_pos {
                            break;
                        }
                    }
                    
                    ip = expr_end;
                    continue;
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
                    
                    // Try to match once (greedy), but it's optional
                    let _before_pos = ctx.pos;
                    let _matched = self.execute_subexpr(ctx, &code[expr_start..expr_end]);
                    // For ?, we don't care if it failed - it's optional
                    
                    ip = expr_end;
                    continue;
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
                        ctx.captures[start_idx] = Some(ctx.pos);
                    }
                    continue;
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
                        ctx.captures[end_idx] = Some(ctx.pos);
                    }
                    continue;
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
                        saved_captures: ctx.captures.clone(),
                    };
                    ctx.backtrack_stack.push(backtrack_frame);
                    
                    // Continue with first alternative (current path)
                    continue;
                }
                
                OpCode::Jump => {
                    // Read jump offset (2 bytes, little-endian)
                    if ip + 1 >= code.len() {
                        return false;
                    }
                    let offset = u16::from_le_bytes([code[ip], code[ip + 1]]) as usize;
                    ip += 2; // Skip the 2-byte offset operand
                    ip += offset; // Then add the offset
                    continue;
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
                    continue;
                }

                OpCode::Fail => {
                    // Try backtracking if we have saved states
                    if let Some(frame) = ctx.backtrack_stack.pop() {
                        // Restore saved state
                        ip = frame.ip;
                        ctx.pos = frame.pos;
                        ctx.captures = frame.saved_captures;
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

    /// Read UTF-8 character from bytecode operands
    fn read_char_operand(&self, code: &[u8], ip: &mut usize) -> Option<char> {
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

    /// Get current character at context position
    fn current_char(&self, ctx: &ExecContext) -> Option<char> {
        if ctx.pos >= ctx.text.len() {
            return None;
        }

        // Convert from UTF-8 bytes to char
        let remaining = &ctx.text[ctx.pos..];
        std::str::from_utf8(remaining).ok()?.chars().next()
    }

    /// Advance context position by one character
    fn advance_char(&self, ctx: &mut ExecContext) {
        if let Some(ch) = self.current_char(ctx) {
            ctx.pos += ch.len_utf8();
        }
    }

    /// Reset capture groups for new match attempt
    fn reset_captures(&self, ctx: &mut ExecContext) {
        for capture in ctx.captures.iter_mut() {
            *capture = None;
        }
        // Also clear backtrack stack for fresh start
        ctx.backtrack_stack.clear();
        // Reset alternative tracking for fresh start
        ctx.current_alternative = None;
    }

    /// Extract capture groups from context
    fn extract_captures(&self, ctx: &ExecContext) -> Vec<Option<(usize, usize)>> {
        let mut groups = Vec::new();
        
        for i in 0..=self.program.num_groups {
            let start_idx = (i * 2) as usize;
            let end_idx = start_idx + 1;
            
            if let (Some(start), Some(end)) = (
                ctx.captures.get(start_idx).and_then(|&x| x),
                ctx.captures.get(end_idx).and_then(|&x| x)
            ) {
                groups.push(Some((start, end)));
            } else {
                groups.push(None);
            }
        }
        
        groups
    }

    /// Extract capture groups with explicit overall match (group 0)
    fn extract_captures_with_match(&self, ctx: &ExecContext, match_start: usize, match_end: usize) -> Vec<Option<(usize, usize)>> {
        let mut groups = Vec::new();
        
        // Group 0 is always the overall match
        groups.push(Some((match_start, match_end)));
        
        // Extract the numbered capture groups (1, 2, 3, ...)
        for i in 1..=self.program.num_groups {
            let start_idx = (i * 2) as usize;
            let end_idx = start_idx + 1;
            
            if let (Some(start), Some(end)) = (
                ctx.captures.get(start_idx).and_then(|&x| x),
                ctx.captures.get(end_idx).and_then(|&x| x)
            ) {
                groups.push(Some((start, end)));
            } else {
                groups.push(None);
            }
        }
        
        groups
    }

    /// Check if we're at a word boundary (\b)
    fn is_at_word_boundary(&self, ctx: &ExecContext) -> bool {
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
        
        let curr_is_word = if let Some(ch) = self.current_char(ctx) {
            is_word_char(ch)
        } else {
            false
        };
        
        // Word boundary exists if exactly one of prev/curr is a word character
        prev_is_word != curr_is_word
    }

    /// Find all non-overlapping matches
    pub fn find_all(&self, text: &str) -> Vec<Match> {
        let mut matches = Vec::new();
        let mut start = 0;
        
        while start <= text.len() {
            if let Some(m) = self.find_first(&text[start..]) {
                let adjusted_match = Match {
                    start: start + m.start,
                    end: start + m.end,
                    groups: m.groups.iter().map(|opt| {
                        opt.map(|(s, e)| (start + s, start + e))
                    }).collect(),
                    matched_alternative: m.matched_alternative,
                };
                
                start = adjusted_match.end.max(adjusted_match.start + 1);
                matches.push(adjusted_match);
            } else {
                break;
            }
        }
        
        matches
    }

    /// Test if pattern matches text
    pub fn is_match(&self, text: &str) -> bool {
        self.find_first(text).is_some()
    }
    
    /// Execute a sub-expression (used for quantifiers)
    fn execute_subexpr(&self, ctx: &mut ExecContext, code: &[u8]) -> bool {
        let mut ip = 0;
        
        loop {
            if ip >= code.len() {
                return true; // Successfully executed all instructions
            }
            
            let op = OpCode::try_from(code[ip]).unwrap_or(OpCode::Fail);
            ip += 1;
            
            match op {
                OpCode::WordAscii => {
                    if let Some(ch) = self.current_char(ctx) {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            self.advance_char(ctx);
                            continue;
                        }
                    }
                    return false;
                }
                
                OpCode::DigitAscii => {
                    if let Some(ch) = self.current_char(ctx) {
                        if ch.is_ascii_digit() {
                            self.advance_char(ctx);
                            continue;
                        }
                    }
                    return false;
                }
                
                OpCode::Char => {
                    // Read UTF-8 character from operands
                    if let Some(expected) = self.read_char_operand(code, &mut ip) {
                        if let Some(actual) = self.current_char(ctx) {
                            if actual == expected {
                                self.advance_char(ctx);
                                continue;
                            }
                        }
                    }
                    return false;
                }
                
                OpCode::Any => {
                    if let Some(ch) = self.current_char(ctx) {
                        if ch != '\n' {
                            self.advance_char(ctx);
                            continue;
                        }
                    }
                    return false;
                }
                
                // Add other opcodes as needed
                _ => {
                    return false;
                }
            }
        }
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
    /// Group counter for captures
    group_counter: u32,
    /// Optimization flags
    flags: ProgramFlags,
    /// Compilation statistics
    stats: CompilationStats,
}

impl OptimizingCompiler {
    /// Create new optimizing compiler
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            char_classes: Vec::new(),
            strings: Vec::new(),
            group_counter: 0,
            flags: ProgramFlags {
                simd_enabled: true,
                has_anchors: false,
                has_backrefs: false,
                has_lookarounds: false,
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
        }
    }

    /// Compile AST to optimized program with multiple passes
    pub fn compile(&mut self, ast: &Regex) -> Program {
        // Reset state
        self.code.clear();
        self.char_classes.clear();
        self.strings.clear();
        self.group_counter = 0;
        
        // Pass 1: Analysis - gather statistics and detect features
        self.analyze_pass(ast);
        
        // Pass 2: Optimization - apply peephole optimizations
        self.optimize_ast(ast);
        
        // Pass 3: Code generation - emit optimized bytecode
        self.codegen_pass(ast);
        
        // Pass 4: Final optimizations - peephole optimization on bytecode
        self.peephole_optimize();
        
        // Emit final Match instruction
        self.emit_op(OpCode::Match);
        
        Program {
            code: self.code.clone(),
            char_classes: self.char_classes.clone(),
            string_literals: self.strings.clone(),
            num_groups: self.group_counter,
            flags: self.flags,
            stats: self.stats,
        }
    }
    
    /// Analysis pass - gather statistics for optimization decisions
    fn analyze_pass(&mut self, ast: &Regex) {
        match ast {
            Regex::Char(_) => self.stats.literal_chars += 1,
            Regex::CharClass(_) => self.stats.char_classes += 1,
            Regex::Quantified { .. } => self.stats.quantifiers += 1,
            Regex::Anchor(_) => self.flags.has_anchors = true,
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
            _ => {}
        }
        
        // Update JIT worthiness based on complexity
        self.stats.jit_worthy = self.stats.literal_chars > 10 || 
                               self.stats.quantifiers > 3 ||
                               self.stats.char_classes > 5;
    }
    
    /// Optimization pass - AST-level optimizations
    fn optimize_ast(&mut self, _ast: &Regex) {
        // TODO: Implement AST optimizations like:
        // - String literal concatenation
        // - Character class merging  
        // - Dead code elimination
        // - Quantifier fusion
    }
    
    /// Code generation pass - emit optimized bytecode
    fn codegen_pass(&mut self, ast: &Regex) {
        match ast {
            Regex::Char(ch) => {
                self.emit_char_op(OpCode::Char, *ch);
            }
            
            Regex::Dot => {
                self.emit_op(OpCode::Any);
            }
            
            Regex::CharClass(char_class) => {
                match char_class {
                    CharClass::Digit { negated: false } => self.emit_op(OpCode::DigitAscii),
                    CharClass::Digit { negated: true } => self.emit_op(OpCode::DigitAsciiNeg),
                    CharClass::Word { negated: false } => self.emit_op(OpCode::WordAscii),
                    CharClass::Word { negated: true } => self.emit_op(OpCode::WordAsciiNeg),
                    CharClass::Space { negated: false } => self.emit_op(OpCode::SpaceAscii),
                    CharClass::Space { negated: true } => self.emit_op(OpCode::SpaceAsciiNeg),
                    _ => {
                        // TODO: Implement custom character classes
                        self.emit_op(OpCode::Any);
                    }
                }
            }
            
            Regex::Anchor(anchor) => {
                match anchor {
                    AnchorType::Start => self.emit_op(OpCode::StartLine),
                    AnchorType::End => self.emit_op(OpCode::EndLine),
                    AnchorType::AbsStart => self.emit_op(OpCode::StartText),
                    AnchorType::AbsEnd => self.emit_op(OpCode::EndText),
                    AnchorType::AbsEndNoNL => self.emit_op(OpCode::EndTextOrNL),
                }
            }
            
            Regex::Sequence(items) => {
                for item in items {
                    self.codegen_pass(item);
                }
            }
            
            Regex::Alternation(alts) => {
                // Implement proper alternation with Split opcodes and backtracking
                if alts.is_empty() {
                    self.emit_op(OpCode::Fail);
                    return;
                }
                
                if alts.len() == 1 {
                    // Single alternative - emit SetAlternative for index 0 and compile
                    self.emit_op(OpCode::SetAlternative);
                    self.code.push(0);  // Alternative index 0
                    self.codegen_pass(&alts[0]);
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
                        self.emit_op(OpCode::SetAlternative);
                        self.code.push(i as u8);
                        self.codegen_pass(alt);
                    } else {
                        // Not the last - emit Split to next alternative
                        self.emit_op(OpCode::Split);
                        let split_offset_pos = self.code.len();
                        self.code.push(0); // Will be patched
                        self.code.push(0); // Will be patched
                        
                        // Current alternative
                        self.emit_op(OpCode::SetAlternative);
                        self.code.push(i as u8);
                        self.codegen_pass(alt);
                        
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
            
            Regex::Quantified { expr, quantifier } => {
                match quantifier {
                    Quantifier::OneOrMore { lazy: false } => {
                        // For A+, emit special PlusGreedy opcode
                        self.emit_op(OpCode::PlusGreedy);
                        
                        // First, collect the sub-expression bytecode
                        let mut sub_compiler = OptimizingCompiler::new();
                        sub_compiler.codegen_pass(expr);
                        let sub_code = sub_compiler.code;
                        
                        // Emit the length of the sub-expression
                        self.code.push(sub_code.len() as u8);
                        
                        // Then emit the sub-expression bytecode
                        self.code.extend(sub_code);
                    }
                    Quantifier::ZeroOrMore { lazy: false } => {
                        // For A*, emit special StarGreedy opcode
                        self.emit_op(OpCode::StarGreedy);
                        
                        // First, collect the sub-expression bytecode
                        let mut sub_compiler = OptimizingCompiler::new();
                        sub_compiler.codegen_pass(expr);
                        let sub_code = sub_compiler.code;
                        
                        // Emit the length of the sub-expression
                        self.code.push(sub_code.len() as u8);
                        
                        // Then emit the sub-expression bytecode
                        self.code.extend(sub_code);
                    }
                    Quantifier::ZeroOrOne { lazy: false } => {
                        // For A?, emit special QuestionGreedy opcode
                        self.emit_op(OpCode::QuestionGreedy);
                        
                        // First, collect the sub-expression bytecode
                        let mut sub_compiler = OptimizingCompiler::new();
                        sub_compiler.codegen_pass(expr);
                        let sub_code = sub_compiler.code;
                        
                        // Emit the length of the sub-expression
                        self.code.push(sub_code.len() as u8);
                        
                        // Then emit the sub-expression bytecode
                        self.code.extend(sub_code);
                    }
                    Quantifier::Range { min, max, lazy: false } => {
                        // For A{n,m}, emit A exactly n times, then optionally up to (m-n) more times
                        let min_count = *min as usize;
                        let max_count = max.unwrap_or(*min) as usize;
                        
                        // Emit required repetitions (min times)
                        for _ in 0..min_count {
                            self.codegen_pass(expr);
                        }
                        
                        // Emit optional repetitions (up to max-min more times)
                        // For exact repetitions like {3}, min == max, so this does nothing
                        for _ in min_count..max_count {
                            // TODO: Implement optional matching with backtracking
                            // For now, just require exact match
                            if max.is_some() && max.unwrap() > *min {
                                self.codegen_pass(expr);
                            }
                        }
                    }
                    _ => {
                        // TODO: Implement other quantifiers  
                        self.codegen_pass(expr);
                    }
                }
            }
            
            Regex::Group { expr, kind: _, .. } => {
                // Only handle capturing groups for now
                // Increment group counter and emit capture opcodes
                self.group_counter += 1;
                let group_id = self.group_counter;
                
                // Update max capture group for flags
                self.flags.max_capture_group = self.flags.max_capture_group.max(group_id);
                
                // Emit SaveStart to capture beginning of group
                self.emit_op(OpCode::SaveStart);
                self.code.push(group_id as u8);
                
                // Compile the inner expression
                self.codegen_pass(expr);
                
                // Emit SaveEnd to capture end of group
                self.emit_op(OpCode::SaveEnd);
                self.code.push(group_id as u8);
            }
            
            _ => {
                // TODO: Implement remaining AST nodes
                self.emit_op(OpCode::Fail);
            }
        }
    }
    
    /// Peephole optimization pass on generated bytecode
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
    
    /// Emit opcode with character operand
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
        use OpCode::*;
        match value {
            0x00 => Ok(Char),
            0x01 => Ok(Any),
            0x10 => Ok(DigitAscii),
            0x11 => Ok(DigitAsciiNeg),
            0x12 => Ok(WordAscii),
            0x13 => Ok(WordAsciiNeg),
            0x14 => Ok(SpaceAscii),
            0x15 => Ok(SpaceAsciiNeg),
            0x30 => Ok(StartLine),
            0x31 => Ok(EndLine),
            0x32 => Ok(StartText),
            0x33 => Ok(EndText),
            0x34 => Ok(EndTextOrNL),
            0x35 => Ok(WordBoundary),
            0x36 => Ok(NonWordBoundary),
            0x40 => Ok(Jump),
            0x41 => Ok(Split),
            0x50 => Ok(SaveStart),
            0x51 => Ok(SaveEnd),
            0x80 => Ok(QuestionGreedy),
            0x82 => Ok(StarGreedy),
            0x84 => Ok(PlusGreedy),
            0x90 => Ok(SetAlternative),
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
    fn test_sequence() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Char('a'),
            Regex::Char('b'),
        ]);
        let program = compiler.compile(&ast);
        
        let vm = RegexVM::new(program);
        assert!(vm.is_match("ab"));
        assert!(!vm.is_match("ba"));
    }

    #[test]
    fn test_anchor_start() {
        let mut compiler = OptimizingCompiler::new();
        let ast = Regex::Sequence(vec![
            Regex::Anchor(AnchorType::Start),
            Regex::Char('a'),
        ]);
        let program = compiler.compile(&ast);
        
        let vm = RegexVM::new(program);
        assert!(vm.is_match("a"));
        assert!(vm.is_match("ab"));
        assert!(!vm.is_match("ba")); // Doesn't start with 'a'
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
        assert!(vm.is_match(""));     // Zero matches
        assert!(vm.is_match("a"));    // One match
        assert!(vm.is_match("aa"));   // Two matches
        assert!(vm.is_match("aaa"));  // Three matches
        assert!(vm.is_match("b"));    // Zero matches, continues to match rest
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
        assert!(vm.is_match(""));     // Zero matches
        assert!(vm.is_match("a"));    // One match
        assert!(vm.is_match("b"));    // Zero matches, continues to match rest
        assert!(vm.is_match("aa"));   // One match, rest ignored
        assert!(vm.is_match("ab"));   // One match, rest ignored
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
        assert!(vm.is_match("b"));      // a* matches zero, b+ matches one
        assert!(vm.is_match("ab"));     // a* matches one, b+ matches one
        assert!(vm.is_match("abb"));    // a* matches one, b+ matches two
        assert!(vm.is_match("aabb"));   // a* matches two, b+ matches two
        assert!(!vm.is_match("a"));     // a* matches one, but b+ needs at least one
        assert!(!vm.is_match(""));      // b+ needs at least one
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
            println!("Debug: match = {:?}", m);
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
            Regex::Sequence(vec![
                Regex::Char('c'),
                Regex::Char('a'),
                Regex::Char('t'),
            ]),
            Regex::Sequence(vec![
                Regex::Char('d'),
                Regex::Char('o'),
                Regex::Char('g'),
            ]),
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
        assert!(!vm.is_match("ca"));    // Incomplete
        assert!(!vm.is_match("do"));    // Incomplete
    }

    #[test]
    fn test_complex_alternation() {
        let mut compiler = OptimizingCompiler::new();
        // Pattern: foo|bar|baz
        let ast = Regex::Alternation(vec![
            Regex::Sequence(vec![
                Regex::Char('f'),
                Regex::Char('o'),
                Regex::Char('o'),
            ]),
            Regex::Sequence(vec![
                Regex::Char('b'),
                Regex::Char('a'),
                Regex::Char('r'),
            ]),
            Regex::Sequence(vec![
                Regex::Char('b'),
                Regex::Char('a'),
                Regex::Char('z'),
            ]),
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
        assert!(!vm.is_match("ba"));   // Matches start of both "bar" and "baz" but neither fully
    }

    #[test]
    fn test_alternation_with_tracking() {
        let mut compiler = OptimizingCompiler::new();
        // Use same pattern as working test_alternation: cat|dog  
        let ast = Regex::Alternation(vec![
            Regex::Sequence(vec![
                Regex::Char('c'),
                Regex::Char('a'), 
                Regex::Char('t'),
            ]),
            Regex::Sequence(vec![
                Regex::Char('d'),
                Regex::Char('o'),
                Regex::Char('g'),
            ]),
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
}
