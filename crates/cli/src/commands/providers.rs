use anyhow::Result;
use audioticon_core::credentials;

use crate::args::ProvidersAction;

pub fn run_providers(action: Option<ProvidersAction>) -> Result<()> {
    match action.unwrap_or(ProvidersAction::List) {
        ProvidersAction::List => {
            let creds = credentials::load_credentials();
            let path = credentials::credentials_path();
            println!("🔑 Skonfigurowani dostawcy TTS ({})", path.display());
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

            let known_providers = [
                ("elevenlabs", "ElevenLabs v3 (Generative AI)"),
                ("openai", "OpenAI TTS"),
                ("azure", "Azure Speech Services"),
            ];

            for (id, name) in &known_providers {
                let env_key = match *id {
                    "elevenlabs" => std::env::var("ELEVENLABS_API_KEY").ok(),
                    "openai" => std::env::var("OPENAI_API_KEY").ok(),
                    "azure" => std::env::var("AZURE_SPEECH_KEY").ok(),
                    _ => None,
                };
                let saved = creds.providers.get(*id);
                let (source, masked, model) =
                    if let Some(k) = env_key.filter(|s| !s.trim().is_empty()) {
                        ("ENV", credentials::mask_api_key(&k), None)
                    } else if let Some(p) = saved {
                        if let Some(ref k) = p.api_key {
                            (
                                "credentials.toml",
                                credentials::mask_api_key(k),
                                p.model_id.clone(),
                            )
                        } else {
                            ("brak", "—".to_string(), None)
                        }
                    } else {
                        ("brak", "—".to_string(), None)
                    };

                let status_badge = if source == "brak" {
                    "⚪ Nieaktywny"
                } else {
                    "🟢 Skonfigurowany"
                };
                println!("  {} [{}]", name, id);
                println!("     Status:  {} ({})", status_badge, source);
                println!("     Klucz:   {}", masked);
                if let Some(m) = model {
                    println!("     Model:   {}", m);
                }
            }
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("💡 Aby dodać klucz: audioticon providers set elevenlabs --key <TWÓJ_KLUCZ>");
        }
        ProvidersAction::Set {
            provider,
            key,
            model,
            voice,
        } => {
            credentials::set_api_key(&provider, &key, model.as_deref(), voice.as_deref())?;
            println!(
                "✅ Zapisano klucz dla dostawcy '{}' w {:?} (prawa 0600)",
                provider,
                credentials::credentials_path()
            );
        }
        ProvidersAction::Check { provider } => {
            let clean = provider.trim().to_lowercase();
            if clean == "elevenlabs" {
                let key = credentials::get_api_key("elevenlabs").ok_or_else(|| {
                    anyhow::anyhow!(
                        "Brak klucza dla ElevenLabs. Użyj: audioticon providers set elevenlabs --key <KLUCZ>"
                    )
                })?;
                println!("🔍 Weryfikuję klucz ElevenLabs z oficjalnym API...");
                match credentials::verify_elevenlabs_key(&key) {
                    Ok(info) => {
                        println!("✅ Klucz ElevenLabs jest poprawny!");
                        println!("   Plan / Tier:     {}", info.tier);
                        println!(
                            "   Zużycie znaków:  {} / {}",
                            info.character_count, info.character_limit
                        );
                        println!(
                            "   Możliwość rozsz: {}",
                            if info.can_extend_character_limit {
                                "tak"
                            } else {
                                "nie"
                            }
                        );
                    }
                    Err(e) => {
                        eprintln!("❌ Błąd weryfikacji klucza ElevenLabs: {}", e);
                    }
                }
            } else {
                println!("ℹ️ Weryfikacja online jest obecnie dostępna dla ElevenLabs.");
            }
        }
        ProvidersAction::Remove { provider } => {
            if credentials::remove_api_key(&provider)? {
                println!("✅ Usunięto konfigurację dostawcy '{}'.", provider);
            } else {
                println!(
                    "ℹ️ Dostawca '{}' nie był skonfigurowany w credentials.toml.",
                    provider
                );
            }
        }
    }
    Ok(())
}
