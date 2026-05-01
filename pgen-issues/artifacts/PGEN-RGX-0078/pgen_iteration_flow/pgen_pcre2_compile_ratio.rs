//! PGEN-side standalone microbench: PGEN regex parser parse-time vs
//! PCRE2 full compile-time, on the same 8-pattern corpus, on the same
//! host, in the same process. Designed to be vendored into PGEN's
//! `rust/examples/` directory so PGEN can iterate on the compile-time
//! gap WITHOUT going through RGX.
//!
//! Drop this file into `pgen/rust/examples/` and add to
//! `pgen/rust/Cargo.toml`:
//!
//! ```toml
//! [[example]]
//! name = "pgen_pcre2_compile_ratio"
//! path = "examples/pgen_pcre2_compile_ratio.rs"
//! required-features = ["generated_parsers"]
//!
//! [dev-dependencies]
//! pcre2 = "0.2"   # libpcre2-8 Rust binding (battle-tested, used widely)
//! ```
//!
//! Run with:
//!
//! ```text
//! cargo run --release --features generated_parsers \
//!   --example pgen_pcre2_compile_ratio
//! ```
//!
//! Output is a per-pattern table of median timings (ns) and the
//! PGEN/PCRE2 ratio. Geomean ratio is the headline number for
//! PGEN-RGX-0078 closure tracking — target ratio < 5x per the RGX
//! ROADMAP.
//!
//! No RGX dependency. No criterion dependency. Single Cargo run.
//! Pair with `pcre2_compile_baseline.c` (the standalone C bench
//! attached alongside this file in `pgen-issues/artifacts/PGEN-RGX-0078/`)
//! if you want to cross-check the `pcre2` Rust crate against
//! libpcre2-8 directly via `cc -O2 -lpcre2-8` — both should agree
//! within the noise floor.

use std::time::Instant;

const ITERATIONS: usize = 5000;
const WARMUP: usize = 200;

/// 8-pattern corpus inherited from PGEN-RGX-0073 / 0078. Kept stable
/// across reports so longitudinal comparisons are apples-to-apples.
const PATTERNS: &[(&str, &str)] = &[
    ("literal_simple", r"test"),
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

/// Median (p50) of a sample vector. Caller passes a sorted slice or
/// sorts in place beforehand. Trivial helper kept inline so the file
/// has no helper-function dependencies.
fn p50(sorted: &[u128]) -> u128 {
    sorted[sorted.len() / 2]
}

fn time_pgen_parse(pattern: &str) -> Vec<u128> {
    // Warmup — first compiles often hit cold caches, JIT codegen for
    // the parser itself, etc. Discard the timings.
    for _ in 0..WARMUP {
        let _ = pgen::embedding_api::parse_grammar_profile_named(
            "regex",
            "regex_default",
            pattern,
        );
    }
    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let t0 = Instant::now();
        let outcome = pgen::embedding_api::parse_grammar_profile_named(
            "regex",
            "regex_default",
            pattern,
        );
        let dt = t0.elapsed().as_nanos();
        // Sanity — bail loudly if the parse fails. We do NOT want to
        // be timing a fail path.
        assert!(
            matches!(outcome.status, pgen::embedding_api::ParseStatus::Success),
            "PGEN parse failed for pattern {:?}: {:?}",
            pattern,
            outcome.diagnostic
        );
        samples.push(dt);
    }
    samples.sort_unstable();
    samples
}

fn time_pcre2_compile(pattern: &str) -> Vec<u128> {
    // Same warmup discipline. PCRE2 compile is implemented in C and
    // doesn't have JIT-codegen warmup behaviour, but allocator state
    // can still vary so we discard the early samples.
    for _ in 0..WARMUP {
        let _ = pcre2::bytes::Regex::new(pattern).expect("PCRE2 should compile");
    }
    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let t0 = Instant::now();
        let _re = pcre2::bytes::Regex::new(pattern).expect("PCRE2 should compile");
        let dt = t0.elapsed().as_nanos();
        samples.push(dt);
    }
    samples.sort_unstable();
    samples
}

fn main() {
    println!(
        "# PGEN regex parse vs PCRE2 full compile — {} samples / {} warmup",
        ITERATIONS, WARMUP
    );
    println!("# Run on `cargo run --release --features generated_parsers --example pgen_pcre2_compile_ratio`.");
    println!();
    println!(
        "{:<22} {:>14} {:>14} {:>10}",
        "pattern", "PGEN parse p50 (ns)", "PCRE2 compile p50 (ns)", "ratio"
    );
    println!("{}", "-".repeat(70));

    let mut log_ratios: Vec<f64> = Vec::with_capacity(PATTERNS.len());
    for (name, pattern) in PATTERNS {
        let pgen_samples = time_pgen_parse(pattern);
        let pcre2_samples = time_pcre2_compile(pattern);
        let pgen_p50 = p50(&pgen_samples) as f64;
        let pcre2_p50 = p50(&pcre2_samples) as f64;
        let ratio = pgen_p50 / pcre2_p50;
        log_ratios.push(ratio.ln());
        println!(
            "{:<22} {:>14.0} {:>14.0} {:>9.1}x",
            name, pgen_p50, pcre2_p50, ratio
        );
    }

    let geomean = (log_ratios.iter().sum::<f64>() / log_ratios.len() as f64).exp();
    println!();
    println!("# Geomean PGEN/PCRE2 ratio: {:.1}x", geomean);
    println!("# RGX ROADMAP closure target: < 5x");
    println!();
    println!("# Closure status:");
    if geomean < 5.0 {
        println!("# ✅ Closure target met (geomean < 5x).");
    } else {
        println!(
            "# ❌ Closure target NOT met (geomean is {:.1}x; needs to be < 5x).",
            geomean
        );
    }
}
