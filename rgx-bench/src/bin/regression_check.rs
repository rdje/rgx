//! Performance regression check for CI.
//!
//! Runs a fixed set of microbenchmarks (rgx and PCRE2 on the seven
//! shared bench patterns from `rgx-bench::PATTERNS`), compares the
//! observed timings against a checked-in baseline, and exits with
//! code 1 if any **rgx-vs-PCRE2 ratio** has regressed by more than
//! the configured tolerance.
//!
//! The baseline lives in `rgx-bench/baselines/main.toml`. Update it
//! by running this binary with the `--update-baseline` flag, then
//! commit the new file alongside the perf change that justifies it.
//! The check fails the CI job otherwise so silent regressions cannot
//! sneak past.
//!
//! Design choices:
//! - **Instant-based timing.** No criterion subprocess; the
//!   `--update-baseline` and `--check` paths share one timing loop
//!   so they're directly comparable.
//! - **Ratio not absolute time.** Hardware varies (GHA runners are
//!   noisy; Apple Silicon vs cloud x86 differs by 2-3×). The
//!   rgx-vs-PCRE2 ratio cancels out the hardware factor — what
//!   matters is whether rgx is still as fast *relative to PCRE2* as
//!   it was at baseline capture.
//! - **Tolerance is per-bench.** Some benches are noisier than
//!   others (especially `literal_simple` at ~16 ns where a single
//!   cache miss is a measurable %). The default tolerance is 20%;
//!   a stricter per-bench override could be added later if needed.
//!
//! Output format is a Markdown-friendly table so PR comments can
//! quote it directly.

use pcre2::bytes::Regex as PcreRegex;
use rgx_bench::{generate_test_data, PATTERNS};
use rgx_core::Regex;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

/// Number of timing samples per benchmark. Median across samples is
/// used to suppress outliers from GC / scheduler noise.
const SAMPLES: usize = 11;

/// Inner-loop iterations per sample. Sized so each sample takes
/// ~1-5 ms on modern hardware — long enough to amortize timer
/// resolution, short enough that the whole suite finishes in <30 s.
const ITERS_PER_SAMPLE: u64 = 10_000;

/// Default tolerance for rgx-vs-PCRE2 ratio regression. A 20% slack
/// allows for normal cross-run noise on shared CI runners while
/// catching the kind of regression that would have masked the 3.7×
/// `email_basic` gap before today's DFA `\b` work.
const DEFAULT_TOLERANCE_PCT: f64 = 20.0;

#[derive(Clone, Debug)]
struct SampleResult {
    pattern: String,
    kind: BenchKind,
    rgx_ns: f64,
    pcre2_ns: f64,
}

impl SampleResult {
    fn ratio(&self) -> f64 {
        self.rgx_ns / self.pcre2_ns
    }
    fn key(&self) -> String {
        format!("{}/{}", self.kind.as_str(), self.pattern)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BenchKind {
    FindFirst,
}

impl BenchKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::FindFirst => "find_first",
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let update = args.iter().any(|a| a == "--update-baseline");
    let baseline_path: PathBuf = args
        .iter()
        .position(|a| a == "--baseline")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("baselines");
            p.push("main.toml");
            p
        });
    let tolerance_pct: f64 = args
        .iter()
        .position(|a| a == "--tolerance-pct")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_TOLERANCE_PCT);

    let results = run_all_benches();

    if update {
        match write_baseline(&baseline_path, &results) {
            Ok(()) => {
                println!("baseline written to {}", baseline_path.display());
                println!();
                print_table(&results, None, tolerance_pct);
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("failed to write baseline: {e}");
                return ExitCode::from(2);
            }
        }
    }

    let baseline = match read_baseline(&baseline_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("failed to read baseline {}: {e}", baseline_path.display());
            eprintln!("(run with --update-baseline to seed it)");
            return ExitCode::from(2);
        }
    };
    let regressions = print_table(&results, Some(&baseline), tolerance_pct);
    if regressions > 0 {
        eprintln!();
        eprintln!(
            "❌ {regressions} bench{} regressed by >{tolerance_pct}% vs baseline",
            if regressions == 1 { "" } else { "es" }
        );
        ExitCode::from(1)
    } else {
        println!();
        println!("✅ all benches within {tolerance_pct}% of baseline");
        ExitCode::SUCCESS
    }
}

/// Run `find_first` on each shared pattern and time both rgx and
/// PCRE2 against representative input data. Returns the median time
/// per sample for both engines.
fn run_all_benches() -> Vec<SampleResult> {
    let mut out = Vec::new();
    for pattern in PATTERNS {
        // Use a 1 KB input — large enough that the per-call overhead
        // (compile lookup, dispatch chain) is amortized; small
        // enough that the suite finishes quickly.
        let input = generate_test_data(1_024, pattern.pattern);
        let rgx = Regex::compile(pattern.pattern).expect("rgx compile");
        let pcre2 = PcreRegex::new(pattern.pattern).expect("pcre2 compile");
        let rgx_ns = time_find_first_rgx(&rgx, &input);
        let pcre2_ns = time_find_first_pcre2(&pcre2, input.as_bytes());
        out.push(SampleResult {
            pattern: pattern.name.to_string(),
            kind: BenchKind::FindFirst,
            rgx_ns,
            pcre2_ns,
        });
    }
    out
}

fn time_find_first_rgx(re: &Regex, input: &str) -> f64 {
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..ITERS_PER_SAMPLE {
            black_box(re.find_first(black_box(input)));
        }
        samples.push(start.elapsed().as_nanos() as f64 / ITERS_PER_SAMPLE as f64);
    }
    median(&mut samples)
}

fn time_find_first_pcre2(re: &PcreRegex, input: &[u8]) -> f64 {
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..ITERS_PER_SAMPLE {
            black_box(re.find(black_box(input)).ok().flatten());
        }
        samples.push(start.elapsed().as_nanos() as f64 / ITERS_PER_SAMPLE as f64);
    }
    median(&mut samples)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

fn write_baseline(path: &PathBuf, results: &[SampleResult]) -> std::io::Result<()> {
    let mut out = String::new();
    out.push_str("# rgx-vs-PCRE2 benchmark baseline.\n");
    out.push_str("# Regenerate with: cargo run --release -p rgx-bench --bin regression_check -- --update-baseline\n");
    out.push_str("# Times are median ns/op across 11 samples × 10000 iterations.\n\n");
    for r in results {
        out.push_str(&format!(
            "[\"{}\"]\nrgx_ns = {}\npcre2_ns = {}\nratio = {}\n\n",
            r.key(),
            r.rgx_ns,
            r.pcre2_ns,
            r.ratio()
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, out)
}

fn read_baseline(path: &PathBuf) -> std::io::Result<BTreeMap<String, BaselineEntry>> {
    let raw = fs::read_to_string(path)?;
    let mut map = BTreeMap::new();
    let mut current_key: Option<String> = None;
    let mut current_entry = BaselineEntry::default();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            if let Some(k) = current_key.take() {
                map.insert(k, std::mem::take(&mut current_entry));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("[\"").and_then(|s| s.strip_suffix("\"]")) {
            if let Some(k) = current_key.take() {
                map.insert(k, std::mem::take(&mut current_entry));
            }
            current_key = Some(rest.to_string());
        } else if let Some(rhs) = line.strip_prefix("rgx_ns = ") {
            current_entry.rgx_ns = rhs.parse().unwrap_or(0.0);
        } else if let Some(rhs) = line.strip_prefix("pcre2_ns = ") {
            current_entry.pcre2_ns = rhs.parse().unwrap_or(0.0);
        } else if let Some(rhs) = line.strip_prefix("ratio = ") {
            current_entry.ratio = rhs.parse().unwrap_or(0.0);
        }
    }
    if let Some(k) = current_key.take() {
        map.insert(k, current_entry);
    }
    Ok(map)
}

#[derive(Clone, Copy, Debug, Default)]
struct BaselineEntry {
    rgx_ns: f64,
    pcre2_ns: f64,
    ratio: f64,
}

/// Print a Markdown table summarising results and (when a baseline
/// is provided) flagging regressions. Returns the number of
/// regressions detected.
fn print_table(
    results: &[SampleResult],
    baseline: Option<&BTreeMap<String, BaselineEntry>>,
    tolerance_pct: f64,
) -> usize {
    if baseline.is_some() {
        println!("| bench | rgx (ns) | pcre2 (ns) | ratio | baseline ratio | Δ% | status |");
        println!("|---|---:|---:|---:|---:|---:|---|");
    } else {
        println!("| bench | rgx (ns) | pcre2 (ns) | ratio |");
        println!("|---|---:|---:|---:|");
    }
    let mut regressions = 0;
    for r in results {
        let cur_ratio = r.ratio();
        match baseline {
            Some(b) => {
                let entry = b.get(&r.key()).copied().unwrap_or_default();
                let base_ratio = entry.ratio;
                // Worse means higher ratio (rgx slower vs PCRE2).
                let delta_pct = if base_ratio > 0.0 {
                    (cur_ratio - base_ratio) / base_ratio * 100.0
                } else {
                    0.0
                };
                let status = if base_ratio == 0.0 {
                    "—" // missing from baseline
                } else if delta_pct > tolerance_pct {
                    regressions += 1;
                    "❌ regressed"
                } else if delta_pct < -tolerance_pct {
                    "🚀 improved"
                } else {
                    "✅ stable"
                };
                println!(
                    "| {} | {:.0} | {:.0} | {:.2} | {:.2} | {:+.1}% | {} |",
                    r.key(),
                    r.rgx_ns,
                    r.pcre2_ns,
                    cur_ratio,
                    base_ratio,
                    delta_pct,
                    status
                );
            }
            None => {
                println!(
                    "| {} | {:.0} | {:.0} | {:.2} |",
                    r.key(),
                    r.rgx_ns,
                    r.pcre2_ns,
                    cur_ratio
                );
            }
        }
    }
    regressions
}
