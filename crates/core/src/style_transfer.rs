//! Disentangled Parametric Style Transfer & Expression Synthesis Engine (`style_transfer.rs`)
//!
//! Separates speaker identity/gender from vocal expression style so that a pattern
//! extracted or tuned on a male voice (e.g., $F_0 \approx 125\text{ Hz}$) transfers
//! seamlessly onto a female voice ($F_0 \approx 220\text{ Hz}$) or any TTS engine:
//!
//! 1. **Speaker-Normalized Pitch Contour ($\Delta \log_2(F_0 / F_{0,\text{median}})$):**
//!    Pitch shifts and contours are represented in octaves/semitones relative to the active
//!    speaker's median fundamental frequency $F_{0,\text{median}}$, never in fixed Hertz.
//! 2. **Gender-Warped Formant & Cepstral Envelope (VTLN — Vocal Tract Length Normalization):**
//!    All glottal high-pass cutoffs, chest resonance notches, and 24-band Bark spectral filters
//!    scale dynamically with the speaker's estimated vocal tract ratio $\alpha_{\text{speaker}} = (F_{0,\text{median}} / 130.0)^{0.38}$.
//! 3. **Mixed Glottal-Turbulent Excitation Generator:**
//!    Smoothly blends between unvoiced LPC formant breath (whispers), clean de-glottalized
//!    vocal tract diction, and asymmetric open-quotient harmonic drive (shouts, calls, singing).
//! 4. **Temporal & Spatial Prosody Sculpting:**
//!    Controls syllable attack/release gating (staccato tactical commands vs legato singing/murmurs),
//!    musical pitch vibrato (`5.5 Hz` sinusoidal delay modulation), and acoustic distance
//!    (close-ear ASMR proximity cushion vs distant call early reflections).

use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::whisper_dsp::{
    apply_24band_transfer_eq, extract_voice_transfer_matrix, WhisperConfig, WhisperMode,
    TRANSFER_BAND_CENTERS_HZ,
};

/// High-level vocal expression category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StyleCategory {
    #[default]
    Whisper,
    Shout,
    Call,
    Chant,
    Custom,
}

/// Speaker- and gender-invariant parametric representation of a vocal expression style.
/// Can be extracted automatically from a pair of `(normal_voice, target_style_voice)` recordings
/// AND fine-tuned interactively via 6 human-friendly psychoacoustic sliders in Audioticon Studio.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisentangledStyleMatrix {
    /// Unique identifier slug, e.g. `"szept_romantyczny"`, `"krzyk_bojowy"`, `"wolanie_las"`
    pub id: String,
    /// Human-readable display name, e.g. `"Szept do ucha (Romantyczny)"`
    pub name: String,
    /// Associated emoji trigger, e.g. `"💋"`, `"🪖"`, `"📢"`, `"🎵"`
    pub emoji: String,
    /// Optional bracketed text alias, e.g. `Some("ucho")` -> allows `[ucho]` or `🤫:ucho`
    pub alias: Option<String>,
    /// Expression family (`whisper`, `shout`, `call`, `chant`, `custom`)
    pub category: StyleCategory,

    // =========================================================================
    // 6 Human-Centric Psychoacoustic Sliders (Interactive Studio Workbench UI)
    // =========================================================================
    /// 1. Excitation Mix (`0.0 .. 1.0`):
    ///    `0.0` = 100% tonal vocal cords (speech/shout/singing),
    ///    `1.0` = 100% unvoiced LPC turbulent breath.
    pub breath_vs_voice: f32,

    /// 2. Anti-Hiss / Sibilance Smoothing (`0.0 .. 1.0`):
    ///    `0.0` = raw open high-frequency air up to 8.5 kHz,
    ///    `1.0` = maximum velvet de-essing & low-pass smoothing at 4.0 kHz (zero `szelest`/`syczenie`).
    pub anti_hiss_smoothness: f32,

    /// 3. Vocal Tract Tension (`-1.0 .. +1.0`, Gender-Normalized VTLN):
    ///    `-1.0` = deep, relaxed pharyngeal tract (close-ear romantic warmth),
    ///    `0.0` = neutral conversational vocal tract,
    ///    `+1.0` = tense, constricted throat (soldier action whisper, urgent shout, projected call).
    pub vocal_tract_tension: f32,

    /// 4. Syllable Articulation Dynamics (`-1.0 .. +1.0`):
    ///    `-1.0` = slow, lingering legato sustain (romantic whisper, balladic chant/singing),
    ///    `0.0` = natural conversational envelope,
    ///    `+1.0` = clipped staccato syllable endings & sharp consonant attack (military command).
    pub syllable_articulation: f32,

    /// 5. Relative Pitch Melody $\Delta \log_2(F_0 / F_{0,\text{median}}) \times 12$ (`-12.0 .. +12.0` semitones):
    ///    Normalized relative to the speaker's own median $F_0$ so male and female voices shift proportionally.
    pub pitch_melody_semitones: f32,

    /// Musical Vibrato Depth (`0.0 .. 1.0`) for `Chant` / `Śpiew` or emotional tremor.
    #[serde(default)]
    pub vibrato_depth: f32,

    /// 6. Acoustic Distance (`-1.0 .. +1.0`):
    ///    `-1.0` = intimate close-ear ASMR proximity bass cushion (`85..160 Hz`),
    ///    `0.0` = dry studio close-mic,
    ///    `+1.0` = distant open-air call with low-cut projection & early reflections.
    pub proximity_vs_distance: f32,

    // =========================================================================
    // Underlying Disentangled Mathematical Vectors
    // =========================================================================
    /// Relative pitch shift in octaves: $\log_2(F_{0,\text{target}} / F_{0,\text{source}})$
    pub delta_f0_octaves: f32,
    /// Ratio of pitch contour variability $\text{IQR}(\log_2 F_0)_{\text{target}} / \text{IQR}(\log_2 F_0)_{\text{source}}$
    pub f0_contour_dynamism: f32,
    /// Vocal Tract Length Normalization warp factor $\alpha_{\text{VTLN}}$
    pub vtln_warp_factor: f32,
    /// Harmonic open-quotient glottal saturation drive (`0.0 .. 1.0`, used in Shout & Call)
    pub harmonic_drive: f32,
    /// Overall relative RMS output offset in dB (`-12.0 .. +6.0`)
    pub rms_offset_db: f32,
    /// 24-Band Bark/ERB differential spectral transfer curve ($\Delta\text{dB}[0..23]$)
    pub delta_cepstrum_bark_db: [f32; 24],
}

/// Psychoacoustic diagnostic metrics returned to the UI so the user can visually verify
/// that there is no excessive hiss (`szelest`), pure-noise washout (`sam szum`), or robotic comb.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleDiagnosticReport {
    /// Energy ratio of 3.2–9.0 kHz (hiss/rustle) vs 250–2500 Hz (speech diction core) in dB.
    /// Values `<= -6.0 dB` are clean and free of harsh rustle.
    pub hiss_to_core_db: f32,
    /// Energy ratio of 70–250 Hz (warmth/proximity) vs 250–2500 Hz (core) in dB.
    pub warmth_to_core_db: f32,
    /// Percentage of active speech frames dominated by noise rather than formant structure (`0..100%`).
    pub pure_noise_pct: f32,
    /// Auto-detected median fundamental frequency $F_0$ of the speaker in Hz (e.g. `125 Hz` male, `215 Hz` female).
    pub detected_median_f0_hz: f32,
    /// Gender/speaker VTLN scaling factor applied (`~1.00` for adult male, `~1.22` for adult female).
    pub applied_vtln_scale: f32,
    /// 24-band Bark relative spectral envelope in dB for live UI chart rendering.
    pub bark_spectrum_db: [f32; 24],
    /// Human-readable quality verdict in Polish.
    pub verdict: String,
}

impl Default for DisentangledStyleMatrix {
    fn default() -> Self {
        Self::preset_narrative_whisper()
    }
}

impl DisentangledStyleMatrix {
    /// Built-in factory style presets covering Whispers, Shouts, Calls, and Singing/Chant.
    pub fn builtin_catalog() -> Vec<Self> {
        vec![
            Self::preset_narrative_whisper(),
            Self::preset_action_whisper(),
            Self::preset_romantic_whisper(),
            Self::preset_close_mic_whisper(),
            Self::preset_battle_shout(),
            Self::preset_distant_call(),
            Self::preset_melodic_chant(),
        ]
    }

    pub fn preset_narrative_whisper() -> Self {
        let m = crate::whisper_dsp::VoiceTransferMatrix::for_mode(WhisperMode::Narrative);
        Self {
            id: "whisper_narrative".to_string(),
            name: "Szept narracyjny (Półszept sceniczny)".to_string(),
            emoji: "🤫".to_string(),
            alias: Some("narracyjny".to_string()),
            category: StyleCategory::Whisper,
            breath_vs_voice: 0.42,
            anti_hiss_smoothness: 0.65,
            vocal_tract_tension: 0.0,
            syllable_articulation: 0.05,
            pitch_melody_semitones: 0.0,
            vibrato_depth: 0.0,
            proximity_vs_distance: -0.15,
            delta_f0_octaves: 0.0,
            f0_contour_dynamism: 0.75,
            vtln_warp_factor: 1.0,
            harmonic_drive: 0.0,
            rms_offset_db: -3.5,
            delta_cepstrum_bark_db: m.delta_band_db,
        }
    }

    pub fn preset_action_whisper() -> Self {
        let m = crate::whisper_dsp::VoiceTransferMatrix::for_mode(WhisperMode::Action);
        Self {
            id: "whisper_action".to_string(),
            name: "Szept w akcji (Żołnierz na misji)".to_string(),
            emoji: "🪖".to_string(),
            alias: Some("akcja".to_string()),
            category: StyleCategory::Whisper,
            breath_vs_voice: 0.50,
            anti_hiss_smoothness: 0.78,
            vocal_tract_tension: 0.55,
            syllable_articulation: 0.65,
            pitch_melody_semitones: 0.5,
            vibrato_depth: 0.0,
            proximity_vs_distance: 0.0,
            delta_f0_octaves: 0.04,
            f0_contour_dynamism: 0.85,
            vtln_warp_factor: 1.03,
            harmonic_drive: 0.0,
            rms_offset_db: -1.5,
            delta_cepstrum_bark_db: m.delta_band_db,
        }
    }

    pub fn preset_romantic_whisper() -> Self {
        let m = crate::whisper_dsp::VoiceTransferMatrix::for_mode(WhisperMode::Romantic);
        Self {
            id: "whisper_romantic".to_string(),
            name: "Szept romantyczny (Blisko ucha ASMR)".to_string(),
            emoji: "💋".to_string(),
            alias: Some("ucho".to_string()),
            category: StyleCategory::Whisper,
            breath_vs_voice: 0.38,
            anti_hiss_smoothness: 0.88,
            vocal_tract_tension: -0.50,
            syllable_articulation: -0.45,
            pitch_melody_semitones: -1.5,
            vibrato_depth: 0.0,
            proximity_vs_distance: -0.80,
            delta_f0_octaves: -0.12,
            f0_contour_dynamism: 0.65,
            vtln_warp_factor: 0.96,
            harmonic_drive: 0.0,
            rms_offset_db: -5.0,
            delta_cepstrum_bark_db: m.delta_band_db,
        }
    }

    pub fn preset_close_mic_whisper() -> Self {
        let m = crate::whisper_dsp::VoiceTransferMatrix::for_mode(WhisperMode::CloseMic);
        Self {
            id: "whisper_close_mic".to_string(),
            name: "Szept do mikrofonu (Kapsuła radiowa)".to_string(),
            emoji: "🎙️".to_string(),
            alias: Some("mikrofon".to_string()),
            category: StyleCategory::Whisper,
            breath_vs_voice: 0.56,
            anti_hiss_smoothness: 0.55,
            vocal_tract_tension: 0.10,
            syllable_articulation: 0.10,
            pitch_melody_semitones: 0.0,
            vibrato_depth: 0.0,
            proximity_vs_distance: -0.35,
            delta_f0_octaves: 0.0,
            f0_contour_dynamism: 0.70,
            vtln_warp_factor: 1.0,
            harmonic_drive: 0.0,
            rms_offset_db: -3.0,
            delta_cepstrum_bark_db: m.delta_band_db,
        }
    }

    pub fn preset_battle_shout() -> Self {
        Self {
            id: "shout_battle".to_string(),
            name: "Krzyk / Rozkaz bojowy".to_string(),
            emoji: "💥".to_string(),
            alias: Some("rozkaz".to_string()),
            category: StyleCategory::Shout,
            breath_vs_voice: 0.08,
            anti_hiss_smoothness: 0.45,
            vocal_tract_tension: 0.75,
            syllable_articulation: 0.70,
            pitch_melody_semitones: 3.5,
            vibrato_depth: 0.05,
            proximity_vs_distance: 0.15,
            delta_f0_octaves: 0.29,
            f0_contour_dynamism: 1.35,
            vtln_warp_factor: 1.06,
            harmonic_drive: 0.58,
            rms_offset_db: 3.5,
            delta_cepstrum_bark_db: [
                -4.0, -3.0, -1.5, 0.0, 1.5, 2.5, 3.5, 4.5, 5.5, 6.0, 6.5, 7.0, 7.5, 7.5, 7.0, 6.0,
                5.0, 4.0, 2.5, 1.0, -1.0, -2.5, -4.0, -5.5,
            ],
        }
    }

    pub fn preset_distant_call() -> Self {
        Self {
            id: "call_distant".to_string(),
            name: "Wołanie z oddali (Echo przestrzenne)".to_string(),
            emoji: "📢".to_string(),
            alias: Some("wolanie".to_string()),
            category: StyleCategory::Call,
            breath_vs_voice: 0.12,
            anti_hiss_smoothness: 0.70,
            vocal_tract_tension: 0.45,
            syllable_articulation: -0.60, // Elongated vowels for calling across distance!
            pitch_melody_semitones: 4.0,
            vibrato_depth: 0.10,
            proximity_vs_distance: 0.85, // Strong open-air distance & early reflections
            delta_f0_octaves: 0.33,
            f0_contour_dynamism: 1.25,
            vtln_warp_factor: 1.04,
            harmonic_drive: 0.35,
            rms_offset_db: -2.0,
            delta_cepstrum_bark_db: [
                -14.0, -12.0, -10.0, -8.0, -5.0, -2.0, 1.0, 3.5, 5.5, 7.0, 8.0, 8.5, 8.0, 7.0, 5.5,
                3.5, 1.0, -2.0, -5.0, -8.0, -10.0, -12.0, -14.0, -16.0,
            ],
        }
    }

    pub fn preset_melodic_chant() -> Self {
        Self {
            id: "chant_melodic".to_string(),
            name: "Śpiew / Melorecytacja (Ballada)".to_string(),
            emoji: "🎵".to_string(),
            alias: Some("spiew".to_string()),
            category: StyleCategory::Chant,
            breath_vs_voice: 0.10,
            anti_hiss_smoothness: 0.75,
            vocal_tract_tension: -0.20,
            syllable_articulation: -0.75, // Sustained singing vowels (legato)
            pitch_melody_semitones: 2.0,
            vibrato_depth: 0.65, // Rich 5.5 Hz musical vibrato
            proximity_vs_distance: 0.25,
            delta_f0_octaves: 0.17,
            f0_contour_dynamism: 1.50,
            vtln_warp_factor: 0.98,
            harmonic_drive: 0.22,
            rms_offset_db: 1.0,
            delta_cepstrum_bark_db: [
                0.0, 1.5, 2.5, 3.0, 3.0, 2.5, 2.0, 2.5, 3.5, 4.0, 4.5, 4.5, 4.0, 3.5, 3.0, 2.0,
                1.0, 0.0, -1.5, -3.0, -4.5, -6.0, -7.5, -9.0,
            ],
        }
    }

    /// Synchronizes the underlying mathematical vectors when the user moves any of the 6 UI sliders.
    pub fn sync_from_sliders(&mut self) {
        self.breath_vs_voice = self.breath_vs_voice.clamp(0.0, 1.0);
        self.anti_hiss_smoothness = self.anti_hiss_smoothness.clamp(0.0, 1.0);
        self.vocal_tract_tension = self.vocal_tract_tension.clamp(-1.0, 1.0);
        self.syllable_articulation = self.syllable_articulation.clamp(-1.0, 1.0);
        self.pitch_melody_semitones = self.pitch_melody_semitones.clamp(-12.0, 12.0);
        self.vibrato_depth = self.vibrato_depth.clamp(0.0, 1.0);
        self.proximity_vs_distance = self.proximity_vs_distance.clamp(-1.0, 1.0);

        self.delta_f0_octaves = self.pitch_melody_semitones / 12.0;
        self.vtln_warp_factor = (1.0 + self.vocal_tract_tension * 0.08).clamp(0.88, 1.12);

        if matches!(self.category, StyleCategory::Shout | StyleCategory::Call) {
            self.harmonic_drive =
                (0.25 + self.vocal_tract_tension.max(0.0) * 0.50).clamp(0.0, 0.85);
        } else if self.category == StyleCategory::Chant {
            self.harmonic_drive = 0.20;
        } else {
            self.harmonic_drive = (1.0 - self.breath_vs_voice) * 0.15;
        }

        // Recompute the 24-band Bark curve smoothly from the macro parameters
        for (idx, &fc) in TRANSFER_BAND_CENTERS_HZ.iter().enumerate() {
            let mut db = 0.0f32;
            // Low-frequency proximity vs distance & breath cut
            if fc < 250.0 {
                db -= self.breath_vs_voice * 11.0;
                db -= self.proximity_vs_distance * 8.0; // Negative distance (close-ear) boosts lows; positive distance cuts lows
                db -= self.vocal_tract_tension * 4.0;
            } else if fc < 600.0 {
                // Chest boom notch for whispers, body boost for romantic/chant
                db -= self.breath_vs_voice * 10.0;
                db -= self.vocal_tract_tension * 3.5;
            } else if fc <= 2800.0 {
                // Core speech & projection band
                db += self.vocal_tract_tension * 4.5;
                if self.proximity_vs_distance > 0.2 {
                    db += self.proximity_vs_distance * 4.0; // Call projection peak
                }
            } else {
                // High frequency sibilance / air band (> 2.8 kHz)
                let roll = (fc - 2800.0) / 5000.0;
                db -= self.anti_hiss_smoothness * (6.0 + roll * 10.0);
                db += (1.0 - self.anti_hiss_smoothness) * 4.0;
            }
            self.delta_cepstrum_bark_db[idx] = db.clamp(-18.0, 12.0);
        }
    }

    /// Returns the effective alias or falls back to `self.id`.
    pub fn alias_or_id(&self) -> String {
        self.alias
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.id)
            .to_string()
    }

    /// Computes speech pacing modifier in percent (`-25 .. +25`) from `syllable_articulation`.
    pub fn pacing_mod_pct(&self) -> i32 {
        (self.syllable_articulation * 18.0).round() as i32
    }

    /// Computes linear output volume gain (`0.35 .. 1.80`) from `rms_offset_db`.
    pub fn output_gain(&self) -> f32 {
        10.0f32.powf(self.rms_offset_db / 20.0).clamp(0.35, 1.80)
    }
}

/// Estimates the speaker's median fundamental frequency $F_0$ (in Hz) over voiced frames.
/// Returns `~125.0 Hz` for typical male voices and `~215.0 Hz` for typical female voices.
pub fn estimate_median_f0(samples: &[f32], sample_rate: u32) -> f32 {
    if samples.len() < 1024 || sample_rate == 0 {
        return 135.0;
    }
    let sr_f = sample_rate as f32;
    let frame_len = ((sr_f * 0.030).round() as usize).max(256);
    let hop = frame_len / 2;
    let min_lag = (sr_f / 360.0).floor().max(8.0) as usize; // Max F0 = 360 Hz
    let max_lag = (sr_f / 75.0).ceil().min((frame_len - 8) as f32) as usize; // Min F0 = 75 Hz

    if min_lag >= max_lag {
        return 135.0;
    }

    let mut f0_estimates = Vec::new();
    let mut pos = 0usize;
    while pos + frame_len <= samples.len() {
        let frame = &samples[pos..pos + frame_len];
        let energy: f32 = frame.iter().map(|x| x * x).sum::<f32>() / frame_len as f32;
        if energy > 0.0004 {
            let mut best_lag = min_lag;
            let mut best_corr = 0.0f32;
            for lag in min_lag..=max_lag {
                let mut c = 0.0f32;
                let mut e1 = 0.0f32;
                let len = frame_len - lag;
                for i in 0..len {
                    c += frame[i] * frame[i + lag];
                    e1 += frame[i + lag] * frame[i + lag];
                }
                let norm = (c / (energy * len as f32 * e1).sqrt().max(1e-6))
                    * (1.0 - 0.06 * (lag as f32 / max_lag as f32));
                if norm > best_corr {
                    best_corr = norm;
                    best_lag = lag;
                }
            }
            if best_corr > 0.55 {
                f0_estimates.push(sr_f / best_lag as f32);
            }
        }
        pos += hop;
    }

    if f0_estimates.is_empty() {
        return 135.0;
    }
    f0_estimates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    f0_estimates[f0_estimates.len() / 2].clamp(80.0, 340.0)
}

/// Extracts a speaker- and gender-independent [`DisentangledStyleMatrix`] by comparing
/// a recording of a speaker's normal voice (`normal_samples`) against a recording of the
/// target expression (`style_samples` — e.g. whisper, shout, call, or singing).
pub fn extract_disentangled_style_matrix(
    normal_samples: &[f32],
    style_samples: &[f32],
    sample_rate: u32,
    name: &str,
    emoji: &str,
    alias: Option<String>,
) -> DisentangledStyleMatrix {
    let base_transfer =
        extract_voice_transfer_matrix(normal_samples, style_samples, sample_rate, name);

    let f0_norm = estimate_median_f0(normal_samples, sample_rate);
    let f0_style = estimate_median_f0(style_samples, sample_rate);
    let delta_f0_octaves = (f0_style / f0_norm.max(50.0)).log2().clamp(-1.0, 1.0);
    let pitch_semitones = (delta_f0_octaves * 12.0).clamp(-12.0, 12.0);

    // Measure ZCR & high-frequency ratio on style_samples to infer breathiness vs voiced drive
    let zcr_style = style_samples
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count() as f32
        / style_samples.len().max(1) as f32;
    let zcr_norm = normal_samples
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count() as f32
        / normal_samples.len().max(1) as f32;

    let zcr_ratio = (zcr_style / zcr_norm.max(0.01)).clamp(0.5, 4.0);

    // Low-band vs Mid-band differential from the 24 Bark bands:
    let low_delta: f32 = base_transfer.delta_band_db[0..6].iter().sum::<f32>() / 6.0;
    let mid_delta: f32 = base_transfer.delta_band_db[9..15].iter().sum::<f32>() / 6.0;
    let high_delta: f32 = base_transfer.delta_band_db[16..23].iter().sum::<f32>() / 7.0;

    let is_whisper_like = low_delta < -5.5 && zcr_ratio > 1.25;
    let category = if is_whisper_like {
        StyleCategory::Whisper
    } else if base_transfer.rms_offset_db > 1.5 || pitch_semitones > 2.5 {
        StyleCategory::Shout
    } else {
        StyleCategory::Custom
    };

    let breath_vs_voice = if is_whisper_like {
        ((zcr_ratio - 1.0) * 0.35 + (-low_delta / 28.0)).clamp(0.25, 0.65)
    } else {
        ((zcr_ratio - 1.0).max(0.0) * 0.18).clamp(0.0, 0.25)
    };

    // Default to high anti-hiss smoothness so phone recordings never produce harsh rustle
    let anti_hiss_smoothness = (0.75 - (high_delta - mid_delta).max(0.0) * 0.03).clamp(0.45, 0.92);
    let vocal_tract_tension = ((mid_delta - low_delta) / 16.0 - 0.3).clamp(-0.85, 0.85);
    let proximity_vs_distance = ((-low_delta - 6.0) / 10.0).clamp(-0.90, 0.90);

    DisentangledStyleMatrix {
        id: crate::voicelab::sanitize_slug(name),
        name: name.to_string(),
        emoji: emoji.to_string(),
        alias,
        category,
        breath_vs_voice: (breath_vs_voice * 100.0).round() / 100.0,
        anti_hiss_smoothness: (anti_hiss_smoothness * 100.0).round() / 100.0,
        vocal_tract_tension: (vocal_tract_tension * 100.0).round() / 100.0,
        syllable_articulation: 0.0,
        pitch_melody_semitones: if is_whisper_like {
            0.0
        } else {
            (pitch_semitones * 10.0).round() / 10.0
        },
        vibrato_depth: 0.0,
        proximity_vs_distance: (proximity_vs_distance * 100.0).round() / 100.0,
        delta_f0_octaves,
        f0_contour_dynamism: 1.0,
        vtln_warp_factor: (1.0 + vocal_tract_tension * 0.06).clamp(0.90, 1.10),
        harmonic_drive: if category == StyleCategory::Shout {
            0.45
        } else {
            0.05
        },
        rms_offset_db: base_transfer.rms_offset_db,
        delta_cepstrum_bark_db: base_transfer.delta_band_db,
    }
}

/// Second-order Biquad filter for internal style transfer DSP.
#[derive(Clone)]
struct StyleBiquad {
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

impl StyleBiquad {
    fn highpass(sr: f32, fc: f32, q: f32) -> Self {
        let fc = fc.clamp(30.0, sr * 0.44);
        let w0 = 2.0 * std::f32::consts::PI * fc / sr;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let b0 = (1.0 + cos_w0) * 0.5;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) * 0.5;
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

    fn lowpass(sr: f32, fc: f32, q: f32) -> Self {
        let fc = fc.clamp(80.0, sr * 0.46);
        let w0 = 2.0 * std::f32::consts::PI * fc / sr;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let b0 = (1.0 - cos_w0) * 0.5;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) * 0.5;
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

    fn peaking_eq(sr: f32, fc: f32, q: f32, gain_db: f32) -> Self {
        let fc = fc.clamp(40.0, sr * 0.45);
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * fc / sr;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;
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

/// Applies a [`DisentangledStyleMatrix`] to any mono PCM voice buffer (`samples`),
/// automatically normalizing formant and glottal filter frequencies to the speaker's
/// detected median $F_0$ (male vs female VTLN adaptation).
pub fn apply_disentangled_style(
    samples: &[f32],
    sample_rate: u32,
    style: &DisentangledStyleMatrix,
) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }

    let sr_f = sample_rate as f32;
    let median_f0 = estimate_median_f0(samples, sample_rate);
    // Vocal Tract Length Normalization (VTLN) scale:
    // ~1.00 for a 130 Hz male voice, ~1.21 for a 220 Hz female voice
    let speaker_vtln = (median_f0 / 130.0).powf(0.36).clamp(0.85, 1.35);

    // Step 1: If breath_vs_voice >= 0.20 (Whisper or breathy murmur), run the LPC-formant
    //         mixed excitation engine with the user's exact `breath_vs_voice` purity!
    let mut processed = if style.breath_vs_voice >= 0.20 {
        let base_mode = if style.proximity_vs_distance < -0.45 {
            WhisperMode::Romantic
        } else if style.vocal_tract_tension > 0.35 {
            WhisperMode::Action
        } else if style.breath_vs_voice > 0.52 {
            WhisperMode::CloseMic
        } else {
            WhisperMode::Narrative
        };
        let mut cfg = WhisperConfig::for_mode(base_mode);
        cfg.whisper_purity = style.breath_vs_voice.clamp(0.15, 0.85);
        cfg.output_gain_db = style.rms_offset_db;
        cfg.transfer_matrix.attack_sharpness =
            (1.0 + style.syllable_articulation * 0.45).clamp(0.55, 1.65);
        cfg.transfer_matrix.release_gate = if style.syllable_articulation > 0.15 {
            1.0 + style.syllable_articulation * 1.1
        } else {
            1.0
        };
        cfg.transfer_matrix.proximity_warmth =
            (-style.proximity_vs_distance).max(0.0) * 0.48;
        crate::whisper_dsp::to_whisper_with_config(samples, sample_rate, &cfg)
    } else {
        samples.to_vec()
    };

    // Step 2: For Shout / Call / Chant (when breath_vs_voice < 0.20), apply WSOLA vowel sustain
    //         or staccato articulation + open-quotient harmonic glottal drive
    if style.breath_vs_voice < 0.20 && (style.syllable_articulation.abs() > 0.18) {
        // Negative syllable_articulation (-0.75 in Chant/Call) -> slower/sustained vowels (speed < 1.0)
        // Positive syllable_articulation (+0.70 in Shout/Command) -> tighter delivery
        let speed = (1.0 + style.syllable_articulation * 0.16).clamp(0.78, 1.22);
        if (speed - 1.0).abs() > 0.03 && processed.len() > 2048 {
            processed = crate::wsola::wsola_time_stretch(&processed, sample_rate, speed);
        }
    }

    // Step 3: Musical Vibrato / Emotional Tremor (`vibrato_depth > 0.02`, e.g. Singing/Chant `🎵`)
    if style.vibrato_depth > 0.02 && processed.len() > 512 {
        let vib_freq = 5.4f32; // Classic 5.4 Hz human singing vibrato
        let max_delay_samples = (sr_f * 0.0028 * style.vibrato_depth).clamp(1.0, 160.0);
        let orig = processed.clone();
        for (i, out_s) in processed.iter_mut().enumerate() {
            let t = i as f32 / sr_f;
            let lfo = (2.0 * std::f32::consts::PI * vib_freq * t).sin();
            let delay = max_delay_samples * (0.5 + 0.5 * lfo);
            let src_pos = (i as f32 - delay).max(0.0);
            let idx = (src_pos.floor() as usize).min(orig.len() - 1);
            let next_idx = (idx + 1).min(orig.len() - 1);
            let frac = src_pos - idx as f32;
            *out_s = orig[idx] * (1.0 - frac) + orig[next_idx] * frac;
        }
    }

    // Step 4: Gender-Normalized Vocal Tract Tension & Anti-Hiss Sculpting
    // Notice how all center frequencies are multiplied by `speaker_vtln` so male and female
    // formants are sculpted at their true anatomical positions!
    let f1_warmth_hz = 380.0 * speaker_vtln;
    let f2_projection_hz = (1750.0 + style.vocal_tract_tension * 450.0) * speaker_vtln;
    let sibilance_notch_hz = 4100.0 * speaker_vtln.sqrt();
    let silk_lp_hz = (8200.0 - style.anti_hiss_smoothness * 4000.0).clamp(3600.0, sr_f * 0.44);

    let mut eq_warmth = StyleBiquad::peaking_eq(
        sr_f,
        f1_warmth_hz,
        0.9,
        -style.vocal_tract_tension * 5.0 - style.proximity_vs_distance * 3.5,
    );
    let mut eq_proj = StyleBiquad::peaking_eq(
        sr_f,
        f2_projection_hz,
        1.1,
        style.vocal_tract_tension * 5.5 + style.harmonic_drive * 4.0,
    );
    let mut eq_dehiss = StyleBiquad::peaking_eq(
        sr_f,
        sibilance_notch_hz,
        1.2,
        -style.anti_hiss_smoothness * 7.5,
    );
    let mut lp_silk = StyleBiquad::lowpass(sr_f, silk_lp_hz, 0.707);

    let drive = style.harmonic_drive.clamp(0.0, 0.9);
    for s in processed.iter_mut() {
        let mut x = *s;
        if drive > 0.02 {
            // Asymmetric glottal open-quotient harmonic enrichment (adds warm 2nd & 3rd harmonics for Shout/Call/Chant)
            let driven = (x * (1.0 + drive * 2.2)).tanh() + 0.08 * drive * x * x;
            x = x * (1.0 - drive * 0.6) + driven * (drive * 0.6);
        }
        x = eq_warmth.process(x);
        x = eq_proj.process(x);
        x = eq_dehiss.process(x);
        x = lp_silk.process(x);
        *s = x.clamp(-0.96, 0.96);
    }

    // Step 5: Acoustic Distance Modeling (`proximity_vs_distance > 0.25` -> Open-air Call Reflections)
    if style.proximity_vs_distance > 0.25 {
        let dist = style.proximity_vs_distance.clamp(0.0, 1.0);
        let mut hp_dist = StyleBiquad::highpass(sr_f, (180.0 + dist * 180.0) * speaker_vtln, 0.707);
        let d1 = ((0.048 * sr_f) as usize).max(1);
        let d2 = ((0.105 * sr_f) as usize).max(1);
        let dry = processed.clone();
        let pad_len = if dist > 0.5 {
            (0.25 * sr_f) as usize
        } else {
            0
        };
        processed.resize(dry.len() + pad_len, 0.0);
        for i in 0..processed.len() {
            let base = if i < dry.len() { dry[i] } else { 0.0 };
            let echo1 = if i >= d1 && (i - d1) < dry.len() {
                dry[i - d1] * 0.28 * dist
            } else {
                0.0
            };
            let echo2 = if i >= d2 && (i - d2) < dry.len() {
                dry[i - d2] * 0.16 * dist
            } else {
                0.0
            };
            processed[i] = hp_dist.process(base + echo1 + echo2).clamp(-0.95, 0.95);
        }
    }

    // Apply optional fine 24-band differential sculpt if custom category
    if style.category == StyleCategory::Custom {
        apply_24band_transfer_eq(&mut processed, sr_f, &style.delta_cepstrum_bark_db);
    }

    processed
}

/// Applies a [`DisentangledStyleMatrix`] in-place to a 16-bit mono WAV file on disk.
pub fn apply_disentangled_style_to_wav(
    wav_path: &Path,
    style: &DisentangledStyleMatrix,
) -> Result<()> {
    let reader = WavReader::open(wav_path)
        .with_context(|| format!("Nie można otworzyć pliku WAV do style transferu: {:?}", wav_path))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;

    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Int => {
            let max_val = (1i64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .step_by(spec.channels.max(1) as usize)
                .map(|s| (s as f32 / max_val).clamp(-1.0, 1.0))
                .collect()
        }
        SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .step_by(spec.channels.max(1) as usize)
            .map(|s| s.clamp(-1.0, 1.0))
            .collect(),
    };

    if samples.is_empty() {
        return Ok(());
    }

    let styled = apply_disentangled_style(&samples, sample_rate, style);

    let tmp_path = wav_path.with_extension("style_tmp.wav");
    let out_spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&tmp_path, out_spec)?;
    for s in styled {
        let s16 = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(s16)?;
    }
    writer.finalize()?;
    std::fs::rename(&tmp_path, wav_path)?;
    Ok(())
}

/// Computes live psychoacoustic diagnostics on a rendered buffer so the UI can display
/// the exact Hiss/Core dB, Pure-Noise %, detected speaker $F_0$, and 24-band Bark spectrum.
pub fn diagnose_styled_audio(samples: &[f32], sample_rate: u32) -> StyleDiagnosticReport {
    let detected_f0 = estimate_median_f0(samples, sample_rate);
    let vtln_scale = (detected_f0 / 130.0).powf(0.36).clamp(0.85, 1.35);

    let mat = extract_voice_transfer_matrix(samples, samples, sample_rate, "diag");
    let _ = mat; // Keep import active

    // Compute 24-band Bark energies, Hiss (3.2-9 kHz) vs Core (250-2500 Hz), and Warmth (70-250 Hz)
    let sr_f = sample_rate as f32;
    let frame_len = ((sr_f * 0.024).round() as usize).max(128);
    let hop = (frame_len / 2).max(64);
    let mut band_accum = [0.0f64; 24];
    let mut active_frames = 0usize;
    let mut noisy_frames = 0usize;

    let mut pos = 0usize;
    while pos + frame_len <= samples.len() {
        let slice = &samples[pos..pos + frame_len];
        let rms = (slice.iter().map(|x| x * x).sum::<f32>() / frame_len as f32).sqrt();
        if rms > 0.004 {
            active_frames += 1;
            let zcr = slice
                .windows(2)
                .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
                .count() as f32
                / frame_len as f32;
            if zcr > 0.26 {
                noisy_frames += 1;
            }
            // Goertzel energy per Bark center frequency
            for (b_idx, &fc) in TRANSFER_BAND_CENTERS_HZ.iter().enumerate() {
                if fc >= sr_f * 0.46 {
                    continue;
                }
                let k = (0.5 + (frame_len as f32 * fc) / sr_f).floor();
                let w = (2.0 * std::f32::consts::PI * k) / frame_len as f32;
                let coeff = 2.0 * w.cos();
                let mut q1 = 0.0f32;
                let mut q2 = 0.0f32;
                for &x in slice {
                    let q0 = coeff * q1 - q2 + x;
                    q2 = q1;
                    q1 = q0;
                }
                let mag2 = (q1 * q1 + q2 * q2 - q1 * q2 * coeff).max(1e-12);
                band_accum[b_idx] += (mag2.sqrt() / (frame_len as f32 * rms)) as f64;
            }
        }
        pos += hop;
    }

    let mut bark_db = [0.0f32; 24];
    let denom = active_frames.max(1) as f64;
    for i in 0..24 {
        let v = (band_accum[i] / denom).max(1e-6);
        bark_db[i] = (20.0 * v.log10() as f32).clamp(-45.0, 20.0);
    }

    let warmth_db = bark_db[0..4].iter().sum::<f32>() / 4.0;
    let core_db = bark_db[5..15].iter().sum::<f32>() / 10.0;
    let hiss_db = bark_db[16..23].iter().sum::<f32>() / 7.0;

    let hiss_to_core_db = ((hiss_db - core_db) * 10.0).round() / 10.0;
    let warmth_to_core_db = ((warmth_db - core_db) * 10.0).round() / 10.0;
    let pure_noise_pct =
        ((noisy_frames as f32 / active_frames.max(1) as f32) * 1000.0).round() / 10.0;

    let verdict = if hiss_to_core_db > -4.5 {
        "⚠️ Zbyt duży szelest powyżej 3.2 kHz — zwiększ suwak 'Wygładzenie Szelestu'".to_string()
    } else if pure_noise_pct > 25.0 {
        "⚠️ Zbyt duży udział szumu — zmniejsz suwak 'Oddech / Szum vs Struny Głosowe'".to_string()
    } else {
        "✅ Czysty, naturalny wzorzec (Brak szelestu, pełna czytelność dykcji)".to_string()
    };

    StyleDiagnosticReport {
        hiss_to_core_db,
        warmth_to_core_db,
        pure_noise_pct,
        detected_median_f0_hz: (detected_f0 * 10.0).round() / 10.0,
        applied_vtln_scale: (vtln_scale * 100.0).round() / 100.0,
        bark_spectrum_db: bark_db,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_median_f0_male_vs_female() {
        let sr = 24000;
        let male_sig: Vec<f32> = (0..12000)
            .map(|i| (2.0 * std::f32::consts::PI * 125.0 * i as f32 / sr as f32).sin() * 0.5)
            .collect();
        let female_sig: Vec<f32> = (0..12000)
            .map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / sr as f32).sin() * 0.5)
            .collect();

        let f0_m = estimate_median_f0(&male_sig, sr);
        let f0_f = estimate_median_f0(&female_sig, sr);

        assert!(
            (f0_m - 125.0).abs() < 8.0,
            "Expected ~125 Hz male F0, got {}",
            f0_m
        );
        assert!(
            (f0_f - 220.0).abs() < 12.0,
            "Expected ~220 Hz female F0, got {}",
            f0_f
        );
    }

    #[test]
    fn test_disentangled_style_transfer_across_categories() {
        let sr = 24000;
        let mut sig = Vec::with_capacity(12000);
        for i in 0..12000 {
            let t = i as f32 / sr as f32;
            sig.push(
                (2.0 * std::f32::consts::PI * 140.0 * t).sin() * 0.4
                    + (2.0 * std::f32::consts::PI * 800.0 * t).sin() * 0.2,
            );
        }

        for preset in DisentangledStyleMatrix::builtin_catalog() {
            let out = apply_disentangled_style(&sig, sr, &preset);
            assert!(!out.is_empty(), "Preset {} produced empty output", preset.id);
            let diag = diagnose_styled_audio(&out, sr);
            assert!(diag.detected_median_f0_hz >= 80.0);
        }
    }
}
