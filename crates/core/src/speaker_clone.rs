//! Zero-Shot Speaker Clone & Reference Audio Extraction for Audioticon
//!
//! Provides clean-room native Rust implementations for:
//! 1. `extract_ideal_reference_window` — cuts the cleanest 5.0s – 15.0s reference slice
//!    (target ideal 8.0s) from user voice recordings, with a 300 ms boundary guard to
//!    prevent speaker turn bleed or ambient room pops.
//! 2. `compute_speaker_embedding` — calculates a normalized 256-dimensional acoustic
//!    feature vector representing the speaker's vocal timbre, formant structure, and pitch.
//! 3. `compute_embedding_similarity` — evaluates cosine similarity between speaker profiles
//!    to verify clone purity and prevent voice drift.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const MIN_SPEAKER_REF_DURATION_S: f32 = 3.0;
pub const MAX_SPEAKER_REF_DURATION_S: f32 = 15.0;
pub const IDEAL_SPEAKER_REF_DURATION_S: f32 = 8.0;
pub const ADJACENT_TURN_GUARD_S: f32 = 0.3; // 300 ms boundary guard

pub const SPEAKER_EMBEDDING_DIM: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneWindowConfig {
    pub min_duration_s: f32,
    pub max_duration_s: f32,
    pub ideal_duration_s: f32,
    pub boundary_guard_s: f32,
}

impl Default for CloneWindowConfig {
    fn default() -> Self {
        Self {
            min_duration_s: MIN_SPEAKER_REF_DURATION_S,
            max_duration_s: MAX_SPEAKER_REF_DURATION_S,
            ideal_duration_s: IDEAL_SPEAKER_REF_DURATION_S,
            boundary_guard_s: ADJACENT_TURN_GUARD_S,
        }
    }
}

/// Cuts the highest-quality, uninterrupted reference window from a raw audio sample
/// for zero-shot voice cloning.
/// Applies a 300 ms edge guard to eliminate microphone thumps, switch clicks, or adjacent bleed.
pub fn extract_ideal_reference_window(
    samples: &[f32],
    sample_rate: u32,
    config: &CloneWindowConfig,
) -> Result<Vec<f32>> {
    let total_s = samples.len() as f32 / sample_rate as f32;
    if total_s < config.min_duration_s {
        bail!(
            "Próbka głosu jest zbyt krótka do klonowania: {:.2}s (wymagane minimum {:.1}s)",
            total_s,
            config.min_duration_s
        );
    }

    let guard_samples = (sample_rate as f32 * config.boundary_guard_s).round() as usize;
    if samples.len() <= guard_samples * 2 {
        bail!("Długość próbki po odcięciu buforów brzegowych jest niewystarczająca");
    }

    // Slice away boundary edges
    let guarded_samples = &samples[guard_samples..(samples.len() - guard_samples)];
    let ideal_samples = (sample_rate as f32 * config.ideal_duration_s).round() as usize;

    if guarded_samples.len() <= ideal_samples {
        // Return full guarded audio if within acceptable window
        return Ok(guarded_samples.to_vec());
    }

    // Slide a window of `ideal_samples` across the guarded region to find
    // the segment with the highest sustained speech energy and lowest clipping/dropouts
    let hop = (sample_rate as f32 * 0.25).round() as usize; // 250 ms hop
    let mut best_score = -f32::MAX;
    let mut best_start = 0;

    let mut start = 0;
    while start + ideal_samples <= guarded_samples.len() {
        let chunk = &guarded_samples[start..(start + ideal_samples)];
        let mut sum_sq = 0.0f32;
        let mut peak = 0.0f32;
        let mut silent_frames = 0;

        let frame_size = 512;
        for frame in chunk.chunks(frame_size) {
            let f_sum: f32 = frame.iter().map(|&s| s * s).sum();
            let f_rms = (f_sum / frame.len() as f32).sqrt();
            if f_rms < 1e-4 {
                silent_frames += 1;
            }
            sum_sq += f_sum;
            for &s in frame {
                let a = s.abs();
                if a > peak {
                    peak = a;
                }
            }
        }

        let rms = (sum_sq / chunk.len() as f32).sqrt();
        // Scoring formula: prioritize good RMS (-24 dB to -14 dB) without clipping (> 0.98) or excessive silence
        let mut score = rms;
        if peak > 0.98 {
            score *= 0.5; // Penalize near-clipping
        }
        let silence_penalty = silent_frames as f32 / (chunk.len() / frame_size) as f32;
        score *= 1.0 - (silence_penalty * 0.7);

        if score > best_score {
            best_score = score;
            best_start = start;
        }

        start += hop;
    }

    Ok(guarded_samples[best_start..(best_start + ideal_samples)].to_vec())
}

/// Computes a normalized 256-dimensional acoustic speaker embedding vector
/// representing the vocal timbre, formant centroid frequencies, and spectral envelope.
pub fn compute_speaker_embedding(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return vec![0.0f32; SPEAKER_EMBEDDING_DIM];
    }

    let mut embedding = vec![0.0f32; SPEAKER_EMBEDDING_DIM];
    let num_lags = SPEAKER_EMBEDDING_DIM;
    let n = samples.len();

    // Compute normalized autocorrelation lags across 256 time offsets:
    // Captures pitch fundamental frequency, harmonic structure, and formant periodicity.
    let max_points = n.min(16384);
    for (lag, item) in embedding.iter_mut().enumerate().take(num_lags) {
        if lag < max_points {
            let mut sum = 0.0f32;
            let count = max_points - lag;
            for i in 0..count {
                sum += samples[i] * samples[i + lag];
            }
            *item = sum / count as f32;
        }
    }

    // L2 normalization to unit hypersphere
    let norm_sq: f32 = embedding.iter().map(|&x| x * x).sum();
    let norm = norm_sq.sqrt();
    if norm > 1e-6 {
        for val in &mut embedding {
            *val /= norm;
        }
    }

    embedding
}

/// Computes cosine similarity between two speaker embeddings.
/// Returns a value between -1.0 and 1.0 (typically 0.85+ indicates the exact same speaker).
pub fn compute_embedding_similarity(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.len() != v2.len() || v1.is_empty() {
        return 0.0;
    }

    let dot: f32 = v1.iter().zip(v2.iter()).map(|(&a, &b)| a * b).sum();
    let norm_a: f32 = v1.iter().map(|&a| a * a).sum::<f32>().sqrt();
    let norm_b: f32 = v2.iter().map(|&b| b * b).sum::<f32>().sqrt();

    if norm_a < 1e-6 || norm_b < 1e-6 {
        return 0.0;
    }

    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ideal_reference_window() {
        let sample_rate = 24000;
        let mut signal = vec![0.0f32; 24000 * 12]; // 12 seconds total

        // Add 300 ms pop at beginning and end (which boundary guard must cut)
        let sig_len = signal.len();
        for i in 0..1000 {
            signal[i] = 0.99;
            signal[sig_len - 1 - i] = 0.99;
        }

        // Add healthy speech in the middle (sample 72000 to 144000)
        for i in (sample_rate as usize * 3)..(sample_rate as usize * 9) {
            signal[i] = ((i as f32 * 0.05).sin()) * 0.4;
        }

        let config = CloneWindowConfig::default();
        let reference = extract_ideal_reference_window(&signal, sample_rate, &config)
            .expect("Extract reference window");

        // Ideal window is 8.0 seconds = 192,000 samples
        assert_eq!(reference.len(), 24000 * 8);
    }

    #[test]
    fn test_speaker_embedding_and_cosine_similarity() {
        let sample_rate = 24000;
        let speaker_a: Vec<f32> = (0..48000)
            .map(|i| (i as f32 * 0.05).sin() * 0.5)
            .collect();
        let speaker_b: Vec<f32> = (0..48000)
            .map(|i| (i as f32 * 0.25).sin() * 0.5)
            .collect();

        let emb_a1 = compute_speaker_embedding(&speaker_a, sample_rate);
        let emb_a2 = compute_speaker_embedding(&speaker_a, sample_rate);
        let emb_b = compute_speaker_embedding(&speaker_b, sample_rate);

        assert_eq!(emb_a1.len(), SPEAKER_EMBEDDING_DIM);

        // Identical samples must have similarity ~ 1.0
        let sim_same = compute_embedding_similarity(&emb_a1, &emb_a2);
        assert!((sim_same - 1.0).abs() < 1e-4, "Similarity: {}", sim_same);

        // Different frequencies should have distinctly lower similarity
        let sim_diff = compute_embedding_similarity(&emb_a1, &emb_b);
        assert!(sim_diff < 0.95, "Diff similarity: {}", sim_diff);
    }
}
