use crate::ast::Regex as RegexAst;
use crate::engine::ExecutionMode;
use crate::error::{Result, RgxError};
use crate::parsing;
use crate::pattern::CompiledPattern;
use crate::vm::OptimizingCompiler as VMCompiler;
use crate::{debug_log, low_log, trace_decision, trace_enter, trace_exit, trace_log};

/// Compiler that transforms regex patterns into optimized execution programs
pub struct Compiler {
    mode: ExecutionMode,
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
        debug_log!("compiler", "AST: {:?}", ast);
        if let Some(msg) = Self::unsupported_feature_message(&ast) {
            trace_decision!(
                "compiler",
                "unsupported_feature_message(ast).is_some()",
                true,
                "rejecting unsupported AST node: {}",
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
            "unsupported_feature_message(ast).is_some()",
            false,
            "AST is eligible for VM compilation"
        );

        // Compile AST into optimized VM bytecode
        debug_log!("compiler", "Compiling AST to VM bytecode...");
        let mut vm_compiler = VMCompiler::new();
        let program = vm_compiler.compile(&ast);

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

    fn unsupported_feature_message(ast: &RegexAst) -> Option<&'static str> {
        match ast {
            RegexAst::CodeBlock { .. } => {
                Some("code-block syntax is parsed but not yet integrated into VM execution")
            }
            RegexAst::Backreference(_) => {
                Some("backreferences are parsed but not yet integrated into VM execution")
            }
            RegexAst::UnicodeClass { .. }
            | RegexAst::CharClass(crate::ast::CharClass::UnicodeClass { .. }) => {
                Some("unicode property classes are parsed but not yet integrated into VM execution")
            }
            RegexAst::Recursion { .. } => {
                Some("recursion syntax is parsed but not yet integrated into VM execution")
            }
            RegexAst::Conditional { .. } => {
                Some("conditional syntax is parsed but not yet integrated into VM execution")
            }
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => {
                items.iter().find_map(Self::unsupported_feature_message)
            }
            RegexAst::Quantified { expr, .. }
            | RegexAst::Group { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => Self::unsupported_feature_message(expr),
            _ => None,
        }
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}
