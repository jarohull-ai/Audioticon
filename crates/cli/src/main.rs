use std::path::{Path, PathBuf};
use anyhow::Result;
use audioticon_core::{AudioDirectorEngine, EdgeTtsBridge, EmojiDirector};

#[tokio::main]
async fn main() -> Result<()> {
    println!("============================================================");
    println!("🎧 AUDIOTICON — Renderowanie Sceny Kijów (Styl: Achaja)");
    println!("============================================================");

    let script_text = r#"
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

    println!("\n[1] Zareżyserowany scenariusz (Styl: Achaja):\n{}", script_text.trim());

    let director = EmojiDirector::default();
    let script = director.parse(script_text);
    println!("\n[2] Oś czasu: {} segmentów (mowa + pauzy). Łączny czas pauz: {} ms", script.items.len(), script.total_pause_ms());

    let cache_dir = PathBuf::from("/tmp/audioticon_cache");
    let bridge = Box::new(EdgeTtsBridge::new("pl-PL-MarekNeural"));
    let engine = AudioDirectorEngine::new(cache_dir, bridge);

    let master_wav = Path::new("/home/jaro/Desktop/kijow_audioticon_styl_achaja.wav");

    println!("\n[3] Przetwarzanie i buforowanie...");
    let start_time = std::time::Instant::now();
    engine.render(&script, master_wav).await?;
    let elapsed = start_time.elapsed();

    // Dodatkowo eksport do MP3
    let master_mp3 = Path::new("/home/jaro/Desktop/kijow_audioticon_styl_achaja.mp3");
    let _ = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(master_wav)
        .arg("-codec:a")
        .arg("libmp3lame")
        .arg("-qscale:a")
        .arg("2")
        .arg(master_mp3)
        .output()
        .await;

    println!("\n============================================================");
    println!("✅ Wygenerowano plik master: {:?}", master_wav);
    println!("🎵 Wygenerowano wersję MP3:  {:?}", master_mp3);
    println!("⏱️ Czas operacji: {:.2?}", elapsed);
    println!("============================================================");

    Ok(())
}
