use criterion::{criterion_group, criterion_main, Criterion};
use rgx_core::Regex;

fn benchmark_basic_regex(c: &mut Criterion) {
    let regex = Regex::compile(r"\d+").unwrap();
    let text = "The year 2025 was significant";

    c.bench_function("basic digit match", |b| b.iter(|| regex.is_match(text)));
}

criterion_group!(benches, benchmark_basic_regex);
criterion_main!(benches);
