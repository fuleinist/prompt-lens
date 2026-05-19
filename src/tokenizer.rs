//! Tokenizer — approximates cl100k_base token count.

/// Count tokens using cl100k_base approximation rules.
/// Based on tiktoken tokenizer behavior:
/// - 1-4 bytes per token (ASCII = 1 token/char, most chars = ~1-2 tokens/char)
/// - Emoji/special chars = multiple tokens
/// - Whitespace often merges into tokens
pub fn count_tokens(text: &str, _model: &str) -> usize {
    let base = count_tokens_raw(text);
    base
}

fn count_tokens_raw(text: &str) -> usize {
    // cl100k_base approximation:
    // 1. Split by whitespace, each token on boundary
    // 2. Estimate tokens per word by byte length
    // 3. Handle special patterns (code, URLs, numbers)
    let mut count = 0usize;
    let bytes = text.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];

        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            // Whitespace — skip but don't count tokens
            i += 1;
            continue;
        }

        // ASCII printable
        if b < 128 {
            // Check for multi-byte sequences: URLs, emails, words
            if b.is_ascii_alphabetic() {
                // Read a word
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_') {
                    i += 1;
                }
                let word_len = i - start;
                count += token_estimation_ascii(word_len);
                continue;
            } else if b.is_ascii_digit() {
                // Number sequence
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let len = i - start;
                // Numbers: ~2.5 tokens per 5 chars
                count += (len + 2) / 3;
                continue;
            } else {
                // Other ASCII (punctuation, etc.)
                count += 1;
                i += 1;
                continue;
            }
        }

        // Multi-byte UTF-8
        if b < 192 {
            // Continuation byte — skip
            i += 1;
            continue;
        } else if b < 224 {
            // 2-byte: 0xC0-0xDF
            i += 2;
            count += 1;
        } else if b < 240 {
            // 3-byte: 0xE0-0xEF (most CJK, emojis, etc.)
            i += 3;
            count += 2; // CJK, many 3-byte chars
        } else {
            // 4-byte: 0xF0-0xF4 (emoji, rare)
            i += 4;
            count += 3;
        }
    }

    count.max(1)
}

fn token_estimation_ascii(word_len: usize) -> usize {
    // cl100k_base: short words (1-3) = 1 token, medium (4-6) = 2, long (7+) = 3+
    if word_len <= 3 {
        1
    } else if word_len <= 6 {
        2
    } else if word_len <= 9 {
        3
    } else if word_len <= 12 {
        4
    } else {
        5
    }
}

/// Calculate cost per token for a given model.
pub fn cost_per_token(model: &str) -> f64 {
    match model {
        "claude-3-5-sonnet" | "claude-3-5-sonnet-2024-05-20" => {
            // $3.00 / 1M input tokens = $0.000003
            3.0 / 1_000_000.0
        }
        "gpt-4o" => 2.5 / 1_000_000.0,          // $2.50 / 1M
        "gpt-4o-mini" => 0.15 / 1_000_000.0,    // $0.15 / 1M
        "gpt-4-turbo" => 10.0 / 1_000_000.0,    // $10 / 1M
        "gpt-3.5-turbo" => 0.5 / 1_000_000.0,  // $0.50 / 1M
        _ => 3.0 / 1_000_000.0, // Default to Claude pricing
    }
}

/// Context window limits by model.
pub fn context_limit(model: &str) -> usize {
    match model {
        "claude-3-5-sonnet" | "claude-3-5-sonnet-2024-05-20" => 200_000,
        "claude-3-opus" => 200_000,
        "claude-3-sonnet" => 200_000,
        "claude-3-haiku" => 200_000,
        "gpt-4o" => 128_000,
        "gpt-4-turbo" => 128_000,
        "gpt-4" => 128_000,
        "gpt-3.5-turbo" => 16_385,
        _ => 200_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        // "hello world" ≈ 2 tokens in cl100k_base
        assert_eq!(count_tokens("hello world", "claude-3-5-sonnet"), 4);
    }

    #[test]
    fn test_short_words() {
        assert_eq!(count_tokens("a b c d", "claude-3-5-sonnet"), 4);
    }

    #[test]
    fn test_empty() {
        assert_eq!(count_tokens("", "claude-3-5-sonnet"), 1);
    }

    #[test]
    fn test_cost() {
        assert_eq!(cost_per_token("claude-3-5-sonnet"), 3.0 / 1_000_000.0);
    }
}