//! One-shot dump of the `parser_embedding_api_contract()` JSON,
//! `parse_grammar_profile_named` outcomes, and AST-dump JSONs for the
//! family of POSIX-class reproducers attached to PGEN-RGX-0076.
//!
//! Run from the repo root:
//!
//! ```text
//! cargo run --release -p rgx-core --example pgen_posix_class_dump
//! ```
//!
//! Outputs land in `pgen-issues/artifacts/PGEN-RGX-0076/`. The contract
//! is dumped once (it's pattern-independent); per-reproducer parse
//! outcomes and pretty AST dumps are written under `pgen_parse_outcomes/`
//! and `pgen_ast_dumps/`.

use std::fs;
use std::path::PathBuf;

const REPRO: &[(&str, &str)] = &[
    // Single class — name-loss visible directly.
    ("simple_alpha", "[[:alpha:]]"),
    ("simple_digit", "[[:digit:]]"),
    ("simple_word", "[[:word:]]"),
    ("simple_space", "[[:space:]]"),
    ("simple_punct", "[[:punct:]]"),
    ("simple_upper", "[[:upper:]]"),
    ("simple_lower", "[[:lower:]]"),
    ("simple_xdigit", "[[:xdigit:]]"),
    ("simple_graph", "[[:graph:]]"),
    ("simple_print", "[[:print:]]"),
    ("simple_alnum", "[[:alnum:]]"),
    ("simple_blank", "[[:blank:]]"),
    ("simple_cntrl", "[[:cntrl:]]"),
    ("simple_ascii", "[[:ascii:]]"),
    // Negated POSIX classes — name AND negation both lost.
    ("negated_alpha", "[[:^alpha:]]"),
    ("negated_digit", "[[:^digit:]]"),
    // Class with multiple POSIX terms — proves the typed shape can't
    // distinguish `alpha` from `digit` since both reduce to `"[:"`.
    ("alpha_and_digit", "[[:alpha:][:digit:]]"),
    // POSIX class composed with literal characters and ranges.
    ("alpha_with_dash", "[[:alpha:]-]"),
    ("alpha_with_range", "[[:alpha:]a-z]"),
    // POSIX class inside `(*UCP)` pragma — drives the RGX-side test
    // `tests::ucp_pragma_unicodefies_posix_classes` (regression pin
    // for PCRE2 widening behaviour).
    ("ucp_alpha", "(*UCP)^[[:alpha:]]+"),
    // POSIX class inside `(?[...])` extended class — independent
    // re-walk path that hits the same posix_class shape.
    ("extended_class_alpha", "(?[ [:alpha:] ])"),
];

fn main() {
    let outdir = PathBuf::from("pgen-issues/artifacts/PGEN-RGX-0076");
    fs::create_dir_all(&outdir).expect("create outdir");

    let contract = pgen::embedding_api::parser_embedding_api_contract();
    let contract_json = serde_json::to_string_pretty(&contract).expect("serialize contract");
    fs::write(outdir.join("pgen_contract.json"), &contract_json).expect("write contract");
    println!("wrote pgen_contract.json ({} bytes)", contract_json.len());

    let outcomes_dir = outdir.join("pgen_parse_outcomes");
    fs::create_dir_all(&outcomes_dir).expect("create outcomes_dir");

    let dumps_dir = outdir.join("pgen_ast_dumps");
    fs::create_dir_all(&dumps_dir).expect("create dumps_dir");

    let inputs_dir = outdir.join("pgen_inputs");
    fs::create_dir_all(&inputs_dir).expect("create inputs_dir");

    for (name, pattern) in REPRO {
        // Persist the exact input as a real file (per protocol §3 — no
        // paraphrasing or re-encoding).
        fs::write(inputs_dir.join(format!("{name}.txt")), pattern).expect("write input");

        let outcome =
            pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern);
        let json = serde_json::to_string_pretty(&outcome).expect("serialize outcome");
        let path = outcomes_dir.join(format!("{name}.json"));
        fs::write(&path, &json).expect("write outcome");

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
            "wrote {name}: outcome ({} bytes), ast_dump ({} bytes)",
            json.len(),
            dump_json.len()
        );
    }
}
