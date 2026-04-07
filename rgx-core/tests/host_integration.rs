//! Comprehensive host integration tests for the rgx regex engine.
//!
//! Exercises every user-facing API across all 6 host integration layers:
//!
//! 1. Data Exchange (variables, typed values, macros)
//! 2. Callbacks (native callback registration)
//! 3. Match Steering (SteerResult variants in find_all context)
//! 4. Structured Events (MatchEvent observer)
//! 5. Async I/O (suspendable matching)
//! 6. File Matching (match_file, match_file_lines, scan_file_lines)
//!
//! Plus cross-layer integration tests that exercise multiple layers together.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rgx_core::{
    value, vars, CodeBlockValue, ExecResult, ExecutionMode, MatchEvent, MatchOutcome, Regex,
    SteerResult, Value,
};

// ============================================================================
// LAYER 1 -- Data Exchange
// ============================================================================

#[test]
fn var_int_reader() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_var("count", 99_i64).unwrap();
    re.register_native("check", |ctx| {
        assert_eq!(ctx.var_int("count"), Some(99));
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn var_float_reader() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_var("rate", 3.14_f64).unwrap();
    re.register_native("check", |ctx| {
        assert_eq!(ctx.var_float("rate"), Some(3.14));
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn var_bool_reader() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_var("debug", true).unwrap();
    re.register_native("check", |ctx| {
        assert_eq!(ctx.var_bool("debug"), Some(true));
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn var_str_reader() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_var("name", "alice").unwrap();
    re.register_native("check", |ctx| {
        assert_eq!(ctx.var_str("name").as_deref(), Some("alice"));
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn var_null() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_typed_variable("empty", Value::Null).unwrap();
    re.register_native("check", |ctx| {
        let v = ctx.typed_variable("empty");
        assert_eq!(v, Some(Value::Null));
        // Typed accessors should return None for Null
        assert_eq!(ctx.var_int("empty"), None);
        assert_eq!(ctx.var_str("empty"), None);
        assert_eq!(ctx.var_bool("empty"), None);
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn var_empty_array() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    let empty: Vec<&str> = vec![];
    re.set_var("items", empty).unwrap();
    re.register_native("check", |ctx| {
        let arr = ctx.var_array("items").unwrap();
        assert!(arr.is_empty());
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn var_empty_map() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    let empty: Vec<(&str, &str)> = vec![];
    re.set_var("config", empty).unwrap();
    re.register_native("check", |ctx| {
        let map = ctx.var_map("config").unwrap();
        assert!(map.is_empty());
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn value_macro_deeply_nested() {
    let v = value!({
        "a" => {
            "b" => {
                "c" => [1_i64, 2_i64, {
                    "d" => true
                }]
            }
        }
    });
    // Verify structure
    let a_map = v.as_map().unwrap();
    assert_eq!(a_map.len(), 1);
    assert_eq!(a_map[0].0, "a");
    let b_map = a_map[0].1.as_map().unwrap();
    assert_eq!(b_map[0].0, "b");
    let c_map = b_map[0].1.as_map().unwrap();
    assert_eq!(c_map[0].0, "c");
    let arr = c_map[0].1.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0].as_i64(), Some(1));
    assert_eq!(arr[1].as_i64(), Some(2));
    let inner_map = arr[2].as_map().unwrap();
    assert_eq!(inner_map[0].0, "d");
    assert_eq!(inner_map[0].1.as_bool(), Some(true));
}

#[test]
fn vars_macro_and_set_vars_equivalent() {
    // Path A: use vars! macro
    let re_a = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    vars!(re_a, {
        "env" => "prod",
        "port" => 8080_i64,
        "tags" => ["a", "b"]
    });
    // Path B: use set_vars with value! macro
    let re_b = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re_b.set_vars(value!({
        "env" => "prod",
        "port" => 8080_i64,
        "tags" => ["a", "b"]
    }));

    // Verify both produce identical results
    for re in [&re_a, &re_b] {
        re.register_native("check", |ctx| {
            assert_eq!(ctx.var_str("env").as_deref(), Some("prod"));
            assert_eq!(ctx.var_int("port"), Some(8080));
            let tags = ctx.var_array("tags").unwrap();
            assert_eq!(tags.len(), 2);
            assert_eq!(tags[0].as_str(), Some("a"));
            assert_eq!(tags[1].as_str(), Some("b"));
            ExecResult::Success
        })
        .unwrap();
        assert!(re.is_match("x"));
    }
}

#[test]
fn typed_variable_backward_compat_string_to_typed() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_variable("x", "hello").unwrap();
    re.register_native("check", |ctx| {
        // Old-style string variable should be readable via typed accessor
        let typed = ctx.typed_variable("x");
        assert!(typed.is_some());
        let val = typed.unwrap();
        assert_eq!(val.as_str(), Some("hello"));
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn typed_variable_backward_compat_typed_to_string() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_typed_variable("x", Value::Int(42)).unwrap();
    re.register_native("check", |ctx| {
        // Typed variable should be readable via old-style string accessor
        let str_val = ctx.variable("x");
        assert_eq!(str_val, Some("42".to_string()));
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}

#[test]
fn structured_result_in_find_all() {
    let re = Regex::with_mode(r"(?<n>\d+)(?{native:enrich})", ExecutionMode::Full).unwrap();
    re.register_native("enrich", |ctx| {
        let n: i64 = ctx.named("n").unwrap_or("0").parse().unwrap_or(0);
        ExecResult::Structured(Value::Map(vec![
            ("val".into(), Value::Int(n)),
            ("doubled".into(), Value::Int(n * 2)),
        ]))
    })
    .unwrap();
    let matches = re.find_all("10 20 30");
    assert_eq!(matches.len(), 3);
    for (i, expected) in [10_i64, 20, 30].iter().enumerate() {
        if let Some(CodeBlockValue::Structured(v)) = &matches[i].code_result {
            let map = v.as_map().unwrap();
            assert_eq!(map[0].1.as_i64(), Some(*expected));
            assert_eq!(map[1].1.as_i64(), Some(expected * 2));
        } else {
            panic!("expected Structured code_result for match {}", i);
        }
    }
}

#[test]
fn branch_number_with_find_all() {
    let re = Regex::compile("cat|dog|bird").unwrap();
    let matches = re.find_all("dog bird cat");
    assert_eq!(matches.len(), 3);
    // "dog" is branch 2, "bird" is branch 3, "cat" is branch 1
    assert_eq!(matches[0].matched_branch_number, Some(2));
    assert_eq!(matches[1].matched_branch_number, Some(3));
    assert_eq!(matches[2].matched_branch_number, Some(1));
}

#[test]
fn numeric_with_code_find_all() {
    let re = Regex::with_mode(r"(?<d>\d)(?{native:score})", ExecutionMode::Full).unwrap();
    re.register_native("score", |ctx| {
        let d: f64 = ctx.named("d").unwrap_or("0").parse().unwrap_or(0.0);
        ExecResult::Numeric(d * 10.0)
    })
    .unwrap();
    let nums = re.find_all_numeric_with_code("3a7b9");
    assert_eq!(nums, vec![30.0, 70.0, 90.0]);
}

#[test]
fn replace_with_code_multiple() {
    let re = Regex::with_mode(r"(?<w>\w+)(?{native:upper})", ExecutionMode::Full).unwrap();
    re.register_native("upper", |ctx| {
        ExecResult::Replacement(ctx.named("w").unwrap_or("").to_uppercase())
    })
    .unwrap();
    let result = re.replace_all_with_code("hello world foo");
    assert_eq!(result, "HELLO WORLD FOO");
}

// ============================================================================
// LAYER 3 -- Match Steering
// ============================================================================

#[test]
fn steer_abort_with_find_all() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = call_count.clone();
    let re = Regex::with_mode(r"cat(?{native:limiter})", ExecutionMode::Full).unwrap();
    re.register_native("limiter", move |_ctx| {
        let n = cc.fetch_add(1, Ordering::SeqCst);
        if n >= 2 {
            ExecResult::Steer(SteerResult::Abort)
        } else {
            ExecResult::Success
        }
    })
    .unwrap();
    // Input has 4 occurrences of "cat"; abort after accepting 2
    let matches = re.find_all("cat cat cat cat");
    // First two succeed, third triggers abort; should get at most 2
    assert_eq!(matches.len(), 2);
}

#[test]
fn steer_skip_with_find_all() {
    let re = Regex::with_mode(r"(?{native:skip3})abc", ExecutionMode::Full).unwrap();
    re.register_native("skip3", |_ctx| ExecResult::Steer(SteerResult::Skip(3)))
        .unwrap();
    // At position 0: skip3 advances to 3, then "abc" matches at 3..6
    let matches = re.find_all("xxxabc");
    assert!(!matches.is_empty());
    assert_eq!(matches[0].start, 0);
    assert_eq!(matches[0].end, 6);
}

#[test]
fn steer_accept_preserves_captures() {
    let re = Regex::with_mode(
        r"(?<word>\w+)(?{native:accept_now})never_reached",
        ExecutionMode::Full,
    )
    .unwrap();
    re.register_native("accept_now", |ctx| {
        // Named capture "word" should be populated
        assert!(ctx.named("word").is_some());
        ExecResult::Steer(SteerResult::Accept)
    })
    .unwrap();
    let m = re.find_first("hello_world").unwrap();
    // Accept should force the match without needing "never_reached"
    assert_eq!(m.start, 0);
    // End position should be after the word capture
    assert!(m.end > 0);
}

#[test]
fn steer_fail_with_backtracking_alternation() {
    let re = Regex::with_mode(r"cat(?{native:reject})|dog", ExecutionMode::Full).unwrap();
    re.register_native("reject", |_ctx| ExecResult::Steer(SteerResult::Fail))
        .unwrap();
    // "cat" matches the literal but Steer::Fail backtracks, so "dog" should match
    let m = re.find_first("catdog").unwrap();
    assert_eq!(m.start, 3);
    assert_eq!(m.end, 6);
}

#[test]
fn steer_continue_does_not_affect_matching() {
    let re_with = Regex::with_mode(r"c.t(?{native:noop})", ExecutionMode::Full).unwrap();
    re_with
        .register_native("noop", |_ctx| ExecResult::Steer(SteerResult::Continue))
        .unwrap();
    let re_without = Regex::compile(r"c.t").unwrap();

    let input = "cat cot cut dog";
    let with_matches = re_with.find_all(input);
    let without_matches = re_without.find_all(input);

    assert_eq!(with_matches.len(), without_matches.len());
    for (a, b) in with_matches.iter().zip(without_matches.iter()) {
        assert_eq!(a.start, b.start);
        assert_eq!(a.end, b.end);
    }
}

// ============================================================================
// LAYER 4 -- Structured Events
// ============================================================================

#[test]
fn event_branch_entered() {
    let re = Regex::compile(r"c.t|d.g").unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    re.on_event(move |event| {
        ev.lock().unwrap().push(event.clone());
    })
    .unwrap();
    let _ = re.find_first("dog");
    let collected = events.lock().unwrap();
    let branch_events: Vec<_> = collected
        .iter()
        .filter(|e| matches!(e, MatchEvent::BranchEntered { .. }))
        .collect();
    assert!(
        !branch_events.is_empty(),
        "expected BranchEntered events for alternation pattern"
    );
}

#[test]
fn event_capture_completed() {
    let re = Regex::compile(r"(c.t)").unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    re.on_event(move |event| {
        ev.lock().unwrap().push(event.clone());
    })
    .unwrap();
    let _ = re.find_first("cat");
    let collected = events.lock().unwrap();
    let capture_events: Vec<_> = collected
        .iter()
        .filter(|e| matches!(e, MatchEvent::CaptureCompleted { .. }))
        .collect();
    assert!(
        !capture_events.is_empty(),
        "expected CaptureCompleted events for capturing group pattern"
    );
}

#[test]
fn event_code_block_evaluated() {
    let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
    re.register_native("check", |_ctx| ExecResult::Success)
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    re.on_event(move |event| {
        ev.lock().unwrap().push(event.clone());
    })
    .unwrap();
    let _ = re.find_first("cat");
    let collected = events.lock().unwrap();
    let code_events: Vec<_> = collected
        .iter()
        .filter(|e| matches!(e, MatchEvent::CodeBlockEvaluated { .. }))
        .collect();
    assert!(
        !code_events.is_empty(),
        "expected CodeBlockEvaluated events for native code block pattern"
    );
}

#[test]
fn events_during_find_all() {
    let re = Regex::compile(r"c.t").unwrap();
    let event_count = Arc::new(AtomicUsize::new(0));
    let ec = event_count.clone();
    re.on_event(move |event| {
        if matches!(
            event,
            MatchEvent::MatchAttemptCompleted { matched: true, .. }
        ) {
            ec.fetch_add(1, Ordering::SeqCst);
        }
    })
    .unwrap();
    let matches = re.find_all("cat cot cut");
    // Should have at least as many successful attempt completions as matches
    assert!(event_count.load(Ordering::SeqCst) >= matches.len());
}

#[test]
fn events_do_not_affect_results() {
    let input = "cat cot cut dog";
    let re_observed = Regex::compile(r"c.t").unwrap();
    let _events = Arc::new(Mutex::new(Vec::new()));
    let ev = _events.clone();
    re_observed
        .on_event(move |event| {
            ev.lock().unwrap().push(event.clone());
        })
        .unwrap();

    let re_plain = Regex::compile(r"c.t").unwrap();

    let observed_matches = re_observed.find_all(input);
    let plain_matches = re_plain.find_all(input);

    assert_eq!(observed_matches.len(), plain_matches.len());
    for (a, b) in observed_matches.iter().zip(plain_matches.iter()) {
        assert_eq!(a.start, b.start);
        assert_eq!(a.end, b.end);
        assert_eq!(a.matched_branch_number, b.matched_branch_number);
    }
}

#[test]
fn event_all_types_in_one_pattern() {
    // Pattern that triggers: MatchAttemptStarted, MatchAttemptCompleted,
    // BranchEntered, CaptureCompleted, BacktrackOccurred, CodeBlockEvaluated
    let re = Regex::with_mode(r"(a*a)(?{native:pass})|(b.b)", ExecutionMode::Full).unwrap();
    re.register_native("pass", |_ctx| ExecResult::Success)
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    re.on_event(move |event| {
        ev.lock().unwrap().push(event.clone());
    })
    .unwrap();
    // "aa" triggers backtracking (a* vs a) and code block; "bob" triggers branch 2
    let _ = re.find_all("aa bob");
    let collected = events.lock().unwrap();

    let has_attempt_started = collected
        .iter()
        .any(|e| matches!(e, MatchEvent::MatchAttemptStarted { .. }));
    let has_attempt_completed = collected
        .iter()
        .any(|e| matches!(e, MatchEvent::MatchAttemptCompleted { .. }));
    let has_branch = collected
        .iter()
        .any(|e| matches!(e, MatchEvent::BranchEntered { .. }));
    let has_capture = collected
        .iter()
        .any(|e| matches!(e, MatchEvent::CaptureCompleted { .. }));
    let has_backtrack = collected
        .iter()
        .any(|e| matches!(e, MatchEvent::BacktrackOccurred { .. }));
    let has_code = collected
        .iter()
        .any(|e| matches!(e, MatchEvent::CodeBlockEvaluated { .. }));

    assert!(has_attempt_started, "missing MatchAttemptStarted");
    assert!(has_attempt_completed, "missing MatchAttemptCompleted");
    assert!(has_branch, "missing BranchEntered");
    assert!(has_capture, "missing CaptureCompleted");
    assert!(has_backtrack, "missing BacktrackOccurred");
    assert!(has_code, "missing CodeBlockEvaluated");
}

// ============================================================================
// LAYER 5 -- Async I/O (Suspendable matching)
// ============================================================================

#[test]
fn suspendable_find_all_not_supported_gracefully() {
    // find_all does not support suspension (it runs synchronously).
    // When using find_all with an unregistered native, verify graceful
    // behavior: either empty results or that it completes without panic.
    let re = Regex::with_mode(r"cat(?{native:unregistered})", ExecutionMode::Full).unwrap();
    // Don't register "unregistered" -- find_all should still work
    let matches = re.find_all("cat dog cat");
    // The behavior is that unregistered callbacks in find_all may cause
    // the match to fail (no suspension pathway), so we just verify no panic
    // and the result is deterministic.
    let _ = matches;
}

#[test]
fn suspendable_abort_via_steering() {
    let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
    // Don't register "check" -- it will suspend
    match re.find_first_suspendable("cat dog") {
        MatchOutcome::Suspended(cont) => {
            assert_eq!(cont.pending_callback_name, "check");
            // Resume with Steer(Abort) -- should stop the search
            match re.resume(*cont, ExecResult::Steer(SteerResult::Abort)) {
                MatchOutcome::Completed(result) => {
                    // Abort should stop the search, resulting in no match
                    assert!(result.is_none(), "expected no match after abort steering");
                }
                MatchOutcome::Suspended(_) => {
                    panic!("expected completed after abort, got another suspension");
                }
            }
        }
        MatchOutcome::Completed(_) => {
            panic!("expected suspension on unregistered callback");
        }
    }
}

#[test]
fn suspendable_accept_via_steering() {
    let re = Regex::with_mode(r"cat(?{native:check})dog", ExecutionMode::Full).unwrap();
    // Don't register "check" -- it will suspend
    match re.find_first_suspendable("catdog") {
        MatchOutcome::Suspended(cont) => {
            assert_eq!(cont.pending_callback_name, "check");
            // Resume with Steer(Accept) -- should force-accept at current position
            match re.resume(*cont, ExecResult::Steer(SteerResult::Accept)) {
                MatchOutcome::Completed(result) => {
                    let m = result.expect("expected a match after accept steering");
                    assert_eq!(m.start, 0);
                    // Accept forces match before "dog" is consumed
                    assert_eq!(m.end, 3);
                }
                MatchOutcome::Suspended(_) => {
                    panic!("expected completed after accept, got suspension");
                }
            }
        }
        MatchOutcome::Completed(_) => {
            panic!("expected suspension on unregistered callback");
        }
    }
}

// ============================================================================
// LAYER 6 -- File Matching
// ============================================================================

fn temp_file(name: &str, content: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("rgx_host_integration_{}", name));
    std::fs::write(&path, content).unwrap();
    path
}

fn cleanup(path: &std::path::Path) {
    std::fs::remove_file(path).ok();
}

#[test]
fn match_file_no_matches() {
    let path = temp_file("no_match.txt", "hello world foo bar");
    let re = Regex::compile("zzzzz").unwrap();
    let matches = re.match_file(&path).unwrap();
    assert!(matches.is_empty());
    cleanup(&path);
}

#[test]
fn match_file_lines_empty_file() {
    let path = temp_file("empty.txt", "");
    let re = Regex::compile("cat").unwrap();
    let matches = re.match_file_lines(&path).unwrap();
    assert!(matches.is_empty());
    cleanup(&path);
}

#[test]
fn scan_file_lines_with_callbacks() {
    let path = temp_file("scan_cb.txt", "cat\ndog\ncat bird\n");
    let re = Regex::with_mode(r"cat(?{native:counter})", ExecutionMode::Full).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let cc = count.clone();
    re.register_native("counter", move |_ctx| {
        cc.fetch_add(1, Ordering::SeqCst);
        ExecResult::Success
    })
    .unwrap();
    let total = re.scan_file_lines(&path).unwrap();
    assert_eq!(total, 2);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    cleanup(&path);
}

#[test]
fn match_file_unicode_content() {
    let path = temp_file("unicode.txt", "cafe\u{0301} na\u{00ef}ve \u{00fc}ber");
    let re = Regex::compile(r"\w+").unwrap();
    let matches = re.match_file(&path).unwrap();
    assert!(
        !matches.is_empty(),
        "expected matches on UTF-8 multi-byte content"
    );
    cleanup(&path);
}

#[test]
fn match_file_lines_preserves_line_text() {
    let path = temp_file(
        "line_text.txt",
        "alpha cat beta\ngamma dog delta\nepsilon cat zeta",
    );
    let re = Regex::compile("cat").unwrap();
    let matches = re.match_file_lines(&path).unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].line, "alpha cat beta");
    assert_eq!(matches[0].line_number, 1);
    assert_eq!(matches[1].line, "epsilon cat zeta");
    assert_eq!(matches[1].line_number, 3);
    cleanup(&path);
}

// ============================================================================
// CROSS-LAYER INTEGRATION TESTS
// ============================================================================

#[test]
fn layer1_and_layer2_variables_in_callback() {
    // Layer 1: set variable; Layer 2: read it in callback
    let re = Regex::with_mode(r"(?<d>\d+)(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_var("threshold", 50_i64).unwrap();
    re.register_native("check", |ctx| {
        let threshold = ctx.var_int("threshold").unwrap_or(0);
        let val: i64 = ctx.named("d").unwrap_or("0").parse().unwrap_or(0);
        if val >= threshold {
            ExecResult::Success
        } else {
            ExecResult::Failure
        }
    })
    .unwrap();
    // 30 < 50 fails, 75 >= 50 succeeds
    assert!(!re.is_match("30"));
    assert!(re.is_match("75"));
}

#[test]
fn layer2_and_layer3_callback_steers_match() {
    // Layer 2: callback; Layer 3: it returns Steer
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = call_count.clone();
    let re = Regex::with_mode(r"cat(?{native:steer})", ExecutionMode::Full).unwrap();
    re.register_native("steer", move |_ctx| {
        let n = cc.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            // First time: fail
            ExecResult::Steer(SteerResult::Fail)
        } else {
            // Second time: accept
            ExecResult::Success
        }
    })
    .unwrap();
    // Input has two "cat" occurrences; first fails, second succeeds
    let m = re.find_first("cat cat").unwrap();
    assert_eq!(m.start, 4);
    assert_eq!(m.end, 7);
}

#[test]
fn layer1_and_layer4_branch_events() {
    // Layer 1: branch numbers; Layer 4: events
    let re = Regex::compile(r"c.t|d.g").unwrap();
    let branch_numbers = Arc::new(Mutex::new(Vec::new()));
    let bn = branch_numbers.clone();
    re.on_event(move |event| {
        if let MatchEvent::BranchEntered { branch, .. } = event {
            bn.lock().unwrap().push(*branch);
        }
    })
    .unwrap();
    let matches = re.find_all("dog cat");
    assert_eq!(matches.len(), 2);
    // Verify branch events were collected
    let collected = branch_numbers.lock().unwrap();
    assert!(
        !collected.is_empty(),
        "expected BranchEntered events with branch numbers"
    );
}

#[test]
fn layer2_and_layer6_callback_during_file_scan() {
    // Layer 2: register callback; Layer 6: scan file
    let path = temp_file("cb_scan.txt", "cat dog\nbird\ncat fish cat\n");
    let count = Arc::new(AtomicUsize::new(0));
    let cc = count.clone();
    let re = Regex::with_mode(r"cat(?{native:bump})", ExecutionMode::Full).unwrap();
    re.register_native("bump", move |_ctx| {
        cc.fetch_add(1, Ordering::SeqCst);
        ExecResult::Success
    })
    .unwrap();
    let total = re.scan_file_lines(&path).unwrap();
    assert_eq!(total, 3);
    assert_eq!(count.load(Ordering::SeqCst), 3);
    cleanup(&path);
}

#[test]
fn all_layers_combined() {
    // Layer 1: set variable
    // Layer 2: register callback
    // Layer 3: callback uses steering
    // Layer 4: observe events
    // Layer 6: match file
    let path = temp_file(
        "all_layers.txt",
        "cat 10\ndog 20\ncat 30\ncat 40\nbird 50\n",
    );
    let re = Regex::with_mode(r"cat(?{native:filter})", ExecutionMode::Full).unwrap();
    re.set_var("max_matches", 2_i64).unwrap();

    let match_count = Arc::new(AtomicUsize::new(0));
    let mc = match_count.clone();
    let max_val = Arc::new(AtomicUsize::new(2));

    re.register_native("filter", move |ctx| {
        let max = ctx.var_int("max_matches").unwrap_or(0) as usize;
        // Update max_val for comparison
        max_val.store(max, Ordering::SeqCst);
        let current = mc.fetch_add(1, Ordering::SeqCst);
        if current >= max {
            ExecResult::Steer(SteerResult::Fail)
        } else {
            ExecResult::Success
        }
    })
    .unwrap();

    // Layer 4: observe events
    let event_count = Arc::new(AtomicUsize::new(0));
    let ec = event_count.clone();
    re.on_event(move |_event| {
        ec.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();

    // Layer 6: match file
    let file_matches = re.match_file_lines(&path).unwrap();
    // Should get at most 2 successful matches (max_matches = 2)
    assert!(
        file_matches.len() <= 2,
        "expected at most 2 matches but got {}",
        file_matches.len()
    );
    // Events should have fired
    assert!(
        event_count.load(Ordering::SeqCst) > 0,
        "expected events to fire during file matching"
    );
    cleanup(&path);
}

// ============================================================================
// ADVERSARIAL & EDGE-CASE TESTS
// ============================================================================

// --- Backtracking after resume ---

#[test]
fn resume_then_backtrack_past_suspension_point() {
    // Pattern: (cat(?{native:check})|dog)
    // Input: "catdog"
    // Scenario:
    //   1. Engine matches "cat", hits check callback, suspends
    //   2. We resume with Failure
    //   3. Engine must backtrack and try "dog" alternative
    //   4. "dog" should match at position 3
    let re = Regex::with_mode(r"(cat(?{native:check})|dog)", ExecutionMode::Full).unwrap();

    match re.find_first_suspendable("catdog") {
        MatchOutcome::Suspended(cont) => {
            assert_eq!(cont.pending_callback_name, "check");
            match re.resume(*cont, ExecResult::Failure) {
                MatchOutcome::Completed(Some(m)) => {
                    // Should find "dog" at position 3, not "cat" at 0
                    assert_eq!((m.start, m.end), (3, 6));
                }
                other => panic!("expected dog match, got {:?}", other),
            }
        }
        other => panic!("expected suspension, got {:?}", other),
    }
}

// --- Steering edge cases ---

#[test]
fn steer_skip_past_end_of_text() {
    // Skip(1000) on a 3-byte input -- should not panic, just fail gracefully
    let re = Regex::with_mode(r"a(?{native:skip_far})", ExecutionMode::Full).unwrap();
    re.register_native("skip_far", |_| ExecResult::Steer(SteerResult::Skip(1000)))
        .unwrap();
    // Should not crash -- skip goes past end, VM should handle gracefully
    let result = re.find_first("abc");
    // Result might be Some or None depending on implementation, but NO PANIC
    drop(result);
}

#[test]
fn steer_accept_at_position_zero_no_captures() {
    // Accept immediately before any captures are filled
    let re = Regex::with_mode(r"(?{native:accept_now})abc", ExecutionMode::Full).unwrap();
    re.register_native("accept_now", |_| ExecResult::Steer(SteerResult::Accept))
        .unwrap();
    let m = re.find_first("xyz").unwrap();
    assert_eq!(m.start, 0);
    assert_eq!(m.end, 0); // zero-width match at position 0
}

#[test]
fn steer_abort_in_find_all_returns_partial() {
    // Abort after 2nd match -- find_all should return first 2 matches only
    let counter = Arc::new(AtomicUsize::new(0));
    let cc = counter.clone();
    let re = Regex::with_mode(r"\w+(?{native:limit})", ExecutionMode::Full).unwrap();
    re.register_native("limit", move |_| {
        let n = cc.fetch_add(1, Ordering::Relaxed);
        if n >= 2 {
            ExecResult::Steer(SteerResult::Abort)
        } else {
            ExecResult::Success
        }
    })
    .unwrap();
    let matches = re.find_all("one two three four five");
    assert!(
        matches.len() <= 2,
        "expected at most 2 matches, got {}",
        matches.len()
    );
}

// --- Thread safety ---

#[test]
fn concurrent_find_first_on_shared_regex() {
    use std::sync::Arc;
    use std::thread;

    let re = Arc::new(Regex::compile(r"\d+").unwrap());
    let mut handles = vec![];

    for i in 0..10 {
        let re = re.clone();
        let input = format!("text {} with number {}", i, i * 100);
        handles.push(thread::spawn(move || {
            let m = re.find_first(&input);
            assert!(m.is_some(), "thread {} should find a match", i);
        }));
    }

    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn concurrent_find_all_on_shared_regex() {
    use std::sync::Arc;
    use std::thread;

    let re = Arc::new(Regex::compile(r"\w+").unwrap());
    let mut handles = vec![];

    for _ in 0..10 {
        let re = re.clone();
        handles.push(thread::spawn(move || {
            let matches = re.find_all("hello world foo bar");
            assert_eq!(matches.len(), 4);
        }));
    }

    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn event_observer_under_concurrent_matching() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::thread;

    let re = Arc::new(Regex::compile(r"\d+").unwrap());
    let event_count = Arc::new(AtomicUsize::new(0));
    let ec = event_count.clone();
    re.on_event(move |_| {
        ec.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    let mut handles = vec![];
    for _ in 0..10 {
        let re = re.clone();
        handles.push(thread::spawn(move || {
            re.find_first("abc 123 def");
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
    assert!(
        event_count.load(Ordering::Relaxed) > 0,
        "events should have fired"
    );
}

// --- Zero-width edge cases ---

#[test]
fn events_during_zero_width_matches() {
    let re = Regex::compile("a*").unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ec = events.clone();
    re.on_event(move |e| ec.lock().unwrap().push(format!("{:?}", e)))
        .unwrap();
    re.find_all("b"); // zero-width matches
    let collected = events.lock().unwrap();
    assert!(
        !collected.is_empty(),
        "events should fire even for zero-width matches"
    );
}

#[test]
fn steering_from_zero_width_callback() {
    // Callback at position 0 (zero-width) returns Accept
    let re = Regex::with_mode(r"(?{native:check})abc", ExecutionMode::Full).unwrap();
    re.register_native("check", |ctx| {
        if ctx.position == 0 {
            ExecResult::Steer(SteerResult::Accept)
        } else {
            ExecResult::Success
        }
    })
    .unwrap();
    let m = re.find_first("abc");
    assert!(m.is_some());
}

// --- Error conditions ---

#[test]
fn match_file_nonexistent_returns_error() {
    let re = Regex::compile("test").unwrap();
    let result = re.match_file("/tmp/rgx_nonexistent_file_12345.txt");
    assert!(result.is_err());
}

#[test]
fn resume_with_error_result_does_not_panic() {
    let re = Regex::with_mode(r"cat(?{native:check})", ExecutionMode::Full).unwrap();
    match re.find_first_suspendable("cat") {
        MatchOutcome::Suspended(cont) => {
            // Resume with an Error result -- should not panic
            let outcome = re.resume(*cont, ExecResult::Error("deliberate error".into()));
            // Should complete (likely no match due to error)
            match outcome {
                MatchOutcome::Completed(_) => {} // OK
                MatchOutcome::Suspended(_) => {} // Also OK
            }
        }
        _ => panic!("expected suspension"),
    }
}

#[test]
fn empty_pattern_is_compile_error() {
    let result = Regex::compile("");
    assert!(result.is_err(), "empty pattern should be a compile error");
}

#[test]
fn find_all_on_empty_input() {
    let re = Regex::compile(r"\w+").unwrap();
    let matches = re.find_all("");
    assert!(matches.is_empty());
}

#[test]
fn find_all_on_empty_input_with_zero_width_pattern() {
    // A zero-width-capable pattern on empty input
    let re = Regex::compile(r"\d*").unwrap();
    let matches = re.find_all("");
    // Zero-width match at position 0 is valid
    assert!(!matches.is_empty());
}

// --- Large input / stress ---

#[test]
fn find_all_on_large_input() {
    let re = Regex::compile(r"\d+").unwrap();
    let input = "word 42 ".repeat(10000); // 80K input with 10K matches
    let matches = re.find_all(&input);
    assert_eq!(matches.len(), 10000);
}

#[test]
fn deeply_nested_variable_access() {
    let re = Regex::with_mode(r"(?{native:check})", ExecutionMode::Full).unwrap();
    re.set_vars(value!({
        "a" => {
            "b" => {
                "c" => {
                    "d" => {
                        "e" => 42_i64
                    }
                }
            }
        }
    }));
    re.register_native("check", |ctx| {
        let a = ctx.typed_variable("a").unwrap();
        // Navigate 5 levels deep
        let val = a
            .as_map()
            .unwrap()
            .iter()
            .find(|(k, _)| k == "b")
            .unwrap()
            .1
            .as_map()
            .unwrap()
            .iter()
            .find(|(k, _)| k == "c")
            .unwrap()
            .1
            .as_map()
            .unwrap()
            .iter()
            .find(|(k, _)| k == "d")
            .unwrap()
            .1
            .as_map()
            .unwrap()
            .iter()
            .find(|(k, _)| k == "e")
            .unwrap()
            .1
            .as_i64()
            .unwrap();
        assert_eq!(val, 42);
        ExecResult::Success
    })
    .unwrap();
    assert!(re.is_match("x"));
}
