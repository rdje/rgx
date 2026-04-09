# File Matching & tail_file

rgx can match directly against files without requiring you to read the entire file into a `String` first. For long-running monitoring, `tail_file` watches a file for new content and fires callbacks on each match -- similar to `tail -f | grep`, but integrated into the engine.

## match_file

Read an entire file and find all matches across its full contents:

```rust,no_run
# use rgx_core::Regex;
let re = Regex::compile(r"TODO:?\s+(.+)")?;

let matches = re.match_file("src/main.rs")?;
for m in &matches {
    println!("{}..{}", m.start, m.end);
}
println!("found {} TODOs", matches.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

`match_file` loads the file into memory and runs `find_all`. Positions are byte offsets within the full file content. This works well for files that fit comfortably in memory.

## match_file_lines

Match a file line by line, getting line numbers with each result:

```rust,no_run
# use rgx_core::Regex;
let re = Regex::compile(r"ERROR\s+(.*)")?;

let matches = re.match_file_lines("/var/log/app.log")?;
for fm in &matches {
    println!("line {}: {}", fm.line_number, fm.line.trim());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Each `FileMatch` contains:

| Field | Type | Description |
|-------|------|-------------|
| `match_result` | `MatchResult` | The match (positions relative to the line) |
| `line_number` | `usize` | 1-based line number in the file |
| `line` | `String` | The full line text |

Line-by-line matching means multi-line patterns won't span across lines. Use `match_file` if you need cross-line matching.

## scan_file

When you only need the count and want any registered callbacks to fire:

```rust,no_run
# use rgx_core::Regex;
let re = Regex::compile(r"(?i)error|warn|fatal")?;

let count = re.scan_file("/var/log/syslog")?;
println!("{count} log events matched");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`scan_file` returns the number of matches found. Any callbacks registered on the regex (native, Lua, etc.) fire during scanning, so you can use this for side-effect processing.

## scan_file_lines

Line-by-line variant of `scan_file`:

```rust,no_run
# use rgx_core::Regex;
let re = Regex::compile(r"CRITICAL")?;

let count = re.scan_file_lines("/var/log/app.log")?;
if count > 0 {
    eprintln!("ALERT: {count} critical events found!");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## tail_file

`tail_file` watches a file for new content and calls your closure for each match in newly appended lines. It returns a `TailHandle` that lets you stop the watcher.

```rust,no_run
# use rgx_core::{Regex, file::TailOptions};
let re = Regex::compile(r"ERROR\s+(.*)")?;

let handle = re.tail_file(
    "/var/log/app.log",
    TailOptions::default(),
    |fm| {
        eprintln!(
            "[line {}] {}",
            fm.line_number,
            fm.line.trim()
        );
    },
);

// The watcher runs in a background thread.
// Do other work here...

// When done, stop the watcher.
handle.stop();
```

### TailOptions

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `poll_interval` | `Duration` | 250ms | Fallback poll interval (see below) |
| `from_end` | `bool` | `true` | Start at end of file (only new content) |

```rust,no_run
# use rgx_core::{Regex, file::TailOptions};
# use std::time::Duration;
let re = Regex::compile(r"ERROR")?;

// Watch from the beginning (process existing content too)
let handle = re.tail_file(
    "/var/log/app.log",
    TailOptions {
        poll_interval: Duration::from_millis(100),
        from_end: false,
    },
    |fm| {
        println!("line {}: {}", fm.line_number, fm.line);
    },
);
# handle.stop();
```

### OS-native file watching

`tail_file` uses OS-native file watching for near-instant notification with zero idle CPU cost:

- **macOS**: kqueue
- **Linux**: inotify

The `poll_interval` is only used as a safety net when OS events are temporarily unavailable. Under normal conditions, new content is detected within milliseconds of being written.

If the OS-native watcher cannot be created (e.g., too many open file descriptors), `tail_file` falls back to polling at the configured interval.

### TailHandle

The `TailHandle` controls the background watcher:

| Method | Description |
|--------|-------------|
| `handle.stop()` | Signal the watcher to stop and wait for it to finish |
| `handle.is_running()` | Check if the background thread is still active |

The watcher also stops automatically when the `TailHandle` is dropped. This means you can use it with RAII patterns:

```rust,no_run
# use rgx_core::{Regex, file::TailOptions};
{
    let re = Regex::compile(r"ERROR").unwrap();
    let _handle = re.tail_file("/var/log/app.log", TailOptions::default(), |fm| {
        eprintln!("{}", fm.line);
    });

    // Watcher is running...
    std::thread::sleep(std::time::Duration::from_secs(10));

}  // _handle is dropped here, watcher stops automatically
```

### File truncation

`tail_file` detects file truncation (e.g., from `logrotate`). When the file becomes shorter than the last read position, the watcher resets to the beginning and reprocesses the file.

## Complete log monitoring example

A production-style log monitor that watches for errors and tracks statistics:

```rust,no_run
# use rgx_core::{Regex, file::TailOptions};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Duration;

let re = Regex::compile(r"(?P<level>ERROR|WARN|INFO)\s+(?P<msg>.*)")?;

let stats = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
let stats_clone = stats.clone();

let handle = re.tail_file(
    "/var/log/app.log",
    TailOptions {
        poll_interval: Duration::from_millis(100),
        from_end: true,
    },
    move |fm| {
        // Extract the log level from the matched line
        let line = &fm.line;
        if line.contains("ERROR") {
            *stats_clone.lock().unwrap()
                .entry("ERROR".to_string())
                .or_insert(0) += 1;
            eprintln!("[ALERT] line {}: {}", fm.line_number, line.trim());
        } else if line.contains("WARN") {
            *stats_clone.lock().unwrap()
                .entry("WARN".to_string())
                .or_insert(0) += 1;
        }
    },
);

// Let it run for a while...
std::thread::sleep(Duration::from_secs(60));

// Print stats before stopping
let counts = stats.lock().unwrap();
for (level, count) in counts.iter() {
    println!("{level}: {count}");
}

handle.stop();
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Method summary

| Method | Input | Returns | Use case |
|--------|-------|---------|----------|
| `match_file(path)` | Whole file | `Vec<MatchResult>` | Find all matches in a file |
| `match_file_lines(path)` | Line by line | `Vec<FileMatch>` | Matches with line numbers |
| `scan_file(path)` | Whole file | `usize` (count) | Count matches, fire callbacks |
| `scan_file_lines(path)` | Line by line | `usize` (count) | Count matches per line |
| `tail_file(path, opts, cb)` | Live watching | `TailHandle` | Monitor file for new matches |

## CLI shortcut

From the command line, `--follow` wraps `tail_file` for you:

```bash
rgx --file /var/log/app.log --follow 'ERROR|WARN'
```

This is equivalent to the `tail_file` API but accessible without writing Rust code. See [CLI Guide](../appendices/cli-guide.md) for details.
