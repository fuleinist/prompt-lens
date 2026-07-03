//! # Tokenizer — cl100k_base token counting
//!
//! Two backends:
//! - **tiktoken-rs** (feature `tiktoken`): calls the real OpenAI BPE tokenizer
//!   via Rust bindings. Accurate counts matching API billing.
//! - **Heuristic** (fallback): word-level estimation calibrated to cl100k_base.
//!   Zero dependencies, fast, but approximate.
//!
//! The heuristic uses these rules:
//! - Very short words (1-2 chars): often merged with next token → 1 token each
//! - Short words (3-4 chars): 1-2 tokens typically
//! - Medium words (5-8 chars): 2-3 tokens (may split at common subword boundaries)
//! - Long words (9+ chars): 3+ tokens (multiple splits in BPE)
//! - Numbers: variable, ~1-2 tokens per 3-4 digits
//! - Punctuation: 1 token each
//! - Whitespace: merged, no token cost

#[cfg(feature = "tiktoken")]
use std::sync::OnceLock;

/// Returns `true` when the active tokenizer is the real tiktoken BPE
/// rather than the heuristic approximation.
pub fn using_real_tokenizer() -> bool {
    #[cfg(feature = "tiktoken")]
    {
        true
    }
    #[cfg(not(feature = "tiktoken"))]
    {
        false
    }
}

/// Map prompt-lens model names to tiktoken-rs model names.
/// Returns `None` when the model is not in tiktoken's registry.
#[cfg(feature = "tiktoken")]
fn tiktoken_model_name(model: &str) -> Option<&'static str> {
    match model {
        "gpt-4o" => Some("gpt-4o"),
        "gpt-4o-mini" => Some("gpt-4o-mini"),
        "gpt-4-turbo" => Some("gpt-4-turbo"),
        "gpt-4" => Some("gpt-4"),
        "gpt-3.5-turbo" => Some("gpt-3.5-turbo"),
        // claude models → fallback
        _ => None,
    }
}

/// Thread-local (via OnceLock) cached tiktoken tokenizer instance.
#[cfg(feature = "tiktoken")]
fn get_tiktoken(model: &str) -> Option<&'static tiktoken_rs::CoreBPE> {
    static CACHE: OnceLock<tiktoken_rs::CoreBPE> = OnceLock::new();
    let tk_model = tiktoken_model_name(model)?;
    let bpe = CACHE.get_or_init(|| {
        tiktoken_rs::get_bpe_from_model(tk_model)
            .expect("tiktoken-rs should provide a valid BPE for known models")
    });
    Some(bpe)
}

/// Count tokens using the best available backend.
///
/// When the `tiktoken` feature is enabled and the model is in tiktoken's
/// registry, uses the real BPE tokenizer. Otherwise falls back to the
/// heuristic approximation.
pub fn count_tokens(text: &str, model: &str) -> usize {
    #[cfg(feature = "tiktoken")]
    {
        if let Some(bpe) = get_tiktoken(model) {
            return bpe.encode_with_special_tokens(text).len();
        }
    }
    let _ = model;
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

/// Catalog entry for one supported model. Cost is input USD per 1M tokens
/// (the only side prompt-lens can quote — output tokens are billed by the
/// provider but are not counted here). The `aliases` list lets older or
/// fully-qualified names resolve to the same entry, matching the previous
/// `cost_per_token` / `context_limit` behavior (e.g. `claude-3-5-sonnet`
/// and `claude-3-5-sonnet-2024-05-20` share pricing).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub name: &'static str,
    #[serde(skip_serializing)]
    pub aliases: &'static [&'static str],
    pub cost_per_million: f64,
    pub context_limit: usize,
}

const MODELS: &[ModelInfo] = &[
    ModelInfo { name: "claude-3-5-sonnet",  aliases: &["claude-3-5-sonnet-2024-05-20"], cost_per_million: 3.00,  context_limit: 200_000 },
    ModelInfo { name: "claude-3-opus",     aliases: &[],                                cost_per_million: 15.00, context_limit: 200_000 },
    ModelInfo { name: "claude-3-sonnet",   aliases: &[],                                cost_per_million: 3.00,  context_limit: 200_000 },
    ModelInfo { name: "claude-3-haiku",    aliases: &[],                                cost_per_million: 0.25,  context_limit: 200_000 },
    ModelInfo { name: "gpt-4o",            aliases: &[],                                cost_per_million: 2.50,  context_limit: 128_000 },
    ModelInfo { name: "gpt-4o-mini",       aliases: &[],                                cost_per_million: 0.15,  context_limit: 128_000 },
    ModelInfo { name: "gpt-4-turbo",       aliases: &["gpt-4"],                          cost_per_million: 10.00, context_limit: 128_000 },
    ModelInfo { name: "gpt-3.5-turbo",     aliases: &[],                                cost_per_million: 0.50,  context_limit:  16_385 },
];

/// Look up a model by primary name or alias. Returns the canonical entry
/// so callers see the primary `name`, not the alias they typed.
fn lookup_model(model: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.name == model || m.aliases.contains(&model))
}

/// The built-in fallback used when a model name is not in the catalog.
/// Matches the previous `_ => 3.0 / 1_000_000.0` default in
/// `cost_per_token` and `_ => 200_000` in `context_limit`.
const DEFAULT_MODEL: &str = "claude-3-5-sonnet";

/// Calculate cost per token for a given model.
pub fn cost_per_token(model: &str) -> f64 {
    let entry = lookup_model(model).unwrap_or_else(|| {
        // Safe: DEFAULT_MODEL is in the catalog.
        lookup_model(DEFAULT_MODEL).expect("default model must be in MODELS")
    });
    entry.cost_per_million / 1_000_000.0
}

/// Context window limits by model.
pub fn context_limit(model: &str) -> usize {
    let entry = lookup_model(model).unwrap_or_else(|| {
        lookup_model(DEFAULT_MODEL).expect("default model must be in MODELS")
    });
    entry.context_limit
}

/// Return the catalog of supported models for the `models` subcommand and
/// any future programmatic consumers. Order matches the `MODELS` table so
/// the rendered table is stable across runs.
pub fn list_models() -> Vec<ModelInfo> {
    MODELS.to_vec()
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

    #[test]
    fn test_cost_per_token_matches_previous_constants() {
        // The catalog refactor must not change the numbers anyone saw
        // before — pin the per-token cost for every catalog entry so a
        // future edit to MODELS can't silently move the decimal.
        assert_eq!(cost_per_token("claude-3-5-sonnet"), 3.0 / 1_000_000.0);
        assert_eq!(cost_per_token("claude-3-5-sonnet-2024-05-20"), 3.0 / 1_000_000.0);
        assert_eq!(cost_per_token("gpt-4o"), 2.5 / 1_000_000.0);
        assert_eq!(cost_per_token("gpt-4o-mini"), 0.15 / 1_000_000.0);
        assert_eq!(cost_per_token("gpt-4-turbo"), 10.0 / 1_000_000.0);
        assert_eq!(cost_per_token("gpt-4"), 10.0 / 1_000_000.0);
        assert_eq!(cost_per_token("gpt-3.5-turbo"), 0.5 / 1_000_000.0);
        // Unknown model still falls back to Claude Sonnet pricing, same
        // as the old `_ => 3.0 / 1_000_000.0` arm.
        assert_eq!(cost_per_token("totally-unknown-model"), 3.0 / 1_000_000.0);
    }

    #[test]
    fn test_context_limit_matches_previous_constants() {
        // Same shape as the cost test: every catalog entry plus the
        // unknown-model fallback must report the same context limit it
        // did before the catalog refactor.
        assert_eq!(context_limit("claude-3-5-sonnet"), 200_000);
        assert_eq!(context_limit("claude-3-opus"), 200_000);
        assert_eq!(context_limit("claude-3-sonnet"), 200_000);
        assert_eq!(context_limit("claude-3-haiku"), 200_000);
        assert_eq!(context_limit("gpt-4o"), 128_000);
        assert_eq!(context_limit("gpt-4-turbo"), 128_000);
        assert_eq!(context_limit("gpt-4"), 128_000);
        assert_eq!(context_limit("gpt-3.5-turbo"), 16_385);
        assert_eq!(context_limit("totally-unknown-model"), 200_000);
    }

    #[test]
    fn test_list_models_returns_catalog_in_order() {
        // `models` subcommand prints the catalog in this order, so
        // downstream tooling that diffs the output stays stable.
        let catalog = list_models();
        let names: Vec<&str> = catalog.iter().map(|m| m.name).collect();
        assert_eq!(
            names,
            vec![
                "claude-3-5-sonnet",
                "claude-3-opus",
                "claude-3-sonnet",
                "claude-3-haiku",
                "gpt-4o",
                "gpt-4o-mini",
                "gpt-4-turbo",
                "gpt-3.5-turbo",
            ]
        );
    }

    #[test]
    fn test_list_models_includes_required_fields() {
        // The subcommand prints cost and context; every entry must have
        // both, otherwise the renderer would print "—" or panic.
        for m in list_models() {
            assert!(m.cost_per_million > 0.0, "{} missing cost", m.name);
            assert!(m.context_limit > 0, "{} missing context_limit", m.name);
        }
    }
}