use anyhow::Result;
use audioticon_core::{import_custom_sfx, list_curated_essentials, list_installed_sfx, sfx_dir};
use std::path::PathBuf;

use crate::args::SfxAction;

pub fn run_sfx(action: Option<SfxAction>) -> Result<()> {
    match action.unwrap_or(SfxAction::List) {
        SfxAction::List => {
            let items = list_installed_sfx();
            println!("🎵 Biblioteka Efektów Dźwiękowych (SFX i Tła)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("📁 Globalny katalog SFX: {}", sfx_dir().display());
            println!("📁 Lokalny katalog SFX:  ./sfx/");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );

            if items.is_empty() {
                println!("  (Brak zainstalowanych plików audio w bibliotece SFX)");
                println!(
                    "  💡 Silnik korzysta z wbudowanej syntezy proceduralnej dla znanych emotikon."
                );
                println!("  💡 Aby zaimportować własny plik audio: audioticon sfx import sciezka/do/dzwieku.wav");
                println!("  💡 Zobacz rekomendowane dźwięki:      audioticon sfx catalog");
            } else {
                for item in items {
                    let loc_str = if item.is_project_local {
                        "lokalny (./sfx)"
                    } else {
                        "globalny"
                    };
                    let size_kb = item.size_bytes / 1024;
                    println!(
                        "  • {:<24} [{}] ({}, {} KB, .{})",
                        item.name, item.category, loc_str, size_kb, item.extension
                    );
                }
            }
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
        }
        SfxAction::Catalog => {
            let curated = list_curated_essentials();
            println!(
                "📚 Rekomendowane Efekty Studyjne do Audiobooków (Darmowe CC0 / Public Domain)"
            );
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("Licencja: CC0 (Można używać bez ograniczeń w projektach komercyjnych i prywatnych)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            for item in curated {
                println!("  📦 {:<20} | Kategoria: {}", item.id, item.category);
                println!("     Opis:  {}", item.description);
                println!("     Tagi:  {}", item.tags.join(", "));
                println!();
            }
            println!("🌐 Darmowe bazy do pobierania sampli w formacie WAV/OGG:");
            println!("   • https://freesound.org/ (Wyszukaj z filtrem 'Creative Commons 0')");
            println!("   • https://sonniss.com/gameaudioarchive (Darmowe gigabajty Foley/SFX)");
            println!("   • https://kenney.nl/assets/category:Audio (100% CC0)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("💡 Pobierz dźwięk i dodaj do projektu: audioticon sfx import plik.wav --name nazwa");
        }
        SfxAction::Import {
            file,
            name,
            project,
        } => {
            let target_dir = if project {
                PathBuf::from("sfx")
            } else {
                sfx_dir()
            };
            let dest = import_custom_sfx(&file, name.as_deref(), &target_dir)?;
            println!("✅ Zaimportowano efekt dźwiękowy!");
            println!("   Plik źródłowy: {:?}", file);
            println!("   Zapisano do:   {:?}", dest);
            let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("efekt");
            println!(
                "   💡 Użycie w scenariuszu: [sfx:{}] lub przypisz do emotki w audioticon.toml",
                stem
            );
        }
    }
    Ok(())
}
