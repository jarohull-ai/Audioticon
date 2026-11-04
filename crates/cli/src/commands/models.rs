use anyhow::Result;

use crate::args::ModelsAction;

pub fn run_models(action: Option<ModelsAction>) -> Result<()> {
    let act = action.unwrap_or(ModelsAction::List {
        lang: None,
        installed: false,
    });
    match act {
        ModelsAction::List { lang, installed } => {
            let dir = audioticon_core::models_dir();
            let filter_info = match (&lang, installed) {
                (Some(l), true) => format!(" (Filtr: język '{}', tylko zainstalowane)", l),
                (Some(l), false) => format!(" (Filtr: język '{}')", l),
                (None, true) => " (Tylko zainstalowane)".to_string(),
                (None, false) => String::new(),
            };
            println!("=========================================================================================================");
            println!(
                "🤖 AUDIOTICON — Menedżer Modeli Neuronowych Piper TTS (Offline){}",
                filter_info
            );
            println!("Folder modeli: {}", dir.display());
            println!("=========================================================================================================");
            println!(
                "{:<4} {:<26} {:<12} {:<12} {:<26} {:<8} Status",
                "Lp.", "Identyfikator", "Język", "Głos", "Licencja", "Rozmiar"
            );
            println!("---------------------------------------------------------------------------------------------------------");
            let mut models = audioticon_core::list_models();
            if let Some(ref l) = lang {
                let l_clean = l.trim().to_lowercase();
                models.retain(|m| m.lang.to_lowercase().starts_with(&l_clean));
            }
            if installed {
                models.retain(|m| m.installed);
            }
            for (idx, m) in models.iter().enumerate() {
                let status_str = if m.installed {
                    "✅ Zainstalowany (Gotowy)"
                } else {
                    "☁️  Dostępny do pobrania"
                };
                let gender_str = match m.gender.as_str() {
                    "female" => "Żeński 👩",
                    "male" => "Męski 👨",
                    _ => "Inny 🎙️",
                };
                println!(
                    "{:02}.  {:<26} {:<12} {:<12} {:<26} ~{:>2} MB  {}",
                    idx + 1,
                    m.id,
                    m.lang_name,
                    gender_str,
                    m.license,
                    m.size_mb,
                    status_str
                );
            }
            println!("=========================================================================================================");
            println!("💡 Wskazówka: pobierz model poleceniem:");
            println!(
                "   audioticon models download <nazwa>  (np. audioticon models download gosia)"
            );
            println!("=========================================================================================================");
        }
        ModelsAction::Download { name, force } => {
            println!(
                "⏳ Pobieranie modelu '{}' z repozytorium HuggingFace...",
                name
            );
            match audioticon_core::download_model(&name, force) {
                Ok(path) => {
                    println!("✅ Sukces! Model został pobrany i jest gotowy do syntezy:");
                    println!("   {}", path.display());
                }
                Err(err) => {
                    eprintln!("❌ Błąd pobierania modelu: {}", err);
                    std::process::exit(1);
                }
            }
        }
        ModelsAction::Remove { name } => match audioticon_core::remove_model(&name) {
            Ok(_) => {
                println!("🗑️ Model '{}' został pomyślnie usunięty z dysku.", name);
            }
            Err(err) => {
                eprintln!("❌ Błąd usuwania modelu: {}", err);
                std::process::exit(1);
            }
        },
        ModelsAction::Update => {
            println!("⏳ Pobieranie najnowszej bazy modeli voices.json z HuggingFace...");
            match audioticon_core::update_voices_catalog() {
                Ok(count) => {
                    println!(
                        "✅ Baza modeli zaktualizowana pomyślnie! Liczba dostępnych głosów: {}",
                        count
                    );
                }
                Err(err) => {
                    eprintln!("❌ Błąd aktualizacji bazy modeli: {}", err);
                    std::process::exit(1);
                }
            }
        }
    }
    Ok(())
}
