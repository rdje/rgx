//! Comprehensive logging system for rgx debugging
//!
//! This module provides debug and trace logging throughout the rgx engine.
//! Backward-compatible flags:
//! - `RGX_DEBUG=1`
//! - `RGX_TRACE=1`
//!
//! Preferred control:
//! - `RGX_VERBOSITY=none|low|medium|high|debug`
//! - `RGX_TRACE_FILE=trace.log` to route logs into a file.
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// UVM-style logging verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Verbosity {
    /// Disable all log emission.
    None = 0,
    /// Coarse milestones and top-level flow.
    Low = 1,
    /// Important decisions and branch summaries.
    Medium = 2,
    /// Detailed operation flow.
    High = 3,
    /// Exhaustive trace detail.
    Debug = 4,
}

impl Verbosity {
    /// Parse verbosity from env/CLI text.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "0" => Some(Self::None),
            "low" | "1" => Some(Self::Low),
            "medium" | "med" | "2" => Some(Self::Medium),
            "high" | "3" => Some(Self::High),
            "debug" | "trace" | "4" => Some(Self::Debug),
            _ => None,
        }
    }

    /// Canonical lower-case text form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Debug => "debug",
        }
    }

    fn emoji(self) -> &'static str {
        match self {
            Self::None => "🔇",
            Self::Low => "🧭",
            Self::Medium => "⚖️",
            Self::High => "🛠️",
            Self::Debug => "🔬",
        }
    }
}

// Backward-compatible global flags.
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static VERBOSITY_LEVEL: AtomicU8 = AtomicU8::new(Verbosity::None as u8);

enum OutputTarget {
    Stderr,
    File(File),
}

struct OutputState {
    target: OutputTarget,
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            target: OutputTarget::Stderr,
        }
    }
}

static OUTPUT_STATE: OnceLock<Mutex<OutputState>> = OnceLock::new();

fn output_state() -> &'static Mutex<OutputState> {
    OUTPUT_STATE.get_or_init(|| Mutex::new(OutputState::default()))
}

fn set_verbosity_inner(level: Verbosity) {
    VERBOSITY_LEVEL.store(level as u8, Ordering::Relaxed);
    DEBUG_ENABLED.store(level >= Verbosity::High, Ordering::Relaxed);
    TRACE_ENABLED.store(level >= Verbosity::Debug, Ordering::Relaxed);
}

fn resolve_env_verbosity(debug: bool, trace: bool) -> Verbosity {
    match std::env::var("RGX_VERBOSITY") {
        Ok(raw) => Verbosity::parse(&raw).unwrap_or_else(|| {
            eprintln!(
                "[WARN] [rgx-core/src/log.rs:init] invalid RGX_VERBOSITY='{raw}'; expected none|low|medium|high|debug"
            );
            if trace {
                Verbosity::Debug
            } else if debug {
                Verbosity::High
            } else {
                Verbosity::None
            }
        }),
        Err(_) => {
            if trace {
                Verbosity::Debug
            } else if debug {
                Verbosity::High
            } else {
                Verbosity::None
            }
        }
    }
}

/// Initialize logging on first use, reading configuration from environment variables.
pub fn init() {
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }

    let debug = std::env::var("RGX_DEBUG").is_ok_and(|v| v == "1");
    let trace = std::env::var("RGX_TRACE").is_ok_and(|v| v == "1");
    let verbosity = resolve_env_verbosity(debug, trace);
    set_verbosity_inner(verbosity);

    if let Ok(path) = std::env::var("RGX_TRACE_FILE") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            if let Err(err) = set_output_file(trimmed) {
                eprintln!(
                    "[WARN] [rgx-core/src/log.rs:init] failed to open trace file '{trimmed}': {err}"
                );
            }
        }
    }
}

/// Return whether debug-level logging is active.
#[allow(clippy::inline_always)] // hot logging check, intentionally inlined
#[inline(always)]
pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Return whether trace-level logging is active.
#[allow(clippy::inline_always)] // hot logging check, intentionally inlined
#[inline(always)]
pub fn is_trace_enabled() -> bool {
    TRACE_ENABLED.load(Ordering::Relaxed)
}

/// Return current global verbosity.
#[allow(clippy::inline_always)] // hot logging check, intentionally inlined
#[inline(always)]
pub fn current_verbosity() -> Verbosity {
    match VERBOSITY_LEVEL.load(Ordering::Relaxed) {
        1 => Verbosity::Low,
        2 => Verbosity::Medium,
        3 => Verbosity::High,
        4 => Verbosity::Debug,
        _ => Verbosity::None,
    }
}

/// Programmatically set verbosity at runtime.
pub fn set_verbosity(level: Verbosity) {
    init();
    set_verbosity_inner(level);
}

/// Check if a minimum verbosity level is enabled.
#[allow(clippy::inline_always)] // hot logging check, intentionally inlined
#[inline(always)]
#[must_use]
pub fn is_verbosity_enabled(min_level: Verbosity) -> bool {
    min_level != Verbosity::None && current_verbosity() >= min_level
}

/// Route logging output to the given file path.
///
/// # Errors
/// Returns an `io::Error` if the file at the given path cannot be created or opened.
///
/// # Panics
/// Panics if the internal output-state mutex is poisoned.
pub fn set_output_file(path: &str) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut output = output_state().lock().expect("output mutex poisoned");
    output.target = OutputTarget::File(file);
    Ok(())
}

fn write_line(line: &str) {
    let mut output = output_state().lock().expect("output mutex poisoned");
    match &mut output.target {
        OutputTarget::Stderr => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "{line}");
        }
        OutputTarget::File(file) => {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

fn emit(
    min_verbosity: Verbosity,
    level: &str,
    source_file: &str,
    source_line: u32,
    source_fn: &str,
    module: &str,
    msg: &str,
) {
    if !is_verbosity_enabled(min_verbosity) {
        return;
    }
    write_line(&format!(
        "[{level}] [{}] [{source_file}:{source_fn}:{source_line}] [{module}] {msg}",
        min_verbosity.emoji()
    ));
}

/// Emit low-level log from internal call paths.
pub fn emit_low(source_file: &str, source_line: u32, source_fn: &str, module: &str, msg: &str) {
    init();
    emit(
        Verbosity::Low,
        "LOW",
        source_file,
        source_line,
        source_fn,
        module,
        msg,
    );
}

/// Emit medium-level log from internal call paths.
pub fn emit_medium(source_file: &str, source_line: u32, source_fn: &str, module: &str, msg: &str) {
    init();
    emit(
        Verbosity::Medium,
        "MEDIUM",
        source_file,
        source_line,
        source_fn,
        module,
        msg,
    );
}

/// Emit high-level log from internal call paths.
pub fn emit_high(source_file: &str, source_line: u32, source_fn: &str, module: &str, msg: &str) {
    init();
    emit(
        Verbosity::High,
        "HIGH",
        source_file,
        source_line,
        source_fn,
        module,
        msg,
    );
}

/// Emit function-entry trace with optional argument snapshot.
pub fn emit_function_enter(
    source_file: &str,
    source_line: u32,
    source_fn: &str,
    module: &str,
    function_name: &str,
    args: &str,
) {
    let suffix = if args.is_empty() {
        String::new()
    } else {
        format!(" | args: {args}")
    };
    emit_high(
        source_file,
        source_line,
        source_fn,
        module,
        &format!("📥 ENTER {function_name}{suffix}"),
    );
}

/// Emit function-exit trace with optional result snapshot.
pub fn emit_function_exit(
    source_file: &str,
    source_line: u32,
    source_fn: &str,
    module: &str,
    function_name: &str,
    result: &str,
) {
    let suffix = if result.is_empty() {
        String::new()
    } else {
        format!(" | result: {result}")
    };
    emit_high(
        source_file,
        source_line,
        source_fn,
        module,
        &format!("📤 EXIT {function_name}{suffix}"),
    );
}

/// Emit decision trace with branch reason.
pub fn emit_decision(
    source_file: &str,
    source_line: u32,
    source_fn: &str,
    module: &str,
    decision: &str,
    taken: bool,
    reason: &str,
) {
    let branch = if taken { "taken" } else { "not taken" };
    emit_medium(
        source_file,
        source_line,
        source_fn,
        module,
        &format!("🧠 DECISION {decision} -> {branch} | reason: {reason}"),
    );
}

/// Emit debug-level log from internal call paths (mapped to high verbosity).
pub fn emit_debug(source_file: &str, source_line: u32, source_fn: &str, module: &str, msg: &str) {
    emit_high(source_file, source_line, source_fn, module, msg);
}

/// Emit trace-level log from internal call paths (mapped to debug verbosity).
pub fn emit_trace(source_file: &str, source_line: u32, source_fn: &str, module: &str, msg: &str) {
    init();
    emit(
        Verbosity::Debug,
        "TRACE",
        source_file,
        source_line,
        source_fn,
        module,
        msg,
    );
}

/// Emit externally filtered logs (e.g. from CLI scaffolding) through the same
/// output sink as internal logging.
pub fn emit_external(
    level: &str,
    module: &str,
    source_file: &str,
    source_line: u32,
    source_fn: &str,
    msg: &str,
) {
    init();
    write_line(&format!(
        "[{level}] [{}] [{source_file}:{source_fn}:{source_line}] [{module}] {msg}",
        current_verbosity().emoji()
    ));
}

/// Emit externally with a minimum verbosity filter.
pub fn emit_external_at(
    min_verbosity: Verbosity,
    level: &str,
    module: &str,
    source_file: &str,
    source_line: u32,
    source_fn: &str,
    msg: &str,
) {
    init();
    emit(
        min_verbosity,
        level,
        source_file,
        source_line,
        source_fn,
        module,
        msg,
    );
}

// ---------------------------------------------------------------------------
// Feature-gated logging macros
//
// When the `trace` feature is enabled, the macros forward to the emit_*
// functions.  Without it they expand to nothing, giving the compiler the
// opportunity to eliminate all format-string construction from hot loops.
// ---------------------------------------------------------------------------

/// Debug logging macro - detailed operational flow.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! debug_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_debug(file!(), line!(), module_path!(), $module, &format!($($arg)*));
        }
    };
}

/// Debug logging macro - zero-cost no-op when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! debug_log {
    ($module:expr, $($arg:tt)*) => {{}};
}

/// Trace logging macro - exhaustive details.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! trace_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_trace(file!(), line!(), module_path!(), $module, &format!($($arg)*));
        }
    };
}

/// Trace logging macro - zero-cost no-op when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! trace_log {
    ($module:expr, $($arg:tt)*) => {{}};
}

/// Low verbosity macro - coarse milestones.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! low_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_low(file!(), line!(), module_path!(), $module, &format!($($arg)*));
        }
    };
}

/// Low verbosity macro - zero-cost no-op when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! low_log {
    ($module:expr, $($arg:tt)*) => {{}};
}

/// Medium verbosity macro - decisions and branch summaries.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! medium_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_medium(file!(), line!(), module_path!(), $module, &format!($($arg)*));
        }
    };
}

/// Medium verbosity macro - zero-cost no-op when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! medium_log {
    ($module:expr, $($arg:tt)*) => {{}};
}

/// High verbosity macro - detailed flow.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! high_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_high(file!(), line!(), module_path!(), $module, &format!($($arg)*));
        }
    };
}

/// High verbosity macro - zero-cost no-op when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! high_log {
    ($module:expr, $($arg:tt)*) => {{}};
}

/// Function entry tracing helper.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! trace_enter {
    ($module:expr, $function_name:expr) => {
        {
            $crate::log::emit_function_enter(file!(), line!(), module_path!(), $module, $function_name, "");
        }
    };
    ($module:expr, $function_name:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_function_enter(file!(), line!(), module_path!(), $module, $function_name, &format!($($arg)*));
        }
    };
}

/// Function entry tracing helper - zero-cost no-op when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! trace_enter {
    ($module:expr, $function_name:expr) => {{}};
    ($module:expr, $function_name:expr, $($arg:tt)*) => {{}};
}

/// Function exit tracing helper.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! trace_exit {
    ($module:expr, $function_name:expr) => {
        {
            $crate::log::emit_function_exit(file!(), line!(), module_path!(), $module, $function_name, "");
        }
    };
    ($module:expr, $function_name:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_function_exit(file!(), line!(), module_path!(), $module, $function_name, &format!($($arg)*));
        }
    };
}

/// Function exit tracing helper - zero-cost no-op when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! trace_exit {
    ($module:expr, $function_name:expr) => {{}};
    ($module:expr, $function_name:expr, $($arg:tt)*) => {{}};
}

/// Decision tracing helper with rationale.
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! trace_decision {
    ($module:expr, $decision:expr, $taken:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_decision(
                file!(),
                line!(),
                module_path!(),
                $module,
                $decision,
                $taken,
                &format!($($arg)*),
            );
        }
    };
}

/// Decision tracing helper - zero-cost no-op when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! trace_decision {
    ($module:expr, $decision:expr, $taken:expr, $($arg:tt)*) => {{}};
}

/// Log a value and return it (useful for debugging intermediate values).
#[cfg(feature = "trace")]
#[macro_export]
macro_rules! debug_val {
    ($module:expr, $name:expr, $val:expr) => {{
        $crate::debug_log!($module, "{} = {:?}", $name, $val);
        $val
    }};
}

/// Log a value and return it - pass-through when `trace` feature is disabled.
#[cfg(not(feature = "trace"))]
#[macro_export]
macro_rules! debug_val {
    ($module:expr, $name:expr, $val:expr) => {{
        $val
    }};
}

/// Hex dump for debugging bytecode.
pub fn hex_dump(module: &str, label: &str, data: &[u8]) {
    init();
    if !is_verbosity_enabled(Verbosity::High) {
        return;
    }

    write_line(&format!(
        "[HIGH] [{}] [{module}] {label} ({} bytes):",
        Verbosity::High.emoji(),
        data.len()
    ));

    for (i, chunk) in data.chunks(16).enumerate() {
        let hex = chunk.iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write;
            write!(s, "{b:02x} ").unwrap();
            s
        });

        let ascii: String = chunk
            .iter()
            .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
            .collect();

        write_line(&format!("  {:04x}: {:48} |{}|", i * 16, hex, ascii));
    }
}

#[cfg(test)]
mod tests {
    use super::Verbosity;

    #[test]
    fn verbosity_parser_accepts_uvm_style_values() {
        assert_eq!(Verbosity::parse("none"), Some(Verbosity::None));
        assert_eq!(Verbosity::parse("low"), Some(Verbosity::Low));
        assert_eq!(Verbosity::parse("medium"), Some(Verbosity::Medium));
        assert_eq!(Verbosity::parse("high"), Some(Verbosity::High));
        assert_eq!(Verbosity::parse("debug"), Some(Verbosity::Debug));
        assert_eq!(Verbosity::parse("trace"), Some(Verbosity::Debug));
        assert_eq!(Verbosity::parse("2"), Some(Verbosity::Medium));
        assert_eq!(Verbosity::parse("unknown"), None);
    }
}
