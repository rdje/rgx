//! File-backed matching — scan files directly without loading into a String.

use crate::engine::MatchResult;
use crate::error::{Result, RgxError};
use crate::Regex;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Result of a line-oriented file match.
#[derive(Debug, Clone)]
pub struct FileMatch {
    /// The match result (start/end positions are relative to the line).
    pub match_result: MatchResult,
    /// 1-based line number in the file.
    pub line_number: usize,
    /// The full line text.
    pub line: String,
}

impl Regex {
    /// Find all matches in a file's contents.
    ///
    /// Reads the entire file as a string (for files that fit in memory) and
    /// runs `find_all` over the full contents.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError::Engine`] if the file cannot be read.
    pub fn match_file<P: AsRef<Path>>(&self, path: P) -> Result<Vec<MatchResult>> {
        let contents = fs::read_to_string(path.as_ref()).map_err(|e| {
            RgxError::Engine(format!(
                "failed to read file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;
        Ok(self.find_all(&contents))
    }

    /// Find all matches in a file, line by line.
    ///
    /// Returns matches with line numbers and line text. Each line is matched
    /// independently, so multi-line patterns will not span across lines.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError::Engine`] if the file cannot be opened or a line
    /// cannot be read.
    pub fn match_file_lines<P: AsRef<Path>>(&self, path: P) -> Result<Vec<FileMatch>> {
        let file = fs::File::open(path.as_ref()).map_err(|e| {
            RgxError::Engine(format!(
                "failed to open file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;
        let reader = BufReader::new(file);
        let mut results = Vec::new();

        for (line_number, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| {
                RgxError::Engine(format!(
                    "failed to read line {} of '{}': {}",
                    line_number + 1,
                    path.as_ref().display(),
                    e
                ))
            })?;
            for m in self.find_all(&line) {
                results.push(FileMatch {
                    match_result: m,
                    line_number: line_number + 1,
                    line: line.clone(),
                });
            }
        }
        Ok(results)
    }

    /// Scan a file and trigger registered callbacks for each match.
    ///
    /// Returns the number of matches found. Callbacks registered on the
    /// engine (native, Lua, JS, etc.) fire implicitly during `find_all`
    /// execution.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError::Engine`] if the file cannot be read.
    pub fn scan_file<P: AsRef<Path>>(&self, path: P) -> Result<usize> {
        let contents = fs::read_to_string(path.as_ref()).map_err(|e| {
            RgxError::Engine(format!(
                "failed to read file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;
        let matches = self.find_all(&contents);
        Ok(matches.len())
    }

    /// Scan a file line by line, triggering registered callbacks for each match.
    ///
    /// Returns the number of matches found. Callbacks registered on the
    /// engine fire implicitly during `find_all` execution.
    ///
    /// # Errors
    ///
    /// Returns [`RgxError::Engine`] if the file cannot be opened or a line
    /// cannot be read.
    pub fn scan_file_lines<P: AsRef<Path>>(&self, path: P) -> Result<usize> {
        let file = fs::File::open(path.as_ref()).map_err(|e| {
            RgxError::Engine(format!(
                "failed to open file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;
        let reader = BufReader::new(file);
        let mut count = 0;

        for line_result in reader.lines() {
            let line = line_result.map_err(|e| RgxError::Engine(format!("read error: {e}")))?;
            count += self.find_all(&line).len();
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use crate::Regex;

    #[test]
    fn match_file_finds_all_matches() {
        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_match_file.txt");
        std::fs::write(&path, "hello cat world\ndog park\ncat again").unwrap();

        let re = Regex::compile("cat").unwrap();
        let matches = re.match_file(&path).unwrap();
        assert_eq!(matches.len(), 2);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn match_file_lines_returns_line_numbers() {
        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_lines.txt");
        std::fs::write(&path, "alpha\nbeta cat\ngamma\ndelta cat dog").unwrap();

        let re = Regex::compile("cat").unwrap();
        let matches = re.match_file_lines(&path).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line_number, 2);
        assert_eq!(matches[1].line_number, 4);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn match_file_nonexistent_returns_error() {
        let re = Regex::compile("cat").unwrap();
        assert!(re.match_file("/nonexistent/file.txt").is_err());
    }

    #[test]
    fn scan_file_counts_matches() {
        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_scan.txt");
        std::fs::write(&path, "cat dog cat bird cat").unwrap();

        let re = Regex::compile("cat").unwrap();
        let count = re.scan_file(&path).unwrap();
        assert_eq!(count, 3);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn scan_file_lines_counts_matches() {
        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_scan_lines.txt");
        std::fs::write(&path, "cat dog\nbird\ncat cat").unwrap();

        let re = Regex::compile("cat").unwrap();
        let count = re.scan_file_lines(&path).unwrap();
        assert_eq!(count, 3);

        std::fs::remove_file(&path).ok();
    }
}
