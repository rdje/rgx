//! Stress and soak tests for the rgx regex engine.
//!
//! These tests run many iterations to shake out non-determinism, memory issues,
//! and edge cases.

use rgx_core::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// =========================================================================
// COMPILATION STRESS -- compile many patterns
// =========================================================================

#[test]
fn compile_1000_different_patterns() {
    for i in 0..1000 {
        let pattern = format!("test{i}|foo{i}|bar{i}");
        let re = Regex::compile(&pattern).unwrap();
        assert!(re.is_match(&format!("test{i}")));
    }
}

#[test]
fn compile_patterns_of_increasing_complexity() {
    for depth in 1..=30 {
        let mut pattern = "a".to_string();
        for _ in 0..depth {
            pattern = format!("({pattern})*");
        }
        // May fail for very deep patterns -- just verify no panic
        let _ = Regex::compile(&pattern);
    }
}

// =========================================================================
// MATCHING STRESS -- match many inputs
// =========================================================================

#[test]
fn match_10000_different_inputs() {
    let re = Regex::compile(r"\w{3,5}").unwrap();
    for i in 0..10_000 {
        let input = format!("word{} and more{} text{}", i, i * 2, i * 3);
        let matches = re.find_all(&input);
        assert!(!matches.is_empty(), "iteration {i} should have matches");
    }
}

#[test]
fn find_all_on_progressively_larger_inputs() {
    let re = Regex::compile(r"\d+").unwrap();
    for size in [10, 100, 1_000, 10_000, 100_000] {
        let input = "x1y".repeat(size);
        let matches = re.find_all(&input);
        assert_eq!(
            matches.len(),
            size,
            "expected {size} matches on input of size {}",
            size * 3
        );
    }
}

#[test]
fn is_match_rapid_fire() {
    let re = Regex::compile(r"[a-z]+").unwrap();
    for _ in 0..100_000 {
        assert!(re.is_match("hello"));
    }
}

// =========================================================================
// VARIABLE STRESS -- set/read thousands of variables
// =========================================================================

#[test]
fn set_1000_variables() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    for i in 0..1000 {
        re.set_var(&format!("var_{i}"), i as i64).unwrap();
    }
    re.register_native("check", |ctx| {
        // Verify a sample of variables
        for i in (0..1000).step_by(100) {
            let val = ctx.var_int(&format!("var_{i}"));
            assert_eq!(val, Some(i as i64), "var_{i} mismatch");
        }
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn overwrite_variable_rapidly() {
    let re = Regex::with_mode(r"(?{native:read})", ExecutionMode::Full).unwrap();
    let last = Arc::new(AtomicUsize::new(0));
    let l = last.clone();
    re.register_native("read", move |ctx| {
        let v = ctx.var_int("counter").unwrap_or(0);
        l.store(v as usize, Ordering::Relaxed);
        ExecResult::Success
    })
    .unwrap();

    for i in 0..10_000 {
        re.set_var("counter", i as i64).unwrap();
        re.is_match("x");
    }
    // Last value should be 9999
    assert_eq!(last.load(Ordering::Relaxed), 9999);
}

// =========================================================================
// CALLBACK STRESS -- callbacks called thousands of times
// =========================================================================

#[test]
fn callback_fires_for_every_match_in_large_find_all() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let re = Regex::with_mode(r"\w+(?{native:count})", ExecutionMode::Full).unwrap();
    re.register_native("count", move |_| {
        c.fetch_add(1, Ordering::Relaxed);
        ExecResult::Success
    })
    .unwrap();

    let input = "word ".repeat(5000);
    let matches = re.find_all(&input);
    assert_eq!(matches.len(), 5000);
    // Callback should have fired at least 5000 times
    // (may fire more due to backtracking)
    assert!(count.load(Ordering::Relaxed) >= 5000);
}

// =========================================================================
// EVENT STRESS -- observer under sustained load
// =========================================================================

#[test]
fn event_observer_under_sustained_matching() {
    let event_count = Arc::new(AtomicUsize::new(0));
    let ec = event_count.clone();
    let re = Regex::compile(r"\d+").unwrap();
    re.on_event(move |_| {
        ec.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    for _ in 0..1000 {
        re.find_all("abc 123 def 456 ghi 789");
    }
    // Should have fired thousands of events
    assert!(event_count.load(Ordering::Relaxed) > 1000);
}

// =========================================================================
// CONCURRENT STRESS -- many threads, sustained
// =========================================================================

#[test]
fn concurrent_matching_sustained() {
    use std::thread;
    let re = Arc::new(Regex::compile(r"\w+").unwrap());
    let total_matches = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];
    for _ in 0..8 {
        let re = re.clone();
        let total = total_matches.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..1000 {
                let matches = re.find_all("hello world foo bar baz");
                total.fetch_add(matches.len(), Ordering::Relaxed);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
    // 8 threads x 1000 iterations x 5 words = 40,000 expected
    assert_eq!(total_matches.load(Ordering::Relaxed), 40_000);
}

// =========================================================================
// FILE STRESS -- scan large files
// =========================================================================

#[test]
fn scan_100k_line_file() {
    let path = std::env::temp_dir().join("rgx_stress_100k.txt");
    let mut content = String::new();
    for i in 0..100_000 {
        content.push_str(&format!("line {i}: data\n"));
    }
    std::fs::write(&path, &content).unwrap();

    let re = Regex::compile(r"\d+").unwrap();
    let count = re.scan_file_lines(&path).unwrap();
    assert_eq!(count, 100_000);

    std::fs::remove_file(&path).ok();
}

// =========================================================================
// ASYNC STRESS -- many suspensions
// =========================================================================

#[test]
fn suspend_resume_100_times() {
    let re = Regex::with_mode(r"x(?{native:check})", ExecutionMode::Full).unwrap();
    // Don't register "check" -- it will suspend

    for _ in 0..100 {
        match re.find_first_suspendable("x") {
            MatchOutcome::Suspended(cont) => match re.resume(*cont, ExecResult::Success) {
                MatchOutcome::Completed(Some(m)) => assert_eq!((m.start, m.end), (0, 1)),
                other => panic!("expected match, got {:?}", other),
            },
            other => panic!("expected suspension, got {:?}", other),
        }
    }
}

// =========================================================================
// STEERING STRESS
// =========================================================================

#[test]
fn steering_skip_across_large_input() {
    let re = Regex::with_mode(r"(?{native:skip10}).{10}", ExecutionMode::Full).unwrap();
    re.register_native("skip10", |_| ExecResult::Steer(SteerResult::Skip(10)))
        .unwrap();
    let input = "a".repeat(100);
    let m = re.find_first(&input);
    // Skip 10, then match 10 chars
    assert!(m.is_some());
}

// =========================================================================
// FUZZ-STYLE -- random valid patterns against random inputs
// =========================================================================

#[test]
fn fuzz_simple_patterns_against_random_ascii() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let patterns = [
        r"\d+",
        r"\w+",
        r"\s+",
        r"[a-z]+",
        r"[A-Z]+",
        r"\d{2,4}",
        r"(a|b|c)+",
        r"[^0-9]+",
        r".+",
        r"[a-z]{1,3}",
    ];

    for seed in 0..1000u64 {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        let hash = hasher.finish();

        let pattern = patterns[(hash % patterns.len() as u64) as usize];
        let input_len = (hash % 200) as usize;
        let input: String = (0..input_len)
            .map(|i| {
                let byte = ((hash.wrapping_add(i as u64)) % 95 + 32) as u8;
                byte as char
            })
            .collect();

        let re = Regex::compile(pattern).unwrap();
        let _ = re.find_first(&input);
        let _ = re.find_all(&input);
        let _ = re.is_match(&input);
        // No panic = pass
    }
}

#[test]
fn fuzz_random_regex_compilation() {
    // Generate random strings that MIGHT be valid regex
    let chars = "abcdefghijklmnopqrstuvwxyz0123456789.*+?|()[]{}^$\\dws";
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    for seed in 0..5000u64 {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        let hash = hasher.finish();

        let len = (hash % 30) as usize + 1;
        let pattern: String = (0..len)
            .map(|i| {
                let idx = ((hash.wrapping_add(i as u64 * 7)) % chars.len() as u64) as usize;
                chars.as_bytes()[idx] as char
            })
            .collect();

        // Just verify no panic -- Ok or Err is fine
        let _ = Regex::compile(&pattern);
    }
}

// =========================================================================
// ALTERNATION STRESS -- many branches
// =========================================================================

#[test]
fn alternation_with_100_branches() {
    // Use fixed-width branch names to avoid prefix-match ambiguity
    // (alternation is ordered: "branch1" would match before "branch10")
    let branches: Vec<String> = (0..100).map(|i| format!("br{i:03}")).collect();
    let pattern = branches.join("|");
    let re = Regex::compile(&pattern).unwrap();

    for i in 0..100 {
        let input = format!("br{i:03}");
        assert!(re.is_match(&input), "should match br{i:03}");
        let m = re.find_first(&input).unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, input.len());
    }
}

// =========================================================================
// REPETITION STRESS -- deeply nested quantifiers
// =========================================================================

#[test]
fn quantifier_on_long_input() {
    let re = Regex::compile(r"a+").unwrap();
    let input = "a".repeat(50_000);
    let m = re.find_first(&input).unwrap();
    assert_eq!(m.start, 0);
    assert_eq!(m.end, 50_000);
}

#[test]
fn find_all_many_small_matches() {
    let re = Regex::compile(r"a").unwrap();
    let input = "a".repeat(10_000);
    let matches = re.find_all(&input);
    assert_eq!(matches.len(), 10_000);
}

// =========================================================================
// UNICODE STRESS -- multi-byte characters
// =========================================================================

#[test]
fn unicode_multibyte_boundary_safety() {
    let re = Regex::compile(r".+").unwrap();
    // Mix of 1-byte, 2-byte, 3-byte, and 4-byte characters
    let inputs = [
        "\u{00e9}\u{00e9}\u{00e9}", // 2-byte: e with acute
        "\u{4e16}\u{754c}",         // 3-byte: CJK
        "\u{1f600}\u{1f601}",       // 4-byte: emoji
        "a\u{00e9}b\u{4e16}c\u{1f600}d",
    ];
    for input in &inputs {
        if let Some(m) = re.find_first(input) {
            assert!(m.start <= m.end);
            assert!(m.end <= input.len());
            assert!(input.is_char_boundary(m.start));
            assert!(input.is_char_boundary(m.end));
        }
    }
}

// =========================================================================
// EMPTY / EDGE CASES
// =========================================================================

#[test]
fn empty_input_across_many_patterns() {
    let patterns = [r"\d+", r"\w+", r"[a-z]+", r".+", r"(a|b)+", r"x{1,3}"];
    for p in &patterns {
        let re = Regex::compile(p).unwrap();
        assert!(
            !re.is_match(""),
            "pattern '{}' should not match empty input",
            p
        );
        assert!(re.find_all("").is_empty());
        assert!(re.find_first("").is_none());
    }
}

#[test]
fn single_char_input_stress() {
    let re = Regex::compile(r".").unwrap();
    for byte in 32u8..=126 {
        let input = String::from(byte as char);
        let m = re.find_first(&input).unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 1);
    }
}

// =========================================================================
// DEEP-NESTING STACK SAFETY -- a deeply nested pattern must never abort the
// host process. PGEN's generated recursive-descent parser recurses once per
// `(` level with no internal guard; RGX bounds the nesting deterministically
// (clean error past the limit) and runs the within-limit parse + compile on
// a guaranteed-deep stack. These tests run on the default libtest thread
// stack -- the exact environment that previously SIGABRT'd. The limit is
// `crate::recursion::MAX_NESTING_DEPTH` (1000); tests use concrete depths
// around it and must NOT hard-code an exported constant (kept crate-private).
// =========================================================================

/// Build `(((…(a)*…)*)*` nested `depth` levels deep.
fn nested_star_pattern(depth: usize) -> String {
    let mut pattern = String::from("a");
    for _ in 0..depth {
        pattern = format!("({pattern})*");
    }
    pattern
}

#[test]
fn deeply_nested_within_limit_does_not_abort() {
    // Well under the 1000 limit but far past the ~16 depth that used to
    // overflow a default libtest thread stack. Must return (Ok or Err)
    // without crashing the process.
    let pattern = nested_star_pattern(200);
    let _ = Regex::compile(&pattern);
}

#[test]
fn pattern_nested_past_limit_returns_clean_error() {
    // Comfortably past MAX_NESTING_DEPTH (1000). Must be rejected with a
    // clean compile error -- never a panic or process abort.
    let pattern = nested_star_pattern(1500);
    match Regex::compile(&pattern) {
        Ok(_) => panic!("a pattern nested 1500 levels deep must be rejected, not compiled"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("nesting too deep"),
                "expected the deterministic nesting-limit error, got: {msg}"
            );
        }
    }
}

#[test]
fn pattern_nested_just_under_limit_is_not_limit_rejected() {
    // 900 < 1000: RGX's deterministic pre-PGEN nesting guard must NOT
    // fire here. (As of PGEN 1.1.77 / PGEN-RGX-0085, PGEN's own stricter
    // 250-deep ceiling rejects 900 first with a *different*, clean parse
    // error — that is fine and expected; what this test pins is that
    // RGX's own "nesting too deep" 1000-guard is not what trips, and
    // that the process never aborts.)
    let pattern = nested_star_pattern(900);
    if let Err(e) = Regex::compile(&pattern) {
        assert!(
            !e.to_string().contains("nesting too deep"),
            "depth 900 (< limit) must not trip the nesting guard"
        );
    }
}

#[test]
fn from_ast_deeply_nested_does_not_abort() {
    // The parser-bypass path: `Regex::from_ast` skips the pre-PGEN scan,
    // so its stack safety rests entirely on the deep-stack compile
    // wrapper. A trusted-but-deep hand-built AST must not abort.
    use rgx_core::ast::{GroupKind, Regex as RegexAst};
    let mut ast = RegexAst::Char('a');
    for _ in 0..800 {
        ast = RegexAst::Group {
            expr: Box::new(ast),
            kind: GroupKind::NonCapturing,
            index: None,
            name: None,
        };
    }
    // Returns Ok or Err depending on downstream limits; the contract under
    // test is "no stack overflow / no process abort".
    let _ = Regex::from_ast(ast);
}
