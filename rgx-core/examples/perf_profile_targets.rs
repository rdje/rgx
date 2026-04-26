//! Tight-loop driver for `samply` profiling. Selects ONE pattern via
//! the `RGX_PROFILE_TARGET` env var and runs millions of iterations of
//! a chosen API method (`find_first` / `find_all` / `is_match`) so a
//! sampling profiler captures hot-path stack frames.
//!
//! Usage (from the repo root):
//!
//! ```text
//! cargo build --release -p rgx-core --example perf_profile_targets
//! RGX_PROFILE_TARGET=email_basic.find_first \
//!   samply record --save-only -o /tmp/email_basic_find_first.json.gz \
//!   target/release/examples/perf_profile_targets
//! samply load /tmp/email_basic_find_first.json.gz   # opens Firefox Profiler UI
//! ```
//!
//! The driver runs for a fixed wall-clock budget (3 seconds by
//! default; override via `RGX_PROFILE_DURATION_MS=5000`). At 1 kHz
//! sampling that yields ~3000 stack samples — enough resolution to
//! identify the top 5-10 hot paths.
//!
//! `RGX_PROFILE_TARGET` format: `<pattern>.<method>`. Methods:
//! `find_first`, `find_all`, `is_match`. Patterns are the bench-trends
//! 8-pattern corpus.
//!
//! Why a separate example instead of inline cargo bench: criterion
//! interleaves measurement overhead with the function under test;
//! samply wants raw cycles in the function. A simple `loop {}` keeps
//! the profile honest.
use rgx_core::Regex;
use std::time::{Duration, Instant};

const PATTERNS: &[(&str, &str)] = &[
    ("literal_simple", "test"),
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

fn make_data(pattern_name: &str) -> String {
    let size: usize = std::env::var("RGX_PROFILE_INPUT_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);
    let mut data = String::with_capacity(size + 200);
    match pattern_name {
        "literal_simple" => {
            data.push_str("prefix ");
            while data.len() < size {
                data.push_str("test ");
                data.push_str("other ");
            }
        }
        "email_basic" | "character_class" => {
            data.push_str("Contact info: ");
            while data.len() < size {
                data.push_str("user@example.com ");
                data.push_str("admin@test.org ");
                data.push_str("some text without emails ");
            }
        }
        "digit_sequence" | "capture_groups" => {
            data.push_str("Date list: ");
            while data.len() < size {
                data.push_str("123-45-6789 ");
                data.push_str("987-65-4321 ");
                data.push_str("filler text 555-123-9999 more ");
            }
        }
        "alternation" => {
            while data.len() < size {
                data.push_str("the cat sat on a mat ");
                data.push_str("a dog ran past a tree ");
                data.push_str("a bird flew above ");
                data.push_str("filler text without animals ");
            }
        }
        "url_simple" => {
            while data.len() < size {
                data.push_str("visit https://example.com/path?q=1 ");
                data.push_str("or http://other.org for details ");
                data.push_str("plain text without links ");
            }
        }
        "anchor_complex" => {
            // Anchor-bound; one matching line per chunk.
            while data.len() < size {
                data.push_str("123 hello foo\n");
                data.push_str("filler line\n");
                data.push_str("999 world bar\n");
            }
        }
        _ => {
            while data.len() < size {
                data.push_str("Start: filler text :End ");
            }
        }
    }
    data
}

fn parse_target() -> (&'static str, &'static str, &'static str) {
    let target = std::env::var("RGX_PROFILE_TARGET")
        .unwrap_or_else(|_| "email_basic.find_first".to_string());
    let (pat_name, method) = target
        .split_once('.')
        .expect("RGX_PROFILE_TARGET format: <pattern>.<method>");
    let (name, pattern) = PATTERNS
        .iter()
        .find(|(n, _)| *n == pat_name)
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "unknown pattern '{pat_name}'; valid: {:?}",
                PATTERNS.iter().map(|(n, _)| *n).collect::<Vec<_>>()
            )
        });
    let method_static: &'static str = match method {
        "find_first" => "find_first",
        "find_all" => "find_all",
        "is_match" => "is_match",
        other => panic!("unknown method '{other}'; valid: find_first, find_all, is_match"),
    };
    (name, pattern, method_static)
}

fn main() {
    let (pat_name, pattern, method) = parse_target();
    let duration_ms: u64 = std::env::var("RGX_PROFILE_DURATION_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let budget = Duration::from_millis(duration_ms);

    let data = make_data(pat_name);
    let re = Regex::compile(pattern).expect("regex compile");

    eprintln!(
        "[perf_profile_targets] pattern='{pattern}' name={pat_name} method={method} input_len={} budget_ms={duration_ms}",
        data.len()
    );

    // Warm up the JIT / DFA caches before sampling starts. samply's
    // first 50ms or so are usually noisy anyway, but explicit warmup
    // also ensures the lazy artifacts are realised.
    for _ in 0..1000 {
        match method {
            "find_first" => {
                std::hint::black_box(re.find_first(&data));
            }
            "find_all" => {
                std::hint::black_box(re.find_all(&data));
            }
            "is_match" => {
                std::hint::black_box(re.is_match(&data));
            }
            _ => unreachable!(),
        }
    }

    let start = Instant::now();
    let mut iterations: u64 = 0;
    while start.elapsed() < budget {
        // Run a batch between time checks so the timing overhead is
        // small relative to the work. 256 iterations at 100 ns/iter =
        // 25 µs per check — about 0.0008% of the 3-second budget.
        for _ in 0..256 {
            match method {
                "find_first" => {
                    std::hint::black_box(re.find_first(&data));
                }
                "find_all" => {
                    std::hint::black_box(re.find_all(&data));
                }
                "is_match" => {
                    std::hint::black_box(re.is_match(&data));
                }
                _ => unreachable!(),
            }
        }
        iterations += 256;
    }

    let elapsed = start.elapsed();
    let ns_per_iter = elapsed.as_nanos() / iterations.max(1) as u128;
    eprintln!(
        "[perf_profile_targets] iterations={iterations} elapsed_ms={} ns_per_iter={ns_per_iter}",
        elapsed.as_millis()
    );
}
