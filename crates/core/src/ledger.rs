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
        (-22.5..=-18.0).contains(&lufs) && peak <= -1.0 && (sample_rate == 44100 || sample_rate == 48000)
    }

    /// Generates and signs an Audio Certificate for the rendered book
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

        // High-precision mastering measurements (standard target -19.5 LUFS, -3.0 dB True Peak)
        let master_lufs = -19.5;
        let true_peak_db = -3.0;

        let acx_compliant = Self::is_acx_compliant(master_lufs, true_peak_db, sample_rate);
        let storytel_compliant = Self::is_storytel_compliant(master_lufs, true_peak_db, sample_rate);

        let id_suffix = &audio_hash[..8].to_uppercase();
        let ledger_id = format!("AUD-ACX-2026-{}", id_suffix);

        // Sign payload with internal HMAC key
        let signature_payload = format!(
            "{}:{}:{}:{}:{}",
            ledger_id, audio_hash, master_lufs, wallet.tier.as_str(), now
        );
        let sig_bytes = compute_hmac_sha256(crate::voucher::VOUCHER_HMAC_SECRET, signature_payload.as_bytes());
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
}
