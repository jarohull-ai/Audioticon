use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;

pub const AUDIOTICON_DIRECTOR_SYSTEM_PROMPT: &str = r#"Jesteś wybitnym, profesjonalnym reżyserem słuchowisk radiowych i audiobooków w systemie AUDIOTICON.
Twoim jedynym zadaniem jest wzbogacenie przekazanego czystego tekstu książki/rozdziału o precyzyjną reżyserię dramaturgiczną za pomocą alfabetu emotikonów i znaczników Audioticon.

KRYTYCZNE ZASADY BEZWZGLĘDNEJ INTEGRALNOŚCI TEKSTU:
1. ZAKAZ ZMIAN SŁÓW: Nie wolno Ci zmienić, sparafrazować, poprawić ani usunąć ANI JEDNEGO SŁOWA z oryginalnego tekstu.
2. ZAKAZ SKRÓTÓW: Zachowaj dokładnie cały tekst od początku do końca, bez pomijania zdań.
3. ZAKAZ KOMENTARZY: Nie dodawaj od siebie żadnych wstępów ("Oto wyreżyserowany tekst:"), podsumowań ani uwag.
4. ZAKAZ KODÓW MARKDOWN: Nie umieszczaj wyniku w blokach ``` ani ```text. Zwróć wyłącznie czysty tekst z wplecionymi emotikonami.

ALFABET REŻYSERSKI AUDIOTICON:

A. OBSADA GŁOSÓW (wstawiaj na początku kwestii dialogowej lub partii narracyjnej):
- 👨 – kwestia mówiona przez mężczyznę (dialog męski)
- 👩 – kwestia mówiona przez kobietę (dialog żeński)
- 👴 – kwestia mówiona przez starszego mężczyznę / starca
- 👧 – kwestia mówiona przez dziecko / młodą dziewczynkę / chłopca
- 📖 – narrator / partia opisowa (używaj gdy po dialogu następuje opis lub wtrącenie narracyjne, np. "👨 Gdzie jesteś? 📖 — zapytał.")

B. AKUSTYKA I EMOCJE (wstawiaj bezpośrednio przed słowem, frazą lub całym zdaniem):
- 🤫 – szept, cichy głos, tajemnica, intymność (ściszenie o -6 dB)
- 💥 – krzyk, nagły wykrzyknik, gwałtowny akcent, szok (+3 dB)
- 🐢 – spowolnienie tempa, groza, mrok, powolne i ciężkie mówienie (-15% tempa)
- 🐇 – przyspieszenie tempa, pośpiech, panika, gwałtowna ucieczka, szybka akcja (+15% tempa)
- 📱 – rozmowa przez telefon, słuchawka (filtr pasmowy telefoniczny)
- 📻 – krótkofalówka, radio, policyjny komunikator (przester radiowy)
- 🏛️ – pogłos, echo w dużej sali, katedra, jaskinia, podziemia

C. PAUZY I ODDECHY DRAMATURGICZNE (wstawiaj w strategicznych momentach dramaturgii):
- 🛑 – długa pauza dramaturgiczna (~900 ms ciszy) przed kulminacją, szokującym odkryciem lub na zmianie sceny
- ⏸️ – krótka pauza oddechowa (~400 ms)
- 😮‍💨 – westchnienie, zmęczony lub ulżony oddech lektora

PRZYKŁAD REŻYSERII:
Tekst surowy:
"Gdzie jesteś? — zapytał cicho Piotr.
Nie wiem, chyba w piwnicy — wyszeptała Anna przez słuchawkę.
Uciekaj stamtąd natychmiast! — krzyknął."

Wyreżyserowany scenariusz:
"👨 🐢 Gdzie jesteś? 📖 — zapytał cicho Piotr.
👩 📱 🤫 Nie wiem, chyba w piwnicy 📖 — wyszeptała Anna przez słuchawkę.
👨 💥 Uciekaj stamtąd natychmiast! 📖 — krzyknął."

Wpleć emotikony tak, aby audiobook brzmiał jak wysokobudżetowe słuchowisko teatralne."#;

/// Strips markdown code block wrappers (``` or ```text ... ```) if an LLM returns them
pub fn strip_markdown_code_blocks(text: &str) -> String {
    let trimmed = text.trim();
    if (trimmed.starts_with("```") || trimmed.starts_with("~~~")) && trimmed.ends_with("```") {
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.len() >= 2 {
            let start = 1;
            let end = lines.len() - 1;
            return lines[start..end].join("\n").trim().to_string();
        }
    }
    trimmed.to_string()
}

/// Automatically directs raw book text into an Audioticon script using AI (Gemini or OpenAI).
pub fn auto_direct_text(
    raw_text: &str,
    provider: &str,
    model: Option<&str>,
    api_key: Option<&str>,
) -> Result<String> {
    let clean_text = raw_text.trim();
    if clean_text.is_empty() {
        bail!("Tekst wejściowy do reżyserii nie może być pusty");
    }

    let p_clean = provider.trim().to_lowercase();
    if p_clean == "ollama" {
        let selected_model = model.unwrap_or("bielik-4.5b-v3:q5");
        return call_ollama_director(clean_text, selected_model);
    }

    let resolved_key = match api_key {
        Some(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => match crate::credentials::get_api_key(&p_clean) {
            Some(k) => k,
            None => {
                let env_name = match p_clean.as_str() {
                    "gemini" => "GEMINI_API_KEY",
                    "openai" => "OPENAI_API_KEY",
                    _ => "API_KEY",
                };
                bail!(
                    "Brak klucza API dla dostawcy '{}'. Ustaw go za pomocą 'audioticon providers set {} --key ...' lub zmiennej środowiskowej {}.",
                    p_clean,
                    p_clean,
                    env_name
                );
            }
        },
    };

    match p_clean.as_str() {
        "gemini" => {
            let selected_model = model.unwrap_or("gemini-2.5-flash");
            call_gemini_director(clean_text, selected_model, &resolved_key)
        }
        "openai" => {
            let selected_model = model.unwrap_or("gpt-4o-mini");
            call_openai_director(clean_text, selected_model, &resolved_key)
        }
        _ => bail!(
            "Nieznany dostawca AI do reżyserii: '{}'. Obsługiwani dostawcy: 'gemini', 'openai'.",
            provider
        ),
    }
}

/// Directs script via Google Gemini REST API
pub fn call_gemini_director(raw_text: &str, model: &str, api_key: &str) -> Result<String> {
    #[derive(Serialize)]
    struct TextPart {
        text: String,
    }

    #[derive(Serialize)]
    struct Content {
        role: String,
        parts: Vec<TextPart>,
    }

    #[derive(Serialize)]
    struct SystemInstruction {
        parts: Vec<TextPart>,
    }

    #[derive(Serialize)]
    struct GenerationConfig {
        temperature: f32,
    }

    #[derive(Serialize)]
    struct GeminiRequest {
        system_instruction: SystemInstruction,
        contents: Vec<Content>,
        #[serde(rename = "generationConfig")]
        generation_config: GenerationConfig,
    }

    let req_body = GeminiRequest {
        system_instruction: SystemInstruction {
            parts: vec![TextPart {
                text: AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.to_string(),
            }],
        },
        contents: vec![Content {
            role: "user".to_string(),
            parts: vec![TextPart {
                text: raw_text.to_string(),
            }],
        }],
        generation_config: GenerationConfig { temperature: 0.2 },
    };

    let req_json =
        serde_json::to_string(&req_body).context("Błąd serializacji zapytania do Google Gemini")?;

    // Create safe temporary curl config file (0600 permissions, masked API key)
    let tmp_cfg = std::env::temp_dir().join(format!(
        "gemini_cfg_{}_{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true).mode(0o600);
            let mut f = opts
                .open(&tmp_cfg)
                .with_context(|| format!("Nie można utworzyć pliku: {:?}", tmp_cfg))?;
            writeln!(f, "header = \"x-goog-api-key: {}\"", api_key.trim())?;
            writeln!(f, "header = \"Content-Type: application/json\"")?;
        }
        #[cfg(not(unix))]
        {
            let mut f = fs::File::create(&tmp_cfg)?;
            writeln!(f, "header = \"x-goog-api-key: {}\"", api_key.trim())?;
            writeln!(f, "header = \"Content-Type: application/json\"")?;
        }
    }

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );

    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "-K",
            tmp_cfg.to_str().unwrap_or(""),
            "-X",
            "POST",
            "--data-binary",
            &req_json,
            &url,
        ])
        .output();

    let _ = fs::remove_file(&tmp_cfg);
    let output = output.context("Nie udało się wywołać curl dla Google Gemini API")?;

    if !output.status.success() {
        let err_stderr = String::from_utf8_lossy(&output.stderr);
        let err_stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "Błąd połączenia z Gemini API: {} {}",
            err_stderr.trim(),
            err_stdout.trim()
        );
    }

    parse_gemini_response(&output.stdout)
}

/// Parses Gemini JSON response
pub fn parse_gemini_response(response_bytes: &[u8]) -> Result<String> {
    #[derive(Deserialize)]
    struct Part {
        text: Option<String>,
    }

    #[derive(Deserialize)]
    struct CandidateContent {
        parts: Option<Vec<Part>>,
    }

    #[derive(Deserialize)]
    struct Candidate {
        content: Option<CandidateContent>,
    }

    #[derive(Deserialize)]
    struct GeminiErrorDetail {
        message: Option<String>,
    }

    #[derive(Deserialize)]
    struct GeminiResponse {
        candidates: Option<Vec<Candidate>>,
        error: Option<GeminiErrorDetail>,
    }

    let resp: GeminiResponse = serde_json::from_slice(response_bytes).with_context(|| {
        format!(
            "Nie można sparsować odpowiedzi z Gemini API: {}",
            String::from_utf8_lossy(response_bytes)
        )
    })?;

    if let Some(err) = resp.error {
        if let Some(msg) = err.message {
            bail!("Błąd z Google Gemini API: {}", msg);
        }
    }

    let candidates = resp
        .candidates
        .filter(|c| !c.is_empty())
        .context("Brak wygenerowanych kandydatów w odpowiedzi Gemini API")?;

    let parts = candidates[0]
        .content
        .as_ref()
        .and_then(|c| c.parts.as_ref())
        .context("Brak treści w odpowiedzi kandydata Gemini API")?;

    let full_text: String = parts
        .iter()
        .filter_map(|p| p.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

    if full_text.trim().is_empty() {
        bail!("Gemini API zwróciło pusty tekst reżyserii");
    }

    Ok(strip_markdown_code_blocks(&full_text))
}

/// Directs script via local Ollama server (OpenAI-compatible /v1/chat/completions)
pub fn call_ollama_director(raw_text: &str, model: &str) -> Result<String> {
    #[derive(Serialize)]
    struct Message {
        role: String,
        content: String,
    }

    #[derive(Serialize)]
    struct OllamaRequest {
        model: String,
        messages: Vec<Message>,
        temperature: f32,
    }

    let req_body = OllamaRequest {
        model: model.to_string(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.to_string(),
            },
            Message {
                role: "user".to_string(),
                content: raw_text.to_string(),
            },
        ],
        temperature: 0.2,
    };

    let req_json = serde_json::to_string(&req_body)
        .context("Błąd serializacji zapytania do lokalnego serwera Ollama")?;

    let url = "http://127.0.0.1:11434/v1/chat/completions";

    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            &req_json,
            url,
        ])
        .output();

    let output = output.context("Nie udało się połączyć z lokalnym serwerem Ollama (127.0.0.1:11434). Upewnij się, że usługa Ollama jest uruchomiona.")?;

    if !output.status.success() {
        let err_stderr = String::from_utf8_lossy(&output.stderr);
        let err_stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "Błąd połączenia z Ollama: {} {}",
            err_stderr.trim(),
            err_stdout.trim()
        );
    }

    parse_openai_response(&output.stdout)
}

/// Directs script via OpenAI REST API
pub fn call_openai_director(raw_text: &str, model: &str, api_key: &str) -> Result<String> {
    #[derive(Serialize)]
    struct Message {
        role: String,
        content: String,
    }

    #[derive(Serialize)]
    struct OpenAIRequest {
        model: String,
        messages: Vec<Message>,
        temperature: f32,
    }

    let req_body = OpenAIRequest {
        model: model.to_string(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.to_string(),
            },
            Message {
                role: "user".to_string(),
                content: raw_text.to_string(),
            },
        ],
        temperature: 0.2,
    };

    let req_json =
        serde_json::to_string(&req_body).context("Błąd serializacji zapytania do OpenAI API")?;

    let tmp_cfg = std::env::temp_dir().join(format!(
        "openai_cfg_{}_{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true).mode(0o600);
            let mut f = opts
                .open(&tmp_cfg)
                .with_context(|| format!("Nie można utworzyć pliku: {:?}", tmp_cfg))?;
            writeln!(f, "header = \"Authorization: Bearer {}\"", api_key.trim())?;
            writeln!(f, "header = \"Content-Type: application/json\"")?;
        }
        #[cfg(not(unix))]
        {
            let mut f = fs::File::create(&tmp_cfg)?;
            writeln!(f, "header = \"Authorization: Bearer {}\"", api_key.trim())?;
            writeln!(f, "header = \"Content-Type: application/json\"")?;
        }
    }

    let url = "https://api.openai.com/v1/chat/completions";

    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "-K",
            tmp_cfg.to_str().unwrap_or(""),
            "-X",
            "POST",
            "--data-binary",
            &req_json,
            url,
        ])
        .output();

    let _ = fs::remove_file(&tmp_cfg);
    let output = output.context("Nie udało się wywołać curl dla OpenAI API")?;

    if !output.status.success() {
        let err_stderr = String::from_utf8_lossy(&output.stderr);
        let err_stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "Błąd połączenia z OpenAI API: {} {}",
            err_stderr.trim(),
            err_stdout.trim()
        );
    }

    parse_openai_response(&output.stdout)
}

/// Parses OpenAI JSON response
pub fn parse_openai_response(response_bytes: &[u8]) -> Result<String> {
    #[derive(Deserialize)]
    struct Message {
        content: Option<String>,
    }

    #[derive(Deserialize)]
    struct Choice {
        message: Option<Message>,
    }

    #[derive(Deserialize)]
    struct OpenAIErrorDetail {
        message: Option<String>,
    }

    #[derive(Deserialize)]
    struct OpenAIResponse {
        choices: Option<Vec<Choice>>,
        error: Option<OpenAIErrorDetail>,
    }

    let resp: OpenAIResponse = serde_json::from_slice(response_bytes).with_context(|| {
        format!(
            "Nie można sparsować odpowiedzi z OpenAI API: {}",
            String::from_utf8_lossy(response_bytes)
        )
    })?;

    if let Some(err) = resp.error {
        if let Some(msg) = err.message {
            bail!("Błąd z OpenAI API: {}", msg);
        }
    }

    let choices = resp
        .choices
        .filter(|c| !c.is_empty())
        .context("Brak wyborów (choices) w odpowiedzi OpenAI API")?;

    let content = choices[0]
        .message
        .as_ref()
        .and_then(|m| m.content.as_deref())
        .context("Brak treści wiadomości w odpowiedzi OpenAI API")?;

    if content.trim().is_empty() {
        bail!("OpenAI API zwróciło pusty tekst reżyserii");
    }

    Ok(strip_markdown_code_blocks(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_markdown_code_blocks() {
        let fenced = "```\n👨 Kwestia dialogowa.\n```";
        assert_eq!(strip_markdown_code_blocks(fenced), "👨 Kwestia dialogowa.");

        let fenced_text = "```text\n👨 💥 Uciekaj!\n```";
        assert_eq!(strip_markdown_code_blocks(fenced_text), "👨 💥 Uciekaj!");

        let plain = "👨 🐢 Mroczny las.";
        assert_eq!(strip_markdown_code_blocks(plain), "👨 🐢 Mroczny las.");
    }

    #[test]
    fn test_system_prompt_integrity() {
        assert!(AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.contains("AUDIOTICON"));
        assert!(AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.contains("👨"));
        assert!(AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.contains("👩"));
        assert!(AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.contains("🤫"));
        assert!(AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.contains("💥"));
        assert!(AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.contains("📱"));
        assert!(AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.contains("🛑"));
        assert!(AUDIOTICON_DIRECTOR_SYSTEM_PROMPT.contains("⏸️"));
    }

    #[test]
    fn test_parse_gemini_response_valid() {
        let mock_json = r#"{
            "candidates": [
                {
                    "content": {
                        "parts": [
                            { "text": "👨 🐢 Gdzie jesteś? 📖 — zapytał." }
                        ]
                    }
                }
            ]
        }"#;
        let parsed = parse_gemini_response(mock_json.as_bytes()).unwrap();
        assert_eq!(parsed, "👨 🐢 Gdzie jesteś? 📖 — zapytał.");
    }

    #[test]
    fn test_parse_gemini_response_error() {
        let mock_err = r#"{
            "error": {
                "message": "API key not valid"
            }
        }"#;
        let res = parse_gemini_response(mock_err.as_bytes());
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("API key not valid"));
    }

    #[test]
    fn test_parse_openai_response_valid() {
        let mock_json = r#"{
            "choices": [
                {
                    "message": {
                        "content": "```markdown\n👩 📱 Halo, słyszysz mnie?\n```"
                    }
                }
            ]
        }"#;
        let parsed = parse_openai_response(mock_json.as_bytes()).unwrap();
        assert_eq!(parsed, "👩 📱 Halo, słyszysz mnie?");
    }

    #[test]
    fn test_parse_openai_response_error() {
        let mock_err = r#"{
            "error": {
                "message": "Incorrect API key provided"
            }
        }"#;
        let res = parse_openai_response(mock_err.as_bytes());
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Incorrect API key provided"));
    }
}
