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

pub const DEFAULT_SAMPLE_RATE: u32 = 24000;

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

/// Pure Rust generation of studio room-tone silence WAV file.
/// Instead of digital dead-zero vacuum (-inf dB), generates an ultra-subtle
/// studio ambient floor at ~ -68 dBFS, completely eliminating headphone gating sensations.
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

    let total_samples = (sample_rate as u64 * duration_ms as u64) / 1000;
    let mut rng = SimpleNoiseRng::new(0x71e9_3a11_c001_cafe ^ (duration_ms as u64));
    let mut lp_state = 0.0f32;

    for _ in 0..total_samples {
        let white = rng.next_f32();
        // Warm lowpass around 800Hz for subtle room tone
        lp_state += 0.15 * (white - lp_state);
        let sample_val = (lp_state * 10.0).clamp(-32768.0, 32767.0) as i16;
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

        let gain = 0.042; // ~ -27.5 dBFS subtle lektor breath
        let val = (colored_noise * env * gain * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(val)?;
    }
    writer.finalize()?;

    // Atomic move
    std::fs::rename(&tmp_path, out_path)
        .with_context(|| format!("Nie można przenieść pliku oddechu do: {:?}", out_path))?;

    Ok(())
}

/// Pure Rust seamless sample-accurate stitching of multiple WAV files into one.
/// Validates headers, enforces bit depth & sample rate, supports mono downmixing,
/// and applies a seamless 5ms equal-power crossfade between consecutive chunks to prevent audible clicks.
pub fn stitch_wav_files(inputs: &[PathBuf], out_path: &Path, sample_rate: u32) -> Result<()> {
    let tmp_out = out_path.with_extension("tmp.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&tmp_out, spec)
        .with_context(|| format!("Nie można utworzyć pliku wynikowego: {:?}", tmp_out))?;

    // 5ms crossfade window (e.g. 120 samples at 24kHz)
    let default_xfade_len = ((sample_rate as f32) * 0.005).round() as usize;
    let mut pending_tail: Vec<i16> = Vec::new();

    for in_file in inputs {
        let file = File::open(in_file)
            .with_context(|| format!("Błąd otwarcia pliku składowego: {:?}", in_file))?;
        let mut reader = WavReader::new(BufReader::new(file))
            .with_context(|| format!("Błąd odczytu nagłówka WAV: {:?}", in_file))?;

        let in_spec = reader.spec();
        if in_spec.sample_rate != sample_rate {
            anyhow::bail!(
                "Niezgodność częstotliwości próbkowania w {:?}: oczekiwano {} Hz, otrzymano {} Hz",
                in_file,
                sample_rate,
                in_spec.sample_rate
            );
        }

        if in_spec.bits_per_sample != 16 || in_spec.sample_format != SampleFormat::Int {
            anyhow::bail!(
                "Niezgodny format audio w {:?}: wymagane 16-bit PCM (Int), otrzymano {}-bit {:?}",
                in_file,
                in_spec.bits_per_sample,
                in_spec.sample_format
            );
        }

        let mut chunk_samples: Vec<i16> = match in_spec.channels {
            1 => {
                let mut v = Vec::with_capacity(reader.duration() as usize);
                for sample in reader.samples::<i16>() {
                    v.push(sample?);
                }
                v
            }
            2 => {
                let mut v = Vec::with_capacity(reader.duration() as usize);
                let mut s_iter = reader.samples::<i16>();
                while let Some(left) = s_iter.next() {
                    let right = s_iter.next().unwrap_or(Ok(0))?;
                    let mono = ((left? as i32 + right as i32) / 2) as i16;
                    v.push(mono);
                }
                v
            }
            other => {
                anyhow::bail!(
                    "Nieobsługiwana liczba kanałów ({}) w pliku {:?}",
                    other,
                    in_file
                );
            }
        };

        if chunk_samples.is_empty() {
            continue;
        }

        if pending_tail.is_empty() {
            // First non-empty chunk: apply micro fade-in at the start
            let head_fade = default_xfade_len.min(chunk_samples.len() / 2);
            for (i, sample) in chunk_samples.iter_mut().enumerate().take(head_fade) {
                let factor = ((i as f32 / head_fade as f32) * std::f32::consts::PI / 2.0).sin();
                *sample = ((*sample as f32) * factor).round() as i16;
            }

            let xfade = default_xfade_len.min(chunk_samples.len() / 4);
            if xfade > 0 && chunk_samples.len() > xfade {
                let split_idx = chunk_samples.len() - xfade;
                for &s in &chunk_samples[..split_idx] {
                    writer.write_sample(s)?;
                }
                pending_tail = chunk_samples[split_idx..].to_vec();
            } else {
                for &s in &chunk_samples {
                    writer.write_sample(s)?;
                }
                pending_tail.clear();
            }
        } else {
            // Crossfade with pending tail from previous chunk
            let xfade = pending_tail.len().min(chunk_samples.len() / 2);
            for i in 0..xfade {
                let t = i as f32 / xfade as f32;
                let fade_out = ((1.0 - t) * std::f32::consts::PI / 2.0).sin();
                let fade_in = (t * std::f32::consts::PI / 2.0).sin();
                let s_prev = (pending_tail[i] as f32) * fade_out;
                let s_curr = (chunk_samples[i] as f32) * fade_in;
                let mixed = (s_prev + s_curr).round().clamp(-32768.0, 32767.0) as i16;
                writer.write_sample(mixed)?;
            }

            if pending_tail.len() > xfade {
                for &s in &pending_tail[xfade..] {
                    writer.write_sample(s)?;
                }
            }

            let remaining_chunk = &chunk_samples[xfade..];
            let next_xfade = default_xfade_len.min(remaining_chunk.len() / 4);
            if next_xfade > 0 && remaining_chunk.len() > next_xfade {
                let split_idx = remaining_chunk.len() - next_xfade;
                for &s in &remaining_chunk[..split_idx] {
                    writer.write_sample(s)?;
                }
                pending_tail = remaining_chunk[split_idx..].to_vec();
            } else {
                for &s in remaining_chunk {
                    writer.write_sample(s)?;
                }
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
            *sample = ((*sample as f32) * factor).round() as i16;
        }
        for s in pending_tail {
            writer.write_sample(s)?;
        }
    }

    writer.finalize()?;

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
        let temp_txt = out_path.with_extension("txt");
        tokio::fs::write(&temp_txt, segment.text.trim()).await?;

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

        let rate_arg = format!("--rate={:+}%", rate_mod);
        let pitch_arg = format!("--pitch={:+}Hz", pitch_mod);
        let vol_arg = format!("--volume={:+}%", segment.volume_mod);

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

        let filter_chain = build_ffmpeg_filter_chain(segment.filter.as_deref());

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
        let pid = std::process::id();
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let temp_raw_wav = out_path.with_extension(format!("{}_{}.piper_raw.wav", pid, unique_id));
        let temp_wav = out_path.with_extension(format!("{}_{}.temp.wav", pid, unique_id));

        let length_scale = if segment.rate_mod < 0 {
            1.0 - (segment.rate_mod as f32 / 100.0)
        } else {
            1.0 / (1.0 + (segment.rate_mod as f32 / 100.0))
        };
        let volume_scale = (1.0 + (segment.volume_mod as f32 / 100.0)).clamp(0.2, 2.0);

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

        let mut filter_chain = build_ffmpeg_filter_chain(segment.filter.as_deref());

        // Handle pitch modification for Piper via ffmpeg asetrate + atempo
        if segment.pitch_mod != 0 {
            let semitones = segment.pitch_mod as f32 / 3.0;
            let factor = 2f32.powf(semitones / 12.0);
            let pitch_filter = format!(
                "asetrate={:.0}*{:.4},aresample={},atempo={:.4}",
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
            model_id: "eleven_multilingual_v2".to_string(),
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
        let voice_label = self.resolve_voice_label(segment.voice.as_deref());
        let raw_voice = voice_label
            .strip_prefix("elevenlabs-")
            .unwrap_or(&voice_label);
        let actual_voice_id = Self::map_voice_alias(raw_voice);

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
            "https://api.elevenlabs.io/v1/text-to-speech/{}",
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

        let filter_chain = build_ffmpeg_filter_chain(segment.filter.as_deref());

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
        let mut cache_hits = 0;

        // Phase 1: Scan timeline, schedule silences and identify missing cache segments
        for (idx, item) in script.items.iter().enumerate() {
            match item {
                DirectedItem::Chapter(title) => {
                    scheduled_items.push((None, Some(title.clone())));
                }
                DirectedItem::Silence(ms) => {
                    let silence_file = self.cache_dir.join(format!("silence_{}ms.wav", ms));
                    if !silence_file.exists() {
                        create_silence_wav(&silence_file, *ms, DEFAULT_SAMPLE_RATE)?;
                    }
                    chunk_paths.push(silence_file.clone());
                    scheduled_items.push((Some(silence_file), None));
                }
                DirectedItem::Breath(ms) => {
                    let breath_file = self.cache_dir.join(format!("breath_{}ms.wav", ms));
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

                    if cached_wav.exists() {
                        cache_hits += 1;
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
                        missing_tasks.push((idx, segment.clone(), cached_wav.clone(), hash));
                    }
                    chunk_paths.push(cached_wav.clone());
                    scheduled_items.push((Some(cached_wav), None));
                }
            }
        }

        // Phase 2: Parallel async synthesis of missing segments
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

            for (idx, seg, out_path, hash) in missing_tasks {
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
                    Ok::<(), anyhow::Error>(())
                });
            }

            while let Some(res) = join_set.join_next().await {
                res.with_context(|| "Błąd zadania syntezy mowy")??;
            }
        }

        // Phase 3: Calculate accurate chapter marks from generated audio duration
        let mut chapters: Vec<ChapterMark> = Vec::new();
        let mut active_chapter: Option<(String, u64)> = None;
        let mut current_ms: u64 = 0;

        for (wav_opt, chapter_opt) in scheduled_items {
            if let Some(ch_title) = chapter_opt {
                if let Some((prev_title, start)) = active_chapter.take() {
                    chapters.push(ChapterMark {
                        title: prev_title,
                        start_ms: start,
                        end_ms: current_ms,
                    });
                }
                active_chapter = Some((ch_title, current_ms));
            }
            if let Some(wav_path) = wav_opt {
                if let Ok(reader) = WavReader::open(&wav_path) {
                    let dur = (reader.duration() as u64 * 1000) / reader.spec().sample_rate as u64;
                    current_ms += dur;
                }
            }
        }

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

        // Phase 4: Stitching all verified segments into master WAV
        println!(
            "\nŁączenie {} segmentów audio w plik wynikowy...",
            chunk_paths.len()
        );
        stitch_wav_files(&chunk_paths, final_wav, DEFAULT_SAMPLE_RATE)?;

        println!(
            "Zakończono! Cache hits: {}, Nowo wygenerowane: {}",
            cache_hits,
            chunk_paths.len() - cache_hits
        );
        Ok(RenderResult {
            total_segments: chunk_paths.len(),
            duration_ms: current_ms,
            chapters,
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
        .arg("aresample=44100,loudnorm=I=-19:TP=-3.5:LRA=7,alimiter=limit=0.55:level=false")
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
        .arg("aresample=44100,loudnorm=I=-19:TP=-3.5:LRA=7,alimiter=limit=0.55:level=false")
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
                sample_rate: 24000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            };
            let mut writer = WavWriter::create(out_path, spec)?;
            for _ in 0..2400 {
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
        }));
        script.items.push(DirectedItem::Speech(TextSegment {
            text: "Zdanie numer dwa.".to_string(),
            voice: None,
            rate_mod: 0,
            pitch_mod: 0,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
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
        }));
        script.items.push(DirectedItem::Speech(TextSegment {
            text: "Zdanie numer cztery.".to_string(),
            voice: None,
            rate_mod: 0,
            pitch_mod: 0,
            volume_mod: 0,
            filter: None,
            actor_tags: vec![],
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
        assert_eq!(reader.spec().sample_rate, 24000);
        assert!(reader.duration() > 0);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
