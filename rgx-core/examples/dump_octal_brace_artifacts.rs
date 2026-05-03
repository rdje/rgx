//! Dump artifacts for PGEN-RGX-0079: contract + parse outcome + AST dump
//! for `\o{1239}` (non-octal digit triggers fall-back to `\o`+`{1239}`).
//! Mirrors the bundle 0006 carried for the valid-octal case.
//!
//! Run: `cargo run --release -p rgx-core --example dump_octal_brace_artifacts`

use std::fs;
use std::path::PathBuf;

fn main() {
    let outdir = PathBuf::from("pgen-issues/artifacts/PGEN-RGX-0079");
    fs::create_dir_all(&outdir).expect("create outdir");

    let contract = pgen::embedding_api::parser_embedding_api_contract();
    let contract_json = serde_json::to_string_pretty(&contract).expect("serialize contract");
    fs::write(outdir.join("pgen_contract.json"), &contract_json).expect("write contract");

    let pat = r"\o{1239}";
    let outcome = pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pat);
    let outcome_json = serde_json::to_string_pretty(&outcome).expect("outcome");
    fs::write(outdir.join("pgen_parse_outcome.json"), &outcome_json).expect("outcome write");

    let dump = pgen::embedding_api::parse_grammar_profile_ast_dump_named(
        "regex",
        "regex_default",
        pat,
        &pgen::embedding_api::AstDumpOptions {
            pretty: true,
            max_ast_bytes: None,
        },
    );
    let dump_json = serde_json::to_string_pretty(&dump).expect("dump");
    fs::write(outdir.join("pgen_embedding_ast_dump.json"), &dump_json).expect("dump write");

    println!("wrote bundle to {}", outdir.display());
}
