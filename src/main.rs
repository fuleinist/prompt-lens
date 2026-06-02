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

    /// Model for cost calculation: claude-3-5-sonnet, gpt-4o, gpt-3.5-turbo.
    /// Falls back to `default_model` in `.prompt-lens.yaml` when omitted.
    #[arg(long)]
    model: Option<String>,

    /// Maximum characters per line in output
    #[arg(long, default_value_t = 80)]
    width: usize,
}

/// Resolve the model to use: explicit `--model` flag wins, then
/// `default_model` from `.prompt-lens.yaml`, then the built-in fallback.
fn resolve_model(explicit: Option<String>) -> String {
    explicit
        .or_else(config::default_model_from_config)
        .unwrap_or_else(|| "claude-3-5-sonnet".to_string())
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
        /// First prompt (use --file1 to read from file instead)
        #[arg(allow_hyphen_values = true)]
        prompt1: Option<String>,

        /// Second prompt (use --file2 to read from file instead)
        #[arg(allow_hyphen_values = true)]
        prompt2: Option<String>,

        /// Read first prompt from file
        #[arg(short = 'f', long = "file1")]
        file1: Option<String>,

        /// Read second prompt from file
        #[arg(long = "file2")]
        file2: Option<String>,

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
    let model = resolve_model(cli.model);

    match cli.command {
        Some(Commands::Analyze { prompt, file, json }) => {
            let raw = read_prompt(prompt, file);
            let text = get_prompt_text(raw);
            if text.is_empty() {
                eprintln!("Error: no content to analyze");
                std::process::exit(1);
            }
            let analysis = analyzer::analyze(&text, &model);
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
            let analysis = analyzer::analyze(&text, &model);
            let suggestions = optimizer::suggest(&text, &analysis);
            if json {
                println!("{}", serde_json::to_string_pretty(&suggestions).unwrap());
            } else {
                optimizer::print_suggestions(&text, &suggestions, apply, cli.width);
            }
        }

        Some(Commands::Diff { prompt1, prompt2, file1, file2, json }) => {
            let text1 = read_prompt(prompt1, file1);
            let text2 = read_prompt(prompt2, file2);
            if text1.is_empty() {
                eprintln!("Error: no content for prompt1. Use argument, --file1, or pipe stdin.");
                std::process::exit(1);
            }
            if text2.is_empty() {
                eprintln!("Error: no content for prompt2. Use argument, --file2, or pipe stdin.");
                std::process::exit(1);
            }
            let diff = diff::diff_prompts(&text1, &text2);
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
                let analysis = analyzer::analyze(text, &model);
                analyzer::print_analysis(&analysis, cli.width);
            }
        }
    }
}