use crate::ast::Regex as RegexAst;
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
        if let Some(msg) = Self::parser_boundary_validation_message(&ast) {
            trace_exit!(
                "compiler",
                "Compiler::compile_ast_with_label",
                "error={}",
                msg
            );
            return Err(RgxError::Compile(msg));
        }
        let total_groups = Self::count_capture_groups(&ast);
        let named_groups = Self::collect_named_groups(&ast);
        let ast = Self::resolve_relative_conditionals(ast, total_groups)?;

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
                let inner_opened = if matches!(kind, crate::ast::GroupKind::Capturing) {
                    opened_groups + 1
                } else {
                    opened_groups
                };
                let (expr, opened_after_expr) =
                    Self::resolve_relative_conditionals_inner(*expr, inner_opened, total_groups)?;
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

    fn parser_boundary_validation_message(ast: &RegexAst) -> Option<String> {
        match ast {
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => items
                .iter()
                .find_map(Self::parser_boundary_validation_message),
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => Self::parser_boundary_validation_message(expr),
            RegexAst::Group { expr, kind, .. } => {
                if matches!(kind, crate::ast::GroupKind::BranchReset) {
                    Some(
                        "branch-reset groups '(?|...)' are parser-recognized but not yet executed by rgx"
                            .to_string(),
                    )
                } else {
                    Self::parser_boundary_validation_message(expr)
                }
            }
            RegexAst::ExtendedCharClass { .. } => Some(
                "Perl extended character classes '(?[...])' are parser-recognized but not yet executed by rgx"
                    .to_string(),
            ),
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
            crate::ast::ConditionalTest::Define => {
                Ok((crate::ast::ConditionalTest::Define, opened_groups))
            }
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
            RegexAst::ExtendedCharClass { .. } => Some(
                "Perl extended character classes '(?[...])' are parser-recognized but not yet executed by rgx"
                    .to_string(),
            ),
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
            RegexAst::Group { expr, kind, .. } => {
                if matches!(kind, crate::ast::GroupKind::BranchReset) {
                    Some(
                        "branch-reset groups '(?|...)' are parser-recognized but not yet executed by rgx"
                            .to_string(),
                    )
                } else {
                    self.feature_validation_message(expr, total_groups, named_groups)
                }
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
        let total_groups = Self::count_capture_groups(ast);
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

    fn count_capture_groups(ast: &RegexAst) -> u32 {
        match ast {
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => {
                items.iter().map(Self::count_capture_groups).sum()
            }
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => Self::count_capture_groups(expr),
            RegexAst::Group { expr, kind, .. } => {
                let current = u32::from(matches!(kind, crate::ast::GroupKind::Capturing));
                current + Self::count_capture_groups(expr)
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let condition_count = match condition {
                    crate::ast::ConditionalTest::Lookahead { expr, .. }
                    | crate::ast::ConditionalTest::Lookbehind { expr, .. } => {
                        Self::count_capture_groups(expr)
                    }
                    crate::ast::ConditionalTest::GroupExists(_)
                    | crate::ast::ConditionalTest::RelativeGroupExists(_)
                    | crate::ast::ConditionalTest::NamedGroupExists(_)
                    | crate::ast::ConditionalTest::Define => 0,
                };
                condition_count
                    + Self::count_capture_groups(true_branch)
                    + false_branch
                        .as_ref()
                        .map_or(0, |branch| Self::count_capture_groups(branch))
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
        let mut next_group = 0;
        Self::collect_named_groups_inner(ast, &mut next_group, &mut named_groups);
        named_groups
    }

    fn collect_named_groups_inner(
        ast: &RegexAst,
        next_group: &mut u32,
        named_groups: &mut std::collections::HashMap<String, u32>,
    ) {
        match ast {
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => {
                for item in items {
                    Self::collect_named_groups_inner(item, next_group, named_groups);
                }
            }
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => {
                Self::collect_named_groups_inner(expr, next_group, named_groups);
            }
            RegexAst::Group {
                expr, kind, name, ..
            } => {
                if matches!(kind, crate::ast::GroupKind::Capturing) {
                    *next_group += 1;
                    if let Some(name) = name {
                        named_groups.insert(name.clone(), *next_group);
                    }
                }
                Self::collect_named_groups_inner(expr, next_group, named_groups);
            }
            RegexAst::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                match condition {
                    crate::ast::ConditionalTest::Lookahead { expr, .. }
                    | crate::ast::ConditionalTest::Lookbehind { expr, .. } => {
                        Self::collect_named_groups_inner(expr, next_group, named_groups);
                    }
                    crate::ast::ConditionalTest::GroupExists(_)
                    | crate::ast::ConditionalTest::RelativeGroupExists(_)
                    | crate::ast::ConditionalTest::NamedGroupExists(_)
                    | crate::ast::ConditionalTest::Define => {}
                }
                Self::collect_named_groups_inner(true_branch, next_group, named_groups);
                if let Some(false_branch) = false_branch {
                    Self::collect_named_groups_inner(false_branch, next_group, named_groups);
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
