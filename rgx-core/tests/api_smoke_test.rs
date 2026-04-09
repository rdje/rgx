//! Smoke test that exercises every public API surface mentioned in the book.
//!
//! If a book example references a method that doesn't exist or has the wrong
//! signature, this test will fail to compile. This is the safety net for
//! keeping the book and the code in sync.

use rgx_core::*;
use std::borrow::Cow;

#[test]
fn smoke_compile_and_basic_match() {
    let re = Regex::compile(r"\d+").unwrap();
    assert!(re.is_match("abc 42"));
    assert!(!re.is_match("no digits"));

    let m = re.find("abc 42").unwrap();
    assert_eq!(m.as_str(), "42");
    assert_eq!(m.start(), 4);
    assert_eq!(m.end(), 6);
    assert_eq!(m.range(), 4..6);
    assert_eq!(m.len(), 2);
    assert!(!m.is_empty());

    let mr = re.find_first("abc 42").unwrap();
    assert_eq!(mr.start, 4);
    assert_eq!(mr.end, 6);
}

#[test]
fn smoke_iterators() {
    let re = Regex::compile(r"\w+").unwrap();
    let text = "one two three";

    let count = re.find_iter(text).count();
    assert_eq!(count, 3);

    let collected: Vec<_> = re.find_iter(text).map(|m| m.as_str().to_string()).collect();
    assert_eq!(collected, vec!["one", "two", "three"]);

    let all = re.find_all(text);
    assert_eq!(all.len(), 3);
}

#[test]
fn smoke_captures() {
    let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})").unwrap();
    let caps = re.captures("2026-04").unwrap();
    assert_eq!(&caps[0], "2026-04");
    assert_eq!(&caps[1], "2026");
    assert_eq!(&caps["year"], "2026");
    assert_eq!(&caps["month"], "04");
    assert_eq!(caps.len(), 3);

    let mut out = String::new();
    caps.expand("$month/$year", &mut out);
    assert_eq!(out, "04/2026");

    let count = re.captures_iter("2025-01 2026-02").count();
    assert_eq!(count, 2);
}

#[test]
fn smoke_replace() {
    let re = Regex::compile(r"(\w+)\s(\w+)").unwrap();

    // String template
    let result = re.replace("hello world", "$2 $1");
    assert_eq!(result, "world hello");

    // Closure (replaces the whole match with the closure's return value)
    let result = re.replace("hello world", |caps: &Captures| caps[1].to_uppercase());
    assert_eq!(result, "HELLO"); // entire "hello world" match → "HELLO"

    // NoExpand
    let re2 = Regex::compile(r"\d+").unwrap();
    let result = re2.replace("price 42", NoExpand("$$$"));
    assert_eq!(result, "price $$$");

    // replacen
    let re3 = Regex::compile(r"\d").unwrap();
    let result = re3.replacen("a1b2c3", 2, "X");
    assert_eq!(result, "aXbXc3");

    // Cow on no match
    let result = re3.replace("abc", "X");
    assert!(matches!(result, Cow::Borrowed(_)));
}

#[test]
fn smoke_split() {
    let re = Regex::compile(r"[,\s]+").unwrap();
    let parts = re.split("one, two, three");
    assert_eq!(parts, vec!["one", "two", "three"]);

    let parts = re.splitn("a, b, c, d", 2);
    assert_eq!(parts, vec!["a", "b, c, d"]);

    let lazy: Vec<_> = re.split_iter("a, b, c").collect();
    assert_eq!(lazy, vec!["a", "b", "c"]);
}

#[test]
fn smoke_regex_builder() {
    let re = RegexBuilder::new(r"hello")
        .case_insensitive()
        .build()
        .unwrap();
    assert!(re.is_match("HELLO"));
    assert!(re.is_match("Hello"));

    let re = RegexBuilder::new(r"^line$").multi_line().build().unwrap();
    assert!(re.is_match("a\nline\nb"));
}

#[test]
fn smoke_escape() {
    let escaped = escape("a.b+c");
    let re = Regex::compile(&escaped).unwrap();
    assert!(re.is_match("a.b+c"));
    assert!(!re.is_match("axbxc"));
}

#[test]
fn smoke_position_aware() {
    let re = Regex::compile(r"\d+").unwrap();
    let text = "12 abc 34 def 56";

    let m = re.find_first_at(text, 5).unwrap();
    assert_eq!(m.start, 7);

    let all = re.find_all_at(text, 5);
    assert_eq!(all.len(), 2);

    assert!(re.is_match_at(text, 5));
    assert_eq!(re.shortest_match("abc 42"), Some(6));
    assert_eq!(re.shortest_match_at("12 abc 34", 3), Some(9));
}

#[test]
fn smoke_regex_set() {
    let set = RegexSet::new(&[r"\d+", r"[a-z]+", r"[A-Z]+"]).unwrap();
    let m = set.matches("abc 123 XYZ");
    assert!(m.matched_all());
    assert!(m.matched(0));
    assert!(m.matched(1));
    assert!(m.matched(2));

    let indices: Vec<usize> = m.iter().collect();
    assert_eq!(indices, vec![0, 1, 2]);

    assert_eq!(set.len(), 3);
    assert!(!set.is_empty());
}

#[test]
fn smoke_regex_cache() {
    let cache = RegexCache::new(16);
    let re1 = cache.get(r"\d+").unwrap();
    let re2 = cache.get(r"\d+").unwrap();
    assert!(std::sync::Arc::ptr_eq(&re1, &re2));
    assert_eq!(cache.len(), 1);
}

#[test]
fn smoke_bytes_regex() {
    use rgx_core::bytes::BytesRegex;
    let re = BytesRegex::compile(r"\d+").unwrap();
    let m = re.find(b"abc 42").unwrap();
    assert_eq!(m.as_bytes(), b"42");
}

#[test]
fn smoke_safety_limits() {
    let re = Regex::compile(r"(a+)+b").unwrap();
    re.set_max_steps(Some(10_000));
    re.set_max_backtrack_frames(Some(1_000));
    re.set_max_recursion_depth(Some(50));
    // Pathological pattern + step limit = no hang
    let _ = re.find_first("aaaaaaaaaaaaaaaaaaaac");
}

#[test]
fn smoke_match_semantics() {
    let re = Regex::compile(r"a|ab").unwrap();
    re.set_match_semantics(MatchSemantics::LeftmostFirst);
    re.set_match_semantics(MatchSemantics::LeftmostLongest);
}

#[test]
fn smoke_partial_matching() {
    let re = Regex::compile(r"hello world").unwrap();
    match re.find_first_partial("hello world") {
        PartialMatchResult::Full(_) => {}
        _ => panic!("expected Full"),
    }
    match re.find_first_partial("hello wor") {
        PartialMatchResult::Partial(_) => {}
        _ => panic!("expected Partial"),
    }
}

#[test]
fn smoke_capture_locations() {
    let re = Regex::compile(r"(\d+)-(\w+)").unwrap();
    let mut locs = re.capture_locations();
    let m = re.captures_read("item 42-abc", &mut locs).unwrap();
    assert_eq!(m.as_str(), "42-abc");
    assert_eq!(locs.get(1), Some((5, 7)));
    assert_eq!(locs.get(2), Some((8, 11)));
}

#[test]
fn smoke_metadata() {
    let re = Regex::compile(r"(?P<year>\d{4})-(?P<month>\d{2})").unwrap();
    assert_eq!(re.as_str(), r"(?P<year>\d{4})-(?P<month>\d{2})");
    assert_eq!(re.captures_len(), 3);
    let names: Vec<_> = re.capture_names().collect();
    assert_eq!(names.len(), 3);
}

#[test]
fn smoke_error_diagnostics() {
    let result = Regex::compile(r"(unclosed");
    assert!(result.is_err());
    if let Err(e) = result {
        let err = e.to_string();
        assert!(err.contains("regex compile error"));
    }
}

#[test]
fn smoke_grapheme_cluster() {
    let re = Regex::compile(r"\X").unwrap();
    let text = "e\u{0301}x"; // e + combining accent
    let m = re.find(text).unwrap();
    assert_eq!(m.as_str(), "e\u{0301}");
}

#[test]
fn smoke_unicode_case_folding() {
    let re = Regex::compile(r"(?i)café").unwrap();
    assert!(re.is_match("CAFÉ"));
    assert!(re.is_match("Café"));
}
