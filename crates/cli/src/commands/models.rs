use anyhow::Result;

use crate::args::ModelsAction;

pub fn run_models(action: Option<ModelsAction>) -> Result<()> {
    let act = action.unwrap_or(ModelsAction::List {
        lang: None,
        installed: false,
    });
    match act {
        ModelsAction::Hub => {
            let catalog = audioticon_core::get_curated_hub_catalog();
            let store = audioticon_core::ReceiptStore::load_or_default();

            println!("=========================================================================================================");
            println!("🌐 AUDIOTICON — Europe & Americas Neural Model Hub (Safe-Harbor EULA)");
            println!("=========================================================================================================");
            println!(
                "{:<4} {:<24} {:<20} {:<16} {:<16} {:<8} Licencja / EULA",
                "Lp.", "Model ID", "Autor", "Kategoria", "Języki", "Rozmiar"
            );
            println!("---------------------------------------------------------------------------------------------------------");

            for (idx, entry) in catalog.iter().enumerate() {
                let badge = match entry.license_type {
                    audioticon_core::LicenseType::CommercialSafe => "🟢 Komercyjna (MIT/Apache)",
                    audioticon_core::LicenseType::NonCommercialOnly => "🟡 Tylko niekomercyjna",
                    audioticon_core::LicenseType::OpenRail => "⚖️  OpenRAIL (Zgodna)",
                };

                let eula_status = if store.has_accepted(&entry.id, &entry.terms_fingerprint) {
                    " [✅ EULA Podpisana]"
                } else if entry.license_type != audioticon_core::LicenseType::CommercialSafe {
                    " [⚠️  Wymaga EULA]"
                } else {
                    ""
                };

                let lang_str = entry.languages.join(", ");

                println!(
                    "{:02}.  {:<24} {:<20} {:<16} {:<16} ~{:>3} MB  {}{}",
                    idx + 1,
                    entry.id,
                    entry.author,
                    format!("{:?}", entry.category),
                    lang_str,
                    entry.size_mb,
                    badge,
                    eula_status
                );
            }
            println!("=========================================================================================================");
            println!("💡 Aby zapoznać się z licencją i podpisać certyfikat EULA, uruchom:");
            println!("   audioticon models accept <model-id>  (np. audioticon models accept omnivoice-zeroshot)");
            println!("=========================================================================================================");
        }
        ModelsAction::Accept { name } => {
            let catalog = audioticon_core::get_curated_hub_catalog();
            let entry = catalog.iter().find(|m| m.id == name);

            if let Some(m) = entry {
                println!("=========================================================================================================");
                println!("📜 WARUNKI LICENCJI MODELU: {}", m.name);
                println!("Autor / Organizacja: {}", m.author);
                println!("Typ licencji: {:?}", m.license_type);
                println!("Licencja SPDX: {}", m.license_spdx);
                println!("Oficjalny URL licencji: {}", m.evidence_url);
                println!("---------------------------------------------------------------------------------------------------------");
                println!("{}", m.license_summary);
                println!("---------------------------------------------------------------------------------------------------------");

                let mut store = audioticon_core::ReceiptStore::load_or_default();
                let receipt = store.record_acceptance(
                    &m.id,
                    &m.terms_fingerprint,
                    true,
                    concat!("audioticon-cli/", env!("CARGO_PKG_VERSION")),
                );
                store.save()?;

                println!("✅ Licencja została zaakceptowana!");
                println!("Kryptograficzny certyfikat EULA:");
                println!("  Identyfikator: {}", receipt.receipt_id);
                println!("  Model:         {}", receipt.model_id);
                println!("  Sygnatura:     {}", receipt.signature);
                println!("  Czas akceptacji: {}", receipt.accepted_at_utc);
                println!("  Baza kwitów:   ~/.local/share/audioticon/model_receipts.json");
                println!("=========================================================================================================");
            } else {
                eprintln!("❌ Nie znaleziono modelu '{}' w katalogu Hub.", name);
                eprintln!("Użyj 'audioticon models hub', aby zobaczyć dostępne modele.");
                std::process::exit(1);
            }
        }
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
            // Weryfikacja Safe-Harbor EULA dla modeli wymagających akceptacji
            if let Err(e) = audioticon_core::check_hub_authorization(&name) {
                eprintln!("⚠️  Brak autoryzacji licencji dla modelu '{}'!", name);
                eprintln!("Szczegóły: {}", e);
                eprintln!("\nAby zapoznać się z licencją i ją zaakceptować, uruchom:");
                eprintln!("   audioticon models accept {}", name);
                std::process::exit(1);
            }

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
