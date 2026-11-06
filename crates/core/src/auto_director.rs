use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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

// ============================================================================
// BIELIK ANNOTATION-ONLY AUDIOBOOK DIRECTOR & FAIL-CLOSED VALIDATOR (RUST PORT)
// ============================================================================
//
// Core safety invariants:
// 1. Source text is owned 100% by the Rust adapter (`source_text_owned_by_adapter = true`).
//    The AI model returns ONLY segment annotations (`speaker`, `emotion`, `pace`, `pitch`,
//    `pause_before_ms`, `pause_after_ms`, `sound_cues`) indexed by `segment_id` — never `text`.
// 2. Every proposal starts as `Proposed` (`accepted_for_render = false`) and is verified by
//    `validate_proposal_envelope`: SHA-256 source hash, 100% ordered source text coverage,
//    narration vs dialogue attribution checks, allowed enums/presets, and mandatory
//    `author_questions` whenever `speaker == "unknown"`.
// 3. Only explicit manual user approval (`approve_proposal_envelope`) transitions
//    `Proposed -> Approved` after re-validating against the current text.

pub const ANNOTATION_ONLY_SYSTEM_PROMPT: &str = r#"Jesteś reżyserem audiobooka w trybie annotation_only.
Nie zwracaj pola text i nie powtarzaj żadnego tekstu źródłowego. Tekst jest własnością adaptera.
Zwróć wyłącznie obiekt JSON o kluczach: annotations, author_questions, warnings, status.
Każdy element tablicy annotations musi mieć dokładnie pola:
- segment_id (np. "segment_001")
- speaker ("narrator", "Anna", "Marek", "Jan" lub "unknown")
- emotion ("neutral", "calm", "sad", "happy", "fear", "anger", "tension", "whisper", "whisper_action", "whisper_romantic", "shout", "call", "chant")
- pace ("very_slow", "slow", "normal", "fast", "very_fast")
- pitch ("low", "normal", "high")
- pause_before_ms (liczba całkowita 0..10000)
- pause_after_ms (liczba całkowita 0..10000)
- sound_cues (tablica obiektów {"type":"dsp_preset","name":"..."} lub pusta tablica [])
Dla speaker="unknown" dodaj pytanie dla autora w tablicy author_questions (np. "segment_002: Kto wypowiada ten fragment?").
Status musi mieć zawsze wartość "proposed"."#;

pub const ALLOWED_EMOTIONS: &[&str] = &[
    "neutral",
    "calm",
    "sad",
    "happy",
    "fear",
    "anger",
    "tension",
    "whisper",
    "whisper_action",
    "whisper_romantic",
    "shout",
    "call",
    "chant",
];

pub const ALLOWED_PACES: &[&str] = &["very_slow", "slow", "normal", "fast", "very_fast"];
pub const ALLOWED_PITCHES: &[&str] = &["low", "normal", "high"];
pub const ALLOWED_DSP_PRESETS: &[&str] = &[
    "szept:narracyjny",
    "szept:akcja",
    "szept:ucho",
    "szept:mikrofon",
    "rozkaz",
    "wolanie",
    "spiew",
    "radio:wojskowe",
    "cathedral:kaplica",
    "whisper:intymny",
    "robot:ciezki",
    "phone",
    "radio",
    "cathedral",
    "dungeon",
    "thought",
    "robot",
];

/// Immutable source segment produced by the deterministic segmenter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceSegment {
    pub segment_id: String,
    pub index: usize,
    pub text: String,
    pub expected_speaker_constraint: String,
}

/// DSP / Style Transfer cue attached to a segment annotation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoundCue {
    #[serde(rename = "type")]
    pub cue_type: String,
    pub name: String,
}

/// Annotation-only metadata proposed by the model (no `text` field!).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentAnnotation {
    pub segment_id: String,
    pub speaker: String,
    pub emotion: String,
    pub pace: String,
    #[serde(default = "default_normal_str")]
    pub pitch: String,
    #[serde(default)]
    pub pause_before_ms: u32,
    pub pause_after_ms: u32,
    #[serde(default)]
    pub sound_cues: Vec<SoundCue>,
}

fn default_normal_str() -> String {
    "normal".to_string()
}

/// Raw JSON response expected from the annotation-only LLM proposer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationOnlyResponse {
    pub annotations: Vec<SegmentAnnotation>,
    #[serde(default)]
    pub author_questions: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default = "default_proposed_str")]
    pub status: String,
}

fn default_proposed_str() -> String {
    "proposed".to_string()
}

/// Merged segment combining immutable adapter-owned `text` with validated performance metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectorProposalSegment {
    pub index: usize,
    pub segment_id: String,
    pub speaker: String,
    pub text: String,
    pub voice: String,
    pub emotion: String,
    pub pace: String,
    pub pitch: String,
    pub pause_before_ms: u32,
    pub pause_after_ms: u32,
    pub sound_cues: Vec<SoundCue>,
}

/// Fail-closed validation report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Proposal review status (`proposed` -> `approved` | `rejected`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Proposed,
    Approved,
    Rejected,
}

/// Audit log entry for manual review transitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionRecord {
    pub from: String,
    pub to: String,
    pub reviewer_note: String,
}

/// Complete auditable proposal envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectorProposalEnvelope {
    pub proposal_id: String,
    pub profile: String,
    pub status: ProposalStatus,
    pub project_id: String,
    pub scene_id: String,
    pub source_text_hash: String,
    pub segments: Vec<DirectorProposalSegment>,
    pub open_questions: Vec<String>,
    pub warnings: Vec<String>,
    pub validation: ValidationReport,
    pub reviewer_note: Option<String>,
    pub decision_history: Vec<DecisionRecord>,
    pub accepted_for_render: bool,
    pub annotation_only: bool,
    pub source_text_owned_by_adapter: bool,
}

/// Computes canonical SHA-256 hex digest of the source text.
pub fn compute_source_text_hash(source_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Determines conservative speaker constraint from Polish punctuation & attribution verbs.
/// Never guesses from narrative context; returns `"narrator"`, a matched speaker name, or `"unknown"`.
pub fn expected_speaker_constraint(text: &str) -> String {
    let stripped = text.trim();
    let verbs = r"(?:powiedziała|powiedział|zapytała|zapytał|szepnęła|szepnął|wyszeptała|wyszeptał|krzyknęła|krzyknął|zawołała|zawołał|odpowiedziała|odpowiedział|warknął|warknęła|rzekł|rzekła)";

    let leading_re =
        Regex::new(&format!(r"^([A-ZĄĆĘŁŃÓŚŹŻ][a-ząćęłńóśźż]+)\s+{}\b", verbs)).unwrap();
    if let Some(caps) = leading_re.captures(stripped) {
        return caps[1].to_string();
    }

    let is_dialogue = stripped.starts_with('—')
        || stripped.starts_with('–')
        || stripped.starts_with('„')
        || stripped.starts_with('"')
        || stripped.starts_with("- ");
    if !is_dialogue {
        return "narrator".to_string();
    }

    let name_before_verb =
        Regex::new(&format!(r"\b([A-ZĄĆĘŁŃÓŚŹŻ][a-ząćęłńóśźż]+)\s+{}\b", verbs)).unwrap();
    if let Some(caps) = name_before_verb.captures(stripped) {
        return caps[1].to_string();
    }

    let verb_before_name =
        Regex::new(&format!(r"{}\s+(?:cicho\s+|głośno\s+)?([A-ZĄĆĘŁŃÓŚŹŻ][a-ząćęłńóśźż]+)\b", verbs))
            .unwrap();
    if let Some(caps) = verb_before_name.captures(stripped) {
        return caps[1].to_string();
    }

    "unknown".to_string()
}

/// Splits source text into deterministic paragraphs and narration/dialogue sub-segments
/// without altering a single word or punctuation mark.
pub fn segment_source_text(source_text: &str) -> Vec<SourceSegment> {
    let para_re = Regex::new(r"\n\s*\n+").unwrap();
    let mut parts: Vec<String> = Vec::new();

    for para in para_re.split(source_text) {
        let trimmed = para.trim();
        if trimmed.is_empty() {
            continue;
        }
        // If paragraph has multiple lines where a line starts with dialogue dash, split by line
        let lines: Vec<&str> = trimmed.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if lines.len() > 1
            && lines.iter().any(|l| {
                l.starts_with('—') || l.starts_with('–') || l.starts_with('„') || l.starts_with('"')
            })
        {
            for l in lines {
                parts.push(l.to_string());
            }
        } else {
            parts.push(trimmed.to_string());
        }
    }

    parts
        .into_iter()
        .enumerate()
        .map(|(idx, text)| {
            let constraint = expected_speaker_constraint(&text);
            SourceSegment {
                segment_id: format!("segment_{:03}", idx + 1),
                index: idx,
                text,
                expected_speaker_constraint: constraint,
            }
        })
        .collect()
}

/// Verifies that the sequence of segments covers 100% of `source_text` in exact order
/// with zero added, removed, or modified words.
pub fn verify_ordered_source_coverage(
    source_text: &str,
    segments: &[DirectorProposalSegment],
) -> bool {
    if segments.is_empty() {
        return false;
    }
    let joined_segments = segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    normalize_ws(source_text) == normalize_ws(&joined_segments)
}

/// Infers sensible Polish emotion, pace, and style cue for the deterministic baseline proposer.
fn infer_heuristic_expressive_cues(text: &str) -> (String, String, Vec<SoundCue>) {
    let lower = text.to_lowercase();
    let mut cues = Vec::new();
    let emotion = if lower.contains("szept")
        || lower.contains("wyszepta")
        || lower.contains("szepn")
        || lower.contains("cicho")
    {
        if lower.contains("ucho") || lower.contains("kocham") || lower.contains("intymn") {
            cues.push(SoundCue {
                cue_type: "dsp_preset".to_string(),
                name: "szept:ucho".to_string(),
            });
            "whisper_romantic".to_string()
        } else if lower.contains("stój")
            || lower.contains("ruchu")
            || lower.contains("kryj")
            || lower.contains("misj")
        {
            cues.push(SoundCue {
                cue_type: "dsp_preset".to_string(),
                name: "szept:akcja".to_string(),
            });
            "whisper_action".to_string()
        } else {
            cues.push(SoundCue {
                cue_type: "dsp_preset".to_string(),
                name: "szept:narracyjny".to_string(),
            });
            "whisper".to_string()
        }
    } else if lower.contains("krzyk")
        || lower.contains("wrzasn")
        || lower.contains("rozkaz")
        || text.contains('!')
    {
        cues.push(SoundCue {
            cue_type: "dsp_preset".to_string(),
            name: "rozkaz".to_string(),
        });
        "shout".to_string()
    } else if lower.contains("zawoła") || lower.contains("oddali") || lower.contains("echo") {
        cues.push(SoundCue {
            cue_type: "dsp_preset".to_string(),
            name: "wolanie".to_string(),
        });
        "call".to_string()
    } else if lower.contains("śpiew") || lower.contains("zanuci") || lower.contains("pieśń") {
        cues.push(SoundCue {
            cue_type: "dsp_preset".to_string(),
            name: "spiew".to_string(),
        });
        "chant".to_string()
    } else {
        "neutral".to_string()
    };

    if lower.contains("słuchawk") || lower.contains("telefon") {
        cues.push(SoundCue {
            cue_type: "dsp_preset".to_string(),
            name: "phone".to_string(),
        });
    } else if lower.contains("radio") || lower.contains("krótkofal") {
        cues.push(SoundCue {
            cue_type: "dsp_preset".to_string(),
            name: "radio:wojskowe".to_string(),
        });
    } else if lower.contains("krypt") || lower.contains("katedr") || lower.contains("loch") {
        cues.push(SoundCue {
            cue_type: "dsp_preset".to_string(),
            name: "cathedral:kaplica".to_string(),
        });
    }

    let pace = if emotion == "whisper_romantic" || emotion == "chant" {
        "slow".to_string()
    } else if emotion == "whisper_action" || emotion == "shout" {
        "fast".to_string()
    } else {
        "normal".to_string()
    };

    (emotion, pace, cues)
}

fn default_voice_for_speaker(speaker: &str) -> String {
    let lower = speaker.to_lowercase();
    if lower == "anna"
        || lower == "ewa"
        || lower == "zofia"
        || lower == "kobieta"
        || lower == "female"
        || lower.ends_with('a') && lower != "narrator"
    {
        "pl-PL-ZofiaNeural".to_string()
    } else if lower == "narrator" || lower == "unknown" {
        "pl_PL-darkman-medium".to_string()
    } else {
        "pl-PL-MarekNeural".to_string()
    }
}

/// Fail-closed validator checking SHA-256 hash, 100% ordered source text coverage,
/// speaker attribution constraints, allowed enums/presets, and mandatory `open_questions` for `"unknown"`.
pub fn validate_proposal_envelope(
    envelope: &DirectorProposalEnvelope,
    source_text: &str,
) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if envelope.source_text_hash != compute_source_text_hash(source_text) {
        errors.push("source_hash_mismatch".to_string());
    }
    if envelope.segments.is_empty() {
        errors.push("segments_missing".to_string());
    }
    if !verify_ordered_source_coverage(source_text, &envelope.segments) {
        errors.push("segment_text_does_not_cover_source".to_string());
    }

    for (expected_idx, seg) in envelope.segments.iter().enumerate() {
        if seg.index != expected_idx {
            errors.push(format!("segment_{}:invalid_index", expected_idx));
        }
        if seg.speaker.trim().is_empty() {
            errors.push(format!("segment_{}:empty_speaker", expected_idx));
        }
        if !ALLOWED_EMOTIONS.contains(&seg.emotion.as_str()) {
            errors.push(format!(
                "segment_{}:invalid_emotion:{}",
                expected_idx, seg.emotion
            ));
        }
        if !ALLOWED_PACES.contains(&seg.pace.as_str()) {
            errors.push(format!(
                "segment_{}:invalid_pace:{}",
                expected_idx, seg.pace
            ));
        }
        if !ALLOWED_PITCHES.contains(&seg.pitch.as_str()) {
            errors.push(format!(
                "segment_{}:invalid_pitch:{}",
                expected_idx, seg.pitch
            ));
        }
        if seg.pause_before_ms > 10_000 {
            errors.push(format!("segment_{}:invalid_pause_before_ms", expected_idx));
        }
        if seg.pause_after_ms > 10_000 {
            errors.push(format!("segment_{}:invalid_pause_after_ms", expected_idx));
        }
        for cue in &seg.sound_cues {
            if cue.cue_type != "dsp_preset"
                || (!ALLOWED_DSP_PRESETS.contains(&cue.name.as_str())
                    && !cue.name.starts_with("style:"))
            {
                errors.push(format!(
                    "segment_{}:invalid_dsp_preset:{}",
                    expected_idx, cue.name
                ));
            }
        }

        // Check narration vs dialogue attribution constraint
        let expected_role = expected_speaker_constraint(&seg.text);
        if expected_role == "narrator" && seg.speaker != "narrator" {
            errors.push(format!("narration_misattributed:{}", seg.segment_id));
        }

        if seg.speaker == "unknown" {
            let has_question = envelope.open_questions.iter().any(|q| {
                q.contains(&seg.segment_id) || q.contains(&format!("segment_{}", expected_idx))
            });
            if !has_question {
                errors.push(format!("missing_author_question:{}", seg.segment_id));
            } else {
                warnings.push(format!(
                    "{}:speaker_needs_author_decision",
                    seg.segment_id
                ));
            }
        }
    }

    ValidationReport {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Builds a deterministic baseline `annotation-only` proposal envelope without calling an external LLM,
/// or merges an `AnnotationOnlyResponse` from Bielik / Gemini / OpenAI onto the immutable source segments.
pub fn build_annotation_only_proposal(
    source_text: &str,
    profile: &str,
    llm_response: Option<AnnotationOnlyResponse>,
) -> DirectorProposalEnvelope {
    let source_segments = segment_source_text(source_text);
    let mut open_questions = Vec::new();
    let mut warnings = vec![
        "Tryb annotation-only: tekst źródłowy jest chroniony przez adapter Rust i wymaga akceptacji autora."
            .to_string(),
    ];

    let by_id: HashMap<String, SegmentAnnotation> = llm_response
        .as_ref()
        .map(|r| {
            for q in &r.author_questions {
                open_questions.push(q.clone());
            }
            for w in &r.warnings {
                warnings.push(w.clone());
            }
            r.annotations
                .iter()
                .map(|a| (a.segment_id.clone(), a.clone()))
                .collect()
        })
        .unwrap_or_default();

    let total_segs = source_segments.len();
    let mut merged_segments = Vec::with_capacity(total_segs);

    for src in &source_segments {
        let (speaker, emotion, pace, pitch, pause_before_ms, pause_after_ms, sound_cues) =
            if let Some(ann) = by_id.get(&src.segment_id) {
                // Enforce speaker safety: narration must stay narrator
                let safe_speaker = if src.expected_speaker_constraint == "narrator" {
                    "narrator".to_string()
                } else if ann.speaker.trim().is_empty() {
                    src.expected_speaker_constraint.clone()
                } else {
                    ann.speaker.trim().to_string()
                };
                let safe_emotion = if ALLOWED_EMOTIONS.contains(&ann.emotion.as_str()) {
                    ann.emotion.clone()
                } else {
                    "neutral".to_string()
                };
                let safe_pace = if ALLOWED_PACES.contains(&ann.pace.as_str()) {
                    ann.pace.clone()
                } else {
                    "normal".to_string()
                };
                let safe_pitch = if ALLOWED_PITCHES.contains(&ann.pitch.as_str()) {
                    ann.pitch.clone()
                } else {
                    "normal".to_string()
                };
                let safe_cues: Vec<SoundCue> = ann
                    .sound_cues
                    .iter()
                    .filter(|c| {
                        c.cue_type == "dsp_preset"
                            && (ALLOWED_DSP_PRESETS.contains(&c.name.as_str())
                                || c.name.starts_with("style:"))
                    })
                    .cloned()
                    .collect();
                (
                    safe_speaker,
                    safe_emotion,
                    safe_pace,
                    safe_pitch,
                    ann.pause_before_ms.min(10_000),
                    ann.pause_after_ms.min(10_000),
                    safe_cues,
                )
            } else {
                let (heur_emotion, heur_pace, heur_cues) = infer_heuristic_expressive_cues(&src.text);
                let pb = if src.index == 0 { 0 } else { 400 };
                let pa = if src.index + 1 == total_segs {
                    900
                } else {
                    400
                };
                (
                    src.expected_speaker_constraint.clone(),
                    heur_emotion,
                    heur_pace,
                    "normal".to_string(),
                    pb,
                    pa,
                    heur_cues,
                )
            };

        if speaker == "unknown"
            && !open_questions
                .iter()
                .any(|q| q.contains(&src.segment_id))
        {
            open_questions.push(format!(
                "{}: Kto wypowiada tę kwestię dialogową? (\"{}\")",
                src.segment_id,
                src.text.chars().take(48).collect::<String>()
            ));
        }

        merged_segments.push(DirectorProposalSegment {
            index: src.index,
            segment_id: src.segment_id.clone(),
            voice: default_voice_for_speaker(&speaker),
            speaker,
            text: src.text.clone(),
            emotion,
            pace,
            pitch,
            pause_before_ms,
            pause_after_ms,
            sound_cues,
        });
    }

    let hash = compute_source_text_hash(source_text);
    let mut envelope = DirectorProposalEnvelope {
        proposal_id: format!("proposal-{}", &hash[..12.min(hash.len())]),
        profile: profile.to_string(),
        status: ProposalStatus::Proposed,
        project_id: "audioticon-studio".to_string(),
        scene_id: "scene-001".to_string(),
        source_text_hash: hash,
        segments: merged_segments,
        open_questions,
        warnings,
        validation: ValidationReport {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        },
        reviewer_note: None,
        decision_history: Vec::new(),
        accepted_for_render: false,
        annotation_only: true,
        source_text_owned_by_adapter: true,
    };

    envelope.validation = validate_proposal_envelope(&envelope, source_text);
    envelope
}

/// Proposes an `annotation-only` direction envelope using Bielik (Ollama), Gemini, OpenAI,
/// or the deterministic Polish heuristic baseline. Never alters `source_text`.
pub fn propose_annotations_auto(
    source_text: &str,
    provider: &str,
    model: Option<&str>,
    api_key: Option<&str>,
) -> Result<DirectorProposalEnvelope> {
    let clean_text = source_text.trim();
    if clean_text.is_empty() {
        bail!("Tekst źródłowy dla reżysera annotation-only nie może być pusty");
    }

    let segments = segment_source_text(clean_text);
    let user_payload = serde_json::json!({
        "project_id": "audioticon-studio",
        "scene_id": "scene-001",
        "allowed_speakers": ["narrator", "Anna", "Marek", "Jan", "unknown"],
        "speaker_rule": "opis bez sygnału mowy = narrator; niejednoznaczny dialog = unknown + author_questions",
        "segments": segments,
    });
    let user_json = serde_json::to_string(&user_payload)?;
    let p_clean = provider.trim().to_lowercase();

    let raw_llm_json = match p_clean.as_str() {
        "bielik" | "ollama" => {
            let m = model.unwrap_or("bielik-4.5b-v3:q5");
            call_ollama_custom_prompt(ANNOTATION_ONLY_SYSTEM_PROMPT, &user_json, m).ok()
        }
        "gemini" => {
            if let Some(key) = api_key
                .map(|s| s.to_string())
                .or_else(|| crate::credentials::get_api_key("gemini"))
            {
                let m = model.unwrap_or("gemini-2.5-flash");
                call_gemini_custom_prompt(ANNOTATION_ONLY_SYSTEM_PROMPT, &user_json, m, &key).ok()
            } else {
                None
            }
        }
        "openai" => {
            if let Some(key) = api_key
                .map(|s| s.to_string())
                .or_else(|| crate::credentials::get_api_key("openai"))
            {
                let m = model.unwrap_or("gpt-4o-mini");
                call_openai_custom_prompt(ANNOTATION_ONLY_SYSTEM_PROMPT, &user_json, m, &key).ok()
            } else {
                None
            }
        }
        _ => None,
    };

    let parsed_resp = raw_llm_json.and_then(|raw| extract_annotation_json(&raw).ok());
    Ok(build_annotation_only_proposal(
        clean_text,
        &p_clean,
        parsed_resp,
    ))
}

fn extract_annotation_json(raw: &str) -> Result<AnnotationOnlyResponse> {
    let stripped = strip_markdown_code_blocks(raw);
    if let Ok(parsed) = serde_json::from_str::<AnnotationOnlyResponse>(&stripped) {
        return Ok(parsed);
    }
    if let (Some(start), Some(end)) = (stripped.find('{'), stripped.rfind('}')) {
        if end > start {
            let sub = &stripped[start..=end];
            if let Ok(parsed) = serde_json::from_str::<AnnotationOnlyResponse>(sub) {
                return Ok(parsed);
            }
        }
    }
    bail!("Nie udało się wyodrębnić kontraktu JSON adnotacji z odpowiedzi modelu")
}

fn call_ollama_custom_prompt(system_prompt: &str, user_text: &str, model: &str) -> Result<String> {
    let req_body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_text }
        ],
        "temperature": 0.0
    });
    let req_json = serde_json::to_string(&req_body)?;
    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "15",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            &req_json,
            "http://127.0.0.1:11434/v1/chat/completions",
        ])
        .output()?;
    if !output.status.success() {
        bail!("Ollama niedostępna");
    }
    parse_openai_response(&output.stdout)
}

fn call_gemini_custom_prompt(
    system_prompt: &str,
    user_text: &str,
    model: &str,
    api_key: &str,
) -> Result<String> {
    let req_body = serde_json::json!({
        "system_instruction": { "parts": [{ "text": system_prompt }] },
        "contents": [{ "role": "user", "parts": [{ "text": user_text }] }],
        "generationConfig": { "temperature": 0.0 }
    });
    let req_json = serde_json::to_string(&req_body)?;
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );
    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "20",
            "-H",
            &format!("x-goog-api-key: {}", api_key.trim()),
            "-H",
            "Content-Type: application/json",
            "-X",
            "POST",
            "--data-binary",
            &req_json,
            &url,
        ])
        .output()?;
    if !output.status.success() {
        bail!("Błąd Gemini API");
    }
    parse_gemini_response(&output.stdout)
}

fn call_openai_custom_prompt(
    system_prompt: &str,
    user_text: &str,
    model: &str,
    api_key: &str,
) -> Result<String> {
    let req_body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_text }
        ],
        "temperature": 0.0
    });
    let req_json = serde_json::to_string(&req_body)?;
    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "20",
            "-H",
            &format!("Authorization: Bearer {}", api_key.trim()),
            "-H",
            "Content-Type: application/json",
            "-X",
            "POST",
            "--data-binary",
            &req_json,
            "https://api.openai.com/v1/chat/completions",
        ])
        .output()?;
    if !output.status.success() {
        bail!("Błąd OpenAI API");
    }
    parse_openai_response(&output.stdout)
}

/// Approves a `Proposed` envelope after applying any author overrides for `"unknown"` speakers
/// and re-validating against `source_text`.
pub fn approve_proposal_envelope(
    envelope: &mut DirectorProposalEnvelope,
    source_text: &str,
    reviewer_note: &str,
    speaker_overrides: Option<&HashMap<String, String>>,
) -> Result<()> {
    if envelope.status != ProposalStatus::Proposed {
        bail!(
            "transition_not_allowed:{:?}->approved (propozycja została już wcześniej zamknięta)",
            envelope.status
        );
    }
    if reviewer_note.trim().is_empty() {
        bail!("reviewer_note_required: Podaj krótką notatkę akceptacji autora.");
    }

    if let Some(overrides) = speaker_overrides {
        for seg in &mut envelope.segments {
            if let Some(new_speaker) = overrides.get(&seg.segment_id) {
                let clean_sp = new_speaker.trim();
                if !clean_sp.is_empty() && clean_sp != "unknown" {
                    seg.speaker = clean_sp.to_string();
                    seg.voice = default_voice_for_speaker(clean_sp);
                }
            }
        }
        // Clear resolved questions
        envelope.open_questions.retain(|q| {
            envelope
                .segments
                .iter()
                .any(|s| s.speaker == "unknown" && q.contains(&s.segment_id))
        });
    }

    // Block approval if any unknown speakers remain unresolved
    if envelope.segments.iter().any(|s| s.speaker == "unknown") {
        bail!(
            "unresolved_unknown_speaker: Przed zatwierdzeniem wskaż mówcę dla kwestii oznaczonych jako 'unknown'."
        );
    }

    let check = validate_proposal_envelope(envelope, source_text);
    envelope.validation = check.clone();
    if !check.valid {
        bail!("approval_blocked: {}", check.errors.join(", "));
    }

    envelope.status = ProposalStatus::Approved;
    envelope.reviewer_note = Some(reviewer_note.trim().to_string());
    envelope.decision_history.push(DecisionRecord {
        from: "proposed".to_string(),
        to: "approved".to_string(),
        reviewer_note: reviewer_note.trim().to_string(),
    });
    Ok(())
}

/// Rejects a `Proposed` envelope and records the rejection reason.
pub fn reject_proposal_envelope(
    envelope: &mut DirectorProposalEnvelope,
    reviewer_note: &str,
) -> Result<()> {
    if envelope.status != ProposalStatus::Proposed {
        bail!(
            "transition_not_allowed:{:?}->rejected",
            envelope.status
        );
    }
    if reviewer_note.trim().is_empty() {
        bail!("reviewer_note_required: Podaj powód odrzucenia propozycji.");
    }
    envelope.status = ProposalStatus::Rejected;
    envelope.reviewer_note = Some(reviewer_note.trim().to_string());
    envelope.accepted_for_render = false;
    envelope.decision_history.push(DecisionRecord {
        from: "proposed".to_string(),
        to: "rejected".to_string(),
        reviewer_note: reviewer_note.trim().to_string(),
    });
    Ok(())
}

/// Exports a validated proposal envelope into Audioticon's emoji-directed script format,
/// mapping emotions, pauses, speakers, and Disentangled Style Transfer cues onto the untouched text.
pub fn export_proposal_to_audioticon_script(
    envelope: &DirectorProposalEnvelope,
    source_text: &str,
) -> Result<String> {
    let check = validate_proposal_envelope(envelope, source_text);
    if !check.valid {
        bail!(
            "Nieprawidłowa propozycja reżyserska: {}",
            check.errors.join(", ")
        );
    }

    let mut lines = Vec::with_capacity(envelope.segments.len());
    for seg in &envelope.segments {
        let mut markers = Vec::new();

        // Speaker icon
        let sp_lower = seg.speaker.to_lowercase();
        if sp_lower == "narrator" {
            markers.push("📖".to_string());
        } else if sp_lower == "anna"
            || sp_lower == "ewa"
            || sp_lower == "zofia"
            || sp_lower == "kobieta"
            || sp_lower.ends_with('a')
        {
            markers.push("👩".to_string());
        } else {
            markers.push("👨".to_string());
        }

        // Pre-pause
        if seg.pause_before_ms >= 700 {
            markers.push("🛑".to_string());
        } else if seg.pause_before_ms >= 300 {
            markers.push("⏸️".to_string());
        }

        // Pace
        match seg.pace.as_str() {
            "very_slow" | "slow" => markers.push("🐢".to_string()),
            "fast" | "very_fast" => markers.push("🐇".to_string()),
            _ => {}
        }

        // Sound cues / Disentangled Style Transfer presets
        let mut has_style_cue = false;
        for cue in &seg.sound_cues {
            match cue.name.as_str() {
                "szept:ucho" | "whisper:intymny" => {
                    markers.push("💋".to_string());
                    has_style_cue = true;
                }
                "szept:akcja" => {
                    markers.push("🪖".to_string());
                    has_style_cue = true;
                }
                "szept:narracyjny" => {
                    markers.push("🤫".to_string());
                    has_style_cue = true;
                }
                "rozkaz" => {
                    markers.push("[rozkaz]".to_string());
                    has_style_cue = true;
                }
                "wolanie" => {
                    markers.push("[wolanie]".to_string());
                    has_style_cue = true;
                }
                "spiew" => {
                    markers.push("🎵".to_string());
                    has_style_cue = true;
                }
                "phone" => markers.push("📱".to_string()),
                "radio" | "radio:wojskowe" => markers.push("📻".to_string()),
                "cathedral" | "cathedral:kaplica" => markers.push("🏛️".to_string()),
                "dungeon" => markers.push("🏰".to_string()),
                "robot" | "robot:ciezki" => markers.push("🤖".to_string()),
                other if other.starts_with("style:") => {
                    markers.push(format!("[{}]", other));
                    has_style_cue = true;
                }
                _ => {}
            }
        }

        if !has_style_cue {
            match seg.emotion.as_str() {
                "whisper" => markers.push("🤫".to_string()),
                "whisper_action" => markers.push("🪖".to_string()),
                "whisper_romantic" => markers.push("💋".to_string()),
                "shout" => markers.push("[rozkaz]".to_string()),
                "call" => markers.push("[wolanie]".to_string()),
                "chant" => markers.push("🎵".to_string()),
                "tension" | "anger" => markers.push("📈".to_string()),
                "fear" | "sad" => markers.push("📉".to_string()),
                _ => {}
            }
        }

        let prefix = markers.join(" ");
        let mut line = if prefix.is_empty() {
            seg.text.clone()
        } else {
            format!("{} {}", prefix, seg.text)
        };

        if seg.pause_after_ms >= 700 {
            line.push_str(" 🛑");
        } else if seg.pause_after_ms >= 300 {
            line.push_str(" ⏸️");
        }
        lines.push(line);
    }

    Ok(lines.join("\n\n"))
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

    #[test]
    fn test_annotation_only_pipeline_and_fail_closed_validator() {
        let source = "W ciemnym zaułku deszcz bębnił o dach.\n— Nie ruszaj się! — wyszeptała Anna przez słuchawkę.\n— Kto tam jest?";
        let mut env = build_annotation_only_proposal(source, "bielik", None);

        assert_eq!(env.status, ProposalStatus::Proposed);
        assert!(env.annotation_only);
        assert!(env.source_text_owned_by_adapter);
        assert!(env.validation.valid);
        assert_eq!(env.segments.len(), 3);
        assert_eq!(env.segments[0].speaker, "narrator");
        assert_eq!(env.segments[1].speaker, "Anna");
        assert_eq!(env.segments[2].speaker, "unknown");
        assert!(!env.open_questions.is_empty());

        // Approval must fail while segment_003 speaker is still "unknown"
        let err_unknown = approve_proposal_envelope(&mut env, source, "Akceptuję", None);
        assert!(err_unknown.is_err());

        // Approval must fail if source text was modified (SHA-256 mismatch)
        let mut overrides = HashMap::new();
        overrides.insert("segment_003".to_string(), "Marek".to_string());
        let err_tampered =
            approve_proposal_envelope(&mut env, "Zmieniony tekst.", "Akceptuję", Some(&overrides));
        assert!(err_tampered.is_err());

        // Valid approval with resolved unknown speaker and untouched source text
        approve_proposal_envelope(&mut env, source, "Zatwierdzono przez autora", Some(&overrides))
            .unwrap();
        assert_eq!(env.status, ProposalStatus::Approved);
        assert!(env.open_questions.is_empty());

        let exported = export_proposal_to_audioticon_script(&env, source).unwrap();
        assert!(exported.contains("📖"));
        assert!(exported.contains("👩"));
        assert!(exported.contains("👨"));
        assert!(exported.contains("📱"));
        assert!(exported.contains("🤫"));
    }
}
