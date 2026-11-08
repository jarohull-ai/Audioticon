use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;

/// Represents a single min/max/RMS peak point on the downsampled audio waveform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaveformPeak {
    /// Minimum normalized sample value in this time window (-1.0 to 0.0)
    pub min: f32,
    /// Maximum normalized sample value in this time window (0.0 to 1.0)
    pub max: f32,
    /// Root-Mean-Square energy in this time window (0.0 to 1.0)
    pub rms: f32,
}

/// Represents a time range occupied by a single script segment along the audio timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentTimeRange {
    /// 0-indexed ordinal number of the segment in the script
    pub index: usize,
    /// Deterministic content hash of this segment
    pub hash: String,
    /// Spoken or directed text
    pub text: String,
    /// Start timestamp in milliseconds from audio origin
    pub start_ms: u64,
    /// End timestamp in milliseconds from audio origin
    pub end_ms: u64,
    /// Total duration of this segment in milliseconds
    pub duration_ms: u64,
    /// Whether this segment was replaced with a user-recorded punch-in take
    pub is_punch_in: bool,
    /// Optional voice identifier or actor name
    #[serde(default)]
    pub actor_or_voice: Option<String>,
}

/// Complete waveform model containing downsampled audio peaks and synchronized segment cues.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaveformData {
    /// Total audio length in milliseconds
    pub duration_ms: u64,
    /// Audio sample rate in Hz (e.g. 24000, 44100, 48000)
    pub sample_rate: u32,
    /// Number of audio channels (1 = mono, 2 = stereo)
    pub channels: u16,
    /// Total decoded PCM samples
    pub total_samples: u64,
    /// Number of downsampled peak points
    pub peak_count: usize,
    /// Bipolar min/max/RMS peaks for drawing dual-sided waveforms
    pub peaks: Vec<WaveformPeak>,
    /// Unipolar 0.0..1.0 RMS amplitude envelope for compact bar charts
    pub rms_envelope: Vec<f32>,
    /// Sentence/clause boundary markers along the audio track
    pub segments: Vec<SegmentTimeRange>,
}

/// Computes normalized downsampled peaks and an RMS envelope from decoded float PCM samples.
pub fn compute_waveform_peaks(
    samples: &[f32],
    target_buckets: usize,
) -> (Vec<WaveformPeak>, Vec<f32>) {
    if samples.is_empty() || target_buckets == 0 {
        return (Vec::new(), Vec::new());
    }

    let actual_buckets = target_buckets.min(samples.len()).max(1);
    let mut peaks = Vec::with_capacity(actual_buckets);
    let mut envelope = Vec::with_capacity(actual_buckets);

    let step = samples.len() as f64 / actual_buckets as f64;

    for i in 0..actual_buckets {
        let start_idx = (i as f64 * step).floor() as usize;
        let mut end_idx = (((i + 1) as f64) * step).floor() as usize;
        if end_idx <= start_idx {
            end_idx = (start_idx + 1).min(samples.len());
        }
        let end_idx = end_idx.min(samples.len());
        let slice = &samples[start_idx..end_idx];

        if slice.is_empty() {
            peaks.push(WaveformPeak {
                min: 0.0,
                max: 0.0,
                rms: 0.0,
            });
            envelope.push(0.0);
            continue;
        }

        let mut min_val = 0.0f32;
        let mut max_val = 0.0f32;
        let mut sum_sq = 0.0f64;

        for &s in slice {
            let clamped = s.clamp(-1.0, 1.0);
            if clamped < min_val {
                min_val = clamped;
            }
            if clamped > max_val {
                max_val = clamped;
            }
            sum_sq += (clamped as f64) * (clamped as f64);
        }

        let rms = ((sum_sq / slice.len() as f64).sqrt() as f32).clamp(0.0, 1.0);
        peaks.push(WaveformPeak {
            min: min_val,
            max: max_val,
            rms,
        });
        envelope.push(rms);
    }

    (peaks, envelope)
}

/// Decodes samples from an in-memory WAV buffer and extracts downsampled waveform peaks.
pub fn extract_waveform_from_wav_bytes(
    wav_bytes: &[u8],
    target_buckets: usize,
) -> Result<WaveformData> {
    let cursor = Cursor::new(wav_bytes);
    let mut reader = hound::WavReader::new(cursor)
        .context("Błąd parsowania nagłówka WAV dla analizy fali dźwiękowej")?;
    let spec = reader.spec();

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            if bits <= 16 {
                let max_val = (1i32 << (bits - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| (s.unwrap_or(0) as f32 / max_val).clamp(-1.0, 1.0))
                    .collect()
            } else {
                let max_val = (1i64 << (bits - 1)) as f64;
                reader
                    .samples::<i32>()
                    .map(|s| ((s.unwrap_or(0) as f64) / max_val) as f32)
                    .collect()
            }
        }
    };

    // If stereo or multi-channel, downmix to mono for waveform display
    let mono_samples: Vec<f32> = if spec.channels > 1 {
        let ch = spec.channels as usize;
        samples
            .chunks_exact(ch)
            .map(|chunk| chunk.iter().sum::<f32>() / ch as f32)
            .collect()
    } else {
        samples
    };

    let total_samples = mono_samples.len() as u64;
    let duration_ms = if spec.sample_rate > 0 {
        ((total_samples as f64 / spec.sample_rate as f64) * 1000.0).round() as u64
    } else {
        0
    };

    let (peaks, rms_envelope) = compute_waveform_peaks(&mono_samples, target_buckets);
    let peak_count = peaks.len();

    Ok(WaveformData {
        duration_ms,
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        total_samples,
        peak_count,
        peaks,
        rms_envelope,
        segments: Vec::new(),
    })
}

/// Reads a WAV file from disk and extracts downsampled waveform data.
pub fn extract_waveform_from_wav_file<P: AsRef<Path>>(
    path: P,
    target_buckets: usize,
) -> Result<WaveformData> {
    let p = path.as_ref();
    let bytes =
        std::fs::read(p).with_context(|| format!("Nie można odczytać pliku WAV: {:?}", p))?;
    extract_waveform_from_wav_bytes(&bytes, target_buckets)
}

/// Combines raw audio waveform peaks with script item durations and detects recorded punch-ins.
pub fn build_waveform_with_script_segments(
    wav_bytes: &[u8],
    script: &crate::types::DirectedScript,
    default_voice: &str,
    item_durations_ms: &[u64],
    cache_dir: &Path,
    target_buckets: usize,
) -> Result<WaveformData> {
    let mut data = extract_waveform_from_wav_bytes(wav_bytes, target_buckets)?;

    let mut segments = Vec::new();
    let mut current_ms: u64 = 0;
    let mut dur_idx: usize = 0;

    for (idx, item) in script.items.iter().enumerate() {
        match item {
            crate::types::DirectedItem::Chapter(_) => {
                // Chapters do not produce separate audio time slices
            }
            crate::types::DirectedItem::Silence(ms) | crate::types::DirectedItem::Breath(ms) => {
                let dur = item_durations_ms
                    .get(dur_idx)
                    .copied()
                    .unwrap_or(*ms as u64);
                current_ms += dur;
                dur_idx += 1;
            }
            crate::types::DirectedItem::SoundEffect(_)
            | crate::types::DirectedItem::Ambience(_) => {
                let dur = item_durations_ms.get(dur_idx).copied().unwrap_or(0);
                current_ms += dur;
                dur_idx += 1;
            }
            crate::types::DirectedItem::Speech(seg) => {
                let seg_dur = item_durations_ms.get(dur_idx).copied().unwrap_or(0);
                dur_idx += 1;

                let voice = seg.voice.as_deref().unwrap_or(default_voice);
                let hash = crate::cache::compute_segment_hash(seg, voice);
                let punch_in_file = cache_dir.join(format!("recorded_{}.wav", hash));
                let is_punch_in = punch_in_file.exists();

                let start_ms = current_ms;
                let end_ms = current_ms + seg_dur;

                segments.push(SegmentTimeRange {
                    index: idx,
                    hash,
                    text: seg.text.clone(),
                    start_ms,
                    end_ms,
                    duration_ms: seg_dur,
                    is_punch_in,
                    actor_or_voice: seg
                        .voice
                        .clone()
                        .or_else(|| Some(default_voice.to_string())),
                });

                current_ms = end_ms;
            }
        }
    }

    data.segments = segments;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_waveform_peaks_empty() {
        let (peaks, env) = compute_waveform_peaks(&[], 100);
        assert!(peaks.is_empty());
        assert!(env.is_empty());
    }

    #[test]
    fn test_compute_waveform_peaks_silence() {
        let samples = vec![0.0f32; 1000];
        let (peaks, env) = compute_waveform_peaks(&samples, 10);
        assert_eq!(peaks.len(), 10);
        assert_eq!(env.len(), 10);
        for p in &peaks {
            assert_eq!(p.min, 0.0);
            assert_eq!(p.max, 0.0);
            assert_eq!(p.rms, 0.0);
        }
    }

    #[test]
    fn test_compute_waveform_peaks_sine_wave() {
        let sample_count = 1000;
        let mut samples = Vec::with_capacity(sample_count);
        for i in 0..sample_count {
            let phase = 2.0 * std::f32::consts::PI * (i as f32 / 100.0);
            samples.push(phase.sin() * 0.8);
        }

        let (peaks, env) = compute_waveform_peaks(&samples, 20);
        assert_eq!(peaks.len(), 20);
        assert_eq!(env.len(), 20);

        assert!(peaks.iter().any(|p| p.min <= -0.5));
        assert!(peaks.iter().any(|p| p.max >= 0.5));
        for p in &peaks {
            // Sine RMS of amplitude 0.8 is approx 0.8 / sqrt(2) ~= 0.565
            assert!(p.rms > 0.4 && p.rms < 0.7);
        }
    }

    #[test]
    fn test_extract_waveform_from_synthetic_wav() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut buffer, spec).unwrap();
            for i in 0..16000 {
                // 1 second of 440 Hz tone
                let t = i as f32 / 16000.0;
                let val = (2.0 * std::f32::consts::PI * 440.0 * t).sin();
                let sample_i16 = (val * 16000.0) as i16;
                writer.write_sample(sample_i16).unwrap();
            }
            writer.finalize().unwrap();
        }

        let wav_bytes = buffer.into_inner();
        let waveform = extract_waveform_from_wav_bytes(&wav_bytes, 50).unwrap();

        assert_eq!(waveform.duration_ms, 1000);
        assert_eq!(waveform.sample_rate, 16000);
        assert_eq!(waveform.channels, 1);
        assert_eq!(waveform.total_samples, 16000);
        assert_eq!(waveform.peak_count, 50);
        assert_eq!(waveform.peaks.len(), 50);
        assert_eq!(waveform.rms_envelope.len(), 50);
        assert!(waveform.peaks[0].rms > 0.2);
    }
}
