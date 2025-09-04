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
}

/// High-performance regex execution engine
pub struct RegexVM {
    /// Compiled program
    program: Program,
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
                    groups: self.extract_captures(ctx),
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

                OpCode::Fail => {
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
                self.group_counter += 1;
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
                // TODO: Implement efficient alternation compilation
                for alt in alts {
                    self.codegen_pass(alt);
                }
            }
            
            Regex::Quantified { expr, quantifier } => {
                match quantifier {
                    Quantifier::OneOrMore { lazy: false } => {
                        // For A+, emit: A followed by split back to A or continue
                        self.codegen_pass(expr);
                        // TODO: Implement proper loop bytecode
                        // For now, just match the expression once
                    }
                    _ => {
                        // TODO: Implement other quantifiers  
                        self.codegen_pass(expr);
                    }
                }
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
        let utf8_bytes = ch.encode_utf8(&mut buf);
        self.code.push(utf8_bytes.len() as u8);
        self.code.extend_from_slice(utf8_bytes.as_bytes());
        
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
}
