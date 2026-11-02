use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::sync::Arc;
use audioticon_core::{
    export_to_m4b, export_to_mp3, AudiobookMetadata, AudioDirectorEngine, DirectedItem,
    EdgeTtsBridge, EmojiDirector, PiperBridge, SpeechSynthesizer,
};

#[derive(Parser)]
#[command(name = "audioticon")]
#[command(author = "Jarosław Kuchta <jarohull@gmail.com>")]
#[command(version = "0.2.0")]
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

        /// Ścieżka do lokalnego modelu ONNX dla Piper (domyślnie: ~/.cache/audioticon/models/pl_PL-darkman-medium.onnx)
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

        /// Uruchamia wbudowaną scenę demonstracyjną Kijów (Styl: Achaja)
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

        /// Uruchamia wbudowaną scenę demonstracyjną Kijów
        #[arg(long)]
        demo: bool,
    },
}

fn get_default_cache_dir() -> PathBuf {
    if let Some(user_cache) = dirs::cache_dir() {
        user_cache.join("audioticon")
    } else {
        PathBuf::from("/tmp/audioticon_cache")
    }
}

fn get_default_piper_model() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        data_dir.join("audioticon").join("models").join("pl_PL-darkman-medium.onnx")
    } else {
        PathBuf::from("/home/jaro/.local/share/audioticon/models/pl_PL-darkman-medium.onnx")
    }
}

const DEMO_SCENE: &str = r#"
📖 Prolog: Kijów 🛑

Kijów, 24.02.2024. 🛑

Adam Richter siedział przy biurku w mieszkaniu na Obołoni w Kijowie. ⏸️
Za oknem było szaro, jak przez większość dni tej zimy. 🛑
Na parapecie stał wyschnięty aloes, 😮‍💨 którego nie podlewał od tygodni. 🛑

Na biurku leżał 💥notes. 🛑

🐢 Zniszczony od wilgoci... 😮‍💨 od dotyku... od noszenia w plecaku, który mokł w deszczu i leżał w błocie. 🛑
Okładka wyblakła, rogi starte, grzbiet pęknięty w trzech miejscach. 🛑

W środku... tysiące zapisanych zdarzeń. 🛑
📉Nazwisk. ⏸️
📉Godzin. ⏸️
📉Współrzędnych. 🛑

Niektóre strony były tak wypisane, że atrament przebijał na drugą stronę. ⏸️
Inne czyste... 😮‍💨 bo zabrakło mu siły, żeby dokończyć zdanie. 🛑

📖 Rozdział 1: Archiwum Pyłu 🛑

Nazwał to: 💥Archiwum Pyłu. 🛑

Otworzył notes na stronie z granicy. 🛑
24.02.2022. ⏸️
Godzina 15:47. ⏸️
Dorohusk. 🛑

Przez chwilę patrzył na te cyfry. 🛑
Potem zamknął notes. 🛑
Nie musiał czytać dalej. 🛑
🤫 Pamiętał zapach. 🛑
"#;

const DEMO_EFFECTS_SCENE: &str = r#"
Oto demonstracja zaawansowanych efektów akustycznych Audioticon. 🛑

Narrator mówi czystym, studyjnym głosem. ⏸️
Ale nagle odbiera telefon. 🛑
📱 "Halo? Richter, słyszysz mnie? Mamy problem na punkcie kontrolnym!" 🛑

W tle odzywa się wojskowa radiostacja krótkofalowa. 🛑
📻 "Baza jeden do patrolu. Zgłaszam naruszenie perymetru w sektorze czwartym. Odbiór." 🛑

Dowódca bierze tubę i krzyczy przez megafon na placu: 🛑
📢 "Wszyscy natychmiast opuścić ten rejon! To nie są ćwiczenia!" 🛑

Adam cofa się w głąb starej, opuszczonej katedry. 🛑
🏛️ Kroki niosły się głuchym echem po kamiennych łukach świątyni. 🛑

Schodzi wąskimi schodami do wilgotnych lochów. 🛑
🏰 Ciemność była lepka, a woda kapała gdzieś w głębi podziemi. 🛑

Zatrzymał się. 🛑
💭 "Czy oni naprawdę myślą, że nie byłem na to przygotowany? Wszystko przewidziałem." 🛑

Nagle z mroku wyłonił się syntetyczny głos bojowego androida: 🛑
🤖 "Cel zidentyfikowany. Rozpoczynam procedurę neutralizacji." 🛑

I znowu zapadła śmiertelna cisza. 🛑
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
                    std::fs::read_dir(&cache_path)?.filter_map(|e| e.ok()).count()
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
            model,
            cache_dir,
            title,
            author,
            narrator,
            demo,
            demo_effects,
        } => {
            let script_text = match input {
                Some(file_path) if !demo && !demo_effects => {
                    std::fs::read_to_string(&file_path)
                        .with_context(|| format!("Nie można odczytać pliku: {:?}", file_path))?
                }
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

            let cache_path = cache_dir.unwrap_or_else(get_default_cache_dir);
            std::fs::create_dir_all(&cache_path)?;

            let synthesizer: Arc<dyn SpeechSynthesizer> = match provider {
                TtsProvider::Edge => Arc::new(EdgeTtsBridge::new(&voice)),
                TtsProvider::Piper => {
                    let m_path = model.unwrap_or_else(get_default_piper_model);
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

            let book_title = title.unwrap_or_else(|| "Archiwum Pyłu".to_string());
            let book_author = author.unwrap_or_else(|| "Adam Richter".to_string());
            let book_narrator = narrator.unwrap_or_else(|| synthesizer.voice_name().to_string());

            println!("============================================================");
            println!("🎧 AUDIOTICON — Renderowanie Scenariusza");
            println!("Tytuł:     {}", book_title);
            println!("Autor:     {}", book_author);
            println!("Lektor:    {}", book_narrator);
            println!("Provider:  {:?}", provider);
            println!("Cache dir: {:?}", cache_path);
            println!("Wyjście:   {:?}", output);
            println!("============================================================");

            let director = EmojiDirector::default();
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
            let render_result = engine.render(&script, &wav_target).await?;
            let elapsed = start_time.elapsed();

            let metadata = AudiobookMetadata {
                title: Some(book_title),
                author: Some(book_author),
                narrator: Some(book_narrator),
                year: Some("2024".to_string()),
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
                println!("\n📑 Wygenerowane rozdziały ({}):", render_result.chapters.len());
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
        Commands::Script { input, format, demo } => {
            let script_text = match input {
                Some(file_path) if !demo => std::fs::read_to_string(&file_path)
                    .with_context(|| format!("Nie można odczytać pliku: {:?}", file_path))?,
                _ => DEMO_SCENE.to_string(),
            };
            let director = EmojiDirector::default();
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
                            DirectedItem::Silence(ms) => println!("  [{:02}] ⏸️ PAUZA: {} ms", idx, ms),
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
    }

    Ok(())
}
