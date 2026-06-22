//! Analyzer — parse prompt structure into layers.

use crate::tokenizer::{count_tokens, cost_per_token, context_limit};

#[derive(Debug, Clone, serde::Serialize)]
pub enum LayerType {
    System,
    User,
    Assistant,
    Tool,
    Section,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Layer {
    pub layer_type: LayerType,
    pub content: String,
    pub start: usize,
    pub end: usize,
    pub tokens: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct AnalyzedPrompt {
    pub tokens: usize,
    pub cost: f64,
    pub context_used: f64,   // 0.0–1.0 fraction of context window
    pub context_limit: usize,
    pub layers: Vec<Layer>,
    pub sections: usize,
    pub lists: usize,
    pub tables: usize,
    pub lines: usize,
    #[serde(skip)]
    pub costly_sections: Vec<(usize, usize, usize)>, // (start_line, end_line, tokens)
    pub model: String,
}

pub fn analyze(text: &str, model: &str) -> AnalyzedPrompt {
    let lines: Vec<&str> = text.lines().collect();
    let total_tokens = count_tokens(text, model);
    let limit = context_limit(model);
    let context_used = total_tokens as f64 / limit as f64;
    let cost = total_tokens as f64 * cost_per_token(model);

    let layers = detect_layers(text);
    let sections = count_pattern(text, "##");
    let lists = count_pattern(text, "\n- ") + count_pattern(text, "\n* ") + count_pattern(text, "\n1. ");
    let tables = count_tables(text);
    let costly = find_costly_sections(&lines, &layers, total_tokens);

    AnalyzedPrompt {
        tokens: total_tokens,
        cost,
        context_used,
        context_limit: limit,
        layers,
        sections,
        lists,
        tables,
        lines: lines.len(),
        costly_sections: costly,
        model: model.to_string(),
    }
}

fn detect_layers(text: &str) -> Vec<Layer> {
    let mut layers = Vec::new();

    // System prompt detection: markdown code fences, XML tags, or prominent markers
    let sys_patterns = [
        ("<system>", "</system>", LayerType::System),
        ("[SYSTEM]", "[/SYSTEM]", LayerType::System),
        ("<|system|>", "<|/system|>", LayerType::System),
    ];

    for (open, close, ltype) in sys_patterns {
        if let Some(start) = text.find(open) {
            let end = text.find(close).map(|p| p + close.len()).unwrap_or(text.len());
            let content = &text[start..end];
            layers.push(Layer {
                layer_type: ltype,
                content: content.to_string(),
                start,
                end,
                tokens: count_tokens(content, "claude-3-5-sonnet"),
            });
        }
    }

    // Tool definitions
    let tool_patterns = ["<tool>", "```tool", "<function>", "<tool_call>"];
    for pat in tool_patterns {
        if let Some(start) = text.find(pat) {
            // Find the closing marker (`</tool>` is 7 bytes, ``` is 3)
            // and advance `end` past it. For markdown fences we must
            // skip past the opener first, otherwise `find("```")` would
            // match the opening fence back to itself and `end` would
            // land inside the opener. The previous implementation tried
            // to infer which pattern matched from `text[start..]`'s
            // prefix, which gave the right answer only when the opener
            // happened to start with ```; for `<tool>...</tool>` it
            // also worked by accident, but for `\`\`\`tool...\n``` ` it
            // truncated the tool block to the three opening bytes.
            let end = text[start..]
                .find("</tool>")
                .map(|p| (p, "</tool>".len()))
                .or_else(|| {
                    text[start + pat.len()..]
                        .find("```")
                        .map(|p| (p + pat.len(), "```".len()))
                })
                .map(|(p, len)| start + p + len)
                .unwrap_or(text.len());
            let content = &text[start..end.min(text.len())];
            layers.push(Layer {
                layer_type: LayerType::Tool,
                content: content.to_string(),
                start,
                end: end.min(text.len()),
                tokens: count_tokens(content, "claude-3-5-sonnet"),
            });
        }
    }

    // If no structured layers, treat whole prompt as User
    if layers.is_empty() {
        layers.push(Layer {
            layer_type: LayerType::User,
            content: text.to_string(),
            start: 0,
            end: text.len(),
            tokens: count_tokens(text, "claude-3-5-sonnet"),
        });
    }

    layers
}

fn count_pattern(text: &str, pattern: &str) -> usize {
    text.matches(pattern).count()
}

fn count_tables(text: &str) -> usize {
    // Simple heuristic: lines with multiple | separators
    let lines: Vec<&str> = text.lines().collect();
    let mut count = 0;
    for line in &lines {
        if line.contains('|') && line.chars().filter(|c| *c == '|').count() >= 3 {
            count += 1;
        }
    }
    count
}

fn find_costly_sections(
    lines: &[&str],
    _layers: &[Layer],
    total_tokens: usize,
) -> Vec<(usize, usize, usize)> {
    if lines.is_empty() {
        return Vec::new();
    }
    let tokens_per_line = total_tokens as f64 / lines.len() as f64;
    let threshold = tokens_per_line * 1.5;

    let mut costly = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line_tokens = count_tokens(lines[i], "claude-3-5-sonnet");
        if line_tokens as f64 > threshold && line_tokens > 10 {
            // Expand to include adjacent lines
            let start = i;
            let mut end = i;
            let mut j = i;
            while j < lines.len() {
                let lt = count_tokens(lines[j], "claude-3-5-sonnet");
                if lt as f64 > threshold * 0.7 {
                    end = j;
                    j += 1;
                } else {
                    break;
                }
            }
            let range_tokens = count_tokens(&lines[start..=end].join("\n"), "claude-3-5-sonnet");
            if range_tokens > threshold as usize {
                costly.push((start + 1, end + 1, range_tokens));
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    costly
}

/// Return the largest byte index `<= max_bytes` that ends a complete
/// char (i.e. a valid char boundary), so `&s[..result]` is a whole-char
/// prefix whose byte length does not exceed `max_bytes`.
fn safe_floor(s: &str, max_bytes: usize) -> usize {
    s.char_indices()
        .map(|(i, c)| (i, i + c.len_utf8()))
        .take_while(|&(_, end)| end <= max_bytes)
        .last()
        .map(|(_, end)| end)
        .unwrap_or(0)
}

pub fn visualize(text: &str, width: usize) {
    // Color output using ANSI codes
    // Cyan = system, Green = user, Yellow = assistant, Magenta = tool
    const RESET: &str = "\x1b[0m";
    const CYAN: &str = "\x1b[36m";
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";
    const MAGENTA: &str = "\x1b[35m";
    const BOLD: &str = "\x1b[1m";

    println!("{BOLD}══╗ prompt-lens visualize{RESET}");
    println!();

    for (i, line) in text.lines().enumerate() {
        let layer = detect_line_layer(line);
        let color = match layer {
            LayerType::System => CYAN,
            LayerType::User => GREEN,
            LayerType::Assistant => YELLOW,
            LayerType::Tool => MAGENTA,
            LayerType::Section => RESET,
        };

        let tokens = count_tokens(line, "claude-3-5-sonnet");
        let prefix = format!("{}[{:2}] ", color, i + 1);
        print!("{}", prefix);

        // Wrap long lines. The wrap budget is `width - 5` to leave room
        // for the `[NN] ` line prefix; we slice on a safe UTF-8 char
        // boundary, not a raw byte index, so non-ASCII lines don't panic.
        let budget = width.saturating_sub(5);
        if line.len() > budget {
            let mut remaining = line;
            let indent = "       ";
            while remaining.len() > budget {
                let cut_at = safe_floor(remaining, budget);
                if cut_at == 0 {
                    // Defensive: budget is smaller than a single char's UTF-8
                    // width. Emit one whole char on its own line so the loop
                    // always makes forward progress.
                    let c = remaining.chars().next().unwrap();
                    println!("{}{}{}", color, c, RESET);
                    remaining = &remaining[c.len_utf8()..];
                } else {
                    let cut = &remaining[..cut_at];
                    println!("{}{}", color, cut);
                    remaining = &remaining[cut_at..];
                }
                print!("{}", indent);
            }
            if !remaining.is_empty() {
                println!("{}{}{}", color, remaining, RESET);
            }
        } else {
            println!("{}{}{}", color, line, RESET);
        }

        if tokens > 50 {
            print!("{}[{:3} tokens]", CYAN, tokens);
        }
        println!();
    }
    println!("{RESET}");
    let total = count_tokens(text, "claude-3-5-sonnet");
    println!("{BOLD}Total: {} tokens{RESET}", total);
}

fn detect_line_layer(line: &str) -> LayerType {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        LayerType::Section
    } else if trimmed.starts_with("<system")
        || trimmed.starts_with("[SYSTEM]")
        // Mirror `detect_layers`'s third system-pattern entry so the
        // visualizer colours ChatML/Llama-2-chat `<|system|>` lines as
        // System instead of falling through to User. Previously these
        // lines were painted green even though `analyze` reported them
        // as a System layer, making `visualize` inconsistent with
        // `analyze`.
        || trimmed.starts_with("<|system|>")
    {
        LayerType::System
    } else if trimmed.starts_with("<tool") || trimmed.starts_with("```tool") {
        LayerType::Tool
    } else {
        LayerType::User
    }
}

pub fn print_analysis(a: &AnalyzedPrompt, _width: usize) {
    const RESET: &str = "\x1b[0m";
    const BOLD: &str = "\x1b[1m";
    const CYAN: &str = "\x1b[36m";
    const RED: &str = "\x1b[31m";
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";

    println!("{BOLD}══╗ prompt-lens analyze{RESET}");

    // Input summary
    let cost_str = format!("${:.4}", a.cost);
    println!("  {CYAN}╠══{RESET} Input: {} tokens | {cost_str} (@ {})", a.tokens, a.model);
    let pct = (a.context_used * 100.0).round() as usize;
    let limit_k = a.context_limit / 1000;
    let used_k = (a.tokens as f64 / 1000.0).round() as usize;
    println!("  {CYAN}╠══{RESET} Context: {}k/{}k ({pct}%)", used_k, limit_k);

    // Layer breakdown
    println!("  {CYAN}╠══{RESET} Breakdown by layer:");
    for layer in &a.layers {
        let name = match layer.layer_type {
            LayerType::System => "System",
            LayerType::User => "User",
            LayerType::Assistant => "Assistant",
            LayerType::Tool => "Tool",
            LayerType::Section => "Section",
        };
        let pct = if a.tokens > 0 {
            (layer.tokens as f64 / a.tokens as f64 * 100.0) as usize
        } else {
            0
        };
        let bar_len = (pct * 20 / 100).max(1).min(20);
        let bar: String = "█".repeat(bar_len) + &"░".repeat(20 - bar_len);
        println!("  {CYAN}║   {YELLOW}{:10}{RESET} {} tokens {CYAN}{}{RESET} {pct}%", name, layer.tokens, bar);
    }

    // Structural markers
    println!("  {CYAN}╠══{RESET} Structural markers: {} sections, {} lists, {} tables",
        a.sections, a.lists, a.tables);

    // Costly sections
    if !a.costly_sections.is_empty() {
        println!("  {CYAN}╠══{RESET} {RED}Costly sections:{RESET}");
        for (start, end, tokens) in &a.costly_sections {
            println!("  {CYAN}║   {RED}lines {}-{} ({} tokens){RESET}", start, end, tokens);
        }
    }

    // Suggestions (pre-computed or quick rules)
    let suggestions = crate::optimizer::quick_suggestions(a);
    if !suggestions.is_empty() {
        println!("  {CYAN}╠══{RESET} Suggestions ({}):", suggestions.len());
        for (i, s) in suggestions.iter().take(5).enumerate() {
            println!("  {CYAN}║   {GREEN}>{RESET} {}{}", i + 1, s);
        }
    }

    println!("  {CYAN}╚══{RESET} {BOLD}Total: {} tokens | ${:.4}{RESET}", a.tokens, a.cost);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_floor_ascii_unchanged() {
        // ASCII only: every char is one byte, so safe_floor is just
        // the min of max_bytes and the string length.
        assert_eq!(safe_floor("hello world", 5), 5);
        assert_eq!(safe_floor("hello world", 100), 11);
    }

    #[test]
    fn test_safe_floor_unicode_walks_back_to_boundary() {
        // 5× 'é' = 10 bytes, all multi-byte. Asking for a cut at byte 7
        // must walk back to 6 (end of the 3rd 'é') so the slice is whole
        // chars and does not panic.
        let s = "ééééé"; // bytes: [é0..2, é2..4, é4..6, é6..8, é8..10]
        assert_eq!(safe_floor(s, 7), 6);
        assert_eq!(safe_floor(s, 0), 0);
        assert_eq!(safe_floor(s, 100), s.len());
    }

    #[test]
    fn test_visualize_does_not_panic_on_long_unicode_line() {
        // Regression: visualizing a long line of multi-byte chars used to
        // panic with "byte index 75 is not a char boundary" because the
        // wrap loop sliced on raw byte indices.
        let text = "é".repeat(200);
        visualize(&text, 80);
    }

    #[test]
    fn test_visualize_does_not_panic_on_mixed_ascii_and_unicode() {
        // 70 ASCII chars puts the line just over the wrap budget; the next
        // char is a 2-byte 'é' so the original byte-75 cut would split
        // inside it.
        let mut text = "a".repeat(70);
        text.push('é');
        text.push_str(&"b".repeat(200));
        visualize(&text, 80);
    }

    fn tool_layers(text: &str) -> Vec<Layer> {
        analyze(text, "claude-3-5-sonnet")
            .layers
            .into_iter()
            .filter(|l| matches!(l.layer_type, LayerType::Tool))
            .collect()
    }

    #[test]
    fn test_tool_layer_markdown_fence_covers_closing_fence() {
        // Markdown code-fence tool block: ```tool ... ```. The detected
        // tool layer must cover both the opening and closing fences, not
        // just the first 3 bytes (the opening ``` itself). The previous
        // implementation called `text[start..].find("```")` *without*
        // skipping the opener, so `find` matched the opening fence and
        // `end` was set to `start + 0 + 3 = start + 3` — truncating the
        // tool block to the three bytes "```".
        let text = "```tool\nfn search(q: &str) { }\n```\nuser question";
        let tools = tool_layers(text);
        assert_eq!(tools.len(), 1, "expected exactly one tool layer");
        let layer = &tools[0];
        assert!(
            layer.content.contains("fn search"),
            "tool layer must contain the tool body, got: {:?}",
            layer.content
        );
        assert!(
            layer.content.ends_with("```"),
            "tool layer must extend to the closing fence, got: {:?}",
            layer.content
        );
        // Token count should reflect the actual tool body, not the 3
        // bytes of the opening fence.
        assert!(
            layer.tokens > 3,
            "tool layer tokens should be more than 3 (just the opener), got {}",
            layer.tokens
        );
    }

    #[test]
    fn test_tool_layer_xml_form_uses_correct_closing_offset() {
        // XML form `<tool>...</tool>`. The closing `</tool>` is 7 bytes
        // and the implementation must end *after* the closing tag, not
        // at the byte just before it (which would chop the final `>`).
        let text = "<tool>get_weather(city)</tool>";
        let tools = tool_layers(text);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].content, text, "XML tool layer should span the full block");
        assert_eq!(tools[0].end, text.len(), "end offset should be at the closing tag's end, not inside it");
    }

    #[test]
    fn test_detect_line_layer_chatml_system_marker() {
        // `detect_layers` already recognises `<|system|>` as a System
        // layer (Llama-2-chat / ChatML). `detect_line_layer` (used by
        // `visualize`) used to only check `<system` and `[SYSTEM]`,
        // so a line starting with `<|system|>` was painted User
        // instead of System — the visualizer disagreed with the
        // analyzer. Pin that the visualizer's per-line detector now
        // recognises the marker too.
        assert!(matches!(
            detect_line_layer("<|system|>You are a helpful assistant."),
            LayerType::System
        ));
        // Leading whitespace must not defeat the match.
        assert!(matches!(
            detect_line_layer("   <|system|>You are a helpful assistant."),
            LayerType::System
        ));
        // Plain prose still falls through to User.
        assert!(matches!(
            detect_line_layer("Hello there"),
            LayerType::User
        ));
    }
}