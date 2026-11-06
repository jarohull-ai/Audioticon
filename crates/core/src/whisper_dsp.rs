//! Studio Whisper Resynthesis DSP (`whisper_dsp.rs`) for Audioticon
//!
//! Converts normal voiced speech (`f32` PCM) into an intimate, natural human whisper
//! using genuine Linear Predictive Coding (LPC) vocal-tract modeling in pure Rust.
//!
//! Why simple volume reduction (`-25%`) or EQ fails:
//! - Normal speech is excited by periodic glottal pulses (fundamental frequency $F_0$
//!   and its harmonics). Lowering volume or cutting bass leaves the harmonic pitch intact,
//!   so the human ear perceives "someone talking quietly from further away", not a whisper.
//! - Furthermore, a downstream opto-leveler will boost a quiet segment back to normal loudness!
//!
//! How `to_whisper` works (Hybrid LPC Source-Filter Whisper Resynthesis):
//! 1. **Pre-emphasis ($1 - 0.97 z^{-1}$):** Flatten spectral tilt before LPC analysis so high-frequency
//!    formants ($F_2, F_3, F_4$) are accurately captured.
//! 2. **Windowed Autocorrelation & Levinson-Durbin Recursion (Order $p = 28$ at 44.1 kHz):**
//!    Every 10 ms (with 20 ms Hanning analysis frames), extracts the exact vocal-tract all-pole filter
//!    $A(z) = 1 + \sum_{k=1}^p a_k z^{-k}$ and residual frame power $G$.
//! 3. **Voicing / Sibilant Detection via Zero-Crossing Rate (ZCR):**
//!    Unvoiced consonants (`s, sz, ś, c, cz, ć, f, ch, p, t, k`) have high ZCR and are ALREADY
//!    turbulent noise in normal speech — we preserve their natural crispness!
//!    Voiced vowels and sonorants (`a, e, i, o, u, y, m, n, l, r`) have periodic glottal $F_0$ —
//!    we replace their periodic excitation with envelope-modulated broadband glottal aspiration noise
//!    filtered through the time-varying all-pole vocal tract $1 / A(z)$.
//! 4. **Close-Mic Whisper Spectral Shaping & Dynamics:**
//!    - Removes sub-glottal chest resonance below 190 Hz (no vocal fold vibration in a whisper).
//!    - Boosts 2.5–6.5 kHz palatal/glottal air turbulence ("close-mic breathiness").
//!    - Soft-knee whisper envelope compression + subtle modulated aspiration breath bed,
//!      calibrated to sit naturally at intimate whisper loudness (-5.5 dB below full voice)
//!      after mastering.

use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperConfig {
    /// LPC filter order (default: 28 for 44.1 kHz, automatically scaled by sample rate)
    pub lpc_order: usize,
    /// Blend of pure unvoiced LPC-resynthesized whisper vs high-passed original consonant detail (default: 0.88)
    pub whisper_purity: f32,
    /// High-frequency air / palatal turbulence boost in dB (default: +5.5 dB)
    pub air_boost_db: f32,
    /// Subtle continuous breath bed level in dBFS during voiced syllables (default: -36.0 dBFS)
    pub breath_layer_db: f32,
    /// Final intimate whisper output level relative to normal speech in dB (default: -5.0 dB)
    pub output_gain_db: f32,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            lpc_order: 28,
            whisper_purity: 0.90,
            air_boost_db: 5.5,
            breath_layer_db: -36.0,
            output_gain_db: -5.0,
        }
    }
}

/// Deterministic XorShift64* PRNG with Gaussian-like (Irwin-Hall) white noise distribution
struct WhisperNoiseGen(u64);

impl WhisperNoiseGen {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        })
    }

    #[inline]
    fn next_uniform(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
        ((v >> 40) as f32 / 16777216.0) * 2.0 - 1.0
    }

    /// Generates zero-mean unit-variance Gaussian-approximated noise sample
    #[inline]
    fn next_gaussian(&mut self) -> f32 {
        // Sum of 3 uniform [-1, 1] variables has variance 3 * (1/3) = 1.0
        self.next_uniform() + self.next_uniform() + self.next_uniform()
    }
}

/// Solves the Yule-Walker equations via Levinson-Durbin recursion.
/// Returns `(a_coeffs, reflection_error_energy)` where `a_coeffs` has length `order + 1` with `a[0] = 1.0`.
pub fn levinson_durbin(r: &[f64], order: usize) -> (Vec<f64>, f64) {
    let mut a = vec![0.0f64; order + 1];
    a[0] = 1.0;

    if r.is_empty() || r[0] <= 1e-12 {
        return (a, 0.0);
    }

    let mut e = r[0];
    let mut a_prev = vec![0.0f64; order + 1];

    for m in 1..=order {
        let mut sum = r[m];
        for k in 1..m {
            sum += a[k] * r[m - k];
        }
        if e.abs() <= 1e-12 {
            break;
        }
        // Reflection coefficient (PARCOR) clamped strictly inside unit circle for guaranteed IIR stability
        let k_m = (-sum / e).clamp(-0.985, 0.985);

        a_prev[..=m].copy_from_slice(&a[..=m]);
        a[m] = k_m;
        for k in 1..m {
            a[k] = a_prev[k] + k_m * a_prev[m - k];
        }

        e *= 1.0 - k_m * k_m;
        if e <= 1e-12 {
            e = 1e-12;
            break;
        }
    }

    // Apply mild bandwidth expansion (gamma = 0.992^k) to prevent overly sharp resonant ringing poles
    let mut gamma = 1.0f64;
    for coeff in a.iter_mut().skip(1) {
        gamma *= 0.992;
        *coeff *= gamma;
    }

    (a, e.max(0.0))
}

/// Second-order Biquad IIR filter for spectral tilt and chest-resonance removal
struct WhisperBiquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl WhisperBiquad {
    fn highpass(sample_rate: f32, cutoff_hz: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn peaking_eq(sample_rate: f32, center_hz: f32, q: f32, gain_db: f32) -> Self {
        let a_amp = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * center_hz / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);

        let b0 = 1.0 + alpha * a_amp;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a_amp;
        let a0 = 1.0 + alpha / a_amp;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a_amp;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn lowpass(sample_rate: f32, cutoff_hz: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Converts a voiced speech buffer (`samples` in `[-1.0, 1.0]`) into a true acoustic whisper
/// using default studio whisper parameters.
pub fn to_whisper(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    to_whisper_with_config(samples, sample_rate, &WhisperConfig::default())
}

/// Converts a voiced speech buffer into a true acoustic whisper with custom [`WhisperConfig`].
pub fn to_whisper_with_config(
    samples: &[f32],
    sample_rate: u32,
    config: &WhisperConfig,
) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }

    let n = samples.len();
    // Scale LPC order appropriately for sample rate (e.g. 16 for 22.05/24 kHz, 28 for 44.1/48 kHz)
    let order = if sample_rate >= 40000 {
        config.lpc_order.clamp(16, 36)
    } else {
        (config.lpc_order * 3 / 5).clamp(12, 22)
    };

    let sr_f = sample_rate as f32;

    // 1. Sub-glottal F0 removal + Pre-emphasis filter before LPC formant extraction:
    // In voiced speech, the glottal fundamental F0 (85..240 Hz) dominates spectral energy.
    // Removing sub-240 Hz F0 energy before autocorrelation ensures Levinson-Durbin allocates
    // 100% of its poles to actual vocal-tract formants (F1..F4: 350..4800 Hz) rather than F0!
    let mut hp_pre_lpc = WhisperBiquad::highpass(sr_f, 240.0, 0.707);
    let pre_emph_coeff = 0.95f32;
    let mut pre_emphasized = Vec::with_capacity(n);
    let mut prev = 0.0f32;
    for &s in samples {
        let no_f0 = hp_pre_lpc.process(s);
        pre_emphasized.push(no_f0 - pre_emph_coeff * prev);
        prev = no_f0;
    }

    // 2. Overlap-Add (OLA) LPC Analysis-Resynthesis (24ms frame, 8ms hop)
    let frame_len = ((sample_rate as f32 * 0.024).round() as usize).max(order + 8);
    let hop_len = (frame_len / 3).max(1);

    let mut window = Vec::with_capacity(frame_len);
    for i in 0..frame_len {
        let w = 0.5
            * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (frame_len - 1).max(1) as f32).cos());
        window.push(w);
    }

    let mut lpc_whisper = vec![0.0f32; n + frame_len];
    let mut ola_weight = vec![0.0f32; n + frame_len];
    let mut rng = WhisperNoiseGen::new(0x517c_c1b7_2722_0a95 ^ (n as u64));

    let mut r = vec![0.0f64; order + 1];
    let breath_floor_amp = 10.0f32.powf(config.breath_layer_db / 20.0);

    let mut pos = 0usize;
    while pos < n {
        let end = (pos + frame_len).min(n);
        let actual_len = end - pos;

        // Windowed frame for autocorrelation
        let mut frame = vec![0.0f64; frame_len];
        let mut orig_energy = 0.0f32;
        let mut zero_crossings = 0usize;

        for i in 0..actual_len {
            let s_pre = pre_emphasized[pos + i];
            frame[i] = (s_pre * window[i]) as f64;
            let s_orig = samples[pos + i];
            orig_energy += s_orig * s_orig;
            if i > 0 && ((samples[pos + i - 1] >= 0.0) != (s_orig >= 0.0)) {
                zero_crossings += 1;
            }
        }

        let orig_rms = (orig_energy / actual_len.max(1) as f32).sqrt();
        let zcr = zero_crossings as f32 / actual_len.max(1) as f32;

        // Compute autocorrelation lags 0..=order
        for lag in 0..=order {
            let mut sum = 0.0f64;
            for i in 0..actual_len.saturating_sub(lag) {
                sum += frame[i] * frame[i + lag];
            }
            r[lag] = sum;
        }

        let (a_coeffs, err_energy) = levinson_durbin(&r, order);
        let gain = (err_energy / actual_len.max(1) as f64).sqrt() as f32;

        // Synthesize frame through all-pole vocal tract filter 1 / A(z) excited by aspiration noise
        let mut synth_history = vec![0.0f32; order];
        let mut synth_frame = vec![0.0f32; actual_len];
        let mut synth_energy = 0.0f32;

        for synth_sample in synth_frame.iter_mut().take(actual_len) {
            // Turbulent glottal aspiration source + subtle breath bed when speech is active
            let breath_mod = if orig_rms > 0.003 {
                breath_floor_amp * 0.25
            } else {
                0.0
            };
            let excitation = rng.next_gaussian() * (gain + breath_mod);

            let mut y = excitation;
            for k in 1..=order {
                y -= (a_coeffs[k] as f32) * synth_history[k - 1];
            }
            y = y.clamp(-4.0, 4.0);

            // Shift filter delay line
            for k in (1..order).rev() {
                synth_history[k] = synth_history[k - 1];
            }
            if order > 0 {
                synth_history[0] = y;
            }

            *synth_sample = y;
            synth_energy += y * y;
        }

        // Match frame RMS energy to original speech envelope so syllable dynamics and articulation remain crystal clear
        let synth_rms = (synth_energy / actual_len.max(1) as f32).sqrt();
        let scale = if synth_rms > 1e-6 {
            (orig_rms / synth_rms).min(6.0)
        } else {
            0.0
        };

        // Voiceless consonants (s, sz, ś, cz, f, t, k) naturally have high ZCR (> 0.16).
        // Blending in a fraction of their original high-frequency transient keeps diction razor-sharp.
        let consonant_blend = if zcr > 0.16 {
            ((zcr - 0.16) * 2.5).clamp(0.0, 0.45)
        } else {
            1.0 - config.whisper_purity.clamp(0.0, 1.0)
        };

        for i in 0..actual_len {
            let w = window[i];
            let resynth = synth_frame[i] * scale;
            let orig_hi = pre_emphasized[pos + i];
            let blended = resynth * (1.0 - consonant_blend) + orig_hi * consonant_blend;
            lpc_whisper[pos + i] += blended * w;
            ola_weight[pos + i] += w * w;
        }

        pos += hop_len;
    }

    // 3. Normalize OLA window weights and apply partial de-emphasis (preserving whisper brightness)
    let mut de_emph_state = 0.0f32;
    let de_emph_coeff = 0.45f32; // Lighter than 0.95 pre-emphasis -> natural +6 dB/octave whisper spectral tilt!

    // Remove sub-220 Hz chest/fundamental resonance completely (whispers have zero F0 chest rumble)
    let mut hp_chest = WhisperBiquad::highpass(sr_f, 220.0, 0.707);
    // Boost palatal & glottal air turbulence band (2.8 kHz - 5.5 kHz)
    let mut eq_air = WhisperBiquad::peaking_eq(sr_f, 3600.0, 1.1, config.air_boost_db);
    // Smooth harsh ultrasonic hiss above 7.5 kHz
    let mut lp_smooth = WhisperBiquad::lowpass(sr_f, (sr_f * 0.42).min(7800.0), 0.707);

    let out_gain = 10.0f32.powf(config.output_gain_db / 20.0);
    let mut output = Vec::with_capacity(n);

    for i in 0..n {
        let w = ola_weight[i];
        let raw = if w > 1e-5 {
            lpc_whisper[i] / w
        } else {
            lpc_whisper[i]
        };

        // Partial de-emphasis
        de_emph_state = raw + de_emph_coeff * de_emph_state;

        // Spectral shaping: cut chest F0, boost whisper air formants, smooth top end
        let s1 = hp_chest.process(de_emph_state);
        let s2 = eq_air.process(s1);
        let s3 = lp_smooth.process(s2);

        // Soft-knee whisper dynamic compression (whispers have narrower dynamic range than full voice)
        let abs_s = s3.abs();
        let compressed = if abs_s > 0.15 {
            let sign = s3.signum();
            sign * (0.15 + (abs_s - 0.15).tanh() * 0.35)
        } else {
            s3
        };

        output.push((compressed * out_gain).clamp(-0.95, 0.95));
    }

    output
}

/// Checks whether a [`crate::types::TextSegment`] is marked for whispered delivery
/// via `ActorTag::Whisper` (`🤫`, `[szept]`, `[whisper]`) or an explicit `"whisper"` / `"🤫"` acoustic preset.
pub fn is_whisper_segment(segment: &crate::types::TextSegment) -> bool {
    segment.actor_tags.contains(&crate::types::ActorTag::Whisper)
        || segment
            .filter
            .as_deref()
            .map(|f| {
                let lower = f.to_lowercase();
                f.contains("🤫") || lower.contains("whisper") || lower.contains("szept")
            })
            .unwrap_or(false)
}

/// Applies in-place LPC Whisper Resynthesis directly to a 16-bit PCM WAV file on disk.
pub fn apply_whisper_to_wav_file(wav_path: &Path) -> Result<()> {
    let file = File::open(wav_path)
        .with_context(|| format!("Nie można otworzyć pliku WAV do konwersji szeptu: {:?}", wav_path))?;
    let mut reader = WavReader::new(BufReader::new(file))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;

    let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .filter_map(|s| s.ok())
            .map(|s| (s as f32) / 32768.0)
            .collect(),
        (SampleFormat::Float, 32) => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
        _ => reader
            .samples::<i32>()
            .filter_map(|s| s.ok())
            .map(|s| (s as f32) / 2147483648.0)
            .collect(),
    };

    if samples.is_empty() {
        return Ok(());
    }

    let whispered = to_whisper(&samples, sample_rate);

    let tmp_path = wav_path.with_extension("whisper.tmp.wav");
    let out_spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&tmp_path, out_spec)?;
    for s in whispered {
        let s16 = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(s16)?;
    }
    writer.finalize()?;
    std::fs::rename(&tmp_path, wav_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whisper_dsp_eliminates_periodic_f0_and_shifts_spectral_tilt() {
        let sample_rate = 24000;
        let duration_samples = 24000; // 1.0 second
        let f0 = 130.0f32; // Typical male vocal fundamental frequency (130 Hz)

        // Construct a synthetic voiced vowel with strong periodic F0 (130 Hz) + formants (650 Hz, 1800 Hz)
        let mut voiced = Vec::with_capacity(duration_samples);
        for i in 0..duration_samples {
            let t = i as f32 / sample_rate as f32;
            let glottal_f0 = (2.0 * std::f32::consts::PI * f0 * t).sin() * 0.55
                + (2.0 * std::f32::consts::PI * (f0 * 2.0) * t).sin() * 0.25;
            let formant1 = (2.0 * std::f32::consts::PI * 650.0 * t).sin() * 0.15;
            let formant2 = (2.0 * std::f32::consts::PI * 1800.0 * t).sin() * 0.08;
            voiced.push(glottal_f0 + formant1 + formant2);
        }

        let whispered = to_whisper(&voiced, sample_rate);
        assert_eq!(whispered.len(), voiced.len());

        // 1. Measure pitch periodicity (normalized autocorrelation at pitch period lag = 24000 / 130 = 184 samples)
        let pitch_lag = (sample_rate as f32 / f0).round() as usize;
        let calc_norm_autocorr = |sig: &[f32], lag: usize| -> f32 {
            let slice = &sig[4000..20000];
            let mut corr = 0.0f32;
            let mut e0 = 0.0f32;
            let mut e1 = 0.0f32;
            for i in 0..(slice.len() - lag) {
                corr += slice[i] * slice[i + lag];
                e0 += slice[i] * slice[i];
                e1 += slice[i + lag] * slice[i + lag];
            }
            corr / (e0 * e1).sqrt().max(1e-6)
        };

        let voiced_periodicity = calc_norm_autocorr(&voiced, pitch_lag);
        let whisper_periodicity = calc_norm_autocorr(&whispered, pitch_lag);

        assert!(
            voiced_periodicity > 0.85,
            "Original voiced signal should be strongly periodic at F0: {}",
            voiced_periodicity
        );
        assert!(
            whisper_periodicity.abs() < 0.25,
            "Whispered signal must eliminate periodic glottal F0 vibration (got {})",
            whisper_periodicity
        );

        // 2. Measure Zero-Crossing Rate (ZCR): turbulent whisper noise has significantly higher ZCR than periodic 130 Hz speech
        let calc_zcr = |sig: &[f32]| -> f32 {
            let crossings = sig
                .windows(2)
                .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
                .count();
            crossings as f32 / sig.len() as f32
        };

        let voiced_zcr = calc_zcr(&voiced);
        let whisper_zcr = calc_zcr(&whispered);
        assert!(
            whisper_zcr > voiced_zcr * 2.5,
            "Whisper ZCR ({:.3}) must be > 2.5x voiced ZCR ({:.3}) due to broadband turbulent aspiration",
            whisper_zcr,
            voiced_zcr
        );
    }
}
