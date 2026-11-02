use crate::audio::build_ffmpeg_filter_chain;
use crate::types::TextSegment;
use sha2::{Digest, Sha256};

pub const CACHE_VERSION: u32 = 4;

/// Computes a unique SHA-256 fingerprint for a text segment.
/// Incorporates expressive actor tags, acoustic filter chain, and CACHE_VERSION so
/// changing DSP presets or actor delivery tags automatically invalidates outdated cache.
pub fn compute_segment_hash(segment: &TextSegment, voice: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CACHE_VERSION.to_le_bytes());
    hasher.update(segment.text.trim().as_bytes());
    hasher.update(segment.rate_mod.to_le_bytes());
    hasher.update(segment.pitch_mod.to_le_bytes());
    hasher.update(segment.volume_mod.to_le_bytes());

    // Hash sorted actor tags
    let mut tags = segment.actor_tags.clone();
    tags.sort();
    for tag in tags {
        hasher.update(format!("{:?}", tag).as_bytes());
    }

    // Hash the exact FFmpeg filter chain rather than just the preset name
    let filter_chain = build_ffmpeg_filter_chain(segment.filter.as_deref());
    hasher.update(filter_chain.as_bytes());

    hasher.update(voice.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ActorTag;

    #[test]
    fn test_hash_consistency() {
        let seg1 = TextSegment {
            text: "Na biurku leżał notes.".to_string(),
            rate_mod: -6,
            pitch_mod: -4,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![ActorTag::Whisper],
        };
        let seg2 = seg1.clone();
        assert_eq!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg2, "pl-PL-MarekNeural")
        );
    }

    #[test]
    fn test_hash_sensitivity() {
        let seg1 = TextSegment {
            text: "Na biurku leżał notes.".to_string(),
            rate_mod: -6,
            pitch_mod: -4,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
        };
        let mut seg2 = seg1.clone();
        seg2.actor_tags.push(ActorTag::Whisper);

        assert_ne!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg2, "pl-PL-MarekNeural")
        );

        let mut seg3 = seg1.clone();
        seg3.rate_mod = -14;
        assert_ne!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg3, "pl-PL-MarekNeural")
        );

        let mut seg4 = seg1.clone();
        seg4.filter = Some("cathedral".to_string());
        assert_ne!(
            compute_segment_hash(&seg1, "pl-PL-MarekNeural"),
            compute_segment_hash(&seg4, "pl-PL-MarekNeural")
        );
    }
}
