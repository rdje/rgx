//! One-shot dump of the `parser_embedding_api_contract()` JSON, the
//! `parse_grammar_profile_named` outcome JSON, and the AST-dump JSON
//! for the multi-piece-concatenation reproducer used in PGEN-RGX-0075.
//!
//! Run from the repo root:
//!
//! ```text
//! cargo run --release -p rgx-core --example pgen_concat_dump
//! ```
//!
//! Outputs are written into `pgen-issues/artifacts/PGEN-RGX-0075/`.

use std::fs;
use std::path::PathBuf;

const REPRO: &[(&str, &str)] = &[
    ("two_chars", "ab"),
    ("three_chars", "abc"),
    ("foo_k_bar", "foo\\Kbar"),
];

fn main() {
    let outdir = PathBuf::from("pgen-issues/artifacts/PGEN-RGX-0075");
    fs::create_dir_all(&outdir).expect("create outdir");

    let contract = pgen::embedding_api::parser_embedding_api_contract();
    let contract_json = serde_json::to_string_pretty(&contract).expect("serialize contract");
    fs::write(outdir.join("pgen_contract.json"), &contract_json).expect("write contract");
    println!("wrote pgen_contract.json ({} bytes)", contract_json.len());

    let outcomes_dir = outdir.join("pgen_parse_outcomes");
    fs::create_dir_all(&outcomes_dir).expect("create outcomes_dir");

    let dumps_dir = outdir.join("pgen_ast_dumps");
    fs::create_dir_all(&dumps_dir).expect("create dumps_dir");

    for (name, pattern) in REPRO {
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

        let dump_outcome = pgen::embedding_api::parse_grammar_profile_ast_dump_named(
            "regex",
            "regex_default",
            pattern,
            &pgen::embedding_api::AstDumpOptions {
                pretty: true,
                max_ast_bytes: None,
            },
        );
        let dump_json = serde_json::to_string_pretty(&dump_outcome).expect("serialize dump");
        let dump_path = dumps_dir.join(format!("{name}.json"));
        fs::write(&dump_path, &dump_json).expect("write dump");
        println!(
            "wrote pgen_ast_dumps/{}.json ({} bytes)",
            name,
            dump_json.len()
        );
    }
}
