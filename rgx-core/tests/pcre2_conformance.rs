//! PCRE2 10.47 testdata conformance harness.
//!
//! Imports `subs/pcre2/testdata/testinput1` + `testoutput1` — the core-syntax
//! Perl-compatible test suite shipped with PCRE2 10.47 — and runs each
//! `(pattern, modifiers, subject, expected)` tuple through RGX, diffing the
//! observed match/no-match outcome against PCRE2's expected output.
//!
//! This is the authoritative source of truth for PCRE2 feature parity —
//! thousands of edge cases curated by the PCRE2 maintainers over decades.
//!
//! Test scope (testinput1 only, this commit):
//! - Perl-compatible features only (the file has `#perltest` header)
//! - non-UTF mode (the file has `#forbid_utf` header)
//! - cases that the parser below understands; everything else is counted
//!   as `skipped` and does not affect the pass-rate metric
//!
//! Runtime: the full suite runs ~1500 test cases which takes a few seconds.
//! Marked `#[ignore]` so it doesn't slow `cargo test`; run explicitly with
//! `cargo test --test pcre2_conformance -- --ignored --nocapture`.
//!
//! The harness emits a per-category report to stderr and currently does
//! NOT fail the test if RGX diverges — it emits the count so the ledger
//! can track improvement. Wiring a known-failures baseline is a follow-up.

use rgx_core::{Regex, RegexBuilder};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Parser for the PCRE2 testinput format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TestCase {
    pattern: String,
    /// PCRE2 flag chars after the closing `/` (e.g. "imsx", "g", "i").
    modifiers: String,
    /// Full modifier text, including named modifiers like `aftertext`.
    full_modifiers: String,
    subject: Vec<u8>,
    expected: Expected,
    /// The subject line was preceded by `\= Expect no match`.
    expect_no_match_annotation: bool,
    line_number: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Expected {
    /// `No match` observed in testoutput.
    NoMatch,
    /// Match with capture groups (group 0 = overall; groups 1..N may be
    /// `None` if unmatched, surfaced as `<unset>` in PCRE2 output).
    ///
    /// For the scope of this harness we only use group 0 (the overall
    /// match span). Capture-group comparison is a natural extension.
    Match { overall: Vec<u8> },
}

/// Parse both files in lockstep, yielding test cases. Unknown / complex
/// cases are dropped silently; the caller counts what lands.
fn parse_cases(testinput: &[u8], testoutput: &[u8]) -> Vec<TestCase> {
    let in_lines: Vec<&[u8]> = split_lines(testinput);
    let out_lines: Vec<&[u8]> = split_lines(testoutput);

    let mut cases = Vec::new();
    let mut i = 0;
    let mut o = 0;
    // Track whether we're inside an `#if !ebcdic` block (ASCII always has
    // it true, so we just keep running; an `#if !EBCDIC_THAT_IS_ALWAYS_TRUE`
    // block reads normally). For portability we skip inside `#if ebcdic`.
    let mut skip_block = false;

    // Also mirror the #if/#endif state on the output side so we stay
    // synchronised. pcre2test includes output unconditionally inside the
    // #if blocks it emits; we mirror skip_block on both sides.

    while i < in_lines.len() && o < out_lines.len() {
        let line = in_lines[i];

        // Comments and blank lines are shared between files.
        if line.is_empty() || line.starts_with(b"#") {
            // Handle conditional blocks
            if line.starts_with(b"#if ") {
                let cond = String::from_utf8_lossy(&line[4..]);
                let c = cond.trim();
                // We execute on non-EBCDIC ASCII, so `!ebcdic` is TRUE and
                // `ebcdic` is FALSE.
                skip_block = matches!(c, "ebcdic");
            } else if line == b"#endif" {
                skip_block = false;
            }
            // Consume the matching comment/blank in testoutput if present.
            if o < out_lines.len() && (out_lines[o].is_empty() || out_lines[o].starts_with(b"#")) {
                o += 1;
            }
            i += 1;
            continue;
        }

        if skip_block {
            i += 1;
            // Consume output too — output also carries the same skipped
            // section. We step forward until we find a pattern line there.
            while o < out_lines.len() && !out_lines[o].starts_with(b"/") {
                o += 1;
            }
            continue;
        }

        // Expect a pattern line: /pattern/modifiers
        if !line.starts_with(b"/") {
            // Unknown line shape (e.g. continuation of a multi-line
            // pattern). Skip it defensively.
            i += 1;
            continue;
        }

        // A pattern can span multiple input lines if it contains newlines
        // (PCRE2 allows this; the pattern continues until we see a closing
        // `/` followed by modifiers on a line). For this initial harness
        // we only handle single-line patterns — multi-line patterns are
        // dropped.
        if !is_complete_pattern_line(line) {
            // Skip until we see the pattern end (a line that looks like
            // modifiers or a blank line).
            i += 1;
            while i < in_lines.len() && !in_lines[i].is_empty() {
                i += 1;
            }
            // Skip the corresponding output until the next blank.
            while o < out_lines.len() && !out_lines[o].is_empty() {
                o += 1;
            }
            continue;
        }

        // Advance output to the matching pattern line.
        while o < out_lines.len() && !out_lines[o].starts_with(b"/") {
            o += 1;
        }
        if o >= out_lines.len() {
            break;
        }

        let Some((pattern_bytes, modifiers_bytes)) = split_pattern_line(line) else {
            i += 1;
            continue;
        };
        let Ok(pattern) = String::from_utf8(pattern_bytes.to_vec()) else {
            // Non-UTF-8 patterns: skip for this harness
            i += 1;
            o += 1;
            continue;
        };
        let full_modifiers = String::from_utf8_lossy(modifiers_bytes).to_string();
        let modifiers = extract_short_modifiers(&full_modifiers);
        let pattern_line_number = i + 1;
        i += 1;
        o += 1;

        // Now parse subject lines under this pattern in both files, in
        // lockstep, until the next blank line (= end of this pattern's
        // block).
        let mut expect_no_match_annotation = false;
        while i < in_lines.len() && !in_lines[i].is_empty() {
            let iline = in_lines[i];
            let trimmed = trim_leading_spaces(iline);

            // `\= Expect no match` is an annotation that changes the
            // default interpretation of subsequent subject lines.
            if trimmed.starts_with(b"\\=") {
                let annotation = String::from_utf8_lossy(&trimmed[2..]);
                if annotation.trim().starts_with("Expect no match") {
                    expect_no_match_annotation = true;
                }
                i += 1;
                // The annotation isn't mirrored in testoutput — it's a
                // testinput-only marker.
                continue;
            }

            // Subject line: strip leading 4 spaces (PCRE2 convention) and
            // decode escapes.
            let Some(subject) = decode_subject(trimmed) else {
                i += 1;
                continue;
            };

            // Match up the output for this subject. Output lines belong
            // to this subject until the next subject line in testinput or
            // a blank line in testoutput.
            let (expected, consumed_output) = parse_subject_output(&out_lines, o);
            o += consumed_output;

            cases.push(TestCase {
                pattern: pattern.clone(),
                modifiers: modifiers.clone(),
                full_modifiers: full_modifiers.clone(),
                subject,
                expected,
                expect_no_match_annotation,
                line_number: pattern_line_number,
            });
            i += 1;
        }

        // Consume the blank line in both files.
        if i < in_lines.len() && in_lines[i].is_empty() {
            i += 1;
        }
        if o < out_lines.len() && out_lines[o].is_empty() {
            o += 1;
        }
    }

    cases
}

fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes
        .split(|&b| b == b'\n')
        .map(|l| {
            // Strip trailing \r for CRLF files.
            if l.ends_with(b"\r") {
                &l[..l.len() - 1]
            } else {
                l
            }
        })
        .collect()
}

fn trim_leading_spaces(line: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < line.len() && line[i] == b' ' {
        i += 1;
    }
    &line[i..]
}

/// A complete pattern line has the form `/.../<modifiers>` with both
/// slashes on the same line and no unescaped `/` inside the pattern
/// body. Conservatively: line starts with `/`, has at least one `/` after
/// position 0, and the last non-modifier-char run is valid modifier text.
fn is_complete_pattern_line(line: &[u8]) -> bool {
    if !line.starts_with(b"/") {
        return false;
    }
    // Walk from the start looking for the closing `/` that is not escaped.
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

/// Extract the "short" flag modifiers (single-letter: i, m, s, x, g, I, etc.)
/// from a PCRE2 modifier string. Ignores named modifiers like `aftertext`,
/// `dupnames`, `no_start_optimize`, etc.
fn extract_short_modifiers(full: &str) -> String {
    let mut out = String::new();
    // Modifiers are comma-separated; short flags are a run of chars before
    // a comma or end. We collect only the first token if it's all letters.
    let first_token = full.split(',').next().unwrap_or("");
    for c in first_token.chars() {
        if c.is_ascii_alphabetic() {
            out.push(c);
        } else {
            break;
        }
    }
    out
}

/// Decode a subject line's escape sequences per PCRE2's testinput rules.
/// Returns the raw subject bytes; returns None if the escape form is one
/// we don't handle.
fn decode_subject(line: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        let b = line[i];
        if b != b'\\' {
            out.push(b);
            i += 1;
            continue;
        }
        if i + 1 >= line.len() {
            // Trailing lone backslash — keep as literal.
            out.push(b'\\');
            i += 1;
            continue;
        }
        let n = line[i + 1];
        match n {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'e' => out.push(0x1b),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'\\' => out.push(b'\\'),
            b'?' => out.push(b'?'),
            b'"' => out.push(b'"'),
            b'\'' => out.push(b'\''),
            b'$' => out.push(b'$'),
            b'/' => out.push(b'/'),
            b'x' => {
                // \xHH or \x{H..H}
                if i + 2 < line.len() && line[i + 2] == b'{' {
                    // Find the closing `}`
                    let mut j = i + 3;
                    while j < line.len() && line[j] != b'}' {
                        j += 1;
                    }
                    if j >= line.len() {
                        return None;
                    }
                    let hex = std::str::from_utf8(&line[i + 3..j]).ok()?;
                    let cp = u32::from_str_radix(hex, 16).ok()?;
                    // Encode as UTF-8 if > 0xFF; else push raw byte.
                    if cp <= 0xFF {
                        out.push(cp as u8);
                    } else {
                        let c = char::from_u32(cp)?;
                        let mut buf = [0u8; 4];
                        let s = c.encode_utf8(&mut buf);
                        out.extend_from_slice(s.as_bytes());
                    }
                    i = j + 1;
                    continue;
                } else {
                    // \xHH (exactly 2 hex digits expected)
                    if i + 3 >= line.len() {
                        return None;
                    }
                    let hex = std::str::from_utf8(&line[i + 2..i + 4]).ok()?;
                    let b = u8::from_str_radix(hex, 16).ok()?;
                    out.push(b);
                    i += 4;
                    continue;
                }
            }
            c if c.is_ascii_digit() => {
                // Octal \NNN — up to 3 octal digits.
                let mut j = i + 1;
                let end = (i + 4).min(line.len());
                while j < end && line[j].is_ascii_digit() && line[j] < b'8' {
                    j += 1;
                }
                let oct = std::str::from_utf8(&line[i + 1..j]).ok()?;
                let v = u32::from_str_radix(oct, 8).ok()?;
                if v <= 0xFF {
                    out.push(v as u8);
                } else {
                    return None;
                }
                i = j;
                continue;
            }
            _ => {
                // Unknown escape — drop this subject line.
                return None;
            }
        }
        i += 2;
    }
    Some(out)
}

/// From testoutput starting at index `start`, read lines that belong to the
/// current subject (up until the next subject, blank, or pattern line).
/// Returns the parsed Expected and number of lines consumed.
fn parse_subject_output(out_lines: &[&[u8]], start: usize) -> (Expected, usize) {
    let mut consumed = 0;
    // First line is the echoed subject (starts with 4 spaces). Skip it.
    if start < out_lines.len() {
        let l = out_lines[start];
        if l.starts_with(b"    ") || l.starts_with(b"\\=") {
            consumed += 1;
        }
    }

    // Next lines are ` 0: ...`, ` 1: ...`, `No match`, or error messages.
    let mut idx = start + consumed;
    let mut overall: Option<Vec<u8>> = None;
    let mut no_match = false;
    while idx < out_lines.len() {
        let l = out_lines[idx];
        if l.is_empty() || l.starts_with(b"/") || l.starts_with(b"#") {
            break;
        }
        // Lines starting with 4 spaces or `\=` are NEW subjects — stop.
        if (l.starts_with(b"    ") || l.starts_with(b"\\=")) && consumed > 0 {
            break;
        }
        let text = String::from_utf8_lossy(l);
        let trimmed = text.trim_start();
        if trimmed == "No match" {
            no_match = true;
            consumed += 1;
            idx += 1;
            break;
        }
        if trimmed.starts_with("0:") {
            // Overall match line. Format: ` 0: <text>`
            // PCRE2 escapes control bytes in the displayed match using
            // `\xHH`, `\\`, `\?`, `\=`, etc. — same grammar as testinput
            // subject lines. Decode back to raw bytes so the comparison
            // is byte-for-byte.
            let body = trimmed.trim_start_matches("0:").trim_start();
            overall = decode_subject(body.as_bytes());
            consumed += 1;
            idx += 1;
            continue;
        }
        if trimmed.starts_with("1:")
            || trimmed.starts_with("2:")
            || trimmed.starts_with("3:")
            || trimmed.starts_with("4:")
            || trimmed.starts_with("5:")
            || trimmed.starts_with("6:")
            || trimmed.starts_with("7:")
            || trimmed.starts_with("8:")
            || trimmed.starts_with("9:")
        {
            // Capture group line — recorded but ignored for the overall
            // comparison.
            consumed += 1;
            idx += 1;
            continue;
        }
        // Error line or unfamiliar output — eat it so we advance.
        consumed += 1;
        idx += 1;
    }

    let expected = if no_match {
        Expected::NoMatch
    } else if let Some(text) = overall {
        Expected::Match { overall: text }
    } else {
        // Pattern produced neither `No match` nor ` 0:` — an unusual case
        // (partial match, error). Treat as `NoMatch` so the harness at
        // least has a definite comparison; downstream can widen this.
        Expected::NoMatch
    };
    (expected, consumed)
}

// ---------------------------------------------------------------------------
// RGX runner: compile a PCRE2 pattern+modifiers through RGX and compare.
// ---------------------------------------------------------------------------

#[derive(Debug)]
#[allow(dead_code)] // detail / reason are consumed by Debug + formatters
enum Outcome {
    Pass,
    Fail { detail: String },
    Skip { reason: &'static str },
    Panic { detail: String },
}

fn run_case(case: &TestCase) -> Outcome {
    // Skip cases with named modifiers that would change semantics in a way
    // we don't model (e.g. `no_start_optimize`, `aftertext`, `dupnames`).
    // We only run the pure-flag subset for this first pass.
    if case.full_modifiers.contains(',') {
        return Outcome::Skip {
            reason: "named PCRE2 modifiers not modelled yet",
        };
    }
    for c in case.modifiers.chars() {
        if !matches!(c, 'i' | 'm' | 's' | 'x' | 'g') {
            return Outcome::Skip {
                reason: "unmodelled short modifier",
            };
        }
    }

    // Subject must be valid UTF-8 for the `&str` API.
    let Ok(subject) = std::str::from_utf8(&case.subject) else {
        return Outcome::Skip {
            reason: "non-UTF-8 subject",
        };
    };

    // Build through RegexBuilder so flag application is consistent.
    // RegexBuilder methods consume self (fluent chain), so we rebind.
    let mut builder = RegexBuilder::new(&case.pattern);
    let mut want_global = false;
    for c in case.modifiers.chars() {
        builder = match c {
            'i' => builder.case_insensitive(),
            'm' => builder.multi_line(),
            's' => builder.dot_matches_new_line(),
            'x' => builder.ignore_whitespace(),
            'g' => {
                want_global = true;
                builder
            }
            _ => unreachable!("pre-filtered above"),
        };
    }

    let re: Regex = match builder.build() {
        Ok(r) => r,
        Err(e) => {
            // RGX-side compile failure. If PCRE2 expected NoMatch or a
            // successful match, this is a real parity gap. If PCRE2 also
            // errored (rare in testinput1 since those go to other files),
            // the harness here would still classify as Fail. Widening the
            // "RGX error matches PCRE2 error" comparison is future work.
            return Outcome::Fail {
                detail: format!("compile error: {e}"),
            };
        }
    };

    match (&case.expected, case.expect_no_match_annotation) {
        (Expected::NoMatch, _) | (_, true) => {
            if re.is_match(subject) {
                return Outcome::Fail {
                    detail: format!("PCRE2 expected no match, RGX matched (subject={subject:?})"),
                };
            }
            Outcome::Pass
        }
        (Expected::Match { overall }, _) => {
            let Some(m) = (if want_global {
                re.find_all(subject).into_iter().next()
            } else {
                re.find_first(subject)
            }) else {
                return Outcome::Fail {
                    detail: format!(
                        "PCRE2 expected match {:?}, RGX no match (subject={subject:?})",
                        String::from_utf8_lossy(overall)
                    ),
                };
            };
            let rgx_match = &subject[m.start..m.end];
            // PCRE2 output prints the match directly, with some escaping
            // for control chars (e.g. `\x09` for tab). For strict byte
            // comparison on ASCII text this works; for cases where PCRE2's
            // output contains `\x..` escapes, `rgx_match == overall` will
            // be false and we flag it as a miscompare the caller can
            // classify.
            if rgx_match.as_bytes() == overall.as_slice() {
                Outcome::Pass
            } else {
                Outcome::Fail {
                    detail: format!(
                        "span mismatch: PCRE2={:?}, RGX={:?}",
                        String::from_utf8_lossy(overall),
                        rgx_match
                    ),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Harness entry point
// ---------------------------------------------------------------------------

fn testdata_path(name: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .unwrap()
        .join("subs/pcre2/testdata")
        .join(name)
}

#[test]
#[ignore = "heavy PCRE2 conformance suite — run with `cargo test --test pcre2_conformance -- --ignored --nocapture`"]
fn pcre2_testinput1_conformance() {
    let testinput = std::fs::read(testdata_path("testinput1"))
        .expect("testinput1 present — did you run `git submodule update --init --recursive`?");
    let testoutput = std::fs::read(testdata_path("testoutput1"))
        .expect("testoutput1 present — did you run `git submodule update --init --recursive`?");

    let cases = parse_cases(&testinput, &testoutput);

    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut skip = 0usize;
    let mut panic_count = 0usize;
    let mut first_failures: Vec<String> = Vec::new();
    let mut first_panics: Vec<String> = Vec::new();

    // Silence the default panic printer: each panic inside `run_case` is
    // caught and reported; the noisy backtrace-style output is
    // distracting for a 1500-case survey.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for case in &cases {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_case(case)))
            .unwrap_or_else(|e| {
                let msg = if let Some(s) = e.downcast_ref::<&'static str>() {
                    (*s).to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "<non-string panic payload>".to_string()
                };
                Outcome::Panic { detail: msg }
            });
        match outcome {
            Outcome::Pass => pass += 1,
            Outcome::Fail { detail } => {
                fail += 1;
                if first_failures.len() < 10 {
                    first_failures.push(format!(
                        "  line {}: /{}/{}: {}",
                        case.line_number, case.pattern, case.modifiers, detail
                    ));
                }
            }
            Outcome::Skip { reason: _ } => skip += 1,
            Outcome::Panic { detail } => {
                panic_count += 1;
                if first_panics.len() < 10 {
                    first_panics.push(format!(
                        "  line {}: /{}/{} on subject {:?}: {}",
                        case.line_number,
                        case.pattern,
                        case.modifiers,
                        String::from_utf8_lossy(&case.subject),
                        detail
                    ));
                }
            }
        }
    }

    std::panic::set_hook(prev_hook);

    let ran = pass + fail;
    let pass_rate = if ran > 0 {
        (pass as f64 / ran as f64) * 100.0
    } else {
        0.0
    };

    eprintln!();
    eprintln!("==== PCRE2 10.47 testinput1 conformance ====");
    eprintln!("parsed cases:  {}", cases.len());
    eprintln!("  pass:        {pass}");
    eprintln!("  fail:        {fail}");
    eprintln!("  panic:       {panic_count}");
    eprintln!("  skip:        {skip}");
    eprintln!("  ran pass-rate: {pass_rate:.1}%");
    eprintln!();
    if !first_panics.is_empty() {
        eprintln!("First {} panics (REAL BUGS):", first_panics.len());
        for p in &first_panics {
            eprintln!("{p}");
        }
        eprintln!();
    }
    if !first_failures.is_empty() {
        eprintln!("First {} failures:", first_failures.len());
        for f in &first_failures {
            eprintln!("{f}");
        }
        eprintln!();
    }

    // First commit: don't fail the test on divergences. The report is the
    // tool; the known-failures baseline comes in a follow-up commit.
    // We DO assert that at least some cases parsed and ran, so the harness
    // itself doesn't silently degrade.
    assert!(ran >= 100, "harness ran too few cases: {ran}");
}
