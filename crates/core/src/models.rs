use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PIPER_VOICES_BASE_URL: &str =
    "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0";

pub const DEFAULT_VOICES_JSON: &str = include_str!("voices.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCatalogItem {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub lang_name: String,
    pub gender: String,
    pub quality: String,
    #[serde(default = "default_license")]
    pub license: String,
    pub size_mb: u32,
    pub sample_rate: u32,
    pub hf_path: String,
    pub installed: bool,
    pub onnx_path: Option<PathBuf>,
}

fn default_license() -> String {
    "CC0".to_string()
}

pub fn models_dir() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("audioticon").join("models")
    } else if let Some(home) = dirs::home_dir() {
        home.join(".local")
            .join("share")
            .join("audioticon")
            .join("models")
    } else {
        PathBuf::from("./models")
    }
}

#[derive(Deserialize)]
struct RawVoiceEntry {
    key: String,
    name: String,
    language: RawLanguage,
    #[serde(default = "default_raw_quality")]
    quality: String,
    #[serde(default)]
    files: HashMap<String, RawFileInfo>,
}

fn default_raw_quality() -> String {
    "medium".to_string()
}

#[derive(Deserialize)]
struct RawLanguage {
    code: String,
    #[serde(default)]
    name_native: Option<String>,
    #[serde(default)]
    name_english: Option<String>,
}

#[derive(Deserialize)]
struct RawFileInfo {
    #[serde(default)]
    size_bytes: u64,
}

fn infer_gender(id: &str, name: &str) -> String {
    let lower_id = id.to_lowercase();
    let lower_name = name.to_lowercase();

    if lower_id.contains("darkman")
        || lower_id.contains("mc_speech")
        || lower_id.contains("lessac")
        || lower_id.contains("ryan")
        || lower_id.contains("guy")
        || lower_id.contains("kareem")
        || lower_id.contains("thorsten")
        || lower_id.contains("pau")
        || lower_id.contains("fabrice")
        || lower_name.contains("male")
    {
        "male".to_string()
    } else if lower_id.contains("gosia")
        || lower_id.contains("amy")
        || lower_id.contains("jenny")
        || lower_id.contains("ona")
        || lower_id.contains("zofia")
        || lower_id.contains("eva")
        || lower_id.contains("alba")
        || lower_id.contains("mariska")
        || lower_name.contains("female")
    {
        "female".to_string()
    } else {
        "neutral".to_string()
    }
}

fn infer_license(id: &str) -> String {
    let lower = id.to_lowercase();
    if lower.contains("darkman") || lower.contains("gosia") || lower.contains("mc_speech") {
        "CC0 (Public Domain)".to_string()
    } else if lower.contains("mls_") {
        "CC-BY-4.0".to_string()
    } else if lower.contains("lessac") {
        "Non-commercial (Blizzard)".to_string()
    } else if lower.contains("carlfm") {
        "CC-BY-SA-4.0".to_string()
    } else if lower.contains("thorsten") {
        "CC0 (Public Domain)".to_string()
    } else {
        "Open Model (Piper / HF)".to_string()
    }
}

pub fn parse_voices_json(json_str: &str) -> Vec<ModelCatalogItem> {
    let raw_map: HashMap<String, RawVoiceEntry> = match serde_json::from_str(json_str) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    let mut items = Vec::new();
    for (_k, v) in raw_map {
        // Find onnx file
        let onnx_path = v.files.keys().find(|f| f.ends_with(".onnx")).cloned();
        if let Some(onnx_rel) = onnx_path {
            let hf_path = if onnx_rel.ends_with(".onnx") {
                onnx_rel[..onnx_rel.len() - 5].to_string()
            } else {
                onnx_rel.clone()
            };

            let size_mb = v
                .files
                .get(&onnx_rel)
                .map(|f| ((f.size_bytes as f64) / (1024.0 * 1024.0)).round() as u32)
                .unwrap_or(60);

            let lang_code = v.language.code.clone();
            let lang_name = v
                .language
                .name_native
                .or(v.language.name_english)
                .unwrap_or_else(|| lang_code.clone());

            let gender = infer_gender(&v.key, &v.name);
            let license = infer_license(&v.key);

            let display_name = match v.key.as_str() {
                "pl_PL-darkman-medium" => "Darkman (Polski lektor męski)".to_string(),
                "pl_PL-gosia-medium" => "Gosia (Polski lektor żeński)".to_string(),
                "pl_PL-mc_speech-medium" => "MC Speech (Polski lektor)".to_string(),
                "pl_PL-mls_6892-low" => "MLS 6892 (Polski lektor)".to_string(),
                "en_US-lessac-medium" => "Lessac (English US Male)".to_string(),
                "en_US-amy-medium" => "Amy (English US Female)".to_string(),
                _ => {
                    let cap_name = if let Some(first) = v.name.chars().next() {
                        first.to_uppercase().collect::<String>() + &v.name[first.len_utf8()..]
                    } else {
                        v.name.clone()
                    };
                    format!("{} ({}, {})", cap_name, lang_code, v.quality)
                }
            };

            items.push(ModelCatalogItem {
                id: v.key,
                name: display_name,
                lang: lang_code,
                lang_name,
                gender,
                quality: v.quality,
                license,
                size_mb,
                sample_rate: 22050,
                hf_path,
                installed: false,
                onnx_path: None,
            });
        }
    }

    items
}

pub fn get_curated_models() -> Vec<ModelCatalogItem> {
    vec![
        ModelCatalogItem {
            id: "pl_PL-darkman-medium".to_string(),
            name: "Darkman (Polski męski)".to_string(),
            lang: "pl_PL".to_string(),
            lang_name: "Polski".to_string(),
            gender: "male".to_string(),
            quality: "medium".to_string(),
            license: "CC0 (Public Domain)".to_string(),
            size_mb: 63,
            sample_rate: 22050,
            hf_path: "pl/pl_PL/darkman/medium/pl_PL-darkman-medium".to_string(),
            installed: false,
            onnx_path: None,
        },
        ModelCatalogItem {
            id: "pl_PL-gosia-medium".to_string(),
            name: "Gosia (Polski żeński)".to_string(),
            lang: "pl_PL".to_string(),
            lang_name: "Polski".to_string(),
            gender: "female".to_string(),
            quality: "medium".to_string(),
            license: "CC0 (Public Domain)".to_string(),
            size_mb: 63,
            sample_rate: 22050,
            hf_path: "pl/pl_PL/gosia/medium/pl_PL-gosia-medium".to_string(),
            installed: false,
            onnx_path: None,
        },
        ModelCatalogItem {
            id: "pl_PL-mc_speech-medium".to_string(),
            name: "MC Speech (Polski lektor)".to_string(),
            lang: "pl_PL".to_string(),
            lang_name: "Polski".to_string(),
            gender: "male".to_string(),
            quality: "medium".to_string(),
            license: "CC0 (Public Domain)".to_string(),
            size_mb: 63,
            sample_rate: 22050,
            hf_path: "pl/pl_PL/mc_speech/medium/pl_PL-mc_speech-medium".to_string(),
            installed: false,
            onnx_path: None,
        },
        ModelCatalogItem {
            id: "en_US-lessac-medium".to_string(),
            name: "Lessac (English US Male)".to_string(),
            lang: "en_US".to_string(),
            lang_name: "English (US)".to_string(),
            gender: "male".to_string(),
            quality: "medium".to_string(),
            license: "Non-commercial (Blizzard)".to_string(),
            size_mb: 63,
            sample_rate: 22050,
            hf_path: "en/en_US/lessac/medium/en_US-lessac-medium".to_string(),
            installed: false,
            onnx_path: None,
        },
        ModelCatalogItem {
            id: "en_US-amy-medium".to_string(),
            name: "Amy (English US Female)".to_string(),
            lang: "en_US".to_string(),
            lang_name: "English (US)".to_string(),
            gender: "female".to_string(),
            quality: "medium".to_string(),
            license: "CC0 (Public Domain)".to_string(),
            size_mb: 63,
            sample_rate: 22050,
            hf_path: "en/en_US/amy/medium/en_US-amy-medium".to_string(),
            installed: false,
            onnx_path: None,
        },
    ]
}

/// Zwraca listę modeli w katalogu z aktualnym statusem instalacji na dysku.
/// Wczytuje pełną bazę voices.json (lokalną lub wbudowaną) i sortuje: PL -> EN -> inne.
pub fn list_models() -> Vec<ModelCatalogItem> {
    let dir = models_dir();

    let custom_json_path = dir.join("voices.json");
    let json_content = if custom_json_path.exists() {
        std::fs::read_to_string(&custom_json_path)
            .unwrap_or_else(|_| DEFAULT_VOICES_JSON.to_string())
    } else {
        DEFAULT_VOICES_JSON.to_string()
    };

    let mut catalog = parse_voices_json(&json_content);
    if catalog.is_empty() {
        catalog = get_curated_models();
    }

    // Sortowanie: polskie modele pierwsze, potem angielskie, potem reszta alfabetycznie
    catalog.sort_by(|a, b| {
        let score_a = if a.lang.starts_with("pl") {
            0
        } else if a.lang.starts_with("en") {
            1
        } else {
            2
        };
        let score_b = if b.lang.starts_with("pl") {
            0
        } else if b.lang.starts_with("en") {
            1
        } else {
            2
        };
        score_a
            .cmp(&score_b)
            .then_with(|| a.lang.cmp(&b.lang))
            .then_with(|| a.id.cmp(&b.id))
    });

    for item in &mut catalog {
        let onnx = dir.join(format!("{}.onnx", item.id));
        let json = dir.join(format!("{}.onnx.json", item.id));
        if onnx.exists() && json.exists() {
            item.installed = true;
            item.onnx_path = Some(onnx);
        }
    }

    // Dodatkowo wyszukaj ewentualne inne modele pobrane ręcznie do katalogu
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("onnx") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if !catalog.iter().any(|c| c.id == stem) {
                        let json = dir.join(format!("{}.onnx.json", stem));
                        catalog.push(ModelCatalogItem {
                            id: stem.to_string(),
                            name: format!("Własny model: {}", stem),
                            lang: if stem.starts_with("pl") {
                                "pl_PL".to_string()
                            } else {
                                "custom".to_string()
                            },
                            lang_name: if stem.starts_with("pl") {
                                "Polski".to_string()
                            } else {
                                "Własny / Lokalny".to_string()
                            },
                            gender: "neutral".to_string(),
                            quality: "custom".to_string(),
                            license: "Własny plik ONNX".to_string(),
                            size_mb: (entry.metadata().map(|m| m.len()).unwrap_or(0)
                                / (1024 * 1024)) as u32,
                            sample_rate: 22050,
                            hf_path: String::new(),
                            installed: json.exists(),
                            onnx_path: Some(path),
                        });
                    }
                }
            }
        }
    }

    catalog
}

/// Dopasowuje nazwę lub alias do modelu z katalogu
pub fn find_model(name_or_id: &str) -> Option<ModelCatalogItem> {
    let clean = name_or_id.trim().to_lowercase();
    let catalog = list_models();

    // 1. Dokładny ID
    if let Some(item) = catalog
        .iter()
        .find(|c| c.id.to_lowercase() == clean)
        .cloned()
    {
        return Some(item);
    }

    // 2. Skrócone aliasy
    let alias_id = match clean.as_str() {
        "darkman" | "male" | "mezczyzna" | "męski" => "pl_PL-darkman-medium",
        "gosia" | "female" | "kobieta" | "żeński" => "pl_PL-gosia-medium",
        "mc" | "mc_speech" | "mcspeech" => "pl_PL-mc_speech-medium",
        "lessac" | "us_male" => "en_US-lessac-medium",
        "amy" | "us_female" => "en_US-amy-medium",
        _ => "",
    };

    if !alias_id.is_empty() {
        if let Some(item) = catalog.iter().find(|c| c.id == alias_id).cloned() {
            return Some(item);
        }
    }

    // 3. Zawiera fragment nazwy
    catalog
        .into_iter()
        .find(|c| c.id.to_lowercase().contains(&clean))
}

fn download_file(url: &str, target_path: &Path) -> Result<()> {
    let part_path = target_path.with_extension("part");

    // Spróbuj curl najpierw
    let curl_res = Command::new("curl")
        .args(["-f", "-L", "-sS", "-o"])
        .arg(&part_path)
        .arg(url)
        .status();

    let success = match curl_res {
        Ok(s) => s.success(),
        Err(_) => false,
    };

    if !success {
        // Fallback do Pythona urllib
        let py_script = format!(
            "import urllib.request; urllib.request.urlretrieve('{}', '{}')",
            url,
            part_path.display()
        );
        let py_res = Command::new("python3").args(["-c", &py_script]).status()?;

        if !py_res.success() {
            let _ = std::fs::remove_file(&part_path);
            bail!("Nie udało się pobrać pliku: {}", url);
        }
    }

    // Weryfikacja czy plik nie jest pusty
    let meta = std::fs::metadata(&part_path)
        .with_context(|| format!("Brak pobranego pliku tymczasowego: {:?}", part_path))?;
    if meta.len() == 0 {
        let _ = std::fs::remove_file(&part_path);
        bail!("Pobrany plik jest pusty: {}", url);
    }

    std::fs::rename(&part_path, target_path)
        .with_context(|| format!("Błąd zapisu docelowego pliku: {:?}", target_path))?;

    Ok(())
}

/// Pobiera wybrany model (.onnx oraz .onnx.json lub pliki z Model Hub) z oficjalnego repozytorium
pub fn download_model(name_or_id: &str, force: bool) -> Result<PathBuf> {
    if name_or_id.contains("..") || name_or_id.contains('/') || name_or_id.contains('\\') {
        bail!("Nieprawidłowy identyfikator modelu: zabronione znaki ścieżki");
    }

    let dir = models_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Nie udało się utworzyć folderu modeli: {:?}", dir))?;

    // 1. Sprawdź, czy to model z katalogu Europe & Americas Neural Model Hub (np. kokoro-82m-int8)
    let clean_id = name_or_id.trim().to_lowercase();
    let hub_catalog = crate::model_hub::get_curated_hub_catalog();
    let hub_match = hub_catalog.iter().find(|m| {
        m.id.to_lowercase() == clean_id || (clean_id == "kokoro" && m.id == "kokoro-82m-int8")
    });

    if let Some(hub_entry) = hub_match {
        if hub_entry.download_urls.is_empty() {
            bail!(
                "Model '{}' w katalogu Hub nie posiada zdefiniowanych adresów URL pobierania",
                hub_entry.id
            );
        }

        let mut primary_target: Option<PathBuf> = None;
        for url in &hub_entry.download_urls {
            let filename = if hub_entry.id == "kokoro-82m-int8" && url.ends_with(".onnx") {
                "kokoro-v1.0.int8.onnx".to_string()
            } else if hub_entry.id == "kokoro-82m-int8" && url.ends_with(".bin") {
                "voices-v1.0.bin".to_string()
            } else {
                url.rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&format!("{}.onnx", hub_entry.id))
                    .to_string()
            };

            let target_path = dir.join(&filename);
            if primary_target.is_none() {
                primary_target = Some(target_path.clone());
            }

            if !target_path.exists() || force {
                download_file(url, &target_path)
                    .with_context(|| format!("Błąd pobierania pliku modelu Hub z: {}", url))?;
            }
        }

        return Ok(primary_target.unwrap_or_else(|| dir.join(&hub_entry.id)));
    }

    // 2. Standardowy katalog modeli Piper TTS (voices.json)
    let item = find_model(name_or_id).ok_or_else(|| {
        anyhow::anyhow!(
            "Nieznany model: '{}'. Dostępne modele Piper: pl_PL-darkman-medium, pl_PL-gosia-medium, pl_PL-mc_speech-medium, en_US-lessac-medium, en_US-amy-medium oraz modele Hub: kokoro-82m-int8",
            name_or_id
        )
    })?;

    if item.hf_path.is_empty() {
        bail!(
            "Model '{}' nie posiada zdefiniowanej ścieżki pobierania",
            item.id
        );
    }

    let onnx_path = dir.join(format!("{}.onnx", item.id));
    let json_path = dir.join(format!("{}.onnx.json", item.id));

    if onnx_path.exists() && json_path.exists() && !force {
        return Ok(onnx_path);
    }

    let onnx_url = format!("{}/{}.onnx", PIPER_VOICES_BASE_URL, item.hf_path);
    let json_url = format!("{}/{}.onnx.json", PIPER_VOICES_BASE_URL, item.hf_path);

    // 1. Pobierz plik JSON konfiguracyjny (mały, ~5KB)
    download_file(&json_url, &json_path)
        .with_context(|| format!("Błąd pobierania metadanych JSON: {}", json_url))?;

    // 2. Pobierz wagi ONNX (~63MB)
    download_file(&onnx_url, &onnx_path)
        .with_context(|| format!("Błąd pobierania wag ONNX: {}", onnx_url))?;

    Ok(onnx_path)
}

/// Usuwa pobrany model offline z dysku
pub fn remove_model(name_or_id: &str) -> Result<()> {
    if name_or_id.contains("..") || name_or_id.contains('/') || name_or_id.contains('\\') {
        bail!("Nieprawidłowy identyfikator modelu: zabronione znaki ścieżki");
    }

    let item = find_model(name_or_id)
        .ok_or_else(|| anyhow::anyhow!("Nieznany model do usunięcia: '{}'", name_or_id))?;

    let dir = models_dir();
    let onnx_path = dir.join(format!("{}.onnx", item.id));
    let json_path = dir.join(format!("{}.onnx.json", item.id));

    if onnx_path.exists() {
        std::fs::remove_file(&onnx_path)?;
    }
    if json_path.exists() {
        std::fs::remove_file(&json_path)?;
    }

    Ok(())
}

/// Pobiera zaktualizowany plik voices.json z oficjalnego repozytorium HuggingFace
pub fn update_voices_catalog() -> Result<usize> {
    let url = "https://huggingface.co/rhasspy/piper-voices/raw/v1.0.0/voices.json";
    let dir = models_dir();
    std::fs::create_dir_all(&dir)?;
    let target = dir.join("voices.json");
    download_file(url, &target)?;
    let models = list_models();
    Ok(models.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_curated_models() {
        let models = list_models();
        assert!(models.len() >= 140);
        assert!(models.iter().any(|m| m.id == "pl_PL-darkman-medium"));
        assert!(models.iter().any(|m| m.id == "pl_PL-gosia-medium"));
        assert!(models.iter().any(|m| m.id == "en_US-lessac-medium"));

        let darkman = models
            .iter()
            .find(|m| m.id == "pl_PL-darkman-medium")
            .expect("darkman should exist");
        assert_eq!(darkman.license, "CC0 (Public Domain)");

        // Multilingual support
        assert!(models.iter().any(|m| m.lang.starts_with("de")));
        assert!(models.iter().any(|m| m.lang.starts_with("uk")));
    }

    #[test]
    fn test_find_model_by_alias() {
        let darkman = find_model("darkman").expect("Should find darkman");
        assert_eq!(darkman.id, "pl_PL-darkman-medium");

        let gosia = find_model("gosia").expect("Should find gosia");
        assert_eq!(gosia.id, "pl_PL-gosia-medium");

        let lessac = find_model("lessac").expect("Should find lessac");
        assert_eq!(lessac.id, "en_US-lessac-medium");
    }
}
