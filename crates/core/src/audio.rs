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
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(out_path, spec)
        .with_context(|| format!("Nie można utworzyć pliku ciszy: {:?}", out_path))?;

    let total_samples = (sample_rate as u64 * duration_ms as u64) / 1000;
    for _ in 0..total_samples {
        writer.write_sample(0i16)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Pure Rust seamless sample-accurate stitching of multiple WAV files into one.
pub fn stitch_wav_files(inputs: &[PathBuf], out_path: &Path, sample_rate: u32) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(out_path, spec)
        .with_context(|| format!("Nie można utworzyć pliku wynikowego: {:?}", out_path))?;

    for in_file in inputs {
        let file = File::open(in_file)
            .with_context(|| format!("Błąd otwarcia pliku składowego: {:?}", in_file))?;
        let mut reader = WavReader::new(BufReader::new(file))
            .with_context(|| format!("Błąd odczytu nagłówka WAV: {:?}", in_file))?;

        for sample in reader.samples::<i16>() {
            let s = sample?;
            writer.write_sample(s)?;
        }
    }
    writer.finalize()?;
    Ok(())
}

/// Trait for TTS synthesizers.
#[async_trait::async_trait]
pub trait SpeechSynthesizer: Send + Sync {
    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()>;
    fn voice_name(&self) -> &str;
}

/// Synthesizer bridge using edge-tts.
pub struct EdgeTtsBridge {
    pub voice: String,
    pub python_bin: PathBuf,
}

impl EdgeTtsBridge {
    pub fn new(voice: impl Into<String>) -> Self {
        let py = if Path::new("/tmp/edge_env/bin/python3").exists() {
            PathBuf::from("/tmp/edge_env/bin/python3")
        } else {
            PathBuf::from("python3")
        };
        Self {
            voice: voice.into(),
            python_bin: py,
        }
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

        // Python argparse requires --arg=val syntax for negative numbers starting with '-'
        let rate_arg = format!("--rate={:+}%", segment.rate_mod);
        let pitch_arg = format!("--pitch={:+}Hz", segment.pitch_mod);
        let vol_arg = format!("--volume={:+}%", segment.volume_mod);

        let temp_mp3 = out_path.with_extension("temp.mp3");

        let status = tokio::process::Command::new("/tmp/edge_env/bin/edge-tts")
            .arg(format!("--voice={}", self.voice))
            .arg(rate_arg)
            .arg(pitch_arg)
            .arg(vol_arg)
            .arg(format!("-f={}", temp_txt.display()))
            .arg(format!("--write-media={}", temp_mp3.display()))
            .status()
            .await?;

        if !status.success() {
            anyhow::bail!("edge-tts zakończył się błędem: {:?}", status);
        }

        // Convert mp3 chunk to 24000Hz mono wav
        let conv = tokio::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&temp_mp3)
            .arg("-ar")
            .arg(DEFAULT_SAMPLE_RATE.to_string())
            .arg("-ac")
            .arg("1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(out_path)
            .output()
            .await?;

        if !conv.status.success() {
            anyhow::bail!("ffmpeg conversion failed");
        }

        let _ = tokio::fs::remove_file(temp_txt).await;
        let _ = tokio::fs::remove_file(temp_mp3).await;
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
