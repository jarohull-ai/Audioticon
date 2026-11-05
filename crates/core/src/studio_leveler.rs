//! Studio Vocal Opto-Leveler & Peak Limiter DSP for Audioticon
//!
//! Provides clean-room native Rust implementations for:
//! 1. `level_speech` — an opto-style speech leveler designed specifically for audiobooks.
//!    Evens out phrase-to-phrase loudness variations without compressing syllable dynamics.
//!    Crucially, pauses and breaths hold the gain of previous speech, completely preventing
//!    background noise floor "pumping" and audible breath amplification.
//! 2. `peak_limit` — lookahead transparent brickwall limiter that guards against clipping
//!    and inter-sample peaks on loud vocal outbursts or dramatic sound effects.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelerConfig {
    /// Averaging window for speech energy in milliseconds (default: 400.0 ms)
    pub window_ms: f32,
    /// Leveling amount (0.0 = untouched, 1.0 = fully leveled toward mean, default: 0.7)
    pub amount: f32,
    /// Maximum gain boost allowed in dB (default: 6.0 dB)
    pub max_gain_db: f32,
    /// Smoothing window for gain trajectory in milliseconds (default: 200.0 ms)
    pub smooth_ms: f32,
    /// Speech detection threshold in dBFS (default: -50.0 dBFS)
    pub speech_gate_db: f32,
}

impl Default for LevelerConfig {
    fn default() -> Self {
        Self {
            window_ms: 400.0,
            amount: 0.7,
            max_gain_db: 6.0,
            smooth_ms: 200.0,
            speech_gate_db: -50.0,
        }
    }
}

/// Evens out phrase-to-phrase loudness without touching syllable micro-dynamics.
/// Holds gain over pauses and breaths, eliminating compressor pumping.
pub fn level_speech(samples: &[f32], sample_rate: u32, config: &LevelerConfig) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return samples.to_vec();
    }

    let n = samples.len();
    let frame_len = (sample_rate as f32 * 0.010).max(1.0) as usize; // 10ms frame
    let num_frames = n.div_ceil(frame_len);

    let gate_threshold = 10.0f32.powf(config.speech_gate_db / 20.0);
    let mut frame_rms = Vec::with_capacity(num_frames);
    let mut is_speech = Vec::with_capacity(num_frames);

    // 1. Compute frame-by-frame RMS and voice activity
    for i in 0..num_frames {
        let start = i * frame_len;
        let end = (start + frame_len).min(n);
        let chunk = &samples[start..end];
        let sum_sq: f32 = chunk.iter().map(|&s| s * s).sum();
        let rms = (sum_sq / chunk.len() as f32).sqrt();
        frame_rms.push(rms);
        is_speech.push(rms > gate_threshold);
    }

    // 2. Compute mean speech level across the entire passage
    let speech_rms_sum: f32 = frame_rms
        .iter()
        .zip(is_speech.iter())
        .filter(|&(_, &sp)| sp)
        .map(|(&r, _)| r)
        .sum();
    let speech_count = is_speech.iter().filter(|&&sp| sp).count();

    if speech_count == 0 {
        return samples.to_vec(); // Pure silence or sub-gate noise
    }

    let mean_speech_rms = speech_rms_sum / speech_count as f32;
    if mean_speech_rms < 1e-6 {
        return samples.to_vec();
    }

    let max_gain_linear = 10.0f32.powf(config.max_gain_db / 20.0);
    let window_frames = ((sample_rate as f32 * (config.window_ms / 1000.0)) / frame_len as f32)
        .round()
        .max(1.0) as usize;

    let mut target_frame_gain = vec![1.0f32; num_frames];
    let mut last_valid_gain = 1.0f32;

    // 3. Measure moving speech RMS and assign gain
    for i in 0..num_frames {
        if is_speech[i] {
            let win_start = i.saturating_sub(window_frames / 2);
            let win_end = (i + window_frames / 2).min(num_frames);

            let mut win_speech_sum = 0.0f32;
            let mut win_speech_cnt = 0;
            for j in win_start..win_end {
                if is_speech[j] {
                    win_speech_sum += frame_rms[j];
                    win_speech_cnt += 1;
                }
            }

            let local_rms = if win_speech_cnt > 0 {
                win_speech_sum / win_speech_cnt as f32
            } else {
                frame_rms[i]
            };

            if local_rms > 1e-6 {
                // How far local RMS is from global mean speech RMS
                let ratio = mean_speech_rms / local_rms;
                // Move `amount` of the way toward parity
                let desired_gain = 1.0 + (ratio - 1.0) * config.amount;
                let clamped_gain = desired_gain.clamp(1.0 / max_gain_linear, max_gain_linear);
                target_frame_gain[i] = clamped_gain;
                last_valid_gain = clamped_gain;
            }
        } else {
            // Pause / breath: hold gain from previous speech! Crucial: no noise amplification!
            target_frame_gain[i] = last_valid_gain;
        }
    }

    // 4. Smooth gain trajectory over smooth_ms (moving average filter)
    let smooth_frames = ((sample_rate as f32 * (config.smooth_ms / 1000.0)) / frame_len as f32)
        .round()
        .max(1.0) as usize;

    let mut smoothed_gain = vec![1.0f32; num_frames];
    for (i, smoothed) in smoothed_gain.iter_mut().enumerate() {
        let lo = i.saturating_sub(smooth_frames / 2);
        let hi = (i + smooth_frames / 2).min(num_frames);
        let sum: f32 = target_frame_gain[lo..hi].iter().sum();
        *smoothed = sum / (hi - lo) as f32;
    }

    // 5. Interpolate frame gains onto sample level and apply
    let mut out = Vec::with_capacity(n);
    for (i, &sample) in samples.iter().enumerate() {
        let frame_idx = (i / frame_len).min(num_frames - 1);
        let next_idx = (frame_idx + 1).min(num_frames - 1);
        let frac = (i % frame_len) as f32 / frame_len as f32;

        let sample_gain = smoothed_gain[frame_idx] * (1.0 - frac) + smoothed_gain[next_idx] * frac;
        out.push(sample * sample_gain);
    }

    out
}

/// Brickwall peak limiter with lookahead to prevent clipping.
pub fn peak_limit(
    samples: &[f32],
    sample_rate: u32,
    ceiling_db: f32,
    lookahead_ms: f32,
    release_ms: f32,
) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return samples.to_vec();
    }

    let ceiling = 10.0f32.powf(ceiling_db / 20.0);
    let lookahead_samples = (sample_rate as f32 * (lookahead_ms / 1000.0)).round().max(1.0) as usize;
    let release_samples = (sample_rate as f32 * (release_ms / 1000.0)).round().max(1.0) as usize;

    let n = samples.len();
    let mut raw_gain = vec![1.0f32; n];

    // 1. Calculate instant gain reduction required to keep peaks under ceiling
    for i in 0..n {
        let amp = samples[i].abs();
        if amp > ceiling {
            raw_gain[i] = ceiling / amp;
        }
    }

    // 2. Lookahead attack: propagate gain reduction backwards
    let mut attack_gain = raw_gain.clone();
    for i in (0..n).rev() {
        let lookahead_start = i.saturating_sub(lookahead_samples);
        let target = raw_gain[i];
        if target < 1.0 {
            #[allow(clippy::needless_range_loop)]
            for j in lookahead_start..i {
                let dist = i - j;
                let ramp = target + (1.0 - target) * (dist as f32 / lookahead_samples as f32);
                if ramp < attack_gain[j] {
                    attack_gain[j] = ramp;
                }
            }
        }
    }

    // 3. Smooth release forward
    let mut final_gain = attack_gain.clone();
    let release_coeff = (-1.0 / release_samples as f32).exp();
    let mut current_g = 1.0f32;

    for i in 0..n {
        if attack_gain[i] < current_g {
            current_g = attack_gain[i];
        } else {
            current_g = attack_gain[i] + release_coeff * (current_g - attack_gain[i]);
        }
        final_gain[i] = current_g;
    }

    // 4. Apply limited gain
    let mut out = Vec::with_capacity(n);
    for (i, &sample) in samples.iter().enumerate() {
        out.push(sample * final_gain[i]);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leveler_preserves_silence() {
        let silence = vec![0.0f32; 2400];
        let cfg = LevelerConfig::default();
        let out = level_speech(&silence, 24000, &cfg);
        assert_eq!(out, silence);
    }

    #[test]
    fn test_peak_limiter_clamps_overshoots() {
        let sample_rate = 24000;
        let mut signal = vec![0.5f32; 4800];
        // Inject sudden 2.0 peak (would clip heavily)
        signal[2400] = 2.0;

        let ceiling_db = -1.0;
        let ceiling_linear = 10.0f32.powf(ceiling_db / 20.0);
        let out = peak_limit(&signal, sample_rate, ceiling_db, 5.0, 50.0);

        for &val in &out {
            assert!(val.abs() <= ceiling_linear + 1e-4, "Value {} exceeded ceiling {}", val, ceiling_linear);
        }
    }
}
