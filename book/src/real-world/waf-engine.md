# WAF Rule Engine

This example builds a Web Application Firewall (WAF) rule engine that uses `RegexSet` for fast multi-rule scanning, match steering for performance-critical decisions, and structured events for audit logging.

## Architecture

A WAF inspects incoming HTTP requests and blocks malicious payloads. The engine needs to:

1. Test many rules simultaneously (SQL injection, XSS, path traversal, etc.)
2. Classify threats by severity
3. Allow/block/log based on configurable policy
4. Produce audit trails for security teams

```rust
# use rgx_core::{Regex, RegexSet, ExecutionMode, ExecResult, SteerResult, MatchEvent, Value, vars};
use std::sync::{Arc, Mutex};

// Note: no `#[derive(Debug, Clone)]` — a compiled `Regex` is neither
// `Debug` nor `Clone` (it owns engine state). Store rule metadata
// separately from the compiled patterns, as `WafEngine` does below.
struct WafRule {
    name: &'static str,
    severity: u8,     // 1 = low, 5 = critical
    action: &'static str,  // "block", "log", "pass"
    pattern: Regex,
}

struct WafEngine {
    rule_set: RegexSet,
    rules: Vec<WafRule>,
}

#[derive(Debug)]
struct WafResult {
    blocked: bool,
    matched_rules: Vec<String>,
    severity: u8,
}
# fn main() {}
```

## Defining rules

Each rule is a regex pattern with metadata:

```rust
# use rgx_core::{Regex, RegexSet, ExecutionMode, ExecResult, SteerResult};
struct RuleDef {
    name: &'static str,
    pattern: &'static str,
    severity: u8,
    action: &'static str,
}

let rule_defs = vec![
    // SQL Injection patterns
    RuleDef {
        name: "sqli_union",
        pattern: r"(?i)union\s+(all\s+)?select",
        severity: 5,
        action: "block",
    },
    RuleDef {
        name: "sqli_comment",
        pattern: r"(?i)(?:--|#|/\*)\s*$",
        severity: 3,
        action: "log",
    },
    RuleDef {
        name: "sqli_or_true",
        pattern: r"(?i)'\s+or\s+'?\d",
        severity: 5,
        action: "block",
    },

    // XSS patterns
    RuleDef {
        name: "xss_script",
        pattern: r"(?i)<script[\s>]",
        severity: 5,
        action: "block",
    },
    RuleDef {
        name: "xss_event_handler",
        pattern: r"(?i)\bon\w+\s*=",
        severity: 4,
        action: "block",
    },
    RuleDef {
        name: "xss_javascript_uri",
        pattern: r"(?i)javascript\s*:",
        severity: 4,
        action: "block",
    },

    // Path traversal
    RuleDef {
        name: "path_traversal",
        pattern: r"\.\./|\.\.\\",
        severity: 4,
        action: "block",
    },

    // Command injection
    RuleDef {
        name: "cmd_injection",
        pattern: r"[;|`]\s*(?:cat|ls|rm|wget|curl|bash|sh)\b",
        severity: 5,
        action: "block",
    },
];

// Build the RegexSet for fast multi-rule matching
let patterns: Vec<&str> = rule_defs.iter().map(|r| r.pattern).collect();
let rule_set = RegexSet::new(&patterns)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Fast multi-rule scanning with RegexSet

`RegexSet::matches` tests all rules in a single pass:

```rust
# use rgx_core::RegexSet;
let rules = RegexSet::new(&[
    r"(?i)union\s+(all\s+)?select",
    r"(?i)<script[\s>]",
    r"\.\./|\.\.\\",
    r"(?i)\bon\w+\s*=",
])?;

let input = "search?q=1' UNION SELECT * FROM users--";

let matches = rules.matches(input);
if matches.matched_any() {
    let triggered: Vec<usize> = matches.iter().collect();
    println!("rules triggered: {:?}", triggered);
    // [0] -- SQL injection rule matched
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Adding steering for early termination

For a WAF, once a critical rule fires, there's no need to keep scanning. Use match steering with `Abort`:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, SteerResult};
let critical_rule = Regex::with_mode(
    r"(?i)(union\s+select|<script|;\s*rm\s)(?{native:enforce})",
    ExecutionMode::Full,
)?;

critical_rule.set_var("max_severity", 5_i64)?;

critical_rule.register_native("enforce", |ctx| {
    let severity = ctx.var_int("max_severity").unwrap_or(0);
    if severity >= 5 {
        // Critical threat -- stop scanning and block immediately
        ExecResult::Steer(SteerResult::Accept)
    } else {
        ExecResult::Steer(SteerResult::Continue)
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Audit logging with structured events

Wire up events to capture every matching decision for the audit trail:

```rust
# use rgx_core::{Regex, MatchEvent};
# use std::sync::{Arc, Mutex};
# let re = Regex::compile(r"(?i)union\s+select")?;
let audit_log = Arc::new(Mutex::new(Vec::<String>::new()));
let log_clone = audit_log.clone();

re.on_event(move |event| {
    match event {
        MatchEvent::MatchAttemptCompleted { position, matched, .. } => {
            if *matched {
                log_clone.lock().unwrap().push(
                    format!("MATCH at position {position}")
                );
            }
        }
        MatchEvent::CodeBlockEvaluated { language, succeeded, position } => {
            log_clone.lock().unwrap().push(
                format!("{language} block at {position}: {}", if *succeeded { "pass" } else { "fail" })
            );
        }
        _ => {}
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Complete WAF engine

Putting it all together:

```rust
# use rgx_core::{Regex, RegexSet};
struct WafEngine {
    set: RegexSet,
    rule_names: Vec<&'static str>,
    severities: Vec<u8>,
    actions: Vec<&'static str>,
}

#[derive(Debug)]
struct WafVerdict {
    allow: bool,
    triggered: Vec<&'static str>,
    max_severity: u8,
}

impl WafEngine {
    fn new(rules: Vec<(&'static str, &str, u8, &'static str)>) -> Self {
        let patterns: Vec<&str> = rules.iter().map(|r| r.1).collect();
        let set = RegexSet::new(&patterns).unwrap();

        WafEngine {
            set,
            rule_names: rules.iter().map(|r| r.0).collect(),
            severities: rules.iter().map(|r| r.2).collect(),
            actions: rules.iter().map(|r| r.3).collect(),
        }
    }

    fn inspect(&self, input: &str) -> WafVerdict {
        let matches = self.set.matches(input);

        let mut triggered = Vec::new();
        let mut max_severity = 0_u8;
        let mut should_block = false;

        for idx in matches.iter() {
            triggered.push(self.rule_names[idx]);
            max_severity = max_severity.max(self.severities[idx]);

            if self.actions[idx] == "block" {
                should_block = true;
            }
        }

        WafVerdict {
            allow: !should_block,
            triggered,
            max_severity,
        }
    }
}

let waf = WafEngine::new(vec![
    ("sqli_union",       r"(?i)union\s+(all\s+)?select", 5, "block"),
    ("sqli_or_true",     r"(?i)'\s+or\s+'?\d",          5, "block"),
    ("xss_script",       r"(?i)<script[\s>]",            5, "block"),
    ("xss_event",        r"(?i)\bon\w+\s*=",             4, "block"),
    ("path_traversal",   r"\.\./|\.\.\\",                4, "block"),
    ("cmd_injection",    r"[;|`]\s*(?:cat|ls|rm|wget)\b",5, "block"),
    ("sqli_comment",     r"(?i)(?:--|#)\s*$",            3, "log"),
]);

// Clean input -- allowed
let v = waf.inspect("search?q=hello+world");
assert!(v.allow);
assert!(v.triggered.is_empty());

// SQL injection attempt -- blocked
let v = waf.inspect("id=1' UNION SELECT * FROM users--");
assert!(!v.allow);
assert!(v.triggered.contains(&"sqli_union"));
assert_eq!(v.max_severity, 5);

// XSS attempt -- blocked
let v = waf.inspect("comment=<script>alert(1)</script>");
assert!(!v.allow);
assert!(v.triggered.contains(&"xss_script"));

// Path traversal -- blocked
let v = waf.inspect("file=../../etc/passwd");
assert!(!v.allow);
assert!(v.triggered.contains(&"path_traversal"));

// Suspicious but log-only
let v = waf.inspect("q=test--");
assert!(v.allow);  // "log" action does not block
assert!(v.triggered.contains(&"sqli_comment"));
assert_eq!(v.max_severity, 3);
```

## Configurable rule sets with typed variables

Use typed variables to load rules from configuration:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult, Value, vars};
let re = Regex::with_mode(
    r"(?P<payload>.+)(?{native:score})",
    ExecutionMode::Full,
)?;

vars!(re, {
    "block_threshold" => 10_i64,
    "weights" => {
        "sqli" => 5_i64,
        "xss" => 4_i64,
        "traversal" => 3_i64
    }
});

re.register_native("score", |ctx| {
    let threshold = ctx.var_int("block_threshold").unwrap_or(10);
    let payload = ctx.named("payload").unwrap_or("");

    // Simple scoring based on pattern presence
    let mut score: i64 = 0;
    if payload.to_lowercase().contains("union") { score += 5; }
    if payload.to_lowercase().contains("<script") { score += 4; }
    if payload.contains("../") { score += 3; }

    if score >= threshold {
        ExecResult::Steer(rgx_core::SteerResult::Accept)
    } else {
        ExecResult::Numeric(score as f64)
    }
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Key takeaways

- `RegexSet` is ideal for WAFs: test many rules in one pass with zero per-rule overhead
- Match steering lets critical rules short-circuit scanning
- Structured events provide audit trails without affecting matching performance
- Typed variables make rule configuration dynamic and updateable at runtime
- The `SetMatches` iterator tells you exactly which rules fired
- Combine `RegexSet` for classification with individual `Regex` for extraction
