use anyhow::{Context, Result};
use audioticon_core::{
    base64_decode, download_model, export_to_m4b, export_to_mp3, find_model, list_models,
    models_dir, presets, remove_model, AudioDirectorEngine, AudiobookMetadata, DirectedItem,
    EdgeTtsBridge, EmojiDirector, ImportedBook, PiperBridge, PresetRegistry, ProjectConfig,
    SpeechSynthesizer,
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
}

#[derive(Subcommand, Debug, Clone)]
enum ModelsAction {
    /// Wyświetla listę dostępnych w katalogu i zainstalowanych modeli offline
    List,
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
}

fn get_default_cache_dir() -> PathBuf {
    if let Some(user_cache) = dirs::cache_dir() {
        user_cache.join("audioticon")
    } else {
        PathBuf::from("/tmp/audioticon_cache")
    }
}

fn get_default_piper_model(lang: &str) -> PathBuf {
    let filename = if lang.starts_with("en") {
        "en_US-lessac-medium.onnx"
    } else {
        "pl_PL-darkman-medium.onnx"
    };
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("audioticon").join("models").join(filename)
    } else {
        PathBuf::from(format!(
            "/home/jaro/.local/share/audioticon/models/{}",
            filename
        ))
    }
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

            let synthesizer: Arc<dyn SpeechSynthesizer> = match provider {
                TtsProvider::Edge => Arc::new(EdgeTtsBridge::new(&voice)),
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
                .unwrap_or_else(|| "Jarosław Kuchta".to_string());
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
            let act = action.unwrap_or(ModelsAction::List);
            match act {
                ModelsAction::List => {
                    let dir = audioticon_core::models_dir();
                    println!("=========================================================================================");
                    println!("🤖 AUDIOTICON — Menedżer Modeli Neuronowych Piper TTS (Offline)");
                    println!("Folder modeli: {}", dir.display());
                    println!("=========================================================================================");
                    println!(
                        "{:<4} {:<24} {:<10} {:<12} {:<10} Status",
                        "Lp.", "Identyfikator", "Język", "Głos", "Rozmiar"
                    );
                    println!("-----------------------------------------------------------------------------------------");
                    let models = audioticon_core::list_models();
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
                            "{:02}.  {:<24} {:<10} {:<12} ~{:>2} MB    {}",
                            idx + 1,
                            m.id,
                            m.lang_name,
                            gender_str,
                            m.size_mb,
                            status_str
                        );
                    }
                    println!("=========================================================================================");
                    println!("💡 Wskazówka: pobierz model poleceniem:");
                    println!("   audioticon models download <nazwa>  (np. audioticon models download gosia)");
                    println!("=========================================================================================");
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
    }

    Ok(())
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

type PiperCache = Arc<Mutex<HashMap<PathBuf, Arc<PiperBridge>>>>;

fn get_cors_header(request: &tiny_http::Request) -> Option<Header> {
    for header in request.headers() {
        if header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("origin")
        {
            let origin = header.value.as_str();
            if origin == "https://jarohull-ai.github.io"
                || origin.starts_with("http://localhost")
                || origin.starts_with("http://127.0.0.1")
                || origin.starts_with("https://localhost")
                || origin.starts_with("https://127.0.0.1")
            {
                return Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin.as_bytes())
                    .ok();
            }
        }
    }
    None
}

async fn handle_studio_request(
    mut request: tiny_http::Request,
    piper_cache: PiperCache,
    default_model: Option<PathBuf>,
) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let cors_header = get_cors_header(&request);

    // Handle OPTIONS
    if method == tiny_http::Method::Options {
        let mut resp = tiny_http::Response::from_string("");
        if let Some(ref h) = cors_header {
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
        }
        let _ = request.respond(resp);
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
        let synth: Arc<dyn SpeechSynthesizer> = if req.provider == "edge" {
            Arc::new(EdgeTtsBridge::new(&req.voice))
        } else {
            let m_path = if let Some(ref custom_m) = req.model {
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
            } else if let Some(item) = find_model(&req.voice) {
                item.onnx_path
                    .unwrap_or_else(|| models_dir().join(format!("{}.onnx", item.id)))
            } else {
                default_model
                    .clone()
                    .unwrap_or_else(|| get_default_piper_model(&clean_lang))
            };

            if !m_path.exists() {
                let err_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..])
                        .unwrap();
                let mut resp = tiny_http::Response::from_string(format!(
                    "Nie znaleziono modelu Piper: {:?}. Pobierz model w Studio lub poleceniem 'audioticon models download <nazwa>'",
                    m_path
                ));
                resp.add_header(err_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
            let mut cache = piper_cache.lock().await;
            if let Some(existing) = cache.get(&m_path) {
                existing.clone()
            } else {
                let bridge = Arc::new(PiperBridge::new(m_path.clone()));
                cache.insert(m_path, bridge.clone());
                bridge
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
