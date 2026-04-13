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

/// Parse both files into block-level streams, then pair matching
/// blocks and extract test cases. A "block" is a consecutive run of
/// non-blank non-comment lines delimited by blank lines — the natural
/// unit of a PCRE2 test file. Multi-line patterns become one block
/// with multiple lines; a single-line pattern with subjects is one
/// block; directives and comments live in their own blocks we skip.
///
/// Block-based pairing is robust against most cursor-sync bugs the
/// previous line-by-line parser had: as long as the blank-line
/// separators line up (which they always do in PCRE2 testdata), the
/// block indices stay in lockstep even if individual block contents
/// differ in length between the two files.
fn parse_cases(testinput: &[u8], testoutput: &[u8]) -> Vec<TestCase> {
    let in_blocks = split_into_blocks(testinput);
    let out_blocks = split_into_blocks(testoutput);

    let mut cases = Vec::new();
    let mut in_skip = false; // set by #if ebcdic / cleared by #endif

    // Pair blocks by pattern-block index. Directive and comment
    // blocks appear in matching positions in both files, so we walk
    // them in lockstep.
    let mut oi = 0;
    for ib in &in_blocks {
        // Advance the output cursor to the next block that pairs with
        // this input block. For non-pattern blocks (comments /
        // directives) both files have matching blocks at the same
        // positions, so a pure index walk works.
        let ob = out_blocks.get(oi);

        let kind = classify_block(&ib.lines);
        if let BlockKind::Directive(directive) = kind {
            if let Some(cond) = directive.strip_prefix("#if ") {
                in_skip = matches!(cond.trim(), "ebcdic");
            } else if directive.trim() == "#endif" {
                in_skip = false;
            }
            oi += 1;
            continue;
        }
        if matches!(kind, BlockKind::Comment) {
            oi += 1;
            continue;
        }
        if in_skip {
            oi += 1;
            continue;
        }

        let Some(ob) = ob else { break };

        match kind {
            BlockKind::Pattern => {
                cases.extend(extract_pattern_cases(ib, &ob.lines));
                oi += 1;
            }
            BlockKind::Directive(_) | BlockKind::Comment => unreachable!(),
        }
    }

    cases
}

#[derive(Debug)]
enum BlockKind<'a> {
    /// Pattern block: first line starts with `/`, possibly spans
    /// multiple lines if the pattern is multi-line, followed by
    /// subject lines (indented) and optional `\=` annotations.
    Pattern,
    /// `#...` directive like `#if !ebcdic` / `#endif` / `#forbid_utf`.
    Directive(&'a str),
    /// Pure `#` comment block (PCRE2 testfiles use leading `#` for
    /// comments outside #if/#endif blocks).
    Comment,
}

fn classify_block<'a>(block: &'a [&[u8]]) -> BlockKind<'a> {
    if let Some(first) = block.first() {
        if first.starts_with(b"/") {
            return BlockKind::Pattern;
        }
        if first.starts_with(b"#") {
            let s = std::str::from_utf8(first).unwrap_or("");
            if s.starts_with("#if ")
                || s.trim() == "#endif"
                || s.starts_with("#perltest")
                || s.starts_with("#forbid_utf")
                || s.starts_with("#newline_default")
                || s.starts_with("#pattern")
                || s.starts_with("#subject")
            {
                return BlockKind::Directive(s);
            }
            return BlockKind::Comment;
        }
    }
    BlockKind::Comment
}

/// Split a testfile into blocks separated by blank lines. Trailing
/// `\r` is stripped from each line. Empty blocks (consecutive blank
/// lines) are dropped. Lines containing ONLY whitespace (spaces or
/// tabs) are treated as block separators — pcre2test uses those
/// interchangeably with truly-empty lines.
fn split_into_blocks(bytes: &[u8]) -> Vec<Block> {
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
                start_line = idx + 1; // 1-based
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

/// A parsed PCRE2 testfile block. `start_line` is the 1-based input
/// line number of the block's first content line — useful for pointing
/// a failure back at the source file.
struct Block<'a> {
    lines: Vec<&'a [u8]>,
    start_line: usize,
}

fn is_blank(line: &[u8]) -> bool {
    line.iter().all(|&b| b == b' ' || b == b'\t')
}

/// Extract test cases from a paired pattern block. Returns an empty
/// list if the pattern is multi-line (not currently supported) or if
/// the block uses modifiers we don't model. Single-line patterns
/// with any number of subjects produce one case per subject.
fn extract_pattern_cases(ib: &Block, ob: &[&[u8]]) -> Vec<TestCase> {
    let pattern_line_number = ib.start_line;
    let Some(pattern_line) = ib.lines.first().copied() else {
        return Vec::new();
    };
    // Only handle single-line patterns for this harness pass. Multi-
    // line patterns require a dedicated parser that concatenates all
    // pre-modifier lines; deferred work.
    if !is_complete_pattern_line(pattern_line) {
        return Vec::new();
    }
    let Some((pattern_bytes, modifiers_bytes)) = split_pattern_line(pattern_line) else {
        return Vec::new();
    };
    let Ok(pattern) = std::str::from_utf8(pattern_bytes) else {
        return Vec::new();
    };
    let pattern = pattern.to_string();
    let full_modifiers = String::from_utf8_lossy(modifiers_bytes).to_string();
    let modifiers = extract_short_modifiers(&full_modifiers);

    // Walk input subjects in order, tracking `\=` annotations between
    // them. Output lines are walked forward through `ob` too, with
    // `\=` annotation echos skipped (pcre2test echoes the annotation
    // line verbatim).
    let mut cases = Vec::new();
    let mut expect_no_match = false;
    let mut oi = 1; // ob[0] is the pattern echo; subject echos start at 1

    for iline in ib.lines.iter().skip(1) {
        let trimmed = trim_leading_spaces(iline);
        if trimmed.starts_with(b"\\=") {
            let annotation = String::from_utf8_lossy(&trimmed[2..]);
            if annotation.trim().starts_with("Expect no match") {
                expect_no_match = true;
            }
            // Skip the annotation echo in output if present.
            while oi < ob.len() && trim_leading_spaces(ob[oi]).starts_with(b"\\=") {
                oi += 1;
            }
            continue;
        }
        let Some(subject) = decode_subject(trimmed) else {
            continue;
        };

        // Read this subject's expected output from ob[oi..].
        let (expected, consumed) = parse_subject_output(ob, oi);
        oi += consumed;

        cases.push(TestCase {
            pattern: pattern.clone(),
            modifiers: modifiers.clone(),
            full_modifiers: full_modifiers.clone(),
            subject,
            expected,
            expect_no_match_annotation: expect_no_match,
            line_number: pattern_line_number,
        });
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

/// Decode a PCRE2 testoutput match-line. Narrower than
/// [`decode_subject`]: output lines only escape non-printable bytes
/// as `\xHH` / `\x{...}` and a literal backslash as `\\`. Everything
/// else — including `\?`, `\=`, `\$` — appears in the output as
/// literal text (a backslash byte followed by the character).
fn decode_output(line: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if line[i] != b'\\' {
            out.push(line[i]);
            i += 1;
            continue;
        }
        if i + 1 >= line.len() {
            out.push(b'\\');
            i += 1;
            continue;
        }
        match line[i + 1] {
            b'\\' => {
                out.push(b'\\');
                i += 2;
            }
            b'x' => {
                if i + 2 < line.len() && line[i + 2] == b'{' {
                    let mut j = i + 3;
                    while j < line.len() && line[j] != b'}' {
                        j += 1;
                    }
                    if j >= line.len() {
                        return None;
                    }
                    let hex = std::str::from_utf8(&line[i + 3..j]).ok()?;
                    let cp = u32::from_str_radix(hex, 16).ok()?;
                    if cp <= 0xFF {
                        out.push(cp as u8);
                    } else {
                        let c = char::from_u32(cp)?;
                        let mut buf = [0u8; 4];
                        let s = c.encode_utf8(&mut buf);
                        out.extend_from_slice(s.as_bytes());
                    }
                    i = j + 1;
                } else {
                    if i + 3 >= line.len() {
                        return None;
                    }
                    let hex = std::str::from_utf8(&line[i + 2..i + 4]).ok()?;
                    let b = u8::from_str_radix(hex, 16).ok()?;
                    out.push(b);
                    i += 4;
                }
            }
            // `\<anything else>` in output = literal `\` + literal char.
            // This is the intentional contract: PCRE2 output uses `\\`
            // for backslash, so ANY other `\x` sequence is NOT an
            // escape — we emit both bytes verbatim.
            _ => {
                out.push(b'\\');
                i += 1;
            }
        }
    }
    Some(out)
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
            // Trailing lone backslash. PCRE2 testinput convention: a
            // backslash at the end of a subject line suppresses the
            // implicit newline — effectively "subject ends here
            // without adding a newline". For our single-line subjects
            // (which don't carry a trailing newline anyway), this
            // translates to "ignore the trailing backslash". Used by
            // tests like `/^$/` against `    \` to mean empty subject.
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
    // `\=` annotation lines are consumed by the outer loop in
    // `parse_cases`, so here we only expect a subject echo.
    if start < out_lines.len() {
        let l = out_lines[start];
        if l.starts_with(b"    ") {
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
            // PCRE2's pcre2test output escapes ONLY non-printable bytes
            // as `\xHH` / `\x{H..H}` and a literal backslash as `\\` —
            // everything else is printed as-is. `\?` in output is NOT
            // an escape for `?`; it's a literal backslash followed by
            // a literal question mark. Use `decode_output` which is
            // intentionally narrower than `decode_subject`.
            let body = trimmed.trim_start_matches("0:").trim_start();
            overall = decode_output(body.as_bytes());
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
    // Histogram of failure categories → (count, first-example line number)
    let mut category_counts: std::collections::BTreeMap<&'static str, (usize, usize, String)> =
        std::collections::BTreeMap::new();
    let mut categorize = |cat: &'static str, case: &TestCase, detail: &str| {
        let entry = category_counts
            .entry(cat)
            .or_insert_with(|| (0, case.line_number, String::new()));
        entry.0 += 1;
        if entry.2.is_empty() {
            entry.2 = format!(
                "line {}: /{}/{} — {}",
                case.line_number, case.pattern, case.modifiers, detail
            );
        }
    };

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
                let cat = classify_failure(&detail);
                categorize(cat, case, &detail);
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
    if !category_counts.is_empty() {
        eprintln!("Failure histogram (sorted by count):");
        let mut buckets: Vec<_> = category_counts.iter().collect();
        buckets.sort_by_key(|(_, (count, _, _))| std::cmp::Reverse(*count));
        for (cat, (count, _line, example)) in buckets {
            eprintln!("  {count:>5}  {cat}");
            eprintln!("         first: {example}");
        }
        eprintln!();
    }
    if !first_failures.is_empty() {
        eprintln!("First {} failure examples (raw):", first_failures.len());
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

/// Classify a failure `detail` string into a bucket name for the
/// histogram. Buckets are deliberately coarse — we want the top few
/// categories to point clearly at a single bug or gap to investigate.
fn classify_failure(detail: &str) -> &'static str {
    // Compile errors dominate, so split them by sub-cause.
    if detail.starts_with("compile error:") {
        if detail.contains("unrecognized simple_escape") {
            return "compile: PGEN rejects simple escape (\\\" \\/ etc)";
        }
        if detail.contains("class_escape resolved to unsupported variant") {
            return "compile: class_escape unsupported variant (e.g. [\\b] [\\c])";
        }
        if detail.contains("unterminated character class") {
            return "compile: unterminated char class (likely \\c[ form)";
        }
        if detail.contains("E_PARSE_FAILURE") {
            return "compile: PGEN parse failure (other)";
        }
        if detail.contains("pgen AST contract mismatch") {
            return "compile: PGEN AST contract mismatch (other)";
        }
        return "compile: other error";
    }
    if detail.starts_with("span mismatch") {
        return "span mismatch (semantic divergence)";
    }
    if detail.starts_with("PCRE2 expected no match, RGX matched") {
        return "false positive (RGX matches where PCRE2 doesn't)";
    }
    if detail.starts_with("PCRE2 expected match") && detail.contains("RGX no match") {
        return "false negative (RGX misses a match PCRE2 finds)";
    }
    "other"
}
