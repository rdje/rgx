//! One-shot dump of `parser_embedding_api_contract()` JSON and
//! `parse_grammar_profile_named` outcome JSON for the 8 patterns
//! used in PGEN-RGX-0073's perf measurement bundle.
//!
//! Run from the repo root:
//!
//! ```text
//! cargo run --release -p rgx-core --example dump_pgen_artifacts
//! ```
//!
//! Outputs are written into
//! `pgen-issues/artifacts/PGEN-RGX-0073/`. The contract is dumped
//! once (it's pattern-independent); the parse outcome is dumped
//! per pattern.
use std::fs;
use std::path::PathBuf;

const PATTERNS: &[(&str, &str)] = &[
    ("literal_simple", r"test"),
    ("digit_sequence", r"\d{3}-\d{2}-\d{4}"),
    (
        "character_class",
        r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
    ),
    ("alternation", r"cat|dog|bird"),
    ("capture_groups", r"(\d{4})-(\d{2})-(\d{2})"),
    ("url_simple", r"https?://\S+"),
    ("email_basic", r"\b\w+@\w+\.\w+\b"),
    ("anchor_complex", r"^(\d+)\s+(?P<word>\w+)\s+(?:foo|bar)$"),
];

fn main() {
    let outdir = PathBuf::from("pgen-issues/artifacts/PGEN-RGX-0073");
    fs::create_dir_all(&outdir).expect("create outdir");

    let contract = pgen::embedding_api::parser_embedding_api_contract();
    let contract_json = serde_json::to_string_pretty(&contract).expect("serialize contract");
    fs::write(outdir.join("pgen_contract.json"), &contract_json).expect("write contract");
    println!("wrote pgen_contract.json ({} bytes)", contract_json.len());

    let outcomes_dir = outdir.join("pgen_parse_outcomes");
    fs::create_dir_all(&outcomes_dir).expect("create outcomes_dir");

    for (name, pattern) in PATTERNS {
        let outcome =
            pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern);
        let json = serde_json::to_string_pretty(&outcome).expect("serialize outcome");
        let path = outcomes_dir.join(format!("{name}.json"));
        fs::write(&path, &json).expect("write outcome");
        println!(
            "wrote pgen_parse_outcomes/{}.json ({} bytes)",
            name,
            json.len()
        );
    }
}
