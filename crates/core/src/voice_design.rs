//! ElevenLabs Voice Design (Text-to-Voice) client
//! Generates unique synthetic AI voices from textual descriptions.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::voicelab::{
    profile_dir, sanitize_slug, save_voice_profile, VoiceProfile, VoiceSampleMeta,
};

/// A generated voice preview returned from ElevenLabs Voice Design
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignedVoicePreview {
    pub generated_voice_id: String,
    pub audio_base_64: String,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub duration_secs: Option<f32>,
}

/// The response payload containing previews from ElevenLabs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceDesignResponse {
    pub previews: Vec<DesignedVoicePreview>,
    pub text: String,
}

/// Calls ElevenLabs `POST https://api.elevenlabs.io/v1/text-to-voice/create-previews`
/// to design a voice based on text description.
pub fn create_voice_design_previews(
    api_key: &str,
    description: &str,
    sample_text: Option<&str>,
) -> Result<VoiceDesignResponse> {
    let trimmed_key = api_key.trim();
    if trimmed_key.is_empty() {
        bail!("Brak klucza API ElevenLabs");
    }
    let desc = description.trim();
    if desc.is_empty() {
        bail!("Opis głosu nie może być pusty. Opisz charakter postaci, wiek, tembr, emocje.");
    }

    let prompt_text = sample_text
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("Przez dłuższą chwilę patrzył na te współrzędne. Zbliża się arktyczny sztorm. To jest próba głosu.");

    let body_json = serde_json::json!({
        "voice_description": desc,
        "text": prompt_text
    });

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_cfg = std::env::temp_dir().join(format!(
        "audioticon_design_cfg_{}_{}.tmp",
        std::process::id(),
        nonce
    ));
    let tmp_payload = std::env::temp_dir().join(format!(
        "audioticon_design_payload_{}_{}.json",
        std::process::id(),
        nonce
    ));

    fs::write(&tmp_payload, serde_json::to_string(&body_json)?)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts.open(&tmp_cfg)?;
        writeln!(f, "header = \"xi-api-key: {}\"", trimmed_key)?;
    }
    #[cfg(not(unix))]
    {
        fs::write(
            &tmp_cfg,
            format!("header = \"xi-api-key: {}\"\n", trimmed_key),
        )?;
    }

    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "-K",
            tmp_cfg.to_str().unwrap_or(""),
            "-X",
            "POST",
            "https://api.elevenlabs.io/v1/text-to-voice/create-previews",
            "-H",
            "Content-Type: application/json",
            "-d",
            &format!("@{}", tmp_payload.display()),
        ])
        .output();

    let _ = fs::remove_file(&tmp_cfg);
    let _ = fs::remove_file(&tmp_payload);

    let output =
        output.context("Nie udało się uruchomić polecenia curl dla ElevenLabs Voice Design")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "Błąd API ElevenLabs Voice Design (status: {}): {} {}",
            output.status,
            stderr,
            stdout
        );
    }

    let resp_str = String::from_utf8_lossy(&output.stdout);
    let parsed: VoiceDesignResponse = serde_json::from_str(&resp_str).with_context(|| {
        format!(
            "Nie można zdekodować odpowiedzi ElevenLabs Voice Design: {}",
            resp_str
        )
    })?;

    if parsed.previews.is_empty() {
        bail!("ElevenLabs nie zwróciło żadnych próbek podglądu dla podanego opisu głosu.");
    }

    Ok(parsed)
}

/// Finalizes a designed voice preview by adding it permanently to ElevenLabs account
/// and registering it in the local VoiceLab profile catalog.
/// Calls `POST https://api.elevenlabs.io/v1/text-to-voice`
pub fn save_designed_voice(
    api_key: &str,
    name: &str,
    description: &str,
    generated_voice_id: &str,
    preview_audio_base64: Option<&str>,
) -> Result<String> {
    let trimmed_key = api_key.trim();
    if trimmed_key.is_empty() {
        bail!("Brak klucza API ElevenLabs");
    }
    let v_name = name.trim();
    if v_name.is_empty() {
        bail!("Nazwa głosu nie może być pusta.");
    }
    let gen_id = generated_voice_id.trim();
    if gen_id.is_empty() {
        bail!("Brak identyfikatora generated_voice_id.");
    }

    let body_json = serde_json::json!({
        "voice_name": v_name,
        "voice_description": description.trim(),
        "generated_voice_id": gen_id
    });

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_cfg = std::env::temp_dir().join(format!(
        "audioticon_save_voice_cfg_{}_{}.tmp",
        std::process::id(),
        nonce
    ));
    let tmp_payload = std::env::temp_dir().join(format!(
        "audioticon_save_voice_payload_{}_{}.json",
        std::process::id(),
        nonce
    ));

    fs::write(&tmp_payload, serde_json::to_string(&body_json)?)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts.open(&tmp_cfg)?;
        writeln!(f, "header = \"xi-api-key: {}\"", trimmed_key)?;
    }
    #[cfg(not(unix))]
    {
        fs::write(
            &tmp_cfg,
            format!("header = \"xi-api-key: {}\"\n", trimmed_key),
        )?;
    }

    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "-K",
            tmp_cfg.to_str().unwrap_or(""),
            "-X",
            "POST",
            "https://api.elevenlabs.io/v1/text-to-voice",
            "-H",
            "Content-Type: application/json",
            "-d",
            &format!("@{}", tmp_payload.display()),
        ])
        .output();

    let _ = fs::remove_file(&tmp_cfg);
    let _ = fs::remove_file(&tmp_payload);

    let output =
        output.context("Nie udało się uruchomić polecenia curl do zapisu głosu w ElevenLabs")?;

    let resp_str = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Błąd zapisu głosu w ElevenLabs (status: {}): {} {}",
            output.status,
            stderr,
            resp_str
        );
    }

    #[derive(Deserialize)]
    struct SaveResp {
        voice_id: String,
    }

    let saved: SaveResp = serde_json::from_str(&resp_str)
        .with_context(|| format!("Nieprawidłowa odpowiedź zapisu głosu: {}", resp_str))?;

    let permanent_voice_id = saved.voice_id;

    // Register local VoiceProfile so the engine, Studio UI, and CLI can immediately use it
    let profile_id = sanitize_slug(v_name);
    let mut profile = VoiceProfile::new(&profile_id, v_name, "neutral", "pl");
    profile.description = Some(description.trim().to_string());
    profile.elevenlabs_voice_id = Some(permanent_voice_id.clone());

    // If base64 preview was supplied, save it as a reference audio sample in the profile
    if let Some(b64) = preview_audio_base64 {
        if let Ok(audio_bytes) = crate::importer::base64_decode(b64) {
            let p_dir = profile_dir(&profile_id);
            let _ = fs::create_dir_all(&p_dir);
            let sample_filename = format!(
                "sample_preview_{}.mp3",
                &permanent_voice_id[..6.min(permanent_voice_id.len())]
            );
            let sample_path = p_dir.join(&sample_filename);
            if fs::write(&sample_path, &audio_bytes).is_ok() {
                profile.samples.push(VoiceSampleMeta {
                    filename: sample_filename,
                    duration_sec: 4.0,
                    sample_rate: 44100,
                    channels: 1,
                    rms_db: -20.0,
                    peak_db: -3.0,
                    snr_db: 40.0,
                    clipping: false,
                    created_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                        .to_string(),
                });
            }
        }
    }

    let _ = save_voice_profile(&profile);

    Ok(permanent_voice_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_description_validation() {
        let res = create_voice_design_previews("dummy_key", "   ", None);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("nie może być pusty"));
    }

    #[test]
    fn test_empty_api_key_validation() {
        let res = create_voice_design_previews("", "stary marynarz", None);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Brak klucza API"));
    }

    #[test]
    fn test_deserialize_voice_design_response() {
        let json_data = r#"{
            "previews": [
                {
                    "audio_base_64": "UklGRiQAAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQAAAAA=",
                    "generated_voice_id": "gen_voice_12345",
                    "media_type": "audio/mpeg",
                    "duration_secs": 3.8
                }
            ],
            "text": "Przykładowe zdanie testowe."
        }"#;

        let resp: VoiceDesignResponse = serde_json::from_str(json_data).expect("deserialize");
        assert_eq!(resp.previews.len(), 1);
        assert_eq!(resp.previews[0].generated_voice_id, "gen_voice_12345");
        assert_eq!(resp.previews[0].duration_secs, Some(3.8));
    }
}
