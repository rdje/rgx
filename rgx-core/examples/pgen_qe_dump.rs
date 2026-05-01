//! Temporary diagnostic: dumps PGEN parse outcomes for `\Q...\E{quantifier}` patterns
//! as evidence for PGEN-RGX-0074. Delete after the issue is filed.

use pgen::embedding_api::{
    parse_grammar_profile_ast_dump_named, parse_grammar_profile_named,
    parser_embedding_api_contract, AstDumpOptions,
};

fn dump(label: &str, input: &str) {
    println!("=== {} ({:?}) ===", label, input);
    let outcome = parse_grammar_profile_named("regex", "regex_default", input);
    match serde_json::to_string_pretty(&outcome) {
        Ok(s) => println!("OUTCOME:\n{}\n", s),
        Err(e) => println!("OUTCOME serialization error: {}\n", e),
    }
    let dump = parse_grammar_profile_ast_dump_named(
        "regex",
        "regex_default",
        input,
        &AstDumpOptions {
            pretty: true,
            max_ast_bytes: None,
        },
    );
    match serde_json::to_string_pretty(&dump) {
        Ok(s) => println!("AST_DUMP:\n{}\n", s),
        Err(e) => println!("AST_DUMP serialization error: {}\n", e),
    }
}

fn main() {
    let contract = parser_embedding_api_contract();
    match serde_json::to_string_pretty(&contract) {
        Ok(s) => println!("CONTRACT:\n{}\n", s),
        Err(e) => println!("CONTRACT serialization error: {}\n", e),
    }
    dump("multi-char QE with {2,}", r"\Qab*\E{2,}");
    dump("multi-char QE with {2}", r"\Qabc\E{2}");
    dump("multi-char QE with ?", r"\Qab*\E?");
    dump("manual control ab\\*{2,}", r"ab\*{2,}");
    dump("single-char QE \\Qa\\E{3}", r"\Qa\E{3}");
}
