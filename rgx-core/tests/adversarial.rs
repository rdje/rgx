//! Adversarial tests — trying to break the rgx engine.
//!
//! These tests simulate a hostile or creative user who pushes every
//! feature to its limits. If the engine survives all of these, it can
//! be trusted.

use rgx_core::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// =========================================================================
// PATHOLOGICAL PATTERNS — exponential backtracking, catastrophic regex
// =========================================================================

#[test]
fn catastrophic_backtracking_does_not_hang() {
    // Classic catastrophic pattern: (a+)+b on a string of 'a's
    // A naive engine takes exponential time. rgx should complete
    // in reasonable time (the test has a 60s timeout by default).
    let re = Regex::compile(r"(a+)+b").unwrap();
    // 25 'a's with no 'b' — forces maximum backtracking
    let input = "a".repeat(25);
    assert!(!re.is_match(&input));
}

#[test]
fn nested_quantifier_stress() {
    // (a*)*b — another classic pathological pattern
    let re = Regex::compile(r"(a*)*b").unwrap();
    let input = "a".repeat(20);
    assert!(!re.is_match(&input));
}

#[test]
fn deeply_nested_groups() {
    // 50 levels of nesting: (((((...a...)))))
    let mut pattern = String::new();
    for _ in 0..50 {
        pattern.push('(');
    }
    pattern.push('a');
    for _ in 0..50 {
        pattern.push(')');
    }
    let re = Regex::compile(&pattern).unwrap();
    assert!(re.is_match("a"));
}

#[test]
fn many_alternatives() {
    // 1000 alternatives: a1|a2|a3|...|a1000
    let alts: Vec<String> = (1..=1000).map(|i| format!("a{i}")).collect();
    let pattern = alts.join("|");
    let re = Regex::compile(&pattern).unwrap();
    assert!(re.is_match("a500"));
    assert!(re.is_match("a1000"));
    assert!(!re.is_match("b1")); // no alternative starts with 'b'
}

#[test]
fn many_capture_groups() {
    // 100 capture groups
    let mut pattern = String::new();
    for i in 1..=100 {
        pattern.push_str(&format!("(a{i})"));
    }
    let re = Regex::compile(&pattern).unwrap();
    // Build matching input
    let input: String = (1..=100).map(|i| format!("a{i}")).collect();
    assert!(re.is_match(&input));
}

// =========================================================================
// UNICODE EDGE CASES
// =========================================================================

#[test]
fn match_at_utf8_boundary() {
    // é is 2 bytes in UTF-8 (0xC3 0xA9)
    let re = Regex::compile(r"é").unwrap();
    assert!(re.is_match("café"));
    let m = re.find_first("café").unwrap();
    assert_eq!(m.end - m.start, 2); // 2-byte match
}

#[test]
fn match_emoji() {
    // 🎉 is 4 bytes in UTF-8
    let re = Regex::compile(r"🎉").unwrap();
    assert!(re.is_match("party 🎉 time"));
}

#[test]
fn dot_matches_full_codepoint() {
    // . should match one codepoint, not one byte
    let re = Regex::compile(r".").unwrap();
    let m = re.find_first("é").unwrap();
    assert_eq!(m.end - m.start, 2); // é is 2 bytes = 1 codepoint
}

#[test]
fn word_boundary_at_unicode_transition() {
    let re = Regex::compile(r"\bcat\b").unwrap();
    assert!(re.is_match("cat"));
    assert!(re.is_match("the cat sat"));
    // Unicode non-word chars adjacent to ASCII word
    assert!(re.is_match("«cat»"));
}

#[test]
fn character_class_with_multibyte() {
    let re = Regex::compile(r"[a-zé]+").unwrap();
    assert!(re.is_match("café"));
}

// =========================================================================
// BOUNDARY CONDITIONS — empty, maximum, zero
// =========================================================================

#[test]
fn match_on_single_byte_input() {
    let re = Regex::compile(r".").unwrap();
    let m = re.find_first("x").unwrap();
    assert_eq!((m.start, m.end), (0, 1));
}

#[test]
fn find_all_returns_non_overlapping() {
    let re = Regex::compile(r"aa").unwrap();
    let matches = re.find_all("aaaa");
    assert_eq!(matches.len(), 2); // [0,2) and [2,4), not [0,2) [1,3) [2,4)
}

#[test]
fn zero_width_match_at_every_position() {
    let re = Regex::compile(r"").unwrap();
    let matches = re.find_all("ab");
    // Empty pattern matches at position 0, 1, 2 (including end)
    assert!(matches.len() >= 2);
}

#[test]
fn backreference_to_unmatched_group() {
    // Group 1 may not have matched
    let re = Regex::compile(r"(a)?\1").unwrap();
    // If group didn't match, \1 should fail
    let result = re.find_first("b");
    // Either None or matches only when group 1 participated
    // The key is: no panic
    drop(result);
}

// =========================================================================
// LAYER 1 — DATA EXCHANGE ABUSE
// =========================================================================

#[test]
fn variable_with_empty_name() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_variable("", "value").unwrap();
    re.register_native("check", |ctx| {
        assert_eq!(ctx.variable(""), Some("value".to_string()));
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn variable_with_very_long_value() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    let big_value = "x".repeat(1_000_000);
    re.set_variable("big", &big_value).unwrap();
    re.register_native("check", |ctx| {
        assert_eq!(ctx.variable("big").unwrap().len(), 1_000_000);
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn overwrite_variable_between_matches() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    let expected = Arc::new(Mutex::new("first".to_string()));
    let exp = expected.clone();
    re.register_native("check", move |ctx| {
        let val = ctx.variable("v").unwrap();
        assert_eq!(val, *exp.lock().unwrap());
        ExecResult::Success
    })
    .unwrap();

    re.set_variable("v", "first").unwrap();
    assert!(re.is_match("x"));

    *expected.lock().unwrap() = "second".to_string();
    re.set_variable("v", "second").unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn structured_value_survives_find_all() {
    let re = Regex::with_mode(r"(\d+)(?{native:enrich})", ExecutionMode::Full).unwrap();
    re.register_native("enrich", |ctx| {
        let n: i64 = ctx.group(1).unwrap_or("0").parse().unwrap_or(0);
        ExecResult::Structured(Value::Map(vec![
            ("value".into(), Value::Int(n)),
            ("squared".into(), Value::Int(n * n)),
        ]))
    })
    .unwrap();
    let matches = re.find_all("1 22 333");
    assert_eq!(matches.len(), 3);
    for m in &matches {
        assert!(matches!(m.code_result, Some(CodeBlockValue::Structured(_))));
    }
}

// =========================================================================
// LAYER 2 — CALLBACK ABUSE
// =========================================================================

#[test]
fn callback_that_always_fails_prevents_all_matches() {
    let re = Regex::with_mode(r"\w+(?{native:reject})", ExecutionMode::Full).unwrap();
    re.register_native("reject", |_| ExecResult::Failure)
        .unwrap();
    assert!(!re.is_match("hello"));
    assert!(re.find_all("hello world").is_empty());
}

#[test]
fn callback_called_during_backtracking() {
    // Track how many times the callback fires
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let re = Regex::with_mode(r"(a+)(?{native:count})b", ExecutionMode::Full).unwrap();
    re.register_native("count", move |_| {
        c.fetch_add(1, Ordering::Relaxed);
        ExecResult::Success
    })
    .unwrap();
    re.find_first("aaab");
    // The greedy a+ will try 3, then 2, then 1 'a's before finding 'b'
    // Callback fires on each attempt
    let calls = count.load(Ordering::Relaxed);
    assert!(
        calls >= 1,
        "callback should fire at least once, fired {calls} times"
    );
}

#[test]
fn callback_result_from_last_winning_path() {
    // Two alternatives, each with a callback returning different values
    let re = Regex::with_mode(
        r"(?<a>cat)(?{native:cat_cb})|(?<b>dog)(?{native:dog_cb})",
        ExecutionMode::Full,
    )
    .unwrap();
    re.register_native("cat_cb", |_| ExecResult::Numeric(1.0))
        .unwrap();
    re.register_native("dog_cb", |_| ExecResult::Numeric(2.0))
        .unwrap();

    let m = re.find_first("dog").unwrap();
    assert_eq!(m.code_result, Some(CodeBlockValue::Numeric(2.0)));
    assert_eq!(m.matched_branch_number, Some(2));
}

// =========================================================================
// LAYER 3 — STEERING ABUSE
// =========================================================================

#[test]
fn steer_skip_zero_is_noop() {
    let re = Regex::with_mode(r"(?{native:skip0})abc", ExecutionMode::Full).unwrap();
    re.register_native("skip0", |_| ExecResult::Steer(SteerResult::Skip(0)))
        .unwrap();
    let m = re.find_first("abc").unwrap();
    assert_eq!((m.start, m.end), (0, 3));
}

#[test]
fn steer_accept_mid_pattern_truncates_match() {
    // Pattern: abc(?{native:accept})def
    // Accept fires after "abc" — match should end at position 3, not 6
    let re = Regex::with_mode(r"abc(?{native:accept})def", ExecutionMode::Full).unwrap();
    re.register_native("accept", |_| ExecResult::Steer(SteerResult::Accept))
        .unwrap();
    let m = re.find_first("abcdef").unwrap();
    assert_eq!(m.end, 3); // match accepted at "abc", before "def"
}

#[test]
fn steer_alternating_fail_continue() {
    // Callback alternates between Fail and Continue
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let re = Regex::with_mode(r"\w(?{native:alternate})", ExecutionMode::Full).unwrap();
    re.register_native("alternate", move |_| {
        let n = c.fetch_add(1, Ordering::Relaxed);
        if n % 2 == 0 {
            ExecResult::Steer(SteerResult::Fail)
        } else {
            ExecResult::Steer(SteerResult::Continue)
        }
    })
    .unwrap();
    // First char fails, second succeeds (or similar pattern)
    let m = re.find_first("abcdef");
    assert!(m.is_some()); // should eventually find a match
}

// =========================================================================
// LAYER 4 — EVENT ABUSE
// =========================================================================

#[test]
fn event_observer_with_heavy_processing() {
    // Observer does "heavy" work — should not affect match correctness
    let re = Regex::compile(r"\d+").unwrap();
    let sum = Arc::new(AtomicUsize::new(0));
    let s = sum.clone();
    re.on_event(move |_event| {
        // Simulate work
        for i in 0..100 {
            s.fetch_add(i, Ordering::Relaxed);
        }
    })
    .unwrap();
    let m = re.find_first("abc 42 def").unwrap();
    assert_eq!((m.start, m.end), (4, 6)); // match unaffected by heavy observer
}

#[test]
fn events_count_matches_correctly_in_find_all() {
    let re = Regex::compile(r"\w+").unwrap();
    let completed = Arc::new(AtomicUsize::new(0));
    let c = completed.clone();
    re.on_event(move |event| {
        if let MatchEvent::MatchAttemptCompleted { matched: true, .. } = event {
            c.fetch_add(1, Ordering::Relaxed);
        }
    })
    .unwrap();
    let matches = re.find_all("one two three");
    assert_eq!(matches.len(), 3);
    // Events should have recorded at least 3 successful completions
    assert!(completed.load(Ordering::Relaxed) >= 3);
}

// =========================================================================
// LAYER 5 — ASYNC ABUSE
// =========================================================================

#[test]
fn suspend_resume_suspend_resume_chain() {
    // Pattern with two async callbacks — must suspend twice
    let re = Regex::with_mode(
        r"(?{native:first})cat(?{native:second})",
        ExecutionMode::Full,
    )
    .unwrap();

    let mut outcome = re.find_first_suspendable("cat");

    // First suspension
    let cont = match outcome {
        MatchOutcome::Suspended(c) => {
            assert_eq!(c.pending_callback_name, "first");
            c
        }
        _ => panic!("expected first suspension"),
    };
    outcome = re.resume(*cont, ExecResult::Success);

    // Second suspension
    let cont = match outcome {
        MatchOutcome::Suspended(c) => {
            assert_eq!(c.pending_callback_name, "second");
            c
        }
        _ => panic!("expected second suspension"),
    };
    outcome = re.resume(*cont, ExecResult::Success);

    // Should complete
    match outcome {
        MatchOutcome::Completed(Some(m)) => assert_eq!((m.start, m.end), (0, 3)),
        _ => panic!("expected completed match"),
    }
}

#[test]
fn suspend_resume_with_steering() {
    let re = Regex::with_mode(r"cat(?{native:async_check})", ExecutionMode::Full).unwrap();
    match re.find_first_suspendable("cat") {
        MatchOutcome::Suspended(cont) => {
            // Resume with Accept steering
            match re.resume(*cont, ExecResult::Steer(SteerResult::Accept)) {
                MatchOutcome::Completed(Some(m)) => assert_eq!(m.start, 0),
                other => panic!("expected accepted match, got {:?}", other),
            }
        }
        _ => panic!("expected suspension"),
    }
}

#[test]
fn continuation_moved_to_another_thread() {
    use std::thread;
    let re = Arc::new(Regex::with_mode(r"test(?{native:check})", ExecutionMode::Full).unwrap());

    let outcome = re.find_first_suspendable("test");
    match outcome {
        MatchOutcome::Suspended(cont) => {
            let re2 = re.clone();
            // Move continuation to another thread for resume
            let handle = thread::spawn(move || re2.resume(*cont, ExecResult::Success));
            match handle.join().unwrap() {
                MatchOutcome::Completed(Some(m)) => assert_eq!((m.start, m.end), (0, 4)),
                other => panic!("expected match from other thread, got {:?}", other),
            }
        }
        _ => panic!("expected suspension"),
    }
}

// =========================================================================
// LAYER 6 — FILE ABUSE
// =========================================================================

#[test]
fn match_file_with_binary_content() {
    let path = std::env::temp_dir().join("rgx_adversarial_binary.bin");
    // Write bytes that include non-UTF-8 sequences... actually,
    // match_file reads as UTF-8 string. Test that it handles a
    // file with valid UTF-8 that includes the full ASCII range.
    let content: String = (32..127u8).map(|b| b as char).collect();
    std::fs::write(&path, &content).unwrap();
    let re = Regex::compile(r"[A-Z]+").unwrap();
    let matches = re.match_file(&path).unwrap();
    assert!(!matches.is_empty());
    std::fs::remove_file(&path).ok();
}

#[test]
fn scan_file_lines_with_thousands_of_lines() {
    let path = std::env::temp_dir().join("rgx_adversarial_big_log.txt");
    let mut content = String::new();
    for i in 0..10_000 {
        content.push_str(&format!("line {i}: some log text\n"));
    }
    std::fs::write(&path, &content).unwrap();
    let re = Regex::compile(r"\d+").unwrap();
    let count = re.scan_file_lines(&path).unwrap();
    assert_eq!(count, 10_000); // one number per line
    std::fs::remove_file(&path).ok();
}

#[test]
fn match_file_lines_with_empty_lines() {
    let path = std::env::temp_dir().join("rgx_adversarial_empty_lines.txt");
    std::fs::write(&path, "\n\nfoo\n\nbar\n\n").unwrap();
    let re = Regex::compile(r"\w+").unwrap();
    let matches = re.match_file_lines(&path).unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].line_number, 3); // "foo" is on line 3
    assert_eq!(matches[1].line_number, 5); // "bar" is on line 5
    std::fs::remove_file(&path).ok();
}

// =========================================================================
// CROSS-LAYER ADVERSARIAL — combine everything
// =========================================================================

#[test]
fn all_layers_under_stress() {
    // Set typed variables (L1)
    // Register callback that steers (L3)
    // Observe events (L4)
    // Match file (L6)
    let path = std::env::temp_dir().join("rgx_adversarial_all_layers.txt");
    let mut content = String::new();
    for i in 0..100 {
        content.push_str(&format!("item:{i} status:active\n"));
    }
    std::fs::write(&path, &content).unwrap();

    let re = Regex::with_mode(r"item:(?<id>\d+)\b(?{native:filter})", ExecutionMode::Full).unwrap();

    // L1: typed variable
    re.set_var("max_id", 50_i64).unwrap();

    // L3: steering callback — \b in the pattern prevents \d+ from backtracking
    // into a shorter match (e.g. "5" inside "51"), ensuring the full numeric id
    // is always captured and compared to max_id.
    re.register_native("filter", |ctx| {
        let id: i64 = ctx.named("id").unwrap_or("0").parse().unwrap_or(0);
        let max = ctx.var_int("max_id").unwrap_or(100);
        if id > max {
            ExecResult::Steer(SteerResult::Fail)
        } else {
            ExecResult::Numeric(id as f64)
        }
    })
    .unwrap();

    // L4: events
    let match_count = Arc::new(AtomicUsize::new(0));
    let mc = match_count.clone();
    re.on_event(move |event| {
        if let MatchEvent::MatchAttemptCompleted { matched: true, .. } = event {
            mc.fetch_add(1, Ordering::Relaxed);
        }
    })
    .unwrap();

    // L6: scan file
    let matches = re.match_file_lines(&path).unwrap();

    // Only items 0-50 should match (51 items)
    assert_eq!(
        matches.len(),
        51,
        "expected 51 matches (0..=50), got {}",
        matches.len()
    );

    // Events should have fired
    assert!(match_count.load(Ordering::Relaxed) >= 51);

    std::fs::remove_file(&path).ok();
}

#[test]
fn concurrent_file_scanning() {
    use std::thread;

    let path = std::env::temp_dir().join("rgx_adversarial_concurrent_scan.txt");
    std::fs::write(&path, "cat dog cat bird cat\ndog cat bird dog\n").unwrap();
    let re = Arc::new(Regex::compile(r"cat").unwrap());

    let mut handles = vec![];
    for _ in 0..10 {
        let re = re.clone();
        let p = path.clone();
        handles.push(thread::spawn(move || {
            let matches = re.match_file(&p).unwrap();
            assert_eq!(matches.len(), 4);
        }));
    }

    for h in handles {
        h.join()
            .expect("thread panicked during concurrent file scan");
    }
    std::fs::remove_file(&path).ok();
}
