use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProviderCredentials {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub voice_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CredentialsConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderCredentials>,
}

pub fn credentials_path() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        config_dir.join("audioticon").join("credentials.toml")
    } else if let Some(home) = dirs::home_dir() {
        home.join(".config")
            .join("audioticon")
            .join("credentials.toml")
    } else {
        PathBuf::from(".credentials.toml")
    }
}

/// Loads credentials from credentials.toml
pub fn load_credentials() -> CredentialsConfig {
    let path = credentials_path();
    if !path.exists() {
        return CredentialsConfig::default();
    }
    match fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => CredentialsConfig::default(),
    }
}

/// Saves credentials to credentials.toml with secure file permissions (0600 on Unix)
pub fn save_credentials(config: &CredentialsConfig) -> Result<()> {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Nie udało się utworzyć folderu: {:?}", parent))?;
    }
    let toml_str = toml::to_string_pretty(config).context("Błąd serializacji credentials.toml")?;
    fs::write(&path, toml_str)
        .with_context(|| format!("Błąd zapisu pliku credentials: {:?}", path))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// Retrieves provider API key, checking environment variables first, then credentials.toml
pub fn get_api_key(provider: &str) -> Option<String> {
    let clean = provider.trim().to_lowercase();

    // 1. Check environment variables
    let env_var = match clean.as_str() {
        "elevenlabs" => "ELEVENLABS_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "azure" => "AZURE_SPEECH_KEY",
        _ => "",
    };

    if !env_var.is_empty() {
        if let Ok(key) = std::env::var(env_var) {
            if !key.trim().is_empty() {
                return Some(key.trim().to_string());
            }
        }
    }

    // 2. Check credentials.toml
    let creds = load_credentials();
    creds
        .providers
        .get(&clean)
        .and_then(|p| p.api_key.as_deref())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Sets provider API key and optional default model/voice
pub fn set_api_key(
    provider: &str,
    api_key: &str,
    model_id: Option<&str>,
    voice_id: Option<&str>,
) -> Result<()> {
    let clean = provider.trim().to_lowercase();
    if clean.is_empty() {
        bail!("Nazwa dostawcy nie może być pusta");
    }
    if api_key.trim().is_empty() {
        bail!("Klucz API nie może być pusty");
    }

    let mut creds = load_credentials();
    let entry = creds.providers.entry(clean).or_default();
    entry.api_key = Some(api_key.trim().to_string());
    if let Some(m) = model_id {
        if !m.trim().is_empty() {
            entry.model_id = Some(m.trim().to_string());
        }
    }
    if let Some(v) = voice_id {
        if !v.trim().is_empty() {
            entry.voice_id = Some(v.trim().to_string());
        }
    }

    save_credentials(&creds)
}

/// Removes provider from credentials.toml
pub fn remove_api_key(provider: &str) -> Result<bool> {
    let clean = provider.trim().to_lowercase();
    let mut creds = load_credentials();
    let removed = creds.providers.remove(&clean).is_some();
    if removed {
        save_credentials(&creds)?;
    }
    Ok(removed)
}

/// Masks an API key for safe UI and log display (e.g. "sk_12...9abc")
pub fn mask_api_key(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.len() <= 8 {
        "••••••••".to_string()
    } else {
        let prefix = &trimmed[..4];
        let suffix = &trimmed[trimmed.len() - 4..];
        format!("{}••••{}", prefix, suffix)
    }
}

/// Returns masked key for provider if configured
pub fn get_masked_key(provider: &str) -> Option<String> {
    get_api_key(provider).map(|k| mask_api_key(&k))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevenLabsAccountInfo {
    pub tier: String,
    pub character_count: u64,
    pub character_limit: u64,
    pub can_extend_character_limit: bool,
}

/// Verifies ElevenLabs API key against live ElevenLabs API
pub fn verify_elevenlabs_key(api_key: &str) -> Result<ElevenLabsAccountInfo> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        bail!("Klucz ElevenLabs jest pusty");
    }

    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "-f",
            "https://api.elevenlabs.io/v1/user/subscription",
            "-H",
            &format!("xi-api-key: {}", trimmed),
        ])
        .output()
        .context("Nie udało się wykonać zapytania curl do API ElevenLabs")?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Błąd weryfikacji klucza ElevenLabs (HTTP status failure): {}",
            err_msg.trim()
        );
    }

    #[derive(Deserialize)]
    struct SubscriptionResp {
        #[serde(default = "default_tier")]
        tier: String,
        #[serde(default)]
        character_count: u64,
        #[serde(default)]
        character_limit: u64,
        #[serde(default)]
        can_extend_character_limit: bool,
    }

    fn default_tier() -> String {
        "unknown".to_string()
    }

    let sub: SubscriptionResp = serde_json::from_slice(&output.stdout)
        .context("Nieprawidłowa odpowiedź JSON z API ElevenLabs")?;

    Ok(ElevenLabsAccountInfo {
        tier: sub.tier,
        character_count: sub.character_count,
        character_limit: sub.character_limit,
        can_extend_character_limit: sub.can_extend_character_limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_api_key() {
        assert_eq!(mask_api_key("short"), "••••••••");
        assert_eq!(mask_api_key("sk_test_1234567890abcdef"), "sk_t••••cdef");
    }

    #[test]
    fn test_credentials_roundtrip() {
        let tmp_dir = std::env::temp_dir().join("audioticon_cred_test");
        let _ = fs::remove_dir_all(&tmp_dir);
        let cred_file = tmp_dir.join("credentials.toml");

        let mut config = CredentialsConfig::default();
        config.providers.insert(
            "elevenlabs".to_string(),
            ProviderCredentials {
                api_key: Some("sk_test_secret_123".to_string()),
                model_id: Some("eleven_multilingual_v2".to_string()),
                voice_id: Some("adam".to_string()),
            },
        );

        fs::create_dir_all(&tmp_dir).unwrap();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        fs::write(&cred_file, toml_str).unwrap();

        let loaded: CredentialsConfig =
            toml::from_str(&fs::read_to_string(&cred_file).unwrap()).unwrap();
        assert_eq!(
            loaded
                .providers
                .get("elevenlabs")
                .unwrap()
                .api_key
                .as_deref(),
            Some("sk_test_secret_123")
        );

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
