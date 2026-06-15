use clap::{Args, Parser, Subcommand};
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
    #[command(flatten)]
    common: CommonArgs,

    #[command(subcommand)]
    command: Option<Commands>,
}

/// Flags that should be accepted at any level of the CLI — before the
/// subcommand *and* after it. The README and the existing
/// `prompt-lens analyze --model gpt-4o "Your prompt here"` example both
/// rely on the post-subcommand form, which the previous top-level
/// placement of `model`/`width` did not allow (clap rejected the
/// positional prompt with "unexpected argument 'Your prompt here'
/// found"). The `global = true` attribute is what makes clap accept the
/// flag in either position.
#[derive(Args)]
struct CommonArgs {
    /// Model for cost calculation: claude-3-5-sonnet, gpt-4o, gpt-3.5-turbo.
    /// Falls back to `default_model` in `.prompt-lens.yaml` when omitted.
    #[arg(long, global = true)]
    model: Option<String>,

    /// Maximum characters per line in output
    #[arg(long, default_value_t = 80, global = true)]
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
    let model = resolve_model(cli.common.model);
    let width = cli.common.width;

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
                analyzer::print_analysis(&analysis, width);
            }
        }

        Some(Commands::Visualize { prompt, file }) => {
            let raw = read_prompt(prompt, file);
            let text = get_prompt_text(raw);
            if text.is_empty() {
                eprintln!("Error: no content to visualize");
                std::process::exit(1);
            }
            analyzer::visualize(&text, width);
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
                optimizer::print_suggestions(&text, &suggestions, apply, width);
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
                diff::print_diff(&diff, width);
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
                analyzer::print_analysis(&analysis, width);
            }
        }
    }
}

#[cfg(test)]
mod cli_flag_placement_tests {
    use super::*;

    // The README and the example in `prompt-lens analyze --model gpt-4o
    // "Your prompt here"` rely on `--model` being accepted *after* the
    // subcommand keyword. The previous top-level placement of the
    // `model` and `width` fields made clap reject the trailing
    // positional prompt with "unexpected argument ... found". The
    // `CommonArgs` flatten + `global = true` fix is what makes the
    // post-subcommand form parse cleanly. These tests pin both
    // placements so a future refactor can't silently break the
    // documented invocation order.
    //
    // We exercise `Cli::try_parse_from` directly rather than running
    // the binary, so the tests stay fast and don't print to a real TTY.

    fn model_from(args: &[&str]) -> Option<String> {
        let cli = Cli::try_parse_from(args).expect("CLI should parse");
        cli.common.model
    }

    fn width_from(args: &[&str]) -> usize {
        let cli = Cli::try_parse_from(args).expect("CLI should parse");
        cli.common.width
    }

    #[test]
    fn model_flag_accepted_after_subcommand() {
        // Documented form: `prompt-lens analyze --model gpt-4o "..."`.
        // This regressed when `model` lived on the top-level `Cli`
        // struct because clap expected the positional prompt before
        // any subcommand-level flag.
        assert_eq!(
            model_from(&["prompt-lens", "analyze", "--model", "gpt-4o", "Hello"]),
            Some("gpt-4o".to_string())
        );
    }

    #[test]
    fn model_flag_accepted_before_subcommand() {
        // Pre-subcommand form, which worked before the fix and must
        // keep working after it.
        assert_eq!(
            model_from(&["prompt-lens", "--model", "gpt-4o", "analyze", "Hello"]),
            Some("gpt-4o".to_string())
        );
    }

    #[test]
    fn model_flag_equals_form_accepted_after_subcommand() {
        // `--model=gpt-4o` is the form clap recommends for flag+value
        // pairs that share a token. Same constraint as
        // model_flag_accepted_after_subcommand.
        assert_eq!(
            model_from(&["prompt-lens", "analyze", "--model=gpt-4o", "Hello"]),
            Some("gpt-4o".to_string())
        );
    }

    #[test]
    fn width_flag_accepted_after_subcommand_with_default_otherwise() {
        // The previous placement only let `--width` come before the
        // subcommand. Pin both that the post-subcommand form parses
        // and that the default (80) is preserved when the flag is
        // omitted.
        assert_eq!(
            width_from(&["prompt-lens", "analyze", "--width", "100", "Hello"]),
            100
        );
        assert_eq!(
            width_from(&["prompt-lens", "analyze", "Hello"]),
            80
        );
    }

    #[test]
    fn model_flag_after_optimize_subcommand_also_parses() {
        // The bug affected every subcommand that has a positional
        // prompt, not just `analyze`. Pin `optimize` so a future
        // subcommand-only fix (e.g. moving the flag onto one
        // subcommand) doesn't regress the others.
        assert_eq!(
            model_from(&[
                "prompt-lens",
                "optimize",
                "--model",
                "claude-3-5-sonnet",
                "Hello"
            ]),
            Some("claude-3-5-sonnet".to_string())
        );
    }
}