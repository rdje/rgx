//! Match multiple regex patterns against a single input simultaneously.
//!
//! ```rust,no_run
//! # use rgx_core::RegexSet;
//! let set = RegexSet::new(&[r"\d+", r"[a-z]+", r"[A-Z]+"]).unwrap();
//! let matches = set.matches("abc 123 XYZ");
//! assert!(matches.matched(0));  // \d+ matched
//! assert!(matches.matched(1));  // [a-z]+ matched
//! assert!(matches.matched(2));  // [A-Z]+ matched
//! ```

use crate::error::{Result, RgxError};
use crate::Regex;

/// A set of compiled regex patterns that can be matched simultaneously.
///
/// This is useful for routing, classification, and filtering where you need
/// to test multiple patterns and determine which ones match.
pub struct RegexSet {
    regexes: Vec<Regex>,
    patterns: Vec<String>,
}

/// The result of matching a [`RegexSet`] against input text.
///
/// Indicates which patterns in the set matched.
#[derive(Clone, Debug)]
pub struct SetMatches {
    matched: Vec<bool>,
}

impl RegexSet {
    /// Compile a set of regex patterns.
    ///
    /// All patterns are compiled independently. If any pattern is invalid,
    /// the entire construction fails.
    ///
    /// # Errors
    /// Returns [`RgxError`] if any pattern fails to compile.
    pub fn new<I, S>(patterns: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let patterns: Vec<String> = patterns
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let mut regexes = Vec::with_capacity(patterns.len());
        for (i, pat) in patterns.iter().enumerate() {
            match Regex::compile(pat) {
                Ok(re) => regexes.push(re),
                Err(e) => {
                    return Err(RgxError::compile(format!("pattern {i} ({pat:?}): {e}")));
                }
            }
        }
        Ok(Self { regexes, patterns })
    }

    /// Create an empty set that matches nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            regexes: Vec::new(),
            patterns: Vec::new(),
        }
    }

    /// Test if any pattern in the set matches the input.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.regexes.iter().any(|re| re.is_match(text))
    }

    /// Determine which patterns match the input.
    #[must_use]
    pub fn matches(&self, text: &str) -> SetMatches {
        let matched = self.regexes.iter().map(|re| re.is_match(text)).collect();
        SetMatches { matched }
    }

    /// The number of patterns in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.regexes.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.regexes.is_empty()
    }

    /// Access the original pattern strings.
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

impl SetMatches {
    /// Whether any pattern matched.
    #[must_use]
    pub fn matched_any(&self) -> bool {
        self.matched.iter().any(|&b| b)
    }

    /// Whether all patterns matched.
    #[must_use]
    pub fn matched_all(&self) -> bool {
        !self.matched.is_empty() && self.matched.iter().all(|&b| b)
    }

    /// Whether the pattern at `index` matched.
    #[must_use]
    pub fn matched(&self, index: usize) -> bool {
        self.matched.get(index).copied().unwrap_or(false)
    }

    /// The number of patterns in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.matched.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.matched.is_empty()
    }

    /// Iterator over the indices of matched patterns.
    pub fn iter(&self) -> SetMatchesIter<'_> {
        SetMatchesIter {
            matched: &self.matched,
            idx: 0,
        }
    }
}

impl IntoIterator for SetMatches {
    type Item = usize;
    type IntoIter = SetMatchesIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        SetMatchesIntoIter {
            matched: self.matched,
            idx: 0,
        }
    }
}

/// Borrowed iterator over matched pattern indices.
pub struct SetMatchesIter<'a> {
    matched: &'a [bool],
    idx: usize,
}

impl<'a> Iterator for SetMatchesIter<'a> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        while self.idx < self.matched.len() {
            let i = self.idx;
            self.idx += 1;
            if self.matched[i] {
                return Some(i);
            }
        }
        None
    }
}

/// Owned iterator over matched pattern indices.
pub struct SetMatchesIntoIter {
    matched: Vec<bool>,
    idx: usize,
}

impl Iterator for SetMatchesIntoIter {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        while self.idx < self.matched.len() {
            let i = self.idx;
            self.idx += 1;
            if self.matched[i] {
                return Some(i);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_set_basic() {
        let set = RegexSet::new(&[r"\d+", r"[a-z]+", r"[A-Z]+"]).unwrap();
        let m = set.matches("abc 123 XYZ");
        assert!(m.matched(0));
        assert!(m.matched(1));
        assert!(m.matched(2));
        assert!(m.matched_any());
        assert!(m.matched_all());
    }

    #[test]
    fn regex_set_partial_match() {
        let set = RegexSet::new(&[r"\d+", r"[A-Z]+"]).unwrap();
        let m = set.matches("abc 123");
        assert!(m.matched(0)); // digits found
        assert!(!m.matched(1)); // no uppercase
        assert!(m.matched_any());
        assert!(!m.matched_all());
    }

    #[test]
    fn regex_set_no_match() {
        let set = RegexSet::new(&[r"\d+", r"[A-Z]+"]).unwrap();
        let m = set.matches("abc def");
        assert!(!m.matched_any());
    }

    #[test]
    fn regex_set_is_match() {
        let set = RegexSet::new(&[r"foo", r"bar"]).unwrap();
        assert!(set.is_match("foo"));
        assert!(set.is_match("bar"));
        assert!(!set.is_match("baz"));
    }

    #[test]
    fn regex_set_empty() {
        let set = RegexSet::empty();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(!set.is_match("anything"));
    }

    #[test]
    fn regex_set_invalid_pattern_fails() {
        let result = RegexSet::new(&[r"\d+", r"(unclosed"]);
        assert!(result.is_err());
    }

    #[test]
    fn regex_set_patterns_accessor() {
        let set = RegexSet::new(&[r"\d+", r"\w+"]).unwrap();
        assert_eq!(set.patterns(), &[r"\d+".to_string(), r"\w+".to_string()]);
    }

    #[test]
    fn regex_set_iter_matched_indices() {
        let set = RegexSet::new(&[r"a", r"b", r"c", r"d"]).unwrap();
        let m = set.matches("ac");
        let indices: Vec<usize> = m.iter().collect();
        assert_eq!(indices, vec![0, 2]); // "a" and "c" matched
    }

    #[test]
    fn regex_set_into_iter() {
        let set = RegexSet::new(&[r"x", r"y", r"z"]).unwrap();
        let m = set.matches("xyz");
        let indices: Vec<usize> = m.into_iter().collect();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn regex_set_routing_use_case() {
        let routes =
            RegexSet::new(&[r"^/api/users", r"^/api/posts", r"^/static/", r"^/health$"]).unwrap();

        let m = routes.matches("/api/users/123");
        assert!(m.matched(0));
        assert!(!m.matched(1));

        let m = routes.matches("/static/style.css");
        assert!(m.matched(2));

        let m = routes.matches("/health");
        assert!(m.matched(3));
    }
}
