use pcre2::bytes::Regex as PcreRegex;
use rgx_bench::{generate_test_data, BenchmarkPattern, PATTERNS};
use rgx_core::Regex;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

#[derive(Debug)]
struct CliOptions {
    mode: CaptureMode,
    output_dir: PathBuf,
}

#[derive(Debug)]
struct TrendSample {
    kind: BenchmarkKind,
    pattern_name: &'static str,
    description: &'static str,
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

    let markdown = render_markdown(&samples, options.mode, generated_at_unix);
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

    println!(
        "[trend_capture] Wrote benchmark trend summary to {} and {}",
        markdown_path.display(),
        tsv_path.display()
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
        pattern_name: pattern.name,
        description: pattern.description,
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
        pattern_name: pattern.name,
        description: pattern.description,
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
        pattern_name: pattern.name,
        description: pattern.description,
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

fn render_markdown(samples: &[TrendSample], mode: CaptureMode, generated_at_unix: u64) -> String {
    let mut out = String::new();
    writeln!(&mut out, "# Benchmark Trend Capture").ok();
    writeln!(&mut out).ok();
    writeln!(&mut out, "- Mode: `{}`", mode.as_str()).ok();
    writeln!(&mut out, "- Generated at (unix): `{generated_at_unix}`").ok();
    writeln!(&mut out, "- Samples: `{}`", samples.len()).ok();
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
