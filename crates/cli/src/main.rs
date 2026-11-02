use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use audioticon_core::{AudioDirectorEngine, EdgeTtsBridge, EmojiDirector};

#[derive(Parser)]
#[command(name = "audioticon")]
#[command(author = "Jarosław Kuchta <jarohull@gmail.com>")]
#[command(version = "0.1.0")]
#[command(about = "🎧 Audioticon — Reżyser audiobooków sterowany emotikonami z cachem zdań", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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

        /// Identyfikator głosu TTS (np. pl-PL-MarekNeural, pl-PL-ZofiaNeural)
        #[arg(short, long, default_value = "pl-PL-MarekNeural")]
        voice: String,

        /// Własny katalog cache (domyślnie ~/.cache/audioticon)
        #[arg(short, long)]
        cache_dir: Option<PathBuf>,

        /// Uruchamia wbudowaną scenę demonstracyjną Kijów (Styl: Achaja)
        #[arg(long)]
        demo: bool,
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
        Commands::Render { input, output, voice, cache_dir, demo } => {
            let script_text = match input {
                Some(file_path) if !demo => {
                    std::fs::read_to_string(&file_path)
                        .with_context(|| format!("Nie można odczytać pliku: {:?}", file_path))?
                }
                _ => {
                    if !demo {
                        println!("ℹ️ Nie podano pliku wejściowego. Uruchamiam scenę demonstracyjną (--demo).\n");
                    }
                    DEMO_SCENE.to_string()
                }
            };

            let cache_path = cache_dir.unwrap_or_else(get_default_cache_dir);
            std::fs::create_dir_all(&cache_path)?;

            println!("============================================================");
            println!("🎧 AUDIOTICON — Renderowanie Scenariusza");
            println!("Głos:      {}", voice);
            println!("Cache dir: {:?}", cache_path);
            println!("Wyjście:   {:?}", output);
            println!("============================================================");

            let director = EmojiDirector::default();
            let script = director.parse(&script_text);
            println!("Oś czasu:  {} segmentów. Łączny czas pauz: {} ms", script.items.len(), script.total_pause_ms());

            let bridge = Box::new(EdgeTtsBridge::new(&voice));
            let engine = AudioDirectorEngine::new(cache_path, bridge);

            // If output is .mp3, render to an intermediate .wav first
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
