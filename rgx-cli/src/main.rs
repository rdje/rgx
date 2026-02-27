use clap::Parser;
use rgx_core::log::Verbosity;
use rgx_core::{ExecutionMode, Regex};
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

    /// Pattern to match
    pattern: String,

    /// Input text (if omitted, reads from stdin)
    #[arg(default_value = "")]
    text: String,
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let verbosity = resolve_verbosity(&cli);
    configure_logging_environment(&cli, verbosity);

    // Initialize rgx_core logging system after env is ready.
    rgx_core::log::init();

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
        if !cli.text.is_empty() {
            logger.debug("main", &format!("Input text: '{}'", cli.text));
        }
    }

    let mode = match cli.mode.as_str() {
        "pure" => ExecutionMode::Pure,
        "safe" => ExecutionMode::Safe,
        _ => ExecutionMode::Full,
    };

    let regex = if mode == ExecutionMode::Pure {
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

    let input = if cli.text.is_empty() {
        use std::io::{self, Read};
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        LOGGER
            .lock()
            .unwrap()
            .debug("main", &format!("Read {} bytes from stdin", buf.len()));
        buf
    } else {
        cli.text
    };

    LOGGER.lock().unwrap().debug(
        "main",
        &format!("Testing pattern against {} bytes of input", input.len()),
    );

    if regex.is_match(&input) {
        LOGGER
            .lock()
            .unwrap()
            .debug("main", "Pattern MATCHES! Finding all occurrences...");
        let matches = regex.find_all(&input);
        LOGGER
            .lock()
            .unwrap()
            .debug("main", &format!("Found {} matches", matches.len()));

        for (i, m) in matches.iter().enumerate() {
            println!("{}..{}", m.start, m.end);
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

    Ok(())
}
