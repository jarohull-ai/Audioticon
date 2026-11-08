//! Disk-Backed PCM Audio Streaming for Audioticon
//!
//! Prevents Out-Of-Memory (OOM) heap exhaustion when synthesizing or mastering
//! long-form audiobooks (e.g. 10h+ novels containing > 2 GB of 32-bit float PCM).
//!
//! Rather than keeping millions of audio samples in RAM, `PcmStreamFile` maintains
//! a fast, seekable, disk-backed raw binary PCM container (little-endian float32 or int16),
//! processing and exporting audio in streaming 64 KB memory-efficient blocks.

use anyhow::{Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const DEFAULT_STREAM_BUFFER_SAMPLES: usize = 16384; // 64 KB block

/// Disk-backed mono float32 PCM stream container.
#[derive(Debug)]
pub struct PcmStreamFile {
    pub path: PathBuf,
    pub sample_rate: u32,
}

impl PcmStreamFile {
    /// Creates a new or truncates an existing disk-backed PCM stream file.
    pub fn create(path: impl AsRef<Path>, sample_rate: u32) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        if let Some(parent) = p.parent() {
            create_dir_all(parent)?;
        }
        // Create/truncate file
        File::create(&p).with_context(|| format!("Nie można utworzyć strumienia PCM: {:?}", p))?;
        Ok(Self {
            path: p,
            sample_rate,
        })
    }

    /// Opens an existing disk-backed PCM stream file for reading and appending.
    pub fn open(path: impl AsRef<Path>, sample_rate: u32) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        if !p.exists() {
            anyhow::bail!("Plik strumienia PCM nie istnieje: {:?}", p);
        }
        Ok(Self {
            path: p,
            sample_rate,
        })
    }

    /// Returns the total number of audio samples (frames) currently on disk.
    pub fn total_samples(&self) -> Result<u64> {
        let meta = std::fs::metadata(&self.path)?;
        Ok(meta.len() / 4) // 4 bytes per 32-bit float
    }

    /// Returns the audio duration in seconds.
    pub fn duration_seconds(&self) -> Result<f64> {
        let samples = self.total_samples()?;
        Ok(samples as f64 / self.sample_rate as f64)
    }

    /// Appends a slice of float32 samples to the end of the disk-backed file.
    pub fn append_samples(&self, samples: &[f32]) -> Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut writer = BufWriter::with_capacity(65536, file);

        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            writer.write_all(&clamped.to_le_bytes())?;
        }
        writer.flush()?;
        Ok(())
    }

    /// Reads an exact range of samples from disk into memory without loading the whole file.
    pub fn read_range(&self, start_sample: u64, count: usize) -> Result<Vec<f32>> {
        let total = self.total_samples()?;
        if start_sample > total {
            anyhow::bail!("Przekroczono zakres próbkowania strumienia PCM");
        }

        let actual_count = count.min((total - start_sample) as usize);
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(start_sample * 4))?;
        let mut reader = BufReader::with_capacity(65536, file);

        let mut out = Vec::with_capacity(actual_count);
        let mut byte_buf = [0u8; 4];

        for _ in 0..actual_count {
            reader.read_exact(&mut byte_buf)?;
            out.push(f32::from_le_bytes(byte_buf));
        }

        Ok(out)
    }

    /// Streams raw PCM samples directly from disk into a standard WAV file.
    /// Memory usage is strictly bounded to `chunk_size` (default ~64 KB).
    pub fn export_wav(
        &self,
        out_wav_path: impl AsRef<Path>,
        chunk_size: Option<usize>,
    ) -> Result<()> {
        let out_p = out_wav_path.as_ref();
        if let Some(parent) = out_p.parent() {
            create_dir_all(parent)?;
        }

        let total = self.total_samples()?;
        let block_size = chunk_size.unwrap_or(DEFAULT_STREAM_BUFFER_SAMPLES);

        let spec = WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        let mut wav_writer = WavWriter::create(out_p, spec)
            .with_context(|| format!("Nie można utworzyć pliku wyjściowego WAV: {:?}", out_p))?;

        let mut read_pos = 0u64;
        while read_pos < total {
            let to_read = ((total - read_pos) as usize).min(block_size);
            let samples = self.read_range(read_pos, to_read)?;
            for s in samples {
                let s16 = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
                wav_writer.write_sample(s16)?;
            }
            read_pos += to_read as u64;
        }

        wav_writer.finalize()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcm_stream_lifecycle_and_bounded_memory() {
        let tmp_dir = std::env::temp_dir().join(format!("test_pcm_stream_{}", std::process::id()));
        let pcm_path = tmp_dir.join("stream.pcm");
        let wav_path = tmp_dir.join("stream.wav");

        let stream = PcmStreamFile::create(&pcm_path, 24000).expect("Create stream");

        // Append 1 second of test audio in two chunks
        let chunk1 = vec![0.25f32; 12000];
        let chunk2 = vec![-0.50f32; 12000];

        stream.append_samples(&chunk1).expect("Append chunk 1");
        stream.append_samples(&chunk2).expect("Append chunk 2");

        assert_eq!(stream.total_samples().unwrap(), 24000);
        assert!((stream.duration_seconds().unwrap() - 1.0).abs() < 1e-4);

        // Read specific range
        let slice = stream.read_range(11999, 3).expect("Read range");
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0], 0.25);
        assert_eq!(slice[1], -0.50);
        assert_eq!(slice[2], -0.50);

        // Stream export to WAV
        stream
            .export_wav(&wav_path, Some(4096))
            .expect("Export WAV");
        assert!(wav_path.exists());
        let reader = hound::WavReader::open(&wav_path).expect("Open WAV");
        assert_eq!(reader.duration(), 24000);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
