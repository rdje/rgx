# Chapter 6: Working with Files

Every chapter so far has matched against strings in memory. In practice, the text you want to scan often lives in files -- log files, configuration files, source code, data exports. You *could* read the file into a string yourself and pass it to `find_all`, but rgx provides purpose-built file methods that handle the I/O for you and integrate cleanly with callbacks.

This is arguably the most practical chapter in the guide. If you work with files (and who doesn't?), you'll use these methods daily.

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

## Real scenarios

### CSV field extraction

Scan a CSV file for rows where a specific field matches a pattern:

```rust
let re = Regex::compile(
    r#"^(?<name>[^,]+),(?<email>[^,]+),(?<role>[^,\n]+)"#
)?;

let rows = re.match_file_lines("employees.csv")?;

for row in &rows {
    let line = &row.line;
    let m = &row.match_result;

    // Extract individual fields from groups
    if let (Some((ns, ne)), Some((es, ee)), Some((rs, re_end))) = (
        m.groups.get(1).and_then(|g| *g),
        m.groups.get(2).and_then(|g| *g),
        m.groups.get(3).and_then(|g| *g),
    ) {
        let name = &line[ns..ne];
        let email = &line[es..ee];
        let role = &line[rs..re_end];
        println!("Line {}: {} ({}) - {}", row.line_number, name, email, role);
    }
}
```

### Config file validation

Check that every line in a config file is either a comment, a blank line, or a valid key=value pair:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};

let re = Regex::with_mode(
    r"^(?<line>\s*#.*|\s*|(?<key>[a-zA-Z_]\w*)\s*=\s*(?<value>.+))$",
    ExecutionMode::Full,
)?;

let matches = re.match_file_lines("app.conf")?;

// Read the file to count total lines
let total_lines = std::fs::read_to_string("app.conf")?
    .lines().count();

if matches.len() < total_lines {
    // Some lines didn't match -- they're invalid
    let matched_lines: std::collections::HashSet<usize> =
        matches.iter().map(|m| m.line_number).collect();

    let file_content = std::fs::read_to_string("app.conf")?;
    for (i, line) in file_content.lines().enumerate() {
        if !matched_lines.contains(&(i + 1)) {
            eprintln!("Invalid config at line {}: {}", i + 1, line);
        }
    }
} else {
    println!("Config file is valid ({} lines checked)", total_lines);
}
```

### Multi-file search

Search across an entire directory of source files for TODO comments:

```rust
use rgx_core::Regex;
use std::fs;

fn search_directory(
    pattern: &str,
    dir: &str,
    extension: &str,
) -> Result<Vec<(String, usize, String)>, Box<dyn std::error::Error>> {
    let re = Regex::compile(pattern)?;
    let mut results = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process files with the right extension
        if path.extension().and_then(|e| e.to_str()) != Some(extension) {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();
        match re.match_file_lines(&path_str) {
            Ok(matches) => {
                for fm in matches {
                    results.push((
                        path_str.clone(),
                        fm.line_number,
                        fm.line.trim().to_string(),
                    ));
                }
            }
            Err(e) => {
                eprintln!("Skipping {}: {}", path_str, e);
            }
        }
    }

    Ok(results)
}

// Usage
let todos = search_directory(r"TODO|FIXME|HACK|XXX", "./src", "rs")?;
for (file, line, text) in &todos {
    println!("{}:{}: {}", file, line, text);
}
println!("\nFound {} items across source files", todos.len());
```

## Building a mini grep: complete walkthrough

Let's build a fully-featured grep-like tool that searches across multiple files with context lines, match highlighting, and summary statistics.

### Step 1: Basic single-file grep

Start with the simplest version -- search one file, print matching lines:

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

    println!("\n{} matches in {}", matches.len(), file_path);

    Ok(())
}
```

### Step 2: Add context lines

Like `grep -C`, show lines before and after each match for context:

```rust
use rgx_core::Regex;

fn rgx_grep_context(
    pattern: &str,
    file_path: &str,
    context_lines: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let re = Regex::compile(pattern)?;
    let matches = re.match_file_lines(file_path)?;

    if matches.is_empty() {
        return Ok(());
    }

    // Read all lines for context display
    let all_lines: Vec<String> = std::fs::read_to_string(file_path)?
        .lines().map(String::from).collect();

    let match_line_numbers: std::collections::HashSet<usize> =
        matches.iter().map(|m| m.line_number).collect();

    let mut last_printed = 0;

    for fm in &matches {
        let line_num = fm.line_number;
        let start = line_num.saturating_sub(context_lines + 1);
        let end = (line_num + context_lines).min(all_lines.len());

        // Print separator between non-adjacent groups
        if last_printed > 0 && start > last_printed {
            println!("--");
        }

        for i in start..end {
            let display_num = i + 1;
            if display_num <= last_printed { continue; }

            if match_line_numbers.contains(&display_num) {
                // Highlight the matching line
                println!(
                    "\x1b[1;31m{}:{}:\x1b[0m {}",
                    file_path, display_num, all_lines[i].trim_end()
                );
            } else {
                // Context line (dimmed)
                println!(
                    "\x1b[2m{}-{}:\x1b[0m {}",
                    file_path, display_num, all_lines[i].trim_end()
                );
            }
            last_printed = display_num;
        }
    }

    println!("\n{} matches in {}", matches.len(), file_path);
    Ok(())
}
```

### Step 3: Multi-file with summary

Search across multiple files and print a summary:

```rust
fn rgx_grep_multi(
    pattern: &str,
    file_paths: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let re = Regex::compile(pattern)?;
    let mut total = 0;
    let mut files_with_matches = 0;

    for path in file_paths {
        match re.match_file_lines(path) {
            Ok(matches) => {
                if !matches.is_empty() {
                    files_with_matches += 1;
                }
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

    println!(
        "\n{} total matches across {} files ({} files had matches)",
        total,
        file_paths.len(),
        files_with_matches,
    );
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

## File matching + callbacks: reactive pipelines

When you combine file matching with callbacks, you create a reactive pipeline: the engine scans the file, and your callbacks react to each match in real time. This is powerful for monitoring, alerting, and data transformation.

### Building a log alerter (detailed walkthrough)

Let's build a comprehensive log alerter step by step.

**The goal:** Scan a log file, classify each line by severity, apply a configurable threshold, count errors, and print real-time alerts.

**Step 1: Define the pattern**

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let re = Regex::with_mode(
    r"(?<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) (?<level>FATAL|ERROR|WARN|INFO|DEBUG):?\s*(?<message>[^\n]+)(?{native:alert_check})",
    ExecutionMode::Full,
)?;
```

**Step 2: Register the callback with severity classification**

```rust
let error_count = Arc::new(AtomicUsize::new(0));
let fatal_count = Arc::new(AtomicUsize::new(0));
let warn_count = Arc::new(AtomicUsize::new(0));
let errors = error_count.clone();
let fatals = fatal_count.clone();
let warns = warn_count.clone();

re.register_native("alert_check", move |ctx| {
    let level = ctx.named("level").unwrap_or("INFO");
    let message = ctx.named("message").unwrap_or("");
    let timestamp = ctx.named("timestamp").unwrap_or("");

    // Read the threshold from a host variable (configurable at runtime)
    let threshold = ctx.variable("alert_threshold")
        .unwrap_or_else(|| "ERROR".to_string());

    // Assign numeric ranks to severity levels
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
        // Print a real-time alert
        eprintln!("[ALERT] {} {} - {}", timestamp, level, message);

        // Track counts by severity
        match level {
            "FATAL" => { fatals.fetch_add(1, Ordering::Relaxed); }
            "ERROR" => { errors.fetch_add(1, Ordering::Relaxed); }
            "WARN"  => { warns.fetch_add(1, Ordering::Relaxed); }
            _ => {}
        }

        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;
```

**Step 3: Configure and run**

```rust
// In production: alert on ERROR and above
re.set_variable("alert_threshold", "ERROR")?;
let prod_alerts = re.scan_file_lines("/var/log/production.log")?;
println!("Production: {} alerts triggered", prod_alerts);

// In staging: alert on WARN and above (more sensitive)
re.set_variable("alert_threshold", "WARN")?;
let staging_alerts = re.scan_file_lines("/var/log/staging.log")?;
println!("Staging: {} alerts triggered", staging_alerts);
```

**Step 4: Read the counters**

```rust
println!(
    "Summary: {} fatal, {} error, {} warn",
    fatal_count.load(Ordering::Relaxed),
    error_count.load(Ordering::Relaxed),
    warn_count.load(Ordering::Relaxed),
);
```

The same compiled pattern handles both environments. The `alert_threshold` variable controls which severity levels trigger alerts. Callbacks fire as each line is scanned, so alerts appear in real time rather than after the entire file is processed.

### Building a data aggregation pipeline

Use callbacks to build summary statistics while scanning:

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

let re = Regex::with_mode(
    r"(?<method>GET|POST|PUT|DELETE) (?<path>/[^\s]*) (?<status>\d{3}) (?<time_ms>\d+)ms(?{native:aggregate})",
    ExecutionMode::Full,
)?;

let stats = Arc::new(Mutex::new(HashMap::<String, (usize, u64)>::new()));
let stats_ref = stats.clone();

re.register_native("aggregate", move |ctx| {
    let path = ctx.named("path").unwrap_or("/unknown").to_string();
    let time_ms: u64 = ctx.named("time_ms").unwrap_or("0").parse().unwrap_or(0);

    let mut map = stats_ref.lock().unwrap();
    let entry = map.entry(path).or_insert((0, 0));
    entry.0 += 1;          // request count
    entry.1 += time_ms;    // total response time

    ExecResult::Success
})?;

re.scan_file_lines("access.log")?;

// Print aggregated results
let map = stats.lock().unwrap();
println!("{:<30} {:>8} {:>12}", "Path", "Requests", "Avg (ms)");
println!("{}", "-".repeat(52));
for (path, (count, total_ms)) in map.iter() {
    println!("{:<30} {:>8} {:>12.1}", path, count, *total_ms as f64 / *count as f64);
}
```

Output:

```
Path                           Requests     Avg (ms)
----------------------------------------------------
/api/users                          342         12.5
/api/orders                         128         45.2
/health                            1205          1.1
/api/products                        89         23.8
```

## Error handling

All file methods return `Result`. Here are the common situations and how to handle them:

### Common errors

| Error | Cause | Recovery |
|-------|-------|----------|
| File not found | Path doesn't exist | Check path, provide default |
| Permission denied | Insufficient file permissions | Run with appropriate permissions |
| Invalid UTF-8 | Binary file or encoding mismatch | Use a different tool for binary files |
| I/O error on line read | Disk error, network mount timeout | Retry or skip the file |

### Handling file errors gracefully

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

### Binary files

rgx file methods expect UTF-8 text. If you try to match a binary file, you'll get an error because the contents aren't valid UTF-8. This is by design -- regex patterns operate on text.

If you need to scan files that might be binary, check first:

```rust
fn is_likely_text(path: &str) -> bool {
    // Read a small sample and check for null bytes
    match std::fs::read(path) {
        Ok(bytes) => {
            let sample = &bytes[..bytes.len().min(8192)];
            !sample.contains(&0)  // null bytes suggest binary
        }
        Err(_) => false,
    }
}

// Use it before scanning
if is_likely_text("mystery_file") {
    let matches = re.match_file_lines("mystery_file")?;
    // ...
} else {
    eprintln!("Skipping binary file: mystery_file");
}
```

### Permission errors

When scanning multiple files, some may be unreadable. Don't let one bad file stop the whole scan:

```rust
let files = vec!["app.log", "secure.log", "access.log"];
for file in &files {
    match re.match_file_lines(file) {
        Ok(matches) => {
            for fm in &matches {
                println!("{}:{}: {}", file, fm.line_number, fm.line.trim());
            }
        }
        Err(e) => {
            eprintln!("Warning: could not read {}: {}", file, e);
            // Continue with the next file
        }
    }
}
```

## Performance notes

### Line-by-line vs whole-file tradeoffs

| Approach | Memory | Multi-line patterns | Speed | Best for |
|----------|--------|-------------------|-------|----------|
| `match_file` | Loads entire file | Yes (patterns span lines) | Faster for small files | Files under ~100MB, multi-line patterns |
| `match_file_lines` | One line at a time | No (each line is independent) | Constant memory | Large files, line-oriented data |
| `scan_file` | Loads entire file | Yes | Fastest (no result allocation) | Callback-driven processing |
| `scan_file_lines` | One line at a time | No | Constant memory | Large file monitoring |

**Rules of thumb:**

- If the file fits in memory and you need multi-line matching, use `match_file`.
- If the file is large or you only need line-level matching, use `match_file_lines`.
- If you only care about triggering callbacks (not collecting results), use `scan_file` or `scan_file_lines`.
- For files over 1GB, always use the line-by-line variants.

### Reuse the compiled regex

Compiling a regex is expensive relative to matching. When scanning multiple files, compile once and reuse:

```rust
// Good: compile once, scan many files
let re = Regex::compile(r"\bERROR\b")?;
for file in &log_files {
    let matches = re.match_file_lines(file)?;
    // ...
}

// Bad: recompiling for every file
for file in &log_files {
    let re = Regex::compile(r"\bERROR\b")?;  // wasteful!
    let matches = re.match_file_lines(file)?;
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
| Search multiple files | Loop over paths, call `match_file_lines` for each |
| Build aggregations | Use `scan_file_lines` + callback with shared state |
| Skip binary files | Check for null bytes before scanning |

## Next

[Chapter 7: Real-World Patterns >>>](07-real-world.md)
