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

/// Options for [`Regex::tail_file`].
pub struct TailOptions {
    /// Fallback poll interval when OS-native events are unavailable (default: 250ms).
    /// On macOS (kqueue) and Linux (inotify), events are delivered instantly and
    /// this interval is only used as a safety net.
    pub poll_interval: std::time::Duration,
    /// Whether to read from the end of the file (true) or the beginning (false).
    /// Default: true (start at end, only see new content).
    pub from_end: bool,
}

impl Default for TailOptions {
    fn default() -> Self {
        Self {
            poll_interval: std::time::Duration::from_millis(250),
            from_end: true,
        }
    }
}

/// A handle to a running [`tail_file`](Regex::tail_file) operation.
///
/// Drop the handle or call [`stop`](TailHandle::stop) to terminate the watcher.
pub struct TailHandle {
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TailHandle {
    /// Signal the tail thread to stop and wait for it to finish.
    pub fn stop(mut self) {
        self.stop_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            handle.join().ok();
        }
    }

    /// Check if the tail thread is still running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|h| !h.is_finished())
    }
}

impl Drop for TailHandle {
    fn drop(&mut self) {
        self.stop_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Regex {
    /// Watch a file for new content and call `on_match` for each match found
    /// in newly appended lines.
    ///
    /// Uses **OS-native file watching** (kqueue on macOS, inotify on Linux)
    /// for near-instant notification with zero idle CPU cost. Falls back to
    /// polling if the OS watcher cannot be created.
    ///
    /// Returns a [`TailHandle`] that stops the watcher when dropped.
    ///
    /// ```rust,no_run
    /// # use rgx_core::{Regex, file::TailOptions};
    /// let re = Regex::compile(r"ERROR.*").unwrap();
    /// let handle = re.tail_file("/var/log/app.log", TailOptions::default(), |fm| {
    ///     eprintln!("line {}: {}", fm.line_number, fm.line);
    /// });
    /// // ... do other work ...
    /// handle.stop();
    /// ```
    pub fn tail_file<P, F>(&self, path: P, options: TailOptions, on_match: F) -> TailHandle
    where
        P: AsRef<Path> + Send + 'static,
        F: Fn(&FileMatch) + Send + 'static,
    {
        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = stop_flag.clone();
        let pattern = self.as_str().to_string();

        let thread = std::thread::spawn(move || {
            let Ok(re) = crate::Regex::compile(&pattern) else {
                return;
            };
            let file_path = path.as_ref().to_path_buf();
            let Ok(metadata) = fs::metadata(&file_path) else {
                return;
            };

            let mut pos = if options.from_end { metadata.len() } else { 0 };
            let mut line_number = if options.from_end {
                fs::read_to_string(&file_path).map_or(0, |s| s.lines().count())
            } else {
                0
            };

            // Process any initial content when from_end is false.
            if !options.from_end {
                tail_read_new(&file_path, &re, &on_match, &mut pos, &mut line_number);
            }

            // Try OS-native watcher first; fall back to polling.
            let (tx, rx) = std::sync::mpsc::channel();
            let _watcher = create_watcher(&file_path, tx.clone(), &options);

            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                // Block until notified or timeout (safety net for missed events).
                match rx.recv_timeout(options.poll_interval) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                // Debounce: drain any queued events so we read once after a burst.
                while rx.try_recv().is_ok() {}
                // Small delay to let the OS flush file metadata after the write.
                std::thread::sleep(std::time::Duration::from_millis(10));
                tail_read_new(&file_path, &re, &on_match, &mut pos, &mut line_number);
            }
        });

        TailHandle {
            stop_flag,
            thread: Some(thread),
        }
    }
}

/// Create an OS-native file watcher. Returns the watcher (must be kept alive)
/// or `None` if the watcher cannot be created (falls back to polling).
fn create_watcher(
    path: &std::path::PathBuf,
    tx: std::sync::mpsc::Sender<()>,
    _options: &TailOptions,
) -> Option<notify::RecommendedWatcher> {
    use notify::{EventKind, RecursiveMode, Watcher};

    let handler_tx = tx;
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                handler_tx.send(()).ok();
            }
        }
    })
    .ok()?;

    watcher.watch(path, RecursiveMode::NonRecursive).ok()?;
    Some(watcher)
}

/// Read newly appended content from a file, match line by line, and invoke the callback.
fn tail_read_new<F>(
    path: &std::path::PathBuf,
    re: &crate::Regex,
    on_match: &F,
    pos: &mut u64,
    line_number: &mut usize,
) where
    F: Fn(&FileMatch),
{
    use std::io::{Read, Seek, SeekFrom};

    let Ok(current_meta) = fs::metadata(path) else {
        return;
    };
    let current_len = current_meta.len();

    if current_len < *pos {
        // File was truncated — reset.
        *pos = 0;
        *line_number = 0;
    }
    if current_len <= *pos {
        return;
    }

    let Ok(file) = fs::File::open(path) else {
        return;
    };
    let mut reader = BufReader::new(file);
    if reader.seek(SeekFrom::Start(*pos)).is_err() {
        return;
    }
    let mut new_data = String::new();
    if reader.read_to_string(&mut new_data).is_err() {
        return;
    }

    for line in new_data.lines() {
        *line_number += 1;
        for m in re.find_all(line) {
            on_match(&FileMatch {
                match_result: m,
                line_number: *line_number,
                line: line.to_string(),
            });
        }
    }

    *pos = current_len;
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

    #[test]
    #[ignore] // Timing-sensitive: run with `cargo test -- --ignored` or in isolation
    fn tail_file_detects_appended_content() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_tail.txt");
        std::fs::write(&path, "initial line\n").unwrap();

        let re = Regex::compile(r"ERROR").unwrap();
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = found.clone();

        let handle = re.tail_file(
            path.clone(),
            super::TailOptions {
                poll_interval: std::time::Duration::from_millis(50),
                from_end: true,
            },
            move |fm| {
                found_clone.lock().unwrap().push(fm.line.clone());
            },
        );

        // Append new content after letting the watcher initialize.
        std::thread::sleep(std::time::Duration::from_millis(300));
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(f, "INFO all good").unwrap();
            writeln!(f, "ERROR something broke").unwrap();
            writeln!(f, "ERROR another failure").unwrap();
            f.flush().unwrap();
        }

        // Wait for the tailer to pick it up — retry with timeout.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if found.lock().unwrap().len() >= 2 {
                break;
            }
            if std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        handle.stop();

        let matches = found.lock().unwrap();
        assert_eq!(
            matches.len(),
            2,
            "expected 2 ERROR matches, got {}",
            matches.len()
        );
        assert!(matches[0].contains("something broke"));
        assert!(matches[1].contains("another failure"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn tail_file_from_beginning() {
        use std::sync::{Arc, Mutex};

        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_tail_begin.txt");
        std::fs::write(&path, "ERROR first\nOK\nERROR second\n").unwrap();

        let re = Regex::compile(r"ERROR").unwrap();
        let found = Arc::new(Mutex::new(Vec::new()));
        let found_clone = found.clone();

        let handle = re.tail_file(
            path.clone(),
            super::TailOptions {
                poll_interval: std::time::Duration::from_millis(50),
                from_end: false,
            },
            move |fm| {
                found_clone.lock().unwrap().push(fm.line_number);
            },
        );

        std::thread::sleep(std::time::Duration::from_millis(200));
        handle.stop();

        let lines = found.lock().unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], 1);
        assert_eq!(lines[1], 3);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn tail_handle_is_running() {
        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_tail_running.txt");
        std::fs::write(&path, "").unwrap();

        let re = Regex::compile(r"x").unwrap();
        let handle = re.tail_file(path.clone(), super::TailOptions::default(), |_| {});

        assert!(handle.is_running());
        handle.stop();

        std::fs::remove_file(&path).ok();
    }
}
