use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use audioticon_core::{AudioDirectorEngine, EdgeTtsBridge, PiperBridge, SpeechSynthesizer, EmojiDirector};

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
}

fn get_default_cache_dir() -> PathBuf {
    if let Some(user_cache) = dirs::cache_dir() {
        user_cache.join("audioticon")
    } else {
        PathBuf::from("/tmp/audioticon_cache")
    }
}

fn get_default_piper_model() -> PathBuf {
    get_default_cache_dir().join("models").join("pl_PL-darkman-medium.onnx")
}

const DEMO_SCENE: &str = r#"
Kijów, dwudziesty czwarty lutego dwa tysiące dwudziestego czwartego roku. 🛑

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

Nazwał to: 💥Archiwum Pyłu. 🛑

Otworzył notes na stronie z granicy. 🛑
Dwudziesty czwarty lutego dwa tysiące dwudziestego drugiego roku. ⏸️
Godzina piętnasta czterdzieści siedem. ⏸️
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
        Commands::Render { input, output, provider, voice, model, cache_dir, demo, demo_effects } => {
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

            let synthesizer: Box<dyn SpeechSynthesizer> = match provider {
                TtsProvider::Edge => {
                    Box::new(EdgeTtsBridge::new(&voice))
                }
                TtsProvider::Piper => {
                    let m_path = model.unwrap_or_else(get_default_piper_model);
                    if !m_path.exists() {
                        anyhow::bail!(
                            "Nie znaleziono modelu Piper: {:?}. Pobierz model do ~/.cache/audioticon/models/",
                            m_path
                        );
                    }
                    Box::new(PiperBridge::new(m_path))
                }
            };

            println!("============================================================");
            println!("🎧 AUDIOTICON — Renderowanie Scenariusza");
            println!("Provider:  {:?}", provider);
            println!("Głos/ID:   {}", synthesizer.voice_name());
            println!("Cache dir: {:?}", cache_path);
            println!("Wyjście:   {:?}", output);
            println!("============================================================");

            let director = EmojiDirector::default();
            let script = director.parse(&script_text);
            println!("Oś czasu:  {} segmentów. Łączny czas pauz: {} ms", script.items.len(), script.total_pause_ms());

            let engine = AudioDirectorEngine::new(cache_path, synthesizer);

            let is_mp3 = output.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("mp3")).unwrap_or(false);
            let wav_target = if is_mp3 {
                output.with_extension("wav")
            } else {
                output.clone()
            };

            let start_time = std::time::Instant::now();
            engine.render(&script, &wav_target).await?;
            let elapsed = start_time.elapsed();

            if is_mp3 {
                println!("\nKonwertowanie master WAV do MP3...");
                let status = tokio::process::Command::new("ffmpeg")
                    .arg("-y")
                    .arg("-i")
                    .arg(&wav_target)
                    .arg("-codec:a")
                    .arg("libmp3lame")
                    .arg("-qscale:a")
                    .arg("2")
                    .arg(&output)
                    .status()
                    .await
                    .with_context(|| "Błąd uruchamiania ffmpeg do konwersji MP3")?;

                if !status.success() {
                    anyhow::bail!("Konwersja do MP3 zakończyła się błędem: {:?}", status);
                }
            }

            println!("\n============================================================");
            println!("✅ Gotowe! Plik wyjściowy: {:?}", output);
            println!("⏱️ Czas operacji: {:.2?}", elapsed);
            println!("============================================================");
        }
    }

    Ok(())
}
