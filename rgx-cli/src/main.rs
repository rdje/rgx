use anyhow::Context;
use clap::Parser;
use rgx_core::log::Verbosity;
use rgx_core::CodeBlockValue;
use rgx_core::{ExecutionMode, MatchResult, Regex};
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

fn format_code_result(value: &CodeBlockValue) -> String {
    match value {
        CodeBlockValue::Replacement(text) => format!("replacement:{text:?}"),
        CodeBlockValue::Numeric(number) => format!("numeric:{number}"),
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
    if matched {
        LOGGER.lock().unwrap().debug(
            "main",
            &format!("Pattern MATCHES! Found {} matches", matches.len()),
        );

        for (i, m) in matches.iter().enumerate() {
            println!("{}", format_match_line(m, cli.show_details));
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
    rgx_core::trace_exit!("cli", "main", "ok=true,matched={}", matched);

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
}
