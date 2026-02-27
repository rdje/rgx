//! Comprehensive logging system for rgx debugging
//!
//! This module provides debug and trace logging throughout the rgx engine.
//! Enable with RGX_DEBUG=1 or RGX_TRACE=1 environment variables.
//! Set RGX_TRACE_FILE=trace.log to route logs into a file.
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

// Global flags set from environment
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);
static INITIALIZED: AtomicBool = AtomicBool::new(false);

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

// Initialize logging on first use
pub fn init() {
    if INITIALIZED.load(Ordering::Relaxed) {
        return;
    }

    let debug = std::env::var("RGX_DEBUG").map_or(false, |v| v == "1");
    let trace = std::env::var("RGX_TRACE").map_or(false, |v| v == "1");

    DEBUG_ENABLED.store(debug || trace, Ordering::Relaxed);
    TRACE_ENABLED.store(trace, Ordering::Relaxed);

    if let Ok(path) = std::env::var("RGX_TRACE_FILE") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            if let Err(err) = set_output_file(trimmed) {
                eprintln!(
                    "[WARN] [rgx-core/src/log.rs:init] failed to open trace file '{}': {}",
                    trimmed, err
                );
            }
        }
    }

    INITIALIZED.store(true, Ordering::Relaxed);
}

#[inline(always)]
pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

#[inline(always)]
pub fn is_trace_enabled() -> bool {
    TRACE_ENABLED.load(Ordering::Relaxed)
}

/// Route tracing output to the given file path.
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
    level: &str,
    source_file: &str,
    source_line: u32,
    source_fn: &str,
    module: &str,
    msg: &str,
) {
    write_line(&format!(
        "[{level}] [{source_file}:{source_fn}:{source_line}] [{module}] {msg}"
    ));
}

/// Emit debug-level log from internal call paths.
pub fn emit_debug(source_file: &str, source_line: u32, source_fn: &str, module: &str, msg: &str) {
    init();
    if is_debug_enabled() {
        emit("DEBUG", source_file, source_line, source_fn, module, msg);
    }
}

/// Emit trace-level log from internal call paths.
pub fn emit_trace(source_file: &str, source_line: u32, source_fn: &str, module: &str, msg: &str) {
    init();
    if is_trace_enabled() {
        emit("TRACE", source_file, source_line, source_fn, module, msg);
    }
}

/// Emit externally filtered logs (e.g. from CLI scaffolding) through the same
/// output sink as debug/trace macros.
pub fn emit_external(
    level: &str,
    module: &str,
    source_file: &str,
    source_line: u32,
    source_fn: &str,
    msg: &str,
) {
    init();
    emit(level, source_file, source_line, source_fn, module, msg);
}

/// Debug logging macro - shows important operations
#[macro_export]
macro_rules! debug_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_debug(file!(), line!(), module_path!(), $module, &format!($($arg)*));
        }
    };
}

/// Trace logging macro - shows EVERYTHING
#[macro_export]
macro_rules! trace_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::emit_trace(file!(), line!(), module_path!(), $module, &format!($($arg)*));
        }
    };
}

/// Log a value and return it (useful for debugging intermediate values)
#[macro_export]
macro_rules! debug_val {
    ($module:expr, $name:expr, $val:expr) => {{
        $crate::debug_log!($module, "{} = {:?}", $name, $val);
        $val
    }};
}

/// Hex dump for debugging bytecode
pub fn hex_dump(module: &str, label: &str, data: &[u8]) {
    init();
    if !is_debug_enabled() {
        return;
    }

    write_line(&format!("[DEBUG] [{module}] {label} ({} bytes):", data.len()));

    for (i, chunk) in data.chunks(16).enumerate() {
        let hex: String = chunk.iter().map(|b| format!("{:02x} ", b)).collect();

        let ascii: String = chunk
            .iter()
            .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
            .collect();

        write_line(&format!("  {:04x}: {:48} |{}|", i * 16, hex, ascii));
    }
}
