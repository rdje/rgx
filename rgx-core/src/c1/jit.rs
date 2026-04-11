//! Cranelift JIT host wrapper.
//!
//! [`JitHost`] is the RGX-side wrapper around `cranelift_jit::JITModule`.
//! It owns the compiled code's memory mapping and the function-pointer
//! lookup table, and exposes a typed `call_fn` helper for invoking
//! compiled functions through their generated entry points.
//!
//! The wrapper is **deliberately small** at C1 step 1: it knows how to
//! create a fresh JIT module for the host target, register external
//! runtime helper symbols (currently none), define a single function
//! from a pre-built `cranelift_codegen::ir::Function`, finalise the
//! definitions, and hand back a typed function pointer. Building the IR
//! itself (the `Function` value) is the caller's responsibility — that
//! lets the smoke test in this module live alongside the wrapper while
//! the real opcode lowering lands in step 3 in `c1/codegen.rs`.
//!
//! # Lifetime invariant
//!
//! The `extern "C" fn` pointers returned by [`JitHost::get_finalized_fn`]
//! are only valid for the lifetime of the `JitHost` that produced them.
//! Dropping the host unmaps the underlying executable memory and any
//! still-held function pointers become dangling. The smoke test
//! enforces this by holding the host across the entire call sequence.
//!
//! # Why a thin wrapper
//!
//! Cranelift's API is stable but verbose: every JIT user has to set up
//! a `JITBuilder`, configure the target ISA, register host symbols,
//! create a `JITModule`, build IR, declare functions, define functions,
//! call `finalize_definitions`, and finally retrieve the function
//! pointer. The wrapper centralises that boilerplate so the rest of
//! the C1 modules (and the differential test harness in step 4) can
//! work with one stable RGX type instead of importing six Cranelift
//! types directly.
//!
//! See `docs/C1_JIT_COMPILATION_DESIGN.md` §4.2 (code-generator
//! choice), §7 (runtime helper layer), and §10 (module layout) for
//! the design context.

use cranelift_codegen::ir::{types, AbiParam, FuncRef, Function, Signature};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module, ModuleError};
use std::fmt;

/// Errors that can be produced by the C1 JIT host.
///
/// Every variant carries enough context to make the failure debuggable
/// without needing the original Cranelift types — the wrapper exists so
/// the rest of RGX never imports `cranelift_module::ModuleError` or
/// `cranelift_codegen::CodegenError` directly.
#[derive(Debug)]
pub enum JitHostError {
    /// The host architecture isn't supported by this Cranelift build.
    /// Returned when `cranelift_native::builder()` fails — typically on
    /// 32-bit targets or architectures Cranelift doesn't have an ISA
    /// backend for.
    HostNotSupported(String),
    /// Cranelift target-ISA configuration produced an error. Carries
    /// the underlying setting error formatted to a string.
    IsaSettingsError(String),
    /// Cranelift refused to build the target ISA. Carries the
    /// underlying lookup error formatted to a string.
    IsaBuildError(String),
    /// `cranelift_module::Module` returned an error from `declare_function`,
    /// `define_function`, or `finalize_definitions`.
    ModuleError(String),
    /// The smoke test (or any other caller) asked for a function ID
    /// that wasn't defined on this host before `finalize_definitions`
    /// was called.
    FunctionNotDefined(FuncId),
    /// The codegen layer refused to JIT-compile the program because
    /// it contains an opcode (or opcode shape) the current C1 step
    /// hasn't implemented yet. Carries a human-readable description
    /// of the unsupported construct. Caller should fall back to the
    /// interpreter for this pattern.
    CodegenUnsupported(String),
}

impl fmt::Display for JitHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostNotSupported(msg) => write!(f, "C1 JIT host not supported: {msg}"),
            Self::IsaSettingsError(msg) => write!(f, "C1 JIT ISA settings error: {msg}"),
            Self::IsaBuildError(msg) => write!(f, "C1 JIT ISA build error: {msg}"),
            Self::ModuleError(msg) => write!(f, "C1 JIT module error: {msg}"),
            Self::FunctionNotDefined(id) => write!(f, "C1 JIT function {id:?} not defined"),
            Self::CodegenUnsupported(msg) => write!(f, "C1 JIT codegen unsupported: {msg}"),
        }
    }
}

impl std::error::Error for JitHostError {}

impl From<ModuleError> for JitHostError {
    fn from(err: ModuleError) -> Self {
        // Use Debug formatting so verifier errors and other
        // multi-line ModuleError variants surface their full
        // detail. The cranelift `Verifier errors` variant only
        // shows the leading message under Display, which makes
        // codegen bugs hard to track down.
        Self::ModuleError(format!("{err:?}"))
    }
}

/// The RGX-side wrapper around `cranelift_jit::JITModule`.
///
/// Owns the compiled code's executable memory mapping and the function
/// declarations table. Function pointers obtained from
/// [`Self::get_finalized_fn`] are valid only for the lifetime of this
/// host — dropping the host unmaps the memory.
///
/// The wrapper is intentionally minimal at C1 step 1: it builds a fresh
/// module, lets the caller hand it a complete `Function` IR value,
/// declares + defines the function in the module, finalises the
/// definitions, and exposes a typed accessor for the resulting function
/// pointer. Step 3 will add a higher-level `compile_program` API once
/// the codegen layer (`c1/codegen.rs`) is in place.
pub struct JitHost {
    module: JITModule,
    /// Monotonic counter used to generate unique function names so
    /// multiple programs can be compiled into the same `JitHost`
    /// without colliding. Each call to [`Self::next_func_index`]
    /// returns the next value and increments the counter.
    next_func_index: u32,
}

impl fmt::Debug for JitHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JitHost").finish_non_exhaustive()
    }
}

impl JitHost {
    /// Create a fresh JIT host targeting the **current process's**
    /// architecture and OS. Uses `cranelift_native::builder()` to
    /// detect the target features and construct a Cranelift ISA, then
    /// wraps a fresh `JITModule` around it.
    ///
    /// The host is empty — it has no functions defined yet. Callers
    /// declare and define functions via [`Self::declare_function`] and
    /// [`Self::define_function`], then finalise via
    /// [`Self::finalize_definitions`] before retrieving function pointers.
    ///
    /// # Errors
    /// Returns [`JitHostError::HostNotSupported`] if Cranelift doesn't
    /// have an ISA backend for the current target.
    pub fn new() -> Result<Self, JitHostError> {
        let mut flag_builder = settings::builder();
        // PIC is intentionally disabled. JIT'd code lives in a single
        // executable mmap region owned by the `JITModule`; nothing in
        // it is dynamically linked, so position independence buys us
        // nothing here. Cranelift 0.101's `JITModule::new` sets
        // `is_pic = false` itself when building for non-x86_64 hosts
        // anyway (the PLT machinery PIC requires is x86_64-only at
        // this version), and forcing PIC on aarch64 panics with
        // "PLT is currently only supported on x86_64". Leaving the
        // setting at Cranelift's default (`false`) is portable and
        // produces tighter code on every host. See design doc §4.2.
        //
        // Speed-tuned default optimization level. The JIT compile cost
        // is amortized over many input bytes, so we can afford the
        // optimizer pass.
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| JitHostError::IsaSettingsError(e.to_string()))?;

        let isa_builder = cranelift_native::builder()
            .map_err(|msg| JitHostError::HostNotSupported(msg.to_string()))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| JitHostError::IsaBuildError(e.to_string()))?;

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Register the runtime helper symbols so JIT'd code can call
        // them via indirect calls. The names here MUST match the
        // `#[no_mangle] extern "C"` symbols in `c1/runtime.rs` AND
        // the names declared via `Module::declare_function` in the
        // codegen layer (`c1/codegen.rs`). Adding a new helper means
        // adding a `builder.symbol(...)` line here AND a matching
        // `Module::declare_function` call in codegen.
        //
        // The address cast is sound because each helper is declared
        // `#[no_mangle] pub unsafe extern "C" fn` so it has a stable
        // C ABI and a stable address. See design doc §7.
        builder.symbol(
            "rgx_runtime_word_boundary_test",
            crate::c1::runtime::rgx_runtime_word_boundary_test as *const u8,
        );

        let module = JITModule::new(builder);
        Ok(Self {
            module,
            next_func_index: 0,
        })
    }

    /// Allocate the next monotonic function index for a fresh
    /// function on this host. Used by the codegen layer to generate
    /// unique function names so multiple programs can be compiled
    /// into the same host without name collisions.
    pub fn next_func_index(&mut self) -> u32 {
        let idx = self.next_func_index;
        self.next_func_index = self.next_func_index.wrapping_add(1);
        idx
    }

    /// Build a fresh empty `Signature` using the host module's default
    /// calling convention. Callers extend this with parameters and
    /// return types before declaring a function.
    #[must_use]
    pub fn make_signature(&self) -> Signature {
        self.module.make_signature()
    }

    /// Declare a function in the JIT module. Returns the [`FuncId`]
    /// that subsequently identifies the function for definition and
    /// finalisation.
    ///
    /// # Errors
    /// Forwards any error from `cranelift_module::Module::declare_function`.
    pub fn declare_function(
        &mut self,
        name: &str,
        linkage: Linkage,
        signature: &Signature,
    ) -> Result<FuncId, JitHostError> {
        self.module
            .declare_function(name, linkage, signature)
            .map_err(JitHostError::from)
    }

    /// Define a previously-declared function with a complete IR
    /// `Function` value. The IR is sealed, optimized, and lowered to
    /// native code by Cranelift's compilation pipeline.
    ///
    /// # Errors
    /// Forwards any error from `cranelift_module::Module::define_function`.
    pub fn define_function(
        &mut self,
        func_id: FuncId,
        function: Function,
    ) -> Result<(), JitHostError> {
        let mut ctx = self.module.make_context();
        ctx.func = function;
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(JitHostError::from)?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    /// Finalise all pending function definitions, transitioning the
    /// JIT module's code memory from RW to RX (executable). After
    /// this call function pointers retrieved via
    /// [`Self::get_finalized_fn`] are safe to invoke.
    ///
    /// # Errors
    /// Forwards any error from `cranelift_module::Module::finalize_definitions`.
    pub fn finalize_definitions(&mut self) -> Result<(), JitHostError> {
        self.module
            .finalize_definitions()
            .map_err(JitHostError::from)
    }

    /// Retrieve the raw native code pointer for a previously-declared
    /// and finalised function. The pointer is only valid for the
    /// lifetime of `self` — dropping the host unmaps the executable
    /// memory and any still-held pointers become dangling.
    ///
    /// Callers transmute this pointer to a typed `extern "C" fn`
    /// signature matching the IR signature they originally declared.
    /// The transmute is safe iff the signature matches; mismatches
    /// produce undefined behaviour at call time.
    #[must_use]
    pub fn get_finalized_fn(&self, func_id: FuncId) -> *const u8 {
        self.module.get_finalized_function(func_id)
    }

    /// Import the `rgx_runtime_word_boundary_test` runtime helper
    /// into the given Cranelift `Function` so JIT'd code can call
    /// it. Returns a [`FuncRef`] usable with `builder.ins().call(...)`.
    ///
    /// The symbol must already have been registered with the
    /// `JITBuilder` (which happens in [`Self::new`]), otherwise
    /// finalisation will fail with a missing-symbol error. The
    /// import declares the function with `Linkage::Import` and the
    /// matching C ABI signature `(i64, i64, i64) -> i8`.
    ///
    /// Each `Function` needs its own import — the `FuncRef` is
    /// scoped to the function it was declared in, not the module.
    ///
    /// # Errors
    /// Forwards any error from `cranelift_module::Module::declare_function`.
    pub fn import_word_boundary_helper(
        &mut self,
        function: &mut Function,
    ) -> Result<FuncRef, JitHostError> {
        // C ABI signature: (text: *const u8, text_len: usize, pos: usize) -> bool.
        // Pointers and usize are i64 on 64-bit; bool returns as i8
        // (the low byte of the return register).
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I8));

        let func_id = self
            .module
            .declare_function("rgx_runtime_word_boundary_test", Linkage::Import, &sig)
            .map_err(JitHostError::from)?;

        let func_ref = self.module.declare_func_in_func(func_id, function);
        Ok(func_ref)
    }
}

/// A finalised JIT-compiled program: a `JitHost` paired with a
/// `FuncId` for the compiled function. This is the engine-layer
/// handle returned by `c1::codegen::compile_program` (via the C1
/// step 5 wrapper) and stored on `Engine::jit_program`.
///
/// The `JitHost` is held inside this struct so its executable
/// memory mapping (and therefore the function pointer the engine
/// dispatch path calls) stays alive for the lifetime of the
/// `JitProgram`. Dropping a `JitProgram` unmaps the executable
/// memory and invalidates any still-held function pointers — but
/// since the host is owned exclusively by this struct, and the
/// engine layer keeps the struct alive for the lifetime of the
/// `Regex`, the function pointer is always valid when the engine
/// dispatches.
///
/// # Sync / Send
///
/// `JitProgram` is `Send` (via the underlying `JitHost` being
/// `Send`) but is NOT `Sync` because `cranelift_jit::JITModule`'s
/// internal symbol table is interior-mutable. The engine layer
/// wraps it in a `Mutex` (mirroring the `c2_dfa` pattern in
/// `engine.rs`) to satisfy `Engine`'s `Sync` requirement. The
/// lock is held only briefly to retrieve the function pointer
/// via `get_finalized_fn`; the actual JIT'd-function call happens
/// after the lock is released.
pub struct JitProgram {
    /// The host that owns the executable memory mapping. Kept
    /// alive for the lifetime of `JitProgram` so the function
    /// pointer below remains valid.
    host: JitHost,
    /// The Cranelift `FuncId` of the compiled program. Used by
    /// `host.get_finalized_fn(func_id)` to retrieve the raw
    /// function pointer.
    func_id: FuncId,
}

impl JitProgram {
    /// Construct a `JitProgram` from a finalised `JitHost` and the
    /// `FuncId` of the compiled function. The host MUST have had
    /// `finalize_definitions` called before this constructor.
    #[must_use]
    pub fn new(host: JitHost, func_id: FuncId) -> Self {
        Self { host, func_id }
    }

    /// Retrieve the raw native function pointer for the compiled
    /// program. The pointer is only valid for the lifetime of the
    /// `JitProgram` — dropping the program unmaps the executable
    /// memory and invalidates the pointer.
    ///
    /// The caller transmutes this to the appropriate `extern "C"
    /// fn` signature matching what the codegen layer produced.
    /// For step 5, that signature is
    /// `unsafe extern "C" fn(text: *const u8, text_len: usize, pos: usize) -> isize`
    /// (the `Step3aJittedFn` type from `c1::codegen`).
    #[doc(hidden)]
    #[must_use]
    pub fn raw_fn_ptr(&self) -> *const u8 {
        self.host.get_finalized_fn(self.func_id)
    }
}

impl fmt::Debug for JitProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JitProgram")
            .field("func_id", &self.func_id)
            .finish_non_exhaustive()
    }
}

// SAFETY: `cranelift_jit::JITModule` contains raw `*const u8`
// pointers (the cached function-pointer lookup table), which
// makes it `!Send` by default. For RGX's use case the JIT module
// is constructed once via `compile_program_to_jit_program`
// (which builds, defines, and finalises the function in a single
// thread), then stored on `Engine` inside a `Mutex` and never
// mutated again. All subsequent use is read-only — `raw_fn_ptr`
// just looks up the cached function pointer. Read-only sharing
// of a `!Sync` type across threads via a `Mutex` is sound, and
// the `Send` impl is what makes the `Mutex<JitProgram>` `Sync`.
//
// The invariant we rely on: after `JitProgram::new` returns,
// nothing inside the contained `JitHost` is ever mutated. The
// engine layer is the sole user and never calls any of the
// mutating methods (`declare_function`, `define_function`,
// `finalize_definitions`, `import_word_boundary_helper`,
// `next_func_index`) on the held host.
unsafe impl Send for JitProgram {}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::ir::{types, AbiParam, Function, InstBuilder, UserFuncName};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    /// **Smoke test for the C1 step 1 JIT host plumbing.**
    ///
    /// Builds a tiny Cranelift function `extern "C" fn() -> i64` whose
    /// body returns the constant `42`, declares + defines + finalises
    /// it on a fresh `JitHost`, retrieves the resulting function
    /// pointer, transmutes it to the matching `extern "C"` signature,
    /// invokes it, and asserts the return value.
    ///
    /// This is the minimum end-to-end exercise of the JIT host
    /// pipeline: target ISA selection, IR construction, function
    /// declaration, function definition, finalisation, function
    /// pointer retrieval, and native invocation. If any of those steps
    /// is broken on the host platform, this test fails — which is
    /// exactly what we want at step 1, where the goal is to prove the
    /// pipeline runs end-to-end without needing real opcode lowering
    /// or any engine wiring.
    ///
    /// Per design doc §1.0 (priority order: 100% accuracy first), this
    /// test does NOT measure performance. It only proves correctness
    /// of the host pipeline. Performance work begins at step 3 once
    /// real opcode lowering lands and the differential gate is active.
    #[test]
    fn smoke_test_jit_returns_constant_42() {
        let mut host = JitHost::new().expect("JIT host construction must succeed on this target");

        // Build a signature for `extern "C" fn() -> i64`.
        let mut sig = host.make_signature();
        sig.returns.push(AbiParam::new(types::I64));

        // Declare the function in the module.
        let func_id = host
            .declare_function("smoke_42", Linkage::Local, &sig)
            .expect("function declaration must succeed");

        // Build the IR: a single basic block that returns the
        // constant 42.
        let mut function = Function::with_name_signature(UserFuncName::user(0, 0), sig.clone());
        {
            let mut fb_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut fb_ctx);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            let const_42 = builder.ins().iconst(types::I64, 42);
            builder.ins().return_(&[const_42]);
            builder.finalize();
        }

        // Define and finalise.
        host.define_function(func_id, function)
            .expect("function definition must succeed");
        host.finalize_definitions()
            .expect("finalisation must succeed");

        // Retrieve the function pointer and call it.
        let raw_ptr = host.get_finalized_fn(func_id);
        assert!(!raw_ptr.is_null(), "finalized function pointer is null");

        // SAFETY: the IR signature `() -> i64` exactly matches the
        // transmuted Rust signature `extern "C" fn() -> i64`. The
        // function pointer is alive for the lifetime of `host`, which
        // is held across this call. The function has no parameters
        // and no side effects, so calling it cannot violate any other
        // invariants.
        let func: extern "C" fn() -> i64 = unsafe { std::mem::transmute(raw_ptr) };
        let result = func();

        assert_eq!(
            result, 42,
            "JIT'd constant function returned {result} instead of 42 — \
             the C1 host pipeline is producing wrong results"
        );
    }

    /// Negative-shape sanity check: declaring a function but never
    /// defining it must NOT crash, and `get_finalized_fn` must not be
    /// called before finalisation. We test the safe path: declare,
    /// define, finalise, then retrieve. This test verifies that
    /// declaring multiple functions on the same host works.
    #[test]
    fn smoke_test_multiple_functions_on_one_host() {
        let mut host = JitHost::new().expect("JIT host construction must succeed");

        // Function 1: () -> i64 returns 1
        let mut sig1 = host.make_signature();
        sig1.returns.push(AbiParam::new(types::I64));
        let func_id_1 = host
            .declare_function("smoke_one", Linkage::Local, &sig1)
            .expect("declare 1 must succeed");
        let mut func1 = Function::with_name_signature(UserFuncName::user(0, 1), sig1.clone());
        {
            let mut fb_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut func1, &mut fb_ctx);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            let one = builder.ins().iconst(types::I64, 1);
            builder.ins().return_(&[one]);
            builder.finalize();
        }
        host.define_function(func_id_1, func1)
            .expect("define 1 must succeed");

        // Function 2: () -> i64 returns 2
        let mut sig2 = host.make_signature();
        sig2.returns.push(AbiParam::new(types::I64));
        let func_id_2 = host
            .declare_function("smoke_two", Linkage::Local, &sig2)
            .expect("declare 2 must succeed");
        let mut func2 = Function::with_name_signature(UserFuncName::user(0, 2), sig2.clone());
        {
            let mut fb_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut func2, &mut fb_ctx);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            let two = builder.ins().iconst(types::I64, 2);
            builder.ins().return_(&[two]);
            builder.finalize();
        }
        host.define_function(func_id_2, func2)
            .expect("define 2 must succeed");

        // Finalise both at once.
        host.finalize_definitions()
            .expect("finalisation must succeed");

        // SAFETY: same as above; signature is `() -> i64`.
        let f1: extern "C" fn() -> i64 =
            unsafe { std::mem::transmute(host.get_finalized_fn(func_id_1)) };
        let f2: extern "C" fn() -> i64 =
            unsafe { std::mem::transmute(host.get_finalized_fn(func_id_2)) };

        assert_eq!(f1(), 1, "first JIT'd function returned wrong value");
        assert_eq!(f2(), 2, "second JIT'd function returned wrong value");
    }
}
