use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn run_direct(
    input: PathBuf,
    output: Option<PathBuf>,
    provider: String,
    model: Option<String>,
    key: Option<String>,
    dry_run: bool,
) -> Result<()> {
    let raw_text = std::fs::read_to_string(&input)
        .with_context(|| format!("Nie można odczytać pliku: {:?}", input))?;
    println!(
        "🎭 Uruchamiam asystenta reżyserii AI (dostawca: '{}', model: '{}')...",
        provider,
        model.as_deref().unwrap_or("domyślny")
    );
    let directed =
        audioticon_core::auto_direct_text(&raw_text, &provider, model.as_deref(), key.as_deref())?;

    if dry_run {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("{}", directed);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    } else {
        let out_path = output.unwrap_or_else(|| input.with_extension("audioticon"));
        std::fs::write(&out_path, &directed)
            .with_context(|| format!("Nie można zapisać pliku: {:?}", out_path))?;
        println!("✅ Zapisano wyreżyserowany scenariusz: {:?}", out_path);
        println!(
            "💡 Odsłuchaj lub wyrenderuj audiobooka: audioticon render {:?}",
            out_path
        );
    }
    Ok(())
}
