use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
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
                filters.push("aecho=0.8:0.88:60|120:0.4|0.25".to_string());
            }
            "dungeon" => {
                filters.push("aecho=0.8:0.75:30|60:0.3|0.2".to_string());
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

/// Offline Local Synthesizer bridge using Piper TTS (ONNX, 100% offline).
pub struct PiperBridge {
    pub model_path: PathBuf,
    pub voice_id: String,
    pub piper_bin: PathBuf,
}

impl PiperBridge {
    pub fn new(model_path: PathBuf) -> Self {
        let name = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("piper_model")
            .to_string();

        let bin = Self::find_piper();
        Self {
            model_path,
            voice_id: format!("piper-{}", name),
            piper_bin: bin,
        }
    }

    fn find_piper() -> PathBuf {
        let candidates = [
            "/tmp/edge_env/bin/piper",
            "/usr/local/bin/piper",
            "/usr/bin/piper",
        ];
        for c in candidates {
            if Path::new(c).exists() {
                return PathBuf::from(c);
            }
        }
        PathBuf::from("piper")
    }
}

#[async_trait::async_trait]
impl SpeechSynthesizer for PiperBridge {
    fn voice_name(&self) -> &str {
        &self.voice_id
    }

    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()> {
        let temp_txt = out_path.with_extension("txt");
        tokio::fs::write(&temp_txt, segment.text.trim()).await?;

        let temp_raw_wav = out_path.with_extension("piper_raw.wav");
        let temp_wav = out_path.with_extension("temp.wav");

        // Map rate_mod to piper length_scale:
        // normal is 1.0; negative rate_mod slows down (length_scale > 1.0); positive accelerates
        let length_scale = if segment.rate_mod < 0 {
            1.0 - (segment.rate_mod as f32 / 100.0)
        } else {
            1.0 / (1.0 + (segment.rate_mod as f32 / 100.0))
        };

        // Map volume_mod to piper volume multiplier
        let volume_scale = (1.0 + (segment.volume_mod as f32 / 100.0)).clamp(0.2, 2.0);

        let status = tokio::process::Command::new(&self.piper_bin)
            .arg("--model")
            .arg(&self.model_path)
            .arg("--input-file")
            .arg(&temp_txt)
            .arg("--output-file")
            .arg(&temp_raw_wav)
            .arg("--length-scale")
            .arg(format!("{:.2}", length_scale))
            .arg("--volume")
            .arg(format!("{:.2}", volume_scale))
            .status()
            .await
            .with_context(|| format!("Nie można uruchomić {:?}", self.piper_bin))?;

        if !status.success() {
            let _ = tokio::fs::remove_file(&temp_txt).await;
            anyhow::bail!("piper zakończył się błędem: {:?}", status);
        }

        let filter_chain = build_ffmpeg_filter_chain(segment.filter.as_deref());

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

        let _ = tokio::fs::remove_file(&temp_txt).await;
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

/// Rendering orchestrator with granular caching.
pub struct AudioDirectorEngine {
    pub cache_dir: PathBuf,
    pub synthesizer: Box<dyn SpeechSynthesizer>,
}

impl AudioDirectorEngine {
    pub fn new(cache_dir: PathBuf, synthesizer: Box<dyn SpeechSynthesizer>) -> Self {
        std::fs::create_dir_all(&cache_dir).ok();
        Self {
            cache_dir,
            synthesizer,
        }
    }

    /// Renders a directed script into a single master audio file.
    pub async fn render(&self, script: &DirectedScript, final_wav: &Path) -> Result<usize> {
        let mut chunk_paths = Vec::new();
        let mut cache_hits = 0;
        let mut cache_misses = 0;

        for (idx, item) in script.items.iter().enumerate() {
            match item {
                DirectedItem::Silence(ms) => {
                    let silence_file = self.cache_dir.join(format!("silence_{}ms.wav", ms));
                    if !silence_file.exists() {
                        create_silence_wav(&silence_file, *ms, DEFAULT_SAMPLE_RATE)?;
                    }
                    chunk_paths.push(silence_file);
                }
                DirectedItem::Speech(segment) => {
                    let hash = compute_segment_hash(segment, self.synthesizer.voice_name());
                    let cached_wav = self.cache_dir.join(format!("{}.wav", hash));

                    if cached_wav.exists() {
                        cache_hits += 1;
                        println!("  [{:02}] ⚡ CACHE HIT (0ms): \"{}\" [{}]", idx, segment.text, &hash[0..8]);
                    } else {
                        cache_misses += 1;
                        println!("  [{:02}] ⏳ GENEROWANIE: \"{}\" [{}]", idx, segment.text, &hash[0..8]);
                        self.synthesizer.synthesize(segment, &cached_wav).await?;
                    }
                    chunk_paths.push(cached_wav);
                }
            }
        }

        println!("\nŁączenie {} segmentów audio w plik wynikowy...", chunk_paths.len());
        stitch_wav_files(&chunk_paths, final_wav, DEFAULT_SAMPLE_RATE)?;

        println!("Zakończono! Cache hits: {}, Nowo wygenerowane: {}", cache_hits, cache_misses);
        Ok(chunk_paths.len())
    }
}
