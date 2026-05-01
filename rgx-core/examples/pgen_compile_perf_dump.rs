//! One-shot dump for PGEN-RGX-0078: persists the 8-pattern corpus,
//! parse outcomes, AST dumps, and live PGEN parse-time measurements
//! into `pgen-issues/artifacts/PGEN-RGX-0078/`.
//!
//! Run from the repo root:
//!
//! ```text
//! cargo run --release -p rgx-core --example pgen_compile_perf_dump
//! ```
//!
//! The companion C baseline (`pgen_iteration_flow/pcre2_compile_baseline.c`)
//! must be run separately via `cc -O2 -lpcre2-8` and its output piped
//! into `measurements/pcre2_compile_p50.txt`. See the bundle's
//! `pgen_iteration_flow/README.md` for the end-to-end recipe.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

const ITERATIONS: usize = 5000;
const WARMUP: usize = 200;

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
    let outdir = PathBuf::from("pgen-issues/artifacts/PGEN-RGX-0078");
    fs::create_dir_all(&outdir).expect("create outdir");

    let inputs_dir = outdir.join("pgen_inputs");
    let outcomes_dir = outdir.join("pgen_parse_outcomes");
    let dumps_dir = outdir.join("pgen_ast_dumps");
    let measurements_dir = outdir.join("measurements");
    fs::create_dir_all(&inputs_dir).expect("inputs dir");
    fs::create_dir_all(&outcomes_dir).expect("outcomes dir");
    fs::create_dir_all(&dumps_dir).expect("dumps dir");
    fs::create_dir_all(&measurements_dir).expect("measurements dir");

    // 1. Persist the contract metadata once.
    let contract = pgen::embedding_api::parser_embedding_api_contract();
    let contract_json = serde_json::to_string_pretty(&contract).expect("serialize contract");
    fs::write(outdir.join("pgen_contract.json"), &contract_json).expect("write contract");
    println!("wrote pgen_contract.json ({} bytes)", contract_json.len());

    // 2. Persist the patterns.tsv for downstream consumers (mirrors
    //    what's in pgen_iteration_flow/ but exposed at top-level too
    //    so the corpus is reachable without descending).
    let mut patterns_tsv = String::new();
    patterns_tsv.push_str("# name\tpattern\n");
    for (name, pat) in PATTERNS {
        patterns_tsv.push_str(name);
        patterns_tsv.push('\t');
        patterns_tsv.push_str(pat);
        patterns_tsv.push('\n');
    }
    fs::write(outdir.join("patterns.tsv"), &patterns_tsv).expect("write patterns");
    println!("wrote patterns.tsv ({} patterns)", PATTERNS.len());

    // 3. Per-pattern: persist input file (exact bytes, no paraphrasing
    //    per protocol §3), parse outcome JSON (proves the parse
    //    SUCCEEDS — important for a perf report so PGEN knows the
    //    timings aren't on a fail path), and AST dump JSON.
    for (name, pattern) in PATTERNS {
        fs::write(inputs_dir.join(format!("{name}.txt")), pattern).expect("input");

        let outcome =
            pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern);
        let outcome_json = serde_json::to_string_pretty(&outcome).expect("outcome");
        fs::write(outcomes_dir.join(format!("{name}.json")), &outcome_json).expect("outcome");

        let dump = pgen::embedding_api::parse_grammar_profile_ast_dump_named(
            "regex",
            "regex_default",
            pattern,
            &pgen::embedding_api::AstDumpOptions {
                pretty: true,
                max_ast_bytes: None,
            },
        );
        let dump_json = serde_json::to_string_pretty(&dump).expect("dump");
        fs::write(dumps_dir.join(format!("{name}.json")), &dump_json).expect("dump");
    }
    println!(
        "wrote {} pattern inputs + {} parse outcomes + {} ast dumps",
        PATTERNS.len(),
        PATTERNS.len(),
        PATTERNS.len()
    );

    // 4. Live PGEN parse-time measurement, persisted in a parseable
    //    format so the companion C baseline output can be combined into
    //    a ratio table by a downstream script.
    let mut p50_lines = String::new();
    p50_lines.push_str("# PGEN regex parser parse-time p50, in nanoseconds.\n");
    p50_lines.push_str("# Methodology: cargo run --release, default allocator,\n");
    p50_lines.push_str("# 5000 samples per pattern, 200 warmup samples discarded.\n");
    p50_lines
        .push_str("# Format: <name>\\t<p50_ns>\\t<min_ns>\\t<mean_ns>\\t<p99_ns>\\t<max_ns>\n");

    for (name, pattern) in PATTERNS {
        // Warmup.
        for _ in 0..WARMUP {
            let _ =
                pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern);
        }
        let mut samples = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            let t0 = Instant::now();
            let _ =
                pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern);
            samples.push(t0.elapsed().as_nanos());
        }
        samples.sort_unstable();
        let p50 = samples[samples.len() / 2];
        let min = *samples.first().unwrap();
        let max = *samples.last().unwrap();
        let mean = samples.iter().sum::<u128>() / samples.len() as u128;
        let p99 = samples[(samples.len() * 99) / 100];
        p50_lines.push_str(&format!("{name}\t{p50}\t{min}\t{mean}\t{p99}\t{max}\n"));
        println!("PGEN parse {name}: p50 = {p50} ns");
    }
    fs::write(measurements_dir.join("pgen_parse_p50.txt"), &p50_lines).expect("p50");
    println!("wrote measurements/pgen_parse_p50.txt");
}
