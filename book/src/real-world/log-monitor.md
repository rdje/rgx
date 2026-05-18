# Log Monitor

This example builds a real-time log monitor using `tail_file` and native callbacks. It watches a log file for structured log entries, classifies them by severity, and tracks metrics.

## The pattern

We want to match standard log lines like:

```text
2026-04-08 14:23:01 ERROR [auth] Failed login for user admin
2026-04-08 14:23:02 INFO  [http] GET /api/users 200 12ms
2026-04-08 14:23:03 WARN  [db]   Connection pool near limit (45/50)
```

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, file::TailOptions};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Duration;

// Pattern with named captures for each field
let re = Regex::with_mode(
    r"(?P<ts>\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2})\s+(?P<level>ERROR|WARN|INFO|DEBUG)\s+\[(?P<component>\w+)\]\s+(?P<message>.+)(?{native:classify})",
    ExecutionMode::Full,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## The callback

The native callback classifies each log line and decides what action to take:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult, file::TailOptions};
# use std::sync::{Arc, Mutex};
# use std::collections::HashMap;
# let re = Regex::with_mode(r"(?P<ts>\S+ \S+)\s+(?P<level>\w+)\s+\[(?P<component>\w+)\]\s+(?P<message>.+)(?{native:classify})", ExecutionMode::Full)?;

// Shared state for metrics
let metrics = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
let metrics_clone = metrics.clone();

re.register_native("classify", move |ctx| {
    let level = ctx.named("level").unwrap_or("UNKNOWN");
    let component = ctx.named("component").unwrap_or("?");
    let message = ctx.named("message").unwrap_or("");

    // Increment the counter for this severity level
    {
        let mut m = metrics_clone.lock().unwrap();
        *m.entry(level.to_string()).or_insert(0) += 1;
    }

    match level {
        "ERROR" => {
            eprintln!("[ALERT] [{component}] {message}");
            ExecResult::Numeric(3.0)  // severity score
        }
        "WARN" => {
            ExecResult::Numeric(2.0)
        }
        "INFO" => {
            ExecResult::Numeric(1.0)
        }
        _ => ExecResult::Numeric(0.0),
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Wiring it up

```rust,no_run
# use rgx_core::{Regex, ExecutionMode, ExecResult, file::TailOptions};
# use std::sync::{Arc, Mutex};
# use std::collections::HashMap;
# use std::time::Duration;
# let re = Regex::with_mode(r"(?P<level>ERROR|WARN|INFO)\s+\[(?P<component>\w+)\]\s+(?P<message>.+)", ExecutionMode::Full)?;
# let metrics = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
let handle = re.tail_file(
    "/var/log/app.log",
    TailOptions {
        poll_interval: Duration::from_millis(100),
        from_end: true,  // only new entries
    },
    |fm| {
        // fm.line_number and fm.line are available for additional processing
    },
);

// Let it run until interrupted
println!("Monitoring /var/log/app.log ... (Ctrl+C to stop)");

// In a real application, you'd wait for a signal here.
// For this example, we'll run for 60 seconds.
std::thread::sleep(Duration::from_secs(60));

// Print summary
let counts = metrics.lock().unwrap();
println!("\n--- Summary ---");
for (level, count) in counts.iter() {
    println!("  {level}: {count}");
}

handle.stop();
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Adding event-based profiling

Layer structured events on top for visibility into the engine's work:

```rust
# use rgx_core::{Regex, ExecutionMode, MatchEvent};
# let re = Regex::with_mode(r"ERROR", ExecutionMode::Full)?;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let lines_scanned = Arc::new(AtomicUsize::new(0));
let matches_found = Arc::new(AtomicUsize::new(0));
let ls = lines_scanned.clone();
let mf = matches_found.clone();

re.on_event(move |event| {
    match event {
        MatchEvent::MatchAttemptStarted { .. } => {
            ls.fetch_add(1, Ordering::Relaxed);
        }
        MatchEvent::MatchAttemptCompleted { matched: true, .. } => {
            mf.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Handling log rotation

`tail_file` automatically detects file truncation. When `logrotate` truncates the file, the watcher resets to position 0 and reprocesses. No extra code needed.

For log rotation that creates new files (e.g., renaming `app.log` to `app.log.1` and creating a fresh `app.log`), the OS-native watcher (kqueue/inotify) detects the file change and the watcher picks up the new file's content.

## Key takeaways

- `tail_file` + native callbacks turns rgx into a log monitoring framework
- The callback can classify, count, alert, and score -- all during matching
- OS-native file watching means near-zero latency and no CPU cost when idle
- Structured events add profiling and debugging without changing the matching logic
- File truncation and rotation are handled automatically
