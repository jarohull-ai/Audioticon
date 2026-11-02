use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use tokio::sync::{Mutex, Semaphore};
use crate::cache::compute_segment_hash;
use crate::types::{DirectedItem, DirectedScript, TextSegment};

pub const DEFAULT_SAMPLE_RATE: u32 = 24000;

/// Pure Rust generation of digital silence WAV file.
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
    for _ in 0..total_samples {
        writer.write_sample(0i16)?;
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
        let out = self.b0 * sample + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = sample;
        self.y2 = self.y1;
        self.y1 = out;
        out
    }
}

/// Fast XorShift64 PRNG for deterministic procedural audio noise
struct SimpleNoiseRng(u64);

impl SimpleNoiseRng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0x853c_49e6_748f_ea9b } else { seed })
    }

    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 as f32) / (u64::MAX as f32)) * 2.0 - 1.0
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
    let decay_samples = (total_samples as usize).saturating_sub(attack_samples).max(1);

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
            ((1.0 - t).max(0.0) * std::f32::consts::PI / 2.0).sin().powi(2)
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
                anyhow::bail!("Nieobsługiwana liczba kanałów ({}) w pliku {:?}", other, in_file);
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
    let mut filters = vec![
        "silenceremove=start_periods=1:start_duration=0.005:start_threshold=-45dB:detection=rms".to_string(),
        "areverse".to_string(),
        "silenceremove=start_periods=1:start_duration=0.005:start_threshold=-45dB:detection=rms".to_string(),
        "areverse".to_string(),
    ];

    if let Some(preset) = filter {
        match preset {
            "phone" => {
                filters.push("highpass=f=300,lowpass=f=3400,volume=2.2,acrusher=level_in=1:level_out=0.9:bits=10:mode=lin".to_string());
            }
            "radio" => {
                filters.push("highpass=f=400,lowpass=f=2800,volume=2.2,treble=g=-2".to_string());
            }
            "megaphone" => {
                filters.push("highpass=f=500,lowpass=f=3800,volume=2.0,acrusher=level_in=1:level_out=1:bits=12:mode=lin".to_string());
            }
            "cathedral" => {
                // Add pad duration before aecho so the reverb tail naturally decays without harsh cutoff
                filters.push("apad=pad_dur=0.8,aecho=0.8:0.88:60|120:0.4|0.25".to_string());
            }
            "dungeon" => {
                filters.push("apad=pad_dur=0.6,aecho=0.8:0.75:30|60:0.3|0.2".to_string());
            }
            "thought" => {
                filters.push("lowpass=f=4500,aecho=0.8:0.6:20:0.2".to_string());
            }
            "robot" => {
                filters.push("flanger=delay=5:depth=2:regen=50:width=80:speed=2,acrusher=bits=12:mode=lin".to_string());
            }
            _ => {}
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
        let candidates = [
            "/tmp/edge_env/bin/edge-tts",
            "/usr/local/bin/edge-tts",
            "/usr/bin/edge-tts",
        ];
        for c in candidates {
            if Path::new(c).exists() {
                return PathBuf::from(c);
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

        let status = tokio::process::Command::new(&self.edge_tts_bin)
            .arg(format!("--voice={}", self.voice))
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

/// Offline Local Synthesizer bridge using a persistent Piper worker (100% offline, model kept in RAM).
pub struct PiperBridge {
    pub model_path: PathBuf,
    pub voice_id: String,
    pub python_bin: PathBuf,
    pub worker_script: PathBuf,
    worker: Mutex<Option<PiperWorkerProcess>>,
}

impl PiperBridge {
    pub fn new(model_path: PathBuf) -> Self {
        let name = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("piper_model")
            .to_string();

        let py = Self::find_python();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/piper_worker.py");

        Self {
            model_path,
            voice_id: format!("piper-{}", name),
            python_bin: py,
            worker_script: script,
            worker: Mutex::new(None),
        }
    }

    fn find_python() -> PathBuf {
        let candidates = [
            "/tmp/edge_env/bin/python3",
            "/usr/bin/python3",
            "python3",
        ];
        for c in candidates {
            if Path::new(c).exists() {
                return PathBuf::from(c);
            }
        }
        PathBuf::from("python3")
    }

    fn spawn_worker(&self) -> Result<PiperWorkerProcess> {
        let mut child = std::process::Command::new(&self.python_bin)
            .arg(&self.worker_script)
            .arg(&self.model_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .with_context(|| format!("Błąd startu procesu pomocniczego Piper: {:?}", self.worker_script))?;

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

        // Acquire worker and synthesize in-memory
        {
            let mut worker_guard = self.worker.lock().await;
            if worker_guard.is_none() {
                *worker_guard = Some(self.spawn_worker()?);
            }

            let worker = worker_guard.as_mut().unwrap();
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

        let mut filter_chain = build_ffmpeg_filter_chain(segment.filter.as_deref());

        // Handle pitch modification for Piper via ffmpeg asetrate + atempo
        if segment.pitch_mod != 0 {
            let semitones = segment.pitch_mod as f32 / 3.0;
            let factor = 2f32.powf(semitones / 12.0);
            let pitch_filter = format!("asetrate=24000*{:.3},atempo={:.3}", factor, 1.0 / factor);
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
    pub async fn render(&self, script: &DirectedScript, final_wav: &Path) -> Result<usize> {
        let mut chunk_paths = Vec::new();
        let mut missing_tasks = Vec::new();
        let mut cache_hits = 0;

        // Phase 1: Scan timeline, schedule silences and identify missing cache segments
        for (idx, item) in script.items.iter().enumerate() {
            match item {
                DirectedItem::Silence(ms) => {
                    let silence_file = self.cache_dir.join(format!("silence_{}ms.wav", ms));
                    if !silence_file.exists() {
                        create_silence_wav(&silence_file, *ms, DEFAULT_SAMPLE_RATE)?;
                    }
                    chunk_paths.push(silence_file);
                }
                DirectedItem::Breath(ms) => {
                    let breath_file = self.cache_dir.join(format!("breath_{}ms.wav", ms));
                    if !breath_file.exists() {
                        create_breath_wav(&breath_file, *ms, DEFAULT_SAMPLE_RATE)?;
                    }
                    chunk_paths.push(breath_file);
                }
                DirectedItem::Speech(segment) => {
                    let hash = compute_segment_hash(segment, self.synthesizer.voice_name());
                    let cached_wav = self.cache_dir.join(format!("{}.wav", hash));

                    if cached_wav.exists() {
                        cache_hits += 1;
                        println!("  [{:02}] ⚡ CACHE HIT (0ms): \"{}\" [{}]", idx, segment.text, &hash[0..8]);
                    } else {
                        missing_tasks.push((idx, segment.clone(), cached_wav.clone(), hash));
                    }
                    chunk_paths.push(cached_wav);
                }
            }
        }

        // Phase 2: Parallel async synthesis of missing segments
        if !missing_tasks.is_empty() {
            let concurrency = self.synthesizer.max_concurrency();
            let semaphore = Arc::new(Semaphore::new(concurrency));
            let total_missing = missing_tasks.len();
            println!("\n🚀 Generowanie {} segmentów równolegle (współbieżność: {})...", total_missing, concurrency);

            let mut join_set = tokio::task::JoinSet::new();

            for (idx, seg, out_path, hash) in missing_tasks {
                let sem = semaphore.clone();
                let synth = self.synthesizer.clone();

                join_set.spawn(async move {
                    let _permit = sem.acquire().await.expect("Semaphore acquire");
                    println!("  [{:02}] ⏳ GENEROWANIE: \"{}\" [{}]", idx, seg.text, &hash[0..8]);
                    synth.synthesize(&seg, &out_path).await?;
                    Ok::<(), anyhow::Error>(())
                });
            }

            while let Some(res) = join_set.join_next().await {
                res.with_context(|| "Błąd zadania syntezy mowy")??;
            }
        }

        // Phase 3: Stitching all verified segments
        println!("\nŁączenie {} segmentów audio w plik wynikowy...", chunk_paths.len());
        stitch_wav_files(&chunk_paths, final_wav, DEFAULT_SAMPLE_RATE)?;

        println!("Zakończono! Cache hits: {}, Nowo wygenerowane: {}", cache_hits, chunk_paths.len() - cache_hits);
        Ok(chunk_paths.len())
    }
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
        assert!(samples.iter().all(|&s| s == 0));
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
        assert!(max_amp > 100, "Breath should not be dead silent: max={}", max_amp);
        assert!(max_amp < 15000, "Breath should remain subtle and not clip: max={}", max_amp);
        let _ = std::fs::remove_file(tmp);
    }
}
