# Execution Modes

Every rgx regex is compiled with an execution mode that controls which features are available. This isn't about limiting what you can do -- it's about paying only for what you use and making intentional security decisions.

## The three modes

### Pure

```rust
let re = Regex::with_mode(r"\d+", ExecutionMode::Pure)?;
```

**What's enabled:** Regex matching only. No code blocks of any kind.

**What's disabled:** Lua, JavaScript, Rhai, WASM, and native callbacks. If the pattern contains a code block syntax like `(?{lua:...})`, it's ignored.

**Performance characteristics:** This is the fastest mode. The engine skips all code-execution infrastructure:
- No `ExecutionManager` is allocated
- No language runtimes are initialized
- No callback dispatch tables are built
- The VM runs a minimal instruction set

**When to use Pure mode:**

- You're matching structural patterns (emails, dates, phone numbers) with no validation logic
- You need maximum throughput (millions of matches per second)
- You're processing untrusted patterns from users and don't want any code execution at all
- You're building a search tool where patterns come from user input

**Example scenario: search engine**

A text search feature where users type regex patterns. You want the full power of regex syntax but absolutely no code execution:

```rust
fn user_search(user_pattern: &str, corpus: &str) -> Vec<(usize, usize)> {
    match Regex::with_mode(user_pattern, ExecutionMode::Pure) {
        Ok(re) => re.find_all(corpus)
            .into_iter()
            .map(|m| (m.start, m.end))
            .collect(),
        Err(_) => Vec::new(), // Invalid pattern from user
    }
}
```

Even if a malicious user submits `\d+(?{lua:os.execute("rm -rf /")})`, nothing happens -- the code block is ignored in Pure mode.

### Safe

```rust
let re = Regex::with_mode(
    r#"\d+(?{lua:return tonumber(arg[0]) > 0})"#,
    ExecutionMode::Safe,
)?;
```

**What's enabled:** Regex matching plus inline code blocks in sandboxed languages:
- Lua (via mlua -- no filesystem, no OS, no network)
- JavaScript (via QuickJS -- no fetch, no eval, no Function constructor)
- Rhai (pure Rust scripting -- no external module resolver)
- WASM (sandboxed module execution)

**What's disabled:** Native Rust callbacks via `(?{native:...})`. If the pattern references a native callback, it will trigger the async suspension path rather than executing inline.

**Security properties:**

Each language runtime is sandboxed:

| Language | Removed capabilities |
|----------|---------------------|
| Lua | `io`, `os`, `debug`, `require`, `loadfile`, `dofile`, `package` |
| JavaScript | `eval`, `Function`, `fetch`, `XMLHttpRequest`; memory and stack limits enforced |
| Rhai | No external module resolver; `print` and `debug` are no-ops |
| WASM | Runs in a wasmtime sandbox with no WASI access |

Code blocks cannot:
- Read or write files
- Make network requests
- Execute system commands
- Load external modules
- Access environment variables
- Spawn processes or threads

Code blocks can:
- Perform arithmetic and string operations
- Access the match context (captures, variables, position)
- Return pass/fail, numeric, or string results
- Call `emit_numeric` / `emit_replacement`

**Performance characteristics:** Slightly slower than Pure due to language runtime initialization. Each code block execution creates a fresh runtime instance (no shared state between evaluations), which costs:
- Lua: ~1-5 microseconds per evaluation
- JavaScript: ~5-20 microseconds per evaluation
- Rhai: ~2-10 microseconds per evaluation
- WASM: ~10-50 microseconds per evaluation (depends on module complexity)

**When to use Safe mode:**

- Patterns come from configuration files you control
- Patterns come from semi-trusted sources (internal tools, admin interfaces)
- You want the convenience of inline validation without registering callbacks
- You're prototyping and want fast iteration (edit the pattern string, no recompile)

**Example scenario: configurable log filter**

A log processing tool where the filter rules live in a YAML config file:

```yaml
# rules.yaml
filters:
  - pattern: '(\d{4}-\d{2}-\d{2})(?{lua:return arg[1] >= vars.min_date})'
    description: "Date range filter"
```

```rust
fn load_filters(config_path: &str) -> Vec<Regex> {
    let config = load_yaml(config_path);
    config.filters
        .iter()
        .filter_map(|rule| {
            // Safe mode: inline code is allowed but can't escape the sandbox
            Regex::with_mode(&rule.pattern, ExecutionMode::Safe).ok()
        })
        .collect()
}
```

The config file author can write Lua/JS/Rhai logic inline, but they can't access the filesystem or network. If the config file is compromised, the worst an attacker can do is write a pattern that matches incorrectly or consumes CPU time (which you can mitigate with time budgets).

### Full

```rust
let re = Regex::with_mode(
    r"\d+(?{native:validate})",
    ExecutionMode::Full,
)?;

re.register_native("validate", |ctx| {
    // Full Rust power here
    ExecResult::Success
})?;
```

**What's enabled:** Everything from Safe mode, plus native Rust callbacks. The `(?{native:name})` syntax becomes active, allowing callbacks registered via `register_native` to run.

**What's disabled:** Nothing. This is the full-power mode.

**Security properties:** Native callbacks are arbitrary Rust closures. They can do anything Rust can do:
- Read and write files
- Make HTTP requests
- Query databases
- Access shared memory
- Spawn threads

This is by design. When you register a native callback, you're writing the code yourself. You control what it does.

**Performance characteristics:** Native callbacks are the fastest callback type because there's no language runtime overhead. A callback that does simple arithmetic runs in nanoseconds. The overhead is whatever your closure does.

**When to use Full mode:**

- You're writing both the pattern and the callbacks
- You need access to external state (databases, files, APIs)
- You need maximum callback performance (no scripting overhead)
- You're building a library that exposes callbacks as an API

**Example scenario: fraud detection**

```rust
let re = Regex::with_mode(
    r"(?<amount>\d+\.\d{2})(?{native:fraud_check})",
    ExecutionMode::Full,
)?;

let fraud_db = Arc::new(FraudDatabase::connect()?);
let db = fraud_db.clone();

re.register_native("fraud_check", move |ctx| {
    let amount: f64 = ctx.named("amount").unwrap_or("0").parse().unwrap_or(0.0);
    let user_id = ctx.variable("user_id").unwrap_or_default();

    // Native callback can access the database
    match db.check_transaction(&user_id, amount) {
        FraudRisk::Low => ExecResult::Success,
        FraudRisk::High => ExecResult::Failure,
        FraudRisk::NeedsReview => ExecResult::Numeric(amount), // Flag for review
    }
})?;
```

## Choosing the right mode

| Question | If yes | If no |
|----------|--------|-------|
| Do I need code execution at all? | Safe or Full | Pure |
| Do patterns come from untrusted users? | Pure | Safe or Full |
| Do patterns come from config files? | Safe | Pure or Full |
| Do I need to access databases/APIs from callbacks? | Full | Safe |
| Do I need maximum matching speed? | Pure | Any |
| Am I prototyping quickly? | Safe (inline code) | Any |

A decision flowchart:

```
Does the pattern have code blocks?
  No  -> Pure
  Yes -> Do you need native Rust callbacks?
           No  -> Safe
           Yes -> Full
```

## Upgrading between modes

Patterns written for Pure mode work in Safe and Full mode. Patterns written for Safe mode work in Full mode. You can always upgrade.

Downgrading is also safe: a Full-mode pattern with `(?{native:check})` in Safe mode will trigger the suspension path instead of failing. A Safe-mode pattern with `(?{lua:...})` in Pure mode will have its code blocks ignored.

This means you can develop with one mode and deploy with another:

```rust
// Development: Full mode for debugging with native callbacks
let re = Regex::with_mode(pattern, ExecutionMode::Full)?;

// Production: Safe mode if callbacks aren't needed
let re = Regex::with_mode(pattern, ExecutionMode::Safe)?;

// High-security: Pure mode to disable all code
let re = Regex::with_mode(pattern, ExecutionMode::Pure)?;
```

## Mode interactions with other features

### Events

Events work in all modes. You can attach an observer to a Pure-mode regex and still receive `MatchAttemptStarted`, `BacktrackOccurred`, etc. You won't receive `CodeBlockEvaluated` events in Pure mode (there are no code blocks to evaluate).

### Async callbacks

Async suspension works in Full mode. In Safe mode, unregistered native callbacks cause suspension. In Pure mode, native callback references are ignored, so suspension never occurs.

### File matching

File matching methods (`match_file`, `match_file_lines`, `scan_file`, `scan_file_lines`) work in all modes. Callbacks fire normally during file scanning in Safe and Full modes.

### Host variables

Variables can be set in any mode. In Pure mode, they exist but no code block can read them. In Safe and Full modes, code blocks access them via `vars`.
