//! Waveform Similarity Overlap-Add (WSOLA) Time-Stretching DSP for Audioticon
//!
//! Provides clean-room native Rust pitch-preserving time-scale modification (0.5x to 2.0x).
//! Used when directing speech pacing with `🐢` (slower) or `🐇` (faster) emojis,
//! without changing the speaker's vocal pitch (no "chipmunk" or "demon" pitch shift).

/// Applies WSOLA time-stretching to a mono audio signal.
/// - `speed_ratio`: e.g. 0.85 for 15% slower, 1.25 for 25% faster. Clamped to [0.5, 2.0].
/// - `sample_rate`: typically 22050, 24000, 44100, or 48000 Hz.
pub fn wsola_time_stretch(samples: &[f32], sample_rate: u32, speed_ratio: f32) -> Vec<f32> {
    let speed = speed_ratio.clamp(0.5, 2.0);
    if samples.is_empty() || (speed - 1.0).abs() < 0.01 {
        return samples.to_vec();
    }

    // Typical pitch search window for human speech (50 Hz - 500 Hz pitch)
    let win_size = ((sample_rate as f32 * 0.025).round() as usize).max(64); // 25ms window
    let hop_synth = win_size / 2; // 50% overlap
    let hop_analysis = ((hop_synth as f32) * speed).round().max(1.0) as usize;
    let max_delta = win_size / 2;

    let target_len = ((samples.len() as f32) / speed).round() as usize;
    let mut output = vec![0.0f32; target_len + win_size * 2];
    let mut weight = vec![0.0f32; target_len + win_size * 2];

    // Precalculate Hanning window
    let mut window = Vec::with_capacity(win_size);
    for i in 0..win_size {
        let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (win_size - 1) as f32).cos());
        window.push(w);
    }

    let mut synth_pos = 0;
    let mut anal_pos = 0;

    while anal_pos + win_size + max_delta < samples.len() && synth_pos + win_size < output.len() {
        // Find best alignment delta via normalized cross-correlation
        let mut best_delta = 0isize;
        let mut best_corr = -f32::MAX;

        let ref_start = synth_pos;

        for delta in -(max_delta as isize)..=(max_delta as isize) {
            let candidate_start = (anal_pos as isize + delta).max(0) as usize;
            if candidate_start + win_size > samples.len() {
                continue;
            }

            let mut corr = 0.0f32;
            let mut energy = 0.0f32;

            for k in 0..win_size {
                let s_ref = output[ref_start + k];
                let s_cand = samples[candidate_start + k];
                corr += s_ref * s_cand;
                energy += s_cand * s_cand;
            }

            let norm_corr = if energy > 1e-6 {
                corr / energy.sqrt()
            } else {
                corr
            };

            if norm_corr > best_corr {
                best_corr = norm_corr;
                best_delta = delta;
            }
        }

        let best_start = (anal_pos as isize + best_delta).max(0) as usize;

        // Overlap-add windowed segment
        for k in 0..win_size {
            if synth_pos + k < output.len() && best_start + k < samples.len() {
                let w = window[k];
                output[synth_pos + k] += samples[best_start + k] * w;
                weight[synth_pos + k] += w;
            }
        }

        synth_pos += hop_synth;
        anal_pos += hop_analysis;
    }

    // Normalize overlapping weights
    let final_len = synth_pos.min(output.len());
    let mut result = Vec::with_capacity(final_len);

    for i in 0..final_len {
        let w = weight[i];
        if w > 1e-4 {
            result.push(output[i] / w);
        } else {
            result.push(output[i]);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wsola_speedup_and_slowdown_length() {
        let sample_rate = 24000;
        let input = vec![0.2f32; 24000]; // 1.0 second

        // Speed up to 1.5x -> duration should decrease to ~0.67s
        let fast = wsola_time_stretch(&input, sample_rate, 1.5);
        let ratio_fast = fast.len() as f32 / input.len() as f32;
        assert!(
            (ratio_fast - 0.67).abs() < 0.15,
            "Fast ratio: {}",
            ratio_fast
        );

        // Slow down to 0.75x -> duration should increase to ~1.33s
        let slow = wsola_time_stretch(&input, sample_rate, 0.75);
        let ratio_slow = slow.len() as f32 / input.len() as f32;
        assert!(
            (ratio_slow - 1.33).abs() < 0.2,
            "Slow ratio: {}",
            ratio_slow
        );
    }
}
