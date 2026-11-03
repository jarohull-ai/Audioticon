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
/// Validates headers and supports mono downmixing if needed.
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

        match in_spec.channels {
            1 => {
                for sample in reader.samples::<i16>() {
                    writer.write_sample(sample?)?;
                }
            }
            2 => {
                // Downmix stereo to mono
                let mut samples = reader.samples::<i16>();
                while let Some(left) = samples.next() {
                    let right = samples.next().unwrap_or(Ok(0))?;
                    let mono = ((left? as i32 + right as i32) / 2) as i16;
                    writer.write_sample(mono)?;
                }
            }
            other => {
                anyhow::bail!(
                    "Nieobsługiwana liczba kanałów ({}) w pliku {:?}",
                    other,
                    in_file
                );
            }
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

    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()> {
        let temp_txt = out_path.with_extension("txt");
        tokio::fs::write(&temp_txt, segment.text.trim()).await?;

        let rate_arg = format!("--rate={:+}%", segment.rate_mod);
        let pitch_arg = format!("--pitch={:+}Hz", segment.pitch_mod);
        let vol_arg = format!("--volume={:+}%", segment.volume_mod);

        let temp_mp3 = out_path.with_extension("temp.mp3");
        let temp_wav = out_path.with_extension("temp.wav");

        let voice_to_use = segment.voice.as_deref().unwrap_or(&self.voice);
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
    _process: std::process::Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

const PIPER_WORKER_CODE: &str = include_str!("piper_worker.py");

/// Offline Local Synthesizer bridge using a persistent Piper worker (100% offline, model kept in RAM).
pub struct PiperBridge {
    pub default_model_path: PathBuf,
    pub voice_id: String,
    pub python_bin: PathBuf,
    pub worker_script: PathBuf,
    workers: Mutex<HashMap<PathBuf, PiperWorkerProcess>>,
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
            _process: child,
            stdin,
            reader,
        })
    }

    /// Resolves character voice strings or aliases to on-disk Piper ONNX model paths
    pub fn resolve_model_path(&self, requested_voice: Option<&str>) -> PathBuf {
        let voice_str = match requested_voice {
            Some(v) if !v.trim().is_empty() && v != "default" => v.trim().to_lowercase(),
            _ => return self.default_model_path.clone(),
        };

        // 1. Direct model lookup via models catalog
        if let Some(item) = crate::models::find_model(&voice_str) {
            if let Some(ref p) = item.onnx_path {
                if p.exists() {
                    return p.clone();
                }
            }
            let p = crate::models::models_dir().join(format!("{}.onnx", item.id));
            if p.exists() {
                return p;
            }
        }

        // 2. Mapping from Edge neural voice names or actor tags to Piper models
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

    fn max_concurrency(&self) -> usize {
        1 // Serialized access to in-memory model worker (runs in ~100ms per sentence)
    }

    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()> {
        let temp_raw_wav = out_path.with_extension("piper_raw.wav");
        let temp_wav = out_path.with_extension("temp.wav");

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

        // Acquire worker and synthesize in-memory
        {
            let mut workers_guard = self.workers.lock().await;
            if !workers_guard.contains_key(&target_model_path) {
                let worker = self.spawn_worker_for(&target_model_path)?;
                workers_guard.insert(target_model_path.clone(), worker);
            }

            let worker = workers_guard.get_mut(&target_model_path).unwrap();
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
        }

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

/// Rendering orchestrator with high-concurrency caching.
pub struct AudioDirectorEngine {
    pub cache_dir: PathBuf,
    pub synthesizer: Arc<dyn SpeechSynthesizer>,
}

impl AudioDirectorEngine {
    pub fn new(cache_dir: PathBuf, synthesizer: Arc<dyn SpeechSynthesizer>) -> Self {
        std::fs::create_dir_all(&cache_dir).ok();
        Self {
            cache_dir,
            synthesizer,
        }
    }

    /// Renders a directed script into a single master audio file with parallel async synthesis.
    /// Calculates accurate millisecond chapter boundaries and returns RenderResult.
    pub async fn render(&self, script: &DirectedScript, final_wav: &Path) -> Result<RenderResult> {
        let mut chunk_paths = Vec::new();
        let mut missing_tasks = Vec::new();
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
                    let voice_name = segment
                        .voice
                        .as_deref()
                        .unwrap_or_else(|| self.synthesizer.voice_name());
                    let hash = compute_segment_hash(segment, voice_name);
                    let cached_wav = self.cache_dir.join(format!("{}.wav", hash));

                    if cached_wav.exists() {
                        cache_hits += 1;
                        println!(
                            "  [{:02}] ⚡ CACHE HIT (0ms): \"{}\" [{}]",
                            idx,
                            segment.text,
                            &hash[0..8]
                        );
                    } else {
                        missing_tasks.push((idx, segment.clone(), cached_wav.clone(), hash));
                    }
                    chunk_paths.push(cached_wav.clone());
                    scheduled_items.push((Some(cached_wav), None));
                }
            }
        }

        // Phase 2: Parallel async synthesis of missing segments
        if !missing_tasks.is_empty() {
            let concurrency = self.synthesizer.max_concurrency();
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
}
