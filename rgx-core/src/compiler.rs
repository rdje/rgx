use crate::error::{Result, RgxError};
use crate::engine::ExecutionMode;
use crate::pattern::CompiledPattern;
use crate::parsing;
use crate::vm::{OptimizingCompiler as VMCompiler};

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
        if pattern.is_empty() {
            return Err(RgxError::Compile("empty pattern".into()));
        }
        
        // Parse pattern into AST using zero-cost compile-time selected parser
        let ast = parsing::parse_pattern(pattern)?;
        
        // Debug output for failing patterns
        if pattern.contains("{3}")||pattern.contains("\\b")||pattern.contains("@") {
            println!("[DEBUG] Pattern: '{}'", pattern);
            println!("[DEBUG] AST: {:?}", ast);
        }
        
        // Compile AST into optimized VM bytecode
        let mut vm_compiler = VMCompiler::new();
        let program = vm_compiler.compile(&ast);
        
        if pattern.contains("{3}")||pattern.contains("\\b")||pattern.contains("@") {
            println!("[DEBUG] Program: {:?}", program);
        }
        
        Ok(CompiledPattern {
            raw: pattern.to_string(),
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

