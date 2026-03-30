use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pcre2::bytes::Regex as PcreRegex;
use rgx_bench::{generate_test_data, INPUT_SIZES, PATTERNS};
use rgx_core::Regex;
use std::time::Duration;

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
