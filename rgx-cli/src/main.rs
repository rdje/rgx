use clap::Parser;
use rgx_core::{Regex, ExecutionMode};
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Parser, Debug)]
#[command(name = "rgx", version, about = "Next-gen high-performance regex engine")]
struct Cli {
    /// Execution mode: pure | safe | full
    #[arg(long, value_parser = ["pure", "safe", "full"], default_value = "pure")]
    mode: String,

    /// Enable debug output (shows compilation and execution details)
    #[arg(long, short = 'd')]
    debug: bool,

    /// Enable trace output (shows EVERYTHING - very verbose)
    #[arg(long, short = 't')]
    trace: bool,

    /// Pattern to match
    pattern: String,

    /// Input text (if omitted, reads from stdin)
    #[arg(default_value = "")]
    text: String,
}

/// Global logger for the entire rgx system
pub static LOGGER: once_cell::sync::Lazy<Arc<Mutex<Logger>>> = 
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Logger::new())));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Off,
    Debug,
    Trace,
}

pub struct Logger {
    level: LogLevel,
}

impl Logger {
    fn new() -> Self {
        Self { level: LogLevel::Off }
    }
    
    pub fn set_level(&mut self, level: LogLevel) {
        self.level = level;
    }
    
    pub fn debug(&self, module: &str, msg: &str) {
        if self.level >= LogLevel::Debug {
            eprintln!("[DEBUG] [{}] {}", module, msg);
        }
    }
    
    pub fn trace(&self, module: &str, msg: &str) {
        if self.level == LogLevel::Trace {
            eprintln!("[TRACE] [{}] {}", module, msg);
        }
    }
}

impl PartialOrd for LogLevel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LogLevel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Set up logging based on CLI flags
    {
        let mut logger = LOGGER.lock().unwrap();
        if cli.trace {
            logger.set_level(LogLevel::Trace);
            eprintln!("[TRACE MODE ENABLED] - Showing EVERYTHING that happens in rgx");
            eprintln!("================================================================================\n");
        } else if cli.debug {
            logger.set_level(LogLevel::Debug);
            eprintln!("[DEBUG MODE ENABLED] - Showing compilation and execution details");
            eprintln!("================================================================================\n");
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

    // Set the global debug flag for rgx_core BEFORE using it
    std::env::set_var("RGX_DEBUG", if cli.debug { "1" } else { "0" });
    std::env::set_var("RGX_TRACE", if cli.trace { "1" } else { "0" });
    
    // Initialize rgx_core logging system
    rgx_core::log::init();

    let regex = if mode == ExecutionMode::Pure {
        LOGGER.lock().unwrap().debug("main", "Compiling pattern in PURE mode...");
        Regex::compile(&cli.pattern)?
    } else {
        LOGGER.lock().unwrap().debug("main", &format!("Compiling pattern in {:?} mode...", mode));
        Regex::with_mode(&cli.pattern, mode)?
    };

    let input = if cli.text.is_empty() {
        use std::io::{self, Read};
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        LOGGER.lock().unwrap().debug("main", &format!("Read {} bytes from stdin", buf.len()));
        buf
    } else {
        cli.text
    };

    LOGGER.lock().unwrap().debug("main", &format!("Testing pattern against {} bytes of input", input.len()));
    
    if regex.is_match(&input) {
        LOGGER.lock().unwrap().debug("main", "Pattern MATCHES! Finding all occurrences...");
        let matches = regex.find_all(&input);
        LOGGER.lock().unwrap().debug("main", &format!("Found {} matches", matches.len()));
        
        for (i, m) in matches.iter().enumerate() {
            println!("{}..{}", m.start, m.end);
            LOGGER.lock().unwrap().trace("main", &format!("Match {}: {}..{} = '{}'" , 
                i, m.start, m.end, 
                &input[m.start..m.end.min(input.len())]
            ));
        }
    } else {
        LOGGER.lock().unwrap().debug("main", "Pattern does NOT match");
    }

    Ok(())
}

