use anyhow::{Context, Result};
use audioticon_core::{
    export_to_m4b, export_to_mp3, presets, AudioDirectorEngine, AudiobookMetadata, DirectedItem,
    EdgeTtsBridge, EmojiDirector, PiperBridge, PresetRegistry, ProjectConfig, SpeechSynthesizer,
};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::Arc;
use tiny_http::Header;

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
            let script_text = match input {
                Some(ref file_path) if !demo && !demo_effects => std::fs::read_to_string(file_path)
                    .with_context(|| format!("Nie można odczytać pliku: {:?}", file_path))?,
                _ => {
                    if demo_effects {
                        DEMO_EFFECTS_SCENE.to_string()
                    } else {
                        if !demo {
                            println!("ℹ️ Nie podano pliku wejściowego. Uruchamiam scenę demonstracyjną (--demo).\n");
                        }
                        DEMO_SCENE.to_string()
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
                    let m_path = model.unwrap_or_else(|| get_default_piper_model(&clean_lang));
                    if !m_path.exists() {
                        anyhow::bail!(
                            "Nie znaleziono modelu Piper: {:?}. Pobierz model do ~/.local/share/audioticon/models/",
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
                .or_else(|| project_meta.and_then(|p| p.title.clone()))
                .unwrap_or_else(|| {
                    input
                        .as_ref()
                        .and_then(|p| p.file_stem().and_then(|s| s.to_str()))
                        .unwrap_or("Audioticon Audiobook")
                        .to_string()
                });
            let book_author = author
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
            let _ = std::process::Command::new("xdg-open").arg(&open_url).spawn();
        });
    }

    loop {
        let server_clone = server.clone();
        let mut request = match tokio::task::spawn_blocking(move || server_clone.recv()).await? {
            Ok(req) => req,
            Err(_) => break,
        };

        let method = request.method().clone();
        let url = request.url().to_string();

        let cors_origin = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
        let cors_methods = Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]).unwrap();
        let cors_headers = Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type"[..]).unwrap();

        // Handle OPTIONS
        if method == tiny_http::Method::Options {
            let mut resp = tiny_http::Response::from_string("");
            resp.add_header(cors_origin);
            resp.add_header(cors_methods);
            resp.add_header(cors_headers);
            let _ = request.respond(resp);
            continue;
        }

        // Route: GET / or GET /index.html
        if method == tiny_http::Method::Get && (url == "/" || url == "/index.html") {
            let html_header = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
            let mut resp = tiny_http::Response::from_string(STUDIO_HTML);
            resp.add_header(html_header);
            resp.add_header(cors_origin);
            let _ = request.respond(resp);
            continue;
        }

        // Route: GET /logo.png
        if method == tiny_http::Method::Get && url == "/logo.png" {
            let png_header = Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap();
            let mut resp = tiny_http::Response::from_data(LOGO_PNG);
            resp.add_header(png_header);
            resp.add_header(cors_origin);
            let _ = request.respond(resp);
            continue;
        }

        // Route: GET /audio/...
        if method == tiny_http::Method::Get && url.starts_with("/audio/") {
            let rel_path = url.trim_start_matches('/');
            let disk_path = PathBuf::from("docs").join(rel_path);
            if disk_path.exists() {
                if let Ok(bytes) = std::fs::read(&disk_path) {
                    let mp3_header = Header::from_bytes(&b"Content-Type"[..], &b"audio/mpeg"[..]).unwrap();
                    let mut resp = tiny_http::Response::from_data(bytes);
                    resp.add_header(mp3_header);
                    resp.add_header(cors_origin);
                    let _ = request.respond(resp);
                    continue;
                }
            }
        }

        // Route: POST /api/parse
        if method == tiny_http::Method::Post && url == "/api/parse" {
            let mut body_str = String::new();
            let _ = request.as_reader().read_to_string(&mut body_str);
            if let Ok(req) = serde_json::from_str::<ApiParseRequest>(&body_str) {
                let director = EmojiDirector::default().with_lang(&req.lang);
                let script = director.parse(&req.text);
                let json_resp = serde_json::json!({
                    "items_count": script.items.len(),
                    "elevenlabs": script.to_elevenlabs_script(),
                    "chattts": script.to_chattts_script(),
                });
                let json_header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(json_header);
                resp.add_header(cors_origin);
                let _ = request.respond(resp);
                continue;
            }
        }

        // Route: POST /api/render
        if method == tiny_http::Method::Post && url == "/api/render" {
            let mut body_str = String::new();
            let _ = request.as_reader().read_to_string(&mut body_str);
            if let Ok(req) = serde_json::from_str::<ApiRenderRequest>(&body_str) {
                let clean_lang = req.lang.to_lowercase();
                let synth: Arc<dyn SpeechSynthesizer> = if req.provider == "edge" {
                    Arc::new(EdgeTtsBridge::new(&req.voice))
                } else {
                    let m_path = model.clone().unwrap_or_else(|| get_default_piper_model(&clean_lang));
                    if !m_path.exists() {
                        let err_header = Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..]).unwrap();
                        let mut resp = tiny_http::Response::from_string(format!("Nie znaleziono modelu Piper: {:?}", m_path));
                        resp.add_header(err_header);
                        resp.add_header(cors_origin);
                        let _ = request.respond(resp.with_status_code(400));
                        continue;
                    }
                    Arc::new(PiperBridge::new(m_path))
                };

                let cache_dir = get_default_cache_dir();
                let _ = std::fs::create_dir_all(&cache_dir);
                let director = EmojiDirector::default().with_lang(&clean_lang);
                let script = director.parse(&req.text);
                let engine = AudioDirectorEngine::new(cache_dir.clone(), synth);

                let wav_target = cache_dir.join(format!("preview_{}.wav", std::process::id()));
                match engine.render(&script, &wav_target).await {
                    Ok(render_res) => {
                        let tmp_mp3 = cache_dir.join(format!("preview_{}.mp3", std::process::id()));
                        let meta = AudiobookMetadata {
                            title: Some("Audioticon Studio Preview".to_string()),
                            author: Some("Jarosław Kuchta".to_string()),
                            narrator: Some("Lektor".to_string()),
                            year: Some("2026".to_string()),
                            language: Some(clean_lang.clone()),
                        };
                        let export_res = export_to_mp3(&wav_target, &tmp_mp3, &render_res.chapters, &meta).await;
                        let _ = std::fs::remove_file(&wav_target);

                        if let Err(e) = export_res {
                            let mut resp = tiny_http::Response::from_string(format!("Błąd masteringu MP3: {}", e));
                            resp.add_header(cors_origin);
                            let _ = request.respond(resp.with_status_code(500));
                            continue;
                        }
                        if let Ok(bytes) = std::fs::read(&tmp_mp3) {
                            let _ = std::fs::remove_file(&tmp_mp3);
                            let mp3_header = Header::from_bytes(&b"Content-Type"[..], &b"audio/mpeg"[..]).unwrap();
                            let mut resp = tiny_http::Response::from_data(bytes);
                            resp.add_header(mp3_header);
                            resp.add_header(cors_origin);
                            let _ = request.respond(resp);
                            continue;
                        }
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&wav_target);
                        let mut resp = tiny_http::Response::from_string(format!("Błąd renderowania: {}", e));
                        resp.add_header(cors_origin);
                        let _ = request.respond(resp.with_status_code(500));
                        continue;
                    }
                }
            }
        }

        // 404 fallback
        let mut resp = tiny_http::Response::from_string("Nie znaleziono (404)");
        resp.add_header(cors_origin);
        let _ = request.respond(resp.with_status_code(404));
    }

    Ok(())
}
