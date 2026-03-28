use crate::error::Result;
use crate::execution::{ExecContext, ExecResult, ExecutionManager};
use crate::pattern::CompiledPattern;
use crate::vm::RegexVM;
use crate::{trace_decision, trace_enter, trace_exit};
use std::sync::Arc;

/// Execution mode that controls performance vs feature tradeoffs
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Maximum performance, pure regex matching only
    Pure,
    /// Code execution in sandboxed environments only
    Safe,
    /// Enables the native-callback path in addition to the sandboxed backends
    Full,
}

/// Match result with position information
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchResult {
    /// Start position in bytes
    pub start: usize,
    /// End position in bytes
    pub end: usize,
    /// 1-based top-level branch number for top-level alternation matches.
    ///
    /// `None` when the pattern has no top-level alternation.
    pub matched_branch_number: Option<usize>,
}

/// High-performance regex execution engine
pub struct Engine {
    /// The compiled VM for pattern execution
    vm: RegexVM,
    /// Execution mode for this engine
    mode: ExecutionMode,
}

impl Engine {
    /// Create new engine from compiled pattern
    pub fn new(pattern: &CompiledPattern) -> Result<Self> {
        trace_enter!(
            "engine",
            "Engine::new",
            "mode={:?},bytecode_len={}",
            pattern.mode,
            pattern.program.code.len()
        );
        let execution_manager =
            if pattern.program.flags.has_code_blocks && pattern.mode != ExecutionMode::Pure {
                Some(Arc::new(ExecutionManager::new()))
            } else {
                None
            };
        let vm = RegexVM::with_execution_manager(pattern.program.clone(), execution_manager);
        let engine = Self {
            vm,
            mode: pattern.mode,
        };
        trace_exit!("engine", "Engine::new", "ok=true,mode={:?}", engine.mode);
        Ok(engine)
    }

    /// Find all non-overlapping matches in the input
    pub fn find_all(&self, text: &[u8]) -> Vec<MatchResult> {
        trace_enter!(
            "engine",
            "Engine::find_all",
            "input_bytes={},mode={:?}",
            text.len(),
            self.mode
        );
        // Convert bytes to string for VM processing
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => {
                trace_decision!(
                    "engine",
                    "std::str::from_utf8(text).is_ok()",
                    true,
                    "dispatching text to VM find_all path"
                );
                s
            }
            Err(err) => {
                trace_decision!(
                    "engine",
                    "std::str::from_utf8(text).is_ok()",
                    false,
                    "invalid UTF-8 input rejected: {}",
                    err
                );
                trace_exit!(
                    "engine",
                    "Engine::find_all",
                    "ok=true,matches=0,reason=invalid_utf8"
                );
                return Vec::new();
            } // Invalid UTF-8
        };

        let matches = self
            .vm
            .find_all(text_str)
            .into_iter()
            .map(|m| MatchResult {
                start: m.start,
                end: m.end,
                matched_branch_number: m.matched_alternative.map(|id| id + 1),
            })
            .collect::<Vec<_>>();
        trace_decision!(
            "engine",
            "matches.is_empty()",
            matches.is_empty(),
            "vm find_all produced {} matches",
            matches.len()
        );
        trace_exit!(
            "engine",
            "Engine::find_all",
            "ok=true,matches={}",
            matches.len()
        );
        matches
    }

    /// Find the first match in the input
    pub fn find_first(&self, text: &[u8]) -> Option<MatchResult> {
        trace_enter!(
            "engine",
            "Engine::find_first",
            "input_bytes={},mode={:?}",
            text.len(),
            self.mode
        );
        // Convert bytes to string for VM processing
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => {
                trace_decision!(
                    "engine",
                    "std::str::from_utf8(text).is_ok()",
                    true,
                    "dispatching text to VM find_first path"
                );
                s
            }
            Err(err) => {
                trace_decision!(
                    "engine",
                    "std::str::from_utf8(text).is_ok()",
                    false,
                    "invalid UTF-8 input rejected: {}",
                    err
                );
                trace_exit!(
                    "engine",
                    "Engine::find_first",
                    "ok=true,found=false,reason=invalid_utf8"
                );
                return None;
            }
        };

        let first = self.vm.find_first(text_str).map(|m| MatchResult {
            start: m.start,
            end: m.end,
            matched_branch_number: m.matched_alternative.map(|id| id + 1),
        });
        trace_decision!(
            "engine",
            "first.is_some()",
            first.is_some(),
            "vm find_first completed"
        );
        trace_exit!(
            "engine",
            "Engine::find_first",
            "ok=true,found={}",
            first.is_some()
        );
        first
    }

    /// Test if pattern matches the input (fastest operation)
    pub fn is_match(&self, text: &[u8]) -> bool {
        trace_enter!(
            "engine",
            "Engine::is_match",
            "input_bytes={},mode={:?}",
            text.len(),
            self.mode
        );
        // Convert bytes to string for VM processing
        if let Ok(text_str) = std::str::from_utf8(text) {
            trace_decision!(
                "engine",
                "std::str::from_utf8(text).is_ok()",
                true,
                "dispatching text to VM is_match path"
            );
            let matched = self.vm.is_match(text_str);
            trace_decision!(
                "engine",
                "vm.is_match(text_str)",
                matched,
                "boolean match evaluation completed"
            );
            trace_exit!("engine", "Engine::is_match", "ok=true,matched={}", matched);
            matched
        } else {
            trace_decision!(
                "engine",
                "std::str::from_utf8(text).is_ok()",
                false,
                "invalid UTF-8 input rejected"
            );
            trace_exit!(
                "engine",
                "Engine::is_match",
                "ok=true,matched=false,reason=invalid_utf8"
            );
            false // Invalid UTF-8 cannot match
        }
    }

    /// Register a native callback on the engine's execution manager.
    pub fn register_native<F>(&self, name: String, callback: F) -> Result<()>
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        self.vm.register_native(name, callback)
    }

    /// Register a named wasm module on the engine's execution manager.
    pub fn register_wasm_module(&self, name: String, module_bytes: Vec<u8>) -> Result<()> {
        self.vm.register_wasm_module(name, module_bytes)
    }

    /// Register or replace a host-provided execution variable on the engine's execution manager.
    pub fn set_variable(&self, name: String, value: String) -> Result<()> {
        self.vm.set_variable(name, value)
    }
}
