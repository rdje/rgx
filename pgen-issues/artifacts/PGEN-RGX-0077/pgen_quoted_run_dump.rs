//! One-shot dump of the `parser_embedding_api_contract()` JSON,
//! `parse_grammar_profile_named` outcomes, and AST-dump JSONs for the
//! family of `\Q...\E quantifier?` reproducers attached to PGEN-RGX-0077.
//!
//! Run from the repo root:
//!
//! ```text
//! cargo run --release -p rgx-core --example pgen_quoted_run_dump
//! ```
//!
//! Outputs land in `pgen-issues/artifacts/PGEN-RGX-0077/`. The contract
//! is dumped once (it's pattern-independent); per-reproducer parse
//! outcomes and pretty AST dumps are written under `pgen_parse_outcomes/`
//! and `pgen_ast_dumps/`. The exact reproducer source text is also
//! persisted under `pgen_inputs/` so the bundle is self-contained per
//! protocol §3.

use std::fs;
use std::path::PathBuf;

const REPRO: &[(&str, &str)] = &[
    // The PGEN-RGX-0074 canonical reproducer. Should produce 3 flat
    // pieces per the 0074 contract description; empirically produces
    // a wrap-1-extra shape per PGEN-RGX-0077.
    ("Qab_star_E_2_more", r"\Qab*\E{2,}"),
    // Lazy and possessive variants on the same canonical pattern.
    ("Qab_star_E_2_more_lazy", r"\Qab*\E{2,}?"),
    // Other quantifier shapes on quoted runs.
    ("Qabc_E_question", r"\Qabc\E?"),
    ("Qab_E_3", r"\Qab\E{3}"),
    ("Qabc_E_2", r"\Qabc\E{2}"),
    ("Qabcdef_E_plus", r"\Qabcdef\E+"),
    ("Qabcdef_E_star", r"\Qabcdef\E*"),
    ("Qab_E_n_m", r"\Qab\E{1,3}"),
    ("Qab_E_n_m_lazy", r"\Qab\E{1,3}?"),
    ("Qab_E_n_m_possessive", r"\Qab\E{1,3}+"),
    // Degenerate cases that should fall through to the standard `piece`
    // branch (single-char or empty quoted run + quantifier). Per the
    // 0074 fix description these should produce ONE piece — and
    // empirically they do, which proves the bug is specific to the
    // multi-char-run path through `piece_quoted_run_quantified`.
    ("Qa_E_3_single_char", r"\Qa\E{3}"),
    ("Q_empty_E_2", r"\Q\E{2}"),
    // Quoted run inside an alternation — exercises the spread context
    // through the alternation rule.
    ("alt_with_Qab_E_q", r"x|\Qab\E?"),
    // Quoted run inside a group — exercises the spread context through
    // a nested pattern.
    ("group_with_Qab_E_q", r"(\Qab\E?)"),
    // Multi-piece concatenation with a quoted-run-with-quantifier in
    // the middle. Demonstrates that the bug fires regardless of
    // surrounding pieces.
    ("xx_Qab_E_q_yy", r"xx\Qab\E?yy"),
    // Control case — pure literal multi-piece concatenation. Should
    // produce 5 flat pieces. Used to prove `concatenation -> [$1**]`
    // works correctly when no piece's value is itself an array.
    ("control_hello", "hello"),
    // Control case — single piece (no quantifier). Should produce 1
    // piece. No quoted-run involvement.
    ("control_a", "a"),
];

fn main() {
    let outdir = PathBuf::from("pgen-issues/artifacts/PGEN-RGX-0077");
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
        // Per protocol §3 — persist the exact source text as a real
        // file (no paraphrasing, no re-encoding).
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
