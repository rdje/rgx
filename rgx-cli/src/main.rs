use anyhow::Context;
use clap::Parser;
use rgx_core::log::Verbosity;
use rgx_core::CodeBlockValue;
use rgx_core::{ExecutionMode, FileMatch, MatchEvent, MatchResult, Regex, Value};
use serde::Serialize;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Parser, Debug)]
#[command(
    name = "rgx",
    version,
    about = "Next-gen high-performance regex engine"
)]
struct Cli {
    /// Execution mode: pure | safe | full
    #[arg(long, value_parser = ["pure", "safe", "full"], default_value = "pure")]
    mode: String,

    /// Host-provided code-block variable (`NAME=VALUE`), repeatable
    #[arg(long = "var", value_name = "NAME=VALUE")]
    variables: Vec<CliVariable>,

    /// Host-provided typed code-block variable as JSON (`NAME=JSON`), repeatable
    #[arg(long = "var-json", value_name = "NAME=JSON")]
    typed_variables: Vec<CliVariable>,

    /// Register a named wasm module for `(?{wasm:module:function})` (`NAME=PATH`), repeatable
    #[arg(long = "wasm-module", value_name = "NAME=PATH")]
    wasm_modules: Vec<CliWasmModule>,

    /// Enable high-verbosity output (legacy alias for --verbosity high)
    #[arg(long, short = 'd')]
    debug: bool,

    /// Enable debug-verbosity output (legacy alias for --verbosity debug)
    #[arg(long, short = 't')]
    trace: bool,

    /// Set UVM-style verbosity level
    #[arg(long, value_parser = ["none", "low", "medium", "high", "debug"])]
    verbosity: Option<String>,

    /// Disable all trace/debug output
    #[arg(long, conflicts_with_all = ["debug", "trace", "verbosity"])]
    quiet: bool,

    /// Route debug/trace output to trace.log instead of the terminal
    #[arg(long)]
    trace_log: bool,

    /// Include branch/code-block details in match output when available
    #[arg(long)]
    show_details: bool,

    /// Read input from a file instead of a positional argument
    #[arg(long = "file", value_name = "PATH")]
    file: Option<PathBuf>,

    /// When used with --file, match each line independently and print line numbers
    #[arg(long = "line-mode", requires = "file")]
    line_mode: bool,

    /// Print the number of matches instead of match spans
    #[arg(long)]
    count: bool,

    /// Scan directories recursively when --file points to a directory
    #[arg(long, short = 'r', requires = "file")]
    recursive: bool,

    /// Follow the file for new content (like tail -f | grep). Press Ctrl-C to stop.
    #[arg(long, short = 'f', requires = "file")]
    follow: bool,

    /// Show N lines of context before and after each match (line-mode only)
    #[arg(long = "context", short = 'C', value_name = "N")]
    context: Option<usize>,

    /// Replace matches with the given string and print the result
    #[arg(long = "replace", value_name = "STRING")]
    replace: Option<String>,

    /// Output matches as JSON
    #[arg(long)]
    json: bool,

    /// Print only the matched text, one per line
    #[arg(long = "only-matching", short = 'o')]
    only_matching: bool,

    /// Print lines that do NOT match the pattern (line-mode only)
    #[arg(long = "invert-match", short = 'v')]
    invert_match: bool,

    /// Print structured match events to stderr (debugging/profiling)
    #[arg(long)]
    events: bool,

    /// Collect and print numeric code block results, one per line
    #[arg(long)]
    numeric: bool,

    /// Replace matches using code block replacement values
    #[arg(long = "replace-with-code")]
    replace_with_code: bool,

    /// Print match statistics summary to stderr at the end
    #[arg(long)]
    stats: bool,

    /// Colorize output: auto (detect terminal), always, never
    #[arg(long, value_parser = ["auto", "always", "never"], default_value = "auto")]
    color: String,

    /// Pattern to match
    pattern: String,

    /// Input text (if omitted, reads from stdin)
    #[arg(default_value = "")]
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CliVariable {
    name: String,
    value: String,
}

impl FromStr for CliVariable {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (name, value) = raw
            .split_once('=')
            .ok_or_else(|| "expected NAME=VALUE".to_string())?;
        if name.is_empty() {
            return Err("variable name must be non-empty".to_string());
        }
        Ok(Self {
            name: name.to_string(),
            value: value.to_string(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CliWasmModule {
    name: String,
    path: PathBuf,
}

impl FromStr for CliWasmModule {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (name, path) = raw
            .split_once('=')
            .ok_or_else(|| "expected NAME=PATH".to_string())?;
        if name.is_empty() {
            return Err("WASM module name must be non-empty".to_string());
        }
        if path.is_empty() {
            return Err("WASM module path must be non-empty".to_string());
        }
        Ok(Self {
            name: name.to_string(),
            path: PathBuf::from(path),
        })
    }
}

/// JSON-serializable match entry for `--json` output.
#[derive(Serialize)]
struct JsonMatch {
    start: usize,
    end: usize,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_result: Option<String>,
}

/// Global logger for the entire rgx system
pub static LOGGER: once_cell::sync::Lazy<Arc<Mutex<Logger>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Logger::new())));

pub struct Logger {
    level: Verbosity,
}

impl Logger {
    fn new() -> Self {
        Self {
            level: Verbosity::None,
        }
    }

    pub fn set_level(&mut self, level: Verbosity) {
        self.level = level;
    }

    pub fn low(&self, module: &str, msg: &str) {
        if self.level >= Verbosity::Low {
            rgx_core::log::emit_external_at(
                Verbosity::Low,
                "LOW",
                module,
                file!(),
                line!(),
                module_path!(),
                msg,
            );
        }
    }

    pub fn debug(&self, module: &str, msg: &str) {
        if self.level >= Verbosity::High {
            rgx_core::log::emit_external_at(
                Verbosity::High,
                "HIGH",
                module,
                file!(),
                line!(),
                module_path!(),
                msg,
            );
        }
    }

    pub fn trace(&self, module: &str, msg: &str) {
        if self.level >= Verbosity::Debug {
            rgx_core::log::emit_external_at(
                Verbosity::Debug,
                "TRACE",
                module,
                file!(),
                line!(),
                module_path!(),
                msg,
            );
        }
    }
}

fn resolve_verbosity(cli: &Cli) -> Verbosity {
    if cli.quiet {
        return Verbosity::None;
    }

    if let Some(value) = &cli.verbosity {
        return Verbosity::parse(value).unwrap_or(Verbosity::None);
    }

    if cli.trace {
        Verbosity::Debug
    } else if cli.debug {
        Verbosity::High
    } else {
        Verbosity::None
    }
}

fn configure_logging_environment(cli: &Cli, verbosity: Verbosity) {
    std::env::set_var("RGX_VERBOSITY", verbosity.as_str());
    std::env::set_var(
        "RGX_DEBUG",
        if verbosity >= Verbosity::High {
            "1"
        } else {
            "0"
        },
    );
    std::env::set_var(
        "RGX_TRACE",
        if verbosity >= Verbosity::Debug {
            "1"
        } else {
            "0"
        },
    );

    if cli.trace_log {
        std::env::set_var("RGX_TRACE_FILE", "trace.log");
    } else {
        std::env::remove_var("RGX_TRACE_FILE");
    }
}

fn apply_cli_variables(regex: &Regex, variables: &[CliVariable]) -> anyhow::Result<()> {
    for variable in variables {
        regex
            .set_variable(variable.name.clone(), variable.value.clone())
            .map_err(anyhow::Error::from)
            .with_context(|| {
                format!(
                    "failed to set CLI variable '{}'; code-block variables require a compiled regex with an attached execution manager",
                    variable.name
                )
            })?;
    }
    Ok(())
}

/// Convert a `serde_json::Value` into an `rgx_core::Value`.
fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => Value::Array(arr.iter().map(json_to_value).collect()),
        serde_json::Value::Object(map) => Value::Map(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect(),
        ),
    }
}

fn apply_cli_typed_variables(regex: &Regex, variables: &[CliVariable]) -> anyhow::Result<()> {
    for variable in variables {
        let value: Value = match serde_json::from_str::<serde_json::Value>(&variable.value) {
            Ok(json) => json_to_value(&json),
            Err(_) => Value::String(variable.value.clone()),
        };
        regex
            .set_typed_variable(variable.name.clone(), value)
            .map_err(anyhow::Error::from)
            .with_context(|| {
                format!(
                    "failed to set CLI typed variable '{}'; code-block variables require a compiled regex with an attached execution manager",
                    variable.name
                )
            })?;
    }
    Ok(())
}

fn apply_cli_wasm_modules(regex: &Regex, wasm_modules: &[CliWasmModule]) -> anyhow::Result<()> {
    for wasm_module in wasm_modules {
        let module_bytes = std::fs::read(&wasm_module.path).with_context(|| {
            format!(
                "failed to read CLI wasm module '{}' from '{}'",
                wasm_module.name,
                wasm_module.path.display()
            )
        })?;
        regex
            .register_wasm_module(wasm_module.name.clone(), module_bytes)
            .map_err(anyhow::Error::from)
            .with_context(|| {
                format!(
                    "failed to register CLI wasm module '{}' from '{}'",
                    wasm_module.name,
                    wasm_module.path.display()
                )
            })?;
    }
    Ok(())
}

fn collect_matches(regex: &Regex, input: &str) -> Vec<MatchResult> {
    regex.find_all(input)
}

// ANSI color codes for match highlighting
const COLOR_MATCH_START: &str = "\x1b[1;31m"; // bold red
const COLOR_LINE_NUM: &str = "\x1b[1;32m"; // bold green
const COLOR_FILE: &str = "\x1b[1;35m"; // bold magenta
const COLOR_SEP: &str = "\x1b[36m"; // cyan
const COLOR_RESET: &str = "\x1b[0m";

/// Resolve the `--color` flag to a boolean.
fn should_color(flag: &str) -> bool {
    match flag {
        "always" => true,
        "never" => false,
        _ => std::io::IsTerminal::is_terminal(&std::io::stdout()),
    }
}

/// Highlight matched portions within a line.
///
/// Given a line and the match spans (relative to the line start), wrap each
/// matched region in ANSI color codes.
fn highlight_line(line: &str, matches: &[MatchResult], line_offset: usize) -> String {
    if matches.is_empty() {
        return line.to_string();
    }
    let mut result = String::with_capacity(line.len() + matches.len() * 20);
    let mut pos = 0;
    for m in matches {
        let rel_start = m.start.saturating_sub(line_offset);
        let rel_end = m.end.saturating_sub(line_offset).min(line.len());
        if rel_start > pos {
            result.push_str(&line[pos..rel_start]);
        }
        result.push_str(COLOR_MATCH_START);
        result.push_str(&line[rel_start..rel_end]);
        result.push_str(COLOR_RESET);
        pos = rel_end;
    }
    if pos < line.len() {
        result.push_str(&line[pos..]);
    }
    result
}

/// Wrap text in match highlight color.
fn color_match(text: &str) -> String {
    format!("{COLOR_MATCH_START}{text}{COLOR_RESET}")
}

/// Format a filename with color.
fn color_file(text: &str) -> String {
    format!("{COLOR_FILE}{text}{COLOR_RESET}")
}

/// Format a line number with color.
fn color_line_num(n: usize) -> String {
    format!("{COLOR_LINE_NUM}{n}{COLOR_RESET}")
}

/// Format a separator with color.
fn color_sep(text: &str) -> String {
    format!("{COLOR_SEP}{text}{COLOR_RESET}")
}

fn format_code_result(value: &CodeBlockValue) -> String {
    match value {
        CodeBlockValue::Replacement(text) => format!("replacement:{text:?}"),
        CodeBlockValue::Numeric(number) => format!("numeric:{number}"),
        CodeBlockValue::Structured(v) => format!("structured:{v}"),
    }
}

fn format_match_line(m: &MatchResult, show_details: bool) -> String {
    if !show_details {
        return format!("{}..{}", m.start, m.end);
    }

    let mut line = format!("{}..{}", m.start, m.end);
    if let Some(branch_number) = m.matched_branch_number {
        let _ = write!(line, " branch={branch_number}");
    }
    if let Some(code_result) = &m.code_result {
        let _ = write!(line, " code={}", format_code_result(code_result));
    }
    line
}

/// Build a `JsonMatch` from a `MatchResult`, slicing `text` from `input`.
fn json_match_from(
    m: &MatchResult,
    input: &str,
    line: Option<usize>,
    file: Option<String>,
) -> JsonMatch {
    JsonMatch {
        start: m.start,
        end: m.end,
        text: input[m.start..m.end.min(input.len())].to_string(),
        line,
        file,
        branch: m.matched_branch_number,
        code_result: m.code_result.as_ref().map(format_code_result),
    }
}

// ---------------------------------------------------------------------------
// Replacement helper
// ---------------------------------------------------------------------------

/// Replace all matches in `input` with `replacement`, returning the resulting string.
fn replace_matches(regex: &Regex, input: &str, replacement: &str) -> String {
    let matches = regex.find_all(input);
    if matches.is_empty() {
        return input.to_string();
    }
    let mut result = String::with_capacity(input.len());
    let mut cursor = 0;
    for m in &matches {
        result.push_str(&input[cursor..m.start]);
        result.push_str(replacement);
        cursor = m.end;
    }
    result.push_str(&input[cursor..]);
    result
}

// ---------------------------------------------------------------------------
// Recursive directory scanner
// ---------------------------------------------------------------------------

/// Collect all regular files under `dir`, recursively.
fn collect_files_recursive(dir: &std::path::Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files_inner(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_inner(dir: &std::path::Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory '{}'", dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("failed to read entry in '{}'", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden directories
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                continue;
            }
            collect_files_inner(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Context-line display helper
// ---------------------------------------------------------------------------

/// Print matches with surrounding context lines and `--` group separators.
fn print_with_context(
    regex: &Regex,
    lines: &[String],
    context: usize,
    prefix: Option<&str>,
    invert: bool,
) {
    let matching: Vec<bool> = lines
        .iter()
        .map(|line| !regex.find_all(line).is_empty())
        .collect();

    // Build a set of line indices to display.
    let mut display = vec![false; lines.len()];
    for (i, is_match) in matching.iter().enumerate() {
        let should_show = if invert { !*is_match } else { *is_match };
        if should_show {
            let start = i.saturating_sub(context);
            let end = (i + context + 1).min(lines.len());
            for slot in &mut display[start..end] {
                *slot = true;
            }
        }
    }

    let mut last_printed: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if !display[i] {
            continue;
        }
        // Print group separator if there is a gap between displayed lines.
        if let Some(prev) = last_printed {
            if i > prev + 1 {
                println!("--");
            }
        }
        let line_num = i + 1;
        let marker = {
            let is_match = matching[i];
            let is_target = if invert { !is_match } else { is_match };
            if is_target {
                ':'
            } else {
                '-'
            }
        };
        if let Some(pfx) = prefix {
            println!("{pfx}:{line_num}{marker}{line}");
        } else {
            println!("{line_num}{marker}{line}");
        }
        last_printed = Some(i);
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let verbosity = resolve_verbosity(&cli);
    configure_logging_environment(&cli, verbosity);

    // Initialize rgx_core logging system after env is ready.
    rgx_core::log::init();
    rgx_core::trace_enter!(
        "cli",
        "main",
        "mode_arg={},pattern_len={},input_arg_len={},verbosity={},quiet={},trace_log={},vars={},wasm_modules={},show_details={}",
        cli.mode,
        cli.pattern.len(),
        cli.text.len(),
        verbosity.as_str(),
        cli.quiet,
        cli.trace_log,
        cli.variables.len(),
        cli.wasm_modules.len(),
        cli.show_details
    );

    // Set up logging based on resolved verbosity.
    {
        let mut logger = LOGGER.lock().unwrap();
        logger.set_level(verbosity);
        logger.low("main", &format!("Verbosity mode: {}", verbosity.as_str()));
        if cli.trace_log {
            logger.low(
                "main",
                "Trace output routing enabled: logs are written to trace.log",
            );
        }
        if verbosity == Verbosity::Debug {
            logger.trace(
                "main",
                "TRACE/DEBUG VERBOSITY ENABLED - exhaustive logging active",
            );
        } else if verbosity >= Verbosity::High {
            logger.debug(
                "main",
                "HIGH VERBOSITY ENABLED - detailed compile/execute logging active",
            );
        }
    }

    // Log the input parameters
    {
        let logger = LOGGER.lock().unwrap();
        logger.debug("main", &format!("Pattern: '{}'", cli.pattern));
        logger.debug("main", &format!("Execution mode: {:?}", cli.mode));
        logger.debug("main", &format!("CLI variables: {}", cli.variables.len()));
        logger.debug(
            "main",
            &format!("CLI wasm modules: {}", cli.wasm_modules.len()),
        );
        logger.debug("main", &format!("Show details: {}", cli.show_details));
        if !cli.text.is_empty() {
            logger.debug("main", &format!("Input text: '{}'", cli.text));
        }
    }

    let use_color = should_color(&cli.color);

    let mode = match cli.mode.as_str() {
        "pure" => ExecutionMode::Pure,
        "safe" => ExecutionMode::Safe,
        _ => ExecutionMode::Full,
    };
    rgx_core::trace_decision!(
        "cli",
        "mode == ExecutionMode::Pure",
        mode == ExecutionMode::Pure,
        "resolved execution mode from cli.mode={}",
        cli.mode
    );

    let regex_result = if mode == ExecutionMode::Pure {
        LOGGER
            .lock()
            .unwrap()
            .debug("main", "Compiling pattern in PURE mode...");
        Regex::compile(&cli.pattern)?
    } else {
        LOGGER
            .lock()
            .unwrap()
            .debug("main", &format!("Compiling pattern in {:?} mode...", mode));
        Regex::with_mode(&cli.pattern, mode)?
    };
    let regex = regex_result;

    if !cli.wasm_modules.is_empty() {
        LOGGER.lock().unwrap().debug(
            "main",
            &format!(
                "Registering {} CLI wasm modules on compiled regex",
                cli.wasm_modules.len()
            ),
        );
        apply_cli_wasm_modules(&regex, &cli.wasm_modules)?;
    }

    if !cli.variables.is_empty() {
        LOGGER.lock().unwrap().debug(
            "main",
            &format!(
                "Applying {} CLI variables to compiled regex",
                cli.variables.len()
            ),
        );
        apply_cli_variables(&regex, &cli.variables)?;
    }

    if !cli.typed_variables.is_empty() {
        LOGGER.lock().unwrap().debug(
            "main",
            &format!(
                "Applying {} CLI typed (JSON) variables to compiled regex",
                cli.typed_variables.len()
            ),
        );
        apply_cli_typed_variables(&regex, &cli.typed_variables)?;
    }

    // Register event observer if --events is specified
    if cli.events {
        regex.on_event(|event: &MatchEvent| {
            eprintln!("{event:?}");
        })?;
    }

    // ---- File-matching mode ----
    if let Some(ref file_path) = cli.file {
        LOGGER.lock().unwrap().debug(
            "main",
            &format!("File matching mode: path={}", file_path.display()),
        );

        // Recursive directory scanning
        if cli.recursive || file_path.is_dir() {
            return run_recursive(&cli, &regex, file_path);
        }

        // --follow mode: tail the file for new matches
        if cli.follow {
            use rgx_core::file::TailOptions;
            let use_color = should_color(&cli.color);
            let handle = regex.tail_file(file_path.clone(), TailOptions::default(), move |fm| {
                let text = &fm.line[fm.match_result.start..fm.match_result.end.min(fm.line.len())];
                if use_color {
                    println!(
                        "{}{}{}",
                        color_line_num(fm.line_number),
                        color_sep(":"),
                        color_match(text)
                    );
                } else {
                    println!("{}:{}", fm.line_number, text);
                }
            });
            // Block until Ctrl-C
            let (tx, rx) = std::sync::mpsc::channel();
            ctrlc::set_handler(move || {
                tx.send(()).ok();
            })
            .ok();
            rx.recv().ok();
            handle.stop();
            return Ok(());
        }

        // --replace on a single file
        if let Some(ref replacement) = cli.replace {
            let contents = std::fs::read_to_string(file_path)
                .with_context(|| format!("failed to read '{}'", file_path.display()))?;
            let result = replace_matches(&regex, &contents, replacement);
            print!("{result}");
            return Ok(());
        }

        // --replace-with-code on a single file
        if cli.replace_with_code {
            let contents = std::fs::read_to_string(file_path)
                .with_context(|| format!("failed to read '{}'", file_path.display()))?;
            let result = regex.replace_all_with_code(&contents);
            print!("{result}");
            return Ok(());
        }

        // --numeric on a single file
        if cli.numeric {
            let contents = std::fs::read_to_string(file_path)
                .with_context(|| format!("failed to read '{}'", file_path.display()))?;
            let matches = collect_matches(&regex, &contents);
            for m in &matches {
                if let Some(CodeBlockValue::Numeric(n)) = &m.code_result {
                    println!("{n}");
                }
            }
            return Ok(());
        }

        if cli.count {
            // --count: just print total number of matches
            let count = if cli.line_mode {
                regex
                    .scan_file_lines(file_path)
                    .map_err(|e| {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    })
                    .unwrap()
            } else {
                regex
                    .scan_file(file_path)
                    .map_err(|e| {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    })
                    .unwrap()
            };
            println!("{count}");
            rgx_core::trace_exit!("cli", "main", "ok=true,file_count={}", count);
            return Ok(());
        }

        // --json on a file
        if cli.json {
            return run_json_file(&regex, file_path, cli.line_mode);
        }

        // --invert-match (line-mode only)
        if cli.invert_match {
            return run_invert_file(&regex, file_path, cli.context);
        }

        // --context (line-mode)
        if cli.context.is_some() && cli.line_mode {
            let contents = std::fs::read_to_string(file_path)
                .with_context(|| format!("failed to read '{}'", file_path.display()))?;
            let lines: Vec<String> = contents.lines().map(String::from).collect();
            print_with_context(&regex, &lines, cli.context.unwrap_or(0), None, false);
            return Ok(());
        }

        if cli.line_mode {
            // --line-mode: match each line independently, print "LINE_NUM: matched_text"
            let file_matches: Vec<FileMatch> =
                regex.match_file_lines(file_path).unwrap_or_else(|e| {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                });
            let _matched = !file_matches.is_empty();
            LOGGER.lock().unwrap().debug(
                "main",
                &format!("Line-mode file scan: {} matches", file_matches.len()),
            );
            for fm in &file_matches {
                let text = &fm.line[fm.match_result.start..fm.match_result.end.min(fm.line.len())];
                if cli.only_matching {
                    if use_color {
                        println!("{}", color_match(text));
                    } else {
                        println!("{text}");
                    }
                } else if use_color {
                    println!(
                        "{}{}{}",
                        color_line_num(fm.line_number),
                        color_sep(":"),
                        color_match(text)
                    );
                } else {
                    println!("{}: {}", fm.line_number, text);
                }
            }
            if cli.stats {
                let contents = std::fs::read_to_string(file_path)
                    .with_context(|| format!("failed to read '{}'", file_path.display()))?;
                let line_count = contents.lines().count();
                eprintln!("---");
                eprintln!(
                    "{} matches in {} lines, 1 file scanned",
                    file_matches.len(),
                    line_count
                );
            }
            rgx_core::trace_exit!("cli", "main", "ok=true,matched={}", _matched);
            return Ok(());
        }

        // Default file mode: scan whole file, print matching spans
        let contents = std::fs::read_to_string(file_path)
            .with_context(|| format!("failed to read '{}'", file_path.display()))?;
        let file_matches = collect_matches(&regex, &contents);
        let _matched = !file_matches.is_empty();
        LOGGER.lock().unwrap().debug(
            "main",
            &format!("File scan: {} matches", file_matches.len()),
        );
        for m in &file_matches {
            if cli.only_matching {
                let text = &contents[m.start..m.end.min(contents.len())];
                if use_color {
                    println!("{}", color_match(text));
                } else {
                    println!("{text}");
                }
            } else if use_color {
                let text = &contents[m.start..m.end.min(contents.len())];
                print!("{}..{} ", m.start, m.end);
                println!("{}", color_match(text));
            } else {
                println!("{}", format_match_line(m, cli.show_details));
            }
        }
        if cli.stats {
            let line_count = contents.lines().count();
            eprintln!("---");
            eprintln!(
                "{} matches in {} lines, 1 file scanned",
                file_matches.len(),
                line_count
            );
        }
        rgx_core::trace_exit!("cli", "main", "ok=true,matched={}", _matched);
        return Ok(());
    }

    // ---- Inline / stdin mode (original behavior) ----
    let input = if cli.text.is_empty() {
        rgx_core::trace_decision!(
            "cli",
            "cli.text.is_empty()",
            true,
            "read candidate input from stdin"
        );
        use std::io::{self, Read};
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        LOGGER
            .lock()
            .unwrap()
            .debug("main", &format!("Read {} bytes from stdin", buf.len()));
        buf
    } else {
        rgx_core::trace_decision!(
            "cli",
            "cli.text.is_empty()",
            false,
            "use positional CLI input argument"
        );
        cli.text
    };

    // --replace on inline/stdin text
    if let Some(ref replacement) = cli.replace {
        let result = replace_matches(&regex, &input, replacement);
        print!("{result}");
        return Ok(());
    }

    // --replace-with-code on inline/stdin text
    if cli.replace_with_code {
        let result = regex.replace_all_with_code(&input);
        print!("{result}");
        return Ok(());
    }

    // --numeric on inline/stdin text
    if cli.numeric {
        let matches = collect_matches(&regex, &input);
        for m in &matches {
            if let Some(CodeBlockValue::Numeric(n)) = &m.code_result {
                println!("{n}");
            }
        }
        return Ok(());
    }

    LOGGER.lock().unwrap().debug(
        "main",
        &format!("Testing pattern against {} bytes of input", input.len()),
    );
    let matches = collect_matches(&regex, &input);
    let matched = !matches.is_empty();
    rgx_core::trace_decision!(
        "cli",
        "!matches.is_empty()",
        matched,
        "input_len={},matches={}",
        input.len(),
        matches.len()
    );

    if cli.count {
        println!("{}", matches.len());
        rgx_core::trace_exit!("cli", "main", "ok=true,count={}", matches.len());
        return Ok(());
    }

    // --json on inline/stdin text
    if cli.json {
        let entries: Vec<JsonMatch> = matches
            .iter()
            .map(|m| json_match_from(m, &input, None, None))
            .collect();
        println!(
            "{}",
            serde_json::to_string(&entries).expect("JSON serialization should not fail")
        );
        rgx_core::trace_exit!("cli", "main", "ok=true,json_entries={}", entries.len());
        return Ok(());
    }

    // --only-matching on inline/stdin text
    if cli.only_matching {
        for m in &matches {
            let text = &input[m.start..m.end.min(input.len())];
            if use_color {
                println!("{}", color_match(text));
            } else {
                println!("{text}");
            }
        }
        rgx_core::trace_exit!("cli", "main", "ok=true,matched={}", matched);
        return Ok(());
    }

    // --invert-match on inline/stdin text (line-oriented)
    if cli.invert_match {
        let lines: Vec<String> = input.lines().map(String::from).collect();
        if let Some(ctx) = cli.context {
            print_with_context(&regex, &lines, ctx, None, true);
        } else {
            for (i, line) in lines.iter().enumerate() {
                if regex.find_all(line).is_empty() {
                    println!("{}:{line}", i + 1);
                }
            }
        }
        rgx_core::trace_exit!("cli", "main", "ok=true,inverted=true");
        return Ok(());
    }

    if matched {
        LOGGER.lock().unwrap().debug(
            "main",
            &format!("Pattern MATCHES! Found {} matches", matches.len()),
        );

        for (i, m) in matches.iter().enumerate() {
            if use_color {
                let matched_text = &input[m.start..m.end.min(input.len())];
                print!("{}..{} ", m.start, m.end);
                println!("{}", color_match(matched_text));
            } else {
                println!("{}", format_match_line(m, cli.show_details));
            }
            LOGGER.lock().unwrap().trace(
                "main",
                &format!(
                    "Match {}: {}..{} = '{}'",
                    i,
                    m.start,
                    m.end,
                    &input[m.start..m.end.min(input.len())]
                ),
            );
        }
    } else {
        LOGGER
            .lock()
            .unwrap()
            .debug("main", "Pattern does NOT match");
    }

    if cli.stats {
        let line_count = input.lines().count();
        eprintln!("---");
        eprintln!("{} matches in {} lines", matches.len(), line_count);
    }

    rgx_core::trace_exit!("cli", "main", "ok=true,matched={}", matched);

    Ok(())
}

// ---------------------------------------------------------------------------
// Recursive directory scanning
// ---------------------------------------------------------------------------

fn run_recursive(cli: &Cli, regex: &Regex, dir: &std::path::Path) -> anyhow::Result<()> {
    let files = collect_files_recursive(dir)?;
    LOGGER.lock().unwrap().debug(
        "main",
        &format!("Recursive scan: {} files in {}", files.len(), dir.display()),
    );

    let mut json_entries: Vec<JsonMatch> = Vec::new();
    let mut total_count: usize = 0;
    let mut stats_total_matches: usize = 0;
    let mut stats_total_lines: usize = 0;
    let mut stats_files_scanned: usize = 0;

    for file_path in &files {
        // Skip binary files by checking the first 512 bytes for null bytes.
        let Ok(raw) = std::fs::read(file_path) else {
            continue;
        };
        let preview = &raw[..raw.len().min(512)];
        if preview.contains(&0) {
            continue;
        }
        let Ok(contents) = String::from_utf8(raw) else {
            continue;
        };

        let display_path = file_path
            .strip_prefix(dir)
            .unwrap_or(file_path)
            .display()
            .to_string();

        // --replace on recursive files
        if let Some(ref replacement) = cli.replace {
            let result = replace_matches(regex, &contents, replacement);
            if result != contents {
                print!("{result}");
            }
            continue;
        }

        // --context on recursive scan
        if cli.context.is_some() {
            let lines: Vec<String> = contents.lines().map(String::from).collect();
            print_with_context(
                regex,
                &lines,
                cli.context.unwrap_or(0),
                Some(&display_path),
                cli.invert_match,
            );
            continue;
        }

        // --invert-match on recursive scan
        if cli.invert_match {
            for (i, line) in contents.lines().enumerate() {
                if regex.find_all(line).is_empty() {
                    println!("{display_path}:{}:{line}", i + 1);
                }
            }
            continue;
        }

        let lines: Vec<&str> = contents.lines().collect();
        stats_files_scanned += 1;
        stats_total_lines += lines.len();
        for (line_idx, line) in lines.iter().enumerate() {
            let line_matches = regex.find_all(line);
            if line_matches.is_empty() {
                continue;
            }
            let line_num = line_idx + 1;
            stats_total_matches += line_matches.len();

            if cli.count {
                total_count += line_matches.len();
                continue;
            }

            for m in &line_matches {
                let text = &line[m.start..m.end.min(line.len())];

                if cli.json {
                    json_entries.push(json_match_from(
                        m,
                        line,
                        Some(line_num),
                        Some(display_path.clone()),
                    ));
                } else if cli.only_matching {
                    println!("{display_path}:{line_num}:{text}");
                } else {
                    println!("{display_path}:{line_num}: {text}");
                }
            }
        }
    }

    if cli.count {
        println!("{total_count}");
    } else if cli.json {
        println!(
            "{}",
            serde_json::to_string(&json_entries).expect("JSON serialization should not fail")
        );
    }

    if cli.stats {
        eprintln!("---");
        let pct = if stats_total_lines > 0 {
            (stats_total_matches as f64 / stats_total_lines as f64) * 100.0
        } else {
            0.0
        };
        eprintln!(
            "{} matches in {} lines ({:.1}%), {} files scanned",
            stats_total_matches, stats_total_lines, pct, stats_files_scanned
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// JSON file output
// ---------------------------------------------------------------------------

fn run_json_file(
    regex: &Regex,
    file_path: &std::path::Path,
    line_mode: bool,
) -> anyhow::Result<()> {
    if line_mode {
        let file_matches: Vec<FileMatch> = regex.match_file_lines(file_path).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        });
        let entries: Vec<JsonMatch> = file_matches
            .iter()
            .map(|fm| json_match_from(&fm.match_result, &fm.line, Some(fm.line_number), None))
            .collect();
        println!(
            "{}",
            serde_json::to_string(&entries).expect("JSON serialization should not fail")
        );
    } else {
        let contents = std::fs::read_to_string(file_path)
            .with_context(|| format!("failed to read '{}'", file_path.display()))?;
        let matches = collect_matches(regex, &contents);
        let entries: Vec<JsonMatch> = matches
            .iter()
            .map(|m| json_match_from(m, &contents, None, None))
            .collect();
        println!(
            "{}",
            serde_json::to_string(&entries).expect("JSON serialization should not fail")
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Invert-match file output
// ---------------------------------------------------------------------------

fn run_invert_file(
    regex: &Regex,
    file_path: &std::path::Path,
    context: Option<usize>,
) -> anyhow::Result<()> {
    let contents = std::fs::read_to_string(file_path)
        .with_context(|| format!("failed to read '{}'", file_path.display()))?;
    let lines: Vec<String> = contents.lines().map(String::from).collect();

    if let Some(ctx) = context {
        print_with_context(regex, &lines, ctx, None, true);
    } else {
        for (i, line) in lines.iter().enumerate() {
            if regex.find_all(line).is_empty() {
                println!("{}:{line}", i + 1);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgx_core::{ExecResult, ExecutionMode};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn cli_variable_parses_name_value_pairs() {
        assert_eq!(
            "env=prod".parse::<CliVariable>().expect("parse env var"),
            CliVariable {
                name: "env".to_string(),
                value: "prod".to_string(),
            }
        );
        assert_eq!(
            "token=a=b".parse::<CliVariable>().expect("parse token var"),
            CliVariable {
                name: "token".to_string(),
                value: "a=b".to_string(),
            }
        );
    }

    #[test]
    fn cli_variable_rejects_malformed_assignments() {
        assert!("env".parse::<CliVariable>().is_err());
        assert!("=prod".parse::<CliVariable>().is_err());
    }

    #[test]
    fn cli_wasm_module_parses_name_path_pairs() {
        assert_eq!(
            "truthy=/tmp/truthy.wasm"
                .parse::<CliWasmModule>()
                .expect("parse wasm module"),
            CliWasmModule {
                name: "truthy".to_string(),
                path: PathBuf::from("/tmp/truthy.wasm"),
            }
        );
        assert_eq!(
            "emit=artifacts/path=with=equals.wasm"
                .parse::<CliWasmModule>()
                .expect("parse wasm module path with equals"),
            CliWasmModule {
                name: "emit".to_string(),
                path: PathBuf::from("artifacts/path=with=equals.wasm"),
            }
        );
    }

    #[test]
    fn cli_wasm_module_rejects_malformed_assignments() {
        assert!("truthy".parse::<CliWasmModule>().is_err());
        assert!("=module.wasm".parse::<CliWasmModule>().is_err());
        assert!("truthy=".parse::<CliWasmModule>().is_err());
    }

    #[test]
    fn format_match_line_preserves_plain_span_output_by_default() {
        let m = MatchResult {
            start: 2,
            end: 5,
            groups: vec![Some((2, 5))],
            matched_branch_number: Some(2),
            code_result: Some(CodeBlockValue::Numeric(7.0)),
        };

        assert_eq!(format_match_line(&m, false), "2..5");
    }

    #[test]
    fn format_match_line_includes_optional_branch_and_code_details() {
        let m = MatchResult {
            start: 2,
            end: 5,
            groups: vec![Some((2, 5))],
            matched_branch_number: Some(2),
            code_result: Some(CodeBlockValue::Replacement("CAT".to_string())),
        };

        assert_eq!(
            format_match_line(&m, true),
            r#"2..5 branch=2 code=replacement:"CAT""#
        );
    }

    #[test]
    fn collect_matches_avoids_duplicate_code_execution_prechecks() {
        let regex =
            Regex::with_mode(r#"a(?{native:count})"#, ExecutionMode::Full).expect("compile regex");
        let invocations = Arc::new(AtomicUsize::new(0));
        let callback_invocations = Arc::clone(&invocations);
        regex
            .register_native("count", move |_| {
                callback_invocations.fetch_add(1, Ordering::SeqCst);
                ExecResult::Success
            })
            .expect("register callback");

        let matches = collect_matches(&regex, "a");

        assert_eq!(matches.len(), 1);
        assert_eq!(invocations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn apply_cli_variables_sets_host_variables_on_compiled_regex() {
        let regex = Regex::with_mode(r#"(?{native:check_env})"#, ExecutionMode::Full)
            .expect("compile regex");
        apply_cli_variables(
            &regex,
            &[CliVariable {
                name: "env".to_string(),
                value: "prod".to_string(),
            }],
        )
        .expect("set CLI variables");
        regex
            .register_native("check_env", |ctx| {
                if ctx.variable("env").as_deref() == Some("prod") {
                    ExecResult::Success
                } else {
                    ExecResult::Failure
                }
            })
            .expect("register callback");

        assert!(regex.is_match(""));
    }

    #[test]
    fn apply_cli_wasm_modules_reports_missing_files() {
        let regex = Regex::with_mode("cat", ExecutionMode::Safe).expect("compile regex");
        let module = CliWasmModule {
            name: "missing".to_string(),
            path: std::env::temp_dir().join("rgx-cli-missing-module-do-not-create.wasm"),
        };

        let err = apply_cli_wasm_modules(&regex, &[module]).expect_err("missing file should fail");
        let msg = err.to_string();
        assert!(msg.contains("failed to read CLI wasm module 'missing'"));
    }

    #[cfg(not(feature = "wasm"))]
    #[test]
    fn apply_cli_wasm_modules_surfaces_registration_failures_without_cli_wasm_feature() {
        let regex = Regex::with_mode("(?{native:placeholder})", ExecutionMode::Full)
            .expect("compile regex");
        let temp_path = std::env::temp_dir().join("rgx-cli-wasm-feature-gate.bin");
        std::fs::write(&temp_path, [0_u8, 1, 2, 3]).expect("write placeholder module bytes");
        let module = CliWasmModule {
            name: "truthy".to_string(),
            path: temp_path.clone(),
        };

        let err = apply_cli_wasm_modules(&regex, &[module])
            .expect_err("missing wasm feature should fail");
        let msg = err.to_string();
        assert!(msg.contains("failed to register CLI wasm module 'truthy'"));
        let error_chain = err.chain().map(ToString::to_string).collect::<Vec<_>>();
        assert!(
            error_chain
                .iter()
                .any(|cause| cause.contains("WASM module registration requires the `wasm` cargo feature"))
                || error_chain
                    .iter()
                    .any(|cause| cause.contains("Failed to compile WASM module truthy")),
            "expected either a wasm feature-gate error or an invalid-module error in chain: {err:#}"
        );

        std::fs::remove_file(temp_path).ok();
    }

    #[cfg(feature = "wasm")]
    fn temp_test_wasm_path(name: &str) -> PathBuf {
        let unique = format!(
            "rgx-cli-{name}-{}-{}.wasm",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after unix epoch")
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[cfg(feature = "wasm")]
    #[test]
    fn apply_cli_wasm_modules_registers_modules_for_safe_patterns() {
        let regex = Regex::with_mode("(?{wasm:truthy:evaluate})", ExecutionMode::Safe)
            .expect("compile regex");
        let module_path = temp_test_wasm_path("truthy");
        let module_bytes = wat::parse_str(
            r#"(module
                (func (export "evaluate") (result i32)
                    i32.const 1)
            )"#,
        )
        .expect("assemble WAT module");
        std::fs::write(&module_path, module_bytes).expect("write wasm module");

        let module = CliWasmModule {
            name: "truthy".to_string(),
            path: module_path.clone(),
        };
        apply_cli_wasm_modules(&regex, &[module]).expect("register CLI wasm module");

        assert!(regex.is_match(""));

        std::fs::remove_file(module_path).ok();
    }

    // ---- File matching integration tests (unit-level) ----

    fn temp_test_file(name: &str, contents: &str) -> PathBuf {
        let unique = format!(
            "rgx-cli-{name}-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::write(&path, contents).expect("write temp test file");
        path
    }

    #[test]
    fn file_match_finds_spans_in_whole_file() {
        let path = temp_test_file("spans", "hello cat world\ndog park\ncat again");
        let re = Regex::compile("cat").unwrap();
        let matches = re.match_file(&path).unwrap();
        assert_eq!(matches.len(), 2);
        // First "cat" starts at offset 6
        assert_eq!(matches[0].start, 6);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn file_match_lines_returns_line_numbers_and_text() {
        let path = temp_test_file("lines", "alpha\nbeta ERROR\ngamma\ndelta WARN end");
        let re = Regex::compile("ERROR|WARN").unwrap();
        let matches = re.match_file_lines(&path).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line_number, 2);
        assert_eq!(
            &matches[0].line[matches[0].match_result.start..matches[0].match_result.end],
            "ERROR"
        );
        assert_eq!(matches[1].line_number, 4);
        assert_eq!(
            &matches[1].line[matches[1].match_result.start..matches[1].match_result.end],
            "WARN"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn scan_file_returns_count() {
        let path = temp_test_file("count", "cat dog cat\nbird\ncat cat cat");
        let re = Regex::compile("cat").unwrap();
        let count = re.scan_file(&path).unwrap();
        assert_eq!(count, 5);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn scan_file_lines_returns_count() {
        let path = temp_test_file("countlines", "cat dog\nbird\ncat cat");
        let re = Regex::compile("cat").unwrap();
        let count = re.scan_file_lines(&path).unwrap();
        assert_eq!(count, 3);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn file_match_nonexistent_returns_error() {
        let re = Regex::compile("cat").unwrap();
        assert!(re
            .match_file("/tmp/rgx_cli_nonexistent_file_xyz.txt")
            .is_err());
    }

    // ---- New feature tests ----

    #[test]
    fn replace_matches_replaces_all_occurrences() {
        let re = Regex::compile("cat|kitten").unwrap();
        let result = replace_matches(&re, "I have a cat and a kitten", "dog");
        assert_eq!(result, "I have a dog and a dog");
    }

    #[test]
    fn replace_matches_returns_original_when_no_match() {
        let re = Regex::compile("xyz").unwrap();
        let result = replace_matches(&re, "hello world", "replaced");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn replace_matches_handles_empty_replacement() {
        let re = Regex::compile("cat").unwrap();
        let result = replace_matches(&re, "a cat sat on a cat mat", "");
        assert_eq!(result, "a  sat on a  mat");
    }

    #[test]
    fn replace_matches_handles_adjacent_matches() {
        let re = Regex::compile("ab").unwrap();
        let result = replace_matches(&re, "ababab", "X");
        assert_eq!(result, "XXX");
    }

    #[test]
    fn collect_files_recursive_finds_files_in_nested_dirs() {
        let base = std::env::temp_dir().join(format!(
            "rgx-cli-recurse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(base.join("sub/deep")).unwrap();
        std::fs::write(base.join("top.txt"), "top").unwrap();
        std::fs::write(base.join("sub/mid.txt"), "mid").unwrap();
        std::fs::write(base.join("sub/deep/bot.txt"), "bot").unwrap();

        let files = collect_files_recursive(&base).unwrap();
        assert_eq!(files.len(), 3);

        // Verify all files are collected (sorted)
        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(&base).unwrap().display().to_string())
            .collect();
        assert!(names.contains(&"top.txt".to_string()));
        assert!(names.contains(&"sub/mid.txt".to_string()));
        assert!(names.contains(&"sub/deep/bot.txt".to_string()));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn collect_files_recursive_skips_hidden_directories() {
        let base = std::env::temp_dir().join(format!(
            "rgx-cli-hidden-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(base.join(".hidden")).unwrap();
        std::fs::write(base.join("visible.txt"), "yes").unwrap();
        std::fs::write(base.join(".hidden/secret.txt"), "no").unwrap();

        let files = collect_files_recursive(&base).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("visible.txt"));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn json_match_serializes_correctly() {
        let entry = JsonMatch {
            start: 5,
            end: 13,
            text: "555-1234".to_string(),
            line: None,
            file: None,
            branch: None,
            code_result: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""start":5"#));
        assert!(json.contains(r#""end":13"#));
        assert!(json.contains(r#""text":"555-1234""#));
        // Should not contain line or file when None
        assert!(!json.contains("line"));
        assert!(!json.contains("file"));
        assert!(!json.contains("branch"));
        assert!(!json.contains("code_result"));
    }

    #[test]
    fn json_match_serializes_with_line_and_file() {
        let entry = JsonMatch {
            start: 0,
            end: 3,
            text: "foo".to_string(),
            line: Some(42),
            file: Some("test.rs".to_string()),
            branch: None,
            code_result: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""line":42"#));
        assert!(json.contains(r#""file":"test.rs""#));
    }

    // ---- New feature tests ----

    #[test]
    fn json_match_serializes_branch_and_code_result_when_present() {
        let entry = JsonMatch {
            start: 0,
            end: 3,
            text: "cat".to_string(),
            line: None,
            file: None,
            branch: Some(1),
            code_result: Some("numeric:42".to_string()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""branch":1"#));
        assert!(json.contains(r#""code_result":"numeric:42""#));
    }

    #[test]
    fn json_to_value_converts_primitives() {
        let v = json_to_value(&serde_json::json!(42));
        assert_eq!(v, Value::Int(42));

        let v = json_to_value(&serde_json::json!(2.72));
        assert_eq!(v, Value::Float(2.72));

        let v = json_to_value(&serde_json::json!(true));
        assert_eq!(v, Value::Bool(true));

        let v = json_to_value(&serde_json::json!("hello"));
        assert_eq!(v, Value::String("hello".to_string()));

        let v = json_to_value(&serde_json::json!(null));
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn json_to_value_converts_arrays_and_objects() {
        let v = json_to_value(&serde_json::json!(["cat", "dog"]));
        assert_eq!(
            v,
            Value::Array(vec![
                Value::String("cat".to_string()),
                Value::String("dog".to_string()),
            ])
        );

        let v = json_to_value(&serde_json::json!({"key": 42}));
        assert_eq!(v, Value::Map(vec![("key".to_string(), Value::Int(42))]));
    }

    #[test]
    fn apply_cli_typed_variables_parses_json_values() {
        let regex =
            Regex::with_mode(r#"(?{native:check})"#, ExecutionMode::Full).expect("compile regex");
        apply_cli_typed_variables(
            &regex,
            &[CliVariable {
                name: "limit".to_string(),
                value: "42".to_string(),
            }],
        )
        .expect("set typed variable");

        regex
            .register_native("check", |ctx| {
                if ctx.typed_variable("limit") == Some(Value::Int(42)) {
                    ExecResult::Success
                } else {
                    ExecResult::Failure
                }
            })
            .expect("register callback");

        assert!(regex.is_match(""));
    }

    #[test]
    fn apply_cli_typed_variables_falls_back_to_string_on_invalid_json() {
        let regex =
            Regex::with_mode(r#"(?{native:check})"#, ExecutionMode::Full).expect("compile regex");
        apply_cli_typed_variables(
            &regex,
            &[CliVariable {
                name: "label".to_string(),
                value: "not-json".to_string(),
            }],
        )
        .expect("set typed variable");

        regex
            .register_native("check", |ctx| {
                if ctx.typed_variable("label") == Some(Value::String("not-json".to_string())) {
                    ExecResult::Success
                } else {
                    ExecResult::Failure
                }
            })
            .expect("register callback");

        assert!(regex.is_match(""));
    }

    #[test]
    fn json_match_from_populates_branch_from_match_result() {
        let m = MatchResult {
            start: 0,
            end: 3,
            groups: vec![Some((0, 3))],
            matched_branch_number: Some(2),
            code_result: Some(CodeBlockValue::Numeric(7.0)),
        };
        let jm = json_match_from(&m, "cat", None, None);
        assert_eq!(jm.branch, Some(2));
        assert_eq!(jm.code_result, Some("numeric:7".to_string()));
    }

    #[test]
    fn replace_all_with_code_replaces_via_code_results() {
        // This tests the engine function directly; the CLI just wraps it.
        // The code block must come after the named capture so it can read the match.
        let regex = Regex::with_mode(r#"(?<w>[a-z]+)(?{native:upper})"#, ExecutionMode::Full)
            .expect("compile regex");
        regex
            .register_native("upper", |ctx| {
                ExecResult::Replacement(ctx.named("w").unwrap_or("").to_uppercase())
            })
            .expect("register callback");
        let result = regex.replace_all_with_code("hello world");
        assert_eq!(result, "HELLO WORLD");
    }
}
