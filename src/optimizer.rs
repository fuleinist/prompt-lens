//! Optimizer — suggest token-saving transformations.

use regex::Regex;

use crate::tokenizer::count_tokens;
use crate::analyzer::AnalyzedPrompt;

#[derive(Debug, serde::Serialize)]
pub struct Suggestion {
    pub description: String,
    pub before_tokens: usize,
    pub after_tokens: usize,
    pub location: String,
    pub pattern: String,
    /// String to substitute for `pattern` when applying. `None` means
    /// delete the matched text (used for duplicate-line / long-header /
    /// example-block suggestions where the user just wants the noise gone).
    pub replacement: Option<String>,
}

pub fn suggest(text: &str, _analysis: &AnalyzedPrompt) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();

    // 1. Remove redundant polite phrases
    let polite_phrases = [
        ("please kindly", "please"),
        ("please be so kind as to", "please"),
        ("could you please", "please"),
        ("would you be so kind as to", "please"),
        ("I would like you to", "you"),
        ("it is requested that you", "you"),
        ("you are hereby requested to", "please"),
        ("in order to", "to"),
        ("due to the fact that", "because"),
        ("for the purpose of", "to"),
        ("at this point in time", "now"),
        ("in the event that", "if"),
        ("with regard to", "about"),
        ("in reference to", "about"),
    ];

    let lower = text.to_lowercase();
    for (verbose, concise) in polite_phrases {
        if lower.contains(verbose) {
            // Count how many times it appears
            let count = lower.matches(verbose).count();
            let before = count_tokens(&format!("{} ", verbose).repeat(count), "claude-3-5-sonnet");
            let after = count_tokens(&format!("{} ", concise).repeat(count), "claude-3-5-sonnet");
            if after < before {
                suggestions.push(Suggestion {
                    description: format!("Replace \"{}\" with \"{}\" ({} occurrences)", verbose, concise, count),
                    before_tokens: before,
                    after_tokens: after,
                    location: "text-wide".to_string(),
                    pattern: verbose.to_string(),
                    replacement: Some(concise.to_string()),
                });
            }
        }
    }

    // 2. Merge duplicate consecutive lines
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j].trim() == lines[i].trim() && !lines[i].trim().is_empty() {
            j += 1;
        }
        if j > i + 1 {
            let dup_count = j - i;
            let removed = dup_count - 1;
            let dup_tokens = count_tokens(&lines[i].repeat(removed), "claude-3-5-sonnet");
            suggestions.push(Suggestion {
                description: format!("Remove {} duplicate consecutive line(s)", removed),
                before_tokens: dup_tokens,
                after_tokens: 0,
                location: format!("line {}", i + 1),
                pattern: lines[i].trim().to_string(),
                replacement: None,
            });
        }
        i += 1;
    }

    // 3. Shorten markdown headers
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with('#') && trimmed.len() > 30 {
            let header_text = trimmed.trim_start_matches('#').trim();
            let excess = trimmed.len() - header_text.len() - 30;
            if excess > 0 {
                let surplus = header_text.len() - (header_text.len().saturating_sub(excess));
                if surplus > 5 {
                    suggestions.push(Suggestion {
                        description: format!("Shorten header ({} chars over limit)", excess),
                        before_tokens: count_tokens(trimmed, "claude-3-5-sonnet"),
                        after_tokens: count_tokens(header_text, "claude-3-5-sonnet"),
                        location: format!("line {}: \"{}\"", lines.iter().position(|l| *l == *line).unwrap_or(0) + 1, &trimmed[..20.min(trimmed.len())]),
                        pattern: trimmed.to_string(),
                        replacement: None,
                    });
                }
            }
        }
    }

    // 4. Suggest moving long examples to files
    let example_block = detect_long_example(text);
    if let Some((start, end, chars)) = example_block {
        let tokens = count_tokens(&text[start..end], "claude-3-5-sonnet");
        suggestions.push(Suggestion {
            description: format!("Move example block ({} chars, ~{} tokens) to a file and reference via path", chars, tokens),
            before_tokens: tokens,
            after_tokens: 30, // Just the path reference
            location: format!("chars {}-{}", start, end),
            pattern: "example_block".to_string(),
            replacement: None,
        });
    }

    suggestions.sort_by_key(|s| s.before_tokens.saturating_sub(s.after_tokens));
    suggestions
}

fn detect_long_example(text: &str) -> Option<(usize, usize, usize)> {
    let markers = ["example:", "example:", "for example:", "e.g.", "such as:"];
    let mut best: Option<(usize, usize, usize)> = None;

    for marker in markers {
        if let Some(pos) = text.to_lowercase().find(marker) {
            // Find the block after the marker
            let after = &text[pos..];
            let block_end = after.find("\n\n")
                .or_else(|| after.find("\n\n\n"))
                .map(|p| pos + p)
                .unwrap_or(text.len());
            let chars = block_end - pos;
            if chars > 200 {
                match best {
                    None => best = Some((pos, block_end, chars)),
                    Some((_, _, best_chars)) if chars > best_chars => {
                        best = Some((pos, block_end, chars));
                    }
                    _ => {}
                }
            }
        }
    }
    best
}

/// Quick suggestions based on analysis alone (no full text scan).
pub fn quick_suggestions(a: &AnalyzedPrompt) -> Vec<String> {
    let mut s = Vec::new();
    if a.context_used > 0.8 {
        s.push(format!("Warning: {}% of context window used", (a.context_used * 100.0).round() as usize));
    }
    if a.lists > 5 {
        s.push(format!("Consider consolidating {} lists into fewer, shorter lists", a.lists));
    }
    if a.tables > 3 {
        s.push(format!("Warning: {} table rows detected — consider CSV format", a.tables));
    }
    if a.sections > 10 {
        s.push(format!("{} section headers detected — reduce if verbose", a.sections));
    }
    s
}

pub fn print_suggestions(text: &str, suggestions: &[Suggestion], apply: bool, _width: usize) {
    const RESET: &str = "\x1b[0m";
    const BOLD: &str = "\x1b[1m";
    const GREEN: &str = "\x1b[32m";
    const CYAN: &str = "\x1b[36m";

    println!("{BOLD}══╗ prompt-lens optimize{RESET}");
    println!("  {CYAN}╠══{RESET} Found {} suggestions:", suggestions.len());

    let mut total_saved = 0usize;
    for (i, sug) in suggestions.iter().enumerate() {
        let saved = sug.before_tokens.saturating_sub(sug.after_tokens);
        total_saved += saved;
        let pct = if sug.before_tokens > 0 {
            (saved as f64 / sug.before_tokens as f64 * 100.0).round() as usize
        } else {
            0
        };
        println!("  {CYAN}║   {GREEN}{}:{RESET} {}", i + 1, sug.description);
        println!("  {CYAN}║       {}-{RESET} tokens: {} → {} (-{saved}, {pct}%)", sug.before_tokens, sug.after_tokens, pct);
        println!("  {CYAN}║       {GREEN}@{RESET} {}", sug.location);
    }

    let current = count_tokens(text, "claude-3-5-sonnet");
    let after = current.saturating_sub(total_saved);
    println!("  {CYAN}╠══{RESET} {BOLD}Estimated savings: {} tokens ({} → {}){RESET}", total_saved, current, after);
    println!("  {CYAN}╚══{RESET} {GREEN}Run with --apply to apply suggestions automatically{RESET}");

    if apply {
        println!("\n  {CYAN}Apply mode — apply all suggestions:{RESET}");
        let optimized = apply_suggestions(text, suggestions);
        println!("{BOLD}Optimized prompt:{RESET}");
        println!("{}", optimized);
    }
}

fn apply_suggestions(text: &str, suggestions: &[Suggestion]) -> String {
    let mut result = text.to_string();
    for sug in suggestions {
        let replacement = sug.replacement.as_deref().unwrap_or("");
        // Use case-insensitive regex for reliable replacement
        if let Ok(re) = Regex::new(&format!(r"(?i){}", regex::escape(&sug.pattern))) {
            result = re.replace_all(&result, replacement).to_string();
        } else {
            // Fallback to simple replace
            result = result.replace(&sug.pattern, replacement);
        }
    }
    result.trim().to_string()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_suggestions_case_insensitive() {
        // Test that "Please kindly" gets replaced even when capitalized
        let text = "Please kindly assist me with this task.";
        let suggestions = vec![Suggestion {
            description: "Replace polite phrase".to_string(),
            before_tokens: 3,
            after_tokens: 1,
            location: "text-wide".to_string(),
            pattern: "please kindly".to_string(),
            replacement: Some("please".to_string()),
        }];
        let result = apply_suggestions(text, &suggestions);
        assert!(!result.contains("Please kindly"), "Should replace 'Please kindly' case-insensitively");
        assert!(result.contains("assist me"), "Should preserve remaining content");
    }

    #[test]
    fn test_apply_suggestions_preserves_remaining() {
        let text = "Could you please help me with this task";
        let suggestions = vec![Suggestion {
            description: "Replace verbose phrase".to_string(),
            before_tokens: 4,
            after_tokens: 2,
            location: "text-wide".to_string(),
            pattern: "could you please".to_string(),
            replacement: Some("please".to_string()),
        }];
        let result = apply_suggestions(text, &suggestions);
        assert!(!result.contains("Could you please"), "Should replace the phrase");
        assert!(result.contains("help me"), "Should preserve remaining");
    }

    #[test]
    fn test_apply_suggestions_replaces_with_concise_form() {
        // Polite phrase suggestions must substitute the concise form,
        // not delete the verbose phrase entirely. Previously, "Please
        // kindly" was replaced with "" so the resulting prompt lost
        // the intended meaning ("Please assist me" -> "assist me").
        let text = "Please kindly help me with this task.";
        let suggestions = vec![Suggestion {
            description: "Replace polite phrase".to_string(),
            before_tokens: 3,
            after_tokens: 1,
            location: "text-wide".to_string(),
            pattern: "please kindly".to_string(),
            replacement: Some("please".to_string()),
        }];
        let result = apply_suggestions(text, &suggestions);
        assert!(result.contains("please help me"), "Should keep the concise form 'please' in place of 'Please kindly'");
        assert!(!result.contains("kindly"), "Should remove the verbose word 'kindly'");
    }

    #[test]
    fn test_apply_suggestions_none_replacement_deletes() {
        // Suggestions with replacement: None should delete the pattern
        // (used for duplicate-line / long-header / example-block noise).
        let text = "keep this. delete_me. end.";
        let suggestions = vec![Suggestion {
            description: "Remove noise".to_string(),
            before_tokens: 1,
            after_tokens: 0,
            location: "text-wide".to_string(),
            pattern: "delete_me".to_string(),
            replacement: None,
        }];
        let result = apply_suggestions(text, &suggestions);
        assert_eq!(result, "keep this. . end.");
        assert!(!result.contains("delete_me"));
    }
}
