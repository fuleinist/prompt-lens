use clap::{Parser, Subcommand};
use std::io::Read;

mod tokenizer;
mod analyzer;
mod optimizer;
mod diff;
mod config;

pub use tokenizer::count_tokens;
pub use analyzer::{AnalyzedPrompt, Layer};
pub use optimizer::Suggestion;
pub use diff::DiffResult;

#[derive(Parser)]
#[command(name = "prompt-lens")]
#[command(about = "Visualize prompt structure, tokenize in real-time, and optimize token count", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Model for cost calculation: claude-3-5-sonnet, gpt-4o, gpt-3.5-turbo
    #[arg(long, default_value = "claude-3-5-sonnet")]
    model: String,

    /// Maximum characters per line in output
    #[arg(long, default_value_t = 80)]
    width: usize,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze a prompt: show token count, cost estimate, structural breakdown
    Analyze {
        /// The prompt text to analyze (or - to read from stdin)
        #[arg(allow_hyphen_values = true)]
        prompt: Option<String>,

        /// Read prompt from file instead of argument
        #[arg(short, long)]
        file: Option<String>,

        /// Output as JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Visualize prompt structure with color-coded layers
    Visualize {
        /// The prompt text to visualize
        #[arg(allow_hyphen_values = true)]
        prompt: Option<String>,

        /// Read prompt from file
        #[arg(short, long)]
        file: Option<String>,
    },

    /// Suggest and apply optimizations to reduce token count
    Optimize {
        /// The prompt text to optimize
        #[arg(allow_hyphen_values = true)]
        prompt: Option<String>,

        /// Read prompt from file
        #[arg(short, long)]
        file: Option<String>,

        /// Apply suggestions automatically (dry-run by default)
        #[arg(short, long)]
        apply: bool,

        /// Output as JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Compare two prompts: side-by-side diff with token delta
    Diff {
        /// First prompt
        #[arg(allow_hyphen_values = true)]
        prompt1: String,

        /// Second prompt
        #[arg(allow_hyphen_values = true)]
        prompt2: String,

        /// Output as JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Initialize .prompt-lens.yaml config file
    Init {
        /// Force overwrite existing config
        #[arg(short, long)]
        force: bool,
    },
}

fn read_prompt(prompt: Option<String>, file: Option<String>) -> String {
    if let Some(fpath) = file {
        std::fs::read_to_string(&fpath)
            .unwrap_or_else(|_| {
                eprintln!("Error: file not found: {}", fpath);
                std::process::exit(2);
            })
    } else if let Some(p) = prompt {
        p
    } else if !atty::is(atty::Stream::Stdin) {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap_or_default();
        input
    } else {
        eprintln!("Error: no prompt provided. Use argument, --file, or pipe stdin.");
        std::process::exit(1);
    }
}

fn get_prompt_text(raw: String) -> String {
    raw.trim().to_string()
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Analyze { prompt, file, json }) => {
            let raw = read_prompt(prompt, file);
            let text = get_prompt_text(raw);
            if text.is_empty() {
                eprintln!("Error: no content to analyze");
                std::process::exit(1);
            }
            let analysis = analyzer::analyze(&text, &cli.model);
            if json {
                println!("{}", serde_json::to_string_pretty(&analysis).unwrap());
            } else {
                analyzer::print_analysis(&analysis, cli.width);
            }
        }

        Some(Commands::Visualize { prompt, file }) => {
            let raw = read_prompt(prompt, file);
            let text = get_prompt_text(raw);
            if text.is_empty() {
                eprintln!("Error: no content to visualize");
                std::process::exit(1);
            }
            analyzer::visualize(&text, cli.width);
        }

        Some(Commands::Optimize { prompt, file, apply, json }) => {
            let raw = read_prompt(prompt, file);
            let text = get_prompt_text(raw);
            if text.is_empty() {
                eprintln!("Error: no content to optimize");
                std::process::exit(1);
            }
            let analysis = analyzer::analyze(&text, &cli.model);
            let suggestions = optimizer::suggest(&text, &analysis);
            if json {
                println!("{}", serde_json::to_string_pretty(&suggestions).unwrap());
            } else {
                optimizer::print_suggestions(&text, &suggestions, apply, cli.width);
            }
        }

        Some(Commands::Diff { prompt1, prompt2, json }) => {
            let diff = diff::diff_prompts(&prompt1, &prompt2);
            if json {
                println!("{}", serde_json::to_string_pretty(&diff).unwrap());
            } else {
                diff::print_diff(&diff, cli.width);
            }
        }

        Some(Commands::Init { force }) => {
            config::init_config(force);
        }

        None => {
            // Default: analyze stdin or show help
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).ok();
            let text = input.trim();
            if text.is_empty() {
                Cli::parse();
            } else {
                let analysis = analyzer::analyze(text, &cli.model);
                analyzer::print_analysis(&analysis, cli.width);
            }
        }
    }
}