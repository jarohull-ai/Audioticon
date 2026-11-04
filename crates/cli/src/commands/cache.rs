use anyhow::{Context, Result};
use audioticon_core::{estimate_cost, presets::PresetRegistry, CostEstimate, ProjectConfig};
use std::path::PathBuf;

use crate::args::{get_default_cache_dir, DEMO_SCENE};

pub fn print_estimate_report(est: &CostEstimate) {
    println!(
        "💰 Szacunek kosztów syntezy mowy ({})",
        est.provider.to_uppercase()
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Liczba segmentów mowy:  {}", est.total_speech_segments);
    println!(
        "  Segmenty w cache:       {} ({:.1}%)",
        est.cached_segments, est.cache_hit_ratio_percent
    );
    println!("  Segmenty do syntezy:    {}", est.segments_to_synthesize);
    println!("  Łączna liczba znaków:   {}", est.total_characters);
    println!("  Znaki do syntezy:       {}", est.characters_to_synthesize);
    if est.estimated_cost_usd > 0.0 {
        println!(
            "  Szacowany koszt:        ~${:.4} USD",
            est.estimated_cost_usd
        );
    } else {
        println!("  Szacowany koszt:        $0.00 USD (darmowy lub 100% cache)");
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

pub fn run_cache(clear: bool, dir: Option<PathBuf>) -> Result<()> {
    let cache_path = dir.unwrap_or_else(get_default_cache_dir);
    if clear {
        if cache_path.exists() {
            std::fs::remove_dir_all(&cache_path)?;
            std::fs::create_dir_all(&cache_path)?;
            println!("🧹 Wyczyszczono bufor: {:?}", cache_path);
        } else {
            println!("Bufor jest pusty: {:?}", cache_path);
        }
    } else {
        let count = if cache_path.exists() {
            std::fs::read_dir(&cache_path)?
                .filter_map(|e| e.ok())
                .count()
        } else {
            0
        };
        println!("📂 Katalog cache: {:?}", cache_path);
        println!("📦 Zapisanych plików segmentów: {}", count);
    }
    Ok(())
}

pub fn run_presets(config: Option<PathBuf>) -> Result<()> {
    let project_cfg = if let Some(ref path) = config {
        ProjectConfig::load_from_path(path)
            .with_context(|| format!("Nie można załadować pliku konfiguracji: {:?}", path))?
    } else {
        ProjectConfig::discover(None)
    };

    let mut registry = PresetRegistry::default();
    registry.load_project_config(&project_cfg);

    println!("============================================================");
    println!("🎛️ AUDIOTICON — Rejestr Presetów Akustycznych");
    if !project_cfg.presets.is_empty() {
        println!(
            "Konfiguracja projektu: załadowano {} własnych wariantów",
            project_cfg.presets.len()
        );
    }
    println!("============================================================");
    println!(
        "{:<4} {:<22} {:<12} Kluczowe parametry",
        "Lp.", "Preset / Wariant", "Bazowy typ"
    );
    println!("------------------------------------------------------------");
    for (idx, (name, preset)) in registry.list_presets().iter().enumerate() {
        let mut details = Vec::new();
        if let Some(vol) = preset.volume {
            details.push(format!("vol={:.2}", vol));
        }
        if let Some(dist) = preset.distortion {
            details.push(format!("dist={:.2}", dist));
        }
        if let Some(bw) = preset.bandwidth {
            details.push(format!("bw={:.2}", bw));
        }
        if let Some(size) = preset.room_size {
            details.push(format!("size={:.2}", size));
        }
        if let Some(tail) = preset.reverb_tail {
            details.push(format!("tail={:.2}", tail));
        }
        if let Some(air) = preset.air {
            details.push(format!("air={:.2}", air));
        }
        if let Some(rate) = preset.rate {
            details.push(format!("rate={:+}%", rate));
        }
        if let Some(pitch) = preset.pitch {
            details.push(format!("pitch={:+}Hz", pitch));
        }
        let details_str = if details.is_empty() {
            "domyślne".to_string()
        } else {
            details.join(", ")
        };
        println!(
            "{:02}.  {:<22} {:<12} {}",
            idx + 1,
            name,
            preset.base,
            details_str
        );
    }
    println!("============================================================");
    Ok(())
}

pub fn run_estimate(
    input: Option<PathBuf>,
    provider: String,
    voice: String,
    cache_dir: Option<PathBuf>,
) -> Result<()> {
    let text = match input {
        Some(p) => std::fs::read_to_string(&p)
            .with_context(|| format!("Nie można odczytać pliku: {:?}", p))?,
        None => DEMO_SCENE.to_string(),
    };
    let cache_path = cache_dir.unwrap_or_else(get_default_cache_dir);
    let est = estimate_cost(&text, &provider, &voice, &cache_path);
    print_estimate_report(&est);
    Ok(())
}
