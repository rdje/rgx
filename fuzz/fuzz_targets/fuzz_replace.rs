//! Fuzz target: compile a pattern and exercise the replace/split APIs.
//!
//! Goal: no panics, no UB. Replace and split must not corrupt output.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    pattern: String,
    text: String,
    replacement: String,
}

fuzz_target!(|input: FuzzInput| {
    let Ok(re) = rgx_core::Regex::compile(&input.pattern) else {
        return;
    };
    re.set_max_steps(Some(50_000));

    let _ = re.replace(&input.text, input.replacement.as_str());
    let _ = re.replace_all(&input.text, input.replacement.as_str());
    let _ = re.replacen(&input.text, 3, input.replacement.as_str());
    let _ = re.split(&input.text);
    let _ = re.splitn(&input.text, 3);
});
