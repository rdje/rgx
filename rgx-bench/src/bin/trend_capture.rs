use pcre2::bytes::Regex as PcreRegex;
use rgx_bench::{generate_test_data, BenchmarkPattern, PATTERNS};
use rgx_core::Regex;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SEARCH_PATTERN_NAMES: &[&str] = &["literal_simple", "email_basic", "capture_groups"];
const SEARCH_INPUT_SIZES: &[usize] = &[1_000, 10_000];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureMode {
    Quick,
    Full,
}

impl CaptureMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Full => "full",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "quick" => Some(Self::Quick),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum BenchmarkKind {
    Compile,
    FindFirst,
    FindAll,
}

impl BenchmarkKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Compile => "compile",
            Self::FindFirst => "find_first",
            Self::FindAll => "find_all",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "compile" => Some(Self::Compile),
            "find_first" => Some(Self::FindFirst),
            "find_all" => Some(Self::FindAll),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct CliOptions {
    mode: CaptureMode,
    output_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct TrendSample {
    kind: BenchmarkKind,
    pattern_name: String,
    description: String,
    input_size: Option<usize>,
    rgx_ns_per_iter: f64,
    pcre2_ns_per_iter: f64,
}

impl TrendSample {
    fn ratio_rgx_over_pcre2(&self) -> f64 {
        self.rgx_ns_per_iter / self.pcre2_ns_per_iter
    }

    fn ratio_label(&self) -> String {
        let ratio = self.ratio_rgx_over_pcre2();
        if ratio < 1.0 {
            format!("{:.2}x faster", 1.0 / ratio)
        } else {
            format!("{ratio:.2}x slower")
        }
    }

    fn input_label(&self) -> String {
        self.input_size
            .map(|size| size.to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    fn key(&self) -> SampleKey {
        SampleKey {
            kind: self.kind,
            pattern_name: self.pattern_name.clone(),
            input_size: self.input_size,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SampleKey {
    kind: BenchmarkKind,
    pattern_name: String,
    input_size: Option<usize>,
}

#[derive(Debug)]
struct HistoricalCapture {
    generated_at_unix: u64,
    samples: Vec<TrendSample>,
}

#[derive(Debug, Clone)]
struct ComparisonSample {
    current: TrendSample,
    previous: TrendSample,
}

impl ComparisonSample {
    fn ratio_change_fraction(&self) -> f64 {
        (self.current.ratio_rgx_over_pcre2() / self.previous.ratio_rgx_over_pcre2()) - 1.0
    }

    fn ratio_change_label(&self) -> String {
        format_change_label(self.ratio_change_fraction())
    }
}

fn main() -> Result<(), String> {
    let options = parse_args()?;
    let generated_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("system clock error: {err}"))?
        .as_secs();
    let samples = collect_samples(options.mode)?;

    fs::create_dir_all(&options.output_dir).map_err(|err| {
        format!(
            "failed to create benchmark trend output directory {}: {err}",
            options.output_dir.display()
        )
    })?;
    let history_dir = options.output_dir.join("history");
    fs::create_dir_all(&history_dir).map_err(|err| {
        format!(
            "failed to create benchmark trend history directory {}: {err}",
            history_dir.display()
        )
    })?;

    let previous_capture = load_most_recent_historical_capture(&history_dir)?;

    let markdown = render_markdown(
        &samples,
        options.mode,
        generated_at_unix,
        previous_capture.as_ref(),
    );
    let tsv = render_tsv(&samples);

    let markdown_path = options.output_dir.join("latest.md");
    fs::write(&markdown_path, markdown.as_bytes()).map_err(|err| {
        format!(
            "failed to write markdown benchmark summary {}: {err}",
            markdown_path.display()
        )
    })?;

    let tsv_path = options.output_dir.join("latest.tsv");
    fs::write(&tsv_path, tsv.as_bytes()).map_err(|err| {
        format!(
            "failed to write tabular benchmark summary {}: {err}",
            tsv_path.display()
        )
    })?;

    let history_markdown_path = history_dir.join(format!("{generated_at_unix}.md"));
    fs::write(&history_markdown_path, markdown.as_bytes()).map_err(|err| {
        format!(
            "failed to write archived markdown benchmark summary {}: {err}",
            history_markdown_path.display()
        )
    })?;

    let history_tsv_path = history_dir.join(format!("{generated_at_unix}.tsv"));
    fs::write(&history_tsv_path, tsv.as_bytes()).map_err(|err| {
        format!(
            "failed to write archived tabular benchmark summary {}: {err}",
            history_tsv_path.display()
        )
    })?;

    println!(
        "[trend_capture] Wrote benchmark trend summary to {} and {}",
        markdown_path.display(),
        tsv_path.display()
    );
    println!(
        "[trend_capture] Archived benchmark snapshot to {} and {}",
        history_markdown_path.display(),
        history_tsv_path.display()
    );
    println!();
    println!("{markdown}");

    Ok(())
}

fn parse_args() -> Result<CliOptions, String> {
    let mut mode = CaptureMode::Quick;
    let mut output_dir = PathBuf::from("target/benchmark-trends");

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--mode requires `quick` or `full`".to_string())?;
                mode = CaptureMode::from_str(&value)
                    .ok_or_else(|| format!("unsupported benchmark trend mode: {value}"))?;
            }
            "--output-dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--output-dir requires a path".to_string())?;
                output_dir = PathBuf::from(value);
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                return Err(format!("unsupported argument: {other}"));
            }
        }
    }

    Ok(CliOptions { mode, output_dir })
}

fn print_usage() {
    println!("trend_capture --mode <quick|full> --output-dir <path>");
}

fn collect_samples(mode: CaptureMode) -> Result<Vec<TrendSample>, String> {
    let selected_patterns = PATTERNS
        .iter()
        .filter(|pattern| SEARCH_PATTERN_NAMES.contains(&pattern.name))
        .collect::<Vec<_>>();

    let mut samples = Vec::new();

    for pattern in &selected_patterns {
        samples.push(measure_compile_case(pattern, mode)?);
    }

    for pattern in &selected_patterns {
        for &input_size in SEARCH_INPUT_SIZES {
            let test_data = generate_test_data(input_size, pattern.pattern);
            samples.push(measure_find_first_case(
                pattern, &test_data, input_size, mode,
            )?);
            samples.push(measure_find_all_case(
                pattern, &test_data, input_size, mode,
            )?);
        }
    }

    Ok(samples)
}

fn measure_compile_case(
    pattern: &BenchmarkPattern,
    mode: CaptureMode,
) -> Result<TrendSample, String> {
    let iterations = match mode {
        CaptureMode::Quick => 200,
        CaptureMode::Full => 1_000,
    };
    let repeats = match mode {
        CaptureMode::Quick => 5,
        CaptureMode::Full => 7,
    };

    let rgx_ns_per_iter = measure_ns_per_iter(iterations, repeats, || {
        let regex = Regex::compile(pattern.pattern).expect("rgx compile benchmark should compile");
        black_box(regex);
    });
    let pcre2_ns_per_iter = measure_ns_per_iter(iterations, repeats, || {
        let regex =
            PcreRegex::new(pattern.pattern).expect("pcre2 compile benchmark should compile");
        black_box(regex);
    });

    Ok(TrendSample {
        kind: BenchmarkKind::Compile,
        pattern_name: pattern.name.to_string(),
        description: pattern.description.to_string(),
        input_size: None,
        rgx_ns_per_iter,
        pcre2_ns_per_iter,
    })
}

fn measure_find_first_case(
    pattern: &BenchmarkPattern,
    test_data: &str,
    input_size: usize,
    mode: CaptureMode,
) -> Result<TrendSample, String> {
    let rgx_regex = Regex::compile(pattern.pattern)
        .map_err(|err| format!("rgx compile failed for {}: {err}", pattern.name))?;
    let pcre2_regex = PcreRegex::new(pattern.pattern)
        .map_err(|err| format!("pcre2 compile failed for {}: {err}", pattern.name))?;

    let iterations = search_iterations(BenchmarkKind::FindFirst, input_size, mode);
    let repeats = search_repeats(mode);

    let rgx_ns_per_iter = measure_ns_per_iter(iterations, repeats, || {
        black_box(
            rgx_regex
                .find_first(test_data)
                .map(|matched| (matched.start, matched.end)),
        );
    });
    let pcre2_ns_per_iter = measure_ns_per_iter(iterations, repeats, || {
        black_box(
            pcre2_regex
                .find(test_data.as_bytes())
                .expect("pcre2 find_first benchmark should succeed")
                .map(|matched| (matched.start(), matched.end())),
        );
    });

    Ok(TrendSample {
        kind: BenchmarkKind::FindFirst,
        pattern_name: pattern.name.to_string(),
        description: pattern.description.to_string(),
        input_size: Some(input_size),
        rgx_ns_per_iter,
        pcre2_ns_per_iter,
    })
}

fn measure_find_all_case(
    pattern: &BenchmarkPattern,
    test_data: &str,
    input_size: usize,
    mode: CaptureMode,
) -> Result<TrendSample, String> {
    let rgx_regex = Regex::compile(pattern.pattern)
        .map_err(|err| format!("rgx compile failed for {}: {err}", pattern.name))?;
    let pcre2_regex = PcreRegex::new(pattern.pattern)
        .map_err(|err| format!("pcre2 compile failed for {}: {err}", pattern.name))?;

    let iterations = search_iterations(BenchmarkKind::FindAll, input_size, mode);
    let repeats = search_repeats(mode);

    let rgx_ns_per_iter = measure_ns_per_iter(iterations, repeats, || {
        black_box(rgx_regex.find_all(test_data).len());
    });
    let pcre2_ns_per_iter = measure_ns_per_iter(iterations, repeats, || {
        black_box(pcre2_regex.find_iter(test_data.as_bytes()).count());
    });

    Ok(TrendSample {
        kind: BenchmarkKind::FindAll,
        pattern_name: pattern.name.to_string(),
        description: pattern.description.to_string(),
        input_size: Some(input_size),
        rgx_ns_per_iter,
        pcre2_ns_per_iter,
    })
}

fn search_iterations(kind: BenchmarkKind, input_size: usize, mode: CaptureMode) -> u32 {
    match (kind, input_size, mode) {
        (BenchmarkKind::FindFirst, 1_000, CaptureMode::Quick) => 250,
        (BenchmarkKind::FindFirst, 10_000, CaptureMode::Quick) => 100,
        (BenchmarkKind::FindAll, 1_000, CaptureMode::Quick) => 100,
        (BenchmarkKind::FindAll, 10_000, CaptureMode::Quick) => 40,
        (BenchmarkKind::FindFirst, 1_000, CaptureMode::Full) => 1_000,
        (BenchmarkKind::FindFirst, 10_000, CaptureMode::Full) => 400,
        (BenchmarkKind::FindAll, 1_000, CaptureMode::Full) => 400,
        (BenchmarkKind::FindAll, 10_000, CaptureMode::Full) => 150,
        (BenchmarkKind::Compile, _, _) => unreachable!("compile uses dedicated iteration table"),
        _ => 50,
    }
}

fn search_repeats(mode: CaptureMode) -> usize {
    match mode {
        CaptureMode::Quick => 5,
        CaptureMode::Full => 7,
    }
}

fn measure_ns_per_iter<F>(iterations: u32, repeats: usize, mut operation: F) -> f64
where
    F: FnMut(),
{
    operation();

    let mut samples = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        let start = Instant::now();
        for _ in 0..iterations {
            operation();
        }
        let elapsed = start.elapsed();
        samples.push(elapsed.as_secs_f64() * 1_000_000_000.0 / f64::from(iterations));
    }

    median(&mut samples)
}

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    let mid = samples.len() / 2;
    if samples.len() % 2 == 0 {
        (samples[mid - 1] + samples[mid]) / 2.0
    } else {
        samples[mid]
    }
}

fn load_most_recent_historical_capture(
    history_dir: &Path,
) -> Result<Option<HistoricalCapture>, String> {
    if !history_dir.exists() {
        return Ok(None);
    }

    let entries = fs::read_dir(history_dir).map_err(|err| {
        format!(
            "failed to read benchmark trend history directory {}: {err}",
            history_dir.display()
        )
    })?;

    let mut latest_capture = None;
    for entry in entries {
        let entry = entry.map_err(|err| {
            format!(
                "failed to read an entry from benchmark trend history directory {}: {err}",
                history_dir.display()
            )
        })?;
        if let Some(candidate) = parse_history_entry(entry)? {
            if latest_capture
                .as_ref()
                .map(|capture: &HistoricalCapture| {
                    capture.generated_at_unix < candidate.generated_at_unix
                })
                .unwrap_or(true)
            {
                latest_capture = Some(candidate);
            }
        }
    }

    Ok(latest_capture)
}

fn parse_history_entry(entry: fs::DirEntry) -> Result<Option<HistoricalCapture>, String> {
    let path = entry.path();
    if path.extension().and_then(|ext| ext.to_str()) != Some("tsv") {
        return Ok(None);
    }

    let generated_at_unix = match path.file_stem().and_then(|stem| stem.to_str()) {
        Some(value) => value.parse::<u64>().map_err(|err| {
            format!(
                "failed to parse benchmark trend history filename {} as unix timestamp: {err}",
                path.display()
            )
        })?,
        None => {
            return Err(format!(
                "failed to read benchmark trend history filename for {}",
                path.display()
            ))
        }
    };

    let raw = fs::read_to_string(&path).map_err(|err| {
        format!(
            "failed to read benchmark trend history snapshot {}: {err}",
            path.display()
        )
    })?;
    let samples = parse_tsv(&raw)?;

    Ok(Some(HistoricalCapture {
        generated_at_unix,
        samples,
    }))
}

fn parse_tsv(raw: &str) -> Result<Vec<TrendSample>, String> {
    let mut lines = raw.lines();
    let Some(header) = lines.next() else {
        return Err("benchmark trend tsv was empty".to_string());
    };
    let expected_header =
        "kind\tpattern\tinput_size\trgx_ns_per_iter\tpcre2_ns_per_iter\trgx_over_pcre2\tdescription";
    if header != expected_header {
        return Err(format!(
            "benchmark trend tsv header mismatch: expected `{expected_header}`, found `{header}`"
        ));
    }

    let mut samples = Vec::new();
    for (line_index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        if columns.len() != 7 {
            return Err(format!(
                "benchmark trend tsv line {} expected 7 columns, found {}",
                line_index + 2,
                columns.len()
            ));
        }
        let kind = BenchmarkKind::from_str(columns[0]).ok_or_else(|| {
            format!(
                "benchmark trend tsv line {} has unknown benchmark kind `{}`",
                line_index + 2,
                columns[0]
            )
        })?;
        let input_size = if columns[2] == "-" {
            None
        } else {
            Some(columns[2].parse::<usize>().map_err(|err| {
                format!(
                    "benchmark trend tsv line {} has invalid input size `{}`: {err}",
                    line_index + 2,
                    columns[2]
                )
            })?)
        };
        let rgx_ns_per_iter = columns[3].parse::<f64>().map_err(|err| {
            format!(
                "benchmark trend tsv line {} has invalid rgx ns/iter `{}`: {err}",
                line_index + 2,
                columns[3]
            )
        })?;
        let pcre2_ns_per_iter = columns[4].parse::<f64>().map_err(|err| {
            format!(
                "benchmark trend tsv line {} has invalid pcre2 ns/iter `{}`: {err}",
                line_index + 2,
                columns[4]
            )
        })?;
        samples.push(TrendSample {
            kind,
            pattern_name: columns[1].to_string(),
            description: columns[6].to_string(),
            input_size,
            rgx_ns_per_iter,
            pcre2_ns_per_iter,
        });
    }

    Ok(samples)
}

fn render_markdown(
    samples: &[TrendSample],
    mode: CaptureMode,
    generated_at_unix: u64,
    previous_capture: Option<&HistoricalCapture>,
) -> String {
    let mut out = String::new();
    writeln!(&mut out, "# Benchmark Trend Capture").ok();
    writeln!(&mut out).ok();
    writeln!(&mut out, "- Mode: `{}`", mode.as_str()).ok();
    writeln!(&mut out, "- Generated at (unix): `{generated_at_unix}`").ok();
    writeln!(&mut out, "- Samples: `{}`", samples.len()).ok();
    match previous_capture {
        Some(capture) => {
            writeln!(
                &mut out,
                "- Previous capture: `{}`",
                capture.generated_at_unix
            )
            .ok();
        }
        None => {
            writeln!(&mut out, "- Previous capture: `none`").ok();
        }
    }
    writeln!(&mut out).ok();
    writeln!(&mut out, "## Aggregate Ratios").ok();
    for kind in [
        BenchmarkKind::Compile,
        BenchmarkKind::FindFirst,
        BenchmarkKind::FindAll,
    ] {
        let mut ratios = samples
            .iter()
            .filter(|sample| sample.kind == kind)
            .map(TrendSample::ratio_rgx_over_pcre2)
            .collect::<Vec<_>>();
        let ratio = median(&mut ratios);
        let summary = if ratio < 1.0 {
            format!("{:.2}x faster median", 1.0 / ratio)
        } else {
            format!("{ratio:.2}x slower median")
        };
        writeln!(&mut out, "- `{}`: {summary}", kind.as_str()).ok();
    }
    writeln!(&mut out).ok();
    render_comparison_markdown(&mut out, samples, previous_capture);
    writeln!(&mut out).ok();
    writeln!(
        &mut out,
        "| Kind | Pattern | Input Size | RGX ns/iter | PCRE2 ns/iter | RGX vs PCRE2 |"
    )
    .ok();
    writeln!(&mut out, "| --- | --- | ---: | ---: | ---: | --- |").ok();
    for sample in samples {
        writeln!(
            &mut out,
            "| {} | {} | {} | {:.1} | {:.1} | {} |",
            sample.kind.as_str(),
            sample.pattern_name,
            sample.input_label(),
            sample.rgx_ns_per_iter,
            sample.pcre2_ns_per_iter,
            sample.ratio_label()
        )
        .ok();
    }
    writeln!(&mut out).ok();
    writeln!(&mut out, "## Pattern Notes").ok();
    for pattern_name in SEARCH_PATTERN_NAMES {
        if let Some(pattern) = PATTERNS
            .iter()
            .find(|pattern| pattern.name == *pattern_name)
        {
            writeln!(&mut out, "- `{}`: {}", pattern.name, pattern.description).ok();
        }
    }
    out
}

fn render_tsv(samples: &[TrendSample]) -> String {
    let mut out = String::new();
    writeln!(
        &mut out,
        "kind\tpattern\tinput_size\trgx_ns_per_iter\tpcre2_ns_per_iter\trgx_over_pcre2\tdescription"
    )
    .ok();
    for sample in samples {
        writeln!(
            &mut out,
            "{}\t{}\t{}\t{:.4}\t{:.4}\t{:.6}\t{}",
            sample.kind.as_str(),
            sample.pattern_name,
            sample.input_label(),
            sample.rgx_ns_per_iter,
            sample.pcre2_ns_per_iter,
            sample.ratio_rgx_over_pcre2(),
            sample.description
        )
        .ok();
    }
    out
}

fn render_comparison_markdown(
    out: &mut String,
    current_samples: &[TrendSample],
    previous_capture: Option<&HistoricalCapture>,
) {
    writeln!(out, "## Delta vs Previous Capture").ok();
    match previous_capture {
        None => {
            writeln!(out, "- No prior archived benchmark capture was available.").ok();
        }
        Some(previous_capture) => {
            let comparisons = compare_samples(current_samples, &previous_capture.samples);
            if comparisons.is_empty() {
                writeln!(
                    out,
                    "- Previous capture `{}` did not share any comparable benchmark rows with the current capture.",
                    previous_capture.generated_at_unix
                )
                .ok();
                return;
            }

            writeln!(
                out,
                "- Comparing against archived capture `{}`.",
                previous_capture.generated_at_unix
            )
            .ok();
            writeln!(out).ok();
            writeln!(out, "### Median Ratio Change By Kind").ok();
            for kind in [
                BenchmarkKind::Compile,
                BenchmarkKind::FindFirst,
                BenchmarkKind::FindAll,
            ] {
                let mut changes = comparisons
                    .iter()
                    .filter(|comparison| comparison.current.kind == kind)
                    .map(ComparisonSample::ratio_change_fraction)
                    .collect::<Vec<_>>();
                if changes.is_empty() {
                    continue;
                }
                let change = median(&mut changes);
                writeln!(
                    out,
                    "- `{}`: {}",
                    kind.as_str(),
                    format_change_label(change)
                )
                .ok();
            }
            writeln!(out).ok();

            let mut regressions = comparisons
                .iter()
                .filter(|comparison| comparison.ratio_change_fraction() > 0.0)
                .collect::<Vec<_>>();
            regressions.sort_by(|left, right| {
                right
                    .ratio_change_fraction()
                    .total_cmp(&left.ratio_change_fraction())
            });

            let mut improvements = comparisons
                .iter()
                .filter(|comparison| comparison.ratio_change_fraction() < 0.0)
                .collect::<Vec<_>>();
            improvements.sort_by(|left, right| {
                left.ratio_change_fraction()
                    .total_cmp(&right.ratio_change_fraction())
            });

            writeln!(out, "### Biggest Regressions").ok();
            if regressions.is_empty() {
                writeln!(out, "- None").ok();
            } else {
                for comparison in regressions.into_iter().take(3) {
                    writeln!(
                        out,
                        "- `{}` / `{}` / `{}`: {} ({} -> {})",
                        comparison.current.kind.as_str(),
                        comparison.current.pattern_name,
                        comparison.current.input_label(),
                        comparison.ratio_change_label(),
                        comparison.previous.ratio_label(),
                        comparison.current.ratio_label()
                    )
                    .ok();
                }
            }
            writeln!(out).ok();

            writeln!(out, "### Biggest Improvements").ok();
            if improvements.is_empty() {
                writeln!(out, "- None").ok();
            } else {
                for comparison in improvements.into_iter().take(3) {
                    writeln!(
                        out,
                        "- `{}` / `{}` / `{}`: {} ({} -> {})",
                        comparison.current.kind.as_str(),
                        comparison.current.pattern_name,
                        comparison.current.input_label(),
                        comparison.ratio_change_label(),
                        comparison.previous.ratio_label(),
                        comparison.current.ratio_label()
                    )
                    .ok();
                }
            }
        }
    }
}

fn compare_samples(
    current_samples: &[TrendSample],
    previous_samples: &[TrendSample],
) -> Vec<ComparisonSample> {
    let previous_by_key = previous_samples
        .iter()
        .cloned()
        .map(|sample| (sample.key(), sample))
        .collect::<std::collections::BTreeMap<_, _>>();

    current_samples
        .iter()
        .filter_map(|sample| {
            previous_by_key
                .get(&sample.key())
                .cloned()
                .map(|previous| ComparisonSample {
                    current: sample.clone(),
                    previous,
                })
        })
        .collect()
}

fn format_change_label(change_fraction: f64) -> String {
    if change_fraction.abs() < 0.0001 {
        "flat".to_string()
    } else if change_fraction < 0.0 {
        format!("{:.2}% improvement", change_fraction.abs() * 100.0)
    } else {
        format!("{:.2}% regression", change_fraction * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(
        kind: BenchmarkKind,
        pattern_name: &str,
        input_size: Option<usize>,
        rgx_ns_per_iter: f64,
        pcre2_ns_per_iter: f64,
    ) -> TrendSample {
        TrendSample {
            kind,
            pattern_name: pattern_name.to_string(),
            description: format!("desc:{pattern_name}"),
            input_size,
            rgx_ns_per_iter,
            pcre2_ns_per_iter,
        }
    }

    #[test]
    fn render_tsv_round_trips_through_parser() {
        let samples = vec![
            sample(BenchmarkKind::Compile, "literal_simple", None, 10.0, 5.0),
            sample(
                BenchmarkKind::FindFirst,
                "email_basic",
                Some(1000),
                25.0,
                10.0,
            ),
        ];

        let parsed = parse_tsv(&render_tsv(&samples)).expect("rendered tsv should parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].kind, BenchmarkKind::Compile);
        assert_eq!(parsed[0].pattern_name, "literal_simple");
        assert_eq!(parsed[0].input_size, None);
        assert!((parsed[0].rgx_ns_per_iter - 10.0).abs() < 0.0001);
        assert_eq!(parsed[1].kind, BenchmarkKind::FindFirst);
        assert_eq!(parsed[1].pattern_name, "email_basic");
        assert_eq!(parsed[1].input_size, Some(1000));
        assert!((parsed[1].pcre2_ns_per_iter - 10.0).abs() < 0.0001);
    }

    #[test]
    fn render_markdown_reports_previous_capture_deltas() {
        let current = vec![
            sample(BenchmarkKind::Compile, "literal_simple", None, 12.0, 6.0),
            sample(
                BenchmarkKind::FindFirst,
                "email_basic",
                Some(1000),
                18.0,
                9.0,
            ),
            sample(
                BenchmarkKind::FindAll,
                "capture_groups",
                Some(1000),
                30.0,
                10.0,
            ),
        ];
        let previous = HistoricalCapture {
            generated_at_unix: 1700000000,
            samples: vec![
                sample(BenchmarkKind::Compile, "literal_simple", None, 10.0, 5.0),
                sample(
                    BenchmarkKind::FindFirst,
                    "email_basic",
                    Some(1000),
                    30.0,
                    10.0,
                ),
                sample(
                    BenchmarkKind::FindAll,
                    "capture_groups",
                    Some(1000),
                    25.0,
                    10.0,
                ),
            ],
        };

        let markdown = render_markdown(&current, CaptureMode::Quick, 1800000000, Some(&previous));
        assert!(markdown.contains("## Delta vs Previous Capture"));
        assert!(markdown.contains("Previous capture: `1700000000`"));
        assert!(markdown.contains("Comparing against archived capture `1700000000`."));
        assert!(markdown.contains("literal_simple"));
        assert!(markdown.contains("email_basic"));
        assert!(markdown.contains("improvement"));
        assert!(markdown.contains("regression"));
    }
}
