use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pcre2::bytes::Regex as PcreRegex;
use rgx_core::Regex;
use std::time::Duration;

// Test data sizes for scaling benchmarks
const INPUT_SIZES: &[usize] = &[100, 1_000, 10_000, 100_000];

// Benchmark patterns covering different regex features
struct BenchmarkPattern {
    name: &'static str,
    pattern: &'static str,
    description: &'static str,
}

const PATTERNS: &[BenchmarkPattern] = &[
    BenchmarkPattern {
        name: "literal_simple",
        pattern: r"test",
        description: "Simple 4-character literal match",
    },
    BenchmarkPattern {
        name: "email_basic",
        pattern: r"\b\w+@\w+\.\w+\b",
        description: "Basic email pattern with word boundaries",
    },
    BenchmarkPattern {
        name: "digit_sequence",
        pattern: r"\d{3}-\d{2}-\d{4}",
        description: "SSN-like pattern with digit quantifiers",
    },
    BenchmarkPattern {
        name: "character_class",
        pattern: r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
        description: "Email with character classes",
    },
    BenchmarkPattern {
        name: "alternation",
        pattern: r"cat|dog|bird",
        description: "Simple alternation",
    },
    BenchmarkPattern {
        name: "capture_groups",
        pattern: r"(\d{4})-(\d{2})-(\d{2})",
        description: "Date capture groups",
    },
    BenchmarkPattern {
        name: "url_simple",
        pattern: r"https?://\S+",
        description: "Simple URL detection",
    },
];

fn generate_test_data(size: usize, pattern: &str) -> String {
    // Generate test data that will have some matches for the given pattern
    match pattern {
        r"test" => {
            let mut data = String::with_capacity(size + 100);
            data.push_str("prefix ");
            while data.len() < size {
                data.push_str("test ");
                data.push_str("other ");
            }
            data.push_str(" suffix");
            data
        }
        r"\b\w+@\w+\.\w+\b" => {
            let mut data = String::with_capacity(size + 200);
            data.push_str("Contact info: ");
            while data.len() < size {
                data.push_str("user@example.com ");
                data.push_str("admin@test.org ");
                data.push_str("some text without emails ");
            }
            data.push_str(" end.");
            data
        }
        r"\d{3}-\d{2}-\d{4}" => {
            let mut data = String::with_capacity(size + 200);
            data.push_str("SSNs: ");
            while data.len() < size {
                data.push_str("123-45-6789 ");
                data.push_str("987-65-4321 ");
                data.push_str("random text 555-123-9999 more text ");
            }
            data.push_str(" done.");
            data
        }
        _ => {
            // Generic test data for other patterns
            let mut data = String::with_capacity(size + 100);
            data.push_str("Start: ");
            while data.len() < size {
                data.push_str("sample text that might match various patterns ");
            }
            data.push_str(" :End");
            data
        }
    }
}

fn benchmark_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("regex_compilation");
    group.measurement_time(Duration::from_secs(10));

    for pattern in PATTERNS {
        group.bench_with_input(
            BenchmarkId::new("rgx_compile", pattern.name),
            &pattern.pattern,
            |b, &pat| {
                b.iter(|| {
                    let _regex = Regex::compile(pat).unwrap();
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("pcre2_compile", pattern.name),
            &pattern.pattern,
            |b, &pat| {
                b.iter(|| {
                    let _regex = PcreRegex::new(pat).unwrap();
                });
            },
        );
    }

    group.finish();
}

fn benchmark_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("regex_throughput");
    group.measurement_time(Duration::from_secs(5));

    for pattern in PATTERNS {
        for &size in INPUT_SIZES {
            let test_data = generate_test_data(size, pattern.pattern);

            group.throughput(criterion::Throughput::Bytes(test_data.len() as u64));

            group.bench_with_input(
                BenchmarkId::new("rgx_throughput", format!("{}_{}", pattern.name, size)),
                &(&pattern.pattern, &test_data),
                |b, &(pat, data)| {
                    let regex = Regex::compile(pat).unwrap();
                    b.iter(|| {
                        let _matches = regex.find_all(data);
                    });
                },
            );

            group.bench_with_input(
                BenchmarkId::new("pcre2_throughput", format!("{}_{}", pattern.name, size)),
                &(&pattern.pattern, &test_data),
                |b, &(pat, data)| {
                    let regex = PcreRegex::new(pat).unwrap();
                    b.iter(|| {
                        let _matches: Vec<_> = regex.find_iter(data.as_bytes()).collect();
                    });
                },
            );
        }
    }

    group.finish();
}

fn benchmark_find_first(c: &mut Criterion) {
    let mut group = c.benchmark_group("regex_find_first");
    group.measurement_time(Duration::from_secs(3));

    for pattern in PATTERNS {
        let test_data = generate_test_data(10_000, pattern.pattern);

        group.bench_with_input(
            BenchmarkId::new("rgx_find_first", pattern.name),
            &(&pattern.pattern, &test_data),
            |b, &(pat, data)| {
                let regex = Regex::compile(pat).unwrap();
                b.iter(|| {
                    let _match = regex.find_first(data);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("pcre2_find_first", pattern.name),
            &(&pattern.pattern, &test_data),
            |b, &(pat, data)| {
                let regex = PcreRegex::new(pat).unwrap();
                b.iter(|| {
                    let _match = regex.find(data.as_bytes());
                });
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(100)
        .warm_up_time(Duration::from_secs(1));
    targets = benchmark_compilation, benchmark_throughput, benchmark_find_first
}

criterion_main!(benches);
