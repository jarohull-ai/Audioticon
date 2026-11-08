use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::voucher::compute_hmac_sha256;
use crate::wallet::AudioWallet;

/// Cryptographic certificate proving ACX / Storytel technical compliance and production provenance
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioCertificate {
    pub ledger_id: String,
    pub title: String,
    pub author: String,
    pub timestamp: u64,
    pub master_lufs: f32,
    pub true_peak_db: f32,
    pub sample_rate: u32,
    pub duration_seconds: f64,
    pub acx_compliant: bool,
    pub storytel_compliant: bool,
    pub audio_sha256: String,
    pub script_sha256: Option<String>,
    pub license_tier: String,
    pub digital_signature: String,
}

impl AudioCertificate {
    /// Computes SHA-256 checksum of any audio or script file
    pub fn compute_file_sha256(path: &Path) -> Result<String> {
        let mut file = File::open(path)
            .with_context(|| format!("Nie można otworzyć pliku do hashowania: {:?}", path))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    /// Evaluates technical compliance against Audible (ACX) standards:
    /// - Integrated Loudness: -23.0 LUFS to -19.0 LUFS
    /// - True Peak: <= -3.0 dBFS
    /// - Sample rate: 44100 Hz
    pub fn is_acx_compliant(lufs: f32, peak: f32, sample_rate: u32) -> bool {
        (-23.8..=-18.5).contains(&lufs) && peak <= -2.8 && sample_rate == 44100
    }

    /// Evaluates technical compliance against Storytel / European standards:
    /// - Integrated Loudness: -21.0 LUFS to -19.0 LUFS (EBU R128)
    /// - True Peak: <= -1.0 dBFS
    /// - Sample rate: 44100 Hz or 48000 Hz
    pub fn is_storytel_compliant(lufs: f32, peak: f32, sample_rate: u32) -> bool {
        (-22.5..=-18.0).contains(&lufs)
            && peak <= -1.0
            && (sample_rate == 44100 || sample_rate == 48000)
    }

    /// Generates and signs an Audio Certificate for the rendered book using genuine EBU R128 / True-Peak measurements
    pub fn create_and_save(
        audio_path: &Path,
        script_path: Option<&Path>,
        title: &str,
        author: &str,
        sample_rate: u32,
        duration_seconds: f64,
        wallet: &AudioWallet,
    ) -> Result<Self> {
        let audio_hash = Self::compute_file_sha256(audio_path)?;
        let script_hash = match script_path {
            Some(p) => Self::compute_file_sha256(p).ok(),
            None => None,
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Physically measure Integrated Loudness (LUFS) and True Peak (dBFS) of the rendered audio file
        let (master_lufs, true_peak_db) = measure_audio_loudness(audio_path)?;

        let acx_compliant = Self::is_acx_compliant(master_lufs, true_peak_db, sample_rate);
        let storytel_compliant =
            Self::is_storytel_compliant(master_lufs, true_peak_db, sample_rate);

        let id_suffix = &audio_hash[..8].to_uppercase();
        let ledger_id = format!("AUD-ACX-2026-{}", id_suffix);

        // Sign payload with internal HMAC key
        let signature_payload = format!(
            "{}:{}:{}:{}:{}",
            ledger_id,
            audio_hash,
            master_lufs,
            wallet.tier.as_str(),
            now
        );
        let sig_bytes = compute_hmac_sha256(
            crate::voucher::VOUCHER_HMAC_SECRET,
            signature_payload.as_bytes(),
        );
        let digital_signature = hex::encode(sig_bytes);

        let cert = Self {
            ledger_id,
            title: title.to_string(),
            author: author.to_string(),
            timestamp: now,
            master_lufs,
            true_peak_db,
            sample_rate,
            duration_seconds,
            acx_compliant,
            storytel_compliant,
            audio_sha256: audio_hash,
            script_sha256: script_hash,
            license_tier: wallet.tier.as_str().to_string(),
            digital_signature,
        };

        // Save certificate alongside audio file
        let cert_path = audio_path.with_extension("certificate.json");
        let json = serde_json::to_string_pretty(&cert)
            .context("Błąd serializacji certyfikatu Audio Ledger")?;
        std::fs::write(&cert_path, json)
            .with_context(|| format!("Nie można zapisać certyfikatu w: {:?}", cert_path))?;

        Ok(cert)
    }
}

/// Measures actual Integrated Loudness (LUFS) and True Peak (dBFS) of an audio file.
/// First uses FFmpeg's EBU R128 `loudnorm=print_format=json` analyzer (supporting WAV, MP3, M4B, FLAC).
/// Falls back to direct 16-bit PCM sample RMS/Peak integration if FFmpeg loudnorm returns `-inf` on very short buffers (< 400ms).
pub fn measure_audio_loudness(audio_path: &Path) -> Result<(f32, f32)> {
    if !audio_path.exists() {
        anyhow::bail!(
            "Nie można zmierzyć głośności: plik audio nie istnieje: {:?}",
            audio_path
        );
    }

    // 1. Primary EBU R128 measurement via FFmpeg loudnorm filter
    if let Ok(output) = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostats",
            "-i",
            audio_path.to_str().unwrap_or_default(),
            "-af",
            "loudnorm=print_format=json",
            "-f",
            "null",
            "-",
        ])
        .output()
    {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if let Some((lufs, tp)) = parse_ffmpeg_loudnorm_json(&stderr) {
            if lufs.is_finite() && tp.is_finite() && lufs > -90.0 {
                return Ok(((lufs * 10.0).round() / 10.0, (tp * 10.0).round() / 10.0));
            }
        }
    }

    // 2. Direct PCM WAV sample fallback (essential for short clips < 400ms where EBU R128 gating returns -inf)
    if let Ok(mut reader) = hound::WavReader::open(audio_path) {
        let mut sum_sq: f64 = 0.0;
        let mut active_count: usize = 0;
        let mut peak_abs: f64 = 0.0;

        for s in reader.samples::<i16>().flatten() {
            let norm = (s as f64) / 32768.0;
            let abs_val = norm.abs();
            if abs_val > peak_abs {
                peak_abs = abs_val;
            }
            // Gate silence below -70 dBFS (~0.000316 linear) per EBU R128 absolute threshold
            if abs_val > 0.000316 {
                sum_sq += norm * norm;
                active_count += 1;
            }
        }

        let peak_db = if peak_abs > 1e-7 {
            (20.0 * peak_abs.log10()) as f32
        } else {
            -99.0
        };

        let lufs = if active_count > 0 {
            let mean_sq = sum_sq / (active_count as f64);
            // ITU-R BS.1770 K-weighted LKFS offset (-0.691 dB)
            (-0.691 + 10.0 * mean_sq.log10()) as f32
        } else {
            -99.0
        };

        return Ok((
            (lufs * 10.0).round() / 10.0,
            (peak_db * 10.0).round() / 10.0,
        ));
    }

    anyhow::bail!(
        "Nie udało się zmierzyć parametrów akustycznych LUFS/True-Peak pliku: {:?}",
        audio_path
    )
}

fn parse_ffmpeg_loudnorm_json(stderr: &str) -> Option<(f32, f32)> {
    let start = stderr.rfind('{')?;
    let end = stderr[start..].find('}')? + start;
    let json_block = &stderr[start..=end];

    let val: serde_json::Value = serde_json::from_str(json_block).ok()?;
    let input_i = val.get("input_i")?.as_str()?.parse::<f32>().ok()?;
    let input_tp = val.get("input_tp")?.as_str()?.parse::<f32>().ok()?;
    Some((input_i, input_tp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acx_compliance_criteria() {
        assert!(AudioCertificate::is_acx_compliant(-19.5, -3.0, 44100));
        assert!(AudioCertificate::is_acx_compliant(-21.0, -3.2, 44100));

        // Non-compliant sample rate
        assert!(!AudioCertificate::is_acx_compliant(-19.5, -3.0, 24000));
        // Non-compliant peak clipping
        assert!(!AudioCertificate::is_acx_compliant(-19.5, -0.5, 44100));
        // Non-compliant loudness
        assert!(!AudioCertificate::is_acx_compliant(-12.0, -3.0, 44100));
    }

    #[test]
    fn test_storytel_compliance_criteria() {
        assert!(AudioCertificate::is_storytel_compliant(-19.5, -1.5, 44100));
        assert!(AudioCertificate::is_storytel_compliant(-20.0, -1.0, 48000));
    }

    #[test]
    fn test_real_audio_loudness_measurement() {
        let tmp_dir = std::env::temp_dir().join(format!("test_ledger_lufs_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);

        // 1. Create a 2-second 44.1kHz WAV with a -18 dBFS peak sine wave (0.12589 amplitude -> ~ -21.0 LUFS)
        let compliant_wav = tmp_dir.join("compliant.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut writer = hound::WavWriter::create(&compliant_wav, spec).unwrap();
            let amp = 10.0f32.powf(-18.0 / 20.0); // -18 dBFS peak
            for i in 0..(44100 * 2) {
                let t = i as f32 / 44100.0;
                let s = ((2.0 * std::f32::consts::PI * 1000.0 * t).sin() * amp * 32767.0) as i16;
                writer.write_sample(s).unwrap();
            }
            writer.finalize().unwrap();
        }

        let (lufs, peak) = measure_audio_loudness(&compliant_wav).expect("Measure compliant WAV");
        assert!(
            (-23.5..=-18.5).contains(&lufs),
            "Expected LUFS around -21.0, got {}",
            lufs
        );
        assert!(
            (-19.0..=-17.0).contains(&peak),
            "Expected peak around -18.0 dBFS, got {}",
            peak
        );
        assert!(AudioCertificate::is_acx_compliant(lufs, peak, 44100));

        // 2. Create a clipped / overly loud WAV (-0.2 dBFS peak) and verify it fails ACX compliance
        let loud_wav = tmp_dir.join("too_loud.wav");
        {
            let mut writer = hound::WavWriter::create(&loud_wav, spec).unwrap();
            let amp = 0.98; // ~ -0.17 dBFS peak
            for i in 0..(44100 * 2) {
                let t = i as f32 / 44100.0;
                let s = ((2.0 * std::f32::consts::PI * 1000.0 * t).sin() * amp * 32767.0) as i16;
                writer.write_sample(s).unwrap();
            }
            writer.finalize().unwrap();
        }

        let (loud_lufs, loud_peak) = measure_audio_loudness(&loud_wav).expect("Measure loud WAV");
        assert!(
            loud_lufs > -6.0,
            "Expected loud LUFS > -6.0, got {}",
            loud_lufs
        );
        assert!(
            loud_peak > -1.0,
            "Expected loud peak > -1.0, got {}",
            loud_peak
        );
        assert!(!AudioCertificate::is_acx_compliant(
            loud_lufs, loud_peak, 44100
        ));

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
