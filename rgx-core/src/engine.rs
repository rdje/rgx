use crate::c2::dfa::{DfaSearchOutcome, LazyDfa};
use crate::c2::Classification;
use crate::error::Result;
use crate::events::MatchEvent;
use crate::execution::{
    CodeBlockValue, ExecContext, ExecResult, ExecutionManager, MatchContinuation, MatchOutcome,
};
use crate::pattern::CompiledPattern;
use crate::vm::RegexVM;
use crate::{trace_decision, trace_enter, trace_exit};
use std::sync::{Arc, Mutex};

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

/// Controls how alternation matches are selected.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatchSemantics {
    /// Leftmost-first: the first alternative that matches wins (PCRE2/Perl default).
    /// `a|ab` on "ab" → matches "a".
    LeftmostFirst,
    /// Leftmost-longest: at each position, the longest match wins (POSIX semantics).
    /// `a|ab` on "ab" → matches "ab".
    LeftmostLongest,
}

impl Default for MatchSemantics {
    fn default() -> Self {
        Self::LeftmostFirst
    }
}

/// Result of a partial-match query.
#[derive(Clone, Debug, PartialEq)]
pub enum PartialMatchResult {
    /// A full match was found.
    Full(MatchResult),
    /// The input ended mid-potential-match. More data might complete the match.
    /// Contains the byte offset where the partial match started.
    Partial(usize),
    /// No match and no partial match — the pattern cannot match this input
    /// even with more data appended.
    NoMatch,
}

/// Match result with position information
#[derive(Clone, Debug, PartialEq)]
pub struct MatchResult {
    /// Start position in bytes
    pub start: usize,
    /// End position in bytes
    pub end: usize,
    /// Capture groups as `(start, end)` byte pairs.
    ///
    /// Index 0 is the overall match. Indices 1..N correspond to numbered
    /// capture groups. `None` means the group did not participate in the match.
    pub groups: Vec<Option<(usize, usize)>>,
    /// 1-based top-level branch number for top-level alternation matches.
    ///
    /// `None` when the pattern has no top-level alternation.
    pub matched_branch_number: Option<usize>,
    /// Last non-boolean code-block value observed on the winning match path.
    ///
    /// This is `None` when the winning path produced only predicate-style
    /// success/failure results.
    pub code_result: Option<CodeBlockValue>,
}

/// High-performance regex execution engine
pub struct Engine {
    /// The compiled VM for pattern execution
    vm: RegexVM,
    /// Execution mode for this engine
    mode: ExecutionMode,
    /// C2 lazy DFA for `is_match` dispatch (step 5b). Built in
    /// [`Engine::new`] only when the pattern's AST is DFA-eligible
    /// (`c2::program::is_c2_dfa_eligible`). Wrapped in `Mutex` because
    /// the DFA's `transition` method mutates its state cache, and
    /// public `Regex` API methods are `&self`.
    c2_dfa: Option<Mutex<LazyDfa>>,
}

/// Convert a VM-level `Match` into a public `MatchResult`.
pub(crate) fn vm_match_to_result(m: crate::vm::Match) -> MatchResult {
    MatchResult {
        start: m.start,
        end: m.end,
        groups: m.groups,
        matched_branch_number: m.matched_alternative.map(|id| id + 1),
        code_result: m.code_result,
    }
}

/// Build a `Mutex<LazyDfa>` for the given AST + C2 program if the
/// pattern is DFA-eligible. Returns `None` if the pattern lacks a C2
/// program (Pike-VM not eligible) or if the AST contains constructs
/// the DFA can't handle (assertions, lazy quantifiers — see
/// [`crate::c2::program::is_c2_dfa_eligible`]). C2 step 5b.
fn build_dfa_if_eligible(
    ast: &crate::ast::Regex,
    c2_program: &Option<crate::c2::CompiledC2Program>,
) -> Option<Mutex<LazyDfa>> {
    let c2 = c2_program.as_ref()?;
    if !crate::c2::program::is_c2_dfa_eligible(ast) {
        return None;
    }
    // Clone the NFA and byte-class map into Arcs so the DFA can own
    // shared references. Cloning is O(states + transitions) — small
    // for typical patterns.
    let nfa = Arc::new(c2.forward_anchored.clone());
    let bcm = Arc::new(c2.byte_class_map.clone());
    LazyDfa::new(nfa, bcm, LazyDfa::DEFAULT_STATE_LIMIT)
        .ok()
        .map(Mutex::new)
}

impl Engine {
    /// Create new engine from compiled pattern
    ///
    /// # Errors
    /// Returns an error if engine initialization fails for the given compiled pattern.
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
        // C2 step 5b: build the lazy DFA if the pattern is DFA-eligible.
        // The DFA serves `is_match` dispatch via `should_dispatch_to_dfa`.
        let c2_dfa = build_dfa_if_eligible(&pattern.ast, &pattern.program.c2_program);
        let vm = RegexVM::with_execution_manager(pattern.program.clone(), execution_manager);
        let engine = Self {
            vm,
            mode: pattern.mode,
            c2_dfa,
        };
        trace_exit!("engine", "Engine::new", "ok=true,mode={:?}", engine.mode);
        Ok(engine)
    }

    /// Returns the lazy DFA for this engine if the pattern is DFA-
    /// eligible AND the runtime state allows DFA dispatch (no event
    /// observer, no runtime safety limits — same constraints as
    /// [`Engine::should_dispatch_to_c2`]).
    ///
    /// The returned `Mutex` must be locked by the caller. The DFA's
    /// `transition` method mutates its state cache, so the lock is
    /// required even from `&self`.
    #[doc(hidden)]
    pub fn should_dispatch_to_dfa(&self) -> Option<&Mutex<LazyDfa>> {
        let dfa = self.c2_dfa.as_ref()?;
        if self.vm.has_event_observer() {
            return None;
        }
        if self.vm.has_runtime_match_limits() {
            return None;
        }
        Some(dfa)
    }

    /// Try to answer `is_match` via the lazy DFA. Returns:
    /// - `Some(true)` / `Some(false)` if the DFA produced a definitive answer
    /// - `None` if the DFA isn't available, the pattern isn't DFA-
    ///   eligible, runtime state forbids DFA dispatch, or the cache
    ///   exhausted during the scan
    ///
    /// The caller (`Regex::is_match` in `lib.rs`) falls back to the
    /// Pike-VM (and ultimately the existing backtracking VM) when this
    /// returns `None`.
    #[doc(hidden)]
    pub fn try_dfa_is_match(&self, input: &[u8]) -> Option<bool> {
        let dfa_mutex = self.should_dispatch_to_dfa()?;
        let mut dfa = dfa_mutex.lock().ok()?;
        // Scan from each position. The DFA finds the first scan
        // position that yields a match. The simulator might exhaust
        // its cache mid-scan; in that case bail and let the caller
        // fall back.
        for start in 0..=input.len() {
            match dfa.find_match_at(input, start) {
                DfaSearchOutcome::Match(_) => return Some(true),
                DfaSearchOutcome::NoMatch => continue,
                DfaSearchOutcome::Exhausted => return None,
            }
        }
        Some(false)
    }

    /// C2 classification of the compiled pattern this engine was built for.
    ///
    /// At C2 step 4c, this remains the source of truth for "is the
    /// pattern classifier-positive". Engine dispatch is decided by
    /// [`Engine::c2_program`] which adds a structural eligibility check
    /// on top of the classification.
    #[doc(hidden)]
    pub fn classification(&self) -> &Classification {
        &self.vm.program.classification
    }

    /// The compiled C2 program for this engine, if the pattern is
    /// **structurally** eligible for Pike-VM dispatch (classifier
    /// positive plus the C2 structural eligibility checks). This is the
    /// raw compile-time check; runtime state (event observers, step
    /// limits) is not considered. Use [`Engine::should_dispatch_to_c2`]
    /// for the full dispatch decision.
    #[doc(hidden)]
    pub fn c2_program(&self) -> Option<&crate::c2::CompiledC2Program> {
        self.vm.program.c2_program.as_ref()
    }

    /// Returns `Some(c2_program)` iff this engine should route the
    /// next API call through the C2 Pike-VM. Combines the compile-time
    /// `c2_program` presence check with the runtime state checks for
    /// features the Pike-VM doesn't yet implement: match event
    /// observers ([`crate::vm::RegexVM::has_event_observer`]) and
    /// runtime safety limits
    /// ([`crate::vm::RegexVM::has_runtime_match_limits`]).
    ///
    /// When `None`, the caller falls back to the existing backtracking
    /// VM. The runtime checks are read on every call so that
    /// `Regex::on_event(...)` and `Regex::set_max_steps(...)` take
    /// effect immediately even if invoked AFTER `Regex::compile`.
    #[doc(hidden)]
    pub fn should_dispatch_to_c2(&self) -> Option<&crate::c2::CompiledC2Program> {
        let c2 = self.vm.program.c2_program.as_ref()?;
        if self.vm.has_event_observer() {
            return None;
        }
        if self.vm.has_runtime_match_limits() {
            return None;
        }
        Some(c2)
    }

    /// Find all non-overlapping matches in the input
    #[must_use]
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
            .map(vm_match_to_result)
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

    /// Find the first match, accepting a pre-validated `&str` directly.
    ///
    /// Used by `bytes::BytesRegex` which handles the UTF-8 boundary itself.
    #[must_use]
    pub(crate) fn vm_find_first(&self, text: &str) -> Option<MatchResult> {
        self.vm.find_first(text).map(vm_match_to_result)
    }

    /// Find all matches, accepting a pre-validated `&str` directly.
    #[must_use]
    pub(crate) fn vm_find_all(&self, text: &str) -> Vec<MatchResult> {
        self.vm
            .find_all(text)
            .into_iter()
            .map(vm_match_to_result)
            .collect()
    }

    /// Find the first match in the input
    #[must_use]
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

        let first = self.vm.find_first(text_str).map(vm_match_to_result);
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
    #[must_use]
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

    /// Find the first match starting the scan at byte position `start`.
    ///
    /// Positions in the returned `MatchResult` are absolute (relative to the
    /// beginning of `text`, not relative to `start`).
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn find_first_at(&self, text: &[u8], start: usize) -> Option<MatchResult> {
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(_) => return None,
        };
        self.vm
            .find_first_at(text_str, start)
            .map(vm_match_to_result)
    }

    /// Find all non-overlapping matches starting the scan at byte position `start`.
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn find_all_at(&self, text: &[u8], start: usize) -> Vec<MatchResult> {
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        self.vm
            .find_all_at(text_str, start)
            .into_iter()
            .map(vm_match_to_result)
            .collect()
    }

    /// Boolean match test starting the scan at byte position `start`.
    ///
    /// # Panics
    /// Panics if `start` is not on a UTF-8 character boundary.
    #[must_use]
    pub fn is_match_at(&self, text: &[u8], start: usize) -> bool {
        self.find_first_at(text, start).is_some()
    }

    /// Find the first match with support for async callback suspension.
    ///
    /// This is the suspendable counterpart to [`find_first`](Self::find_first).
    /// When an unregistered native callback is encountered, returns
    /// [`MatchOutcome::Suspended`] with a continuation that can be resumed
    /// after the callback is resolved externally.
    #[must_use]
    pub fn find_first_suspendable(&self, text: &[u8]) -> MatchOutcome {
        let Ok(text_str) = std::str::from_utf8(text) else {
            return MatchOutcome::Completed(None);
        };
        self.vm.find_first_suspendable(text_str)
    }

    /// Resume a suspended match after the caller resolves an async callback.
    ///
    /// See [`MatchContinuation`] for details on the continuation-passing protocol.
    #[must_use]
    pub fn resume(
        &self,
        continuation: MatchContinuation,
        callback_result: ExecResult,
    ) -> MatchOutcome {
        self.vm.resume(continuation, callback_result)
    }

    /// Named capture group map: group name → 1-based group number.
    #[must_use]
    pub fn named_groups(&self) -> &std::collections::HashMap<String, u32> {
        &self.vm.program.named_groups
    }

    /// Number of capture groups in the compiled program (excluding group 0).
    #[must_use]
    pub fn num_groups(&self) -> u32 {
        self.vm.program.num_groups
    }

    /// Find the first match, or report a partial match when the input
    /// ends mid-potential-match.
    #[must_use]
    pub fn find_first_partial(&self, text: &[u8]) -> PartialMatchResult {
        let text_str = match std::str::from_utf8(text) {
            Ok(s) => s,
            Err(_) => return PartialMatchResult::NoMatch,
        };
        self.vm.find_first_partial(text_str)
    }

    /// Set the match semantics (leftmost-first or leftmost-longest).
    pub fn set_match_semantics(&self, semantics: MatchSemantics) {
        self.vm.set_match_semantics(semantics);
    }

    /// Set the maximum number of opcode steps per match attempt.
    ///
    /// Prevents exponential backtracking from hanging the engine on
    /// pathological patterns like `(a+)+b`. When the limit is reached,
    /// the match attempt fails (returns no-match). Pass `None` to
    /// remove the limit (default).
    pub fn set_max_steps(&self, limit: Option<u64>) {
        self.vm.set_max_steps(limit);
    }

    /// Set the maximum backtrack stack depth per match attempt.
    pub fn set_max_backtrack_frames(&self, limit: Option<u64>) {
        self.vm.set_max_backtrack_frames(limit);
    }

    /// Set the maximum recursion depth per match attempt.
    pub fn set_max_recursion_depth(&self, limit: Option<u64>) {
        self.vm.set_max_recursion_depth(limit);
    }

    /// Register a native callback on the engine's execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this engine.
    pub fn register_native<F>(&self, name: &str, callback: F) -> Result<()>
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        self.vm.register_native(name, callback)
    }

    /// Register a named wasm module on the engine's execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached or the WASM module is invalid.
    pub fn register_wasm_module(&self, name: String, module_bytes: Vec<u8>) -> Result<()> {
        self.vm.register_wasm_module(name, module_bytes)
    }

    /// Register or replace a host-provided execution variable on the engine's execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this engine.
    pub fn set_variable(&self, name: &str, value: String) -> Result<()> {
        self.vm.set_variable(name, value)
    }

    /// Register or replace a typed host-provided execution variable on the engine's execution manager.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this engine.
    pub fn set_typed_variable(&self, name: &str, value: crate::execution::Value) -> Result<()> {
        self.vm.set_typed_variable(name, value)
    }

    /// Set a host variable with automatic type conversion.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if no execution manager is attached to this engine.
    pub fn set_var<V: Into<crate::execution::Value>>(&self, name: &str, value: V) -> Result<()> {
        self.set_typed_variable(name, value.into())
    }

    /// Register an event observer for structured match events.
    ///
    /// The observer receives [`MatchEvent`] values at key execution points.
    /// Only one observer may be active; calling this again replaces any
    /// previous observer.
    pub fn set_event_observer<F>(&self, observer: F)
    where
        F: Fn(&MatchEvent) + Send + Sync + 'static,
    {
        self.vm.set_event_observer(observer);
    }
}
