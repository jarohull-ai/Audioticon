use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Base directory for storing global sound effects (~/.local/share/audioticon/sfx)
pub fn sfx_dir() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("audioticon").join("sfx")
    } else if let Some(home) = dirs::home_dir() {
        home.join(".local")
            .join("share")
            .join("audioticon")
            .join("sfx")
    } else {
        PathBuf::from("./sfx")
    }
}

/// Base directory for continuous ambient tracks (~/.local/share/audioticon/ambient)
pub fn ambient_dir() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("audioticon").join("ambient")
    } else if let Some(home) = dirs::home_dir() {
        home.join(".local")
            .join("share")
            .join("audioticon")
            .join("ambient")
    } else {
        PathBuf::from("./ambient")
    }
}

/// Information about an installed or discovered sound effect
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SfxAssetInfo {
    pub name: String,
    pub path: PathBuf,
    pub extension: String,
    pub size_bytes: u64,
    pub category: String,
    pub is_project_local: bool,
}

/// Curated essential sound effect definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CuratedSfxItem {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub license: String,
    pub tags: Vec<String>,
}

/// Supported audio file extensions in priority order
pub const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &["wav", "ogg", "mp3", "flac"];

/// Finds an SFX asset by canonical or alias name across project local and user shared directories
pub fn find_sfx_asset(name: &str) -> Option<PathBuf> {
    let clean = name.trim().to_lowercase();

    // 1. Check local project ./sfx/{clean}.{ext}
    for ext in SUPPORTED_AUDIO_EXTENSIONS {
        let p = PathBuf::from(format!("sfx/{}.{}", clean, ext));
        if p.exists() && p.is_file() {
            return Some(p);
        }
    }

    // 2. Check global sfx_dir() (~/.local/share/audioticon/sfx/{clean}.{ext})
    let global_dir = sfx_dir();
    for ext in SUPPORTED_AUDIO_EXTENSIONS {
        let p = global_dir.join(format!("{}.{}", clean, ext));
        if p.exists() && p.is_file() {
            return Some(p);
        }
    }

    None
}

/// Finds an ambient background soundscape asset by name
pub fn find_ambient_asset(name: &str) -> Option<PathBuf> {
    let clean = name.trim().to_lowercase();

    // 1. Check local project ./ambient/{clean}.{ext}
    for ext in SUPPORTED_AUDIO_EXTENSIONS {
        let p = PathBuf::from(format!("ambient/{}.{}", clean, ext));
        if p.exists() && p.is_file() {
            return Some(p);
        }
    }

    // 2. Check global ambient_dir()
    let global_dir = ambient_dir();
    for ext in SUPPORTED_AUDIO_EXTENSIONS {
        let p = global_dir.join(format!("{}.{}", clean, ext));
        if p.exists() && p.is_file() {
            return Some(p);
        }
    }

    // Fallback: check sfx directory if ambient file is placed there
    find_sfx_asset(name)
}

/// Lists all installed sound effects from project and global directory
pub fn list_installed_sfx() -> Vec<SfxAssetInfo> {
    let mut results = Vec::new();

    // Scan project local ./sfx
    let project_dir = PathBuf::from("sfx");
    if project_dir.exists() && project_dir.is_dir() {
        scan_directory(&project_dir, true, &mut results);
    }

    // Scan global sfx_dir
    let global = sfx_dir();
    if global.exists() && global.is_dir() {
        scan_directory(&global, false, &mut results);
    }

    // Deduplicate by name (project local overrides global)
    results.sort_by(|a, b| a.name.cmp(&b.name));
    results.dedup_by(|a, b| a.name == b.name);
    results
}

fn scan_directory(dir: &Path, is_project: bool, results: &mut Vec<SfxAssetInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if SUPPORTED_AUDIO_EXTENSIONS.contains(&ext_lower.as_str()) {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        let category = infer_category(stem);
                        results.push(SfxAssetInfo {
                            name: stem.to_string(),
                            path: path.clone(),
                            extension: ext_lower,
                            size_bytes,
                            category,
                            is_project_local: is_project,
                        });
                    }
                }
            }
        }
    }
}

/// Infers category from sound name
pub fn infer_category(name: &str) -> String {
    let n = name.to_lowercase();
    if n.contains("drzwi") || n.contains("door") || n.contains("zamek") || n.contains("klucz") {
        "Drzwi i Zamki".to_string()
    } else if n.contains("krok")
        || n.contains("footstep")
        || n.contains("chodz")
        || n.contains("lod")
    {
        "Kroki i Ruch".to_string()
    } else if n.contains("deszcz")
        || n.contains("rain")
        || n.contains("wiatr")
        || n.contains("wind")
        || n.contains("burza")
        || n.contains("grzmot")
    {
        "Pogoda i Otoczenie".to_string()
    } else if n.contains("telefon")
        || n.contains("phone")
        || n.contains("radio")
        || n.contains("bell")
        || n.contains("dzwonek")
    {
        "Technika i Sygnały".to_string()
    } else if n.contains("szklo")
        || n.contains("glass")
        || n.contains("papier")
        || n.contains("notes")
        || n.contains("kartka")
    {
        "Przedmioty".to_string()
    } else if n.contains("strzal")
        || n.contains("shot")
        || n.contains("wybuch")
        || n.contains("boom")
        || n.contains("bron")
        || n.contains("pistolet")
        || n.contains("przeladowanie")
        || n.contains("walka")
    {
        "Akcja i Walka".to_string()
    } else {
        "Różne".to_string()
    }
}

/// Imports a user-provided audio file into the project or user SFX directory
pub fn import_custom_sfx(
    source_path: &Path,
    custom_name: Option<&str>,
    target_dir: &Path,
) -> Result<PathBuf> {
    if !source_path.exists() || !source_path.is_file() {
        bail!("Plik źródłowy nie istnieje: {:?}", source_path);
    }

    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "wav".to_string());

    if !SUPPORTED_AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        bail!(
            "Nieobsługiwany format pliku: .{}. Obsługiwane formaty: wav, ogg, mp3, flac",
            ext
        );
    }

    let dest_name = if let Some(n) = custom_name {
        let clean = n.trim().to_lowercase().replace(' ', "_");
        format!("{}.{}", clean, ext)
    } else {
        source_path
            .file_name()
            .and_then(|n| n.to_str())
            .context("Nieprawidłowa nazwa pliku")?
            .to_string()
    };

    std::fs::create_dir_all(target_dir)
        .with_context(|| format!("Nie można utworzyć katalogu docelowego: {:?}", target_dir))?;

    let dest_path = target_dir.join(dest_name);
    std::fs::copy(source_path, &dest_path)
        .with_context(|| format!("Błąd kopiowania pliku do: {:?}", dest_path))?;

    Ok(dest_path)
}

/// Returns the curated list of essential sound effects for audiobooks
pub fn list_curated_essentials() -> Vec<CuratedSfxItem> {
    vec![
        CuratedSfxItem {
            id: "drzwi_drewno".to_string(),
            name: "Drzwi drewniane (skrzypienie i zamknięcie)".to_string(),
            category: "Drzwi i Zamki".to_string(),
            description: "Czyste nagranie skrzypiących starych drzwi pokojowych".to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "drzwi".to_string(),
                "door".to_string(),
                "skrzyp".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "drzwi_stal".to_string(),
            name: "Wrota stalowe / bunkier".to_string(),
            category: "Drzwi i Zamki".to_string(),
            description: "Ciężkie metalowe drzwi schronu lub bramy więziennej".to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "drzwi".to_string(),
                "metal".to_string(),
                "bunkier".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "kroki_beton".to_string(),
            name: "Kroki w korytarzu / beton".to_string(),
            category: "Kroki i Ruch".to_string(),
            description: "Pojedyncze kroki w butach z twardą podeszwą po posadzce".to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "krok".to_string(),
                "kroki".to_string(),
                "footstep".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "kroki_bloto".to_string(),
            name: "Kroki w błocie / deszczu".to_string(),
            category: "Kroki i Ruch".to_string(),
            description: "Chrzęst i plask kroków na mokrej, leśnej ścieżce".to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "krok".to_string(),
                "bloto".to_string(),
                "deszcz".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "radio_szum".to_string(),
            name: "Wojskowa radiostacja (szum i klik)".to_string(),
            category: "Technika i Sygnały".to_string(),
            description: "Charakterystyczny trzask PTT i szum fali nośnej radiotelefonu"
                .to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "radio".to_string(),
                "szum".to_string(),
                "walkie".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "telefon_dzwonek".to_string(),
            name: "Dzwonek telefonu stacjonarnego / gong".to_string(),
            category: "Technika i Sygnały".to_string(),
            description: "Dwutonowy dzwonek gongowy z naturalnym wybrzmieniem".to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "telefon".to_string(),
                "ding".to_string(),
                "bell".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "papier_notes".to_string(),
            name: "Przewracanie kartki / szelest notesu".to_string(),
            category: "Przedmioty".to_string(),
            description: "Szelest przewracanej strony starego notesu".to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "papier".to_string(),
                "kartka".to_string(),
                "notes".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "szklo_stluczone".to_string(),
            name: "Brzęk tłuczonego szkła".to_string(),
            category: "Przedmioty".to_string(),
            description: "Odgłos rozbijanej butelki lub szyby".to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "szklo".to_string(),
                "glass".to_string(),
                "crash".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "uderzenie_metal".to_string(),
            name: "Uderzenie pięścią w metal".to_string(),
            category: "Akcja i Walka".to_string(),
            description: "Głuche dudnienie metalowej płyty lub wrót".to_string(),
            license: "CC0".to_string(),
            tags: vec![
                "uderzenie".to_string(),
                "metal".to_string(),
                "thud".to_string(),
            ],
        },
        CuratedSfxItem {
            id: "przeladowanie_broni".to_string(),
            name: "Szczęk przeładowania zamka".to_string(),
            category: "Akcja i Walka".to_string(),
            description: "Metaliczny szczęk rygla broni palnej".to_string(),
            license: "CC0".to_string(),
            tags: vec!["bron".to_string(), "zamek".to_string(), "gun".to_string()],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sfx_dir_not_empty() {
        let dir = sfx_dir();
        assert!(dir.to_string_lossy().contains("audioticon"));
    }

    #[test]
    fn test_infer_category() {
        assert_eq!(infer_category("stare_drzwi_drewniane"), "Drzwi i Zamki");
        assert_eq!(infer_category("kroki_po_schodach"), "Kroki i Ruch");
        assert_eq!(infer_category("ulewny_deszcz"), "Pogoda i Otoczenie");
        assert_eq!(infer_category("telefon_stacjonarny"), "Technika i Sygnały");
        assert_eq!(infer_category("rozbite_szklo"), "Przedmioty");
        assert_eq!(infer_category("przeladowanie_pistoletu"), "Akcja i Walka");
    }

    #[test]
    fn test_curated_essentials_list() {
        let list = list_curated_essentials();
        assert!(list.len() >= 10);
        assert!(list.iter().any(|i| i.id == "drzwi_stal"));
        assert!(list.iter().any(|i| i.id == "radio_szum"));
    }

    #[test]
    fn test_import_custom_sfx_rejects_non_audio() {
        let tmp = std::env::temp_dir().join(format!("test_sfx_import_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let txt_file = tmp.join("dummy.txt");
        let _ = std::fs::write(&txt_file, "not an audio file");

        let dest_dir = tmp.join("target_sfx");
        let res = import_custom_sfx(&txt_file, None, &dest_dir);
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
