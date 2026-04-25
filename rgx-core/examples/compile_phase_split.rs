//! Measurement-only: split `Regex::compile` wall-clock time into its
//! three top-level phases — PGEN parsing, AST→bytecode + C2 build,
//! and engine construction (DFAs + JIT) — across the standard bench
//! pattern corpus.
//!
//! Run with:
//!
//! ```text
//! cargo run --release -p rgx-core --example compile_phase_split
//! ```
//!
//! Output is a per-pattern table of median timings (ns) with each
//! phase's share of total. Used to determine whether PGEN dominates
//! the compile budget (which would gate the achievable gap-to-PCRE2).
//!
//! This is not a benchmark crate — the numbers are good-enough for
//! a phase-split decision, not for absolute perf claims. Criterion
//! lives in `rgx-bench` for that.
use rgx_core::{Compiler, Engine, Regex};
use std::time::{Duration, Instant};

const ITERATIONS: usize = 1000;
const WARMUP: usize = 50;

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

#[derive(Default)]
struct PhaseSamples {
    pgen_parse: Vec<Duration>,
    compile_ast: Vec<Duration>,
    engine_new: Vec<Duration>,
    full_compile: Vec<Duration>,
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn percentile(samples: &mut [Duration], p: f64) -> Duration {
    samples.sort();
    let idx = ((samples.len() as f64 - 1.0) * p).round() as usize;
    samples[idx]
}

fn mean(samples: &[Duration]) -> Duration {
    let total: u128 = samples.iter().map(|d| d.as_nanos()).sum();
    Duration::from_nanos((total / samples.len() as u128) as u64)
}

fn measure_pattern(pattern: &str) -> PhaseSamples {
    let mut samples = PhaseSamples::default();

    // Warm up — first few compiles allocate jit/DFA caches and
    // hit cold paths in the allocator. Skip those samples.
    for _ in 0..WARMUP {
        let _ = Regex::compile(pattern);
    }

    for _ in 0..ITERATIONS {
        // Phase 1: PGEN parse.
        let t0 = Instant::now();
        let ast = rgx_core::parsing::parse_pattern(pattern).expect("parse");
        let pgen_elapsed = t0.elapsed();

        // Phase 2: AST → bytecode + classifier + C2 program build.
        let t1 = Instant::now();
        let compiled = Compiler::new().compile_ast(ast).expect("compile_ast");
        let compile_ast_elapsed = t1.elapsed();

        // Phase 3: Engine construction (DFA caches + JIT codegen).
        let t2 = Instant::now();
        let _engine = Engine::new(&compiled).expect("engine");
        let engine_elapsed = t2.elapsed();

        // Full Regex::compile for a control. We measure it
        // independently because phase-1+2+3 has measurement overhead
        // that a single Regex::compile doesn't.
        let t3 = Instant::now();
        let _re = Regex::compile(pattern).expect("compile");
        let full_elapsed = t3.elapsed();

        samples.pgen_parse.push(pgen_elapsed);
        samples.compile_ast.push(compile_ast_elapsed);
        samples.engine_new.push(engine_elapsed);
        samples.full_compile.push(full_elapsed);
    }

    samples
}

fn ns(d: Duration) -> u128 {
    d.as_nanos()
}

fn pct(part: Duration, total: Duration) -> f64 {
    if total.as_nanos() == 0 {
        0.0
    } else {
        (part.as_nanos() as f64 / total.as_nanos() as f64) * 100.0
    }
}

fn main() {
    println!("# Compile-phase split — median over {ITERATIONS} samples (after {WARMUP} warmup)");
    println!();
    println!("Phases (timed individually):");
    println!("  - PGEN parse        — `parsing::parse_pattern`");
    println!("  - compile_ast       — `Compiler::new().compile_ast(ast)`");
    println!(
        "                          (assignment passes + lower + bytecode + classifier + C2 NFAs)"
    );
    println!("  - Engine::new       — DFA caches (×3) + JIT codegen");
    println!("  - full Regex::compile — independent end-to-end measurement (control)");
    println!();
    println!(
        "{:<22} {:>12} {:>12} {:>12} {:>14} | {:>10} {:>12} {:>10} {:>14}",
        "pattern",
        "pgen (ns)",
        "ast (ns)",
        "engine (ns)",
        "phase-sum (ns)",
        "full (ns)",
        "pgen/full",
        "ast/full",
        "engine/full"
    );
    println!(
        "{:-<22} {:->12} {:->12} {:->12} {:->14} | {:->10} {:->12} {:->10} {:->14}",
        "", "", "", "", "", "", "", "", ""
    );

    for (name, pattern) in PATTERNS {
        let mut samples = measure_pattern(pattern);
        let pgen = median(&mut samples.pgen_parse);
        let compile_ast = median(&mut samples.compile_ast);
        let engine_new = median(&mut samples.engine_new);
        let full = median(&mut samples.full_compile);
        let phase_sum = pgen + compile_ast + engine_new;

        println!(
            "{:<22} {:>12} {:>12} {:>12} {:>14} | {:>10} {:>11.1}% {:>9.1}% {:>13.1}%",
            name,
            ns(pgen),
            ns(compile_ast),
            ns(engine_new),
            ns(phase_sum),
            ns(full),
            pct(pgen, full),
            pct(compile_ast, full),
            pct(engine_new, full),
        );
    }

    // Per-pattern detail block: mean/p50/p99/min/max for PGEN.
    println!();
    println!("# PGEN parse-time distribution (per-pattern)");
    println!();
    println!(
        "{:<22} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "pattern", "min", "p50", "mean", "p99", "max", "samples"
    );
    println!(
        "{:-<22} {:->10} {:->10} {:->10} {:->10} {:->10} {:->10}",
        "", "", "", "", "", "", ""
    );
    for (name, pattern) in PATTERNS {
        let mut samples = measure_pattern(pattern);
        let min_ = *samples.pgen_parse.iter().min().unwrap();
        let max_ = *samples.pgen_parse.iter().max().unwrap();
        let p50 = median(&mut samples.pgen_parse);
        let p99 = percentile(&mut samples.pgen_parse, 0.99);
        let m = mean(&samples.pgen_parse);
        println!(
            "{:<22} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
            name,
            ns(min_),
            ns(p50),
            ns(m),
            ns(p99),
            ns(max_),
            samples.pgen_parse.len(),
        );
    }

    println!();
    println!("Notes:");
    println!("  - phase-sum vs full: the gap is timer overhead and any work the");
    println!("    full-compile path does that the phase-by-phase path doesn't");
    println!("    (e.g. unicode-name-escape rewriting, error wrapping).");
    println!("  - Decision rule: if PGEN >50% of `full Regex::compile`, the");
    println!("    achievable RGX-vs-PCRE2 compile gap is bounded by PGEN's own");
    println!("    parse speed and a PGEN-side fix is needed (per CLAUDE.md");
    println!("    `PGEN is the sole parser` rule, the fix lands in PGEN, not RGX).");
}
