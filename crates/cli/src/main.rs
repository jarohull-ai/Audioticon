use anyhow::{Context, Result};
use audioticon_core::{
    base64_decode, compute_segment_hash, credentials, download_model, estimate_cost, export_to_m4b,
    export_to_mp3, find_model, list_models, models_dir, presets, remove_model,
    update_voices_catalog, AudioDirectorEngine, AudiobookMetadata, CostEstimate, DirectedItem,
    EdgeTtsBridge, ElevenLabsBridge, EmojiDirector, ImportedBook, PiperBridge, PresetRegistry,
    ProjectConfig, SpeechSynthesizer,
};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tiny_http::Header;
use tokio::sync::Mutex;

static REQ_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Parser)]
#[command(name = "audioticon")]
#[command(author = "Jarosław Kuchta")]
#[command(version)]
#[command(about = "🎧 Audioticon — Reżyser audiobooków sterowany emotikonami z cachem zdań i filtrami DSP", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum TtsProvider {
    /// Microsoft Edge TTS (wysoka jakość online)
    Edge,
    /// Piper TTS (100% lokalny, ultra szybki model neuronowy ONNX)
    Piper,
    /// ElevenLabs v3 API (profesjonalne głosy generative AI)
    Elevenlabs,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum ScriptFormat {
    /// Format ElevenLabs v3 z tagami aktorskimi ([whispers], [sighs], [shouts])
    Elevenlabs,
    /// Format ChatTTS z tagami inline ([whisper], [sigh], [shout])
    Chattts,
    /// Strukturalna oś czasu z deskryptorami stylu promptów
    Timeline,
}

#[derive(Subcommand)]
enum Commands {
    /// Renderuje tekst ze znacznikami emoji do pliku audio (.wav lub .mp3)
    Render {
        /// Ścieżka do pliku z tekstem ze znacznikami emoji (lub pomiń i użyj --demo)
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Ścieżka do pliku wyjściowego (.wav lub .mp3)
        #[arg(short, long, default_value = "output.mp3")]
        output: PathBuf,

        /// Silnik syntezy mowy: edge (online) lub piper (lokalny offline)
        #[arg(long, value_enum, default_value_t = TtsProvider::Edge)]
        provider: TtsProvider,

        /// Identyfikator głosu Edge-TTS (np. pl-PL-MarekNeural, pl-PL-ZofiaNeural)
        #[arg(short, long, default_value = "pl-PL-MarekNeural")]
        voice: String,

        /// Język tekstu (pl lub en - wyłącza polską normalizację liczb dla języka angielskiego)
        #[arg(long, default_value = "pl")]
        lang: String,

        /// Ścieżka do lokalnego modelu ONNX dla Piper (domyślnie: ~/.local/share/audioticon/models/)
        #[arg(long)]
        model: Option<PathBuf>,

        /// Własny katalog cache (domyślnie ~/.cache/audioticon)
        #[arg(short, long)]
        cache_dir: Option<PathBuf>,

        /// Tytuł audiobooka (metadane M4B / MP3)
        #[arg(long)]
        title: Option<String>,

        /// Autor książki (metadane M4B / MP3)
        #[arg(long)]
        author: Option<String>,

        /// Lektor / narrator (metadane M4B / MP3)
        #[arg(long)]
        narrator: Option<String>,

        /// Rok wydania (metadane M4B / MP3)
        #[arg(long)]
        year: Option<String>,

        /// Uruchamia wbudowaną scenę demonstracyjną (Stacja Polarna Aurora)
        #[arg(long)]
        demo: bool,

        /// Uruchamia scenę demonstracyjną wszystkich efektów akustycznych (telefon, radio, katedra, myśli, robot)
        #[arg(long)]
        demo_effects: bool,

        /// Wyświetl tylko szacunkową liczbę znaków i koszt bez generowania audio
        #[arg(long)]
        estimate: bool,
    },
    /// Zarządzanie lokalnym buforem audio (cache)
    Cache {
        /// Wyczyść wszystkie zapisane segmenty
        #[arg(long)]
        clear: bool,

        /// Własny katalog bufora
        #[arg(short, long)]
        dir: Option<PathBuf>,
    },
    /// Eksportuje zareżyserowany tekst z emotikonami do formatów modeli ekspresyjnych (ElevenLabs, ChatTTS)
    Script {
        /// Ścieżka do pliku z tekstem ze znacznikami emoji (lub pomiń i użyj --demo)
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Format wyjściowy tagów aktorskich
        #[arg(short, long, value_enum, default_value_t = ScriptFormat::Elevenlabs)]
        format: ScriptFormat,

        /// Język tekstu (pl lub en)
        #[arg(long, default_value = "pl")]
        lang: String,

        /// Uruchamia wbudowaną scenę demonstracyjną
        #[arg(long)]
        demo: bool,
    },
    /// Wyświetla listę dostępnych presetów akustycznych (systemowych i z audioticon.toml)
    Presets {
        /// Ścieżka do pliku projektu audioticon.toml
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Uruchamia lokalny serwer Audioticon Studio (Workbench) z interfejsem w przeglądarce
    #[command(alias = "studio")]
    Serve {
        /// Port serwera HTTP (domyślnie 3000)
        #[arg(short, long, default_value_t = 3000)]
        port: u16,

        /// Adres nasłuchu (domyślnie 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Domyślny silnik syntezy (piper lub edge)
        #[arg(long, value_enum, default_value_t = TtsProvider::Piper)]
        provider: TtsProvider,

        /// Domyślny język (pl lub en)
        #[arg(long, default_value = "pl")]
        lang: String,

        /// Ścieżka do lokalnego modelu ONNX dla Piper
        #[arg(long)]
        model: Option<PathBuf>,

        /// Nie otwieraj automatycznie przeglądarki
        #[arg(long)]
        no_browser: bool,
    },
    /// Zarządzanie lokalnymi modelami neuronowymi Piper TTS (offline)
    Models {
        #[command(subcommand)]
        action: Option<ModelsAction>,
    },
    /// Importuje całą książkę (.epub, .docx, .txt, .md) z autodetekcją rozdziałów i konwersją do scenariusza
    Import {
        /// Ścieżka do pliku książki (.epub, .docx, .txt, .md)
        #[arg(short, long)]
        input: PathBuf,

        /// Ścieżka do wyjściowego pliku ze scenariuszem (.txt)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Nadpisz tytuł książki
        #[arg(long)]
        title: Option<String>,

        /// Nadpisz autora książki
        #[arg(long)]
        author: Option<String>,

        /// Wyświetl tylko strukturę rozdziałów i statystyki bez zapisu pliku
        #[arg(long)]
        preview: bool,

        /// Podziel i zapisz rozdziały do osobnych plików w podanym katalogu
        #[arg(long)]
        split_dir: Option<PathBuf>,
    },
    /// Zarządzanie kluczami API zewnętrznych dostawców (ElevenLabs, OpenAI, Azure)
    Providers {
        #[command(subcommand)]
        action: Option<ProvidersAction>,
    },
    /// Szacuje liczbę znaków i koszt syntezy skryptu (uwzględniając trafienia w cache)
    Estimate {
        /// Ścieżka do pliku ze scenariuszem (lub pomiń i użyj tekstu demonstracyjnego)
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Dostawca syntezy mowy: elevenlabs, openai, azure, piper, edge
        #[arg(long, default_value = "elevenlabs")]
        provider: String,

        /// Identyfikator głosu (np. adam, antoni, rachel)
        #[arg(short, long, default_value = "adam")]
        voice: String,

        /// Własny katalog cache
        #[arg(short, long)]
        cache_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum ProvidersAction {
    /// Wyświetla listę skonfigurowanych dostawców i zamaskowane klucze API
    List,
    /// Konfiguruje klucz API dostawcy (zapisywany bezpiecznie w credentials.toml)
    Set {
        /// Identyfikator dostawcy (np. elevenlabs, openai, azure)
        provider: String,
        /// Klucz API
        #[arg(short, long)]
        key: String,
        /// Domyślny model (opcjonalny, np. eleven_multilingual_v2)
        #[arg(short, long)]
        model: Option<String>,
        /// Domyślny głos (opcjonalny, np. adam, antoni)
        #[arg(short, long)]
        voice: Option<String>,
    },
    /// Weryfikuje ważność klucza API i sprawdza stan konta (np. subskrypcja ElevenLabs)
    Check {
        /// Identyfikator dostawcy do sprawdzenia (domyślnie: elevenlabs)
        #[arg(default_value = "elevenlabs")]
        provider: String,
    },
    /// Usuwa klucz API dostawcy z konfiguracji
    Remove {
        /// Identyfikator dostawcy
        provider: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum ModelsAction {
    /// Wyświetla listę dostępnych w katalogu i zainstalowanych modeli offline
    List {
        /// Filtruj po kodzie języka (np. pl, en, de, uk, es)
        #[arg(short, long)]
        lang: Option<String>,
        /// Pokaż tylko zainstalowane modele
        #[arg(short, long)]
        installed: bool,
    },
    /// Pobiera wybrany model z oficjalnego repozytorium HuggingFace
    Download {
        /// Identyfikator lub alias modelu (np. gosia, darkman, lessac, amy, pl_PL-gosia-medium)
        name: String,
        /// Wymuś ponowne pobranie nawet jeśli plik już istnieje
        #[arg(long)]
        force: bool,
    },
    /// Usuwa pobrany model offline z dysku
    Remove {
        /// Identyfikator lub alias modelu do usunięcia
        name: String,
    },
    /// Pobiera najnowszą bazę voices.json z oficjalnego repozytorium Piper
    Update,
}

fn get_default_cache_dir() -> PathBuf {
    if let Some(user_cache) = dirs::cache_dir() {
        user_cache.join("audioticon")
    } else {
        std::env::temp_dir().join("audioticon_cache")
    }
}

fn get_default_piper_model(lang: &str) -> PathBuf {
    let filename = if lang.starts_with("en") {
        "en_US-lessac-medium.onnx"
    } else {
        "pl_PL-darkman-medium.onnx"
    };
    audioticon_core::models::models_dir().join(filename)
}

const DEMO_SCENE: &str = r#"
📖 Prolog: Stacja Aurora 🛑

Spitsbergen, 15.01.2026. 🛑

Jan Zawadzki siedział przy pulpicie stacji badawczej na Spitsbergenie. ⏸️
Za oknem szalała zamieć, jak przez większość nocy polarnej. 🛑
Na blacie stał kubek z zimną herbatą, 😮‍💨 której nie pił od godzin. 🛑

Na stole leżał 💥dziennik pokładowy. 🛑

🐢 Przetarty na krawędziach... 😮‍💨 od ciągłego przeglądania... od noszenia w skafandrze arktycznym. 🛑
Okładka zmatowiała, grzbiet przetarty w kilku miejscach. 🛑

W środku... setki pomiarów i odczytów. 🛑
📉Temperatur. ⏸️
📉Wiatrów. ⏸️
📉Współrzędnych. 🛑

Niektóre wpisy nanoszono pośpiesznie ołówkiem. ⏸️
Inne były staranne... 😮‍💨 z czasów, gdy na stacji panował jeszcze całkowity spokój. 🛑

📖 Rozdział 1: Sygnał z Lodu 🛑

Nazwał ten zapis: 💥Projekt Aurora. 🛑

Otworzył dziennik na stronie z pierwszego kontaktu. 🛑
15.01.2025. ⏸️
Godzina 03:14. ⏸️
Sektor Północny. 🛑

Przez dłuższą chwilę patrzył na te współrzędne. 🛑
Potem zamknął dziennik. 🛑
Nie musiał czytać dalej. 🛑
🤫 Pamiętał każdy szczegół. 🛑
"#;

const DEMO_EFFECTS_SCENE: &str = r#"
Oto demonstracja zaawansowanych efektów akustycznych Audioticon. 🛑

Narrator mówi czystym, studyjnym głosem. ⏸️
Ale nagle odzywa się interkom satelitarny. 🛑
📱 "Halo? Stacja Aurora, słyszycie nas? Zbliża się arktyczny sztorm!" 🛑

W tle trzaskają zakłócenia wojskowej radiostacji krótkofalowej. 🛑
📻 "Baza do patrolu śnieżnego. Zgłaszam nagły spadek ciśnienia. Natychmiast powrót do hangaru. Odbiór." 🛑

Kierownik stacji włącza tubę i krzyczy przez megafon w hali maszyn: 🛑
📢 "Wszyscy zabezpieczyć śluzy zewnętrzne! To nie są ćwiczenia!" 🛑

Jan wchodzi w głąb wysokiej, opuszczonej kopuły obserwatorium. 🛑
🏛️ Kroki niosły się głuchym echem po stalowych i kamiennych łukach. 🛑

Schodzi technicznymi schodami do podziemnego tunelu w wiecznej zmarzlinie. 🛑
🏰 Ciemność była absolutna, a krople lodu stukały cicho o metalowe podpory. 🛑

Zatrzymał się. 🛑
💭 "Czy aparatura pomiarowa wytrzyma to ciśnienie? Musimy utrzymać zasilanie generatora." 🛑

Nagle z terminala wyłonił się syntetyczny głos autonomicznego systemu stacji: 🛑
🤖 "Autodiagnostyka zakończona. Wykryto anomalię w reaktorze pomocniczym." 🛑

I znowu zapadła lodowata cisza. 🛑
"#;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Cache { clear, dir } => {
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
        }
        Commands::Render {
            input,
            output,
            provider,
            voice,
            lang,
            model,
            cache_dir,
            title,
            author,
            narrator,
            year,
            demo,
            demo_effects,
            estimate,
        } => {
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
                        let text = std::fs::read_to_string(file_path).with_context(|| {
                            format!("Nie można odczytać pliku: {:?}", file_path)
                        })?;
                        (text, None, None)
                    }
                }
                _ => {
                    if demo_effects {
                        (DEMO_EFFECTS_SCENE.to_string(), None, None)
                    } else {
                        if !demo {
                            println!("ℹ️ Nie podano pliku wejściowego. Uruchamiam scenę demonstracyjną (--demo).\n");
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
            };

            let ext = output
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let is_m4b = ext == "m4b";
            let is_mp3 = ext == "mp3";
            let is_wav = ext == "wav";

            if !is_m4b && !is_mp3 && !is_wav {
                anyhow::bail!(
                    "Nieobsługiwany format pliku wyjściowego: {:?}. Obsługiwane rozszerzenia: .m4b, .mp3, .wav",
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

            let director = EmojiDirector::with_registry(registry).with_lang(&lang);
            let script = director.parse(&script_text);
            println!(
                "Oś czasu:  {} segmentów. Łączny czas pauz: {} ms",
                script.items.len(),
                script.total_pause_ms()
            );

            let engine = AudioDirectorEngine::new(cache_path.clone(), synthesizer);

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
            println!("============================================================");
        }
        Commands::Script {
            input,
            format,
            lang,
            demo,
        } => {
            let clean_lang = lang.to_lowercase();
            if clean_lang != "pl" && clean_lang != "en" {
                anyhow::bail!(
                    "Nieobsługiwany język: '{}'. Audioticon obsługuje obecnie 'pl' oraz 'en'",
                    lang
                );
            }

            let script_text = match input {
                Some(file_path) if !demo => std::fs::read_to_string(&file_path)
                    .with_context(|| format!("Nie można odczytać pliku: {:?}", file_path))?,
                _ => DEMO_SCENE.to_string(),
            };
            let director = EmojiDirector::default().with_lang(&clean_lang);
            let script = director.parse(&script_text);

            match format {
                ScriptFormat::Elevenlabs => {
                    println!("{}", script.to_elevenlabs_script());
                }
                ScriptFormat::Chattts => {
                    println!("{}", script.to_chattts_script());
                }
                ScriptFormat::Timeline => {
                    println!("============================================================");
                    println!("🎭 AUDIOTICON — Partytura Aktorska Osi Czasu");
                    println!("============================================================");
                    for (idx, item) in script.items.iter().enumerate() {
                        match item {
                            DirectedItem::Chapter(title) => {
                                println!("\n[{:02}] 📖 ROZDZIAŁ: {}", idx, title)
                            }
                            DirectedItem::Silence(ms) => {
                                println!("  [{:02}] ⏸️ PAUZA: {} ms", idx, ms)
                            }
                            DirectedItem::Breath(ms) => {
                                println!("  [{:02}] 😮‍💨 ODDECH / WESTCHNIENIE: {} ms", idx, ms)
                            }
                            DirectedItem::Speech(seg) => {
                                let mut meta = Vec::new();
                                if let Some(style) = seg.style_prompt() {
                                    meta.push(format!("Ekspresja: \"{}\"", style));
                                }
                                if let Some(ref filter) = seg.filter {
                                    meta.push(format!("Filtr: {}", filter));
                                }
                                if seg.rate_mod != -6 {
                                    meta.push(format!("Tempo: {:+}%", seg.rate_mod));
                                }
                                if seg.pitch_mod != -4 {
                                    meta.push(format!("Wysokość: {:+}Hz", seg.pitch_mod));
                                }
                                let meta_str = if meta.is_empty() {
                                    String::new()
                                } else {
                                    format!(" [{}]", meta.join(" | "))
                                };
                                println!("  [{:02}] 🗣️ \"{}\"{}", idx, seg.text, meta_str);
                            }
                        }
                    }
                    println!("\n============================================================");
                }
            }
        }
        Commands::Presets { config } => {
            let project_cfg = if let Some(cfg_path) = config {
                ProjectConfig::load_from_path(&cfg_path)?
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
        }
        Commands::Serve {
            port,
            host,
            provider,
            lang,
            model,
            no_browser,
        } => {
            run_studio_server(host, port, provider, lang, model, no_browser).await?;
        }
        Commands::Models { action } => {
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
                    println!("   audioticon models download <nazwa>  (np. audioticon models download gosia)");
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
        }
        Commands::Import {
            input,
            output,
            title,
            author,
            preview,
            split_dir,
        } => {
            println!("=========================================================================================");
            println!("📚 AUDIOTICON — Importer Książek (.epub, .docx, .txt, .md)");
            println!("Plik źródłowy: {:?}", input);
            println!("=========================================================================================");

            let mut book = ImportedBook::from_file(&input)?;

            if let Some(t) = title {
                book.title = Some(t);
            }
            if let Some(a) = author {
                book.author = Some(a);
            }

            let title_str = book.title.as_deref().unwrap_or("Bez tytułu");
            let author_str = book.author.as_deref().unwrap_or("Nieznany autor");
            let total_words = book.total_words();
            let total_sec = book.total_estimated_seconds();
            let total_min = total_sec / 60;

            println!("Tytuł:   {}", title_str);
            println!("Autor:   {}", author_str);
            println!(
                "Rozmiar: {} rozdziałów • {} słów • szacowany czas czytania: ~{} min ({:02}:{:02}h)",
                book.chapters.len(),
                total_words,
                total_min,
                total_sec / 3600,
                (total_sec % 3600) / 60
            );
            println!("-----------------------------------------------------------------------------------------");
            println!(
                "{:<4} {:<40} {:<12} Szacowany czas",
                "Nr", "Tytuł rozdziału", "Liczba słów"
            );
            println!("-----------------------------------------------------------------------------------------");

            for ch in &book.chapters {
                let min = ch.estimated_seconds / 60;
                let sec = ch.estimated_seconds % 60;
                println!(
                    "{:02}.  {:<40} {:<12} ~{:02}:{:02}",
                    ch.index,
                    ch.title.chars().take(38).collect::<String>(),
                    format!("{} słów", ch.word_count),
                    min,
                    sec
                );
            }
            println!("=========================================================================================");

            if let Some(ref dir) = split_dir {
                std::fs::create_dir_all(dir)?;
                for ch in &book.chapters {
                    let file_name = format!("rozdzial_{:02}.txt", ch.index);
                    let file_path = dir.join(file_name);
                    let ch_script = format!("📖 {} 🛑\n\n{}", ch.title, ch.content);
                    std::fs::write(&file_path, ch_script)?;
                }
                println!(
                    "✅ Zapisano {} rozdziałów w katalogu: {:?}",
                    book.chapters.len(),
                    dir
                );
            }

            if !preview {
                let script = book.to_script();
                let out_path = output.unwrap_or_else(|| input.with_extension("scenariusz.txt"));
                std::fs::write(&out_path, &script)?;
                println!("✅ Wygenerowano pełny scenariusz Audioticon:");
                println!("   {:?}", out_path);
                println!("💡 Możesz go teraz wyrenderować poleceniem:");
                println!(
                    "   audioticon render --input {:?} --output audiobook.m4b",
                    out_path
                );
            }
        }
        Commands::Estimate {
            input,
            provider,
            voice,
            cache_dir,
        } => {
            let text = match input {
                Some(p) => std::fs::read_to_string(&p)
                    .with_context(|| format!("Nie można odczytać pliku: {:?}", p))?,
                None => DEMO_SCENE.to_string(),
            };
            let cache_path = cache_dir.unwrap_or_else(get_default_cache_dir);
            let est = estimate_cost(&text, &provider, &voice, &cache_path);
            print_estimate_report(&est);
        }
        Commands::Providers { action } => {
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
        }
    }

    Ok(())
}

fn print_estimate_report(est: &CostEstimate) {
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

const STUDIO_HTML: &str = include_str!("../../../docs/index.html");
const LOGO_PNG: &[u8] = include_bytes!("../../../docs/logo.png");

#[derive(serde::Deserialize)]
struct ApiRenderRequest {
    text: String,
    #[serde(default = "default_provider_str")]
    provider: String,
    #[serde(default = "default_lang_str")]
    lang: String,
    #[serde(default = "default_voice_str")]
    voice: String,
    #[serde(default)]
    model: Option<String>,
}
fn default_provider_str() -> String {
    "piper".to_string()
}
fn default_lang_str() -> String {
    "pl".to_string()
}
fn default_voice_str() -> String {
    "pl-PL-MarekNeural".to_string()
}

#[derive(serde::Deserialize)]
struct ApiParseRequest {
    text: String,
    #[serde(default = "default_lang_str")]
    lang: String,
}

#[derive(serde::Deserialize)]
struct ApiExportRequest {
    #[serde(default)]
    text: String,
    #[serde(default = "default_provider_str")]
    provider: String,
    #[serde(default = "default_lang_str")]
    lang: String,
    #[serde(default = "default_voice_str")]
    voice: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "default_export_format")]
    format: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    narrator: Option<String>,
    #[serde(default)]
    year: Option<String>,
}

fn default_export_format() -> String {
    "m4b".to_string()
}

fn url_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(val as char);
                i += 3;
            } else {
                result.push('%');
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

type PiperCache = Arc<Mutex<HashMap<PathBuf, Arc<PiperBridge>>>>;

async fn resolve_synthesizer(
    provider: &str,
    voice: &str,
    model: Option<&str>,
    clean_lang: &str,
    default_model: Option<&PathBuf>,
    piper_cache: &PiperCache,
) -> Result<Arc<dyn SpeechSynthesizer>, String> {
    if provider == "edge" {
        Ok(Arc::new(EdgeTtsBridge::new(voice)))
    } else if provider == "elevenlabs" {
        let api_key = credentials::get_api_key("elevenlabs").ok_or_else(|| {
            "Brak skonfigurowanego klucza API dla ElevenLabs. Skonfiguruj klucz w ustawieniach dostawców API.".to_string()
        })?;
        let creds = credentials::load_credentials();
        let el_config = creds.providers.get("elevenlabs");
        let configured_voice = el_config
            .and_then(|p| p.voice_id.as_deref())
            .unwrap_or("adam");
        let configured_model = el_config
            .and_then(|p| p.model_id.as_deref())
            .unwrap_or("eleven_multilingual_v2");

        let effective_voice =
            if voice == "pl-PL-MarekNeural" || voice == "default" || voice.is_empty() {
                configured_voice
            } else {
                voice
            };
        Ok(Arc::new(ElevenLabsBridge::with_model(
            api_key,
            effective_voice,
            configured_model,
        )))
    } else {
        let m_path = if let Some(custom_m) = model {
            if let Some(item) = find_model(custom_m) {
                item.onnx_path
                    .unwrap_or_else(|| models_dir().join(format!("{}.onnx", item.id)))
            } else {
                let p = PathBuf::from(custom_m);
                if p.exists() {
                    p
                } else {
                    models_dir().join(custom_m)
                }
            }
        } else if let Some(item) = find_model(voice) {
            item.onnx_path
                .unwrap_or_else(|| models_dir().join(format!("{}.onnx", item.id)))
        } else {
            default_model
                .cloned()
                .unwrap_or_else(|| get_default_piper_model(clean_lang))
        };

        if !m_path.exists() {
            return Err(format!(
                "Nie znaleziono modelu Piper: {:?}. Pobierz model w Studio lub poleceniem 'audioticon models download <nazwa>'",
                m_path
            ));
        }

        let mut cache = piper_cache.lock().await;
        if let Some(existing) = cache.get(&m_path) {
            Ok(existing.clone())
        } else {
            let bridge = Arc::new(PiperBridge::new(m_path.clone()));
            cache.insert(m_path, bridge.clone());
            Ok(bridge)
        }
    }
}

fn is_allowed_origin(origin: &str) -> bool {
    let clean = origin.trim().trim_end_matches('/');
    clean == "https://jarohull-ai.github.io"
        || clean == "http://localhost"
        || clean.starts_with("http://localhost:")
        || clean == "http://127.0.0.1"
        || clean.starts_with("http://127.0.0.1:")
        || clean == "https://localhost"
        || clean.starts_with("https://localhost:")
        || clean == "https://127.0.0.1"
        || clean.starts_with("https://127.0.0.1:")
}

fn get_cors_header(request: &tiny_http::Request) -> Option<Header> {
    for header in request.headers() {
        if header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("origin")
        {
            let origin = header.value.as_str();
            if is_allowed_origin(origin) {
                return Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin.as_bytes())
                    .ok();
            }
        }
    }
    None
}

fn has_json_content_type(request: &tiny_http::Request) -> bool {
    for header in request.headers() {
        if header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("content-type")
        {
            let val = header.value.as_str().to_lowercase();
            return val.contains("application/json");
        }
    }
    false
}

async fn handle_studio_request(
    mut request: tiny_http::Request,
    piper_cache: PiperCache,
    default_model: Option<PathBuf>,
) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let cors_header = get_cors_header(&request);

    // CSRF & Cross-Origin Protection: reject untrusted Origin
    for header in request.headers() {
        if header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("origin")
        {
            let origin = header.value.as_str();
            if !is_allowed_origin(origin) {
                let err_json = serde_json::json!({
                    "error": "403 Forbidden: Nieautoryzowane źródło żądania (Untrusted Origin rejected)",
                    "origin": origin
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                let _ = request.respond(resp.with_status_code(403));
                return;
            }
        }
    }

    // Handle OPTIONS preflight
    if method == tiny_http::Method::Options {
        if let Some(ref h) = cors_header {
            let mut resp = tiny_http::Response::from_string("");
            resp.add_header(h.clone());
            if let Ok(vary) = Header::from_bytes(&b"Vary"[..], &b"Origin"[..]) {
                resp.add_header(vary);
            }
            let cors_methods = Header::from_bytes(
                &b"Access-Control-Allow-Methods"[..],
                &b"GET, POST, OPTIONS"[..],
            )
            .unwrap();
            let cors_headers =
                Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type"[..])
                    .unwrap();
            resp.add_header(cors_methods);
            resp.add_header(cors_headers);
            let _ = request.respond(resp.with_status_code(204));
        } else {
            let mut resp = tiny_http::Response::from_string("{\"error\": \"403 Forbidden\"}");
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            let _ = request.respond(resp.with_status_code(403));
        }
        return;
    }

    // Enforce Content-Type: application/json on all JSON API endpoints to prevent CSRF simple requests
    if method == tiny_http::Method::Post
        && (url.starts_with("/api/providers")
            || url.starts_with("/api/models")
            || url.starts_with("/api/render")
            || url.starts_with("/api/estimate")
            || url.starts_with("/api/import")
            || url.starts_with("/api/parse")
            || url.starts_with("/api/cache"))
        && !has_json_content_type(&request)
    {
        let err_json = serde_json::json!({
            "error": "415 Unsupported Media Type: Wymagany nagłówek Content-Type: application/json"
        });
        let mut resp = tiny_http::Response::from_string(err_json.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp.with_status_code(415));
        return;
    }

    // Route: GET / or GET /index.html
    if method == tiny_http::Method::Get && (url == "/" || url == "/index.html") {
        let html_header =
            Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
        let mut resp = tiny_http::Response::from_string(STUDIO_HTML);
        resp.add_header(html_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /logo.png
    if method == tiny_http::Method::Get && url == "/logo.png" {
        let png_header = Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap();
        let mut resp = tiny_http::Response::from_data(LOGO_PNG);
        resp.add_header(png_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /audio/... (sanitized against path traversal)
    if method == tiny_http::Method::Get && url.starts_with("/audio/") {
        let subpath = url.trim_start_matches("/audio/");
        if !subpath.contains("..") && !subpath.contains('\\') {
            let disk_path = PathBuf::from("docs/audio").join(subpath);
            if disk_path.exists() && disk_path.is_file() {
                if let Ok(bytes) = std::fs::read(&disk_path) {
                    let mp3_header =
                        Header::from_bytes(&b"Content-Type"[..], &b"audio/mpeg"[..]).unwrap();
                    let mut resp = tiny_http::Response::from_data(bytes);
                    resp.add_header(mp3_header);
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }
            }
        }
        let mut resp = tiny_http::Response::from_string("Nie znaleziono pliku audio (404)");
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp.with_status_code(404));
        return;
    }

    // Route: GET /api/providers
    if method == tiny_http::Method::Get && url == "/api/providers" {
        let creds = credentials::load_credentials();
        let providers_list = serde_json::json!({
            "providers": [
                {
                    "id": "elevenlabs",
                    "name": "ElevenLabs v3",
                    "has_key": credentials::get_api_key("elevenlabs").is_some(),
                    "masked_key": credentials::get_masked_key("elevenlabs"),
                    "model_id": creds.providers.get("elevenlabs").and_then(|p| p.model_id.clone()).unwrap_or_else(|| "eleven_multilingual_v2".to_string()),
                    "voice_id": creds.providers.get("elevenlabs").and_then(|p| p.voice_id.clone()).unwrap_or_else(|| "adam".to_string())
                },
                {
                    "id": "openai",
                    "name": "OpenAI TTS",
                    "has_key": credentials::get_api_key("openai").is_some(),
                    "masked_key": credentials::get_masked_key("openai"),
                    "model_id": creds.providers.get("openai").and_then(|p| p.model_id.clone()).unwrap_or_else(|| "tts-1".to_string()),
                    "voice_id": creds.providers.get("openai").and_then(|p| p.voice_id.clone()).unwrap_or_else(|| "alloy".to_string())
                },
                {
                    "id": "azure",
                    "name": "Azure Cognitive Services",
                    "has_key": credentials::get_api_key("azure").is_some(),
                    "masked_key": credentials::get_masked_key("azure"),
                    "model_id": serde_json::Value::Null,
                    "voice_id": creds.providers.get("azure").and_then(|p| p.voice_id.clone()).unwrap_or_else(|| "pl-PL-MarekNeural".to_string())
                }
            ]
        });

        let json_header = Header::from_bytes(
            &b"Content-Type"[..],
            &b"application/json; charset=utf-8"[..],
        )
        .unwrap();
        let mut resp = tiny_http::Response::from_string(providers_list.to_string());
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/providers/set
    if method == tiny_http::Method::Post && url == "/api/providers/set" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(64 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct SetReq {
            provider: String,
            api_key: String,
            model_id: Option<String>,
            voice_id: Option<String>,
        }

        match serde_json::from_str::<SetReq>(&body_str) {
            Ok(data) => {
                if let Err(e) = credentials::set_api_key(
                    &data.provider,
                    &data.api_key,
                    data.model_id.as_deref(),
                    data.voice_id.as_deref(),
                ) {
                    let err = serde_json::json!({ "error": e.to_string() });
                    let mut resp = tiny_http::Response::from_string(err.to_string());
                    resp.add_header(
                        Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json; charset=utf-8"[..],
                        )
                        .unwrap(),
                    );
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(400));
                    return;
                }

                let resp_json = serde_json::json!({
                    "status": "ok",
                    "provider": data.provider,
                    "masked_key": credentials::mask_api_key(&data.api_key)
                });
                let mut resp = tiny_http::Response::from_string(resp_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err =
                    serde_json::json!({ "error": "Nieprawidłowy JSON", "details": e.to_string() });
                let mut resp = tiny_http::Response::from_string(err.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: POST /api/providers/check
    if method == tiny_http::Method::Post && url == "/api/providers/check" {
        let mut body_str = String::new();
        let _ = request
            .as_reader()
            .take(64 * 1024)
            .read_to_string(&mut body_str);

        #[derive(serde::Deserialize)]
        struct CheckReq {
            provider: Option<String>,
            api_key: Option<String>,
        }

        let (prov, direct_key) = match serde_json::from_str::<CheckReq>(&body_str) {
            Ok(r) => (
                r.provider.unwrap_or_else(|| "elevenlabs".to_string()),
                r.api_key,
            ),
            Err(_) => ("elevenlabs".to_string(), None),
        };

        let key = direct_key
            .or_else(|| credentials::get_api_key(&prov))
            .unwrap_or_default();

        if key.trim().is_empty() {
            let err = serde_json::json!({ "error": "Brak klucza API do weryfikacji. Wprowadź klucz przed sprawdzeniem." });
            let mut resp = tiny_http::Response::from_string(err.to_string());
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        match credentials::verify_elevenlabs_key(&key) {
            Ok(info) => {
                let resp_json = serde_json::json!({
                    "status": "ok",
                    "provider": "elevenlabs",
                    "tier": info.tier,
                    "character_count": info.character_count,
                    "character_limit": info.character_limit,
                    "can_extend_character_limit": info.can_extend_character_limit,
                });
                let mut resp = tiny_http::Response::from_string(resp_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err = serde_json::json!({ "error": e.to_string() });
                let mut resp = tiny_http::Response::from_string(err.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: POST /api/estimate
    if method == tiny_http::Method::Post && url == "/api/estimate" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct EstReq {
            text: String,
            #[serde(default = "default_est_provider")]
            provider: String,
            #[serde(default = "default_est_voice")]
            voice: String,
        }

        fn default_est_provider() -> String {
            "elevenlabs".to_string()
        }
        fn default_est_voice() -> String {
            "adam".to_string()
        }

        match serde_json::from_str::<EstReq>(&body_str) {
            Ok(data) => {
                let cache_dir = get_default_cache_dir();
                let est = estimate_cost(&data.text, &data.provider, &data.voice, &cache_dir);
                let json_data = serde_json::to_string(&est).unwrap_or_default();
                let mut resp = tiny_http::Response::from_string(json_data);
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err =
                    serde_json::json!({ "error": "Nieprawidłowy JSON", "details": e.to_string() });
                let mut resp = tiny_http::Response::from_string(err.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: GET /api/models
    if method == tiny_http::Method::Get && url == "/api/models" {
        let models = list_models();
        let json_resp = serde_json::to_string(&models).unwrap_or_else(|_| "[]".to_string());
        let json_header = Header::from_bytes(
            &b"Content-Type"[..],
            &b"application/json; charset=utf-8"[..],
        )
        .unwrap();
        let mut resp = tiny_http::Response::from_string(json_resp);
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/models/download
    if method == tiny_http::Method::Post && url == "/api/models/download" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct DownloadReq {
            model: String,
            #[serde(default)]
            force: bool,
        }

        let d_req = match serde_json::from_str::<DownloadReq>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match download_model(&d_req.model, d_req.force) {
            Ok(path) => {
                let res_json = serde_json::json!({
                    "status": "ok",
                    "model": d_req.model,
                    "path": path.to_string_lossy(),
                    "message": "Model został pomyślnie pobrany i jest gotowy do syntezy!"
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(res_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Błąd pobierania modelu",
                    "details": e.to_string()
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
            }
        }
        return;
    }

    // Route: POST /api/models/remove
    if method == tiny_http::Method::Post && url == "/api/models/remove" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct RemoveReq {
            model: String,
        }

        if let Ok(r) = serde_json::from_str::<RemoveReq>(&body_str) {
            let _ = remove_model(&r.model);
            let res_json = serde_json::json!({ "status": "ok", "model": r.model });
            let json_header = Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap();
            let mut resp = tiny_http::Response::from_string(res_json.to_string());
            resp.add_header(json_header);
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp);
        }
        return;
    }

    // Route: POST /api/models/update
    if method == tiny_http::Method::Post && url == "/api/models/update" {
        match update_voices_catalog() {
            Ok(count) => {
                let res_json = serde_json::json!({
                    "status": "ok",
                    "count": count,
                    "message": format!("Baza modeli zaktualizowana! Dostępne głosy: {}", count)
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(res_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Błąd aktualizacji bazy modeli",
                    "details": e.to_string()
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
            }
        }
        return;
    }

    // Route: POST /api/import
    if method == tiny_http::Method::Post && url == "/api/import" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(50 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Błąd odczytu danych książki\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct ImportReq {
            filename: String,
            #[serde(default)]
            content_base64: Option<String>,
            #[serde(default)]
            text: Option<String>,
        }

        let i_req = match serde_json::from_str::<ImportReq>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let ext = PathBuf::from(&i_req.filename)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let title_hint = PathBuf::from(&i_req.filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        let book_res = if let Some(ref b64) = i_req.content_base64 {
            match base64_decode(b64) {
                Ok(bytes) => match ext.as_str() {
                    "epub" => ImportedBook::from_epub_bytes(&bytes, title_hint.as_deref()),
                    "docx" => ImportedBook::from_docx_bytes(&bytes, title_hint.as_deref()),
                    _ => {
                        let text = String::from_utf8_lossy(&bytes);
                        Ok(ImportedBook::from_text_str(&text, title_hint.as_deref()))
                    }
                },
                Err(err) => Err(anyhow::anyhow!("Błąd dekodowania base64: {}", err)),
            }
        } else if let Some(ref txt) = i_req.text {
            Ok(ImportedBook::from_text_str(txt, title_hint.as_deref()))
        } else {
            Err(anyhow::anyhow!(
                "Brak treści książki (text lub content_base64)"
            ))
        };

        match book_res {
            Ok(book) => {
                let script = book.to_script();
                let res_json = serde_json::json!({
                    "title": book.title,
                    "author": book.author,
                    "language": book.language,
                    "total_words": book.total_words(),
                    "total_estimated_seconds": book.total_estimated_seconds(),
                    "chapters": book.chapters,
                    "script": script,
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(res_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
            }
            Err(err) => {
                let err_json = serde_json::json!({
                    "error": "Błąd importu książki",
                    "details": err.to_string()
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
            }
        }
        return;
    }

    // Route: POST /api/parse
    if method == tiny_http::Method::Post && url == "/api/parse" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let req = match serde_json::from_str::<ApiParseRequest>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let director = EmojiDirector::default().with_lang(&req.lang);
        let script = director.parse(&req.text);
        let json_resp = serde_json::json!({
            "items_count": script.items.len(),
            "elevenlabs": script.to_elevenlabs_script(),
            "chattts": script.to_chattts_script(),
        });
        let json_header =
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/cache/status
    if method == tiny_http::Method::Post && url == "/api/cache/status" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(10 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let req = match serde_json::from_str::<ApiRenderRequest>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let clean_lang = req.lang.to_lowercase();
        let synth_res = resolve_synthesizer(
            &req.provider,
            &req.voice,
            req.model.as_deref(),
            &clean_lang,
            default_model.as_ref(),
            &piper_cache,
        )
        .await;

        let director = EmojiDirector::default().with_lang(&clean_lang);
        let script = director.parse(&req.text);
        let cache_dir = get_default_cache_dir();

        let mut segments_info = Vec::new();
        let mut cached_count = 0;
        let mut total_speech = 0;

        for item in &script.items {
            if let DirectedItem::Speech(ref seg) = item {
                total_speech += 1;
                let voice_name = match synth_res {
                    Ok(ref s) => s.resolve_voice_label(seg.voice.as_deref()),
                    Err(_) => {
                        if req.provider == "edge" {
                            let clean = seg
                                .voice
                                .as_deref()
                                .unwrap_or(&req.voice)
                                .trim()
                                .to_lowercase();
                            if clean.contains("zofia")
                                || clean.contains("kobieta")
                                || clean.contains("female")
                            {
                                "pl-PL-ZofiaNeural".to_string()
                            } else if clean.contains("marek")
                                || clean.contains("mezczyzna")
                                || clean.contains("male")
                            {
                                "pl-PL-MarekNeural".to_string()
                            } else {
                                req.voice.clone()
                            }
                        } else {
                            let m_name = req.model.as_deref().unwrap_or(&req.voice);
                            let stem = PathBuf::from(m_name)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("pl_PL-darkman-medium")
                                .to_string();
                            let clean = seg.voice.as_deref().unwrap_or("").trim().to_lowercase();
                            if clean.contains("zofia")
                                || clean.contains("gosia")
                                || clean.contains("kobieta")
                                || clean.contains("female")
                            {
                                "piper-pl_PL-gosia-medium".to_string()
                            } else if clean.contains("guard")
                                || clean.contains("straznik")
                                || clean.contains("strażnik")
                                || clean.contains("mc")
                            {
                                "piper-pl_PL-mc_speech-medium".to_string()
                            } else if clean.contains("marek")
                                || clean.contains("male")
                                || clean.contains("mezczyzna")
                                || clean.contains("mężczyzna")
                            {
                                let mc_cand = audioticon_core::models_dir()
                                    .join("pl_PL-mc_speech-medium.onnx");
                                if mc_cand.exists() {
                                    "piper-pl_PL-mc_speech-medium".to_string()
                                } else {
                                    format!("piper-{}", stem)
                                }
                            } else {
                                format!("piper-{}", stem)
                            }
                        }
                    }
                };
                let hash = compute_segment_hash(seg, &voice_name);
                let cached_file = cache_dir.join(format!("{}.wav", hash));
                let in_cache = cached_file.exists();
                let file_size = if in_cache {
                    std::fs::metadata(&cached_file)
                        .map(|m| m.len())
                        .unwrap_or(0)
                } else {
                    0
                };
                if in_cache {
                    cached_count += 1;
                }
                segments_info.push(serde_json::json!({
                    "index": total_speech,
                    "text": seg.text,
                    "voice": voice_name,
                    "hash": hash,
                    "short_hash": &hash[..8.min(hash.len())],
                    "in_cache": in_cache,
                    "size_bytes": file_size,
                }));
            }
        }

        let ratio = if total_speech > 0 {
            cached_count as f64 / total_speech as f64
        } else {
            0.0
        };

        let json_resp = serde_json::json!({
            "in_cache": cached_count == total_speech && total_speech > 0,
            "total_speech_segments": total_speech,
            "cached_count": cached_count,
            "cache_hit_ratio": ratio,
            "segments": segments_info,
        });

        let json_header = Header::from_bytes(
            &b"Content-Type"[..],
            &b"application/json; charset=utf-8"[..],
        )
        .unwrap();
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/render
    if method == tiny_http::Method::Post && url == "/api/render" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let req = match serde_json::from_str::<ApiRenderRequest>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let clean_lang = req.lang.to_lowercase();
        let synth = match resolve_synthesizer(
            &req.provider,
            &req.voice,
            req.model.as_deref(),
            &clean_lang,
            default_model.as_ref(),
            &piper_cache,
        )
        .await
        {
            Ok(s) => s,
            Err(err_msg) => {
                let err_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..])
                        .unwrap();
                let mut resp = tiny_http::Response::from_string(err_msg);
                resp.add_header(err_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let cache_dir = get_default_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        let director = EmojiDirector::default().with_lang(&clean_lang);
        let script = director.parse(&req.text);
        let engine = AudioDirectorEngine::new(cache_dir.clone(), synth);

        let req_id = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
        let wav_target = cache_dir.join(format!("preview_{}_{}.wav", std::process::id(), req_id));
        let tmp_mp3 = cache_dir.join(format!("preview_{}_{}.mp3", std::process::id(), req_id));

        match engine.render(&script, &wav_target).await {
            Ok(render_res) => {
                let meta = AudiobookMetadata {
                    title: Some("Audioticon Studio Preview".to_string()),
                    author: None,
                    narrator: Some("Lektor".to_string()),
                    year: Some("2026".to_string()),
                    language: Some(clean_lang.clone()),
                };
                let export_res =
                    export_to_mp3(&wav_target, &tmp_mp3, &render_res.chapters, &meta).await;
                let _ = std::fs::remove_file(&wav_target);

                if let Err(e) = export_res {
                    let mut resp =
                        tiny_http::Response::from_string(format!("Błąd masteringu MP3: {}", e));
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(500));
                    return;
                }
                if let Ok(bytes) = std::fs::read(&tmp_mp3) {
                    let _ = std::fs::remove_file(&tmp_mp3);
                    let mp3_header =
                        Header::from_bytes(&b"Content-Type"[..], &b"audio/mpeg"[..]).unwrap();
                    let mut resp = tiny_http::Response::from_data(bytes);
                    resp.add_header(mp3_header);
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&wav_target);
                let mut resp =
                    tiny_http::Response::from_string(format!("Błąd renderowania: {}", e));
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: POST /api/export or GET /api/export
    if (method == tiny_http::Method::Post || method == tiny_http::Method::Get)
        && (url == "/api/export" || url.starts_with("/api/export?"))
    {
        let req: ApiExportRequest = if method == tiny_http::Method::Post {
            let mut body_str = String::new();
            if request
                .as_reader()
                .take(10 * 1024 * 1024)
                .read_to_string(&mut body_str)
                .is_err()
            {
                let mut resp = tiny_http::Response::from_string(
                    "{\"error\": \"Nie można odczytać zapytania\"}",
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }

            let parsed: ApiExportRequest = match serde_json::from_str::<ApiExportRequest>(&body_str)
            {
                Ok(r) => r,
                Err(e) => {
                    let err_json = serde_json::json!({
                        "error": "Nieprawidłowy format JSON",
                        "details": e.to_string()
                    });
                    let json_header =
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                    let mut resp = tiny_http::Response::from_string(err_json.to_string());
                    resp.add_header(json_header);
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(400));
                    return;
                }
            };

            if parsed.text.trim().is_empty() {
                let err_json = serde_json::json!({
                    "error": "Brak tekstu do wyrenderowania. Pole 'text' nie może być puste."
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
            parsed
        } else {
            // GET /api/export
            let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
            let mut params = std::collections::HashMap::new();
            for pair in query.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    params.insert(k, url_decode(v));
                }
            }
            let text = params.get("text").cloned().unwrap_or_default();
            if text.trim().is_empty() {
                let err_json = serde_json::json!({
                    "error": "Brak tekstu do wyrenderowania. Podaj parametr 'text' w zapytaniu GET lub wyślij zapytanie POST z ciałem JSON."
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }

            let fmt = params
                .get("format")
                .cloned()
                .unwrap_or_else(|| "m4b".to_string());
            ApiExportRequest {
                text,
                provider: params
                    .get("provider")
                    .cloned()
                    .unwrap_or_else(default_provider_str),
                lang: params.get("lang").cloned().unwrap_or_else(default_lang_str),
                voice: params
                    .get("voice")
                    .cloned()
                    .unwrap_or_else(default_voice_str),
                model: params.get("model").cloned(),
                format: fmt,
                title: params.get("title").cloned(),
                author: params.get("author").cloned(),
                narrator: Some("Lektor".to_string()),
                year: Some("2026".to_string()),
            }
        };

        let clean_lang = req.lang.to_lowercase();
        let synth = match resolve_synthesizer(
            &req.provider,
            &req.voice,
            req.model.as_deref(),
            &clean_lang,
            default_model.as_ref(),
            &piper_cache,
        )
        .await
        {
            Ok(s) => s,
            Err(err_msg) => {
                let err_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..])
                        .unwrap();
                let mut resp = tiny_http::Response::from_string(err_msg);
                resp.add_header(err_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let cache_dir = get_default_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        let director = EmojiDirector::default().with_lang(&clean_lang);
        let script = director.parse(&req.text);
        let engine = AudioDirectorEngine::new(cache_dir.clone(), synth);

        let req_id = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
        let wav_target = cache_dir.join(format!("export_{}_{}.wav", std::process::id(), req_id));

        let is_m4b = req.format.to_lowercase() == "m4b";
        let out_target = if is_m4b {
            cache_dir.join(format!("export_{}_{}.m4b", std::process::id(), req_id))
        } else {
            cache_dir.join(format!("export_{}_{}.mp3", std::process::id(), req_id))
        };

        let meta = AudiobookMetadata {
            title: req
                .title
                .clone()
                .or_else(|| Some("Audioticon Audiobook".to_string())),
            author: req.author.clone(),
            narrator: req.narrator.clone().or_else(|| Some("Lektor".to_string())),
            year: req.year.clone().or_else(|| Some("2026".to_string())),
            language: Some(clean_lang.clone()),
        };

        match engine.render(&script, &wav_target).await {
            Ok(render_res) => {
                let export_res = if is_m4b {
                    export_to_m4b(&wav_target, &out_target, &render_res.chapters, &meta).await
                } else {
                    export_to_mp3(&wav_target, &out_target, &render_res.chapters, &meta).await
                };
                let _ = std::fs::remove_file(&wav_target);

                if let Err(e) = export_res {
                    let mut resp = tiny_http::Response::from_string(format!(
                        "Błąd eksportu {}: {}",
                        req.format, e
                    ));
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(500));
                    return;
                }

                if let Ok(bytes) = std::fs::read(&out_target) {
                    let _ = std::fs::remove_file(&out_target);
                    let ct_bytes = if is_m4b {
                        b"audio/mp4"[..].to_vec()
                    } else {
                        b"audio/mpeg"[..].to_vec()
                    };
                    let ext = if is_m4b { "m4b" } else { "mp3" };
                    let safe_title = meta
                        .title
                        .as_deref()
                        .unwrap_or("audiobook")
                        .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");
                    let cd_str = format!("attachment; filename=\"{}.{}\"", safe_title, ext);

                    let ct_header =
                        Header::from_bytes(&b"Content-Type"[..], &ct_bytes[..]).unwrap();
                    let cd_header =
                        Header::from_bytes(&b"Content-Disposition"[..], cd_str.as_bytes()).unwrap();

                    let mut resp = tiny_http::Response::from_data(bytes);
                    resp.add_header(ct_header);
                    resp.add_header(cd_header);
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&wav_target);
                let mut resp =
                    tiny_http::Response::from_string(format!("Błąd renderowania: {}", e));
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // 404 fallback
    let mut resp = tiny_http::Response::from_string("Nie znaleziono (404)");
    if let Some(ref h) = cors_header {
        resp.add_header(h.clone());
    }
    let _ = request.respond(resp.with_status_code(404));
}

async fn run_studio_server(
    host: String,
    port: u16,
    default_provider: TtsProvider,
    default_lang: String,
    model: Option<PathBuf>,
    no_browser: bool,
) -> Result<()> {
    let addr = format!("{}:{}", host, port);
    let server = Arc::new(
        tiny_http::Server::http(&addr)
            .map_err(|e| anyhow::anyhow!("Nie można uruchomić serwera na {}: {}", addr, e))?,
    );

    println!("============================================================");
    println!("🚀 AUDIOTICON STUDIO (WORKBENCH) URUCHOMIONE!");
    println!("============================================================");
    println!("🌐 Adres w przeglądarce: http://{}", addr);
    println!("⚙️  Domyślny silnik:     {:?}", default_provider);
    println!("🗣️  Domyślny język:      {}", default_lang);
    println!("💡 Wpisz tekst z emotikonami lub aliasami [pauza] i odsłuchaj na żywo.");
    println!("Naciśnij Ctrl+C, aby zatrzymać serwer.");
    println!("============================================================");

    if !no_browser {
        let open_url = format!("http://{}", addr);
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&open_url).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", &open_url])
                .spawn();
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = std::process::Command::new("xdg-open")
                .arg(&open_url)
                .spawn();
        });
    }

    let piper_cache: PiperCache = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let server_clone = server.clone();
        let request = match tokio::task::spawn_blocking(move || server_clone.recv()).await? {
            Ok(req) => req,
            Err(_) => break,
        };

        let piper_cache = piper_cache.clone();
        let default_model = model.clone();
        tokio::spawn(async move {
            handle_studio_request(request, piper_cache, default_model).await;
        });
    }

    Ok(())
}
