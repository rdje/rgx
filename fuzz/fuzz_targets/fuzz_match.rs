//! Fuzz target: compile a pattern and match against arbitrary input.
//!
//! Goal: no panics, no UB, no infinite hangs. Uses step limits as a safety net.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    pattern: String,
    text: String,
}

fuzz_target!(|input: FuzzInput| {
    let Ok(re) = rgx_core::Regex::compile(&input.pattern) else {
        return;
    };
    // Prevent hangs on pathological patterns.
    re.set_max_steps(Some(50_000));

    let _ = re.is_match(&input.text);
    let _ = re.find_first(&input.text);
    let _ = re.find_all(&input.text);
    let _ = re.find(&input.text);
    let _ = re.captures(&input.text);
    let _ = re.shortest_match(&input.text);
});
