use criterion::{criterion_group, criterion_main, Criterion};
use rgx_core::Regex;

fn throughput_benchmark(c: &mut Criterion) {
    let regex = Regex::compile(r"test").unwrap();
    let text = "test data for throughput measurement";
    
    c.bench_function("throughput test", |b| {
        b.iter(|| regex.find_all(text))
    });
}

criterion_group!(benches, throughput_benchmark);
criterion_main!(benches);
