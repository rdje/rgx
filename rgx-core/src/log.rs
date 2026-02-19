//! Comprehensive logging system for rgx debugging
//!
//! This module provides debug and trace logging throughout the rgx engine.
//! Enable with RGX_DEBUG=1 or RGX_TRACE=1 environment variables.

use std::sync::atomic::{AtomicBool, Ordering};

// Global flags set from environment
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);

// Initialize logging on first use
pub fn init() {
    let debug = std::env::var("RGX_DEBUG").map_or(false, |v| v == "1");
    let trace = std::env::var("RGX_TRACE").map_or(false, |v| v == "1");

    DEBUG_ENABLED.store(debug || trace, Ordering::Relaxed);
    TRACE_ENABLED.store(trace, Ordering::Relaxed);
}

#[inline(always)]
pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

#[inline(always)]
pub fn is_trace_enabled() -> bool {
    TRACE_ENABLED.load(Ordering::Relaxed)
}

/// Debug logging macro - shows important operations
#[macro_export]
macro_rules! debug_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::init();
            if $crate::log::is_debug_enabled() {
                eprintln!("[DEBUG] [{}:{}] [{}] {}",
                    file!(), line!(), $module, format!($($arg)*));
            }
        }
    };
}

/// Trace logging macro - shows EVERYTHING
#[macro_export]
macro_rules! trace_log {
    ($module:expr, $($arg:tt)*) => {
        {
            $crate::log::init();
            if $crate::log::is_trace_enabled() {
                eprintln!("[TRACE] [{}:{}] [{}] {}",
                    file!(), line!(), $module, format!($($arg)*));
            }
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
    if !is_debug_enabled() {
        return;
    }

    eprintln!("[DEBUG] [{}] {} ({} bytes):", module, label, data.len());

    for (i, chunk) in data.chunks(16).enumerate() {
        let hex: String = chunk.iter().map(|b| format!("{:02x} ", b)).collect();

        let ascii: String = chunk
            .iter()
            .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
            .collect();

        eprintln!("  {:04x}: {:48} |{}|", i * 16, hex, ascii);
    }
}
