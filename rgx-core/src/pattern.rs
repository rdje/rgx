use crate::engine::ExecutionMode;
use crate::ast::Regex as RegexAst;
use crate::vm::Program;

/// A compiled regex pattern with AST and optimized VM bytecode
#[derive(Clone)]
pub struct CompiledPattern {
    /// Original pattern string
    pub raw: String,
    /// Execution mode for this pattern
    pub mode: ExecutionMode,
    /// Parsed AST representation
    pub ast: RegexAst,
    /// Compiled VM program with optimizations
    pub program: Program,
}

/// Pattern builder and analysis utilities
pub struct Pattern;

