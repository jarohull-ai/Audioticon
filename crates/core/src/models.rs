use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PIPER_VOICES_BASE_URL: &str =
    "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCatalogItem {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub lang_name: String,
    pub gender: String,
    pub quality: String,
    pub size_mb: u32,
    pub sample_rate: u32,
    pub hf_path: String,
    pub installed: bool,
    pub onnx_path: Option<PathBuf>,
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

pub fn get_curated_models() -> Vec<ModelCatalogItem> {
    vec![
        ModelCatalogItem {
            id: "pl_PL-darkman-medium".to_string(),
            name: "Darkman (Polski męski)".to_string(),
            lang: "pl_PL".to_string(),
            lang_name: "Polski".to_string(),
            gender: "male".to_string(),
            quality: "medium".to_string(),
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
            size_mb: 63,
            sample_rate: 22050,
            hf_path: "en/en_US/amy/medium/en_US-amy-medium".to_string(),
            installed: false,
            onnx_path: None,
        },
    ]
}

/// Zwraca listę modeli w katalogu z aktualnym statusem instalacji na dysku
pub fn list_models() -> Vec<ModelCatalogItem> {
    let dir = models_dir();
    let mut catalog = get_curated_models();

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
                                "en_US".to_string()
                            },
                            lang_name: if stem.starts_with("pl") {
                                "Polski".to_string()
                            } else {
                                "Inny".to_string()
                            },
                            gender: "unknown".to_string(),
                            quality: "custom".to_string(),
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

/// Pobiera wybrany model (.onnx oraz .onnx.json) z oficjalnego repozytorium HuggingFace
pub fn download_model(name_or_id: &str, force: bool) -> Result<PathBuf> {
    let item = find_model(name_or_id).ok_or_else(|| {
        anyhow::anyhow!(
            "Nieznany model: '{}'. Dostępne: pl_PL-darkman-medium, pl_PL-gosia-medium, pl_PL-mc_speech-medium, en_US-lessac-medium, en_US-amy-medium",
            name_or_id
        )
    })?;

    if item.hf_path.is_empty() {
        bail!(
            "Model '{}' nie posiada zdefiniowanej ścieżki pobierania",
            item.id
        );
    }

    let dir = models_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Nie udało się utworzyć folderu modeli: {:?}", dir))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_curated_models() {
        let models = list_models();
        assert!(models.len() >= 5);
        assert!(models.iter().any(|m| m.id == "pl_PL-darkman-medium"));
        assert!(models.iter().any(|m| m.id == "pl_PL-gosia-medium"));
        assert!(models.iter().any(|m| m.id == "en_US-lessac-medium"));
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
