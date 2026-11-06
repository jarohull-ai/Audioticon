use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;
use zip::ZipArchive;

/// Usuwa powtórzony nagłówek/tytuł rozdziału z początku jego treści
pub fn strip_leading_title(content: &str, title: &str) -> String {
    let clean_title = title.trim();
    if clean_title.is_empty() {
        return content.trim().to_string();
    }
    let norm_title = clean_title.to_lowercase();
    let lines: Vec<&str> = content.lines().collect();
    let mut start_idx = 0;

    while start_idx < lines.len() {
        let line = lines[start_idx].trim();
        if line.is_empty() {
            start_idx += 1;
            continue;
        }
        let line_lower = line.to_lowercase();
        let stripped_line = line_lower.trim_start_matches('#').trim();

        if stripped_line == norm_title
            || line_lower == norm_title
            || (norm_title.starts_with("rozdział") && line_lower.starts_with("rozdział"))
            || (norm_title.starts_with("chapter") && line_lower.starts_with("chapter"))
            || (norm_title == "prolog" && line_lower == "prolog")
            || (norm_title == "epilog" && line_lower == "epilog")
        {
            start_idx += 1;
        } else {
            break;
        }
    }

    lines[start_idx..].join("\n").trim().to_string()
}

/// Rozpoznaje język tekstu (domyślnie 'pl', a przy dominacji słów angielskich i braku polskich znaków 'en')
pub fn detect_text_language(text: &str) -> String {
    let polish_chars = ['ą', 'ć', 'ę', 'ł', 'ń', 'ó', 'ś', 'ź', 'ż'];
    let lower = text.to_lowercase();
    let pl_char_count = lower.chars().filter(|c| polish_chars.contains(c)).count();

    if pl_char_count > 0 {
        return "pl".to_string();
    }

    // Sprawdź częste angielskie słówka stopwords
    let words: Vec<&str> = lower.split_whitespace().take(300).collect();
    let mut en_matches = 0;
    for w in &words {
        let clean = w.trim_matches(|c: char| !c.is_alphabetic());
        match clean {
            "the" | "and" | "of" | "to" | "in" | "is" | "that" | "it" | "was" | "for" | "on"
            | "are" | "as" | "with" | "his" | "they" | "at" | "be" | "this" | "from"
            | "chapter" => {
                en_matches += 1;
            }
            _ => {}
        }
    }

    if en_matches >= 5 {
        "en".to_string()
    } else {
        "pl".to_string()
    }
}

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

    /// Importuje książkę z pliku na dysku na podstawie rozszerzenia (.epub, .pdf, .docx, .doc, .odt, .rtf, .txt, .md)
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

        Self::from_bytes(&bytes, &ext, filename_title.as_deref())
    }

    /// Uniwersalny importer z pamięci (obsługuje .epub, .pdf, .docx, .doc, .odt, .rtf, .txt, .md)
    pub fn from_bytes(bytes: &[u8], ext: &str, title_hint: Option<&str>) -> Result<Self> {
        let clean_ext = ext.trim().trim_start_matches('.').to_lowercase();
        match clean_ext.as_str() {
            "epub" => Self::from_epub_bytes(bytes, title_hint),
            "docx" => Self::from_docx_bytes(bytes, title_hint),
            "odt" => Self::from_odt_bytes(bytes, title_hint),
            "pdf" => Self::from_pdf_bytes(bytes, title_hint),
            "doc" => Self::from_doc_bytes(bytes, title_hint),
            "rtf" => Self::from_rtf_bytes(bytes, title_hint),
            "txt" | "md" | "markdown" | "text" => {
                let text = String::from_utf8_lossy(bytes);
                Ok(Self::from_text_str(&text, title_hint))
            }
            other => {
                bail!(
                    "Nieobsługiwany format pliku: '.{}'. Obsługiwane formaty: .epub, .pdf, .docx, .doc, .odt, .rtf, .txt, .md",
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
                    let clean_body = strip_leading_title(&body, &actual_title);
                    let words = clean_body.split_whitespace().count();
                    // Średnia prędkość czytania lektora: ~140 słów na minutę (~2.33 słowa/sek)
                    let est_sec = (words as f32 / 2.33).ceil() as u32;

                    chapters.push(ImportedChapter {
                        index: chapters.len() + 1,
                        title: actual_title,
                        content: clean_body,
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

        let detected_lang = detect_text_language(raw_text);

        ImportedBook {
            title: title_hint.map(|s| s.to_string()),
            author: None,
            language: Some(detected_lang),
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

        let mut manifest_items: HashMap<String, String> = HashMap::new();
        let mut spine_order: Vec<String> = Vec::new();
        let mut opf_found = false;

        let item_regex = Regex::new(
            r#"<item\b[^>]*\bid="([^"]+)"[^>]*\bhref="([^"]+)"|<item\b[^>]*\bhref="([^"]+)"[^>]*\bid="([^"]+)""#,
        )
        .expect("Valid regex");
        let spine_regex = Regex::new(r#"<itemref\b[^>]*\bidref="([^"]+)""#).expect("Valid regex");

        // 1. Spróbuj odczytać metadane z content.opf oraz zmapować manifest i spine
        for i in 0..archive.len() {
            let file_name = match archive.by_index(i) {
                Ok(entry) => entry.name().map(|n| n.to_string()).unwrap_or_default(),
                Err(_) => continue,
            };
            if file_name.ends_with(".opf") {
                if let Ok(mut opf_file) = archive.by_name(&file_name) {
                    let mut opf_xml = String::new();
                    let _ = opf_file.read_to_string(&mut opf_xml);
                    opf_found = true;

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

                    // Katalog relatywny dla plików zadeklarowanych w OPF
                    let opf_dir = if let Some(pos) = file_name.rfind('/') {
                        file_name[..=pos].to_string()
                    } else {
                        String::new()
                    };

                    // Parsowanie <manifest>
                    for cap in item_regex.captures_iter(&opf_xml) {
                        let (id, href) = if let (Some(id), Some(href)) = (cap.get(1), cap.get(2)) {
                            (id.as_str(), href.as_str())
                        } else if let (Some(href), Some(id)) = (cap.get(3), cap.get(4)) {
                            (id.as_str(), href.as_str())
                        } else {
                            continue;
                        };
                        let clean_href = href.split('#').next().unwrap_or(href);
                        let full_path = if clean_href.starts_with('/') {
                            clean_href.trim_start_matches('/').to_string()
                        } else {
                            format!("{}{}", opf_dir, clean_href)
                        };
                        manifest_items.insert(id.to_string(), full_path);
                    }

                    // Parsowanie <spine>
                    for cap in spine_regex.captures_iter(&opf_xml) {
                        spine_order.push(cap[1].to_string());
                    }
                }
                break;
            }
        }

        // 2. Wyznacz kolejność plików z treścią na podstawie <spine> lub fallback na sortowanie nazw plików
        let mut content_entries: Vec<String> = Vec::new();

        if opf_found && !spine_order.is_empty() {
            for idref in &spine_order {
                if let Some(target_path) = manifest_items.get(idref) {
                    let lower = target_path.to_lowercase();
                    if (lower.ends_with(".xhtml")
                        || lower.ends_with(".html")
                        || lower.ends_with(".htm"))
                        && !lower.contains("cover")
                        && !lower.contains("toc")
                        && !lower.contains("nav")
                        && archive.by_name(target_path).is_ok()
                        && !content_entries.contains(target_path)
                    {
                        content_entries.push(target_path.clone());
                    }
                }
            }
        }

        // Fallback: jeśli spine nie dało plików, przeskanuj archiwum ZIP alfabetycznie
        if content_entries.is_empty() {
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
        }

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

                        // Usuń zduplikowany tytuł z początku treści rozdziału
                        let clean_text = strip_leading_title(&text, &actual_title);
                        let words = clean_text.split_whitespace().count();
                        let est_sec = (words as f32 / 2.33).ceil() as u32;

                        chapters.push(ImportedChapter {
                            index: chapters.len() + 1,
                            title: actual_title,
                            content: clean_text,
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
                let clean_body = strip_leading_title(&body, &actual_title);
                let words = clean_body.split_whitespace().count();
                let est_sec = (words as f32 / 2.33).ceil() as u32;

                chapters.push(ImportedChapter {
                    index: chapters.len() + 1,
                    title: actual_title,
                    content: clean_body,
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

    /// Importuje książkę z formatu OpenDocument Text (.odt — LibreOffice / OpenOffice)
    pub fn from_odt_bytes(bytes: &[u8], title_hint: Option<&str>) -> Result<Self> {
        let reader = Cursor::new(bytes);
        let mut archive = ZipArchive::new(reader)
            .context("Nie udało się otworzyć pliku ODT jako archiwum ZIP")?;

        let mut content_file = archive
            .by_name("content.xml")
            .context("Brak content.xml wewnątrz archiwum ODT")?;

        let mut xml_content = String::new();
        content_file
            .read_to_string(&mut xml_content)
            .context("Błąd odczytu content.xml")?;

        let block_re = Regex::new(r"(?s)<text:(?:p|h)\b[^>]*>(.*?)</text:(?:p|h)>")
            .expect("Valid ODT regex");
        let mut lines = Vec::new();
        for cap in block_re.captures_iter(&xml_content) {
            let inner = decode_html_entities(&strip_html_tags(&cap[1]));
            let trimmed = inner.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
        let joined = lines.join("\n\n");
        if joined.trim().is_empty() {
            bail!("Nie znaleziono tekstu w dokumencie ODT");
        }
        Ok(Self::from_text_str(&joined, title_hint))
    }

    /// Importuje książkę z formatu PDF (.pdf) — używa `pdftotext` (jeśli dostępny) lub natywnego ekstraktora strumieni PDF
    pub fn from_pdf_bytes(bytes: &[u8], title_hint: Option<&str>) -> Result<Self> {
        // 1. Próba użycia systemowego pdftotext (Poppler) dla pełnej obsługi skompresowanych fontów CID
        let tmp_dir = std::env::temp_dir();
        let pid = std::process::id();
        let tmp_pdf = tmp_dir.join(format!("audioticon_import_{}.pdf", pid));
        if std::fs::write(&tmp_pdf, bytes).is_ok() {
            let output = std::process::Command::new("pdftotext")
                .arg("-enc")
                .arg("UTF-8")
                .arg(&tmp_pdf)
                .arg("-")
                .output();
            let _ = std::fs::remove_file(&tmp_pdf);
            if let Ok(out) = output {
                if out.status.success() {
                    let extracted = String::from_utf8_lossy(&out.stdout).to_string();
                    if !extracted.trim().is_empty() {
                        return Ok(Self::from_text_str(&extracted, title_hint));
                    }
                }
            }
        }

        // 2. Natywny fallback w czystym Rust: ekstrakcja operatorów tekstowych Tj / TJ z pliku PDF
        let extracted = extract_pdf_text_native(bytes);
        if extracted.trim().is_empty() {
            bail!("Nie udało się wyodrębnić warstwy tekstowej z pliku PDF (plik może być zeskanowanym obrazem bez OCR)");
        }
        Ok(Self::from_text_str(&extracted, title_hint))
    }

    /// Importuje książkę z klasycznego formatu Microsoft Word (.doc)
    pub fn from_doc_bytes(bytes: &[u8], title_hint: Option<&str>) -> Result<Self> {
        // Jeśli plik .doc to w rzeczywistości ZIP (.docx) lub RTF
        if bytes.starts_with(b"PK\x03\x04") {
            return Self::from_docx_bytes(bytes, title_hint);
        }
        if bytes.starts_with(b"{\\rtf") {
            return Self::from_rtf_bytes(bytes, title_hint);
        }

        // Próba użycia `antiword` lub `catdoc`, jeśli zainstalowane w systemie
        let tmp_dir = std::env::temp_dir();
        let tmp_doc = tmp_dir.join(format!("audioticon_import_{}.doc", std::process::id()));
        if std::fs::write(&tmp_doc, bytes).is_ok() {
            for tool in ["antiword", "catdoc"] {
                if let Ok(out) = std::process::Command::new(tool).arg(&tmp_doc).output() {
                    if out.status.success() {
                        let txt = String::from_utf8_lossy(&out.stdout).to_string();
                        if txt.split_whitespace().count() >= 3 {
                            let _ = std::fs::remove_file(&tmp_doc);
                            return Ok(Self::from_text_str(&txt, title_hint));
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(&tmp_doc);
        }

        // Natywny ekstraktor strumienia tekstowego z binarnego pliku OLE2 Word (.doc)
        let extracted = extract_legacy_doc_text(bytes);
        if extracted.trim().is_empty() {
            bail!("Nie znaleziono czytelnego tekstu w pliku .doc");
        }
        Ok(Self::from_text_str(&extracted, title_hint))
    }

    /// Importuje książkę z formatu Rich Text Format (.rtf)
    pub fn from_rtf_bytes(bytes: &[u8], title_hint: Option<&str>) -> Result<Self> {
        let raw = String::from_utf8_lossy(bytes);
        let text = extract_rtf_text(&raw);
        if text.trim().is_empty() {
            bail!("Nie znaleziono czytelnego tekstu w pliku RTF");
        }
        Ok(Self::from_text_str(&text, title_hint))
    }
}

/// Natywna ekstrakcja tekstu z PDF (obsługa operatorów `(tekst) Tj` oraz `[(tekst)] TJ`)
fn extract_pdf_text_native(bytes: &[u8]) -> String {
    let raw = String::from_utf8_lossy(bytes);
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current_line = String::new();

    // Dopasuj literały w nawiasach przed Tj / TJ lub wewnątrz tablic TJ
    let literal_re = Regex::new(r"\(([^()\\]*(?:\\.[^()\\]*)*)\)").expect("Valid PDF regex");
    let bt_et_re = Regex::new(r"(?s)\bBT\b(.*?)\bET\b").expect("Valid BT/ET regex");

    let mut found_bt = false;
    for block in bt_et_re.captures_iter(&raw) {
        found_bt = true;
        let inner = &block[1];
        for cap in literal_re.captures_iter(inner) {
            let s = unescape_pdf_string(&cap[1]);
            if !s.is_empty() {
                current_line.push_str(&s);
            }
        }
        if !current_line.trim().is_empty() {
            paragraphs.push(current_line.trim().to_string());
            current_line.clear();
        }
    }

    if !found_bt {
        for cap in literal_re.captures_iter(&raw) {
            let s = unescape_pdf_string(&cap[1]);
            if s.chars().any(|c| c.is_alphabetic()) {
                paragraphs.push(s.trim().to_string());
            }
        }
    }

    paragraphs.join("\n")
}

fn unescape_pdf_string(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
        .replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\\\", "\\")
}

/// Natywna ekstrakcja tekstu z formatu RTF (z obsługą polskich znaków CP1250 `\'xx` oraz `\uNNNN`)
fn extract_rtf_text(rtf: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = rtf.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' || c == '}' {
            i += 1;
            continue;
        }
        if c == '\\' {
            i += 1;
            if i >= chars.len() {
                break;
            }
            if chars[i] == '\'' && i + 2 < chars.len() {
                let hex_str: String = chars[i + 1..=i + 2].iter().collect();
                if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                    out.push(cp1250_byte_to_char(byte));
                }
                i += 3;
                continue;
            }
            if chars[i] == '\\' || chars[i] == '{' || chars[i] == '}' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let mut word = String::new();
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                word.push(chars[i]);
                i += 1;
            }
            let mut num = String::new();
            if i < chars.len() && (chars[i] == '-' || chars[i].is_ascii_digit()) {
                num.push(chars[i]);
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    num.push(chars[i]);
                    i += 1;
                }
            }
            if i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            match word.as_str() {
                "par" | "line" | "sect" => out.push('\n'),
                "tab" => out.push('\t'),
                "u" => {
                    if let Ok(code) = num.parse::<i32>() {
                        let ucode = if code < 0 {
                            (code + 65536) as u32
                        } else {
                            code as u32
                        };
                        if let Some(ch) = char::from_u32(ucode) {
                            out.push(ch);
                        }
                        if i < chars.len() && chars[i] == '?' {
                            i += 1;
                        }
                    }
                }
                _ => {}
            }
        } else if c != '\r' && c != '\n' {
            out.push(c);
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

fn cp1250_byte_to_char(b: u8) -> char {
    match b {
        0xA5 => 'Ą',
        0xB9 => 'ą',
        0xC6 => 'Ć',
        0xE6 => 'ć',
        0xCA => 'Ę',
        0xEA => 'ę',
        0xA3 => 'Ł',
        0xB3 => 'ł',
        0xD1 => 'Ń',
        0xF1 => 'ń',
        0xD3 => 'Ó',
        0xF3 => 'ó',
        0x8C => 'Ś',
        0x9C => 'ś',
        0x8F => 'Ź',
        0x9F => 'ź',
        0xAF => 'Ż',
        0xBF => 'ż',
        0x84 => '„',
        0x94 => '”',
        0x96 => '–',
        0x97 => '—',
        other if other >= 0x20 => other as char,
        _ => ' ',
    }
}

/// Wyciąga ciągłe bloki tekstu UTF-16LE oraz UTF-8/CP1250 z binarnych plików MS Word (.doc)
fn extract_legacy_doc_text(bytes: &[u8]) -> String {
    // 1. Sprawdź czy to czysty tekst z rozszerzeniem .doc
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        if !bytes.starts_with(b"\xD0\xCF\x11\xE0") {
            return utf8.to_string();
        }
    }

    // 2. Próba odczytu bloków UTF-16LE (standard w Word 97-2003 FIB)
    let mut utf16_runs = Vec::new();
    let mut current_u16 = String::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let code = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        let ch_opt = char::from_u32(code as u32);
        if let Some(ch) = ch_opt {
            let valid = ch.is_alphabetic()
                || ch.is_ascii_newline_or_punct()
                || matches!(ch, ' ' | '—' | '–' | '„' | '”');
            if valid {
                current_u16.push(ch);
                i += 2;
                continue;
            }
        }
        if current_u16.chars().filter(|c| c.is_alphabetic()).count() >= 12 {
            utf16_runs.push(current_u16.trim().to_string());
        }
        current_u16.clear();
        i += 2;
    }
    if current_u16.chars().filter(|c| c.is_alphabetic()).count() >= 12 {
        utf16_runs.push(current_u16.trim().to_string());
    }
    if !utf16_runs.is_empty() {
        return utf16_runs.join("\n\n");
    }

    // 3. Fallback na 8-bitowe ciągi znaków (ASCII / CP1250)
    let mut runs = Vec::new();
    let mut cur = String::new();
    for &b in bytes {
        if b == b'\n' || b == b'\r' || (0x20..=0x7E).contains(&b) {
            cur.push(if b == b'\r' { '\n' } else { b as char });
        } else {
            if cur.chars().filter(|c| c.is_alphabetic()).count() >= 10 {
                runs.push(cur.trim().to_string());
            }
            cur.clear();
        }
    }
    if cur.chars().filter(|c| c.is_alphabetic()).count() >= 10 {
        runs.push(cur.trim().to_string());
    }
    runs.join("\n\n")
}

trait CharDocExt {
    fn is_ascii_newline_or_punct(&self) -> bool;
}
impl CharDocExt for char {
    fn is_ascii_newline_or_punct(&self) -> bool {
        *self == '\n'
            || *self == '\r'
            || self.is_ascii_digit()
            || self.is_ascii_punctuation()
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

    #[test]
    fn test_strip_duplicate_titles() {
        let content =
            "Prolog\n\nProlog\n\nWielka przestrzeń kosmiczna rozpościerała się w nieskończoność.";
        let cleaned = strip_leading_title(content, "Prolog");
        assert_eq!(
            cleaned,
            "Wielka przestrzeń kosmiczna rozpościerała się w nieskończoność."
        );

        let md_content = "## Rozdział 1\nRozdział 1\nTekst właściwy rozdziału.";
        let cleaned_md = strip_leading_title(md_content, "Rozdział 1");
        assert_eq!(cleaned_md, "Tekst właściwy rozdziału.");
    }

    #[test]
    fn test_language_detection_pl_en() {
        let pl_text = "To jest wspaniała książka pełna przygód, emocji i niezwykłych bohaterów.";
        assert_eq!(detect_text_language(pl_text), "pl");

        let en_text = "This is a great book with exciting characters and adventures in the galaxy.";
        assert_eq!(detect_text_language(en_text), "en");
    }

    #[test]
    fn test_epub_spine_canonical_order() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            let options = SimpleFileOptions::default();

            zip.start_file("mimetype", options).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();

            // OPF z manifestem i spine definiującym kolejność inną niż alfabetyczna:
            // z_dedication.xhtml (id: ded) -> a_ch01.xhtml (id: ch1)
            zip.start_file("OEBPS/content.opf", options).unwrap();
            let opf = r#"<package><metadata>
                <dc:title>Kolejność Spine</dc:title>
            </metadata>
            <manifest>
                <item id="ch1" href="a_ch01.xhtml" media-type="application/xhtml+xml"/>
                <item id="ded" href="z_dedication.xhtml" media-type="application/xhtml+xml"/>
            </manifest>
            <spine>
                <itemref idref="ded"/>
                <itemref idref="ch1"/>
            </spine>
            </package>"#;
            zip.write_all(opf.as_bytes()).unwrap();

            zip.start_file("OEBPS/a_ch01.xhtml", options).unwrap();
            zip.write_all(
                r#"<html><body><h1>Rozdział 1</h1><p>Tekst rozdziału 1.</p></body></html>"#
                    .as_bytes(),
            )
            .unwrap();

            zip.start_file("OEBPS/z_dedication.xhtml", options).unwrap();
            zip.write_all(
                r#"<html><body><h1>Dedykacja</h1><p>Dla wszystkich czytelników.</p></body></html>"#
                    .as_bytes(),
            )
            .unwrap();

            zip.finish().unwrap();
        }

        let book = ImportedBook::from_epub_bytes(&buf, None).expect("Should parse EPUB with spine");
        assert_eq!(book.chapters.len(), 2);
        // Dedykacja musi być pierwsza zgodnie ze spine, mimo że nazwa pliku z_dedication.xhtml jest alfabetycznie na końcu!
        assert_eq!(book.chapters[0].title, "Dedykacja");
        assert_eq!(book.chapters[1].title, "Rozdział 1");
    }

    #[test]
    fn test_import_pdf_rtf_odt_and_doc() {
        // 1. Test PDF native stream extraction
        let fake_pdf = b"%PDF-1.4\n1 0 obj\n<< /Length 120 >>\nstream\nBT\n(Rozdzial 1: Tajemnica) Tj\nET\nBT\n(W ciemnym lesie slychac bylo szept wiatru.) Tj\nET\nendstream\nendobj\n%%EOF";
        let pdf_book =
            ImportedBook::from_bytes(fake_pdf, "pdf", Some("Tajemnica PDF")).expect("PDF import");
        assert!(!pdf_book.chapters.is_empty());
        assert!(pdf_book.to_script().contains("szept wiatru"));

        // 2. Test RTF with Polish characters
        let rtf_bytes = br#"{\rtf1\ansi\ansicpg1250
Rozdzia\'b3 1: Powr\'f3t\par
Za oknem pada\'b3 g\'easty deszcz.\par
}"#;
        let rtf_book =
            ImportedBook::from_bytes(rtf_bytes, "rtf", Some("Książka RTF")).expect("RTF import");
        assert_eq!(rtf_book.chapters[0].title, "Rozdział 1: Powrót");
        assert!(rtf_book.chapters[0].content.contains("padał gęsty deszcz"));

        // 3. Test ODT (OpenDocument Writer)
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut odt_buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut odt_buf));
            let options = SimpleFileOptions::default();
            zip.start_file("content.xml", options).unwrap();
            let xml = r#"<office:document-content>
                <office:body><office:text>
                    <text:h>Rozdział 1: LibreOffice</text:h>
                    <text:p>To jest treść rozdziału z dokumentu ODT.</text:p>
                </office:text></office:body>
            </office:document-content>"#;
            zip.write_all(xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        let odt_book =
            ImportedBook::from_bytes(&odt_buf, "odt", Some("Test ODT")).expect("ODT import");
        assert_eq!(odt_book.chapters[0].title, "Rozdział 1: LibreOffice");
        assert!(odt_book.chapters[0].content.contains("dokumentu ODT"));

        // 4. Test legacy .doc (UTF-16LE embedded stream in OLE2 container)
        let mut doc_bytes = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1\x00\x00\x00\x00".to_vec();
        let sample_u16 = "Rozdział 1: Stary Word\nTo jest tekst zapisany w klasycznym formacie binarnego dokumentu Word.";
        for u in sample_u16.encode_utf16() {
            doc_bytes.extend_from_slice(&u.to_le_bytes());
        }
        doc_bytes.extend_from_slice(&[0u8; 16]);
        let doc_book =
            ImportedBook::from_bytes(&doc_bytes, "doc", Some("Stary Word")).expect("DOC import");
        assert!(!doc_book.chapters.is_empty());
        assert!(doc_book.to_script().contains("klasycznym formacie"));
    }
}
