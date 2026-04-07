# Chapter 7: Real-World Patterns

This chapter brings together everything from the guide into complete, working examples. Each one is a real program you could adapt and deploy. They're ordered by complexity, starting with a focused tool and building toward a multi-layer system.

## Example 1: Log monitor

A log monitoring tool that watches a log file, classifies entries by severity, counts errors, and triggers alerts when thresholds are exceeded.

This combines: file matching, native callbacks, host variables, steering, and events.

```rust
use rgx_core::{
    ExecResult, ExecutionMode, MatchEvent, Regex, SteerResult,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A log monitor that classifies, counts, and alerts.
struct LogMonitor {
    regex: Regex,
    counts: Arc<Mutex<HashMap<String, usize>>>,
    backtrack_count: Arc<AtomicUsize>,
}

impl LogMonitor {
    fn new(alert_threshold: &str, max_errors: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let re = Regex::with_mode(
            r"(?<ts>\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2})\s*(?<level>FATAL|ERROR|WARN|INFO|DEBUG)\s*(?<source>\[[\w.]+\])?\s*(?<msg>[^\n]+)(?{native:classify})",
            ExecutionMode::Full,
        )?;

        let counts: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
        let callback_counts = counts.clone();

        let max_err = max_errors;

        re.register_native("classify", move |ctx| {
            let level = ctx.named("level").unwrap_or("UNKNOWN");
            let threshold = ctx.variable("threshold")
                .unwrap_or_else(|| "INFO".to_string());

            let level_rank = rank_level(level);
            let threshold_rank = rank_level(&threshold);

            // Count this level
            {
                let mut map = callback_counts.lock().unwrap();
                *map.entry(level.to_string()).or_insert(0) += 1;

                // If we've hit too many errors, abort the scan
                let error_total = map.get("ERROR").copied().unwrap_or(0)
                    + map.get("FATAL").copied().unwrap_or(0);
                if error_total >= max_err {
                    eprintln!(
                        "ABORT: {} errors/fatals reached limit of {}",
                        error_total, max_err
                    );
                    return ExecResult::Steer(SteerResult::Abort);
                }
            }

            if level_rank >= threshold_rank {
                ExecResult::Success
            } else {
                ExecResult::Failure
            }
        })?;

        re.set_variable("threshold", alert_threshold)?;

        // Set up backtrack monitoring
        let backtrack_count = Arc::new(AtomicUsize::new(0));
        let bt = backtrack_count.clone();
        re.on_event(move |event| {
            if matches!(event, MatchEvent::BacktrackOccurred { .. }) {
                bt.fetch_add(1, Ordering::Relaxed);
            }
        })?;

        Ok(Self {
            regex: re,
            counts,
            backtrack_count,
        })
    }

    fn scan(&self, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let count = self.regex.scan_file_lines(path)?;
        Ok(count)
    }

    fn report(&self) {
        let counts = self.counts.lock().unwrap();
        println!("--- Log Monitor Report ---");
        for level in &["FATAL", "ERROR", "WARN", "INFO", "DEBUG"] {
            let count = counts.get(*level).copied().unwrap_or(0);
            if count > 0 {
                println!("  {}: {}", level, count);
            }
        }
        println!(
            "  Engine backtracks: {}",
            self.backtrack_count.load(Ordering::Relaxed)
        );
    }
}

fn rank_level(level: &str) -> u8 {
    match level {
        "FATAL" => 5,
        "ERROR" => 4,
        "WARN" => 3,
        "INFO" => 2,
        "DEBUG" => 1,
        _ => 0,
    }
}

// Usage:
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let monitor = LogMonitor::new("WARN", 1000)?;
    let matched = monitor.scan("/var/log/app.log")?;
    println!("Lines matching threshold: {}", matched);
    monitor.report();
    Ok(())
}
```

## Example 2: Tokenizer / Lexer

A complete lexer for a simple expression language. Uses branch numbers to identify token types in a single pass.

This combines: alternation, branch identification, named captures, find_all.

```rust
use rgx_core::Regex;

#[derive(Debug)]
struct Token {
    kind: TokenKind,
    text: String,
    position: usize,
}

#[derive(Debug, PartialEq)]
enum TokenKind {
    Number,
    Identifier,
    String,
    Operator,
    Punctuation,
    Keyword,
    Whitespace,
    Comment,
    Unknown,
}

fn tokenize(source: &str) -> Result<Vec<Token>, Box<dyn std::error::Error>> {
    // Each alternative is a branch. Branch numbers are 1-based.
    let lexer = Regex::compile(
        r"(?<num>\d+(?:\.\d+)?)|(?<kw>let|if|else|fn|return|true|false|while)|(?<id>[a-zA-Z_]\w*)|(?<str>\"(?:[^\"\\]|\\.)*\")|(?<cmt>//[^\n]*)|(?<op>[+\-*/=<>!&|]{1,2})|(?<punc>[(){}\[\];,.])|(?<ws>\s+)|(?<unk>.)"
    )?;

    let tokens: Vec<Token> = lexer
        .find_all(source)
        .into_iter()
        .filter_map(|m| {
            let text = source[m.start..m.end].to_string();
            let kind = match m.matched_branch_number {
                Some(1) => TokenKind::Number,
                Some(2) => TokenKind::Keyword,
                Some(3) => TokenKind::Identifier,
                Some(4) => TokenKind::String,
                Some(5) => TokenKind::Comment,
                Some(6) => TokenKind::Operator,
                Some(7) => TokenKind::Punctuation,
                Some(8) => TokenKind::Whitespace,
                Some(9) => TokenKind::Unknown,
                _ => TokenKind::Unknown,
            };

            // Skip whitespace and comments
            if kind == TokenKind::Whitespace || kind == TokenKind::Comment {
                return None;
            }

            Some(Token {
                kind,
                text,
                position: m.start,
            })
        })
        .collect();

    Ok(tokens)
}

// Usage:
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
        let x = 42;
        let name = "hello";
        if x > 10 {
            return true;
        }
    "#;

    let tokens = tokenize(source)?;

    for token in &tokens {
        println!("{:12} {:?} at {}", format!("{:?}", token.kind), token.text, token.position);
    }

    // Output:
    // Keyword      "let" at 9
    // Identifier   "x" at 13
    // Operator     "=" at 15
    // Number       "42" at 17
    // Punctuation  ";" at 19
    // Keyword      "let" at 29
    // Identifier   "name" at 33
    // Operator     "=" at 38
    // String       "\"hello\"" at 40
    // Punctuation  ";" at 47
    // ...

    Ok(())
}
```

The entire lexer is one regex. Branch numbers map directly to token types. No manual character-by-character scanning, no state machine. Adding a new token type means adding one more alternative to the pattern.

## Example 3: Data validation pipeline

A pipeline that validates CSV records using callbacks. Each field has its own validation rule, and the pipeline reports detailed errors.

This combines: native callbacks, named captures, host variables, numeric results.

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct ValidationError {
    line: usize,
    field: String,
    value: String,
    reason: String,
}

fn validate_csv(
    path: &str,
) -> Result<(usize, Vec<ValidationError>), Box<dyn std::error::Error>> {
    // Pattern matches: name, age, email, phone
    let re = Regex::with_mode(
        r#"^(?<name>[^,]+),(?<age>\d+),(?<email>[^,]+),(?<phone>[^,\n]+)$(?{native:validate_record})"#,
        ExecutionMode::Full,
    )?;

    let errors: Arc<Mutex<Vec<ValidationError>>> = Arc::new(Mutex::new(Vec::new()));
    let err_ref = errors.clone();
    let line_counter: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let line_ref = line_counter.clone();

    re.register_native("validate_record", move |ctx| {
        let mut current_line = line_ref.lock().unwrap();
        *current_line += 1;
        let line_num = *current_line;
        drop(current_line);

        let name = ctx.named("name").unwrap_or("");
        let age_str = ctx.named("age").unwrap_or("0");
        let email = ctx.named("email").unwrap_or("");
        let phone = ctx.named("phone").unwrap_or("");

        let mut record_errors = Vec::new();

        // Validate name: at least 2 characters, letters and spaces only
        if name.len() < 2 || !name.chars().all(|c| c.is_alphabetic() || c == ' ') {
            record_errors.push(ValidationError {
                line: line_num,
                field: "name".to_string(),
                value: name.to_string(),
                reason: "Must be at least 2 alphabetic characters".to_string(),
            });
        }

        // Validate age: 0-150
        let age: u32 = age_str.parse().unwrap_or(0);
        if age > 150 {
            record_errors.push(ValidationError {
                line: line_num,
                field: "age".to_string(),
                value: age_str.to_string(),
                reason: "Must be between 0 and 150".to_string(),
            });
        }

        // Validate email: basic check for @ and domain
        if !email.contains('@') || !email.contains('.') {
            record_errors.push(ValidationError {
                line: line_num,
                field: "email".to_string(),
                value: email.to_string(),
                reason: "Must contain @ and a domain".to_string(),
            });
        }

        // Validate phone: digits, dashes, spaces, parens
        if !phone.chars().all(|c| c.is_ascii_digit() || c == '-' || c == ' ' || c == '(' || c == ')') {
            record_errors.push(ValidationError {
                line: line_num,
                field: "phone".to_string(),
                value: phone.to_string(),
                reason: "Must contain only digits, dashes, spaces, and parentheses".to_string(),
            });
        }

        if record_errors.is_empty() {
            ExecResult::Success
        } else {
            err_ref.lock().unwrap().extend(record_errors);
            ExecResult::Failure
        }
    })?;

    let valid_count = re.scan_file_lines(path)?;
    let all_errors = errors.lock().unwrap().clone();

    Ok((valid_count, all_errors))
}

// Usage:
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (valid, errors) = validate_csv("contacts.csv")?;

    println!("{} valid records", valid);

    if !errors.is_empty() {
        println!("\nValidation errors:");
        for err in &errors {
            println!(
                "  Line {}, field '{}' = '{}': {}",
                err.line, err.field, err.value, err.reason
            );
        }
    }

    Ok(())
}
```

## Example 4: Configuration file parser

A parser for INI-style configuration files that extracts sections, keys, and values, with type validation via callbacks.

This combines: branch identification, named captures, Lua callbacks, host variables.

```rust
use rgx_core::{ExecResult, ExecutionMode, Regex};
use std::collections::HashMap;

#[derive(Debug)]
enum ConfigValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

fn parse_config(
    path: &str,
) -> Result<HashMap<String, HashMap<String, ConfigValue>>, Box<dyn std::error::Error>> {
    // Branch 1: section header [name]
    // Branch 2: key = value
    // Branch 3: comment (# or ;)
    // Branch 4: blank line
    let re = Regex::with_mode(
        r"^\[(?<section>[^\]]+)\]$|^(?<key>[\w.]+)\s*=\s*(?<value>.+)$|^[#;].*$|^\s*$",
        ExecutionMode::Safe,
    )?;

    let matches = re.match_file_lines(path)?;

    let mut config: HashMap<String, HashMap<String, ConfigValue>> = HashMap::new();
    let mut current_section = String::from("default");

    for fm in &matches {
        match fm.match_result.matched_branch_number {
            Some(1) => {
                // Section header
                let line = &fm.line;
                if let Some(start) = line.find('[') {
                    if let Some(end) = line.find(']') {
                        current_section = line[start + 1..end].trim().to_string();
                        config.entry(current_section.clone()).or_default();
                    }
                }
            }
            Some(2) => {
                // Key-value pair
                let line = fm.line.trim();
                if let Some(eq_pos) = line.find('=') {
                    let key = line[..eq_pos].trim().to_string();
                    let raw_value = line[eq_pos + 1..].trim().to_string();

                    let value = parse_value(&raw_value);

                    config
                        .entry(current_section.clone())
                        .or_default()
                        .insert(key, value);
                }
            }
            Some(3) | Some(4) => {
                // Comment or blank line -- skip
            }
            _ => {}
        }
    }

    Ok(config)
}

fn parse_value(raw: &str) -> ConfigValue {
    // Try boolean
    match raw.to_lowercase().as_str() {
        "true" | "yes" | "on" => return ConfigValue::Boolean(true),
        "false" | "no" | "off" => return ConfigValue::Boolean(false),
        _ => {}
    }

    // Try integer
    if let Ok(n) = raw.parse::<i64>() {
        return ConfigValue::Integer(n);
    }

    // Try float
    if let Ok(f) = raw.parse::<f64>() {
        return ConfigValue::Float(f);
    }

    // String (strip quotes if present)
    let trimmed = raw.trim_matches('"').trim_matches('\'');
    ConfigValue::String(trimmed.to_string())
}

// Usage:
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_config("app.ini")?;

    for (section, pairs) in &config {
        println!("[{}]", section);
        for (key, value) in pairs {
            println!("  {} = {:?}", key, value);
        }
    }

    Ok(())
}
```

For an `app.ini` like:

```ini
[database]
host = localhost
port = 5432
ssl = true

[server]
bind = 0.0.0.0
port = 8080
workers = 4
```

The output:

```
[database]
  host = String("localhost")
  port = Integer(5432)
  ssl = Boolean(true)
[server]
  bind = String("0.0.0.0")
  port = Integer(8080)
  workers = Integer(4)
```

## Example 5: Simple WAF rule engine

A Web Application Firewall rule engine that scans HTTP requests against a set of attack patterns. Each rule has a severity, and the engine uses steering to enforce processing budgets.

This combines: callbacks, steering, events, host variables, branch identification.

```rust
use rgx_core::{
    ExecResult, ExecutionMode, MatchEvent, Regex, SteerResult,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone)]
struct WafAlert {
    rule_id: String,
    severity: String,
    matched_text: String,
    description: String,
}

struct WafEngine {
    rules: Vec<WafRule>,
}

struct WafRule {
    id: String,
    description: String,
    severity: String,
    regex: Regex,
}

impl WafEngine {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let rules = vec![
            WafEngine::build_rule(
                "SQL-001",
                "SQL injection via UNION SELECT",
                "HIGH",
                r"(?i)union\s+(all\s+)?select\s",
            )?,
            WafEngine::build_rule(
                "SQL-002",
                "SQL injection via comment termination",
                "HIGH",
                r"(?:'|\x27)\s*(--|#|/\*)",
            )?,
            WafEngine::build_rule(
                "XSS-001",
                "Script tag injection",
                "HIGH",
                r"(?i)<\s*script[^>]*>",
            )?,
            WafEngine::build_rule(
                "XSS-002",
                "Event handler injection",
                "MEDIUM",
                r"(?i)\bon\w+\s*=\s*['\"]",
            )?,
            WafEngine::build_rule(
                "PATH-001",
                "Path traversal attempt",
                "MEDIUM",
                r"\.\./|\.\.\x5c",
            )?,
            WafEngine::build_rule(
                "CMD-001",
                "Command injection",
                "CRITICAL",
                r";\s*(ls|cat|rm|wget|curl|bash|sh|nc)\s",
            )?,
        ];

        Ok(Self { rules })
    }

    fn build_rule(
        id: &str,
        description: &str,
        severity: &str,
        pattern: &str,
    ) -> Result<WafRule, Box<dyn std::error::Error>> {
        let budget_pattern = format!(
            r"(?<hit>{})(?{{native:waf_check}})",
            pattern
        );
        let re = Regex::with_mode(&budget_pattern, ExecutionMode::Full)?;

        let rule_id = id.to_string();
        let rule_severity = severity.to_string();
        let rule_desc = description.to_string();

        re.register_native("waf_check", move |ctx| {
            let budget_ms: u128 = ctx.variable("budget_ms")
                .unwrap_or_else(|| "5".to_string())
                .parse()
                .unwrap_or(5);

            // In a real WAF, you'd check a shared timer here
            // For simplicity, we always accept the match
            ExecResult::Steer(SteerResult::Accept)
        })?;

        Ok(WafRule {
            id: id.to_string(),
            description: description.to_string(),
            severity: severity.to_string(),
            regex: re,
        })
    }

    fn scan_request(
        &self,
        request: &str,
        budget_ms: u128,
    ) -> Vec<WafAlert> {
        let start = Instant::now();
        let mut alerts = Vec::new();

        for rule in &self.rules {
            // Check time budget
            if start.elapsed().as_millis() > budget_ms {
                break;
            }

            rule.regex.set_variable("budget_ms", &budget_ms.to_string()).ok();

            let matches = rule.regex.find_all(request);
            for m in &matches {
                let matched_text = request[m.start..m.end].to_string();
                alerts.push(WafAlert {
                    rule_id: rule.id.clone(),
                    severity: rule.severity.clone(),
                    matched_text,
                    description: rule.description.clone(),
                });
            }
        }

        alerts
    }

    fn evaluate(&self, request: &str) -> WafDecision {
        let alerts = self.scan_request(request, 10); // 10ms budget

        if alerts.is_empty() {
            return WafDecision::Allow;
        }

        // Block on any CRITICAL or HIGH severity
        let should_block = alerts.iter().any(|a| {
            a.severity == "CRITICAL" || a.severity == "HIGH"
        });

        if should_block {
            WafDecision::Block(alerts)
        } else {
            WafDecision::Warn(alerts)
        }
    }
}

#[derive(Debug)]
enum WafDecision {
    Allow,
    Warn(Vec<WafAlert>),
    Block(Vec<WafAlert>),
}

// Usage:
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let waf = WafEngine::new()?;

    // Normal request
    let normal = "GET /api/users?page=1 HTTP/1.1";
    match waf.evaluate(normal) {
        WafDecision::Allow => println!("ALLOW: {}", normal),
        _ => println!("Unexpected block"),
    }

    // SQL injection attempt
    let sqli = "GET /api/users?id=1 UNION SELECT * FROM passwords HTTP/1.1";
    match waf.evaluate(sqli) {
        WafDecision::Block(alerts) => {
            println!("BLOCK: {}", sqli);
            for alert in &alerts {
                println!(
                    "  Rule {}: {} [{}] matched: '{}'",
                    alert.rule_id, alert.description, alert.severity, alert.matched_text
                );
            }
        }
        _ => println!("Should have been blocked"),
    }

    // XSS attempt
    let xss = r#"POST /comment body=<script>alert("xss")</script>"#;
    match waf.evaluate(xss) {
        WafDecision::Block(alerts) => {
            println!("BLOCK: {}", xss);
            for alert in &alerts {
                println!("  Rule {}: {} [{}]", alert.rule_id, alert.description, alert.severity);
            }
        }
        _ => println!("Should have been blocked"),
    }

    // Mild suspicion (event handler, medium severity)
    let sus = r#"GET /page?q=test onclick="alert(1)" HTTP/1.1"#;
    match waf.evaluate(sus) {
        WafDecision::Warn(alerts) => {
            println!("WARN: {}", sus);
            for alert in &alerts {
                println!("  Rule {}: {}", alert.rule_id, alert.description);
            }
        }
        WafDecision::Block(alerts) => {
            println!("BLOCK: {}", sus);
            for alert in &alerts {
                println!("  Rule {}: {}", alert.rule_id, alert.description);
            }
        }
        WafDecision::Allow => println!("ALLOW: {}", sus),
    }

    Ok(())
}
```

This WAF engine:
- Compiles each rule once, runs them many times
- Uses `Accept` steering to stop scanning after the first hit per rule (no need to find all instances of the same attack)
- Respects a time budget (stops evaluating rules if the budget is exceeded)
- Returns structured alerts with rule IDs, severity, and matched text
- Makes block/warn/allow decisions based on the highest severity alert

## What's next

You now have the full rgx toolkit: pattern matching, data exchange, predicate callbacks, match steering, structured events, async I/O, file matching, and real-world patterns.

For quick lookups, see the [Quick Reference](quick-reference.md). For deep dives into specific topics, see [Execution Modes](execution-modes.md) and [Context Reference](context-reference.md).
