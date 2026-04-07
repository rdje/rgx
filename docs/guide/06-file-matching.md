# Chapter 6: Working with Files

Every chapter so far has matched against strings in memory. In practice, the text you want to scan often lives in files -- log files, configuration files, source code, data exports. You *could* read the file into a string yourself and pass it to `find_all`, but rgx provides purpose-built file methods that handle the I/O for you and integrate cleanly with callbacks.

## Why match files directly?

### Convenience

The manual approach requires boilerplate:

```rust
// Manual file matching -- works, but tedious
let contents = std::fs::read_to_string("server.log")?;
let matches = re.find_all(&contents);
```

Two lines instead of one. And if you want line numbers, you need more work:

```rust
// Manual line-by-line matching -- even more boilerplate
let file = std::fs::File::open("server.log")?;
let reader = std::io::BufReader::new(file);
for (line_num, line) in reader.lines().enumerate() {
    let line = line?;
    for m in re.find_all(&line) {
        println!("Line {}: match at {}..{}", line_num + 1, m.start, m.end);
    }
}
```

### Callback integration

The real benefit is that file matching methods work with the same callbacks you've already registered. Your validation logic, steering actions, and event observers all fire normally during file matching. There's no separate "file mode" -- it's the same engine, just fed from a file instead of a string.

## match_file: whole-file matching

`match_file` reads the entire file into memory and runs `find_all` over the contents:

```rust
use rgx_core::Regex;

let re = Regex::compile(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b")?;

let matches = re.match_file("server.log")?;
println!("Found {} IP addresses", matches.len());

// Each match has start/end positions relative to the file contents
for m in &matches {
    println!("IP at byte offset {}..{}", m.start, m.end);
}
```

This is the simplest approach. Use it when:
- The file fits comfortably in memory
- You need byte-level positions within the entire file
- Multi-line patterns need to work across line boundaries

### When not to use match_file

If the file is very large (multiple gigabytes), `match_file` will load the whole thing into memory. For large files, prefer `match_file_lines` which processes one line at a time.

## match_file_lines: line-by-line matching with line numbers

`match_file_lines` reads the file line by line and returns matches annotated with line numbers and line text:

```rust
use rgx_core::Regex;

let re = Regex::compile(r"ERROR|FATAL")?;

let matches = re.match_file_lines("application.log")?;

for fm in &matches {
    println!(
        "Line {}: {} (match at column {})",
        fm.line_number,
        fm.line.trim(),
        fm.match_result.start,
    );
}
// Line 42: 2026-04-04 ERROR: Connection refused (match at column 11)
// Line 187: 2026-04-04 FATAL: Out of memory (match at column 11)
```

Each `FileMatch` contains:

| Field | Type | Description |
|-------|------|-------------|
| `match_result` | `MatchResult` | The match (positions relative to the line, not the file) |
| `line_number` | `usize` | 1-based line number |
| `line` | `String` | The full text of the line |

### Example: extracting structured data from logs

```rust
let re = Regex::compile(
    r"(?<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) (?<level>\w+): (?<message>.*)"
)?;

let entries = re.match_file_lines("app.log")?;

for entry in &entries {
    let line = &entry.line;
    let m = &entry.match_result;

    // The match_result carries branch information and code results
    println!(
        "[Line {}] Matched at {}..{} in: {}",
        entry.line_number,
        m.start,
        m.end,
        line.trim(),
    );
}
```

## scan_file: whole-file scanning with callbacks

`scan_file` reads the file and runs `find_all`, but instead of returning match details, it returns the count of matches. Its primary purpose is to trigger callbacks registered on the regex:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<ip>\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})(?{native:log_ip})",
    ExecutionMode::Full,
)?;

re.register_native("log_ip", |ctx| {
    let ip = ctx.named("ip").unwrap_or("unknown");
    println!("Seen IP: {}", ip);
    ExecResult::Success
})?;

let count = re.scan_file("access.log")?;
println!("Total IPs found: {}", count);
```

The callback fires for every match as the engine processes the file. This is useful for:
- Side-effect-driven scanning (logging, alerting, counting)
- Building aggregations without storing all matches

## scan_file_lines: line-by-line scanning with callbacks

`scan_file_lines` combines line-by-line reading with callback execution:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"(?<level>ERROR|FATAL).*(?<msg>[A-Z][a-z]+ [a-z]+)(?{native:alert})",
    ExecutionMode::Full,
)?;

re.register_native("alert", |ctx| {
    let level = ctx.named("level").unwrap_or("UNKNOWN");
    let msg = ctx.named("msg").unwrap_or("");
    eprintln!("ALERT [{}]: {}", level, msg);
    ExecResult::Success
})?;

let count = re.scan_file_lines("production.log")?;
println!("Processed {} error/fatal lines", count);
```

This processes one line at a time, so memory usage stays constant regardless of file size. The callback fires as each matching line is processed, giving you real-time visibility.

## Building a simple grep-like tool

Let's combine file matching with branch identification to build a colorized grep:

```rust
use rgx_core::Regex;

fn rgx_grep(pattern: &str, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let re = Regex::compile(pattern)?;

    let matches = re.match_file_lines(file_path)?;

    if matches.is_empty() {
        eprintln!("No matches found in {}", file_path);
        return Ok(());
    }

    for fm in &matches {
        let line = &fm.line;
        let m = &fm.match_result;

        // Build a highlighted version of the line
        let before = &line[..m.start];
        let matched = &line[m.start..m.end];
        let after = &line[m.end..];

        println!(
            "{}:{}: {}\x1b[1;31m{}\x1b[0m{}",
            file_path,
            fm.line_number,
            before,
            matched,
            after,
        );
    }

    println!("\n{} matches in {} lines", matches.len(), file_path);

    Ok(())
}
```

Usage:

```rust
rgx_grep(r"ERROR|FATAL", "application.log")?;
```

Output:

```
application.log:42: 2026-04-04 10:15:23 ERROR: Connection refused
application.log:187: 2026-04-04 10:22:01 FATAL: Out of memory
application.log:201: 2026-04-04 10:22:15 ERROR: Retry limit exceeded

3 matches in application.log
```

### Supporting multiple files

```rust
fn rgx_grep_multi(
    pattern: &str,
    file_paths: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let re = Regex::compile(pattern)?;
    let mut total = 0;

    for path in file_paths {
        match re.match_file_lines(path) {
            Ok(matches) => {
                for fm in &matches {
                    let m = &fm.match_result;
                    let matched_text = &fm.line[m.start..m.end];
                    println!(
                        "{}:{}:{}: {}",
                        path,
                        fm.line_number,
                        m.start,
                        matched_text,
                    );
                }
                total += matches.len();
            }
            Err(e) => {
                eprintln!("Error reading {}: {}", path, e);
            }
        }
    }

    println!("\n{} total matches across {} files", total, file_paths.len());
    Ok(())
}
```

## Building a log alerter

Now let's build something more sophisticated: a log alerter that uses callbacks to classify severity and host variables to control alerting thresholds:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn build_log_alerter() -> Result<Regex, Box<dyn std::error::Error>> {
    let re = Regex::with_mode(
        r"(?<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) (?<level>FATAL|ERROR|WARN|INFO|DEBUG):?\s*(?<message>[^\n]+)(?{native:alert_check})",
        ExecutionMode::Full,
    )?;

    let error_count = Arc::new(AtomicUsize::new(0));
    let fatal_count = Arc::new(AtomicUsize::new(0));
    let errors = error_count.clone();
    let fatals = fatal_count.clone();

    re.register_native("alert_check", move |ctx| {
        let level = ctx.named("level").unwrap_or("INFO");
        let message = ctx.named("message").unwrap_or("");
        let timestamp = ctx.named("timestamp").unwrap_or("");

        let threshold = ctx.variable("alert_threshold")
            .unwrap_or_else(|| "ERROR".to_string());

        // Determine if this level meets the threshold
        let level_rank = match level {
            "FATAL" => 5,
            "ERROR" => 4,
            "WARN" => 3,
            "INFO" => 2,
            "DEBUG" => 1,
            _ => 0,
        };
        let threshold_rank = match threshold.as_str() {
            "FATAL" => 5,
            "ERROR" => 4,
            "WARN" => 3,
            "INFO" => 2,
            "DEBUG" => 1,
            _ => 0,
        };

        if level_rank >= threshold_rank {
            eprintln!("[ALERT] {} {} - {}", timestamp, level, message);

            match level {
                "ERROR" => { errors.fetch_add(1, Ordering::Relaxed); }
                "FATAL" => { fatals.fetch_add(1, Ordering::Relaxed); }
                _ => {}
            }

            ExecResult::Success
        } else {
            ExecResult::Failure
        }
    })?;

    Ok(re)
}

// Usage
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let alerter = build_log_alerter()?;

    // In production: alert on ERROR and above
    alerter.set_variable("alert_threshold", "ERROR")?;
    let prod_alerts = alerter.scan_file_lines("/var/log/production.log")?;
    println!("Production: {} alerts triggered", prod_alerts);

    // In staging: alert on WARN and above (more sensitive)
    alerter.set_variable("alert_threshold", "WARN")?;
    let staging_alerts = alerter.scan_file_lines("/var/log/staging.log")?;
    println!("Staging: {} alerts triggered", staging_alerts);

    Ok(())
}
```

The same compiled pattern handles both environments. The `alert_threshold` variable controls which severity levels trigger alerts. Callbacks fire as each line is scanned, so alerts appear in real time rather than after the entire file is processed.

## Error handling

All file methods return `Result`. Common errors:

| Error | Cause | Recovery |
|-------|-------|----------|
| File not found | Path doesn't exist | Check path, provide default |
| Permission denied | Insufficient file permissions | Run with appropriate permissions |
| Invalid UTF-8 | Binary file or encoding mismatch | Use a different tool for binary files |
| I/O error on line read | Disk error, network mount timeout | Retry or skip the file |

```rust
match re.match_file_lines("maybe_missing.log") {
    Ok(matches) => {
        println!("Found {} matches", matches.len());
    }
    Err(e) => {
        eprintln!("Could not scan file: {}", e);
        // Decide: skip, retry, or abort
    }
}
```

## Summary

| What you want | How |
|---------------|-----|
| Match entire file | `re.match_file("path")` |
| Match file line by line | `re.match_file_lines("path")` |
| Scan file with callbacks | `re.scan_file("path")` |
| Scan file line by line with callbacks | `re.scan_file_lines("path")` |
| Get line numbers | Use `match_file_lines` -- each `FileMatch` has `.line_number` |
| Get matched line text | Use `match_file_lines` -- each `FileMatch` has `.line` |
| Handle missing files | Match on the `Result` |

## Next

[Chapter 7: Real-World Patterns >>>](07-real-world.md)
