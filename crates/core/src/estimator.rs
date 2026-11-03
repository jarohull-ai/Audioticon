use crate::cache::compute_segment_hash;
use crate::parser::EmojiDirector;
use crate::types::DirectedItem;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostEstimate {
    pub provider: String,
    pub total_speech_segments: usize,
    pub cached_segments: usize,
    pub segments_to_synthesize: usize,
    pub total_characters: usize,
    pub cached_characters: usize,
    pub characters_to_synthesize: usize,
    pub estimated_cost_usd: f64,
    pub cache_hit_ratio_percent: f64,
}

/// Estimates character counts and cost for synthesizing a given script with specified TTS provider
pub fn estimate_cost(
    script_text: &str,
    provider: &str,
    voice: &str,
    cache_dir: &Path,
) -> CostEstimate {
    let director = EmojiDirector::default();
    let script = director.parse(script_text);

    let clean_provider = provider.trim().to_lowercase();
    let effective_voice = match clean_provider.as_str() {
        "piper" => {
            if voice.starts_with("piper-") {
                voice.to_string()
            } else {
                format!("piper-{}", voice)
            }
        }
        "elevenlabs" => {
            if voice.starts_with("elevenlabs-") {
                voice.to_string()
            } else {
                format!("elevenlabs-{}", voice)
            }
        }
        _ => voice.to_string(),
    };

    let mut total_speech_segments: usize = 0;
    let mut cached_segments: usize = 0;
    let mut total_characters: usize = 0;
    let mut cached_characters: usize = 0;

    for item in &script.items {
        if let DirectedItem::Speech(segment) = item {
            if segment.text.trim().is_empty() {
                continue;
            }
            let char_count = segment.text.chars().count();
            total_speech_segments += 1;
            total_characters += char_count;

            let voice_for_seg = segment.voice.as_deref().unwrap_or(&effective_voice);
            let hash = compute_segment_hash(segment, voice_for_seg);
            let wav_path = cache_dir.join(format!("{}.wav", hash));

            if wav_path.exists() {
                cached_segments += 1;
                cached_characters += char_count;
            }
        }
    }

    let segments_to_synthesize = total_speech_segments.saturating_sub(cached_segments);
    let characters_to_synthesize = total_characters.saturating_sub(cached_characters);

    // Cost rates in USD per character
    let rate_per_char = match clean_provider.as_str() {
        "elevenlabs" => 0.00030, // ~$0.30 per 1,000 chars (standard tier)
        "openai" => 0.000015,    // ~$0.015 per 1,000 chars
        "azure" => 0.000016,     // ~$0.016 per 1,000 chars
        _ => 0.0,                // Piper and Edge are free
    };

    let estimated_cost_usd = (characters_to_synthesize as f64) * rate_per_char;
    let cache_hit_ratio_percent = if total_characters > 0 {
        ((cached_characters as f64) / (total_characters as f64)) * 100.0
    } else {
        0.0
    };

    CostEstimate {
        provider: clean_provider,
        total_speech_segments,
        cached_segments,
        segments_to_synthesize,
        total_characters,
        cached_characters,
        characters_to_synthesize,
        estimated_cost_usd,
        cache_hit_ratio_percent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_cost_free_provider() {
        let tmp = std::env::temp_dir();
        let est = estimate_cost(
            "📖 Rozdział 1 🛑 Pierwsze zdanie. 🛑 Drugie zdanie.",
            "piper",
            "pl_PL-darkman-medium",
            &tmp,
        );
        // "📖 Rozdział 1" (lektor speech announcement) + 2 sentences = 3 speech segments
        assert_eq!(est.total_speech_segments, 3);
        assert_eq!(est.estimated_cost_usd, 0.0);
        assert!(est.total_characters > 20);
    }

    #[test]
    fn test_estimate_cost_elevenlabs() {
        let tmp = std::env::temp_dir().join("non_existent_cache_test");
        let text = "To jest tekst testowy.";
        let est = estimate_cost(text, "elevenlabs", "adam", &tmp);
        assert_eq!(est.total_speech_segments, 1);
        assert_eq!(est.cached_segments, 0);
        assert!(est.estimated_cost_usd > 0.0);
    }
}
