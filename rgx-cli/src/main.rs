use clap::{Parser, Subcommand};
use rgx_core::{Regex, ExecutionMode};

#[derive(Parser, Debug)]
#[command(name = "rgx", version, about = "Next-gen high-performance regex engine")]
struct Cli {
    /// Execution mode: pure | safe | full
    #[arg(long, value_parser = ["pure", "safe", "full"], default_value = "pure")]
    mode: String,

    /// Pattern to match
    pattern: String,

    /// Input text (if omitted, reads from stdin)
    #[arg(default_value = "")]
    text: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mode = match cli.mode.as_str() {
        "pure" => ExecutionMode::Pure,
        "safe" => ExecutionMode::Safe,
        _ => ExecutionMode::Full,
    };

    let regex = if mode == ExecutionMode::Pure {
        Regex::compile(&cli.pattern)?
    } else {
        Regex::with_mode(&cli.pattern, mode)?
    };

    let input = if cli.text.is_empty() {
        use std::io::{self, Read};
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        cli.text
    };

    if regex.is_match(&input) {
        for m in regex.find_all(&input) {
            println!("{}..{}", m.start, m.end);
        }
    }

    Ok(())
}

