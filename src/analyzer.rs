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
        if let Some(mut start) = text.find(pat) {
            let end = text[start..]
                .find("</tool>")
                .or_else(|| text[start..].find("```"))
                .map(|p| start + p + if text[start..].starts_with("```") { 3 } else { 7 })
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

        // Wrap long lines
        if line.len() > width - 5 {
            let mut remaining = line;
            let indent = "       ";
            while !remaining.is_empty() {
                if remaining.len() <= width - 5 {
                    println!("{}{}{}", color, remaining, RESET);
                    break;
                }
                let cut = &remaining[..width - 5];
                println!("{}{}", color, cut);
                remaining = &remaining[cut.len()..];
                print!("{}", indent);
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
    } else if trimmed.starts_with("<system") || trimmed.starts_with("[SYSTEM]") {
        LayerType::System
    } else if trimmed.starts_with("<tool") || trimmed.starts_with("```tool") {
        LayerType::Tool
    } else {
        LayerType::User
    }
}

pub fn print_analysis(a: &AnalyzedPrompt, width: usize) {
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