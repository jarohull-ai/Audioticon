use anyhow::{Context, Result};
use audioticon_core::{DirectedItem, EmojiDirector};
use std::path::PathBuf;

use crate::args::{ScriptFormat, DEMO_SCENE};

pub fn run_script(
    input: Option<PathBuf>,
    format: ScriptFormat,
    lang: String,
    demo: bool,
) -> Result<()> {
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
    Ok(())
}
