use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedChapter {
    pub index: usize,
    pub title: String,
    pub content: String,
    pub word_count: usize,
    pub estimated_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedBook {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub chapters: Vec<ImportedChapter>,
}

impl ImportedBook {
    pub fn total_words(&self) -> usize {
        self.chapters.iter().map(|c| c.word_count).sum()
    }

    pub fn total_estimated_seconds(&self) -> u32 {
        self.chapters.iter().map(|c| c.estimated_seconds).sum()
    }

    /// Konwertuje zaimportowaną książkę do gotowego scenariusza Audioticon ze znacznikami rozdziałów 📖 i stopów 🛑
    pub fn to_script(&self) -> String {
        let mut script = String::new();

        for (idx, ch) in self.chapters.iter().enumerate() {
            if idx > 0 {
                script.push_str("\n\n");
            }
            // Zapisz nagłówek rozdziału
            script.push_str(&format!("📖 {} 🛑\n\n", ch.title.trim()));

            // Zapisz treść rozdziału
            script.push_str(ch.content.trim());
        }

        script
    }

    /// Importuje książkę z pliku na dysku na podstawie rozszerzenia (.epub, .docx, .txt, .md)
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref();
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let bytes =
            std::fs::read(p).with_context(|| format!("Nie można odczytać pliku: {:?}", p))?;
        let filename_title = p
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        match ext.as_str() {
            "epub" => Self::from_epub_bytes(&bytes, filename_title.as_deref()),
            "docx" => Self::from_docx_bytes(&bytes, filename_title.as_deref()),
            "txt" | "md" | "text" => {
                let text = String::from_utf8_lossy(&bytes);
                Ok(Self::from_text_str(&text, filename_title.as_deref()))
            }
            other => {
                bail!(
                    "Nieobsługiwany format pliku: '.{}'. Obsługiwane formaty: .epub, .docx, .txt, .md",
                    other
                );
            }
        }
    }

    /// Importuje plik tekstowy / Markdown z inteligentnym podziałem na rozdziały
    pub fn from_text_str(raw_text: &str, title_hint: Option<&str>) -> Self {
        let chapter_regex = Regex::new(
            r"(?i)(?:^|\n)\s*(?:#+\s*)?(?:rozdzia[łl]|rozdz\.|chapter|ch\.|akt|cz[ęe][śs][ćc]|prolog|epilog|ksi[ęe]ga|wst[ęe]p)\b[^\n]*",
        )
        .expect("Valid regex");

        let lines: Vec<&str> = raw_text.lines().collect();
        let mut chapters: Vec<ImportedChapter> = Vec::new();
        let mut current_title = String::new();
        let mut current_lines: Vec<&str> = Vec::new();

        let flush_chapter =
            |chapters: &mut Vec<ImportedChapter>, title: &mut String, lines: &mut Vec<&str>| {
                let body = lines.join("\n").trim().to_string();
                if !body.is_empty() || !title.is_empty() {
                    let actual_title = if title.is_empty() {
                        format!("Rozdział {}", chapters.len() + 1)
                    } else {
                        title.clone()
                    };
                    let words = body.split_whitespace().count();
                    // Średnia prędkość czytania lektora: ~140 słów na minutę (~2.33 słowa/sek)
                    let est_sec = (words as f32 / 2.33).ceil() as u32;

                    chapters.push(ImportedChapter {
                        index: chapters.len() + 1,
                        title: actual_title,
                        content: body,
                        word_count: words,
                        estimated_seconds: est_sec,
                    });
                }
                title.clear();
                lines.clear();
            };

        let is_heading = |line: &str| -> bool {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return false;
            }
            if trimmed.starts_with("# ") || trimmed.starts_with("## ") {
                return true;
            }
            chapter_regex.is_match(trimmed)
        };

        for line in lines {
            let trimmed = line.trim();
            if is_heading(trimmed) && trimmed.len() < 120 {
                flush_chapter(&mut chapters, &mut current_title, &mut current_lines);
                let clean_title = trimmed.trim_start_matches('#').trim().to_string();
                current_title = clean_title;
            } else {
                current_lines.push(line);
            }
        }
        flush_chapter(&mut chapters, &mut current_title, &mut current_lines);

        // Jeśli nie wykryto żadnego nagłówka rozdziału, potraktuj całość jako Rozdział 1
        if chapters.is_empty() && !raw_text.trim().is_empty() {
            let body = raw_text.trim().to_string();
            let words = body.split_whitespace().count();
            chapters.push(ImportedChapter {
                index: 1,
                title: title_hint.unwrap_or("Rozdział 1").to_string(),
                content: body,
                word_count: words,
                estimated_seconds: (words as f32 / 2.33).ceil() as u32,
            });
        }

        ImportedBook {
            title: title_hint.map(|s| s.to_string()),
            author: None,
            language: Some("pl".to_string()),
            chapters,
        }
    }

    /// Importuje książkę z formatu EPUB (kontener ZIP + pliki XHTML)
    pub fn from_epub_bytes(bytes: &[u8], title_hint: Option<&str>) -> Result<Self> {
        let reader = Cursor::new(bytes);
        let mut archive = ZipArchive::new(reader)
            .context("Nie udało się otworzyć pliku EPUB jako archiwum ZIP")?;

        let mut book_title = title_hint.map(|s| s.to_string());
        let mut book_author = None;
        let mut book_lang = Some("pl".to_string());

        // 1. Spróbuj odczytać metadane z content.opf jeśli istnieje
        for i in 0..archive.len() {
            let file_name = match archive.by_index(i) {
                Ok(entry) => entry.name().map(|n| n.to_string()).unwrap_or_default(),
                Err(_) => continue,
            };
            if file_name.ends_with(".opf") {
                if let Ok(mut opf_file) = archive.by_name(&file_name) {
                    let mut opf_xml = String::new();
                    let _ = opf_file.read_to_string(&mut opf_xml);
                    if let Some(t) = extract_xml_tag(&opf_xml, "dc:title") {
                        if !t.is_empty() {
                            book_title = Some(decode_html_entities(&t));
                        }
                    }
                    if let Some(a) = extract_xml_tag(&opf_xml, "dc:creator") {
                        if !a.is_empty() {
                            book_author = Some(decode_html_entities(&a));
                        }
                    }
                    if let Some(l) = extract_xml_tag(&opf_xml, "dc:language") {
                        if !l.is_empty() {
                            book_lang = Some(l.trim().to_lowercase());
                        }
                    }
                }
                break;
            }
        }

        // 2. Znajdź i posortuj pliki z treścią (.xhtml, .html)
        let mut content_entries: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            let name = match archive.by_index(i) {
                Ok(entry) => entry.name().map(|n| n.to_string()).unwrap_or_default(),
                Err(_) => continue,
            };
            let lower = name.to_lowercase();
            if (lower.ends_with(".xhtml") || lower.ends_with(".html"))
                && !lower.contains("cover")
                && !lower.contains("toc")
                && !lower.contains("nav")
            {
                content_entries.push(name);
            }
        }
        content_entries.sort();

        let mut chapters: Vec<ImportedChapter> = Vec::new();

        for entry_name in &content_entries {
            if let Ok(mut file) = archive.by_name(entry_name) {
                let mut html = String::new();
                if file.read_to_string(&mut html).is_ok() {
                    let text = html_to_plain_text(&html);
                    if !text.trim().is_empty() {
                        // Spróbuj wydobyć nagłówek z h1 / h2 lub z pierwszej linii
                        let title = extract_xml_tag(&html, "h1")
                            .or_else(|| extract_xml_tag(&html, "h2"))
                            .map(|h| decode_html_entities(&strip_html_tags(&h)))
                            .unwrap_or_else(|| {
                                text.lines()
                                    .next()
                                    .unwrap_or("")
                                    .chars()
                                    .take(60)
                                    .collect::<String>()
                            });

                        let actual_title = if title.trim().is_empty() {
                            format!("Rozdział {}", chapters.len() + 1)
                        } else {
                            title.trim().to_string()
                        };

                        let words = text.split_whitespace().count();
                        let est_sec = (words as f32 / 2.33).ceil() as u32;

                        chapters.push(ImportedChapter {
                            index: chapters.len() + 1,
                            title: actual_title,
                            content: text,
                            word_count: words,
                            estimated_seconds: est_sec,
                        });
                    }
                }
            }
        }

        if chapters.is_empty() {
            bail!("Nie znaleziono czytelnych rozdziałów w pliku EPUB");
        }

        Ok(ImportedBook {
            title: book_title,
            author: book_author,
            language: book_lang,
            chapters,
        })
    }

    /// Importuje książkę z formatu Microsoft Word (.docx)
    pub fn from_docx_bytes(bytes: &[u8], title_hint: Option<&str>) -> Result<Self> {
        let reader = Cursor::new(bytes);
        let mut archive = ZipArchive::new(reader)
            .context("Nie udało się otworzyć pliku DOCX jako archiwum ZIP")?;

        let mut doc_file = archive
            .by_name("word/document.xml")
            .context("Brak word/document.xml wewnątrz archiwum DOCX")?;

        let mut xml_content = String::new();
        doc_file
            .read_to_string(&mut xml_content)
            .context("Błąd odczytu document.xml")?;

        // Rozbij na akapity <w:p>
        let p_regex = Regex::new(r"(?s)<w:p\b[^>]*>(.*?)</w:p>").expect("Valid regex");
        let t_regex = Regex::new(r"<w:t\b[^>]*>(.*?)</w:t>").expect("Valid regex");
        let heading_regex =
            Regex::new(r#"<w:pStyle\s+w:val="Heading(\d)|Nag[łl][oó]wek(\d)|Title|Tytu[łl]""#)
                .expect("Valid regex");

        let mut extracted_paragraphs: Vec<(bool, String)> = Vec::new();

        for p_cap in p_regex.captures_iter(&xml_content) {
            let p_inner = &p_cap[1];
            let is_heading = heading_regex.is_match(p_inner);

            let mut text = String::new();
            for t_cap in t_regex.captures_iter(p_inner) {
                text.push_str(&decode_html_entities(&t_cap[1]));
            }

            let trimmed = text.trim();
            if !trimmed.is_empty() {
                extracted_paragraphs.push((is_heading, trimmed.to_string()));
            }
        }

        // Grupowanie w rozdziały
        let mut chapters = Vec::new();
        let mut current_title = String::new();
        let mut current_body_paragraphs = Vec::new();

        let flush = |chapters: &mut Vec<ImportedChapter>,
                     title: &mut String,
                     paragraphs: &mut Vec<String>| {
            if !paragraphs.is_empty() || !title.is_empty() {
                let body = paragraphs.join("\n\n");
                let actual_title = if title.is_empty() {
                    format!("Rozdział {}", chapters.len() + 1)
                } else {
                    title.clone()
                };
                let words = body.split_whitespace().count();
                let est_sec = (words as f32 / 2.33).ceil() as u32;

                chapters.push(ImportedChapter {
                    index: chapters.len() + 1,
                    title: actual_title,
                    content: body,
                    word_count: words,
                    estimated_seconds: est_sec,
                });
            }
            title.clear();
            paragraphs.clear();
        };

        let chapter_keyword_re = Regex::new(
            r"(?i)^(?:rozdzia[łl]|rozdz\.|chapter|ch\.|akt|cz[ęe][śs][ćc]|prolog|epilog)\b",
        )
        .expect("Valid regex");

        for (is_h, p_text) in extracted_paragraphs {
            let is_chapter_keyword = chapter_keyword_re.is_match(&p_text);

            if (is_h || is_chapter_keyword) && p_text.len() < 120 {
                flush(
                    &mut chapters,
                    &mut current_title,
                    &mut current_body_paragraphs,
                );
                current_title = p_text;
            } else {
                current_body_paragraphs.push(p_text);
            }
        }
        flush(
            &mut chapters,
            &mut current_title,
            &mut current_body_paragraphs,
        );

        if chapters.is_empty() {
            bail!("Nie znaleziono tekstu w dokumencie DOCX");
        }

        Ok(ImportedBook {
            title: title_hint.map(|s| s.to_string()),
            author: None,
            language: Some("pl".to_string()),
            chapters,
        })
    }
}

/// Dekoduje ciąg base64 do wektora bajtów
pub fn base64_decode(input: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for b in input.bytes() {
        let val = match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' | b'\t' => continue,
            _ => bail!("Nieprawidłowy znak base64: {}", b as char),
        };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Usuwa znaczniki HTML z tekstu
pub fn strip_html_tags(html: &str) -> String {
    let re = Regex::new(r"<[^>]*>").expect("Valid regex");
    re.replace_all(html, "").to_string()
}

/// Dekoduje encje HTML (&nbsp;, &mdash;, &amp; itp.)
pub fn decode_html_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&mdash;", "—")
        .replace("&#8212;", "—")
        .replace("&ndash;", "–")
        .replace("&#8211;", "–")
        .replace("&hellip;", "...")
        .replace("&#8230;", "...")
        .replace("&laquo;", "«")
        .replace("&raquo;", "»")
}

/// Wyciąga zawartość prostego tagu XML (np. <dc:title>Tytuł</dc:title>)
fn extract_xml_tag(xml: &str, tag_name: &str) -> Option<String> {
    let pattern = format!(r"(?s)<{}\b[^>]*>(.*?)</{}>", tag_name, tag_name);
    if let Ok(re) = Regex::new(&pattern) {
        if let Some(cap) = re.captures(xml) {
            return Some(cap[1].trim().to_string());
        }
    }
    None
}

/// Konwertuje HTML/XHTML do czystego tekstu zachowując strukturę akapitów
fn html_to_plain_text(html: &str) -> String {
    // 1. Zamień tagi blokowe na nowe linie
    let block_re = Regex::new(r"(?i)</?(?:p|div|br|h1|h2|h3|h4|h5|h6|li|blockquote)\b[^>]*>")
        .expect("Valid regex");
    let with_newlines = block_re.replace_all(html, "\n");

    // 2. Usuń pozostałe tagi inline (<span...>, <em...>, <strong...> itp.)
    let stripped = strip_html_tags(&with_newlines);

    // 3. Odkoduj encje HTML
    let decoded = decode_html_entities(&stripped);

    // 4. Uporządkuj puste linie
    let mut cleaned_lines = Vec::new();
    for line in decoded.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            cleaned_lines.push(trimmed);
        }
    }

    cleaned_lines.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_text_chapters() {
        let text = r#"
Rozdział 1: Początek Wyprawy
To było ciemnej i burzliwej nocy. Deszcz bębnił o parapet.

Rozdział 2: Zaginiony Artefakt
Weszli do komnaty. Na ołtarzu stała złota szkatułka.
"#;
        let book = ImportedBook::from_text_str(text, Some("Tajemnica"));
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "Rozdział 1: Początek Wyprawy");
        assert!(book.chapters[0].content.contains("To było ciemnej"));
        assert_eq!(book.chapters[1].title, "Rozdział 2: Zaginiony Artefakt");

        let script = book.to_script();
        assert!(script.contains("📖 Rozdział 1: Początek Wyprawy 🛑"));
        assert!(script.contains("📖 Rozdział 2: Zaginiony Artefakt 🛑"));
    }

    #[test]
    fn test_import_markdown_headers() {
        let md = r#"
# Prolog
Cisza panowała w całym układzie.

## Rozdział I: Stacja Orbitalna
Kapitan spojrzał na radar.
"#;
        let book = ImportedBook::from_text_str(md, None);
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "Prolog");
        assert_eq!(book.chapters[1].title, "Rozdział I: Stacja Orbitalna");
    }

    #[test]
    fn test_html_entities_and_tags() {
        let html = "<p>Jan powiedzia&lstrok;: &bdquo;Nie ma sprawy!&rdquo;&nbsp;&mdash; i wyszed&lstrok;.</p>";
        let cleaned = decode_html_entities(&strip_html_tags(html));
        assert!(cleaned.contains("—"));
    }

    #[test]
    fn test_import_synthetic_epub() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            let options = SimpleFileOptions::default();

            zip.start_file("mimetype", options).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();

            zip.start_file("OEBPS/content.opf", options).unwrap();
            let opf = r#"<package><metadata>
                <dc:title>Testowa Książka</dc:title>
                <dc:creator>Jan Testowy</dc:creator>
                <dc:language>pl</dc:language>
            </metadata></package>"#;
            zip.write_all(opf.as_bytes()).unwrap();

            zip.start_file("OEBPS/ch01.xhtml", options).unwrap();
            let ch1 = r#"<html><body><h1>Rozdział Pierwszy</h1><p>Pierwsze zdanie testu książki.</p></body></html>"#;
            zip.write_all(ch1.as_bytes()).unwrap();

            zip.finish().unwrap();
        }

        let book = ImportedBook::from_epub_bytes(&buf, None).expect("Should parse EPUB");
        assert_eq!(book.title.as_deref(), Some("Testowa Książka"));
        assert_eq!(book.author.as_deref(), Some("Jan Testowy"));
        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.chapters[0].title, "Rozdział Pierwszy");
        assert!(book.chapters[0].content.contains("Pierwsze zdanie testu"));
    }

    #[test]
    fn test_import_synthetic_docx() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            let options = SimpleFileOptions::default();

            zip.start_file("word/document.xml", options).unwrap();
            let docx_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:body>
                    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Rozdział 1: Wstęp</w:t></w:r></w:p>
                    <w:p><w:r><w:t>Akapit pierwszy w dokumencie Word.</w:t></w:r></w:p>
                </w:body>
            </w:document>"#;
            zip.write_all(docx_xml.as_bytes()).unwrap();

            zip.finish().unwrap();
        }

        let book =
            ImportedBook::from_docx_bytes(&buf, Some("Moja Książka")).expect("Should parse DOCX");
        assert_eq!(book.title.as_deref(), Some("Moja Książka"));
        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.chapters[0].title, "Rozdział 1: Wstęp");
        assert!(book.chapters[0].content.contains("Akapit pierwszy"));
    }
}
