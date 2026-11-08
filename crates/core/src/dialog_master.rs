//! Multi-Voice Dialog Master and Automatic Character Casting Engine (`dialog_master.rs`)
//!
//! Provides automated dialogue parsing, character identity extraction,
//! gender/archetype heuristics, and voice assignment (Auto-Casting) for audiobooks
//! and audio dramas.
//!
//! Supported dialogue formats:
//! 1. Dramatic script prefix format:
//!    `Jan: Dzień dobry, pani Anno.`
//!    `Anna: Co pana sprowadza?`
//! 2. Novel quotation / em-dash format with inquit clauses:
//!    `— Nie możemy dłużej czekać — powiedziała Anna.`
//!    `— Zgadzam się — odparł Jan stanowczo.`
//! 3. English / international quotation marks:
//!    `"I don't believe you," said Mary.`
//!    `"It is true," replied John.`

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CharacterProfile {
    pub name: String,
    pub suggested_gender: String, // "female", "male", "narrator"
    pub assigned_voice: String,
    pub line_count: usize,
    pub word_count: usize,
    pub sample_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DialogAnalysisResult {
    pub characters: Vec<CharacterProfile>,
    pub total_dialogue_lines: usize,
    pub total_narrator_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyCastingRequest {
    pub text: String,
    pub character_voice_map: HashMap<String, String>,
}

static DRAMATIC_PREFIX_RE: OnceLock<Regex> = OnceLock::new();
static INQUIT_POLISH_RE: OnceLock<Regex> = OnceLock::new();
static INQUIT_ENGLISH_RE: OnceLock<Regex> = OnceLock::new();

fn get_dramatic_prefix_re() -> &'static Regex {
    DRAMATIC_PREFIX_RE.get_or_init(|| {
        Regex::new(r"^([A-ZĄĆĘŁŃÓŚŹŻ][a-ząćęłńóśźżA-Z0-9_\- ]{1,25}):\s*(.*)$").unwrap()
    })
}

fn get_inquit_polish_re() -> &'static Regex {
    INQUIT_POLISH_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:—|-)\s*(?:powiedział|powiedziała|rzekł|rzekła|odparł|odparła|zapytał|zapytała|krzyknął|krzyknęła|szepnął|szepnęła|zawołał|zawołała|stwierdził|stwierdziła|dodał|dodała|westchnął|westchnęła|mruknął|mruknęła|bąknął|bąknęła)\s+([A-ZĄĆĘŁŃÓŚŹŻ][a-ząćęłńóśźż]+)").unwrap()
    })
}

fn get_inquit_english_re() -> &'static Regex {
    INQUIT_ENGLISH_RE.get_or_init(|| {
        Regex::new(r#"(?i)["”]\s*(?:said|asked|replied|shouted|whispered|screamed|added|sighed)\s+([A-Z][a-z]+)"#).unwrap()
    })
}

/// Infers gender ("female", "male", or "narrator") from character name and language conventions.
pub fn infer_character_gender(name: &str) -> &'static str {
    let lower = name.trim().to_lowercase();
    if lower == "narrator" || lower == "lektor" || lower == "narracja" || lower == "stage" {
        return "narrator";
    }

    // Female keywords
    if lower.contains("kobieta")
        || lower.contains("matka")
        || lower.contains("mama")
        || lower.contains("córka")
        || lower.contains("dziewczyna")
        || lower.contains("pani")
        || lower.contains("królowa")
        || lower.contains("księżniczka")
        || lower.contains("babcia")
        || lower.contains("ciotka")
        || lower.contains("woman")
        || lower.contains("mother")
        || lower.contains("queen")
        || lower.contains("girl")
        || lower.contains("lady")
    {
        return "female";
    }

    // Common international / English female names without 'a' ending
    let common_female_names = [
        "mary",
        "sarah",
        "bella",
        "emma",
        "alice",
        "lucy",
        "helen",
        "grace",
        "kate",
        "chloe",
        "elizabeth",
        "claire",
        "jane",
        "jessica",
        "rachel",
        "amy",
        "zoe",
        "dorothy",
        "lily",
    ];
    if common_female_names.contains(&lower.as_str()) {
        return "female";
    }

    // Male keywords
    if lower.contains("mężczyzna")
        || lower.contains("ojciec")
        || lower.contains("tata")
        || lower.contains("syn")
        || lower.contains("chłopiec")
        || lower.contains("pan")
        || lower.contains("król")
        || lower.contains("książę")
        || lower.contains("dziadek")
        || lower.contains("stryj")
        || lower.contains("wuj")
        || lower.contains("strażnik")
        || lower.contains("rycerz")
        || lower.contains("man")
        || lower.contains("father")
        || lower.contains("king")
        || lower.contains("boy")
        || lower.contains("guard")
    {
        return "male";
    }

    // Polish female names mostly end with 'a' (exceptions like Kuba, Barnaba, Kosma, Bonawentura are male)
    let male_a_exceptions = ["kuba", "barnaba", "kosma", "bonawentura", "jarema"];
    if male_a_exceptions.contains(&lower.as_str()) {
        return "male";
    }

    if lower.ends_with('a') {
        "female"
    } else {
        "male"
    }
}

/// Suggests a default voice for a character based on their gender and language.
pub fn suggest_default_voice(gender: &str, lang: &str) -> &'static str {
    if lang.starts_with("en") {
        match gender {
            "female" => "af_bella",
            "male" => "am_adam",
            _ => "bm_george",
        }
    } else {
        match gender {
            "female" => "pl_PL-gosia-medium",
            "male" => "pl_PL-darkman-medium",
            _ => "pl_PL-mc_speech-medium",
        }
    }
}

/// Analyzes dialogue structure and returns all extracted characters with dialogue metrics.
pub fn analyze_dialogue(text: &str, lang: &str) -> DialogAnalysisResult {
    let mut stats: BTreeMap<String, (usize, usize, Vec<String>)> = BTreeMap::new();
    let mut total_dialogue = 0;
    let mut total_narrator = 0;

    let dramatic_re = get_dramatic_prefix_re();
    let inquit_pl_re = get_inquit_polish_re();
    let inquit_en_re = get_inquit_english_re();

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 1. Check dramatic prefix "Jan: ..."
        if let Some(caps) = dramatic_re.captures(trimmed) {
            let char_name = caps.get(1).unwrap().as_str().trim().to_string();
            let speech = caps.get(2).unwrap().as_str().trim().to_string();
            let words = speech.split_whitespace().count();

            let lower = char_name.to_lowercase();
            if lower == "narrator" || lower == "lektor" || lower == "narracja" {
                total_narrator += 1;
            } else {
                total_dialogue += 1;
            }
            let entry = stats.entry(char_name).or_insert_with(|| (0, 0, Vec::new()));
            entry.0 += 1;
            entry.1 += words;
            if entry.2.len() < 3 && !speech.is_empty() {
                entry.2.push(speech);
            }
            continue;
        }

        // 2. Check novel inquit Polish: "— ... — powiedział Jan"
        if let Some(caps) = inquit_pl_re.captures(trimmed) {
            let char_name = caps.get(1).unwrap().as_str().trim().to_string();
            let words = trimmed.split_whitespace().count();

            let lower = char_name.to_lowercase();
            if lower == "narrator" || lower == "lektor" || lower == "narracja" {
                total_narrator += 1;
            } else {
                total_dialogue += 1;
            }
            let entry = stats.entry(char_name).or_insert_with(|| (0, 0, Vec::new()));
            entry.0 += 1;
            entry.1 += words;
            if entry.2.len() < 3 {
                entry.2.push(trimmed.to_string());
            }
            continue;
        }

        // 3. Check novel inquit English: `"..." said John`
        if let Some(caps) = inquit_en_re.captures(trimmed) {
            let char_name = caps.get(1).unwrap().as_str().trim().to_string();
            let words = trimmed.split_whitespace().count();

            let lower = char_name.to_lowercase();
            if lower == "narrator" || lower == "lektor" || lower == "narracja" {
                total_narrator += 1;
            } else {
                total_dialogue += 1;
            }
            let entry = stats.entry(char_name).or_insert_with(|| (0, 0, Vec::new()));
            entry.0 += 1;
            entry.1 += words;
            if entry.2.len() < 3 {
                entry.2.push(trimmed.to_string());
            }
            continue;
        }

        // 4. Otherwise counts as Narrator / stage direction
        total_narrator += 1;
        let words = trimmed.split_whitespace().count();
        let entry = stats
            .entry("Narrator".to_string())
            .or_insert_with(|| (0, 0, Vec::new()));
        entry.0 += 1;
        entry.1 += words;
        if entry.2.len() < 3 {
            entry.2.push(trimmed.to_string());
        }
    }

    let mut characters = Vec::new();
    for (name, (line_count, word_count, samples)) in stats {
        let gender = infer_character_gender(&name);
        let assigned_voice = suggest_default_voice(gender, lang).to_string();
        characters.push(CharacterProfile {
            name,
            suggested_gender: gender.to_string(),
            assigned_voice,
            line_count,
            word_count,
            sample_lines: samples,
        });
    }

    // Sort characters: Narrator first, then by line count descending
    characters.sort_by(|a, b| {
        if a.name == "Narrator" {
            std::cmp::Ordering::Less
        } else if b.name == "Narrator" {
            std::cmp::Ordering::Greater
        } else {
            b.line_count.cmp(&a.line_count)
        }
    });

    DialogAnalysisResult {
        characters,
        total_dialogue_lines: total_dialogue,
        total_narrator_lines: total_narrator,
    }
}

/// Applies character-to-voice casting map to the manuscript, inserting explicit
/// Audioticon voice tags (`[głos:model]`) where appropriate.
pub fn apply_dialogue_casting(text: &str, casting: &HashMap<String, String>) -> String {
    let dramatic_re = get_dramatic_prefix_re();
    let inquit_pl_re = get_inquit_polish_re();
    let inquit_en_re = get_inquit_english_re();

    let narrator_voice = casting.get("Narrator").cloned();
    let mut result_lines = Vec::new();
    let mut current_voice_tag: Option<String> = None;

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            result_lines.push(String::new());
            continue;
        }

        // 1. Dramatic script prefix "Jan: ..."
        if let Some(caps) = dramatic_re.captures(trimmed) {
            let char_name = caps.get(1).unwrap().as_str().trim();
            let speech = caps.get(2).unwrap().as_str().trim();

            if let Some(voice) = casting.get(char_name) {
                if current_voice_tag.as_deref() != Some(voice.as_str()) {
                    current_voice_tag = Some(voice.clone());
                    result_lines.push(format!("[głos:{}] {}", voice, speech));
                } else {
                    result_lines.push(speech.to_string());
                }
            } else {
                result_lines.push(speech.to_string());
            }
            continue;
        }

        // 2. Inquit Polish "— ... — powiedział Jan"
        if let Some(caps) = inquit_pl_re.captures(trimmed) {
            let char_name = caps.get(1).unwrap().as_str().trim();
            if let Some(voice) = casting.get(char_name) {
                if current_voice_tag.as_deref() != Some(voice.as_str()) {
                    current_voice_tag = Some(voice.clone());
                    result_lines.push(format!("[głos:{}] {}", voice, trimmed));
                } else {
                    result_lines.push(trimmed.to_string());
                }
            } else {
                result_lines.push(trimmed.to_string());
            }
            continue;
        }

        // 3. Inquit English `"..." said John`
        if let Some(caps) = inquit_en_re.captures(trimmed) {
            let char_name = caps.get(1).unwrap().as_str().trim();
            if let Some(voice) = casting.get(char_name) {
                if current_voice_tag.as_deref() != Some(voice.as_str()) {
                    current_voice_tag = Some(voice.clone());
                    result_lines.push(format!("[głos:{}] {}", voice, trimmed));
                } else {
                    result_lines.push(trimmed.to_string());
                }
            } else {
                result_lines.push(trimmed.to_string());
            }
            continue;
        }

        // 4. Narrator / descriptive text
        if let Some(ref n_voice) = narrator_voice {
            if current_voice_tag.as_deref() != Some(n_voice.as_str()) {
                current_voice_tag = Some(n_voice.clone());
                result_lines.push(format!("[głos:{}] {}", n_voice, trimmed));
            } else {
                result_lines.push(trimmed.to_string());
            }
        } else {
            result_lines.push(trimmed.to_string());
        }
    }

    result_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_character_gender() {
        assert_eq!(infer_character_gender("Anna"), "female");
        assert_eq!(infer_character_gender("Katarzyna"), "female");
        assert_eq!(infer_character_gender("Królowa"), "female");
        assert_eq!(infer_character_gender("Mary"), "female");

        assert_eq!(infer_character_gender("Jan"), "male");
        assert_eq!(infer_character_gender("Marek"), "male");
        assert_eq!(infer_character_gender("Kuba"), "male"); // special exception
        assert_eq!(infer_character_gender("Strażnik"), "male");

        assert_eq!(infer_character_gender("Narrator"), "narrator");
        assert_eq!(infer_character_gender("Lektor"), "narrator");
    }

    #[test]
    fn test_analyze_dramatic_dialogue() {
        let script = r#"
        Jan: Dzień dobry pani Anno.
        Anna: Dzień dobry panie Janie, miło pana widzieć.
        Jan: Mamy dużo pracy dzisiaj.
        Narrator: W tym momencie zegar wybił południe.
        "#;

        let res = analyze_dialogue(script, "pl");
        assert_eq!(res.characters.len(), 3);
        assert_eq!(res.total_dialogue_lines, 3);
        assert_eq!(res.total_narrator_lines, 1);

        let jan = res.characters.iter().find(|c| c.name == "Jan").unwrap();
        assert_eq!(jan.line_count, 2);
        assert_eq!(jan.suggested_gender, "male");
        assert_eq!(jan.assigned_voice, "pl_PL-darkman-medium");

        let anna = res.characters.iter().find(|c| c.name == "Anna").unwrap();
        assert_eq!(anna.line_count, 1);
        assert_eq!(anna.suggested_gender, "female");
        assert_eq!(anna.assigned_voice, "pl_PL-gosia-medium");
    }

    #[test]
    fn test_analyze_inquit_polish_novel() {
        let novel = r#"
        — Musimy natychmiast uciekać — powiedziała Anna ze strachem.
        — Zgadzam się, zabierz tylko najważniejsze rzeczy — odparł Marek.
        Wokół domu wiatr potrząsał gałęziami starych dębów.
        "#;

        let res = analyze_dialogue(novel, "pl");
        assert!(res.characters.iter().any(|c| c.name == "Anna"));
        assert!(res.characters.iter().any(|c| c.name == "Marek"));
        assert!(res.characters.iter().any(|c| c.name == "Narrator"));
    }

    #[test]
    fn test_apply_dialogue_casting() {
        let script = "Jan: Witaj.\nAnna: Cześć.\nI odeszli.";
        let mut casting = HashMap::new();
        casting.insert("Jan".to_string(), "pl_PL-darkman-medium".to_string());
        casting.insert("Anna".to_string(), "pl_PL-gosia-medium".to_string());
        casting.insert("Narrator".to_string(), "pl_PL-mc_speech-medium".to_string());

        let formatted = apply_dialogue_casting(script, &casting);
        assert!(formatted.contains("[głos:pl_PL-darkman-medium] Witaj."));
        assert!(formatted.contains("[głos:pl_PL-gosia-medium] Cześć."));
        assert!(formatted.contains("[głos:pl_PL-mc_speech-medium] I odeszli."));
    }
}
