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
    let trimmed1: Vec<&str> = lines1.iter().map(|s| s.trim()).collect();
    let trimmed2: Vec<&str> = lines2.iter().map(|s| s.trim()).collect();

    // Count how many times each non-empty line appears in each prompt
    let mut count1: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut count2: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for l in &trimmed1 { if !l.is_empty() { *count1.entry(l).or_insert(0) += 1; } }
    for l in &trimmed2 { if !l.is_empty() { *count2.entry(l).or_insert(0) += 1; } }

    // Compare counts: extra occurrences in prompt1 are "removed", extra in prompt2 are "added"
    let mut removed: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();

    let all_keys: std::collections::HashSet<&str> = count1.keys().chain(count2.keys()).cloned().collect();
    for key in all_keys {
        let c1 = count1.get(key).copied().unwrap_or(0);
        let c2 = count2.get(key).copied().unwrap_or(0);
        if c1 > c2 {
            // Line appears more times in prompt1 — the extras are "removed"
            for _ in 0..(c1 - c2) {
                removed.push(key.to_string());
            }
        } else if c2 > c1 {
            // Line appears more times in prompt2 — the extras are "added"
            for _ in 0..(c2 - c1) {
                added.push(key.to_string());
            }
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_handles_duplicate_lines() {
        // "hello" appears twice in prompt1 but once in prompt2.
        // The extra "hello" in prompt1 should be shown as removed.
        let result = diff_prompts("hello\nhello\nworld", "hello\nworld");
        assert_eq!(result.added_lines.len(), 0, "should have no additions");
        assert_eq!(result.removed_lines.len(), 1, "should show 1 removed line");
        assert_eq!(result.removed_lines[0], "hello");
    }

    #[test]
    fn test_diff_additions_and_removals() {
        let result = diff_prompts("a\nb", "b\nc");
        assert_eq!(result.added_lines, vec!["c"]);
        assert_eq!(result.removed_lines, vec!["a"]);
    }

    #[test]
    fn test_diff_identical() {
        let result = diff_prompts("hello world", "hello world");
        assert!(result.added_lines.is_empty());
        assert!(result.removed_lines.is_empty());
    }
}