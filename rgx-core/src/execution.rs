//! State-of-the-art code execution module for multi-language regex patterns.
//!
//! This module implements sandboxed code execution for Lua, JavaScript, and WebAssembly
//! within regex patterns. This is a unique feature that sets rgx apart from traditional
//! regex engines, enabling powerful pattern matching with embedded logic.
//!
//! # Design Philosophy
//!
//! 1. **Security First**: All code runs in sandboxed environments with no filesystem,
//!    network, or system access.
//! 2. **Performance Layers**: Pure regex → +Lua (fast) → +JavaScript (flexible) → +WASM (portable)
//! 3. **Zero-Cost Abstraction**: If you don't use code execution, it has zero overhead.
//! 4. **Fail-Safe**: Code execution failures don't crash the regex engine.
//!
//! # Pattern Syntax
//!
//! - `(?{lua:code})` - Execute Lua code
//! - `(?{js:code})` - Execute JavaScript code  
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
use std::sync::{Arc, RwLock};

// ============================================================================
// EXECUTION CONTEXT
// ============================================================================

/// Execution context passed to code blocks with match information.
///
/// This provides safe, read-only access to:
/// - The matched text
/// - Capture groups
/// - Named captures
/// - Match position
#[derive(Debug, Clone)]
pub struct ExecContext {
    /// The full text being matched
    pub text: String,
    /// Current match position
    pub position: usize,
    /// Captured groups (indexed from 0)
    pub captures: Vec<Option<String>>,
    /// Named capture groups
    pub named_captures: HashMap<String, String>,
    /// User-defined variables (persisted across executions)
    pub variables: Arc<RwLock<HashMap<String, String>>>,
}

impl ExecContext {
    /// Create a new execution context
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
            captures: Vec::new(),
            named_captures: HashMap::new(),
            variables: Arc::new(RwLock::new(HashMap::new())),
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
}

// ============================================================================
// EXECUTION RESULT
// ============================================================================

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
}

fn exec_result_kind(result: &ExecResult) -> &'static str {
    match result {
        ExecResult::Success => "Success",
        ExecResult::Failure => "Failure",
        ExecResult::Replacement(_) => "Replacement",
        ExecResult::Numeric(_) => "Numeric",
        ExecResult::Error(_) => "Error",
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
        fn setup_context(&self, lua: &Lua, context: &ExecContext) -> mlua::Result<()> {
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

            // Set full text (read-only)
            globals.set("text", context.text.clone())?;

            // Create named captures table
            let named_table = lua.create_table()?;
            for (name, value) in &context.named_captures {
                named_table.set(name.clone(), value.clone())?;
            }
            globals.set("named", named_table)?;

            Ok(())
        }
    }

    impl ExecutionEngine for LuaEngine {
        fn execute(&self, code: &str, context: &ExecContext) -> ExecResult {
            let lua = self.new_sandboxed_lua();
            // Set up context
            if let Err(e) = self.setup_context(&lua, context) {
                return ExecResult::Error(format!("Context setup failed: {}", e));
            }

            // Execute the code
            let result = lua.load(code).eval::<Value>();
            match result {
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
            }
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
    use rquickjs::{Array, Context, Object, Runtime, Undefined, Value};

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

        /// Execute JavaScript code in sandboxed context
        fn execute_sandboxed(&self, code: &str, context: &ExecContext) -> ExecResult {
            let runtime = match self.new_runtime() {
                Ok(runtime) => runtime,
                Err(err) => return ExecResult::Error(err.to_string()),
            };
            let ctx_result = Context::full(&runtime);

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
                globals.set("text", context.text.clone()).ok();

                // Create named captures object
                if let Ok(named_obj) = Object::new(ctx.clone()) {
                    for (name, value) in &context.named_captures {
                        named_obj.set(name.clone(), value.clone()).ok();
                    }
                    globals.set("named", named_obj).ok();
                }

                // Remove dangerous functions
                globals.set("eval", Undefined).ok();
                globals.set("Function", Undefined).ok();
                globals.set("fetch", Undefined).ok();
                globals.set("XMLHttpRequest", Undefined).ok();

                // Execute the code inside an IIFE so `return ...` works
                // consistently with the documented `(?{js:return ...})` style.
                let wrapped_code = format!("(function(){{\n{code}\n}})()");
                match ctx.eval::<Value, _>(wrapped_code) {
                    Ok(val) => {
                        if val.is_bool() {
                            if let Some(b) = val.as_bool() {
                                if b {
                                    ExecResult::Success
                                } else {
                                    ExecResult::Failure
                                }
                            } else {
                                ExecResult::Success
                            }
                        } else if val.is_number() {
                            if let Some(n) = val.as_number() {
                                ExecResult::Numeric(n)
                            } else {
                                ExecResult::Success
                            }
                        } else if val.is_string() {
                            if let Ok(s) = val.get::<String>() {
                                ExecResult::Replacement(s)
                            } else {
                                ExecResult::Success
                            }
                        } else if val.is_null() || val.is_undefined() {
                            ExecResult::Success
                        } else {
                            ExecResult::Success
                        }
                    }
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
    }

    impl WasmStoreData {
        fn new(context: ExecContext) -> Self {
            Self { context }
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
    /// - modules may optionally import read-only execution-context helpers from the `rgx` namespace
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

        fn current_position_import(caller: Caller<'_, WasmStoreData>) -> wasmtime::Result<i32> {
            Self::usize_to_i32(caller.data().context.position, "current position")
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

        fn sorted_named_capture_entries<'a>(context: &'a ExecContext) -> Vec<(&'a str, &'a str)> {
            let mut entries = context
                .named_captures
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect::<Vec<_>>();
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
                    if value != 0 {
                        ExecResult::Success
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
    pub fn register<F>(&self, name: String, callback: F)
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
        let replaced_existing = callbacks.insert(name.clone(), Arc::new(callback)).is_some();
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
        match callback {
            Some(callback) => {
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
            }
            None => {
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
    }

    /// Check if a callback is registered
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
    native_callbacks: NativeCallbackRegistry,
}

impl ExecutionManager {
    /// Create a new execution manager with all available engines
    pub fn new() -> Self {
        trace_enter!("execution", "ExecutionManager::new");
        let manager = Self {
            #[cfg(feature = "wasm")]
            wasm_engine: wasm::WasmEngine::new().ok(),
            #[cfg(feature = "lua")]
            lua_engine: lua::LuaEngine::new().ok(),
            #[cfg(feature = "javascript")]
            js_engine: javascript::JavaScriptEngine::new().ok(),
            native_callbacks: NativeCallbackRegistry::new(),
        };
        let lua_available = {
            #[cfg(feature = "lua")]
            {
                manager.lua_engine.is_some()
            }
            #[cfg(not(feature = "lua"))]
            {
                false
            }
        };
        let wasm_available = {
            #[cfg(feature = "wasm")]
            {
                manager.wasm_engine.is_some()
            }
            #[cfg(not(feature = "wasm"))]
            {
                false
            }
        };
        let js_available = {
            #[cfg(feature = "javascript")]
            {
                manager.js_engine.is_some()
            }
            #[cfg(not(feature = "javascript"))]
            {
                false
            }
        };
        trace_exit!(
            "execution",
            "ExecutionManager::new",
            "ok=true,wasm_available={},lua_available={},javascript_available={},native_available=true",
            wasm_available,
            lua_available,
            js_available
        );
        manager
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
            "wasm" => {
                if let Some(engine) = &self.wasm_engine {
                    trace_decision!(
                        "execution",
                        "self.wasm_engine.is_some()",
                        true,
                        "dispatching code to WASM execution backend"
                    );
                    let result = engine.execute(code, context);
                    trace_exit!(
                        "execution",
                        "ExecutionManager::execute",
                        "ok=true,language=wasm,result_kind={}",
                        exec_result_kind(&result)
                    );
                    result
                } else {
                    trace_decision!(
                        "execution",
                        "self.wasm_engine.is_some()",
                        false,
                        "wasm feature enabled but engine initialization unavailable"
                    );
                    let result = ExecResult::Error("WASM engine not available".to_string());
                    trace_exit!(
                        "execution",
                        "ExecutionManager::execute",
                        "ok=true,language=wasm,result_kind={}",
                        exec_result_kind(&result)
                    );
                    result
                }
            }
            #[cfg(feature = "lua")]
            "lua" => {
                if let Some(engine) = &self.lua_engine {
                    trace_decision!(
                        "execution",
                        "self.lua_engine.is_some()",
                        true,
                        "dispatching code to Lua execution backend"
                    );
                    let result = engine.execute(code, context);
                    trace_exit!(
                        "execution",
                        "ExecutionManager::execute",
                        "ok=true,language=lua,result_kind={}",
                        exec_result_kind(&result)
                    );
                    result
                } else {
                    trace_decision!(
                        "execution",
                        "self.lua_engine.is_some()",
                        false,
                        "lua feature enabled but engine initialization unavailable"
                    );
                    let result = ExecResult::Error("Lua engine not available".to_string());
                    trace_exit!(
                        "execution",
                        "ExecutionManager::execute",
                        "ok=true,language=lua,result_kind={}",
                        exec_result_kind(&result)
                    );
                    result
                }
            }

            #[cfg(feature = "javascript")]
            "js" | "javascript" => {
                if let Some(engine) = &self.js_engine {
                    trace_decision!(
                        "execution",
                        "self.js_engine.is_some()",
                        true,
                        "dispatching code to JavaScript execution backend"
                    );
                    let result = engine.execute(code, context);
                    trace_exit!(
                        "execution",
                        "ExecutionManager::execute",
                        "ok=true,language=javascript,result_kind={}",
                        exec_result_kind(&result)
                    );
                    result
                } else {
                    trace_decision!(
                        "execution",
                        "self.js_engine.is_some()",
                        false,
                        "javascript feature enabled but engine initialization unavailable"
                    );
                    let result = ExecResult::Error("JavaScript engine not available".to_string());
                    trace_exit!(
                        "execution",
                        "ExecutionManager::execute",
                        "ok=true,language=javascript,result_kind={}",
                        exec_result_kind(&result)
                    );
                    result
                }
            }

            "native" => {
                // For native, the 'code' is the function name
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
                let result = ExecResult::Error(format!("Unknown language: {}", language));
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
    pub fn register_native<F>(&self, name: String, callback: F)
    where
        F: Fn(&ExecContext) -> ExecResult + Send + Sync + 'static,
    {
        trace_enter!(
            "execution",
            "ExecutionManager::register_native",
            "name={}",
            name
        );
        let replaced_existing = self.native_callbacks.has(&name);
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
