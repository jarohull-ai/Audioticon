use anyhow::Result;
use audioticon_core::ImportedBook;
use std::path::PathBuf;

pub fn run_import(
    input: PathBuf,
    output: Option<PathBuf>,
    title: Option<String>,
    author: Option<String>,
    preview: bool,
    split_dir: Option<PathBuf>,
) -> Result<()> {
    println!(
        "========================================================================================="
    );
    println!("📚 AUDIOTICON — Importer Książek (.epub, .docx, .txt, .md)");
    println!("Plik źródłowy: {:?}", input);
    println!(
        "========================================================================================="
    );

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
    println!(
        "-----------------------------------------------------------------------------------------"
    );
    println!(
        "{:<4} {:<40} {:<12} Szacowany czas",
        "Nr", "Tytuł rozdziału", "Liczba słów"
    );
    println!(
        "-----------------------------------------------------------------------------------------"
    );

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
    println!(
        "========================================================================================="
    );

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
    Ok(())
}
