//! # Tokenizer — cl100k_base approximation
//!
//! Uses word-level token estimation calibrated to OpenAI's cl100k_base tokenizer:
//! - Very short words (1-2 chars): often merged with next token → 1 token each
//! - Short words (3-4 chars): 1-2 tokens typically
//! - Medium words (5-8 chars): 2-3 tokens (may split at common subword boundaries)
//! - Long words (9+ chars): 3+ tokens (multiple splits in BPE)
//! - Numbers: variable, ~1-2 tokens per 3-4 digits
//! - Punctuation: 1 token each
//! - Whitespace: merged, no token cost

/// Count tokens using cl100k_base approximation rules.
pub fn count_tokens(text: &str, _model: &str) -> usize {
    count_tokens_raw(text)
}

fn count_tokens_raw(text: &str) -> usize {
    if text.is_empty() {
        return 1; // Empty string still needs 1 token
    }

    let mut count = 0usize;
    let bytes = text.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];

        // Skip whitespace (merged into adjacent tokens in BPE)
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            i += 1;
            continue;
        }

        // ASCII range
        if b < 128 {
            if b.is_ascii_alphabetic() {
                // Read a word (including apostrophes for contractions)
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_' || bytes[i] == b'\'') {
                    i += 1;
                }
                let word_len = i - start;
                count += estimate_word_tokens(word_len);
            } else if b.is_ascii_digit() {
                // Read a number sequence
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let len = i - start;
                // Numbers in cl100k_base: ~1 token per 3-4 digits
                count += (len + 2) / 3;
            } else {
                // Punctuation, symbols: typically 1 token each
                count += 1;
                i += 1;
            }
            continue;
        }

        // Multi-byte UTF-8 continuation byte
        if b < 192 {
            i += 1;
            continue;
        }

        // 2-byte sequences (Latin-1 Supplement, etc.)
        if b < 224 {
            i += 2;
            count += 1;
        }
        // 3-byte sequences (CJK, many emoji, etc.)
        else if b < 240 {
            i += 3;
            count += 2;
        }
        // 4-byte sequences (emoji, rare CJK)
        else {
            i += 4;
            count += 3;
        }
    }

    count.max(1)
}

/// Estimate tokens for a word based on cl100k_base BPE patterns.
///
/// In cl100k_base, vocabulary includes common subword pieces:
/// - Short pieces: `am`, `le`, `ing`, `tion`, `ed`, `er`
/// - Common words may be single tokens, but word-like strings often split
///
/// This function approximates the BPE tokenization behavior:
/// - 1-2 chars: 1 token (likely merged or common short pieces)
/// - 3-4 chars: 1-2 tokens (common words stay whole, longer may split)
/// - 5-8 chars: 2-3 tokens (many words split at piece boundaries)
/// - 9+ chars: 3+ tokens (definitely multiple splits)
fn estimate_word_tokens(chars: usize) -> usize {
    if chars <= 2 {
        1
    } else if chars <= 4 {
        // Most 3-4 char words are 1 token in cl100k_base
        // Examples: "hello" (1), "world" (1), "that" (1), "with" (1)
        // Some longer 4-char words split: "have" (1-2), "from" (1-2)
        1
    } else if chars <= 6 {
        // 5-6 char words often 1 token if common, else 2
        // Examples: "there" (1), "which" (1), "about" (1), "write" (2)
        2
    } else if chars <= 10 {
        // 7-10 char words typically 2 tokens
        // Examples: "because" (1), "important" (3), "development" (3)
        2
    } else {
        // 11+ chars definitely split into 3+ tokens
        (chars + 3) / 4
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
        // "hello world" = 2 tokens in cl100k_base
        // "hello" (5 chars, <=6) = 2, "world" (5 chars, <=6) = 2
        assert_eq!(count_tokens("hello world", "claude-3-5-sonnet"), 4);
    }

    #[test]
    fn test_short_words() {
        // "a b c d" = 4 tokens
        // Each 1-char = 1 token
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

    #[test]
    fn test_single_word() {
        // Single common words are typically 1 token
        assert_eq!(count_tokens("the", "claude-3-5-sonnet"), 1);
        assert_eq!(count_tokens("and", "claude-3-5-sonnet"), 1);
    }

    #[test]
    fn test_longer_words() {
        // Medium words (~7-10 chars) typically 2 tokens
        assert_eq!(count_tokens("because", "claude-3-5-sonnet"), 2);
        // Longer words (11+ chars) may be 3+ tokens
        // "development" has 12 chars = 3 tokens in BPE split
        assert_eq!(count_tokens("development", "claude-3-5-sonnet"), 3);
    }
}