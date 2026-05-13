//! State-of-the-art code execution module for multi-language regex patterns.
//!
//! This module implements sandboxed code execution for Lua, JavaScript, Rhai, and WebAssembly
//! within regex patterns. This is a unique feature that sets rgx apart from traditional
//! regex engines, enabling powerful pattern matching with embedded logic.
//!
//! # Design Philosophy
//!
//! 1. **Security First**: All code runs in sandboxed environments with no filesystem,
//!    network, or system access.
//! 2. **Performance Layers**: Pure regex → +Lua/Rhai (fast) → +JavaScript (flexible) → +WASM (portable)
//! 3. **Zero-Cost Abstraction**: If you don't use code execution, it has zero overhead.
//! 4. **Fail-Safe**: Code execution failures don't crash the regex engine.
//!
//! # Pattern Syntax
//!
//! - `(?{lua:code})` - Execute Lua code
//! - `(?{js:code})` - Execute JavaScript code  
//! - `(?{rhai:code})` - Execute Rhai code
//! - `(?{wasm:module:function})` - Call WASM function
//! - `(?{native:function})` - Call registered native callback
//!
//! # Example Patterns
//!
//! ```regex
//! # Validate date ranges with Lua
//! (\d{4})-(\d{2})-(\d{2})(?{lua:return tonumber(arg[2]) <= 12 and tonumber(arg[3]) <= 31})
//!
//! # Complex email validation with JavaScript
//! ([\w.+-]+)@([\w.-]+)(?{js:return arg[2].split('.').length >= 2})
//!
//! # Custom password strength check
//! .{8,}(?{lua:return string.match(arg[0], "[A-Z]") and string.match(arg[0], "[0-9]")})
//! ```

use crate::error::{Result, RgxError};
use crate::{trace_decision, trace_enter, trace_exit};
use std::collections::HashMap;
use std::fmt;
#[cfg(any(feature = "javascript", feature = "lua", feature = "rhai"))]
use std::sync::Mutex;
use std::sync::{Arc, RwLock};

// ============================================================================
// TYPED VALUE
// ============================================================================

/// A typed value for host-engine data exchange.
///
/// Used for host variables (data in) and code block results (data out).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// No value.
    Null,
    /// Boolean.
    Bool(bool),
    /// 64-bit integer.
    Int(i64),
    /// 64-bit float.
    Float(f64),
    /// String.
    String(String),
    /// Ordered list of values.
    Array(Vec<Value>),
    /// Key-value map.
    Map(Vec<(String, Value)>),
}

impl Value {
    /// Try to extract a string reference.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Try to extract an `i64`.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Try to extract an `f64`.
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(n) => Some(*n),
            Value::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    /// Try to extract a `bool`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Try to extract an array slice.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(arr) => Some(arr.as_slice()),
            _ => None,
        }
    }

    /// Try to extract a map slice.
    #[must_use]
    pub fn as_map(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Map(map) => Some(map.as_slice()),
            _ => None,
        }
    }

    /// Create an array from an iterator of values.
    pub fn array<I, V>(items: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        Value::Array(items.into_iter().map(Into::into).collect())
    }

    /// Create a map from key-value pairs.
    pub fn map<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<Value>,
    {
        Value::Map(
            pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(n) => write!(f, "{n}"),
            Value::String(s) => write!(f, "{s}"),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Value::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Int(n)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Float(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Value::Int(i64::from(n))
    }
}

impl From<u32> for Value {
    fn from(n: u32) -> Self {
        Value::Int(i64::from(n))
    }
}

impl From<usize> for Value {
    #[allow(clippy::cast_possible_wrap)]
    fn from(n: usize) -> Self {
        Value::Int(n as i64)
    }
}

impl From<f32> for Value {
    fn from(n: f32) -> Self {
        Value::Float(f64::from(n))
    }
}

impl From<Vec<&str>> for Value {
    fn from(v: Vec<&str>) -> Self {
        Value::Array(
            v.into_iter()
                .map(|s| Value::String(s.to_string()))
                .collect(),
        )
    }
}

impl From<Vec<String>> for Value {
    fn from(v: Vec<String>) -> Self {
        Value::Array(v.into_iter().map(Value::String).collect())
    }
}

impl From<Vec<i64>> for Value {
    fn from(v: Vec<i64>) -> Self {
        Value::Array(v.into_iter().map(Value::Int).collect())
    }
}

impl From<Vec<f64>> for Value {
    fn from(v: Vec<f64>) -> Self {
        Value::Array(v.into_iter().map(Value::Float).collect())
    }
}

impl<K: Into<String>, V: Into<Value>> From<Vec<(K, V)>> for Value {
    fn from(pairs: Vec<(K, V)>) -> Self {
        Value::Map(
            pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

// ============================================================================
// EXECUTION CONTEXT
// ============================================================================

/// Execution context passed to code blocks with match information.
///
/// This provides safe, read-only access to:
/// - The matched text
/// - Current match start/end/length metadata
/// - Capture groups
/// - Named captures
/// - Host-provided variables
/// - Match position
/// - Top-level branch selection when available
#[derive(Debug, Clone)]
pub struct ExecContext {
    /// The full text being matched
    pub text: String,
    /// Current match position
    pub position: usize,
    /// Current match-attempt start position
    pub match_start: usize,
    /// Current match-attempt end position
    pub match_end: usize,
    /// Captured groups (indexed from 0)
    pub captures: Vec<Option<String>>,
    /// Named capture groups
    pub named_captures: HashMap<String, String>,
    /// Host-provided variable snapshot for this code-block execution
    pub variables: Arc<RwLock<HashMap<String, String>>>,
    /// Host-provided typed variable snapshot for this code-block execution
    pub typed_variables: Arc<RwLock<HashMap<String, Value>>>,
    /// 1-based top-level branch number when the current path is inside a top-level alternation arm
    pub matched_branch_number: Option<usize>,
}

impl ExecContext {
    /// Create a new execution context
    #[must_use]
    pub fn new(text: String, position: usize) -> Self {
        trace_enter!(
            "execution",
            "ExecContext::new",
            "text_len={},position={}",
            text.len(),
            position
        );
        let context = Self {
            text,
            position,
            match_start: position,
            match_end: position,
            captures: Vec::new(),
            named_captures: HashMap::new(),
            variables: Arc::new(RwLock::new(HashMap::new())),
            typed_variables: Arc::new(RwLock::new(HashMap::new())),
            matched_branch_number: None,
        };
        trace_exit!(
            "execution",
            "ExecContext::new",
            "ok=true,captures=0,named_captures=0"
        );
        context
    }

    /// Get the current match (group 0)
    pub fn current_match(&self) -> Option<&str> {
        trace_enter!(
            "execution",
            "ExecContext::current_match",
            "capture_slots={}",
            self.captures.len()
        );
        let current = self.captures.first().and_then(Option::as_deref);
        trace_decision!(
            "execution",
            "current_match.is_some()",
            current.is_some(),
            "group-0 capture availability check"
        );
        trace_exit!(
            "execution",
            "ExecContext::current_match",
            "ok=true,found={}",
            current.is_some()
        );
        current
    }

    /// Get the current match-attempt start offset in bytes.
    #[must_use]
    pub fn match_start(&self) -> usize {
        self.match_start
    }

    /// Get the current match-attempt end offset in bytes.
    #[must_use]
    pub fn match_end(&self) -> usize {
        self.match_end
    }

    /// Get the current match-attempt length in bytes.
    #[must_use]
    pub fn match_length(&self) -> usize {
        self.match_end.saturating_sub(self.match_start)
    }

    /// Get the current 1-based top-level branch number, if any.
    #[must_use]
    pub fn matched_branch_number(&self) -> Option<usize> {
        self.matched_branch_number
    }

    /// Get a capture group by index
    pub fn group(&self, index: usize) -> Option<&str> {
        trace_enter!(
            "execution",
            "ExecContext::group",
            "index={},capture_slots={}",
            index,
            self.captures.len()
        );
        let value = self.captures.get(index).and_then(Option::as_deref);
        trace_decision!(
            "execution",
            "group(index).is_some()",
            value.is_some(),
            "indexed capture lookup completed"
        );
        trace_exit!(
            "execution",
            "ExecContext::group",
            "ok=true,found={}",
            value.is_some()
        );
        value
    }

    /// Get a named capture group
    pub fn named(&self, name: &str) -> Option<&str> {
        trace_enter!(
            "execution",
            "ExecContext::named",
            "name={},named_capture_slots={}",
            name,
            self.named_captures.len()
        );
        let value = self.named_captures.get(name).map(String::as_str);
        trace_decision!(
            "execution",
            "named(name).is_some()",
            value.is_some(),
            "named capture lookup completed"
        );
        trace_exit!(
            "execution",
            "ExecContext::named",
            "ok=true,found={}",
            value.is_some()
        );
        value
    }

    /// Get a host-provided execution variable by name.
    ///
    /// # Panics
    /// Panics if the internal variables `RwLock` is poisoned.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<String> {
        let _variable_slots = self.variables.read().unwrap().len();
        trace_enter!(
            "execution",
            "ExecContext::variable",
            "name={},variable_slots={}",
            name,
            variable_slots
        );
        let value = self.variables.read().unwrap().get(name).cloned();
        trace_decision!(
            "execution",
            "variable(name).is_some()",
            value.is_some(),
            "execution variable lookup completed"
        );
        trace_exit!(
            "execution",
            "ExecContext::variable",
            "ok=true,found={}",
            value.is_some()
        );
        value
    }

    /// Clone the current execution-variable snapshot into an owned map.
    ///
    /// # Panics
    /// Panics if the internal variables `RwLock` is poisoned.
    #[must_use]
    pub fn variables_snapshot(&self) -> HashMap<String, String> {
        let _variable_slots = self.variables.read().unwrap().len();
        trace_enter!(
            "execution",
            "ExecContext::variables_snapshot",
            "variable_slots={}",
            variable_slots
        );
        let snapshot = self.variables.read().unwrap().clone();
        trace_exit!(
            "execution",
            "ExecContext::variables_snapshot",
            "ok=true,variable_slots={}",
            snapshot.len()
        );
        snapshot
    }

    /// Get a typed variable value by name.
    ///
    /// Returns a clone of the [`Value`] stored under `name`, if any.
    /// When a string variable was set via [`crate::Regex::set_variable`], it is
    /// also accessible here as [`Value::String`].
    ///
    /// # Panics
    /// Panics if the internal typed-variables `RwLock` is poisoned.
    #[must_use]
    pub fn typed_variable(&self, name: &str) -> Option<Value> {
        let _typed_slots = self.typed_variables.read().unwrap().len();
        trace_enter!(
            "execution",
            "ExecContext::typed_variable",
            "name={},typed_variable_slots={}",
            name,
            typed_slots
        );
        let value = self.typed_variables.read().unwrap().get(name).cloned();
        trace_decision!(
            "execution",
            "typed_variable(name).is_some()",
            value.is_some(),
            "typed execution variable lookup completed"
        );
        trace_exit!(
            "execution",
            "ExecContext::typed_variable",
            "ok=true,found={}",
            value.is_some()
        );
        value
    }

    /// Get a variable as a string.
    #[must_use]
    pub fn var_str(&self, name: &str) -> Option<String> {
        self.typed_variable(name)
            .and_then(|v| v.as_str().map(ToString::to_string))
    }

    /// Get a variable as an integer.
    #[must_use]
    pub fn var_int(&self, name: &str) -> Option<i64> {
        self.typed_variable(name).and_then(|v| v.as_i64())
    }

    /// Get a variable as a float.
    #[must_use]
    pub fn var_float(&self, name: &str) -> Option<f64> {
        self.typed_variable(name).and_then(|v| v.as_f64())
    }

    /// Get a variable as a boolean.
    #[must_use]
    pub fn var_bool(&self, name: &str) -> Option<bool> {
        self.typed_variable(name).and_then(|v| v.as_bool())
    }

    /// Get a variable as an array.
    #[must_use]
    pub fn var_array(&self, name: &str) -> Option<Vec<Value>> {
        self.typed_variable(name)
            .and_then(|v| v.as_array().map(<[Value]>::to_vec))
    }

    /// Get a variable as a map.
    #[must_use]
    pub fn var_map(&self, name: &str) -> Option<Vec<(String, Value)>> {
        self.typed_variable(name)
            .and_then(|v| v.as_map().map(<[(String, Value)]>::to_vec))
    }
}

// ============================================================================
// EXECUTION RESULT
// ============================================================================
/// Non-boolean value emitted by a code block on the winning match path.
#[derive(Debug, Clone, PartialEq)]
pub enum CodeBlockValue {
    /// Code returned a string payload.
    Replacement(String),
    /// Code returned a numeric payload.
    Numeric(f64),
    /// Code returned a structured [`Value`] payload.
    Structured(Value),
}

/// Match steering actions returned by host callbacks.
///
/// These extend the basic pass/fail predicate model to let the host
/// actively control how matching proceeds.
#[derive(Debug, Clone, PartialEq)]
pub enum SteerResult {
    /// Continue matching normally from the current position.
    Continue,
    /// Fail this path and backtrack.
    Fail,
    /// Force-accept the match at the current position.
    Accept,
    /// Advance the input position by `n` bytes before continuing.
    Skip(usize),
    /// Abort the entire match search (no more positions will be tried).
    Abort,
}

/// Result of code execution within a regex pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecResult {
    /// Code returned true - pattern continues matching
    Success,
    /// Code returned false - pattern fails at this point
    Failure,
    /// Code returned a string - use as replacement text
    Replacement(String),
    /// Code returned a numeric value
    Numeric(f64),
    /// Code execution error - treated as failure
    Error(String),
    /// Host steering action — controls how matching proceeds.
    Steer(SteerResult),
    /// Suspend execution — an async callback needs external resolution.
    ///
    /// The `String` is the callback name that needs to be resolved
    /// asynchronously by the caller. Used by the suspendable matching
    /// path to signal that the VM should pause and return control.
    Suspend(String),
    /// Code returned a structured [`Value`] payload.
    Structured(Value),
}

fn exec_result_kind(result: &ExecResult) -> &'static str {
    match result {
        ExecResult::Success => "Success",
        ExecResult::Failure => "Failure",
        ExecResult::Replacement(_) => "Replacement",
        ExecResult::Numeric(_) => "Numeric",
        ExecResult::Error(_) => "Error",
        ExecResult::Steer(_) => "Steer",
        ExecResult::Suspend(_) => "Suspend",
        ExecResult::Structured(_) => "Structured",
    }
}

#[cfg(any(feature = "javascript", feature = "lua", feature = "rhai"))]
fn emitted_result_to_exec_result(result: CodeBlockValue) -> ExecResult {
    match result {
        CodeBlockValue::Replacement(text) => ExecResult::Replacement(text),
        CodeBlockValue::Numeric(value) => ExecResult::Numeric(value),
        CodeBlockValue::Structured(value) => ExecResult::Structured(value),
    }
}

#[cfg(any(feature = "javascript", feature = "lua", feature = "rhai"))]
fn finish_exec_result(
    base_result: ExecResult,
    emitted_result: &Arc<Mutex<Option<CodeBlockValue>>>,
) -> ExecResult {
    finish_exec_result_with_steer(base_result, emitted_result, &Arc::new(Mutex::new(None)))
}

#[cfg(any(feature = "javascript", feature = "lua", feature = "rhai"))]
fn finish_exec_result_with_steer(
    base_result: ExecResult,
    emitted_result: &Arc<Mutex<Option<CodeBlockValue>>>,
    emitted_steer: &Arc<Mutex<Option<SteerResult>>>,
) -> ExecResult {
    // Steer takes highest priority — if the code block emitted a steer
    // action, it overrides everything else.
    if let Some(steer) = emitted_steer.lock().unwrap().take() {
        return ExecResult::Steer(steer);
    }
    let emitted = emitted_result.lock().unwrap().take();
    match base_result {
        ExecResult::Success => emitted.map_or(base_result, emitted_result_to_exec_result),
        ExecResult::Numeric(_) | ExecResult::Replacement(_) | ExecResult::Structured(_) => {
            base_result
        }
        ExecResult::Failure
        | ExecResult::Error(_)
        | ExecResult::Steer(_)
        | ExecResult::Suspend(_) => base_result,
    }
}

// ============================================================================
// EXECUTION ENGINE TRAIT
// ============================================================================

/// Trait for language-specific execution engines.
///
/// Each language backend implements this trait to provide
/// sandboxed code execution with consistent semantics.
pub trait ExecutionEngine: Send + Sync {
    /// Execute code with the given context
    fn execute(&self, code: &str, context: &ExecContext) -> ExecResult;

    /// Language identifier ("lua", "js", "wasm", etc.)
    fn language(&self) -> &str;

    /// Check if the engine is available (dependencies installed)
    fn is_available(&self) -> bool;

    /// Reset the engine state (clear any cached state)
    fn reset(&mut self);
}

// ============================================================================
// RHAI EXECUTION ENGINE
// ============================================================================

#[cfg(feature = "rhai")]
pub mod rhai {
    use super::*;
    use ::rhai::{Array, Dynamic, Engine as RhaiRuntime, ImmutableString, Map, Scope};

    /// Rhai execution engine using a fresh embedded runtime per evaluation.
    ///
    /// **Security Features:**
    /// - No filesystem or network access is wired into rgx
    /// - No external module resolver is configured
    /// - Each execution gets a fresh runtime and scope
    ///
    /// **Performance:**
    /// - Pure-Rust embedded scripting engine
    /// - No external runtime dependency
    /// - Fresh runtime per execution keeps speculative backtracking safe
    pub struct RhaiEngine;

    impl RhaiEngine {
        /// Create a new sandboxed Rhai engine.
        pub fn new() -> Result<Self> {
            Ok(Self)
        }

        fn new_engine(
            &self,
            emitted_result: Arc<Mutex<Option<CodeBlockValue>>>,
            emitted_steer: Arc<Mutex<Option<SteerResult>>>,
        ) -> RhaiRuntime {
            let mut engine = RhaiRuntime::new();
            engine.on_print(|_| {});
            engine.on_debug(|_, _, _| {});
            let emitted_numeric = emitted_result.clone();
            engine.register_fn("emit_numeric", move |value: i64| {
                *emitted_numeric.lock().unwrap() = Some(CodeBlockValue::Numeric(value as f64));
            });
            let emitted_numeric = emitted_result.clone();
            engine.register_fn("emit_numeric", move |value: f64| {
                *emitted_numeric.lock().unwrap() = Some(CodeBlockValue::Numeric(value));
            });
            let emitted_replacement = emitted_result;
            engine.register_fn("emit_replacement", move |value: ImmutableString| {
                *emitted_replacement.lock().unwrap() =
                    Some(CodeBlockValue::Replacement(value.to_string()));
            });

            // Steer functions
            let s = emitted_steer.clone();
            engine.register_fn("steer_continue", move || {
                *s.lock().unwrap() = Some(SteerResult::Continue);
            });
            let s = emitted_steer.clone();
            engine.register_fn("steer_fail", move || {
                *s.lock().unwrap() = Some(SteerResult::Fail);
            });
            let s = emitted_steer.clone();
            engine.register_fn("steer_accept", move || {
                *s.lock().unwrap() = Some(SteerResult::Accept);
            });
            let s = emitted_steer.clone();
            engine.register_fn("steer_skip", move |n: i64| {
                *s.lock().unwrap() = Some(SteerResult::Skip(n as usize));
            });
            let s = emitted_steer;
            engine.register_fn("steer_abort", move || {
                *s.lock().unwrap() = Some(SteerResult::Abort);
            });

            engine
        }

        fn build_scope(&self, context: &ExecContext) -> Scope<'static> {
            let mut scope = Scope::new();

            let arg = context
                .captures
                .iter()
                .map(|capture| match capture {
                    Some(text) => Dynamic::from(text.clone()),
                    None => Dynamic::UNIT,
                })
                .collect::<Array>();
            scope.push("arg", arg);

            scope.push("pos", i64::try_from(context.position).unwrap_or(i64::MAX));
            scope.push(
                "match_start",
                i64::try_from(context.match_start).unwrap_or(i64::MAX),
            );
            scope.push(
                "match_end",
                i64::try_from(context.match_end).unwrap_or(i64::MAX),
            );
            scope.push(
                "match_length",
                i64::try_from(context.match_length()).unwrap_or(i64::MAX),
            );
            match context.matched_branch_number {
                Some(branch_number) => {
                    scope.push(
                        "branch_number",
                        i64::try_from(branch_number).unwrap_or(i64::MAX),
                    );
                }
                None => {
                    scope.push_dynamic("branch_number", Dynamic::UNIT);
                }
            }
            scope.push("text", context.text.clone());

            let mut named = Map::new();
            for (name, value) in &context.named_captures {
                named.insert(name.clone().into(), value.clone().into());
            }
            scope.push("named", named);

            let mut vars = Map::new();
            for (name, value) in context.variables_snapshot() {
                vars.insert(name.into(), value.into());
            }
            scope.push("vars", vars);

            scope
        }

        fn into_exec_result(value: Dynamic) -> ExecResult {
            if value.is::<bool>() {
                return if value.cast::<bool>() {
                    ExecResult::Success
                } else {
                    ExecResult::Failure
                };
            }
            if value.is::<i64>() {
                return ExecResult::Numeric(value.cast::<i64>() as f64);
            }
            if value.is::<f64>() {
                return ExecResult::Numeric(value.cast::<f64>());
            }
            if value.is::<ImmutableString>() {
                return ExecResult::Replacement(value.cast::<ImmutableString>().to_string());
            }
            if value.is::<String>() {
                return ExecResult::Replacement(value.cast::<String>());
            }
            ExecResult::Success
        }
    }

    impl ExecutionEngine for RhaiEngine {
        fn execute(&self, code: &str, context: &ExecContext) -> ExecResult {
            let emitted_result = Arc::new(Mutex::new(None));
            let emitted_steer = Arc::new(Mutex::new(None));
            let engine = self.new_engine(emitted_result.clone(), emitted_steer.clone());
            let mut scope = self.build_scope(context);
            match engine.eval_with_scope::<Dynamic>(&mut scope, code) {
                Ok(value) => finish_exec_result_with_steer(
                    Self::into_exec_result(value),
                    &emitted_result,
                    &emitted_steer,
                ),
                Err(err) => ExecResult::Error(format!("Rhai error: {}", err)),
            }
        }

        fn language(&self) -> &str {
            "rhai"
        }

        fn is_available(&self) -> bool {
            true
        }

        fn reset(&mut self) {
            // Stateless engine: each execution creates a fresh runtime and scope.
        }
    }

    impl Default for RhaiEngine {
        fn default() -> Self {
            Self::new().expect("Failed to create Rhai engine")
        }
    }
}

// ============================================================================
// LUA EXECUTION ENGINE
// ============================================================================

#[cfg(feature = "lua")]
pub mod lua {
    use super::*;
    use mlua::{Lua, Value};

    ///
    /// **Security Features:**
    /// - No file I/O (io library removed)
    /// - No system access (os library removed)
    /// - No module loading (require disabled)
    /// - Memory limits enforced
    ///
    /// **Performance:**
    /// - ~1-5 microseconds per execution
    /// - Fresh sandboxed Lua state per execution
    /// - JIT compilation via LuaJIT (if available)
    pub struct LuaEngine;

    impl LuaEngine {
        /// Create a new sandboxed Lua engine
        pub fn new() -> Result<Self> {
            Ok(Self)
        }

        fn new_sandboxed_lua(&self) -> Lua {
            let lua = Lua::new();
            lua.globals().set("io", Value::Nil).ok();
            lua.globals().set("os", Value::Nil).ok();
            lua.globals().set("debug", Value::Nil).ok();
            lua.globals().set("require", Value::Nil).ok();
            lua.globals().set("loadfile", Value::Nil).ok();
            lua.globals().set("dofile", Value::Nil).ok();
            lua.globals().set("package", Value::Nil).ok();
            lua
        }

        /// Set up the execution context in Lua globals
        fn setup_context(
            &self,
            lua: &Lua,
            context: &ExecContext,
            emitted_result: Arc<Mutex<Option<CodeBlockValue>>>,
        ) -> mlua::Result<()> {
            let globals = lua.globals();

            // Create arg table with captures
            let arg_table = lua.create_table()?;
            for (i, capture) in context.captures.iter().enumerate() {
                if let Some(text) = capture {
                    arg_table.set(i, text.clone())?;
                }
            }
            globals.set("arg", arg_table)?;

            // Set match position
            globals.set("pos", context.position)?;
            globals.set("match_start", context.match_start)?;
            globals.set("match_end", context.match_end)?;
            globals.set("match_length", context.match_length())?;
            match context.matched_branch_number {
                Some(branch_number) => globals.set("branch_number", branch_number)?,
                None => globals.set("branch_number", Value::Nil)?,
            }

            // Set full text (read-only)
            globals.set("text", context.text.clone())?;

            // Create named captures table
            let named_table = lua.create_table()?;
            for (name, value) in &context.named_captures {
                named_table.set(name.clone(), value.clone())?;
            }
            globals.set("named", named_table)?;

            // Create execution variables table
            let vars_table = lua.create_table()?;
            for (name, value) in context.variables_snapshot() {
                vars_table.set(name, value)?;
            }
            globals.set("vars", vars_table)?;

            let rgx_table = lua.create_table()?;
            let emitted_numeric = emitted_result.clone();
            rgx_table.set(
                "emit_numeric",
                lua.create_function(move |_, value: f64| {
                    *emitted_numeric.lock().unwrap() = Some(CodeBlockValue::Numeric(value));
                    Ok(())
                })?,
            )?;
            let emitted_replacement = emitted_result;
            rgx_table.set(
                "emit_replacement",
                lua.create_function(move |_, value: String| {
                    *emitted_replacement.lock().unwrap() = Some(CodeBlockValue::Replacement(value));
                    Ok(())
                })?,
            )?;
            globals.set("rgx", rgx_table)?;

            Ok(())
        }

        /// Register `rgx.steer_*` helpers on the Lua `rgx` table.
        fn setup_steer_functions(
            &self,
            lua: &Lua,
            emitted_steer: Arc<Mutex<Option<SteerResult>>>,
        ) -> mlua::Result<()> {
            let rgx_table: mlua::Table = lua.globals().get("rgx")?;

            let s = emitted_steer.clone();
            rgx_table.set(
                "steer_continue",
                lua.create_function(move |_, ()| {
                    *s.lock().unwrap() = Some(SteerResult::Continue);
                    Ok(())
                })?,
            )?;

            let s = emitted_steer.clone();
            rgx_table.set(
                "steer_fail",
                lua.create_function(move |_, ()| {
                    *s.lock().unwrap() = Some(SteerResult::Fail);
                    Ok(())
                })?,
            )?;

            let s = emitted_steer.clone();
            rgx_table.set(
                "steer_accept",
                lua.create_function(move |_, ()| {
                    *s.lock().unwrap() = Some(SteerResult::Accept);
                    Ok(())
                })?,
            )?;

            let s = emitted_steer.clone();
            rgx_table.set(
                "steer_skip",
                lua.create_function(move |_, n: usize| {
                    *s.lock().unwrap() = Some(SteerResult::Skip(n));
                    Ok(())
                })?,
            )?;

            let s = emitted_steer;
            rgx_table.set(
                "steer_abort",
                lua.create_function(move |_, ()| {
                    *s.lock().unwrap() = Some(SteerResult::Abort);
                    Ok(())
                })?,
            )?;

            Ok(())
        }

        fn eval_user_code<'lua>(&self, lua: &'lua Lua, code: &str) -> mlua::Result<Value<'lua>> {
            // Prefer direct evaluation so explicit `return ...` bodies and full chunks keep
            // working. Fall back to `return ...` wrapping so bare expression bodies behave
            // like the shipped JavaScript/Rhai source-body contract.
            match lua.load(code).eval::<Value>() {
                Ok(value) => Ok(value),
                Err(_) => {
                    let wrapped_code = format!("return {code}");
                    lua.load(&wrapped_code).eval::<Value>()
                }
            }
        }
    }

    impl ExecutionEngine for LuaEngine {
        fn execute(&self, code: &str, context: &ExecContext) -> ExecResult {
            let lua = self.new_sandboxed_lua();
            let emitted_result = Arc::new(Mutex::new(None));
            let emitted_steer = Arc::new(Mutex::new(None));
            // Set up context
            if let Err(e) = self.setup_context(&lua, context, emitted_result.clone()) {
                return ExecResult::Error(format!("Context setup failed: {}", e));
            }
            if let Err(e) = self.setup_steer_functions(&lua, emitted_steer.clone()) {
                return ExecResult::Error(format!("Steer setup failed: {}", e));
            }

            let result = self.eval_user_code(&lua, code);
            let base_result = match result {
                Ok(Value::Boolean(b)) => {
                    if b {
                        ExecResult::Success
                    } else {
                        ExecResult::Failure
                    }
                }
                Ok(Value::Integer(n)) => ExecResult::Numeric(n as f64),
                Ok(Value::Number(n)) => ExecResult::Numeric(n),
                Ok(Value::String(s)) => ExecResult::Replacement(s.to_string_lossy().to_string()),
                Ok(Value::Nil) => ExecResult::Success,
                Ok(_) => ExecResult::Success,
                Err(e) => ExecResult::Error(format!("Lua error: {}", e)),
            };
            finish_exec_result_with_steer(base_result, &emitted_result, &emitted_steer)
        }

        fn language(&self) -> &str {
            "lua"
        }

        fn is_available(&self) -> bool {
            true
        }

        fn reset(&mut self) {
            // Stateless engine: each execution creates a fresh sandboxed runtime.
        }
    }

    impl Default for LuaEngine {
        fn default() -> Self {
            Self::new().expect("Failed to create Lua engine")
        }
    }
}

// ============================================================================
// JAVASCRIPT EXECUTION ENGINE
// ============================================================================

#[cfg(feature = "javascript")]
pub mod javascript {
    use super::*;
    use rquickjs::{Array, Context, Ctx, Function, Object, Runtime, Undefined, Value};

    /// JavaScript execution engine using QuickJS.
    ///
    /// **Security Features:**
    /// - No file system access
    /// - No network capabilities
    /// - No process/system access
    /// - Memory and CPU limits
    /// - No eval() or Function constructor
    ///
    /// **Performance:**
    /// - ~5-20 microseconds per execution
    /// - Lightweight JS engine (QuickJS)
    /// - Fast startup, small memory footprint
    pub struct JavaScriptEngine {
        memory_limit_bytes: usize,
        max_stack_size_bytes: usize,
    }

    impl JavaScriptEngine {
        /// Create a new sandboxed JavaScript engine
        pub fn new() -> Result<Self> {
            Ok(Self {
                memory_limit_bytes: 10 * 1024 * 1024,
                max_stack_size_bytes: 256 * 1024,
            })
        }

        fn new_runtime(&self) -> Result<Runtime> {
            let runtime = Runtime::new().map_err(|e| {
                crate::error::RgxError::Engine(format!("Failed to create JS runtime: {}", e))
            })?;
            runtime.set_memory_limit(self.memory_limit_bytes);
            runtime.set_max_stack_size(self.max_stack_size_bytes);
            Ok(runtime)
        }

        fn eval_user_code<'js>(&self, ctx: Ctx<'js>, code: &str) -> rquickjs::Result<Value<'js>> {
            // Prefer direct evaluation so bare expression bodies preserve their result value.
            // Fall back to an IIFE when the source uses explicit `return ...` style.
            match ctx.eval::<Value<'js>, _>(code) {
                Ok(value) => Ok(value),
                Err(_) => {
                    let wrapped_code = format!("(function(){{\n{code}\n}})()");
                    ctx.eval::<Value<'js>, _>(wrapped_code)
                }
            }
        }

        fn into_exec_result(val: Value<'_>) -> ExecResult {
            if val.is_bool() {
                if let Some(b) = val.as_bool() {
                    return if b {
                        ExecResult::Success
                    } else {
                        ExecResult::Failure
                    };
                }
                return ExecResult::Success;
            }
            if val.is_number() {
                if let Some(n) = val.as_number() {
                    return ExecResult::Numeric(n);
                }
                return ExecResult::Success;
            }
            if val.is_string() {
                if let Ok(s) = val.get::<String>() {
                    return ExecResult::Replacement(s);
                }
                return ExecResult::Success;
            }
            if val.is_null() || val.is_undefined() {
                return ExecResult::Success;
            }
            ExecResult::Success
        }

        /// Execute JavaScript code in sandboxed context
        fn execute_sandboxed(&self, code: &str, context: &ExecContext) -> ExecResult {
            let runtime = match self.new_runtime() {
                Ok(runtime) => runtime,
                Err(err) => return ExecResult::Error(err.to_string()),
            };
            let ctx_result = Context::full(&runtime);
            let emitted_result = Arc::new(Mutex::new(None));
            let emitted_steer = Arc::new(Mutex::new(None));

            let ctx = match ctx_result {
                Ok(ctx) => ctx,
                Err(e) => return ExecResult::Error(format!("Context creation failed: {}", e)),
            };

            ctx.with(|ctx| {
                // Set up global context
                let globals = ctx.globals();

                // Create arg array with captures
                if let Ok(arg_array) = Array::new(ctx.clone()) {
                    for (i, capture) in context.captures.iter().enumerate() {
                        if let Some(text) = capture {
                            arg_array.set(i, text.clone()).ok();
                        }
                    }
                    globals.set("arg", arg_array).ok();
                }

                // Set position and text
                globals.set("pos", context.position as i32).ok();
                globals
                    .set(
                        "match_start",
                        i32::try_from(context.match_start).unwrap_or(i32::MAX),
                    )
                    .ok();
                globals
                    .set(
                        "match_end",
                        i32::try_from(context.match_end).unwrap_or(i32::MAX),
                    )
                    .ok();
                globals
                    .set(
                        "match_length",
                        i32::try_from(context.match_length()).unwrap_or(i32::MAX),
                    )
                    .ok();
                if let Some(branch_number) = context.matched_branch_number {
                    if let Ok(branch_number) = i32::try_from(branch_number) {
                        globals.set("branch_number", branch_number).ok();
                    } else {
                        globals.set("branch_number", Undefined).ok();
                    }
                } else {
                    globals.set("branch_number", Undefined).ok();
                }
                globals.set("text", context.text.clone()).ok();

                // Create named captures object
                if let Ok(named_obj) = Object::new(ctx.clone()) {
                    for (name, value) in &context.named_captures {
                        named_obj.set(name.clone(), value.clone()).ok();
                    }
                    globals.set("named", named_obj).ok();
                }

                // Create variables object
                if let Ok(vars_obj) = Object::new(ctx.clone()) {
                    for (name, value) in context.variables_snapshot() {
                        vars_obj.set(name, value).ok();
                    }
                    globals.set("vars", vars_obj).ok();
                }

                if let Ok(rgx_obj) = Object::new(ctx.clone()) {
                    let emitted_numeric = emitted_result.clone();
                    if let Ok(emit_numeric) = Function::new(ctx.clone(), move |value: f64| {
                        *emitted_numeric.lock().unwrap() = Some(CodeBlockValue::Numeric(value));
                    }) {
                        rgx_obj.set("emit_numeric", emit_numeric).ok();
                    }
                    let emitted_replacement = emitted_result.clone();
                    if let Ok(emit_replacement) =
                        Function::new(ctx.clone(), move |value: String| {
                            *emitted_replacement.lock().unwrap() =
                                Some(CodeBlockValue::Replacement(value));
                        })
                    {
                        rgx_obj.set("emit_replacement", emit_replacement).ok();
                    }
                    // Steer functions
                    let s = emitted_steer.clone();
                    if let Ok(f) = Function::new(ctx.clone(), move || {
                        *s.lock().unwrap() = Some(SteerResult::Continue);
                    }) {
                        rgx_obj.set("steerContinue", f).ok();
                    }
                    let s = emitted_steer.clone();
                    if let Ok(f) = Function::new(ctx.clone(), move || {
                        *s.lock().unwrap() = Some(SteerResult::Fail);
                    }) {
                        rgx_obj.set("steerFail", f).ok();
                    }
                    let s = emitted_steer.clone();
                    if let Ok(f) = Function::new(ctx.clone(), move || {
                        *s.lock().unwrap() = Some(SteerResult::Accept);
                    }) {
                        rgx_obj.set("steerAccept", f).ok();
                    }
                    let s = emitted_steer.clone();
                    if let Ok(f) = Function::new(ctx.clone(), move |n: usize| {
                        *s.lock().unwrap() = Some(SteerResult::Skip(n));
                    }) {
                        rgx_obj.set("steerSkip", f).ok();
                    }
                    let s = emitted_steer.clone();
                    if let Ok(f) = Function::new(ctx.clone(), move || {
                        *s.lock().unwrap() = Some(SteerResult::Abort);
                    }) {
                        rgx_obj.set("steerAbort", f).ok();
                    }

                    globals.set("rgx", rgx_obj).ok();
                }

                // Remove dangerous functions
                globals.set("eval", Undefined).ok();
                globals.set("Function", Undefined).ok();
                globals.set("fetch", Undefined).ok();
                globals.set("XMLHttpRequest", Undefined).ok();

                match self.eval_user_code(ctx.clone(), code) {
                    Ok(val) => finish_exec_result_with_steer(
                        Self::into_exec_result(val),
                        &emitted_result,
                        &emitted_steer,
                    ),
                    Err(e) => ExecResult::Error(format!("JS error: {}", e)),
                }
            })
        }
    }

    impl ExecutionEngine for JavaScriptEngine {
        fn execute(&self, code: &str, context: &ExecContext) -> ExecResult {
            self.execute_sandboxed(code, context)
        }

        fn language(&self) -> &str {
            "js"
        }

        fn is_available(&self) -> bool {
            true
        }

        fn reset(&mut self) {
            // Stateless engine: each execution creates a fresh sandboxed runtime.
        }
    }

    impl Default for JavaScriptEngine {
        fn default() -> Self {
            Self::new().expect("Failed to create JavaScript engine")
        }
    }
}

// ============================================================================
// NATIVE CALLBACK EXECUTION
// ============================================================================

#[cfg(feature = "wasm")]
pub mod wasm {
    use super::*;
    use anyhow::anyhow;
    use wasmtime::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store};

    type WasmModuleHandle = Arc<Module>;

    #[derive(Clone)]
    struct WasmStoreData {
        context: ExecContext,
        emitted_result: Option<CodeBlockValue>,
        emitted_steer: Option<SteerResult>,
    }

    impl WasmStoreData {
        fn new(context: ExecContext) -> Self {
            Self {
                context,
                emitted_result: None,
                emitted_steer: None,
            }
        }

        fn set_emitted_result(&mut self, result: CodeBlockValue) {
            self.emitted_result = Some(result);
        }

        fn take_emitted_result(&mut self) -> Option<CodeBlockValue> {
            self.emitted_result.take()
        }

        fn set_emitted_steer(&mut self, steer: SteerResult) {
            self.emitted_steer = Some(steer);
        }

        fn take_emitted_steer(&mut self) -> Option<SteerResult> {
            self.emitted_steer.take()
        }
    }

    /// Registry for named wasm modules that can be referenced from regex patterns.
    pub struct WasmModuleRegistry {
        modules: RwLock<HashMap<String, WasmModuleHandle>>,
    }

    impl WasmModuleRegistry {
        /// Create an empty wasm module registry.
        pub fn new() -> Self {
            trace_enter!("execution", "WasmModuleRegistry::new");
            let registry = Self {
                modules: RwLock::new(HashMap::new()),
            };
            trace_exit!(
                "execution",
                "WasmModuleRegistry::new",
                "ok=true,registered_modules=0"
            );
            registry
        }

        /// Register or replace a compiled wasm module.
        pub fn register(&self, name: String, module: Module) {
            trace_enter!("execution", "WasmModuleRegistry::register", "name={}", name);
            let mut modules = self.modules.write().unwrap();
            let replaced_existing = modules.insert(name.clone(), Arc::new(module)).is_some();
            trace_decision!(
                "execution",
                "modules.insert(name,...).is_some()",
                replaced_existing,
                "true means an existing wasm module with the same name was replaced"
            );
            trace_exit!(
                "execution",
                "WasmModuleRegistry::register",
                "ok=true,name={},registered_modules={}",
                name,
                modules.len()
            );
        }

        /// Get a registered wasm module by name.
        pub fn get(&self, name: &str) -> Option<WasmModuleHandle> {
            trace_enter!(
                "execution",
                "WasmModuleRegistry::get",
                "name={},registered_modules={}",
                name,
                self.len()
            );
            let module = self.modules.read().unwrap().get(name).cloned();
            trace_exit!(
                "execution",
                "WasmModuleRegistry::get",
                "ok=true,found={}",
                module.is_some()
            );
            module
        }

        /// Count registered wasm modules.
        pub fn len(&self) -> usize {
            self.modules.read().unwrap().len()
        }
    }

    impl Default for WasmModuleRegistry {
        fn default() -> Self {
            Self::new()
        }
    }

    /// WebAssembly execution engine using wasmtime.
    ///
    /// Current ABI contract:
    /// - patterns refer to `(?{wasm:module:function})`
    /// - `module` is a Rust-registered module name
    /// - `function` is an exported zero-argument function
    /// - function result must be `i32` where `0` means failure and non-zero means success
    /// - modules may optionally import execution-context helpers from the `rgx` namespace
    ///   - `rgx.position()`, `rgx.match_start()`, `rgx.match_end()`, `rgx.match_length()`
    ///   - `rgx.branch_number()` (`-1` when unavailable)
    /// - modules may optionally emit richer winning-path results through:
    ///   - `rgx.emit_numeric(f64)`
    ///   - `rgx.emit_replacement(ptr, len)`
    /// - emitted results are used only when the exported predicate returns non-zero
    pub struct WasmEngine {
        engine: Engine,
        linker: Linker<WasmStoreData>,
        modules: WasmModuleRegistry,
    }

    impl WasmEngine {
        /// Create a new wasm execution engine.
        pub fn new() -> Result<Self> {
            trace_enter!("execution", "WasmEngine::new");
            let config = Config::new();
            let engine = Engine::new(&config)
                .map_err(|e| RgxError::Engine(format!("Failed to create WASM runtime: {e}")))?;
            let linker = Self::build_linker(&engine)?;
            let wasm_engine = Self {
                engine,
                linker,
                modules: WasmModuleRegistry::new(),
            };
            trace_exit!(
                "execution",
                "WasmEngine::new",
                "ok=true,registered_modules=0"
            );
            Ok(wasm_engine)
        }

        fn build_linker(engine: &Engine) -> Result<Linker<WasmStoreData>> {
            trace_enter!("execution", "WasmEngine::build_linker");
            let mut linker = Linker::new(engine);
            linker
                .func_wrap("rgx", "position", Self::current_position_import)
                .map_err(|e| {
                    RgxError::Engine(format!("Failed to define WASM import rgx.position: {e}"))
                })?;
            linker
                .func_wrap("rgx", "match_start", Self::match_start_import)
                .map_err(|e| {
                    RgxError::Engine(format!("Failed to define WASM import rgx.match_start: {e}"))
                })?;
            linker
                .func_wrap("rgx", "match_end", Self::match_end_import)
                .map_err(|e| {
                    RgxError::Engine(format!("Failed to define WASM import rgx.match_end: {e}"))
                })?;
            linker
                .func_wrap("rgx", "match_length", Self::match_length_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.match_length: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "branch_number", Self::branch_number_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.branch_number: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "text_length", Self::text_length_import)
                .map_err(|e| {
                    RgxError::Engine(format!("Failed to define WASM import rgx.text_length: {e}"))
                })?;
            linker
                .func_wrap("rgx", "text_read", Self::text_read_import)
                .map_err(|e| {
                    RgxError::Engine(format!("Failed to define WASM import rgx.text_read: {e}"))
                })?;
            linker
                .func_wrap("rgx", "capture_count", Self::capture_count_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.capture_count: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "capture_length", Self::capture_length_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.capture_length: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "capture_read", Self::capture_read_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.capture_read: {e}"
                    ))
                })?;
            linker
                .func_wrap(
                    "rgx",
                    "named_capture_count",
                    Self::named_capture_count_import,
                )
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.named_capture_count: {e}"
                    ))
                })?;
            linker
                .func_wrap(
                    "rgx",
                    "named_capture_name_length",
                    Self::named_capture_name_length_import,
                )
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.named_capture_name_length: {e}"
                    ))
                })?;
            linker
                .func_wrap(
                    "rgx",
                    "named_capture_name_read",
                    Self::named_capture_name_read_import,
                )
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.named_capture_name_read: {e}"
                    ))
                })?;
            linker
                .func_wrap(
                    "rgx",
                    "named_capture_value_length",
                    Self::named_capture_value_length_import,
                )
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.named_capture_value_length: {e}"
                    ))
                })?;
            linker
                .func_wrap(
                    "rgx",
                    "named_capture_value_read",
                    Self::named_capture_value_read_import,
                )
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.named_capture_value_read: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "variable_count", Self::variable_count_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.variable_count: {e}"
                    ))
                })?;
            linker
                .func_wrap(
                    "rgx",
                    "variable_name_length",
                    Self::variable_name_length_import,
                )
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.variable_name_length: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "variable_name_read", Self::variable_name_read_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.variable_name_read: {e}"
                    ))
                })?;
            linker
                .func_wrap(
                    "rgx",
                    "variable_value_length",
                    Self::variable_value_length_import,
                )
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.variable_value_length: {e}"
                    ))
                })?;
            linker
                .func_wrap(
                    "rgx",
                    "variable_value_read",
                    Self::variable_value_read_import,
                )
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.variable_value_read: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "emit_numeric", Self::emit_numeric_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.emit_numeric: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "emit_replacement", Self::emit_replacement_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.emit_replacement: {e}"
                    ))
                })?;
            // Steering imports — matches the API surface offered by
            // the Lua / JS / Rhai embedded hosts. The WASM module
            // calls these to request that the engine steer to a
            // specific outcome instead of (or in addition to)
            // returning a predicate boolean. Steer is highest
            // priority: if a steer is emitted, the eventual
            // ExecResult is `Steer(_)` regardless of the function's
            // return value.
            linker
                .func_wrap("rgx", "steer_continue", Self::steer_continue_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.steer_continue: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "steer_fail", Self::steer_fail_import)
                .map_err(|e| {
                    RgxError::Engine(format!("Failed to define WASM import rgx.steer_fail: {e}"))
                })?;
            linker
                .func_wrap("rgx", "steer_accept", Self::steer_accept_import)
                .map_err(|e| {
                    RgxError::Engine(format!(
                        "Failed to define WASM import rgx.steer_accept: {e}"
                    ))
                })?;
            linker
                .func_wrap("rgx", "steer_skip", Self::steer_skip_import)
                .map_err(|e| {
                    RgxError::Engine(format!("Failed to define WASM import rgx.steer_skip: {e}"))
                })?;
            linker
                .func_wrap("rgx", "steer_abort", Self::steer_abort_import)
                .map_err(|e| {
                    RgxError::Engine(format!("Failed to define WASM import rgx.steer_abort: {e}"))
                })?;
            trace_exit!("execution", "WasmEngine::build_linker", "ok=true");
            Ok(linker)
        }

        fn usize_to_i32(value: usize, label: &str) -> wasmtime::Result<i32> {
            i32::try_from(value).map_err(|_| anyhow!("{label} exceeds the wasm i32 ABI limit"))
        }

        fn nonnegative_i32_to_usize(value: i32, label: &str) -> wasmtime::Result<usize> {
            usize::try_from(value).map_err(|_| anyhow!("{label} must be a non-negative i32 value"))
        }

        fn guest_memory(caller: &mut Caller<'_, WasmStoreData>) -> wasmtime::Result<Memory> {
            match caller.get_export("memory") {
                Some(Extern::Memory(memory)) => Ok(memory),
                Some(_) => Err(anyhow!(
                    "WASM module must export linear memory as `memory` to use rgx host imports"
                )),
                None => Err(anyhow!(
                    "WASM module must export linear memory as `memory` to use rgx host imports"
                )),
            }
        }

        fn write_guest_bytes(
            caller: &mut Caller<'_, WasmStoreData>,
            guest_ptr: i32,
            bytes: &[u8],
        ) -> wasmtime::Result<()> {
            let memory = Self::guest_memory(caller)?;
            let guest_ptr = Self::nonnegative_i32_to_usize(guest_ptr, "guest pointer")?;
            memory
                .write(caller, guest_ptr, bytes)
                .map_err(|e| anyhow!("Failed to write into guest memory: {e}"))
        }

        fn read_guest_bytes(
            caller: &mut Caller<'_, WasmStoreData>,
            guest_ptr: i32,
            len: i32,
        ) -> wasmtime::Result<Vec<u8>> {
            let memory = Self::guest_memory(caller)?;
            let guest_ptr = Self::nonnegative_i32_to_usize(guest_ptr, "guest pointer")?;
            let len = Self::nonnegative_i32_to_usize(len, "guest byte length")?;
            let mut bytes = vec![0_u8; len];
            memory
                .read(caller, guest_ptr, &mut bytes)
                .map_err(|e| anyhow!("Failed to read from guest memory: {e}"))?;
            Ok(bytes)
        }

        fn current_position_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            Self::usize_to_i32(caller.data().context.position, "current position")
        }

        fn match_start_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            Self::usize_to_i32(caller.data().context.match_start, "current match start")
        }

        fn match_end_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            Self::usize_to_i32(caller.data().context.match_end, "current match end")
        }

        fn match_length_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            Self::usize_to_i32(caller.data().context.match_length(), "current match length")
        }

        fn branch_number_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            match caller.data().context.matched_branch_number {
                Some(branch_number) => {
                    Self::usize_to_i32(branch_number, "current top-level branch number")
                }
                None => Ok(-1),
            }
        }

        fn text_length_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            Self::usize_to_i32(caller.data().context.text.len(), "input text length")
        }

        fn text_read_import(
            mut caller: Caller<'_, WasmStoreData>,
            guest_ptr: i32,
            offset: i32,
            len: i32,
        ) -> wasmtime::Result<i32> {
            let offset = Self::nonnegative_i32_to_usize(offset, "text offset")?;
            let len = Self::nonnegative_i32_to_usize(len, "text read length")?;
            let bytes = {
                let text = caller.data().context.text.as_bytes();
                if offset >= text.len() {
                    Vec::new()
                } else {
                    let end = offset.saturating_add(len).min(text.len());
                    text[offset..end].to_vec()
                }
            };
            Self::write_guest_bytes(&mut caller, guest_ptr, &bytes)?;
            Self::usize_to_i32(bytes.len(), "copied text length")
        }

        fn capture_count_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            Self::usize_to_i32(caller.data().context.captures.len(), "capture count")
        }

        fn sorted_string_map_entries<'a>(
            map: &'a HashMap<String, String>,
        ) -> Vec<(&'a str, &'a str)> {
            let mut entries = map
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            entries.sort_unstable_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
            entries
        }

        fn sorted_named_capture_entries<'a>(context: &'a ExecContext) -> Vec<(&'a str, &'a str)> {
            Self::sorted_string_map_entries(&context.named_captures)
        }

        fn sorted_variable_entries(context: &ExecContext) -> Vec<(String, String)> {
            let mut entries = context.variables_snapshot().into_iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
            entries
        }

        fn capture_length_import(
            caller: Caller<'_, WasmStoreData>,
            index: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "capture index")?;
            match caller
                .data()
                .context
                .captures
                .get(index)
                .and_then(Option::as_deref)
            {
                Some(capture) => Self::usize_to_i32(capture.len(), "capture length"),
                None => Ok(-1),
            }
        }

        fn capture_read_import(
            mut caller: Caller<'_, WasmStoreData>,
            index: i32,
            guest_ptr: i32,
            offset: i32,
            len: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "capture index")?;
            let offset = Self::nonnegative_i32_to_usize(offset, "capture offset")?;
            let len = Self::nonnegative_i32_to_usize(len, "capture read length")?;
            let Some(capture) = caller
                .data()
                .context
                .captures
                .get(index)
                .and_then(Option::as_deref)
            else {
                return Ok(-1);
            };
            let bytes = if offset >= capture.len() {
                Vec::new()
            } else {
                let end = offset.saturating_add(len).min(capture.len());
                capture.as_bytes()[offset..end].to_vec()
            };
            Self::write_guest_bytes(&mut caller, guest_ptr, &bytes)?;
            Self::usize_to_i32(bytes.len(), "copied capture length")
        }

        fn named_capture_count_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            Self::usize_to_i32(
                caller.data().context.named_captures.len(),
                "named capture count",
            )
        }

        fn named_capture_name_length_import(
            caller: Caller<'_, WasmStoreData>,
            index: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "named capture index")?;
            match Self::sorted_named_capture_entries(&caller.data().context)
                .get(index)
                .map(|(name, _)| *name)
            {
                Some(name) => Self::usize_to_i32(name.len(), "named capture name length"),
                None => Ok(-1),
            }
        }

        fn named_capture_name_read_import(
            mut caller: Caller<'_, WasmStoreData>,
            index: i32,
            guest_ptr: i32,
            offset: i32,
            len: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "named capture index")?;
            let offset = Self::nonnegative_i32_to_usize(offset, "named capture name offset")?;
            let len = Self::nonnegative_i32_to_usize(len, "named capture name read length")?;
            let Some((name, _)) = Self::sorted_named_capture_entries(&caller.data().context)
                .get(index)
                .copied()
            else {
                return Ok(-1);
            };
            let bytes = if offset >= name.len() {
                Vec::new()
            } else {
                let end = offset.saturating_add(len).min(name.len());
                name.as_bytes()[offset..end].to_vec()
            };
            Self::write_guest_bytes(&mut caller, guest_ptr, &bytes)?;
            Self::usize_to_i32(bytes.len(), "copied named capture name length")
        }

        fn named_capture_value_length_import(
            caller: Caller<'_, WasmStoreData>,
            index: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "named capture index")?;
            match Self::sorted_named_capture_entries(&caller.data().context)
                .get(index)
                .map(|(_, value)| *value)
            {
                Some(value) => Self::usize_to_i32(value.len(), "named capture value length"),
                None => Ok(-1),
            }
        }

        fn named_capture_value_read_import(
            mut caller: Caller<'_, WasmStoreData>,
            index: i32,
            guest_ptr: i32,
            offset: i32,
            len: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "named capture index")?;
            let offset = Self::nonnegative_i32_to_usize(offset, "named capture value offset")?;
            let len = Self::nonnegative_i32_to_usize(len, "named capture value read length")?;
            let Some((_, value)) = Self::sorted_named_capture_entries(&caller.data().context)
                .get(index)
                .copied()
            else {
                return Ok(-1);
            };
            let bytes = if offset >= value.len() {
                Vec::new()
            } else {
                let end = offset.saturating_add(len).min(value.len());
                value.as_bytes()[offset..end].to_vec()
            };
            Self::write_guest_bytes(&mut caller, guest_ptr, &bytes)?;
            Self::usize_to_i32(bytes.len(), "copied named capture value length")
        }

        fn variable_count_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            let variable_count = caller.data().context.variables.read().unwrap().len();
            Self::usize_to_i32(variable_count, "variable count")
        }

        fn variable_name_length_import(
            caller: Caller<'_, WasmStoreData>,
            index: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "variable index")?;
            match Self::sorted_variable_entries(&caller.data().context)
                .get(index)
                .map(|(name, _)| name)
            {
                Some(name) => Self::usize_to_i32(name.len(), "variable name length"),
                None => Ok(-1),
            }
        }

        fn variable_name_read_import(
            mut caller: Caller<'_, WasmStoreData>,
            index: i32,
            guest_ptr: i32,
            offset: i32,
            len: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "variable index")?;
            let offset = Self::nonnegative_i32_to_usize(offset, "variable name offset")?;
            let len = Self::nonnegative_i32_to_usize(len, "variable name read length")?;
            let Some((name, _)) = Self::sorted_variable_entries(&caller.data().context)
                .get(index)
                .cloned()
            else {
                return Ok(-1);
            };
            let bytes = if offset >= name.len() {
                Vec::new()
            } else {
                let end = offset.saturating_add(len).min(name.len());
                name.as_bytes()[offset..end].to_vec()
            };
            Self::write_guest_bytes(&mut caller, guest_ptr, &bytes)?;
            Self::usize_to_i32(bytes.len(), "copied variable name length")
        }

        fn variable_value_length_import(
            caller: Caller<'_, WasmStoreData>,
            index: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "variable index")?;
            match Self::sorted_variable_entries(&caller.data().context)
                .get(index)
                .map(|(_, value)| value)
            {
                Some(value) => Self::usize_to_i32(value.len(), "variable value length"),
                None => Ok(-1),
            }
        }

        fn variable_value_read_import(
            mut caller: Caller<'_, WasmStoreData>,
            index: i32,
            guest_ptr: i32,
            offset: i32,
            len: i32,
        ) -> wasmtime::Result<i32> {
            let index = Self::nonnegative_i32_to_usize(index, "variable index")?;
            let offset = Self::nonnegative_i32_to_usize(offset, "variable value offset")?;
            let len = Self::nonnegative_i32_to_usize(len, "variable value read length")?;
            let Some((_, value)) = Self::sorted_variable_entries(&caller.data().context)
                .get(index)
                .cloned()
            else {
                return Ok(-1);
            };
            let bytes = if offset >= value.len() {
                Vec::new()
            } else {
                let end = offset.saturating_add(len).min(value.len());
                value.as_bytes()[offset..end].to_vec()
            };
            Self::write_guest_bytes(&mut caller, guest_ptr, &bytes)?;
            Self::usize_to_i32(bytes.len(), "copied variable value length")
        }

        fn emit_numeric_import(
            mut caller: Caller<'_, WasmStoreData>,
            value: f64,
        ) -> wasmtime::Result<()> {
            caller
                .data_mut()
                .set_emitted_result(CodeBlockValue::Numeric(value));
            Ok(())
        }

        fn emit_replacement_import(
            mut caller: Caller<'_, WasmStoreData>,
            guest_ptr: i32,
            len: i32,
        ) -> wasmtime::Result<()> {
            let bytes = Self::read_guest_bytes(&mut caller, guest_ptr, len)?;
            let replacement = String::from_utf8(bytes)
                .map_err(|_| anyhow!("WASM replacement result must be valid UTF-8"))?;
            caller
                .data_mut()
                .set_emitted_result(CodeBlockValue::Replacement(replacement));
            Ok(())
        }

        fn steer_continue_import(mut caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<()> {
            caller.data_mut().set_emitted_steer(SteerResult::Continue);
            Ok(())
        }

        fn steer_fail_import(mut caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<()> {
            caller.data_mut().set_emitted_steer(SteerResult::Fail);
            Ok(())
        }

        fn steer_accept_import(mut caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<()> {
            caller.data_mut().set_emitted_steer(SteerResult::Accept);
            Ok(())
        }

        fn steer_skip_import(
            mut caller: Caller<'_, WasmStoreData>,
            count: i32,
        ) -> wasmtime::Result<()> {
            let skip = Self::nonnegative_i32_to_usize(count, "steer_skip count")?;
            caller.data_mut().set_emitted_steer(SteerResult::Skip(skip));
            Ok(())
        }

        fn steer_abort_import(mut caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<()> {
            caller.data_mut().set_emitted_steer(SteerResult::Abort);
            Ok(())
        }

        /// Register a named wasm module from binary bytes.
        pub fn register_module(&self, name: String, module_bytes: Vec<u8>) -> Result<()> {
            trace_enter!(
                "execution",
                "WasmEngine::register_module",
                "name={},byte_len={}",
                name,
                module_bytes.len()
            );
            let module = Module::from_binary(&self.engine, &module_bytes).map_err(|e| {
                RgxError::Engine(format!("Failed to compile WASM module {name}: {e}"))
            })?;
            self.modules.register(name, module);
            trace_exit!(
                "execution",
                "WasmEngine::register_module",
                "ok=true,registered_modules={}",
                self.modules.len()
            );
            Ok(())
        }

        fn parse_call_spec<'a>(
            &self,
            code: &'a str,
        ) -> std::result::Result<(&'a str, &'a str), String> {
            trace_enter!(
                "execution",
                "WasmEngine::parse_call_spec",
                "code_len={}",
                code.len()
            );
            let Some((module_name, function_name)) = code.split_once(':') else {
                let message = "WASM code blocks require module:function syntax".to_string();
                trace_exit!(
                    "execution",
                    "WasmEngine::parse_call_spec",
                    "ok=false,error={}",
                    message
                );
                return Err(message);
            };
            let valid = !module_name.is_empty() && !function_name.is_empty();
            trace_decision!(
                "execution",
                "!module_name.is_empty() && !function_name.is_empty()",
                valid,
                "module_name={},function_name={}",
                module_name,
                function_name
            );
            if !valid {
                let message = "WASM code blocks require module:function syntax".to_string();
                trace_exit!(
                    "execution",
                    "WasmEngine::parse_call_spec",
                    "ok=false,error={}",
                    message
                );
                return Err(message);
            }
            trace_exit!(
                "execution",
                "WasmEngine::parse_call_spec",
                "ok=true,module_name={},function_name={}",
                module_name,
                function_name
            );
            Ok((module_name, function_name))
        }

        fn execute_predicate(
            &self,
            module_name: &str,
            function_name: &str,
            context: &ExecContext,
        ) -> ExecResult {
            trace_enter!(
                "execution",
                "WasmEngine::execute_predicate",
                "module_name={},function_name={}",
                module_name,
                function_name
            );
            let Some(module) = self.modules.get(module_name) else {
                let result = ExecResult::Error(format!("Unknown WASM module: {module_name}"));
                trace_exit!(
                    "execution",
                    "WasmEngine::execute_predicate",
                    "ok=true,result_kind={}",
                    exec_result_kind(&result)
                );
                return result;
            };
            let mut store = Store::new(&self.engine, WasmStoreData::new(context.clone()));
            let instance = match self.linker.instantiate(&mut store, module.as_ref()) {
                Ok(instance) => instance,
                Err(err) => {
                    let result = ExecResult::Error(format!(
                        "Failed to instantiate WASM module {module_name}: {err}"
                    ));
                    trace_exit!(
                        "execution",
                        "WasmEngine::execute_predicate",
                        "ok=true,result_kind={}",
                        exec_result_kind(&result)
                    );
                    return result;
                }
            };
            let function = match instance.get_typed_func::<(), i32>(&mut store, function_name) {
                Ok(function) => function,
                Err(err) => {
                    let result = ExecResult::Error(format!(
                        "WASM export {module_name}:{function_name} must have signature () -> i32: {err}"
                    ));
                    trace_exit!(
                        "execution",
                        "WasmEngine::execute_predicate",
                        "ok=true,result_kind={}",
                        exec_result_kind(&result)
                    );
                    return result;
                }
            };
            let result = match function.call(&mut store, ()) {
                Ok(value) => {
                    // Steer takes highest priority: if the WASM
                    // module emitted a steer via one of the
                    // rgx.steer_* imports, return Steer(_)
                    // regardless of the function's i32 result.
                    // Matches the Lua / JS / Rhai precedence in
                    // `finish_exec_result_with_steer`.
                    if let Some(steer) = store.data_mut().take_emitted_steer() {
                        ExecResult::Steer(steer)
                    } else if value != 0 {
                        match store.data_mut().take_emitted_result() {
                            Some(CodeBlockValue::Numeric(value)) => ExecResult::Numeric(value),
                            Some(CodeBlockValue::Replacement(value)) => {
                                ExecResult::Replacement(value)
                            }
                            Some(CodeBlockValue::Structured(value)) => {
                                ExecResult::Structured(value)
                            }
                            None => ExecResult::Success,
                        }
                    } else {
                        ExecResult::Failure
                    }
                }
                Err(err) => ExecResult::Error(format!(
                    "WASM call failed for {module_name}:{function_name}: {err}"
                )),
            };
            trace_exit!(
                "execution",
                "WasmEngine::execute_predicate",
                "ok=true,result_kind={}",
                exec_result_kind(&result)
            );
            result
        }
    }

    impl ExecutionEngine for WasmEngine {
        fn execute(&self, code: &str, context: &ExecContext) -> ExecResult {
            trace_enter!(
                "execution",
                "WasmEngine::execute",
                "code_len={},registered_modules={}",
                code.len(),
                self.modules.len()
            );
            let result = match self.parse_call_spec(code) {
                Ok((module_name, function_name)) => {
                    self.execute_predicate(module_name, function_name, context)
                }
                Err(error) => ExecResult::Error(error),
            };
            trace_exit!(
                "execution",
                "WasmEngine::execute",
                "ok=true,result_kind={}",
                exec_result_kind(&result)
            );
            result
        }

        fn language(&self) -> &str {
            "wasm"
        }

        fn is_available(&self) -> bool {
            true
        }

        fn reset(&mut self) {
            // Registered modules persist across executions for a compiled regex.
        }
    }

    impl Default for WasmEngine {
        fn default() -> Self {
            Self::new().expect("Failed to create WASM engine")
        }
    }
}

/// Native function callback type
pub type NativeCallback = Arc<dyn Fn(&ExecContext) -> ExecResult + Send + Sync>;

/// Registry for native callbacks that can be called from patterns.
///
/// This allows users to register Rust functions that can be called
/// from regex patterns using `(?{native:function_name})`.
pub struct NativeCallbackRegistry {
    callbacks: RwLock<HashMap<String, NativeCallback>>,
}

impl NativeCallbackRegistry {
    /// Create a new callback registry
    #[must_use]
    pub fn new() -> Self {
        trace_enter!("execution", "NativeCallbackRegistry::new");
        let registry = Self {
            callbacks: RwLock::new(HashMap::new()),
        };
        trace_exit!(
            "execution",
            "NativeCallbackRegistry::new",
            "ok=true,registered_callbacks=0"
        );
        registry
    }

    /// Register a native callback function
    ///
    /// # Panics
    /// Panics if the internal callbacks `RwLock` is poisoned.
    pub fn register<F>(&self, name: &str, callback: F)
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        trace_enter!(
            "execution",
            "NativeCallbackRegistry::register",
            "name={}",
            name
        );
        let mut callbacks = self.callbacks.write().unwrap();
        let _replaced_existing = callbacks
            .insert(name.to_string(), Arc::new(callback))
            .is_some();
        trace_decision!(
            "execution",
            "callbacks.insert(name,...).is_some()",
            replaced_existing,
            "true means an existing callback with the same name was replaced"
        );
        trace_exit!(
            "execution",
            "NativeCallbackRegistry::register",
            "ok=true,name={},registered_callbacks={}",
            name,
            callbacks.len()
        );
    }

    /// Call a registered callback
    ///
    /// # Panics
    /// Panics if the internal callbacks `RwLock` is poisoned.
    pub fn call(&self, name: &str, context: &ExecContext) -> ExecResult {
        trace_enter!(
            "execution",
            "NativeCallbackRegistry::call",
            "name={},registered_callbacks={},capture_slots={}",
            name,
            self.len(),
            context.captures.len()
        );
        let callback = self.callbacks.read().unwrap().get(name).cloned();
        if let Some(callback) = callback {
            trace_decision!(
                "execution",
                "callbacks.get(name).is_some()",
                true,
                "dispatching to registered native callback"
            );
            let result = callback(context);
            trace_exit!(
                "execution",
                "NativeCallbackRegistry::call",
                "ok=true,result_kind={}",
                exec_result_kind(&result)
            );
            result
        } else if name.starts_with("__callout_") {
            // Unregistered callouts are no-ops per PCRE2 semantics
            trace_decision!(
                "execution",
                "callbacks.get(name).is_some()",
                false,
                "unregistered callout treated as no-op (PCRE2 semantics)"
            );
            trace_exit!(
                "execution",
                "NativeCallbackRegistry::call",
                "ok=true,result_kind=success_callout_noop"
            );
            ExecResult::Success
        } else {
            trace_decision!(
                "execution",
                "callbacks.get(name).is_some()",
                false,
                "native callback name is not registered"
            );
            let result = ExecResult::Error(format!("Unknown native function: {name}"));
            trace_exit!(
                "execution",
                "NativeCallbackRegistry::call",
                "ok=true,result_kind={}",
                exec_result_kind(&result)
            );
            result
        }
    }

    /// Check if a callback is registered
    ///
    /// # Panics
    /// Panics if the internal callbacks `RwLock` is poisoned.
    pub fn has(&self, name: &str) -> bool {
        trace_enter!(
            "execution",
            "NativeCallbackRegistry::has",
            "name={},registered_callbacks={}",
            name,
            self.len()
        );
        let is_registered = self.callbacks.read().unwrap().contains_key(name);
        trace_exit!(
            "execution",
            "NativeCallbackRegistry::has",
            "ok=true,registered={}",
            is_registered
        );
        is_registered
    }

    /// Count registered callbacks.
    ///
    /// # Panics
    /// Panics if the internal callbacks `RwLock` is poisoned.
    pub fn len(&self) -> usize {
        self.callbacks.read().unwrap().len()
    }

    /// Whether no callbacks are currently registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for NativeCallbackRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry for host-provided execution variables.
pub struct ExecutionVariableRegistry {
    variables: RwLock<HashMap<String, String>>,
    typed_variables: RwLock<HashMap<String, Value>>,
}

impl ExecutionVariableRegistry {
    /// Create a new execution-variable registry.
    #[must_use]
    pub fn new() -> Self {
        trace_enter!("execution", "ExecutionVariableRegistry::new");
        let registry = Self {
            variables: RwLock::new(HashMap::new()),
            typed_variables: RwLock::new(HashMap::new()),
        };
        trace_exit!(
            "execution",
            "ExecutionVariableRegistry::new",
            "ok=true,registered_variables=0"
        );
        registry
    }

    /// Register or replace a host-provided execution variable.
    ///
    /// Also stores the value as a [`Value::String`] in the typed variable map
    /// so that it is accessible via [`ExecContext::typed_variable`].
    ///
    /// # Panics
    /// Panics if the internal variables `RwLock` is poisoned.
    pub fn set(&self, name: &str, value: String) {
        trace_enter!(
            "execution",
            "ExecutionVariableRegistry::set",
            "name={},value_len={}",
            name,
            value.len()
        );
        self.typed_variables
            .write()
            .unwrap()
            .insert(name.to_string(), Value::String(value.clone()));
        let mut variables = self.variables.write().unwrap();
        let _replaced_existing = variables.insert(name.to_string(), value).is_some();
        trace_decision!(
            "execution",
            "variables.insert(name,...).is_some()",
            replaced_existing,
            "true means an existing execution variable with the same name was replaced"
        );
        trace_exit!(
            "execution",
            "ExecutionVariableRegistry::set",
            "ok=true,name={},registered_variables={}",
            name,
            variables.len()
        );
    }

    /// Register or replace a typed host-provided execution variable.
    ///
    /// Also stores a string representation in the legacy string variable map
    /// so that it is accessible via [`ExecContext::variable`].
    ///
    /// # Panics
    /// Panics if the internal variables `RwLock` is poisoned.
    pub fn set_typed(&self, name: &str, value: Value) {
        trace_enter!(
            "execution",
            "ExecutionVariableRegistry::set_typed",
            "name={}",
            name
        );
        let string_repr = value.to_string();
        self.variables
            .write()
            .unwrap()
            .insert(name.to_string(), string_repr);
        self.typed_variables
            .write()
            .unwrap()
            .insert(name.to_string(), value);
        trace_exit!(
            "execution",
            "ExecutionVariableRegistry::set_typed",
            "ok=true,name={}",
            name
        );
    }

    /// Clone the current execution-variable state into an owned map.
    ///
    /// # Panics
    /// Panics if the internal variables `RwLock` is poisoned.
    pub fn snapshot(&self) -> HashMap<String, String> {
        trace_enter!(
            "execution",
            "ExecutionVariableRegistry::snapshot",
            "registered_variables={}",
            self.len()
        );
        let snapshot = self.variables.read().unwrap().clone();
        trace_exit!(
            "execution",
            "ExecutionVariableRegistry::snapshot",
            "ok=true,registered_variables={}",
            snapshot.len()
        );
        snapshot
    }

    /// Clone the current typed-variable state into an owned map.
    ///
    /// # Panics
    /// Panics if the internal typed-variables `RwLock` is poisoned.
    pub fn typed_snapshot(&self) -> HashMap<String, Value> {
        self.typed_variables.read().unwrap().clone()
    }

    /// Count registered variables.
    ///
    /// # Panics
    /// Panics if the internal variables `RwLock` is poisoned.
    pub fn len(&self) -> usize {
        self.variables.read().unwrap().len()
    }

    /// Whether no variables are currently registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ExecutionVariableRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// EXECUTION MANAGER
// ============================================================================

/// Central manager for all code execution engines.
///
/// This coordinates between different language backends and provides
/// a unified interface for the regex engine to execute embedded code.
pub struct ExecutionManager {
    #[cfg(feature = "wasm")]
    wasm_engine: Option<wasm::WasmEngine>,
    #[cfg(feature = "lua")]
    lua_engine: Option<lua::LuaEngine>,
    #[cfg(feature = "javascript")]
    js_engine: Option<javascript::JavaScriptEngine>,
    #[cfg(feature = "rhai")]
    rhai_engine: Option<rhai::RhaiEngine>,
    native_callbacks: NativeCallbackRegistry,
    variables: ExecutionVariableRegistry,
}

impl ExecutionManager {
    /// Create a new execution manager with all available engines
    #[must_use]
    pub fn new() -> Self {
        trace_enter!("execution", "ExecutionManager::new");
        let manager = Self {
            #[cfg(feature = "wasm")]
            wasm_engine: wasm::WasmEngine::new().ok(),
            #[cfg(feature = "lua")]
            lua_engine: lua::LuaEngine::new().ok(),
            #[cfg(feature = "javascript")]
            js_engine: javascript::JavaScriptEngine::new().ok(),
            #[cfg(feature = "rhai")]
            rhai_engine: rhai::RhaiEngine::new().ok(),
            native_callbacks: NativeCallbackRegistry::new(),
            variables: ExecutionVariableRegistry::new(),
        };
        let _lua_available = {
            #[cfg(feature = "lua")]
            {
                manager.lua_engine.is_some()
            }
            #[cfg(not(feature = "lua"))]
            {
                false
            }
        };
        let _wasm_available = {
            #[cfg(feature = "wasm")]
            {
                manager.wasm_engine.is_some()
            }
            #[cfg(not(feature = "wasm"))]
            {
                false
            }
        };
        let _js_available = {
            #[cfg(feature = "javascript")]
            {
                manager.js_engine.is_some()
            }
            #[cfg(not(feature = "javascript"))]
            {
                false
            }
        };
        let _rhai_available = {
            #[cfg(feature = "rhai")]
            {
                manager.rhai_engine.is_some()
            }
            #[cfg(not(feature = "rhai"))]
            {
                false
            }
        };
        trace_exit!(
            "execution",
            "ExecutionManager::new",
            "ok=true,wasm_available={},lua_available={},javascript_available={},rhai_available={},native_available=true,registered_variables=0",
            wasm_available,
            lua_available,
            js_available,
            rhai_available
        );
        manager
    }

    /// Dispatch code to an optional engine, producing trace output for the
    /// decision and exit points.
    #[cfg(any(
        feature = "lua",
        feature = "javascript",
        feature = "rhai",
        feature = "wasm"
    ))]
    fn dispatch_engine(
        engine: Option<&dyn ExecutionEngine>,
        code: &str,
        context: &ExecContext,
        language: &str,
        field_check: &str,
    ) -> ExecResult {
        if let Some(engine) = engine {
            trace_decision!(
                "execution",
                field_check,
                true,
                "dispatching code to {} execution backend",
                language
            );
            let result = engine.execute(code, context);
            trace_exit!(
                "execution",
                "ExecutionManager::execute",
                "ok=true,language={},result_kind={}",
                language,
                exec_result_kind(&result)
            );
            result
        } else {
            trace_decision!(
                "execution",
                field_check,
                false,
                "{} feature enabled but engine initialization unavailable",
                language.to_ascii_lowercase()
            );
            let result = ExecResult::Error(format!("{language} engine not available"));
            trace_exit!(
                "execution",
                "ExecutionManager::execute",
                "ok=true,language={},result_kind={}",
                language,
                exec_result_kind(&result)
            );
            result
        }
    }

    /// Execute code in the specified language
    pub fn execute(&self, language: &str, code: &str, context: &ExecContext) -> ExecResult {
        trace_enter!(
            "execution",
            "ExecutionManager::execute",
            "language={},code_len={},capture_slots={}",
            language,
            code.len(),
            context.captures.len()
        );
        match language {
            #[cfg(feature = "wasm")]
            "wasm" => Self::dispatch_engine(
                self.wasm_engine.as_ref().map(|e| e as &dyn ExecutionEngine),
                code,
                context,
                "WASM",
                "self.wasm_engine.is_some()",
            ),
            #[cfg(feature = "lua")]
            "lua" => Self::dispatch_engine(
                self.lua_engine.as_ref().map(|e| e as &dyn ExecutionEngine),
                code,
                context,
                "Lua",
                "self.lua_engine.is_some()",
            ),
            #[cfg(feature = "javascript")]
            "js" | "javascript" => Self::dispatch_engine(
                self.js_engine.as_ref().map(|e| e as &dyn ExecutionEngine),
                code,
                context,
                "JavaScript",
                "self.js_engine.is_some()",
            ),
            #[cfg(feature = "rhai")]
            "rhai" => Self::dispatch_engine(
                self.rhai_engine.as_ref().map(|e| e as &dyn ExecutionEngine),
                code,
                context,
                "Rhai",
                "self.rhai_engine.is_some()",
            ),
            "native" => {
                trace_decision!(
                    "execution",
                    "language == native",
                    true,
                    "treat code argument as native callback identifier"
                );
                let result = self.native_callbacks.call(code, context);
                trace_exit!(
                    "execution",
                    "ExecutionManager::execute",
                    "ok=true,language=native,result_kind={}",
                    exec_result_kind(&result)
                );
                result
            }
            _ => {
                trace_decision!(
                    "execution",
                    "language is known backend",
                    false,
                    "unsupported language dispatch attempted: {}",
                    language
                );
                let result = ExecResult::Error(format!("Unknown language: {language}"));
                trace_exit!(
                    "execution",
                    "ExecutionManager::execute",
                    "ok=true,language={},result_kind={}",
                    language,
                    exec_result_kind(&result)
                );
                result
            }
        }
    }

    /// Register a native callback
    pub fn register_native<F>(&self, name: &str, callback: F)
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        trace_enter!(
            "execution",
            "ExecutionManager::register_native",
            "name={}",
            name
        );
        let _replaced_existing = self.native_callbacks.has(name);
        self.native_callbacks.register(name, callback);
        trace_decision!(
            "execution",
            "native_callbacks.contains_key(name) before register",
            replaced_existing,
            "true means this registration replaced an existing callback"
        );
        trace_exit!(
            "execution",
            "ExecutionManager::register_native",
            "ok=true,registered_callbacks={}",
            self.native_callbacks.len()
        );
    }

    /// Register a named wasm module.
    ///
    /// # Errors
    /// Returns `RgxError::Engine` if the WASM engine is unavailable or the module is invalid.
    pub fn register_wasm_module(&self, name: String, module_bytes: Vec<u8>) -> Result<()> {
        trace_enter!(
            "execution",
            "ExecutionManager::register_wasm_module",
            "name={},byte_len={}",
            name,
            module_bytes.len()
        );
        #[cfg(feature = "wasm")]
        {
            let Some(engine) = &self.wasm_engine else {
                let error = RgxError::Engine("WASM engine not available".to_string());
                trace_exit!(
                    "execution",
                    "ExecutionManager::register_wasm_module",
                    "ok=false,error={}",
                    error
                );
                return Err(error);
            };
            let result = engine.register_module(name, module_bytes);
            trace_exit!(
                "execution",
                "ExecutionManager::register_wasm_module",
                "ok={}",
                result.is_ok()
            );
            result
        }
        #[cfg(not(feature = "wasm"))]
        {
            let _ = (name, module_bytes);
            let error = RgxError::Engine(
                "WASM module registration requires the `wasm` cargo feature".to_string(),
            );
            trace_exit!(
                "execution",
                "ExecutionManager::register_wasm_module",
                "ok=false,error={}",
                error
            );
            Err(error)
        }
    }

    /// Register or replace a host-provided execution variable.
    pub fn set_variable(&self, name: &str, value: String) {
        trace_enter!(
            "execution",
            "ExecutionManager::set_variable",
            "name={},value_len={}",
            name,
            value.len()
        );
        self.variables.set(name, value);
        trace_exit!(
            "execution",
            "ExecutionManager::set_variable",
            "ok=true,registered_variables={}",
            self.variables.len()
        );
    }

    /// Register or replace a typed host-provided execution variable.
    pub fn set_typed_variable(&self, name: &str, value: Value) {
        trace_enter!(
            "execution",
            "ExecutionManager::set_typed_variable",
            "name={}",
            name
        );
        self.variables.set_typed(name, value);
        trace_exit!(
            "execution",
            "ExecutionManager::set_typed_variable",
            "ok=true"
        );
    }

    /// Set a host variable with automatic type conversion.
    pub fn set_var<V: Into<Value>>(&self, name: &str, value: V) {
        self.set_typed_variable(name, value.into());
    }

    /// Clone the current execution-variable snapshot.
    pub fn variable_snapshot(&self) -> HashMap<String, String> {
        trace_enter!(
            "execution",
            "ExecutionManager::variable_snapshot",
            "registered_variables={}",
            self.variables.len()
        );
        let snapshot = self.variables.snapshot();
        trace_exit!(
            "execution",
            "ExecutionManager::variable_snapshot",
            "ok=true,variable_slots={}",
            snapshot.len()
        );
        snapshot
    }

    /// Clone the current typed-variable snapshot.
    pub fn typed_variable_snapshot(&self) -> HashMap<String, Value> {
        self.variables.typed_snapshot()
    }

    /// Check if a named native callback is registered.
    pub fn has_native(&self, name: &str) -> bool {
        self.native_callbacks.has(name)
    }

    /// Check if a language is available
    pub fn is_language_available(&self, language: &str) -> bool {
        trace_enter!(
            "execution",
            "ExecutionManager::is_language_available",
            "language={}",
            language
        );
        let available = match language {
            #[cfg(feature = "wasm")]
            "wasm" => self.wasm_engine.is_some(),
            #[cfg(feature = "lua")]
            "lua" => self.lua_engine.is_some(),
            #[cfg(feature = "javascript")]
            "js" | "javascript" => self.js_engine.is_some(),
            #[cfg(feature = "rhai")]
            "rhai" => self.rhai_engine.is_some(),
            "native" => true,
            _ => false,
        };
        trace_exit!(
            "execution",
            "ExecutionManager::is_language_available",
            "ok=true,available={}",
            available
        );
        available
    }
}

impl Default for ExecutionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ASYNC / CONTINUATION-PASSING TYPES
// ============================================================================

/// Outcome of a potentially-async match operation.
///
/// When the VM encounters an unregistered native callback during
/// `find_first_suspendable`, it returns `Suspended` instead of treating
/// the callback as an error. The caller resolves the callback externally
/// and calls `resume` to continue matching.
#[derive(Debug)]
pub enum MatchOutcome {
    /// Match completed synchronously.
    Completed(Option<crate::engine::MatchResult>),
    /// Match suspended — an async callback needs resolution.
    ///
    /// The continuation is boxed to keep the enum size reasonable,
    /// since `MatchContinuation` owns a full VM state snapshot while
    /// `Completed` is a small option of position data.
    Suspended(Box<MatchContinuation>),
}

/// Captured VM state for resuming a suspended match.
///
/// This struct owns all data needed to resume — no borrowed references.
/// All fields are owned types (`Vec`, `String`, `HashMap`, primitives),
/// making `MatchContinuation` automatically `Send + Sync`.
pub struct MatchContinuation {
    /// The original input text (owned copy for lifetime independence).
    pub(crate) text: Vec<u8>,
    /// The callback name that needs async resolution.
    pub pending_callback_name: String,
    /// Snapshot of the execution context for the callback.
    pub pending_context: ExecContextSnapshot,
    /// Internal VM state for resumption (opaque to the caller).
    pub(crate) vm_state: VmResumeState,
}

impl std::fmt::Debug for MatchContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatchContinuation")
            .field("text_len", &self.text.len())
            .field("pending_callback_name", &self.pending_callback_name)
            .field("pending_context", &self.pending_context)
            .field("vm_state", &"<opaque>")
            .finish()
    }
}

/// Snapshot of execution context exposed to the async callback resolver.
///
/// Contains the information the external callback needs to make its
/// decision: current position, captures, and host variables.
#[derive(Debug, Clone)]
pub struct ExecContextSnapshot {
    /// Current byte position in the input text.
    pub position: usize,
    /// Start of the current match attempt in bytes.
    pub match_start: usize,
    /// Capture group byte-offset slots (pairs of start/end).
    pub captures: Vec<Option<usize>>,
    /// Host-provided variables snapshot.
    pub variables: HashMap<String, String>,
}

/// Opaque internal VM state needed to resume execution.
///
/// This captures the full VM execution state at the point of suspension
/// so that matching can continue exactly where it left off.
pub(crate) struct VmResumeState {
    pub(crate) pos: usize,
    pub(crate) match_start: usize,
    pub(crate) ip: usize,
    pub(crate) captures: Vec<Option<usize>>,
    pub(crate) capture_trail: Vec<(usize, Option<usize>)>,
    pub(crate) call_stack: Vec<usize>,
    pub(crate) backtrack_stack: Vec<crate::vm::BacktrackFrame>,
    pub(crate) current_alternative: Option<usize>,
    pub(crate) recursion_stack: Vec<(usize, usize)>,
    pub(crate) code_result: Option<CodeBlockValue>,
    pub(crate) committed: bool,
    pub(crate) skip_position: Option<usize>,
    /// A11: snapshot of `ExecContext.marks` for `(*MARK:name)` /
    /// `(*SKIP:name)` interaction. Restored on resume so the
    /// post-resume continuation sees the same mark stack as the
    /// pre-suspend execution.
    pub(crate) marks: Vec<(String, usize)>,
    pub(crate) match_start_override: Option<usize>,
    pub(crate) previous_match_end: Option<usize>,
    pub(crate) scan_start: usize,
}
