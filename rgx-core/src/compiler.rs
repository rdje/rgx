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
        let ast = Self::assign_capture_indices(ast);
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
        let total_groups = Self::max_capture_group(&ast);
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

    fn assign_capture_indices(ast: RegexAst) -> RegexAst {
        let (ast, _) = Self::assign_capture_indices_inner(ast, 1);
        ast
    }

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
            } => match kind {
                crate::ast::GroupKind::Capturing => {
                    let group_id = next_group;
                    let (expr, next) =
                        Self::assign_capture_indices_inner(*expr, group_id.saturating_add(1));
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
                    let (expr, next) = Self::assign_branch_reset_expr(*expr, next_group);
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
                    let (expr, next) = Self::assign_capture_indices_inner(*expr, next_group);
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
            },
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
            | RegexAst::Recursion { .. }
            | RegexAst::CodeBlock { .. }
            | RegexAst::Empty => (ast, next_group),
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
            crate::ast::ConditionalTest::Define => {
                (crate::ast::ConditionalTest::Define, next_group)
            }
        }
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
                let (expr, opened_after_expr) =
                    if matches!(kind, crate::ast::GroupKind::BranchReset) {
                        Self::resolve_relative_conditionals_branch_reset(
                            *expr,
                            opened_groups,
                            total_groups,
                        )?
                    } else {
                        let inner_opened = if matches!(kind, crate::ast::GroupKind::Capturing) {
                            index.unwrap_or_else(|| opened_groups.saturating_add(1))
                        } else {
                            opened_groups
                        };
                        Self::resolve_relative_conditionals_inner(
                            *expr,
                            inner_opened,
                            total_groups,
                        )?
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
            | RegexAst::Lookbehind { expr, .. } => Self::parser_boundary_validation_message(expr),
            RegexAst::Group { expr, .. } => Self::parser_boundary_validation_message(expr),
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
            RegexAst::Group { expr, .. } => {
                self.feature_validation_message(expr, total_groups, named_groups)
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
        let total_groups = Self::max_capture_group(ast);
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

    fn max_capture_group(ast: &RegexAst) -> u32 {
        match ast {
            RegexAst::Sequence(items) | RegexAst::Alternation(items) => {
                items.iter().map(Self::max_capture_group).max().unwrap_or(0)
            }
            RegexAst::Quantified { expr, .. }
            | RegexAst::Lookahead { expr, .. }
            | RegexAst::Lookbehind { expr, .. } => Self::max_capture_group(expr),
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
            | RegexAst::Recursion { .. }
            | RegexAst::CodeBlock { .. }
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
            | RegexAst::Lookbehind { expr, .. } => {
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
