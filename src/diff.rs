//! Diff — compare two prompts.

use crate::tokenizer::count_tokens;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DiffResult {
    pub tokens1: usize,
    pub tokens2: usize,
    pub delta: i64,
    pub additions: usize,
    pub deletions: usize,
    pub added_lines: Vec<String>,
    pub removed_lines: Vec<String>,
}

pub fn diff_prompts(prompt1: &str, prompt2: &str) -> DiffResult {
    let lines1: Vec<&str> = prompt1.lines().collect();
    let lines2: Vec<&str> = prompt2.lines().collect();
    let set1: Vec<&str> = lines1.iter().map(|s| s.trim()).collect();
    let set2: Vec<&str> = lines2.iter().map(|s| s.trim()).collect();

    let added: Vec<String> = set2.iter()
        .filter(|l| !l.is_empty() && !set1.contains(l))
        .map(|s| s.to_string())
        .collect();

    let removed: Vec<String> = set1.iter()
        .filter(|l| !l.is_empty() && !set2.contains(l))
        .map(|s| s.to_string())
        .collect();

    let tokens1 = count_tokens(prompt1, "claude-3-5-sonnet");
    let tokens2 = count_tokens(prompt2, "claude-3-5-sonnet");

    DiffResult {
        tokens1,
        tokens2,
        delta: tokens2 as i64 - tokens1 as i64,
        additions: added.len(),
        deletions: removed.len(),
        added_lines: added,
        removed_lines: removed,
    }
}

pub fn print_diff(diff: &DiffResult, _width: usize) {
    const RESET: &str = "\x1b[0m";
    const BOLD: &str = "\x1b[1m";
    const GREEN: &str = "\x1b[32m";
    const RED: &str = "\x1b[31m";
    const CYAN: &str = "\x1b[36m";

    println!("{BOLD}══╗ prompt-lens diff{RESET}");

    let sign = if diff.delta >= 0 { "+" } else { "" };
    println!("  {CYAN}╠══{RESET} Token delta: {}{} tokens ({} → {})", sign, diff.delta, diff.tokens1, diff.tokens2);

    if !diff.removed_lines.is_empty() {
        println!("  {CYAN}╠══{RESET} {RED}Removed ({}):{RESET}", diff.deletions);
        for line in diff.removed_lines.iter().take(10) {
            println!("  {CYAN}║   {RED}-{RESET} {}", line.chars().take(80).collect::<String>());
        }
        if diff.removed_lines.len() > 10 {
            println!("  {CYAN}║   {RED}... and {} more{RESET}", diff.removed_lines.len() - 10);
        }
    }

    if !diff.added_lines.is_empty() {
        println!("  {CYAN}╠══{RESET} {GREEN}Added ({}):{RESET}", diff.additions);
        for line in diff.added_lines.iter().take(10) {
            println!("  {CYAN}║   {GREEN}+{RESET} {}", line.chars().take(80).collect::<String>());
        }
        if diff.added_lines.len() > 10 {
            println!("  {CYAN}║   {GREEN}... and {} more{RESET}", diff.added_lines.len() - 10);
        }
    }

    if diff.removed_lines.is_empty() && diff.added_lines.is_empty() {
        println!("  {CYAN}╠══{RESET} No text differences (token count may still differ)");
    }

    println!("  {CYAN}╚══{RESET} {} additions, {} deletions", diff.additions, diff.deletions);
}