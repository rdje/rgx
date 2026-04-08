//! Fuzz target: compile arbitrary byte sequences as regex patterns.
//!
//! Goal: no panics, no UB, no infinite loops. Compile errors are fine.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(pattern) = std::str::from_utf8(data) {
        // Attempt compilation — errors are expected and fine.
        let _ = rgx_core::Regex::compile(pattern);
    }
});
