//! Generator for PGEN bug-report bundles per
//! `subs/pgen/docs/contracts/PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md`.
//!
//! Walks `subs/pcre2/testdata/testinput1`, identifies patterns where
//! PGEN's behavior triggers an RGX compile failure (PGEN parse failure,
//! PGEN-emitted contract mismatch, or PGEN-rejected input that PCRE2
//! accepts), deduplicates by pattern string, and writes one
//! `pgen-issues/PGEN-RGX-NNNN.yaml` + matching artifact bundle per
//! unique pattern.
//!
//! Run with:
//!   cargo run -p rgx-core --bin file_pgen_issues --features pgen-parser
//!
//! For each report bundle the tool writes:
//!   pgen-issues/PGEN-RGX-NNNN.yaml
//!   pgen-issues/artifacts/PGEN-RGX-NNNN/repro_input.txt
//!   pgen-issues/artifacts/PGEN-RGX-NNNN/pgen_contract.json
//!   pgen-issues/artifacts/PGEN-RGX-NNNN/pgen_parse_outcome.json
//!
//! The `pgen_trace.log` artifact is NOT generated automatically — it
//! requires running parseability_probe externally with
//! `PGEN_TRACE_VERBOSITY=debug`. The YAML's `command` field carries
//! the exact invocation a maintainer can run to capture the trace
//! when investigating the report.
//!
//! Reports are SCOPED to true PGEN bugs:
//! - PGEN rejects a pattern that PCRE2 accepts (`should parse but fails`)
//! - PGEN produces an AST node shape RGX's adapter can't lower
//!   ("contract mismatch")
//!
//! NOT reported here (those are RGX adapter gaps, tracked in
//! `docs/BACKLOG.md` C7):
//! - Unsupported `simple_escape` chars like `\"`, `\/` — PGEN parses
//!   correctly; RGX's adapter has no case for the resulting node.
//! - Unsupported `class_escape` variants like `[\b]`, `[\c]` — same
//!   reason.

use rgx_core::Regex;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// All PCRE2 10.47 testinput files with a single paired testoutput.
/// Width-specific files (8/11/12/14/22) are omitted because they
/// ship multi-width output variants that don't map to RGX's
/// byte-oriented engine.
const PCRE2_TESTFILES: &[&str] = &[
    "testinput1",
    "testinput2",
    "testinput3",
    "testinput4",
    "testinput5",
    "testinput6",
    "testinput7",
    "testinput9",
    "testinput10",
    "testinput13",
    // testinput15 skipped: hangs RGX on catastrophic-backtracking
    // patterns (see harness comment).
    "testinput16",
    "testinput17",
    "testinput18",
    "testinput19",
    "testinput20",
    "testinput21",
    "testinput23",
    "testinput24",
    "testinput25",
    "testinput26",
    "testinput27",
    "testinput28",
    "testinput29",
];

fn main() {
    // Isolation mode: `--scan <file>` walks one testinput file,
    // printing each pattern string (with line number) to stderr BEFORE
    // attempting `Regex::compile`. The last line printed before a
    // process abort is the culprit. Use this to locate patterns that
    // trigger PGEN stack overflows (which `catch_unwind` can't catch).
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--scan" {
        scan_single_file(&args[2]);
        return;
    }

    // `--ast-dump-only <pattern> <output_path>` writes the PGEN AST
    // dump JSON for the supplied pattern to the given path. Used to
    // backfill `pgen_ast_dump.json` for previously-generated reports.
    if args.len() >= 4 && args[1] == "--ast-dump-only" {
        let pat = &args[2];
        let out = &args[3];
        let opts = pgen::embedding_api::AstDumpOptions {
            pretty: true,
            max_ast_bytes: None,
        };
        let dump = pgen::embedding_api::parse_grammar_profile_ast_dump_named(
            "regex",
            "regex_default",
            pat,
            &opts,
        );
        std::fs::write(out, serde_json::to_string_pretty(&dump).expect("serialize"))
            .expect("write ast dump");
        eprintln!("wrote ast dump for {pat:?} to {out}");
        return;
    }

    // Single-report mode: `--single <pattern> [--source <file:line>]
    // [--summary-override <text>]`. Generates ONE PGEN-RGX-NNNN report
    // bundle for the supplied pattern, regardless of whether it's the
    // first failure of its category. Used when a cluster has been
    // distilled to a minimal repro and we only want one report for the
    // whole cluster.
    if args.len() >= 3 && args[1] == "--single" {
        let pattern = &args[2];
        let mut source_file = "<cluster repro>".to_string();
        let mut source_line: usize = 0;
        let mut summary_override: Option<String> = None;
        let mut bug_class_override: Option<PgenCategory> = None;
        let mut actual_override: Option<String> = None;
        let mut expected_override: Option<String> = None;
        let mut i = 3;
        while i < args.len() {
            match args[i].as_str() {
                "--source" if i + 1 < args.len() => {
                    let s = &args[i + 1];
                    if let Some((f, ln)) = s.split_once(':') {
                        source_file = f.to_string();
                        source_line = ln.parse().unwrap_or(0);
                    }
                    i += 2;
                }
                "--summary-override" if i + 1 < args.len() => {
                    summary_override = Some(args[i + 1].clone());
                    i += 2;
                }
                "--bug-class" if i + 1 < args.len() => {
                    bug_class_override = match args[i + 1].as_str() {
                        "parse-failure" => Some(PgenCategory::ParseFailure),
                        "contract-mismatch" => Some(PgenCategory::ContractMismatch),
                        "unterminated-class" => Some(PgenCategory::UnterminatedCharClass),
                        "accepts-pcre2-rejects" => Some(PgenCategory::AcceptsPcre2Rejects),
                        "wrong-ast" => Some(PgenCategory::WrongAstSemantics),
                        other => {
                            eprintln!(
                                "!! unknown --bug-class value {other:?}; expected one of: \
                                 parse-failure, contract-mismatch, unterminated-class, \
                                 accepts-pcre2-rejects, wrong-ast"
                            );
                            return;
                        }
                    };
                    i += 2;
                }
                "--actual" if i + 1 < args.len() => {
                    actual_override = Some(args[i + 1].clone());
                    i += 2;
                }
                "--expected" if i + 1 < args.len() => {
                    expected_override = Some(args[i + 1].clone());
                    i += 2;
                }
                _ => i += 1,
            }
        }
        emit_single_report(
            pattern,
            source_file,
            source_line,
            summary_override,
            bug_class_override,
            actual_override,
            expected_override,
        );
        return;
    }

    let mut unique_patterns: BTreeSet<String> = BTreeSet::new();
    let mut report_inputs: Vec<ReportInput> = Vec::new();

    for file_name in PCRE2_TESTFILES {
        let Ok(testinput) = std::fs::read(testdata_path(file_name)) else {
            eprintln!("!! skipping {file_name}: file not present");
            continue;
        };
        let blocks = split_into_blocks(&testinput);

        for (idx, block) in blocks.iter().enumerate() {
            let Some(first) = block.lines.first() else {
                continue;
            };
            if !first.starts_with(b"/") || !is_complete_pattern_line(first) {
                continue;
            }
            let Some((pat_bytes, _mod_bytes)) = split_pattern_line(first) else {
                continue;
            };
            let Ok(pat_str) = std::str::from_utf8(pat_bytes) else {
                continue;
            };
            // Historical stack-overflow guard: PGEN-RGX-0054 and 0055
            // were the two known process-aborting patterns. 0055 was
            // fixed by PGEN 1.1.19; 0054 was fixed by PGEN 1.1.21
            // (commit e617960, "Align regex parser with PCRE2 source
            // audit"). No guard currently needed. Add one here if a
            // new pattern shape overflows PGEN again.
            let compile_result = Regex::compile(pat_str);
            let Err(err) = compile_result else { continue };
            let msg = err.to_string();
            let Some(category) = classify_pgen_error(&msg) else {
                continue;
            };
            if !unique_patterns.insert(pat_str.to_string()) {
                continue;
            }
            report_inputs.push(ReportInput {
                pattern: pat_str.to_string(),
                error_message: msg,
                category,
                source_block_index: idx,
                source_line: block.start_line,
                source_file: file_name.to_string(),
            });
        }
    }

    eprintln!(
        "Found {} unique PGEN-related failing patterns across {} testinput files",
        report_inputs.len(),
        PCRE2_TESTFILES.len(),
    );

    // Optional cap from CLI for dry-run smoke tests:
    //   cargo run --bin file_pgen_issues --features pgen-parser -- --max 5
    let mut max_reports = report_inputs.len();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--max" && i + 1 < args.len() {
            max_reports = args[i + 1].parse().expect("--max takes a positive integer");
            i += 2;
        } else if args[i] == "--list-only" {
            for r in &report_inputs {
                println!(
                    "[{cat:?}] (line {ln}): {pat}",
                    cat = r.category,
                    ln = r.source_line,
                    pat = r.pattern
                );
            }
            return;
        } else {
            i += 1;
        }
    }
    if max_reports < report_inputs.len() {
        eprintln!("(capping to {max_reports} reports for this run)");
        report_inputs.truncate(max_reports);
    }

    let next_id = next_available_pgen_issue_id();
    let pgen_contract = capture_pgen_contract();
    eprintln!("Next available PGEN-RGX id: {next_id:04}");

    let pgen_issues_root = repo_root().join("pgen-issues");
    let artifacts_root = pgen_issues_root.join("artifacts");

    let rgx_commit = git_short_head().unwrap_or_else(|| "unknown".into());
    let host_os = std::env::consts::OS.to_string();
    let host_arch = std::env::consts::ARCH.to_string();
    let parser_backend = pgen_commit_short().unwrap_or_else(|| "unknown".into());
    let parser_release = pgen_release_version();
    let integration_contract = pgen_integration_contract_version();

    for (i, report) in report_inputs.iter().enumerate() {
        let id = next_id + i as u32;
        let id_str = format!("PGEN-RGX-{id:04}");
        let artifact_dir = artifacts_root.join(&id_str);
        std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");

        // 1) repro_input.txt — the exact failing pattern, no trailing newline.
        std::fs::write(artifact_dir.join("repro_input.txt"), &report.pattern)
            .expect("write repro_input");

        // 2) pgen_contract.json — shared snapshot.
        std::fs::write(artifact_dir.join("pgen_contract.json"), &pgen_contract)
            .expect("write pgen_contract");

        // 3) pgen_parse_outcome.json — capture per pattern.
        let outcome = capture_parse_outcome(&report.pattern);
        std::fs::write(artifact_dir.join("pgen_parse_outcome.json"), &outcome)
            .expect("write pgen_parse_outcome");

        // 4) YAML report.
        let yaml = build_yaml_report(
            &id_str,
            report,
            &rgx_commit,
            &host_os,
            &host_arch,
            &parser_backend,
            &parser_release,
            &integration_contract,
        );
        std::fs::write(pgen_issues_root.join(format!("{id_str}.yaml")), &yaml)
            .expect("write yaml report");

        eprintln!(
            "  wrote {} (line {}, category={:?})",
            id_str, report.source_line, report.category
        );
    }

    eprintln!(
        "\nDone. Wrote {} report bundles into {}",
        report_inputs.len(),
        pgen_issues_root.display()
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PgenCategory {
    /// PGEN failed to parse a pattern PCRE2 accepts.
    ParseFailure,
    /// PGEN parsed but produced an AST shape RGX's adapter doesn't
    /// recognize at the documented contract surface.
    ContractMismatch,
    /// PGEN parse error specific to char-class termination.
    UnterminatedCharClass,
    /// PGEN accepts a pattern PCRE2 10.47 rejects. RGX compiles
    /// cleanly (often producing a wrong-semantics program) because
    /// there's no adapter error to trigger. Only reachable via the
    /// explicit `--bug-class accepts-pcre2-rejects` CLI override.
    AcceptsPcre2Rejects,
    /// PGEN parsed successfully but the emitted AST disagrees with
    /// PCRE2 semantics — RGX compiles (so no adapter error) but the
    /// program matches differently from PCRE2. Only reachable via
    /// `--bug-class wrong-ast`.
    WrongAstSemantics,
}

struct ReportInput {
    pattern: String,
    error_message: String,
    category: PgenCategory,
    source_block_index: usize,
    source_line: usize,
    source_file: String,
}

#[allow(clippy::too_many_arguments)]
fn emit_single_report(
    pattern: &str,
    source_file: String,
    source_line: usize,
    summary_override: Option<String>,
    bug_class_override: Option<PgenCategory>,
    actual_override: Option<String>,
    expected_override: Option<String>,
) {
    // If no bug class is explicitly supplied, fall back to the
    // historical behaviour: attempt RGX compilation and infer the
    // category from the error shape. When RGX compiles cleanly AND
    // no bug class was supplied, the caller almost certainly meant
    // `--bug-class accepts-pcre2-rejects` or `--bug-class wrong-ast`
    // — so we bail out with a pointer instead of writing a
    // misleading report.
    let (err_msg, category) = if let Some(cat) = bug_class_override {
        // Try compile to capture any error message for `actual_behavior`,
        // but don't require it to fail. For AcceptsPcre2Rejects /
        // WrongAstSemantics the interesting fact is that RGX *did*
        // compile — record that or the caller-supplied actual text.
        let err_msg = match Regex::compile(pattern) {
            Ok(_) => actual_override.clone().unwrap_or_else(|| {
                "RGX compiled the pattern successfully (no error). \
                 PGEN accepted it; the divergence from PCRE2 is in the \
                 emitted AST or the grammar's permissiveness, not in \
                 whether RGX can turn the pattern into a program."
                    .to_string()
            }),
            Err(e) => e.to_string(),
        };
        (err_msg, cat)
    } else {
        match Regex::compile(pattern) {
            Ok(_) => {
                eprintln!(
                    "!! RGX successfully compiled the pattern; \
                     specify `--bug-class accepts-pcre2-rejects` or \
                     `--bug-class wrong-ast` (plus `--actual <text>` \
                     if you want a custom actual_behavior line) to \
                     file a report for this divergence shape."
                );
                return;
            }
            Err(e) => {
                let err_msg = e.to_string();
                let cat = classify_pgen_error(&err_msg).unwrap_or(PgenCategory::ContractMismatch);
                (err_msg, cat)
            }
        }
    };

    let next_id = next_available_pgen_issue_id();
    let id = format!("PGEN-RGX-{next_id:04}");
    let pgen_issues_root = repo_root().join("pgen-issues");
    let artifact_dir = pgen_issues_root.join("artifacts").join(&id);
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");

    std::fs::write(artifact_dir.join("repro_input.txt"), pattern).expect("write repro_input");
    std::fs::write(
        artifact_dir.join("pgen_contract.json"),
        capture_pgen_contract(),
    )
    .expect("write pgen_contract");
    let outcome = capture_parse_outcome(pattern);
    std::fs::write(artifact_dir.join("pgen_parse_outcome.json"), &outcome)
        .expect("write pgen_parse_outcome");
    // Per protocol §5: "include AST dump when parse succeeds but the
    // structure or semantics are wrong". For ContractMismatch we know
    // PGEN parsed cleanly — capture the dump so PGEN maintainers see
    // exactly which node shape needs the grammar change.
    if outcome.contains("\"status\": \"success\"") {
        let opts = pgen::embedding_api::AstDumpOptions {
            pretty: true,
            max_ast_bytes: None,
        };
        let dump = pgen::embedding_api::parse_grammar_profile_ast_dump_named(
            "regex",
            "regex_default",
            pattern,
            &opts,
        );
        let dump_json = serde_json::to_string_pretty(&dump).expect("serialize ast dump");
        std::fs::write(artifact_dir.join("pgen_ast_dump.json"), dump_json)
            .expect("write pgen_ast_dump");
    }

    let report = ReportInput {
        pattern: pattern.to_string(),
        error_message: err_msg,
        category,
        source_block_index: 0,
        source_line,
        source_file,
    };

    let rgx_commit = git_short_head().unwrap_or_else(|| "unknown".into());
    let host_os = std::env::consts::OS.to_string();
    let host_arch = std::env::consts::ARCH.to_string();
    let parser_backend = pgen_commit_short().unwrap_or_else(|| "unknown".into());
    let parser_release = pgen_release_version();
    let integration_contract = pgen_integration_contract_version();
    let mut yaml = build_yaml_report(
        &id,
        &report,
        &rgx_commit,
        &host_os,
        &host_arch,
        &parser_backend,
        &parser_release,
        &integration_contract,
    );
    if let Some(s) = summary_override {
        // Replace the auto-generated summary line with the override
        // for human-readable cluster-level descriptions.
        replace_yaml_block(&mut yaml, "summary: |\n", "\nstatus:", &s);
    }
    if let Some(s) = expected_override {
        replace_yaml_block(
            &mut yaml,
            "expected_behavior: |\n",
            "\n\nactual_behavior:",
            &s,
        );
    }
    if let Some(s) = actual_override {
        replace_yaml_block(&mut yaml, "actual_behavior: |\n", "\n\nreproduction:", &s);
    }
    std::fs::write(pgen_issues_root.join(format!("{id}.yaml")), &yaml).expect("write yaml report");
    eprintln!("wrote {id} (category={category:?}) for pattern {pattern:?}");
}

/// Replace a two-space-indented text block within a YAML document
/// framed by `start_marker` (inclusive) and `end_marker` (exclusive).
/// Used to swap auto-generated summary / expected / actual blocks
/// for caller-supplied cluster-tailored wording.
fn replace_yaml_block(yaml: &mut String, start_marker: &str, end_marker: &str, replacement: &str) {
    let Some(start) = yaml.find(start_marker) else {
        return;
    };
    let after = start + start_marker.len();
    let Some(end_rel) = yaml[after..].find(end_marker) else {
        return;
    };
    let end = after + end_rel;
    let indented = replacement
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    yaml.replace_range(after..end, &indented);
}

fn scan_single_file(file_name: &str) {
    use std::io::Write;
    let path = testdata_path(file_name);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("cannot read {}", path.display());
        return;
    };
    let blocks = split_into_blocks(&bytes);
    for block in blocks.iter() {
        let Some(first) = block.lines.first() else {
            continue;
        };
        if !first.starts_with(b"/") || !is_complete_pattern_line(first) {
            continue;
        }
        let Some((pat_bytes, _)) = split_pattern_line(first) else {
            continue;
        };
        let Ok(pat_str) = std::str::from_utf8(pat_bytes) else {
            continue;
        };
        // Flush-print BEFORE compile so the abort shows the culprit.
        eprint!(
            "[{ln}] trying: {pat}  ... ",
            ln = block.start_line,
            pat = pat_str
        );
        std::io::stderr().flush().ok();
        let _ = Regex::compile(pat_str);
        eprintln!("OK");
    }
    eprintln!("scan of {file_name} completed without aborting");
}

fn classify_pgen_error(msg: &str) -> Option<PgenCategory> {
    if msg.contains("unterminated character class") {
        return Some(PgenCategory::UnterminatedCharClass);
    }
    if msg.contains("E_PARSE_FAILURE: generated regex parse failed") {
        return Some(PgenCategory::ParseFailure);
    }
    // The class_item / class_escape / simple_escape variants are RGX
    // adapter gaps — PGEN parsed successfully but produced a node
    // RGX's adapter doesn't have a case for. NOT a PGEN bug per the
    // protocol; tracked in BACKLOG C7 instead.
    if msg.contains("pgen AST contract mismatch") {
        if msg.contains("unrecognized simple_escape") {
            return None;
        }
        if msg.contains("class_escape resolved to unsupported variant") {
            return None;
        }
        if msg.contains("class_item has no known variant") {
            // This one IS arguably PGEN-side: PGEN emitted a node
            // shape that doesn't match the documented contract for
            // class_item (RGX adapter expects specific child shapes).
            // Report as contract mismatch.
            return Some(PgenCategory::ContractMismatch);
        }
        return Some(PgenCategory::ContractMismatch);
    }
    None
}

fn build_yaml_report(
    id: &str,
    report: &ReportInput,
    rgx_commit: &str,
    host_os: &str,
    host_arch: &str,
    parser_backend: &str,
    parser_release: &str,
    integration_contract: &str,
) -> String {
    let summary = match report.category {
        PgenCategory::ParseFailure => format!(
            "PGEN regex parser rejects pattern {pat:?} that PCRE2 10.47 accepts.",
            pat = report.pattern
        ),
        PgenCategory::UnterminatedCharClass => format!(
            "PGEN regex parser reports `unterminated character class` on pattern {pat:?} that PCRE2 10.47 parses cleanly.",
            pat = report.pattern
        ),
        PgenCategory::ContractMismatch => format!(
            "PGEN regex parser produces an AST node shape RGX's adapter does not recognize for pattern {pat:?}.",
            pat = report.pattern
        ),
        PgenCategory::AcceptsPcre2Rejects => format!(
            "PGEN regex parser accepts pattern {pat:?} that PCRE2 10.47 rejects at compile time. RGX inherits the permissive behaviour and matches against a subject where PCRE2 would refuse to compile.",
            pat = report.pattern
        ),
        PgenCategory::WrongAstSemantics => format!(
            "PGEN regex parser emits an AST for pattern {pat:?} whose semantics diverge from PCRE2 10.47 — RGX compiles without error but produces a program that matches differently from PCRE2 (override `expected_behavior` / `actual_behavior` fields with the concrete divergence).",
            pat = report.pattern
        ),
    };
    let bug_class = match report.category {
        PgenCategory::ParseFailure | PgenCategory::UnterminatedCharClass => {
            "should_parse_but_fails"
        }
        PgenCategory::ContractMismatch | PgenCategory::WrongAstSemantics => {
            "parses_but_returns_wrong_ast"
        }
        PgenCategory::AcceptsPcre2Rejects => "should_fail_but_parses",
    };
    let expected = match report.category {
        PgenCategory::ParseFailure | PgenCategory::UnterminatedCharClass => {
            "PGEN should accept the pattern. PCRE2 10.47 parses it as a valid regex; the corresponding case in `subs/pcre2/testdata/testinput1` (line {line}) expects a successful match.".replace("{line}", &report.source_line.to_string())
        }
        PgenCategory::ContractMismatch => "PGEN should emit an AST whose node shapes match the documented `class_item` (or analogous) contract that the RGX adapter walks. The current output triggers RGX's contract guard at compile time.".to_string(),
        PgenCategory::AcceptsPcre2Rejects => "PGEN should reject the pattern at parse time, matching PCRE2 10.47's compile-time rejection. Currently PGEN accepts it and emits an AST; any downstream engine that trusts PGEN's output inherits PCRE2-incompatible permissiveness.".to_string(),
        PgenCategory::WrongAstSemantics => "PGEN should emit an AST whose semantics align with PCRE2 10.47's documented matching behaviour for this construct. See `actual_behavior` for the concrete divergence and `pgen_ast_dump.json` for the node shape PGEN emits.".to_string(),
    };
    let actual = format!(
        "RGX `Regex::compile({pat:?})` returns:\n      {err}",
        pat = report.pattern,
        err = report
            .error_message
            .lines()
            .next()
            .unwrap_or(&report.error_message),
    );
    let command = format!(
        "cd subs/pgen && PGEN_TRACE_VERBOSITY=debug \\\n    cargo run --manifest-path rust/Cargo.toml --features generated_parsers \\\n      --bin parseability_probe -- --parse regex \\\n      ../../pgen-issues/artifacts/{id}/repro_input.txt \\\n      --profile regex_default --trace \\\n      --trace-log-file ../../pgen-issues/artifacts/{id}/pgen_trace.log"
    );
    // Source-block wording depends on the divergence shape.
    let source_tail = match report.category {
        PgenCategory::AcceptsPcre2Rejects => {
            "PCRE2 10.47 *rejects* this pattern at compile time; PGEN \
             accepts it. See `expected_behavior` / `actual_behavior` \
             for the exact rule."
        }
        PgenCategory::WrongAstSemantics => {
            "PCRE2 10.47 parses AND matches this pattern; PGEN parses \
             but emits an AST whose semantics diverge from PCRE2. See \
             `actual_behavior` for the divergence and \
             `pgen_ast_dump.json` for PGEN's emitted shape."
        }
        _ => {
            "PCRE2 10.47 accepts and matches this pattern in its own \
             pcre2test harness; PGEN's regex grammar diverges as \
             recorded under `actual_behavior`."
        }
    };
    // Reproduction "Expected/Actual" one-liners also depend on the
    // divergence shape.
    let (repro_expected, repro_actual) = match report.category {
        PgenCategory::AcceptsPcre2Rejects => (
            "parseability_probe rejects the input with a `\\N is not \
             allowed in a character class`-style diagnostic (or the \
             analogous PCRE2 reason for this cluster).",
            "PGEN accepts the pattern; `pgen_parse_outcome.json` \
             records `status: success` with no diagnostic. RGX then \
             compiles the (PCRE2-incompatible) AST and matches \
             against subjects PCRE2 would refuse.",
        ),
        PgenCategory::WrongAstSemantics => (
            "parseability_probe accepts the input and emits an AST \
             whose shape matches PCRE2's documented matching \
             behaviour for this construct.",
            "PGEN accepts the input but emits an AST whose shape \
             diverges from PCRE2; see `pgen_ast_dump.json` and \
             `actual_behavior`.",
        ),
        _ => (
            "parseability_probe accepts the input (`parse_full \
             passed`).",
            "PGEN rejects with the diagnostic recorded in \
             `pgen_parse_outcome.json` (or, for contract mismatches, \
             RGX's compile error captured in `actual_behavior`).",
        ),
    };
    let date = current_utc_timestamp();
    format!(
        r#"id: {id}
summary: |
  {summary}
status: open
opened_at: {date}
first_seen_at: {date}
last_updated_at: {date}

# === Parser identity (per PGEN_PARSER_ISSUE_REPORTING_PROTOCOL.md §1) ===
parser_backend: pgen
parser_backend_version: "{parser_backend}"
parser_release_version: "{parser_release}"
integration_contract_version: "{integration_contract}"
parser_family: regex
profile: regex_default
integration_surface: parseability_probe
generated_artifact: "PGEN submodule subs/pgen, embedded via rgx-core/src/parsing.rs"

# === Host project identity (per protocol §2) ===
host_project: rgx
rgx_commit: {rgx_commit}
host_os: {host_os}
host_arch: {host_arch}
rust_toolchain: "rustc per workspace rust-version = 1.88"
cargo_features: "default + pgen-parser"

upstream_report:
  reported: false
  issue_id: null
  issue_url: null
  reported_at: null

context:
  feature_flag: pgen-parser
  parser_entrypoint: rgx-core/src/parsing.rs
  command: |
    {command}
  pattern: {pat_yaml}
  source: |
    Discovered by the RGX PCRE2 conformance harness
    (`rgx-core/tests/pcre2_conformance.rs`) while scanning
    `subs/pcre2/testdata/{source_file}` block #{block_idx} starting
    near input line {source_line}. {source_tail}

# === Bug class (per protocol §4) ===
bug_class: {bug_class}

expected_behavior: |
  {expected}

actual_behavior: |
  {actual}

reproduction: |
  Reproducer artifacts under pgen-issues/artifacts/{id}/:
    - repro_input.txt — exact failing input (one line, no trailing newline)
    - pgen_contract.json — captured `parser_embedding_api_contract()` JSON
    - pgen_parse_outcome.json — captured `parse_grammar_profile_named` JSON

  Reproduction command (from rgx repo root) — captures the trace:
    {command}

  Expected: {repro_expected}
  Actual: {repro_actual}

impact: |
  One of {n_failures} PGEN-related failures uncovered by RGX's
  PCRE2 10.47 testinput1 conformance harness. Each failing pattern
  also blocks RGX from passing the corresponding pcre2test case.
  Aggregate impact is tracked in `docs/BACKLOG.md` C7.

resolution:
  status: unresolved
  fixed_in_rgx_commit: null
  verified_at: null
  verification_notes: |
    Add closing validation evidence here when the issue is resolved.
"#,
        id = id,
        summary = indent(&summary, 2),
        date = date,
        parser_backend = parser_backend,
        parser_release = parser_release,
        integration_contract = integration_contract,
        rgx_commit = rgx_commit,
        host_os = host_os,
        host_arch = host_arch,
        command = indent(&command, 4),
        pat_yaml = yaml_quote_pattern(&report.pattern),
        block_idx = report.source_block_index,
        source_line = report.source_line,
        source_file = report.source_file,
        source_tail = indent(source_tail, 4),
        bug_class = bug_class,
        expected = indent(&expected, 2),
        actual = indent(&actual, 2),
        repro_expected = indent(repro_expected, 4),
        repro_actual = indent(repro_actual, 4),
        n_failures = "many", // approximate; harness emits the exact count when run
    )
}

/// Return the current UTC instant as an ISO-8601 string suitable for
/// the report's `opened_at` / `first_seen_at` / `last_updated_at`
/// fields.
fn current_utc_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = now / 86_400;
    let secs_in_day = now % 86_400;
    // Civil-from-days algorithm (Howard Hinnant), public domain.
    let z: i64 = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn indent(s: &str, n: usize) -> String {
    let pad = " ".repeat(n);
    s.lines()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                l.to_string()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn yaml_quote_pattern(p: &str) -> String {
    // YAML double-quoted string: backslash + double-quote escape.
    let escaped: String = p
        .chars()
        .map(|c| match c {
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            '\r' => "\\r".to_string(),
            c if (c as u32) < 0x20 => format!("\\x{:02X}", c as u32),
            c => c.to_string(),
        })
        .collect();
    format!("\"{escaped}\"")
}

fn capture_pgen_contract() -> String {
    let contract = pgen::embedding_api::parser_embedding_api_contract();
    serde_json::to_string_pretty(&contract).expect("serialize contract")
}

fn capture_parse_outcome(pattern: &str) -> String {
    let outcome =
        pgen::embedding_api::parse_grammar_profile_named("regex", "regex_default", pattern);
    serde_json::to_string_pretty(&outcome).expect("serialize outcome")
}

fn pgen_release_version() -> String {
    pgen::embedding_api::parser_embedding_api_contract()
        .regex_parser_release_version
        .clone()
}

fn pgen_integration_contract_version() -> String {
    pgen::embedding_api::parser_embedding_api_contract()
        .regex_integration_contract_version
        .clone()
}

fn pgen_commit_short() -> Option<String> {
    let pgen_dir = repo_root().join("subs/pgen");
    let out = std::process::Command::new("git")
        .args(["-C", pgen_dir.to_str()?, "rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_short_head() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .current_dir(repo_root())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn next_available_pgen_issue_id() -> u32 {
    let mut max_id = 0u32;
    let dir = repo_root().join("pgen-issues");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 1;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(rest) = name.strip_prefix("PGEN-RGX-") {
            if let Some(num_part) = rest.strip_suffix(".yaml") {
                if let Ok(n) = num_part.parse::<u32>() {
                    max_id = max_id.max(n);
                }
            }
        }
    }
    max_id + 1
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate manifest dir has a parent")
        .to_path_buf()
}

fn testdata_path(name: &str) -> PathBuf {
    repo_root().join("subs/pcre2/testdata").join(name)
}

// ---------------------------------------------------------------------------
// Mini block-parser (lifted from the conformance harness, kept inline so
// this binary can stand alone without re-organizing the test module).
// ---------------------------------------------------------------------------

struct Block<'a> {
    lines: Vec<&'a [u8]>,
    start_line: usize,
}

fn split_into_blocks(bytes: &[u8]) -> Vec<Block<'_>> {
    let lines = split_lines(bytes);
    let mut blocks = Vec::new();
    let mut current: Vec<&[u8]> = Vec::new();
    let mut start_line: usize = 0;
    for (idx, line) in lines.iter().enumerate() {
        if is_blank(line) {
            if !current.is_empty() {
                blocks.push(Block {
                    lines: std::mem::take(&mut current),
                    start_line,
                });
            }
        } else {
            if current.is_empty() {
                start_line = idx + 1;
            }
            current.push(line);
        }
    }
    if !current.is_empty() {
        blocks.push(Block {
            lines: current,
            start_line,
        });
    }
    blocks
}

fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes
        .split(|&b| b == b'\n')
        .map(|l| {
            if l.ends_with(b"\r") {
                &l[..l.len() - 1]
            } else {
                l
            }
        })
        .collect()
}

fn is_blank(line: &[u8]) -> bool {
    line.iter().all(|&b| b == b' ' || b == b'\t')
}

fn is_complete_pattern_line(line: &[u8]) -> bool {
    if !line.starts_with(b"/") {
        return false;
    }
    let mut i = 1;
    while i < line.len() {
        if line[i] == b'\\' && i + 1 < line.len() {
            i += 2;
            continue;
        }
        if line[i] == b'/' {
            return true;
        }
        i += 1;
    }
    false
}

fn split_pattern_line(line: &[u8]) -> Option<(&[u8], &[u8])> {
    if !line.starts_with(b"/") {
        return None;
    }
    let mut i = 1;
    while i < line.len() {
        if line[i] == b'\\' && i + 1 < line.len() {
            i += 2;
            continue;
        }
        if line[i] == b'/' {
            return Some((&line[1..i], &line[i + 1..]));
        }
        i += 1;
    }
    None
}
