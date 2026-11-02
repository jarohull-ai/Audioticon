use sha2::{Digest, Sha256};
use crate::types::TextSegment;

/// Computes a unique SHA-256 fingerprint for a text segment.
/// Used for granular caching: if text and parameters haven't changed,
/// we reuse the cached audio file directly from SSD.
pub fn compute_segment_hash(segment: &TextSegment, voice: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(segment.text.trim().as_bytes());
    hasher.update(&segment.rate_mod.to_le_bytes());
    hasher.update(&segment.pitch_mod.to_le_bytes());
    hasher.update(&segment.volume_mod.to_le_bytes());
    if let Some(f) = &segment.filter {
        hasher.update(f.as_bytes());
    }
    hasher.update(voice.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_consistency() {
        let seg1 = TextSegment {
            text: "Na biurku leżał notes.".to_string(),
            rate_mod: -6,
            pitch_mod: -4,
            volume_mod: 0,
            filter: None,
        };
        let seg2 = seg1.clone();
        assert_eq!(compute_segment_hash(&seg1, "pl-PL-MarekNeural"), compute_segment_hash(&seg2, "pl-PL-MarekNeural"));
    }

    #[test]
    fn test_hash_sensitivity() {
        let seg1 = TextSegment {
            text: "Na biurku leżał notes.".to_string(),
            rate_mod: -6,
            pitch_mod: -4,
            volume_mod: 0,
            filter: None,
        };
        let mut seg2 = seg1.clone();
        seg2.rate_mod = -14;
        assert_ne!(compute_segment_hash(&seg1, "pl-PL-MarekNeural"), compute_segment_hash(&seg2, "pl-PL-MarekNeural"));
    }
}
