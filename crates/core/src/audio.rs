use crate::cache::compute_segment_hash;
use crate::types::{
    AudiobookMetadata, ChapterMark, DirectedItem, DirectedScript, RenderResult, TextSegment,
};
use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

pub const DEFAULT_SAMPLE_RATE: u32 = 44100;

/// Checks if `ffmpeg` binary is available on the system PATH and executable
pub fn check_ffmpeg_installed() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Returns a helpful Polish installation guide if ffmpeg is missing
pub fn ffmpeg_missing_instructions() -> &'static str {
    "Program 'ffmpeg' nie został wykryty w Twoim systemie.\n\
    Audioticon wykorzystuje ffmpeg do masteringu EBU R128, eksportu M4B/FLAC i precyzyjnych pomiarów głośności.\n\n\
    Instalacja w 1 krok:\n\
    • Ubuntu/Debian/Mint:  sudo apt install ffmpeg\n\
    • macOS (Homebrew):    brew install ffmpeg\n\
    • Windows (winget):    winget install Gyan.FFmpeg\n\
    • Windows (ręcznie):   pobierz z https://ffmpeg.org/download.html i dodaj do PATH"
}

/// Fast XorShift64 PRNG for deterministic procedural audio noise
struct SimpleNoiseRng(u64);

impl SimpleNoiseRng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x853c_49e6_748f_ea9b
        } else {
            seed
        })
    }

    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 as f32) / (u64::MAX as f32)) * 2.0 - 1.0
    }
}

/// Pure Rust generation of studio room-tone silence WAV file using [`crate::room_tone::RoomToneProfile`].
/// Instead of digital dead-zero vacuum (-inf dB), generates an ultra-subtle
/// studio ambient floor at ~ -62 dBFS, completely eliminating headphone gating sensations.
pub fn create_silence_wav(out_path: &Path, duration_ms: u32, sample_rate: u32) -> Result<()> {
    let tmp_path = out_path.with_extension("tmp.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&tmp_path, spec)
        .with_context(|| format!("Nie można utworzyć pliku ciszy: {:?}", tmp_path))?;

    let total_samples = ((sample_rate as u64 * duration_ms as u64) / 1000) as usize;
    let profile = crate::room_tone::RoomToneProfile::default();
    let samples = profile.generate_samples(
        total_samples,
        sample_rate,
        0x71e9_3a11_c001_cafe ^ (duration_ms as u64),
    );

    for s in samples {
        let sample_val = (s * 8000.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(sample_val)?;
    }
    writer.finalize()?;

    // Atomic move
    std::fs::rename(&tmp_path, out_path)
        .with_context(|| format!("Nie można przenieść pliku ciszy do: {:?}", out_path))?;

    Ok(())
}

/// Biquad digital filter for formant modeling (Audio EQ Cookbook)
struct BiquadFilter {
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

impl BiquadFilter {
    fn bandpass(sample_rate: f32, center_freq: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * center_freq / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * w0.cos();
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

    fn process(&mut self, sample: f32) -> f32 {
        let out = self.b0 * sample + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = sample;
        self.y2 = self.y1;
        self.y1 = out;
        out
    }
}

/// Generates an organic lektor breath / sigh WAV file.
/// Uses a procedural acoustic model with vocal tract formants (750Hz and 1750Hz)
/// shaped by an organic attack/decay respiratory envelope.
pub fn create_breath_wav(out_path: &Path, duration_ms: u32, sample_rate: u32) -> Result<()> {
    create_breath_wav_with_gain(out_path, duration_ms, sample_rate, -27.5)
}

/// Generates a procedural human lektor breath WAV with configurable amplitude gain in dBFS.
pub fn create_breath_wav_with_gain(
    out_path: &Path,
    duration_ms: u32,
    sample_rate: u32,
    gain_db: f32,
) -> Result<()> {
    let tmp_path = out_path.with_extension("tmp.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&tmp_path, spec)
        .with_context(|| format!("Nie można utworzyć pliku oddechu: {:?}", tmp_path))?;

    let total_samples = (sample_rate as u64 * duration_ms as u64) / 1000;
    let mut bp_pharynx = BiquadFilter::bandpass(sample_rate as f32, 750.0, 1.2);
    let mut bp_oral = BiquadFilter::bandpass(sample_rate as f32, 1750.0, 1.5);
    let mut rng = SimpleNoiseRng::new(0xa5a5_5a5a_1234_5678);

    let attack_samples = ((total_samples as f32) * 0.22) as usize;
    let decay_samples = (total_samples as usize)
        .saturating_sub(attack_samples)
        .max(1);

    // Convert dBFS to linear amplitude factor
    let gain = 10.0f32.powf(gain_db / 20.0);

    for i in 0..total_samples as usize {
        let white_sample = rng.next_f32();
        let f1 = bp_pharynx.process(white_sample);
        let f2 = bp_oral.process(white_sample) * 0.55;
        let colored_noise = f1 + f2;

        let env = if i < attack_samples {
            let t = i as f32 / attack_samples as f32;
            (t * std::f32::consts::PI / 2.0).sin()
        } else {
            let t = (i - attack_samples) as f32 / decay_samples as f32;
            ((1.0 - t).max(0.0) * std::f32::consts::PI / 2.0)
                .sin()
                .powi(2)
        };

        let val = (colored_noise * env * gain * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(val)?;
    }
    writer.finalize()?;

    // Atomic move
    std::fs::rename(&tmp_path, out_path)
        .with_context(|| format!("Nie można przenieść pliku oddechu do: {:?}", out_path))?;

    Ok(())
}

fn sanitize_file_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Generates or loads a sound effect (Foley / SFX) WAV file.
/// Looks for custom assets on disk (`./sfx/{name}.wav`, `~/.local/share/audioticon/sfx/{name}.wav`),
/// and falls back to pure Rust procedural sound synthesis.
pub fn create_sfx_wav(name: &str, out_path: &Path, sample_rate: u32) -> Result<()> {
    let clean = name.trim().to_lowercase();

    // 1. Check disk assets (local project or user library, supporting wav, ogg, mp3, flac)
    if let Some(src) = crate::sfx::find_sfx_asset(&clean) {
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                src.to_str().unwrap(),
                "-ar",
                &sample_rate.to_string(),
                "-ac",
                "1",
                out_path.to_str().unwrap(),
            ])
            .output();
        if let Ok(res) = status {
            if res.status.success() {
                return Ok(());
            }
        }
    }

    // Pure Rust procedural synthesis
    let tmp_path = out_path.with_extension("tmp.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&tmp_path, spec)
        .with_context(|| format!("Nie można utworzyć pliku SFX: {:?}", tmp_path))?;

    let duration_ms = match clean.as_str() {
        s if s.contains("bell")
            || s.contains("telefon")
            || s.contains("ding")
            || s.contains("ping") =>
        {
            380
        }
        s if s.contains("paper") || s.contains("notes") || s.contains("kartka") => 320,
        s if s.contains("footstep")
            || s.contains("crunch")
            || s.contains("lod")
            || s.contains("krok") =>
        {
            280
        }
        s if s.contains("door") || s.contains("drzwi") => 400,
        s if s.contains("thunder") || s.contains("boom") || s.contains("grzmot") => 900,
        s if s.contains("car") || s.contains("auto") || s.contains("silnik") => 600,
        s if s.contains("shot")
            || s.contains("strzal")
            || s.contains("gun")
            || s.contains("pistolet") =>
        {
            450
        }
        s if s.contains("heartbeat") || s.contains("serce") || s.contains("puls") => 850,
        s if s.contains("clock") || s.contains("zegar") || s.contains("tykanie") => 180,
        s if s.contains("glass") || s.contains("szklo") || s.contains("brzek") => 500,
        s if s.contains("water") || s.contains("kropla") || s.contains("drop") => 250,
        s if s.contains("click") || s.contains("switch") || s.contains("pstryk") => 80,
        _ => 250,
    };

    let total_samples = (sample_rate as u64 * duration_ms as u64) / 1000;
    let mut rng = SimpleNoiseRng::new(0x1337_c001_beef_cafe ^ (duration_ms as u64));

    if clean.contains("bell")
        || clean.contains("telefon")
        || clean.contains("ding")
        || clean.contains("ping")
    {
        // Bell / Phone notification chime: dual sine (880Hz + 1760Hz) with exponential decay
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let decay = (-12.0 * t).exp();
            let tone = (2.0 * std::f32::consts::PI * 880.0 * t).sin() * 0.65
                + (2.0 * std::f32::consts::PI * 1760.0 * t).sin() * 0.35;
            let val = (tone * decay * 0.28 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("paper") || clean.contains("notes") || clean.contains("kartka") {
        // Paper rustle: bandpass-filtered noise around 2400Hz with flutter modulation
        let mut bp = BiquadFilter::bandpass(sample_rate as f32, 2400.0, 2.2);
        for i in 0..total_samples {
            let t = i as f32 / total_samples as f32;
            let env = (t * std::f32::consts::PI).sin();
            let flutter = 0.7
                + 0.3 * (2.0 * std::f32::consts::PI * 18.0 * (i as f32 / sample_rate as f32)).sin();
            let noise = bp.process(rng.next_f32());
            let val = (noise * env * flutter * 0.16 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("footstep")
        || clean.contains("crunch")
        || clean.contains("lod")
        || clean.contains("krok")
    {
        // Ice/snow crunch: crisp high-frequency burst around 3500Hz with fast decay
        let mut bp = BiquadFilter::bandpass(sample_rate as f32, 3500.0, 2.5);
        for i in 0..total_samples {
            let t = i as f32 / total_samples as f32;
            let env = (1.0 - t).powi(3);
            let noise = bp.process(rng.next_f32());
            let val = (noise * env * 0.22 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("door") || clean.contains("drzwi") {
        // Door thud: decaying low frequency (110Hz)
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let decay = (-10.0 * t).exp();
            let tone = (2.0 * std::f32::consts::PI * 110.0 * t).sin();
            let val = (tone * decay * 0.35 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("thunder") || clean.contains("boom") || clean.contains("grzmot") {
        // Thunder: lowpass rumble
        let mut bp = BiquadFilter::bandpass(sample_rate as f32, 85.0, 1.2);
        for i in 0..total_samples {
            let t = i as f32 / total_samples as f32;
            let env = (1.0 - t).powi(2);
            let noise = bp.process(rng.next_f32());
            let val = (noise * env * 0.35 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("shot")
        || clean.contains("strzal")
        || clean.contains("gun")
        || clean.contains("pistolet")
    {
        // Gunshot: sharp transient noise burst + low-mid explosive thump with exponential decay
        let mut lp = BiquadFilter::bandpass(sample_rate as f32, 160.0, 1.0);
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let transient = (-60.0 * t).exp() * rng.next_f32();
            let boom = (-14.0 * t).exp() * lp.process(rng.next_f32()) * 1.5;
            let sample = transient * 0.45 + boom * 0.35;
            let val = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("heartbeat") || clean.contains("serce") || clean.contains("puls") {
        // Heartbeat: classic dual acoustic thumps (lub-dub at 0ms and 320ms)
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let beat1_t = t;
            let beat2_t = t - 0.32;
            let b1 = if (0.0..0.22).contains(&beat1_t) {
                (-22.0 * beat1_t).exp() * (2.0 * std::f32::consts::PI * 58.0 * beat1_t).sin()
            } else {
                0.0
            };
            let b2 = if (0.0..0.22).contains(&beat2_t) {
                (-25.0 * beat2_t).exp() * (2.0 * std::f32::consts::PI * 76.0 * beat2_t).sin() * 0.8
            } else {
                0.0
            };
            let sample = (b1 + b2) * 0.40;
            let val = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("clock") || clean.contains("zegar") || clean.contains("tykanie") {
        // Clock tick: precise high-Q resonant click at 1400Hz
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let decay = (-55.0 * t).exp();
            let tone = (2.0 * std::f32::consts::PI * 1400.0 * t).sin();
            let val = (tone * decay * 0.26 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("glass") || clean.contains("szklo") || clean.contains("brzek") {
        // Shattering glass: dual high modal rings (2850Hz + 4200Hz) with turbulent scatter noise
        let mut bp = BiquadFilter::bandpass(sample_rate as f32, 3800.0, 2.5);
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let decay = (-15.0 * t).exp();
            let ring = (2.0 * std::f32::consts::PI * 2850.0 * t).sin() * 0.5
                + (2.0 * std::f32::consts::PI * 4200.0 * t).sin() * 0.5;
            let noise = bp.process(rng.next_f32()) * (-25.0 * t).exp();
            let sample = (ring * 0.22 + noise * 0.20) * decay;
            let val = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("water") || clean.contains("kropla") || clean.contains("drop") {
        // Water droplet: upward frequency sweep from 450Hz to 1100Hz with fast bell envelope
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let freq = 450.0 + 650.0 * (t * 8.0).min(1.0);
            let phase = 2.0 * std::f32::consts::PI * freq * t;
            let decay = (-24.0 * t).exp();
            let sample = phase.sin() * decay * 0.32;
            let val = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else if clean.contains("click") || clean.contains("switch") || clean.contains("pstryk") {
        // Switch click: short high impulse
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let decay = (-90.0 * t).exp();
            let tone = (2.0 * std::f32::consts::PI * 2400.0 * t).sin();
            let val = (tone * decay * 0.30 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    } else {
        // Default engine / vehicle low drone
        for i in 0..total_samples {
            let t = i as f32 / sample_rate as f32;
            let decay = (-3.0 * t).exp();
            let tone = (2.0 * std::f32::consts::PI * 55.0 * t).sin() * 0.7
                + (2.0 * std::f32::consts::PI * 110.0 * t).sin() * 0.3;
            let val = (tone * decay * 0.20 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        }
    }

    writer.finalize()?;
    std::fs::rename(&tmp_path, out_path)?;
    Ok(())
}

/// Generates an assembled ambient background track across the entire timeline duration.
/// Each ambient interval (start_ms, end_ms, name) is rendered with smooth procedural soundscapes.
pub fn create_ambient_track(
    spans: &[(u64, u64, String)],
    total_duration_ms: u64,
    out_path: &Path,
    sample_rate: u32,
) -> Result<()> {
    let tmp_path = out_path.with_extension("tmp.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&tmp_path, spec).with_context(|| {
        format!(
            "Nie można utworzyć ścieżki tła akustycznego: {:?}",
            tmp_path
        )
    })?;

    let total_samples = (sample_rate as u64 * total_duration_ms) / 1000;
    let mut rng = SimpleNoiseRng::new(0xcafe_babe_dead_beef ^ total_duration_ms);

    let mut bp_wind = BiquadFilter::bandpass(sample_rate as f32, 380.0, 1.1);
    let mut bp_rain = BiquadFilter::bandpass(sample_rate as f32, 2200.0, 0.9);
    let mut bp_border = BiquadFilter::bandpass(sample_rate as f32, 320.0, 1.2);
    let mut bp_sea = BiquadFilter::bandpass(sample_rate as f32, 220.0, 0.8);
    let mut bp_cafe = BiquadFilter::bandpass(sample_rate as f32, 650.0, 1.4);
    let mut bp_forest = BiquadFilter::bandpass(sample_rate as f32, 4500.0, 3.5);
    let mut bp_fire = BiquadFilter::bandpass(sample_rate as f32, 320.0, 1.0);

    for s_idx in 0..total_samples {
        let current_ms = (s_idx * 1000) / sample_rate as u64;
        let active_span = spans
            .iter()
            .find(|(start, end, _)| current_ms >= *start && current_ms < *end);

        if let Some((_, _, name)) = active_span {
            let clean = name.to_lowercase();
            let t = s_idx as f32 / sample_rate as f32;

            let sample_f32 = if clean.contains("rain") || clean.contains("deszcz") {
                let noise = bp_rain.process(rng.next_f32());
                noise * 0.035
            } else if clean.contains("sea")
                || clean.contains("morze")
                || clean.contains("ocean")
                || clean.contains("fale")
            {
                // Ocean swell: slow 0.12Hz wave cycle
                let swell = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 0.12 * t).sin();
                let wave_noise = bp_sea.process(rng.next_f32());
                wave_noise * swell * 0.045
            } else if clean.contains("cafe")
                || clean.contains("kawiarnia")
                || clean.contains("restauracja")
                || clean.contains("pub")
            {
                // Warm cafe ambience: diffuse chatter flutter
                let chatter = bp_cafe.process(rng.next_f32());
                let sub = (2.0 * std::f32::consts::PI * 80.0 * t).sin() * 0.015;
                chatter * 0.025 + sub
            } else if clean.contains("space")
                || clean.contains("kosmos")
                || clean.contains("statek")
                || clean.contains("scifi")
            {
                // Deep space / sci-fi ship hum: 48Hz + 96Hz drone
                (2.0 * std::f32::consts::PI * 48.0 * t).sin() * 0.030
                    + (2.0 * std::f32::consts::PI * 96.0 * t).sin() * 0.012
            } else if clean.contains("forest")
                || clean.contains("las")
                || clean.contains("swierszcze")
                || clean.contains("noc")
            {
                // Night forest: 4Hz pulsating crickets
                let pulse = (2.0 * std::f32::consts::PI * 4.0 * t).sin().max(0.0).powi(2);
                let crickets = bp_forest.process(rng.next_f32()) * pulse * 0.030;
                let soft_wind = bp_wind.process(rng.next_f32()) * 0.015;
                crickets + soft_wind
            } else if clean.contains("fire")
                || clean.contains("ogien")
                || clean.contains("ognisko")
                || clean.contains("kominek")
            {
                // Fire crackle: warm rumble + sporadic pop
                let rumble = bp_fire.process(rng.next_f32()) * 0.022;
                let pop = if rng.next_f32() > 0.9982 {
                    0.12 * (rng.next_f32() - 0.5)
                } else {
                    0.0
                };
                rumble + pop
            } else if clean.contains("border")
                || clean.contains("granica")
                || clean.contains("car")
                || clean.contains("auto")
            {
                let engine = (2.0 * std::f32::consts::PI * 45.0 * t).sin() * 0.5
                    + (2.0 * std::f32::consts::PI * 90.0 * t).sin() * 0.25;
                let wind = bp_border.process(rng.next_f32()) * 0.5;
                engine * 0.03 + wind * 0.025
            } else {
                // Winter wind
                let gust = 0.55 + 0.45 * (2.0 * std::f32::consts::PI * 0.18 * t).sin();
                let wind = bp_wind.process(rng.next_f32());
                wind * gust * 0.040
            };

            let val = (sample_f32 * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(val)?;
        } else {
            // Subtle room tone silence
            writer.write_sample(0)?;
        }
    }

    writer.finalize()?;
    std::fs::rename(&tmp_path, out_path)?;
    Ok(())
}

/// Mixes dialogue track and ambient background track using sidechain auto-ducking.
/// When speech is present, the ambient soundscape ducks down smoothly by -16dB.
/// In pauses, the atmospheric soundscape rises back to audible immersion.
pub fn mix_dialogue_with_ambience(
    dialogue_path: &Path,
    ambient_path: &Path,
    out_path: &Path,
) -> Result<()> {
    let tmp_path = out_path.with_extension("duck_mix.tmp.wav");
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            ambient_path.to_str().unwrap(),
            "-i",
            dialogue_path.to_str().unwrap(),
            "-filter_complex",
            "[0:a][1:a]sidechaincompress=threshold=0.07:ratio=6:attack=40:release=350[ducked];[ducked][1:a]amix=inputs=2:duration=first:dropout_transition=0",
            tmp_path.to_str().unwrap(),
        ])
        .output()
        .with_context(|| "Nie można uruchomić ffmpeg do miksowania tła ze ścieżką lektora")?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Błąd FFmpeg podczas miksowania tła akustycznego: {}", err);
    }

    std::fs::rename(&tmp_path, out_path)?;
    Ok(())
}

/// Pure Rust seamless sample-accurate stitching of multiple WAV files into one.
/// Uses disk-backed [`crate::pcm_stream::PcmStreamFile`] so memory stays strictly bounded (< 25 MB)
/// even on 10h+ novels, applies [`crate::studio_leveler::level_speech`] & [`crate::studio_leveler::peak_limit`]
/// for studio-grade dynamic consistency, and applies a seamless 5ms equal-power crossfade between chunks.
pub fn stitch_wav_files(inputs: &[PathBuf], out_path: &Path, sample_rate: u32) -> Result<()> {
    let tmp_out = out_path.with_extension("tmp.wav");
    let tmp_pcm = out_path.with_extension("stream.tmp.pcm");
    let pcm_stream = crate::pcm_stream::PcmStreamFile::create(&tmp_pcm, sample_rate)?;
    let leveler_cfg = crate::studio_leveler::LevelerConfig::default();

    // 5ms crossfade window (e.g. 120 samples at 24kHz)
    let default_xfade_len = ((sample_rate as f32) * 0.005).round() as usize;
    let mut pending_tail: Vec<f32> = Vec::new();

    for in_file in inputs {
        let file = File::open(in_file)
            .with_context(|| format!("Błąd otwarcia pliku składowego: {:?}", in_file))?;
        let mut reader = WavReader::new(BufReader::new(file))
            .with_context(|| format!("Błąd odczytu nagłówka WAV: {:?}", in_file))?;

        let in_spec = reader.spec();
        if in_spec.sample_rate != sample_rate {
            let _ = std::fs::remove_file(&tmp_pcm);
            anyhow::bail!(
                "Niezgodność częstotliwości próbkowania w {:?}: oczekiwano {} Hz, otrzymano {} Hz",
                in_file,
                sample_rate,
                in_spec.sample_rate
            );
        }

        if in_spec.bits_per_sample != 16 || in_spec.sample_format != SampleFormat::Int {
            let _ = std::fs::remove_file(&tmp_pcm);
            anyhow::bail!(
                "Niezgodny format audio w {:?}: wymagane 16-bit PCM (Int), otrzymano {}-bit {:?}",
                in_file,
                in_spec.bits_per_sample,
                in_spec.sample_format
            );
        }

        let raw_samples: Vec<f32> = match in_spec.channels {
            1 => {
                let mut v = Vec::with_capacity(reader.duration() as usize);
                for sample in reader.samples::<i16>() {
                    v.push((sample? as f32) / 32768.0);
                }
                v
            }
            2 => {
                let mut v = Vec::with_capacity(reader.duration() as usize);
                let mut s_iter = reader.samples::<i16>();
                while let Some(left) = s_iter.next() {
                    let right = s_iter.next().unwrap_or(Ok(0))?;
                    let mono = ((left? as i32 + right as i32) / 2) as f32 / 32768.0;
                    v.push(mono);
                }
                v
            }
            other => {
                let _ = std::fs::remove_file(&tmp_pcm);
                anyhow::bail!(
                    "Nieobsługiwana liczba kanałów ({}) w pliku {:?}",
                    other,
                    in_file
                );
            }
        };

        if raw_samples.is_empty() {
            continue;
        }

        // Apply Studio Opto-Leveler & Lookahead Peak Limiter per chunk
        let leveled = crate::studio_leveler::level_speech(&raw_samples, sample_rate, &leveler_cfg);
        let mut chunk_samples =
            crate::studio_leveler::peak_limit(&leveled, sample_rate, -1.0, 5.0, 50.0);

        if pending_tail.is_empty() {
            // First non-empty chunk: apply micro fade-in at the start
            let head_fade = default_xfade_len.min(chunk_samples.len() / 2);
            for (i, sample) in chunk_samples.iter_mut().enumerate().take(head_fade) {
                let factor = ((i as f32 / head_fade as f32) * std::f32::consts::PI / 2.0).sin();
                *sample *= factor;
            }

            let xfade = default_xfade_len.min(chunk_samples.len() / 4);
            if xfade > 0 && chunk_samples.len() > xfade {
                let split_idx = chunk_samples.len() - xfade;
                pcm_stream.append_samples(&chunk_samples[..split_idx])?;
                pending_tail = chunk_samples[split_idx..].to_vec();
            } else {
                pcm_stream.append_samples(&chunk_samples)?;
                pending_tail.clear();
            }
        } else {
            // Crossfade with pending tail from previous chunk
            let xfade = pending_tail.len().min(chunk_samples.len() / 2);
            let mut xfade_buf = Vec::with_capacity(xfade + pending_tail.len());
            for i in 0..xfade {
                let t = i as f32 / xfade as f32;
                let fade_out = ((1.0 - t) * std::f32::consts::PI / 2.0).sin();
                let fade_in = (t * std::f32::consts::PI / 2.0).sin();
                let mixed = (pending_tail[i] * fade_out + chunk_samples[i] * fade_in).clamp(-1.0, 1.0);
                xfade_buf.push(mixed);
            }

            if pending_tail.len() > xfade {
                xfade_buf.extend_from_slice(&pending_tail[xfade..]);
            }
            pcm_stream.append_samples(&xfade_buf)?;

            let remaining_chunk = &chunk_samples[xfade..];
            let next_xfade = default_xfade_len.min(remaining_chunk.len() / 4);
            if next_xfade > 0 && remaining_chunk.len() > next_xfade {
                let split_idx = remaining_chunk.len() - next_xfade;
                pcm_stream.append_samples(&remaining_chunk[..split_idx])?;
                pending_tail = remaining_chunk[split_idx..].to_vec();
            } else {
                pcm_stream.append_samples(remaining_chunk)?;
                pending_tail.clear();
            }
        }
    }

    // Flush final tail with micro fade-out
    if !pending_tail.is_empty() {
        let tail_len = pending_tail.len();
        for (i, sample) in pending_tail.iter_mut().enumerate() {
            let t = (tail_len - 1 - i) as f32 / tail_len as f32;
            let factor = (t * std::f32::consts::PI / 2.0).sin();
            *sample *= factor;
        }
        pcm_stream.append_samples(&pending_tail)?;
    }

    // Stream disk-backed PCM container directly into final WAV with bounded 64KB chunk buffer
    pcm_stream.export_wav(&tmp_out, None)?;
    let _ = std::fs::remove_file(&tmp_pcm);

    // Atomic rename
    std::fs::rename(&tmp_out, out_path)
        .with_context(|| format!("Nie można przenieść pliku wynikowego do: {:?}", out_path))?;

    Ok(())
}

/// Constructs FFmpeg audio filter chain with silence trimming and DSP acoustic presets.
pub fn build_ffmpeg_filter_chain(filter: Option<&str>) -> String {
    let registry = crate::presets::get_active_registry();
    build_ffmpeg_filter_chain_with_registry(filter, &registry)
}

/// Constructs FFmpeg audio filter chain using an explicit preset registry.
pub fn build_ffmpeg_filter_chain_with_registry(
    filter: Option<&str>,
    registry: &crate::presets::PresetRegistry,
) -> String {
    let mut filters = vec![
        "silenceremove=start_periods=1:start_duration=0.005:start_threshold=-45dB:detection=rms"
            .to_string(),
        "areverse".to_string(),
        "silenceremove=start_periods=1:start_duration=0.005:start_threshold=-45dB:detection=rms"
            .to_string(),
        "areverse".to_string(),
    ];

    if let Some(preset_tag) = filter {
        let (preset, _warn) = registry.resolve(preset_tag);
        let dsp_filter = preset.to_ffmpeg_filter();
        if !dsp_filter.is_empty() {
            filters.push(dsp_filter);
        }
    } else {
        // Natural narration default: clean studio vocal conditioning
        // (75 Hz subsonic roll-off, de-essing for harsh sibilants, +1.2dB clarity at 3.2kHz, -1.0dB chestiness cut at 260Hz)
        filters.push("highpass=f=75,deesser=i=0.35:m=0.5:f=0.5:s=o,equalizer=f=3200:t=q:w=1.2:g=1.2,equalizer=f=260:t=q:w=1.0:g=-1.0".to_string());
    }

    filters.join(",")
}

/// Trait for TTS synthesizers.
#[async_trait::async_trait]
pub trait SpeechSynthesizer: Send + Sync {
    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()>;
    fn voice_name(&self) -> &str;
    fn max_concurrency(&self) -> usize {
        6 // Default async network concurrency
    }
    /// Resolves character tags or aliases into concrete voice identifiers used for hashing and diagnostics.
    fn resolve_voice_label(&self, requested: Option<&str>) -> String {
        requested
            .filter(|s| !s.trim().is_empty() && *s != "default")
            .unwrap_or_else(|| self.voice_name())
            .to_string()
    }
}

/// Synthesizer bridge using edge-tts with audio trimming and DSP filter chaining.
pub struct EdgeTtsBridge {
    pub voice: String,
    pub edge_tts_bin: PathBuf,
}

impl EdgeTtsBridge {
    pub fn new(voice: impl Into<String>) -> Self {
        let bin = Self::find_edge_tts();
        Self {
            voice: voice.into(),
            edge_tts_bin: bin,
        }
    }

    pub fn with_binary(voice: impl Into<String>, bin_path: PathBuf) -> Self {
        Self {
            voice: voice.into(),
            edge_tts_bin: bin_path,
        }
    }

    fn find_edge_tts() -> PathBuf {
        // 1. Check dedicated audioticon virtualenv (~/.local/share/audioticon/venv/bin/edge-tts)
        if let Some(data_dir) = dirs::data_dir() {
            let venv_edge = data_dir.join("audioticon/venv/bin/edge-tts");
            if venv_edge.exists() {
                return venv_edge;
            }
        }
        // 2. Check active VIRTUAL_ENV
        if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
            let p = PathBuf::from(venv).join("bin/edge-tts");
            if p.exists() {
                return p;
            }
        }
        // 3. Fallbacks
        let temp_edge = std::env::temp_dir().join("edge_env/bin/edge-tts");
        let candidates = [
            temp_edge.as_path(),
            Path::new("/usr/local/bin/edge-tts"),
            Path::new("/usr/bin/edge-tts"),
        ];
        for c in candidates {
            if c.exists() {
                return c.to_path_buf();
            }
        }
        PathBuf::from("edge-tts")
    }
}

#[async_trait::async_trait]
impl SpeechSynthesizer for EdgeTtsBridge {
    fn voice_name(&self) -> &str {
        &self.voice
    }

    fn max_concurrency(&self) -> usize {
        6 // Run up to 6 requests concurrently to Microsoft servers
    }

    fn resolve_voice_label(&self, requested: Option<&str>) -> String {
        match requested {
            Some(v) if !v.trim().is_empty() && v != "default" => {
                let trimmed = v.trim();
                let clean = if let Some(stripped) = trimmed.strip_prefix("edge:") {
                    stripped.trim()
                } else if let Some(stripped) = trimmed.strip_prefix("piper:") {
                    stripped.trim()
                } else {
                    trimmed
                };
                let lower = clean.to_lowercase();
                if lower.contains("zofia") || lower.contains("kobieta") || lower.contains("female")
                {
                    "pl-PL-ZofiaNeural".to_string()
                } else if lower.contains("marek")
                    || lower.contains("mezczyzna")
                    || lower.contains("male")
                {
                    "pl-PL-MarekNeural".to_string()
                } else {
                    clean.to_string()
                }
            }
            _ => self.voice.clone(),
        }
    }

    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()> {
        let trimmed = segment.text.trim();
        if !trimmed.chars().any(|c| c.is_alphanumeric()) {
            return create_silence_wav(out_path, 80, DEFAULT_SAMPLE_RATE);
        }

        let temp_txt = out_path.with_extension("txt");
        tokio::fs::write(&temp_txt, trimmed).await?;

        let voice_to_use = self.resolve_voice_label(segment.voice.as_deref());

        // When male character (👨) is cast on the same voice as narrator (Marek),
        // apply distinct -20Hz pitch shift and -10% pacing to provide clear masculine contrast in dialogue:
        let is_same_as_narrator = voice_to_use == self.voice;
        let is_character_override =
            segment.voice.is_some() && segment.voice.as_deref() != Some("default");
        let (pitch_mod, rate_mod) = if is_same_as_narrator && is_character_override {
            let p = if segment.pitch_mod == -4 || segment.pitch_mod == 0 {
                -20
            } else {
                segment.pitch_mod
            };
            let r = if segment.rate_mod == -6 || segment.rate_mod == 0 {
                -10
            } else {
                segment.rate_mod
            };
            (p, r)
        } else {
            (segment.pitch_mod, segment.rate_mod)
        };

        let is_whisper = crate::whisper_dsp::is_whisper_segment(segment);
        let rate_arg = format!("--rate={:+}%", rate_mod);
        let pitch_arg = format!("--pitch={:+}Hz", pitch_mod);
        let vol_arg = if is_whisper {
            "--volume=+0%".to_string()
        } else {
            format!("--volume={:+}%", segment.volume_mod)
        };

        let temp_mp3 = out_path.with_extension("temp.mp3");
        let temp_wav = out_path.with_extension("temp.wav");

        let status = tokio::process::Command::new(&self.edge_tts_bin)
            .arg(format!("--voice={}", voice_to_use))
            .arg(rate_arg)
            .arg(pitch_arg)
            .arg(vol_arg)
            .arg(format!("-f={}", temp_txt.display()))
            .arg(format!("--write-media={}", temp_mp3.display()))
            .status()
            .await
            .with_context(|| format!("Nie można uruchomić {:?}", self.edge_tts_bin))?;

        if !status.success() {
            let _ = tokio::fs::remove_file(&temp_txt).await;
            anyhow::bail!("edge-tts zakończył się błędem: {:?}", status);
        }

        let custom_style = segment
            .filter
            .as_deref()
            .and_then(|f| crate::presets::get_active_registry().resolve_style(f));

        // For whisper or disentangled style segments, skip the legacy FFmpeg bandpass filter so LPC/style DSP
        // receives full-bandwidth formants and sibilants.
        let filter_chain = if is_whisper || custom_style.is_some() {
            "anull".to_string()
        } else if let Some(ref filter_name) = segment.filter {
            let registry = crate::presets::get_active_registry();
            let (preset, _warn) = registry.resolve(filter_name);
            let dsp = preset.to_ffmpeg_filter();
            if dsp.is_empty() {
                "anull".to_string()
            } else {
                dsp
            }
        } else {
            "anull".to_string()
        };

        let conv = tokio::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&temp_mp3)
            .arg("-af")
            .arg(&filter_chain)
            .arg("-ar")
            .arg(DEFAULT_SAMPLE_RATE.to_string())
            .arg("-ac")
            .arg("1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(&temp_wav)
            .output()
            .await?;

        let _ = tokio::fs::remove_file(&temp_txt).await;
        let _ = tokio::fs::remove_file(&temp_mp3).await;

        if !conv.status.success() {
            let stderr = String::from_utf8_lossy(&conv.stderr);
            let _ = tokio::fs::remove_file(&temp_wav).await;
            anyhow::bail!("ffmpeg conversion failed: {}", stderr);
        }

        if let Some(ref style) = custom_style {
            crate::style_transfer::apply_disentangled_style_to_wav(&temp_wav, style)?;
        } else if is_whisper {
            let mode =
                crate::whisper_dsp::WhisperMode::from_filter_tag(segment.filter.as_deref());
            crate::whisper_dsp::apply_whisper_to_wav_file_with_mode(&temp_wav, mode)?;
        }

        tokio::fs::rename(&temp_wav, out_path)
            .await
            .with_context(|| format!("Błąd atomowego zapisu do cache: {:?}", out_path))?;

        Ok(())
    }
}

/// Inner state of the persistent Piper Python worker process.
struct PiperWorkerProcess {
    process: std::process::Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl Drop for PiperWorkerProcess {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

const PIPER_WORKER_CODE: &str = include_str!("piper_worker.py");

/// Offline Local Synthesizer bridge using a persistent Piper worker (100% offline, model kept in RAM).
pub struct PiperBridge {
    pub default_model_path: PathBuf,
    pub voice_id: String,
    pub python_bin: PathBuf,
    pub worker_script: PathBuf,
    workers: Mutex<HashMap<PathBuf, Vec<PiperWorkerProcess>>>,
}

impl PiperBridge {
    pub fn new(model_path: PathBuf) -> Self {
        let name = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("piper_model")
            .to_string();

        let py = Self::find_python();
        let script = Self::ensure_worker_script();

        Self {
            default_model_path: model_path,
            voice_id: format!("piper-{}", name),
            python_bin: py,
            worker_script: script,
            workers: Mutex::new(HashMap::new()),
        }
    }

    async fn acquire_worker(&self, model_path: &Path) -> Result<PiperWorkerProcess> {
        let worker_opt = {
            let mut guard = self.workers.lock().await;
            guard.get_mut(model_path).and_then(|pool| pool.pop())
        };

        if let Some(worker) = worker_opt {
            Ok(worker)
        } else {
            self.spawn_worker_for(model_path)
        }
    }

    async fn return_worker(&self, model_path: &Path, worker: PiperWorkerProcess) {
        let mut guard = self.workers.lock().await;
        guard
            .entry(model_path.to_path_buf())
            .or_default()
            .push(worker);
    }

    fn ensure_worker_script() -> PathBuf {
        let script_dir = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("audioticon");
        let _ = std::fs::create_dir_all(&script_dir);
        let script_path = script_dir.join("piper_worker.py");
        if !script_path.exists()
            || std::fs::read_to_string(&script_path)
                .map(|s| s != PIPER_WORKER_CODE)
                .unwrap_or(true)
        {
            let _ = std::fs::write(&script_path, PIPER_WORKER_CODE);
        }
        script_path
    }

    fn find_python() -> PathBuf {
        // 1. Check dedicated audioticon virtualenv (~/.local/share/audioticon/venv/bin/python3)
        if let Some(data_dir) = dirs::data_dir() {
            let venv_py = data_dir.join("audioticon/venv/bin/python3");
            if venv_py.exists() {
                return venv_py;
            }
        }
        // 2. Check active VIRTUAL_ENV
        if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
            let p = PathBuf::from(venv).join("bin/python3");
            if p.exists() {
                return p;
            }
        }
        // 3. Fallbacks
        let temp_py = std::env::temp_dir().join("edge_env/bin/python3");
        let candidates = [
            temp_py.as_path(),
            Path::new("/usr/local/bin/python3"),
            Path::new("/usr/bin/python3"),
            Path::new("python3"),
        ];
        for c in candidates {
            if c.exists() {
                return c.to_path_buf();
            }
        }
        PathBuf::from("python3")
    }

    fn spawn_worker_for(&self, model_path: &Path) -> Result<PiperWorkerProcess> {
        let mut child = std::process::Command::new(&self.python_bin)
            .arg(&self.worker_script)
            .arg(model_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .with_context(|| {
                format!(
                    "Błąd startu procesu pomocniczego Piper dla modelu {:?}: {:?}",
                    model_path, self.worker_script
                )
            })?;

        let stdin = child.stdin.take().expect("Child stdin open");
        let stdout = child.stdout.take().expect("Child stdout open");
        let mut reader = BufReader::new(stdout);

        let mut ready_line = String::new();
        reader.read_line(&mut ready_line)?;

        Ok(PiperWorkerProcess {
            process: child,
            stdin,
            reader,
        })
    }

    /// Resolves character voice strings or aliases to on-disk Piper ONNX model paths
    pub fn resolve_model_path(&self, requested_voice: Option<&str>) -> PathBuf {
        let raw_voice = match requested_voice {
            Some(v) if !v.trim().is_empty() && v != "default" => v.trim(),
            _ => return self.default_model_path.clone(),
        };

        // 1. Strip provider prefix if present (e.g. "piper:pl_PL-mc_speech-medium" -> "pl_PL-mc_speech-medium")
        let clean = if let Some(stripped) = raw_voice.strip_prefix("piper:") {
            stripped.trim()
        } else if let Some(stripped) = raw_voice.strip_prefix("edge:") {
            stripped.trim()
        } else {
            raw_voice
        };

        // 2. Direct path to .onnx file (relative or absolute)
        let direct_path = PathBuf::from(clean);
        if !clean.contains("..")
            && direct_path.exists()
            && direct_path.extension().and_then(|s| s.to_str()) == Some("onnx")
        {
            return direct_path;
        }

        // 3. Direct filename in models_dir
        let models_dir = crate::models::models_dir();
        if !clean.contains("..") && !clean.contains('/') && !clean.contains('\\') {
            let direct_in_models = models_dir.join(clean);
            if direct_in_models.exists()
                && direct_in_models.extension().and_then(|s| s.to_str()) == Some("onnx")
            {
                return direct_in_models;
            }
            let onnx_in_models = models_dir.join(format!("{}.onnx", clean));
            if onnx_in_models.exists() {
                return onnx_in_models;
            }
        }

        // 4. Direct model lookup via models catalog
        if let Some(item) = crate::models::find_model(clean) {
            if let Some(ref p) = item.onnx_path {
                if p.exists() {
                    return p.clone();
                }
            }
            let p = models_dir.join(format!("{}.onnx", item.id));
            if p.exists() {
                return p;
            }
        }

        let voice_str = clean.to_lowercase();

        // 5. Mapping from Edge neural voice names or actor tags to Piper models
        let target_id = if voice_str.contains("zofia")
            || voice_str.contains("gosia")
            || voice_str.contains("female")
            || voice_str.contains("kobieta")
            || voice_str.contains("agnieszka")
        {
            "pl_PL-gosia-medium"
        } else if voice_str == "mc"
            || voice_str.starts_with("mc_")
            || voice_str.contains("mc_speech")
            || voice_str.contains("mcspeech")
            || voice_str.contains("guard")
            || voice_str.contains("straznik")
            || voice_str.contains("strażnik")
        {
            "pl_PL-mc_speech-medium"
        } else if voice_str.contains("darkman") {
            "pl_PL-darkman-medium"
        } else if voice_str.contains("marek")
            || voice_str.contains("male")
            || voice_str.contains("mezczyzna")
            || voice_str.contains("mężczyzna")
        {
            // If mc_speech model is available, use it for male characters so it differs from narrator (darkman)
            let mc_candidate = crate::models::models_dir().join("pl_PL-mc_speech-medium.onnx");
            if mc_candidate.exists() {
                "pl_PL-mc_speech-medium"
            } else {
                "pl_PL-darkman-medium"
            }
        } else if voice_str.contains("amy") || voice_str.contains("jenny") {
            "en_US-amy-medium"
        } else if voice_str.contains("lessac")
            || voice_str.contains("ryan")
            || voice_str.contains("guy")
        {
            "en_US-lessac-medium"
        } else {
            ""
        };

        if !target_id.is_empty() {
            let candidate = crate::models::models_dir().join(format!("{}.onnx", target_id));
            if candidate.exists() {
                return candidate;
            }
        }

        // 3. Fallback to default model
        self.default_model_path.clone()
    }
}

#[async_trait::async_trait]
impl SpeechSynthesizer for PiperBridge {
    fn voice_name(&self) -> &str {
        &self.voice_id
    }

    fn resolve_voice_label(&self, requested: Option<&str>) -> String {
        let target_model_path = self.resolve_model_path(requested);
        let stem = target_model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("pl_PL-darkman-medium");
        format!("piper-{}", stem)
    }

    fn max_concurrency(&self) -> usize {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        // Piper ONNX synthesis is CPU-bound. Clamping between 1 and 6 threads utilizes
        // multi-core processors (e.g. Ryzen 12-core) for 3-4x speedups without thrashing CPU cache.
        cpus.clamp(1, 6)
    }

    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()> {
        let trimmed = segment.text.trim();
        if !trimmed.chars().any(|c| c.is_alphanumeric()) {
            return create_silence_wav(out_path, 80, DEFAULT_SAMPLE_RATE);
        }

        let pid = std::process::id();
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let temp_raw_wav = out_path.with_extension(format!("{}_{}.piper_raw.wav", pid, unique_id));
        let temp_wav = out_path.with_extension(format!("{}_{}.temp.wav", pid, unique_id));

        let is_whisper = crate::whisper_dsp::is_whisper_segment(segment);
        let length_scale = if segment.rate_mod < 0 {
            1.0 - (segment.rate_mod as f32 / 100.0)
        } else {
            1.0 / (1.0 + (segment.rate_mod as f32 / 100.0))
        };
        let volume_scale = if is_whisper {
            1.0
        } else {
            (1.0 + (segment.volume_mod as f32 / 100.0)).clamp(0.2, 2.0)
        };

        let req_json = serde_json::json!({
            "text": segment.text.trim(),
            "out_path": temp_raw_wav.to_str().unwrap_or_default(),
            "length_scale": length_scale,
            "volume": volume_scale
        });

        let target_model_path = self.resolve_model_path(segment.voice.as_deref());

        // Acquire worker from model pool
        let mut worker = self.acquire_worker(&target_model_path).await?;

        // Synthesize in-memory via stdin/stdout
        let synth_res: Result<()> = {
            let mut line = req_json.to_string();
            line.push('\n');

            worker.stdin.write_all(line.as_bytes())?;
            worker.stdin.flush()?;

            let mut resp_line = String::new();
            worker.reader.read_line(&mut resp_line)?;

            let val: serde_json::Value = serde_json::from_str(&resp_line)
                .with_context(|| format!("Nieprawidłowa odpowiedź procesu Piper: {}", resp_line))?;

            if val.get("status").and_then(|s| s.as_str()) != Some("ok") {
                anyhow::bail!("Piper worker error: {:?}", val);
            }
            Ok(())
        };

        if let Err(e) = synth_res {
            // Worker is in an invalid state or pipe broke; do NOT return to pool, drop it!
            let _ = tokio::fs::remove_file(&temp_raw_wav).await;
            return Err(e);
        }

        // Return healthy worker to pool for reuse
        self.return_worker(&target_model_path, worker).await;

        let actual_sr = match hound::WavReader::open(&temp_raw_wav) {
            Ok(reader) => reader.spec().sample_rate,
            Err(_) => 22050,
        };

        let custom_style = segment
            .filter
            .as_deref()
            .and_then(|f| crate::presets::get_active_registry().resolve_style(f));

        let mut filter_chain = if is_whisper || custom_style.is_some() {
            build_ffmpeg_filter_chain(None)
        } else {
            build_ffmpeg_filter_chain(segment.filter.as_deref())
        };

        // Handle pitch modification for Piper via ffmpeg asetrate + atempo
        if segment.pitch_mod != 0 && !is_whisper && custom_style.is_none() {
            let semitones = segment.pitch_mod as f32 / 3.0;
            let factor = 2f32.powf(semitones / 12.0);
            let pitch_filter = format!(
                "asetrate={:.0}*{:.4},aresample={}:resampler=soxr,atempo={:.4}",
                actual_sr,
                factor,
                DEFAULT_SAMPLE_RATE,
                1.0 / factor
            );
            filter_chain = format!("{},{}", filter_chain, pitch_filter);
        }

        // Process through ffmpeg for silence trimming, sample rate unification, and DSP filters
        let conv = tokio::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&temp_raw_wav)
            .arg("-af")
            .arg(&filter_chain)
            .arg("-ar")
            .arg(DEFAULT_SAMPLE_RATE.to_string())
            .arg("-ac")
            .arg("1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(&temp_wav)
            .output()
            .await?;

        let _ = tokio::fs::remove_file(&temp_raw_wav).await;

        if !conv.status.success() {
            let stderr = String::from_utf8_lossy(&conv.stderr);
            let _ = tokio::fs::remove_file(&temp_wav).await;
            anyhow::bail!("ffmpeg conversion failed: {}", stderr);
        }

        if let Some(ref style) = custom_style {
            crate::style_transfer::apply_disentangled_style_to_wav(&temp_wav, style)?;
        } else if is_whisper {
            let mode =
                crate::whisper_dsp::WhisperMode::from_filter_tag(segment.filter.as_deref());
            crate::whisper_dsp::apply_whisper_to_wav_file_with_mode(&temp_wav, mode)?;
        }

        tokio::fs::rename(&temp_wav, out_path)
            .await
            .with_context(|| format!("Błąd atomowego zapisu do cache: {:?}", out_path))?;

        Ok(())
    }
}

/// Synthesizer bridge using ElevenLabs REST API with expressive actor tags and caching.
pub struct ElevenLabsBridge {
    pub api_key: String,
    pub voice: String,
    pub model_id: String,
}

impl ElevenLabsBridge {
    pub fn new(api_key: impl Into<String>, voice: impl Into<String>) -> Self {
        let v = voice.into();
        let default_v = if v.is_empty() || v == "default" || v == "pl-PL-MarekNeural" {
            "adam".to_string()
        } else {
            v
        };
        Self {
            api_key: api_key.into(),
            voice: default_v,
            model_id: "eleven_v3".to_string(),
        }
    }

    pub fn with_model(
        api_key: impl Into<String>,
        voice: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        let v = voice.into();
        let default_v = if v.is_empty() || v == "default" || v == "pl-PL-MarekNeural" {
            "adam".to_string()
        } else {
            v
        };
        Self {
            api_key: api_key.into(),
            voice: default_v,
            model_id: model_id.into(),
        }
    }

    /// Maps voice aliases, character roles, or foreign Edge neural voice names to valid ElevenLabs voice IDs
    pub fn map_voice_alias(alias_or_id: &str) -> &str {
        let clean = alias_or_id.trim().to_lowercase();
        match clean.as_str() {
            "antoni" => "ErXwobaYiN019PkySvjV",
            "adam" => "pNInz6obpgDQGcFmaJgB",
            "rachel" => "21m00Tcm4TlvDq8ikWAM",
            "domi" => "AZnzlk1XvdvUeBnXmlld",
            "bella" => "EXAVITQu4vr4xnSDxMaL",
            "josh" => "TxGEqnHWrfWFTfGW9XjX",
            "elli" => "MF3mGyEYCl7XYWbV9V6O",
            "bzipek" => "JzXwT2R1MhLW7Pq3Q6p",
            "roger" => "CwhRBWXzGAHq8TQ4Fs17",
            "harry" => "SOYHLrjzK2X1ezoPC6cr",
            "sam" => "UgBBs2ckDm3j6sX9q6aX",
            "gigi" => "XrExE5yFIh4Om3iX1d2v",
            "jessica" => "ZspeM3T1m7Df6L7OqgXhZ",
            "aria" => "ZQ4q5T6oX8P9v3M2aN1bR",
            _ => {
                // If the parser or script assigned a female role (👩, Zofia, Gosia)
                if clean.contains("zofia")
                    || clean.contains("kobieta")
                    || clean.contains("female")
                    || clean.contains("gosia")
                    || clean.contains("ona")
                    || clean.contains("woman")
                    || clean.contains("agnieszka")
                {
                    "21m00Tcm4TlvDq8ikWAM" // Rachel (female)
                }
                // If the parser or script assigned a male character role (👨, Marek, Darkman, MC Speech, strażnik)
                else if clean.contains("marek")
                    || clean.contains("mezczyzna")
                    || clean.contains("mężczyzna")
                    || clean.contains("male")
                    || clean.contains("darkman")
                    || clean.contains("mc_speech")
                    || clean.contains("guard")
                    || clean.contains("straznik")
                {
                    "TxGEqnHWrfWFTfGW9XjX" // Josh (male character contrast against narrator Adam)
                }
                // Default narrator fallback
                else if clean == "default" || clean == "narrator" || clean == "lektor" {
                    "pNInz6obpgDQGcFmaJgB" // Adam (narrator)
                } else {
                    alias_or_id.trim()
                }
            }
        }
    }

    /// Resolves voice alias or character name, checking both built-in mappings and local custom designed/cloned voices
    pub fn resolve_voice_id(alias_or_id: &str) -> String {
        let mapped = Self::map_voice_alias(alias_or_id);
        if mapped != alias_or_id.trim() {
            return mapped.to_string();
        }
        // Check local VoiceLab profiles for custom cloned or designed voices
        let clean = alias_or_id.trim().to_lowercase();
        if let Ok(profiles) = crate::voicelab::list_voice_profiles() {
            for p in profiles {
                if p.id.eq_ignore_ascii_case(&clean) || p.name.to_lowercase() == clean {
                    if let Some(voice_id) = p.elevenlabs_voice_id {
                        return voice_id;
                    }
                }
            }
        }
        alias_or_id.trim().to_string()
    }
}

#[async_trait::async_trait]
impl SpeechSynthesizer for ElevenLabsBridge {
    fn voice_name(&self) -> &str {
        &self.voice
    }

    fn max_concurrency(&self) -> usize {
        4 // Up to 4 parallel HTTP calls to ElevenLabs API
    }

    fn resolve_voice_label(&self, requested: Option<&str>) -> String {
        let raw = match requested {
            Some(r) if !r.trim().is_empty() && r != "default" => {
                let trimmed = r.trim();
                if let Some(stripped) = trimmed.strip_prefix("elevenlabs:") {
                    stripped.trim()
                } else if let Some(stripped) = trimmed.strip_prefix("edge:") {
                    stripped.trim()
                } else if let Some(stripped) = trimmed.strip_prefix("piper:") {
                    stripped.trim()
                } else {
                    trimmed
                }
            }
            _ => &self.voice,
        };

        // If raw is generic Edge default like pl-PL-MarekNeural and caller is narrator, map to self.voice
        let voice_to_map = if raw == "pl-PL-MarekNeural"
            && self.voice != "pl-PL-MarekNeural"
            && requested.is_none()
        {
            &self.voice
        } else {
            raw
        };

        let mapped = Self::map_voice_alias(voice_to_map);
        if mapped.starts_with("elevenlabs-") {
            mapped.to_string()
        } else {
            format!("elevenlabs-{}", mapped)
        }
    }

    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()> {
        let trimmed = segment.text.trim();
        if !trimmed.chars().any(|c| c.is_alphanumeric()) {
            return create_silence_wav(out_path, 80, DEFAULT_SAMPLE_RATE);
        }

        let voice_label = self.resolve_voice_label(segment.voice.as_deref());
        let raw_voice = voice_label
            .strip_prefix("elevenlabs-")
            .unwrap_or(&voice_label);
        let actual_voice_id = Self::resolve_voice_id(raw_voice);

        let formatted_text = segment.format_for_elevenlabs();

        let temp_json = out_path.with_extension("req.json");
        let temp_mp3 = out_path.with_extension("temp.mp3");
        let temp_wav = out_path.with_extension("temp.wav");
        let tmp_cfg = out_path.with_extension("curl_cfg.tmp");

        let body = serde_json::json!({
            "text": formatted_text,
            "model_id": self.model_id,
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75
            }
        });

        tokio::fs::write(&temp_json, serde_json::to_vec(&body)?).await?;

        // Securely pass API key via 0600 curl config file so key is never visible in ps / proc
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true).mode(0o600);
            if let Ok(mut f) = opts.open(&tmp_cfg) {
                let _ = writeln!(f, "header = \"xi-api-key: {}\"", self.api_key.trim());
            }
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::write(
                &tmp_cfg,
                format!("header = \"xi-api-key: {}\"\n", self.api_key.trim()),
            );
        }

        let url = format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}?optimize_streaming_latency=1&output_format=mp3_44100_128",
            actual_voice_id
        );
        let curl_res = tokio::process::Command::new("curl")
            .arg("-sS")
            .arg("-f")
            .arg("-X")
            .arg("POST")
            .arg(&url)
            .arg("-K")
            .arg(&tmp_cfg)
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-H")
            .arg("Accept: audio/mpeg")
            .arg("--data")
            .arg(format!("@{}", temp_json.display()))
            .arg("-o")
            .arg(&temp_mp3)
            .output()
            .await;

        let _ = tokio::fs::remove_file(&tmp_cfg).await;
        let _ = tokio::fs::remove_file(&temp_json).await;

        let curl_output = curl_res.context("Nie udało się uruchomić procesu curl")?;

        if !curl_output.status.success() {
            let stderr = String::from_utf8_lossy(&curl_output.stderr);
            let _ = tokio::fs::remove_file(&temp_mp3).await;
            anyhow::bail!("ElevenLabs API call failed: {}", stderr.trim());
        }

        // Adaptive DSP: for ElevenLabs pristine neural output, if no explicit acoustic filter is requested,
        // we skip artificial de-essing/EQ conditioning and let the pure generative audio shine through untouched!
        let filter_chain = if let Some(ref filter_name) = segment.filter {
            let registry = crate::presets::get_active_registry();
            let (preset, _warn) = registry.resolve(filter_name);
            let dsp = preset.to_ffmpeg_filter();
            if dsp.is_empty() {
                "anull".to_string()
            } else {
                dsp
            }
        } else {
            "anull".to_string()
        };

        let conv = tokio::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&temp_mp3)
            .arg("-af")
            .arg(&filter_chain)
            .arg("-ar")
            .arg(DEFAULT_SAMPLE_RATE.to_string())
            .arg("-ac")
            .arg("1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(&temp_wav)
            .output()
            .await?;

        let _ = tokio::fs::remove_file(&temp_mp3).await;

        if !conv.status.success() {
            let stderr = String::from_utf8_lossy(&conv.stderr);
            let _ = tokio::fs::remove_file(&temp_wav).await;
            anyhow::bail!("ffmpeg conversion failed for ElevenLabs audio: {}", stderr);
        }

        tokio::fs::rename(&temp_wav, out_path)
            .await
            .with_context(|| format!("Błąd atomowego zapisu do cache: {:?}", out_path))?;

        Ok(())
    }
}

/// Rendering orchestrator with high-concurrency caching.
pub struct AudioDirectorEngine {
    pub cache_dir: PathBuf,
    pub synthesizer: Arc<dyn SpeechSynthesizer>,
    pub concurrency_override: Option<usize>,
}

impl AudioDirectorEngine {
    pub fn new(cache_dir: PathBuf, synthesizer: Arc<dyn SpeechSynthesizer>) -> Self {
        std::fs::create_dir_all(&cache_dir).ok();
        Self {
            cache_dir,
            synthesizer,
            concurrency_override: None,
        }
    }

    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        if concurrency > 0 {
            self.concurrency_override = Some(concurrency);
        }
        self
    }

    /// Renders a directed script into a single master audio file with parallel async synthesis.
    /// Calculates accurate millisecond chapter boundaries and returns RenderResult.
    pub async fn render(&self, script: &DirectedScript, final_wav: &Path) -> Result<RenderResult> {
        let mut chunk_paths = Vec::new();
        let mut missing_tasks = Vec::new();
        let mut scheduled_hashes = std::collections::HashSet::new();
        let mut scheduled_items = Vec::new();
        let mut ambient_events = Vec::new();
        let mut cache_hits = 0;

        let journal_path = self.cache_dir.join(".audioticon_batch_state.json");
        let book_title = final_wav
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Audioticon_Master");
        let mut journal =
            crate::batch_pipeline::BatchStateJournal::open_or_create(&journal_path, book_title)
                .unwrap_or_default();
        journal.total_segments = script
            .items
            .iter()
            .filter(|i| matches!(i, DirectedItem::Speech(_)))
            .count();
        journal.total_chapters = script
            .items
            .iter()
            .filter(|i| matches!(i, DirectedItem::Chapter(_)))
            .count()
            .max(1);

        let mut chapter_counter = 0usize;

        // Phase 1: Scan timeline, schedule silences, breaths, SFX and identify missing cache segments
        for (idx, item) in script.items.iter().enumerate() {
            match item {
                DirectedItem::Chapter(title) => {
                    chapter_counter += 1;
                    scheduled_items.push((None, Some(title.clone())));
                }
                DirectedItem::Ambience(amb) => {
                    ambient_events.push((scheduled_items.len(), amb.clone()));
                }
                DirectedItem::SoundEffect(sfx_name) => {
                    let sfx_file = self.cache_dir.join(format!(
                        "sfx_{}_{}hz.wav",
                        sanitize_file_name(sfx_name),
                        DEFAULT_SAMPLE_RATE
                    ));
                    if !sfx_file.exists() {
                        create_sfx_wav(sfx_name, &sfx_file, DEFAULT_SAMPLE_RATE)?;
                    }
                    chunk_paths.push(sfx_file.clone());
                    scheduled_items.push((Some(sfx_file), None));
                }
                DirectedItem::Silence(ms) => {
                    let silence_file = self
                        .cache_dir
                        .join(format!("silence_{}ms_{}hz.wav", ms, DEFAULT_SAMPLE_RATE));
                    if !silence_file.exists() {
                        create_silence_wav(&silence_file, *ms, DEFAULT_SAMPLE_RATE)?;
                    }
                    chunk_paths.push(silence_file.clone());
                    scheduled_items.push((Some(silence_file), None));
                }
                DirectedItem::Breath(ms) => {
                    let breath_file = self
                        .cache_dir
                        .join(format!("breath_{}ms_{}hz.wav", ms, DEFAULT_SAMPLE_RATE));
                    if !breath_file.exists() {
                        create_breath_wav(&breath_file, *ms, DEFAULT_SAMPLE_RATE)?;
                    }
                    chunk_paths.push(breath_file.clone());
                    scheduled_items.push((Some(breath_file), None));
                }
                DirectedItem::Speech(segment) => {
                    let voice_name = self
                        .synthesizer
                        .resolve_voice_label(segment.voice.as_deref());
                    let hash = compute_segment_hash(segment, &voice_name);
                    let punch_in_wav = self.cache_dir.join(format!("recorded_{}.wav", hash));
                    let (cached_wav, is_punch_in) = if punch_in_wav.exists() {
                        (punch_in_wav, true)
                    } else {
                        (self.cache_dir.join(format!("{}.wav", hash)), false)
                    };

                    if cached_wav.exists() || journal.is_segment_done(&hash) {
                        cache_hits += 1;
                        journal.mark_segment_done(
                            &hash,
                            chapter_counter.max(1),
                            idx,
                            cached_wav.clone(),
                            0,
                        );
                        if is_punch_in {
                            println!(
                                "  [{:02}] 🎙️ PUNCH-IN HIT: \"{}\" [{}]",
                                idx,
                                segment.text,
                                &hash[0..8]
                            );
                        } else {
                            println!(
                                "  [{:02}] ⚡ CACHE HIT (0ms): \"{}\" [{}]",
                                idx,
                                segment.text,
                                &hash[0..8]
                            );
                        }
                    } else if scheduled_hashes.insert(hash.clone()) {
                        // Deduplicate: only synthesize unique segment hashes once per render session
                        missing_tasks.push((
                            idx,
                            chapter_counter.max(1),
                            segment.clone(),
                            cached_wav.clone(),
                            hash,
                        ));
                    }
                    chunk_paths.push(cached_wav.clone());
                    scheduled_items.push((Some(cached_wav), None));
                }
            }
        }
        let _ = journal.save();

        // Phase 2: Parallel async synthesis of missing segments with crash-resume journaling
        if !missing_tasks.is_empty() {
            let concurrency = self
                .concurrency_override
                .unwrap_or_else(|| self.synthesizer.max_concurrency())
                .max(1);
            let semaphore = Arc::new(Semaphore::new(concurrency));
            let total_missing = missing_tasks.len();
            println!(
                "\n🚀 Generowanie {} segmentów równolegle (współbieżność: {})...",
                total_missing, concurrency
            );

            let mut join_set = tokio::task::JoinSet::new();

            for (idx, ch_id, seg, out_path, hash) in missing_tasks {
                let sem = semaphore.clone();
                let synth = self.synthesizer.clone();

                join_set.spawn(async move {
                    let _permit = sem.acquire().await.expect("Semaphore acquire");
                    println!(
                        "  [{:02}] ⏳ GENEROWANIE: \"{}\" [{}]",
                        idx,
                        seg.text,
                        &hash[0..8]
                    );
                    synth.synthesize(&seg, &out_path).await?;
                    Ok::<(String, usize, usize, PathBuf), anyhow::Error>((
                        hash, ch_id, idx, out_path,
                    ))
                });
            }

            while let Some(res) = join_set.join_next().await {
                let (hash, ch_id, idx, out_path) =
                    res.with_context(|| "Błąd zadania syntezy mowy")??;
                journal.mark_segment_done(&hash, ch_id, idx, out_path, 0);
                let _ = journal.save();
            }
        }

        // Phase 3: Calculate accurate chapter marks & ambient soundscape spans from generated audio duration
        let mut chapters: Vec<ChapterMark> = Vec::new();
        let mut active_chapter: Option<(String, u64)> = None;
        let mut current_ms: u64 = 0;
        let mut item_start_ms: Vec<u64> = Vec::new();
        let mut item_durations_ms: Vec<u64> = Vec::new();

        for (wav_opt, chapter_opt) in &scheduled_items {
            item_start_ms.push(current_ms);
            if let Some(ch_title) = chapter_opt {
                if let Some((prev_title, start)) = active_chapter.take() {
                    chapters.push(ChapterMark {
                        title: prev_title,
                        start_ms: start,
                        end_ms: current_ms,
                    });
                }
                active_chapter = Some((ch_title.clone(), current_ms));
            }
            if let Some(wav_path) = wav_opt {
                if let Ok(reader) = WavReader::open(wav_path) {
                    let dur = (reader.duration() as u64 * 1000) / reader.spec().sample_rate as u64;
                    current_ms += dur;
                    item_durations_ms.push(dur);
                } else {
                    item_durations_ms.push(0);
                }
            }
        }
        item_start_ms.push(current_ms);

        if let Some((title, start)) = active_chapter.take() {
            chapters.push(ChapterMark {
                title,
                start_ms: start,
                end_ms: current_ms,
            });
        }

        if chapters.is_empty() && current_ms > 0 {
            chapters.push(ChapterMark {
                title: "Rozdział 1".to_string(),
                start_ms: 0,
                end_ms: current_ms,
            });
        }

        // Build ambient spans (start_ms, end_ms, name)
        let mut ambient_spans: Vec<(u64, u64, String)> = Vec::new();
        let mut current_amb: Option<(u64, String)> = None;

        for (item_idx, amb_opt) in ambient_events {
            let event_ms = *item_start_ms.get(item_idx).unwrap_or(&current_ms);
            if let Some((start, name)) = current_amb.take() {
                if event_ms > start {
                    ambient_spans.push((start, event_ms, name));
                }
            }
            if let Some(new_name) = amb_opt {
                current_amb = Some((event_ms, new_name));
            }
        }
        if let Some((start, name)) = current_amb.take() {
            if current_ms > start {
                ambient_spans.push((start, current_ms, name));
            }
        }

        // Phase 4: Stitching all verified segments and ambient soundscape mixing
        println!(
            "\nŁączenie {} segmentów audio w plik wynikowy...",
            chunk_paths.len()
        );
        let dialogue_target = if ambient_spans.is_empty() {
            final_wav.to_path_buf()
        } else {
            self.cache_dir
                .join(format!("dialogue_raw_{}.wav", std::process::id()))
        };

        stitch_wav_files(&chunk_paths, &dialogue_target, DEFAULT_SAMPLE_RATE)?;

        if !ambient_spans.is_empty() {
            println!(
                "🌲 Generowanie tła akustycznego (ambience: {} sekwencji, sidechain ducking)...",
                ambient_spans.len()
            );
            let ambient_track = self
                .cache_dir
                .join(format!("ambient_{}.wav", std::process::id()));
            create_ambient_track(
                &ambient_spans,
                current_ms,
                &ambient_track,
                DEFAULT_SAMPLE_RATE,
            )?;
            mix_dialogue_with_ambience(&dialogue_target, &ambient_track, final_wav)?;
            let _ = std::fs::remove_file(&dialogue_target);
            let _ = std::fs::remove_file(&ambient_track);
        }

        println!(
            "Zakończono! Cache hits: {}, Nowo wygenerowane: {}",
            cache_hits,
            chunk_paths.len() - cache_hits
        );
        let subtitles = crate::subtitles::build_subtitles_from_script_and_durations(script, &item_durations_ms);
        let default_v = self.synthesizer.resolve_voice_label(None);
        let waveform = std::fs::read(final_wav)
            .ok()
            .and_then(|bytes| {
                crate::waveform::build_waveform_with_script_segments(
                    &bytes,
                    script,
                    &default_v,
                    &item_durations_ms,
                    &self.cache_dir,
                    512,
                )
                .ok()
            });
        Ok(RenderResult {
            total_segments: chunk_paths.len(),
            duration_ms: current_ms,
            chapters,
            subtitles: Some(subtitles),
            waveform,
        })
    }
}

/// Exports master WAV to M4B (AAC in QuickTime container) with embedded chapters and audiobook metadata.
pub async fn export_to_m4b(
    wav_path: &Path,
    m4b_path: &Path,
    chapters: &[ChapterMark],
    metadata: &AudiobookMetadata,
) -> Result<()> {
    if !check_ffmpeg_installed() {
        anyhow::bail!("Błąd eksportu M4B: {}", ffmpeg_missing_instructions());
    }

    let meta_file = m4b_path.with_extension("ffmeta.txt");
    let mut meta_content = String::from(";FFMETADATA1\n");
    if let Some(ref title) = metadata.title {
        meta_content.push_str(&format!("title={}\n", title));
        meta_content.push_str(&format!("album={}\n", title));
    }
    if let Some(ref author) = metadata.author {
        meta_content.push_str(&format!("artist={}\n", author));
    }
    if let Some(ref narrator) = metadata.narrator {
        meta_content.push_str(&format!("album_artist={}\n", narrator));
        meta_content.push_str(&format!("composer={}\n", narrator));
    }
    if let Some(ref year) = metadata.year {
        meta_content.push_str(&format!("date={}\n", year));
    }
    meta_content.push_str("genre=Audiobook\n\n");

    for ch in chapters {
        meta_content.push_str("[CHAPTER]\n");
        meta_content.push_str("TIMEBASE=1/1000\n");
        meta_content.push_str(&format!("START={}\n", ch.start_ms));
        meta_content.push_str(&format!("END={}\n", ch.end_ms));
        meta_content.push_str(&format!("title={}\n\n", ch.title));
    }

    std::fs::write(&meta_file, &meta_content)
        .with_context(|| format!("Nie można zapisać pliku metadanych: {:?}", meta_file))?;

    let tmp_m4b = m4b_path.with_extension("tmp.m4b");
    let lang_tag = match metadata.language.as_deref() {
        Some("en") | Some("eng") => "language=eng",
        _ => "language=pol",
    };

    let status = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(wav_path)
        .arg("-i")
        .arg(&meta_file)
        .arg("-map_metadata")
        .arg("1")
        .arg("-map")
        .arg("0:a")
        .arg("-af")
        .arg("aresample=44100,loudnorm=I=-19:TP=-3.5:LRA=7,alimiter=limit=0.67:level=false")
        .arg("-ar")
        .arg("44100")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("96k")
        .arg("-metadata:s:a:0")
        .arg(lang_tag)
        .arg("-f")
        .arg("mp4")
        .arg("-brand")
        .arg("M4B ")
        .arg("-movflags")
        .arg("+faststart")
        .arg(&tmp_m4b)
        .status()
        .await
        .with_context(|| "Błąd uruchamiania ffmpeg do konwersji M4B")?;

    let _ = std::fs::remove_file(&meta_file);

    if !status.success() {
        let _ = std::fs::remove_file(&tmp_m4b);
        anyhow::bail!(
            "Konwersja do M4B zakończyła się niepowodzeniem: {:?}",
            status
        );
    }

    std::fs::rename(&tmp_m4b, m4b_path)
        .with_context(|| format!("Nie można przenieść pliku do {:?}", m4b_path))?;

    Ok(())
}

/// Exports master WAV to MP3 with chapters and ID3v2 metadata.
pub async fn export_to_mp3(
    wav_path: &Path,
    mp3_path: &Path,
    chapters: &[ChapterMark],
    metadata: &AudiobookMetadata,
) -> Result<()> {
    if !check_ffmpeg_installed() {
        anyhow::bail!("Błąd eksportu MP3: {}", ffmpeg_missing_instructions());
    }

    let meta_file = mp3_path.with_extension("ffmeta.txt");
    let mut meta_content = String::from(";FFMETADATA1\n");
    if let Some(ref title) = metadata.title {
        meta_content.push_str(&format!("title={}\n", title));
        meta_content.push_str(&format!("album={}\n", title));
    }
    if let Some(ref author) = metadata.author {
        meta_content.push_str(&format!("artist={}\n", author));
    }
    if let Some(ref narrator) = metadata.narrator {
        meta_content.push_str(&format!("album_artist={}\n", narrator));
    }
    if let Some(ref year) = metadata.year {
        meta_content.push_str(&format!("date={}\n", year));
    }
    meta_content.push_str("genre=Audiobook\n\n");

    for ch in chapters {
        meta_content.push_str("[CHAPTER]\n");
        meta_content.push_str("TIMEBASE=1/1000\n");
        meta_content.push_str(&format!("START={}\n", ch.start_ms));
        meta_content.push_str(&format!("END={}\n", ch.end_ms));
        meta_content.push_str(&format!("title={}\n\n", ch.title));
    }

    std::fs::write(&meta_file, &meta_content)
        .with_context(|| format!("Nie można zapisać pliku metadanych: {:?}", meta_file))?;

    let tmp_mp3 = mp3_path.with_extension("tmp.mp3");

    let status = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(wav_path)
        .arg("-i")
        .arg(&meta_file)
        .arg("-map_metadata")
        .arg("1")
        .arg("-af")
        .arg("aresample=44100,loudnorm=I=-19:TP=-3.5:LRA=7,alimiter=limit=0.67:level=false")
        .arg("-ar")
        .arg("44100")
        .arg("-codec:a")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg("192k")
        .arg("-id3v2_version")
        .arg("3")
        .arg(&tmp_mp3)
        .status()
        .await
        .with_context(|| "Błąd uruchamiania ffmpeg do konwersji MP3")?;

    let _ = std::fs::remove_file(&meta_file);

    if !status.success() {
        let _ = std::fs::remove_file(&tmp_mp3);
        anyhow::bail!(
            "Konwersja do MP3 zakończyła się niepowodzeniem: {:?}",
            status
        );
    }

    std::fs::rename(&tmp_mp3, mp3_path)
        .with_context(|| format!("Nie można przenieść pliku do {:?}", mp3_path))?;

    Ok(())
}

/// Exports master WAV to FLAC (Free Lossless Audio Codec) with 24-bit/16-bit precision, embedded chapters, and ACX certification metadata.
pub async fn export_to_flac(
    wav_path: &Path,
    flac_path: &Path,
    chapters: &[ChapterMark],
    metadata: &AudiobookMetadata,
    bit_depth: u32,
) -> Result<()> {
    if !check_ffmpeg_installed() {
        anyhow::bail!("Błąd eksportu FLAC: {}", ffmpeg_missing_instructions());
    }

    let meta_file = flac_path.with_extension("ffmeta.txt");
    let mut meta_content = String::from(";FFMETADATA1\n");
    if let Some(ref title) = metadata.title {
        meta_content.push_str(&format!("title={}\n", title));
        meta_content.push_str(&format!("album={}\n", title));
    }
    if let Some(ref author) = metadata.author {
        meta_content.push_str(&format!("artist={}\n", author));
    }
    if let Some(ref narrator) = metadata.narrator {
        meta_content.push_str(&format!("album_artist={}\n", narrator));
        meta_content.push_str(&format!("composer={}\n", narrator));
    }
    if let Some(ref year) = metadata.year {
        meta_content.push_str(&format!("date={}\n", year));
    }
    meta_content.push_str("genre=Audiobook\n");
    meta_content.push_str("comment=Audioticon Studio Master Lossless (ACX/EBU R128 Compliant)\n\n");

    for ch in chapters {
        meta_content.push_str("[CHAPTER]\n");
        meta_content.push_str("TIMEBASE=1/1000\n");
        meta_content.push_str(&format!("START={}\n", ch.start_ms));
        meta_content.push_str(&format!("END={}\n", ch.end_ms));
        meta_content.push_str(&format!("title={}\n\n", ch.title));
    }

    std::fs::write(&meta_file, &meta_content)
        .with_context(|| format!("Nie można zapisać pliku metadanych: {:?}", meta_file))?;

    let tmp_flac = flac_path.with_extension("tmp.flac");
    let sample_fmt = if bit_depth == 24 { "s32" } else { "s16" };
    let dither_filter = if bit_depth == 24 {
        "aresample=48000:resampler=soxr:precision=28,loudnorm=I=-19:TP=-3.0:LRA=7"
    } else {
        "aresample=44100:resampler=soxr:precision=24,loudnorm=I=-19:TP=-3.0:LRA=7"
    };

    let status = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(wav_path)
        .arg("-i")
        .arg(&meta_file)
        .arg("-map_metadata")
        .arg("1")
        .arg("-map")
        .arg("0:a")
        .arg("-af")
        .arg(dither_filter)
        .arg("-c:a")
        .arg("flac")
        .arg("-sample_fmt")
        .arg(sample_fmt)
        .arg(&tmp_flac)
        .status()
        .await
        .with_context(|| "Błąd uruchamiania ffmpeg do konwersji FLAC")?;

    let _ = std::fs::remove_file(&meta_file);

    if !status.success() {
        let _ = std::fs::remove_file(&tmp_flac);
        anyhow::bail!("Konwersja do FLAC zakończyła się niepowodzeniem: {:?}", status);
    }

    std::fs::rename(&tmp_flac, flac_path)
        .with_context(|| format!("Nie można przenieść pliku do {:?}", flac_path))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_silence_wav() {
        let tmp = std::env::temp_dir().join("test_silence.wav");
        create_silence_wav(&tmp, 200, 24000).expect("Create silence");
        let reader = WavReader::open(&tmp).expect("Read silence");
        assert_eq!(reader.spec().sample_rate, 24000);
        assert_eq!(reader.duration(), 4800); // 200ms * 24 samples/ms
        let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
        let max_amp = samples.iter().map(|&s| s.abs()).max().unwrap_or(0);
        assert!(
            max_amp <= 50,
            "Silence should remain ultra subtle studio room tone: max={}",
            max_amp
        );
        assert!(
            max_amp > 0,
            "Silence should provide room tone instead of digital zero vacuum"
        );
        let _ = std::fs::remove_file(tmp);
    }

    #[test]
    fn test_create_breath_wav() {
        let tmp = std::env::temp_dir().join("test_breath.wav");
        create_breath_wav(&tmp, 350, 24000).expect("Create breath");
        let reader = WavReader::open(&tmp).expect("Read breath");
        assert_eq!(reader.spec().sample_rate, 24000);
        assert_eq!(reader.duration(), 8400); // 350ms * 24 samples/ms
        let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
        // Breath must not be silent, and must have smooth bounded amplitude
        let max_amp = samples.iter().map(|&s| s.abs()).max().unwrap_or(0);
        assert!(
            max_amp > 100,
            "Breath should not be dead silent: max={}",
            max_amp
        );
        assert!(
            max_amp < 15000,
            "Breath should remain subtle and not clip: max={}",
            max_amp
        );
        let _ = std::fs::remove_file(tmp);
    }

    #[test]
    fn test_piper_voice_resolution() {
        let dummy_default = std::env::temp_dir().join("models/pl_PL-darkman-medium.onnx");
        let bridge = PiperBridge::new(dummy_default.clone());

        // Female / Zofia alias should resolve to gosia model
        let female_path = bridge.resolve_model_path(Some("pl-PL-ZofiaNeural"));
        assert!(female_path.to_string_lossy().contains("gosia") || female_path == dummy_default);

        // Explicit provider prefix "piper:pl_PL-gosia-medium"
        let prefixed_path = bridge.resolve_model_path(Some("piper:pl_PL-gosia-medium"));
        assert!(
            prefixed_path.to_string_lossy().contains("gosia") || prefixed_path == dummy_default
        );

        // Male / Marek alias should resolve to darkman or mc_speech model
        let male_path = bridge.resolve_model_path(Some("pl-PL-MarekNeural"));
        assert!(
            male_path.to_string_lossy().contains("darkman")
                || male_path.to_string_lossy().contains("mc_speech")
        );

        // Default should resolve to default model
        let default_path = bridge.resolve_model_path(None);
        assert_eq!(default_path, dummy_default);
    }

    #[tokio::test]
    async fn test_export_to_m4b_synthetic() {
        let tmp_dir = std::env::temp_dir();
        let tmp_wav = tmp_dir.join("test_synth_master.wav");
        let tmp_m4b = tmp_dir.join("test_synth_output.m4b");

        // Create a 1-second synthetic 24kHz mono WAV
        let spec = WavSpec {
            channels: 1,
            sample_rate: 24000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&tmp_wav, spec).expect("Create WAV");
        for t in 0..24000 {
            let sample =
                (3000.0 * (2.0 * std::f32::consts::PI * 440.0 * (t as f32) / 24000.0).sin()) as i16;
            writer.write_sample(sample).expect("Write sample");
        }
        writer.finalize().expect("Finalize WAV");

        let chapters = vec![
            ChapterMark {
                start_ms: 0,
                end_ms: 500,
                title: "Rozdział 1: Start".to_string(),
            },
            ChapterMark {
                start_ms: 500,
                end_ms: 1000,
                title: "Rozdział 2: Finał".to_string(),
            },
        ];
        let meta = AudiobookMetadata {
            title: Some("Synthetic Test Book".to_string()),
            author: Some("Jarosław Kuchta".to_string()),
            narrator: Some("Lektor".to_string()),
            year: Some("2026".to_string()),
            language: Some("pl".to_string()),
        };

        let res = export_to_m4b(&tmp_wav, &tmp_m4b, &chapters, &meta).await;
        let _ = std::fs::remove_file(&tmp_wav);

        assert!(res.is_ok(), "export_to_m4b failed: {:?}", res.err());
        assert!(tmp_m4b.exists());
        let m4b_bytes = std::fs::read(&tmp_m4b).expect("Read m4b");
        let _ = std::fs::remove_file(&tmp_m4b);

        assert!(m4b_bytes.len() > 1000);
        // Verify ISO Base Media File Format and M4B brand
        assert!(
            m4b_bytes.windows(4).any(|w| w == b"ftyp"),
            "M4B must contain ftyp atom"
        );
        assert!(
            m4b_bytes.windows(4).any(|w| w == b"M4B "),
            "M4B must contain M4B brand atom"
        );
    }

    #[test]
    fn test_elevenlabs_voice_resolution_and_aliases() {
        let bridge = ElevenLabsBridge::new("sk_test_mock", "adam");

        // 1. Narrator (requested = None)
        assert_eq!(
            bridge.resolve_voice_label(None),
            "elevenlabs-pNInz6obpgDQGcFmaJgB"
        );

        // 2. Character 👨 (parser maps to "pl-PL-MarekNeural") -> mapped to distinct male character (Josh)
        assert_eq!(
            bridge.resolve_voice_label(Some("pl-PL-MarekNeural")),
            "elevenlabs-TxGEqnHWrfWFTfGW9XjX"
        );

        // 3. Character 👩 (parser maps to "pl-PL-ZofiaNeural") -> mapped to female voice (Rachel)
        assert_eq!(
            bridge.resolve_voice_label(Some("pl-PL-ZofiaNeural")),
            "elevenlabs-21m00Tcm4TlvDq8ikWAM"
        );

        // 4. Named ElevenLabs alias
        assert_eq!(
            bridge.resolve_voice_label(Some("domi")),
            "elevenlabs-AZnzlk1XvdvUeBnXmlld"
        );
        assert_eq!(
            bridge.resolve_voice_label(Some("elevenlabs:antoni")),
            "elevenlabs-ErXwobaYiN019PkySvjV"
        );
        assert_eq!(
            bridge.resolve_voice_label(Some("bzipek")),
            "elevenlabs-JzXwT2R1MhLW7Pq3Q6p"
        );
        assert_eq!(
            bridge.resolve_voice_label(Some("roger")),
            "elevenlabs-CwhRBWXzGAHq8TQ4Fs17"
        );

        // 5. Custom 20-char raw ID preserved
        assert_eq!(
            bridge.resolve_voice_label(Some("custom_voice_id_9999")),
            "elevenlabs-custom_voice_id_9999"
        );
    }

    #[test]
    fn test_stitch_wav_files_crossfade_continuity() {
        let tmp_dir = std::env::temp_dir().join(format!("test_stitch_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let chunk1_path = tmp_dir.join("chunk1.wav");
        let chunk2_path = tmp_dir.join("chunk2.wav");
        let out_path = tmp_dir.join("stitched_out.wav");

        let spec = WavSpec {
            channels: 1,
            sample_rate: 24000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        // Write chunk 1 (2400 samples, ~100ms)
        let mut w1 = WavWriter::create(&chunk1_path, spec).unwrap();
        for t in 0..2400 {
            let s = (10000.0 * (2.0 * std::f32::consts::PI * 440.0 * (t as f32) / 24000.0).sin())
                as i16;
            w1.write_sample(s).unwrap();
        }
        w1.finalize().unwrap();

        // Write chunk 2 (2400 samples, ~100ms)
        let mut w2 = WavWriter::create(&chunk2_path, spec).unwrap();
        for t in 0..2400 {
            let s =
                (8000.0 * (2.0 * std::f32::consts::PI * 880.0 * (t as f32) / 24000.0).sin()) as i16;
            w2.write_sample(s).unwrap();
        }
        w2.finalize().unwrap();

        let res = stitch_wav_files(
            &[chunk1_path.clone(), chunk2_path.clone()],
            &out_path,
            24000,
        );
        assert!(res.is_ok(), "Stitching must succeed: {:?}", res.err());

        let reader = WavReader::open(&out_path).expect("Open stitched WAV");
        assert_eq!(reader.spec().sample_rate, 24000);
        assert_eq!(reader.spec().channels, 1);
        let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();

        // 2400 + 2400 minus crossfade overlap (~120 samples)
        assert!(samples.len() >= 4600 && samples.len() <= 4800);

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_stitch_wav_files_validation_errors() {
        let tmp_dir = std::env::temp_dir().join(format!("test_stitch_err_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let bad_rate_path = tmp_dir.join("bad_rate.wav");
        let out_path = tmp_dir.join("out.wav");

        let bad_spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::create(&bad_rate_path, bad_spec).unwrap();
        w.write_sample(100i16).unwrap();
        w.finalize().unwrap();

        // Attempt to stitch expecting 24000 Hz
        let err = stitch_wav_files(&[bad_rate_path], &out_path, 24000).unwrap_err();
        assert!(
            err.to_string().contains("Niezgodność częstotliwości"),
            "Error must report sample rate mismatch: {}",
            err
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    struct MockParallelSynthesizer {
        active_count: std::sync::atomic::AtomicUsize,
        max_seen: std::sync::atomic::AtomicUsize,
        total_calls: std::sync::atomic::AtomicUsize,
    }

    impl MockParallelSynthesizer {
        fn new() -> Self {
            Self {
                active_count: std::sync::atomic::AtomicUsize::new(0),
                max_seen: std::sync::atomic::AtomicUsize::new(0),
                total_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl SpeechSynthesizer for MockParallelSynthesizer {
        fn voice_name(&self) -> &str {
            "mock-parallel"
        }
        fn max_concurrency(&self) -> usize {
            4
        }
        async fn synthesize(&self, _segment: &TextSegment, out_path: &Path) -> Result<()> {
            use std::sync::atomic::Ordering;
            self.total_calls.fetch_add(1, Ordering::SeqCst);
            let cur = self.active_count.fetch_add(1, Ordering::SeqCst) + 1;
            let mut prev_max = self.max_seen.load(Ordering::SeqCst);
            while cur > prev_max {
                match self.max_seen.compare_exchange_weak(
                    prev_max,
                    cur,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break,
                    Err(actual) => prev_max = actual,
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(15)).await;

            let spec = WavSpec {
                channels: 1,
                sample_rate: DEFAULT_SAMPLE_RATE,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            };
            let mut writer = WavWriter::create(out_path, spec)?;
            for _ in 0..4410 {
                writer.write_sample(500i16)?;
            }
            writer.finalize()?;

            self.active_count.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_parallel_multi_threaded_render_and_deduplication() {
        use std::sync::atomic::Ordering;
        let tmp_dir = std::env::temp_dir().join(format!("test_parallel_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let cache_dir = tmp_dir.join("cache");
        let final_wav = tmp_dir.join("master.wav");

        let mock_synth = Arc::new(MockParallelSynthesizer::new());
        let engine =
            AudioDirectorEngine::new(cache_dir.clone(), mock_synth.clone()).with_concurrency(4);

        let mut script = DirectedScript::new();
        script
            .items
            .push(DirectedItem::Chapter("Rozdział 1".to_string()));
        script.items.push(DirectedItem::Speech(TextSegment {
            text: "Zdanie numer jeden.".to_string(),
            voice: None,
            rate_mod: 0,
            pitch_mod: 0,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
            ..Default::default()
        }));
        script.items.push(DirectedItem::Speech(TextSegment {
            text: "Zdanie numer dwa.".to_string(),
            voice: None,
            rate_mod: 0,
            pitch_mod: 0,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
            ..Default::default()
        }));
        // Duplicate text of segment 1 -> must deduplicate in synthesis!
        script.items.push(DirectedItem::Speech(TextSegment {
            text: "Zdanie numer jeden.".to_string(),
            voice: None,
            rate_mod: 0,
            pitch_mod: 0,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
            ..Default::default()
        }));
        script.items.push(DirectedItem::Silence(100));
        script
            .items
            .push(DirectedItem::Chapter("Rozdział 2".to_string()));
        script.items.push(DirectedItem::Speech(TextSegment {
            text: "Zdanie numer trzy.".to_string(),
            voice: None,
            rate_mod: 0,
            pitch_mod: 0,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
            ..Default::default()
        }));
        script.items.push(DirectedItem::Speech(TextSegment {
            text: "Zdanie numer cztery.".to_string(),
            voice: None,
            rate_mod: 0,
            pitch_mod: 0,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
            ..Default::default()
        }));

        let res = engine.render(&script, &final_wav).await;
        assert!(res.is_ok(), "Render must succeed: {:?}", res.err());
        let render_info = res.unwrap();

        // 5 speech segments + 1 silence = 6 total chunks stitched
        assert_eq!(render_info.total_segments, 6);
        assert_eq!(render_info.chapters.len(), 2);
        assert_eq!(render_info.chapters[0].title, "Rozdział 1");
        assert_eq!(render_info.chapters[1].title, "Rozdział 2");

        // Total unique speech texts: 4 ("jeden", "dwa", "trzy", "cztery").
        // "jeden" appeared twice so total synthesis calls must be 4, NOT 5!
        assert_eq!(mock_synth.total_calls.load(Ordering::SeqCst), 4);

        // Verify parallel execution occurred concurrently
        assert!(
            mock_synth.max_seen.load(Ordering::SeqCst) >= 2,
            "Concurrency must be >= 2 during parallel rendering, was: {}",
            mock_synth.max_seen.load(Ordering::SeqCst)
        );

        // Verify stitched file integrity
        assert!(final_wav.exists());
        let reader = WavReader::open(&final_wav).expect("Open final master WAV");
        assert_eq!(reader.spec().sample_rate, DEFAULT_SAMPLE_RATE);
        assert!(reader.duration() > 0);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_create_sfx_and_ambient_synthesis() {
        let tmp_dir = std::env::temp_dir().join(format!("test_sfx_amb_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);

        // 1. Test SFX synthesis for bell and paper, plus new procedural generators
        let bell_path = tmp_dir.join("test_bell.wav");
        create_sfx_wav("bell", &bell_path, 24000).expect("Create bell SFX");
        assert!(bell_path.exists());
        let bell_reader = WavReader::open(&bell_path).expect("Read bell SFX");
        assert_eq!(bell_reader.spec().sample_rate, 24000);
        assert!(bell_reader.duration() > 0);

        let paper_path = tmp_dir.join("test_paper.wav");
        create_sfx_wav("paper", &paper_path, 24000).expect("Create paper SFX");
        assert!(paper_path.exists());
        let paper_reader = WavReader::open(&paper_path).expect("Read paper SFX");
        assert_eq!(paper_reader.spec().sample_rate, 24000);
        assert!(paper_reader.duration() > 0);

        let shot_path = tmp_dir.join("test_shot.wav");
        create_sfx_wav("strzal", &shot_path, 24000).expect("Create gunshot SFX");
        assert!(shot_path.exists());

        let heart_path = tmp_dir.join("test_heart.wav");
        create_sfx_wav("serce", &heart_path, 24000).expect("Create heartbeat SFX");
        assert!(heart_path.exists());

        let water_path = tmp_dir.join("test_water.wav");
        create_sfx_wav("kropla", &water_path, 24000).expect("Create water droplet SFX");
        assert!(water_path.exists());

        // 2. Test Ambient soundscape generation
        let amb_path = tmp_dir.join("test_amb.wav");
        let spans = vec![
            (0u64, 500u64, "morze".to_string()),
            (500u64, 1000u64, "kawiarnia".to_string()),
            (1000u64, 1500u64, "kosmos".to_string()),
        ];
        create_ambient_track(&spans, 1500, &amb_path, 24000).expect("Create ambient track");
        assert!(amb_path.exists());
        let amb_reader = WavReader::open(&amb_path).expect("Read ambient track");
        assert_eq!(amb_reader.spec().sample_rate, 24000);
        assert_eq!(amb_reader.duration(), 36000); // 1.5s * 24000 = 36000 samples

        // 3. Test mixing dialogue with ambient ducking
        let dialogue_path = tmp_dir.join("test_dialogue.wav");
        create_silence_wav(&dialogue_path, 1500, 24000).expect("Create dialogue dummy");
        let mixed_path = tmp_dir.join("test_mixed.wav");
        mix_dialogue_with_ambience(&dialogue_path, &amb_path, &mixed_path)
            .expect("Mix dialogue with ambience");
        assert!(mixed_path.exists());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[tokio::test]
    async fn test_export_to_flac_synthetic() {
        let tmp_dir = std::env::temp_dir().join(format!("test_flac_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let master_wav = tmp_dir.join("master.wav");
        create_silence_wav(&master_wav, 1000, 24000).expect("Create master dummy");

        let out_flac = tmp_dir.join("book.flac");
        let chapters = vec![
            ChapterMark {
                start_ms: 0,
                end_ms: 500,
                title: "Wstęp".to_string(),
            },
            ChapterMark {
                start_ms: 500,
                end_ms: 1000,
                title: "Finał".to_string(),
            },
        ];
        let meta = AudiobookMetadata {
            title: Some("Master Flac".to_string()),
            author: Some("Jarosław Kuchta".to_string()),
            narrator: Some("Lektor".to_string()),
            year: Some("2026".to_string()),
            language: Some("pl".to_string()),
        };

        let res = export_to_flac(&master_wav, &out_flac, &chapters, &meta, 24).await;
        assert!(res.is_ok(), "FLAC export failed: {:?}", res.err());
        assert!(out_flac.exists());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
