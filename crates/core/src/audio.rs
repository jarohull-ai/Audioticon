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
        // Fallback to PATH lookup
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

        // Construct ffmpeg audio filter chain:
        // 1. Remove leading and trailing silence padding introduced by edge-tts/MP3 encoder delay
        let mut filters = vec![
            "silenceremove=start_periods=1:start_duration=0.005:start_threshold=-45dB:detection=rms".to_string(),
            "areverse".to_string(),
            "silenceremove=start_periods=1:start_duration=0.005:start_threshold=-45dB:detection=rms".to_string(),
            "areverse".to_string(),
        ];

        // 2. Apply DSP filter effects if requested by emoji
        if let Some(ref filter_name) = segment.filter {
            match filter_name.as_str() {
                "phone" => {
                    filters.push("bandpass=f=1850:w=3100,acrusher=level_in=1:level_out=0.8:bits=8:mode=log".to_string());
                }
                "radio" => {
                    filters.push("bandpass=f=1800:w=2600,volume=1.2,treble=g=-3".to_string());
                }
                "megaphone" => {
                    filters.push("highpass=f=500,lowpass=f=4000,acrusher=level_in=1:level_out=1:bits=10:mode=lin,volume=1.3".to_string());
                }
                _ => {}
            }
        }

        let filter_chain = filters.join(",");

        // Convert mp3 to 24000Hz mono wav with silence trimming and DSP filter applied
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

        // Cleanup temporary raw files
        let _ = tokio::fs::remove_file(&temp_txt).await;
        let _ = tokio::fs::remove_file(&temp_mp3).await;

        if !conv.status.success() {
            let stderr = String::from_utf8_lossy(&conv.stderr);
            let _ = tokio::fs::remove_file(&temp_wav).await;
            anyhow::bail!("ffmpeg conversion failed: {}", stderr);
        }

        // Atomic commit to cache
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
