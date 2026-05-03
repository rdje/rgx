//! Dump PGEN-RGX-0080 artifacts: contract + parse outcome + AST dumps
//! for the 5-pattern matrix that demonstrates inconsistent whitespace
//! handling inside `{m,n}` counted quantifiers.
//!
//! Run: `cargo run --release -p rgx-core --example dump_quant_ws_artifacts`

use std::fs;
use std::path::PathBuf;

const PATTERNS: &[(&str, &str)] = &[
    // baseline — no whitespace, should parse as quantifier {1,2}
    ("baseline_no_ws", r"a{1,2}"),
    // outer whitespace only — currently parses correctly as {1,2}
    ("outer_ws_only", r"a{ 1,2 }"),
    // whitespace between digits and commas — currently misparses as
    // 10 separate literal pieces (a, {, ' ', 1, ' ', ',', ' ', 2, ' ', }).
    // PCRE2 accepts and matches `aa`.
    ("inner_ws_full", r"a{ 1 , 2 }"),
    // whitespace before comma only — also misparses.
    ("inner_ws_pre_comma", r"a{1 ,2}"),
    // whitespace after comma only — also misparses.
    ("inner_ws_post_comma", r"a{1, 2}"),
];

fn main() {
    let outdir = PathBuf::from("pgen-issues/artifacts/PGEN-RGX-0080");
    fs::create_dir_all(&outdir).expect("create outdir");

    let contract = pgen::embedding_api::parser_embedding_api_contract();
    let contract_json = serde_json::to_string_pretty(&contract).expect("contract");
    fs::write(outdir.join("pgen_contract.json"), &contract_json).expect("contract write");

    for (name, pat) in PATTERNS {
        let outcome =
            pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pat);
        let outcome_json = serde_json::to_string_pretty(&outcome).expect("outcome");
        fs::write(
            outdir.join(format!("pgen_parse_outcome_{name}.json")),
            &outcome_json,
        )
        .expect("outcome write");

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
        fs::write(
            outdir.join(format!("pgen_ast_dump_{name}.json")),
            &dump_json,
        )
        .expect("dump write");
    }

    println!(
        "wrote bundle to {} ({} patterns)",
        outdir.display(),
        PATTERNS.len()
    );
}
