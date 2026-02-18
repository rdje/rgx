use crate::error::{Result, RgxError};
use crate::engine::ExecutionMode;
use crate::ast::Regex as RegexAst;
use crate::pattern::CompiledPattern;
use crate::parsing;
use crate::vm::{OptimizingCompiler as VMCompiler};
use crate::{debug_log, trace_log};

/// Compiler that transforms regex patterns into optimized execution programs
pub struct Compiler { 
    mode: ExecutionMode 
}

impl Compiler {
    /// Create new compiler with pure execution mode (maximum performance)
    pub fn new() -> Self { 
        Self { mode: ExecutionMode::Pure } 
    }
    
    /// Create compiler with specific execution mode
    pub fn with_mode(mode: ExecutionMode) -> Self { 
        Self { mode } 
    }

    /// Compile regex pattern into optimized bytecode program
    pub fn compile(&self, pattern: &str) -> Result<CompiledPattern> {
        debug_log!("compiler", "=== STARTING COMPILATION ===");
        debug_log!("compiler", "Pattern: '{}'", pattern);
        debug_log!("compiler", "Mode: {:?}", self.mode);
        
        if pattern.is_empty() {
            debug_log!("compiler", "ERROR: Empty pattern");
            return Err(RgxError::Compile("empty pattern".into()));
        }
        
        // Parse pattern into AST using zero-cost compile-time selected parser
        debug_log!("compiler", "Parsing pattern into AST...");
        let ast = parsing::parse_pattern(pattern)?;
        self.compile_ast_with_label(ast, pattern)
    }

    /// Compile a pre-built AST into optimized VM bytecode.
    ///
    /// This enables parser-independent development of VM/compiler/engine
    /// while parser work progresses in parallel.
    pub fn compile_ast(&self, ast: RegexAst) -> Result<CompiledPattern> {
        debug_log!("compiler", "=== STARTING AST-ONLY COMPILATION ===");
        debug_log!("compiler", "Mode: {:?}", self.mode);
        self.compile_ast_with_label(ast, "<ast>")
    }

    fn compile_ast_with_label(&self, ast: RegexAst, raw_label: &str) -> Result<CompiledPattern> {
        debug_log!("compiler", "AST: {:?}", ast);

        // Compile AST into optimized VM bytecode
        debug_log!("compiler", "Compiling AST to VM bytecode...");
        let mut vm_compiler = VMCompiler::new();
        let program = vm_compiler.compile(&ast);

        debug_log!("compiler", "Program compiled:");
        debug_log!("compiler", "  - Bytecode length: {} bytes", program.code.len());
        debug_log!("compiler", "  - Character classes: {}", program.char_classes.len());
        debug_log!("compiler", "  - String literals: {}", program.string_literals.len());
        debug_log!("compiler", "  - Capture groups: {}", program.num_groups);
        debug_log!("compiler", "  - Flags: {:?}", program.flags);
        debug_log!("compiler", "  - Stats: {:?}", program.stats);

        trace_log!("compiler", "Full program: {:?}", program);

        // Hex dump the bytecode for debugging
        crate::log::hex_dump("compiler", "VM Bytecode", &program.code);

        debug_log!("compiler", "=== COMPILATION COMPLETE ===");

        Ok(CompiledPattern {
            raw: raw_label.to_string(),
            mode: self.mode,
            ast,
            program,
        })
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

