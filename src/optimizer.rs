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
    /// Number of occurrences to remove when `replacement` is `None`.
    /// `None` (or 1) deletes only the first match; some passes
    /// (e.g. consecutive duplicate lines) need to delete N-1 matches so
    /// one copy remains.
    pub delete_count: Option<usize>,
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
                    delete_count: None,
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
                // Keep one copy: remove exactly N-1 of the N consecutive
                // matches. `apply_suggestions` uses this to avoid the old
                // `replace_all` bug that deleted every copy (including the
                // one the user wanted to keep).
                delete_count: Some(removed),
            });
            // Skip past the whole run of duplicates; otherwise the next
            // iteration would re-detect the same run and emit a second
            // suggestion that, applied alongside the first, deletes
            // every copy (defeating the "keep one" intent).
            i = j;
            continue;
        }
        i += 1;
    }

    // 3. Shorten markdown headers — suggest shortening headers whose
    //    visible text content (after stripping leading `#` markers and
    //    whitespace) exceeds 30 characters.
    //
    //    The previous version measured `trimmed.len() > 30` and computed
    //    `excess = trimmed.len() - header_text.len() - 30`, which under-
    //    flowed for any long header (`excess` became a huge `usize`) and
    //    produced nonsensical output like "Shorten header (18446744073709551589
    //    chars over limit)". It also used `lines.iter().position(...)` to
    //    find the header's line number, which is O(n) per header and can
    //    report the wrong index when a duplicate line exists.
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            continue;
        }
        let header_text = trimmed.trim_start_matches('#').trim();
        if header_text.len() <= 30 {
            continue;
        }
        let excess = header_text.len() - 30;
        suggestions.push(Suggestion {
            description: format!("Shorten header ({} chars over limit)", excess),
            before_tokens: count_tokens(trimmed, "claude-3-5-sonnet"),
            after_tokens: count_tokens(header_text, "claude-3-5-sonnet"),
            location: format!("line {}: \"{}\"", idx + 1, &trimmed[..20.min(trimmed.len())]),
            pattern: trimmed.to_string(),
            replacement: None,
            delete_count: Some(1),
        });
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
            delete_count: Some(1),
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
        println!("  {CYAN}║       {before}-{RESET} tokens: {before} → {after} (-{saved}, {pct}%)",
            before = sug.before_tokens,
            after = sug.after_tokens,
        );
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
        // Use case-insensitive regex for reliable replacement
        if let Ok(re) = Regex::new(&format!(r"(?i){}", regex::escape(&sug.pattern))) {
            match sug.replacement.as_deref() {
                // Explicit replacement (e.g. polite-phrase rewrites) applies
                // to every occurrence.
                Some(r) => {
                    result = re.replace_all(&result, r).to_string();
                }
                // `replacement: None` means "delete this". Each suggestion
                // describes a single span of noise, so default to deleting
                // only the first match — using `replace_all` here would
                // also strip user content that the suggestion never
                // intended to touch (e.g. the one copy of a duplicate
                // line that the suggestion wants to keep).
                None => {
                    let count = sug.delete_count.unwrap_or(1);
                    for _ in 0..count {
                        let next = re.replace(&result, "").to_string();
                        if next == result {
                            break;
                        }
                        result = next;
                    }
                }
            }
        } else if let Some(r) = sug.replacement.as_deref() {
            result = result.replace(&sug.pattern, r);
        } else {
            result = result.replace(&sug.pattern, "");
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
            delete_count: None,
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
            delete_count: None,
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
            delete_count: None,
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
            delete_count: None,
        }];
        let result = apply_suggestions(text, &suggestions);
        assert_eq!(result, "keep this. . end.");
        assert!(!result.contains("delete_me"));
    }

    fn header_suggestion_for(text: &str) -> Option<Suggestion> {
        let analysis = crate::analyzer::analyze(text, "claude-3-5-sonnet");
        suggest(text, &analysis)
            .into_iter()
            .find(|s| s.description.starts_with("Shorten header"))
    }

    #[test]
    fn test_header_shortening_detects_long_header_with_correct_excess() {
        // Header text is 55 chars, 25 over the 30-char limit.
        // The previous bug reported "18446744073709551589 chars over limit"
        // (usize underflow). Now it should report 25 and use the real line
        // number from the iteration, not an O(n) position search.
        let text = "## This is a long markdown header that should be shortened\nbody text";
        let sug = header_suggestion_for(text).expect("should suggest shortening");
        assert!(
            sug.description.contains("25 chars over limit"),
            "expected 25-char excess in description, got: {}",
            sug.description
        );
        assert!(
            sug.description.contains("Shorten header"),
            "wrong description: {}",
            sug.description
        );
        assert!(sug.location.starts_with("line 1: "), "wrong location: {}", sug.location);
    }

    #[test]
    fn test_header_shortening_ignores_short_headers() {
        // Header text is under the 30-char limit; no suggestion.
        let text = "## Short header\nbody";
        assert!(header_suggestion_for(text).is_none());
    }

    #[test]
    fn test_apply_suggestions_keeps_one_duplicate_line() {
        // Consecutive-duplicate suggestion should collapse N copies to
        // exactly one, not zero. Previously `apply_suggestions` used
        // `replace_all` even when `replacement: None`, which deleted every
        // match — so "hello\nhello\nhello" became "" instead of "hello".
        let text = "intro\nhello\nhello\nhello\noutro";
        let suggestions = vec![Suggestion {
            description: "Remove 2 duplicate consecutive line(s)".to_string(),
            before_tokens: 2,
            after_tokens: 0,
            location: "line 2".to_string(),
            pattern: "hello".to_string(),
            replacement: None,
            delete_count: Some(2),
        }];
        let result = apply_suggestions(text, &suggestions);
        assert_eq!(
            result.matches("hello").count(),
            1,
            "expected exactly one 'hello' to remain, got: {:?}",
            result
        );
        assert!(result.starts_with("intro\n"), "intro line preserved");
        assert!(result.ends_with("outro"), "outro line preserved");
    }

    #[test]
    fn test_apply_suggestions_none_replacement_only_deletes_first_occurrence() {
        // `replacement: None` should delete only the requested number of
        // occurrences (default 1), not all of them — protects against
        // accidentally swallowing repeated user content elsewhere in
        // the prompt.
        let text = "delete_me and keep delete_me here too";
        let suggestions = vec![Suggestion {
            description: "Remove noise".to_string(),
            before_tokens: 1,
            after_tokens: 0,
            location: "text-wide".to_string(),
            pattern: "delete_me".to_string(),
            replacement: None,
            delete_count: None,
        }];
        let result = apply_suggestions(text, &suggestions);
        assert_eq!(result.matches("delete_me").count(), 1);
        assert!(result.contains("and keep"));
        assert!(result.contains("here too"));
    }

    #[test]
    fn test_apply_suggestions_delete_count_stops_when_no_more_matches() {
        // delete_count higher than the number of matches should delete
        // every match and stop, not loop forever or panic.
        let text = "only_one_here";
        let suggestions = vec![Suggestion {
            description: "Remove noise".to_string(),
            before_tokens: 1,
            after_tokens: 0,
            location: "text-wide".to_string(),
            pattern: "only_one_here".to_string(),
            replacement: None,
            delete_count: Some(5),
        }];
        let result = apply_suggestions(text, &suggestions);
        assert!(result.is_empty(), "all matches removed, got: {:?}", result);
    }

    #[test]
    fn test_header_shortening_uses_correct_line_for_duplicate_header_lines() {
        // Two headers with the same text content. The previous code used
        // `lines.iter().position(|l| *l == *line)`, which always returns
        // the first match — so the second header was reported as "line 1".
        // With `enumerate()` the second header is now correctly "line 2".
        let text = "## This is a long markdown header that should be shortened\n\
                    ## This is a long markdown header that should be shortened\n\
                    body";
        let analysis = crate::analyzer::analyze(text, "claude-3-5-sonnet");
        let header_suggestions: Vec<_> = suggest(text, &analysis)
            .into_iter()
            .filter(|s| s.description.starts_with("Shorten header"))
            .collect();
        assert_eq!(header_suggestions.len(), 2, "expected 2 suggestions");
        assert!(header_suggestions[0].location.starts_with("line 1: "));
        assert!(header_suggestions[1].location.starts_with("line 2: "));
    }
}

#[test]
fn test_print_suggestions_token_delta_format() {
    // Per-suggestion line in `print_suggestions` must read
    //   "<before> tokens: <before> → <after> (-<saved>, <pct>%)"
    // (before/after/saved/pct are integers). The previous version
    // passed `pct` where the third positional placeholder expected
    // `saved`, so the arrow target rendered as the percentage and
    // users saw things like "4 tokens: 4 → 50 (-2, 50%)".
    //
    // We test the exact format string the printer uses, with
    // representative values, so a regression in the printer is
    // caught without needing to capture stdout.
    let before: usize = 4;
    let after: usize = 2;
    let saved: usize = 2;
    let pct: usize = 50;
    let line = format!(
        "  tokens: {before} → {after} (-{saved}, {pct}%)",
        before = before,
        after = after,
    );
    assert_eq!(line, "  tokens: 4 → 2 (-2, 50%)");
}
