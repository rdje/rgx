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
    compare_against: ComparisonBaselineSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComparisonBaselineSelection {
    Auto,
    None,
    Timestamp(u64),
}

impl ComparisonBaselineSelection {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "none" => Ok(Self::None),
            other => other
                .parse::<u64>()
                .map(Self::Timestamp)
                .map_err(|_| {
                    format!(
                        "unsupported --compare-against value `{other}`; expected `auto`, `none`, or a unix timestamp"
                    )
                }),
        }
    }

    fn requested_label(self) -> String {
        match self {
            Self::Auto => "auto".to_string(),
            Self::None => "none".to_string(),
            Self::Timestamp(timestamp) => timestamp.to_string(),
        }
    }
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

#[derive(Debug, Clone)]
struct HistoricalCapture {
    generated_at_unix: u64,
    mode: CaptureMode,
    samples: Vec<TrendSample>,
}

#[derive(Debug, Clone)]
struct ComparisonBaseline {
    selection: ComparisonBaselineSelection,
    capture: Option<HistoricalCapture>,
}

impl ComparisonBaseline {
    fn resolved_label(&self) -> String {
        match (&self.selection, &self.capture) {
            (ComparisonBaselineSelection::None, _) => "disabled".to_string(),
            (_, Some(capture)) => {
                format!("{} ({})", capture.generated_at_unix, capture.mode.as_str())
            }
            _ => "none".to_string(),
        }
    }
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

#[derive(Debug, Clone)]
struct HistorySummaryRow {
    generated_at_unix: u64,
    compile_ratio: Option<f64>,
    find_first_ratio: Option<f64>,
    find_all_ratio: Option<f64>,
    compile_delta: Option<f64>,
    find_first_delta: Option<f64>,
    find_all_delta: Option<f64>,
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
    let history_root = options.output_dir.join("history");
    fs::create_dir_all(&history_root).map_err(|err| {
        format!(
            "failed to create benchmark trend history directory {}: {err}",
            history_root.display()
        )
    })?;
    let mode_history_dir = history_dir_for_mode(&history_root, options.mode);
    fs::create_dir_all(&mode_history_dir).map_err(|err| {
        format!(
            "failed to create mode-scoped benchmark trend history directory {}: {err}",
            mode_history_dir.display()
        )
    })?;

    let comparison_baseline =
        load_comparison_baseline(&history_root, options.mode, options.compare_against)?;

    let markdown = render_markdown(
        &samples,
        options.mode,
        generated_at_unix,
        &comparison_baseline,
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

    let mode_markdown_path = options
        .output_dir
        .join(format!("latest-{}.md", options.mode.as_str()));
    fs::write(&mode_markdown_path, markdown.as_bytes()).map_err(|err| {
        format!(
            "failed to write mode-scoped markdown benchmark summary {}: {err}",
            mode_markdown_path.display()
        )
    })?;

    let mode_tsv_path = options
        .output_dir
        .join(format!("latest-{}.tsv", options.mode.as_str()));
    fs::write(&mode_tsv_path, tsv.as_bytes()).map_err(|err| {
        format!(
            "failed to write mode-scoped tabular benchmark summary {}: {err}",
            mode_tsv_path.display()
        )
    })?;

    let history_markdown_path = mode_history_dir.join(format!("{generated_at_unix}.md"));
    fs::write(&history_markdown_path, markdown.as_bytes()).map_err(|err| {
        format!(
            "failed to write archived markdown benchmark summary {}: {err}",
            history_markdown_path.display()
        )
    })?;

    let history_tsv_path = mode_history_dir.join(format!("{generated_at_unix}.tsv"));
    fs::write(&history_tsv_path, tsv.as_bytes()).map_err(|err| {
        format!(
            "failed to write archived tabular benchmark summary {}: {err}",
            history_tsv_path.display()
        )
    })?;

    let history_captures = load_historical_captures(&history_root, options.mode)?;
    let history_summary_markdown = render_history_summary_markdown(&history_captures, options.mode);
    let history_summary_tsv = render_history_summary_tsv(&history_captures, options.mode);

    let history_summary_markdown_path = options
        .output_dir
        .join(format!("history-{}.md", options.mode.as_str()));
    fs::write(
        &history_summary_markdown_path,
        history_summary_markdown.as_bytes(),
    )
    .map_err(|err| {
        format!(
            "failed to write rolling history markdown summary {}: {err}",
            history_summary_markdown_path.display()
        )
    })?;

    let history_summary_tsv_path = options
        .output_dir
        .join(format!("history-{}.tsv", options.mode.as_str()));
    fs::write(&history_summary_tsv_path, history_summary_tsv.as_bytes()).map_err(|err| {
        format!(
            "failed to write rolling history tabular summary {}: {err}",
            history_summary_tsv_path.display()
        )
    })?;

    println!(
        "[trend_capture] Wrote benchmark trend summary to {}, {}, {}, and {}",
        markdown_path.display(),
        tsv_path.display(),
        mode_markdown_path.display(),
        mode_tsv_path.display()
    );
    println!(
        "[trend_capture] Archived benchmark snapshot to {} and {}",
        history_markdown_path.display(),
        history_tsv_path.display()
    );
    println!(
        "[trend_capture] Wrote rolling history summary to {} and {}",
        history_summary_markdown_path.display(),
        history_summary_tsv_path.display()
    );
    println!();
    println!("{markdown}");

    Ok(())
}

fn parse_args() -> Result<CliOptions, String> {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from<I>(args: I) -> Result<CliOptions, String>
where
    I: IntoIterator<Item = String>,
{
    let mut mode = CaptureMode::Quick;
    let mut output_dir = PathBuf::from("target/benchmark-trends");
    let mut compare_against = ComparisonBaselineSelection::Auto;

    let mut args = args.into_iter();
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
            "--compare-against" => {
                let value = args.next().ok_or_else(|| {
                    "--compare-against requires `auto`, `none`, or a unix timestamp".to_string()
                })?;
                compare_against = ComparisonBaselineSelection::parse(&value)?;
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

    Ok(CliOptions {
        mode,
        output_dir,
        compare_against,
    })
}

fn print_usage() {
    println!(
        "trend_capture --mode <quick|full> --output-dir <path> --compare-against <auto|none|unix-timestamp>"
    );
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

fn history_dir_for_mode(history_root: &Path, mode: CaptureMode) -> PathBuf {
    history_root.join(mode.as_str())
}

fn load_historical_captures(
    history_root: &Path,
    mode: CaptureMode,
) -> Result<Vec<HistoricalCapture>, String> {
    let mut captures_by_timestamp = std::collections::BTreeMap::new();

    for capture in
        load_historical_captures_from_dir(&history_dir_for_mode(history_root, mode), mode)?
    {
        captures_by_timestamp.insert(capture.generated_at_unix, capture);
    }

    // Legacy unscoped history entries were effectively quick-mode captures.
    if mode == CaptureMode::Quick {
        for capture in load_historical_captures_from_dir(history_root, CaptureMode::Quick)? {
            captures_by_timestamp
                .entry(capture.generated_at_unix)
                .or_insert(capture);
        }
    }

    Ok(captures_by_timestamp.into_values().collect())
}

fn load_historical_captures_from_dir(
    history_dir: &Path,
    mode: CaptureMode,
) -> Result<Vec<HistoricalCapture>, String> {
    if !history_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(history_dir).map_err(|err| {
        format!(
            "failed to read benchmark trend history directory {}: {err}",
            history_dir.display()
        )
    })?;

    let mut captures = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            format!(
                "failed to read an entry from benchmark trend history directory {}: {err}",
                history_dir.display()
            )
        })?;
        if let Some(candidate) = parse_history_entry(entry, mode)? {
            captures.push(candidate);
        }
    }

    captures.sort_by_key(|capture| capture.generated_at_unix);
    Ok(captures)
}

fn load_most_recent_historical_capture(
    history_root: &Path,
    mode: CaptureMode,
) -> Result<Option<HistoricalCapture>, String> {
    Ok(load_historical_captures(history_root, mode)?.pop())
}

fn load_comparison_baseline(
    history_root: &Path,
    mode: CaptureMode,
    selection: ComparisonBaselineSelection,
) -> Result<ComparisonBaseline, String> {
    let capture = match selection {
        ComparisonBaselineSelection::Auto => {
            load_most_recent_historical_capture(history_root, mode)?
        }
        ComparisonBaselineSelection::None => None,
        ComparisonBaselineSelection::Timestamp(generated_at_unix) => {
            load_historical_capture_by_timestamp(history_root, mode, generated_at_unix)?
        }
    };

    Ok(ComparisonBaseline { selection, capture })
}

fn load_historical_capture_by_timestamp(
    history_root: &Path,
    mode: CaptureMode,
    generated_at_unix: u64,
) -> Result<Option<HistoricalCapture>, String> {
    let path = history_dir_for_mode(history_root, mode).join(format!("{generated_at_unix}.tsv"));
    if !path.exists() {
        if mode == CaptureMode::Quick {
            let legacy_path = history_root.join(format!("{generated_at_unix}.tsv"));
            if !legacy_path.exists() {
                return Ok(None);
            }
            return Ok(Some(load_historical_capture(
                &legacy_path,
                generated_at_unix,
                CaptureMode::Quick,
            )?));
        }
        return Ok(None);
    }

    Ok(Some(load_historical_capture(
        &path,
        generated_at_unix,
        mode,
    )?))
}

fn parse_history_entry(
    entry: fs::DirEntry,
    mode: CaptureMode,
) -> Result<Option<HistoricalCapture>, String> {
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

    Ok(Some(load_historical_capture(
        &path,
        generated_at_unix,
        mode,
    )?))
}

fn load_historical_capture(
    path: &Path,
    generated_at_unix: u64,
    mode: CaptureMode,
) -> Result<HistoricalCapture, String> {
    let raw = fs::read_to_string(path).map_err(|err| {
        format!(
            "failed to read benchmark trend history snapshot {}: {err}",
            path.display()
        )
    })?;
    let samples = parse_tsv(&raw)?;

    Ok(HistoricalCapture {
        generated_at_unix,
        mode,
        samples,
    })
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
    comparison_baseline: &ComparisonBaseline,
) -> String {
    let mut out = String::new();
    writeln!(&mut out, "# Benchmark Trend Capture").ok();
    writeln!(&mut out).ok();
    writeln!(&mut out, "- Mode: `{}`", mode.as_str()).ok();
    writeln!(&mut out, "- Generated at (unix): `{generated_at_unix}`").ok();
    writeln!(&mut out, "- Samples: `{}`", samples.len()).ok();
    writeln!(
        &mut out,
        "- Compare against request: `{}`",
        comparison_baseline.selection.requested_label()
    )
    .ok();
    writeln!(
        &mut out,
        "- Resolved comparison baseline: `{}`",
        comparison_baseline.resolved_label()
    )
    .ok();
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
    render_comparison_markdown(&mut out, samples, mode, comparison_baseline);
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

fn aggregate_ratio_median(samples: &[TrendSample], kind: BenchmarkKind) -> Option<f64> {
    let mut ratios = samples
        .iter()
        .filter(|sample| sample.kind == kind)
        .map(TrendSample::ratio_rgx_over_pcre2)
        .collect::<Vec<_>>();
    if ratios.is_empty() {
        None
    } else {
        Some(median(&mut ratios))
    }
}

fn aggregate_ratio_delta(
    current_samples: &[TrendSample],
    previous_samples: &[TrendSample],
    kind: BenchmarkKind,
) -> Option<f64> {
    let current = aggregate_ratio_median(current_samples, kind)?;
    let previous = aggregate_ratio_median(previous_samples, kind)?;
    if previous == 0.0 {
        None
    } else {
        Some((current / previous) - 1.0)
    }
}

fn format_ratio_summary(ratio: f64) -> String {
    if ratio < 1.0 {
        format!("{:.2}x faster median", 1.0 / ratio)
    } else {
        format!("{ratio:.2}x slower median")
    }
}

fn build_history_summary_rows(captures: &[HistoricalCapture]) -> Vec<HistorySummaryRow> {
    captures
        .iter()
        .enumerate()
        .map(|(index, capture)| {
            let previous = index.checked_sub(1).and_then(|prior| captures.get(prior));
            HistorySummaryRow {
                generated_at_unix: capture.generated_at_unix,
                compile_ratio: aggregate_ratio_median(&capture.samples, BenchmarkKind::Compile),
                find_first_ratio: aggregate_ratio_median(
                    &capture.samples,
                    BenchmarkKind::FindFirst,
                ),
                find_all_ratio: aggregate_ratio_median(&capture.samples, BenchmarkKind::FindAll),
                compile_delta: previous.and_then(|prior| {
                    aggregate_ratio_delta(&capture.samples, &prior.samples, BenchmarkKind::Compile)
                }),
                find_first_delta: previous.and_then(|prior| {
                    aggregate_ratio_delta(
                        &capture.samples,
                        &prior.samples,
                        BenchmarkKind::FindFirst,
                    )
                }),
                find_all_delta: previous.and_then(|prior| {
                    aggregate_ratio_delta(&capture.samples, &prior.samples, BenchmarkKind::FindAll)
                }),
            }
        })
        .collect()
}

fn render_history_summary_markdown(captures: &[HistoricalCapture], mode: CaptureMode) -> String {
    let mut out = String::new();
    writeln!(&mut out, "# Benchmark Trend History").ok();
    writeln!(&mut out).ok();
    writeln!(&mut out, "- Mode: `{}`", mode.as_str()).ok();
    writeln!(&mut out, "- Entries: `{}`", captures.len()).ok();
    if let Some(oldest) = captures.first() {
        writeln!(
            &mut out,
            "- Oldest capture (unix): `{}`",
            oldest.generated_at_unix
        )
        .ok();
    }
    if let Some(latest) = captures.last() {
        writeln!(
            &mut out,
            "- Latest capture (unix): `{}`",
            latest.generated_at_unix
        )
        .ok();
    }
    writeln!(&mut out).ok();

    if captures.is_empty() {
        writeln!(&mut out, "- No archived captures yet.").ok();
        return out;
    }

    writeln!(
        &mut out,
        "| Generated at | Compile median | Find First median | Find All median | Compile delta vs previous | Find First delta vs previous | Find All delta vs previous |"
    )
    .ok();
    writeln!(&mut out, "| ---: | --- | --- | --- | --- | --- | --- |").ok();

    for row in build_history_summary_rows(captures).into_iter().rev() {
        writeln!(
            &mut out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            row.generated_at_unix,
            row.compile_ratio
                .map(format_ratio_summary)
                .unwrap_or_else(|| "-".to_string()),
            row.find_first_ratio
                .map(format_ratio_summary)
                .unwrap_or_else(|| "-".to_string()),
            row.find_all_ratio
                .map(format_ratio_summary)
                .unwrap_or_else(|| "-".to_string()),
            row.compile_delta
                .map(format_change_label)
                .unwrap_or_else(|| "-".to_string()),
            row.find_first_delta
                .map(format_change_label)
                .unwrap_or_else(|| "-".to_string()),
            row.find_all_delta
                .map(format_change_label)
                .unwrap_or_else(|| "-".to_string()),
        )
        .ok();
    }

    out
}

fn render_history_summary_tsv(captures: &[HistoricalCapture], mode: CaptureMode) -> String {
    let mut out = String::new();
    writeln!(
        &mut out,
        "generated_at_unix\tmode\tcompile_ratio\tfind_first_ratio\tfind_all_ratio\tcompile_delta_fraction\tfind_first_delta_fraction\tfind_all_delta_fraction"
    )
    .ok();

    for row in build_history_summary_rows(captures) {
        writeln!(
            &mut out,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.generated_at_unix,
            mode.as_str(),
            format_optional_tsv_number(row.compile_ratio),
            format_optional_tsv_number(row.find_first_ratio),
            format_optional_tsv_number(row.find_all_ratio),
            format_optional_tsv_number(row.compile_delta),
            format_optional_tsv_number(row.find_first_delta),
            format_optional_tsv_number(row.find_all_delta),
        )
        .ok();
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
    current_mode: CaptureMode,
    comparison_baseline: &ComparisonBaseline,
) {
    writeln!(out, "## Delta vs Comparison Baseline").ok();
    match (
        &comparison_baseline.selection,
        comparison_baseline.capture.as_ref(),
    ) {
        (ComparisonBaselineSelection::None, _) => {
            writeln!(out, "- Comparison was disabled for this capture.").ok();
        }
        (ComparisonBaselineSelection::Auto, None) => {
            writeln!(out, "- No prior archived benchmark capture was available.").ok();
        }
        (ComparisonBaselineSelection::Timestamp(timestamp), None) => {
            writeln!(
                out,
                "- Requested archived `{}` capture `{timestamp}` was not found.",
                current_mode.as_str()
            )
            .ok();
        }
        (selection, Some(previous_capture)) => {
            let comparisons = compare_samples(current_samples, &previous_capture.samples);
            if comparisons.is_empty() {
                writeln!(
                    out,
                    "- Comparison baseline `{}` did not share any comparable benchmark rows with the current capture.",
                    previous_capture.generated_at_unix
                )
                .ok();
                return;
            }

            let intro = match selection {
                ComparisonBaselineSelection::Auto => format!(
                    "- Comparing against archived `{}` capture `{}`.",
                    previous_capture.mode.as_str(),
                    previous_capture.generated_at_unix
                ),
                ComparisonBaselineSelection::Timestamp(_) => format!(
                    "- Comparing against requested archived `{}` capture `{}`.",
                    previous_capture.mode.as_str(),
                    previous_capture.generated_at_unix
                ),
                ComparisonBaselineSelection::None => unreachable!(),
            };
            writeln!(out, "{intro}").ok();
            writeln!(out).ok();
            render_comparison_sections(out, &comparisons);
        }
    }
}

fn render_comparison_sections(out: &mut String, comparisons: &[ComparisonSample]) {
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

fn format_optional_tsv_number(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.6}"))
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempTestDir {
        path: PathBuf,
    }

    impl TempTestDir {
        fn new(prefix: &str) -> Self {
            let unique = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rgx-trend-capture-{prefix}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("temp test dir should be creatable");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_history_capture(
        history_root: &Path,
        mode: Option<CaptureMode>,
        generated_at_unix: u64,
        samples: &[TrendSample],
    ) {
        let history_dir = match mode {
            Some(mode) => history_dir_for_mode(history_root, mode),
            None => history_root.to_path_buf(),
        };
        fs::create_dir_all(&history_dir).expect("history directory should be creatable");
        let path = history_dir.join(format!("{generated_at_unix}.tsv"));
        fs::write(&path, render_tsv(samples)).expect("history capture should be writable");
    }

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
    fn parse_args_accepts_compare_against_history_id() {
        let options = parse_args_from([
            "--mode".to_string(),
            "full".to_string(),
            "--compare-against".to_string(),
            "1700000000".to_string(),
            "--output-dir".to_string(),
            "/tmp/bench-trends".to_string(),
        ])
        .expect("explicit comparison baseline should parse");

        assert_eq!(options.mode, CaptureMode::Full);
        assert_eq!(options.output_dir, PathBuf::from("/tmp/bench-trends"));
        assert_eq!(
            options.compare_against,
            ComparisonBaselineSelection::Timestamp(1700000000)
        );
    }

    #[test]
    fn parse_args_accepts_disabled_comparison() {
        let options = parse_args_from(["--compare-against".to_string(), "none".to_string()])
            .expect("disabled comparison baseline should parse");

        assert_eq!(options.mode, CaptureMode::Quick);
        assert_eq!(options.output_dir, PathBuf::from("target/benchmark-trends"));
        assert_eq!(options.compare_against, ComparisonBaselineSelection::None);
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
            mode: CaptureMode::Quick,
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

        let comparison_baseline = ComparisonBaseline {
            selection: ComparisonBaselineSelection::Auto,
            capture: Some(previous),
        };

        let markdown = render_markdown(
            &current,
            CaptureMode::Quick,
            1800000000,
            &comparison_baseline,
        );
        assert!(markdown.contains("## Delta vs Comparison Baseline"));
        assert!(markdown.contains("Compare against request: `auto`"));
        assert!(markdown.contains("Resolved comparison baseline: `1700000000 (quick)`"));
        assert!(markdown.contains("Comparing against archived `quick` capture `1700000000`."));
        assert!(markdown.contains("literal_simple"));
        assert!(markdown.contains("email_basic"));
        assert!(markdown.contains("improvement"));
        assert!(markdown.contains("regression"));
    }

    #[test]
    fn render_markdown_reports_disabled_comparison() {
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
        let comparison_baseline = ComparisonBaseline {
            selection: ComparisonBaselineSelection::None,
            capture: None,
        };

        let markdown = render_markdown(
            &current,
            CaptureMode::Quick,
            1800000000,
            &comparison_baseline,
        );

        assert!(markdown.contains("Compare against request: `none`"));
        assert!(markdown.contains("Resolved comparison baseline: `disabled`"));
        assert!(markdown.contains("Comparison was disabled for this capture."));
    }

    #[test]
    fn render_markdown_reports_missing_requested_baseline() {
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
        let comparison_baseline = ComparisonBaseline {
            selection: ComparisonBaselineSelection::Timestamp(1700000000),
            capture: None,
        };

        let markdown = render_markdown(
            &current,
            CaptureMode::Quick,
            1800000000,
            &comparison_baseline,
        );

        assert!(markdown.contains("Compare against request: `1700000000`"));
        assert!(markdown.contains("Resolved comparison baseline: `none`"));
        assert!(markdown.contains("Requested archived `quick` capture `1700000000` was not found."));
    }

    #[test]
    fn render_history_summary_markdown_reports_longitudinal_rows() {
        let captures = vec![
            HistoricalCapture {
                generated_at_unix: 1700000000,
                mode: CaptureMode::Quick,
                samples: vec![
                    sample(BenchmarkKind::Compile, "literal_simple", None, 10.0, 5.0),
                    sample(
                        BenchmarkKind::FindFirst,
                        "email_basic",
                        Some(1000),
                        20.0,
                        10.0,
                    ),
                    sample(
                        BenchmarkKind::FindAll,
                        "capture_groups",
                        Some(1000),
                        30.0,
                        10.0,
                    ),
                ],
            },
            HistoricalCapture {
                generated_at_unix: 1800000000,
                mode: CaptureMode::Quick,
                samples: vec![
                    sample(BenchmarkKind::Compile, "literal_simple", None, 12.0, 6.0),
                    sample(
                        BenchmarkKind::FindFirst,
                        "email_basic",
                        Some(1000),
                        18.0,
                        10.0,
                    ),
                    sample(
                        BenchmarkKind::FindAll,
                        "capture_groups",
                        Some(1000),
                        33.0,
                        10.0,
                    ),
                ],
            },
        ];

        let markdown = render_history_summary_markdown(&captures, CaptureMode::Quick);
        assert!(markdown.contains("# Benchmark Trend History"));
        assert!(markdown.contains("Entries: `2`"));
        assert!(markdown.contains("| 1800000000 |"));
        assert!(markdown.contains("2.00x slower median"));
        assert!(markdown.contains("10.00% regression"));
        assert!(markdown.contains("10.00% improvement"));
    }

    #[test]
    fn render_history_summary_tsv_includes_delta_columns() {
        let captures = vec![
            HistoricalCapture {
                generated_at_unix: 1700000000,
                mode: CaptureMode::Full,
                samples: vec![
                    sample(BenchmarkKind::Compile, "literal_simple", None, 10.0, 5.0),
                    sample(
                        BenchmarkKind::FindFirst,
                        "email_basic",
                        Some(1000),
                        20.0,
                        10.0,
                    ),
                    sample(
                        BenchmarkKind::FindAll,
                        "capture_groups",
                        Some(1000),
                        30.0,
                        10.0,
                    ),
                ],
            },
            HistoricalCapture {
                generated_at_unix: 1800000000,
                mode: CaptureMode::Full,
                samples: vec![
                    sample(BenchmarkKind::Compile, "literal_simple", None, 11.0, 5.0),
                    sample(
                        BenchmarkKind::FindFirst,
                        "email_basic",
                        Some(1000),
                        18.0,
                        10.0,
                    ),
                    sample(
                        BenchmarkKind::FindAll,
                        "capture_groups",
                        Some(1000),
                        33.0,
                        10.0,
                    ),
                ],
            },
        ];

        let tsv = render_history_summary_tsv(&captures, CaptureMode::Full);
        assert!(tsv.contains("generated_at_unix\tmode\tcompile_ratio"));
        assert!(tsv.contains("1700000000\tfull\t2.000000\t2.000000\t3.000000\t-\t-\t-"));
        assert!(tsv.contains(
            "1800000000\tfull\t2.200000\t1.800000\t3.300000\t0.100000\t-0.100000\t0.100000"
        ));
    }

    #[test]
    fn load_historical_captures_merges_legacy_and_mode_scoped_quick_history() {
        let temp_dir = TempTestDir::new("quick-history-merge");
        let history_root = temp_dir.path().join("history");
        let samples = vec![sample(
            BenchmarkKind::Compile,
            "literal_simple",
            None,
            12.0,
            6.0,
        )];

        write_history_capture(&history_root, None, 1700000000, &samples);
        write_history_capture(
            &history_root,
            Some(CaptureMode::Quick),
            1800000000,
            &samples,
        );

        let captures = load_historical_captures(&history_root, CaptureMode::Quick)
            .expect("quick history should load");

        assert_eq!(captures.len(), 2);
        assert_eq!(captures[0].generated_at_unix, 1700000000);
        assert_eq!(captures[1].generated_at_unix, 1800000000);
        assert!(captures
            .iter()
            .all(|capture| capture.mode == CaptureMode::Quick));
    }

    #[test]
    fn auto_baseline_prefers_mode_scoped_history() {
        let temp_dir = TempTestDir::new("quick-mode-history");
        let history_root = temp_dir.path().join("history");
        let samples = vec![sample(
            BenchmarkKind::Compile,
            "literal_simple",
            None,
            12.0,
            6.0,
        )];

        write_history_capture(&history_root, None, 1700000000, &samples);
        write_history_capture(
            &history_root,
            Some(CaptureMode::Quick),
            1800000000,
            &samples,
        );

        let baseline = load_comparison_baseline(
            &history_root,
            CaptureMode::Quick,
            ComparisonBaselineSelection::Auto,
        )
        .expect("auto quick baseline should load");

        let capture = baseline.capture.expect("quick baseline should resolve");
        assert_eq!(capture.generated_at_unix, 1800000000);
        assert_eq!(capture.mode, CaptureMode::Quick);
    }

    #[test]
    fn full_auto_baseline_does_not_fall_back_to_legacy_quick_history() {
        let temp_dir = TempTestDir::new("full-mode-history");
        let history_root = temp_dir.path().join("history");
        let samples = vec![sample(
            BenchmarkKind::Compile,
            "literal_simple",
            None,
            12.0,
            6.0,
        )];

        write_history_capture(&history_root, None, 1700000000, &samples);

        let baseline = load_comparison_baseline(
            &history_root,
            CaptureMode::Full,
            ComparisonBaselineSelection::Auto,
        )
        .expect("auto full baseline lookup should succeed");

        assert!(baseline.capture.is_none());
    }

    #[test]
    fn quick_explicit_timestamp_can_fall_back_to_legacy_history() {
        let temp_dir = TempTestDir::new("quick-legacy-history");
        let history_root = temp_dir.path().join("history");
        let samples = vec![sample(
            BenchmarkKind::Compile,
            "literal_simple",
            None,
            12.0,
            6.0,
        )];

        write_history_capture(&history_root, None, 1700000000, &samples);

        let baseline = load_comparison_baseline(
            &history_root,
            CaptureMode::Quick,
            ComparisonBaselineSelection::Timestamp(1700000000),
        )
        .expect("explicit quick baseline lookup should succeed");

        let capture = baseline
            .capture
            .expect("quick legacy baseline should resolve");
        assert_eq!(capture.generated_at_unix, 1700000000);
        assert_eq!(capture.mode, CaptureMode::Quick);
    }
}
