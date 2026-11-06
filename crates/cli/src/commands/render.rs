use anyhow::{Context, Result};
use audioticon_core::{
    credentials, estimate_cost, export_to_m4b, export_to_mp3, find_model, models_dir,
    parse_size_string, presets, prune_cache_lru, AudioCertificate, AudioDirectorEngine,
    AudioWallet, AudiobookMetadata, EdgeTtsBridge, ElevenLabsBridge, EmojiDirector, ImportedBook,
    PiperBridge, KokoroBridge, KokoroVoice, PresetRegistry, ProjectConfig, SpeechSynthesizer, DEFAULT_MAX_CACHE_BYTES,
    DEFAULT_SAMPLE_RATE,
};
use std::path::PathBuf;
use std::sync::Arc;

use crate::args::{
    get_default_cache_dir, get_default_piper_model, BreathIntensityArg, ProductionStyleArg,
    TtsProvider, DEMO_EFFECTS_SCENE, DEMO_SCENE,
};
use crate::commands::cache::print_estimate_report;

#[allow(clippy::too_many_arguments)]
pub async fn run_render(
    input: Option<PathBuf>,
    output: PathBuf,
    provider: TtsProvider,
    voice: String,
    lang: String,
    model: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    title: Option<String>,
    author: Option<String>,
    narrator: Option<String>,
    year: Option<String>,
    demo: bool,
    demo_effects: bool,
    estimate: bool,
    max_cache_size: Option<String>,
    jobs: Option<usize>,
    style: ProductionStyleArg,
    cadence: bool,
    breath_intensity: BreathIntensityArg,
    bit_depth: u32,
) -> Result<()> {
    let (script_text, imported_title, imported_author) = match input {
        Some(ref file_path) if !demo && !demo_effects => {
            let ext = file_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext == "epub" || ext == "docx" {
                println!(
                    "📚 Wykryto plik książki ({}). Automatyczne importowanie rozdziałów...",
                    ext.to_uppercase()
                );
                let book = ImportedBook::from_file(file_path)?;
                let total_words = book.total_words();
                let total_min = book.total_estimated_seconds() / 60;
                println!(
                    "   Znaleziono {} rozdziałów (łącznie {} słów, ~{} min)\n",
                    book.chapters.len(),
                    total_words,
                    total_min
                );
                let script = book.to_script();
                (script, book.title, book.author)
            } else {
                let text = std::fs::read_to_string(file_path)
                    .with_context(|| format!("Nie można odczytać pliku: {:?}", file_path))?;
                (text, None, None)
            }
        }
        _ => {
            if demo_effects {
                (DEMO_EFFECTS_SCENE.to_string(), None, None)
            } else {
                if !demo {
                    println!(
                        "ℹ️ Nie podano pliku wejściowego. Uruchamiam scenę demonstracyjną (--demo).\n"
                    );
                }
                (DEMO_SCENE.to_string(), None, None)
            }
        }
    };

    let clean_lang = lang.to_lowercase();
    if clean_lang != "pl" && clean_lang != "en" {
        anyhow::bail!(
            "Nieobsługiwany język: '{}'. Audioticon obsługuje obecnie 'pl' (polski z verbalizerem) oraz 'en' (angielski z passthrough)",
            lang
        );
    }

    let cache_path = cache_dir.unwrap_or_else(get_default_cache_dir);
    std::fs::create_dir_all(&cache_path)?;

    let creds = credentials::load_credentials();

    if estimate {
        let (prov_str, est_voice) = match provider {
            TtsProvider::Edge => ("edge", voice.as_str()),
            TtsProvider::Piper => ("piper", voice.as_str()),
            TtsProvider::Kokoro => ("kokoro", voice.as_str()),
            TtsProvider::Elevenlabs => {
                let v = if voice == "pl-PL-MarekNeural" || voice == "default" {
                    creds
                        .providers
                        .get("elevenlabs")
                        .and_then(|p| p.voice_id.as_deref())
                        .unwrap_or("adam")
                } else {
                    &voice
                };
                ("elevenlabs", v)
            }
        };
        let est = estimate_cost(&script_text, prov_str, est_voice, &cache_path);
        print_estimate_report(&est);
        return Ok(());
    }

    let synthesizer: Arc<dyn SpeechSynthesizer> = match provider {
        TtsProvider::Edge => Arc::new(EdgeTtsBridge::new(&voice)),
        TtsProvider::Elevenlabs => {
            let api_key = credentials::get_api_key("elevenlabs").ok_or_else(|| {
                anyhow::anyhow!(
                    "Nie znaleziono klucza API dla ElevenLabs.\nSkonfiguruj klucz za pomocą:\n   audioticon providers set elevenlabs --key <TWÓJ_KLUCZ>\nlub ustaw zmienną środowiskową:\n   export ELEVENLABS_API_KEY=\"twój-klucz\""
                )
            })?;
            let creds = credentials::load_credentials();
            let el_config = creds.providers.get("elevenlabs");
            let configured_voice = el_config
                .and_then(|p| p.voice_id.as_deref())
                .unwrap_or("adam");
            let configured_model = el_config
                .and_then(|p| p.model_id.as_deref())
                .unwrap_or("eleven_multilingual_v2");

            let effective_voice = if voice == "pl-PL-MarekNeural" || voice == "default" {
                configured_voice
            } else {
                &voice
            };
            Arc::new(ElevenLabsBridge::with_model(
                api_key,
                effective_voice,
                configured_model,
            ))
        }
        TtsProvider::Piper => {
            let m_path = if let Some(ref custom_m) = model {
                if let Some(item) = find_model(&custom_m.to_string_lossy()) {
                    item.onnx_path
                        .unwrap_or_else(|| models_dir().join(format!("{}.onnx", item.id)))
                } else if custom_m.exists() {
                    custom_m.clone()
                } else {
                    models_dir().join(custom_m)
                }
            } else if let Some(item) = find_model(&voice) {
                item.onnx_path
                    .unwrap_or_else(|| models_dir().join(format!("{}.onnx", item.id)))
            } else {
                get_default_piper_model(&clean_lang)
            };

            if !m_path.exists() {
                anyhow::bail!(
                    "Nie znaleziono modelu Piper: {:?}.\nAby pobrać model, użyj polecenia:\n   audioticon models download <nazwa>  (np. audioticon models download gosia)",
                    m_path
                );
            }
            Arc::new(PiperBridge::new(m_path))
        }
        TtsProvider::Kokoro => {
            let m_path = if let Some(ref custom_m) = model {
                if custom_m.exists() || custom_m.is_absolute() {
                    custom_m.clone()
                } else {
                    models_dir().join(custom_m)
                }
            } else {
                KokoroBridge::default_model_path()
            };

            let kokoro_voice = if voice != "default" && voice != "pl-PL-MarekNeural" {
                voice.parse::<KokoroVoice>().unwrap_or(KokoroVoice::AfBella)
            } else {
                KokoroVoice::AfBella
            };

            Arc::new(KokoroBridge::new(m_path, Some(kokoro_voice)))
        }
    };

    let ext = output
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let is_m4b = ext == "m4b";
    let is_mp3 = ext == "mp3";
    let is_flac = ext == "flac";
    let is_wav = ext == "wav";

    if !is_m4b && !is_mp3 && !is_flac && !is_wav {
        anyhow::bail!(
            "Nieobsługiwany format pliku wyjściowego: {:?}. Obsługiwane rozszerzenia: .m4b, .mp3, .flac, .wav",
            output
        );
    }

    // Discover and load project config (audioticon.toml)
    let project_cfg = ProjectConfig::discover(input.as_deref());
    let mut registry = PresetRegistry::default();
    registry.load_project_config(&project_cfg);
    presets::set_active_registry(registry.clone());

    let project_meta = project_cfg.project.as_ref();
    let book_title = title
        .or(imported_title)
        .or_else(|| project_meta.and_then(|p| p.title.clone()))
        .unwrap_or_else(|| {
            input
                .as_ref()
                .and_then(|p| p.file_stem().and_then(|s| s.to_str()))
                .unwrap_or("Audioticon Audiobook")
                .to_string()
        });
    let book_author = author
        .or(imported_author)
        .or_else(|| project_meta.and_then(|p| p.author.clone()))
        .unwrap_or_else(|| "Audioticon Author".to_string());
    let book_narrator = narrator
        .or_else(|| project_meta.and_then(|p| p.narrator.clone()))
        .unwrap_or_else(|| synthesizer.voice_name().to_string());
    let book_year = year
        .or_else(|| project_meta.and_then(|p| p.year.clone()))
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| (1970 + d.as_secs() / 31_557_600).to_string())
                .unwrap_or_else(|_| "2026".to_string())
        });

    println!("============================================================");
    println!("🎧 AUDIOTICON — Renderowanie Scenariusza");
    println!("Tytuł:     {}", book_title);
    println!("Autor:     {}", book_author);
    println!("Lektor:    {}", book_narrator);
    println!("Rok:       {}", book_year);
    println!("Styl:      {}", style);
    if cadence {
        println!("Kadencja:  aktywna ({})", breath_intensity);
    } else {
        println!("Kadencja:  wyłączona");
    }
    println!("Provider:  {:?}", provider);
    println!("Cache dir: {:?}", cache_path);
    println!("Wyjście:   {:?}", output);
    if !project_cfg.presets.is_empty() {
        println!(
            "Presety:   zaimportowano {} wariantów z audioticon.toml",
            project_cfg.presets.len()
        );
    }
    println!("============================================================");

    let director = EmojiDirector::with_registry(registry)
        .with_lang(&lang)
        .with_style(style.into());
    let mut script = director.parse(&script_text);

    if cadence {
        let cadence_cfg = match breath_intensity {
            BreathIntensityArg::Subtle => audioticon_core::CadenceConfig::subtle(),
            BreathIntensityArg::Natural => audioticon_core::CadenceConfig::natural(),
            BreathIntensityArg::Off => audioticon_core::CadenceConfig::off(),
        };
        script = audioticon_core::apply_cadence(script, &cadence_cfg);
    }

    println!(
        "Oś czasu:  {} segmentów. Łączny czas pauz: {} ms",
        script.items.len(),
        script.total_pause_ms()
    );

    let mut engine = AudioDirectorEngine::new(cache_path.clone(), synthesizer);
    if let Some(j) = jobs {
        engine = engine.with_concurrency(j);
    }

    let wav_target = if is_wav {
        output.clone()
    } else {
        cache_path.join(format!("master_{}.wav", std::process::id()))
    };

    let start_time = std::time::Instant::now();
    let render_result = match engine.render(&script, &wav_target).await {
        Ok(res) => res,
        Err(err) => {
            if !is_wav {
                let _ = std::fs::remove_file(&wav_target);
            }
            return Err(err);
        }
    };
    let elapsed = start_time.elapsed();

    let metadata = AudiobookMetadata {
        title: Some(book_title),
        author: Some(book_author),
        narrator: Some(book_narrator),
        year: Some(book_year),
        language: Some(clean_lang.clone()),
    };

    if is_m4b {
        println!("\n📦 Eksportowanie audiobooka M4B z rozdziałami...");
        export_to_m4b(&wav_target, &output, &render_result.chapters, &metadata).await?;
        let _ = std::fs::remove_file(&wav_target);
    } else if is_flac {
        println!(
            "\n📦 Eksportowanie bezstratnego audiobooka FLAC ({}-bit Studio Master)...",
            bit_depth
        );
        audioticon_core::export_to_flac(
            &wav_target,
            &output,
            &render_result.chapters,
            &metadata,
            bit_depth,
        )
        .await?;
        let _ = std::fs::remove_file(&wav_target);
    } else if is_mp3 {
        println!("\n📦 Eksportowanie audiobooka MP3 z metadanymi i rozdziałami...");
        export_to_mp3(&wav_target, &output, &render_result.chapters, &metadata).await?;
        let _ = std::fs::remove_file(&wav_target);
    }

    if !render_result.chapters.is_empty() {
        println!(
            "\n📑 Wygenerowane rozdziały ({}):",
            render_result.chapters.len()
        );
        for ch in &render_result.chapters {
            let start_sec = ch.start_ms as f64 / 1000.0;
            let end_sec = ch.end_ms as f64 / 1000.0;
            println!(
                "  • [{:02}:{:05.2} - {:02}:{:05.2}] {}",
                (start_sec / 60.0) as u32,
                start_sec % 60.0,
                (end_sec / 60.0) as u32,
                end_sec % 60.0,
                ch.title
            );
        }
    }

    println!("\n============================================================");
    println!("✅ Gotowe! Plik wyjściowy: {:?}", output);
    let dur_sec = render_result.duration_ms as f64 / 1000.0;
    println!(
        "⏱️ Długość audio: {:02}:{:05.2} | Czas renderu: {:.2?}",
        (dur_sec / 60.0) as u32,
        dur_sec % 60.0,
        elapsed
    );

    // Audio Ledger: ACX / Storytel Cryptographic Certification
    let wallet = AudioWallet::load();
    if wallet.acx_certificate_unlocked {
        let cert_title = metadata.title.as_deref().unwrap_or("Audiobook");
        let cert_author = metadata.author.as_deref().unwrap_or("Audioticon");
        match AudioCertificate::create_and_save(
            &output,
            input.as_deref(),
            cert_title,
            cert_author,
            DEFAULT_SAMPLE_RATE,
            dur_sec,
            &wallet,
        ) {
            Ok(cert) => {
                println!("\n📜 AUDIO LEDGER — CERTYFIKAT ZGODNOŚCI ACX / STORYTEL");
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("  Identyfikator:    {}", cert.ledger_id);
                println!(
                    "  Zgodność ACX:     {}",
                    if cert.acx_compliant {
                        "✅ ZGODNY (Audible Ready)"
                    } else {
                        "⚠️ Poza zakresem"
                    }
                );
                println!(
                    "  Zgodność Storytel:{}",
                    if cert.storytel_compliant {
                        "✅ ZGODNY (EBU R128)"
                    } else {
                        "⚠️ Poza zakresem"
                    }
                );
                println!(
                    "  Poziom głośności: {:.1} LUFS (Norma: -23.0 do -19.0 LUFS)",
                    cert.master_lufs
                );
                println!(
                    "  True Peak:        {:.1} dBFS (Norma: <= -3.0 dBFS)",
                    cert.true_peak_db
                );
                println!("  Suma SHA-256:     {}...", &cert.audio_sha256[..16]);
                println!(
                    "  Zapisano certyfikat: {:?}",
                    output.with_extension("certificate.json")
                );
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            }
            Err(e) => {
                eprintln!("⚠️ Nie udało się wygenerować certyfikatu ACX: {}", e);
            }
        }
    } else {
        println!("\n💡 Wskazówka: Odblokuj oficjalny Certyfikat Zgodności ACX/Storytel (Audio Ledger) w Pakiecie Pisarz!");
        println!("   Wpisz w terminalu: audioticon wallet topup");
    }

    // Automatic LRU cache cleanup
    let limit_bytes = match max_cache_size {
        Some(ref sz) => parse_size_string(sz).unwrap_or(DEFAULT_MAX_CACHE_BYTES),
        None => DEFAULT_MAX_CACHE_BYTES,
    };
    if let Ok((pruned, freed)) = prune_cache_lru(&cache_path, limit_bytes) {
        if pruned > 0 {
            println!(
                "🧹 Porządkowanie cache (LRU): usunięto {} starych plików TTS, zwolniono {:.2} MB",
                pruned,
                freed as f64 / (1024.0 * 1024.0)
            );
        }
    }
    println!("============================================================");

    Ok(())
}
